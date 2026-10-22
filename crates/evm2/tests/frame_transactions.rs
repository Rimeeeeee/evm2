//! Native Bogota execution, journal, and settlement regressions.

use alloy_consensus::{Transaction, TxEip8141, TxLegacy, transaction::Recovered};
use alloy_eips::{
    eip8141::{
        Frame, FrameLimits, FrameMode, FrameSignature, FrameStatus, NONCE_MANAGER, SignatureScheme,
        nonce_slot,
    },
    eip8272::{
        RECENT_ROOT_ADDRESS, RECENT_ROOT_CODE, RECENT_ROOT_CODE_HASH, RECENT_ROOT_ENTRY_DOMAIN,
        RECENT_ROOT_STORAGE_DOMAIN,
    },
};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use evm2::{
    BaseEvmTypes, Evm, FrameTransactionResult, Precompiles, SpecId, TxResultExt,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, eip8141::FrameGas, ethereum_tx_registry},
    evm::{AccountInfo, Bal, BlockAccessIndex, Database, Db, InMemoryDB},
    interpreter::{GasTracker, InstrStop},
};
use k256::ecdsa::SigningKey;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);
const ASSERTION: Address = Address::with_last_byte(0xc0);

fn frame(mode: FrameMode, target: Address) -> Frame {
    Frame {
        mode,
        flags: if mode == FrameMode::Verify { 3 } else { 0 },
        target: target.into(),
        limits: FrameLimits { execution: 100_000, state: 300_000 },
        ..Default::default()
    }
}

fn payload(frames: Vec<Frame>) -> TxEip8141 {
    TxEip8141 {
        chain_id: 1,
        sender: SENDER,
        nonce_keys: Some(vec![U256::ZERO]),
        frames,
        ..Default::default()
    }
}

const fn recovered(tx: TxEip8141) -> Recovered<TxEnvelope> {
    // An outer recovered sender is deliberately unrelated to the explicit payload sender.
    Recovered::new_unchecked(TxEnvelope::Eip8141(tx), Address::ZERO)
}

fn evm(body: &[u8], assertion: &[u8]) -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    for (address, code) in
        [(SENDER, &[0x60, 3, 0x5f, 0x5f, 0xaa][..]), (BODY, body), (ASSERTION, assertion)]
    {
        db.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    Evm::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        Precompiles::base(SpecId::BOGOTA),
    )
}

#[test]
fn state_gas_cannot_spill_into_execution() {
    let mut gas = GasTracker::new_isolated(1_000_000, 9);
    assert_eq!(gas.spend_state(10), Err(InstrStop::OutOfGas));
    assert_eq!(gas.remaining(), 1_000_000);
    assert_eq!(gas.reservoir(), 9);
    gas.spend_state(9).unwrap();
    assert_eq!(gas.remaining(), 1_000_000);
    assert_eq!(gas.reservoir(), 0);
}

#[test]
fn frame_floor_is_charged_in_addition_to_state() {
    let result = TxResultExt::<()> {
        total_gas_spent: 180_000,
        state_gas_spent: 100_000,
        floor_gas: 100_000,
        frame: Some(FrameTransactionResult::default()),
        ..Default::default()
    };
    assert_eq!(result.tx_gas_used(), 200_000);
    assert_eq!(result.execution_gas_spent(), 100_000);
    let mapped = result.map_ext(|()| 7);
    assert_eq!(mapped.tx_gas_used(), 200_000);
    assert_eq!(mapped.ext, 7);
    assert!(mapped.frame.is_some());
}

#[test]
fn approval_executes_body_and_commits_explicit_sender_nonce() {
    let mut evm = evm(&[0x60, 42, 0x5f, 0x55, 0x00], &[]);
    let tx =
        recovered(payload(vec![frame(FrameMode::Verify, SENDER), frame(FrameMode::Sender, BODY)]));
    let result = evm.transact(&tx).unwrap().commit();
    let outcome = result.frame.unwrap();
    assert!(result.status);
    assert_eq!(outcome.payer, SENDER);
    assert_eq!(outcome.receipts.len(), 2);
    assert!(outcome.receipts.iter().all(|r| r.status == FrameStatus::Success));
    assert!(outcome.receipts[1].gas_used.state > 0);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::from(42)
    );
}

#[test]
fn assertion_failure_reverts_body_and_preserves_payment_prefix() {
    let mut evm = evm(&[0x60, 42, 0x5f, 0x55, 0x00], &[0x5f, 0x5f, 0xfd]);
    let tx = recovered(payload(vec![
        frame(FrameMode::Verify, SENDER),
        frame(FrameMode::Sender, BODY),
        frame(FrameMode::PostTx, ASSERTION),
        frame(FrameMode::PostTx, ASSERTION),
    ]));
    let result = evm.transact(&tx).unwrap().commit();
    assert!(!result.status);
    let outcome = result.frame.unwrap();
    assert_eq!(
        outcome.receipts.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![
            FrameStatus::Success,
            FrameStatus::Success,
            FrameStatus::Failure,
            FrameStatus::SkippedAtomicBatch
        ]
    );
    assert_eq!(outcome.receipts[1].gas_used.state, 0);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::ZERO
    );
}

#[test]
fn executable_prefix_discards_nonce_and_does_not_execute_body() {
    let mut evm = evm(&[0xfe], &[]);
    let tx =
        recovered(payload(vec![frame(FrameMode::Verify, SENDER), frame(FrameMode::Sender, BODY)]));
    let prefix = evm.validate_frame_prefix(&tx, 1).unwrap();
    assert_eq!(prefix.payer, SENDER);
    assert!(prefix.sender_approved);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
    let result = evm.transact(&tx).unwrap().discard();
    assert_eq!(result.frame.unwrap().receipts[1].status, FrameStatus::Failure);
}

#[test]
fn keyed_nonce_updates_protocol_storage_without_bumping_account_nonce() {
    let mut evm = evm(&[], &[]);
    let mut tx = payload(vec![frame(FrameMode::Verify, SENDER)]);
    tx.nonce_keys = Some(vec![U256::from(7), U256::from(9)]);
    let _ = evm.transact(&recovered(tx)).unwrap().commit();
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
    for key in [7, 9] {
        assert_eq!(
            evm.state_mut()
                .storage(&NONCE_MANAGER)
                .unwrap()
                .into_slot(nonce_slot(SENDER, U256::from(key)))
                .unwrap()
                .current(),
            U256::from(1)
        );
    }
}

#[test]
fn empty_account_verification_uses_canonical_transaction_signature() {
    let key = SigningKey::from_slice(&[0x11; 32]).unwrap();
    let sender =
        Address::from_raw_public_key(&key.verifying_key().to_sec1_point(false).as_bytes()[1..]);
    let mut tx = payload(vec![frame(FrameMode::Verify, sender)]);
    tx.sender = sender;
    tx.signatures.push(FrameSignature { scheme: SignatureScheme::Secp256k1, ..Default::default() });
    let (sig, recid) = key.sign_prehash_recoverable(tx.signature_hash().as_slice());
    let mut bytes = vec![recid.to_byte()];
    bytes.extend_from_slice(sig.to_bytes().as_slice());
    tx.signatures[0].signature = bytes.into();
    let mut evm = evm(&[], &[]);
    evm.overlay_db_mut().insert_account_info(
        &sender,
        AccountInfo::default().with_balance(U256::from(1_000_000_000)),
    );
    let result = evm.transact(&recovered(tx)).unwrap().commit();
    assert_eq!(result.frame.unwrap().payer, sender);
    assert_eq!(evm.state_mut().account(&sender).unwrap().nonce(), 1);
}

#[test]
fn intrinsic_reservation_matches_locked_shared_schedule() {
    let mut tx = payload(vec![frame(FrameMode::Verify, SENDER)]);
    tx.frames[0].data = vec![0, 1, 2, 0].into();
    let mut version = evm2::Version::new(SpecId::BOGOTA);
    version.gas_params.set(evm2::version::GasId::TxTokenCost, 1);
    version.gas_params.set(evm2::version::GasId::TxFloorCostPerToken, 16);
    version.gas_params.set(evm2::version::GasId::TxFloorZeroByteMultiplier, 4);
    let gas = FrameGas::calculate(&tx, &version).unwrap();
    assert_eq!(
        gas.intrinsic,
        tx.as_frame_ref().calculate_execution_gas_limit_with_token_cost(1)
            - tx.total_frame_execution_gas_limit()
    );
    assert_eq!(gas.floor, tx.as_frame_ref().calculate_calldata_floor());
    assert_eq!(gas.maximum, gas.execution + gas.state);
}

#[test]
fn restoration_does_not_fund_a_later_frame() {
    let mut evm = evm(&[0x5f, 0x35, 0x5f, 0x55, 0x00], &[]);
    let mut write = frame(FrameMode::Sender, BODY);
    write.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
    let mut restore = frame(FrameMode::Sender, BODY);
    restore.limits.state = 0;
    let mut unfunded = write.clone();
    unfunded.limits.state = 0;
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER), write, restore, unfunded]));
    let result = evm.transact(&tx).unwrap().commit();
    let frames = result.frame.unwrap();
    assert_eq!(frames.receipts[1].gas_used.state, 0);
    assert_eq!(frames.receipts[2].status, FrameStatus::Success);
    assert_eq!(frames.receipts[3].status, FrameStatus::Failure);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::ZERO
    );
}

#[test]
fn reverted_restoration_preserves_the_original_owners_state_charge() {
    let code =
        [0x5f, 0x35, 0x5f, 0x55, 0x60, 0x20, 0x35, 0x60, 0x0b, 0x57, 0x00, 0x5b, 0x5f, 0x5f, 0xfd];
    let mut evm = evm(&code, &[]);
    let mut write = frame(FrameMode::Sender, BODY);
    write.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
    let mut restore = frame(FrameMode::Sender, BODY);
    let mut data = vec![0; 64];
    data[63] = 1;
    restore.data = data.into();
    restore.limits.state = 0;
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER), write, restore]));
    let result = evm.transact(&tx).unwrap().commit();
    let frames = result.frame.unwrap();
    assert!(frames.receipts[1].gas_used.state > 0);
    assert_eq!(frames.receipts[2].status, FrameStatus::Failure);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::from(1)
    );
}

#[test]
fn atomic_failure_preserves_status_and_skips_the_linked_suffix() {
    let mut evm = evm(&[0x60, 1, 0x5f, 0x55, 0x00], &[0x5f, 0x5f, 0xfd]);
    let mut write = frame(FrameMode::Sender, BODY);
    write.flags = 4;
    let mut fail = frame(FrameMode::Sender, ASSERTION);
    fail.flags = 4;
    let tx = recovered(payload(vec![
        frame(FrameMode::Verify, SENDER),
        write,
        fail,
        frame(FrameMode::Sender, BODY),
    ]));
    let result = evm.transact(&tx).unwrap().commit();
    let frames = result.frame.unwrap();
    assert_eq!(
        frames.receipts.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![
            FrameStatus::Success,
            FrameStatus::Success,
            FrameStatus::Failure,
            FrameStatus::SkippedAtomicBatch
        ]
    );
    assert_eq!(frames.receipts[1].gas_used.state, 0);
    assert!(frames.rolled_back[1]);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::ZERO
    );
}

#[test]
fn insufficient_value_pays_only_entry_access() {
    let mut evm = evm(&[], &[]);
    let mut transfer = frame(FrameMode::Sender, BODY);
    transfer.value = U256::MAX;
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER), transfer]));
    let result = evm.transact(&tx).unwrap().discard();
    let frames = result.frame.unwrap();
    assert_eq!(frames.stops[1], InstrStop::OutOfFunds);
    assert_eq!(frames.receipts[1].gas_used.execution, 3_000);
    assert_eq!(frames.receipts[1].gas_used.state, 0);
}

#[test]
fn prefix_stops_before_the_requested_body_even_without_a_payer() {
    let mut evm = evm(&[0xfe], &[]);
    let mut approve_execution = frame(FrameMode::Verify, SENDER);
    approve_execution.flags = 2;
    evm.overlay_db_mut().insert_account_info(
        &SENDER,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x60, 2, 0x5f, 0x5f, 0xaa]))),
    );
    let tx = recovered(payload(vec![approve_execution, frame(FrameMode::Sender, BODY)]));
    assert!(evm.validate_frame_prefix(&tx, 1).is_err());
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
}

#[test]
fn generic_envelope_fee_accessors_preserve_u256() {
    let mut tx = payload(vec![frame(FrameMode::Verify, SENDER)]);
    let large = U256::from(1) << 180;
    tx.fees.max_fee_per_gas = large;
    tx.fees.max_priority_fee_per_gas = large - U256::from(1);
    tx.fees.max_fee_per_blob_gas = large;
    let tx = TxEnvelope::Eip8141(tx);
    assert_eq!(tx.max_fee_per_gas_u256(), large);
    assert_eq!(tx.max_priority_fee_per_gas_u256(), Some(large - U256::from(1)));
    assert_eq!(tx.max_fee_per_blob_gas_u256(), Some(large));
    assert_eq!(tx.effective_gas_price_u256(Some(1)), large);
}

#[test]
fn recent_root_runtime_uses_actual_slot_and_the_locked_window() {
    assert_eq!(alloy_primitives::keccak256(&RECENT_ROOT_CODE), RECENT_ROOT_CODE_HASH);
    for (age, valid) in [(0, false), (1, true), (8191, true), (8192, false)] {
        let mut evm = evm(&[], &[]);
        let current_slot = 10_000u64;
        let slot = current_slot - age;
        let source = [0x11; 32];
        let root = [0x22; 32];
        let mut tuple = source.to_vec();
        tuple.extend_from_slice(&slot.to_be_bytes());
        tuple.extend_from_slice(&root);
        let mut entry = RECENT_ROOT_ENTRY_DOMAIN.as_slice().to_vec();
        entry.extend_from_slice(&tuple);
        let value = U256::from_be_bytes(alloy_primitives::keccak256(entry).0);
        let mut key = RECENT_ROOT_STORAGE_DOMAIN.as_slice().to_vec();
        key.extend_from_slice(&source);
        key.extend_from_slice(&(slot % 8192).to_be_bytes());
        let key = U256::from_be_bytes(alloy_primitives::keccak256(key).0);
        evm.overlay_db_mut().insert_account_info(
            &RECENT_ROOT_ADDRESS,
            AccountInfo::default().with_code(Bytecode::new_legacy(RECENT_ROOT_CODE.clone())),
        );
        evm.overlay_db_mut().insert_account_storage(&RECENT_ROOT_ADDRESS, &key, &value);
        evm.set_block(BlockEnvExt {
            slot_num: U256::from(current_slot),
            timestamp: U256::from(123),
            ..*evm.block()
        });
        let mut verifier = frame(FrameMode::Verify, RECENT_ROOT_ADDRESS);
        verifier.flags = 0;
        verifier.data = tuple.into();
        let tx = recovered(payload(vec![verifier, frame(FrameMode::Verify, SENDER)]));
        assert_eq!(
            evm.transact(&tx).map(|executed| executed.discard()).is_ok(),
            valid,
            "age {age}"
        );
    }
}

#[test]
fn bal_positioned_frames_preserve_prefix_writes_after_assertion_failure() {
    let body = [0x5f, 0x35, 0x5f, 0x55, 0x00];
    let assertion = [0x5f, 0x5f, 0xfd];
    let mut transactions = Vec::new();
    for nonce in 0..2 {
        let mut write = frame(FrameMode::Sender, BODY);
        write.data = U256::from(42 - nonce).to_be_bytes::<32>().to_vec().into();
        let mut tx = payload(vec![frame(FrameMode::Verify, SENDER), write]);
        tx.nonce = nonce;
        tx.nonce_keys = Some(vec![U256::from(7)]);
        if nonce == 1 {
            tx.frames.push(frame(FrameMode::PostTx, ASSERTION));
        }
        transactions.push(recovered(tx));
    }
    let mut serial = evm(&body, &assertion);
    serial.state_mut().enable_bal_builder();
    serial.state_mut().reset_bal_index();
    let mut expected = Vec::new();
    for tx in &transactions {
        serial.state_mut().bump_bal_index();
        expected.push(serial.transact(tx).unwrap().commit());
    }
    assert!(!expected[1].status);
    let bal = Arc::new(serial.state_mut().take_bal_builder().unwrap());
    let mut rebuilt = Bal::new();
    for (index, tx) in transactions.iter().enumerate() {
        let index = BlockAccessIndex::new(index as u64 + 1);
        let mut positioned = evm(&body, &assertion);
        positioned.state_mut().set_bal(bal.clone());
        positioned.state_mut().set_bal_index(index);
        let detached = positioned.transact(tx).unwrap().detach();
        assert_eq!(detached.result, expected[index.get() as usize - 1]);
        rebuilt.commit(index, detached.pending_state);
    }
    assert_eq!(rebuilt, *bal);
    assert_eq!(
        serial
            .state_mut()
            .storage(&NONCE_MANAGER)
            .unwrap()
            .into_slot(nonce_slot(SENDER, U256::from(7)))
            .unwrap()
            .current(),
        U256::from(2)
    );
    assert_eq!(
        serial.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::from(42)
    );
}

#[test]
fn atomic_group_cannot_end_on_an_assertion() {
    let mut evm = evm(&[], &[]);
    let mut body = frame(FrameMode::Sender, BODY);
    body.flags = 4;
    let tx = recovered(payload(vec![
        frame(FrameMode::Verify, SENDER),
        body,
        frame(FrameMode::PostTx, ASSERTION),
    ]));
    assert!(evm.transact(&tx).is_err());
    let valid = recovered(payload(vec![frame(FrameMode::Verify, SENDER)]));
    assert!(evm.transact(&valid).unwrap().discard().status);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
}

#[test]
fn preparation_uses_the_active_execution_cap() {
    let mut evm = evm(&[], &[]);
    let mut version = *evm.version();
    version.tx_gas_limit_cap = 40_000_000;
    evm.set_execution_config(
        evm2::ExecutionConfig::for_spec_and_version(SpecId::BOGOTA, version),
        SpecId::BOGOTA,
        ethereum_tx_registry(SpecId::BOGOTA),
        Precompiles::base(SpecId::BOGOTA),
    );
    evm.set_block(BlockEnvExt { gas_limit: U256::from(50_000_000), ..*evm.block() });
    let mut verify = frame(FrameMode::Verify, SENDER);
    verify.limits.execution = 20_000_000;
    let tx = recovered(payload(vec![verify]));
    assert!(evm.transact(&tx).unwrap().discard().status);
}

#[derive(Debug)]
struct CountingRunner(Arc<AtomicUsize>);

impl evm2::InterpreterRunner<BaseEvmTypes> for CountingRunner {
    fn run<'frame, 'host>(
        &self,
        _: &mut evm2::interpreter::Interpreter<'frame, 'host, BaseEvmTypes>,
        _: &mut Evm<'host, BaseEvmTypes>,
    ) -> Option<InstrStop> {
        self.0.fetch_add(1, Ordering::Relaxed);
        None
    }
}

#[test]
fn prefix_forces_interpreted_observation_and_restores_the_full_execution_runner() {
    let mut evm = evm(&[], &[]);
    let calls = Arc::new(AtomicUsize::new(0));
    evm.set_interpreter_runner(CountingRunner(calls.clone()));
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER)]));
    evm.validate_frame_prefix(&tx, 1).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(evm.transact(&tx).unwrap().discard().status);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

struct PanicAtFrameEnd;

impl evm2::Inspector<BaseEvmTypes> for PanicAtFrameEnd {
    fn frame_end(
        &mut self,
        _: usize,
        _: &alloy_eips::eip8141::FrameReceipt,
        _: &Bytes,
        _: InstrStop,
        _: &mut Evm<'_, BaseEvmTypes>,
    ) {
        panic!("prefix inspector failed after approval");
    }
}

#[test]
fn prefix_unwind_discards_approved_nonce_and_restores_runner() {
    let mut evm = evm(&[], &[]);
    let calls = Arc::new(AtomicUsize::new(0));
    evm.set_interpreter_runner(CountingRunner(calls.clone()));
    evm.set_inspector(PanicAtFrameEnd);
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER)]));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evm.validate_frame_prefix(&tx, 1).unwrap();
    }));
    assert!(panic.is_err());
    evm.clear_inspector();
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
    assert!(evm.transact(&tx).unwrap().discard().status);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

struct FailingDatabase(Arc<AtomicUsize>);

impl FailingDatabase {
    fn fail<T>(&self) -> Result<T, std::io::Error> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(std::io::Error::other("frame database failure"))
    }
}

impl Database for FailingDatabase {
    type Error = std::io::Error;

    fn get_account(&mut self, _: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.fail()
    }

    fn get_code_by_hash(&mut self, _: &B256) -> Result<Bytecode, Self::Error> {
        self.fail()
    }

    fn get_storage(&mut self, _: &Address, _: &U256) -> Result<U256, Self::Error> {
        self.fail()
    }

    fn get_block_hash(&mut self, _: &U256) -> Result<B256, Self::Error> {
        self.fail()
    }
}

#[test]
fn unused_malformed_signature_precedes_database_reads_and_fatal_errors_survive() {
    let mut evm = evm(&[], &[]);
    let reads = Arc::new(AtomicUsize::new(0));
    evm.set_database(Db::new(FailingDatabase(reads.clone())));
    let mut malformed = payload(vec![frame(FrameMode::Verify, SENDER)]);
    malformed.signatures.push(FrameSignature {
        scheme: SignatureScheme::Secp256k1,
        signature: Bytes::from_static(&[0]),
        ..Default::default()
    });
    assert!(matches!(
        evm.validate_frame_prefix(&recovered(malformed), 1),
        Err(evm2::registry::HandlerError::InvalidFrameTransaction(_))
    ));
    assert_eq!(reads.load(Ordering::Relaxed), 0);

    evm.overlay_db_mut().insert_account_info(
        &SENDER,
        AccountInfo::default()
            .with_balance(U256::from(1_000_000_000))
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x60, 3, 0x5f, 0x5f, 0xaa]))),
    );
    let tx =
        recovered(payload(vec![frame(FrameMode::Verify, SENDER), frame(FrameMode::Sender, BODY)]));
    assert!(
        matches!(evm.transact(&tx), Err(evm2::registry::HandlerError::Database(error)) if error.is_fatal())
    );
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
    evm.overlay_db_mut().insert_account_info(&Address::ZERO, AccountInfo::default());
    let valid = recovered(payload(vec![frame(FrameMode::Verify, SENDER)]));
    assert!(evm.transact(&valid).unwrap().discard().status);
}

#[test]
fn every_transaction_resolution_clears_frame_context_before_an_ordinary_transaction() {
    for resolution in ["commit", "discard", "detach", "drop", "invalid"] {
        let mut evm = evm(&[0x5f, 0xb0, 0x50, 0x00], &[]);
        let tx = recovered(payload(vec![
            frame(FrameMode::Verify, SENDER),
            frame(FrameMode::Sender, BODY),
        ]));
        match resolution {
            "commit" => {
                assert!(evm.transact(&tx).unwrap().commit().status);
            }
            "discard" => {
                assert!(evm.transact(&tx).unwrap().discard().status);
            }
            "detach" => {
                assert!(evm.transact(&tx).unwrap().detach().result.status);
            }
            "drop" => {
                drop(evm.transact(&tx).unwrap());
            }
            "invalid" => {
                assert!(evm.transact(&recovered(payload(Vec::new()))).is_err());
            }
            _ => unreachable!(),
        }
        let caller = Address::with_last_byte(0xd0);
        evm.overlay_db_mut().insert_account_info(
            &caller,
            AccountInfo::default().with_balance(U256::from(1_000_000_000)),
        );
        let normal = Recovered::new_unchecked(
            TxEnvelope::Legacy(TxLegacy {
                gas_limit: 100_000,
                to: TxKind::Call(BODY),
                ..Default::default()
            }),
            caller,
        );
        let result = evm.transact(&normal).unwrap().discard();
        assert!(!result.status, "{resolution}");
        assert_eq!(result.stop, InstrStop::OpcodeNotFound, "{resolution}");
        assert!(result.frame.is_none(), "{resolution}");
    }
}

#[derive(Default)]
struct EntryFailureInspector {
    starts: Vec<usize>,
    ends: Vec<(usize, FrameStatus, InstrStop)>,
}

impl evm2::Inspector<BaseEvmTypes> for EntryFailureInspector {
    fn frame_start(&mut self, index: usize, _: &Frame, _: &mut Evm<'_, BaseEvmTypes>) {
        self.starts.push(index);
    }

    fn frame_end(
        &mut self,
        index: usize,
        receipt: &alloy_eips::eip8141::FrameReceipt,
        _: &Bytes,
        stop: InstrStop,
        _: &mut Evm<'_, BaseEvmTypes>,
    ) {
        self.ends.push((index, receipt.status, stop));
    }
}

#[test]
fn verify_entry_failure_emits_its_frame_end_before_invalidating_the_prefix() {
    let mut evm = evm(&[], &[]);
    evm.set_inspector(EntryFailureInspector::default());
    let mut verify = frame(FrameMode::Verify, SENDER);
    verify.limits.execution = 0;
    assert!(evm.validate_frame_prefix(&recovered(payload(vec![verify])), 1).is_err());
    let inspector = evm.clear_inspector_as::<EntryFailureInspector>().unwrap();
    assert_eq!(inspector.starts, vec![0]);
    assert_eq!(inspector.ends, vec![(0, FrameStatus::Failure, InstrStop::OutOfGas)]);
}
