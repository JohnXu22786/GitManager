//! Fallible structural decoding for persisted Observation graphs.
//!
//! All map/set owners use sorted fallible containers. Existing semantic
//! observation validation remains in its production caller.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::{OrderedMap, OrderedSet};
use crate::product_json::{Number, Value};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::fmt;

use super::fallible_local_artifact_decode::{decode_local_artifact, DecodedLocalArtifact};
use super::fallible_persisted_decode::decode_record_ref;
use super::fallible_value_decode::{decode_type, decode_values, JsonKind, ValueDecodeError};
use super::{Digest, FallibleValues, RecordRef, Type, MAX_COLLECTION, MAX_ITEMS};

const OBSERVATION_FIELDS: &[&str] = &["point", "data_digest", "session_digest", "values", "value_types", "view", "view_schema", "outputs"];
const VIEW_OBSERVATION_FIELDS: &[&str] = &["view", "rows", "controls", "selected", "enabled_actions", "form_values"];
const PRESENTED_ROW_FIELDS: &[&str] = &["record", "cells", "enabled_actions"];
const VIEW_SCHEMA_FIELDS: &[&str] = &["entity", "columns", "row_actions"];

#[derive(Debug, Serialize)]
pub(super) struct DecodedObservation {
    point: String,
    data_digest: Digest,
    session_digest: Digest,
    values: FallibleValues,
    value_types: FallibleTypeMap,
    view: DecodedViewObservation,
    #[serde(skip_serializing_if = "Option::is_none")]
    view_schema: Option<DecodedViewSchema>,
    outputs: Vec<DecodedLocalArtifact>,
}

#[derive(Debug, Serialize)]
struct DecodedViewObservation {
    view: String,
    rows: Vec<DecodedPresentedRow>,
    controls: FallibleValues,
    selected: Vec<RecordRef>,
    enabled_actions: FallibleStringSet,
    form_values: FallibleValues,
}

#[derive(Debug, Serialize)]
struct DecodedPresentedRow {
    record: RecordRef,
    cells: FallibleValues,
    enabled_actions: FallibleStringSet,
}

#[derive(Debug, Serialize)]
struct DecodedViewSchema {
    entity: String,
    columns: FallibleTypeMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    row_actions: Option<FallibleStringMap>,
}

struct FallibleTypeMap(OrderedMap<String, Type>);

impl fmt::Debug for FallibleTypeMap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.0.iter()).finish()
    }
}

impl Serialize for FallibleTypeMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, value)?; }
        map.end()
    }
}

struct FallibleStringMap(OrderedMap<String, String>);

impl fmt::Debug for FallibleStringMap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.0.iter()).finish()
    }
}

impl Serialize for FallibleStringMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, value)?; }
        map.end()
    }
}

struct FallibleStringSet(OrderedSet<String>);

impl fmt::Debug for FallibleStringSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.0.iter()).finish()
    }
}

impl Serialize for FallibleStringSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.iter() { sequence.serialize_element(value)?; }
        sequence.end()
    }
}

pub(super) fn decode_observation(value: Value, cx: &mut AllocationContext) -> Result<DecodedObservation, ValueDecodeError> {
    let fields = struct_fields(value, "a struct Observation", OBSERVATION_FIELDS, cx, false)?;
    let mut point = None;
    let mut data_digest = None;
    let mut session_digest = None;
    let mut values = None;
    let mut value_types = None;
    let mut view = None;
    let mut view_schema = None;
    let mut outputs = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "point" => point = Some(string(value, "a string")?),
            "data_digest" => data_digest = Some(digest(value)?),
            "session_digest" => session_digest = Some(digest(value)?),
            "values" => values = Some(decode_values(value, cx)?),
            "value_types" => value_types = Some(decode_type_map(value, cx, "observation value type count")?),
            "view" => view = Some(decode_view_observation(value, cx)?),
            "view_schema" => view_schema = Some(optional(value, |value| decode_view_schema(value, cx))?),
            "outputs" => outputs = Some(decode_outputs(value, cx)?),
            _ => return Err(unknown_field(key, OBSERVATION_FIELDS)),
        }
    }
    Ok(DecodedObservation {
        point: required(point, "point")?,
        data_digest: required(data_digest, "data_digest")?,
        session_digest: required(session_digest, "session_digest")?,
        values: required(values, "values")?,
        value_types: required(value_types, "value_types")?,
        view: required(view, "view")?,
        view_schema: view_schema.unwrap_or(None),
        outputs: required(outputs, "outputs")?,
    })
}

fn decode_view_observation(value: Value, cx: &mut AllocationContext) -> Result<DecodedViewObservation, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ViewObservation", VIEW_OBSERVATION_FIELDS, cx, false)?;
    let mut view = None;
    let mut rows = None;
    let mut controls = None;
    let mut selected = None;
    let mut enabled_actions = None;
    let mut form_values = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "view" => view = Some(string(value, "a string")?),
            "rows" => rows = Some(decode_rows(value, cx)?),
            "controls" => controls = Some(decode_values(value, cx)?),
            "selected" => selected = Some(decode_record_refs(value, cx, "selected record count")?),
            "enabled_actions" => enabled_actions = Some(decode_string_set(value, cx, "view action count")?),
            "form_values" => form_values = Some(decode_values(value, cx)?),
            _ => return Err(unknown_field(key, VIEW_OBSERVATION_FIELDS)),
        }
    }
    Ok(DecodedViewObservation {
        view: required(view, "view")?,
        rows: required(rows, "rows")?,
        controls: required(controls, "controls")?,
        selected: required(selected, "selected")?,
        enabled_actions: required(enabled_actions, "enabled_actions")?,
        form_values: required(form_values, "form_values")?,
    })
}

fn decode_rows(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedPresentedRow>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), MAX_COLLECTION, "presented row count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(decode_presented_row(value, cx)?); }
    Ok(decoded)
}

fn decode_presented_row(value: Value, cx: &mut AllocationContext) -> Result<DecodedPresentedRow, ValueDecodeError> {
    let fields = struct_fields(value, "a struct PresentedRow", PRESENTED_ROW_FIELDS, cx, false)?;
    let mut record = None;
    let mut cells = None;
    let mut enabled_actions = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "record" => record = Some(decode_record_ref(value, cx)?),
            "cells" => cells = Some(decode_values(value, cx)?),
            "enabled_actions" => enabled_actions = Some(decode_string_set(value, cx, "row action count")?),
            _ => return Err(unknown_field(key, PRESENTED_ROW_FIELDS)),
        }
    }
    Ok(DecodedPresentedRow {
        record: required(record, "record")?,
        cells: required(cells, "cells")?,
        enabled_actions: required(enabled_actions, "enabled_actions")?,
    })
}

fn decode_view_schema(value: Value, cx: &mut AllocationContext) -> Result<DecodedViewSchema, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ViewSchema", VIEW_SCHEMA_FIELDS, cx, true)?;
    let mut entity = None;
    let mut columns = None;
    let mut row_actions = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "entity" => entity = Some(string(value, "a string")?),
            "columns" => columns = Some(decode_type_map(value, cx, "view schema column count")?),
            "row_actions" => row_actions = Some(optional(value, |value| decode_string_map(value, cx, "view row action count"))?),
            _ => return Err(unknown_field(key, VIEW_SCHEMA_FIELDS)),
        }
    }
    Ok(DecodedViewSchema {
        entity: required(entity, "entity")?,
        columns: required(columns, "columns")?,
        row_actions: row_actions.unwrap_or(None),
    })
}

fn decode_outputs(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedLocalArtifact>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), MAX_ITEMS, "observation output count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(decode_local_artifact(value, cx)?); }
    Ok(decoded)
}

fn decode_record_refs(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<RecordRef>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), MAX_COLLECTION, resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(decode_record_ref(value, cx)?); }
    Ok(decoded)
}

fn decode_string_set(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<FallibleStringSet, ValueDecodeError> {
    let values = array(value)?;
    let mut decoded = OrderedSet::new();
    for value in values {
        let value = string(value, "a string")?;
        if !decoded.contains(value.as_str()) && decoded.len() == MAX_ITEMS {
            return Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS });
        }
        decoded.try_insert(value, cx)?;
    }
    Ok(FallibleStringSet(decoded))
}

fn decode_type_map(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<FallibleTypeMap, ValueDecodeError> {
    let values = object(value, "a map")?;
    bounded(values.len(), MAX_ITEMS, resource)?;
    let mut decoded = OrderedMap::new();
    for (key, value) in values.into_entries() { decoded.try_insert(key, decode_type(value, cx)?, cx)?; }
    Ok(FallibleTypeMap(decoded))
}

fn decode_string_map(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<FallibleStringMap, ValueDecodeError> {
    let values = object(value, "a map")?;
    bounded(values.len(), MAX_ITEMS, resource)?;
    let mut decoded = OrderedMap::new();
    for (key, value) in values.into_entries() { decoded.try_insert(key, string(value, "a string")?, cx)?; }
    Ok(FallibleStringMap(decoded))
}

fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext, trailing_default: bool) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Array(values) => {
            let minimum = names.len().saturating_sub(usize::from(trailing_default));
            if values.len() < minimum || values.len() > names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let mut fields = OrderedMap::new();
            fields.try_reserve(names.len(), cx)?;
            for (name, value) in names.iter().zip(values) {
                let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(name, value, cx)?;
            }
            Ok(fields)
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn object(value: Value, expected: &'static str) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value { Value::Object(fields) => Ok(fields), value => Err(invalid_type(value, expected)) }
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value { Value::Array(values) => Ok(values), value => Err(invalid_type(value, "a sequence")) }
}

fn optional<T>(value: Value, decode: impl FnOnce(Value) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value).map(Some) }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value, "a lowercase 64-character digest")?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

fn bounded(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
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
    use crate::product_contract::{FieldDefinition, LocalArtifact, Observation, OutputFormat, PresentedRow, ViewObservation, ViewSchema};
    use std::collections::{BTreeMap, BTreeSet};
    use std::num::NonZeroUsize;

    fn syntax(bytes: &[u8]) -> Value { product_json::parse_json_bytes(bytes, &mut AllocationContext::new()).unwrap() }
    fn digest(ch: char) -> Digest { Digest::try_from(ch.to_string().repeat(64)).unwrap() }

    fn fixture() -> Observation {
        let column = FieldDefinition { id: "name".into(), label: "Name".into(), value_type: Type::Text };
        let output = LocalArtifact::from_rows("roster", OutputFormat::Csv, vec![column], vec![BTreeMap::from([("name".into(), super::super::DataValue::Text { value: "Zoë".into() })])]).unwrap();
        Observation {
            point: "result".into(), data_digest: digest('a'), session_digest: digest('b'),
            values: BTreeMap::from([("count".into(), super::super::DataValue::Integer { value: 1 })]),
            value_types: BTreeMap::from([("count".into(), Type::Integer)]),
            view: ViewObservation {
                view: "people".into(),
                rows: vec![PresentedRow {
                    record: RecordRef { entity: "person".into(), record: "p1".into() },
                    cells: BTreeMap::from([("name".into(), super::super::DataValue::Text { value: "Zoë".into() })]),
                    enabled_actions: BTreeSet::from(["open".into(), "edit".into()]),
                }],
                controls: BTreeMap::from([("active".into(), super::super::DataValue::Boolean { value: true })]),
                selected: vec![RecordRef { entity: "person".into(), record: "p1".into() }],
                enabled_actions: BTreeSet::from(["open".into()]),
                form_values: BTreeMap::from([("query".into(), super::super::DataValue::Text { value: "A".into() })]),
            },
            view_schema: Some(ViewSchema {
                entity: "person".into(),
                columns: BTreeMap::from([("name".into(), Type::Text), ("id".into(), Type::Integer)]),
                row_actions: Some(BTreeMap::from([("open".into(), "inspect".into()), ("edit".into(), "update".into())])),
            }),
            outputs: vec![output],
        }
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
    }

    #[test]
    fn nested_observation_round_trips_canonical_bytes_and_sorted_owners() {
        let expected = fixture();
        let bytes = serde_json::to_vec(&expected).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert_eq!(expected.view.enabled_actions.iter().cloned().collect::<Vec<_>>(), ["open"]);
        assert_eq!(expected.view.rows[0].enabled_actions.iter().cloned().collect::<Vec<_>>(), ["edit", "open"]);
    }

    #[test]
    fn struct_sequence_forms_preserve_declared_serde_order() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        let mut view = value["view"].as_object().unwrap().clone();
        let mut rows = view["rows"].as_array().unwrap().clone();
        let row = rows[0].as_object().unwrap().clone();
        let record = row["record"].as_object().unwrap();
        let record_sequence = serde_json::Value::Array(vec![record["entity"].clone(), record["record"].clone()]);
        rows[0] = serde_json::Value::Array(vec![record_sequence, row["cells"].clone(), row["enabled_actions"].clone()]);
        let selected = view["selected"].as_array().unwrap();
        view["selected"] = serde_json::Value::Array(selected.iter().map(|reference| {
            let reference = reference.as_object().unwrap();
            serde_json::Value::Array(vec![reference["entity"].clone(), reference["record"].clone()])
        }).collect());
        view["rows"] = serde_json::Value::Array(rows);
        let view = serde_json::Value::Array(["view", "rows", "controls", "selected", "enabled_actions", "form_values"].iter().map(|key| view[*key].clone()).collect());
        let schema = value["view_schema"].as_object().unwrap().clone();
        value["view"] = view;
        value["view_schema"] = serde_json::Value::Array(["entity", "columns", "row_actions"].iter().map(|key| schema[*key].clone()).collect());
        let object = value.as_object().unwrap();
        let sequence = serde_json::Value::Array(OBSERVATION_FIELDS.iter().map(|key| object[*key].clone()).collect());
        let oracle: Observation = serde_json::from_value(sequence.clone()).unwrap();
        let bytes = serde_json::to_vec(&sequence).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        let growths = cx.growth_requests();
        for nth in 1..=growths {
            let mut fail = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_observation(syntax(&bytes), &mut fail).unwrap_err(), &fail);
            let mut retry = AllocationContext::new();
            assert!(decode_observation(syntax(&bytes), &mut retry).is_ok());
        }
    }

    #[test]
    fn view_schema_sequence_may_omit_its_trailing_default_field() {
        let sequence = serde_json::json!(["person", {"name":{"kind":"text"}}]);
        let oracle: ViewSchema = serde_json::from_value(sequence.clone()).unwrap();
        assert_eq!(None, oracle.row_actions);
        let bytes = serde_json::to_vec(&sequence).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_view_schema(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn observation_and_view_schema_optional_fields_default_and_skip_when_absent() {
        let expected = fixture();
        let mut value = serde_json::to_value(expected).unwrap();
        value.as_object_mut().unwrap().remove("view_schema");
        value["view"] = serde_json::to_value(&fixture().view).unwrap();
        value["view_schema"] = serde_json::json!({"entity":"person","columns":{"name":{"kind":"text"}}});
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: Observation = serde_json::from_value(value).unwrap();
        assert!(oracle.view_schema.is_some());
        assert!(oracle.view_schema.as_ref().unwrap().row_actions.is_none());
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());

        let mut value = serde_json::to_value(fixture()).unwrap();
        value.as_object_mut().unwrap().remove("view_schema");
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: Observation = serde_json::from_value(value).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(None, oracle.view_schema);
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());

        let mut value = serde_json::to_value(fixture()).unwrap();
        value["view_schema"]["row_actions"] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: Observation = serde_json::from_value(value).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert!(oracle.view_schema.as_ref().unwrap().row_actions.is_none());
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());

        let mut value = serde_json::to_value(fixture()).unwrap();
        value["view_schema"] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: Observation = serde_json::from_value(value).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(None, oracle.view_schema);
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn set_duplicates_collapse_and_map_duplicates_are_rejected() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        value["view"]["enabled_actions"] = serde_json::json!(["z", "a", "z", "m"]);
        value["view"]["rows"][0]["enabled_actions"] = serde_json::json!(["open", "edit", "open"]);
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: Observation = serde_json::from_value(value).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_observation(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert_eq!(decoded.view.enabled_actions.0.iter().cloned().collect::<Vec<_>>(), ["a", "m", "z"]);
        let duplicate = match product_json::parse_json_bytes(br#"{"value_types":{"x":{"kind":"integer"},"x":{"kind":"text"}}}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate map key was accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn malformed_unknown_range_and_collection_depth_limits_are_preserved() {
        let valid = serde_json::to_value(fixture()).unwrap();
        let mut missing = valid.clone(); missing.as_object_mut().unwrap().remove("view");
        assert!(matches!(decode_observation(syntax(&serde_json::to_vec(&missing).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("view"))));
        let mut unknown = valid.clone(); unknown.as_object_mut().unwrap().insert("future".into(), serde_json::Value::Null);
        assert!(matches!(decode_observation(syntax(&serde_json::to_vec(&unknown).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let mut nested_unknown = valid.clone(); nested_unknown["view"]["rows"][0]["future"] = serde_json::Value::Null;
        assert!(matches!(decode_observation(syntax(&serde_json::to_vec(&nested_unknown).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let mut bad_digest = valid.clone(); bad_digest["data_digest"] = serde_json::json!("bad");
        assert!(matches!(decode_observation(syntax(&serde_json::to_vec(&bad_digest).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("invalid lowercase digest"))));
        let mut bad_cell = valid.clone(); bad_cell["view"]["rows"][0]["cells"]["name"] = serde_json::json!({"kind":"integer","value":9223372036854775808u64});
        assert!(matches!(decode_observation(syntax(&serde_json::to_vec(&bad_cell).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "i64", .. })));
        assert!(matches!(decode_rows(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_COLLECTION + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "presented row count", .. })));
        assert!(matches!(decode_record_refs(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_COLLECTION + 1).collect()), &mut AllocationContext::new(), "selected record count"), Err(ValueDecodeError::LimitExceeded { resource: "selected record count", .. })));
        assert!(matches!(decode_outputs(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "observation output count", .. })));
        let repeated = || Value::Array(std::iter::repeat_with(|| Value::String("x".into())).take(MAX_ITEMS + 1).collect());
        assert_eq!(decode_string_set(repeated(), &mut AllocationContext::new(), "action count").unwrap().0.len(), 1);
        let mut unique = (0..=MAX_ITEMS).map(|i| Value::String(format!("action-{i}"))).collect();
        assert!(matches!(decode_string_set(Value::Array(std::mem::take(&mut unique)), &mut AllocationContext::new(), "action count"), Err(ValueDecodeError::LimitExceeded { resource: "action count", .. })));
        let oversized_object = || {
            let mut map = OrderedMap::new();
            let mut cx = AllocationContext::new();
            for i in 0..=MAX_ITEMS { map.try_insert(format!("k{i}"), Value::Null, &mut cx).unwrap(); }
            Value::Object(map)
        };
        assert!(matches!(decode_type_map(oversized_object(), &mut AllocationContext::new(), "view schema column count"), Err(ValueDecodeError::LimitExceeded { resource: "view schema column count", .. })));
        assert!(matches!(decode_string_map(oversized_object(), &mut AllocationContext::new(), "view row action count"), Err(ValueDecodeError::LimitExceeded { resource: "view row action count", .. })));
        assert!(matches!(decode_values(oversized_object(), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "value count", .. })));
        let mut deep = serde_json::json!({"kind":"text"});
        for _ in 0..=super::super::MAX_TYPE_DEPTH { deep = serde_json::json!({"kind":"optional","item":deep}); }
        assert!(matches!(decode_type(deep_value(deep), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "type nesting depth", .. })));
    }

    fn deep_value(value: serde_json::Value) -> Value {
        product_json::parse_json_bytes(&serde_json::to_vec(&value).unwrap(), &mut AllocationContext::new()).unwrap()
    }

    #[test]
    fn every_reached_growth_refusal_cleans_and_retries() {
        let expected = fixture();
        let bytes = serde_json::to_vec(&expected).unwrap();
        let mut baseline = AllocationContext::new();
        decode_observation(syntax(&bytes), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_observation(syntax(&bytes), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decode_observation(syntax(&bytes), &mut retry).unwrap()).unwrap());
        }
    }
}
