//! EIP-8250 empty-list legacy selection, zero keys, and single-use nonce exhaustion.

mod common;

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{
    Frame, FrameLimits, FrameMode, FrameStatus, NONCE_MANAGER, nonce_keys_hash, nonce_slot,
};
use alloy_primitives::{Address, Bytes, U256};
use common::{APPROVE_BOTH, BODY, SENDER, stored};
use evm2::{
    BaseEvmTypes, Evm, Inspector,
    bytecode::Bytecode,
    ethereum::TxEnvelope,
    evm::{AccountInfo, Bal, BlockAccessIndex},
    interpreter::InstrStop,
    registry::HandlerError,
    version::GasId,
};
use std::sync::Arc;

const FAILURE: Address = Address::with_last_byte(0xc0);
const WRITE_BODY: &[u8] = &[0x60, 42, 0x5f, 0x55, 0x00];

struct Observe;

impl Inspector<BaseEvmTypes> for Observe {}

fn frame(mode: FrameMode, target: Address, flags: u8) -> Frame {
    Frame {
        flags,
        limits: FrameLimits { execution: 150_000, state: 300_000 },
        ..common::frame(mode, target)
    }
}

fn payload(keys: Vec<U256>, sequence: u64, frames: Vec<Frame>) -> Recovered<TxEnvelope> {
    common::recovered(TxEip8141 {
        nonce: sequence,
        nonce_keys: Some(keys),
        ..common::payload(frames)
    })
}

fn approval(keys: Vec<U256>, sequence: u64) -> Recovered<TxEnvelope> {
    payload(keys, sequence, vec![frame(FrameMode::Verify, SENDER, 3)])
}

fn evm(
    sender: &[u8],
    body: &[u8],
    legacy_nonce: u64,
    inspected: bool,
) -> Evm<'static, BaseEvmTypes> {
    let mut db = common::database([(BODY, body), (FAILURE, &[0x5f, 0x5f, 0xfd])]);
    db.insert_account_info(&SENDER, common::account(sender).with_nonce(legacy_nonce));
    db.insert_account_info(
        &NONCE_MANAGER,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x60, 0, 0x60, 0, 0xfd]))),
    );
    let mut evm = common::evm(db);
    if inspected {
        evm.set_inspector(Observe);
    }
    evm
}

fn keyed_nonce(evm: &mut Evm<'_, BaseEvmTypes>, key: U256) -> U256 {
    stored(evm, NONCE_MANAGER, nonce_slot(SENDER, key))
}

fn legacy_nonce(evm: &mut Evm<'_, BaseEvmTypes>) -> u64 {
    evm.state_mut().account(&SENDER).unwrap().nonce()
}

const fn first_use_cost(evm: &Evm<'_, BaseEvmTypes>) -> u64 {
    evm.version().gas_params.get(GasId::SstoreSetState)
}

fn assert_nonce_mismatch(evm: &mut Evm<'_, BaseEvmTypes>, tx: &Recovered<TxEnvelope>) {
    assert!(matches!(
        evm.validate_tx(tx),
        Err(HandlerError::InvalidFrameTransaction("nonce sequence mismatch"))
    ));
    assert!(matches!(
        evm.validate_frame_prefix(tx, 1),
        Err(HandlerError::InvalidFrameTransaction("nonce sequence mismatch"))
    ));
    assert!(matches!(
        evm.transact(tx),
        Err(HandlerError::InvalidFrameTransaction("nonce sequence mismatch"))
    ));
}

#[test]
fn empty_keys_select_account_nonce_while_zero_is_an_ordinary_storage_key() {
    for inspected in [false, true] {
        let mut legacy = evm(APPROVE_BOTH, &[], 5, inspected);
        legacy.overlay_db_mut().insert_account_storage(
            &NONCE_MANAGER,
            &nonce_slot(SENDER, U256::ZERO),
            &U256::from(7),
        );
        let result = legacy.transact(&approval(vec![], 5)).unwrap().commit();
        assert!(result.status);
        assert_eq!(result.state_gas_spent, 0);
        assert_eq!(legacy_nonce(&mut legacy), 6);
        assert_eq!(keyed_nonce(&mut legacy, U256::ZERO), U256::from(7));

        for keys in [vec![U256::ZERO], vec![U256::ZERO, U256::from(1)]] {
            let mut keyed = evm(APPROVE_BOTH, &[], 5, inspected);
            let cost = first_use_cost(&keyed) * keys.len() as u64;
            let result = keyed.transact(&approval(keys.clone(), 0)).unwrap().commit();
            assert!(result.status);
            assert_eq!(result.state_gas_spent, cost);
            assert_eq!(result.frame.unwrap().receipts[0].gas_used.state, cost);
            assert_eq!(legacy_nonce(&mut keyed), 5);
            for key in keys {
                assert_eq!(keyed_nonce(&mut keyed, key), U256::from(1));
            }
        }
    }
}

#[test]
fn single_use_consumes_every_fresh_key_to_max_and_rejects_all_replays() {
    for inspected in [false, true] {
        let mut evm = evm(APPROVE_BOTH, &[], 5, inspected);
        let keys = vec![U256::ZERO, U256::MAX];
        let cost = 2 * first_use_cost(&evm);
        let result = evm.transact(&approval(keys.clone(), u64::MAX)).unwrap().commit();
        assert!(result.status);
        assert_eq!(result.state_gas_spent, cost);
        assert_eq!(legacy_nonce(&mut evm), 5);
        for &key in &keys {
            assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
            for sequence in [0, u64::MAX - 1, u64::MAX] {
                assert_nonce_mismatch(&mut evm, &approval(vec![key], sequence));
                assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
            }
        }
        assert_nonce_mismatch(&mut evm, &approval(keys, u64::MAX));
    }
}

#[test]
fn sequential_exhaustion_and_single_use_checks_preserve_full_storage_width() {
    for inspected in [false, true] {
        let key = U256::from(7);
        let mut evm = evm(APPROVE_BOTH, &[], 5, inspected);
        evm.overlay_db_mut().insert_account_storage(
            &NONCE_MANAGER,
            &nonce_slot(SENDER, key),
            &U256::from(u64::MAX - 1),
        );
        let result = evm.transact(&approval(vec![key], u64::MAX - 1)).unwrap().commit();
        assert!(result.status);
        assert_eq!(result.state_gas_spent, 0);
        assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
        assert_nonce_mismatch(&mut evm, &approval(vec![key], u64::MAX));
        assert_eq!(legacy_nonce(&mut evm), 5);

        for (stored, sequence) in [
            (U256::from(1) << 64, u64::MAX),
            (U256::MAX, u64::MAX),
            ((U256::from(1) << 64) + U256::from(1), 1),
        ] {
            let mut evm = self::evm(APPROVE_BOTH, &[], 5, inspected);
            evm.overlay_db_mut().insert_account_storage(
                &NONCE_MANAGER,
                &nonce_slot(SENDER, key),
                &stored,
            );
            assert_nonce_mismatch(&mut evm, &approval(vec![key], sequence));
            assert_eq!(keyed_nonce(&mut evm, key), stored);
        }
    }
}

#[test]
fn one_used_key_rejects_single_use_without_consuming_any_fresh_key() {
    for inspected in [false, true] {
        for used in [U256::from(1), U256::from(u64::MAX)] {
            let mut evm = evm(APPROVE_BOTH, &[], 5, inspected);
            let fresh = U256::ZERO;
            let exhausted = U256::from(9);
            evm.overlay_db_mut().insert_account_storage(
                &NONCE_MANAGER,
                &nonce_slot(SENDER, exhausted),
                &used,
            );
            assert_nonce_mismatch(&mut evm, &approval(vec![fresh, exhausted], u64::MAX));
            assert_eq!(keyed_nonce(&mut evm, fresh), U256::ZERO);
            assert_eq!(keyed_nonce(&mut evm, exhausted), used);
            assert_eq!(legacy_nonce(&mut evm), 5);
        }
    }
}

#[test]
fn legacy_max_is_invalid_while_keyed_approval_ignores_an_exhausted_account_nonce() {
    for inspected in [false, true] {
        let mut evm = evm(APPROVE_BOTH, &[], u64::MAX, inspected);
        let legacy = approval(vec![], u64::MAX);
        assert!(matches!(evm.validate_tx(&legacy), Err(HandlerError::NonceOverflow)));
        assert!(matches!(evm.transact(&legacy), Err(HandlerError::NonceOverflow)));
        for (key, sequence) in [(U256::ZERO, 0), (U256::from(1), u64::MAX)] {
            let result = evm.transact(&approval(vec![key], sequence)).unwrap().commit();
            assert!(result.status);
            assert_eq!(keyed_nonce(&mut evm, key), U256::from(sequence.saturating_add(1)));
            assert_eq!(legacy_nonce(&mut evm), u64::MAX);
        }
    }
}

#[test]
fn single_use_first_use_budget_is_checked_before_writing_any_key() {
    for inspected in [false, true] {
        let mut evm = evm(APPROVE_BOTH, &[], 5, inspected);
        let keys = vec![U256::ZERO, U256::from(7)];
        let cost = 2 * first_use_cost(&evm);
        let mut insufficient = frame(FrameMode::Verify, SENDER, 3);
        insufficient.limits.state = cost - 1;
        let tx = payload(keys.clone(), u64::MAX, vec![insufficient]);
        assert!(matches!(
            evm.transact(&tx),
            Err(HandlerError::InvalidFrameTransaction("VERIFY frame failed"))
        ));
        for &key in &keys {
            assert_eq!(keyed_nonce(&mut evm, key), U256::ZERO);
        }
        let result = evm.transact(&approval(keys.clone(), u64::MAX)).unwrap().commit();
        assert!(result.status);
        assert_eq!(result.state_gas_spent, cost);
        for key in keys {
            assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
        }
        assert_eq!(legacy_nonce(&mut evm), 5);
    }
}

// Nonempty input delegates APPROVE to BODY, then fails. Empty input approves directly.
fn reverting_nested_approval(exit: &[u8]) -> Vec<u8> {
    common::delegating_sender(BODY, exit)
}

#[test]
fn nested_parent_failure_restores_single_use_keys_and_allows_one_later_approval() {
    for inspected in [false, true] {
        for exit in [&[0x5f, 0x5f, 0xfd][..], &[0xfe][..]] {
            let mut evm = evm(&reverting_nested_approval(exit), APPROVE_BOTH, 5, inspected);
            let keys = vec![U256::ZERO, U256::from(7)];
            let cost = 2 * first_use_cost(&evm);
            let mut nested = frame(FrameMode::Default, SENDER, 3);
            nested.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
            let tx =
                payload(keys.clone(), u64::MAX, vec![nested, frame(FrameMode::Verify, SENDER, 3)]);
            let result = evm.transact(&tx).unwrap().commit();
            assert!(result.status);
            assert_eq!(result.state_gas_spent, cost);
            let outcome = result.frame.unwrap();
            assert_eq!(outcome.receipts[0].status, FrameStatus::Failure);
            assert_eq!(outcome.receipts[0].gas_used.state, 0);
            assert_eq!(outcome.receipts[1].status, FrameStatus::Success);
            assert_eq!(outcome.receipts[1].gas_used.state, cost);
            for key in keys {
                assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
            }
            assert_eq!(legacy_nonce(&mut evm), 5);
        }
    }
}

#[test]
fn atomic_or_assertion_body_rollback_preserves_single_use_payment_consumption() {
    for inspected in [false, true] {
        for assertion in [false, true] {
            let mut evm = evm(APPROVE_BOTH, WRITE_BODY, 5, inspected);
            let keys = vec![U256::ZERO, U256::from(7)];
            let cost = 2 * first_use_cost(&evm);
            let tx = payload(
                keys.clone(),
                u64::MAX,
                vec![
                    frame(FrameMode::Verify, SENDER, 3),
                    frame(FrameMode::Sender, BODY, if assertion { 0 } else { 4 }),
                    frame(
                        if assertion { FrameMode::Assert } else { FrameMode::Sender },
                        FAILURE,
                        0,
                    ),
                ],
            );
            let result = evm.transact(&tx).unwrap().commit();
            assert_eq!(result.status, !assertion);
            assert_eq!(result.state_gas_spent, cost);
            let outcome = result.frame.unwrap();
            assert_eq!(outcome.receipts[0].gas_used.state, cost);
            assert_eq!(outcome.receipts[1].gas_used.state, 0);
            assert!(outcome.rolled_back[1]);
            assert_eq!(outcome.stops[2], InstrStop::Revert);
            assert_eq!(outcome.assertion_failure, assertion.then_some(2));
            assert_eq!(stored(&mut evm, BODY, U256::ZERO), U256::ZERO);
            for &key in &keys {
                assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
            }
            assert_eq!(legacy_nonce(&mut evm), 5);
            assert_nonce_mismatch(&mut evm, &approval(keys, u64::MAX));
        }
    }
}

#[test]
fn repeated_prefix_and_discard_do_not_consume_single_use_keys_before_commit() {
    for inspected in [false, true] {
        let mut evm = evm(APPROVE_BOTH, &[], 5, inspected);
        let keys = vec![U256::ZERO, U256::MAX];
        let tx = approval(keys.clone(), u64::MAX);
        let cost = 2 * first_use_cost(&evm);
        for _ in 0..3 {
            let prefix = evm.validate_frame_prefix(&tx, 1).unwrap();
            assert!(prefix.sender_approved);
            assert_eq!(prefix.state_gas, cost);
            let result = evm.transact(&tx).unwrap().discard();
            assert!(result.status);
            assert_eq!(result.state_gas_spent, cost);
            for &key in &keys {
                assert_eq!(keyed_nonce(&mut evm, key), U256::ZERO);
            }
            assert_eq!(legacy_nonce(&mut evm), 5);
        }
        assert!(evm.transact(&tx).unwrap().commit().status);
        for key in keys {
            assert_eq!(keyed_nonce(&mut evm, key), U256::from(u64::MAX));
        }
    }
}

#[test]
fn zero_and_single_use_keys_reconstruct_the_same_bal_in_positioned_execution() {
    for inspected in [false, true] {
        let transactions = [
            approval(vec![], 5),
            approval(vec![U256::ZERO], u64::MAX),
            approval(vec![U256::from(1)], 0),
            approval(vec![U256::from(2)], u64::MAX),
            approval(vec![U256::from(1)], 1),
        ];
        let mut serial = evm(APPROVE_BOTH, &[], 5, inspected);
        let cost = first_use_cost(&serial);
        serial.state_mut().enable_bal_builder();
        serial.state_mut().reset_bal_index();
        let mut expected = Vec::new();
        for tx in &transactions {
            serial.state_mut().bump_bal_index();
            expected.push(serial.transact(tx).unwrap().commit());
        }
        assert_eq!(
            expected.iter().map(|result| result.state_gas_spent).collect::<Vec<_>>(),
            [0, cost, cost, cost, 0]
        );
        let bal = Arc::new(serial.state_mut().take_bal_builder().unwrap());
        let mut rebuilt = Bal::new();
        for (index, tx) in transactions.iter().enumerate() {
            let bal_index = BlockAccessIndex::new(index as u64 + 1);
            let mut positioned = evm(APPROVE_BOTH, &[], 5, inspected);
            positioned.state_mut().set_bal(bal.clone());
            positioned.state_mut().set_bal_index(bal_index);
            let detached = positioned.transact(tx).unwrap().detach();
            assert_eq!(detached.result, expected[index]);
            rebuilt.commit(bal_index, detached.pending_state);
        }
        assert_eq!(rebuilt, *bal);
        assert_eq!(legacy_nonce(&mut serial), 6);
        assert_eq!(keyed_nonce(&mut serial, U256::ZERO), U256::from(u64::MAX));
        assert_eq!(keyed_nonce(&mut serial, U256::from(1)), U256::from(2));
        assert_eq!(keyed_nonce(&mut serial, U256::from(2)), U256::from(u64::MAX));
        assert_nonce_mismatch(&mut serial, &approval(vec![U256::ZERO, U256::from(3)], u64::MAX));
        assert_eq!(keyed_nonce(&mut serial, U256::from(3)), U256::ZERO);
    }
}

#[test]
fn nonce_introspection_returns_zero_first_key_for_the_empty_legacy_set() {
    let mut code = Vec::new();
    for (index, selector) in [1, 13, 14, 15, 16].into_iter().enumerate() {
        code.extend_from_slice(&[0x60, selector, 0xb0, 0x60, (index * 32) as u8, 0x52]);
    }
    code.extend_from_slice(&[0x60, 160, 0x5f, 0xf3]);
    for inspected in [false, true] {
        for keys in [vec![], vec![U256::ZERO], vec![U256::from(7)]] {
            let mut evm = evm(APPROVE_BOTH, &code, 5, inspected);
            let sequence = if keys.is_empty() { 5 } else { 0 };
            let result = evm
                .transact(&payload(
                    keys.clone(),
                    sequence,
                    vec![frame(FrameMode::Verify, SENDER, 3), frame(FrameMode::Default, BODY, 0)],
                ))
                .unwrap()
                .discard();
            assert!(result.status, "keys={keys:?}, inspected={inspected}");
            let output = &result.frame.unwrap().outputs[1];
            let actual = output
                .as_chunks::<32>()
                .0
                .iter()
                .copied()
                .map(U256::from_be_bytes)
                .collect::<Vec<_>>();
            assert_eq!(
                actual,
                [
                    U256::from(sequence),
                    U256::from(5),
                    U256::from(keys.len()),
                    U256::from_be_bytes(nonce_keys_hash(&keys).0),
                    keys.first().copied().unwrap_or_default(),
                ]
            );
        }
    }
}
