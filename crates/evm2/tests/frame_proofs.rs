//! PROOF-frame verification, purity, isolation, and introspection regressions.

mod common;

use alloy_eips::eip8141::{
    Frame, FrameLimits, FrameMode, LEANSPHINCS_VERIFIER, P256_VERIFIER, PROOF_READ_FLAG,
    SECP256K1_VERIFIER,
};
use alloy_primitives::{Address, B256, U256, hex};
use common::{APPROVE_BOTH, SENDER, frame, payload, recovered};
use evm2::{
    Precompiles, SpecId,
    interpreter::GasTracker,
    precompiles::frame_verifiers,
    registry::{FrameTransactionError, HandlerError},
};

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const VERIFIER: Address = Address::with_last_byte(0xc1);

fn proof(target: Address, msg: B256, witness: &[u8], gas: u64) -> Frame {
    let mut data = B256::repeat_byte(1).to_vec();
    data.extend_from_slice(msg.as_slice());
    data.extend_from_slice(witness);
    Frame {
        mode: FrameMode::Proof,
        target: target.into(),
        data: data.into(),
        limits: FrameLimits { execution: gas, state: 0 },
        ..Default::default()
    }
}

#[test]
fn verifier_registration_is_fork_specific_and_cache_order_independent() {
    for spec in
        [SpecId::OSAKA, SpecId::AMSTERDAM, SpecId::BOGOTA, SpecId::AMSTERDAM, SpecId::BOGOTA]
    {
        let precompiles = Precompiles::<evm2::BaseEvmTypes>::base(spec);
        for address in [SECP256K1_VERIFIER, P256_VERIFIER, LEANSPHINCS_VERIFIER] {
            assert_eq!(precompiles.as_map().contains(&address), spec == SpecId::BOGOTA);
        }
    }
}

#[test]
fn malformed_verifier_inputs_fail_and_consume_every_forwarded_gas_unit() {
    for (run, cost, len) in [
        (frame_verifiers::secp256k1 as fn(&[u8], &mut GasTracker) -> _, 3000, 129),
        (frame_verifiers::p256, 6900, 192),
        (frame_verifiers::leansphincs, 13200, 7804),
    ] {
        for length in [0, 63, len - 1, len, len + 1] {
            let mut gas = GasTracker::new_isolated(cost + 100, 0);
            assert!(run(&vec![0; length], &mut gas).is_err());
            assert_eq!(gas.remaining(), 0);
        }
        let mut gas = GasTracker::new_isolated(cost - 1, 0);
        assert!(run(&vec![0; len], &mut gas).is_err());
        assert_eq!(gas.remaining(), 0);
    }
}

#[test]
fn leansphincs_known_answer_and_mutations_use_the_exact_verifier_abi() {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("data/leansphincs-proof.json")).unwrap();
    let mut input = hex::decode(vector["public_key"].as_str().unwrap()).unwrap();
    input.extend(hex::decode(vector["message"].as_str().unwrap()).unwrap());
    input.extend(hex::decode(vector["signature"].as_str().unwrap()).unwrap());
    assert_eq!(input.len(), 7804);
    let mut gas = GasTracker::new_isolated(13201, 0);
    let output = frame_verifiers::leansphincs(&input, &mut gas).unwrap();
    assert_eq!(output.into_bytes().as_ref(), U256::ONE.to_be_bytes::<32>());
    assert_eq!(gas.remaining(), 1);
    for offset in [0, 31, 32, 63, 64, 1000, 7803] {
        input[offset] ^= 1;
        let mut gas = GasTracker::new_isolated(20000, 0);
        assert!(frame_verifiers::leansphincs(&input, &mut gas).is_err());
        assert_eq!(gas.remaining(), 0);
        input[offset] ^= 1;
    }
}

#[test]
fn contract_verification_is_charged_in_full_and_does_not_warm_the_body_target() {
    let db = common::database_with_nonce_manager([(SENDER, APPROVE_BOTH), (VERIFIER, &[0x00][..])]);
    let mut evm = common::evm(db);
    let tx = payload(vec![
        proof(VERIFIER, B256::ZERO, &[], 4000),
        frame(FrameMode::Verify, SENDER),
        frame(FrameMode::Sender, VERIFIER),
    ]);
    let result = evm.transact(&recovered(tx)).unwrap().discard();
    let receipts = result.frame.unwrap().receipts;
    assert_eq!(receipts[0].gas_used.execution, 4000);
    assert_eq!(receipts[0].gas_used.state, 0);
    assert_eq!(receipts[2].gas_used.execution, 3000);
}

#[test]
fn pure_verifiers_reject_context_state_calls_and_unknown_extensions() {
    for opcode in [
        0x30, 0x31, 0x32, 0x33, 0x3a, 0x3b, 0x3c, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
        0x47, 0x48, 0x49, 0x4a, 0x4b, 0x54, 0x5c, 0xaa, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xf1, 0xf2,
        0xf4,
    ] {
        // Supply operands so a missing purity guard cannot pass this test via stack underflow.
        let mut code = vec![0x5f; 7];
        code.extend_from_slice(&[opcode, 0x00]);
        let db = common::database_with_nonce_manager([
            (SENDER, APPROVE_BOTH),
            (VERIFIER, code.as_slice()),
        ]);
        let mut evm = common::evm(db);
        let tx = payload(vec![
            proof(VERIFIER, B256::ZERO, &[], 10000),
            frame(FrameMode::Verify, SENDER),
        ]);
        assert_eq!(
            evm.transact(&recovered(tx)).unwrap_err(),
            HandlerError::InvalidFrameTransaction(FrameTransactionError::ProofFrameFailed),
            "opcode {opcode:#x}"
        );
    }
}

#[test]
fn zero_message_is_resolved_for_the_verifier_without_mutating_the_encoded_claim() {
    // Return failure unless CALLDATALOAD(32) is nonzero.
    let code = [0x60, 32, 0x35, 0x15, 0x60, 8, 0x57, 0x00, 0x5b, 0x5f, 0x5f, 0xfd];
    let db =
        common::database_with_nonce_manager([(SENDER, APPROVE_BOTH), (VERIFIER, code.as_slice())]);
    let mut evm = common::evm(db);
    let tx =
        payload(vec![proof(VERIFIER, B256::ZERO, &[], 10000), frame(FrameMode::Verify, SENDER)]);
    let encoded = tx.frames[0].data.clone();
    assert!(evm.transact(&recovered(tx.clone())).is_ok());
    assert_eq!(tx.frames[0].data, encoded);
}

#[test]
fn proof_data_requires_verify_mode_and_the_read_flag_even_for_zero_length_copy() {
    // FRAMEDATACOPY(0,0,0,0), then approve payment and execution.
    let read = [0x5f, 0x5f, 0x5f, 0x5f, 0xb2, 0x60, 3, 0x5f, 0x5f, 0xaa];
    for readable in [false, true] {
        let db = common::database_with_nonce_manager([
            (SENDER, read.as_slice()),
            (VERIFIER, &[0x00][..]),
        ]);
        let mut evm = common::evm(db);
        let mut claim = proof(VERIFIER, B256::ZERO, &[], 4000);
        claim.flags = if readable { PROOF_READ_FLAG } else { 0 };
        let tx = payload(vec![claim, frame(FrameMode::Verify, SENDER)]);
        assert_eq!(evm.transact(&recovered(tx)).is_ok(), readable);
    }
}

#[test]
fn proof_contracts_bypass_the_configured_compiled_runner() {
    let db = common::database_with_nonce_manager([(SENDER, APPROVE_BOTH), (VERIFIER, &[0x00][..])]);
    let mut evm = common::evm(db);
    let calls = Arc::new(AtomicUsize::new(0));
    evm.set_interpreter_runner(common::CountingRunner(calls.clone()));
    let tx =
        payload(vec![proof(VERIFIER, B256::ZERO, &[], 4000), frame(FrameMode::Verify, SENDER)]);
    assert!(evm.transact(&recovered(tx)).is_ok());
    assert_eq!(calls.load(Ordering::Relaxed), 1, "only VERIFY may enter the compiled runner");
}

#[test]
fn pure_staticcall_only_accepts_active_pure_precompiles() {
    for (target, valid) in [(2, true), (1, false), (0xc2, false)] {
        // STATICCALL(gas, target, 0, 0, 0, 0), discard success, STOP.
        let code = [0x5f, 0x5f, 0x5f, 0x5f, 0x60, target, 0x61, 0x03, 0xe8, 0xfa, 0x50, 0x00];
        let db = common::database_with_nonce_manager([
            (SENDER, APPROVE_BOTH),
            (VERIFIER, code.as_slice()),
        ]);
        let mut evm = common::evm(db);
        let tx = payload(vec![
            proof(VERIFIER, B256::ZERO, &[], 10000),
            frame(FrameMode::Verify, SENDER),
        ]);
        assert_eq!(evm.transact(&recovered(tx)).is_ok(), valid, "target {target:#x}");
    }
}

#[test]
fn proof_verifiers_reject_empty_code_delegation_and_stateful_ecrecover() {
    for code in [vec![], [vec![0xef, 0x01, 0x00], vec![0; 20]].concat()] {
        let db = common::database_with_nonce_manager([
            (SENDER, APPROVE_BOTH),
            (VERIFIER, code.as_slice()),
        ]);
        let mut evm = common::evm(db);
        let tx = payload(vec![
            proof(VERIFIER, B256::ZERO, &[], 10000),
            frame(FrameMode::Verify, SENDER),
        ]);
        assert_eq!(
            evm.transact(&recovered(tx)).unwrap_err(),
            HandlerError::InvalidFrameTransaction(FrameTransactionError::ProofFrameFailed)
        );
    }
    let db = common::database_with_nonce_manager([(SENDER, APPROVE_BOTH)]);
    let mut evm = common::evm(db);
    let tx = payload(vec![
        proof(Address::with_last_byte(1), B256::ZERO, &[], 10000),
        frame(FrameMode::Verify, SENDER),
    ]);
    assert_eq!(
        evm.transact(&recovered(tx)).unwrap_err(),
        HandlerError::InvalidFrameTransaction(FrameTransactionError::ProofFrameFailed)
    );
}

#[test]
fn native_prefix_precheck_preserves_full_budget_charging_and_execution_results() {
    let vector: serde_json::Value =
        serde_json::from_str(include_str!("data/leansphincs-proof.json")).unwrap();
    let mut input = hex::decode(vector["public_key"].as_str().unwrap()).unwrap();
    input.extend(hex::decode(vector["message"].as_str().unwrap()).unwrap());
    input.extend(hex::decode(vector["signature"].as_str().unwrap()).unwrap());
    let claim = Frame {
        mode: FrameMode::Proof,
        target: LEANSPHINCS_VERIFIER.into(),
        data: input.into(),
        limits: FrameLimits { execution: 20000, state: 0 },
        ..Default::default()
    };
    let db = common::database_with_nonce_manager([(SENDER, APPROVE_BOTH)]);
    let mut evm = common::evm(db);
    let tx = recovered(payload(vec![claim, frame(FrameMode::Verify, SENDER)]));
    let prefix = evm.validate_frame_prefix(&tx, 2).unwrap();
    let result = evm.transact(&tx).unwrap().discard();
    let receipts = result.frame.unwrap().receipts;
    assert_eq!(receipts[0].gas_used.execution, 20000);
    assert_eq!(
        prefix.execution_gas,
        receipts.iter().map(|receipt| receipt.gas_used.execution).sum::<u64>()
    );
}
