//! Static frame validation and first-error ordering before untrusted database access.

mod common;

use alloy_consensus::TxEip8141;
use alloy_eips::eip8141::{
    EXPIRY_DATA_LENGTH, EXPIRY_VERIFIER, Frame, FrameLimits, FrameMode, FrameSignature, SECP256K1N,
    SECP256R1N, SignatureMessage, SignatureScheme, validate_nonce_keys,
};
use alloy_primitives::{Address, B256, U256};
use common::{BODY, FailingDatabase, SENDER, recovered};
use evm2::{
    BaseEvmTypes, Evm, EvmFeatures, ExecutionConfig, Precompiles, SpecId, Version,
    env::BlockEnvExt,
    ethereum::{eip8141::FrameGas, ethereum_tx_registry},
    evm::Db,
    registry::HandlerError,
};
use k256::ecdsa::SigningKey;
use p256::ecdsa::{
    Signature as P256Signature, SigningKey as P256SigningKey, signature::hazmat::PrehashSigner,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn frame(mode: FrameMode, target: Address, flags: u8) -> Frame {
    Frame { flags, ..common::frame(mode, target) }
}

fn payload() -> TxEip8141 {
    common::payload(vec![frame(FrameMode::Verify, SENDER, 3), frame(FrameMode::Sender, BODY, 0)])
}

fn evm(version: Version, block: BlockEnvExt) -> (Evm<'static, BaseEvmTypes>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let evm = Evm::new_with_execution_config(
        ExecutionConfig::for_spec_and_version(SpecId::BOGOTA, version),
        SpecId::BOGOTA,
        block,
        ethereum_tx_registry(SpecId::BOGOTA),
        Db::new(FailingDatabase(reads.clone(), "validation performed a database read")),
        Precompiles::base(SpecId::BOGOTA),
    );
    (evm, reads)
}

const fn invalid(reason: &'static str) -> HandlerError {
    HandlerError::InvalidFrameTransaction(reason)
}

fn blob_hash() -> B256 {
    let mut hash = B256::ZERO;
    hash[0] = 1;
    hash
}

fn assert_static_error(name: &str, tx: TxEip8141, expected: HandlerError) {
    for prefix in [false, true].into_iter().filter(|prefix| !prefix || !tx.frames.is_empty()) {
        let (mut evm, reads) = evm(Version::new(SpecId::BOGOTA), BlockEnvExt::default());
        let tx = recovered(tx.clone());
        let error = if prefix {
            evm.validate_frame_prefix(&tx, 1).unwrap_err()
        } else {
            evm.validate_tx(&tx).unwrap_err()
        };
        assert_eq!(error, expected, "{name}, prefix={prefix}");
        assert_eq!(reads.load(Ordering::Relaxed), 0, "{name}, prefix={prefix}");
        assert!(evm.logs().is_empty());
    }
}

#[test]
fn malformed_frame_structure_and_nonce_schemas_are_rejected_without_reads() {
    type Case = (&'static str, fn(&mut TxEip8141), &'static str);
    let cases: &[Case] = &[
        ("no frames", |tx| tx.frames.clear(), "EIP-8141 frame count must be in 1..=64"),
        (
            "65 frames",
            |tx| tx.frames.resize(65, frame(FrameMode::Default, BODY, 0)),
            "EIP-8141 frame count must be in 1..=64",
        ),
        (
            "assert suffix",
            |tx| tx.frames[0].mode = FrameMode::Assert,
            "EIP-7906 ASSERT frames must form a trailing suffix",
        ),
        ("reserved flags", |tx| tx.frames[1].flags = 0x80, "reserved EIP-8141 frame flag is set"),
        (
            "verify value",
            |tx| tx.frames[0].value = U256::from(1),
            "frame value is only valid in sender mode",
        ),
        (
            "default value",
            |tx| {
                tx.frames[1].mode = FrameMode::Default;
                tx.frames[1].value = U256::from(1);
            },
            "frame value is only valid in sender mode",
        ),
        (
            "approval target",
            |tx| tx.frames[1].flags = 2,
            "execution approval target must resolve to the sender",
        ),
        ("verify atomic", |tx| tx.frames[0].flags |= 4, "invalid atomic EIP-8141 frame"),
        ("atomic end", |tx| tx.frames[1].flags = 4, "invalid atomic EIP-8141 frame"),
        (
            "atomic before verify",
            |tx| {
                tx.frames[1].flags = 4;
                tx.frames.push(frame(FrameMode::Verify, SENDER, 0));
            },
            "invalid atomic EIP-8141 frame",
        ),
        (
            "atomic approval",
            |tx| {
                tx.frames[1].flags = 5;
                tx.frames.push(frame(FrameMode::Sender, BODY, 0));
            },
            "atomic batch frames must not approve payment or execution",
        ),
        (
            "atomic successor approval",
            |tx| {
                tx.frames[1].flags = 4;
                tx.frames.push(frame(FrameMode::Default, BODY, 1));
            },
            "atomic batch frames must not approve payment or execution",
        ),
        (
            "atomic assert successor",
            |tx| {
                tx.frames[1].flags = 4;
                tx.frames.push(frame(FrameMode::Assert, BODY, 0));
            },
            "EIP-7906 atomic batch cannot contain an ASSERT frame",
        ),
        (
            "assert atomic",
            |tx| {
                tx.frames[1].mode = FrameMode::Assert;
                tx.frames[1].flags = 4;
            },
            "EIP-7906 atomic batch cannot contain an ASSERT frame",
        ),
        (
            "expiry fields",
            |tx| tx.frames.push(frame(FrameMode::Verify, EXPIRY_VERIFIER, 0)),
            "invalid or repeated expiry verifier frame",
        ),
        (
            "expiry repeated",
            |tx| {
                let mut expiry = frame(FrameMode::Verify, EXPIRY_VERIFIER, 0);
                expiry.limits.state = 0;
                expiry.data = vec![0; EXPIRY_DATA_LENGTH].into();
                tx.frames.extend([expiry.clone(), expiry]);
            },
            "invalid or repeated expiry verifier frame",
        ),
        (
            "expiry approval flags",
            |tx| {
                let mut expiry = valid_expiry();
                expiry.flags = 1;
                tx.frames.push(expiry);
            },
            "invalid or repeated expiry verifier frame",
        ),
        (
            "expiry short data",
            |tx| {
                let mut expiry = valid_expiry();
                expiry.data = vec![0; EXPIRY_DATA_LENGTH - 1].into();
                tx.frames.push(expiry);
            },
            "invalid or repeated expiry verifier frame",
        ),
        (
            "expiry long data",
            |tx| {
                let mut expiry = valid_expiry();
                expiry.data = vec![0; EXPIRY_DATA_LENGTH + 1].into();
                tx.frames.push(expiry);
            },
            "invalid or repeated expiry verifier frame",
        ),
        (
            "expiry state budget",
            |tx| {
                let mut expiry = valid_expiry();
                expiry.limits.state = 1;
                tx.frames.push(expiry);
            },
            "invalid or repeated expiry verifier frame",
        ),
        (
            "assert value",
            |tx| {
                tx.frames[1].mode = FrameMode::Assert;
                tx.frames[1].value = U256::from(1);
            },
            "frame value is only valid in sender mode",
        ),
        (
            "keyed schema absent",
            |tx| tx.nonce_keys = None,
            "nonce payload does not match activation",
        ),
        (
            "17 nonce keys",
            |tx| tx.nonce_keys = Some((1..=17).map(U256::from).collect()),
            validate_nonce_keys(&(1..=17).map(U256::from).collect::<Vec<_>>()).unwrap_err(),
        ),
        (
            "duplicate keys",
            |tx| tx.nonce_keys = Some(vec![U256::from(1), U256::from(1)]),
            "EIP-8250 nonce keys must be strictly increasing",
        ),
        (
            "descending keys",
            |tx| tx.nonce_keys = Some(vec![U256::from(2), U256::from(1)]),
            "EIP-8250 nonce keys must be strictly increasing",
        ),
    ];
    for (name, mutate, reason) in cases {
        let mut tx = payload();
        mutate(&mut tx);
        assert_static_error(name, tx, invalid(reason));
    }
    let mut tx = payload();
    tx.nonce = u64::MAX;
    assert_static_error("sequence overflow", tx, HandlerError::NonceOverflow);
    let mut tx = payload();
    tx.frames[1].limits.execution = u64::MAX;
    assert_static_error("execution sum overflow", tx, invalid("gas arithmetic overflow"));
    let mut tx = payload();
    tx.frames[1].limits.state = u64::MAX;
    assert_static_error("state sum overflow", tx, invalid("gas arithmetic overflow"));
    let mut tx = payload();
    tx.frames[0].limits.state = 0;
    tx.frames[1].limits.state = u64::MAX - 100_000;
    assert_static_error("combined reservation overflow", tx, invalid("gas arithmetic overflow"));
}

#[test]
fn every_signature_is_validated_before_account_or_nonce_reads() {
    let mut signatures = vec![
        (
            "secp length",
            FrameSignature {
                scheme: SignatureScheme::Secp256k1,
                signature: vec![0; 64].into(),
                ..Default::default()
            },
        ),
        (
            "p256 length",
            FrameSignature {
                scheme: SignatureScheme::P256,
                signature: vec![0; 127].into(),
                ..Default::default()
            },
        ),
        ("arbitrary signer", FrameSignature { signer: SENDER.into(), ..Default::default() }),
        (
            "zero explicit message",
            FrameSignature { msg: SignatureMessage::Explicit(B256::ZERO), ..Default::default() },
        ),
    ];
    let mut secp = vec![0; 65];
    secp[0] = 2;
    secp[32] = 1;
    secp[64] = 1;
    signatures.push((
        "secp recovery bit",
        FrameSignature {
            scheme: SignatureScheme::Secp256k1,
            signature: secp.into(),
            ..Default::default()
        },
    ));
    let mut high_s = vec![0; 65];
    high_s[32] = 1;
    high_s[33..].copy_from_slice(&((SECP256K1N >> 1usize) + U256::from(1)).to_be_bytes::<32>());
    signatures.push((
        "secp high s",
        FrameSignature {
            scheme: SignatureScheme::Secp256k1,
            signature: high_s.into(),
            ..Default::default()
        },
    ));
    for (scheme, order, length, offset) in [
        (SignatureScheme::Secp256k1, SECP256K1N, 65, 1),
        (SignatureScheme::P256, SECP256R1N, 128, 0),
    ] {
        for (name, r, s) in [
            ("zero r", U256::ZERO, U256::from(1)),
            ("out of range r", order, U256::from(1)),
            ("zero s", U256::from(1), U256::ZERO),
            ("high s", U256::from(1), (order >> 1usize) + U256::from(1)),
        ] {
            let mut bytes = vec![0; length];
            bytes[offset..offset + 32].copy_from_slice(&r.to_be_bytes::<32>());
            bytes[offset + 32..offset + 64].copy_from_slice(&s.to_be_bytes::<32>());
            signatures.push((
                name,
                FrameSignature { scheme, signature: bytes.into(), ..Default::default() },
            ));
        }
    }
    let mut p256 = vec![0; 128];
    p256[31] = 1;
    p256[63] = 1;
    signatures.push((
        "p256 public key signer mismatch",
        FrameSignature {
            scheme: SignatureScheme::P256,
            signature: p256.into(),
            ..Default::default()
        },
    ));
    for (name, signature) in signatures {
        let mut tx = payload();
        tx.signatures = vec![FrameSignature::default(), signature];
        let reason = if name == "zero explicit message" {
            "explicit signature message must be a non-zero digest"
        } else {
            "invalid frame signature structure"
        };
        assert_static_error(name, tx, invalid(reason));
    }
    // Structurally canonical, cryptographically valid signature for a different signer.
    let mut tx = payload();
    let key = SigningKey::from_slice(&[0x11; 32]).unwrap();
    let message = B256::with_last_byte(1);
    let (signature, recovery) = key.sign_prehash_recoverable(message.as_slice());
    let mut bytes = vec![recovery.to_byte()];
    bytes.extend_from_slice(&signature.to_bytes());
    tx.signatures.push(FrameSignature {
        scheme: SignatureScheme::Secp256k1,
        msg: SignatureMessage::Explicit(message),
        signature: bytes.into(),
        ..Default::default()
    });
    assert_static_error("wrong recovered signer", tx, invalid("signature validation failed"));
}

#[test]
fn resource_fee_and_blob_validation_precedes_database_reads() {
    let mut cases = Vec::new();
    let version = Version::new(SpecId::BOGOTA);
    let block = BlockEnvExt::default();
    let tx = payload();
    let gas = FrameGas::calculate(&tx, &version).unwrap();
    let mut capped = version;
    capped.tx_gas_limit_cap = gas.execution - 1;
    cases.push((
        "active execution cap",
        tx.clone(),
        capped,
        block,
        HandlerError::TxGasLimitGreaterThanCap { gas_limit: gas.execution, cap: gas.execution - 1 },
    ));
    let mut small_block = block;
    small_block.gas_limit = U256::from(1);
    cases.push((
        "independent block dimensions",
        tx.clone(),
        version,
        small_block,
        HandlerError::GasLimitMoreThanBlock {
            gas_limit: gas.execution.max(gas.state),
            block_gas_limit: U256::from(1),
        },
    ));
    let mut chain = tx.clone();
    chain.chain_id = 2;
    cases.push((
        "chain",
        chain,
        version,
        block,
        HandlerError::InvalidChainId { expected: 1, got: 2 },
    ));
    let mut priced_block = block;
    priced_block.basefee = U256::from(10);
    cases.push((
        "base-fee budget",
        tx.clone(),
        version,
        priced_block,
        HandlerError::FrameFeeBudgetTooLow {
            required: U256::from(gas.maximum) * U256::from(10),
            got: tx.fees.max_fee,
        },
    ));
    let mut blobs = tx.clone();
    blobs.blob_versioned_hashes = vec![blob_hash(); version.max_blobs_per_tx + 1];
    cases.push((
        "blob count",
        blobs,
        version,
        block,
        HandlerError::TooManyBlobs {
            have: version.max_blobs_per_tx + 1,
            max: version.max_blobs_per_tx,
        },
    ));
    let mut blob_version = tx.clone();
    blob_version.blob_versioned_hashes.push(B256::ZERO);
    cases.push((
        "blob version",
        blob_version,
        version,
        block,
        HandlerError::BlobVersionNotSupported,
    ));
    let mut blob_fee = tx.clone();
    blob_fee.blob_versioned_hashes.push(blob_hash());
    blob_fee.fees.max_fee = U256::from(2 * 131_072 - 1);
    let mut blob_block = block;
    blob_block.blob_basefee = U256::from(2);
    cases.push((
        "blob base-fee budget",
        blob_fee,
        version,
        blob_block,
        HandlerError::FrameFeeBudgetTooLow {
            required: U256::from(2 * 131_072),
            got: U256::from(2 * 131_072 - 1),
        },
    ));
    let mut overflow = tx;
    overflow.fees.max_fee = U256::MAX;
    let mut expensive_execution = block;
    expensive_execution.basefee = U256::MAX;
    cases.push((
        "payment overflow",
        overflow,
        version,
        expensive_execution,
        invalid("payment overflow"),
    ));
    let mut blobs = payload();
    blobs.blob_versioned_hashes.push(blob_hash());
    blobs.fees.max_fee = U256::MAX;
    let mut expensive_blobs = block;
    expensive_blobs.blob_basefee = U256::MAX;
    cases.push((
        "blob charge overflow",
        blobs,
        version,
        expensive_blobs,
        invalid("payment overflow"),
    ));
    for (name, tx, version, block, expected) in cases {
        let (mut evm, reads) = evm(version, block);
        assert_eq!(evm.validate_tx(&recovered(tx)).unwrap_err(), expected, "{name}");
        assert_eq!(reads.load(Ordering::Relaxed), 0, "{name}");
    }
}

#[test]
fn competing_errors_follow_static_preparation_order_and_valid_payload_reaches_db() {
    let mut tx = payload();
    tx.frames.clear();
    tx.nonce_keys = Some((1..=17).map(U256::from).collect());
    assert_static_error(
        "frame count before nonce keys",
        tx,
        invalid("EIP-8141 frame count must be in 1..=64"),
    );
    let mut tx = payload();
    tx.frames[1].flags = 0x80;
    tx.nonce_keys = Some((1..=17).map(U256::from).collect());
    assert_static_error(
        "structure before nonce keys",
        tx,
        invalid("reserved EIP-8141 frame flag is set"),
    );
    let mut tx = payload();
    tx.nonce_keys = Some((1..=17).map(U256::from).collect());
    tx.nonce = u64::MAX;
    assert_static_error(
        "keys before sequence",
        tx,
        invalid(validate_nonce_keys(&(1..=17).map(U256::from).collect::<Vec<_>>()).unwrap_err()),
    );
    let mut tx = payload();
    tx.nonce = u64::MAX;
    tx.frames[1].limits.execution = u64::MAX;
    assert_static_error("sequence before gas arithmetic", tx, HandlerError::NonceOverflow);
    let mut tx = payload();
    tx.chain_id = 2;
    tx.fees.max_priority_fee_per_gas = U256::from(1);
    assert_static_error(
        "chain before priority",
        tx,
        HandlerError::InvalidChainId { expected: 1, got: 2 },
    );
    let mut tx = payload();
    tx.chain_id = 2;
    tx.signatures.push(FrameSignature { scheme: SignatureScheme::P256, ..Default::default() });
    assert_static_error(
        "signature shape before chain",
        tx,
        invalid("invalid frame signature structure"),
    );
    let (mut evm, reads) = evm(Version::new(SpecId::BOGOTA), BlockEnvExt::default());
    assert!(
        matches!(evm.validate_tx(&recovered(payload())), Err(HandlerError::Database(error)) if error.is_fatal())
    );
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn activation_and_signature_placeholder_controls_do_not_relax_static_shape() {
    let mut version = Version::new(SpecId::BOGOTA);
    version.features.remove(EvmFeatures::EIP8141);
    let (mut inactive, reads) = evm(version, BlockEnvExt::default());
    assert_eq!(
        inactive.validate_tx(&recovered(payload())).unwrap_err(),
        HandlerError::UnsupportedTransactionType(6)
    );
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let mut version = Version::new(SpecId::BOGOTA);
    version.features.remove(EvmFeatures::EIP8250);
    let (mut inactive, reads) = evm(version, BlockEnvExt::default());
    assert_eq!(
        inactive.validate_tx(&recovered(payload())).unwrap_err(),
        invalid("nonce payload does not match activation")
    );
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let mut version = Version::new(SpecId::BOGOTA);
    version.features.insert(EvmFeatures::FRAME_SIGNATURE_PLACEHOLDERS);
    let (mut evm, reads) = evm(version, BlockEnvExt::default());
    let mut tx = payload();
    tx.signatures.push(FrameSignature { scheme: SignatureScheme::Secp256k1, ..Default::default() });
    assert!(matches!(evm.validate_tx(&recovered(tx.clone())), Err(HandlerError::Database(_))));
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    tx.signatures[0].msg = SignatureMessage::Explicit(B256::ZERO);
    assert_eq!(
        evm.validate_tx(&recovered(tx)).unwrap_err(),
        invalid("explicit signature message must be a non-zero digest")
    );
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn valid_structure_boundaries_reach_database_validation() {
    let version = Version::new(SpecId::BOGOTA);
    let mut cases = vec![("baseline", payload(), version)];
    let mut tx = payload();
    tx.frames.truncate(1);
    cases.push(("one frame", tx, version));
    let mut tx = payload();
    tx.frames.resize(64, frame(FrameMode::Sender, BODY, 0));
    cases.push(("64 frames", tx, version));
    let mut tx = payload();
    tx.nonce_keys = Some((1..=16).map(U256::from).collect());
    cases.push(("16 keys", tx, version));
    let mut tx = payload();
    tx.nonce_keys = Some(vec![U256::ZERO]);
    cases.push(("zero keyed domain", tx, version));
    let mut tx = payload();
    tx.nonce_keys = Some(vec![U256::ZERO, U256::from(1)]);
    cases.push(("zero mixed with another key", tx, version));
    let mut tx = payload();
    tx.nonce_keys = Some(vec![U256::ZERO]);
    tx.nonce = u64::MAX;
    cases.push(("single-use zero keyed domain", tx, version));
    let mut tx = payload();
    tx.nonce_keys = None;
    let mut legacy = version;
    legacy.features.remove(EvmFeatures::EIP8250);
    cases.push(("pre-keyed schema", tx, legacy));
    let mut tx = payload();
    tx.frames[0].target = Default::default();
    cases.push(("implicit sender approval target", tx, version));
    let mut tx = payload();
    tx.nonce = u64::MAX - 1;
    cases.push(("last incrementable sequence", tx, version));
    let mut tx = payload();
    tx.frames[1].limits = FrameLimits::default();
    cases.push(("zero frame budgets", tx, version));
    let mut tx = payload();
    let mut expiry = frame(FrameMode::Verify, EXPIRY_VERIFIER, 0);
    expiry.limits.state = 0;
    expiry.data = vec![0; EXPIRY_DATA_LENGTH].into();
    tx.frames.push(expiry);
    cases.push(("constrained expiry", tx, version));
    let mut tx = payload();
    tx.blob_versioned_hashes = vec![blob_hash(); version.max_blobs_per_tx];
    tx.fees.max_fee = U256::MAX;
    cases.push(("maximum blob count", tx, version));
    let mut tx = payload();
    tx.fees.max_fee = U256::MAX;
    cases.push(("maximum aggregate budget", tx, version));
    let mut tx = payload();
    tx.fees.max_priority_fee_per_gas = U256::MAX;
    cases.push(("maximum priority with zero budget", tx, version));
    for (name, tx, version) in cases {
        let (mut evm, reads) = evm(version, BlockEnvExt::default());
        assert!(
            matches!(evm.validate_tx(&recovered(tx)), Err(HandlerError::Database(error)) if error.is_fatal()),
            "{name}"
        );
        assert_eq!(reads.load(Ordering::Relaxed), 1, "{name}");
    }
}

#[test]
fn native_p256_verification_accepts_a_valid_signature_and_rejects_invalid_crypto_before_reads() {
    let key = P256SigningKey::from_slice(&[0x22; 32]).unwrap();
    let message = B256::with_last_byte(1);
    let signature: P256Signature = key.sign_prehash(message.as_slice()).unwrap();
    let signature = signature.normalize_s();
    let public = key.verifying_key().to_sec1_point(false);
    let mut bytes = signature.to_bytes().to_vec();
    bytes.extend_from_slice(&public.as_bytes()[1..]);
    let mut tx = payload();
    tx.signatures = vec![
        FrameSignature::default(),
        FrameSignature {
            scheme: SignatureScheme::P256,
            signer: Address::from_raw_public_key(&public.as_bytes()[1..]).into(),
            msg: SignatureMessage::Explicit(message),
            signature: bytes.into(),
        },
    ];
    let (mut evm, reads) = evm(Version::new(SpecId::BOGOTA), BlockEnvExt::default());
    assert!(matches!(evm.validate_tx(&recovered(tx.clone())), Err(HandlerError::Database(_))));
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    // Preserve canonical scalars and the signer/public-key match while invalidating the signature.
    let mut invalid_bytes = tx.signatures[1].signature.to_vec();
    invalid_bytes[..32].fill(0);
    invalid_bytes[31] = 1;
    tx.signatures[1].signature = invalid_bytes.into();
    assert_static_error("p256 invalid crypto", tx, invalid("signature validation failed"));
}

#[test]
fn prefix_bounds_are_checked_before_payload_preparation_or_database_access() {
    for (tx, boundary) in [
        (payload(), 0),
        (payload(), 3),
        (
            {
                let mut tx = payload();
                tx.frames.clear();
                tx
            },
            1,
        ),
    ] {
        let (mut evm, reads) = evm(Version::new(SpecId::BOGOTA), BlockEnvExt::default());
        assert_eq!(
            evm.validate_frame_prefix(&recovered(tx), boundary).unwrap_err(),
            invalid("prefix outside frame bounds")
        );
        assert_eq!(reads.load(Ordering::Relaxed), 0);
    }
}

fn valid_expiry() -> Frame {
    let mut expiry = frame(FrameMode::Verify, EXPIRY_VERIFIER, 0);
    expiry.limits.state = 0;
    expiry.data = vec![0; EXPIRY_DATA_LENGTH].into();
    expiry
}
