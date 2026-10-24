//! Deterministic sequence coverage against a small independent rollback model.

mod common;

use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::eip8141::{Frame, FrameMode, FrameStatus, NONCE_MANAGER, nonce_slot};
use alloy_primitives::{Address, Bytes, U256};
use common::{APPROVE_BOTH, BODY, SENDER};
use evm2::{
    BaseEvmTypes, Evm, Inspector,
    ethereum::{TxEnvelope, eip8141::FrameGas},
    evm::{AccountInfo, PendingState},
    interpreter::{InstrStop, Interpreter, Message, MessageResult},
};
use std::sync::{Arc, Mutex};

const HALT: Address = Address::with_last_byte(0xb1);
const ASSERTION: Address = Address::with_last_byte(0xc0);

fn frame(mode: FrameMode, target: Address, flags: u8) -> Frame {
    Frame { flags, ..common::frame(mode, target) }
}

fn evm(assertion_fails: bool) -> Evm<'static, BaseEvmTypes> {
    // Store calldata[0] and emit a log, then revert when calldata[32] is nonzero.
    // Failed writes/logs must disappear, including when a surrounding atomic batch fails later.
    let body = [
        0x5f, 0x35, 0x5f, 0x55, 0x5f, 0x5f, 0xa0, 0x60, 0x20, 0x35, 0x15, 0x60, 0x11, 0x57, 0x5f,
        0x5f, 0xfd, 0x5b, 0x00,
    ];
    let mut db = common::database([
        (SENDER, APPROVE_BOTH),
        (BODY, &body),
        (HALT, &[0xfe]),
        (ASSERTION, if assertion_fails { &[0x5f, 0x5f, 0xfd][..] } else { &[0x00] }),
    ]);
    db.insert_account_info(&NONCE_MANAGER, AccountInfo::default().with_nonce(1));
    common::evm(db)
}

fn stored(state: &PendingState, address: Address, key: U256) -> U256 {
    state
        .account_changes()
        .find(|account| account.address == address)
        .and_then(|account| account.storage.slots.get(&key))
        .map(|slot| slot.value.current)
        .unwrap_or_default()
}

#[derive(Default)]
struct Hooks {
    calls: usize,
    ends: usize,
    frames: Vec<usize>,
}

struct Observe(Arc<Mutex<Hooks>>);

impl Inspector<BaseEvmTypes> for Observe {
    fn call(
        &mut self,
        _: &mut Interpreter<'_, '_, BaseEvmTypes>,
        _: &mut Message,
    ) -> Option<MessageResult> {
        self.0.lock().unwrap().calls += 1;
        None
    }

    fn call_end(
        &mut self,
        _: &mut Interpreter<'_, '_, BaseEvmTypes>,
        _: &Message,
        _: &mut MessageResult,
    ) {
        self.0.lock().unwrap().ends += 1;
    }

    fn frame_end(
        &mut self,
        index: usize,
        _: &alloy_eips::eip8141::FrameReceipt,
        _: &Bytes,
        _: InstrStop,
        _: &mut Evm<'_, BaseEvmTypes>,
    ) {
        self.0.lock().unwrap().frames.push(index);
    }
}

#[test]
fn generated_sequences_match_rollback_model_and_inspected_prefix_reuse() {
    let mut cases = 0;
    // Three independently chosen actions: write zero, write one, write+revert, exceptional halt.
    // Two link bits form every contiguous atomic partition of those actions.
    for actions in 0usize..64 {
        for links in 0usize..4 {
            // No assertions, two passing assertions, or first assertion fails and second is
            // skipped.
            for assertions in 0..3 {
                for keyed in [false, true] {
                    let label = format!(
                        "actions={actions}, links={links}, assertions={assertions}, keyed={keyed}"
                    );
                    let choices = [actions & 3, (actions >> 2) & 3, (actions >> 4) & 3];
                    let mut frames = vec![frame(FrameMode::Verify, SENDER, 3)];
                    for (i, &choice) in choices.iter().enumerate() {
                        let atomic = i < 2 && links & (1 << i) != 0;
                        let mut f = frame(
                            FrameMode::Sender,
                            if choice == 3 { HALT } else { BODY },
                            if atomic { 4 } else { 0 },
                        );
                        if choice != 3 {
                            let mut data = [0; 64];
                            data[31] = if choice == 0 { 0 } else { 1 };
                            data[63] = u8::from(choice == 2);
                            f.data = Bytes::copy_from_slice(&data);
                        }
                        frames.push(f);
                    }
                    if assertions != 0 {
                        frames.extend(vec![frame(FrameMode::Assert, ASSERTION, 0); 2]);
                    }
                    let tx = Recovered::new_unchecked(
                        TxEnvelope::Eip8141(TxEip8141 {
                            chain_id: 1,
                            sender: SENDER,
                            nonce_keys: Some(if keyed { vec![U256::from(1)] } else { vec![] }),
                            frames,
                            ..Default::default()
                        }),
                        Address::ZERO,
                    );
                    let mut plain_evm = evm(assertions == 2);
                    let plain = plain_evm
                        .transact(&tx)
                        .unwrap_or_else(|error| panic!("{label}: {error:?}"))
                        .detach();
                    let mut inspected_evm = evm(assertions == 2);
                    // Admission must leave neither a nonce increment nor warmth/gas effects in the
                    // subsequent full transaction on the same EVM instance.
                    assert!(
                        inspected_evm.validate_frame_prefix(&tx, 1).unwrap().sender_approved,
                        "{label}"
                    );
                    let hooks = Arc::new(Mutex::new(Hooks::default()));
                    inspected_evm.set_inspector(Observe(hooks.clone()));
                    let inspected = inspected_evm.transact(&tx).unwrap().detach();
                    assert_eq!(plain, inspected, "{label}");

                    let outcome = plain.result.frame.as_ref().unwrap();
                    let mut statuses = vec![FrameStatus::Success; outcome.receipts.len()];
                    let mut rolled_back = vec![false; outcome.receipts.len()];
                    let mut live_logs = [false; 3];
                    let mut value = 0;
                    let mut start = 0;
                    while start < 3 {
                        let mut end = start;
                        while end < 2 && links & (1 << end) != 0 {
                            end += 1;
                        }
                        let before = value;
                        for i in start..=end {
                            if choices[i] >= 2 {
                                statuses[i + 1] = FrameStatus::Failure;
                                // Roll back earlier successes in this group and skip its suffix.
                                value = before;
                                for j in start..i {
                                    rolled_back[j + 1] = true;
                                    live_logs[j] = false;
                                }
                                statuses[i + 2..end + 2].fill(FrameStatus::SkippedAtomicBatch);
                                break;
                            }
                            value = choices[i];
                            live_logs[i] = true;
                        }
                        start = end + 1;
                    }
                    if assertions == 2 {
                        value = 0;
                        live_logs.fill(false);
                        rolled_back[1..4].fill(true);
                        statuses[4] = FrameStatus::Failure;
                        statuses[5] = FrameStatus::SkippedAtomicBatch;
                    }
                    assert_eq!(
                        stored(&plain.pending_state, BODY, U256::ZERO),
                        U256::from(value),
                        "{label}"
                    );
                    assert_eq!(
                        outcome.receipts.iter().map(|r| r.status).collect::<Vec<_>>(),
                        statuses,
                        "{label}"
                    );
                    assert_eq!(outcome.rolled_back, rolled_back, "{label}");
                    assert_eq!(
                        outcome.assertion_failure,
                        (assertions == 2).then_some(4),
                        "{label}"
                    );
                    assert_eq!(plain.result.status, assertions != 2, "{label}");
                    assert_eq!(
                        outcome.receipts.iter().map(|r| r.logs.len()).sum::<usize>(),
                        live_logs.iter().filter(|&&live| live).count(),
                        "{label}"
                    );
                    for (receipt, &rolled) in outcome.receipts.iter().zip(&rolled_back) {
                        if rolled || receipt.status != FrameStatus::Success {
                            assert_eq!(receipt.gas_used.state, 0, "{label}");
                            assert!(receipt.logs.is_empty(), "{label}");
                        }
                        if receipt.status == FrameStatus::SkippedAtomicBatch {
                            assert_eq!(receipt.gas_used.execution, 0, "{label}");
                        }
                    }
                    let intrinsic =
                        FrameGas::calculate(tx.inner().as_eip8141().unwrap(), plain_evm.version())
                            .unwrap()
                            .intrinsic;
                    assert_eq!(
                        plain.result.state_gas_spent,
                        outcome.receipts.iter().map(|r| r.gas_used.state).sum::<u64>(),
                        "{label}"
                    );
                    assert_eq!(
                        plain.result.total_gas_spent,
                        intrinsic
                            + outcome
                                .receipts
                                .iter()
                                .map(|r| r.gas_used.execution + r.gas_used.state)
                                .sum::<u64>(),
                        "{label}"
                    );
                    assert_eq!(
                        plain.pending_state.account_info(&SENDER).unwrap().nonce,
                        u64::from(!keyed),
                        "{label}"
                    );
                    if keyed {
                        assert_eq!(
                            stored(
                                &plain.pending_state,
                                NONCE_MANAGER,
                                nonce_slot(SENDER, U256::from(1))
                            ),
                            U256::from(1),
                            "{label}"
                        );
                    }
                    let hooks = hooks.lock().unwrap();
                    assert_eq!(hooks.calls, hooks.ends, "{label}");
                    assert_eq!(
                        hooks.frames,
                        (0..outcome.receipts.len()).collect::<Vec<_>>(),
                        "{label}"
                    );
                    assert_eq!(
                        hooks.calls,
                        statuses
                            .iter()
                            .filter(|&&status| status != FrameStatus::SkippedAtomicBatch)
                            .count(),
                        "{label}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 1536);
}
