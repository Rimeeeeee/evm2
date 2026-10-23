//! Executing transactions independently against an already-built EIP-7928 Block
//! Access List (BAL).
//!
//! With a BAL attached, reads are served from it at the transaction's block access
//! index (`i + 1` for transaction `i`), so transaction `i` sees the post-state of
//! transactions `0..i` without those transactions having been committed to this
//! EVM. Every execution only needs the pre-block database plus the shared BAL,
//! which is what makes the transactions of a block executable in parallel.
//!
//! Each worker thread detaches its transaction's pending state -- the transaction
//! overlay moved out of the EVM -- and sends it back to the main thread, which folds
//! it into a fresh BAL and checks it against the block's BAL: the validation half of
//! EIP-7928. The pending state still streams the change-set persistence consumers
//! (e.g. reth) apply to the database.

use alloy_consensus::{TxEip8141, TxLegacy, transaction::Recovered};
use alloy_eip7928::BalanceChange;
use alloy_eips::eip8141::{Frame, FrameLimits, FrameMode, NONCE_MANAGER, TransactionFees};
use alloy_primitives::{Address, Bytes, TxKind, U256};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId, TxResultWithState,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{RecoveredTxEnvelope, TxEnvelope, ethereum_tx_registry},
    evm::{
        AccountInfo, Bal, BlockAccessIndex, BlockStateAccumulator, InMemoryDB, StateChangeSource,
    },
    registry::{HandlerError, HandlerResult},
};
use std::sync::Arc;

const CALLER: Address = Address::with_last_byte(0xaa);
const ALICE: Address = Address::with_last_byte(0xbb);
const BOB: Address = Address::with_last_byte(0xcc);

fn main() {
    // Transaction 1 spends money ALICE only receives in transaction 0, so it cannot
    // execute against pre-block state alone.
    let transactions = [transfer(CALLER, ALICE, 1_000_000, 0), transfer(ALICE, BOB, 400_000, 0)];

    // Sequential execution with the builder enabled produces the block's BAL (see the
    // `bal_build` example). A validator would instead decode it from the block body.
    let bal = build_bal(&transactions);
    assert_eq!(
        bal.accounts.get(&ALICE).unwrap().account_info.balance.changes,
        vec![
            BalanceChange::new(idx(1), U256::from(1_000_000)),
            BalanceChange::new(idx(2), U256::from(600_000)),
        ]
    );

    // Without the BAL, transaction 1 on pre-block state fails: ALICE has no funds yet.
    let mut evm = pre_block_evm();
    assert!(evm.transact(&transactions[1]).is_err());

    // With the BAL attached, each transaction executes on its own EVM over the same
    // pre-block database, one worker thread per transaction. Reads positioned at
    // index `i + 1` see all writes recorded at indices `<= i`, i.e. exactly the
    // transaction's pre-state, so no thread needs another thread's committed state.
    //
    // Each worker detaches its transaction into an owned result + pending state and
    // sends both back to main through its join handle. Main is the single consumer:
    // it folds each pending state into a fresh BAL in transaction order (`Bal` writes
    // must be appended with ascending indices) and checks the rebuilt BAL against the
    // block's BAL, which is exactly how a validator confirms the block's BAL is
    // correct.
    let bal = Arc::new(bal);
    let mut bal_builder = Bal::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = transactions
            .iter()
            .enumerate()
            .map(|(i, tx)| {
                let bal = bal.clone();
                s.spawn(move || execute_with_bal(bal, i as u64 + 1, tx))
            })
            .collect();
        for (i, handle) in handles.into_iter().enumerate() {
            let TxResultWithState { result, pending_state: pending, .. } =
                handle.join().expect("worker thread panicked");
            assert!(result.status);

            // The pending state still carries the change-set that persistence
            // consumers (e.g. reth) stream to the database through a
            // `StateChangeSink`; detaching it for the BAL fold loses nothing.
            bal_builder.commit(idx(i as u64 + 1), pending);
            println!("transaction {i} executed in parallel, gas used {}", result.tx_gas_used());
        }
    });
    assert_eq!(bal_builder, *bal, "BAL rebuilt from parallel execution must match the block's BAL");
    println!("rebuilt BAL from parallel outputs matches the block's BAL");
    frame_example();
}

/// Executes one transaction over pre-block state with reads served from `bal` at
/// `index`. A read the BAL does not cover returns a nonfatal database error carrying `BalError`,
/// which during validation means the BAL is invalid (use
/// `set_allow_bal_db_fallback(true)` to instead fall through to the database, e.g.
/// for RPC calls on BAL-positioned state).
///
/// The executed transaction is detached into an owned [`TxResultWithState`] so it
/// can leave the worker thread; nothing is committed to this EVM, which is dropped
/// here.
fn execute_with_bal(bal: Arc<Bal>, index: u64, tx: &RecoveredTxEnvelope) -> TxResultWithState {
    let mut evm = pre_block_evm();
    evm.state_mut().set_bal(bal);
    evm.state_mut().set_bal_index(BlockAccessIndex::new(index));
    evm.transact(tx).expect("transaction should execute").detach()
}

fn build_bal(transactions: &[RecoveredTxEnvelope]) -> Bal {
    let mut evm = pre_block_evm();
    evm.state_mut().enable_bal_builder();
    evm.state_mut().reset_bal_index();
    for tx in transactions {
        evm.state_mut().bump_bal_index();
        let result = evm.transact(tx).expect("transaction should execute").commit();
        assert!(result.status);
    }
    evm.state_mut().take_bal_builder().expect("builder was enabled")
}

fn pre_block_evm() -> Evm<'static, BaseEvmTypes> {
    let mut database = InMemoryDB::default();
    database.insert_account_info(
        &CALLER,
        AccountInfo::default().with_balance(U256::from(1_000_000_000_u64)),
    );

    let spec = SpecId::AMSTERDAM;
    Evm::new(
        spec,
        BlockEnvExt::default(),
        ethereum_tx_registry(spec),
        database,
        Precompiles::base(spec),
    )
}

fn transfer(from: Address, to: Address, value: u64, nonce: u64) -> RecoveredTxEnvelope {
    Recovered::new_unchecked(
        TxEnvelope::Legacy(TxLegacy {
            to: TxKind::Call(to),
            value: U256::from(value),
            gas_limit: 300_000,
            nonce,
            ..Default::default()
        }),
        from,
    )
}

const fn idx(index: u64) -> BlockAccessIndex {
    BlockAccessIndex::new(index)
}

const FRAME_SENDER: Address = Address::with_last_byte(0xd0);
const PAYER: Address = Address::with_last_byte(0xd1);
const STORAGE: Address = Address::with_last_byte(0xd2);
const ASSERTION: Address = Address::with_last_byte(0xd3);
const REVERTER: Address = Address::with_last_byte(0xd4);

fn frame_example() {
    let transactions = frame_transactions();
    let (bal, expected) = serial_frames(&transactions).expect("serial frame block must execute");
    let actual = ordered_frames(parallel_frames(Arc::new(bal.clone()), &transactions))
        .expect("parallel frame block must execute");
    assert_eq!(actual, expected, "parallel frame outcomes and transaction state");
    assert_eq!(fold_frames(&actual), fold_frames(&expected));
    assert_eq!(fold_bal(&actual), bal);
    println!("mixed legacy/frame execution rebuilt the same outcomes, state and BAL");
}

fn frame_evm() -> Evm<'static, BaseEvmTypes> {
    let mut database = InMemoryDB::default();
    database.insert_account_info(
        &CALLER,
        AccountInfo::default().with_balance(U256::from(1_000_000_000_u64)),
    );
    database.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    for (address, code) in [
        (FRAME_SENDER, &[0x60, 2, 0x5f, 0x5f, 0xaa][..]),
        // Empty ordinary calls fund the payer; nonempty frame calls approve payment.
        (PAYER, &[0x36, 0x60, 5, 0x57, 0x00, 0x5b, 0x60, 1, 0x5f, 0x5f, 0xaa][..]),
        (STORAGE, &[0x5f, 0x35, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x00][..]),
        (ASSERTION, &[0x5f, 0x35, 0x60, 8, 0x57, 0x5f, 0x5f, 0xfd, 0x5b, 0x00][..]),
        (REVERTER, &[0x5f, 0x5f, 0xfd][..]),
    ] {
        database.insert_account_info(
            &address,
            AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    Evm::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        database,
        Precompiles::base(SpecId::BOGOTA),
    )
}

fn frame_transactions() -> Vec<RecoveredTxEnvelope> {
    let mut atomic = frame_transaction(2, 0, 9, true);
    let TxEnvelope::Eip8141(payload) = atomic.inner_mut() else { unreachable!() };
    payload.frames[2].flags = 4;
    payload.frames.insert(3, body_frame(FrameMode::Sender, REVERTER, 0, 4));
    payload.frames.insert(4, body_frame(FrameMode::Sender, STORAGE, 99, 0));
    let mut ordinary = transfer(CALLER, STORAGE, 0, 1);
    let TxEnvelope::Legacy(tx) = ordinary.inner_mut() else { unreachable!() };
    tx.input = word(42);
    vec![
        transfer(CALLER, PAYER, 20_000_000, 0),
        frame_transaction(1, 0, 7, true),
        frame_transaction(1, 1, 0, false),
        atomic,
        ordinary,
        frame_transaction(1, 2, 0, true),
        frame_transaction(2, 1, 11, true),
    ]
}

fn frame_transaction(key: u64, nonce: u64, value: u64, assertion: bool) -> RecoveredTxEnvelope {
    Recovered::new_unchecked(
        TxEnvelope::Eip8141(TxEip8141 {
            chain_id: 1,
            sender: FRAME_SENDER,
            nonce,
            nonce_keys: Some(vec![U256::from(key)]),
            frames: vec![
                body_frame(FrameMode::Verify, FRAME_SENDER, 0, 2),
                body_frame(FrameMode::Verify, PAYER, 0, 1),
                body_frame(FrameMode::Sender, STORAGE, value, 0),
                body_frame(FrameMode::Assert, ASSERTION, u64::from(assertion), 0),
            ],
            fees: TransactionFees {
                max_fee: U256::from(10_000_000),
                max_priority_fee_per_gas: U256::ONE,
            },
            ..Default::default()
        }),
        Address::ZERO,
    )
}

fn body_frame(mode: FrameMode, target: Address, value: u64, flags: u8) -> Frame {
    Frame {
        mode,
        target: target.into(),
        data: word(value),
        flags,
        limits: FrameLimits { execution: 100_000, state: 300_000 },
        ..Default::default()
    }
}

fn word(value: u64) -> Bytes {
    U256::from(value).to_be_bytes::<32>().to_vec().into()
}

fn serial_frames(
    transactions: &[RecoveredTxEnvelope],
) -> Result<(Bal, Vec<TxResultWithState>), (usize, HandlerError)> {
    let mut evm = frame_evm();
    let mut outcomes = Vec::with_capacity(transactions.len());
    for (index, tx) in transactions.iter().enumerate() {
        let outcome = evm.transact(tx).map_err(|error| (index, error))?.detach();
        evm.state_mut().commit_source(&outcome.pending_state);
        outcomes.push(outcome);
    }
    Ok((fold_bal(&outcomes), outcomes))
}

fn parallel_frames(
    bal: Arc<Bal>,
    transactions: &[RecoveredTxEnvelope],
) -> Vec<HandlerResult<TxResultWithState>> {
    std::thread::scope(|scope| {
        let workers = transactions
            .iter()
            .enumerate()
            .map(|(index, tx)| {
                let bal = bal.clone();
                scope.spawn(move || {
                    let mut evm = frame_evm();
                    evm.state_mut().set_bal(bal);
                    evm.state_mut().set_bal_index(idx(index as u64 + 1));
                    evm.transact(tx).map(evm2::ExecutedTx::detach)
                })
            })
            .collect::<Vec<_>>();
        workers.into_iter().map(|worker| worker.join().expect("frame worker panicked")).collect()
    })
}

fn ordered_frames(
    outcomes: Vec<HandlerResult<TxResultWithState>>,
) -> Result<Vec<TxResultWithState>, (usize, HandlerError)> {
    // Resolve in block order even if a later worker detects its error first.
    outcomes
        .into_iter()
        .enumerate()
        .map(|(index, result)| result.map_err(|error| (index, error)))
        .collect()
}

fn fold_bal(outcomes: &[TxResultWithState]) -> Bal {
    let mut bal = Bal::new();
    for (index, outcome) in outcomes.iter().enumerate() {
        bal.commit(idx(index as u64 + 1), outcome.pending_state.clone());
    }
    bal
}

fn fold_frames(outcomes: &[TxResultWithState]) -> BlockStateAccumulator {
    let mut state = BlockStateAccumulator::new();
    for outcome in outcomes {
        let Ok(()) = outcome.pending_state.visit(&mut state);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_eips::eip8141::nonce_slot;
    use evm2::evm::BalError;

    #[test]
    fn mixed_frames_match_serial_outcomes_state_gas_and_bal() {
        let transactions = frame_transactions();
        let (bal, expected) = serial_frames(&transactions).unwrap();
        let actual = ordered_frames(parallel_frames(Arc::new(bal.clone()), &transactions)).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(fold_frames(&actual), fold_frames(&expected));
        assert_eq!(fold_bal(&actual), bal);
        assert!(!actual[2].result.status, "failed assertion rolls back only the body");
        assert_eq!(actual[2].result.frame.as_ref().unwrap().assertion_failure, Some(3));
        assert_eq!(
            actual[3].result.frame.as_ref().unwrap().receipts[4].status,
            alloy_eips::eip8141::FrameStatus::SkippedAtomicBatch
        );
        let state = fold_frames(&actual);
        let storage = state.storage_sorted();
        for (key, expected) in [(1, 3), (2, 2)] {
            let slot = nonce_slot(FRAME_SENDER, U256::from(key));
            let (_, value) = storage
                .iter()
                .find(|(key, _)| key.address() == NONCE_MANAGER && key.key() == slot)
                .unwrap();
            assert_eq!(value.current, U256::from(expected));
        }
        let (_, value) = storage
            .iter()
            .find(|(key, _)| key.address() == STORAGE && key.key().is_zero())
            .unwrap();
        assert_eq!(value.current, U256::from(11));
    }

    #[test]
    fn ordered_consumer_preserves_the_first_transaction_error() {
        let mut transactions = frame_transactions();
        let (bal, _) = serial_frames(&transactions).unwrap();
        let TxEnvelope::Eip8141(payload) = transactions[1].inner_mut() else { unreachable!() };
        payload.nonce = 9;
        let TxEnvelope::Eip8141(payload) = transactions[2].inner_mut() else { unreachable!() };
        payload.frames.clear();
        let expected = serial_frames(&transactions).unwrap_err();
        let actual = ordered_frames(parallel_frames(Arc::new(bal), &transactions)).unwrap_err();
        assert_eq!(actual.0, expected.0);
        assert_eq!(actual.1.to_string(), expected.1.to_string());
        assert_eq!(actual.0, 1);
    }

    #[test]
    fn missing_protocol_slot_is_an_input_error_before_later_worker_failures() {
        let mut transactions = frame_transactions();
        let (mut bal, _) = serial_frames(&transactions).unwrap();
        let slot = nonce_slot(FRAME_SENDER, U256::ONE);
        bal.accounts.get_mut(&NONCE_MANAGER).unwrap().storage.storage.remove(&slot);
        let TxEnvelope::Eip8141(payload) = transactions[2].inner_mut() else { unreachable!() };
        payload.frames.clear();
        let error = ordered_frames(parallel_frames(Arc::new(bal), &transactions)).unwrap_err();
        assert_eq!(error.0, 1);
        let HandlerError::Database(error) = error.1 else { panic!("expected BAL input error") };
        assert!(!error.is_fatal());
        assert_eq!(
            error.downcast_ref::<BalError>(),
            Some(&BalError::SlotNotFound { address: NONCE_MANAGER, slot })
        );
    }
}
