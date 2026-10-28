//! Fallible structural codec for the actual persisted Pointer.
//!
//! Deliberately unregistered in production: this does not route CURRENT reads,
//! writes, upgrades or diagnostics. Callers must still fund all work from their
//! original budget; measuring output creates no credit or new input limit.
use super::pointer::Pointer;
use crate::product_alloc::{AllocationContext, AllocationSite, FallibleStringWriter, FormatFailure};
use crate::product_app::{Expected, SchemaFailure, UnexpectedOwned};
use crate::product_contract::Digest;
use crate::product_json::{self, canonical, Number, Value};
use std::fmt::{self, Write};

const FIELDS: &[&str] = &["magic", "version", "project_id", "revision", "object"];

#[derive(Debug)]
pub(super) enum PointerDecodeError {
    Json(product_json::Error),
    Schema(SchemaFailure),
    InvalidDigest,
}
impl From<SchemaFailure> for PointerDecodeError {
    fn from(error: SchemaFailure) -> Self { Self::Schema(error) }
}
impl fmt::Display for PointerDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => fmt::Display::fmt(error, f),
            Self::Schema(error) => fmt::Display::fmt(error, f),
            Self::InvalidDigest => f.write_str("invalid SHA-256 digest"),
        }
    }
}
impl PointerDecodeError {
    pub(super) fn try_render(&self, cx: &mut AllocationContext) -> Result<String, FormatFailure> {
        let mut output = String::new();
        self.try_append_diagnostic(&mut output, cx)?;
        Ok(output)
    }

    /// Refusal rolls back appended text, leaving both prefix and cause intact.
    pub(super) fn try_append_diagnostic(&self, output: &mut String,
        cx: &mut AllocationContext) -> Result<(), FormatFailure> {
        let mut writer = FallibleStringWriter::new(output, cx, AllocationSite::Formatting);
        // Closed formatting graph: inline JSON/schema causes and this static
        // digest cause only, with their already-qualified primitive formatters.
        let result = write!(&mut writer, "{self}");
        writer.finish(result)
    }
}

fn unexpected(value: Value) -> UnexpectedOwned {
    match value {
        Value::Null => UnexpectedOwned::Unit,
        Value::Bool(value) => UnexpectedOwned::Bool(value),
        Value::Number(Number::U64(value)) => UnexpectedOwned::U64(value),
        Value::Number(Number::I64(value)) => UnexpectedOwned::I64(value),
        Value::Number(Number::F64(value)) => UnexpectedOwned::F64(value),
        Value::String(value) => UnexpectedOwned::String(value),
        Value::Array(_) => UnexpectedOwned::Sequence,
        Value::Object(_) => UnexpectedOwned::Map,
    }
}
fn invalid_type(value: Value, expected: &'static str) -> PointerDecodeError {
    SchemaFailure::InvalidType { actual: unexpected(value), expected: Expected::Text(expected) }.into()
}
fn string(value: Value) -> Result<String, PointerDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}
fn unsigned(value: Value, maximum: u64, expected: &'static str) -> Result<u64, PointerDecodeError> {
    match value {
        Value::Number(Number::U64(value)) if value <= maximum => Ok(value),
        Value::Number(Number::I64(value)) if value >= 0 && value as u64 <= maximum => Ok(value as u64),
        value @ Value::Number(Number::U64(_) | Number::I64(_)) => {
            Err(SchemaFailure::InvalidValue {
                actual: unexpected(value), expected: Expected::Text(expected),
            }.into())
        }
        value => Err(invalid_type(value, expected)),
    }
}
fn digest(value: Value) -> Result<Digest, PointerDecodeError> {
    Digest::from_owned_lower_hex(string(value)?).ok_or(PointerDecodeError::InvalidDigest)
}
fn next_element(values: &mut std::vec::IntoIter<Value>, length: usize) -> Result<Value, PointerDecodeError> {
    values.next().ok_or_else(|| SchemaFailure::InvalidLength {
        actual: length, expected: Expected::Structure { name: "Pointer", fields: 5 },
    }.into())
}

/// Only this parser feeds conversion: discarded containers have the accepted
/// store parser's at-most127-container depth and concrete standard owners.
pub(super) fn decode_pointer(bytes: &[u8], cx: &mut AllocationContext) -> Result<Pointer, PointerDecodeError> {
    let value = product_json::parse_store_json_bytes(bytes, cx).map_err(PointerDecodeError::Json)?;
    from_parsed(value)
}

// Private, never an arbitrary programmatic/deep Value entrypoint. After parsing
// there are zero growth requests: move strings and iterator storage, keeping
// scalar slots on the stack. Early returns drop prefixes and remainders once.
fn from_parsed(value: Value) -> Result<Pointer, PointerDecodeError> {
    match value {
        Value::Object(fields) => {
            let (mut magic, mut version, mut project_id, mut revision, mut object) =
                (None, None, None, None, None);
            for (key, value) in fields.into_entries() {
                // OrderedMap iteration is lexical, as in the old from_value.
                // Decode immediately; missing checks happen only after it ends.
                match key.as_str() {
                    "magic" => magic = Some(string(value)?),
                    "version" => version = Some(unsigned(value, u32::MAX as u64, "u32")? as u32),
                    "project_id" => project_id = Some(string(value)?),
                    "revision" => revision = Some(unsigned(value, u64::MAX, "u64")?),
                    "object" => object = Some(digest(value)?),
                    _ => return Err(SchemaFailure::UnknownField { key, expected: FIELDS }.into()),
                }
            }
            Ok(Pointer {
                magic: magic.ok_or(SchemaFailure::MissingField("magic"))?,
                version: version.ok_or(SchemaFailure::MissingField("version"))?,
                project_id: project_id.ok_or(SchemaFailure::MissingField("project_id"))?,
                revision: revision.ok_or(SchemaFailure::MissingField("revision"))?,
                object: object.ok_or(SchemaFailure::MissingField("object"))?,
            })
        }
        Value::Array(values) => {
            let length = values.len();
            let mut values = values.into_iter();
            let magic = string(next_element(&mut values, length)?)?;
            let version = unsigned(next_element(&mut values, length)?, u32::MAX as u64, "u32")? as u32;
            let project_id = string(next_element(&mut values, length)?)?;
            let revision = unsigned(next_element(&mut values, length)?, u64::MAX, "u64")?;
            let object = digest(next_element(&mut values, length)?)?;
            if length > 5 {
                return Err(SchemaFailure::InvalidLength {
                    actual: length, expected: Expected::Text("fewer elements in array"),
                }.into());
            }
            Ok(Pointer { magic, version, project_id, revision, object })
        }
        value => Err(invalid_type(value, "struct Pointer")),
    }
}

fn emit(pointer: &Pointer, sink: &mut canonical::Sink<'_>) -> Result<(), canonical::EncodeError> {
    sink.append(b"{\"magic\":")?;
    canonical::string(&pointer.magic, sink)?;
    sink.append(b",\"object\":")?;
    canonical::string(pointer.object.as_str(), sink)?;
    sink.append(b",\"project_id\":")?;
    canonical::string(&pointer.project_id, sink)?;
    sink.append(b",\"revision\":")?;
    canonical::number(&Number::U64(pointer.revision), sink)?;
    sink.append(b",\"version\":")?;
    canonical::number(&Number::U64(pointer.version as u64), sink)?;
    sink.append(b"}")
}

/// Two fixed borrowed walks and exactly one output reservation. No owned Value,
/// generic Serialize, traversal workspace, cloning or diagnostic fallback.
pub(super) fn encode_pointer(pointer: &Pointer, cx: &mut AllocationContext) -> Result<Vec<u8>, canonical::EncodeError> {
    let mut measure = canonical::Sink::measure();
    emit(pointer, &mut measure)?;
    let length = measure.position();
    let mut output = canonical::reserve_output(length, cx)?;
    let mut sink = canonical::Sink::emit(&mut output);
    emit(pointer, &mut sink)?;
    sink.finish(length)?;
    Ok(output)
}
