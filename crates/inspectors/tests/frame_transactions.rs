//! Final frame trace reconciliation through native transaction execution.

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{
    ENTRY_POINT, Frame, FrameLimits, FrameMode, FrameSignature, FrameStatus, SignatureScheme,
};
use alloy_primitives::{Address, Bytes, U256};
use alloy_rpc_types_trace::geth::{
    CallConfig, GethDebugBuiltInTracerType, GethDebugTracerType, GethDefaultTracingOptions,
    GethTrace, erc7562::Erc7562Config,
};
use evm2::{
    BaseEvmTypes, Evm, Precompiles, SpecId,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
    interpreter::Host,
};
use evm2_inspectors::tracing::{DebugInspector, TracingInspector, TracingInspectorConfig};
use k256::ecdsa::SigningKey;

const APPROVE_BOTH: &[u8] = &[0x60, 3, 0x5f, 0x5f, 0xaa];
const STORE_AND_LOG: &[u8] = &[0x60, 1, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x00];
const REVERT: &[u8] = &[0x5f, 0x5f, 0xfd];

#[test]
fn native_finalization_reconciles_assertion_rollback_and_skipped_frames() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let assertion = Address::with_last_byte(0xc0);
    let mut evm = evm(&[(sender, APPROVE_BOTH), (body, STORE_AND_LOG), (assertion, REVERT)]);
    evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    let tx = Recovered::new_unchecked(
        TxEnvelope::Eip8141(payload(
            sender,
            vec![
                frame(FrameMode::Verify, sender, 3),
                frame(FrameMode::Sender, body, 0),
                frame(FrameMode::Assert, assertion, 0),
                frame(FrameMode::Assert, assertion, 0),
            ],
        )),
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

    let geth = inspector.geth_builder().geth_call_traces_with_result_gas(
        CallConfig { with_log: Some(true), ..Default::default() },
        &result,
    );
    assert_eq!(geth.calls.len(), 4);
    assert_eq!(geth.calls[3].gas_used, U256::ZERO);
    assert_eq!(geth.calls[3].error.as_deref(), Some("frame skipped"));
    assert!(geth.calls[1].logs.is_empty());
    let steps = inspector.geth_builder().geth_traces_with_result_gas(
        &result,
        Bytes::new(),
        GethDefaultTracingOptions::default(),
    );
    assert!(!steps.struct_logs.is_empty());
    assert!(steps.struct_logs.iter().all(|step| step.depth == 1));

    let vm = inspector.into_parity_builder().vm_trace();
    assert_eq!(vm.ops.len(), 3);
    assert_eq!(vm.ops.iter().map(|op| op.pc).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert!(vm.ops.iter().all(|op| !op.sub.as_ref().unwrap().ops.is_empty()));
}

fn frame(mode: FrameMode, target: Address, flags: u8) -> Frame {
    Frame {
        mode,
        target: target.into(),
        flags,
        limits: FrameLimits { execution: 100_000, state: 300_000 },
        ..Default::default()
    }
}

fn transaction(frames: Vec<Frame>) -> Recovered<TxEnvelope> {
    Recovered::new_unchecked(
        TxEnvelope::Eip8141(payload(Address::with_last_byte(0xa0), frames)),
        Address::with_last_byte(0xff),
    )
}

fn payload(sender: Address, frames: Vec<Frame>) -> TxEip8141 {
    TxEip8141 { chain_id: 1, sender, nonce_keys: Some(vec![]), frames, ..Default::default() }
}

fn evm(codes: &[(Address, &[u8])]) -> Evm<'static, BaseEvmTypes> {
    let mut db = InMemoryDB::default();
    for (address, code) in codes {
        db.insert_account_info(
            address,
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

fn default_verification_transaction(data: Bytes) -> (Address, Recovered<TxEnvelope>) {
    let key = SigningKey::from_slice(&[0x11; 32]).unwrap();
    let sender =
        Address::from_raw_public_key(&key.verifying_key().to_sec1_point(false).as_bytes()[1..]);
    let mut tx = payload(sender, vec![Frame { data, ..frame(FrameMode::Verify, sender, 3) }]);
    tx.signatures.push(FrameSignature {
        scheme: SignatureScheme::Secp256k1,
        signature: vec![0; 65].into(),
        ..Default::default()
    });
    let (sig, recid) = key.sign_prehash_recoverable(tx.signature_hash().as_slice());
    let mut signature = vec![recid.to_byte()];
    signature.extend_from_slice(sig.to_bytes().as_slice());
    tx.signatures[0].signature = signature.into();
    (sender, Recovered::new_unchecked(TxEnvelope::Eip8141(tx), Address::ZERO))
}

#[test]
fn native_default_account_verification_is_visible_in_debug_and_mux_results() {
    for mux in [false, true] {
        let (sender, tx) = default_verification_transaction(Bytes::new());
        let mut evm = evm(&[(sender, &[])]);
        let opts = if mux {
            serde_json::json!({ "tracer": "muxTracer", "tracerConfig": { "callTracer": { "onlyTopCall": true } } })
        } else {
            serde_json::json!({ "tracer": "callTracer", "tracerConfig": { "onlyTopCall": true } })
        };
        evm.set_inspector(DebugInspector::new(serde_json::from_value(opts).unwrap()).unwrap());
        let result = evm.transact(&tx).unwrap().detach();
        let mut inspector = evm.clear_inspector_as::<DebugInspector>().unwrap();
        let block = *evm.block_env();
        let trace = inspector.get_result(None, &tx, &block, &result, evm.database_mut()).unwrap();
        let calls = match trace {
            GethTrace::CallTracer(calls) => calls,
            GethTrace::MuxTracer(mut result) => {
                let GethTrace::CallTracer(calls) = result
                    .0
                    .remove(&GethDebugTracerType::BuiltInTracer(
                        GethDebugBuiltInTracerType::CallTracer,
                    ))
                    .unwrap()
                else {
                    panic!("mux call tracer")
                };
                calls
            }
            _ => panic!("call tracer result"),
        };
        assert_eq!(calls.from, sender);
        assert_eq!(calls.calls.len(), 1);
        assert_eq!(calls.calls[0].to, Some(sender));
        assert_eq!(
            calls.calls[0].gas_used,
            U256::from(result.result.frame.as_ref().unwrap().receipts[0].gas_used.execution)
        );
        assert!(calls.calls[0].error.is_none());
    }
}

#[test]
fn native_atomic_failure_keeps_receipt_gas_and_skips_only_its_group() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let revert = Address::with_last_byte(0xc0);
    let mut evm = evm(&[(sender, APPROVE_BOTH), (body, STORE_AND_LOG), (revert, REVERT)]);
    evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    let result = evm
        .transact(&transaction(vec![
            frame(FrameMode::Verify, sender, 3),
            frame(FrameMode::Sender, body, 4),
            frame(FrameMode::Sender, revert, 4),
            frame(FrameMode::Sender, body, 0),
            frame(FrameMode::Sender, body, 0),
        ]))
        .unwrap()
        .discard();
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    let nodes = inspector.traces().nodes();
    assert_eq!(nodes[0].children.len(), 5);
    let outcome = result.frame.as_ref().unwrap();
    for (index, child) in nodes[0].children.iter().enumerate() {
        let trace = &nodes[*child].trace;
        assert_eq!(trace.gas_used, outcome.receipts[index].gas_used.execution);
        assert_eq!(trace.state_gas_used, outcome.receipts[index].gas_used.state);
        assert_eq!(trace.rolled_back, outcome.rolled_back[index]);
    }
    assert!(result.status);
    assert!(!nodes[0].trace.success);
    assert!(nodes[0].trace.is_error());
    assert!(nodes[nodes[0].children[1]].logs.is_empty());
    assert!(nodes[nodes[0].children[3]].trace.skipped);
    assert_eq!(nodes[nodes[0].children[4]].logs.len(), 1);
    let calls = inspector.geth_builder().geth_call_traces_with_result_gas(
        CallConfig { with_log: Some(true), ..Default::default() },
        &result,
    );
    assert_eq!(calls.error.as_deref(), Some("frame transaction failed"));
    assert_eq!(calls.gas_used, U256::from(result.tx_gas_used()));
    assert_eq!(calls.execution_gas_used, Some(U256::from(result.execution_gas_spent())));
    assert_eq!(calls.state_gas_used, Some(U256::from(result.state_gas_spent())));
    assert_eq!(calls.calls[3].gas_used, U256::ZERO);
    assert_eq!(calls.calls[4].logs.len(), 1);
    assert_eq!(calls.calls[4].logs[0].index, Some(0));
    let final_frame_gas = &outcome.receipts[4].gas_used;
    assert!(final_frame_gas.state > 0);
    assert_eq!(calls.calls[4].gas, U256::from(100_000));
    assert_eq!(calls.calls[4].gas_used, U256::from(final_frame_gas.execution));
    assert_eq!(calls.calls[4].execution_gas_used, Some(U256::from(final_frame_gas.execution)));
    assert_eq!(calls.calls[4].state_gas_used, Some(U256::from(final_frame_gas.state)));
    let parity = inspector.into_parity_builder();
    let traces = parity.clone().into_transaction_traces();
    assert_eq!(
        traces.iter().map(|trace| trace.trace_address.clone()).collect::<Vec<_>>(),
        vec![vec![], vec![0], vec![1], vec![2], vec![3], vec![4]]
    );
    assert_eq!(traces[0].error.as_deref(), Some("frame transaction failed"));
    assert_eq!(traces[0].result.as_ref().unwrap().gas_used(), result.tx_gas_used());
    assert_eq!(traces[4].error.as_deref(), Some("frame skipped"));
    assert!(traces[4].result.is_none());
    assert_eq!(traces[5].action.as_call().unwrap().gas, 100_000);
    assert_eq!(traces[5].result.as_ref().unwrap().gas_used(), final_frame_gas.execution);
    let vm = parity.vm_trace();
    assert!(vm.code.is_empty());
    assert_eq!(vm.ops.iter().map(|op| op.pc).collect::<Vec<_>>(), vec![0, 1, 2, 4]);
}

#[test]
fn native_debug_context_preserves_payload_sender_and_all_frame_entry_outcomes() {
    let sender = Address::with_last_byte(0xa0);
    let identity = Address::with_last_byte(4);
    let mut evm = evm(&[(sender, APPROVE_BOTH)]);
    let mut precompile = frame(FrameMode::Sender, identity, 0);
    precompile.data = Bytes::from_static(&[1, 2, 3]);
    let mut unaffordable = frame(FrameMode::Sender, Address::with_last_byte(0xb0), 0);
    unaffordable.value = U256::from(2_000_000_000u64);
    let tx = transaction(vec![frame(FrameMode::Verify, sender, 3), precompile, unaffordable]);
    let config = CallConfig { only_top_call: Some(true), with_log: Some(true) };
    evm.set_inspector(DebugInspector::CallTracer(
        TracingInspector::new(TracingInspectorConfig::all().set_exclude_precompile_calls(true)),
        config,
    ));
    let result = evm.transact(&tx).unwrap().detach();
    let outcome = result.result.frame.as_ref().unwrap();
    let mut inspector = evm.clear_inspector_as::<DebugInspector>().unwrap();
    let block = *evm.block_env();
    let GethTrace::CallTracer(calls) =
        inspector.get_result(None, &tx, &block, &result, evm.database_mut()).unwrap()
    else {
        panic!("call tracer result")
    };
    assert_eq!(calls.from, sender);
    assert_eq!(calls.to, Some(ENTRY_POINT));
    assert_eq!(calls.calls.len(), 3);
    assert_eq!(calls.calls[1].to, Some(identity));
    assert_eq!(calls.calls[1].output, Some(Bytes::from_static(&[1, 2, 3])));
    assert_eq!(calls.calls[2].gas_used, U256::from(outcome.receipts[2].gas_used.execution));
    assert!(calls.calls[2].gas_used < U256::from(100_000));
    assert_eq!(outcome.receipts[2].status, FrameStatus::Failure);
}

#[test]
fn prefix_inspection_finalizes_only_executed_frames_and_can_be_fused() {
    let sender = Address::with_last_byte(0xa0);
    let mut evm = evm(&[(sender, APPROVE_BOTH)]);
    evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    let tx =
        transaction(vec![frame(FrameMode::Verify, sender, 3), frame(FrameMode::Sender, sender, 0)]);
    evm.validate_frame_prefix(&tx, 1).unwrap();
    let mut inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    assert_eq!(inspector.traces().nodes()[0].children.len(), 1);
    assert!(inspector.traces().nodes()[0].trace.success);
    inspector.fuse();
    evm.set_inspector(*inspector);
    let _ = evm.transact(&tx).unwrap().discard();
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    assert_eq!(inspector.traces().nodes()[0].children.len(), 2);
}

#[test]
fn synthetic_roots_keep_nested_steps_and_global_structlog_limits() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let child = Address::with_last_byte(0xd0);
    let mut evm = evm(&[
        (sender, APPROVE_BOTH),
        (body, &[0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x60, 0xd0, 0x61, 0xff, 0xff, 0xf1, 0x00]),
        (child, &[0x60, 42, 0x50, 0x00]),
    ]);
    evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
    let result = evm
        .transact(&transaction(vec![
            frame(FrameMode::Verify, sender, 3),
            frame(FrameMode::Sender, body, 0),
        ]))
        .unwrap()
        .discard();
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    let full = inspector.geth_builder().geth_traces(
        result.tx_gas_used(),
        Bytes::new(),
        GethDefaultTracingOptions::default(),
    );
    let expected =
        inspector.traces().nodes().iter().map(|node| node.trace.steps.len()).sum::<usize>();
    assert_eq!(full.struct_logs.len(), expected);
    assert_eq!(full.struct_logs.iter().map(|step| step.depth).max(), Some(2));
    assert_eq!(full.struct_logs.iter().map(|step| step.depth).min(), Some(1));
    let limited = inspector.geth_builder().geth_traces(
        result.tx_gas_used(),
        Bytes::new(),
        GethDefaultTracingOptions { limit: Some(6), ..Default::default() },
    );
    assert_eq!(limited.struct_logs, full.struct_logs[..6]);
    let vm = inspector.into_parity_builder().vm_trace();
    assert_eq!(vm.ops.len(), 2);
    let body = vm.ops[1].sub.as_ref().unwrap();
    assert_eq!(body.ops.iter().filter(|op| op.sub.is_some()).count(), 1);
    assert_eq!(body.ops.iter().find_map(|op| op.sub.as_ref()).unwrap().ops.len(), 3);
}

#[test]
fn assertion_failure_preserves_payment_prefix_logs_and_the_block_log_offset() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let assertion = Address::with_last_byte(0xc0);
    let mut evm = evm(&[
        (sender, &[0x5f, 0x5f, 0xa0, 0x60, 3, 0x5f, 0x5f, 0xaa]),
        (body, &[0x5f, 0x5f, 0xa0, 0x00]),
        (assertion, REVERT),
    ]);
    let mut inspector = TracingInspector::new(TracingInspectorConfig::all());
    inspector.set_next_log_index(7);
    evm.set_inspector(inspector);
    let result = evm
        .transact(&transaction(vec![
            frame(FrameMode::Default, sender, 3),
            frame(FrameMode::Sender, body, 0),
            frame(FrameMode::Assert, assertion, 0),
        ]))
        .unwrap()
        .discard();
    assert!(!result.status);
    let outcome = result.frame.as_ref().unwrap();
    assert_eq!(outcome.receipts[0].logs.len(), 1);
    assert!(outcome.receipts[1].logs.is_empty());
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    let calls = inspector.geth_builder().geth_call_traces(
        CallConfig { with_log: Some(true), ..Default::default() },
        result.tx_gas_used(),
    );
    assert_eq!(calls.calls[0].logs.len(), 1);
    assert_eq!(calls.calls[0].logs[0].index, Some(7));
    assert!(calls.calls[1].logs.is_empty());
    let erc7562 = inspector
        .geth_builder()
        .geth_erc7562_traces(
            Erc7562Config { with_log: Some(true), ..Default::default() },
            result.tx_gas_used(),
            evm.database_mut(),
        )
        .unwrap();
    assert_eq!(erc7562.calls[0].logs.len(), 1);
    assert_eq!(erc7562.calls[0].logs[0].index, Some(7));
    assert!(erc7562.calls[1].logs.is_empty());
}

#[cfg(feature = "js-tracer")]
mod js {
    use super::*;
    use evm2::ethereum::eip8141::FrameGas;
    use evm2_inspectors::tracing::js::JsInspector;
    use serde_json::json;

    const TRACER: &str = r#"{
        events: [],
        enter: function(frame) { this.events.push(['enter', frame.getFrameIndex(), frame.getType()]); },
        exit: function(frame) { this.events.push(['exit', frame.getFrameIndex(), frame.getGasUsed(), toHex(frame.getOutput()), frame.getError() || null]); },
        fault: function() {},
        result: function(ctx) {
            return {
                type: ctx.type, from: toHex(ctx.from), to: toHex(ctx.to),
                gas: ctx.gas, intrinsic: ctx.intrinsicGas, events: this.events,
                frames: ctx.frames.map(function(frame) {
                    return [frame.index, frame.status, frame.gasUsed, frame.stateGasUsed,
                            toHex(frame.output), frame.rolledBack, frame.skipped, frame.logs.length];
                })
            };
        }
    }"#;

    const STORE_LOG_RETURN: &[u8] =
        &[0x60, 1, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x60, 42, 0x5f, 0x53, 0x60, 1, 0x5f, 0xf3];

    #[test]
    fn javascript_gas_price_reports_the_transaction_lower_bound() {
        let sender = Address::with_last_byte(0xa0);
        let body = Address::with_last_byte(0xb0);
        let mut evm =
            evm(&[(sender, APPROVE_BOTH), (body, &[0x3a, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3])]);
        evm.set_block(BlockEnvExt { basefee: U256::from(3), ..Default::default() });
        let mut payload = payload(
            sender,
            vec![frame(FrameMode::Verify, sender, 3), frame(FrameMode::Sender, body, 0)],
        );
        let maximum = U256::from(FrameGas::calculate(&payload, evm.version()).unwrap().maximum);
        payload.fees.max_priority_fee_per_gas = U256::from(11);
        payload.fees.max_fee = maximum * U256::from(3) + maximum / U256::from(2);
        let tx = Recovered::new_unchecked(TxEnvelope::Eip8141(payload), Address::ZERO);
        evm.set_inspector(
            JsInspector::new(
                r#"{ fault: function() {}, result: function(ctx) { return ctx.gasPrice; } }"#
                    .into(),
                json!(null),
            )
            .unwrap(),
        );
        let result = evm.transact(&tx).unwrap().detach();
        let outcome = result.result.frame.as_ref().unwrap();
        assert!(result.result.status);
        assert_eq!(outcome.gas_price, U256::from(3));
        assert_eq!(U256::from_be_slice(&outcome.outputs[1]), U256::from(3));
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        let block = *evm.block_env();
        let actual = inspector.json_result(&result, &tx, &block, evm.database_mut()).unwrap();
        assert_eq!(actual, json!(3));
    }

    #[test]
    fn javascript_default_verification_without_interpreter_emits_one_enter_and_exit() {
        let (sender, tx) = default_verification_transaction(Bytes::new());
        let mut evm = evm(&[(sender, &[])]);
        evm.set_inspector(JsInspector::new(TRACER.into(), json!(null)).unwrap());
        let result = evm.transact(&tx).unwrap().detach();
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        let block = *evm.block_env();
        let actual = inspector.json_result(&result, &tx, &block, evm.database_mut()).unwrap();
        assert_eq!(actual["events"].as_array().unwrap().len(), 2);
        assert_eq!(actual["events"][0], json!(["enter", 0, "STATICCALL"]));
        assert_eq!(
            actual["events"][1][2],
            result.result.frame.as_ref().unwrap().receipts[0].gas_used.execution
        );
        assert_eq!(actual["frames"][0][1], 1);
    }

    #[test]
    fn javascript_debug_lifecycle_and_result_reconcile_assertion_rollback() {
        let sender = Address::with_last_byte(0xa0);
        let body = Address::with_last_byte(0xb0);
        let assertion = Address::with_last_byte(0xc0);
        let mut evm = evm(&[(sender, APPROVE_BOTH), (body, STORE_LOG_RETURN), (assertion, REVERT)]);
        let tx = transaction(vec![
            frame(FrameMode::Verify, sender, 3),
            frame(FrameMode::Sender, body, 0),
            frame(FrameMode::Assert, assertion, 0),
            frame(FrameMode::Assert, assertion, 0),
        ]);
        evm.set_inspector(DebugInspector::Js(Box::new(
            JsInspector::new(TRACER.into(), json!(null)).unwrap(),
        )));
        let result = evm.transact(&tx).unwrap().detach();
        let outcome = result.result.frame.as_ref().unwrap();
        let mut inspector = evm.clear_inspector_as::<DebugInspector>().unwrap();
        let block = *evm.block_env();
        let GethTrace::JS(actual) =
            inspector.get_result(None, &tx, &block, &result, evm.database_mut()).unwrap()
        else {
            panic!("JS result")
        };
        assert_eq!(actual["type"], "FRAME");
        assert_eq!(actual["from"], sender.to_string().to_lowercase());
        assert_eq!(actual["to"], ENTRY_POINT.to_string().to_lowercase());
        assert!(actual["gas"].as_u64().unwrap() > 1_200_000);
        assert!(actual["intrinsic"].as_u64().unwrap() > 0);
        let events = actual["events"].as_array().unwrap();
        assert_eq!(events.len(), 8);
        for index in 0..4 {
            assert_eq!(events[index * 2][0], "enter");
            assert_eq!(events[index * 2][1], index);
            assert_eq!(events[index * 2 + 1][0], "exit");
            assert_eq!(events[index * 2 + 1][1], index);
            assert_eq!(events[index * 2 + 1][2], outcome.receipts[index].gas_used.execution);
        }
        // Notifications describe the live frame; ctx.frames is the final canonical result.
        assert_eq!(events[3][3], "0x2a");
        assert_eq!(actual["frames"][1][4], "0x");
        assert_eq!(actual["frames"][1][3], 0);
        assert_eq!(actual["frames"][1][5], true);
        assert_eq!(actual["frames"][1][7], 0);
        assert_eq!(actual["frames"][3], json!([3, 2, 0, 0, "0x", false, true, 0]));
        assert_eq!(events[7][4], "frame skipped");
    }

    #[test]
    fn javascript_prefix_reset_and_precompile_and_entry_failure_have_balanced_hooks() {
        let sender = Address::with_last_byte(0xa0);
        let mut evm = evm(&[(sender, APPROVE_BOTH)]);
        let mut identity = frame(FrameMode::Sender, Address::with_last_byte(4), 0);
        identity.data = Bytes::from_static(&[42]);
        let mut unaffordable = frame(FrameMode::Sender, Address::with_last_byte(0xb0), 0);
        unaffordable.value = U256::from(2_000_000_000u64);
        let tx = transaction(vec![frame(FrameMode::Verify, sender, 3), identity, unaffordable]);
        evm.set_inspector(JsInspector::new(TRACER.into(), json!(null)).unwrap());
        evm.validate_frame_prefix(&tx, 1).unwrap();
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        inspector.fuse().unwrap();
        evm.set_inspector(*inspector);
        let result = evm.transact(&tx).unwrap().detach();
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        let block = *evm.block_env();
        let actual = inspector.json_result(&result, &tx, &block, evm.database_mut()).unwrap();
        assert_eq!(actual["events"].as_array().unwrap().len(), 6);
        assert_eq!(actual["frames"][1][4], "0x2a");
        assert_eq!(actual["frames"][2][1], 0);
        assert!(actual["frames"][2][2].as_u64().unwrap() < 100_000);
        assert_eq!(actual["events"][5][1], 2);
        assert_eq!(actual["events"][5][2], actual["frames"][2][2]);
        assert_eq!(actual["events"][5][4], "insufficient balance for transfer");
    }

    #[test]
    fn javascript_frame_notification_errors_surface_when_collecting_results() {
        let sender = Address::with_last_byte(0xa0);
        let mut evm = evm(&[(sender, APPROVE_BOTH)]);
        let tx = transaction(vec![frame(FrameMode::Verify, sender, 3)]);
        evm.set_inspector(
            JsInspector::new(
                r#"{
            fault: function() {}, exit: function() { throw new Error('frame notification'); },
            result: function() { return 'unreachable'; }
        }"#
                .into(),
                json!(null),
            )
            .unwrap(),
        );
        let result = evm.transact(&tx).unwrap().detach();
        assert!(result.result.status);
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        let block = *evm.block_env();
        assert_eq!(
            inspector
                .json_result(&result, &tx, &block, evm.database_mut())
                .unwrap_err()
                .to_string(),
            "Error: frame notification (unknown at :2:60)\n    at exit (unknown at :2:60)"
        );
    }

    #[test]
    fn javascript_atomic_rollback_preserves_output_and_reconciles_state_and_logs() {
        let sender = Address::with_last_byte(0xa0);
        let body = Address::with_last_byte(0xb0);
        let failure = Address::with_last_byte(0xc0);
        let mut evm = evm(&[(sender, APPROVE_BOTH), (body, STORE_LOG_RETURN), (failure, REVERT)]);
        let tx = transaction(vec![
            frame(FrameMode::Verify, sender, 3),
            frame(FrameMode::Sender, body, 4),
            frame(FrameMode::Sender, failure, 4),
            frame(FrameMode::Sender, body, 0),
            frame(FrameMode::Sender, body, 0),
        ]);
        evm.set_inspector(JsInspector::new(TRACER.into(), json!(null)).unwrap());
        let result = evm.transact(&tx).unwrap().detach();
        assert!(result.result.status);
        let mut inspector = evm.clear_inspector_as::<JsInspector>().unwrap();
        let block = *evm.block_env();
        let actual = inspector.json_result(&result, &tx, &block, evm.database_mut()).unwrap();
        assert_eq!(actual["events"].as_array().unwrap().len(), 10);
        assert_eq!(actual["frames"][1][4], "0x2a");
        assert_eq!(actual["frames"][1][3], 0);
        assert_eq!(actual["frames"][1][5], true);
        assert_eq!(actual["frames"][1][7], 0);
        assert_eq!(actual["frames"][3][6], true);
        assert_eq!(actual["frames"][4][4], "0x2a");
        assert!(actual["frames"][4][3].as_u64().unwrap() > 0);
        assert_eq!(actual["frames"][4][7], 1);
    }
}

#[test]
fn synthetic_and_executed_frames_respect_input_recording() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let assertion = Address::with_last_byte(0xc0);
    let input = Bytes::from_static(&[1, 2, 3]);
    for config in [
        TracingInspectorConfig::none(),
        TracingInspectorConfig::parity_vm_trace(),
        TracingInspectorConfig::default_parity(),
    ] {
        let mut evm = evm(&[(sender, APPROVE_BOTH), (body, &[0x00]), (assertion, REVERT)]);
        let mut frames = [
            (FrameMode::Verify, sender, 3),
            (FrameMode::Sender, body, 0),
            (FrameMode::Assert, assertion, 0),
            (FrameMode::Assert, assertion, 0),
        ]
        .into_iter()
        .map(|(mode, target, flags)| Frame { data: input.clone(), ..frame(mode, target, flags) })
        .collect::<Vec<_>>();
        frames[1].value = U256::from(2_000_000_000u64);
        evm.set_inspector(TracingInspector::new(config));
        let result = evm.call_tx(&transaction(frames)).unwrap();
        assert!(!result.status);
        let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
        let nodes = inspector.traces().nodes();
        assert_eq!(nodes[0].children.len(), 4);
        let expected = if config.record_inputs { input.clone() } else { Bytes::new() };
        for (index, entered_evm) in [true, false, true, false].into_iter().enumerate() {
            let trace = &nodes[nodes[0].children[index]].trace;
            assert_eq!(trace.entered_evm, entered_evm, "frame {index}");
            assert_eq!(trace.data, expected, "frame {index}, config={config:?}");
        }
        assert!(!nodes[nodes[0].children[1]].trace.success);
        assert!(nodes[nodes[0].children[3]].trace.skipped);
    }
}

#[test]
fn synthetic_default_verification_respects_input_recording() {
    let input = Bytes::from_static(&[1, 2, 3]);
    for config in [
        TracingInspectorConfig::none(),
        TracingInspectorConfig::parity_vm_trace(),
        TracingInspectorConfig::default_parity(),
    ] {
        let (sender, tx) = default_verification_transaction(input.clone());
        let mut evm = evm(&[(sender, &[])]);
        evm.set_inspector(TracingInspector::new(config));
        assert!(evm.call_tx(&tx).unwrap().status);
        let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
        let nodes = inspector.traces().nodes();
        assert_eq!(nodes[0].children.len(), 1);
        let trace = &nodes[nodes[0].children[0]].trace;
        assert!(!trace.entered_evm);
        assert_eq!(trace.data, if config.record_inputs { input.clone() } else { Bytes::new() });
    }
}

#[test]
fn many_frame_vm_traces_keep_nested_calls_and_excluded_precompiles_in_each_subtree() {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let child = Address::with_last_byte(0xd0);
    let mut code = Vec::new();
    for target in [child, Address::with_last_byte(4), child] {
        code.extend_from_slice(&[0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73]);
        code.extend_from_slice(target.as_slice());
        code.extend_from_slice(&[0x5a, 0xf1, 0x50]);
    }
    code.push(0x00);
    let mut evm = evm(&[(sender, APPROVE_BOTH), (body, &code), (child, &[0x60, 42, 0x50, 0x00])]);
    evm.set_inspector(TracingInspector::new(
        TracingInspectorConfig::parity_vm_trace().set_exclude_precompile_calls(true),
    ));
    let frames = core::iter::once(frame(FrameMode::Verify, sender, 3))
        .chain(core::iter::repeat_n(frame(FrameMode::Sender, body, 0), 63))
        .collect();
    assert!(evm.call_tx(&transaction(frames)).unwrap().status);
    let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
    let vm = inspector.into_parity_builder().vm_trace();
    assert_eq!(vm.ops.len(), 64);
    for (index, operation) in vm.ops.iter().enumerate() {
        assert_eq!(operation.pc, index);
        let trace = operation.sub.as_ref().unwrap();
        if index == 0 {
            continue;
        }
        assert_eq!(trace.code.as_ref(), code.as_slice());
        let children =
            trace.ops.iter().filter_map(|operation| operation.sub.as_ref()).collect::<Vec<_>>();
        assert_eq!(children.len(), 3, "frame {index}");
        assert_eq!(children[0].ops.len(), 3, "first child in frame {index}");
        assert!(children[1].code.is_empty());
        assert!(children[1].ops.is_empty(), "excluded precompile in frame {index}");
        assert_eq!(children[2].ops.len(), 3, "last child in frame {index}");
    }
}

#[test]
fn pre_evm_frame_failures_keep_receipt_gas_and_later_frame_traces() {
    let sender = Address::with_last_byte(0xa0);
    let failed = Address::with_last_byte(0xb0);
    let later = Address::with_last_byte(0xc0);
    for (execution, value, geth_error, parity_error) in [
        (1, U256::ZERO, "out of gas", "Out of gas"),
        (
            100_000,
            U256::from(2_000_000_000u64),
            "insufficient balance for transfer",
            "Insufficient balance for transfer",
        ),
    ] {
        let mut evm = evm(&[
            (sender, APPROVE_BOTH),
            (failed, &[0x00]),
            (later, &[0x60, 42, 0x5f, 0x53, 0x60, 1, 0x5f, 0xf3]),
        ]);
        evm.set_inspector(TracingInspector::new(TracingInspectorConfig::all()));
        let mut failed_frame = frame(FrameMode::Sender, failed, 0);
        failed_frame.limits.execution = execution;
        failed_frame.value = value;
        let result = evm
            .transact(&transaction(vec![
                frame(FrameMode::Verify, sender, 3),
                failed_frame,
                frame(FrameMode::Sender, later, 0),
            ]))
            .unwrap()
            .discard();
        let outcome = result.frame.as_ref().unwrap();
        assert!(result.status);
        assert_eq!(outcome.receipts[1].status, FrameStatus::Failure);
        assert_eq!(outcome.receipts[2].status, FrameStatus::Success);
        let inspector = evm.clear_inspector_as::<TracingInspector>().unwrap();
        let nodes = inspector.traces().nodes();
        assert!(!nodes[0].trace.success);
        assert!(nodes[0].trace.is_error());
        assert!(!nodes[nodes[0].children[1]].trace.entered_evm);
        let steps = inspector.geth_builder().geth_traces(
            result.tx_gas_used(),
            Bytes::new(),
            GethDefaultTracingOptions::default(),
        );
        assert!(steps.failed);
        for only_top_call in [false, true] {
            let calls = inspector.geth_builder().geth_call_traces(
                CallConfig { only_top_call: Some(only_top_call), ..Default::default() },
                result.tx_gas_used(),
            );
            assert_eq!(calls.from, sender);
            assert_eq!(calls.to, Some(ENTRY_POINT));
            assert_eq!(calls.error.as_deref(), Some("frame transaction failed"));
            assert_eq!(calls.gas_used, U256::from(result.tx_gas_used()));
            assert!(calls.output.is_none());
            assert_eq!(calls.calls.len(), 3);
            let call = &calls.calls[1];
            assert_eq!(call.to, Some(failed));
            assert_eq!(call.error.as_deref(), Some(geth_error));
            assert_eq!(call.gas, U256::from(execution));
            assert_eq!(call.gas_used, U256::from(outcome.receipts[1].gas_used.execution));
            assert!(call.calls.is_empty());
            assert!(call.output.is_none());
            assert_eq!(calls.calls[2].output, Some(Bytes::from_static(&[42])));
        }
        let parity = inspector.into_parity_builder();
        let traces = parity.clone().into_transaction_traces();
        assert_eq!(traces.len(), 4);
        assert_eq!(traces[0].error.as_deref(), Some("frame transaction failed"));
        assert_eq!(traces[0].result.as_ref().unwrap().gas_used(), result.tx_gas_used());
        assert_eq!(traces[2].trace_address, vec![1]);
        assert_eq!(traces[2].error.as_deref(), Some(parity_error));
        assert!(traces[2].result.is_none());
        assert_eq!(traces[2].subtraces, 0);
        assert_eq!(traces[3].trace_address, vec![2]);
        let vm = parity.vm_trace();
        assert!(vm.code.is_empty());
        assert_eq!(vm.ops.iter().map(|op| op.pc).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(
            vm.ops[1].sub.as_ref().unwrap().code.as_ref(),
            &[0x60, 42, 0x5f, 0x53, 0x60, 1, 0x5f, 0xf3]
        );
    }
}
