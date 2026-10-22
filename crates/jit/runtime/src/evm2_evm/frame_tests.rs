//! Frame transaction execution must use compiled code with the same gas and rollback semantics.

use super::*;
use crate::runtime::{ArtifactStore, RuntimeArtifactStore, RuntimeConfig, RuntimeTuning};
use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{Frame, FrameLimits, FrameMode, FrameSignature, SignatureScheme};
use alloy_primitives::{Address, Bytes, U256};
use evm2::{
    Precompiles, SpecId, TxResultWithState,
    bytecode::Bytecode,
    env::BlockEnvExt,
    ethereum::{TxEnvelope, ethereum_tx_registry},
    evm::{AccountInfo, InMemoryDB},
};
use std::sync::Arc;

fn run(backend: Option<&JitBackend>, state: u64, revert: bool) -> TxResultWithState {
    let sender = Address::with_last_byte(0xa0);
    let body = Address::with_last_byte(0xb0);
    let assertion = Address::with_last_byte(0xc0);
    let validation = [0x60, 3, 0x5f, 0x5f, 0xaa];
    let body_code = [
        0x5f, 0xb0, 0x50, // TXPARAM type
        0x60, 1, 0x60, 1, 0xb3, 0x50, // FRAMEPARAM execution budget
        0x60, 1, 0x5f, 0xb4, 0x50, // SIGPARAM scheme
        0x60, 1, 0x5f, 0xb1, 0x50, // FRAMEDATALOAD
        0x60, 1, 0x60, 32, 0x5f, 0x5f, 0xb2, // FRAMEDATACOPY
        0x5f, 0x60, 4, 0x5f, 0x60, 32, 0xb5, // SIGDATACOPY
        0x5f, 0x35, 0x5f, 0x55, // Ordinary SSTORE, with isolated state gas
        0x60, 36, 0x5f, 0xa0, 0x00, // Consensus event includes both copies
    ];
    let mut assertion_code = vec![
        0x5f, 0x60, 12, 0xb6, 0x50, // TXTRACE event count
        0x5f, 0x60, 0xb0, 0x60, 6, 0xb7, 0x50, // TXDIFF address slot count
        0x60, 36, 0x5f, 0x5f, 0x5f, 0xb8, // EVENTDATACOPY
    ];
    assertion_code.extend_from_slice(if revert {
        &[0x5f, 0x5f, 0xfd]
    } else {
        &[0x60, 36, 0x5f, 0xf3]
    });
    let mut database = InMemoryDB::default();
    for (address, code) in [
        (sender, validation.as_slice()),
        (body, body_code.as_slice()),
        (assertion, assertion_code.as_slice()),
    ] {
        database.insert_account_info(
            &address,
            AccountInfo::default()
                .with_balance(U256::from(1_000_000_000))
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
        );
    }
    let frames = [
        (FrameMode::Verify, sender, 3),
        (FrameMode::Sender, body, 0),
        (FrameMode::PostTx, assertion, 0),
    ]
    .into_iter()
    .map(|(mode, target, flags)| Frame {
        mode,
        target: target.into(),
        flags,
        limits: FrameLimits { execution: 100_000, state },
        data: U256::from(42).to_be_bytes::<32>().to_vec().into(),
        ..Default::default()
    })
    .collect();
    let tx = TxEip8141 {
        sender,
        chain_id: 1,
        frames,
        nonce_keys: Some(vec![U256::ZERO]),
        signatures: vec![FrameSignature {
            scheme: SignatureScheme::Arbitrary,
            signature: Bytes::from_static(b"abcd"),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut evm = Evm::<BaseEvmTypes>::new(
        SpecId::BOGOTA,
        BlockEnvExt::default(),
        ethereum_tx_registry(SpecId::BOGOTA),
        database,
        Precompiles::base(SpecId::BOGOTA),
    );
    if let Some(backend) = backend {
        evm.set_interpreter_runner(JitInterpreterRunner::new(backend.clone()));
    }
    evm.transact(&Recovered::new_unchecked(TxEnvelope::Eip8141(tx), Address::ZERO))
        .unwrap()
        .detach()
}

#[test]
fn native_frames_match_interpreter_with_jit_aot_and_reloaded_artifacts() {
    for aot in [false, true] {
        let store = Arc::new(RuntimeArtifactStore::new().unwrap()) as Arc<dyn ArtifactStore>;
        let config = RuntimeConfig {
            enabled: true,
            blocking: true,
            aot,
            single_error: false,
            store: aot.then_some(store),
            tuning: RuntimeTuning {
                jit_hot_threshold: 0,
                jit_worker_count: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut backend = JitBackend::new(config.clone()).unwrap();
        for reload in [false, true] {
            if reload && aot {
                backend = JitBackend::new(config.clone()).unwrap();
            }
            for state in [0, 300_000] {
                for revert in [false, true] {
                    let interpreted = run(None, state, revert);
                    let before = backend.stats().lookup_hits;
                    let compiled = run(Some(&backend), state, revert);
                    assert_eq!(
                        compiled.result, interpreted.result,
                        "aot={aot}, reload={reload}, state={state}, revert={revert}"
                    );
                    assert_eq!(compiled.pending_state, interpreted.pending_state);
                    assert!(
                        backend.stats().lookup_hits >= before + 3,
                        "every root must actually execute compiled code"
                    );
                    assert_eq!(backend.stats().compilations_failed, 0);
                }
            }
        }
    }
}
