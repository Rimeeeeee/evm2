//! Final frame trace reconciliation through native transaction execution.

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{Frame, FrameLimits, FrameMode};
use alloy_primitives::{Address, Bytes, U256};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
};
use evm2_inspectors::tracing::{TracingInspector, TracingInspectorConfig};

#[test]
fn native_finalization_reconciles_assertion_rollback_and_skipped_frames() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let assertion = Address::with_last_byte(0xc0);
    let mut db = InMemoryDB::default();
    for (address, code) in [
        (sender, &[0x60, 3, 0x5f, 0x5f, 0xaa][..]),
        (body, &[0x60, 1, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x00][..]),
        (assertion, &[0x5f, 0x5f, 0xfd][..]),
    ] {
        db.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    let mut evm = Evm::<BaseEvmTypes>::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        db,
        Precompiles::base(SpecId::BOGOTA),
    );
    evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    let frames = [
        (FrameMode::Verify, sender, 3),
        (FrameMode::Sender, body, 0),
        (FrameMode::PostTx, assertion, 0),
        (FrameMode::PostTx, assertion, 0),
    ]
    .into_iter()
    .map(|(mode, target, flags)| Frame {
        mode,
        target: target.into(),
        flags,
        limits: FrameLimits { execution: 100_000, state: 300_000 },
        ..Default::default()
    })
    .collect();
    let tx = Recovered::new_unchecked(
        TxEnvelope::Eip8141(TxEip8141 {
            chain_id: 1,
            sender,
            nonce_keys: Some(vec![U256::ZERO]),
            frames,
            ..Default::default()
        }),
        Address::ZERO,
    );
    let result = evm.transact(&tx).unwrap().discard();
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    let nodes = inspector.traces().nodes();
    assert_eq!(nodes[0].children.len(), 4);
    assert_eq!(nodes[0].trace.gas_used, result.tx_gas_used());
    assert!(!nodes[0].trace.success);
    let body_node = &nodes[nodes[0].children[1]];
    assert_eq!(body_node.trace.frame_index, Some(1));
    assert_eq!(body_node.trace.depth, 1);
    assert!(body_node.trace.success);
    assert!(body_node.trace.rolled_back);
    assert_eq!(body_node.trace.state_gas_used, 0);
    assert!(body_node.logs.is_empty());
    let skipped = &nodes[nodes[0].children[3]];
    assert!(skipped.trace.skipped);
    assert_eq!(skipped.trace.gas_used, 0);
    assert_eq!(skipped.trace.state_gas_used, 0);
}
