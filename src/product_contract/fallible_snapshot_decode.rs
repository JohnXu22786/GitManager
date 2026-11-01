//! Fallible structural decoding for persisted Product Contract snapshots.
//!
//! The DTO composes the bounded leaf decoders while keeping stored Values in
//! their fallible ordered-map representation. Semantic validation remains a
//! separate caller step.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_entity_decode::decode_entity_definition;
use super::fallible_persisted_decode::{decode_business_event, decode_record, DecodedBusinessEvent, DecodedRecord};
use super::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use super::{EntityDefinition, MAX_COLLECTION, MAX_EVENTS, MAX_ITEMS};

const DATA_SNAPSHOT_FIELDS: &[&str] = &["version", "project_id", "generation", "schema", "records", "events"];

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct DecodedDataSnapshot {
    pub(super) version: u32,
    pub(super) project_id: String,
    pub(super) generation: u64,
    pub(super) schema: Vec<EntityDefinition>,
    pub(super) records: Vec<DecodedRecord>,
    pub(super) events: Vec<DecodedBusinessEvent>,
}

pub(super) fn decode_data_snapshot(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedDataSnapshot, ValueDecodeError> {
    let fields = object(value, "a struct DataSnapshot")?;
    let mut version = None;
    let mut project_id = None;
    let mut generation = None;
    let mut schema = None;
    let mut record_values = None;
    let mut event_values = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "version" => version = Some(integer_u32(value)?),
            "project_id" => project_id = Some(string(value)?),
            "generation" => generation = Some(integer_u64(value)?),
            "schema" => schema = Some(entity_definitions(value, cx)?),
            "records" => record_values = Some(records(value, cx)?),
            "events" => event_values = Some(events(value, cx)?),
            _ => return Err(unknown_field(key)),
        }
    }
    Ok(DecodedDataSnapshot {
        version: required(version, "version")?,
        project_id: required(project_id, "project_id")?,
        generation: required(generation, "generation")?,
        schema: required(schema, "schema")?,
        records: required(record_values, "records")?,
        events: required(event_values, "events")?,
    })
}

fn entity_definitions(value: Value, cx: &mut AllocationContext) -> Result<Vec<EntityDefinition>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, "entity schema count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_entity_definition(value, cx)?);
    }
    Ok(decoded)
}

fn records(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedRecord>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_COLLECTION, "record count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_record(value, cx)?);
    }
    Ok(decoded)
}

fn events(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedBusinessEvent>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_EVENTS, "business event count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_business_event(value, cx)?);
    }
    Ok(decoded)
}

fn object(
    value: Value,
    expected: &'static str,
) -> Result<crate::product_alloc::ordered::OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        value => Err(invalid_type(value, expected)),
    }
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn string(value: Value) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}

fn integer_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }
        }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }
        }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn integer_u64(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }
        }),
        value => Err(invalid_type(value, "u64")),
    }
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

fn unknown_field(key: String) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected: DATA_SNAPSHOT_FIELDS }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn bounded_length(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit {
        Err(ValueDecodeError::LimitExceeded { resource, limit })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::{AllocationFailureKind, AllocationSite};
    use crate::product_contract::DataSnapshot;
    use crate::product_json::{self, Cause};
    use std::num::NonZeroUsize;

    const D: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
        serde_json::from_str(source).unwrap()
    }

    fn allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
        assert!(matches!(failure.site, AllocationSite::Vector | AllocationSite::OrderedMap | AllocationSite::Box));
    }

    fn snapshot_source() -> String {
        format!(r#"{{"version":1,"project_id":"project","generation":2,"schema":[{{"id":"person","label":"Person","fields":[{{"id":"name","label":"Name","value_type":{{"kind":"text"}}}},{{"id":"items","label":"Items","value_type":{{"kind":"list","item":{{"kind":"text"}}}}}}],"unique":[["name"]],"constraints":[{{"kind":"equal","left":{{"kind":"field","record":{{"kind":"variable","name":"record"}},"field":"name"}},"right":{{"kind":"literal","value_type":{{"kind":"text"}},"value":{{"kind":"text","value":"Ada"}}}}}}]}}],"records":[{{"entity":"person","id":"p1","revision":1,"created_program":"{D}","archived":false,"values":{{"items":{{"kind":"list","item_type":{{"kind":"text"}},"items":[{{"kind":"text","value":"雪"}}]}},"name":{{"kind":"text","value":"Ada"}}}}}}],"events":[{{"id":"event1","sequence":1,"operation_id":"op1","action":"edit","day":20,"program":"{D}","changes":[{{"entity":"person","record":"p1","before":null,"after":{{"name":{{"kind":"text","value":"Ada"}}}},"archived":false}}],"outputs":["{D}"]}}]}}"#)
    }

    #[test]
    fn nested_snapshot_dto_preserves_canonical_wire_bytes() {
        let source = snapshot_source();
        let expected: DataSnapshot = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_data_snapshot(syntax(&source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert_eq!(decoded.version, super::super::CONTRACT_VERSION);
    }

    #[test]
    fn allocation_refusal_at_every_composed_vector_growth_cleans_and_retries() {
        let source = snapshot_source();
        let expected: DataSnapshot = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_data_snapshot(syntax(&source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        let growths = cx.growth_requests();
        assert!(growths >= 10);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_data_snapshot(syntax(&source), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decode_data_snapshot(syntax(&source), &mut retry).unwrap()).unwrap());
        }
    }

    #[test]
    fn shape_version_range_duplicate_and_unknown_errors_remain_distinct() {
        assert!(matches!(decode_data_snapshot(syntax(r#"{"version":1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("project_id"))));
        assert!(matches!(decode_data_snapshot(syntax(r#"[]"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { actual: JsonKind::Sequence, .. })));
        assert!(matches!(decode_data_snapshot(syntax(r#"{"version":4294967296,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        assert!(matches!(decode_data_snapshot(syntax(r#"{"version":-1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));

        let unsupported: DecodedDataSnapshot = decode_data_snapshot(syntax(r#"{"version":2,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]}"#), &mut AllocationContext::new()).unwrap();
        assert_eq!(unsupported.version, 2, "structural decoding preserves version for the later semantic version validator");

        let error = decode_data_snapshot(syntax(r#"{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[],"extra":true}"#), &mut AllocationContext::new()).unwrap_err();
        let ValueDecodeError::UnknownField { key, expected } = error else { panic!("expected unknown field") };
        assert_eq!(key, "extra");
        assert_eq!(expected, DATA_SNAPSHOT_FIELDS);

        let mut cx = AllocationContext::new();
        let error = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate snapshot member was accepted"),
        };
        assert_eq!(error.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn snapshot_collection_limits_are_checked_before_vector_reservation() {
        let entity = r#"{"id":"e","label":"E","fields":[],"unique":[],"constraints":[]}"#;
        let schema_at_limit = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[{}],"records":[],"events":[]}}"#, vec![entity; MAX_ITEMS].join(","));
        assert!(decode_data_snapshot(syntax(&schema_at_limit), &mut AllocationContext::new()).is_ok());
        let schema_over = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[{}],"records":[],"events":[]}}"#, vec![entity; MAX_ITEMS + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_data_snapshot(syntax(&schema_over), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "entity schema count", limit: MAX_ITEMS })));
        assert_eq!(cx.growth_requests(), 0);

        let over_records = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[],"records":[{}],"events":[]}}"#, vec!["null"; MAX_COLLECTION + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_data_snapshot(syntax(&over_records), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "record count", limit: MAX_COLLECTION })));
        assert_eq!(cx.growth_requests(), 0);
        let records_at_limit = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[],"records":[{}],"events":[]}}"#, vec!["null"; MAX_COLLECTION].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_data_snapshot(syntax(&records_at_limit), &mut cx), Err(ValueDecodeError::InvalidType { actual: JsonKind::Null, .. })));
        assert_eq!(cx.growth_requests(), 1, "record vector at the limit is reserved before invalid element decoding");

        let over_events = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[{}]}}"#, vec!["null"; MAX_EVENTS + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_data_snapshot(syntax(&over_events), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "business event count", limit: MAX_EVENTS })));
        assert_eq!(cx.growth_requests(), 0);
        let events_at_limit = format!(r#"{{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[{}]}}"#, vec!["null"; MAX_EVENTS].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_data_snapshot(syntax(&events_at_limit), &mut cx), Err(ValueDecodeError::InvalidType { actual: JsonKind::Null, .. })));
        assert_eq!(cx.growth_requests(), 1, "event vector at the limit is reserved before invalid element decoding");
    }
}
