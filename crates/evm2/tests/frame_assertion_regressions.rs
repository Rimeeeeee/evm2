//! Bogota assertion selectors, transaction-local views, and ordinary recent-root execution.

use alloy_consensus::{TxEip8141, TxLegacy, transaction::Recovered};
use alloy_eips::{
    eip8141::{Frame, FrameLimits, FrameMode, FrameStatus, NONCE_MANAGER},
    eip8272::{
        RECENT_ROOT_ADDRESS, RECENT_ROOT_CODE, RECENT_ROOT_ENTRY_DOMAIN, RECENT_ROOT_STORAGE_DOMAIN,
    },
};
use alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, TxKind, U256, keccak256};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, eip8141::FrameGas, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
    interpreter::InstrStop,
};

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);
const OTHER: Address = Address::with_last_byte(0xb1);
const ASSERTION: Address = Address::with_last_byte(0xc0);
const RECEIVER: Address = Address::with_last_byte(0xd0);
const WRITER: Address = Address::with_last_byte(0xe0);
const BALANCE: u64 = 1_000_000_000;

fn frame(mode: FrameMode, target: Address) -> Frame {
    Frame {
        mode,
        target: target.into(),
        flags: if mode == FrameMode::Verify { 3 } else { 0 },
        limits: FrameLimits { execution: 200_000, state: 400_000 },
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
    Recovered::new_unchecked(TxEnvelope::Eip8141(tx), Address::ZERO)
}

fn evm(body: &[u8], other: &[u8], assertion: &[u8]) -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    for (address, code) in [
        (SENDER, &[0x60, 3, 0x5f, 0x5f, 0xaa][..]),
        (BODY, body),
        (OTHER, other),
        (ASSERTION, assertion),
    ] {
        db.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(BALANCE))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    db.insert_account_info(&RECEIVER, AccountInfo::default().with_balance(U256::from(100)));
    db.insert_account_info(&WRITER, AccountInfo::default().with_balance(U256::from(BALANCE)));
    Evm::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        Precompiles::base(SpecId::BOGOTA),
    )
}

fn word(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

fn push(code: &mut Vec<u8>, value: U256) {
    code.push(0x7f);
    code.extend_from_slice(&value.to_be_bytes::<32>());
}

fn push_u64(code: &mut Vec<u8>, value: u64) {
    push(code, U256::from(value));
}

fn store_output(code: &mut Vec<u8>, offset: u64) {
    push_u64(code, offset);
    code.push(0x52);
}

fn trace(code: &mut Vec<u8>, selector: U256, index: U256, offset: u64) {
    push(code, index);
    push(code, selector);
    code.push(0xb6);
    store_output(code, offset);
}

fn diff(code: &mut Vec<u8>, selector: U256, operand: U256, third: U256, offset: u64) {
    push(code, third);
    push(code, operand);
    push(code, selector);
    code.push(0xb7);
    store_output(code, offset);
}

fn return_memory(code: &mut Vec<u8>, offset: u64, len: u64) {
    push_u64(code, len);
    push_u64(code, offset);
    code.push(0xf3);
}

fn words(data: &[u8]) -> Vec<U256> {
    assert_eq!(data.len() % 32, 0);
    data.chunks_exact(32).map(U256::from_be_slice).collect()
}

#[test]
fn all_assertion_selectors_observe_sorted_net_changes_and_indexed_events() {
    let mut body = vec![0x60, 90, 0x60, 9, 0x55, 0x60, 20, 0x60, 2, 0x55];
    // Deploy a one-byte STOP runtime. The CREATE result is intentionally unused.
    body.extend_from_slice(&[0x63, 0x60, 0x01, 0x5f, 0xf3, 0x5f, 0x52]);
    body.extend_from_slice(&[0x60, 4, 0x60, 28, 0x5f, 0xf0, 0x50]);
    for value in [
        U256::ZERO,
        U256::ZERO,
        U256::ZERO,
        U256::ZERO,
        U256::from(7),
        word(RECEIVER),
        U256::from(30_000),
    ] {
        push(&mut body, value);
    }
    body.extend_from_slice(&[0xf1, 0x50]);
    push_u64(&mut body, 0xabcd);
    store_output(&mut body, 0);
    // Topics one and two repeat; indexed-topic lookup must count this log once.
    for value in [0x22, 0x21, 0x21, 0x10, 2, 30] {
        push_u64(&mut body, value);
    }
    body.extend_from_slice(&[0xa4, 0x00]);
    // The same topic as topic zero is excluded from the indexed-topic lookup.
    let other = [0x60, 11, 0x60, 1, 0x55, 0x60, 0x21, 0x5f, 0x5f, 0xa1, 0x00];
    let deployed = BODY.create(0);
    let deployed_hash = U256::from_be_bytes(keccak256([0x00]).0);
    let body_hash = U256::from_be_bytes(keccak256(&body).0);
    let mut assertion = Vec::new();
    for selector in 0..=0x15 {
        // Amsterdam's ETH transfer log precedes the explicit LOG4.
        let index = u64::from((0x0d..=0x13).contains(&selector));
        trace(&mut assertion, U256::from(selector), U256::from(index), selector * 32);
    }
    let queries = [
        (0x00, word(BODY), U256::from(2), U256::ZERO),
        (0x01, word(BODY), U256::from(2), U256::from(20)),
        (0x02, word(BODY), U256::ZERO, U256::from(BALANCE)),
        (0x03, word(BODY), U256::ZERO, U256::from(BALANCE - 7)),
        (0x04, word(BODY), U256::ZERO, body_hash),
        (0x05, word(BODY), U256::ZERO, body_hash),
        (0x06, word(BODY), U256::ZERO, U256::from(2)),
        (0x07, word(BODY), U256::from(1), U256::from(1)),
        (0x08, word(BODY), U256::ZERO, U256::from(1)),
        (0x09, word(BODY), U256::ZERO, U256::from(1)),
        (0x0a, word(BODY), U256::ZERO, U256::from(7)),
        (0x0b, U256::from(0x21), U256::ZERO, U256::from(1)),
        (0x0c, U256::from(0x21), U256::ZERO, U256::from(1)),
        (0x04, word(deployed), U256::ZERO, U256::from_be_bytes(KECCAK256_EMPTY.0)),
        (0x05, word(deployed), U256::ZERO, deployed_hash),
        (0x0a, word(deployed), U256::ZERO, U256::from(9)),
        (0x07, word(OTHER), U256::ZERO, U256::from(2)),
        (0x09, word(OTHER), U256::ZERO, U256::from(2)),
        (0x0b, U256::from(0x10), U256::ZERO, U256::ZERO),
        (0x0b, U256::MAX, U256::ZERO, U256::ZERO),
        (0x0a, U256::MAX, U256::ZERO, U256::ZERO),
    ];
    for (i, &(selector, operand, third, _)) in queries.iter().enumerate() {
        diff(&mut assertion, U256::from(selector), operand, third, (22 + i as u64) * 32);
    }
    return_memory(&mut assertion, 0, (22 + queries.len() as u64) * 32);
    let mut evm = evm(&body, &other, &assertion);
    let mut tx = payload(vec![
        frame(FrameMode::Verify, SENDER),
        frame(FrameMode::Sender, BODY),
        frame(FrameMode::Sender, OTHER),
        frame(FrameMode::PostTx, ASSERTION),
        frame(FrameMode::PostTx, ASSERTION),
    ]);
    tx.fees.max_fee_per_gas = U256::from(1);
    tx.fees.max_priority_fee_per_gas = U256::from(1);
    let precharge = U256::from(FrameGas::calculate(&tx, evm.version()).unwrap().maximum);
    let result = evm.transact(&recovered(tx)).unwrap().commit();
    assert!(result.status, "{:?}", result.frame);
    let result = result.frame.unwrap();
    assert!(result.receipts.iter().all(|r| r.status == FrameStatus::Success));
    let mut expected = vec![
        U256::from(3),
        U256::from(3),
        U256::from(1),
        word(SENDER),
        U256::from(BALANCE),
        U256::from(BALANCE) - precharge,
        word(BODY),
        U256::from(2),
        U256::ZERO,
        U256::from(20),
        word(deployed),
        deployed_hash,
        U256::from(3),
        word(BODY),
        U256::from(4),
        U256::from(0x10),
        U256::from(0x21),
        U256::from(0x21),
        U256::from(0x22),
        U256::from(2),
        precharge,
        word(SENDER),
    ];
    expected.extend(queries.iter().map(|q| q.3));
    assert_eq!(words(&result.outputs[3]), expected);
    // A second assertion sees the same frozen view despite TXDIFF's extra live reads.
    assert_eq!(result.outputs[3], result.outputs[4]);
    assert_eq!(result.precharge, precharge);
}

#[test]
fn selector_and_index_operands_are_checked_at_full_width() {
    let mut cases = Vec::new();
    for selector in 0..=0x15 {
        let mut code = Vec::new();
        trace(&mut code, U256::from(selector), U256::MAX, 0);
        cases.push(code);
    }
    for selector in [U256::from(0x16), U256::from(0x100), U256::MAX] {
        let mut code = Vec::new();
        trace(&mut code, selector, U256::ZERO, 0);
        cases.push(code);
    }
    for selector in [U256::from(0x0d), U256::from(0x100), U256::MAX] {
        let mut code = Vec::new();
        diff(&mut code, selector, word(BODY), U256::ZERO, 0);
        cases.push(code);
    }
    for selector in [2, 3, 4, 5, 6, 8, 10, 11] {
        let mut code = Vec::new();
        diff(&mut code, U256::from(selector), word(BODY), U256::from(1), 0);
        cases.push(code);
    }
    for selector in [7, 9, 12] {
        let mut code = Vec::new();
        diff(&mut code, U256::from(selector), word(BODY), U256::MAX, 0);
        cases.push(code);
    }
    for (i, code) in cases.iter().enumerate() {
        let mut evm = evm(&[0x60, 42, 0x5f, 0x55, 0x00], &[], code);
        let tx = recovered(payload(vec![
            frame(FrameMode::Verify, SENDER),
            frame(FrameMode::Sender, BODY),
            frame(FrameMode::PostTx, ASSERTION),
        ]));
        let result = evm.transact(&tx).unwrap().commit();
        assert!(!result.status, "case {i}");
        let result = result.frame.unwrap();
        assert_eq!(result.stops[2], InstrStop::OpcodeNotFound, "case {i}");
        assert_eq!(result.assertion_failure, Some(2));
        assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
        assert_eq!(
            evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
            U256::ZERO
        );
    }
}

#[test]
fn event_copy_bounds_and_zero_length_follow_return_data_rules() {
    let body = [0x60, 0xab, 0x5f, 0x53, 0x60, 1, 0x5f, 0xa0, 0x00];
    for (index, memory, offset, len, stop, expected) in [
        (U256::ZERO, U256::ZERO, U256::ZERO, 1, InstrStop::Return, vec![0xab]),
        (U256::ZERO, U256::MAX, U256::from(1), 0, InstrStop::Return, vec![]),
        (U256::ZERO, U256::ZERO, U256::from(1), 1, InstrStop::OutOfOffset, vec![]),
        (U256::ZERO, U256::ZERO, U256::MAX, 0, InstrStop::OutOfOffset, vec![]),
        (U256::from(1), U256::ZERO, U256::ZERO, 0, InstrStop::OpcodeNotFound, vec![]),
        (U256::MAX, U256::ZERO, U256::ZERO, 0, InstrStop::OpcodeNotFound, vec![]),
    ] {
        let mut assertion = Vec::new();
        for value in [U256::from(len), offset, memory, index] {
            push(&mut assertion, value);
        }
        assertion.push(0xb8);
        return_memory(&mut assertion, 0, len);
        let mut evm = evm(&body, &[], &assertion);
        let tx = recovered(payload(vec![
            frame(FrameMode::Verify, SENDER),
            frame(FrameMode::Sender, BODY),
            frame(FrameMode::PostTx, ASSERTION),
        ]));
        let result = evm.transact(&tx).unwrap().discard().frame.unwrap();
        assert_eq!(result.stops[2], stop, "index {index}, offset {offset}, len {len}");
        assert_eq!(result.outputs[2].as_ref(), expected);
    }
}

#[test]
fn reverted_logs_and_restored_slots_are_absent_and_views_reset_between_transactions() {
    // The successful body changes a slot back to its transaction original.
    let body = [0x60, 1, 0x5f, 0x55, 0x5f, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x00];
    let reverted = [0x60, 2, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x5f, 0x5f, 0xfd];
    let mut assertion = Vec::new();
    trace(&mut assertion, U256::from(1), U256::ZERO, 0);
    trace(&mut assertion, U256::from(12), U256::ZERO, 32);
    diff(&mut assertion, U256::from(10), word(BODY), U256::ZERO, 64);
    return_memory(&mut assertion, 0, 96);
    let mut evm = evm(&body, &reverted, &assertion);
    let first = recovered(payload(vec![
        frame(FrameMode::Verify, SENDER),
        frame(FrameMode::Sender, BODY),
        frame(FrameMode::Sender, OTHER),
        frame(FrameMode::PostTx, ASSERTION),
    ]));
    let result = evm.transact(&first).unwrap().commit().frame.unwrap();
    assert_eq!(result.receipts[2].status, FrameStatus::Failure);
    assert_eq!(words(&result.outputs[3]), [U256::ZERO, U256::from(1), U256::ZERO]);
    let mut second =
        payload(vec![frame(FrameMode::Verify, SENDER), frame(FrameMode::PostTx, ASSERTION)]);
    second.nonce = 1;
    let result = evm.transact(&recovered(second)).unwrap().commit().frame.unwrap();
    assert_eq!(words(&result.outputs[1]), [U256::ZERO; 3]);
}

#[test]
fn assertion_opcodes_are_unavailable_to_body_and_ordinary_transactions() {
    for opcode in [0xb6, 0xb7, 0xb8] {
        let code = [0x5f, 0x5f, 0x5f, 0x5f, opcode, 0x00];
        let mut evm = evm(&code, &[], &[]);
        let tx = recovered(payload(vec![
            frame(FrameMode::Verify, SENDER),
            frame(FrameMode::Sender, BODY),
        ]));
        let result = evm.transact(&tx).unwrap().discard().frame.unwrap();
        assert_eq!(result.receipts[1].status, FrameStatus::Failure);
        assert_eq!(result.stops[1], InstrStop::OpcodeNotFound);

        let ordinary = Recovered::new_unchecked(
            TxEnvelope::Legacy(TxLegacy {
                gas_limit: 1_000_000,
                to: TxKind::Call(BODY),
                ..Default::default()
            }),
            WRITER,
        );
        let result = evm.transact(&ordinary).unwrap().discard();
        assert!(!result.status);
        assert_eq!(result.stop, InstrStop::OpcodeNotFound);
    }
}

fn install_recent_root(evm: &mut Evm<'_, BaseEvmTypes>) {
    evm.overlay_db_mut().insert_account_info(
        &RECENT_ROOT_ADDRESS,
        AccountInfo::default().with_code(Bytecode::new_legacy(RECENT_ROOT_CODE.clone())),
    );
}

fn recent_tuple(source: [u8; 32], slot: u64, root: [u8; 32]) -> Vec<u8> {
    let mut tuple = source.to_vec();
    tuple.extend_from_slice(&slot.to_be_bytes());
    tuple.extend_from_slice(&root);
    tuple
}

fn recent_slot(source: [u8; 32], slot: u64) -> U256 {
    let mut input = RECENT_ROOT_STORAGE_DOMAIN.as_slice().to_vec();
    input.extend_from_slice(&source);
    input.extend_from_slice(&(slot % 8192).to_be_bytes());
    U256::from_be_bytes(keccak256(input).0)
}

fn recent_entry(tuple: &[u8]) -> U256 {
    let mut input = RECENT_ROOT_ENTRY_DOMAIN.as_slice().to_vec();
    input.extend_from_slice(tuple);
    U256::from_be_bytes(keccak256(input).0)
}

fn recent_query(target: Address, data: Vec<u8>) -> Recovered<TxEnvelope> {
    let mut verifier = frame(FrameMode::Verify, target);
    verifier.flags = 0;
    verifier.data = data.into();
    recovered(payload(vec![verifier, frame(FrameMode::Verify, SENDER)]))
}

#[test]
fn recent_root_write_read_and_ring_overwrite_execute_ordinary_contract_code() {
    let mut evm = evm(&[], &[], &[]);
    install_recent_root(&mut evm);
    let salt = [0x11; 32];
    let mut source_input = WRITER.as_slice().to_vec();
    source_input.extend_from_slice(&salt);
    let source = keccak256(source_input).0;
    for (nonce, slot, root) in [(0, 100, [0x22; 32]), (1, 8292, [0x33; 32])] {
        evm.set_block(BlockEnvExt {
            slot_num: U256::from(slot),
            timestamp: U256::from(987_654),
            ..*evm.block()
        });
        let mut data = salt.to_vec();
        data.extend_from_slice(&root);
        let write = Recovered::new_unchecked(
            TxEnvelope::Legacy(TxLegacy {
                nonce,
                gas_limit: 1_000_000,
                to: TxKind::Call(RECENT_ROOT_ADDRESS),
                input: data.into(),
                ..Default::default()
            }),
            WRITER,
        );
        assert!(evm.transact(&write).unwrap().commit().status);
        let tuple = recent_tuple(source, slot, root);
        assert_eq!(
            evm.state_mut()
                .storage(&RECENT_ROOT_ADDRESS)
                .unwrap()
                .into_slot(recent_slot(source, slot))
                .unwrap()
                .current(),
            recent_entry(&tuple)
        );
        assert!(evm.transact(&recent_query(RECENT_ROOT_ADDRESS, tuple.clone())).is_err());
        evm.set_block(BlockEnvExt { slot_num: U256::from(slot + 1), ..*evm.block() });
        assert!(
            evm.transact(&recent_query(RECENT_ROOT_ADDRESS, tuple.clone()))
                .unwrap()
                .discard()
                .status
        );
        if nonce == 1 {
            // Even with the new slot, an overwritten root does not satisfy the commitment.
            assert!(
                evm.transact(&recent_query(
                    RECENT_ROOT_ADDRESS,
                    recent_tuple(source, slot, [0x22; 32])
                ))
                .is_err()
            );
            assert!(
                evm.transact(&recent_query(
                    RECENT_ROOT_ADDRESS,
                    recent_tuple(source, 100, [0x22; 32])
                ))
                .is_err()
            );
        }
    }
}

#[test]
fn recent_root_verification_checks_every_tuple_and_delegated_storage_context() {
    let source = [0x11; 32];
    let tuple = recent_tuple(source, 100, [0x22; 32]);
    let alias = Address::with_last_byte(0xf0);
    let mut evm = evm(&[], &[], &[]);
    install_recent_root(&mut evm);
    evm.overlay_db_mut().insert_account_info(
        &alias,
        AccountInfo::default().with_code(Bytecode::new_eip7702(RECENT_ROOT_ADDRESS)),
    );
    evm.overlay_db_mut().insert_account_storage(
        &alias,
        &recent_slot(source, 100),
        &recent_entry(&tuple),
    );
    evm.set_block(BlockEnvExt { slot_num: U256::from(101), ..*evm.block() });
    // Delegated execution reads the alias's state, not the implementation account's state.
    assert!(evm.transact(&recent_query(alias, tuple.clone())).unwrap().discard().status);
    assert!(evm.transact(&recent_query(RECENT_ROOT_ADDRESS, tuple.clone())).is_err());
    assert!(evm.transact(&recent_query(alias, tuple.repeat(16))).unwrap().discard().status);
    let mut changed_source = tuple.clone();
    changed_source[0] ^= 1;
    let mut changed_slot = tuple.clone();
    changed_slot[39] ^= 1;
    let mut changed_root = tuple.clone();
    changed_root[71] ^= 1;
    let mut truncated = tuple.clone();
    truncated.pop();
    let mut trailing = tuple.clone();
    trailing.push(0);
    for invalid in [
        vec![],
        tuple.repeat(17),
        changed_source,
        changed_slot,
        changed_root.clone(),
        truncated,
        trailing,
    ] {
        assert!(evm.transact(&recent_query(alias, invalid)).is_err());
    }
    let mut second_invalid = tuple.clone();
    second_invalid.extend_from_slice(&changed_root);
    assert!(evm.transact(&recent_query(alias, second_invalid)).is_err());
    // Failure cleanup does not poison a later valid executable-prefix check.
    let valid = recent_query(alias, tuple);
    let prefix = evm.validate_frame_prefix(&valid, 2).unwrap();
    assert_eq!(prefix.payer, SENDER);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 0);
}

#[test]
fn recent_root_address_has_no_implicit_precompile_or_installed_runtime() {
    for (runtime, valid) in [
        (None, false),
        (Some(RECENT_ROOT_CODE.clone()), false),
        (Some(Bytes::from_static(&[0x00])), true),
    ] {
        let mut evm = evm(&[], &[], &[]);
        if let Some(runtime) = runtime {
            evm.overlay_db_mut().insert_account_info(
                &RECENT_ROOT_ADDRESS,
                AccountInfo::default().with_code(Bytecode::new_legacy(runtime)),
            );
        }
        // Malformed verifier data is governed entirely by the actual deployed runtime.
        assert_eq!(
            evm.transact(&recent_query(RECENT_ROOT_ADDRESS, vec![1]))
                .map(|tx| tx.discard())
                .is_ok(),
            valid
        );
    }
}

#[test]
fn recent_root_write_is_rejected_in_static_assertions_and_preserves_the_payment_prefix() {
    let mut evm = evm(&[], &[], &[]);
    install_recent_root(&mut evm);
    evm.set_block(BlockEnvExt { slot_num: U256::from(100), ..*evm.block() });
    let mut write = frame(FrameMode::PostTx, RECENT_ROOT_ADDRESS);
    write.data = vec![0x11; 64].into();
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER), write]));
    let result = evm.transact(&tx).unwrap().commit();
    assert!(!result.status);
    let result = result.frame.unwrap();
    assert_eq!(result.stops[1], InstrStop::StateChangeDuringStaticCall);
    assert_eq!(result.assertion_failure, Some(1));
    assert_eq!(result.receipts[1].gas_used.state, 0);
    assert_eq!(evm.state_mut().account(&SENDER).unwrap().nonce(), 1);
}
