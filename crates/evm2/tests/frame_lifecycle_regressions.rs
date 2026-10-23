//! Approval, keyed-nonce and cross-frame gas rollback regressions for pinned Bogota.

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{
    Frame, FrameLimits, FrameMode, FrameSignature, FrameStatus, NONCE_MANAGER, SignatureMessage,
    SignatureScheme, nonce_slot,
};
use alloy_primitives::{Address, B256, Bytes, U256};
use evm2::{
    BaseEvmTypes, Evm, EvmFeatures, ExecutionConfig, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
    interpreter::InstrStop,
    registry::HandlerError,
    version::GasId,
};

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);
const FAILURE: Address = Address::with_last_byte(0xc0);
const APPROVE_BOTH: &[u8] = &[0x60, 3, 0x5f, 0x5f, 0xaa];

fn frame(mode: FrameMode, target: Address, flags: u8) -> Frame {
    Frame {
        mode,
        flags,
        target: target.into(),
        limits: FrameLimits { execution: 150_000, state: 300_000 },
        ..Default::default()
    }
}

fn payload(frames: Vec<Frame>, keys: Vec<U256>) -> TxEip8141 {
    TxEip8141 { chain_id: 1, sender: SENDER, nonce_keys: Some(keys), frames, ..Default::default() }
}

const fn recovered(tx: TxEip8141) -> Recovered<TxEnvelope> {
    Recovered::new_unchecked(TxEnvelope::Eip8141(tx), Address::ZERO)
}

fn evm(sender: &[u8], body: &[u8]) -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    for (address, code) in [(SENDER, sender), (BODY, body), (FAILURE, &[0x5f, 0x5f, 0xfd][..])] {
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

fn nonce(evm: &mut Evm<'_, BaseEvmTypes>, key: U256) -> U256 {
    if key.is_zero() {
        U256::from(evm.state_mut().account(&SENDER).unwrap().nonce())
    } else {
        evm.state_mut()
            .storage(&NONCE_MANAGER)
            .unwrap()
            .into_slot(nonce_slot(SENDER, key))
            .unwrap()
            .current()
    }
}

fn set_placeholders(evm: &mut Evm<'_, BaseEvmTypes>, enabled: bool) {
    let mut version = *evm.version();
    version.features.set(EvmFeatures::FRAME_SIGNATURE_PLACEHOLDERS, enabled);
    evm.set_execution_config(
        ExecutionConfig::for_spec_and_version(SpecId::BOGOTA, version),
        SpecId::BOGOTA,
        ethereum_tx_registry(SpecId::BOGOTA),
        Precompiles::base(SpecId::BOGOTA),
    );
}

#[test]
fn reserved_zero_signature_message_is_rejected_even_with_placeholders() {
    for placeholders in [false, true] {
        for scheme in
            [SignatureScheme::Arbitrary, SignatureScheme::Secp256k1, SignatureScheme::P256]
        {
            let mut evm = evm(APPROVE_BOTH, &[]);
            set_placeholders(&mut evm, placeholders);
            let mut tx = payload(vec![frame(FrameMode::Verify, SENDER, 3)], vec![]);
            tx.signatures.push(FrameSignature {
                scheme,
                msg: SignatureMessage::Explicit(B256::ZERO),
                ..Default::default()
            });
            let tx = recovered(tx);
            assert!(matches!(
                evm.validate_tx(&tx),
                Err(HandlerError::InvalidFrameTransaction(
                    "explicit signature message must be a non-zero digest"
                ))
            ));
            assert!(matches!(
                evm.validate_frame_prefix(&tx, 1),
                Err(HandlerError::InvalidFrameTransaction(
                    "explicit signature message must be a non-zero digest"
                ))
            ));
            assert!(matches!(
                evm.transact(&tx),
                Err(HandlerError::InvalidFrameTransaction(
                    "explicit signature message must be a non-zero digest"
                ))
            ));
            assert_eq!(nonce(&mut evm, U256::ZERO), U256::ZERO);
        }
    }
}

#[test]
fn nonzero_arbitrary_message_and_unsigned_simulation_placeholders_remain_valid() {
    for scheme in [SignatureScheme::Arbitrary, SignatureScheme::Secp256k1, SignatureScheme::P256] {
        let mut evm = evm(APPROVE_BOTH, &[]);
        set_placeholders(&mut evm, true);
        let mut tx = payload(vec![frame(FrameMode::Verify, SENDER, 3)], vec![]);
        tx.signatures.push(FrameSignature {
            scheme,
            msg: SignatureMessage::Explicit(B256::with_last_byte(1)),
            ..Default::default()
        });
        assert!(evm.transact(&recovered(tx)).unwrap().discard().status);
    }
}

// A nonempty input delegates approval to BODY, then exits with the requested stop. Empty input
// approves directly, allowing a later frame to retry after an earlier parent failure.
fn delegating_sender(parent_exit: &[u8]) -> Vec<u8> {
    let mut code = vec![0x5f, 0x35, 0x15, 0x60, 0, 0x57];
    code.extend_from_slice(&[0x5f, 0x5f, 0x5f, 0x5f, 0x73]);
    code.extend_from_slice(BODY.as_slice());
    code.extend_from_slice(&[0x5a, 0xf4, 0x50]);
    code.extend_from_slice(parent_exit);
    code[4] = code.len().try_into().unwrap();
    code.push(0x5b);
    code.extend_from_slice(APPROVE_BOTH);
    code
}

#[test]
fn nested_approval_commits_only_after_its_parent_succeeds() {
    for keys in [vec![], vec![U256::from(7), U256::MAX]] {
        let mut evm = evm(&delegating_sender(&[0x00]), APPROVE_BOTH);
        let mut delegated = frame(FrameMode::Default, SENDER, 3);
        delegated.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
        let tx = recovered(payload(vec![delegated], keys.clone()));
        let result = evm.transact(&tx).unwrap().commit();
        assert!(result.status);
        let outcome = result.frame.unwrap();
        assert_eq!(outcome.payer, SENDER);
        assert_eq!(outcome.receipts[0].status, FrameStatus::Success);
        assert_eq!(nonce(&mut evm, U256::ZERO), U256::from(u8::from(keys.is_empty())));
        for key in keys {
            assert_eq!(nonce(&mut evm, key), U256::from(1));
        }
    }
}

#[test]
fn reverted_or_halted_parent_discards_nested_approval_and_nonce_debits() {
    for parent_exit in [&[0x5f, 0x5f, 0xfd][..], &[0xfe][..]] {
        for keys in [vec![], vec![U256::from(7), U256::MAX]] {
            let mut evm = evm(&delegating_sender(parent_exit), APPROVE_BOTH);
            let mut delegated = frame(FrameMode::Default, SENDER, 3);
            delegated.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
            let tx = recovered(payload(
                vec![delegated, frame(FrameMode::Verify, SENDER, 3)],
                keys.clone(),
            ));
            let result = evm.transact(&tx).unwrap().commit();
            assert!(result.status);
            let outcome = result.frame.unwrap();
            assert_eq!(outcome.receipts[0].status, FrameStatus::Failure);
            assert_eq!(outcome.receipts[0].gas_used.state, 0);
            assert_eq!(outcome.receipts[1].status, FrameStatus::Success);
            assert_eq!(nonce(&mut evm, U256::ZERO), U256::from(u8::from(keys.is_empty())));
            for key in keys {
                assert_eq!(nonce(&mut evm, key), U256::from(1));
            }
        }
    }
}

#[test]
fn all_key_first_use_charges_are_required_before_any_nonce_lane_advances() {
    let slot_cost =
        u64::from(evm2::Version::new(SpecId::BOGOTA).gas_params.get(GasId::SstoreSetState));
    for shortage in [1, slot_cost] {
        let mut evm = evm(APPROVE_BOTH, &[]);
        let keys = vec![U256::from(1), U256::MAX];
        let mut insufficient = frame(FrameMode::Default, SENDER, 3);
        insufficient.limits.state = 2 * slot_cost - shortage;
        let tx = recovered(payload(
            vec![insufficient, frame(FrameMode::Verify, SENDER, 3)],
            keys.clone(),
        ));
        let result = evm.transact(&tx).unwrap().commit();
        let outcome = result.frame.unwrap();
        assert_eq!(outcome.stops[0], InstrStop::OutOfGas);
        assert_eq!(outcome.receipts[0].gas_used.state, 0);
        assert_eq!(outcome.receipts[1].gas_used.state, 2 * slot_cost);
        for key in keys {
            assert_eq!(nonce(&mut evm, key), U256::from(1));
        }
        assert_eq!(nonce(&mut evm, U256::ZERO), U256::ZERO);
    }
}

#[test]
fn mixed_first_use_and_existing_nonce_lanes_charge_only_new_storage() {
    let mut evm = evm(APPROVE_BOTH, &[]);
    let existing = U256::from(7);
    let fresh = U256::MAX;
    evm.overlay_db_mut().insert_account_storage(
        &NONCE_MANAGER,
        &nonce_slot(SENDER, existing),
        &U256::from(3),
    );
    // Sequence checking is optional for simulations, while payment still writes every lane
    // to transaction.sequence + 1 and determines first-use cost from current protocol storage.
    let mut version = *evm.version();
    version.features.remove(EvmFeatures::NONCE_CHECK);
    evm.set_execution_config(
        ExecutionConfig::for_spec_and_version(SpecId::BOGOTA, version),
        SpecId::BOGOTA,
        ethereum_tx_registry(SpecId::BOGOTA),
        Precompiles::base(SpecId::BOGOTA),
    );
    let slot_cost = u64::from(evm.version().gas_params.get(GasId::SstoreSetState));
    let mut verify = frame(FrameMode::Verify, SENDER, 3);
    verify.limits.state = slot_cost;
    let result =
        evm.transact(&recovered(payload(vec![verify], vec![existing, fresh]))).unwrap().commit();
    assert_eq!(result.frame.unwrap().receipts[0].gas_used.state, slot_cost);
    assert_eq!(nonce(&mut evm, existing), U256::from(1));
    assert_eq!(nonce(&mut evm, fresh), U256::from(1));
}

#[test]
fn mismatched_lane_invalidates_before_any_key_is_consumed() {
    let mut evm = evm(APPROVE_BOTH, &[]);
    let keys = vec![U256::from(7), U256::MAX];
    evm.overlay_db_mut().insert_account_storage(
        &NONCE_MANAGER,
        &nonce_slot(SENDER, keys[1]),
        &U256::from(9),
    );
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER, 3)], keys.clone()));
    assert!(matches!(
        evm.transact(&tx),
        Err(HandlerError::InvalidFrameTransaction("nonce sequence mismatch"))
    ));
    assert_eq!(nonce(&mut evm, keys[0]), U256::ZERO);
    assert_eq!(nonce(&mut evm, keys[1]), U256::from(9));
    assert_eq!(nonce(&mut evm, U256::ZERO), U256::ZERO);
}

#[test]
fn atomic_restore_rollback_restores_the_original_frames_state_gas_ownership() {
    for final_restore in [false, true] {
        let mut evm = evm(APPROVE_BOTH, &[0x5f, 0x35, 0x5f, 0x55, 0x00]);
        let mut write = frame(FrameMode::Sender, BODY, 0);
        write.data = U256::from(1).to_be_bytes::<32>().to_vec().into();
        let mut restore = frame(FrameMode::Sender, BODY, 4);
        restore.limits.state = 0;
        let mut frames = vec![
            frame(FrameMode::Verify, SENDER, 3),
            write,
            restore,
            frame(FrameMode::Sender, FAILURE, 0),
        ];
        if final_restore {
            let mut restore = frame(FrameMode::Sender, BODY, 0);
            restore.limits.state = 0;
            frames.push(restore);
        }
        let result = evm.transact(&recovered(payload(frames, vec![]))).unwrap().commit();
        let outcome = result.frame.unwrap();
        let slot_cost = u64::from(evm.version().gas_params.get(GasId::SstoreSetState));
        assert_eq!(outcome.receipts[1].gas_used.state, if final_restore { 0 } else { slot_cost });
        assert_eq!(outcome.receipts[2].status, FrameStatus::Success);
        assert!(outcome.rolled_back[2]);
        assert_eq!(outcome.receipts[2].gas_used.state, 0);
        assert_eq!(outcome.receipts[3].status, FrameStatus::Failure);
        assert_eq!(
            evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
            U256::from(u8::from(!final_restore))
        );
    }
}

#[test]
fn later_frame_can_cancel_an_earlier_frames_storage_clear_refund() {
    let mut evm = evm(APPROVE_BOTH, &[0x5f, 0x35, 0x5f, 0x55, 0x00]);
    evm.overlay_db_mut().insert_account_storage(&BODY, &U256::ZERO, &U256::from(7));
    let clear = frame(FrameMode::Sender, BODY, 0);
    let mut rewrite = frame(FrameMode::Sender, BODY, 0);
    rewrite.data = U256::from(9).to_be_bytes::<32>().to_vec().into();
    let tx = recovered(payload(vec![frame(FrameMode::Verify, SENDER, 3), clear, rewrite], vec![]));
    let result = evm.transact(&tx).unwrap().commit();
    assert_eq!(result.refunded, 0);
    assert_eq!(result.state_gas_spent, 0);
    assert_eq!(
        evm.state_mut().storage(&BODY).unwrap().into_slot(U256::ZERO).unwrap().current(),
        U256::from(9)
    );
}

#[test]
fn a_nested_external_callee_cannot_approve_the_top_level_targets_scope() {
    let mut sender = vec![0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73];
    sender.extend_from_slice(BODY.as_slice());
    sender.extend_from_slice(&[
        0x5a, 0xf1, // CALL BODY: its APPROVE must fail because its destination is BODY.
        0x5f, 0x52, // Return the child success flag as the direct approval's output.
        0x60, 3, 0x60, 32, 0x5f, 0xaa,
    ]);
    for keys in [vec![], vec![U256::from(7), U256::MAX]] {
        let mut evm = evm(&sender, APPROVE_BOTH);
        let tx = recovered(payload(vec![frame(FrameMode::Default, SENDER, 3)], keys.clone()));
        let result = evm.transact(&tx).unwrap().commit();
        let outcome = result.frame.unwrap();
        assert_eq!(outcome.payer, SENDER);
        assert_eq!(outcome.outputs[0].as_ref(), [0; 32]);
        assert_eq!(nonce(&mut evm, U256::ZERO), U256::from(u8::from(keys.is_empty())));
        for key in keys {
            assert_eq!(nonce(&mut evm, key), U256::from(1));
        }
    }
}

#[test]
fn protocol_nonce_reads_and_writes_do_not_warm_ordinary_evm_accesses() {
    let key = U256::MAX;
    let mut manager = vec![0x7f];
    manager.extend_from_slice(&nonce_slot(SENDER, key).to_be_bytes::<32>());
    manager.extend_from_slice(&[0x54, 0x50, 0x00]); // SLOAD; POP; STOP.
    let mut evm = evm(APPROVE_BOTH, &[]);
    evm.overlay_db_mut().insert_account_info(
        &NONCE_MANAGER,
        AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(manager.into())),
    );
    let tx = recovered(payload(
        vec![
            frame(FrameMode::Verify, SENDER, 3),
            frame(FrameMode::Default, NONCE_MANAGER, 0),
            frame(FrameMode::Default, NONCE_MANAGER, 0),
        ],
        vec![key],
    ));
    let result = evm.transact(&tx).unwrap().commit();
    let outcome = result.frame.unwrap();
    assert_eq!(outcome.receipts[1].gas_used.execution, 3_000 + 3 + 2_100 + 2);
    assert_eq!(outcome.receipts[2].gas_used.execution, 100 + 3 + 100 + 2);
    assert_eq!(nonce(&mut evm, key), U256::from(1));
}

#[test]
fn overlapping_and_disjoint_domains_advance_independently_between_transactions() {
    let mut evm = evm(APPROVE_BOTH, &[]);
    let slot_cost = u64::from(evm.version().gas_params.get(GasId::SstoreSetState));
    for (keys, sequence, expected_state_gas) in
        [(vec![7, 9], 0, 2 * slot_cost), (vec![9], 1, 0), (vec![11], 0, slot_cost)]
    {
        let mut tx = payload(
            vec![frame(FrameMode::Verify, SENDER, 3)],
            keys.into_iter().map(U256::from).collect(),
        );
        tx.nonce = sequence;
        let result = evm.transact(&recovered(tx)).unwrap().commit();
        assert_eq!(result.frame.unwrap().receipts[0].gas_used.state, expected_state_gas);
    }
    let mut stale_overlap =
        payload(vec![frame(FrameMode::Verify, SENDER, 3)], vec![U256::from(7), U256::from(9)]);
    stale_overlap.nonce = 1;
    assert!(matches!(
        evm.transact(&recovered(stale_overlap)),
        Err(HandlerError::InvalidFrameTransaction("nonce sequence mismatch"))
    ));
    for (key, expected) in [(0, 0), (7, 1), (9, 2), (11, 1)] {
        assert_eq!(nonce(&mut evm, U256::from(key)), U256::from(expected));
    }
}

#[test]
fn keyed_payment_uses_the_active_state_gas_schedule() {
    let mut evm = evm(APPROVE_BOTH, &[]);
    let mut version = *evm.version();
    version.gas_params.set(GasId::SstoreSetState, 17);
    evm.set_execution_config(
        ExecutionConfig::for_spec_and_version(SpecId::BOGOTA, version),
        SpecId::BOGOTA,
        ethereum_tx_registry(SpecId::BOGOTA),
        Precompiles::base(SpecId::BOGOTA),
    );
    let mut verify = frame(FrameMode::Verify, SENDER, 3);
    verify.limits.state = 34;
    let tx = recovered(payload(vec![verify], vec![U256::from(7), U256::MAX]));
    let prefix = evm.validate_frame_prefix(&tx, 1).unwrap();
    assert_eq!(prefix.state_gas, 34);
    assert_eq!(nonce(&mut evm, U256::from(7)), U256::ZERO);
    assert_eq!(nonce(&mut evm, U256::MAX), U256::ZERO);
    let result = evm.transact(&tx).unwrap().commit();
    assert_eq!(result.frame.unwrap().receipts[0].gas_used.state, 34);
    assert_eq!(nonce(&mut evm, U256::from(7)), U256::from(1));
    assert_eq!(nonce(&mut evm, U256::MAX), U256::from(1));
}
