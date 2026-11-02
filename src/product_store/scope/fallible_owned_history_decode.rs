//! Allocation-aware structural decoders for the persisted OwnedV2 history graph.
//!
//! These DTOs are deliberately separate from the production loader. They compose
//! the bounded OwnedV2 and Product Contract leaves without infallible maps.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use crate::product_contract::{
    decode_captured_program_fallible, decode_data_snapshot_fallible,
    decode_session_state_fallible, Digest,
    FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot,
    FallibleDecodedSessionState, MAX_ITEMS,
};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_owned_basis_layer_decode::{decode_owned_layer_v2, DecodedOwnedLayerV2};
use super::fallible_owned_composition_decode::{
    decode_owned_adoption_v2, decode_owned_composition_v2, decode_owned_initialization_v2,
    DecodedOwnedAdoptionV2, DecodedOwnedCompositionV2, DecodedOwnedInitializationV2,
};
use super::fallible_scalar_decode::{
    decode_compatibility_report, decode_scope_scenario_correspondence, DecodedCompatibilityReport,
    DecodedScopeScenarioCorrespondence, FallibleScopeMap,
};

const REHEARSAL_FIELDS: &[&str] = &[
    "version", "manifest", "seed", "layer", "initialization", "compatibility",
    "recorded_by", "recorded_revision", "witnesses",
];
const CORRESPONDENCE_FIELDS: &[&str] = &[
    "version", "source", "original", "target", "day", "projected", "operation_seed",
    "scenario", "layer", "composition", "initialization", "basis", "session", "decisions",
    "random_seed",
];
const CORRESPONDENCE_RECEIPT_FIELDS: &[&str] = &["proof", "operation", "revision"];
const SCOPE_STATE_FIELDS: &[&str] = &[
    "version", "layers", "compositions", "initializations", "adoptions", "rehearsals",
    "correspondences",
];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedRehearsalV2 {
    version: u32,
    manifest: DecodedOwnedCompositionV2,
    seed: Digest,
    layer: Option<DecodedOwnedLayerV2>,
    initialization: Option<DecodedOwnedInitializationV2>,
    compatibility: DecodedCompatibilityReport,
    recorded_by: String,
    recorded_revision: u64,
    witnesses: FallibleScopeMap<String, Digest>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedCorrespondenceV2 {
    version: u32,
    source: FallibleDecodedCapturedProgram,
    original: FallibleDecodedDataSnapshot,
    target: Digest,
    day: i32,
    projected: Digest,
    operation_seed: Digest,
    scenario: Option<DecodedScopeScenarioCorrespondence>,
    layer: Digest,
    composition: Digest,
    initialization: Digest,
    basis: Digest,
    session: FallibleDecodedSessionState,
    decisions: Digest,
    random_seed: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedCorrespondenceReceiptV2 {
    proof: DecodedOwnedCorrespondenceV2,
    operation: String,
    revision: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedScopeStateV2 {
    version: u32,
    layers: FallibleScopeMap<Digest, DecodedOwnedLayerV2>,
    compositions: FallibleScopeMap<Digest, DecodedOwnedCompositionV2>,
    initializations: Vec<DecodedOwnedInitializationV2>,
    adoptions: Vec<DecodedOwnedAdoptionV2>,
    rehearsals: FallibleScopeMap<Digest, DecodedOwnedRehearsalV2>,
    correspondences: FallibleScopeMap<Digest, DecodedOwnedCorrespondenceReceiptV2>,
}
impl DecodedOwnedScopeStateV2 {
    pub(crate) fn version(&self) -> u32 { self.version }
    pub(crate) fn layer(&self, id: &Digest) -> Option<&DecodedOwnedLayerV2> { self.layers.get(id) }
}

pub(crate) fn decode_owned_rehearsal_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedRehearsalV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedRehearsalV2", REHEARSAL_FIELDS);
    let (mut version, mut manifest, mut seed, mut layer, mut initialization, mut compatibility,
        mut recorded_by, mut recorded_revision, mut witnesses) =
        (None, None, None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "manifest" => manifest = Some(decode_owned_composition_v2(value, cx)?),
            "seed" => seed = Some(digest(value)?),
            "layer" => layer = Some(optional(value, cx, decode_owned_layer_v2)?),
            "initialization" => initialization = Some(optional(value, cx, decode_owned_initialization_v2)?),
            "compatibility" => compatibility = Some(decode_compatibility_report(value, cx)?),
            "recorded_by" => recorded_by = Some(string(value, "a string")?),
            "recorded_revision" => recorded_revision = Some(u64_value(value)?),
            "witnesses" => witnesses = Some(id_digest_map(value, cx)?),
            _ => return Err(unknown(key, REHEARSAL_FIELDS)),
        }
    }
    Ok(DecodedOwnedRehearsalV2 {
        version: required(version, "version")?, manifest: required(manifest, "manifest")?,
        seed: required(seed, "seed")?, layer: layer.unwrap_or(None),
        initialization: initialization.unwrap_or(None),
        compatibility: required(compatibility, "compatibility")?,
        recorded_by: required(recorded_by, "recorded_by")?,
        recorded_revision: required(recorded_revision, "recorded_revision")?,
        witnesses: required(witnesses, "witnesses")?,
    })
}

pub(crate) fn decode_owned_correspondence_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedCorrespondenceV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedCorrespondenceV2", CORRESPONDENCE_FIELDS);
    let (mut version, mut source, mut original, mut target, mut day, mut projected,
        mut operation_seed, mut scenario, mut layer, mut composition, mut initialization,
        mut basis, mut session, mut decisions, mut random_seed) =
        (None, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "source" => source = Some(decode_captured_program_fallible(value, cx)?),
            "original" => original = Some(decode_data_snapshot_fallible(value, cx)?),
            "target" => target = Some(digest(value)?),
            "day" => day = Some(i32_value(value)?),
            "projected" => projected = Some(digest(value)?),
            "operation_seed" => operation_seed = Some(digest(value)?),
            "scenario" => scenario = Some(optional(value, cx, decode_scope_scenario_correspondence)?),
            "layer" => layer = Some(digest(value)?),
            "composition" => composition = Some(digest(value)?),
            "initialization" => initialization = Some(digest(value)?),
            "basis" => basis = Some(digest(value)?),
            "session" => session = Some(decode_session_state_fallible(value, cx)?),
            "decisions" => decisions = Some(digest(value)?),
            "random_seed" => random_seed = Some(u64_value(value)?),
            _ => return Err(unknown(key, CORRESPONDENCE_FIELDS)),
        }
    }
    Ok(DecodedOwnedCorrespondenceV2 {
        version: required(version, "version")?, source: required(source, "source")?,
        original: required(original, "original")?, target: required(target, "target")?,
        day: required(day, "day")?, projected: required(projected, "projected")?,
        operation_seed: required(operation_seed, "operation_seed")?, scenario: scenario.unwrap_or(None),
        layer: required(layer, "layer")?, composition: required(composition, "composition")?,
        initialization: required(initialization, "initialization")?, basis: required(basis, "basis")?,
        session: required(session, "session")?, decisions: required(decisions, "decisions")?,
        random_seed: required(random_seed, "random_seed")?,
    })
}

pub(crate) fn decode_owned_correspondence_receipt_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedCorrespondenceReceiptV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedCorrespondenceReceiptV2", CORRESPONDENCE_RECEIPT_FIELDS);
    let (mut proof, mut operation, mut revision) = (None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "proof" => proof = Some(decode_owned_correspondence_v2(value, cx)?),
            "operation" => operation = Some(string(value, "a string")?),
            "revision" => revision = Some(u64_value(value)?),
            _ => return Err(unknown(key, CORRESPONDENCE_RECEIPT_FIELDS)),
        }
    }
    Ok(DecodedOwnedCorrespondenceReceiptV2 {
        proof: required(proof, "proof")?, operation: required(operation, "operation")?,
        revision: required(revision, "revision")?,
    })
}

pub(crate) fn decode_owned_scope_state_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedScopeStateV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedScopeStateV2", SCOPE_STATE_FIELDS);
    let (mut version, mut layers, mut compositions, mut initializations, mut adoptions,
        mut rehearsals, mut correspondences) = (None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "layers" => layers = Some(digest_map(value, super::contract::MAX_LAYERS, "owned state layer count", cx, decode_owned_layer_v2)?),
            "compositions" => compositions = Some(digest_map(value, MAX_ITEMS, "owned state composition count", cx, decode_owned_composition_v2)?),
            "initializations" => initializations = Some(decode_vec(value, super::contract::MAX_LAYERS, "owned state initialization count", cx, decode_owned_initialization_v2)?),
            "adoptions" => adoptions = Some(decode_vec(value, MAX_ITEMS, "owned state adoption count", cx, decode_owned_adoption_v2)?),
            "rehearsals" => rehearsals = Some(digest_map(value, MAX_ITEMS, "owned state rehearsal count", cx, decode_owned_rehearsal_v2)?),
            "correspondences" => correspondences = Some(digest_map(value, MAX_ITEMS, "owned state correspondence count", cx, decode_owned_correspondence_receipt_v2)?),
            _ => return Err(unknown(key, SCOPE_STATE_FIELDS)),
        }
    }
    Ok(DecodedOwnedScopeStateV2 {
        version: required(version, "version")?, layers: required(layers, "layers")?,
        compositions: required(compositions, "compositions")?,
        initializations: required(initializations, "initializations")?,
        adoptions: required(adoptions, "adoptions")?, rehearsals: required(rehearsals, "rehearsals")?,
        correspondences: required(correspondences, "correspondences")?,
    })
}

fn digest_map<V>(
    value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<V, ValueDecodeError>,
) -> Result<FallibleScopeMap<Digest, V>, ValueDecodeError> {
    let Value::Object(entries) = value else { return Err(invalid_type(value, "a map")); };
    bounded(entries.len(), limit, resource)?;
    let mut result = FallibleScopeMap::new();
    for (key, value) in entries.into_entries() {
        let key = Digest::from_owned_lower_hex(key)
            .ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))?;
        let value = decode(value, cx)?;
        result.insert(key, value, cx)?;
    }
    Ok(result)
}

fn id_digest_map(value: Value, cx: &mut AllocationContext) -> Result<FallibleScopeMap<String, Digest>, ValueDecodeError> {
    let Value::Object(entries) = value else { return Err(invalid_type(value, "a map")); };
    bounded(entries.len(), MAX_ITEMS, "owned rehearsal witness count")?;
    let mut result = FallibleScopeMap::new();
    for (key, value) in entries.into_entries() {
        result.insert(key, digest(value)?, cx)?;
    }
    Ok(result)
}

fn decode_vec<T>(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Vec<T>, ValueDecodeError> {
    let Value::Array(values) = value else { return Err(invalid_type(value, "a sequence")); };
    bounded(values.len(), limit, resource)?;
    let mut result = Vec::new();
    product_alloc::try_reserve(&mut result, values.len(), cx, AllocationSite::Vector)?;
    for value in values { result.push(decode(value, cx)?); }
    Ok(result)
}

fn optional<T>(value: Value, cx: &mut AllocationContext, decode: impl FnOnce(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    if matches!(value, Value::Null) { Ok(None) } else { decode(value, cx).map(Some) }
}

fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str]) -> StructFields {
    match value {
        Value::Object(fields) => StructFields::Object(fields.into_entries()),
        Value::Array(values) => { let actual = values.len(); StructFields::Sequence { names, values: values.into_iter(), actual, index: 0 } }
        value => StructFields::Invalid { actual: kind(&value), expected },
    }
}
enum StructFields {
    Object(std::vec::IntoIter<(String, Value)>),
    Sequence { names: &'static [&'static str], values: std::vec::IntoIter<Value>, actual: usize, index: usize },
    Invalid { actual: JsonKind, expected: &'static str },
}
impl StructFields {
    fn next(&mut self, cx: &mut AllocationContext) -> Result<Option<(String, Value)>, ValueDecodeError> {
        match self {
            Self::Object(values) => Ok(values.next()),
            Self::Sequence { names, values, actual, index } => {
                if let Some(name) = names.get(*index) {
                    let Some(value) = values.next() else { return Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() }); };
                    *index += 1;
                    Ok(Some((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value)))
                } else if values.next().is_some() {
                    Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() })
                } else { Ok(None) }
            }
            Self::Invalid { actual, expected } => Err(ValueDecodeError::InvalidType { actual: *actual, expected: *expected }),
        }
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value, "a string")?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
}
fn u32_value(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| invalid_value(NumericValue::Unsigned(value), "u32")),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| invalid_value(NumericValue::Signed(value), "u32")),
        value => Err(invalid_type(value, "u32")),
    }
}
fn u64_value(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| invalid_value(NumericValue::Signed(value), "u64")),
        value => Err(invalid_type(value, "u64")),
    }
}
fn i32_value(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| invalid_value(NumericValue::Unsigned(value), "i32")),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| invalid_value(NumericValue::Signed(value), "i32")),
        value => Err(invalid_type(value, "i32")),
    }
}
fn invalid_value(actual: NumericValue, expected: &'static str) -> ValueDecodeError { ValueDecodeError::InvalidValue { actual, expected } }
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    ValueDecodeError::InvalidType { actual: kind(&value), expected }
}
fn kind(value: &Value) -> JsonKind {
    match value {
        Value::Null => JsonKind::Null, Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String, Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    }
}
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
fn bounded(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::{decode_owned_correspondence_v2, decode_owned_scope_state_v2, DecodedOwnedScopeStateV2};
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::{canonical_bytes, Digest, MAX_EXPR_DEPTH, MAX_ITEMS};
    use crate::product_contract::fallible_value_decode::ValueDecodeError;
    use crate::product_json::{self, Value};
    use crate::product_store::scope::owned_wire::{OwnedCorrespondenceV2, OwnedScopeStateV2};
    use serde::Serialize;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn digest(c: char) -> String { std::iter::repeat(c).take(64).collect() }
    fn snapshot() -> serde_json::Value {
        serde_json::json!({"version":1,"project_id":"project","generation":3,"schema":[],"records":[],"events":[]})
    }
    fn session() -> serde_json::Value {
        serde_json::json!({"view":"main","values":{},"focused_record":null})
    }
    fn source() -> serde_json::Value {
        serde_json::json!({
            "artifact":{"kind":"generated_app","version":1,"language":"rust","raw_digest":digest('a'),"program_digest":digest('b'),"semantic_digest":digest('c')},
            "binding":{"project_id":"project","program_path":"src/app.rs","task":null,"producer":{"kind":"user_authored"},"source_digest":digest('d')},
            "source_bytes":[0,1,255],
            "program":{"version":1,"id":"app","label":"App 雪","entities":[],"state":[],"actions":[],"outputs":[],"observables":[],"views":[],"initial_view":"main"}
        })
    }
    fn correspondence(scenario: bool) -> serde_json::Value {
        let spec = serde_json::json!({"version":1,"id":"scenario","label":"Scenario","seed":snapshot(),"session":session(),"clock_day":-2,"random_seed":9,"inputs":[],"validity":[]});
        serde_json::json!({
            "version":2,"source":source(),"original":snapshot(),"target":digest('e'),"day":-4,
            "projected":digest('f'),"operation_seed":digest('1'),
            "scenario":if scenario { serde_json::json!({"original":spec,"projected":spec}) } else { serde_json::Value::Null },
            "layer":digest('2'),"composition":digest('3'),"initialization":digest('4'),"basis":digest('5'),
            "session":session(),"decisions":digest('6'),"random_seed":7
        })
    }
    fn composition() -> serde_json::Value {
        serde_json::json!({
            "version":2,"lowering":2,
            "basis":{"container_format":2,"family":"owned_v2","frozen":{"snapshot":digest('3'),"revision":1,"active":digest('4'),"data":snapshot(),"session":session(),"day":-1,"decisions":digest('5')},"sources":[],"graph":{"version":1,"revision":0,"decisions":[]},"artifacts":[]},
            "transition":"adoption","business":digest('6'),"layers":[digest('7')],"active":[digest('7')],"previous":null,
            "operation":"op","runtime":"runtime","driver":"driver","output":digest('8'),"correspondences":[]
        })
    }
    fn initialization() -> serde_json::Value {
        serde_json::json!({
            "version":2,"operation":"init","layer":digest('1'),"before_data":digest('2'),"after_data":digest('3'),
            "before_schema":digest('4'),"after_schema":digest('5'),"additions":[],"auxiliary":[],
            "business_projection":digest('6'),"original_events":digest('7'),"original_records":0,"original_event_count":0,
            "capture_day":-1,"capture_program":digest('8'),"prior_snapshot":digest('9'),"adoption_revision":4
        })
    }
    fn compatibility() -> serde_json::Value {
        serde_json::json!({"state":"compatible","current_data":digest('1'),"target_program":digest('2'),"retained_records":[],"retained_events":[],"retained_fields":[],"issues":[]})
    }
    fn decode_correspondence(value: &serde_json::Value, cx: &mut AllocationContext) -> Result<super::DecodedOwnedCorrespondenceV2, ValueDecodeError> {
        decode_owned_correspondence_v2(syntax(&serde_json::to_string(value).unwrap()), cx)
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn empty_state_matches_owned_scope_default_canonical_bytes() {
        let source = r#"{"version":2,"layers":{},"compositions":{},"initializations":[],"adoptions":[],"rehearsals":{},"correspondences":{}}"#;
        let expected: OwnedScopeStateV2 = serde_json::from_str(source).unwrap();
        let decoded: DecodedOwnedScopeStateV2 = decode_owned_scope_state_v2(syntax(source), &mut AllocationContext::new()).unwrap();
        assert_eq!(expected, OwnedScopeStateV2::default());
        assert_eq!(canonical(&expected), canonical(&decoded));
    }

    #[test]
    fn correspondence_and_scenario_graph_match_canonical_bytes() {
        for with_scenario in [false, true] {
            let value = correspondence(with_scenario);
            let expected: OwnedCorrespondenceV2 = serde_json::from_value(value.clone()).unwrap();
            let decoded = decode_correspondence(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
        }
    }

    #[test]
    fn populated_state_sorts_correspondence_keys_and_matches_canonical_bytes() {
        let state = serde_json::json!({
            "version":2,"layers":{},"compositions":{},"initializations":[],"adoptions":[],"rehearsals":{},
            "correspondences":{
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff":{"proof":correspondence(false),"operation":"second","revision":4},
                "0000000000000000000000000000000000000000000000000000000000000000":{"proof":correspondence(true),"operation":"first","revision":3}
            }
        });
        let expected: OwnedScopeStateV2 = serde_json::from_value(state.clone()).unwrap();
        let decoded = decode_owned_scope_state_v2(syntax(&serde_json::to_string(&state).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        assert_eq!(expected.correspondences.len(), 2);
    }

    #[test]
    fn rehearsal_and_populated_history_sections_match_canonical_bytes() {
        let rehearsal = serde_json::json!({
            "version":2,"manifest":composition(),"seed":digest('a'),"layer":null,"initialization":initialization(),
            "compatibility":compatibility(),"recorded_by":"owner","recorded_revision":9,
            "witnesses":{"zeta":digest('f'),"alpha":digest('0')}
        });
        let state = serde_json::json!({
            "version":2,"layers":{},"compositions":{(digest('c')):composition()},"initializations":[initialization()],
            "adoptions":[{"version":2,"decisions":["decision-b","decision-a"],"plan":digest('d'),"composition":digest('e'),"initialization":null,"initialized_compatibility":compatibility(),"revision":7}],
            "rehearsals":{(digest('a')):rehearsal},"correspondences":{}
        });
        let expected: OwnedScopeStateV2 = serde_json::from_value(state.clone()).unwrap();
        let decoded = decode_owned_scope_state_v2(syntax(&serde_json::to_string(&state).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        assert_eq!(decoded.rehearsals.keys().next(), Some(&Digest::from_owned_lower_hex(digest('a')).unwrap()));

        let mut omitted_options = rehearsal.clone();
        omitted_options.as_object_mut().unwrap().remove("layer");
        omitted_options.as_object_mut().unwrap().remove("initialization");
        let state = serde_json::json!({"version":2,"layers":{},"compositions":{},"initializations":[],"adoptions":[],"rehearsals":{(digest('a')):omitted_options},"correspondences":{}});
        let expected: OwnedScopeStateV2 = serde_json::from_value(state.clone()).unwrap();
        let decoded = decode_owned_scope_state_v2(syntax(&serde_json::to_string(&state).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
    }

    #[test]
    fn malformed_fields_limits_and_numeric_ranges_keep_fixed_classification() {
        let mut value = correspondence(false);
        value.as_object_mut().unwrap().insert("future".into(), serde_json::json!(true));
        assert!(matches!(decode_correspondence(&value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let mut value = correspondence(false);
        value.as_object_mut().unwrap().remove("session");
        assert!(matches!(decode_correspondence(&value, &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("session"))));

        let mut value = correspondence(false);
        value["day"] = serde_json::json!(i64::MAX);
        assert!(matches!(decode_correspondence(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "i32", .. })));

        let mut value = correspondence(false);
        let mut expression = serde_json::json!({"kind":"today"});
        for _ in 0..=MAX_EXPR_DEPTH { expression = serde_json::json!({"kind":"count","items":expression}); }
        value["source"]["program"]["observables"] = serde_json::json!([{"id":"count","label":"Count","value":expression}]);
        assert!(matches!(decode_correspondence(&value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: MAX_EXPR_DEPTH })));

        let source = format!(r#"{{"version":2,"layers":{{}},"compositions":{{}},"initializations":[],"adoptions":[],"rehearsals":{{}},"correspondences":{{{}}}}}"#, (0..=MAX_ITEMS).map(|i| format!("\"{:064x}\":{{}}", i)).collect::<Vec<_>>().join(","));
        assert!(matches!(decode_owned_scope_state_v2(syntax(&source), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "owned state correspondence count", limit: MAX_ITEMS })));
    }

    #[test]
    fn every_reached_growth_refusal_cleans_partial_state_and_retry_succeeds() {
        let value = serde_json::json!({
            "version":2,"layers":{},"compositions":{},"initializations":[],"adoptions":[],"rehearsals":{},
            "correspondences":{
                "0000000000000000000000000000000000000000000000000000000000000000":{"proof":correspondence(true),"operation":"retry 雪","revision":8}
            }
        });
        let source = serde_json::to_string(&value).unwrap();
        let mut baseline = AllocationContext::new();
        let expected = decode_owned_scope_state_v2(syntax(&source), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode_owned_scope_state_v2(syntax(&source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retry = decode_owned_scope_state_v2(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&retry));
        }
    }

    #[test]
    fn witness_map_is_sorted_and_allocation_refusal_is_reported() {
        let mut cx = AllocationContext::new();
        let low = Digest::from_owned_lower_hex(digest('0')).unwrap();
        let high = Digest::from_owned_lower_hex(digest('f')).unwrap();
        let map_value = format!(r#"{{"z":"{}","a":"{}"}}"#, digest('f'), digest('0'));
        let map = super::id_digest_map(syntax(&map_value), &mut cx).unwrap();
        assert_eq!(map.keys().cloned().collect::<Vec<_>>(), ["a", "z"]);
        assert_eq!(map.iter().map(|(_, v)| v).collect::<Vec<_>>(), [&low, &high]);
        let mut fail = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        assert!(matches!(super::id_digest_map(syntax(&map_value), &mut fail), Err(ValueDecodeError::Allocation(_))));
    }

    #[test]
    fn struct_sequences_match_serde_and_duplicate_map_keys_are_rejected_by_parser() {
        let value = correspondence(false);
        let sequence = serde_json::Value::Array([
            "version", "source", "original", "target", "day", "projected", "operation_seed", "scenario",
            "layer", "composition", "initialization", "basis", "session", "decisions", "random_seed",
        ].iter().map(|field| value[*field].clone()).collect());
        let expected: OwnedCorrespondenceV2 = serde_json::from_value(sequence.clone()).unwrap();
        let decoded = decode_owned_correspondence_v2(syntax(&serde_json::to_string(&sequence).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));

        let mut bad_values = vec![serde_json::json!("wrong")];
        bad_values.extend((0..15).map(|_| serde_json::Value::Null));
        let bad_sequence = serde_json::Value::Array(bad_values);
        let error = decode_owned_correspondence_v2(syntax(&serde_json::to_string(&bad_sequence).unwrap()), &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, ValueDecodeError::InvalidType { expected: "u32", .. }));

        let duplicate_state = format!(r#"{{"version":2,"layers":{{}},"compositions":{{}},"initializations":[],"adoptions":[],"rehearsals":{{}},"correspondences":{{"{}":{{"proof":{},"operation":"old","revision":1}},"{}":{{"proof":{},"operation":"new","revision":2}}}}}}"#,
            digest('a'), serde_json::to_string(&correspondence(false)).unwrap(), digest('a'), serde_json::to_string(&correspondence(true)).unwrap());
        assert!(product_json::parse_json_bytes(duplicate_state.as_bytes(), &mut AllocationContext::new()).is_err());
    }
}
