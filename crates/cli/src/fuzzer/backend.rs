use crate::fuzzer::{
    case::{EvmCase, FuzzTxKind},
    normalize::{
        CanonicalAccount, CanonicalState, FrameOutcome, FuzzOutcomeKind, Outcome, TxReceipt,
        apply_account_changes, canonical_accounts, canonical_log, state_from_revm,
    },
};
use alloy_primitives::{Address, map::HashMap};
use evm2::{
    BaseEvmConfigSelector, BaseEvmTypes, Evm, EvmConfigSelector, Precompiles, SpecId, Version,
    bytecode::Bytecode,
    ethereum::ethereum_tx_registry,
    evm::{AccountInfo as Evm2AccountInfo, InMemoryDB},
    interpreter::InstrStop,
};
use revm::{
    ExecuteCommitEvm, ExecuteEvm, MainBuilder, MainContext,
    context::{CfgEnv, Context},
    context_interface::{either::Either, result::ExecutionResult},
    database::{EmptyDB as RevmEmptyDB, InMemoryDB as RevmInMemoryDB, State as RevmState},
    primitives::hardfork::SpecId as RevmSpecId,
};

pub(crate) trait EvmBackend {
    fn name(&self) -> &'static str;

    fn run(&mut self, case: &EvmCase) -> Outcome;
}

#[derive(Default)]
pub(crate) struct Evm2Backend {
    evm: Option<Evm<'static, BaseEvmTypes>>,
}

impl EvmBackend for Evm2Backend {
    fn name(&self) -> &'static str {
        "evm2"
    }

    fn run(&mut self, case: &EvmCase) -> Outcome {
        let evm = if let Some(evm) = &mut self.evm {
            evm.set_block(case.block.evm2());
            if evm.spec_id() != case.spec {
                evm.set_execution_config(
                    BaseEvmConfigSelector::execution_config(case.spec),
                    case.spec,
                    ethereum_tx_registry(case.spec),
                    Precompiles::base(case.spec),
                );
            }
            evm.set_database(evm2_db(case));
            evm
        } else {
            self.evm.insert(Evm::new(
                case.spec,
                case.block.evm2(),
                ethereum_tx_registry(case.spec),
                evm2_db(case),
                Precompiles::base(case.spec),
            ))
        };
        let mut receipts = Vec::with_capacity(1 + case.extra_txs.len());
        for tx in case.txs() {
            let result = evm.transact(&tx.evm2());
            match result {
                Ok(result) => {
                    let mut state = CanonicalState::default();
                    let Ok(tx_result) = result.commit_with(&mut state);
                    let frame = tx_result.frame.as_ref().map(|frame| FrameOutcome {
                        payer: frame.payer,
                        receipts: frame.receipts.clone(),
                        outputs: frame.outputs.iter().map(|output| output.to_vec()).collect(),
                        spent: tx_result.total_gas_spent,
                        state_gas: tx_result.state_gas_spent,
                        refunded: tx_result.refunded,
                        floor: tx_result.floor_gas,
                        block_execution: tx_result.execution_gas_spent(),
                    });
                    let output = if frame.is_some() {
                        None
                    } else if tx_result.status || tx_result.stop == InstrStop::Revert {
                        Some(tx_result.output.to_vec())
                    } else {
                        None
                    };
                    receipts.push(TxReceipt {
                        kind: if tx_result.status {
                            FuzzOutcomeKind::Success
                        } else {
                            FuzzOutcomeKind::RevertOrHalt
                        },
                        gas_used: Some(tx_result.tx_gas_used()),
                        output,
                        logs: tx_result.logs.iter().map(canonical_log).collect(),
                        state,
                        error: None,
                        frame,
                    });
                }
                Err(err) => {
                    receipts.push(TxReceipt::error(err.into()));
                    break;
                }
            }
        }
        Outcome::from_receipts(receipts)
    }
}

type RevmExecutor = revm::MainnetEvm<revm::handler::MainnetContext<RevmState<RevmInMemoryDB>>>;

#[derive(Default)]
pub(crate) struct RevmBackend {
    evm: Option<RevmExecutor>,
    accounts: HashMap<Address, CanonicalAccount>,
}

impl EvmBackend for RevmBackend {
    fn name(&self) -> &'static str {
        "revm"
    }

    fn run(&mut self, case: &EvmCase) -> Outcome {
        let mut cfg = CfgEnv::new();
        cfg.set_spec_and_mainnet_gas_params(revm_spec(case.spec));
        cfg.enable_eip8250 = case.spec.enables(SpecId::BOGOTA);
        cfg.set_max_blobs_per_tx(Version::base(case.spec).max_blobs_per_tx as u64);
        cfg = cfg.disable_tx_chain_id_check();
        let context = Context::mainnet()
            .with_cfg(cfg)
            .with_block(case.block.revm())
            .with_db(RevmState::builder().with_database(revm_db(case)).build());
        let evm = if let Some(evm) = &mut self.evm {
            evm.ctx = context;
            evm.instruction = revm::handler::instructions::EthInstructions::new_mainnet_with_spec(
                revm_spec(case.spec),
            );
            evm
        } else {
            self.evm.insert(context.build_mainnet())
        };

        let mut receipts = Vec::with_capacity(1 + case.extra_txs.len());
        let accounts = &mut self.accounts;
        canonical_accounts(case, accounts);
        for tx in case.txs() {
            let mut tx_env = tx.revm();
            if tx.kind == FuzzTxKind::Eip7702 {
                tx_env.authorization_list =
                    tx.eip7702_authorization_list().into_iter().map(Either::Left).collect();
            }
            match evm.transact(tx_env) {
                Ok(result) => {
                    let kind = if result.result.is_success() {
                        FuzzOutcomeKind::Success
                    } else {
                        FuzzOutcomeKind::RevertOrHalt
                    };
                    let state = result.state;
                    let canonical_state = state_from_revm(&state, accounts);
                    let frame = if let ExecutionResult::FrameTransaction {
                        payer,
                        frame_receipts,
                        frame_outputs,
                        gas,
                        ..
                    } = &result.result
                    {
                        Some(FrameOutcome {
                            payer: *payer,
                            receipts: frame_receipts.clone(),
                            outputs: frame_outputs.iter().map(|output| output.to_vec()).collect(),
                            spent: gas.total_gas_spent(),
                            state_gas: gas.state_gas_spent_final(),
                            refunded: gas.inner_refunded(),
                            floor: gas.floor_gas(),
                            block_execution: gas.frame_block_regular_gas_used(),
                        })
                    } else {
                        None
                    };
                    let receipt = TxReceipt {
                        kind,
                        gas_used: Some(result.result.tx_gas_used()),
                        output: result.result.output().map(|output| output.to_vec()),
                        logs: result.result.logs().iter().map(canonical_log).collect(),
                        state: canonical_state,
                        error: None,
                        frame,
                    };
                    evm.commit(state);
                    apply_account_changes(accounts, &receipt.state);
                    receipts.push(receipt);
                }
                Err(err) => {
                    receipts.push(TxReceipt::error(err.into()));
                    break;
                }
            }
        }
        Outcome::from_receipts(receipts)
    }
}

fn evm2_db(case: &EvmCase) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    for account in &case.accounts {
        db.insert_account_info(
            &account.address,
            Evm2AccountInfo::default()
                .with_balance(account.balance)
                .with_nonce(account.nonce)
                .with_code(Bytecode::new_legacy(account.code.clone())),
        );
        for (key, value) in &account.storage {
            db.insert_account_storage(&account.address, key, value);
        }
    }
    db
}

fn revm_db(case: &EvmCase) -> RevmInMemoryDB {
    let mut db = RevmInMemoryDB::new(RevmEmptyDB::new());
    for account in &case.accounts {
        let mut info = revm::state::AccountInfo {
            balance: account.balance,
            nonce: account.nonce,
            code: Some(revm::state::Bytecode::new_legacy(account.code.clone())),
            ..Default::default()
        };
        db.insert_contract(&mut info);
        db.insert_account_info(account.address, info);
        for (key, value) in &account.storage {
            if let Err(err) = db.insert_account_storage(account.address, *key, *value) {
                panic!("revm in-memory storage insertion failed: {err:?}");
            }
        }
    }
    db
}

fn revm_spec(spec: SpecId) -> RevmSpecId {
    match spec {
        SpecId::FRONTIER => RevmSpecId::FRONTIER,
        SpecId::HOMESTEAD => RevmSpecId::HOMESTEAD,
        SpecId::TANGERINE => RevmSpecId::TANGERINE,
        SpecId::SPURIOUS_DRAGON => RevmSpecId::SPURIOUS_DRAGON,
        SpecId::BYZANTIUM => RevmSpecId::BYZANTIUM,
        SpecId::PETERSBURG => RevmSpecId::PETERSBURG,
        SpecId::ISTANBUL => RevmSpecId::ISTANBUL,
        SpecId::BERLIN => RevmSpecId::BERLIN,
        SpecId::LONDON => RevmSpecId::LONDON,
        SpecId::MERGE => RevmSpecId::MERGE,
        SpecId::SHANGHAI => RevmSpecId::SHANGHAI,
        SpecId::CANCUN => RevmSpecId::CANCUN,
        SpecId::PRAGUE => RevmSpecId::PRAGUE,
        SpecId::OSAKA => RevmSpecId::OSAKA,
        SpecId::AMSTERDAM => RevmSpecId::AMSTERDAM,
        SpecId::BOGOTA => RevmSpecId::BOGOTA,
        other => panic!("unsupported reference fuzzer fork: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fuzzer::{
        case::CaseGenerator, features::FuzzFeatures, normalize::FuzzError, rng::Gen,
    };
    use alloy_eips::eip8141::FrameStatus;

    #[test]
    fn reused_backends_match_fresh_executors() {
        let mut generator = CaseGenerator::default();
        let mut evm2 = Evm2Backend::default();
        let mut revm = RevmBackend::default();
        for seed in 0..512 {
            let case = generator.generate(&mut Gen::new(seed));
            let expected = RevmBackend::default().run(case);
            assert_eq!(revm.run(case), expected, "revm seed {seed}");
            assert_eq!(Evm2Backend::default().run(case), expected, "fresh evm2 seed {seed}");
            assert_eq!(evm2.run(case), expected, "reused evm2 seed {seed}");
        }
    }

    #[test]
    fn frame_generation_compares_approvals_atomic_nonces_assertions_and_recent_roots() {
        let mut generator = CaseGenerator::default();
        let mut native = Evm2Backend::default();
        let mut reference = RevmBackend::default();
        let mut features = FuzzFeatures::empty();
        let mut skipped = false;
        let mut state_growth = false;
        let mut sponsored = false;
        for seed in 0..1024 {
            let case = generator.generate(&mut Gen::new(seed));
            if case.tx.kind != FuzzTxKind::Eip8141 {
                continue;
            }
            features |= case.features;
            let expected = reference.run(case);
            let actual = native.run(case);
            assert_eq!(actual, expected, "frame seed {seed}: {case:#?}");
            for receipt in &actual.receipts {
                if let Some(frame) = &receipt.frame {
                    skipped |= frame
                        .receipts
                        .iter()
                        .any(|receipt| receipt.status == FrameStatus::SkippedAtomicBatch);
                    state_growth |= frame.state_gas > 0;
                    sponsored |= frame.payer == Address::with_last_byte(0x44);
                }
            }
        }
        assert!(features.contains(
            FuzzFeatures::FRAME_ATOMIC
                | FuzzFeatures::FRAME_ASSERTION
                | FuzzFeatures::FRAME_KEYED_NONCE
                | FuzzFeatures::FRAME_NESTED_APPROVAL
                | FuzzFeatures::FRAME_SPONSOR
                | FuzzFeatures::FRAME_INVALID
                | FuzzFeatures::FRAME_RECENT_ROOT
        ));
        assert!(skipped);
        assert!(state_growth);
        assert!(sponsored);
    }

    #[test]
    fn frame_regressions_replay_every_scenario_with_account_and_keyed_nonces() {
        let mut native = Evm2Backend::default();
        let mut reference = RevmBackend::default();
        for scenario in 0..14 {
            for keyed in [false, true] {
                let case = CaseGenerator::frame_regression_case(scenario, keyed);
                let encoded = serde_json::to_vec(&case).unwrap();
                let replay: EvmCase = serde_json::from_slice(&encoded).unwrap();
                let expected = reference.run(&case);
                assert_eq!(native.run(&case), expected, "scenario {scenario}, keyed {keyed}");
                assert_eq!(reference.run(&replay), expected);
                assert_eq!(native.run(&replay), expected);
                if matches!(scenario, 9..=12) {
                    assert_eq!(expected.kind, FuzzOutcomeKind::Error);
                } else if scenario == 13 {
                    let verifier = &case.tx.frame_transaction.as_ref().unwrap().frames[0];
                    let slot = u64::from_be_bytes(verifier.data[32..40].try_into().unwrap());
                    let age = case.block.slot_num - slot;
                    if age == 0 || age >= 8192 {
                        assert_eq!(expected.kind, FuzzOutcomeKind::Error);
                        assert_eq!(expected.error, Some(FuzzError::InvalidFrameTransaction));
                    } else {
                        assert!(expected.receipts[0].frame.is_some());
                    }
                } else {
                    assert!(expected.receipts[0].frame.is_some(), "scenario {scenario}");
                }
            }
        }
    }

    #[test]
    fn frame_regression_rejects_type_six_before_bogota() {
        for spec in [SpecId::CANCUN, SpecId::PRAGUE, SpecId::OSAKA, SpecId::AMSTERDAM] {
            let mut case = CaseGenerator::frame_regression_case(0, false);
            case.spec = spec;
            let expected = RevmBackend::default().run(&case);
            assert_eq!(Evm2Backend::default().run(&case), expected, "{spec:?}");
            assert_eq!(expected.error, Some(FuzzError::UnsupportedTransactionType));
        }
    }

    #[test]
    fn frame_combinations_compare_approvals_atomic_assertions_and_nonce_sequences() {
        let mut native = Evm2Backend::default();
        let mut reference = RevmBackend::default();
        for approval in [0, 3, 4, 5] {
            for keyed in [false, true] {
                for atomic in [false, true] {
                    for assertions in [false, true] {
                        let mut case = CaseGenerator::frame_regression_case(approval, keyed);
                        let payload = case.tx.frame_transaction.as_mut().unwrap();
                        if atomic {
                            let source = CaseGenerator::frame_regression_case(1, keyed);
                            let frames = &source.tx.frame_transaction.as_ref().unwrap().frames;
                            payload.frames.pop();
                            payload.frames.extend_from_slice(&frames[1..]);
                        }
                        if assertions {
                            let source = CaseGenerator::frame_regression_case(2, keyed);
                            let frames = &source.tx.frame_transaction.as_ref().unwrap().frames;
                            payload.frames.extend_from_slice(&frames[2..]);
                        }
                        let mut next = case.tx.clone();
                        next.frame_transaction.as_mut().unwrap().nonce = 1;
                        case.extra_txs.push(next);
                        let expected = reference.run(&case);
                        assert_eq!(
                            native.run(&case),
                            expected,
                            "approval {approval}, keyed {keyed}, atomic {atomic}, assertions {assertions}"
                        );
                        assert_eq!(expected.receipts.len(), 2);
                        for receipt in expected.receipts {
                            let frame = receipt.frame.unwrap();
                            if atomic {
                                assert!(frame.receipts.iter().any(|receipt| {
                                    receipt.status == FrameStatus::SkippedAtomicBatch
                                }));
                            }
                            if assertions {
                                assert_eq!(receipt.kind, FuzzOutcomeKind::RevertOrHalt);
                            }
                        }
                    }
                }
            }
        }
    }
}
