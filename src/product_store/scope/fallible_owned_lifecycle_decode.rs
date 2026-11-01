//! Fallible structural decoding for the persisted owned acceptance lifecycle.
//!
//! This stays disconnected from production persistence. Nested scenario and
//! run evidence graphs are composed from their Product Contract DTO decoders.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::OrderedMap;
use crate::product_contract::{
    decode_record_ref_fallible, decode_run_evidence_fallible,
    decode_scenario_spec_fallible, Digest, FallibleDecodedRunEvidence,
    FallibleDecodedScenarioSpec, MAX_ITEMS,
};
use crate::product_contract::fallible_value_decode::{JsonKind, ValueDecodeError};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::owned_contract::OwnedSourceSide;

const WITNESS_FIELDS: &[&str] = &["side", "scenario", "completed", "unfinished"];
const LIFECYCLE_FIELDS: &[&str] = &["version", "witnesses", "evidence", "disclosure", "event", "origin"];
const SCENARIO_FIELDS: &[&str] = &["version", "id", "label", "seed", "session", "clock_day", "random_seed", "inputs", "validity"];
const ORIGINS: &[&str] = &["explicit_host_ui", "synthetic_operator_fixture"];
const SOURCE_SIDES: &[&str] = &["before", "after"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedLifecycleV2 {
    version: u32,
    witnesses: Vec<DecodedLifecycleWitness>,
    evidence: Vec<FallibleDecodedRunEvidence>,
    disclosure: Digest,
    event: Digest,
    origin: DecodedOwnedAcceptanceOriginV2,
}

#[derive(Debug, Serialize)]
struct DecodedLifecycleWitness {
    side: OwnedSourceSide,
    scenario: FallibleDecodedScenarioSpec,
    completed: crate::product_contract::RecordRef,
    unfinished: crate::product_contract::RecordRef,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DecodedOwnedAcceptanceOriginV2 {
    ExplicitHostUi,
    SyntheticOperatorFixture,
}

pub(crate) fn decode_owned_lifecycle_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedLifecycleV2, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedLifecycleV2", LIFECYCLE_FIELDS, cx)?;
    let (mut version, mut witnesses, mut evidence, mut disclosure, mut event, mut decoded_origin) =
        (None, None, None, None, None, None);
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "witnesses" => witnesses = Some(decode_vec(value, "lifecycle witness count", cx, decode_witness)?),
            "evidence" => evidence = Some(decode_vec(value, "lifecycle run evidence count", cx, |v, cx| decode_run_evidence_fallible(v, cx))?),
            "disclosure" => disclosure = Some(digest(value)?),
            "event" => event = Some(digest(value)?),
            "origin" => decoded_origin = Some(origin(value)?),
            _ => return Err(unknown(key, LIFECYCLE_FIELDS)),
        }
    }
    Ok(DecodedOwnedLifecycleV2 {
        version: required(version, "version")?,
        witnesses: required(witnesses, "witnesses")?,
        evidence: required(evidence, "evidence")?,
        disclosure: required(disclosure, "disclosure")?,
        event: required(event, "event")?,
        origin: required(decoded_origin, "origin")?,
    })
}

fn decode_witness(value: Value, cx: &mut AllocationContext) -> Result<DecodedLifecycleWitness, ValueDecodeError> {
    let fields = struct_fields(value, "a struct LifecycleWitness", WITNESS_FIELDS, cx)?;
    let (mut side, mut scenario, mut completed, mut unfinished) = (None, None, None, None);
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "side" => side = Some(source_side(value)?),
            "scenario" => scenario = Some(decode_scenario(value, cx)?),
            "completed" => completed = Some(decode_record_ref_fallible(value, cx)?),
            "unfinished" => unfinished = Some(decode_record_ref_fallible(value, cx)?),
            _ => return Err(unknown(key, WITNESS_FIELDS)),
        }
    }
    Ok(DecodedLifecycleWitness {
        side: required(side, "side")?,
        scenario: required(scenario, "scenario")?,
        completed: required(completed, "completed")?,
        unfinished: required(unfinished, "unfinished")?,
    })
}

fn decode_vec<T>(
    value: Value,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    if values.len() > MAX_ITEMS {
        return Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS });
    }
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode(value, cx)?);
    }
    Ok(decoded)
}

fn decode_scenario(value: Value, cx: &mut AllocationContext) -> Result<FallibleDecodedScenarioSpec, ValueDecodeError> {
    match value {
        Value::Array(values) => {
            if values.len() != SCENARIO_FIELDS.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: SCENARIO_FIELDS.len() });
            }
            let mut fields = OrderedMap::new();
            fields.try_reserve(SCENARIO_FIELDS.len(), cx)?;
            for (name, value) in SCENARIO_FIELDS.iter().zip(values) {
                let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(name, value, cx)?;
            }
            decode_scenario_spec_fallible(Value::Object(fields), cx)
        }
        value => decode_scenario_spec_fallible(value, cx),
    }
}

fn source_side(value: Value) -> Result<OwnedSourceSide, ValueDecodeError> {
    match unit_variant(value, SOURCE_SIDES, "unit enum OwnedSourceSide")?.as_str() {
        "before" => Ok(OwnedSourceSide::Before),
        "after" => Ok(OwnedSourceSide::After),
        _ => unreachable!("variant was checked"),
    }
}

fn origin(value: Value) -> Result<DecodedOwnedAcceptanceOriginV2, ValueDecodeError> {
    match unit_variant(value, ORIGINS, "unit enum OwnedAcceptanceOriginV2")?.as_str() {
        "explicit_host_ui" => Ok(DecodedOwnedAcceptanceOriginV2::ExplicitHostUi),
        "synthetic_operator_fixture" => Ok(DecodedOwnedAcceptanceOriginV2::SyntheticOperatorFixture),
        _ => unreachable!("variant was checked"),
    }
}

fn unit_variant(value: Value, variants: &'static [&'static str], expected: &'static str) -> Result<String, ValueDecodeError> {
    let tag = match value {
        Value::String(tag) => tag,
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, payload) = fields.into_entries().next().expect("one field was checked");
            if !variants.contains(&tag.as_str()) {
                return Err(ValueDecodeError::UnknownVariant { tag, expected: variants });
            }
            match payload {
                Value::Null => tag,
                payload => return Err(invalid_type(payload, "unit")),
            }
        }
        value => return Err(invalid_type(value, expected)),
    };
    if variants.contains(&tag.as_str()) {
        Ok(tag)
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: variants })
    }
}

fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Array(values) => {
            if values.len() != names.len() {
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

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = match value {
        Value::String(value) => value,
        value => return Err(invalid_type(value, "a lowercase 64-character digest")),
    };
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

fn u32_value(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: crate::product_contract::fallible_value_decode::NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: crate::product_contract::fallible_value_decode::NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
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
    use crate::product_contract::{canonical_bytes, MAX_ITEMS};
    use crate::product_store::scope::OwnedLifecycleV2;
    use crate::product_json;
    use serde_json::{json, Value as SerdeValue};
    use std::num::NonZeroUsize;

    fn fixture(origin: &str) -> SerdeValue {
        let d = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        json!({
            "version": 2,
            "witnesses": [{
                "side": "before",
                "scenario": {
                    "version": 1, "id": "scenario", "label": "fixture",
                    "seed": {"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]},
                    "session": {"view":"main","values":{},"focused_record":null},
                    "clock_day": 0, "random_seed": 7, "inputs": [], "validity": []
                },
                "completed": {"entity":"task","record":"done"},
                "unfinished": {"entity":"task","record":"todo"}
            }],
            "evidence": [{
                "version":1,"id":"run","origin":"test_fixture",
                "binding": {
                    "source": {"project_id":"p","program_path":"apps/a.json","task":null,
                        "producer":{"kind":"fixture","name":"fixture"},"source_digest":d},
                    "artifact":{"kind":"generated_app","version":1,"language":"product-contract-v1","raw_digest":d,"program_digest":d,"semantic_digest":d},
                    "data_digest":d,"input_digest":d,"scenario_digest":d,"decision_digest":d,"session_digest":d,
                    "runtime_version":"r","driver_version":"d"
                },
                "source_after": {"project_id":"p","program_path":"apps/a.json","task":null,
                    "producer":{"kind":"fixture","name":"fixture"},"source_digest":d},
                "state":"observed","trace":[],"observations":[],
                "limits":{"fuel":0,"collection_items":0,"action_steps":0,"transaction_writes":0,"output_bytes":0,"elapsed_millis":0},
                "errors":[],"uncovered":[]
            }],
            "disclosure": d, "event": d, "origin": origin
        })
    }
    fn decode(value: &SerdeValue, cx: &mut AllocationContext) -> Result<DecodedOwnedLifecycleV2, ValueDecodeError> {
        let bytes = serde_json::to_vec(value).unwrap();
        decode_owned_lifecycle_v2(product_json::parse_json_bytes(&bytes, &mut AllocationContext::new()).unwrap(), cx)
    }
    fn canonical(value: &impl Serialize) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn lifecycle_graph_matches_canonical_serde_for_each_origin() {
        for origin in ["explicit_host_ui", "synthetic_operator_fixture"] {
            let mut value = fixture(origin);
            let mut after = value["witnesses"][0].clone();
            after["side"] = json!("after");
            value["witnesses"].as_array_mut().unwrap().push(after);
            let expected: OwnedLifecycleV2 = serde_json::from_value(value.clone()).unwrap();
            let decoded = decode(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
            assert_eq!(decoded.witnesses.len(), 2);
        }
    }

    #[test]
    fn enum_forms_and_struct_sequence_preserve_serde_wire_semantics() {
        let mut value = fixture("explicit_host_ui");
        value["origin"] = json!({"explicit_host_ui":null});
        value["witnesses"][0]["side"] = json!({"before":null});
        let expected: OwnedLifecycleV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new()).unwrap()));

        let scenario = value["witnesses"][0]["scenario"].as_object().unwrap();
        let scenario_fields = ["version", "id", "label", "seed", "session", "clock_day", "random_seed", "inputs", "validity"];
        let mut scenario_sequence: Vec<_> = scenario_fields.iter().map(|field| scenario[*field].clone()).collect();
        value["witnesses"][0]["scenario"] = SerdeValue::Array(scenario_sequence);

        let witness = value["witnesses"][0].as_object().unwrap();
        let mut witness_sequence = vec![witness["side"].clone(), witness["scenario"].clone(), witness["completed"].clone(), witness["unfinished"].clone()];
        value["witnesses"][0] = SerdeValue::Array(witness_sequence);
        let root = value.as_object().unwrap();
        let mut root_sequence = vec![root["version"].clone(), root["witnesses"].clone(), root["evidence"].clone(), root["disclosure"].clone(), root["event"].clone(), root["origin"].clone()];
        value = SerdeValue::Array(root_sequence);
        let expected: OwnedLifecycleV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new()).unwrap()));
    }

    #[test]
    fn overlong_struct_sequences_match_serde_rejection() {
        let mut value = fixture("explicit_host_ui");
        let witness = value["witnesses"][0].as_object().unwrap();
        let mut witness_sequence = vec![witness["side"].clone(), witness["scenario"].clone(), witness["completed"].clone(), witness["unfinished"].clone()];
        witness_sequence.push(SerdeValue::Null);
        value["witnesses"][0] = SerdeValue::Array(witness_sequence);
        assert!(serde_json::from_value::<OwnedLifecycleV2>(value.clone()).is_err());
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 5, expected: 4 })));

        let mut value = fixture("explicit_host_ui");
        let scenario = value["witnesses"][0]["scenario"].as_object().unwrap();
        let fields = ["version", "id", "label", "seed", "session", "clock_day", "random_seed", "inputs", "validity"];
        let mut sequence: Vec<_> = fields.iter().map(|field| scenario[*field].clone()).collect();
        sequence.push(SerdeValue::Null);
        value["witnesses"][0]["scenario"] = SerdeValue::Array(sequence);
        assert!(serde_json::from_value::<OwnedLifecycleV2>(value.clone()).is_err());
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 10, expected: 9 })));

        let value = fixture("explicit_host_ui");
        let root = value.as_object().unwrap();
        let mut sequence = vec![root["version"].clone(), root["witnesses"].clone(), root["evidence"].clone(), root["disclosure"].clone(), root["event"].clone(), root["origin"].clone()];
        sequence.push(SerdeValue::Null);
        let value = SerdeValue::Array(sequence);
        assert!(serde_json::from_value::<OwnedLifecycleV2>(value.clone()).is_err());
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 7, expected: 6 })));
    }

    #[test]
    fn malformed_unknown_missing_duplicate_numeric_and_collection_inputs_are_rejected() {
        let mut value = fixture("explicit_host_ui");
        value.as_object_mut().unwrap().remove("event");
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("event"))));
        let mut value = fixture("explicit_host_ui");
        value["future"] = json!(true);
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let mut value = fixture("explicit_host_ui");
        value["origin"] = json!("future_origin");
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let mut value = fixture("explicit_host_ui");
        value["version"] = json!(u64::from(u32::MAX) + 1);
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        let mut value = fixture("explicit_host_ui");
        value["witnesses"][0]["side"] = json!({"before":true});
        assert!(matches!(decode(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "unit", .. })));
        let duplicate = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate field was accepted"),
        };
        assert_eq!(duplicate.cause, product_json::Cause::DuplicateObjectMember);
        let too_many = Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect());
        assert!(matches!(decode_vec::<()>(too_many, "lifecycle witness count", &mut AllocationContext::new(), |_, _| Ok(())), Err(ValueDecodeError::LimitExceeded { resource: "lifecycle witness count", limit: MAX_ITEMS })));
    }

    #[test]
    fn every_lifecycle_growth_refusal_drops_partial_state_and_retries() {
        let value = fixture("synthetic_operator_fixture");
        let mut baseline = AllocationContext::new();
        decode(&value, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode(&value, &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retried = decode(&value, &mut AllocationContext::new()).unwrap();
            let expected: OwnedLifecycleV2 = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(canonical(&expected), canonical(&retried));
        }
        assert_eq!(baseline.first_failure().map(|f| f.kind), None);
        let mut fail_first = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        assert!(matches!(decode(&fixture("explicit_host_ui"), &mut fail_first), Err(ValueDecodeError::Allocation(failure)) if failure.kind == AllocationFailureKind::Injected));
    }
}
