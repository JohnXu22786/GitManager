//! Fallible structural decoding for persisted ScenarioSpec values.
//!
//! The DTO composes the existing snapshot, session, value and accepted-property
//! decoders. Semantic validation remains in the existing production caller.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::OrderedMap;
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_decision_decode::decode_accepted_property;
use super::fallible_persisted_decode::{decode_record_ref, DecodedSessionState};
use super::fallible_snapshot_decode::{decode_data_snapshot, DecodedDataSnapshot};
use super::fallible_value_decode::{decode_data_value, decode_values, JsonKind, NumericValue, ValueDecodeError};
use super::{AcceptedProperty, DataValue, FallibleValues, RecordRef, MAX_ITEMS};

const SCENARIO_FIELDS: &[&str] = &["version", "id", "label", "seed", "session", "clock_day", "random_seed", "inputs", "validity"];
const INPUT_VARIANTS: &[&str] = &["invoke", "control", "activate", "submit", "navigate", "advance_clock", "observe"];
const INVOKE_FIELDS: &[&str] = &["action", "arguments"];
const CONTROL_FIELDS: &[&str] = &["view", "control", "value"];
const ACTIVATE_FIELDS: &[&str] = &["view", "binding", "row"];
const SUBMIT_FIELDS: &[&str] = &["view", "arguments"];
const NAVIGATE_FIELDS: &[&str] = &["view"];
const ADVANCE_CLOCK_FIELDS: &[&str] = &["days"];
const OBSERVE_FIELDS: &[&str] = &["point"];

#[derive(Debug, Serialize)]
pub(super) struct DecodedScenarioSpec {
    version: u32,
    id: String,
    label: String,
    seed: DecodedDataSnapshot,
    session: DecodedSessionState,
    clock_day: i32,
    random_seed: u64,
    inputs: Vec<DecodedSemanticInput>,
    validity: Vec<AcceptedProperty>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum DecodedSemanticInput {
    Invoke { action: String, arguments: FallibleValues },
    Control { view: String, control: String, value: DataValue },
    Activate { view: String, binding: String, row: Option<RecordRef> },
    Submit { view: String, arguments: FallibleValues },
    Navigate { view: String },
    AdvanceClock { days: u32 },
    Observe { point: String },
}

pub(super) fn decode_scenario_spec(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedScenarioSpec, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct ScenarioSpec", SCENARIO_FIELDS)?;
    let mut version = None;
    let mut id = None;
    let mut label = None;
    let mut seed = None;
    let mut session = None;
    let mut clock_day = None;
    let mut random_seed = None;
    let mut inputs = None;
    let mut validity = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(unsigned_u32(value)?),
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "seed" => seed = Some(decode_data_snapshot(value, cx)?),
            "session" => session = Some(super::fallible_persisted_decode::decode_session_state(value, cx)?),
            "clock_day" => clock_day = Some(signed_i32(value)?),
            "random_seed" => random_seed = Some(unsigned_u64(value)?),
            "inputs" => inputs = Some(decode_inputs(value, cx)?),
            "validity" => validity = Some(decode_validity(value, cx)?),
            _ => return Err(ValueDecodeError::UnknownField { key, expected: SCENARIO_FIELDS }),
        }
    }
    Ok(DecodedScenarioSpec {
        version: required(version, "version")?,
        id: required(id, "id")?,
        label: required(label, "label")?,
        seed: required(seed, "seed")?,
        session: required(session, "session")?,
        clock_day: required(clock_day, "clock_day")?,
        random_seed: required(random_seed, "random_seed")?,
        inputs: required(inputs, "inputs")?,
        validity: required(validity, "validity")?,
    })
}

fn decode_inputs(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedSemanticInput>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), "scenario input count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_semantic_input(value, cx)?);
    }
    Ok(decoded)
}

pub(super) fn decode_semantic_input(value: Value, cx: &mut AllocationContext) -> Result<DecodedSemanticInput, ValueDecodeError> {
    let (tag, fields) = tagged(value)?;
    match tag.as_str() {
        "invoke" => {
            let mut action = None;
            let mut arguments = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "action" => action = Some(string(value, "a string")?),
                    "arguments" => arguments = Some(decode_values(value, cx)?),
                    _ => return Err(unknown_field(key, INVOKE_FIELDS)),
                }
            }
            Ok(DecodedSemanticInput::Invoke {
                action: required(action, "action")?,
                arguments: required(arguments, "arguments")?,
            })
        }
        "control" => {
            let mut view = None;
            let mut control = None;
            let mut input_value = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "view" => view = Some(string(value, "a string")?),
                    "control" => control = Some(string(value, "a string")?),
                    "value" => input_value = Some(decode_data_value(value, cx)?),
                    _ => return Err(unknown_field(key, CONTROL_FIELDS)),
                }
            }
            Ok(DecodedSemanticInput::Control {
                view: required(view, "view")?,
                control: required(control, "control")?,
                value: required(input_value, "value")?,
            })
        }
        "activate" => {
            let mut view = None;
            let mut binding = None;
            let mut row = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "view" => view = Some(string(value, "a string")?),
                    "binding" => binding = Some(string(value, "a string")?),
                    "row" => row = Some(optional_record_ref(value, cx)?),
                    _ => return Err(unknown_field(key, ACTIVATE_FIELDS)),
                }
            }
            Ok(DecodedSemanticInput::Activate {
                view: required(view, "view")?,
                binding: required(binding, "binding")?,
                row: row.unwrap_or(None),
            })
        }
        "submit" => {
            let mut view = None;
            let mut arguments = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "view" => view = Some(string(value, "a string")?),
                    "arguments" => arguments = Some(decode_values(value, cx)?),
                    _ => return Err(unknown_field(key, SUBMIT_FIELDS)),
                }
            }
            Ok(DecodedSemanticInput::Submit {
                view: required(view, "view")?,
                arguments: required(arguments, "arguments")?,
            })
        }
        "navigate" => {
            let mut view = None;
            for (key, value) in fields.into_entries() {
                if key == "view" { view = Some(string(value, "a string")?); }
                else { return Err(unknown_field(key, NAVIGATE_FIELDS)); }
            }
            Ok(DecodedSemanticInput::Navigate { view: required(view, "view")? })
        }
        "advance_clock" => {
            let mut days = None;
            for (key, value) in fields.into_entries() {
                if key == "days" { days = Some(unsigned_u32(value)?); }
                else { return Err(unknown_field(key, ADVANCE_CLOCK_FIELDS)); }
            }
            Ok(DecodedSemanticInput::AdvanceClock { days: required(days, "days")? })
        }
        "observe" => {
            let mut point = None;
            for (key, value) in fields.into_entries() {
                if key == "point" { point = Some(string(value, "a string")?); }
                else { return Err(unknown_field(key, OBSERVE_FIELDS)); }
            }
            Ok(DecodedSemanticInput::Observe { point: required(point, "point")? })
        }
        _ => unreachable!("semantic input tag comes from the closed variant table"),
    }
}

fn decode_validity(value: Value, cx: &mut AllocationContext) -> Result<Vec<AcceptedProperty>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), "scenario validity property count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_accepted_property(value, cx)?);
    }
    Ok(decoded)
}

fn tagged(value: Value) -> Result<(String, OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = object(value, "an internally tagged enum SemanticInput")?;
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = string(tag, "a string variant identifier")?;
    if INPUT_VARIANTS.contains(&tag.as_str()) {
        Ok((tag, fields))
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: INPUT_VARIANTS })
    }
}

fn object(value: Value, expected: &'static str) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        value => Err(invalid_type(value, expected)),
    }
}

enum StructFields {
    Object(std::vec::IntoIter<(String, Value)>),
    Sequence {
        names: &'static [&'static str],
        values: std::vec::IntoIter<Value>,
        index: usize,
        actual: usize,
    },
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

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn optional_record_ref(value: Value, cx: &mut AllocationContext) -> Result<Option<RecordRef>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => decode_record_ref(value, cx).map(Some),
    }
}

fn bounded(len: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > MAX_ITEMS { Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS }) } else { Ok(()) }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn unsigned_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn unsigned_u64(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }),
        value => Err(invalid_type(value, "u64")),
    }
}

fn signed_i32(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i32" }),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "i32" }),
        value => Err(invalid_type(value, "i32")),
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
    use crate::product_contract::{
        BusinessEvent, DataSnapshot, PropertyPredicate, PropertyTerm, Record, ScenarioSpec,
        SemanticInput, SessionState, Type, CONTRACT_VERSION,
    };
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    fn syntax(source: &[u8]) -> Value {
        product_json::parse_json_bytes(source, &mut AllocationContext::new()).unwrap()
    }

    fn digest(ch: char) -> super::super::Digest {
        super::super::Digest::try_from(ch.to_string().repeat(64)).unwrap()
    }

    fn fixture() -> ScenarioSpec {
        let seed = DataSnapshot {
            version: CONTRACT_VERSION,
            project_id: "project".into(),
            generation: 4,
            schema: vec![super::super::EntityDefinition {
                id: "person".into(), label: "Person".into(), fields: vec![], unique: vec![], constraints: vec![],
            }],
            records: vec![Record {
                entity: "person".into(), id: "p1".into(), revision: 1,
                created_program: digest('a'), archived: false,
                values: BTreeMap::from([("name".into(), DataValue::Text { value: "Zoë".into() })]),
            }],
            events: vec![BusinessEvent {
                id: "event-1".into(), sequence: 1, operation_id: "op-1".into(), action: "create".into(),
                day: 10, program: digest('b'), changes: vec![], outputs: vec![digest('c')],
            }],
        };
        let session = SessionState {
            view: "people".into(),
            values: BTreeMap::from([("active".into(), DataValue::Boolean { value: true })]),
            focused_record: Some(RecordRef { entity: "person".into(), record: "p1".into() }),
        };
        ScenarioSpec {
            version: 1,
            id: "scenario".into(),
            label: "复合 scenario".into(),
            seed,
            session,
            clock_day: 12,
            random_seed: u64::MAX,
            inputs: vec![
                SemanticInput::Invoke { action: "create".into(), arguments: BTreeMap::from([("name".into(), DataValue::Text { value: "Ada".into() })]) },
                SemanticInput::Control { view: "people".into(), control: "active".into(), value: DataValue::Boolean { value: true } },
                SemanticInput::Activate { view: "people".into(), binding: "open".into(), row: Some(RecordRef { entity: "person".into(), record: "p1".into() }) },
                SemanticInput::Submit { view: "form".into(), arguments: BTreeMap::from([("name".into(), DataValue::Text { value: "Bob".into() })]) },
                SemanticInput::Navigate { view: "people".into() },
                SemanticInput::AdvanceClock { days: u32::MAX },
                SemanticInput::Observe { point: "result".into() },
            ],
            validity: vec![super::super::AcceptedProperty {
                id: "has_people".into(), description: "at least one person".into(),
                predicate: PropertyPredicate::And { values: vec![
                    PropertyPredicate::Equal {
                        left: PropertyTerm::Literal { value_type: Type::Text, value: DataValue::Text { value: "ok".into() } },
                        right: PropertyTerm::Observed { point: "result".into(), observable: "status".into(), value_type: Type::Text },
                    },
                    PropertyPredicate::Not { value: Box::new(PropertyPredicate::Less {
                        left: PropertyTerm::ViewRows { point: "result".into(), entity: "person".into() },
                        right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: 0 } },
                    }) },
                ] },
            }],
        }
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
    }

    #[test]
    fn full_nested_scenario_and_all_input_variants_preserve_canonical_bytes() {
        let expected = fixture();
        let bytes = serde_json::to_vec(&expected).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_scenario_spec(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert_eq!(expected.inputs.len(), INPUT_VARIANTS.len());
        let encoded = serde_json::to_value(&decoded).unwrap();
        for (input, variant) in encoded["inputs"].as_array().unwrap().iter().zip(INPUT_VARIANTS) {
            assert_eq!(input["kind"], *variant);
        }
    }

    #[test]
    fn missing_optional_activate_row_defaults_to_none() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        let inputs = value["inputs"].as_array_mut().unwrap();
        inputs[2].as_object_mut().unwrap().remove("row");
        let bytes = serde_json::to_vec(&value).unwrap();
        let oracle: ScenarioSpec = serde_json::from_value(value).unwrap();
        let row = oracle.inputs.iter().find_map(|input| match input { SemanticInput::Activate { row, .. } => Some(row.clone()), _ => None }).unwrap();
        assert_eq!(None, row);
        let mut cx = AllocationContext::new();
        let decoded = decode_scenario_spec(syntax(&bytes), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&oracle).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn malformed_unknown_duplicate_and_numeric_inputs_are_classified() {
        let valid = serde_json::to_value(fixture()).unwrap();
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("seed");
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&missing).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("seed"))));
        let mut unknown = valid.clone();
        unknown.as_object_mut().unwrap().insert("future".into(), serde_json::Value::Null);
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&unknown).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let mut bad_input = valid.clone();
        bad_input["inputs"][0]["kind"] = serde_json::json!("future");
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&bad_input).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let mut bad_field = valid.clone();
        bad_field["inputs"][0]["future"] = serde_json::Value::Null;
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&bad_field).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let duplicate = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate scenario field accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);
        for (field, number, expected) in [
            ("version", serde_json::json!(4294967296u64), "u32"),
            ("clock_day", serde_json::json!(2147483648i64), "i32"),
            ("random_seed", serde_json::json!(-1), "u64"),
        ] {
            let mut value = valid.clone(); value[field] = number;
            assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&value).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: got, .. }) if got == expected));
        }
        let mut bad_days = valid.clone(); bad_days["inputs"][5]["days"] = serde_json::json!(-1);
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&bad_days).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
    }

    #[test]
    fn outer_collections_and_property_depth_are_bounded_before_growth() {
        let at_limit_inputs = serde_json::Value::Array(std::iter::repeat_with(|| serde_json::json!({"kind":"observe","point":"p"})).take(MAX_ITEMS).collect());
        assert!(decode_inputs(syntax(&serde_json::to_vec(&at_limit_inputs).unwrap()), &mut AllocationContext::new()).is_ok());
        assert!(matches!(decode_inputs(Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "scenario input count", limit: MAX_ITEMS })));
        let property = serde_json::json!({"id":"p","description":"P","predicate":{"kind":"equal","left":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}});
        let at_limit_validity = serde_json::Value::Array(std::iter::repeat_with(|| property.clone()).take(MAX_ITEMS).collect());
        assert!(decode_validity(syntax(&serde_json::to_vec(&at_limit_validity).unwrap()), &mut AllocationContext::new()).is_ok());
        let mut over_validity = serde_json::to_value(fixture()).unwrap();
        over_validity["validity"] = serde_json::Value::Array(std::iter::repeat_with(|| serde_json::Value::Null).take(MAX_ITEMS + 1).collect());
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&over_validity).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "scenario validity property count", limit: MAX_ITEMS })));
        let mut property = serde_json::to_value(fixture()).unwrap();
        let mut predicate = serde_json::json!({"kind":"equal","left":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}});
        for _ in 0..=super::super::MAX_EXPR_DEPTH {
            predicate = serde_json::json!({"kind":"not","value":predicate});
        }
        property["validity"][0]["predicate"] = predicate;
        assert!(matches!(decode_scenario_spec(syntax(&serde_json::to_vec(&property).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "property expression depth", .. })));
    }

    #[test]
    fn allocation_refusal_at_every_reached_growth_cleans_and_retries() {
        let expected = fixture();
        let bytes = serde_json::to_vec(&expected).unwrap();
        let mut baseline = AllocationContext::new();
        decode_scenario_spec(syntax(&bytes), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_scenario_spec(syntax(&bytes), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decode_scenario_spec(syntax(&bytes), &mut retry).unwrap()).unwrap());
        }
    }
}
