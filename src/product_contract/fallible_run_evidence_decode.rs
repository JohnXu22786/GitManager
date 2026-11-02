//! Fallible structural decoding for persisted RunEvidence graphs.
//!
//! Existing validation remains in its production caller. The DTO composes the
//! fallible persisted leaves without materializing infallible collection owners.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::OrderedMap;
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_capture_decode::{decode_artifact, decode_source_binding};
use super::fallible_observation_decode::{decode_observation, DecodedObservation};
use super::fallible_scenario_decode::{decode_semantic_input, DecodedSemanticInput};
use super::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use super::{Digest, EvidenceState, ExecutionOrigin, RunBinding, RuntimeLimits, SourceBinding, StepOutcome, MAX_ITEMS, MAX_TYPE_DEPTH};

const RUN_EVIDENCE_FIELDS: &[&str] = &["version", "id", "origin", "binding", "source_after", "state", "trace", "observations", "limits", "errors", "uncovered"];
const RUN_BINDING_FIELDS: &[&str] = &["source", "artifact", "data_digest", "input_digest", "scenario_digest", "decision_digest", "session_digest", "runtime_version", "driver_version"];
const TRACE_STEP_FIELDS: &[&str] = &["index", "input", "before_data", "after_data", "before_session", "after_session", "outputs", "outcome", "diagnostic"];
const RUNTIME_LIMITS_FIELDS: &[&str] = &["fuel", "collection_items", "action_steps", "transaction_writes", "output_bytes", "elapsed_millis"];
const EVIDENCE_STATES: &[&str] = &["observed", "no_difference_found", "inconclusive", "unsupported", "stale", "failed"];
const EXECUTION_ORIGINS: &[&str] = &["production_runtime", "legacy_runtime", "test_fixture"];
const STEP_OUTCOMES: &[&str] = &["applied", "rejected", "failed", "budget_exhausted"];

#[derive(Debug, Serialize)]
pub(super) struct DecodedRunEvidence {
    version: u32,
    id: String,
    origin: ExecutionOrigin,
    binding: RunBinding,
    source_after: SourceBinding,
    state: EvidenceState,
    trace: Vec<DecodedTraceStep>,
    observations: Vec<DecodedObservation>,
    limits: RuntimeLimits,
    errors: Vec<String>,
    uncovered: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DecodedTraceStep {
    index: usize,
    input: DecodedSemanticInput,
    before_data: Digest,
    after_data: Digest,
    before_session: Digest,
    after_session: Digest,
    outputs: Vec<Digest>,
    outcome: StepOutcome,
    diagnostic: Option<String>,
}

pub(super) fn decode_run_evidence(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedRunEvidence, ValueDecodeError> {
    let fields = struct_fields(value, "a struct RunEvidence", RUN_EVIDENCE_FIELDS, cx)?;
    let mut version = None;
    let mut id = None;
    let mut origin = None;
    let mut binding = None;
    let mut source_after = None;
    let mut state = None;
    let mut trace = None;
    let mut observations = None;
    let mut limits = None;
    let mut errors = None;
    let mut uncovered = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "version" => version = Some(unsigned_u32(value)?),
            "id" => id = Some(string(value, "a string")?),
            "origin" => origin = Some(execution_origin(value)?),
            "binding" => binding = Some(decode_run_binding(value, cx)?),
            "source_after" => source_after = Some(decode_source_binding(value, cx)?),
            "state" => state = Some(evidence_state(value)?),
            "trace" => trace = Some(decode_trace(value, cx)?),
            "observations" => observations = Some(decode_observations(value, cx)?),
            "limits" => limits = Some(decode_runtime_limits(value, cx)?),
            "errors" => errors = Some(decode_strings(value, cx, "run error count")?),
            "uncovered" => uncovered = Some(decode_strings(value, cx, "uncovered obligation count")?),
            _ => return Err(unknown_field(key, RUN_EVIDENCE_FIELDS)),
        }
    }
    Ok(DecodedRunEvidence {
        version: required(version, "version")?,
        id: required(id, "id")?,
        origin: required(origin, "origin")?,
        binding: required(binding, "binding")?,
        source_after: required(source_after, "source_after")?,
        state: required(state, "state")?,
        trace: required(trace, "trace")?,
        observations: required(observations, "observations")?,
        limits: required(limits, "limits")?,
        errors: required(errors, "errors")?,
        uncovered: required(uncovered, "uncovered")?,
    })
}

pub(super) fn decode_run_binding(value: Value, cx: &mut AllocationContext) -> Result<RunBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct RunBinding", RUN_BINDING_FIELDS, cx)?;
    let mut source = None;
    let mut artifact = None;
    let mut data_digest = None;
    let mut input_digest = None;
    let mut scenario_digest = None;
    let mut decision_digest = None;
    let mut session_digest = None;
    let mut runtime_version = None;
    let mut driver_version = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "source" => source = Some(decode_source_binding(value, cx)?),
            "artifact" => artifact = Some(decode_artifact(value, cx)?),
            "data_digest" => data_digest = Some(digest(value)?),
            "input_digest" => input_digest = Some(digest(value)?),
            "scenario_digest" => scenario_digest = Some(digest(value)?),
            "decision_digest" => decision_digest = Some(digest(value)?),
            "session_digest" => session_digest = Some(digest(value)?),
            "runtime_version" => runtime_version = Some(string(value, "a string")?),
            "driver_version" => driver_version = Some(string(value, "a string")?),
            _ => return Err(unknown_field(key, RUN_BINDING_FIELDS)),
        }
    }
    Ok(RunBinding {
        source: required(source, "source")?,
        artifact: required(artifact, "artifact")?,
        data_digest: required(data_digest, "data_digest")?,
        input_digest: required(input_digest, "input_digest")?,
        scenario_digest: required(scenario_digest, "scenario_digest")?,
        decision_digest: required(decision_digest, "decision_digest")?,
        session_digest: required(session_digest, "session_digest")?,
        runtime_version: required(runtime_version, "runtime_version")?,
        driver_version: required(driver_version, "driver_version")?,
    })
}

fn decode_trace(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedTraceStep>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), "run trace count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_trace_step(value, cx)?);
    }
    Ok(decoded)
}

fn decode_trace_step(value: Value, cx: &mut AllocationContext) -> Result<DecodedTraceStep, ValueDecodeError> {
    let fields = struct_fields(value, "a struct TraceStep", TRACE_STEP_FIELDS, cx)?;
    let mut index = None;
    let mut input = None;
    let mut before_data = None;
    let mut after_data = None;
    let mut before_session = None;
    let mut after_session = None;
    let mut outputs = None;
    let mut outcome = None;
    let mut diagnostic: Option<Option<String>> = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "index" => index = Some(integer_usize(value)?),
            "input" => input = Some(decode_semantic_input(value, cx)?),
            "before_data" => before_data = Some(digest(value)?),
            "after_data" => after_data = Some(digest(value)?),
            "before_session" => before_session = Some(digest(value)?),
            "after_session" => after_session = Some(digest(value)?),
            "outputs" => outputs = Some(decode_digests(value, cx, "trace output count")?),
            "outcome" => outcome = Some(step_outcome(value)?),
            "diagnostic" => diagnostic = Some(optional(value, |value| string(value, "a string"))?),
            _ => return Err(unknown_field(key, TRACE_STEP_FIELDS)),
        }
    }
    Ok(DecodedTraceStep {
        index: required(index, "index")?,
        input: required(input, "input")?,
        before_data: required(before_data, "before_data")?,
        after_data: required(after_data, "after_data")?,
        before_session: required(before_session, "before_session")?,
        after_session: required(after_session, "after_session")?,
        outputs: required(outputs, "outputs")?,
        outcome: required(outcome, "outcome")?,
        diagnostic: diagnostic.unwrap_or(None),
    })
}

fn decode_observations(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedObservation>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), "run observation count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_observation(value, cx)?);
    }
    Ok(decoded)
}

fn decode_runtime_limits(value: Value, cx: &mut AllocationContext) -> Result<RuntimeLimits, ValueDecodeError> {
    let fields = struct_fields(value, "a struct RuntimeLimits", RUNTIME_LIMITS_FIELDS, cx)?;
    let mut fuel = None;
    let mut collection_items = None;
    let mut action_steps = None;
    let mut transaction_writes = None;
    let mut output_bytes = None;
    let mut elapsed_millis = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "fuel" => fuel = Some(unsigned_u64(value)?),
            "collection_items" => collection_items = Some(integer_usize(value)?),
            "action_steps" => action_steps = Some(integer_usize(value)?),
            "transaction_writes" => transaction_writes = Some(integer_usize(value)?),
            "output_bytes" => output_bytes = Some(integer_usize(value)?),
            "elapsed_millis" => elapsed_millis = Some(unsigned_u64(value)?),
            _ => return Err(unknown_field(key, RUNTIME_LIMITS_FIELDS)),
        }
    }
    Ok(RuntimeLimits {
        fuel: required(fuel, "fuel")?,
        collection_items: required(collection_items, "collection_items")?,
        action_steps: required(action_steps, "action_steps")?,
        transaction_writes: required(transaction_writes, "transaction_writes")?,
        output_bytes: required(output_bytes, "output_bytes")?,
        elapsed_millis: required(elapsed_millis, "elapsed_millis")?,
    })
}

fn decode_strings(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<String>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(string(value, "a string")?);
    }
    Ok(decoded)
}

fn decode_digests(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<Digest>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(digest(value)?);
    }
    Ok(decoded)
}

fn evidence_state(value: Value) -> Result<EvidenceState, ValueDecodeError> {
    match unit_variant(value, EVIDENCE_STATES, "unit enum EvidenceState")?.as_str() {
        "observed" => Ok(EvidenceState::Observed),
        "no_difference_found" => Ok(EvidenceState::NoDifferenceFound),
        "inconclusive" => Ok(EvidenceState::Inconclusive),
        "unsupported" => Ok(EvidenceState::Unsupported),
        "stale" => Ok(EvidenceState::Stale),
        "failed" => Ok(EvidenceState::Failed),
        _ => unreachable!("variant was checked"),
    }
}

fn execution_origin(value: Value) -> Result<ExecutionOrigin, ValueDecodeError> {
    match unit_variant(value, EXECUTION_ORIGINS, "unit enum ExecutionOrigin")?.as_str() {
        "production_runtime" => Ok(ExecutionOrigin::ProductionRuntime),
        "legacy_runtime" => Ok(ExecutionOrigin::LegacyRuntime),
        "test_fixture" => Ok(ExecutionOrigin::TestFixture),
        _ => unreachable!("variant was checked"),
    }
}

fn step_outcome(value: Value) -> Result<StepOutcome, ValueDecodeError> {
    match unit_variant(value, STEP_OUTCOMES, "unit enum StepOutcome")?.as_str() {
        "applied" => Ok(StepOutcome::Applied),
        "rejected" => Ok(StepOutcome::Rejected),
        "failed" => Ok(StepOutcome::Failed),
        "budget_exhausted" => Ok(StepOutcome::BudgetExhausted),
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

fn struct_fields(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
    cx: &mut AllocationContext,
) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
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

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value, "a lowercase 64-character digest")?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

fn integer_usize(value: Value) -> Result<usize, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "usize" }),
        Value::Number(Number::I64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "usize" }),
        value => Err(invalid_type(value, "usize")),
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

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn optional<T>(value: Value, decode: impl FnOnce(Value) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => decode(value).map(Some),
    }
}

fn bounded(len: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > MAX_ITEMS { Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS }) } else { Ok(()) }
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
    use serde_json::{json, Value as SerdeValue};
    use std::num::NonZeroUsize;

    fn digest(ch: char) -> Digest { Digest::try_from(ch.to_string().repeat(64)).unwrap() }

    fn fixture() -> super::super::RunEvidence {
        let source = SourceBinding {
            project_id: "project".into(),
            program_path: "apps/example.json".into(),
            task: Some(super::super::TaskSource { task_id: "task".into(), source_fingerprint: "fingerprint".into() }),
            producer: super::super::Producer::LiveAgent {
                provider: "provider".into(), invocation_id: "invocation".into(), session_id: Some("session".into()), request_digest: digest('a'),
            },
            source_digest: digest('b'),
        };
        let artifact = super::super::ArtifactRef {
            kind: super::super::ArtifactKind::GeneratedApp,
            version: 1,
            language: "product-contract-v1".into(),
            raw_digest: digest('b'),
            program_digest: digest('c'),
            semantic_digest: digest('d'),
        };
        let binding = RunBinding {
            source: source.clone(), artifact, data_digest: digest('e'), input_digest: digest('f'), scenario_digest: digest('0'),
            decision_digest: digest('1'), session_digest: digest('2'), runtime_version: "runtime".into(), driver_version: "driver".into(),
        };
        let mut values = super::super::Values::new();
        values.insert("name".into(), super::super::DataValue::Text { value: "row value".into() });
        let local_artifact = super::super::LocalArtifact {
            output: "export".into(), format: super::super::OutputFormat::Json,
            columns: vec![super::super::FieldDefinition { id: "name".into(), label: "Name".into(), value_type: super::super::Type::Text }],
            rows: vec![values.clone()], bytes: vec![b'{', b'}'], bytes_digest: digest('3'), digest: digest('4'),
        };
        let observation = super::super::Observation {
            point: "point".into(), data_digest: digest('e'), session_digest: digest('2'), values: values.clone(),
            value_types: std::collections::BTreeMap::from([("name".into(), super::super::Type::Text)]),
            view: super::super::ViewObservation {
                view: "view".into(),
                rows: vec![super::super::PresentedRow {
                    record: super::super::RecordRef { entity: "person".into(), record: "p1".into() },
                    cells: values.clone(), enabled_actions: std::collections::BTreeSet::from(["edit".into()]),
                }],
                controls: Default::default(),
                selected: vec![super::super::RecordRef { entity: "person".into(), record: "p1".into() }],
                enabled_actions: std::collections::BTreeSet::from(["open".into()]),
                form_values: values.clone(),
            },
            view_schema: Some(super::super::ViewSchema {
                entity: "person".into(), columns: std::collections::BTreeMap::from([("name".into(), super::super::Type::Text)]),
                row_actions: Some(std::collections::BTreeMap::from([("edit".into(), "edit_person".into())])),
            }),
            outputs: vec![local_artifact],
        };
        super::super::RunEvidence {
            version: 1, id: "run".into(), origin: ExecutionOrigin::ProductionRuntime, binding, source_after: source,
            state: EvidenceState::Observed,
            trace: vec![super::super::TraceStep {
                index: 0, input: super::super::SemanticInput::Invoke { action: "rename".into(), arguments: values }, before_data: digest('e'), after_data: digest('e'),
                before_session: digest('2'), after_session: digest('2'), outputs: vec![digest('4')], outcome: StepOutcome::Applied, diagnostic: Some("fixture diagnostic".into()),
            }],
            observations: vec![observation],
            limits: RuntimeLimits::default(), errors: vec![], uncovered: vec![],
        }
    }

    fn syntax(bytes: &[u8]) -> Value { product_json::parse_json_bytes(bytes, &mut AllocationContext::new()).unwrap() }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
    }

    fn assert_canonical(expected: &impl serde::Serialize, actual: &impl serde::Serialize) {
        let expected = super::super::canonical_bytes(expected).unwrap();
        let actual = super::super::canonical_bytes(actual).unwrap();
        assert_eq!(std::str::from_utf8(&expected).unwrap(), std::str::from_utf8(&actual).unwrap());
    }

    fn unit_wire(variant: &str) -> SerdeValue {
        let mut wire = serde_json::Map::new();
        wire.insert(variant.into(), json!(null));
        SerdeValue::Object(wire)
    }

    #[test]
    fn complete_run_evidence_graph_preserves_canonical_bytes() {
        let evidence = fixture();
        let bytes = serde_json::to_vec(&evidence).unwrap();
        let oracle: super::super::RunEvidence = serde_json::from_slice(&bytes).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_run_evidence(syntax(&bytes), &mut cx).unwrap();
        assert_canonical(&oracle, &decoded);
    }

    #[test]
    fn run_binding_accepts_artifact_kind_map_form() {
        let evidence = fixture();
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["binding"]["artifact"]["kind"] = unit_wire("generated_app");
        let bytes = serde_json::to_vec(&wire).unwrap();
        let oracle: super::super::RunEvidence = serde_json::from_slice(&bytes).unwrap();
        let decoded = decode_run_evidence(syntax(&bytes), &mut AllocationContext::new()).unwrap();
        assert_canonical(&oracle, &decoded);
    }

    #[test]
    fn all_unit_enum_variants_and_map_forms_match_serde() {
        for value in ["observed", "no_difference_found", "inconclusive", "unsupported", "stale", "failed"] {
            let wire = json!(value);
            let oracle: EvidenceState = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(oracle, evidence_state(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
            let wire = unit_wire(value);
            assert_eq!(oracle, evidence_state(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
        }
        for value in ["production_runtime", "legacy_runtime", "test_fixture"] {
            let wire = json!(value);
            let oracle: ExecutionOrigin = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(oracle, execution_origin(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
            let wire = unit_wire(value);
            assert_eq!(oracle, execution_origin(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
        }
        for value in ["applied", "rejected", "failed", "budget_exhausted"] {
            let wire = json!(value);
            let oracle: StepOutcome = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(oracle, step_outcome(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
            let wire = unit_wire(value);
            assert_eq!(oracle, step_outcome(syntax(&serde_json::to_vec(&wire).unwrap())).unwrap());
        }
        assert!(matches!(unit_variant(syntax(br#"{"observed":true}"#), EVIDENCE_STATES, "unit enum"), Err(ValueDecodeError::InvalidType { expected: "unit", .. })));
        assert!(matches!(unit_variant(syntax(br#"{"observed":null,"stale":null}"#), EVIDENCE_STATES, "unit enum"), Err(ValueDecodeError::InvalidLength { actual: 2, expected: 1 })));
        assert!(matches!(unit_variant(syntax(br#"{"future":true}"#), EVIDENCE_STATES, "unit enum"), Err(ValueDecodeError::UnknownVariant { tag, .. }) if tag == "future"));
    }

    #[test]
    fn optional_diagnostic_and_source_defaults_match_serde() {
        let evidence = fixture();
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0].as_object_mut().unwrap().remove("diagnostic");
        wire["binding"]["source"].as_object_mut().unwrap().remove("task");
        wire["source_after"].as_object_mut().unwrap().remove("task");
        wire["binding"]["source"]["producer"].as_object_mut().unwrap().remove("session_id");
        wire["source_after"]["producer"].as_object_mut().unwrap().remove("session_id");
        let bytes = serde_json::to_vec(&wire).unwrap();
        let oracle: super::super::RunEvidence = serde_json::from_slice(&bytes).unwrap();
        let decoded = decode_run_evidence(syntax(&bytes), &mut AllocationContext::new()).unwrap();
        assert_canonical(&oracle, &decoded);

    }

    #[test]
    fn struct_sequences_match_serde_field_order() {
        let evidence = fixture();
        let mut wire = serde_json::to_value(&evidence).unwrap();
        let trace = wire["trace"][0].as_object().unwrap();
        let fields = ["index", "input", "before_data", "after_data", "before_session", "after_session", "outputs", "outcome", "diagnostic"];
        wire["trace"][0] = SerdeValue::Array(fields.iter().map(|field| trace[*field].clone()).collect());
        let limits = wire["limits"].as_object().unwrap();
        let fields = ["fuel", "collection_items", "action_steps", "transaction_writes", "output_bytes", "elapsed_millis"];
        wire["limits"] = SerdeValue::Array(fields.iter().map(|field| limits[*field].clone()).collect());
        let fields = ["version", "id", "origin", "binding", "source_after", "state", "trace", "observations", "limits", "errors", "uncovered"];
        let sequence = SerdeValue::Array(fields.iter().map(|field| wire[*field].clone()).collect());
        let bytes = serde_json::to_vec(&sequence).unwrap();
        let oracle: super::super::RunEvidence = serde_json::from_slice(&bytes).unwrap();
        let decoded = decode_run_evidence(syntax(&bytes), &mut AllocationContext::new()).unwrap();
        assert_canonical(&oracle, &decoded);

        let mut overlong = serde_json::to_value(&evidence).unwrap();
        let trace = overlong["trace"][0].as_object().unwrap();
        let fields = ["index", "input", "before_data", "after_data", "before_session", "after_session", "outputs", "outcome", "diagnostic"];
        let mut sequence: Vec<_> = fields.iter().map(|field| trace[*field].clone()).collect();
        sequence.push(SerdeValue::Null);
        overlong["trace"][0] = SerdeValue::Array(sequence);
        let bytes = serde_json::to_vec(&overlong).unwrap();
        assert!(serde_json::from_slice::<super::super::RunEvidence>(&bytes).is_err());
        assert!(matches!(decode_run_evidence(syntax(&bytes), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 10, expected: 9 })));
    }

    #[test]
    fn malformed_duplicate_range_and_collection_inputs_are_typed() {
        let evidence = fixture();
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire.as_object_mut().unwrap().remove("state");
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("state"))));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["future"] = json!(true);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["origin"] = json!("future_origin");
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["version"] = json!(u64::from(u32::MAX) + 1);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0]["index"] = json!(-1);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "usize", .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0]["diagnostic"] = json!(false);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "a string", .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0]["outcome"] = json!("future_outcome");
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { expected: STEP_OUTCOMES, .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["limits"]["fuel"] = json!(-1);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u64", .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0]["future"] = json!(true);
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { expected: TRACE_STEP_FIELDS, .. })));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["trace"][0]["input"] = json!({"kind":"control", "view":"v", "control":"c", "value":{"kind":"list", "item_type":nested_list_type(MAX_TYPE_DEPTH + 1), "items":[]}});
        assert!(matches!(decode_run_evidence(syntax(&serde_json::to_vec(&wire).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "type nesting depth", .. })));
        let duplicate = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate RunEvidence field was accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);
        let too_many = || Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect());
        assert!(matches!(decode_trace(too_many(), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "run trace count", .. })));
        assert!(matches!(decode_observations(too_many(), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "run observation count", .. })));
        assert!(matches!(decode_strings(too_many(), &mut AllocationContext::new(), "run error count"), Err(ValueDecodeError::LimitExceeded { resource: "run error count", .. })));
        assert!(matches!(decode_digests(too_many(), &mut AllocationContext::new(), "trace output count"), Err(ValueDecodeError::LimitExceeded { resource: "trace output count", .. })));
    }

    #[test]
    fn usize_conversion_rejects_values_outside_target_range() {
        let max_plus_one = (usize::MAX as u128) + 1;
        if max_plus_one <= u128::from(u64::MAX) {
            let value = Value::Number(Number::U64(max_plus_one as u64));
            assert!(matches!(integer_usize(value), Err(ValueDecodeError::InvalidValue { expected: "usize", .. })));
        }
        assert!(matches!(integer_usize(Value::Number(Number::F64(1.0))), Err(ValueDecodeError::InvalidType { expected: "usize", .. })));
    }

    fn nested_list_type(depth: usize) -> SerdeValue {
        let mut value = json!({"kind":"text"});
        for _ in 0..depth { value = json!({"kind":"list", "item":value}); }
        value
    }

    #[test]
    fn usize_limits_at_target_boundary_preserve_serde_bytes() {
        let evidence = fixture();
        let mut wire = serde_json::to_value(&evidence).unwrap();
        wire["limits"]["collection_items"] = json!(usize::MAX);
        let bytes = serde_json::to_vec(&wire).unwrap();
        let oracle: super::super::RunEvidence = serde_json::from_slice(&bytes).unwrap();
        let decoded = decode_run_evidence(syntax(&bytes), &mut AllocationContext::new()).unwrap();
        assert_canonical(&oracle, &decoded);
    }

    #[test]
    fn allocation_refusal_at_each_growth_cleans_and_retries() {
        let evidence = fixture();
        let bytes = serde_json::to_vec(&evidence).unwrap();
        let expected = super::super::canonical_bytes(&evidence).unwrap();
        let mut baseline = AllocationContext::new();
        decode_run_evidence(syntax(&bytes), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_run_evidence(syntax(&bytes), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            let decoded = decode_run_evidence(syntax(&bytes), &mut retry).unwrap();
            assert_eq!(expected, super::super::canonical_bytes(&decoded).unwrap());
        }
    }
}
