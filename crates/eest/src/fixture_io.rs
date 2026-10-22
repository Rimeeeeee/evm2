use crate::{binary, blockchaintest::BlockchainTest};
use alloy_primitives::Bytes;
use alloy_rlp::{Header, PayloadView};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    path::Path,
};
use thiserror::Error;
use zstd::stream::{read::Decoder as ZstdDecoder, write::Encoder as ZstdEncoder};

const ZSTD_COMPRESSION_LEVEL: i32 = 19;

/// Error while reading an EEST fixture.
#[derive(Debug, Error)]
pub enum FixtureReadError {
    /// File read failed.
    #[error("read error: {0}")]
    Read(#[from] io::Error),
    /// JSON decoding failed.
    #[error("JSON decoding failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Postcard decoding failed.
    #[error("postcard decoding failed: {0}")]
    Postcard(#[from] postcard::Error),
}

/// Error while writing an EEST fixture.
#[derive(Debug, Error)]
pub enum FixtureWriteError {
    /// File write failed.
    #[error("write error: {0}")]
    Write(#[from] io::Error),
    /// JSON encoding failed.
    #[error("JSON encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Postcard encoding failed.
    #[error("postcard encoding failed: {0}")]
    Postcard(#[from] postcard::Error),
}

/// Returns true when the path uses the binary fixture extension.
pub fn is_binary_path(path: &Path) -> bool {
    has_extension(path, "bin") || is_zstd_path(path) && stem_has_extension(path, "bin")
}

/// Reads a plain JSON fixture or zstd-compressed JSON fixture.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let mut reader: Box<dyn Read> =
        if is_zstd_path(path) { Box::new(ZstdDecoder::new(file)?) } else { Box::new(file) };
    let mut input = String::new();
    reader.read_to_string(&mut input)?;
    Ok(input)
}

/// Reads a blockchain test fixture from JSON, zstd-compressed JSON, or postcard binary.
pub fn read_blockchain(path: &Path) -> Result<BlockchainTest, FixtureReadError> {
    if is_binary_path(path) {
        let bytes = read_bytes(path)?;
        Ok(binary::from_bytes(&bytes)?)
    } else {
        let input = read_to_string(path)?;
        Ok(parse_json(&input)?)
    }
}

/// Writes a blockchain test fixture as JSON, zstd-compressed JSON, or postcard binary.
pub fn write_blockchain(path: &Path, suite: &BlockchainTest) -> Result<(), FixtureWriteError> {
    if is_binary_path(path) {
        let bytes = binary::to_vec(suite)?;
        if is_zstd_path(path) {
            write_zstd_bytes(path, &bytes)?;
        } else {
            fs::write(path, bytes)?;
        }
    } else {
        let writer = BufWriter::new(File::create(path)?);
        if is_zstd_path(path) {
            let mut encoder = ZstdEncoder::new(writer, ZSTD_COMPRESSION_LEVEL)?;
            serde_json::to_writer(&mut encoder, suite)?;
            encoder.finish()?;
        } else {
            serde_json::to_writer(writer, suite)?;
        }
    }
    Ok(())
}

fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    if is_zstd_path(path) {
        let file = File::open(path)?;
        let mut decoder = ZstdDecoder::new(file)?;
        let mut bytes = Vec::new();
        decoder.read_to_end(&mut bytes)?;
        Ok(bytes)
    } else {
        fs::read(path)
    }
}

fn write_zstd_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let writer = BufWriter::new(File::create(path)?);
    let mut encoder = ZstdEncoder::new(writer, ZSTD_COMPRESSION_LEVEL)?;
    encoder.write_all(bytes)?;
    encoder.finish()?;
    Ok(())
}

fn is_zstd_path(path: &Path) -> bool {
    has_extension(path, "zst")
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|candidate| candidate == extension)
}

fn stem_has_extension(path: &Path, extension: &str) -> bool {
    path.file_stem().is_some_and(|stem| {
        Path::new(stem).extension().is_some_and(|candidate| candidate == extension)
    })
}

/// Reads canonical frame bytes instead of the fixture's explanatory transaction metadata.
/// Invalid fixtures deliberately contain values outside Rust's integer/address types. Keeping
/// their wire payload authoritative lets the actual decoder reject them without narrowing them.
pub(crate) fn parse_json<T: DeserializeOwned>(input: &str) -> Result<T, serde_json::Error> {
    let mut value: Value = serde_json::from_str(input)?;
    if let Some(cases) = value.as_object_mut() {
        for case in cases.values_mut() {
            if let Some(posts) = case.get("post").and_then(Value::as_object) {
                let posts =
                    posts.values().filter_map(Value::as_array).flatten().collect::<Vec<_>>();
                if !posts.is_empty()
                    && posts.iter().all(|post| {
                        post.get("txbytes")
                            .and_then(Value::as_str)
                            .is_some_and(|s| s.starts_with("0x06"))
                    })
                {
                    case["transaction"] = json!({
                        "data": ["0x"], "gasLimit": ["0x0"], "nonce": "0x0", "value": ["0x0"]
                    });
                }
            }
            if let Some(blocks) = case.get_mut("blocks").and_then(Value::as_array_mut) {
                for block in blocks {
                    let wire = block
                        .get("rlp")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<Bytes>().ok())
                        .and_then(|bytes| block_transaction_bytes(&bytes).ok());
                    for field in ["transactions", "rlp_decoded"] {
                        let transactions = if field == "transactions" {
                            block.get_mut(field)
                        } else {
                            block.get_mut(field).and_then(|decoded| decoded.get_mut("transactions"))
                        };
                        if let Some(transactions) = transactions.and_then(Value::as_array_mut) {
                            for (index, transaction) in transactions.iter_mut().enumerate() {
                                let bytes =
                                    wire.as_ref().and_then(|txs| txs.get(index)).cloned().or_else(
                                        || {
                                            transaction.get("txbytes").cloned().and_then(|v| {
                                                serde_json::from_value::<Bytes>(v).ok()
                                            })
                                        },
                                    );
                                if let Some(bytes) = bytes.filter(|bytes| bytes.first() == Some(&6))
                                {
                                    *transaction = json!({
                                        "txbytes": bytes, "type": "0x6", "data": "0x", "gasLimit": "0x0",
                                        "nonce": "0x0", "r": "0x0", "s": "0x0", "v": "0x0", "value": "0x0"
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    serde_json::from_value(value)
}

/// Extracts transaction wire bytes from the block body, without accepting network wrappers.
pub(crate) fn block_transaction_bytes(bytes: &[u8]) -> Result<Vec<Bytes>, alloy_rlp::Error> {
    let mut input = bytes;
    let PayloadView::List(fields) = Header::decode_raw(&mut input)? else {
        return Err(alloy_rlp::Error::UnexpectedString);
    };
    if !input.is_empty() {
        return Err(alloy_rlp::Error::Custom("trailing block bytes"));
    }
    let mut transactions = *fields.get(1).ok_or(alloy_rlp::Error::InputTooShort)?;
    let PayloadView::List(transactions) = Header::decode_raw(&mut transactions)? else {
        return Err(alloy_rlp::Error::UnexpectedString);
    };
    transactions
        .into_iter()
        .map(|bytes| {
            let mut payload = bytes;
            let header = Header::decode(&mut payload)?;
            Ok(Bytes::copy_from_slice(if header.list {
                bytes
            } else {
                &payload[..header.payload_length]
            }))
        })
        .collect()
}
