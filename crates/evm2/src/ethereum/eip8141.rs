//! Bogota devnet 1 frame transaction preparation and ordered execution.
//!
//! Newer draft signing, nonce and assertion layouts must not be enabled under this fork.

use super::{effective_gas_price, validate_chain_id, validate_gas_price, validate_priority_fee};
use crate::{
    Evm, EvmFeatures, EvmTypes, HostError, TxResult, TxResultExt, Version,
    bytecode::Bytecode,
    env::TxEnvExt,
    evm::{FrameTransactionResult, StateCheckpoint, frame::FrameTransactionRuntime},
    interpreter::{
        GasTracker, Host, InstrStop, Message, MessageExt, MessageKind, MessageResultExt,
    },
    precompiles,
    registry::{HandlerError, HandlerResult, TxRequest},
    version::GasId,
};
use alloc::{sync::Arc, vec::Vec};
use alloy_consensus::{TxEip8141, transaction::Recovered};
use alloy_eips::{
    eip4844::{DATA_GAS_PER_BLOB, VERSIONED_HASH_VERSION_KZG},
    eip8141::{
        ENTRY_POINT, Frame, FrameGasUsed, FrameMode, FrameReceipt, FrameStatus, NONCE_MANAGER,
        SECP256K1N, SECP256R1N, SignatureScheme, nonce_calldata, nonce_slot, validate_nonce_keys,
    },
};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};

/// Schedule-aware static reservation; state and execution are independent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameGas {
    /// Intrinsic execution cost, including all signature verification.
    pub intrinsic: u64,
    /// Calldata floor in the execution dimension.
    pub floor: u64,
    /// Worst-case execution dimension for block admission.
    pub execution: u64,
    /// Worst-case state dimension for block admission.
    pub state: u64,
    /// Combined maximum gas funded by payment approval.
    pub maximum: u64,
    /// Protocol signature verification work.
    pub signatures: u64,
}

/// Prepared native frame transaction. Preparation runs no application calls.
#[derive(Clone, Debug)]
pub struct PreparedFrameTx {
    /// Canonical immutable payload with its explicit sender.
    pub payload: Arc<TxEip8141>,
    /// Canonical Bogota signing hash.
    pub signature_hash: B256,
    /// Active-schedule gas and reservation metadata.
    pub gas: FrameGas,
    /// Effective execution gas price.
    pub gas_price: U256,
    /// Blob fee charged at the current block's blob base fee.
    pub blob_charge: U256,
    /// Maximum amount collected at payment approval.
    pub max_cost: U256,
    /// Legacy account nonce before any frame executes.
    pub legacy_nonce: u64,
}

/// Read-only executable-prefix result consumed by node admission policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameValidationResult {
    /// Account that successfully approved payment.
    pub payer: Address,
    /// Maximum approved payment exposure.
    pub max_cost: U256,
    /// Exclusive index after the successful payment frame.
    pub prefix_end: usize,
    /// Whether execution as the sender was approved.
    pub sender_approved: bool,
    /// Signature verification gas for the complete transaction.
    pub signature_gas: u64,
    /// Execution gas consumed by the prefix frames.
    pub execution_gas: u64,
    /// State gas consumed by the prefix frames.
    pub state_gas: u64,
}

struct Batch {
    state: StateCheckpoint,
    approval: crate::evm::frame::FrameApprovalState,
    gas: usize,
    start: usize,
    warmth: Option<Vec<(Address, Vec<U256>)>>,
}

const fn invalid(reason: &'static str) -> HandlerError {
    HandlerError::InvalidFrameTransaction(reason)
}

impl FrameGas {
    /// Computes reservations from the original full-width payload and active schedule.
    pub fn calculate(tx: &TxEip8141, version: &Version) -> HandlerResult<Self> {
        let gp = &version.gas_params;
        let nonce_data =
            tx.nonce_keys.as_deref().map(|keys| nonce_calldata(keys, tx.nonce)).unwrap_or_default();
        let sum = |a: u64, b: u64| a.checked_add(b).ok_or(invalid("gas arithmetic overflow"));
        let mul = |a: u64, b: u64| a.checked_mul(b).ok_or(invalid("gas arithmetic overflow"));
        let count = tx
            .nonce_keys
            .as_deref()
            .map_or(0, |keys| if keys == [U256::ZERO] { 0 } else { keys.len() as u64 });
        let access = mul(
            count,
            sum(
                sum(
                    gp.get(GasId::WarmStorageReadCost).into(),
                    u64::from(gp.get(GasId::ColdStorageAdditionalCost)),
                )?,
                gp.get(GasId::SstoreSetWithoutLoadCost).into(),
            )?,
        )?;
        let signatures =
            tx.signatures.iter().try_fold(0, |s, sig| sum(s, sig.verification_gas()))?;
        let value = tx
            .frames
            .iter()
            .filter(|f| !f.value.is_zero() && f.target_address().is_some_and(|a| a != tx.sender))
            .try_fold(0, |s, _| sum(s, 6_000))?;
        let base = sum(
            sum(sum(12_000, mul(tx.frames.len() as u64, 475)?)?, signatures)?,
            sum(value, access)?,
        )?;
        let mut fields = core::iter::once(nonce_data.as_slice())
            .chain(tx.frames.iter().map(|frame| frame.data.as_ref()))
            .chain(tx.signatures.iter().flat_map(|sig| {
                [sig.signer.as_bytes(), sig.msg.as_bytes(), sig.signature.as_ref()]
            }));
        let (tokens, floor_tokens) = fields.try_fold((0, 0), |total, bytes| {
            bytes.iter().try_fold(total, |(tokens, floor), byte| {
                let nonzero = u64::from(gp.get(GasId::TxTokenNonZeroByteMultiplier));
                Ok::<_, HandlerError>((
                    sum(tokens, if *byte == 0 { 1 } else { nonzero })?,
                    sum(
                        floor,
                        if *byte == 0 {
                            gp.get(GasId::TxFloorZeroByteMultiplier).into()
                        } else {
                            nonzero
                        },
                    )?,
                ))
            })
        })?;
        let intrinsic = sum(base, mul(tokens, gp.get(GasId::TxTokenCost).into())?)?;
        let floor = sum(base, mul(floor_tokens, gp.get(GasId::TxFloorCostPerToken).into())?)?;
        let execution =
            sum(intrinsic, tx.frames.iter().try_fold(0, |s, f| sum(s, f.limits.execution))?)?
                .max(floor);
        let state = tx.frames.iter().try_fold(0, |s, f| sum(s, f.limits.state))?;
        Ok(Self { intrinsic, floor, execution, state, maximum: sum(execution, state)?, signatures })
    }
}

/// Performs shared preparation without executing a frame or consuming a nonce.
pub fn prepare<T: EvmTypes>(
    req: &mut TxRequest<'_, '_, T, TxEip8141>,
) -> HandlerResult<PreparedFrameTx> {
    prepare_payload(req.host, req.tx.inner())
}

fn prepare_payload<T: EvmTypes>(
    host: &mut Evm<'_, T>,
    tx: &TxEip8141,
) -> HandlerResult<PreparedFrameTx> {
    if !host.feature(EvmFeatures::EIP8141) {
        return Err(HandlerError::UnsupportedTransactionType(6));
    }
    let placeholders = host.feature(EvmFeatures::FRAME_SIGNATURE_PLACEHOLDERS);
    validate_structure(tx, placeholders)?;
    if tx.nonce_keys.is_some() != host.feature(EvmFeatures::EIP8250) {
        return Err(invalid("nonce payload does not match activation"));
    }
    if let Some(keys) = &tx.nonce_keys {
        validate_nonce_keys(keys).map_err(invalid)?;
    }
    if tx.nonce == u64::MAX {
        return Err(HandlerError::NonceOverflow);
    }
    let gas = FrameGas::calculate(tx, host.version())?;
    if gas.execution > host.version().tx_gas_limit_cap {
        return Err(HandlerError::TxGasLimitGreaterThanCap {
            gas_limit: gas.execution,
            cap: host.version().tx_gas_limit_cap,
        });
    }
    if host.feature(EvmFeatures::BLOCK_GAS_LIMIT_CHECK)
        && U256::from(gas.execution.max(gas.state)) > host.block.gas_limit
    {
        return Err(HandlerError::GasLimitMoreThanBlock {
            gas_limit: gas.execution.max(gas.state),
            block_gas_limit: host.block.gas_limit,
        });
    }
    validate_chain_id(host.version(), Some(tx.chain_id), false)?;
    validate_priority_fee(
        host.version(),
        tx.fees.max_fee_per_gas,
        tx.fees.max_priority_fee_per_gas,
    )?;
    let gas_price = effective_gas_price(
        tx.fees.max_fee_per_gas,
        tx.fees.max_priority_fee_per_gas,
        host.block.basefee,
    );
    validate_gas_price(host.version(), gas_price, host.block.basefee)?;
    if tx.blob_versioned_hashes.len() > host.version().max_blobs_per_tx {
        return Err(HandlerError::TooManyBlobs {
            have: tx.blob_versioned_hashes.len(),
            max: host.version().max_blobs_per_tx,
        });
    }
    if tx.blob_versioned_hashes.iter().any(|h| h[0] != VERSIONED_HASH_VERSION_KZG) {
        return Err(HandlerError::BlobVersionNotSupported);
    }
    if !tx.blob_versioned_hashes.is_empty()
        && tx.fees.max_fee_per_blob_gas < host.block.blob_basefee
    {
        return Err(HandlerError::BlobFeeCapLessThanBlobBaseFee {
            max_fee_per_blob_gas: tx.fees.max_fee_per_blob_gas,
            blob_base_fee: host.block.blob_basefee,
        });
    }
    let blob_charge = U256::from(tx.blob_versioned_hashes.len())
        .checked_mul(U256::from(DATA_GAS_PER_BLOB))
        .and_then(|v| v.checked_mul(host.block.blob_basefee))
        .ok_or(invalid("payment overflow"))?;
    let max_cost = U256::from(gas.maximum)
        .checked_mul(tx.fees.max_fee_per_gas)
        .and_then(|v| v.checked_add(blob_charge))
        .ok_or(invalid("payment overflow"))?;
    let signature_hash = tx.signature_hash();
    validate_signatures(tx, signature_hash, placeholders)?;
    if host.feature(EvmFeatures::NONCE_CHECK)
        && let Some(keys) = tx.nonce_keys.as_deref().filter(|keys| *keys != [U256::ZERO])
    {
        for key in keys {
            if host.state.storage(&NONCE_MANAGER)?.into_slot(nonce_slot(tx.sender, *key))?.current()
                != U256::from(tx.nonce)
            {
                return Err(invalid("nonce sequence mismatch"));
            }
        }
    } else if host.feature(EvmFeatures::NONCE_CHECK) {
        let expected = host.state.account(&tx.sender)?.nonce();
        if expected != tx.nonce {
            return Err(HandlerError::InvalidNonce { expected, got: tx.nonce });
        }
    }
    let legacy_nonce = host.state.account(&tx.sender)?.nonce();
    Ok(PreparedFrameTx {
        payload: Arc::new(tx.clone()),
        signature_hash,
        gas,
        gas_price,
        blob_charge,
        max_cost,
        legacy_nonce,
    })
}

fn validate_signatures(tx: &TxEip8141, hash: B256, placeholders: bool) -> HandlerResult<()> {
    for sig in &tx.signatures {
        if placeholders && sig.scheme != SignatureScheme::Arbitrary && sig.signature.is_empty() {
            continue;
        }
        let msg = if sig.signs_transaction_hash() {
            hash.0
        } else {
            sig.explicit_message().ok_or(invalid("invalid signature message"))?.0
        };
        let expected = if sig.signer.is_empty() {
            tx.sender
        } else {
            sig.signer_address().unwrap_or_default()
        };
        let valid = match sig.scheme {
            SignatureScheme::Arbitrary => sig.signer.is_empty(),
            SignatureScheme::Secp256k1 if sig.signature.len() == 65 && sig.signature[0] <= 1 => {
                let r = U256::from_be_slice(&sig.signature[1..33]);
                let s = U256::from_be_slice(&sig.signature[33..]);
                !r.is_zero()
                    && r < SECP256K1N
                    && !s.is_zero()
                    && s <= (SECP256K1N >> 1)
                    && precompiles::crypto()
                        .secp256k1_ecrecover(
                            sig.signature[1..].try_into().unwrap(),
                            sig.signature[0],
                            &msg,
                        )
                        .ok()
                        .map(|v| Address::from_slice(&v[12..]))
                        == Some(expected)
            }
            SignatureScheme::P256 if sig.signature.len() == 128 => {
                let r = U256::from_be_slice(&sig.signature[..32]);
                let s = U256::from_be_slice(&sig.signature[32..64]);
                !r.is_zero()
                    && r < SECP256R1N
                    && !s.is_zero()
                    && s <= (SECP256R1N >> 1)
                    && Address::from_slice(&keccak256(&sig.signature[64..])[12..]) == expected
                    && precompiles::crypto().secp256r1_verify_signature(
                        &msg,
                        sig.signature[..64].try_into().unwrap(),
                        sig.signature[64..].try_into().unwrap(),
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(invalid("signature validation failed"));
        }
    }
    Ok(())
}

fn validate_structure(tx: &TxEip8141, placeholders: bool) -> HandlerResult<()> {
    // Shared wire validation also applies a fixed gas schedule. Native execution must use
    // the selected Version, so validate structure here and calculate reservations separately.
    if tx.frames.is_empty() || tx.frames.len() > 64 {
        return Err(invalid("EIP-8141 frame count must be in 1..=64"));
    }
    if !alloy_eips::eip8141::has_valid_post_tx_suffix(&tx.frames) {
        return Err(invalid("EIP-7906 POST_TX frames must form a trailing suffix"));
    }
    if tx.blob_versioned_hashes.is_empty() && !tx.fees.max_fee_per_blob_gas.is_zero() {
        return Err(invalid("blob fee is non-zero without blob hashes"));
    }
    if !tx.blob_versioned_hashes.is_empty() && tx.fees.max_fee_per_blob_gas.is_zero() {
        return Err(invalid("blob hashes require a non-zero blob fee"));
    }
    let mut expiry = false;
    for (index, frame) in tx.frames.iter().enumerate() {
        let previous_atomic = index > 0 && tx.frames[index - 1].is_atomic_batch();
        if frame.has_reserved_flags() {
            return Err(invalid("reserved EIP-8141 frame flag is set"));
        }
        if frame.is_post_tx() && (frame.is_atomic_batch() || previous_atomic) {
            return Err(invalid("EIP-7906 atomic batch cannot contain a POST_TX frame"));
        }
        if frame.mode != FrameMode::Sender && !frame.value.is_zero() {
            return Err(invalid("frame value is only valid in sender mode"));
        }
        if frame.flags & 2 != 0 && frame.resolved_target(tx.sender) != tx.sender {
            return Err(invalid("execution approval target must resolve to the sender"));
        }
        if frame.is_atomic_batch()
            && (frame.mode == FrameMode::Verify
                || index + 1 == tx.frames.len()
                || tx.frames[index + 1].mode == FrameMode::Verify)
        {
            return Err(invalid("invalid atomic EIP-8141 frame"));
        }
        if (frame.is_atomic_batch() || previous_atomic) && frame.flags & 3 != 0 {
            return Err(invalid("atomic batch frames must not approve payment or execution"));
        }
        if frame.is_expiry_verifier() {
            if expiry || !frame.has_valid_expiry_verifier_fields() {
                return Err(invalid("invalid or repeated expiry verifier frame"));
            }
            expiry = true;
        }
    }
    for signature in &tx.signatures {
        // The shared message enum can be constructed directly with the reserved zero digest.
        // Its scalar/length validator does not check that invariant, including for arbitrary
        // witnesses. Simulation placeholders waive cryptography only, never message validity.
        if !signature.signs_transaction_hash()
            && signature.explicit_message().is_none_or(|message| message.is_zero())
        {
            return Err(invalid("explicit signature message must be a non-zero digest"));
        }
        if placeholders
            && signature.scheme != SignatureScheme::Arbitrary
            && signature.signature.is_empty()
        {
            continue;
        }
        signature
            .validate_structure_with_sender(tx.sender)
            .map_err(|_| invalid("invalid frame signature structure"))?;
    }
    Ok(())
}

/// Executes the already-prepared frame transaction and settles payment once.
pub fn execute_prepared<T: EvmTypes>(
    req: TxRequest<'_, '_, T, TxEip8141>,
    prepared: PreparedFrameTx,
) -> HandlerResult<TxResult<T>> {
    let host = req.host;
    let result = run(host, &prepared, None);
    host.frame_tx = None;
    result
}

fn run<T: EvmTypes>(
    host: &mut Evm<'_, T>,
    p: &PreparedFrameTx,
    prefix: Option<usize>,
) -> HandlerResult<TxResult<T>> {
    let tx = &p.payload;
    let mut runtime = FrameTransactionRuntime::with_capacity(tx.sender, tx.frames.len());
    runtime.payload = tx.clone();
    runtime.signature_hash = p.signature_hash;
    runtime.max_cost = p.max_cost;
    runtime.legacy_nonce = p.legacy_nonce;
    host.frame_tx = Some(runtime);
    host.inspect_frame(|inspector, host| inspector.frame_transaction_start(tx, host));
    host.state.account(&tx.sender)?.warm();
    if host.feature(EvmFeatures::EIP3651) {
        host.state.prewarm(&host.block.beneficiary);
    }
    host.warm_precompiles();
    let mut receipts = Vec::<FrameReceipt>::with_capacity(tx.frames.len());
    let mut outputs = Vec::with_capacity(tx.frames.len());
    let mut refunds = Vec::with_capacity(tx.frames.len());
    let mut batch = None::<Batch>;
    let mut body = None::<Batch>;
    let mut failed = false;
    let mut assertion_failure = None;
    let mut rolled_back = alloc::vec![false; tx.frames.len()];
    let mut stops = alloc::vec![InstrStop::Stop; tx.frames.len()];
    let mut i = 0;
    while i < tx.frames.len() {
        if prefix == Some(i) {
            break;
        }
        let f = &tx.frames[i];
        if f.mode == FrameMode::Sender && !host.frame_tx.as_ref().unwrap().approval.sender_approved
        {
            return Err(invalid("SENDER before execution approval"));
        }
        if batch.is_none() && f.is_atomic_batch() {
            batch = Some(checkpoint(host, receipts.len(), true));
        }
        let state = host.state.checkpoint();
        let rt = host.frame_tx.as_mut().unwrap();
        rt.current_frame_index = i;
        rt.resolved_target = f.resolved_target(tx.sender);
        rt.state_gas_used.push(0);
        let gas_checkpoint = rt.state_gas_checkpoint();
        let log_start = host.logs().len();
        host.inspect_frame(|inspector, host| inspector.frame_start(i, f, host));
        let (stop, execution, state_used, refund, output) = execute_frame(host, p, f)?;
        stops[i] = stop;
        if !stop.is_success() {
            host.state.rollback(state, host.version().features);
            host.frame_tx.as_mut().unwrap().revert_state_gas(gas_checkpoint);
        }
        host.state.clear_transient_storage();
        let logs = if stop.is_success() { host.logs()[log_start..].to_vec() } else { Vec::new() };
        let status = if stop.is_success() { FrameStatus::Success } else { FrameStatus::Failure };
        receipts.push(FrameReceipt {
            status,
            gas_used: FrameGasUsed { execution, state: state_used },
            logs,
        });
        host.inspect_frame(|inspector, host| {
            inspector.frame_end(i, receipts.last().unwrap(), &output, stop, host)
        });
        if f.mode == FrameMode::Verify && !stop.is_success() {
            return Err(invalid("VERIFY frame failed"));
        }
        outputs.push(output);
        refunds.push(refund);
        let rt = host.frame_tx.as_mut().unwrap();
        rt.statuses.push(status);
        rt.execution_gas_used.push(execution);
        rt.state_gas_used[i] = state_used;
        if stop.is_success() {
            rt.commit_root_approval();
        }
        if body.is_none() && stop.is_success() && rt.approval.payer.is_some() {
            body = Some(checkpoint(host, receipts.len(), false));
        }
        if !stop.is_success() && f.mode == FrameMode::PostTx {
            if let Some(c) = body.take() {
                rolled_back[c.start..receipts.len() - 1].fill(true);
                rollback(host, c, &mut receipts, &mut refunds, &mut outputs, true);
            }
            failed = true;
            assertion_failure = Some(i);
            for _ in i + 1..tx.frames.len() {
                skipped(host, &mut receipts, &mut refunds, &mut outputs);
            }
            break;
        }
        if !stop.is_success()
            && let Some(c) = batch.take()
        {
            rolled_back[c.start..receipts.len() - 1].fill(true);
            rollback(host, c, &mut receipts, &mut refunds, &mut outputs, false);
            while tx.frames[i].is_atomic_batch() {
                i += 1;
                skipped(host, &mut receipts, &mut refunds, &mut outputs);
            }
        } else if !f.is_atomic_batch() {
            batch = None;
        }
        if let Some(prefix_end) = prefix
            && host.frame_tx.as_ref().unwrap().approval.payer.is_some()
        {
            if receipts.len() != prefix_end
                || receipts.iter().any(|r| r.status != FrameStatus::Success)
            {
                return Err(invalid("validation prefix boundary mismatch"));
            }
            break;
        }
        i += 1;
    }
    let rt = host.frame_tx.as_ref().unwrap();
    let payer = rt.approval.payer.ok_or(invalid("missing payer"))?;
    let precharge = rt.approval.gas_pre_charge;
    for (r, state) in receipts.iter_mut().zip(&rt.state_gas_used) {
        r.gas_used.state = *state;
    }
    let state = receipts.iter().map(|r| r.gas_used.state).sum::<u64>();
    let spent =
        p.gas.intrinsic + receipts.iter().map(|r| r.gas_used.execution).sum::<u64>() + state;
    let refund = refunds.into_iter().fold(0i64, i64::saturating_add).max(0) as u64;
    let refund =
        refund.min(spent / u64::from(host.version().gas_params.get(GasId::MaxRefundQuotient)));
    let result = TxResultExt {
        status: !failed,
        total_gas_spent: spent,
        state_gas_spent: state,
        floor_gas: p.gas.floor,
        refunded: refund,
        stop: if failed { InstrStop::Revert } else { InstrStop::Return },
        frame: Some(FrameTransactionResult {
            payer,
            precharge,
            assertion_failure,
            rolled_back,
            stops,
            receipts,
            outputs,
        }),
        ..Default::default()
    };
    if prefix.is_none() && host.feature(EvmFeatures::FEE_CHARGE) {
        let used = U256::from(result.tx_gas_used());
        let actual = used
            .checked_mul(p.gas_price)
            .and_then(|v| v.checked_add(p.blob_charge))
            .ok_or(invalid("settlement overflow"))?;
        host.state.account(&payer)?.add_balance(precharge.saturating_sub(actual));
        let beneficiary = host.block.beneficiary;
        host.state
            .account(&beneficiary)?
            .add_balance(used * p.gas_price.saturating_sub(host.block.basefee));
    }
    host.inspect_frame(|inspector, host| inspector.frame_transaction_end(&result, host));
    Ok(result)
}

fn checkpoint<T: EvmTypes>(host: &Evm<'_, T>, start: usize, warmth: bool) -> Batch {
    let rt = host.frame_tx.as_ref().unwrap();
    Batch {
        state: host.state.checkpoint(),
        approval: rt.approval,
        gas: rt.state_gas_checkpoint(),
        start,
        warmth: warmth.then(|| host.state.frame_warmth_snapshot()),
    }
}

fn rollback<T: EvmTypes>(
    host: &mut Evm<'_, T>,
    c: Batch,
    receipts: &mut [FrameReceipt],
    refunds: &mut [i64],
    outputs: &mut [Bytes],
    clear_outputs: bool,
) {
    host.state.rollback(c.state, host.version().features);
    if let Some(warmth) = c.warmth {
        host.state.restore_frame_warmth(&warmth);
    }
    let rt = host.frame_tx.as_mut().unwrap();
    rt.approval = c.approval;
    rt.root_approval = None;
    rt.approval_stack.clear();
    rt.revert_state_gas(c.gas);
    for j in c.start..receipts.len().saturating_sub(1) {
        receipts[j].logs.clear();
        receipts[j].gas_used.state = 0;
        refunds[j] = 0;
        rt.state_gas_used[j] = 0;
        if clear_outputs {
            outputs[j] = Bytes::new();
        }
    }
}

fn skipped<T: EvmTypes>(
    host: &mut Evm<'_, T>,
    receipts: &mut Vec<FrameReceipt>,
    refunds: &mut Vec<i64>,
    outputs: &mut Vec<Bytes>,
) {
    let status = FrameStatus::SkippedAtomicBatch;
    receipts.push(FrameReceipt {
        status,
        gas_used: FrameGasUsed { execution: 0, state: 0 },
        logs: Vec::new(),
    });
    refunds.push(0);
    outputs.push(Bytes::new());
    let index = receipts.len() - 1;
    host.inspect_frame(|inspector, host| {
        inspector.frame_end(index, receipts.last().unwrap(), &Bytes::new(), InstrStop::Stop, host)
    });
    let rt = host.frame_tx.as_mut().unwrap();
    rt.statuses.push(status);
    rt.execution_gas_used.push(0);
    rt.state_gas_used.push(0);
}

fn execute_frame<T: EvmTypes>(
    host: &mut Evm<'_, T>,
    p: &PreparedFrameTx,
    f: &Frame,
) -> HandlerResult<(InstrStop, u64, u64, i64, Bytes)> {
    let target = f.resolved_target(p.payload.sender);
    let mut gas = GasTracker::new_isolated(f.limits.execution, f.limits.state);
    let caller = if f.mode == FrameMode::Sender { p.payload.sender } else { ENTRY_POINT };
    let run = (|| -> Result<MessageResultExt<T::MessageResultExt>, HostError> {
        let warm = host.version().gas_params.get(GasId::WarmStorageReadCost) as u64;
        let cold = host.version().gas_params.cold_account_additional_cost();
        gas.spend(warm)?;
        let load = host.load_account(&target, false, gas.remaining() < cold)?;
        if load.is_cold {
            gas.spend(cold)?;
        }
        if !f.value.is_zero() && host.state.account(&p.payload.sender)?.balance() < f.value {
            return Err(InstrStop::OutOfFunds.into());
        }
        if !f.value.is_zero() && load.is_empty {
            gas.spend_state(host.version().gas_params.new_account_state_gas())?;
        }
        let precompile = host.precompiles().contains(&target);
        let mut code = if precompile {
            Bytecode::default()
        } else {
            host.state.account(&target)?.load_code()?
        };
        let mut code_address = target;
        if !precompile && let Some(delegate) = code.eip7702_address() {
            gas.spend(warm)?;
            let load = host.load_account(&delegate, true, gas.remaining() < cold)?;
            if load.is_cold {
                gas.spend(cold)?;
            }
            code = load.code;
            code_address = delegate;
        }
        let env = TxEnvExt {
            origin: caller,
            gas_price: p.gas_price,
            chain_id: U256::from(host.version().chain_id),
            blob_hashes: p
                .payload
                .blob_versioned_hashes
                .iter()
                .map(|h| U256::from_be_bytes(h.0))
                .collect(),
            ..Default::default()
        };
        let mut message = MessageExt {
            kind: if matches!(f.mode, FrameMode::Verify | FrameMode::PostTx) {
                MessageKind::StaticCall
            } else {
                MessageKind::Call
            },
            gas_limit: gas.remaining(),
            reservoir: gas.reservoir(),
            destination: target,
            call_target: target,
            caller,
            code,
            code_address,
            input: f.data.clone(),
            value: f.value,
            ..Message::<T>::default()
        };
        Ok(host.execute_message(&env, &mut message)?)
    })();
    let (stop, state, refund, output) = match run {
        Ok(result) => {
            gas.set_remaining(result.gas.remaining());
            gas.set_reservoir(result.gas.reservoir());
            (
                result.stop,
                f.limits.state.saturating_sub(result.gas.reservoir()),
                result.gas.refunded(),
                result.output,
            )
        }
        Err(HostError::Halt(stop)) => (stop, 0, 0, Bytes::new()),
        Err(HostError::Execution(error)) => return Err(error.into()),
    };
    Ok((
        stop,
        if stop.is_halt() && stop != InstrStop::OutOfFunds {
            f.limits.execution
        } else {
            f.limits.execution - gas.remaining()
        },
        if stop.is_success() { state } else { 0 },
        if stop.is_success() { refund } else { 0 },
        output,
    ))
}

impl<T: EvmTypes<Tx = super::TxEnvelope>> Evm<'_, T> {
    /// Validates all signatures, executes only the payable prefix, and discards all effects.
    ///
    /// Prefix execution always uses the interpreter, including when full transactions use a
    /// compiled runner. This preserves per-opcode admission observations in reverted calls.
    pub fn validate_frame_prefix(
        &mut self,
        tx: &Recovered<T::Tx>,
        prefix_end: usize,
    ) -> HandlerResult<FrameValidationResult> {
        self.simulate_frames(|this| {
            let payload =
                tx.inner().as_eip8141().ok_or(invalid("prefix requires a frame transaction"))?;
            if prefix_end == 0 || prefix_end > payload.frames.len() {
                return Err(invalid("prefix outside frame bounds"));
            }
            let p = prepare_payload(this, payload)?;
            let r = run(this, &p, Some(prefix_end))?;
            let frame = r.frame.as_ref().unwrap();
            if frame.receipts.len() != prefix_end {
                return Err(invalid("validation prefix boundary mismatch"));
            }
            Ok(FrameValidationResult {
                payer: frame.payer,
                max_cost: p.max_cost,
                prefix_end,
                sender_approved: this.frame_tx.as_ref().unwrap().approval.sender_approved,
                signature_gas: p.gas.signatures,
                execution_gas: frame.receipts.iter().map(|r| r.gas_used.execution).sum(),
                state_gas: r.state_gas_spent,
            })
        })
    }
}
