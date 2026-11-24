//! Allocation-aware DTO decoders for owned composition and lifecycle receipts.
//!
//! These remain disconnected from production persistence. They compose the
//! already bounded scope, ownership, and Product Contract leaf decoders.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use crate::product_contract::{Digest, MAX_COLLECTION, MAX_EVENTS, MAX_ITEMS};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_owned_basis_layer_decode::{decode_owned_basis_v2, DecodedOwnedBasisV2};
use super::fallible_owned_source_decode::{decode_owned_source_request, DecodedOwnedSourceRequest};
use super::fallible_scalar_decode::{
    decode_compatibility_report, decode_digest_set, decode_metadata_addition, decode_scope_transition,
    DecodedCompatibilityReport, DecodedMetadataAddition, FallibleDigestSet,
};

const IMPLEMENTATION_CORRESPONDENCE_FIELDS: &[&str] = &["version", "source", "target", "sources"];
const COMPOSITION_FIELDS: &[&str] = &[
    "version", "lowering", "basis", "transition", "business", "layers", "active", "previous",
    "operation", "runtime", "driver", "output", "correspondences",
];
const INITIALIZATION_FIELDS: &[&str] = &[
    "version", "operation", "layer", "before_data", "after_data", "before_schema", "after_schema",
    "additions", "auxiliary", "business_projection", "original_events", "original_records",
    "original_event_count", "capture_day", "capture_program", "prior_snapshot", "adoption_revision",
];
const ADOPTION_FIELDS: &[&str] = &[
    "version", "decisions", "plan", "composition", "initialization", "initialized_compatibility",
    "revision",
];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedImplementationCorrespondenceV2 {
    version: u32,
    source: Digest,
    target: Digest,
    sources: DecodedOwnedSourceRequest,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedCompositionV2 {
    version: u32,
    lowering: u32,
    basis: DecodedOwnedBasisV2,
    transition: super::contract::ScopeTransition,
    business: Digest,
    layers: Vec<Digest>,
    active: FallibleDigestSet,
    previous: Option<Digest>,
    operation: String,
    runtime: String,
    driver: String,
    output: Digest,
    correspondences: Vec<DecodedOwnedImplementationCorrespondenceV2>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedInitializationV2 {
    version: u32,
    operation: String,
    layer: Digest,
    before_data: Digest,
    after_data: Digest,
    before_schema: Digest,
    after_schema: Digest,
    additions: Vec<DecodedMetadataAddition>,
    auxiliary: Vec<crate::product_contract::EntityDefinition>,
    business_projection: Digest,
    original_events: Digest,
    original_records: usize,
    original_event_count: usize,
    capture_day: i32,
    capture_program: Digest,
    prior_snapshot: Digest,
    adoption_revision: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedAdoptionV2 {
    version: u32,
    decisions: Vec<String>,
    plan: Digest,
    composition: Digest,
    initialization: Option<Digest>,
    initialized_compatibility: DecodedCompatibilityReport,
    revision: u64,
}

pub(crate) fn decode_owned_implementation_correspondence_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedImplementationCorrespondenceV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedImplementationCorrespondenceV2", IMPLEMENTATION_CORRESPONDENCE_FIELDS)?;
    let (mut version, mut source, mut target, mut sources) = (None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "source" => source = Some(digest(value)?),
            "target" => target = Some(digest(value)?),
            "sources" => sources = Some(decode_owned_source_request(value, cx)?),
            _ => return Err(unknown(key, IMPLEMENTATION_CORRESPONDENCE_FIELDS)),
        }
    }
    Ok(DecodedOwnedImplementationCorrespondenceV2 {
        version: required(version, "version")?, source: required(source, "source")?,
        target: required(target, "target")?, sources: required(sources, "sources")?,
    })
}

pub(crate) fn decode_owned_composition_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedCompositionV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedCompositionV2", COMPOSITION_FIELDS)?;
    let (mut version, mut lowering, mut basis, mut transition, mut business, mut layers, mut active,
        mut previous, mut operation, mut runtime, mut driver, mut output, mut correspondences) =
        (None, None, None, None, None, None, None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "lowering" => lowering = Some(u32_value(value)?),
            "basis" => basis = Some(decode_owned_basis_v2(value, cx)?),
            "transition" => transition = Some(decode_scope_transition(value)?),
            "business" => business = Some(digest(value)?),
            "layers" => layers = Some(digest_vec(value, super::contract::MAX_LAYERS, "owned composition layer count", cx)?),
            "active" => active = Some(decode_digest_set(value, super::contract::MAX_LAYERS, "owned composition active layer count", cx)?),
            "previous" => previous = Some(optional_digest(value)?),
            "operation" => operation = Some(string(value, "a string")?),
            "runtime" => runtime = Some(string(value, "a string")?),
            "driver" => driver = Some(string(value, "a string")?),
            "output" => output = Some(digest(value)?),
            "correspondences" => correspondences = Some(decode_vec(value, MAX_ITEMS, "owned implementation correspondence count", cx, decode_owned_implementation_correspondence_v2)?),
            _ => return Err(unknown(key, COMPOSITION_FIELDS)),
        }
    }
    Ok(DecodedOwnedCompositionV2 {
        version: required(version, "version")?, lowering: required(lowering, "lowering")?,
        basis: required(basis, "basis")?, transition: required(transition, "transition")?,
        business: required(business, "business")?, layers: required(layers, "layers")?,
        active: active.ok_or(ValueDecodeError::MissingField("active"))?,
        previous: previous.unwrap_or(None), operation: required(operation, "operation")?,
        runtime: required(runtime, "runtime")?, driver: required(driver, "driver")?,
        output: required(output, "output")?, correspondences: required(correspondences, "correspondences")?,
    })
}

pub(crate) fn decode_owned_initialization_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedInitializationV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedInitializationV2", INITIALIZATION_FIELDS)?;
    let (mut version, mut operation, mut layer, mut before_data, mut after_data, mut before_schema,
        mut after_schema, mut additions, mut auxiliary, mut business_projection, mut original_events,
        mut original_records, mut original_event_count, mut capture_day, mut capture_program,
        mut prior_snapshot, mut adoption_revision) =
        (None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "operation" => operation = Some(string(value, "a string")?),
            "layer" => layer = Some(digest(value)?),
            "before_data" => before_data = Some(digest(value)?),
            "after_data" => after_data = Some(digest(value)?),
            "before_schema" => before_schema = Some(digest(value)?),
            "after_schema" => after_schema = Some(digest(value)?),
            "additions" => additions = Some(decode_vec(value, MAX_COLLECTION, "owned initialization addition count", cx, decode_metadata_addition)?),
            "auxiliary" => auxiliary = Some(decode_vec(value, MAX_ITEMS, "owned initialization auxiliary entity count", cx, crate::product_contract::fallible_entity_decode::decode_entity_definition)?),
            "business_projection" => business_projection = Some(digest(value)?),
            "original_events" => original_events = Some(digest(value)?),
            "original_records" => original_records = Some(usize_value(value)?),
            "original_event_count" => original_event_count = Some(usize_value(value)?),
            "capture_day" => capture_day = Some(i32_value(value)?),
            "capture_program" => capture_program = Some(digest(value)?),
            "prior_snapshot" => prior_snapshot = Some(digest(value)?),
            "adoption_revision" => adoption_revision = Some(u64_value(value)?),
            _ => return Err(unknown(key, INITIALIZATION_FIELDS)),
        }
    }
    let version = required(version, "version")?;
    let operation = required(operation, "operation")?;
    let layer = required(layer, "layer")?;
    let before_data = required(before_data, "before_data")?;
    let after_data = required(after_data, "after_data")?;
    let before_schema = required(before_schema, "before_schema")?;
    let after_schema = required(after_schema, "after_schema")?;
    let additions = required(additions, "additions")?;
    let auxiliary = required(auxiliary, "auxiliary")?;
    let business_projection = required(business_projection, "business_projection")?;
    let original_events = required(original_events, "original_events")?;
    let original_records = required(original_records, "original_records")?;
    let original_event_count = required(original_event_count, "original_event_count")?;
    let capture_day = required(capture_day, "capture_day")?;
    let capture_program = required(capture_program, "capture_program")?;
    let prior_snapshot = required(prior_snapshot, "prior_snapshot")?;
    let adoption_revision = required(adoption_revision, "adoption_revision")?;
    bounded(original_records, MAX_COLLECTION, "owned initialization original record count")?;
    bounded(original_event_count, MAX_EVENTS, "owned initialization original event count")?;
    Ok(DecodedOwnedInitializationV2 {
        version, operation, layer, before_data, after_data, before_schema, after_schema, additions,
        auxiliary, business_projection, original_events, original_records, original_event_count,
        capture_day, capture_program, prior_snapshot, adoption_revision,
    })
}

pub(crate) fn decode_owned_adoption_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedAdoptionV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedAdoptionV2", ADOPTION_FIELDS)?;
    let (mut version, mut decisions, mut plan, mut composition, mut initialization, mut initialized_compatibility, mut revision) =
        (None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "decisions" => decisions = Some(string_vec(value, MAX_ITEMS, "owned adoption decision count", cx)?),
            "plan" => plan = Some(digest(value)?),
            "composition" => composition = Some(digest(value)?),
            "initialization" => initialization = Some(optional_digest(value)?),
            "initialized_compatibility" => initialized_compatibility = Some(decode_compatibility_report(value, cx)?),
            "revision" => revision = Some(u64_value(value)?),
            _ => return Err(unknown(key, ADOPTION_FIELDS)),
        }
    }
    Ok(DecodedOwnedAdoptionV2 {
        version: required(version, "version")?, decisions: required(decisions, "decisions")?,
        plan: required(plan, "plan")?, composition: required(composition, "composition")?,
        initialization: initialization.unwrap_or(None),
        initialized_compatibility: required(initialized_compatibility, "initialized_compatibility")?,
        revision: required(revision, "revision")?,
    })
}

fn digest_vec(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<Digest>, ValueDecodeError> {
    decode_vec(value, limit, resource, cx, |value, _| digest(value))
}
fn string_vec(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<String>, ValueDecodeError> {
    decode_vec(value, limit, resource, cx, |value, _| string(value, "a string"))
}
fn decode_vec<T>(
    value: Value,
    limit: usize,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value { Value::Array(values) => values, value => return Err(invalid_type(value, "a sequence")) };
    bounded(values.len(), limit, resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(decode(value, cx)?); }
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
                    let Some(value) = values.next() else { return Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() }); };
                    *index += 1;
                    Ok(Some((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value)))
                } else if values.next().is_some() {
                    Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() })
                } else { Ok(None) }
            }
        }
    }
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str]) -> Result<StructFields, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(StructFields::Object(fields.into_entries())),
        Value::Array(values) => { let actual = values.len(); Ok(StructFields::Sequence { names, values: values.into_iter(), index: 0, actual }) }
        value => Err(invalid_type(value, expected)),
    }
}
fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    Digest::from_owned_lower_hex(string(value, "a lowercase 64-character digest")?).ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}
fn optional_digest(value: Value) -> Result<Option<Digest>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => digest(value).map(Some) }
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
}
fn u32_value(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}
fn u64_value(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }),
        value => Err(invalid_type(value, "u64")),
    }
}
fn usize_value(value: Value) -> Result<usize, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "usize" }),
        Value::Number(Number::I64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "usize" }),
        value => Err(invalid_type(value, "usize")),
    }
}
fn i32_value(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i32" }),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "i32" }),
        value => Err(invalid_type(value, "i32")),
    }
}
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn bounded(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) }
}
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null, Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger, Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint, Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence, Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationFailureKind;
    use crate::product_contract::canonical_bytes;
    use crate::product_json;
    use crate::product_store::scope::owned_wire::{OwnedAdoptionV2, OwnedCompositionV2, OwnedInitializationV2};
    use serde_json::{json, Value as SerdeValue};
    use std::num::NonZeroUsize;

    fn digest(c: char) -> String { std::iter::repeat(c).take(64).collect() }
    fn source_request() -> SerdeValue { json!({"before":digest('1'),"after":digest('2'),"root_entity":"person","edits":[]}) }
    fn data() -> SerdeValue { json!({"version":1,"project_id":"project","generation":2,"schema":[],"records":[],"events":[]}) }
    fn frozen() -> SerdeValue {
        json!({"snapshot":digest('3'),"revision":1,"active":digest('4'),"data":data(),"session":{"view":"main","values":{},"focused_record":null},"day":-1,"decisions":digest('5')})
    }
    fn basis() -> SerdeValue {
        json!({"container_format":2,"family":"owned_v2","frozen":frozen(),"sources":[],"graph":{"version":1,"revision":0,"decisions":[]},"artifacts":[]})
    }
    fn composition() -> SerdeValue {
        json!({"version":2,"lowering":2,"basis":basis(),"transition":"adoption","business":digest('6'),
            "layers":[digest('7'),digest('7'),digest('8')],"active":[digest('8'),digest('7'),digest('7')],"previous":null,
            "operation":"op","runtime":"runtime","driver":"driver","output":digest('9'),
            "correspondences":[{"version":2,"source":digest('a'),"target":digest('b'),"sources":source_request()}]})
    }
    fn entity() -> SerdeValue { json!({"id":"aux","label":"Aux","fields":[],"unique":[],"constraints":[]}) }
    fn initialization() -> SerdeValue {
        json!({"version":2,"operation":"op","layer":digest('1'),"before_data":digest('2'),"after_data":digest('3'),
            "before_schema":digest('4'),"after_schema":digest('5'),"additions":[{"record":{"entity":"person","record":"r1"},"values":{"note":{"kind":"text","value":"héllo"}}}],
            "auxiliary":[entity()],"business_projection":digest('6'),"original_events":digest('7'),"original_records":1,"original_event_count":2,
            "capture_day":-3,"capture_program":digest('8'),"prior_snapshot":digest('9'),"adoption_revision":4})
    }
    fn compatibility() -> SerdeValue { json!({"state":"compatible","current_data":digest('1'),"target_program":digest('2'),"retained_records":[],"retained_events":[],"retained_fields":[],"issues":[]}) }
    fn adoption() -> SerdeValue { json!({"version":2,"decisions":["d2","d1"],"plan":digest('3'),"composition":digest('4'),"initialization":null,"initialized_compatibility":compatibility(),"revision":5}) }
    fn syntax(value: &SerdeValue) -> Value { product_json::parse_json_bytes(&serde_json::to_vec(value).unwrap(), &mut AllocationContext::new()).unwrap() }
    fn decode<T>(value: &SerdeValue, cx: &mut AllocationContext, f: impl FnOnce(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<T, ValueDecodeError> { f(syntax(value), cx) }
    fn canonical(value: &impl Serialize) -> Vec<u8> { canonical_bytes(value).unwrap() }
    fn struct_sequence(value: &mut SerdeValue, names: &[&str]) {
        let object = value.as_object().expect("fixture struct");
        *value = SerdeValue::Array(names.iter().map(|name| object[*name].clone()).collect());
    }

    #[test]
    fn owned_composition_and_correspondences_match_canonical_bytes_and_set_order() {
        for transition in ["adoption", "withdrawal", "evolution"] {
            let mut value = composition();
            value["transition"] = json!(transition);
            let expected: OwnedCompositionV2 = serde_json::from_value(value.clone()).unwrap();
            let decoded = decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
            assert_eq!(decoded.active.iter().count(), 2, "BTreeSet duplicate entries collapse");
            assert_eq!(decoded.active.iter().next().unwrap(), &Digest::from_owned_lower_hex(digest('7')).unwrap());
        }
        let mut value = composition();
        value["transition"] = json!({"adoption":null});
        let expected: OwnedCompositionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2).unwrap()));
    }

    #[test]
    fn initialization_composes_additions_and_auxiliary_entities() {
        let value = initialization();
        let expected: OwnedInitializationV2 = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
    }

    #[test]
    fn adoption_optional_field_and_compatibility_match_serde() {
        let value = adoption();
        let expected: OwnedAdoptionV2 = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));

        let mut value = adoption();
        value.as_object_mut().unwrap().remove("initialization");
        let expected: OwnedAdoptionV2 = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        let mut value = adoption();
        value["initialization"] = json!(digest('a'));
        let expected: OwnedAdoptionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).unwrap()));

        let mut value = adoption();
        value["initialized_compatibility"]["state"] = json!({"compatible":null});
        let expected: OwnedAdoptionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).unwrap()));
    }

    #[test]
    fn struct_sequences_and_optional_composition_previous_match_serde() {
        let mut value = composition();
        value.as_object_mut().unwrap().remove("previous");
        let expected: OwnedCompositionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2).unwrap()));

        let mut value = composition();
        struct_sequence(&mut value["correspondences"][0]["sources"], &["before", "after", "root_entity", "edits"]);
        struct_sequence(&mut value["correspondences"][0], &["version", "source", "target", "sources"]);
        struct_sequence(&mut value["basis"], &["container_format", "family", "frozen", "sources", "graph", "artifacts"]);
        struct_sequence(&mut value, &["version", "lowering", "basis", "transition", "business", "layers", "active", "previous", "operation", "runtime", "driver", "output", "correspondences"]);
        let expected: OwnedCompositionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2).unwrap()));

        let mut malformed = composition();
        malformed["version"] = json!("bad");
        let mut malformed_sequence: Vec<_> = COMPOSITION_FIELDS.iter().map(|name| malformed[*name].clone()).collect();
        malformed_sequence.push(json!(null));
        assert!(matches!(decode(&SerdeValue::Array(malformed_sequence), &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));

        let mut value = initialization();
        struct_sequence(&mut value["auxiliary"][0], &["id", "label", "fields", "unique", "constraints"]);
        struct_sequence(&mut value, &["version", "operation", "layer", "before_data", "after_data", "before_schema", "after_schema", "additions", "auxiliary", "business_projection", "original_events", "original_records", "original_event_count", "capture_day", "capture_program", "prior_snapshot", "adoption_revision"]);
        let expected: OwnedInitializationV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2).unwrap()));

        let mut value = adoption();
        struct_sequence(&mut value["initialized_compatibility"], &["state", "current_data", "target_program", "retained_records", "retained_events", "retained_fields", "issues"]);
        struct_sequence(&mut value, &["version", "decisions", "plan", "composition", "initialization", "initialized_compatibility", "revision"]);
        let expected: OwnedAdoptionV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).unwrap()));

        let short = SerdeValue::Array(vec![json!(2)]);
        assert!(matches!(decode(&short, &mut AllocationContext::new(), decode_owned_adoption_v2), Err(ValueDecodeError::InvalidLength { actual: 1, expected: 7 })));

        let mut overlong = composition();
        struct_sequence(&mut overlong, COMPOSITION_FIELDS);
        overlong.as_array_mut().unwrap().push(json!(null));
        assert!(serde_json::from_value::<OwnedCompositionV2>(overlong.clone()).is_err());
        assert!(matches!(decode(&overlong, &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::InvalidLength { actual: 14, expected: 13 })));

        let mut overlong = initialization();
        struct_sequence(&mut overlong, INITIALIZATION_FIELDS);
        overlong.as_array_mut().unwrap().push(json!(null));
        assert!(serde_json::from_value::<OwnedInitializationV2>(overlong.clone()).is_err());
        assert!(matches!(decode(&overlong, &mut AllocationContext::new(), decode_owned_initialization_v2), Err(ValueDecodeError::InvalidLength { actual: 18, expected: 17 })));

        let mut overlong = adoption();
        struct_sequence(&mut overlong, ADOPTION_FIELDS);
        overlong.as_array_mut().unwrap().push(json!(null));
        assert!(serde_json::from_value::<OwnedAdoptionV2>(overlong.clone()).is_err());
        assert!(matches!(decode(&overlong, &mut AllocationContext::new(), decode_owned_adoption_v2), Err(ValueDecodeError::InvalidLength { actual: 8, expected: 7 })));
    }

    #[test]
    fn malformed_unknown_duplicate_numeric_and_collection_errors_are_bounded() {
        let mut value = composition();
        value["transition"] = json!("future");
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::UnknownVariant { .. })));
        value = composition();
        value["transition"] = json!({"adoption":false});
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::InvalidType { expected: "unit", .. })));
        value = composition();
        value["lowering"] = json!(-1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        value = composition();
        value["future"] = json!(1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_composition_v2), Err(ValueDecodeError::UnknownField { .. })));
        value = initialization();
        value["original_records"] = json!(-1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2), Err(ValueDecodeError::InvalidValue { expected: "usize", .. })));
        value = initialization();
        value["original_event_count"] = json!(MAX_EVENTS + 1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2), Err(ValueDecodeError::LimitExceeded { resource: "owned initialization original event count", .. })));
        value = initialization();
        value.as_object_mut().unwrap().remove("version");
        value["original_records"] = json!(MAX_COLLECTION + 1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2), Err(ValueDecodeError::MissingField("version"))));
        value = initialization();
        value.as_object_mut().unwrap().remove("original_records");
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2), Err(ValueDecodeError::MissingField("original_records"))));
        value = adoption();
        value["revision"] = json!(-1);
        assert!(matches!(decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2), Err(ValueDecodeError::InvalidValue { expected: "u64", .. })));
        let mut cx = AllocationContext::new();
        let too_many = Value::Array(std::iter::repeat_with(|| Value::String(digest('c'))).take(super::super::contract::MAX_LAYERS + 1).collect());
        assert!(matches!(digest_vec(too_many, super::super::contract::MAX_LAYERS, "layers", &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "layers", .. })));
        assert_eq!(cx.growth_requests(), 0);
        let duplicate_active = Value::Array(std::iter::repeat_with(|| Value::String(digest('c'))).take(super::super::contract::MAX_LAYERS + 1).collect());
        let active = decode_digest_set(duplicate_active, super::super::contract::MAX_LAYERS, "active", &mut cx).unwrap();
        assert_eq!(active.iter().count(), 1, "BTreeSet semantics collapse repeated values even when the input is longer than the distinct limit");
        let distinct_active = Value::Array((0..=super::super::contract::MAX_LAYERS).map(|n| Value::String(format!("{:064x}", n + 1))).collect());
        assert!(matches!(decode_digest_set(distinct_active, super::super::contract::MAX_LAYERS, "active", &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "active", .. })));
        let duplicate = match product_json::parse_json_bytes(br#"{"version":2,"version":3}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate field was accepted"),
        };
        assert_eq!(duplicate.cause, product_json::Cause::DuplicateObjectMember);
    }

    #[test]
    fn allocation_refusal_at_each_receipt_growth_drops_and_retries() {
        for (value, decode_fn) in [
            (composition(), decode_owned_composition_v2 as fn(Value, &mut AllocationContext) -> Result<_, _>),
        ] {
            let mut baseline = AllocationContext::new();
            decode(&value, &mut baseline, decode_fn).unwrap();
            let growths = baseline.growth_requests();
            assert!(growths > 0);
            for ordinal in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
                assert!(matches!(decode(&value, &mut cx, decode_fn), Err(ValueDecodeError::Allocation(error)) if error.kind == AllocationFailureKind::Injected), "growth {ordinal}/{growths}");
                let retry = decode(&value, &mut AllocationContext::new(), decode_fn).unwrap();
                let expected: OwnedCompositionV2 = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(canonical(&expected), canonical(&retry));
            }
        }
        let mut sequence_value = composition();
        struct_sequence(&mut sequence_value["correspondences"][0]["sources"], &["before", "after", "root_entity", "edits"]);
        struct_sequence(&mut sequence_value["correspondences"][0], &["version", "source", "target", "sources"]);
        struct_sequence(&mut sequence_value["basis"], &["container_format", "family", "frozen", "sources", "graph", "artifacts"]);
        struct_sequence(&mut sequence_value, &["version", "lowering", "basis", "transition", "business", "layers", "active", "previous", "operation", "runtime", "driver", "output", "correspondences"]);
        let expected: OwnedCompositionV2 = serde_json::from_value(sequence_value.clone()).unwrap();
        let mut baseline = AllocationContext::new();
        let decoded = decode(&sequence_value, &mut baseline, decode_owned_composition_v2).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode(&sequence_value, &mut cx, decode_owned_composition_v2), Err(ValueDecodeError::Allocation(_))), "sequence growth {ordinal}/{growths}");
            let retry = decode(&sequence_value, &mut AllocationContext::new(), decode_owned_composition_v2).unwrap();
            assert_eq!(canonical(&expected), canonical(&retry));
        }
        let value = initialization();
        let mut baseline = AllocationContext::new();
        decode(&value, &mut baseline, decode_owned_initialization_v2).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode(&value, &mut cx, decode_owned_initialization_v2), Err(ValueDecodeError::Allocation(_))));
            assert!(decode(&value, &mut AllocationContext::new(), decode_owned_initialization_v2).is_ok());
        }
        let value = adoption();
        let mut baseline = AllocationContext::new();
        decode(&value, &mut baseline, decode_owned_adoption_v2).unwrap();
        let growths = baseline.growth_requests();
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode(&value, &mut cx, decode_owned_adoption_v2), Err(ValueDecodeError::Allocation(_))));
            assert!(decode(&value, &mut AllocationContext::new(), decode_owned_adoption_v2).is_ok());
        }
    }
}
