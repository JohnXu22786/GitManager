//! Fallible structural decoding for persisted LocalArtifact values.
//!
//! The private DTO uses the existing fallible field/value leaves and keeps
//! semantic artifact validation in the existing caller.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};
use serde::Serialize;

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

    fn artifacts() -> [LocalArtifact; 2] {
        let columns = vec![text_column("name")];
        let rows = vec![BTreeMap::from([("name".into(), super::super::DataValue::Text { value: "Ada, 雪".into() })])];
        [
            LocalArtifact::from_rows("roster", OutputFormat::Csv, columns.clone(), rows.clone()).unwrap(),
            LocalArtifact::from_rows("roster", OutputFormat::Json, columns, rows).unwrap(),
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
        let [artifact, _] = artifacts();
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
        let [artifact, _] = artifacts();
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
