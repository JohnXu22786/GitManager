//! Bounded JSON syntax intake for untrusted proposal data.
//!
//! A returned [`serde_json::Value`] is only a parsed document, never an accepted
//! proposal or execution evidence. The proposal importer must still validate its
//! typed schema, versions, project/base identity, allowed operations and counts,
//! then replay locally before adoption. Members such as `passed` have no special
//! meaning here. This module neither opens paths nor executes input contents.

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
#[path = "tool_proposal_input/capture_payment_core.rs"]
pub(crate) mod capture_payment_core;
#[path = "tool_proposal_input/capture_paid.rs"]
mod capture_paid;
pub(super) use capture_paid::{parse_json_bytes_with_work, InputWorkError};
pub(super) use capture_paid::{lexical_envelope_with_work, LexicalEnvelope};

use std::fmt;
use std::io::{self, Read};

/// Maximum encoded input size: 1 MiB, including all whitespace and UTF-8 bytes.
/// This bounds source size, not the exact heap size of the parsed value.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// At most 64 nested containers (objects and arrays combined).
/// A scalar root has depth zero; an empty root array or object has depth one.
/// This stays below serde_json's own recursion guard without disabling it.
pub const MAX_NESTING_DEPTH: usize = 64;

#[derive(Debug)]
pub enum InputError {
    InputTooLarge,
    InputExceedsLimit {
        max_bytes: usize,
    },
    NestingTooDeep,
    DuplicateObjectMember,
    /// Malformed JSON, invalid UTF-8, trailing input, or numbers outside
    /// serde_json's supported numeric range. Retains its line/column details.
    InvalidJson(serde_json::Error),
    /// An underlying reader failure, including failure to establish EOF.
    Io(io::Error),
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(formatter, "JSON input exceeds {MAX_INPUT_BYTES} bytes"),
            Self::InputExceedsLimit { max_bytes } => {
                write!(formatter, "JSON input exceeds {max_bytes} bytes")
            }
            Self::NestingTooDeep => {
                write!(
                    formatter,
                    "JSON nesting exceeds {MAX_NESTING_DEPTH} containers"
                )
            }
            Self::DuplicateObjectMember => formatter.write_str("duplicate JSON object member"),
            Self::InvalidJson(error) => write!(formatter, "invalid JSON: {error}"),
            Self::Io(error) => write!(formatter, "could not read JSON input: {error}"),
        }
    }
}

impl std::error::Error for InputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Parse exactly one JSON value after checking the byte limit, before parsing
/// or allocating its tree. Object names are compared after JSON escape decoding;
/// duplicate names in the same object are rejected, never silently overwritten.
/// Numeric representation follows the existing serde_json configuration.
///
/// Oversize input takes precedence over syntax errors. Otherwise the first
/// encountered syntax, duplicate-member or depth error is returned.
pub fn parse_json_bytes(bytes: &[u8]) -> Result<Value, InputError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(InputError::InputTooLarge);
    }
    parse_json_bytes_with_limit(bytes, MAX_INPUT_BYTES)
}

/// Parse a local envelope under an explicit caller-owned byte cap. This does
/// not alter the 1 MiB proposal/source intake API or its reader limit.
pub(crate) fn parse_json_bytes_with_limit(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<Value, InputError> {
    if bytes.len() > max_bytes {
        return Err(InputError::InputExceedsLimit { max_bytes });
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let mut violation = None;
    let result = ValueSeed {
        depth: 0,
        violation: &mut violation,
        paid: None,
    }
    .deserialize(&mut deserializer);
    let value = result.map_err(|error| violation.unwrap_or(InputError::InvalidJson(error)))?;
    deserializer.end().map_err(InputError::InvalidJson)?;
    Ok(value)
}

/// Read a bounded document, then apply [`parse_json_bytes`].
///
/// Reads at most `MAX_INPUT_BYTES + 1` bytes, including a single excess-byte
/// probe. The cap is applied to the reader before `read_to_end`, so even a source
/// that never reaches EOF cannot cause unbounded buffering. At the exact limit,
/// EOF is required; an error from that probe is preserved as an I/O error.
/// Interruptions are retried by the standard reader. This does not impose a
/// timeout on a blocking reader; callers own cancellation and source selection.
/// Read errors take precedence over parsing the incomplete buffered prefix.
pub fn read_json(reader: impl Read) -> Result<Value, InputError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(InputError::Io)?;
    parse_json_bytes(&bytes)
}

// Use serde's tokenizer and syntax checks, customizing only container traversal
// so depth and duplicate members are checked before accepting nested values.
struct ValueSeed<'v, 'w> {
    depth: usize,
    // Preserve typed intake errors through serde's generic error interface.
    violation: &'v mut Option<InputError>,
    paid: Option<&'w std::cell::RefCell<dyn capture_paid::IntakeCharge + 'w>>,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl ValueSeed<'_, '_> {
    fn check_container<E: de::Error>(&mut self) -> Result<(), E> {
        self.pay::<E>(2)?;
        if self.depth >= MAX_NESTING_DEPTH {
            *self.violation = Some(InputError::NestingTooDeep);
            return Err(E::custom("JSON nesting limit exceeded"));
        }
        Ok(())
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        self.copy_string::<E>(value.len())?;
        self.visit_string(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        self.pay::<E>(2)?;
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut sequence: A) -> Result<Value, A::Error> {
        self.check_container()?;
        self.pay::<A::Error>(3)?;
        let mut values = Vec::new();
        loop {
            self.pay::<A::Error>(3)?;
            let Some(value) = sequence.next_element_seed(ValueSeed {
            depth: self.depth + 1,
            violation: self.violation,
            paid: self.paid,
        })? else { break };
            self.push::<A::Error>(&mut values, value)?;
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut object: A) -> Result<Value, A::Error> {
        self.check_container()?;
        self.pay::<A::Error>(9)?;
        let mut values = Map::new();
        loop {
            self.pay::<A::Error>(1)?;
            let Some(key) = object.next_key::<String>()? else { break };
            self.map_search::<A::Error>(values.len(), &key)?;
            if values.contains_key(&key) {
                *self.violation = Some(InputError::DuplicateObjectMember);
                return Err(de::Error::custom("duplicate JSON object member"));
            }
            self.pay::<A::Error>(3)?;
            let value = object.next_value_seed(ValueSeed {
                depth: self.depth + 1,
                violation: self.violation,
                paid: self.paid,
            })?;
            self.map_insert::<A::Error>(values.len(), &key)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

#[cfg(test)]
#[path = "product_contract/capture_input_stage_tests.rs"]
mod capture_input_stage_tests;
