use alloy_consensus::{
    EthereumTxEnvelope, TxEip4844, TxEip8141, TypedTransaction,
    transaction::{Recovered, SignerRecoverable},
};
use alloy_eips::{
    Decodable2718,
    eip7702::SignedAuthorization,
    eip8141::{
        Frame, FrameAddress, FrameLimits, FrameMode, FrameSignature, SignatureMessage,
        SignatureScheme, TransactionFees,
    },
};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use alloy_rpc_types_eth::{
    AccessList as RpcAccessList, AccessListItem as RpcAccessListItem, TransactionInput,
    TransactionRequest,
};
use evm2::ethereum::{RecoveredTxEnvelope, TxEnvelope};
use k256::ecdsa::SigningKey;
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
pub(crate) enum TxBuildError {
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
    if fields.tx_type == Some(6) || fields.frames.is_some() {
        return build_frame_tx(fields);
    }
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

    let tx =
        request.build_consensus_tx().map_err(|err| TxBuildError::BuildTransaction(err.error))?;
    Ok(recovered_envelope(tx, fields.caller))
}

fn build_frame_tx(fields: TxFields) -> Result<RecoveredTxEnvelope, TxBuildError> {
    let invalid = |reason: &str| TxBuildError::BuildTransaction(reason.into());
    let frames = fields
        .frames
        .ok_or_else(|| invalid("type 6 requires frames"))?
        .into_iter()
        .map(|frame| {
            let mode = u8::try_from(frame.mode)
                .ok()
                .and_then(FrameMode::try_from_u8)
                .ok_or_else(|| invalid("invalid frame mode"))?;
            let target = FrameAddress::try_from(
                frame.target.as_ref().map_or(&[][..], |target| target.as_ref()),
            )
            .map_err(|error| invalid(&error.to_string()))?;
            Ok(Frame {
                mode,
                target,
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
        })
        .collect::<Result<_, TxBuildError>>()?;
    let signatures = fields
        .signatures
        .into_iter()
        .map(|signature| {
            Ok(FrameSignature {
                scheme: u8::try_from(signature.scheme)
                    .ok()
                    .and_then(|scheme| SignatureScheme::try_from(scheme).ok())
                    .ok_or_else(|| invalid("invalid signature scheme"))?,
                signer: FrameAddress::try_from(signature.signer.as_ref())
                    .map_err(|error| invalid(&error.to_string()))?,
                msg: SignatureMessage::try_from(signature.msg.as_ref())
                    .map_err(|error| invalid(&error.to_string()))?,
                signature: signature.signature,
            })
        })
        .collect::<Result<_, TxBuildError>>()?;
    let payload = TxEip8141 {
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
            max_fee_per_gas: fields.max_fee_per_gas.unwrap_or_default(),
            max_priority_fee_per_gas: fields.max_priority_fee_per_gas.unwrap_or_default(),
            max_fee_per_blob_gas: fields.max_fee_per_blob_gas.unwrap_or_default(),
        },
        blob_versioned_hashes: fields.blob_versioned_hashes,
    };
    Ok(Recovered::new_unchecked(TxEnvelope::Eip8141(payload), fields.caller))
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

#[cfg(test)]
mod tests {
    use super::TestAuthorization;
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
}
