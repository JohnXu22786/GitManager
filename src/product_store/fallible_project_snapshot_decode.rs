//! Private fallible decoding for the persisted ProjectSnapshot graph.
//!
//! The decoder composes the bounded Product Contract and scope DTO leaves;
//! production parsing and semantic validation remain separate.
use crate::product_alloc::{self, ordered::OrderedMap, AllocationContext, AllocationSite};
use crate::product_contract::{
    decode_adoption_plan_fallible, decode_captured_program_fallible,
    decode_data_snapshot_fallible, decode_decision_graph_fallible,
    decode_local_artifact_fallible, decode_session_state_fallible, Digest,
    FallibleDecodedAdoptionPlan, FallibleDecodedCapturedProgram,
    FallibleDecodedDataSnapshot, FallibleDecodedDecisionGraph,
    FallibleDecodedLocalArtifact, FallibleDecodedSessionState, IdentityDomain, MAX_COLLECTION,
    MAX_ITEMS,
};
use crate::product_contract::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use crate::product_json::{Number, Value};
use crate::product_store::scope::{decode_scope_envelope_fallible, FallibleDecodedScopeEnvelope};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

const PROJECT_FIELDS: &[&str] = &[
    "magic", "version", "revision", "programs", "active_revision", "data", "session",
    "clock_day", "decisions", "adoptions", "operations", "artifacts", "scope",
];
const OPERATION_RECEIPT_FIELDS: &[&str] = &["operation", "request", "revision"];
const ADOPTION_RECEIPT_FIELDS: &[&str] = &["plan", "previous", "active", "revision"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedProjectSnapshot {
    magic: String,
    version: u32,
    revision: u64,
    programs: Vec<FallibleDecodedCapturedProgram>,
    active_revision: Digest,
    data: FallibleDecodedDataSnapshot,
    session: FallibleDecodedSessionState,
    clock_day: i32,
    decisions: FallibleDecodedDecisionGraph,
    adoptions: Vec<DecodedAdoptionReceipt>,
    operations: DecodedOperationMap,
    artifacts: Vec<FallibleDecodedLocalArtifact>,
    scope: FallibleDecodedScopeEnvelope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectSnapshotCloneError {
    Allocation(crate::product_alloc::AllocationFailure),
    Invariant,
}

impl DecodedProjectSnapshot {
    pub(crate) fn canonical_bytes_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<Vec<u8>, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_bytes(self, cx)
    }

    pub(crate) fn canonical_digest_with(
        &self,
        domain: IdentityDomain,
        cx: &mut AllocationContext,
    ) -> Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(domain, self, cx)
    }

    pub(crate) fn validate_decisions_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<(), crate::product_contract::DecisionValidationError> {
        self.decisions.validate_with(cx)
    }

    pub(crate) fn decision_identity_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<Digest, crate::product_contract::DecisionValidationError> {
        self.decisions.identity_with(cx)
    }

    pub(crate) fn try_clone_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<Self, ProjectSnapshotCloneError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => {
                    ProjectSnapshotCloneError::Allocation(failure)
                }
                _ => ProjectSnapshotCloneError::Invariant,
            })?;
        decode_project_snapshot(value, cx).map_err(|error| match error {
            ValueDecodeError::Allocation(failure) => ProjectSnapshotCloneError::Allocation(failure),
            _ => ProjectSnapshotCloneError::Invariant,
        })
    }

    pub(crate) fn data(&self) -> &FallibleDecodedDataSnapshot {
        &self.data
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Serialize)]
struct DecodedOperationReceipt {
    operation: String,
    request: Digest,
    revision: u64,
}

#[derive(Debug, Serialize)]
struct DecodedAdoptionReceipt {
    plan: FallibleDecodedAdoptionPlan,
    previous: Digest,
    active: Digest,
    revision: u64,
}

struct DecodedOperationMap(OrderedMap<String, DecodedOperationReceipt>);

impl std::fmt::Debug for DecodedOperationMap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_map().entries(self.0.iter()).finish()
    }
}

impl Serialize for DecodedOperationMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub(crate) fn decode_project_snapshot(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedProjectSnapshot, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct ProjectSnapshot", PROJECT_FIELDS)?;
    let mut magic = None;
    let mut version = None;
    let mut revision = None;
    let mut programs = None;
    let mut active_revision = None;
    let mut data = None;
    let mut session = None;
    let mut clock_day = None;
    let mut decisions = None;
    let mut adoptions = None;
    let mut operations = None;
    let mut artifacts = None;
    let mut scope = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "magic" => magic = Some(string(value)?),
            "version" => version = Some(integer_u32(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            "programs" => programs = Some(decode_vec(value, MAX_ITEMS, "project program count", cx, decode_captured_program_fallible)?),
            "active_revision" => active_revision = Some(digest(value)?),
            "data" => data = Some(decode_data_snapshot_fallible(value, cx)?),
            "session" => session = Some(decode_session_state_fallible(value, cx)?),
            "clock_day" => clock_day = Some(integer_i32(value)?),
            "decisions" => decisions = Some(decode_decision_graph_fallible(value, cx)?),
            "adoptions" => adoptions = Some(decode_vec(value, MAX_ITEMS, "project adoption receipt count", cx, decode_adoption_receipt)?),
            "operations" => operations = Some(decode_operation_map(value, cx)?),
            "artifacts" => artifacts = Some(decode_vec(value, MAX_ITEMS, "project artifact count", cx, decode_local_artifact_fallible)?),
            "scope" => scope = Some(decode_scope_envelope_fallible(value, cx)?),
            _ => return Err(ValueDecodeError::UnknownField { key, expected: PROJECT_FIELDS }),
        }
    }
    Ok(DecodedProjectSnapshot {
        magic: required(magic, "magic")?,
        version: required(version, "version")?,
        revision: required(revision, "revision")?,
        programs: required(programs, "programs")?,
        active_revision: required(active_revision, "active_revision")?,
        data: required(data, "data")?,
        session: required(session, "session")?,
        clock_day: required(clock_day, "clock_day")?,
        decisions: required(decisions, "decisions")?,
        adoptions: required(adoptions, "adoptions")?,
        operations: required(operations, "operations")?,
        artifacts: required(artifacts, "artifacts")?,
        scope: required(scope, "scope")?,
    })
}

fn decode_operation_map(value: Value, cx: &mut AllocationContext) -> Result<DecodedOperationMap, ValueDecodeError> {
    let Value::Object(entries) = value else { return Err(invalid_type(value, "a map")); };
    if entries.len() > MAX_COLLECTION {
        return Err(ValueDecodeError::LimitExceeded { resource: "project operation count", limit: MAX_COLLECTION });
    }
    let mut decoded = OrderedMap::new();
    for (key, value) in entries.into_entries() {
        let receipt = decode_operation_receipt(value, cx)?;
        decoded.try_insert(key, receipt, cx)?;
    }
    Ok(DecodedOperationMap(decoded))
}

fn decode_operation_receipt(value: Value, cx: &mut AllocationContext) -> Result<DecodedOperationReceipt, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OperationReceipt", OPERATION_RECEIPT_FIELDS)?;
    let mut operation = None;
    let mut request = None;
    let mut revision = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "operation" => operation = Some(string(value)?),
            "request" => request = Some(digest(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            _ => return Err(ValueDecodeError::UnknownField { key, expected: OPERATION_RECEIPT_FIELDS }),
        }
    }
    Ok(DecodedOperationReceipt {
        operation: required(operation, "operation")?,
        request: required(request, "request")?,
        revision: required(revision, "revision")?,
    })
}

fn decode_adoption_receipt(value: Value, cx: &mut AllocationContext) -> Result<DecodedAdoptionReceipt, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct AdoptionReceipt", ADOPTION_RECEIPT_FIELDS)?;
    let mut plan = None;
    let mut previous = None;
    let mut active = None;
    let mut revision = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "plan" => plan = Some(decode_adoption_plan_fallible(value, cx)?),
            "previous" => previous = Some(digest(value)?),
            "active" => active = Some(digest(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            _ => return Err(ValueDecodeError::UnknownField { key, expected: ADOPTION_RECEIPT_FIELDS }),
        }
    }
    Ok(DecodedAdoptionReceipt {
        plan: required(plan, "plan")?,
        previous: required(previous, "previous")?,
        active: required(active, "active")?,
        revision: required(revision, "revision")?,
    })
}

fn decode_vec<T>(
    value: Value,
    limit: usize,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Vec<T>, ValueDecodeError> {
    let Value::Array(values) = value else { return Err(invalid_type(value, "a sequence")); };
    if values.len() > limit {
        return Err(ValueDecodeError::LimitExceeded { resource, limit });
    }
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode(value, cx)?);
    }
    Ok(decoded)
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
        Value::Array(values) => Ok(StructFields::Sequence { names, actual: values.len(), values: values.into_iter(), index: 0 }),
        value => Err(invalid_type(value, expected)),
    }
}

fn string(value: Value) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = match value {
        Value::String(value) => value,
        value => return Err(invalid_type(value, "a lowercase 64-character digest")),
    };
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}

fn integer_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn integer_u64(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }),
        value => Err(invalid_type(value, "u64")),
    }
}

fn integer_i32(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i32" }),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "i32" }),
        value => Err(invalid_type(value, "i32")),
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
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
    use crate::product_contract::{canonical_bytes, MAX_ITEMS};
    use crate::product_json::{self, Cause};
    use crate::product_store::{ProjectSnapshot, scope::{ScopeEnvelope, ScopeState}};
    use std::num::NonZeroUsize;

    const D0: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const D1: &str = "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const D2: &str = "2123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const D3: &str = "3123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn syntax(value: &serde_json::Value) -> Value {
        product_json::parse_json_bytes(&serde_json::to_vec(value).unwrap(), &mut AllocationContext::new()).unwrap()
    }

    fn fixture() -> serde_json::Value {
        let source = json!({
            "project_id":"project", "program_path":"app.json", "task":null,
            "producer":{"kind":"user_authored"}, "source_digest":D0
        });
        let target = json!({
            "kind":"generated_app", "version":1, "language":"json-v1",
            "raw_digest":D0, "program_digest":D1, "semantic_digest":D2
        });
        let binding = json!({
            "source":source, "artifact":target,
            "data_digest":D1, "input_digest":D2, "scenario_digest":D3,
            "decision_digest":D0, "session_digest":D1,
            "runtime_version":"runtime", "driver_version":"driver"
        });
        let plan = json!({
            "version":1,"id":"adoption","project_id":"project","expected_generation":4,
            "expected_data":D1,"expected_decisions":D2,"expected_session":D3,
            "current_source":source,"target":target,
            "scope":{"operations":["read","edit","read"],"population":{"kind":"all"},"conditions":{},"excluded_records":[],"unknowns":[]},
            "compatibility":{"state":"compatible","current_data":D1,"target_program":D1,"retained_records":[],"retained_events":[],"retained_fields":[],"issues":[]},
            "required_decisions":["decision"],
            "checks":[{"decision":"decision","decision_digest":D0,"property_digest":D1,"scope":"applies","state":"satisfied","binding":binding,"evidence":D2,"explanation":"checked"}],
            "evidence":[D2],"retire_decisions":[]
        });
        let scope = serde_json::to_value(ScopeEnvelope::ScalarV1(ScopeState::default())).unwrap();
        json!({
            "magic":"gitmanager.generated-project","version":3,"revision":9,
            "programs":[{
                "artifact":target,"binding":source,"source_bytes":[123,125],
                "program":{"version":1,"id":"app","label":"App","entities":[],"state":[],"actions":[],"outputs":[],"observables":[],"views":[],"initial_view":"main"}
            }],
            "active_revision":D1,
            "data":{"version":1,"project_id":"project","generation":2,"schema":[],"records":[],"events":[]},
            "session":{"view":"main","values":{"theme":{"kind":"text","value":"dark"}},"focused_record":null},
            "clock_day":17,"decisions":{"version":1,"revision":2,"decisions":[]},
            "adoptions":[{"plan":plan,"previous":D0,"active":D1,"revision":5}],
            "operations":{"zeta":{"operation":"zeta","request":D3,"revision":7},"alpha":{"operation":"alpha","request":D2,"revision":6}},
            "artifacts":[{"output":"report","format":"json","columns":[],"rows":[{"note":{"kind":"text","value":"✓"}}],"bytes":[123],"bytes_digest":D2,"digest":D3}],
            "scope":scope
        })
    }

    fn decoded(source: &serde_json::Value) -> DecodedProjectSnapshot {
        decode_project_snapshot(syntax(source), &mut AllocationContext::new()).unwrap()
    }

    fn struct_sequence(value: &mut serde_json::Value, fields: &[&str]) {
        let object = value.as_object().expect("fixture object");
        *value = serde_json::Value::Array(fields.iter().map(|field| object[*field].clone()).collect());
    }

    fn assert_every_growth_refuses_and_retries(input: &serde_json::Value) {
        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let mut baseline = AllocationContext::new();
        let decoded = decode_project_snapshot(syntax(input), &mut baseline).unwrap();
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&decoded).unwrap());
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            let result = decode_project_snapshot(syntax(input), &mut cx);
            let Err(ValueDecodeError::Allocation(failure)) = result else { panic!("growth {ordinal}/{growths} should refuse") };
            assert_eq!(Some(failure), cx.first_failure());
            assert_eq!(failure.kind, AllocationFailureKind::Injected);
            let retry = decode_project_snapshot(syntax(input), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&retry).unwrap());
        }
    }

    #[test]
    fn full_project_graph_and_receipts_match_typed_canonical_bytes() {
        let input = fixture();
        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let actual = decoded(&input);
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
        assert_eq!(actual.operations.0.get("alpha").unwrap().operation, "alpha");
        assert_eq!(actual.operations.0.get_index(0).unwrap().0, "alpha");

        let mut no_evidence = input;
        no_evidence["adoptions"][0]["plan"]["checks"][0].as_object_mut().unwrap().remove("evidence");
        let expected: ProjectSnapshot = serde_json::from_value(no_evidence.clone()).unwrap();
        let actual = decoded(&no_evidence);
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());

        let mut unit_map = fixture();
        unit_map["adoptions"][0]["plan"]["checks"][0]["scope"] = json!({"applies":null});
        let expected: ProjectSnapshot = serde_json::from_value(unit_map.clone()).unwrap();
        let actual = decoded(&unit_map);
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
    }

    #[test]
    fn project_snapshot_decision_wrappers_use_fallible_validation_and_identity() {
        let mut input = fixture();
        input["decisions"] = serde_json::json!({
            "version":1,"revision":5,"decisions":[{
                "id":"decision","revision":1,"request":"Keep this behavior",
                "scope":{"operations":["edit"],"population":{"kind":"all"},"conditions":{},"excluded_records":[],"unknowns":[]},
                "outcome":{"kind":"keep_current"},"status":{"kind":"active"},
                "obligations":[],"scenarios":[D0],"witness":D1,"supersedes":[]
            }]
        });
        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let decoded = decoded(&input);
        let mut cx = AllocationContext::new();
        decoded.validate_decisions_with(&mut cx).unwrap();
        let mut cx = AllocationContext::new();
        let actual_identity = decoded.decision_identity_with(&mut cx).unwrap();
        assert_eq!(actual_identity, expected.decisions.identity().unwrap());
    }

    #[test]
    fn promoted_project_snapshot_has_fallible_bytes_digest_and_clone() {
        let input = fixture();
        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let actual = decoded(&input);
        let expected_bytes = canonical_bytes(&expected).unwrap();

        let mut bytes_cx = AllocationContext::new();
        assert_eq!(actual.canonical_bytes_with(&mut bytes_cx).unwrap(), expected_bytes);
        let byte_growths = bytes_cx.growth_requests();
        assert!(byte_growths > 0);
        for nth in 1..=byte_growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(
                actual.canonical_bytes_with(&mut cx),
                Err(crate::product_backup::fallible_codec::CodecError::Allocation(_))
            ));
        }

        let mut digest_cx = AllocationContext::new();
        assert_eq!(
            actual.canonical_digest_with(IdentityDomain::Data, &mut digest_cx).unwrap(),
            crate::product_contract::canonical_digest(IdentityDomain::Data, &expected).unwrap(),
        );

        let mut clone_cx = AllocationContext::new();
        let cloned = actual.try_clone_with(&mut clone_cx).unwrap();
        assert_eq!(actual.canonical_bytes_with(&mut AllocationContext::new()).unwrap(),
                   cloned.canonical_bytes_with(&mut AllocationContext::new()).unwrap());
        let clone_growths = clone_cx.growth_requests();
        assert!(clone_growths > 0);
        for nth in 1..=clone_growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = actual.try_clone_with(&mut cx).unwrap_err();
            let ProjectSnapshotCloneError::Allocation(failure) = error else {
                panic!("allocation refusal was not preserved: {error:?}");
            };
            assert_eq!(cx.first_failure(), Some(failure));
            assert_eq!(expected_bytes, actual.canonical_bytes_with(&mut AllocationContext::new()).unwrap());
            let retried = actual.try_clone_with(&mut AllocationContext::new()).unwrap();
            assert_eq!(expected_bytes, retried.canonical_bytes_with(&mut AllocationContext::new()).unwrap());
        }
    }

    #[test]
    fn owned_scope_envelope_composes_inside_project_snapshot() {
        let mut input = fixture();
        input["scope"] = json!({"family":"owned_v2","state":{"version":2,"layers":{},"compositions":{},"initializations":[],"adoptions":[],"rehearsals":{},"correspondences":{}}});
        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let actual = decoded(&input);
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
        assert_every_growth_refuses_and_retries(&input);
    }

    #[test]
    fn project_and_inline_receipts_accept_serde_sequence_forms() {
        let mut input = fixture();
        struct_sequence(&mut input["adoptions"][0]["plan"]["checks"][0]["binding"]["source"], &["project_id", "program_path", "task", "producer", "source_digest"]);
        struct_sequence(&mut input["adoptions"][0]["plan"]["checks"][0]["binding"]["artifact"], &["kind", "version", "language", "raw_digest", "program_digest", "semantic_digest"]);
        struct_sequence(&mut input["adoptions"][0]["plan"]["checks"][0]["binding"], &["source", "artifact", "data_digest", "input_digest", "scenario_digest", "decision_digest", "session_digest", "runtime_version", "driver_version"]);
        struct_sequence(&mut input["adoptions"][0]["plan"]["checks"][0], &["decision", "decision_digest", "property_digest", "scope", "state", "binding", "evidence", "explanation"]);
        struct_sequence(&mut input["adoptions"][0]["plan"]["scope"], &["operations", "population", "conditions", "excluded_records", "unknowns"]);
        struct_sequence(&mut input["adoptions"][0]["plan"]["compatibility"], &["state", "current_data", "target_program", "retained_records", "retained_events", "retained_fields", "issues"]);
        struct_sequence(&mut input["adoptions"][0]["plan"], &["version", "id", "project_id", "expected_generation", "expected_data", "expected_decisions", "expected_session", "current_source", "target", "scope", "compatibility", "required_decisions", "checks", "evidence", "retire_decisions"]);
        struct_sequence(&mut input["adoptions"][0], &["plan", "previous", "active", "revision"]);
        struct_sequence(&mut input["operations"]["alpha"], &["operation", "request", "revision"]);
        struct_sequence(&mut input, PROJECT_FIELDS);

        let expected: ProjectSnapshot = serde_json::from_value(input.clone()).unwrap();
        let actual = decoded(&input);
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
        assert_every_growth_refuses_and_retries(&input);
    }

    #[test]
    fn malformed_unknown_duplicate_numeric_and_limit_errors_are_bounded() {
        let mut missing = fixture();
        missing.as_object_mut().unwrap().remove("magic");
        assert!(matches!(decode_project_snapshot(syntax(&missing), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("magic"))));

        let mut unknown = fixture();
        unknown["future"] = json!(true);
        assert!(matches!(decode_project_snapshot(syntax(&unknown), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let mut bad = fixture();
        bad["adoptions"][0]["plan"]["checks"][0]["binding"]["session_digest"] = json!("bad");
        let error = decode_project_snapshot(syntax(&bad), &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, ValueDecodeError::InvalidFormat("invalid lowercase digest")), "got {error:?}");

        let mut bad_range = fixture();
        bad_range["clock_day"] = json!(2147483648u64);
        assert!(matches!(decode_project_snapshot(syntax(&bad_range), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "i32", .. })));

        let mut bad_digest = fixture();
        bad_digest["active_revision"] = json!("bad");
        assert!(matches!(decode_project_snapshot(syntax(&bad_digest), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("invalid lowercase digest"))));

        let mut unknown_unit = fixture();
        unknown_unit["adoptions"][0]["plan"]["checks"][0]["scope"] = json!({"future":false});
        assert!(matches!(decode_project_snapshot(syntax(&unknown_unit), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));

        let duplicate = r#"{"magic":"a","magic":"b"}"#;
        let Err(error) = product_json::parse_json_bytes(duplicate.as_bytes(), &mut AllocationContext::new()) else { panic!("duplicate member is rejected") };
        assert!(matches!(error.cause, Cause::DuplicateObjectMember));

        let mut too_many = fixture();
        too_many["programs"] = serde_json::Value::Array((0..=MAX_ITEMS).map(|_| json!(null)).collect());
        assert!(matches!(decode_project_snapshot(syntax(&too_many), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "project program count", limit: MAX_ITEMS })));

        let mut too_many_adoptions = fixture();
        too_many_adoptions["adoptions"] = serde_json::Value::Array((0..=MAX_ITEMS).map(|_| json!(null)).collect());
        assert!(matches!(decode_project_snapshot(syntax(&too_many_adoptions), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "project adoption receipt count", limit: MAX_ITEMS })));

        let mut too_many_artifacts = fixture();
        too_many_artifacts["artifacts"] = serde_json::Value::Array((0..=MAX_ITEMS).map(|_| json!(null)).collect());
        assert!(matches!(decode_project_snapshot(syntax(&too_many_artifacts), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "project artifact count", limit: MAX_ITEMS })));

        let mut too_many_operations = fixture();
        let mut operations = serde_json::Map::new();
        for index in 0..=MAX_COLLECTION {
            operations.insert(format!("op-{index:04}"), json!(null));
        }
        too_many_operations["operations"] = serde_json::Value::Object(operations);
        assert!(matches!(decode_project_snapshot(syntax(&too_many_operations), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "project operation count", limit: MAX_COLLECTION })));

        let short = json!(["magic"]);
        assert!(matches!(decode_project_snapshot(syntax(&short), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 1, expected: 13 })));
    }

    #[test]
    fn refusal_at_each_reached_owner_growth_cleans_up_and_retries() {
        let input = fixture();
        assert_every_growth_refuses_and_retries(&input);
    }

    use serde_json::json;
}
