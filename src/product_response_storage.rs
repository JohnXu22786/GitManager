//! Lossless, bounded storage of one original provider response, not evidence
//! authority. No files, archives, sidecars, provider invocation or execution.
//!
//! Successful decoding establishes only exact bytes and domain syntax. The
//! caller must still validate the response for its ORIGINAL request, compare
//! its raw digest with the ACTUAL receipt.result_digest, retain the actual
//! producer and check source/Basis/consent identities. These hashes are local
//! integrity claims, not signatures or protection from coherent local tampering.
//!
//! Standalone input must use `parse`; an embedded field requires the existing
//! strict whole-journal parser before typed deserialization, then `to_bytes`.
//! Serde deserialization alone grants neither size validation nor authority.
//! The enclosing record still has the unchanged 1 MiB limit: a field fitting
//! here does not prove the complete journal fits. A later fixed host schema may
//! retain at most two separate responses, never an unbounded response archive.
//! Production registration, journal integration/migration and backup are separate.

#[path = "tool_proposal_input.rs"]
mod bounded_input;

use crate::product_contract::{ContractError, DevelopmentResponse};
use crate::product_provider::{digest, MAX_RESULT_BYTES};
use flate2::{write::ZlibEncoder, Compression, Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

const ENCODING: &str = "gitmanager.host-response.zlib-bytes";
const VERSION: u32 = 1;
const CHUNK_BYTES: usize = 8192;

/// Exactly one zlib stream of the original UTF-8 JSON bytes, including their
/// whitespace, property order and escape spelling. Never reserialize a parsed
/// response as a substitute for its receipt-bound original bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactStoredResponseV1 {
    encoding: String,
    version: u32,
    raw_bytes: usize,
    raw_sha256: String,
    compressed_bytes: Vec<u8>,
}

impl ExactStoredResponseV1 {
    /// Reject an unrepresentable response without changing any existing state.
    /// Even a valid raw response may not fit the compressed field's JSON cap.
    pub fn from_bytes(raw: &[u8], result_byte_limit: usize) -> Result<Self, ContractError> {
        validate_limit(result_byte_limit)?;
        validate_length(raw.len(), result_byte_limit)?;
        validate_response(raw)?;
        let output = CappedWriter::new(Vec::new(), MAX_RESULT_BYTES);
        let mut encoder = ZlibEncoder::new(output, Compression::default());
        encoder
            .write_all(raw)
            .map_err(|error| invalid(format!("response compression failed: {error}")))?;
        let compressed_bytes = encoder
            .finish()
            .map_err(|error| invalid(format!("response compression failed: {error}")))?
            .inner;
        let stored = Self {
            encoding: ENCODING.into(),
            version: VERSION,
            raw_bytes: raw.len(),
            raw_sha256: digest(raw),
            compressed_bytes,
        };
        stored.validate_metadata(result_byte_limit)?;
        Ok(stored)
    }

    /// Strict standalone intake: encoded size, duplicate keys and depth are
    /// checked before the DTO is allocated. Decode validation is also required.
    pub fn parse(bytes: &[u8], result_byte_limit: usize) -> Result<Self, ContractError> {
        validate_limit(result_byte_limit)?;
        let value =
            bounded_input::parse_json_bytes(bytes).map_err(|error| invalid(error.to_string()))?;
        let stored: Self =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        stored.to_bytes(result_byte_limit)?;
        Ok(stored)
    }

    pub fn to_bytes(&self, result_byte_limit: usize) -> Result<Vec<u8>, ContractError> {
        self.to_bytes_with_cancel(result_byte_limit, &|| false)
    }

    /// The host may cancel between bounded input/output chunks. Cancellation
    /// returns an error, never a partial usable response, and changes no state.
    pub fn to_bytes_with_cancel(
        &self,
        result_byte_limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, ContractError> {
        check_cancelled(cancelled)?;
        self.validate_metadata(result_byte_limit)?;
        // The declared size is checked before allocating. One excess byte is
        // decoded only into the fixed scratch buffer, never appended to raw.
        let mut raw = Vec::with_capacity(self.raw_bytes);
        let mut decoder = Decompress::new(true);
        let mut chunk = [0u8; CHUNK_BYTES];
        loop {
            check_cancelled(cancelled)?;
            let before_in = decoder.total_in() as usize;
            let before_out = decoder.total_out() as usize;
            let input_end = (before_in + CHUNK_BYTES).min(self.compressed_bytes.len());
            let output_len = CHUNK_BYTES.min(self.raw_bytes + 1 - before_out);
            let status = decoder
                .decompress(
                    &self.compressed_bytes[before_in..input_end],
                    &mut chunk[..output_len],
                    FlushDecompress::None,
                )
                .map_err(|error| {
                    if error.needs_dictionary().is_some() {
                        invalid("stored response requires a forbidden zlib dictionary")
                    } else {
                        invalid(format!("invalid stored response zlib stream: {error}"))
                    }
                })?;
            let after_in = decoder.total_in() as usize;
            let after_out = decoder.total_out() as usize;
            if after_out > self.raw_bytes {
                return Err(invalid("stored response exceeds its declared raw length"));
            }
            raw.extend_from_slice(&chunk[..after_out - before_out]);
            if status == Status::StreamEnd {
                if after_in != self.compressed_bytes.len() {
                    return Err(invalid(
                        "stored response has trailing or concatenated streams",
                    ));
                }
                if after_out != self.raw_bytes {
                    return Err(invalid("stored response raw length mismatch"));
                }
                break;
            }
            if before_in == after_in && before_out == after_out {
                return Err(invalid("stored response is truncated or makes no progress"));
            }
        }
        check_cancelled(cancelled)?;
        if digest(&raw) != self.raw_sha256 {
            return Err(invalid("stored response raw SHA256 mismatch"));
        }
        validate_response(&raw)?;
        Ok(raw)
    }

    fn validate_metadata(&self, result_byte_limit: usize) -> Result<(), ContractError> {
        validate_limit(result_byte_limit)?;
        if self.encoding != ENCODING || self.version != VERSION {
            return Err(invalid("unsupported stored response encoding or version"));
        }
        validate_length(self.raw_bytes, result_byte_limit)?;
        if self.compressed_bytes.is_empty() || self.compressed_bytes.len() > MAX_RESULT_BYTES {
            return Err(invalid(
                "stored response compressed bytes exceed the allowed length",
            ));
        }
        if self.raw_sha256.len() != 64
            || !self
                .raw_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("stored response has an invalid raw SHA256"));
        }
        // Count directly into a bounded sink, not a potentially multi-MiB JSON
        // allocation. A decimal Vec<u8> can exceed the enclosing JSON cap.
        serde_json::to_writer(
            CappedWriter::new(io::sink(), bounded_input::MAX_INPUT_BYTES),
            self,
        )
        .map_err(|_| invalid("stored response JSON exceeds the existing byte limit"))
    }
}

fn invalid(message: impl Into<String>) -> ContractError {
    ContractError(message.into())
}
fn validate_limit(limit: usize) -> Result<(), ContractError> {
    if limit == 0 || limit > MAX_RESULT_BYTES {
        return Err(invalid("invalid response result byte limit"));
    }
    Ok(())
}
fn validate_length(length: usize, limit: usize) -> Result<(), ContractError> {
    if length == 0 || length > limit {
        return Err(invalid(
            "stored response raw length exceeds the allowed length",
        ));
    }
    Ok(())
}
fn validate_response(raw: &[u8]) -> Result<(), ContractError> {
    std::str::from_utf8(raw).map_err(|_| invalid("stored response is not UTF-8"))?;
    DevelopmentResponse::parse(raw)?;
    Ok(())
}
fn check_cancelled(cancelled: &dyn Fn() -> bool) -> Result<(), ContractError> {
    if cancelled() {
        return Err(invalid("stored response decode cancelled"));
    }
    Ok(())
}

/// Bound serialized/compressed output before writing or growing its buffer.
struct CappedWriter<W> {
    inner: W,
    remaining: usize,
}
impl<W> CappedWriter<W> {
    fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            remaining: limit,
        }
    }
}
impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("stored response byte limit exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
