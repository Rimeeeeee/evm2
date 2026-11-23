//! EIP-8288 dependency frames consume declared execution gas without an EVM call.

mod common;

use alloy_eips::{
    eip8141::{Frame, FrameLimits, FrameMode, FrameStatus},
    eip8288::{
        DependencyValidationError, LEANSPHINCS_SCHEME, LEANSPHINCS_VERIFICATION_GAS,
        SignatureDependency,
    },
};
use alloy_primitives::{B256, Bytes, U256};
use common::{APPROVE_BOTH, BODY, SENDER, frame, payload, recovered, set_version};
use evm2::{
    EvmFeatures, SpecId, Version,
    ethereum::eip8141::FrameGas,
    registry::{FrameTransactionError, HandlerError},
};

fn dependency_frame(count: usize) -> Frame {
    Frame {
        mode: FrameMode::DependencyVerify,
        limits: FrameLimits { execution: count as u64 * LEANSPHINCS_VERIFICATION_GAS, state: 0 },
        data: SignatureDependency {
            scheme: LEANSPHINCS_SCHEME,
            data_hash: B256::ZERO,
            verification_key_hash: B256::repeat_byte(1),
        }
        .encode()
        .repeat(count)
        .into(),
        ..Default::default()
    }
}

#[test]
fn dependencies_require_explicit_activation() {
    let mut evm = common::evm(common::database_with_nonce_manager([(SENDER, APPROVE_BOTH)]));
    let tx = recovered(payload(vec![dependency_frame(1), frame(FrameMode::Verify, SENDER)]));
    assert_eq!(
        evm.validate_tx(&tx),
        Err(HandlerError::InvalidFrameTransaction(FrameTransactionError::DependenciesNotActive))
    );
}

#[test]
fn dependency_receipt_and_introspection_survive_later_revert() {
    // Return FRAMEPARAM(mode, 0) and FRAMEDATACOPY(0)'s scheme word from the sender body.
    let body = [
        0x60, 2, 0x5f, 0xb3, 0x5f, 0x52, 0x5f, 0x60, 32, 0x5f, 0x60, 32, 0xb2, 0x60, 64, 0x5f, 0xf3,
    ];
    for body in [body.as_slice(), &[0x5f, 0x5f, 0xfd]] {
        let mut evm = common::evm(common::database_with_nonce_manager([
            (SENDER, APPROVE_BOTH),
            (BODY, body),
        ]));
        let mut version = Version::new(SpecId::BOGOTA);
        version.features.insert(EvmFeatures::EIP8288);
        set_version(&mut evm, version);
        let tx = payload(vec![
            dependency_frame(2),
            frame(FrameMode::Verify, SENDER),
            frame(FrameMode::Sender, BODY),
        ]);
        let gas = FrameGas::calculate(&tx, evm.version()).unwrap();
        assert_eq!(
            gas.execution,
            tx.calculate_gas_limit().max(tx.calculate_calldata_floor())
                - tx.total_frame_state_gas_limit()
        );
        let result = evm.transact(&recovered(tx)).unwrap().commit();
        let receipts = &result.frame.as_ref().unwrap().receipts;
        assert_eq!(receipts.len(), 3);
        assert_eq!(receipts[0].status, FrameStatus::Success);
        assert_eq!(receipts[0].gas_used.execution, 6_000);
        assert_eq!(receipts[0].gas_used.state, 0);
        assert!(receipts[0].logs.is_empty());
        if body.last() == Some(&0xf3) {
            let mut expected = U256::from(4).to_be_bytes::<32>().to_vec();
            expected.extend_from_slice(&U256::from(LEANSPHINCS_SCHEME).to_be_bytes::<32>());
            assert_eq!(result.frame.as_ref().unwrap().outputs[2], Bytes::from(expected));
        } else {
            assert_eq!(receipts[2].status, FrameStatus::Failure);
        }
    }
}

#[test]
fn invalid_dependency_target_is_rejected() {
    let mut evm = common::evm(common::database_with_nonce_manager([(SENDER, APPROVE_BOTH)]));
    let mut version = Version::new(SpecId::BOGOTA);
    version.features.insert(EvmFeatures::EIP8288);
    set_version(&mut evm, version);
    let mut dependency = dependency_frame(1);
    dependency.target = SENDER.into();
    let tx = recovered(payload(vec![dependency, frame(FrameMode::Verify, SENDER)]));
    assert_eq!(
        evm.validate_tx(&tx),
        Err(HandlerError::InvalidFrameTransaction(FrameTransactionError::InvalidDependency(
            DependencyValidationError::FrameFields
        )))
    );
}
