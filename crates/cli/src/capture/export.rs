use super::{
    error::CaptureError,
    model::{CapturedBlock, CapturedCase, CapturedInput, State as CapturedState},
};
use alloy_consensus::{
    EthereumTxEnvelope, Header, TxEip4844,
    transaction::{Transaction as _, to_eip155_value},
};
use alloy_eips::{Encodable2718, Typed2718, eip2930::AccessList, eip7702::SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, FixedBytes, U256};
use evm2::SpecId;
use evm2_eest::{
    AccessListItem, TestAuthorization,
    blockchaintest::{
        Account, Block, BlockHash as EestBlockHash, BlockHeader, BlockchainTest,
        BlockchainTestCase, ForkSpec, SealEngine, State as EestState, Transaction, Withdrawal,
    },
};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) fn suite(capture: &CapturedCase) -> Result<BlockchainTest, CaptureError> {
    let network = network(capture)?;
    let inputs = captured_blocks(capture)?;
    let decoded_blocks =
        inputs.iter().map(export_block).collect::<Result<Vec<_>, CaptureError>>()?;
    let first = decoded_blocks.first().expect("blocks is not empty");
    let last = decoded_blocks.last().expect("blocks is not empty");
    let first_header = first.block_header.as_ref().expect("exported blocks include headers");
    let last_header = last.block_header.as_ref().expect("exported blocks include headers");
    let name = format!("mainnet_{}_{}", first_header.number, last_header.number);
    let genesis_block_header = parent_header(first);
    let lastblockhash = last_header.hash;

    let mut cases = BTreeMap::new();
    cases.insert(
        name,
        BlockchainTestCase {
            genesis_block_header,
            genesis_rlp: None,
            blocks: decoded_blocks,
            post_state: capture.post_state.as_ref().map(|state| export_state(capture, state).0),
            pre: export_state(capture, &capture.pre_state),
            block_hashes: export_block_hashes(capture),
            lastblockhash,
            network,
            seal_engine: SealEngine::NoProof,
        },
    );
    Ok(BlockchainTest(cases))
}

fn export_block_hashes(capture: &CapturedCase) -> Vec<EestBlockHash> {
    capture
        .pre_state
        .block_hashes
        .iter()
        .map(|block_hash| EestBlockHash {
            number: U256::from(block_hash.number),
            hash: block_hash.hash,
        })
        .collect()
}

fn captured_blocks(capture: &CapturedCase) -> Result<&[CapturedBlock], CaptureError> {
    let blocks = match &capture.input {
        CapturedInput::Block(block) => std::slice::from_ref(block.as_ref()),
        CapturedInput::Blocks(blocks) => blocks.blocks.as_slice(),
    };
    if blocks.is_empty() {
        return Err(CaptureError::EmptyCapture);
    }
    Ok(blocks)
}

fn network(capture: &CapturedCase) -> Result<ForkSpec, CaptureError> {
    let first = capture.versions.versions.first().ok_or(CaptureError::EmptyCapturedVersions)?;
    if capture.versions.versions.iter().any(|version| version.spec_id != first.spec_id) {
        return Err(CaptureError::MultipleSpecs);
    }
    let spec =
        SpecId::try_from_u32(first.spec_id).ok_or(CaptureError::UnsupportedSpec(first.spec_id))?;
    match spec {
        SpecId::FRONTIER => Ok(ForkSpec::Frontier),
        SpecId::HOMESTEAD => Ok(ForkSpec::Homestead),
        SpecId::TANGERINE => Ok(ForkSpec::EIP150),
        SpecId::SPURIOUS_DRAGON => Ok(ForkSpec::EIP158),
        SpecId::BYZANTIUM => Ok(ForkSpec::Byzantium),
        SpecId::PETERSBURG => Ok(ForkSpec::ConstantinopleFix),
        SpecId::ISTANBUL => Ok(ForkSpec::Istanbul),
        SpecId::BERLIN => Ok(ForkSpec::Berlin),
        SpecId::LONDON => Ok(ForkSpec::London),
        SpecId::MERGE => Ok(ForkSpec::Paris),
        SpecId::SHANGHAI => Ok(ForkSpec::Shanghai),
        SpecId::CANCUN => Ok(ForkSpec::Cancun),
        SpecId::PRAGUE => Ok(ForkSpec::Prague),
        SpecId::OSAKA => Ok(ForkSpec::Osaka),
        SpecId::AMSTERDAM => Ok(ForkSpec::Amsterdam),
        SpecId::BOGOTA => Ok(ForkSpec::Bogota),
        _ => Err(CaptureError::UnsupportedSpec(first.spec_id)),
    }
}

fn export_state(capture: &CapturedCase, state: &CapturedState) -> EestState {
    let codes = capture
        .code_table
        .codes
        .iter()
        .map(|code| (code.code_hash, code.bytecode.clone()))
        .collect::<BTreeMap<_, _>>();
    let accounts = state
        .accounts
        .iter()
        .map(|account| {
            let code = codes.get(&account.code_hash).cloned().unwrap_or_default();
            let storage = account
                .storage
                .iter()
                .map(|entry| (U256::from_be_bytes(entry.slot.0), entry.value))
                .collect();
            (
                account.address,
                Account {
                    balance: account.balance,
                    code,
                    nonce: U256::from(account.nonce),
                    storage,
                },
            )
        })
        .collect();
    EestState(accounts)
}

fn parent_header(first: &Block) -> BlockHeader {
    let mut header = first.block_header.clone().expect("exported blocks include headers");
    header.hash = header.parent_hash;
    header.number = header.number.saturating_sub(U256::from(1));
    header.gas_used = U256::ZERO;
    header.transactions_trie = B256::ZERO;
    header.receipt_trie = B256::ZERO;
    header.state_root = B256::ZERO;
    header.uncle_hash = B256::ZERO;
    header
}

fn export_block(input: &CapturedBlock) -> Result<Block, CaptureError> {
    let block = &input.block;
    Ok(Block {
        block_header: Some(export_header(&block.header)),
        rlp: Bytes::new(),
        expect_exception: None,
        transactions: Some(
            block
                .body
                .transactions
                .iter()
                .zip(input.transactions.iter())
                .map(|(tx, input_tx)| export_transaction(tx, input_tx.signer))
                .collect(),
        ),
        uncle_headers: Some(block.body.ommers.iter().map(export_header).collect()),
        withdrawals: Some(
            block
                .body
                .withdrawals
                .as_ref()
                .map(|withdrawals| {
                    withdrawals
                        .iter()
                        .map(|withdrawal| Withdrawal {
                            index: U256::from(withdrawal.index),
                            validator_index: U256::from(withdrawal.validator_index),
                            address: withdrawal.address,
                            amount: U256::from(withdrawal.amount),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        block_access_list: None,
        rlp_decoded: None,
    })
}

fn export_header(header: &Header) -> BlockHeader {
    BlockHeader {
        bloom: Bytes::copy_from_slice(header.logs_bloom.as_slice()),
        coinbase: header.beneficiary,
        difficulty: header.difficulty,
        extra_data: header.extra_data.clone(),
        gas_limit: U256::from(header.gas_limit),
        gas_used: U256::from(header.gas_used),
        hash: header.hash_slow(),
        mix_hash: header.mix_hash,
        nonce: FixedBytes::from(header.nonce.0),
        number: U256::from(header.number),
        parent_hash: header.parent_hash,
        receipt_trie: header.receipts_root,
        state_root: header.state_root,
        timestamp: U256::from(header.timestamp),
        transactions_trie: header.transactions_root,
        uncle_hash: header.ommers_hash,
        base_fee_per_gas: header.base_fee_per_gas.map(U256::from),
        withdrawals_root: header.withdrawals_root,
        blob_gas_used: header.blob_gas_used.map(U256::from),
        excess_blob_gas: header.excess_blob_gas.map(U256::from),
        parent_beacon_block_root: header.parent_beacon_block_root,
        requests_hash: header.requests_hash,
        target_blobs_per_block: None,
        slot_number: header.slot_number.map(U256::from),
        block_access_list_hash: header.block_access_list_hash,
    }
}

fn export_transaction(tx: &EthereumTxEnvelope<TxEip4844>, signer: Address) -> Transaction {
    if let Some(frame) = tx.as_eip8141() {
        return Transaction {
            txbytes: Some(tx.encoded_2718().into()),
            transaction_type: Some(U256::from(6)),
            sender: Some(frame.sender),
            nonce: U256::from(frame.nonce),
            chain_id: Some(U256::from(frame.chain_id)),
            max_fee: Some(frame.fees.max_fee),
            max_priority_fee_per_gas: Some(frame.fees.max_priority_fee_per_gas),
            blob_versioned_hashes: frame.blob_versioned_hashes.clone(),
            hash: Some(*tx.tx_hash()),
            ..Default::default()
        };
    }
    let signature = tx.signature().expect("frame envelopes were handled above");
    let tx_type = tx.ty();
    Transaction {
        txbytes: None,
        frames: None,
        signatures: Vec::new(),
        nonce_keys: None,
        transaction_type: Some(U256::from(tx_type)),
        sender: Some(signer),
        data: tx.input().clone(),
        gas_limit: U256::from(tx.gas_limit()),
        gas_price: tx.gas_price().map(U256::from),
        nonce: U256::from(tx.nonce()),
        r: signature.r(),
        s: signature.s(),
        v: U256::from(signature_v(tx_type, signature.v(), tx.chain_id())),
        value: tx.value(),
        to: tx.kind().into_to(),
        chain_id: tx.chain_id().map(U256::from),
        access_list: tx.access_list().map(export_access_list),
        max_fee_per_gas: tx.tx_type().is_dynamic_fee().then(|| U256::from(tx.max_fee_per_gas())),
        max_fee: None,
        max_priority_fee_per_gas: tx.max_priority_fee_per_gas().map(U256::from),
        blob_versioned_hashes: tx.blob_versioned_hashes().unwrap_or_default().to_vec(),
        max_fee_per_blob_gas: tx.max_fee_per_blob_gas().map(U256::from),
        authorization_list: tx.authorization_list().map(export_authorization_list),
        hash: Some(*tx.tx_hash()),
    }
}

fn signature_v(tx_type: u8, y_parity: bool, chain_id: Option<u64>) -> u128 {
    if tx_type == 0 { to_eip155_value(y_parity, chain_id) } else { u128::from(y_parity as u8) }
}

fn export_access_list(access_list: &AccessList) -> Vec<AccessListItem> {
    access_list
        .0
        .iter()
        .map(|item| AccessListItem {
            address: item.address,
            storage_keys: item.storage_keys.clone(),
        })
        .collect()
}

fn export_authorization_list(authorizations: &[SignedAuthorization]) -> Vec<TestAuthorization> {
    authorizations
        .iter()
        .map(|authorization| TestAuthorization {
            value: json!({
                "chainId": *authorization.inner().chain_id(),
                "address": *authorization.inner().address(),
                "nonce": U256::from(authorization.inner().nonce()),
                "yParity": U256::from(authorization.y_parity()),
                "r": authorization.r(),
                "s": authorization.s(),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::TxEip8141;
    use alloy_eips::{
        Decodable2718,
        eip8141::{Frame, FrameSignature},
    };
    use alloy_primitives::Sealable;

    #[test]
    fn frame_export_replays_canonical_wire_with_explicit_sender_and_full_width_fees() {
        let sender = Address::with_last_byte(0x42);
        let mut payload = TxEip8141 {
            sender,
            nonce: 7,
            nonce_keys: Some(vec![U256::ONE, U256::MAX]),
            frames: vec![Frame::default()],
            signatures: vec![FrameSignature {
                signature: Bytes::from_static(&[1, 2, 3]),
                ..Default::default()
            }],
            ..Default::default()
        };
        payload.fees.max_fee = U256::MAX;
        let envelope = EthereumTxEnvelope::<TxEip4844>::Eip8141(payload.clone().seal_slow());
        let exported = export_transaction(&envelope, Address::ZERO);
        assert_eq!(exported.sender, Some(sender));
        assert_eq!(exported.max_fee, Some(U256::MAX));
        assert_eq!(exported.max_fee_per_gas, None);
        assert_eq!(exported.max_fee_per_blob_gas, None);
        assert_eq!(exported.txbytes.as_ref().unwrap().as_ref(), envelope.encoded_2718().as_slice());
        let encoded = serde_json::to_vec(&exported).unwrap();
        let replay = serde_json::from_slice::<Transaction>(&encoded).unwrap();
        let decoded =
            EthereumTxEnvelope::<TxEip4844>::decode_2718(&mut replay.txbytes.unwrap().as_ref())
                .unwrap();
        let EthereumTxEnvelope::Eip8141(frame) = decoded else {
            panic!("expected frame transaction")
        };
        assert_eq!(frame.inner(), &payload);
    }
}
