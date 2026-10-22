use crate::fuzzer::{
    backend::EvmBackend,
    case::{EvmCase, TARGET},
};
use evm2::interpreter::op;

pub(crate) fn differs(backends: &mut [&mut dyn EvmBackend; 2], case: &EvmCase) -> bool {
    backends[0].run(case) != backends[1].run(case)
}

pub(crate) fn minimize_case(backends: &mut [&mut dyn EvmBackend; 2], mut case: EvmCase) -> EvmCase {
    minimize_accounts(backends, &mut case);
    minimize_storage(backends, &mut case);
    minimize_calldata(backends, &mut case);
    minimize_frames(backends, &mut case);
    minimize_frame_signatures(backends, &mut case);
    minimize_target_code(backends, &mut case);
    case
}

fn minimize_accounts(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    let mut index = 2;
    while index < case.accounts.len() {
        let mut candidate = case.clone();
        candidate.accounts.remove(index);
        if differs(backends, &candidate) {
            *case = candidate;
        } else {
            index += 1;
        }
    }
}

fn minimize_storage(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    for account_index in 0..case.accounts.len() {
        while let Some(key) = case.accounts[account_index].storage.keys().min().copied() {
            let mut candidate = case.clone();
            candidate.accounts[account_index].storage.remove(&key);
            if differs(backends, &candidate) {
                *case = candidate;
            } else {
                break;
            }
        }
    }
}

fn minimize_calldata(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    let mut len = case.tx.input.len();
    while len > 0 {
        let next = len / 2;
        let mut candidate = case.clone();
        candidate.tx.input = candidate.tx.input.slice(..next);
        if differs(backends, &candidate) {
            *case = candidate;
            len = next;
        } else {
            break;
        }
    }
}

fn minimize_target_code(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    let Some(target_index) = case.accounts.iter().position(|account| account.address == TARGET)
    else {
        return;
    };
    let mut len = case.accounts[target_index].code.len();
    while len > 1 {
        let next = len / 2;
        // The candidate is `next` bytes plus an appended STOP, so it only shrinks the code when
        // `next + 1 < len`. Without this guard a two-byte code (`next == 1`, `next + 1 == len`)
        // would loop forever, re-testing an identical candidate and never making progress.
        if next + 1 >= len {
            break;
        }
        let mut candidate = case.clone();
        let mut code = candidate.accounts[target_index].code.slice(..next).to_vec();
        code.push(op::STOP);
        candidate.accounts[target_index].code = code.into();
        if differs(backends, &candidate) {
            *case = candidate;
            len = next + 1;
        } else {
            break;
        }
    }
}

fn minimize_frames(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    let mut extra = case.extra_txs.len();
    while extra > 0 {
        extra -= 1;
        let mut candidate = case.clone();
        candidate.extra_txs.remove(extra);
        if differs(backends, &candidate) {
            *case = candidate;
        }
    }
    for tx_index in 0..1 + case.extra_txs.len() {
        let frame_count = case
            .txs()
            .nth(tx_index)
            .and_then(|tx| tx.frame_transaction.as_ref())
            .map_or(0, |tx| tx.frames.len());
        for frame_index in (0..frame_count).rev() {
            let mut candidate = case.clone();
            let tx = core::iter::once(&mut candidate.tx)
                .chain(&mut candidate.extra_txs)
                .nth(tx_index)
                .unwrap();
            tx.frame_transaction.as_mut().unwrap().frames.remove(frame_index);
            if differs(backends, &candidate) {
                *case = candidate;
            }
        }
        let frame_count = case
            .txs()
            .nth(tx_index)
            .and_then(|tx| tx.frame_transaction.as_ref())
            .map_or(0, |tx| tx.frames.len());
        for frame_index in 0..frame_count {
            let mut len =
                case.txs().nth(tx_index).unwrap().frame_transaction.as_ref().unwrap().frames
                    [frame_index]
                    .data
                    .len();
            while len > 0 {
                let mut candidate = case.clone();
                let tx = core::iter::once(&mut candidate.tx)
                    .chain(&mut candidate.extra_txs)
                    .nth(tx_index)
                    .unwrap();
                let frame = &mut tx.frame_transaction.as_mut().unwrap().frames[frame_index];
                frame.data = frame.data.slice(..len / 2);
                if differs(backends, &candidate) {
                    *case = candidate;
                    len /= 2;
                } else {
                    break;
                }
            }
        }
    }
}

fn minimize_frame_signatures(backends: &mut [&mut dyn EvmBackend; 2], case: &mut EvmCase) {
    for tx_index in 0..1 + case.extra_txs.len() {
        let signature_count = case
            .txs()
            .nth(tx_index)
            .and_then(|tx| tx.frame_transaction.as_ref())
            .map_or(0, |tx| tx.signatures.len());
        for signature_index in (0..signature_count).rev() {
            let mut candidate = case.clone();
            let tx = core::iter::once(&mut candidate.tx)
                .chain(&mut candidate.extra_txs)
                .nth(tx_index)
                .unwrap();
            tx.frame_transaction.as_mut().unwrap().signatures.remove(signature_index);
            if differs(backends, &candidate) {
                *case = candidate;
            }
        }
        let signature_count = case
            .txs()
            .nth(tx_index)
            .and_then(|tx| tx.frame_transaction.as_ref())
            .map_or(0, |tx| tx.signatures.len());
        for signature_index in 0..signature_count {
            let mut len =
                case.txs().nth(tx_index).unwrap().frame_transaction.as_ref().unwrap().signatures
                    [signature_index]
                    .signature
                    .len();
            while len > 0 {
                let mut candidate = case.clone();
                let tx = core::iter::once(&mut candidate.tx)
                    .chain(&mut candidate.extra_txs)
                    .nth(tx_index)
                    .unwrap();
                let signature =
                    &mut tx.frame_transaction.as_mut().unwrap().signatures[signature_index];
                signature.signature = signature.signature.slice(..len / 2);
                if differs(backends, &candidate) {
                    *case = candidate;
                    len /= 2;
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzzer::{
        case::CaseGenerator,
        normalize::{FuzzError, Outcome},
    };
    use alloy_eips::eip8141::FrameSignature;

    struct MarkerBackend {
        probe: bool,
        signatures: bool,
    }

    impl EvmBackend for MarkerBackend {
        fn name(&self) -> &'static str {
            "marker"
        }

        fn run(&mut self, case: &EvmCase) -> Outcome {
            let marked = case.txs().filter_map(|tx| tx.frame_transaction.as_ref()).any(|tx| {
                if self.signatures {
                    tx.signatures.iter().any(|signature| signature.signature.first() == Some(&0x42))
                } else {
                    tx.frames.iter().any(|frame| frame.data.first() == Some(&0x42))
                }
            });
            Outcome::error(if self.probe && marked {
                FuzzError::InvalidFrameTransaction
            } else {
                FuzzError::InvalidNonce
            })
        }
    }

    #[test]
    fn frame_minimization_preserves_a_later_transaction_and_its_reproducer() {
        for signatures in [false, true] {
            let mut case = CaseGenerator::frame_regression_case(0, true);
            case.tx.frame_transaction.as_mut().unwrap().signatures.clear();
            let mut later = case.tx.clone();
            let payload = later.frame_transaction.as_mut().unwrap();
            payload.signatures.clear();
            payload.signatures.push(FrameSignature {
                signature: vec![if signatures { 0x42 } else { 0x11 }; 32].into(),
                ..Default::default()
            });
            payload.frames[1].data = vec![if signatures { 0x11 } else { 0x42 }; 32].into();
            case.extra_txs = vec![case.tx.clone(), later, case.tx.clone()];
            let mut baseline = MarkerBackend { probe: false, signatures };
            let mut probe = MarkerBackend { probe: true, signatures };
            let backends: &mut [&mut dyn EvmBackend; 2] = &mut [&mut baseline, &mut probe];
            assert!(differs(backends, &case));
            let minimized = minimize_case(backends, case);
            assert!(differs(backends, &minimized));
            assert_eq!(minimized.extra_txs.len(), 1);
            assert!(minimized.tx.frame_transaction.as_ref().unwrap().frames.is_empty());
            let payload = minimized.extra_txs[0].frame_transaction.as_ref().unwrap();
            if signatures {
                assert!(payload.frames.is_empty());
                assert_eq!(payload.signatures.len(), 1);
                assert_eq!(payload.signatures[0].signature.as_ref(), &[0x42]);
            } else {
                assert!(payload.signatures.is_empty());
                assert_eq!(payload.frames.len(), 1);
                assert_eq!(payload.frames[0].data.as_ref(), &[0x42]);
            }
            let encoded = serde_json::to_vec(&minimized).unwrap();
            let replay = serde_json::from_slice(&encoded).unwrap();
            assert!(differs(backends, &replay));
        }
    }
}
