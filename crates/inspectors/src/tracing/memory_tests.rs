//! Repeated tracing releases transaction owners and retains only reusable step allocations.

use super::*;
use alloc::{
    sync::{Arc, Weak},
    vec,
};
use alloy_eips::eip8141::FrameLimits;
use core::ops::Deref;
use evm2::{
    BaseEvmTypes, Precompiles,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
};

const SENDER: Address = Address::with_last_byte(0xa0);
const BODY: Address = Address::with_last_byte(0xb0);

fn transaction(data: Bytes) -> Recovered<TxEnvelope> {
    Recovered::new_unchecked(
        TxEnvelope::Eip8141(TxEip8141 {
            chain_id: 1,
            sender: SENDER,
            nonce_keys: Some(vec![]),
            frames: [
                (FrameMode::Verify, SENDER, 3, Bytes::new()),
                (FrameMode::Sender, BODY, 0, data),
            ]
            .into_iter()
            .map(|(mode, target, flags, data)| Frame {
                mode,
                target: target.into(),
                flags,
                data,
                limits: FrameLimits { execution: 100_000, state: 300_000 },
                ..Default::default()
            })
            .collect(),
            ..Default::default()
        }),
        Address::ZERO,
    )
}

fn evm() -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    for (address, code) in [(SENDER, &[0x60, 3, 0x5f, 0x5f, 0xaa][..]), (BODY, &[0x00][..])] {
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
fn repeated_frame_and_prefix_resets_bound_the_step_buffer_pool() {
    let mut evm = evm();
    let tx = transaction(Bytes::new());
    let mut inspector = TracingInspector::new(TracingInspectorConfig::all());
    for iteration in 0..128 {
        evm.set_inspector(inspector);
        if iteration % 2 == 0 {
            evm.validate_frame_prefix(&tx, 1).unwrap();
        } else {
            assert!(evm.call_tx(&tx).unwrap().status);
        }
        inspector = *evm.clear_inspector_as::<TracingInspector>().unwrap();
        inspector.fuse();
        assert!(inspector.frame_transaction.is_none());
        assert!(inspector.trace_stack.is_empty());
        assert_eq!(inspector.traces.nodes().len(), 1);
        assert!(inspector.reusable_step_vecs.len() <= 2, "iteration {iteration}");
        assert!(inspector.reusable_step_vecs.iter().all(|steps| steps.capacity() != 0));
    }
    let pool_len = inspector.reusable_step_vecs.len();
    for _ in 0..128 {
        inspector.fuse();
        assert_eq!(inspector.reusable_step_vecs.len(), pool_len);
    }
}

#[derive(Debug)]
struct OwnedPayload {
    bytes: Vec<u8>,
    _alive: Arc<()>,
}

impl AsRef<[u8]> for OwnedPayload {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn tracked_data() -> (Bytes, Weak<()>) {
    type InnerBytes = <Bytes as Deref>::Target;
    let alive = Arc::new(());
    let weak = Arc::downgrade(&alive);
    let data =
        Bytes(InnerBytes::from_owner(OwnedPayload { bytes: vec![42; 16_384], _alive: alive }));
    (data, weak)
}

#[test]
fn completed_native_traces_release_unrecorded_frame_payloads_before_reset() {
    for prefix in [false, true] {
        let (data, weak) = tracked_data();
        let tx = transaction(data);
        let mut evm = evm();
        evm.set_inspector(TracingInspector::new(TracingInspectorConfig::none()));
        if prefix {
            evm.validate_frame_prefix(&tx, 1).unwrap();
        } else {
            assert!(evm.call_tx(&tx).unwrap().status);
        }
        let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
        drop(tx);
        assert!(inspector.frame_transaction.is_none());
        assert!(inspector.traces().nodes().iter().all(|node| node.trace.data.is_empty()));
        assert!(weak.upgrade().is_none(), "prefix={prefix}");
    }
}

#[test]
fn failed_native_frame_validation_releases_payload_on_reset() {
    for prefix in [false, true] {
        let (data, weak) = tracked_data();
        let tx = transaction(data);
        let mut evm = evm();
        evm.database_as_mut::<InMemoryDB>().unwrap().insert_account_info(
            &SENDER,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x5f, 0x5f, 0xfd]))),
        );
        evm.set_inspector(TracingInspector::new(TracingInspectorConfig::none()));
        if prefix {
            assert!(evm.validate_frame_prefix(&tx, 1).is_err());
        } else {
            assert!(evm.call_tx(&tx).is_err());
        }
        let mut inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
        drop(tx);
        assert!(inspector.frame_transaction.is_some());
        assert!(weak.upgrade().is_some());
        inspector.fuse();
        assert!(weak.upgrade().is_none(), "prefix={prefix}");
    }
}

#[cfg(feature = "js-tracer")]
mod js {
    use super::*;
    use crate::tracing::js::JsInspector;

    #[test]
    fn fused_js_tracers_release_frame_payload_and_callback_owners() {
        let mut evm = evm();
        let mut inspector = JsInspector::new(
            r#"{
            step: function(log) { this.log = log; },
            enter: function(frame) { this.frame = frame; },
            exit: function(result) { this.exitResult = result; },
            fault: function() {}, result: function() { return 1; }
        }"#
            .into(),
            serde_json::Value::Null,
        )
        .unwrap();
        for iteration in 0..32 {
            let (data, weak) = tracked_data();
            let tx = transaction(data);
            evm.set_inspector(inspector);
            if iteration % 2 == 0 {
                evm.validate_frame_prefix(&tx, 1).unwrap();
            } else {
                let result = evm.transact(&tx).unwrap().detach();
                inspector = *evm.clear_inspector_as::<JsInspector>().unwrap();
                let block = *evm.block();
                assert_eq!(
                    inspector.json_result(&result, &tx, &block, evm.database_mut()).unwrap(),
                    serde_json::json!(1)
                );
                evm.set_inspector(inspector);
            }
            inspector = *evm.clear_inspector_as::<JsInspector>().unwrap();
            drop(tx);
            // Result collection still needs the original envelope until the inspector is reset.
            assert!(weak.upgrade().is_some());
            inspector.fuse().unwrap();
            boa_gc::force_collect();
            assert!(weak.upgrade().is_none(), "iteration {iteration}");
        }
    }
}
