//! Paired native/reference transaction checks for the normal EEST fixture executors.

use crate::{execute::state_root_from_database, tx::reference_tx};
use alloy_eips::eip8141::FrameReceipt;
use alloy_primitives::{Address, B256, Bytes, Log};
use evm2::{
    SpecId, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::RecoveredTxEnvelope,
    evm::{AccountInfo, InMemoryDB},
    registry::HandlerError,
    version::Version,
};
use revm::{
    Context, ExecuteEvm, MainBuilder, MainContext,
    context::result::{EVMError, ExecutionResult},
    context_interface::block::BlobExcessGasAndPrice,
    database::{CacheDB, EmptyDB},
    primitives::hardfork::SpecId as ReferenceSpec,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    state_root: B256,
    success: bool,
    kind: &'static str,
    spent: u64,
    state: u64,
    refunded: u64,
    floor: u64,
    fee: u64,
    block_execution: u64,
    output: Bytes,
    created_address: Option<Address>,
    logs: Vec<Log>,
    frame: Option<(Address, Vec<FrameReceipt>, Vec<Bytes>)>,
}

#[derive(Debug)]
pub(crate) enum Failure {
    Rejected { reason: String, state_root: B256 },
    Internal(String),
}

pub(crate) fn execute(
    pre: &InMemoryDB,
    block: BlockEnv,
    spec: SpecId,
    tx: &RecoveredTxEnvelope,
) -> Result<Outcome, Failure> {
    let mut db = CacheDB::<EmptyDB>::default();
    for (&address, info) in &pre.cache.accounts {
        if let Some(info) = info {
            let code = pre
                .cache
                .contracts
                .get(&info.code_hash)
                .map(|code| code.original_bytes())
                .unwrap_or_default();
            db.insert_account_info(
                address,
                revm::state::AccountInfo::default()
                    .with_nonce(info.nonce)
                    .with_balance(info.balance)
                    .with_code(
                        revm::bytecode::Bytecode::new_raw_checked(code.clone())
                            .unwrap_or_else(|_| revm::bytecode::Bytecode::new_legacy(code)),
                    ),
            );
        }
    }
    for (&address, storage) in &pre.cache.storage {
        for (&key, &value) in &storage.slots {
            db.insert_account_storage(address, key, value).unwrap();
        }
    }
    for (&number, &hash) in &pre.cache.block_hashes {
        db.cache.block_hashes.insert(number, hash);
    }
    let reference_spec = reference_spec(spec)?;
    let basefee =
        u64::try_from(block.basefee).map_err(|error| Failure::Internal(error.to_string()))?;
    let gas_limit =
        u64::try_from(block.gas_limit).map_err(|error| Failure::Internal(error.to_string()))?;
    let slot_num =
        u64::try_from(block.slot_num).map_err(|error| Failure::Internal(error.to_string()))?;
    let blob_gasprice =
        u128::try_from(block.blob_basefee).map_err(|error| Failure::Internal(error.to_string()))?;
    let mut evm = Context::mainnet()
        .modify_cfg_chained(|cfg| {
            cfg.set_spec_and_mainnet_gas_params(reference_spec);
            cfg.enable_eip8250 = spec.enables(SpecId::BOGOTA);
            cfg.set_max_blobs_per_tx(Version::base(spec).max_blobs_per_tx as u64);
        })
        .modify_block_chained(|env| {
            env.number = block.number;
            env.beneficiary = block.beneficiary;
            env.timestamp = block.timestamp;
            env.gas_limit = gas_limit;
            env.basefee = basefee;
            env.difficulty = block.difficulty;
            env.prevrandao = Some(B256::from(block.prevrandao));
            env.slot_num = slot_num;
            env.blob_excess_gas_and_price =
                Some(BlobExcessGasAndPrice { excess_blob_gas: 0, blob_gasprice });
        })
        .with_db(db)
        .build_mainnet();
    let reference_tx = reference_tx(tx).map_err(Failure::Internal)?;
    let executed = evm.transact(reference_tx).map_err(|error| match error {
        EVMError::Transaction(error) => Failure::Rejected {
            reason: format!("{error:?}"),
            state_root: state_root_from_database(pre),
        },
        error => Failure::Internal(format!("{error:?}")),
    })?;
    let mut post = pre.clone();
    for (address, account) in executed.state {
        if !account.is_touched() {
            continue;
        }
        if account.is_selfdestructed()
            || (spec.enables(SpecId::SPURIOUS_DRAGON) && account.info.is_empty())
        {
            post.cache.accounts.insert(address, None);
            post.cache.storage.remove(&address);
            continue;
        }
        if account.is_created() {
            post.cache.storage.remove(&address);
        }
        let mut info = AccountInfo::new(
            account.info.balance,
            account.info.nonce,
            account.info.code_hash,
            None,
        );
        if let Some(code) = account.info.code {
            info = info.with_code(Bytecode::new_raw(code.original_bytes()));
        }
        post.insert_account_info(&address, info);
        for (key, value) in account.storage {
            post.insert_account_storage(&address, &key, &value.present_value());
        }
    }
    let result = executed.result;
    let gas = result.gas();
    let frame = match &result {
        ExecutionResult::FrameTransaction { payer, frame_receipts, frame_outputs, .. } => {
            Some((*payer, frame_receipts.clone(), frame_outputs.clone()))
        }
        _ => None,
    };
    Ok(Outcome {
        state_root: state_root_from_database(&post),
        success: result.is_success(),
        kind: match &result {
            ExecutionResult::Success { .. } => "success",
            ExecutionResult::Revert { .. } => "revert",
            ExecutionResult::Halt { .. } => "halt",
            ExecutionResult::FrameTransaction { .. } => "frame",
        },
        spent: gas.total_gas_spent(),
        state: gas.state_gas_spent_final(),
        refunded: gas.inner_refunded(),
        floor: gas.floor_gas(),
        fee: result.tx_gas_used(),
        block_execution: result.block_regular_gas_used(),
        output: if frame.is_some() {
            Bytes::new()
        } else {
            result.clone().into_output().unwrap_or_default()
        },
        created_address: result.created_address(),
        logs: result.logs().to_vec(),
        frame,
    })
}

pub(crate) fn compare(
    expected: Result<Outcome, Failure>,
    native: Result<&TxResult, &HandlerError>,
    post: &InMemoryDB,
) -> Result<(), String> {
    match (expected, native) {
        (Err(Failure::Internal(reason)), actual) => {
            Err(format!("reference infrastructure failure: {reason}; native: {actual:?}"))
        }
        (Err(Failure::Rejected { reason, state_root }), Err(native)) => {
            if matches!(
                native,
                HandlerError::Database(_)
                    | HandlerError::Fatal(_)
                    | HandlerError::External(_)
                    | HandlerError::WrongTransactionType { .. }
            ) {
                return Err(format!(
                    "native internal failure {native:?}; reference rejected: {reason}"
                ));
            }
            let actual = state_root_from_database(post);
            if actual != state_root {
                return Err(format!(
                    "rejected transaction changed state: native {actual}; reference {state_root}"
                ));
            }
            Ok(())
        }
        (Ok(expected), Ok(native)) => {
            let actual = Outcome {
                state_root: state_root_from_database(post),
                success: native.status,
                kind: if native.frame.is_some() {
                    "frame"
                } else if native.stop.is_success() {
                    "success"
                } else if native.stop.is_revert() {
                    "revert"
                } else {
                    "halt"
                },
                spent: native.total_gas_spent,
                state: native.state_gas_spent,
                refunded: native.refunded,
                floor: native.floor_gas,
                fee: native.tx_gas_used(),
                block_execution: native.execution_gas_spent(),
                output: native.output.clone(),
                created_address: native.created_address,
                logs: native.logs.clone(),
                frame: native
                    .frame
                    .as_ref()
                    .map(|frame| (frame.payer, frame.receipts.clone(), frame.outputs.clone())),
            };
            if actual != expected {
                return Err(format!(
                    "native/reference mismatch\nnative: {actual:#?}\nreference: {expected:#?}"
                ));
            }
            Ok(())
        }
        (expected, actual) => Err(format!(
            "native/reference acceptance mismatch\nnative: {actual:?}\nreference: {expected:?}"
        )),
    }
}

fn reference_spec(spec: SpecId) -> Result<ReferenceSpec, Failure> {
    Ok(match spec {
        SpecId::FRONTIER => ReferenceSpec::FRONTIER,
        SpecId::HOMESTEAD => ReferenceSpec::HOMESTEAD,
        SpecId::TANGERINE => ReferenceSpec::TANGERINE,
        SpecId::SPURIOUS_DRAGON => ReferenceSpec::SPURIOUS_DRAGON,
        SpecId::BYZANTIUM => ReferenceSpec::BYZANTIUM,
        SpecId::PETERSBURG => ReferenceSpec::PETERSBURG,
        SpecId::ISTANBUL => ReferenceSpec::ISTANBUL,
        SpecId::BERLIN => ReferenceSpec::BERLIN,
        SpecId::LONDON => ReferenceSpec::LONDON,
        SpecId::MERGE => ReferenceSpec::MERGE,
        SpecId::SHANGHAI => ReferenceSpec::SHANGHAI,
        SpecId::CANCUN => ReferenceSpec::CANCUN,
        SpecId::PRAGUE => ReferenceSpec::PRAGUE,
        SpecId::OSAKA => ReferenceSpec::OSAKA,
        SpecId::AMSTERDAM => ReferenceSpec::AMSTERDAM,
        SpecId::BOGOTA => ReferenceSpec::BOGOTA,
        other => return Err(Failure::Internal(format!("unsupported reference fork {other:?}"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;
    use evm2::DatabaseError;
    use std::io;

    #[test]
    fn reference_internal_failures_and_native_fatal_errors_never_agree_with_rejection() {
        let database = InMemoryDB::default();
        let rejection = || {
            Err(Failure::Rejected {
                reason: "fixture validation failure".into(),
                state_root: state_root_from_database(&database),
            })
        };
        assert!(compare(rejection(), Err(&HandlerError::InsufficientFunds), &database).is_ok());
        for error in [
            HandlerError::Database(DatabaseError::new(io::Error::other("database failure"), true)),
            HandlerError::Fatal("fatal host failure".into()),
            HandlerError::External("unclassified handler failure".into()),
            HandlerError::WrongTransactionType { expected: 6 },
        ] {
            assert!(compare(rejection(), Err(&error), &database).is_err());
        }
        assert!(
            compare(
                Err(Failure::Internal("conversion failed".into())),
                Err(&HandlerError::InsufficientFunds),
                &database
            )
            .is_err()
        );
        let mut modified = database.clone();
        modified
            .insert_account_info(&Address::ZERO, AccountInfo::default().with_balance(U256::ONE));
        assert!(compare(rejection(), Err(&HandlerError::InsufficientFunds), &modified).is_err());
    }
}
