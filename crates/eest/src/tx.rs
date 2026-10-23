use alloy_consensus::{
    EthereumTxEnvelope, Transaction, TxEip4844, TxEip8141, TypedTransaction,
    transaction::{Recovered, SignerRecoverable},
};
use alloy_eips::{
    Decodable2718, Typed2718,
    eip7702::SignedAuthorization,
    eip8141::{
        Frame, FrameAddress, FrameLimits, FrameMode, FrameSignature, SignatureMessage,
        SignatureScheme, TransactionFees, nonce_calldata,
    },
};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use alloy_rpc_types_eth::{
    AccessList as RpcAccessList, AccessListItem as RpcAccessListItem, TransactionInput,
    TransactionRequest,
};
use evm2::ethereum::{RecoveredTxEnvelope, TxEnvelope};
use k256::ecdsa::SigningKey;
use revm::{
    context::TxEnv,
    context_interface::{either::Either, transaction::FrameTransaction},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Access list entry shared by EEST fixture formats.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessListItem {
    /// Accessed account.
    pub address: Address,
    /// Accessed storage keys.
    pub storage_keys: Vec<B256>,
}

/// EIP-7702 authorization entry shared by EEST fixture formats.
#[derive(Clone, Debug)]
pub struct TestAuthorization {
    /// Raw authorization JSON.
    pub value: serde_json::Value,
}

/// A frame in the pinned execution-specs fixture schema. Limits remain full-width until conversion.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFrame {
    /// Frame execution mode.
    pub mode: U256,
    /// Approval scope and atomic link flags.
    pub flags: U256,
    /// Empty or explicit target bytes.
    pub target: Option<Bytes>,
    /// Execution budget.
    pub gas_limit: U256,
    /// State budget.
    pub state_gas_limit: U256,
    /// Transfer value.
    pub value: U256,
    /// Frame input.
    pub data: Bytes,
}

/// Raw signature metadata in the pinned execution-specs fixture schema.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFrameSignature {
    /// Signature scheme, retained full-width until validation.
    pub scheme: U256,
    /// Empty or explicit signer bytes.
    pub signer: Bytes,
    /// Empty or explicit signed digest bytes.
    pub msg: Bytes,
    /// Protocol signature or arbitrary witness.
    pub signature: Bytes,
}

impl Serialize for TestAuthorization {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TestAuthorization {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        normalize_authorization_value(&mut value);
        Ok(Self { value })
    }
}

fn normalize_authorization_value(value: &mut serde_json::Value) {
    if let Some(object) = value.as_object_mut()
        && object.contains_key("v")
        && object.contains_key("yParity")
    {
        object.remove("v");
    }
}

/// Error while building a consensus transaction from fixture fields.
#[derive(Debug, thiserror::Error)]
pub enum TxBuildError {
    /// Numeric value overflowed the target type.
    #[error("value overflows {0}")]
    Overflow(&'static str),
    /// Transaction request could not be converted to a consensus transaction.
    #[error("could not build consensus transaction: {0}")]
    BuildTransaction(String),
    /// JSON decoding failed.
    #[error(transparent)]
    SerdeDeserialize(#[from] serde_json::Error),
}

/// Common transaction fields already selected from a fixture.
pub(crate) struct TxFields {
    /// Explicit transaction type.
    pub(crate) tx_type: Option<u8>,
    /// Recovered or explicit sender.
    pub(crate) caller: Address,
    /// Transaction recipient, or create.
    pub(crate) kind: TxKind,
    /// Transaction calldata.
    pub(crate) data: Bytes,
    /// Gas limit.
    pub(crate) gas_limit: u64,
    /// Transaction nonce.
    pub(crate) nonce: u64,
    /// Ether value.
    pub(crate) value: U256,
    /// Optional chain ID.
    pub(crate) chain_id: Option<U256>,
    /// Legacy gas price.
    pub(crate) gas_price: Option<U256>,
    /// EIP-1559 max fee.
    pub(crate) max_fee_per_gas: Option<U256>,
    /// EIP-8141 total signed fee budget.
    pub(crate) max_fee: Option<U256>,
    /// EIP-1559 priority fee.
    pub(crate) max_priority_fee_per_gas: Option<U256>,
    /// EIP-2930 access list.
    pub(crate) access_list: Option<RpcAccessList>,
    /// EIP-7702 authorization list.
    pub(crate) authorization_list: Option<Vec<SignedAuthorization>>,
    /// EIP-4844 blob hashes.
    pub(crate) blob_versioned_hashes: Vec<B256>,
    /// EIP-4844 max fee per blob gas.
    pub(crate) max_fee_per_blob_gas: Option<U256>,
    /// Ordered native frame payload, if type 6.
    pub(crate) frames: Option<Vec<TestFrame>>,
    /// Signature entries for a frame transaction.
    pub(crate) signatures: Vec<TestFrameSignature>,
    /// Optional keyed nonce schema.
    pub(crate) nonce_keys: Option<Vec<U256>>,
}

/// Builds an evm2 recovered transaction envelope from common fixture fields.
pub(crate) fn build_recovered_tx(fields: TxFields) -> Result<RecoveredTxEnvelope, TxBuildError> {
    let caller = fields.caller;
    let tx = if fields.tx_type == Some(6) || fields.frames.is_some() {
        TypedTransaction::Eip8141(build_frame_tx(fields)?)
    } else {
        build_request_tx(fields)?
    };
    Ok(recovered_envelope(tx, caller))
}

fn build_request_tx(fields: TxFields) -> Result<TypedTransaction, TxBuildError> {
    let mut request = TransactionRequest::default()
        .from(fields.caller)
        .gas_limit(fields.gas_limit)
        .nonce(fields.nonce)
        .value(fields.value)
        .input(TransactionInput::from(fields.data));
    request.to = Some(fields.kind);
    request.transaction_type = fields.tx_type;
    request.chain_id = fields
        .chain_id
        .map(TryInto::try_into)
        .transpose()
        .map_err(|_| TxBuildError::Overflow("chainId"))?;
    if !matches!(fields.tx_type, Some(2..=4)) {
        request.gas_price = fields
            .gas_price
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| TxBuildError::Overflow("gasPrice"))?;
        if request.gas_price.is_none()
            && (matches!(fields.tx_type, Some(0 | 1))
                || (fields.max_fee_per_gas.is_none() && fields.max_priority_fee_per_gas.is_none()))
        {
            request.gas_price = Some(0);
        }
    }
    request.max_fee_per_gas = fields
        .max_fee_per_gas
        .map(TryInto::try_into)
        .transpose()
        .map_err(|_| TxBuildError::Overflow("maxFeePerGas"))?;
    request.max_priority_fee_per_gas =
        if fields.max_fee_per_gas.is_some() && fields.max_priority_fee_per_gas.is_none() {
            Some(0)
        } else {
            fields
                .max_priority_fee_per_gas
                .map(TryInto::try_into)
                .transpose()
                .map_err(|_| TxBuildError::Overflow("maxPriorityFeePerGas"))?
        };
    request.max_fee_per_blob_gas = fields
        .max_fee_per_blob_gas
        .map(TryInto::try_into)
        .transpose()
        .map_err(|_| TxBuildError::Overflow("maxFeePerBlobGas"))?;
    request.access_list = fields.access_list;
    request.authorization_list = fields.authorization_list;
    if fields.max_fee_per_blob_gas.is_some()
        || matches!(fields.tx_type, Some(3))
        || !fields.blob_versioned_hashes.is_empty()
    {
        request.blob_versioned_hashes = Some(fields.blob_versioned_hashes);
    }

    request.build_consensus_tx().map_err(|err| TxBuildError::BuildTransaction(err.error))
}

fn build_frame_tx(fields: TxFields) -> Result<TxEip8141, TxBuildError> {
    let frames = fields
        .frames
        .ok_or_else(|| TxBuildError::BuildTransaction("type 6 requires frames".into()))?
        .into_iter()
        .map(Frame::try_from)
        .collect::<Result<_, TxBuildError>>()?;
    let signatures =
        fields.signatures.into_iter().map(FrameSignature::try_from).collect::<Result<_, _>>()?;
    Ok(TxEip8141 {
        chain_id: fields
            .chain_id
            .unwrap_or(U256::ONE)
            .try_into()
            .map_err(|_| TxBuildError::Overflow("chainId"))?,
        nonce: fields.nonce,
        nonce_keys: fields.nonce_keys,
        sender: fields.caller,
        frames,
        signatures,
        fees: TransactionFees {
            max_fee: fields.max_fee.unwrap_or_default(),
            max_priority_fee_per_gas: fields.max_priority_fee_per_gas.unwrap_or_default(),
        },
        blob_versioned_hashes: fields.blob_versioned_hashes,
    })
}

impl TryFrom<TestFrame> for Frame {
    type Error = TxBuildError;

    fn try_from(frame: TestFrame) -> Result<Self, Self::Error> {
        Ok(Self {
            mode: u8::try_from(frame.mode)
                .ok()
                .and_then(FrameMode::try_from_u8)
                .ok_or_else(|| TxBuildError::BuildTransaction("invalid frame mode".into()))?,
            target: FrameAddress::try_from(
                frame.target.as_ref().map_or(&[][..], |target| target.as_ref()),
            )
            .map_err(|error| TxBuildError::BuildTransaction(error.to_string()))?,
            flags: frame.flags.try_into().map_err(|_| TxBuildError::Overflow("frame flags"))?,
            limits: FrameLimits {
                execution: frame
                    .gas_limit
                    .try_into()
                    .map_err(|_| TxBuildError::Overflow("frame execution gas"))?,
                state: frame
                    .state_gas_limit
                    .try_into()
                    .map_err(|_| TxBuildError::Overflow("frame state gas"))?,
            },
            value: frame.value,
            data: frame.data,
        })
    }
}

impl TryFrom<TestFrameSignature> for FrameSignature {
    type Error = TxBuildError;

    fn try_from(signature: TestFrameSignature) -> Result<Self, Self::Error> {
        Ok(Self {
            scheme: u8::try_from(signature.scheme)
                .ok()
                .and_then(|scheme| SignatureScheme::try_from(scheme).ok())
                .ok_or_else(|| TxBuildError::BuildTransaction("invalid signature scheme".into()))?,
            signer: FrameAddress::try_from(signature.signer.as_ref())
                .map_err(|error| TxBuildError::BuildTransaction(error.to_string()))?,
            msg: SignatureMessage::try_from(signature.msg.as_ref())
                .map_err(|error| TxBuildError::BuildTransaction(error.to_string()))?,
            signature: signature.signature,
        })
    }
}

/// Decodes canonical transaction bytes, including self-authorized frame envelopes.
pub(crate) fn build_recovered_tx_bytes(bytes: &[u8]) -> Result<RecoveredTxEnvelope, TxBuildError> {
    let mut input = bytes;
    let envelope = EthereumTxEnvelope::<TxEip4844>::decode_2718(&mut input)
        .map_err(|error| TxBuildError::BuildTransaction(error.to_string()))?;
    if !input.is_empty() {
        return Err(TxBuildError::BuildTransaction("trailing transaction bytes".into()));
    }
    let sender = envelope
        .recover_signer()
        .map_err(|error| TxBuildError::BuildTransaction(error.to_string()))?;
    Ok(Recovered::new_unchecked(envelope.into(), sender))
}

/// Converts EEST access list items into RPC access list items.
pub(crate) fn rpc_access_list<'a>(
    items: impl IntoIterator<Item = &'a AccessListItem>,
) -> RpcAccessList {
    RpcAccessList(items.into_iter().map(rpc_access_list_item).collect())
}

fn rpc_access_list_item(item: &AccessListItem) -> RpcAccessListItem {
    RpcAccessListItem { address: item.address, storage_keys: item.storage_keys.clone() }
}

/// Deserializes signed authorizations from their raw fixture JSON form.
pub(crate) fn signed_authorizations(
    raw: Option<&[TestAuthorization]>,
) -> Result<Option<Vec<SignedAuthorization>>, TxBuildError> {
    let Some(authorizations) = raw else {
        return Ok(None);
    };
    let authorizations = authorizations
        .iter()
        .map(|authorization| serde_json::from_value(authorization.value.clone()))
        .collect::<Result<_, _>>()?;
    Ok(Some(authorizations))
}

/// Recovers an address from a private key.
pub(crate) fn recover_address(private_key: &[u8]) -> Option<Address> {
    let key = SigningKey::from_slice(private_key).ok()?;
    let public_key = key.verifying_key().to_sec1_point(false);
    Some(Address::from_raw_public_key(&public_key.as_bytes()[1..]))
}

fn recovered_envelope(tx: TypedTransaction, caller: Address) -> RecoveredTxEnvelope {
    match tx {
        TypedTransaction::Legacy(tx) => Recovered::new_unchecked(TxEnvelope::Legacy(tx), caller),
        TypedTransaction::Eip2930(tx) => Recovered::new_unchecked(TxEnvelope::Eip2930(tx), caller),
        TypedTransaction::Eip1559(tx) => Recovered::new_unchecked(TxEnvelope::Eip1559(tx), caller),
        TypedTransaction::Eip4844(tx) => Recovered::new_unchecked(TxEnvelope::Eip4844(tx), caller),
        TypedTransaction::Eip7702(tx) => Recovered::new_unchecked(tx.into(), caller),
        TypedTransaction::Eip8141(tx) => {
            let sender = tx.sender;
            Recovered::new_unchecked(TxEnvelope::Eip8141(tx), sender)
        }
    }
}

/// Converts a native recovered envelope without narrowing canonical frame fees or replacing its
/// sender.
pub fn reference_tx(recovered: &RecoveredTxEnvelope) -> Result<TxEnv, String> {
    let tx = recovered.inner();
    let mut env = TxEnv {
        tx_type: tx.ty(),
        caller: recovered.signer(),
        gas_limit: tx.gas_limit(),
        gas_price: tx.max_fee_per_gas(),
        kind: tx.kind(),
        value: tx.value(),
        data: tx.input().clone(),
        nonce: tx.nonce(),
        chain_id: tx.chain_id(),
        gas_priority_fee: tx.max_priority_fee_per_gas(),
        access_list: tx.access_list().cloned().unwrap_or_default(),
        blob_hashes: tx.blob_versioned_hashes().unwrap_or_default().to_vec(),
        max_fee_per_blob_gas: tx.max_fee_per_blob_gas().unwrap_or_default(),
        authorization_list: tx
            .authorization_list()
            .unwrap_or_default()
            .iter()
            .cloned()
            .map(Either::Left)
            .collect(),
        ..Default::default()
    };
    if let Some(tx) = tx.as_eip8141() {
        let frame = FrameTransaction {
            nonce_keys: tx.nonce_keys.clone(),
            nonce_calldata: tx
                .nonce_keys
                .as_deref()
                .map(|keys| nonce_calldata(keys, tx.nonce))
                .unwrap_or_default(),
            frames: tx.frames.clone(),
            signatures: tx.signatures.clone(),
            signature_hash: tx.signature_hash(),
            max_fee: tx.fees.max_fee,
            max_priority_fee_per_gas: tx.fees.max_priority_fee_per_gas,
        };
        env.caller = tx.sender;
        env.kind = TxKind::Call(tx.sender);
        env.gas_limit = frame.gas_limit(tx.sender).unwrap_or(u64::MAX);
        env.frame_transaction = Some(Box::new(frame));
    }
    Ok(env)
}

/// Converts a blockchain fixture transaction, preserving authoritative canonical wire bytes.
pub fn reference_block_transaction(
    tx: &crate::blockchaintest::Transaction,
) -> Result<TxEnv, String> {
    let recovered =
        crate::blockchaintest::execute::build_tx(tx).map_err(|error| error.to_string())?;
    reference_tx(&recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::ReceiptEnvelope;
    use alloy_eips::{
        Encodable2718,
        eip8141::{FrameGasUsed, FrameReceipt, FrameReceiptPayload, FrameStatus},
    };
    use alloy_primitives::{hex, keccak256};
    use serde_json::json;

    #[test]
    fn test_authorization_serializes_as_raw_fixture_value() {
        let value = json!({
            "chainId": "0x1",
            "address": "0x0000000000000000000000000000000000000001",
            "nonce": "0x0",
            "yParity": "0x1",
            "r": "0x2",
            "s": "0x3",
        });
        let authorization = TestAuthorization { value: value.clone() };

        assert_eq!(serde_json::to_value(authorization).unwrap(), value);
    }

    #[test]
    fn canonical_frame_bytes_are_authoritative_and_require_exact_input() {
        // Independently assembled type-6 seven-field list with one six-field frame.
        let raw = hex!(
            "06e60180941111111111111111111111111111111111111111c9c8808080c280808080c0c28080c0"
        );
        let recovered = build_recovered_tx_bytes(&raw).unwrap();
        assert_eq!(recovered.signer(), Address::repeat_byte(0x11));
        let frame = recovered.inner().as_eip8141().unwrap();
        assert_eq!(frame.encoded_2718(), raw);
        assert_eq!(frame.tx_hash(), keccak256(raw));
        assert_eq!(frame.signature_hash(), keccak256(raw));
        assert_eq!(frame.chain_id, 1);
        assert_eq!(frame.nonce_keys, None);
        assert_eq!(frame.frames, vec![Frame::default()]);
        let reference = crate::reference_tx(&recovered).unwrap();
        assert_eq!(reference.caller, frame.sender);
        assert_eq!(reference.frame_transaction.unwrap().frames, frame.frames);

        for length in 0..raw.len() {
            assert!(build_recovered_tx_bytes(&raw[..length]).is_err(), "prefix {length}");
        }
        let mut trailing = raw.to_vec();
        trailing.push(0x80);
        assert!(matches!(build_recovered_tx_bytes(&trailing),
            Err(TxBuildError::BuildTransaction(message)) if message == "trailing transaction bytes"));
        let mut network = Vec::new();
        frame.network_encode(&mut network);
        assert!(build_recovered_tx_bytes(&network).is_err());
    }

    #[test]
    fn frame_fixture_conversion_keeps_full_width_fields_and_signing_elision() {
        for length in [0, 1, 55, 56, 255, 256] {
            let mut tx = TxEip8141 {
                chain_id: 1,
                nonce: u64::MAX,
                nonce_keys: Some(vec![U256::ONE, U256::MAX]),
                sender: Address::repeat_byte(0x11),
                frames: vec![Frame {
                    data: vec![0x81; length].into(),
                    value: U256::MAX,
                    ..Default::default()
                }],
                signatures: vec![
                    FrameSignature { signature: vec![0x82; length].into(), ..Default::default() },
                    FrameSignature {
                        msg: SignatureMessage::Explicit(B256::repeat_byte(0x33)),
                        signature: vec![0x84; length].into(),
                        ..Default::default()
                    },
                ],
                fees: TransactionFees {
                    max_fee: U256::MAX,
                    max_priority_fee_per_gas: U256::MAX - U256::ONE,
                },
                ..Default::default()
            };
            let recovered = build_recovered_tx_bytes(&tx.encoded_2718()).unwrap();
            assert_eq!(recovered.inner().as_eip8141().unwrap(), &tx);
            let reference = crate::reference_tx(&recovered).unwrap();
            assert_eq!(reference.nonce, u64::MAX);
            let frame = reference.frame_transaction.unwrap();
            assert_eq!(frame.max_fee, U256::MAX);
            assert_eq!(frame.max_priority_fee_per_gas, U256::MAX - U256::ONE);
            assert_eq!(frame.nonce_keys, tx.nonce_keys);
            let mut nonce_calldata = vec![0xe2, 0x01, 0xa0];
            nonce_calldata.extend([0xff; 32]);
            assert_eq!(frame.nonce_calldata, nonce_calldata);
            assert_eq!(frame.signature_hash, tx.signature_hash());

            let signature_hash = tx.signature_hash();
            tx.signatures[0].signature = vec![0x85; length].into();
            assert_eq!(tx.signature_hash(), signature_hash);
            tx.signatures[0].signature = vec![0x85; length + 1].into();
            assert_ne!(tx.signature_hash(), signature_hash);
            tx.signatures[0].signature = vec![0x85; length].into();
            tx.signatures[1].signature = vec![0x86; length + 1].into();
            assert_ne!(tx.signature_hash(), signature_hash);
        }
    }

    #[test]
    fn frame_signature_hash_commits_to_elided_lengths_as_rlp_integers() {
        for length in [0, 1, 55, 56, 127, 128, 255, 256] {
            let tx = TxEip8141 {
                chain_id: 1,
                sender: Address::repeat_byte(0x11),
                signatures: vec![FrameSignature {
                    scheme: SignatureScheme::Arbitrary,
                    signature: vec![0x81; length].into(),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let signature = rlp_list(&[
                alloy_rlp::encode(u8::from(SignatureScheme::Arbitrary)),
                vec![0x80],
                vec![0x80],
                alloy_rlp::encode(length),
            ]);
            let mut sender = vec![0x94];
            sender.extend_from_slice(tx.sender.as_slice());
            let mut signing_bytes = vec![6];
            signing_bytes.extend(rlp_list(&[
                vec![1],
                vec![0x80],
                sender,
                vec![0xc0],
                rlp_list(&[signature]),
                vec![0xc2, 0x80, 0x80],
                vec![0xc0],
            ]));
            assert_eq!(tx.signature_hash(), keccak256(signing_bytes), "length {length}");
        }
    }

    fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
        let payload_length = items.iter().map(Vec::len).sum::<usize>();
        assert!(payload_length <= 55);
        let mut out = vec![0xc0 + payload_length as u8];
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    #[test]
    fn frame_json_uses_the_full_width_total_budget_in_native_and_reference_builders() {
        let transaction = crate::blockchaintest::Transaction {
            transaction_type: Some(U256::from(6)),
            sender: Some(Address::repeat_byte(0x11)),
            frames: Some(vec![]),
            max_fee: Some(U256::MAX),
            max_fee_per_gas: Some(U256::ONE),
            max_fee_per_blob_gas: Some(U256::ONE),
            max_priority_fee_per_gas: Some(U256::MAX - U256::ONE),
            ..Default::default()
        };
        let json = serde_json::to_value(&transaction).unwrap();
        assert_eq!(json["maxFee"], serde_json::to_value(U256::MAX).unwrap());
        let decoded = serde_json::from_value(json).unwrap();
        let native = crate::blockchaintest::execute::build_tx(&decoded).unwrap();
        let fees = &native.inner().as_eip8141().unwrap().fees;
        assert_eq!(fees.max_fee, U256::MAX);
        assert_eq!(fees.max_priority_fee_per_gas, U256::MAX - U256::ONE);
        let reference = crate::reference_block_transaction(&decoded).unwrap();
        let fees = reference.frame_transaction.unwrap();
        assert_eq!(fees.max_fee, U256::MAX);
        assert_eq!(fees.max_priority_fee_per_gas, U256::MAX - U256::ONE);
    }

    #[test]
    fn canonical_frame_receipt_has_no_output_or_bloom_wire_fields() {
        let receipt = ReceiptEnvelope::Eip8141(
            FrameReceiptPayload {
                cumulative_gas_used: 128,
                payer: Address::repeat_byte(0x11),
                frame_receipts: vec![
                    FrameReceipt {
                        status: FrameStatus::Success,
                        gas_used: FrameGasUsed { execution: 1, state: 2 },
                        logs: vec![],
                    },
                    FrameReceipt { status: FrameStatus::SkippedAtomicBatch, ..Default::default() },
                ],
            }
            .into(),
        );
        // Type 6, [cumulative gas, payer, [[status, [execution,state], logs], ...]].
        let expected =
            hex!("06e48180941111111111111111111111111111111111111111ccc501c20102c0c502c28080c0");
        assert_eq!(receipt.encoded_2718(), expected);
        assert_eq!(ReceiptEnvelope::decode_2718_exact(&expected).unwrap(), receipt);
        let mut invalid_status = expected;
        invalid_status[27] = 3;
        assert!(ReceiptEnvelope::decode_2718_exact(&invalid_status).is_err());
        let mut trailing = expected.to_vec();
        trailing.push(0x80);
        assert!(ReceiptEnvelope::decode_2718_exact(&trailing).is_err());
    }

    #[test]
    fn reference_conversion_preserves_invalid_shapes_for_handler_validation() {
        let payload = TxEip8141 {
            sender: Address::repeat_byte(0x11),
            nonce: u64::MAX,
            nonce_keys: Some(vec![]),
            frames: vec![Frame {
                flags: 0xff,
                limits: FrameLimits { execution: u64::MAX, state: u64::MAX },
                ..Default::default()
            }],
            signatures: vec![FrameSignature {
                scheme: SignatureScheme::Secp256k1,
                ..Default::default()
            }],
            ..Default::default()
        };
        let recovered = Recovered::new_unchecked(
            TxEnvelope::Eip8141(payload.clone()),
            Address::repeat_byte(0x99),
        );
        let reference = reference_tx(&recovered).unwrap();
        assert_eq!(reference.caller, payload.sender);
        assert_eq!(reference.kind, TxKind::Call(payload.sender));
        assert_eq!(reference.nonce, u64::MAX);
        assert_eq!(reference.gas_limit, u64::MAX);
        let frame = reference.frame_transaction.unwrap();
        assert_eq!(frame.nonce_keys, Some(vec![]));
        let mut nonce_calldata = vec![0xc0, 0x88];
        nonce_calldata.extend([0xff; 8]);
        assert_eq!(frame.nonce_calldata, nonce_calldata);
        assert_eq!(frame.frames, payload.frames);
        assert_eq!(frame.signatures, payload.signatures);
    }
}
