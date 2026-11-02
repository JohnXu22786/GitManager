//! Fallible structural decoding for persisted LocalArtifact values.
//!
//! The private DTO composes fallible field/value leaves and validates output
//! receipts without converting their ordered rows back to legacy maps.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};
use serde::ser::{SerializeMap, SerializeTuple};
use serde::{Serialize, Serializer};

use super::fallible_entity_decode::decode_field_definition;
use super::fallible_value_decode::{decode_values, JsonKind, NumericValue, ValueDecodeError};
use super::{Digest, FallibleValues, FieldDefinition, OutputFormat, MAX_COLLECTION, MAX_ITEMS, MAX_OUTPUT_BYTES};

const LOCAL_ARTIFACT_FIELDS: &[&str] = &["output", "format", "columns", "rows", "bytes", "bytes_digest", "digest"];
const OUTPUT_FORMATS: &[&str] = &["csv", "json"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedLocalArtifact {
    output: String,
    format: OutputFormat,
    columns: Vec<FieldDefinition>,
    rows: Vec<FallibleValues>,
    bytes: Vec<u8>,
    bytes_digest: Digest,
    digest: Digest,
}

impl DecodedLocalArtifact {
    pub(crate) fn digest(&self) -> &Digest { &self.digest }
    pub(crate) fn byte_len(&self) -> usize { self.bytes.len() }

    pub(crate) fn validate_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<(), super::AppValidationError> {
        use super::{AppValidationError, ContractError, IdentityDomain};
        let contract = |message: &'static str| AppValidationError::Contract(ContractError(message.into()));
        super::check_id(&self.output).map_err(AppValidationError::Contract)?;
        validate_columns(&self.columns, cx)?;
        if self.columns.is_empty() || self.rows.len() > MAX_COLLECTION || self.bytes.len() > MAX_OUTPUT_BYTES {
            return Err(contract("artifact size/columns invalid"));
        }
        for row in &self.rows {
            if row.len() != self.columns.len() || self.columns.iter().any(|column| !row.contains_key(&column.id)) {
                return Err(contract("artifact row columns mismatch"));
            }
            for column in &self.columns {
                let value = row.get(&column.id).expect("row keys were checked");
                super::validate_value(value, &column.value_type, 0).map_err(AppValidationError::Contract)?;
                super::validate_untyped_value(value, 0).map_err(AppValidationError::Contract)?;
                if matches!(value, super::DataValue::List { .. } | super::DataValue::Null) {
                    return Err(contract("artifact cells must be scalar"));
                }
            }
        }
        let bytes_digest = crate::product_backup::fallible_codec::bytes_digest(IdentityDomain::OutputBytes, &self.bytes, cx)
            .map_err(codec_error)?;
        if self.bytes_digest != bytes_digest { return Err(contract("artifact byte digest mismatch")); }
        let encoded = encode_output_fallible(self.format, &self.columns, &self.rows, cx)?;
        if self.bytes != encoded { return Err(contract("artifact bytes do not encode observed rows")); }
        let receipt = crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Output,
            &ArtifactReceipt(&self.output, self.format, &self.columns, &self.rows, &self.bytes_digest),
            cx,
        ).map_err(codec_error)?;
        if self.digest != receipt { return Err(contract("artifact semantic receipt mismatch")); }
        Ok(())
    }
}

fn codec_error(error: crate::product_backup::fallible_codec::CodecError) -> super::AppValidationError {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => super::AppValidationError::Allocation(failure),
        crate::product_backup::fallible_codec::CodecError::OutputLimitExceeded => output_limit_error(),
        _ => super::AppValidationError::Contract(super::ContractError("artifact codec invariant".into())),
    }
}

fn validate_columns(
    columns: &[FieldDefinition],
    cx: &mut AllocationContext,
) -> Result<(), super::AppValidationError> {
    let mut ids = crate::product_alloc::ordered::OrderedSet::new();
    for column in columns {
        super::check_id(&column.id).map_err(super::AppValidationError::Contract)?;
        if !ids.try_insert(column.id.as_str(), cx).map_err(super::AppValidationError::Allocation)? {
            return Err(super::AppValidationError::Contract(super::ContractError(format!("duplicate ID: {}", column.id))));
        }
    }
    if ids.len() > MAX_ITEMS {
        return Err(super::AppValidationError::Contract(super::ContractError("too many definitions or steps".into())));
    }
    if columns.is_empty() {
        return Err(super::AppValidationError::Contract(super::ContractError("output needs declared columns".into())));
    }
    for column in columns {
        super::nonempty(&column.label).map_err(super::AppValidationError::Contract)?;
        super::standalone_type(&column.value_type, 0).map_err(super::AppValidationError::Contract)?;
        if !column.value_type.is_scalar() {
            return Err(super::AppValidationError::Contract(super::ContractError("output column type must be scalar".into())));
        }
    }
    Ok(())
}

fn encode_output_fallible(
    format: super::OutputFormat,
    columns: &[FieldDefinition],
    rows: &[FallibleValues],
    cx: &mut AllocationContext,
) -> Result<Vec<u8>, super::AppValidationError> {
    use super::{AppValidationError, ContractError, OutputFormat};
    validate_columns(columns, cx)?;
    if rows.len() > MAX_COLLECTION { return Err(AppValidationError::Contract(ContractError("invalid output dimensions".into()))); }
    let mut bytes = Vec::new();
    if format == OutputFormat::Csv {
        for (index, column) in columns.iter().enumerate() {
            if index > 0 { append(&mut bytes, b",", cx)?; }
            append_csv_field(&mut bytes, &column.id, cx)?;
        }
        append(&mut bytes, b"\r\n", cx)?;
    } else {
        append(&mut bytes, b"[", cx)?;
    }
    for (index, row) in rows.iter().enumerate() {
        if row.len() != columns.len() || columns.iter().any(|column| !row.contains_key(&column.id)) {
            return Err(AppValidationError::Contract(ContractError("output row columns mismatch".into())));
        }
        if format == OutputFormat::Csv {
            for (column_index, column) in columns.iter().enumerate() {
                if column_index > 0 { append(&mut bytes, b",", cx)?; }
                let value = row.get(&column.id).expect("row keys were checked");
                append_csv_value(&mut bytes, value, cx)?;
            }
            append(&mut bytes, b"\r\n", cx)?;
        } else {
            if index > 0 { append(&mut bytes, b",", cx)?; }
            let mut object = crate::product_alloc::ordered::OrderedMap::new();
            for column in columns {
                let value = row.get(&column.id).expect("row keys were checked");
                object.try_insert(column.id.as_str(), value, cx).map_err(AppValidationError::Allocation)?;
            }
            let remaining = MAX_OUTPUT_BYTES.checked_sub(bytes.len())
                .and_then(|remaining| remaining.checked_sub(1))
                .ok_or_else(output_limit_error)?;
            let encoded = crate::product_backup::fallible_codec::canonical_bytes_bounded(
                &OutputRow(&object), remaining, cx,
            ).map_err(codec_error)?;
            append(&mut bytes, &encoded, cx)?;
        }
    }
    if format == OutputFormat::Json { append(&mut bytes, b"]", cx)?; }
    Ok(bytes)
}

fn append(bytes: &mut Vec<u8>, value: &[u8], cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    if !matches!(bytes.len().checked_add(value.len()), Some(length) if length <= MAX_OUTPUT_BYTES) {
        return Err(output_limit_error());
    }
    crate::product_alloc::try_reserve(bytes, value.len(), cx, AllocationSite::Bytes).map_err(super::AppValidationError::Allocation)?;
    bytes.extend_from_slice(value);
    Ok(())
}

fn output_limit_error() -> super::AppValidationError {
    super::AppValidationError::Contract(super::ContractError("output byte limit exceeded".into()))
}

fn append_csv_field(bytes: &mut Vec<u8>, value: &str, cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    append_csv_quoted_with_literal_prefix(
        bytes,
        value,
        value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']),
        cx,
    )
}

fn append_csv_value(bytes: &mut Vec<u8>, value: &super::DataValue, cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    use super::{AppValidationError, ContractError, DataValue};
    let mut rendered = Vec::new();
    match value {
        DataValue::Text { value } => {
            if value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) { append(&mut rendered, b"'", cx)?; }
            append(&mut rendered, value.as_bytes(), cx)?;
            append_csv_quoted(bytes, std::str::from_utf8(&rendered).expect("UTF-8 source preserved"), cx)?;
        }
        DataValue::Boolean { value } => append_csv_quoted(bytes, if *value { "true" } else { "false" }, cx)?,
        DataValue::Integer { value } => {
            append_i64_csv(bytes, *value, cx)?;
        }
        DataValue::Date { days } => {
            let date = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                .and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(i64::from(*days))))
                .ok_or_else(|| AppValidationError::Contract(ContractError("date overflow".into())))?;
            append_date_csv(bytes, date, cx)?;
        }
        DataValue::Reference { .. } => {
            let encoded = crate::product_backup::fallible_codec::canonical_bytes(&JsonScalar(value), cx).map_err(codec_error)?;
            append_csv_quoted(bytes, std::str::from_utf8(&encoded).expect("canonical JSON is UTF-8"), cx)?;
        }
        DataValue::Null | DataValue::List { .. } => return Err(AppValidationError::Contract(ContractError("output cells must be scalar".into()))),
    }
    Ok(())
}

fn append_csv_quoted(bytes: &mut Vec<u8>, value: &str, cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    append_csv_quoted_with_literal_prefix(bytes, value, false, cx)
}

fn append_csv_quoted_with_literal_prefix(
    bytes: &mut Vec<u8>,
    value: &str,
    literal_prefix: bool,
    cx: &mut AllocationContext,
) -> Result<(), super::AppValidationError> {
    if value.contains([',', '"', '\r', '\n']) {
        append(bytes, b"\"", cx)?;
        if literal_prefix { append(bytes, b"'", cx)?; }
        for chunk in value.split_inclusive('"') {
            if let Some(prefix) = chunk.strip_suffix('"') {
                append(bytes, prefix.as_bytes(), cx)?;
                append(bytes, b"\"\"", cx)?;
            } else {
                append(bytes, chunk.as_bytes(), cx)?;
            }
        }
        append(bytes, b"\"", cx)?;
    } else {
        if literal_prefix { append(bytes, b"'", cx)?; }
        append(bytes, value.as_bytes(), cx)?;
    }
    Ok(())
}

pub(crate) struct OutputRow<'a>(pub(crate) &'a crate::product_alloc::ordered::OrderedMap<&'a str, &'a super::DataValue>);
impl Serialize for OutputRow<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, &JsonScalar(value))?; }
        map.end()
    }
}
pub(crate) struct JsonScalar<'a>(pub(crate) &'a super::DataValue);
impl Serialize for JsonScalar<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            super::DataValue::Text { value } => serializer.serialize_str(value),
            super::DataValue::Boolean { value } => serializer.serialize_bool(*value),
            super::DataValue::Integer { value } => serializer.serialize_i64(*value),
            super::DataValue::Date { days } => {
                let date = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                    .and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(i64::from(*days))))
                    .ok_or_else(|| serde::ser::Error::custom("date overflow"))?;
                let mut rendered = StackString::<16>::new();
                std::fmt::Write::write_fmt(&mut rendered, format_args!("{date}"))
                    .map_err(serde::ser::Error::custom)?;
                serializer.serialize_str(rendered.as_str())
            }
            super::DataValue::Reference { entity, record } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("entity", entity)?;
                map.serialize_entry("record", record)?;
                map.end()
            }
            super::DataValue::Null | super::DataValue::List { .. } => Err(serde::ser::Error::custom("output cells must be scalar")),
        }
    }
}

struct StackString<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> StackString<N> {
    fn new() -> Self { Self { bytes: [0; N], len: 0 } }
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("date display is UTF-8")
    }
}
impl<const N: usize> std::fmt::Write for StackString<N> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(std::fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        destination.copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub(crate) struct ArtifactReceipt<'a>(pub(crate) &'a str, pub(crate) super::OutputFormat, pub(crate) &'a [FieldDefinition], pub(crate) &'a [FallibleValues], pub(crate) &'a Digest);
impl Serialize for ArtifactReceipt<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(5)?;
        tuple.serialize_element(self.0)?;
        tuple.serialize_element(&self.1)?;
        tuple.serialize_element(self.2)?;
        tuple.serialize_element(self.3)?;
        tuple.serialize_element(self.4)?;
        tuple.end()
    }
}

fn append_i64_csv(bytes: &mut Vec<u8>, value: i64, cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    let mut buffer = [0u8; 20];
    let negative = value < 0;
    let mut number = value.unsigned_abs();
    let mut index = buffer.len();
    loop {
        index -= 1;
        buffer[index] = b'0' + (number % 10) as u8;
        number /= 10;
        if number == 0 { break; }
    }
    if negative { index -= 1; buffer[index] = b'-'; }
    append_csv_quoted(bytes, std::str::from_utf8(&buffer[index..]).expect("ASCII integer"), cx)
}

fn append_date_csv(bytes: &mut Vec<u8>, date: chrono::NaiveDate, cx: &mut AllocationContext) -> Result<(), super::AppValidationError> {
    let mut rendered = StackString::<16>::new();
    std::fmt::Write::write_fmt(&mut rendered, format_args!("{date}"))
        .map_err(|_| super::AppValidationError::Contract(super::ContractError("date formatting invariant".into())))?;
    append_csv_quoted(bytes, rendered.as_str(), cx)
}

pub(crate) fn decode_local_artifact(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedLocalArtifact, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct LocalArtifact", LOCAL_ARTIFACT_FIELDS)?;
    let mut output = None;
    let mut format = None;
    let mut columns = None;
    let mut rows = None;
    let mut bytes = None;
    let mut bytes_digest = None;
    let mut digest_value = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "output" => output = Some(string(value, "a string")?),
            "format" => format = Some(output_format(value)?),
            "columns" => columns = Some(decode_columns(value, cx)?),
            "rows" => rows = Some(decode_rows(value, cx)?),
            "bytes" => bytes = Some(decode_bytes(value, cx)?),
            "bytes_digest" => bytes_digest = Some(digest(value)?),
            "digest" => digest_value = Some(digest(value)?),
            _ => return Err(unknown_field(key, LOCAL_ARTIFACT_FIELDS)),
        }
    }
    Ok(DecodedLocalArtifact {
        output: required(output, "output")?,
        format: required(format, "format")?,
        columns: required(columns, "columns")?,
        rows: required(rows, "rows")?,
        bytes: required(bytes, "bytes")?,
        bytes_digest: required(bytes_digest, "bytes_digest")?,
        digest: required(digest_value, "digest")?,
    })
}

fn output_format(value: Value) -> Result<OutputFormat, ValueDecodeError> {
    let (tag, unit) = match value {
        Value::String(tag) => (tag, None),
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, unit) = fields.into_entries().next().expect("one-entry map was checked");
            (tag, Some(unit))
        }
        value => return Err(invalid_type(value, "a string variant identifier or unit variant map")),
    };
    match tag.as_str() {
        "csv" => {
            if let Some(unit) = unit { if !matches!(unit, Value::Null) { return Err(invalid_type(unit, "unit")); } }
            Ok(OutputFormat::Csv)
        }
        "json" => {
            if let Some(unit) = unit { if !matches!(unit, Value::Null) { return Err(invalid_type(unit, "unit")); } }
            Ok(OutputFormat::Json)
        }
        _ => Err(ValueDecodeError::UnknownVariant { tag, expected: OUTPUT_FORMATS }),
    }
}

fn decode_columns(value: Value, cx: &mut AllocationContext) -> Result<Vec<FieldDefinition>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    bounded(values.len(), MAX_ITEMS, "artifact column count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_field_definition(value, cx)?);
    }
    Ok(decoded)
}

fn decode_rows(value: Value, cx: &mut AllocationContext) -> Result<Vec<FallibleValues>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    bounded(values.len(), MAX_COLLECTION, "artifact row count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_values(value, cx)?);
    }
    Ok(decoded)
}

fn decode_bytes(value: Value, cx: &mut AllocationContext) -> Result<Vec<u8>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    bounded(values.len(), MAX_OUTPUT_BYTES, "artifact byte count")?;
    let mut bytes = Vec::new();
    product_alloc::try_reserve(&mut bytes, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        bytes.push(unsigned_u8(value)?);
    }
    Ok(bytes)
}

fn unsigned_u8(value: Value) -> Result<u8, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u8::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u8" }),
        Value::Number(Number::I64(value)) => u8::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u8" }),
        value => Err(invalid_type(value, "u8")),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value, "a lowercase 64-character digest")?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

enum StructFields {
    Object(std::vec::IntoIter<(String, Value)>),
    Sequence { names: &'static [&'static str], values: std::vec::IntoIter<Value>, index: usize, actual: usize },
}
impl StructFields {
    fn next(&mut self, cx: &mut AllocationContext) -> Result<Option<(String, Value)>, ValueDecodeError> {
        match self {
            Self::Object(fields) => Ok(fields.next()),
            Self::Sequence { names, values, index, actual } => {
                if let Some(name) = names.get(*index) {
                    let Some(value) = values.next() else {
                        return Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() });
                    };
                    *index += 1;
                    let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                    Ok(Some((name, value)))
                } else if values.next().is_some() {
                    Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() })
                } else {
                    Ok(None)
                }
            }
        }
    }
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str]) -> Result<StructFields, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(StructFields::Object(fields.into_entries())),
        Value::Array(values) => {
            let actual = values.len();
            Ok(StructFields::Sequence { names, values: values.into_iter(), index: 0, actual })
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn bounded(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationFailureKind;
    use crate::product_json::{self, Cause};
    use super::super::LocalArtifact;
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    fn syntax(source: &[u8]) -> Value {
        product_json::parse_json_bytes(source, &mut AllocationContext::new()).unwrap()
    }

    fn text_column(id: &str) -> FieldDefinition {
        FieldDefinition { id: id.into(), label: format!("{id} label"), value_type: super::super::Type::Text }
    }

    fn artifacts() -> [LocalArtifact; 6] {
        let columns = vec![text_column("name")];
        let rows = vec![BTreeMap::from([("name".into(), super::super::DataValue::Text { value: "Ada, 雪".into() })])];
        let formula_header = LocalArtifact::from_rows(
            "roster",
            OutputFormat::Csv,
            vec![text_column("-amount"), text_column("normal")],
            vec![BTreeMap::from([
                ("-amount".into(), super::super::DataValue::Text { value: "safe".into() }),
                ("normal".into(), super::super::DataValue::Text { value: "value".into() }),
            ])],
        ).unwrap();
        let date_json = LocalArtifact::from_rows(
            "dates",
            OutputFormat::Json,
            vec![FieldDefinition { id: "day".into(), label: "Day".into(), value_type: super::super::Type::Date }],
            vec![BTreeMap::from([("day".into(), super::super::DataValue::Date { days: 0 })])],
        ).unwrap();
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        let extended_csv = |output: &str, year| {
            let date = chrono::NaiveDate::from_ymd_opt(year, 2, 3).unwrap();
            let days = date.signed_duration_since(epoch).num_days() as i32;
            LocalArtifact::from_rows(
                output,
                OutputFormat::Csv,
                vec![FieldDefinition { id: "day".into(), label: "Day".into(), value_type: super::super::Type::Date }],
                vec![BTreeMap::from([("day".into(), super::super::DataValue::Date { days })])],
            ).unwrap()
        };
        [
            LocalArtifact::from_rows("roster", OutputFormat::Csv, columns.clone(), rows.clone()).unwrap(),
            LocalArtifact::from_rows("roster", OutputFormat::Json, columns, rows).unwrap(),
            formula_header,
            date_json,
            extended_csv("date_max", 9999),
            extended_csv("date_min", 1),
        ]
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
    }

    #[test]
    fn existing_csv_json_artifacts_preserve_canonical_bytes_and_receipts() {
        for artifact in artifacts() {
            if artifact.columns.first().is_some_and(|column| column.id == "-amount") {
                assert_eq!(artifact.bytes, b"'-amount,normal\r\nsafe,value\r\n");
            }
            if artifact.output == "dates" {
                assert_eq!(artifact.bytes, br#"[{"day":"1970-01-01"}]"#);
            }
            if artifact.output == "date_max" || artifact.output == "date_min" {
                let date = if artifact.output == "date_max" { "9999-02-03" } else { "0001-02-03" };
                assert_eq!(artifact.bytes, format!("day\r\n{date}\r\n").as_bytes());
            }
            let input = serde_json::to_vec(&artifact).unwrap();
            let mut cx = AllocationContext::new();
            let decoded = decode_local_artifact(syntax(&input), &mut cx).unwrap();
            assert_eq!(artifact.bytes, decoded.bytes);
            assert_eq!(artifact.bytes_digest.as_str(), decoded.bytes_digest.as_str());
            assert_eq!(artifact.digest.as_str(), decoded.digest.as_str());
            assert_eq!(super::super::canonical_bytes(&artifact).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        }
    }

    #[test]
    fn output_validation_preserves_legacy_bytes_and_allocation_refusal() {
        for artifact in artifacts() {
            artifact.validate().unwrap();
            let input = serde_json::to_vec(&artifact).unwrap();
            let decoded = decode_local_artifact(syntax(&input), &mut AllocationContext::new()).unwrap();
            let mut baseline = AllocationContext::new();
            decoded.validate_with(&mut baseline).unwrap();
            let growths = baseline.growth_requests();
            assert!(growths > 0);
            for ordinal in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
                let error = decoded.validate_with(&mut cx).unwrap_err();
                let super::super::AppValidationError::Allocation(failure) = error else {
                    panic!("artifact allocation refusal {ordinal}/{growths} was not preserved")
                };
                assert_eq!(Some(failure), cx.first_failure());
                decoded.validate_with(&mut AllocationContext::new()).unwrap();
            }
        }
    }

    #[test]
    fn output_validation_preserves_legacy_column_error_order() {
        for bad_id in [false, true] {
            let mut value = serde_json::to_value(artifacts()[0].clone()).unwrap();
            if bad_id {
                value["columns"][0]["id"] = serde_json::json!("Upper");
            } else {
                let first_column = value["columns"][0].clone();
                value["columns"].as_array_mut().unwrap().push(first_column);
            }
            let legacy: LocalArtifact = serde_json::from_value(value.clone()).unwrap();
            let expected = legacy.validate().unwrap_err();
            let decoded = decode_local_artifact(syntax(&serde_json::to_vec(&value).unwrap()), &mut AllocationContext::new()).unwrap();
            let super::super::AppValidationError::Contract(actual) = decoded.validate_with(&mut AllocationContext::new()).unwrap_err() else {
                panic!("column semantic errors remain contract errors")
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn oversized_csv_and_json_output_stop_at_the_configured_byte_limit() {
        use super::super::{AppValidationError, ContractError, MAX_TEXT_BYTES};

        let mut setup = AllocationContext::new();
        let csv_columns = vec![text_column("cell")];
        let mut csv_rows = Vec::new();
        for _ in 0..300 {
            let mut row = FallibleValues::new();
            row.try_insert(
                "cell".into(),
                super::super::DataValue::Text { value: "x".repeat(MAX_TEXT_BYTES) },
                &mut setup,
            ).unwrap();
            csv_rows.push(row);
        }
        let mut cx = AllocationContext::new();
        assert!(matches!(
            encode_output_fallible(OutputFormat::Csv, &csv_columns, &csv_rows, &mut cx),
            Err(AppValidationError::Contract(ContractError(message))) if message == "output byte limit exceeded"
        ));
        assert_eq!(cx.first_failure(), None);

        let json_columns: Vec<_> = (0..MAX_ITEMS)
            .map(|index| FieldDefinition {
                id: format!("{index:03}{}", "a".repeat(125)),
                label: "Column".into(),
                value_type: super::super::Type::Text,
            })
            .collect();
        let mut json_rows = Vec::new();
        for _ in 0..40 {
            let mut row = FallibleValues::new();
            for column in &json_columns {
                row.try_insert(
                    column.id.clone(),
                    super::super::DataValue::Text { value: "x".into() },
                    &mut setup,
                ).unwrap();
            }
            json_rows.push(row);
        }
        let mut cx = AllocationContext::new();
        assert!(matches!(
            encode_output_fallible(OutputFormat::Json, &json_columns, &json_rows, &mut cx),
            Err(AppValidationError::Contract(ContractError(message))) if message == "output byte limit exceeded"
        ));
        assert_eq!(cx.first_failure(), None);
    }

    #[test]
    fn output_format_accepts_both_derived_serde_unit_variant_forms() {
        for (wire, expected) in [
            (serde_json::json!("csv"), OutputFormat::Csv),
            (serde_json::json!({"csv": null}), OutputFormat::Csv),
            (serde_json::json!("json"), OutputFormat::Json),
            (serde_json::json!({"json": null}), OutputFormat::Json),
        ] {
            let oracle: OutputFormat = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(expected, oracle);
            let input = serde_json::to_vec(&wire).unwrap();
            assert_eq!(expected, output_format(syntax(&input)).unwrap());
        }
        assert!(matches!(
            output_format(syntax(br#"{"other":null}"#)),
            Err(ValueDecodeError::UnknownVariant { .. })
        ));
        assert!(matches!(
            output_format(syntax(br#"{"csv":null,"json":null}"#)),
            Err(ValueDecodeError::InvalidLength { actual: 2, expected: 1 })
        ));
        assert!(matches!(
            output_format(syntax(br#"{"missing":"payload"}"#)),
            Err(ValueDecodeError::UnknownVariant { .. })
        ));
        assert!(matches!(
            output_format(syntax(br#"{"csv":"payload"}"#)),
            Err(ValueDecodeError::InvalidType { expected: "unit", .. })
        ));
    }

    #[test]
    fn struct_sequence_form_preserves_derived_serde_field_order() {
        let [artifact, _, _, _, _, _] = artifacts();
        let object = serde_json::to_value(&artifact).unwrap();
        let serde_json::Value::Object(fields) = object else { unreachable!() };
        let sequence = serde_json::Value::Array(
            LOCAL_ARTIFACT_FIELDS
                .iter()
                .map(|name| fields.get(*name).unwrap().clone())
                .collect(),
        );
        let oracle: LocalArtifact = serde_json::from_value(sequence.clone()).unwrap();
        let input = serde_json::to_vec(&sequence).unwrap();
        let mut baseline = AllocationContext::new();
        let decoded = decode_local_artifact(syntax(&input), &mut baseline).unwrap();
        assert_eq!(
            super::super::canonical_bytes(&oracle).unwrap(),
            super::super::canonical_bytes(&decoded).unwrap()
        );
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_local_artifact(syntax(&input), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert!(decode_local_artifact(syntax(&input), &mut retry).is_ok());
        }
    }

    #[test]
    fn struct_sequence_rejects_missing_and_trailing_elements_like_from_value() {
        let mut cx = AllocationContext::new();
        assert!(matches!(
            decode_local_artifact(Value::Array(vec![Value::String("output".into())]), &mut cx),
            Err(ValueDecodeError::InvalidLength { actual: 1, expected: 7 })
        ));
        let [artifact, _, _, _, _, _] = artifacts();
        let serde_json::Value::Object(fields) = serde_json::to_value(&artifact).unwrap() else { unreachable!() };
        let mut trailing_values: Vec<_> = LOCAL_ARTIFACT_FIELDS.iter().map(|name| fields[*name].clone()).collect();
        trailing_values.push(serde_json::Value::Null);
        let trailing_source = serde_json::to_vec(&serde_json::Value::Array(trailing_values)).unwrap();
        let trailing = syntax(&trailing_source);
        assert!(matches!(
            decode_local_artifact(trailing, &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidLength { actual: 8, expected: 7 })
        ));
        assert!(serde_json::from_value::<LocalArtifact>(serde_json::json!([null,null,null,null,null,null,null,null])).is_err());

        let mut sequence = serde_json::json!(artifacts()[0]);
        let serde_json::Value::Object(fields) = sequence else { unreachable!() };
        let mut values: Vec<_> = LOCAL_ARTIFACT_FIELDS.iter().map(|name| fields[*name].clone()).collect();
        values[0] = serde_json::Value::Bool(false);
        values.push(serde_json::Value::Null);
        let sequence = serde_json::Value::Array(values);
        let serde_error = serde_json::from_value::<LocalArtifact>(sequence.clone()).unwrap_err().to_string();
        assert!(serde_error.contains("string"), "Serde validates the first field before trailing length: {serde_error}");
        let input = serde_json::to_vec(&sequence).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "a string", .. })));
    }

    #[test]
    fn nested_values_and_every_reached_growth_refuse_cleanly_then_retry() {
        let mut row = BTreeMap::new();
        row.insert("name".into(), super::super::DataValue::List {
            item_type: super::super::Type::Text,
            items: vec![super::super::DataValue::Text { value: "nested".into() }, super::super::DataValue::Text { value: "values".into() }],
        });
        let artifact = LocalArtifact {
            output: "nested".into(),
            format: OutputFormat::Json,
            columns: vec![text_column("name")],
            rows: vec![row],
            bytes: vec![b'[', b']'],
            bytes_digest: Digest::try_from("a".repeat(64)).unwrap(),
            digest: Digest::try_from("b".repeat(64)).unwrap(),
        };
        let input = serde_json::to_vec(&artifact).unwrap();
        let expected = super::super::canonical_bytes(&artifact).unwrap();
        let mut baseline = AllocationContext::new();
        decode_local_artifact(syntax(&input), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_local_artifact(syntax(&input), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            let decoded = decode_local_artifact(syntax(&input), &mut retry).unwrap();
            assert_eq!(expected, super::super::canonical_bytes(&decoded).unwrap());
        }
    }

    #[test]
    fn malformed_duplicate_unknown_range_and_dimension_errors_are_fixed() {
        let artifact = &artifacts()[0];
        let mut value = serde_json::to_value(artifact).unwrap();
        let serde_json::Value::Object(_) = value else { unreachable!() };
        let mut cx = AllocationContext::new();
        let duplicate = match product_json::parse_json_bytes(br#"{"output":"x","output":"y"}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate LocalArtifact member was accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);

        let mut missing = value.clone();
        if let serde_json::Value::Object(ref mut fields) = missing { fields.remove("bytes"); }
        let input = serde_json::to_vec(&missing).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("bytes"))));

        let mut unknown = value.clone();
        if let serde_json::Value::Object(ref mut fields) = unknown { fields.insert("future".into(), serde_json::Value::Null); }
        let input = serde_json::to_vec(&unknown).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let mut format = value.clone();
        if let serde_json::Value::Object(ref mut fields) = format { fields.insert("format".into(), serde_json::Value::String("xml".into())); }
        let input = serde_json::to_vec(&format).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));

        let mut bad_digest = value.clone();
        if let serde_json::Value::Object(ref mut fields) = bad_digest { fields.insert("digest".into(), serde_json::Value::String("bad".into())); }
        let input = serde_json::to_vec(&bad_digest).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("invalid lowercase digest"))));

        let mut bad_byte = value.clone();
        if let serde_json::Value::Object(ref mut fields) = bad_byte { fields.insert("bytes".into(), serde_json::json!([256])); }
        let input = serde_json::to_vec(&bad_byte).unwrap();
        assert!(matches!(decode_local_artifact(syntax(&input), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u8", .. })));

        assert!(matches!(decode_columns(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "artifact column count", limit: MAX_ITEMS })));
        assert!(matches!(decode_rows(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_COLLECTION + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "artifact row count", limit: MAX_COLLECTION })));
        assert!(matches!(decode_bytes(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_OUTPUT_BYTES + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "artifact byte count", limit: MAX_OUTPUT_BYTES })));
    }
}
