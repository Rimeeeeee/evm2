use crate::{
    blockchaintest::{
        Account, Block, BlockHash, BlockHeader, BlockchainTest, BlockchainTestCase, DecodedBlock,
        ForkSpec, SealEngine, State, Transaction, Withdrawal,
    },
    tx::{AccessListItem, TestAuthorization},
};
use alloy_eip7928::BlockAccessList;
use alloy_primitives::{Address, B256, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Binary fixture identity; schema changes require regeneration.
const BINARY_FORMAT: [u8; 5] = *b"EV2F\x02";

pub(crate) fn to_vec(suite: &BlockchainTest) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(&(BINARY_FORMAT, BlockchainTestRef::new(suite)))
}

pub(crate) fn from_bytes(bytes: &[u8]) -> Result<BlockchainTest, postcard::Error> {
    if !bytes.starts_with(&BINARY_FORMAT) {
        return Err(postcard::Error::DeserializeBadEncoding);
    }
    let (suite, remaining) =
        postcard::take_from_bytes::<BlockchainTestWire>(&bytes[BINARY_FORMAT.len()..])?;
    if !remaining.is_empty() {
        return Err(postcard::Error::DeserializeBadEncoding);
    }
    Ok(suite.into())
}

#[derive(Serialize)]
struct BlockchainTestRef<'a>(Vec<(&'a String, BlockchainTestCaseRef<'a>)>);

impl<'a> BlockchainTestRef<'a> {
    fn new(suite: &'a BlockchainTest) -> Self {
        Self(suite.0.iter().map(|(name, case)| (name, BlockchainTestCaseRef::new(case))).collect())
    }
}

#[derive(Serialize)]
struct BlockchainTestCaseRef<'a> {
    genesis_block_header: &'a BlockHeader,
    genesis_rlp: &'a Option<Bytes>,
    blocks: Vec<BlockRef<'a>>,
    post_state: &'a Option<BTreeMap<Address, Account>>,
    pre: &'a State,
    block_hashes: &'a Vec<BlockHash>,
    lastblockhash: &'a B256,
    network: &'a ForkSpec,
    seal_engine: &'a SealEngine,
}

impl<'a> BlockchainTestCaseRef<'a> {
    fn new(case: &'a BlockchainTestCase) -> Self {
        Self {
            genesis_block_header: &case.genesis_block_header,
            genesis_rlp: &case.genesis_rlp,
            blocks: case.blocks.iter().map(BlockRef::new).collect(),
            post_state: &case.post_state,
            pre: &case.pre,
            block_hashes: &case.block_hashes,
            lastblockhash: &case.lastblockhash,
            network: &case.network,
            seal_engine: &case.seal_engine,
        }
    }
}

#[derive(Serialize)]
struct BlockRef<'a> {
    block_header: &'a Option<BlockHeader>,
    rlp_decoded: Option<DecodedBlockRef<'a>>,
    rlp: &'a Bytes,
    expect_exception: &'a Option<String>,
    transactions: Option<Vec<TransactionWire>>,
    uncle_headers: &'a Option<Vec<BlockHeader>>,
    withdrawals: &'a Option<Vec<Withdrawal>>,
    block_access_list: &'a Option<BlockAccessList>,
}

impl<'a> BlockRef<'a> {
    fn new(block: &'a Block) -> Self {
        Self {
            block_header: &block.block_header,
            rlp_decoded: block.rlp_decoded.as_ref().map(DecodedBlockRef::new),
            rlp: &block.rlp,
            expect_exception: &block.expect_exception,
            transactions: block
                .transactions
                .as_ref()
                .map(|transactions| transactions.iter().map(TransactionWire::new).collect()),
            uncle_headers: &block.uncle_headers,
            withdrawals: &block.withdrawals,
            block_access_list: &block.block_access_list,
        }
    }
}

#[derive(Serialize)]
struct DecodedBlockRef<'a> {
    block_header: &'a Option<BlockHeader>,
    transactions: Vec<TransactionWire>,
    uncle_headers: &'a Vec<BlockHeader>,
    withdrawals: &'a Vec<Withdrawal>,
    block_access_list: &'a Option<BlockAccessList>,
}

impl<'a> DecodedBlockRef<'a> {
    fn new(block: &'a DecodedBlock) -> Self {
        Self {
            block_header: &block.block_header,
            transactions: block.transactions.iter().map(TransactionWire::new).collect(),
            uncle_headers: &block.uncle_headers,
            withdrawals: &block.withdrawals,
            block_access_list: &block.block_access_list,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct TransactionWire {
    frames: Option<Vec<crate::tx::TestFrame>>,
    signatures: Vec<crate::tx::TestFrameSignature>,
    nonce_keys: Option<Vec<U256>>,
    txbytes: Option<Bytes>,
    transaction_type: Option<U256>,
    sender: Option<Address>,
    data: Bytes,
    gas_limit: U256,
    gas_price: Option<U256>,
    nonce: U256,
    r: U256,
    s: U256,
    v: U256,
    value: U256,
    to: Option<Address>,
    chain_id: Option<U256>,
    access_list: Option<Vec<AccessListItem>>,
    max_fee_per_gas: Option<U256>,
    max_fee: Option<U256>,
    max_priority_fee_per_gas: Option<U256>,
    blob_versioned_hashes: Vec<B256>,
    max_fee_per_blob_gas: Option<U256>,
    authorization_list: Option<Vec<AuthorizationWire>>,
    hash: Option<B256>,
}

impl TransactionWire {
    fn new(tx: &Transaction) -> Self {
        Self {
            txbytes: tx.txbytes.clone(),
            frames: tx.frames.clone(),
            signatures: tx.signatures.clone(),
            nonce_keys: tx.nonce_keys.clone(),
            transaction_type: tx.transaction_type,
            sender: tx.sender,
            data: tx.data.clone(),
            gas_limit: tx.gas_limit,
            gas_price: tx.gas_price,
            nonce: tx.nonce,
            r: tx.r,
            s: tx.s,
            v: tx.v,
            value: tx.value,
            to: tx.to,
            chain_id: tx.chain_id,
            access_list: tx.access_list.clone(),
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee: tx.max_fee,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes.clone(),
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            authorization_list: tx
                .authorization_list
                .as_ref()
                .map(|authorizations| authorizations.iter().map(AuthorizationWire::new).collect()),
            hash: tx.hash,
        }
    }
}

impl From<TransactionWire> for Transaction {
    fn from(value: TransactionWire) -> Self {
        Self {
            txbytes: value.txbytes,
            frames: value.frames,
            signatures: value.signatures,
            nonce_keys: value.nonce_keys,
            transaction_type: value.transaction_type,
            sender: value.sender,
            data: value.data,
            gas_limit: value.gas_limit,
            gas_price: value.gas_price,
            nonce: value.nonce,
            r: value.r,
            s: value.s,
            v: value.v,
            value: value.value,
            to: value.to,
            chain_id: value.chain_id,
            access_list: value.access_list,
            max_fee_per_gas: value.max_fee_per_gas,
            max_fee: value.max_fee,
            max_priority_fee_per_gas: value.max_priority_fee_per_gas,
            blob_versioned_hashes: value.blob_versioned_hashes,
            max_fee_per_blob_gas: value.max_fee_per_blob_gas,
            authorization_list: value
                .authorization_list
                .map(|authorizations| authorizations.into_iter().map(Into::into).collect()),
            hash: value.hash,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizationWire {
    chain_id: U256,
    address: Address,
    nonce: U256,
    y_parity: U256,
    r: U256,
    s: U256,
}

impl AuthorizationWire {
    fn new(authorization: &TestAuthorization) -> Self {
        serde_json::from_value(authorization.value.clone())
            .expect("fixture authorization must use EEST fields")
    }
}

impl From<AuthorizationWire> for TestAuthorization {
    fn from(value: AuthorizationWire) -> Self {
        let mut object = serde_json::Map::new();
        object.insert("chainId".to_string(), serde_json::to_value(value.chain_id).unwrap());
        object.insert("address".to_string(), serde_json::to_value(value.address).unwrap());
        object.insert("nonce".to_string(), serde_json::to_value(value.nonce).unwrap());
        object.insert("yParity".to_string(), serde_json::to_value(value.y_parity).unwrap());
        object.insert("r".to_string(), serde_json::to_value(value.r).unwrap());
        object.insert("s".to_string(), serde_json::to_value(value.s).unwrap());
        Self { value: serde_json::Value::Object(object) }
    }
}

#[derive(Deserialize)]
struct BlockchainTestWire(Vec<(String, BlockchainTestCaseWire)>);

impl From<BlockchainTestWire> for BlockchainTest {
    fn from(value: BlockchainTestWire) -> Self {
        Self(value.0.into_iter().map(|(name, case)| (name, case.into())).collect())
    }
}

#[derive(Deserialize)]
struct BlockchainTestCaseWire {
    genesis_block_header: BlockHeader,
    genesis_rlp: Option<Bytes>,
    blocks: Vec<BlockWire>,
    post_state: Option<BTreeMap<Address, Account>>,
    pre: State,
    block_hashes: Vec<BlockHash>,
    lastblockhash: B256,
    network: ForkSpec,
    seal_engine: SealEngine,
}

impl From<BlockchainTestCaseWire> for BlockchainTestCase {
    fn from(value: BlockchainTestCaseWire) -> Self {
        Self {
            genesis_block_header: value.genesis_block_header,
            genesis_rlp: value.genesis_rlp,
            blocks: value.blocks.into_iter().map(Into::into).collect(),
            post_state: value.post_state,
            pre: value.pre,
            block_hashes: value.block_hashes,
            lastblockhash: value.lastblockhash,
            network: value.network,
            seal_engine: value.seal_engine,
        }
    }
}

#[derive(Deserialize)]
struct BlockWire {
    block_header: Option<BlockHeader>,
    rlp_decoded: Option<DecodedBlockWire>,
    rlp: Bytes,
    expect_exception: Option<String>,
    transactions: Option<Vec<TransactionWire>>,
    uncle_headers: Option<Vec<BlockHeader>>,
    withdrawals: Option<Vec<Withdrawal>>,
    block_access_list: Option<BlockAccessList>,
}

impl From<BlockWire> for Block {
    fn from(value: BlockWire) -> Self {
        Self {
            block_header: value.block_header,
            rlp_decoded: value.rlp_decoded.map(Into::into),
            rlp: value.rlp,
            expect_exception: value.expect_exception,
            transactions: value
                .transactions
                .map(|transactions| transactions.into_iter().map(Into::into).collect()),
            uncle_headers: value.uncle_headers,
            withdrawals: value.withdrawals,
            block_access_list: value.block_access_list,
        }
    }
}

#[derive(Deserialize)]
struct DecodedBlockWire {
    block_header: Option<BlockHeader>,
    transactions: Vec<TransactionWire>,
    uncle_headers: Vec<BlockHeader>,
    withdrawals: Vec<Withdrawal>,
    block_access_list: Option<BlockAccessList>,
}

impl From<DecodedBlockWire> for DecodedBlock {
    fn from(value: DecodedBlockWire) -> Self {
        Self {
            block_header: value.block_header,
            transactions: value.transactions.into_iter().map(Into::into).collect(),
            uncle_headers: value.uncle_headers,
            withdrawals: value.withdrawals,
            block_access_list: value.block_access_list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::{TestFrame, TestFrameSignature};
    use alloy_primitives::hex;

    #[test]
    fn binary_frame_fixture_preserves_full_width_metadata_and_authoritative_wire_bytes() {
        let raw = hex!(
            "06e60180941111111111111111111111111111111111111111c9c8808080c280808080c0c28080c0"
        );
        let transaction = Transaction {
            txbytes: Some(raw.to_vec().into()),
            transaction_type: Some(U256::from(6)),
            sender: Some(Address::repeat_byte(0x22)),
            nonce_keys: Some(vec![U256::ONE, U256::MAX]),
            max_fee_per_gas: Some(U256::MAX),
            max_fee: Some(U256::MAX - U256::from(3)),
            max_priority_fee_per_gas: Some(U256::MAX - U256::ONE),
            max_fee_per_blob_gas: Some(U256::MAX - U256::from(2)),
            frames: Some(vec![TestFrame {
                mode: U256::MAX,
                flags: U256::MAX,
                target: Some(Bytes::from_static(&[1, 2, 3])),
                gas_limit: U256::MAX,
                state_gas_limit: U256::MAX,
                value: U256::MAX,
                data: vec![0x81; 256].into(),
            }]),
            signatures: vec![TestFrameSignature {
                scheme: U256::MAX,
                signer: Bytes::from_static(&[4, 5, 6]),
                msg: B256::repeat_byte(0x33).as_slice().to_vec().into(),
                signature: vec![0x82; 256].into(),
            }],
            ..Default::default()
        };
        let suite = BlockchainTest(BTreeMap::from([(
            "frame-wire-authority".into(),
            BlockchainTestCase {
                genesis_block_header: BlockHeader::default(),
                genesis_rlp: Some(Bytes::from_static(&[0xc0])),
                blocks: vec![Block {
                    transactions: Some(vec![transaction.clone()]),
                    rlp_decoded: Some(DecodedBlock {
                        transactions: vec![transaction],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                post_state: None,
                pre: State::default(),
                block_hashes: vec![],
                lastblockhash: B256::ZERO,
                network: ForkSpec::Bogota,
                seal_engine: SealEngine::NoProof,
            },
        )]));
        let encoded = to_vec(&suite).unwrap();
        let decoded = from_bytes(&encoded).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), serde_json::to_value(&suite).unwrap());
        let block = &decoded.0["frame-wire-authority"].blocks[0];
        for transaction in [
            &block.transactions.as_ref().unwrap()[0],
            &block.rlp_decoded.as_ref().unwrap().transactions[0],
        ] {
            let recovered = crate::blockchaintest::execute::build_tx(transaction).unwrap();
            assert_eq!(recovered.signer(), Address::repeat_byte(0x11));
            let frame = recovered.inner().as_eip8141().unwrap();
            assert_eq!(frame.nonce_keys, None);
            assert_eq!(frame.fees.max_fee, U256::ZERO);
            assert_eq!(frame.frames, vec![alloy_eips::eip8141::Frame::default()]);
        }
        let mut wrong_schema = encoded.clone();
        wrong_schema[4] = 0;
        assert!(from_bytes(&wrong_schema).is_err());
        let mut trailing = encoded;
        trailing.push(0);
        assert!(from_bytes(&trailing).is_err());
    }

    #[test]
    fn binary_format_is_checked_before_decoding_the_fixture_payload() {
        for input in [&[][..], b"EV2F", b"EV2F\0\xff", b"EV2F\x01\xff", b"\xff\xff\xff\xff\xff"] {
            assert!(matches!(from_bytes(input), Err(postcard::Error::DeserializeBadEncoding)));
        }
        assert!(matches!(
            from_bytes(&BINARY_FORMAT),
            Err(postcard::Error::DeserializeUnexpectedEnd)
        ));
    }
}
