//! Private, bounded structural decoding for the scalar scope request/layer graph.
//!
//! These DTOs are intentionally not connected to production persistence yet.
//! They compose the existing Product Contract fallible decoders and retain
//! ordered fallible ownership where the persisted model uses BTreeSet.
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationSite};
use crate::product_contract::fallible_expr_decode::decode_expr;
use crate::product_contract::fallible_value_decode::{decode_type, JsonKind, NumericValue, ValueDecodeError};
use crate::product_contract::{decode_data_snapshot_fallible, decode_record_ref_fallible, decode_session_state_fallible, Digest, Expr, FallibleDecodedDataSnapshot, FallibleDecodedSessionState, RecordRef, Type, MAX_COLLECTION, MAX_EXPR_DEPTH, MAX_ITEMS};
use crate::product_json::{Number, Value};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

const POPULATION_VARIANTS: &[&str] = &["all", "future_work", "selected_unfinished"];
const SELECTED_FIELDS: &[&str] = &["records"];
const LIFECYCLE_FIELDS: &[&str] = &["entity", "completed", "source"];
const UPDATE_FIELDS: &[&str] = &["action", "path", "field"];
const EMIT_COLUMN_FIELDS: &[&str] = &["action", "path", "column"];
const VIEW_COLUMN_FIELDS: &[&str] = &["view", "column"];
const EMIT_ITEMS_FIELDS: &[&str] = &["action", "path"];
const OBSERVABLE_FIELDS: &[&str] = &["observable"];
const DESTINATION_VARIANTS: &[&str] = &["update", "create_value", "emit_column", "view_column", "emit_items", "observable"];
const PATCH_REQUEST_FIELDS: &[&str] = &["destination", "entity", "subject", "value_type"];
const REQUEST_FIELDS: &[&str] = &["population", "operations", "excluded_records", "lifecycles", "patches"];
const BASIS_FIELDS: &[&str] = &["snapshot", "revision", "active", "data", "session", "day", "decisions"];
const PATCH_FIELDS: &[&str] = &["request", "before", "after"];
const LAYER_FIELDS: &[&str] = &["version", "operation", "basis", "candidate", "request", "patches"];

/// BTreeSet-compatible unique Id sequence with allocation-aware growth.
pub(crate) struct FallibleIdSet(OrderedSet<String>);
impl std::fmt::Debug for FallibleIdSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}
impl PartialEq for FallibleIdSet {
    fn eq(&self, other: &Self) -> bool { self.0.iter().eq(other.0.iter()) }
}
impl Eq for FallibleIdSet {}
impl FallibleIdSet {
    fn new() -> Self { Self(OrderedSet::new()) }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &String> { self.0.iter() }
    fn insert(&mut self, value: String, cx: &mut AllocationContext) -> Result<(), ValueDecodeError> {
        if !self.0.contains(value.as_str()) && self.0.len() == MAX_ITEMS {
            return Err(ValueDecodeError::LimitExceeded { resource: "scope operation count", limit: MAX_ITEMS });
        }
        self.0.try_insert(value, cx)?;
        Ok(())
    }
}
impl Serialize for FallibleIdSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.iter() { seq.serialize_element(value)?; }
        seq.end()
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedScopeRequest {
    pub(crate) population: ScopePopulationDto,
    pub(crate) operations: FallibleIdSet,
    pub(crate) excluded_records: Vec<RecordRef>,
    pub(crate) lifecycles: Vec<LifecycleBindingDto>,
    pub(crate) patches: Vec<EffectPatchRequestDto>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScopePopulationDto {
    All,
    FutureWork,
    SelectedUnfinished { records: Vec<RecordRef> },
}
impl Serialize for ScopePopulationDto {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let (tag, records) = match self {
            Self::All => ("all", None), Self::FutureWork => ("future_work", None),
            Self::SelectedUnfinished { records } => ("selected_unfinished", Some(records)),
        };
        let mut map = serializer.serialize_map(Some(1 + usize::from(records.is_some())))?;
        map.serialize_entry("kind", tag)?;
        if let Some(records) = records { map.serialize_entry("records", records)?; }
        map.end()
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct LifecycleBindingDto { entity: String, completed: Expr, source: Digest }
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct EffectPatchRequestDto { destination: EffectDestinationDto, entity: String, subject: String, value_type: Type }
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EffectDestinationDto {
    Update { action: String, path: Vec<usize>, field: String },
    CreateValue { action: String, path: Vec<usize>, field: String },
    EmitColumn { action: String, path: Vec<usize>, column: String },
    ViewColumn { view: String, column: String },
    EmitItems { action: String, path: Vec<usize> },
    Observable { observable: String },
}
impl Serialize for EffectDestinationDto {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Update { action, path, field } => { let mut m = serializer.serialize_map(Some(4))?; m.serialize_entry("kind", "update")?; m.serialize_entry("action", action)?; m.serialize_entry("path", path)?; m.serialize_entry("field", field)?; m.end() }
            Self::CreateValue { action, path, field } => { let mut m = serializer.serialize_map(Some(4))?; m.serialize_entry("kind", "create_value")?; m.serialize_entry("action", action)?; m.serialize_entry("path", path)?; m.serialize_entry("field", field)?; m.end() }
            Self::EmitColumn { action, path, column } => { let mut m = serializer.serialize_map(Some(4))?; m.serialize_entry("kind", "emit_column")?; m.serialize_entry("action", action)?; m.serialize_entry("path", path)?; m.serialize_entry("column", column)?; m.end() }
            Self::ViewColumn { view, column } => { let mut m = serializer.serialize_map(Some(3))?; m.serialize_entry("kind", "view_column")?; m.serialize_entry("view", view)?; m.serialize_entry("column", column)?; m.end() }
            Self::EmitItems { action, path } => { let mut m = serializer.serialize_map(Some(3))?; m.serialize_entry("kind", "emit_items")?; m.serialize_entry("action", action)?; m.serialize_entry("path", path)?; m.end() }
            Self::Observable { observable } => { let mut m = serializer.serialize_map(Some(2))?; m.serialize_entry("kind", "observable")?; m.serialize_entry("observable", observable)?; m.end() }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedFrozenBasis {
    snapshot: Digest, revision: u64, active: Digest, data: FallibleDecodedDataSnapshot,
    session: FallibleDecodedSessionState, day: i32, decisions: Digest,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedEffectPatch { request: EffectPatchRequestDto, before: Expr, after: Expr }
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedScopeLayer {
    version: u32, operation: String, basis: DecodedFrozenBasis, candidate: Digest,
    request: DecodedScopeRequest, patches: Vec<DecodedEffectPatch>,
}

pub(crate) fn decode_scope_population(value: Value, cx: &mut AllocationContext) -> Result<ScopePopulationDto, ValueDecodeError> {
    let (tag, fields) = tagged(value, POPULATION_VARIANTS)?;
    match tag.as_str() {
        "all" => unit_variant(fields, &[]).map(|()| ScopePopulationDto::All),
        "future_work" => unit_variant(fields, &[]).map(|()| ScopePopulationDto::FutureWork),
        "selected_unfinished" => {
            let fields = struct_fields(Value::Object(fields), "SelectedUnfinished", SELECTED_FIELDS, cx)?;
            let mut records = None;
            for (key, value) in fields.into_iter() {
                if key == "records" { records = Some(record_refs(value, cx, "selected scope record count")?); }
                else { return Err(unknown(key, SELECTED_FIELDS)); }
            }
            Ok(ScopePopulationDto::SelectedUnfinished { records: required(records, "records")? })
        }
        _ => unreachable!(),
    }
}

pub(crate) fn decode_lifecycle_binding(value: Value, cx: &mut AllocationContext) -> Result<LifecycleBindingDto, ValueDecodeError> {
    let fields = struct_fields(value, "a struct LifecycleBinding", LIFECYCLE_FIELDS, cx)?;
    let (mut entity, mut completed, mut source) = (None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "entity" => entity = Some(string(value, "a string")?),
        "completed" => completed = Some(decode_expr(value, cx)?),
        "source" => source = Some(digest(value)?),
        _ => return Err(unknown(key, LIFECYCLE_FIELDS)),
    }}
    Ok(LifecycleBindingDto { entity: required(entity, "entity")?, completed: required(completed, "completed")?, source: required(source, "source")? })
}

pub(crate) fn decode_effect_destination(value: Value, cx: &mut AllocationContext) -> Result<EffectDestinationDto, ValueDecodeError> {
    let (tag, fields) = tagged(value, DESTINATION_VARIANTS)?;
    let names = match tag.as_str() {
        "update" | "create_value" => UPDATE_FIELDS, "emit_column" => EMIT_COLUMN_FIELDS,
        "view_column" => VIEW_COLUMN_FIELDS, "emit_items" => EMIT_ITEMS_FIELDS,
        "observable" => OBSERVABLE_FIELDS, _ => unreachable!(),
    };
    let fields = struct_fields(Value::Object(fields), "an internally tagged EffectDestination", names, cx)?;
    let (mut action, mut path, mut field, mut column, mut view, mut observable) = (None, None, None, None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "action" => action = Some(string(value, "a string")?),
        "path" => path = Some(path_indices(value, cx)?),
        "field" => field = Some(string(value, "a string")?),
        "column" => column = Some(string(value, "a string")?),
        "view" => view = Some(string(value, "a string")?),
        "observable" => observable = Some(string(value, "a string")?),
        _ => return Err(unknown(key, names)),
    }}
    let out = match tag.as_str() {
        "update" => EffectDestinationDto::Update { action: required(action, "action")?, path: required(path, "path")?, field: required(field, "field")? },
        "create_value" => EffectDestinationDto::CreateValue { action: required(action, "action")?, path: required(path, "path")?, field: required(field, "field")? },
        "emit_column" => EffectDestinationDto::EmitColumn { action: required(action, "action")?, path: required(path, "path")?, column: required(column, "column")? },
        "view_column" => EffectDestinationDto::ViewColumn { view: required(view, "view")?, column: required(column, "column")? },
        "emit_items" => EffectDestinationDto::EmitItems { action: required(action, "action")?, path: required(path, "path")? },
        "observable" => EffectDestinationDto::Observable { observable: required(observable, "observable")? }, _ => unreachable!(),
    };
    Ok(out)
}

pub(crate) fn decode_effect_patch_request(value: Value, cx: &mut AllocationContext) -> Result<EffectPatchRequestDto, ValueDecodeError> {
    let fields = struct_fields(value, "a struct EffectPatchRequest", PATCH_REQUEST_FIELDS, cx)?;
    let (mut destination, mut entity, mut subject, mut value_type) = (None, None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "destination" => destination = Some(decode_effect_destination(value, cx)?),
        "entity" => entity = Some(string(value, "a string")?), "subject" => subject = Some(string(value, "a string")?),
        "value_type" => value_type = Some(decode_type(value, cx)?), _ => return Err(unknown(key, PATCH_REQUEST_FIELDS)),
    }}
    Ok(EffectPatchRequestDto { destination: required(destination, "destination")?, entity: required(entity, "entity")?, subject: required(subject, "subject")?, value_type: required(value_type, "value_type")? })
}

pub(crate) fn decode_scope_request(value: Value, cx: &mut AllocationContext) -> Result<DecodedScopeRequest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ScopeRequest", REQUEST_FIELDS, cx)?;
    let (mut population, mut operations, mut excluded, mut lifecycles, mut patches) = (None, None, None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "population" => population = Some(decode_scope_population(value, cx)?),
        "operations" => operations = Some(operation_set(value, cx)?),
        "excluded_records" => excluded = Some(record_refs(value, cx, "excluded scope record count")?),
        "lifecycles" => lifecycles = Some(lifecycle_bindings(value, cx)?),
        "patches" => patches = Some(patch_requests(value, cx)?), _ => return Err(unknown(key, REQUEST_FIELDS)),
    }}
    Ok(DecodedScopeRequest { population: required(population, "population")?, operations: required(operations, "operations")?, excluded_records: required(excluded, "excluded_records")?, lifecycles: required(lifecycles, "lifecycles")?, patches: required(patches, "patches")? })
}

pub(crate) fn decode_frozen_basis(value: Value, cx: &mut AllocationContext) -> Result<DecodedFrozenBasis, ValueDecodeError> {
    let fields = struct_fields(value, "a struct FrozenBasis", BASIS_FIELDS, cx)?;
    let (mut snapshot, mut revision, mut active, mut data, mut session, mut day, mut decisions) = (None, None, None, None, None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "snapshot" => snapshot = Some(digest(value)?), "revision" => revision = Some(u64_value(value)?),
        "active" => active = Some(digest(value)?), "data" => data = Some(decode_data_snapshot_fallible(value, cx)?),
        "session" => session = Some(decode_session_state_fallible(value, cx)?), "day" => day = Some(i32_value(value)?),
        "decisions" => decisions = Some(digest(value)?), _ => return Err(unknown(key, BASIS_FIELDS)),
    }}
    Ok(DecodedFrozenBasis { snapshot: required(snapshot, "snapshot")?, revision: required(revision, "revision")?, active: required(active, "active")?, data: required(data, "data")?, session: required(session, "session")?, day: required(day, "day")?, decisions: required(decisions, "decisions")? })
}

pub(crate) fn decode_effect_patch(value: Value, cx: &mut AllocationContext) -> Result<DecodedEffectPatch, ValueDecodeError> {
    let fields = struct_fields(value, "a struct EffectPatch", PATCH_FIELDS, cx)?;
    let (mut request, mut before, mut after) = (None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "request" => request = Some(decode_effect_patch_request(value, cx)?), "before" => before = Some(decode_expr(value, cx)?),
        "after" => after = Some(decode_expr(value, cx)?), _ => return Err(unknown(key, PATCH_FIELDS)),
    }}
    Ok(DecodedEffectPatch { request: required(request, "request")?, before: required(before, "before")?, after: required(after, "after")? })
}

pub(crate) fn decode_scope_layer(value: Value, cx: &mut AllocationContext) -> Result<DecodedScopeLayer, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ScopeLayer", LAYER_FIELDS, cx)?;
    let (mut version, mut operation, mut basis, mut candidate, mut request, mut patches) = (None, None, None, None, None, None);
    for (key, value) in fields.into_iter() { match key.as_str() {
        "version" => version = Some(u32_value(value)?), "operation" => operation = Some(string(value, "a string")?),
        "basis" => basis = Some(decode_frozen_basis(value, cx)?), "candidate" => candidate = Some(digest(value)?),
        "request" => request = Some(decode_scope_request(value, cx)?), "patches" => patches = Some(effect_patches(value, cx)?),
        _ => return Err(unknown(key, LAYER_FIELDS)),
    }}
    Ok(DecodedScopeLayer { version: required(version, "version")?, operation: required(operation, "operation")?, basis: required(basis, "basis")?, candidate: required(candidate, "candidate")?, request: required(request, "request")?, patches: required(patches, "patches")? })
}

fn lifecycle_bindings(value: Value, cx: &mut AllocationContext) -> Result<Vec<LifecycleBindingDto>, ValueDecodeError> {
    decode_vec(value, MAX_ITEMS, "scope lifecycle count", cx, decode_lifecycle_binding)
}
fn patch_requests(value: Value, cx: &mut AllocationContext) -> Result<Vec<EffectPatchRequestDto>, ValueDecodeError> {
    decode_vec(value, MAX_ITEMS, "scope patch request count", cx, decode_effect_patch_request)
}
fn effect_patches(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedEffectPatch>, ValueDecodeError> {
    decode_vec(value, MAX_ITEMS, "scope effect patch count", cx, decode_effect_patch)
}
fn record_refs(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<RecordRef>, ValueDecodeError> {
    decode_vec(value, MAX_COLLECTION, resource, cx, decode_record_ref_fallible)
}
fn decode_vec<T>(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext, mut f: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Vec<T>, ValueDecodeError> {
    let values = array(value)?; bounded(values.len(), limit, resource)?;
    let mut out = Vec::new(); product_alloc::try_reserve(&mut out, values.len(), cx, AllocationSite::Vector)?;
    for value in values { out.push(f(value, cx)?); } Ok(out)
}
fn operation_set(value: Value, cx: &mut AllocationContext) -> Result<FallibleIdSet, ValueDecodeError> {
    let values = array(value)?; let mut out = FallibleIdSet::new();
    for value in values { out.insert(string(value, "a string")?, cx)?; } Ok(out)
}
fn path_indices(value: Value, cx: &mut AllocationContext) -> Result<Vec<usize>, ValueDecodeError> {
    let values = array(value)?; bounded(values.len(), MAX_EXPR_DEPTH, "effect destination path length")?;
    let mut out = Vec::new(); product_alloc::try_reserve(&mut out, values.len(), cx, AllocationSite::Vector)?;
    for value in values { out.push(usize_value(value)?); } Ok(out)
}

fn tagged(value: Value, variants: &'static [&'static str]) -> Result<(String, crate::product_alloc::ordered::OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = match value { Value::Object(map) => map, value => return Err(invalid_type(value, "an internally tagged enum")) };
    let tag = match fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))? { Value::String(tag) => tag, value => return Err(invalid_type(value, "a string variant identifier")) };
    if variants.contains(&tag.as_str()) { Ok((tag, fields)) } else { Err(ValueDecodeError::UnknownVariant { tag, expected: variants }) }
}
fn unit_variant(fields: crate::product_alloc::ordered::OrderedMap<String, Value>, expected: &'static [&'static str]) -> Result<(), ValueDecodeError> {
    if let Some((key, _)) = fields.into_entries().next() { Err(unknown(key, expected)) } else { Ok(()) }
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<std::vec::IntoIter<(String, Value)>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields.into_entries()),
        Value::Array(values) => {
            if values.len() != names.len() { return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() }); }
            let mut fields = Vec::new();
            product_alloc::try_reserve(&mut fields, names.len(), cx, AllocationSite::Vector)?;
            for (name, value) in names.iter().zip(values.into_iter().take(names.len())) { fields.push((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value)); }
            Ok(fields.into_iter())
        }
        value => Err(invalid_type(value, expected)),
    }
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> { match value { Value::String(v) => Ok(v), value => Err(invalid_type(value, expected)) } }
fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    Digest::from_owned_lower_hex(string(value, "a string")?)
        .ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}
fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> { match value { Value::Array(v) => Ok(v), value => Err(invalid_type(value, "a sequence")) } }
fn u32_value(value: Value) -> Result<u32, ValueDecodeError> { match value { Value::Number(Number::U64(v)) => u32::try_from(v).map_err(|_| invalid_value(NumericValue::Unsigned(v), "u32")), Value::Number(Number::I64(v)) => u32::try_from(v).map_err(|_| invalid_value(NumericValue::Signed(v), "u32")), value => Err(invalid_type(value, "u32")) } }
fn u64_value(value: Value) -> Result<u64, ValueDecodeError> { match value { Value::Number(Number::U64(v)) => Ok(v), Value::Number(Number::I64(v)) => u64::try_from(v).map_err(|_| invalid_value(NumericValue::Signed(v), "u64")), value => Err(invalid_type(value, "u64")) } }
fn usize_value(value: Value) -> Result<usize, ValueDecodeError> { match value { Value::Number(Number::U64(v)) => usize::try_from(v).map_err(|_| invalid_value(NumericValue::Unsigned(v), "usize")), Value::Number(Number::I64(v)) => usize::try_from(v).map_err(|_| invalid_value(NumericValue::Signed(v), "usize")), value => Err(invalid_type(value, "usize")) } }
fn i32_value(value: Value) -> Result<i32, ValueDecodeError> { match value { Value::Number(Number::U64(v)) => i32::try_from(v).map_err(|_| invalid_value(NumericValue::Unsigned(v), "i32")), Value::Number(Number::I64(v)) => i32::try_from(v).map_err(|_| invalid_value(NumericValue::Signed(v), "i32")), value => Err(invalid_type(value, "i32")) } }
fn invalid_value(actual: NumericValue, expected: &'static str) -> ValueDecodeError { ValueDecodeError::InvalidValue { actual, expected } }
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError { let actual = match value { Value::Null => JsonKind::Null, Value::Bool(_) => JsonKind::Boolean, Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger, Value::Number(Number::I64(_)) => JsonKind::SignedInteger, Value::Number(Number::F64(_)) => JsonKind::FloatingPoint, Value::String(_) => JsonKind::String, Value::Array(_) => JsonKind::Sequence, Value::Object(_) => JsonKind::Map }; ValueDecodeError::InvalidType { actual, expected } }
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn bounded(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> { if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_contract::canonical_bytes;
    use super::super::contract::{EffectDestination, EffectPatch as ModelEffectPatch, EffectPatchRequest, LifecycleBinding as ModelLifecycleBinding, ScopeLayer as ModelScopeLayer, ScopePopulation as ModelPopulation, ScopeRequest as ModelScopeRequest};
    use crate::product_json;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn destination_and_population_variants_match_serde_canonical_bytes() {
        let destinations = [
            r#"{"kind":"update","action":"a","path":[1,2],"field":"f"}"#,
            r#"{"kind":"create_value","action":"a","path":[1],"field":"f"}"#,
            r#"{"kind":"emit_column","action":"a","path":[0],"column":"c"}"#,
            r#"{"kind":"view_column","view":"v","column":"c"}"#,
            r#"{"kind":"emit_items","action":"a","path":[3]}"#,
            r#"{"kind":"observable","observable":"o"}"#,
        ];
        for source in destinations {
            let expected: EffectDestination = serde_json::from_str(source).unwrap();
            let actual = decode_effect_destination(syntax(source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&actual));
        }
        for source in [
            r#"{"kind":"all"}"#,
            r#"{"kind":"future_work"}"#,
            r#"{"kind":"selected_unfinished","records":[{"entity":"e","record":"r"}]}"#,
        ] {
            let expected: ModelPopulation = serde_json::from_str(source).unwrap();
            let actual = decode_scope_population(syntax(source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&actual));
        }
    }

    #[test]
    fn request_and_full_layer_preserve_sorted_set_and_wire_bytes() {
        let request_source = r#"{"population":{"kind":"selected_unfinished","records":[{"entity":"e","record":"r"}]},"operations":["z","a","z","b"],"excluded_records":[],"lifecycles":[],"patches":[]}"#;
        let expected: ModelScopeRequest = serde_json::from_str(request_source).unwrap();
        let actual = decode_scope_request(syntax(request_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(actual.operations.iter().map(String::as_str).collect::<Vec<_>>(), ["a", "b", "z"]);
        assert_eq!(canonical(&expected), canonical(&actual));

        let d = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let layer_source = format!(r#"{{"version":1,"operation":"op","basis":{{"snapshot":"{d}","revision":4,"active":"{d}","data":{{"version":1,"project_id":"p","generation":2,"schema":[],"records":[{{"entity":"person","id":"r1","revision":1,"created_program":"{d}","archived":false,"values":{{"name":{{"kind":"text","value":"Ada"}}}}}}],"events":[]}},"session":{{"view":"v","values":{{"cursor":{{"kind":"integer","value":1}}}},"focused_record":null}},"day":-3,"decisions":"{d}"}},"candidate":"{d}","request":{{"population":{{"kind":"all"}},"operations":["b","a","b"],"excluded_records":[],"lifecycles":[],"patches":[]}},"patches":[]}}"#);
        let expected: ModelScopeLayer = serde_json::from_str(&layer_source).unwrap();
        let actual = decode_scope_layer(syntax(&layer_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));

        let sequence = r#"[1,"op",["0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",4,"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",[1,"p",2,[],[["person","r1",1,"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",false,{"name":{"kind":"text","value":"Ada"}}]],[]],["v",{},null],-3,"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"],"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",[{"kind":"all"},["b","a"],[],[],[]],[]]"#;
        let expected_seq: ModelScopeLayer = serde_json::from_str(sequence).unwrap();
        let actual_seq = decode_scope_layer(syntax(sequence), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_seq), canonical(&actual_seq));
        let sequence_with_trailing = format!("{},{{\"ignored\":[1,2,3]}}]", sequence.strip_suffix(']').unwrap());
        assert!(serde_json::from_value::<ModelScopeLayer>(serde_json::from_str(&sequence_with_trailing).unwrap()).is_err());
        assert!(matches!(decode_scope_layer(syntax(&sequence_with_trailing), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 7, expected: 6 })));
    }

    #[test]
    fn lifecycle_and_patch_request_graphs_match_serde_bytes() {
        let d = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let lifecycle_source = format!(r#"{{"entity":"person","completed":{{"kind":"equal","left":{{"kind":"today"}},"right":{{"kind":"today"}}}},"source":"{d}"}}"#);
        let expected: ModelLifecycleBinding = serde_json::from_str(&lifecycle_source).unwrap();
        let actual = decode_lifecycle_binding(syntax(&lifecycle_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));

        let request_source = r#"{"destination":{"kind":"update","action":"save","path":[0,2],"field":"name"},"entity":"person","subject":"record","value_type":{"kind":"optional","item":{"kind":"list","item":{"kind":"text"}}}}"#;
        let expected: EffectPatchRequest = serde_json::from_str(request_source).unwrap();
        let actual = decode_effect_patch_request(syntax(request_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));

        let patch_source = format!(r#"{{"request":{{"destination":{{"kind":"observable","observable":"today"}},"entity":"person","subject":"record","value_type":{{"kind":"date"}}}},"before":{{"kind":"today"}},"after":{{"kind":"today"}}}}"#);
        let expected: ModelEffectPatch = serde_json::from_str(&patch_source).unwrap();
        let actual = decode_effect_patch(syntax(&patch_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));
    }

    #[test]
    fn malformed_unknown_ranges_and_collection_limits_are_rejected() {
        assert!(matches!(decode_scope_population(syntax(r#"{"kind":"future"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_effect_destination(syntax(r#"{"kind":"update","action":"a","path":[],"field":"f","x":1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_scope_request(syntax(r#"{"population":{"kind":"all"},"operations":[],"excluded_records":[],"lifecycles":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("patches"))));
        assert!(matches!(decode_scope_layer(syntax(r#"[4294967296,"op",null,null,null,null]"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { .. })));
        assert!(matches!(decode_lifecycle_binding(syntax(r#"{"entity":"e","completed":{"kind":"today"},"source":"x"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));
        let malformed_basis = r#"{"snapshot":"x","revision":0,"active":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","data":{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]},"session":{"view":"v","values":{},"focused_record":null},"day":0,"decisions":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#;
        assert!(matches!(decode_frozen_basis(syntax(malformed_basis), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));
        let too_long_path = format!(r#"{{"kind":"emit_items","action":"a","path":[{}]}}"#, std::iter::repeat("0").take(MAX_EXPR_DEPTH + 1).collect::<Vec<_>>().join(","));
        assert!(matches!(decode_effect_destination(syntax(&too_long_path), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "effect destination path length", .. })));
        let too_many_lifecycles = format!(r#"{{"population":{{"kind":"all"}},"operations":[],"excluded_records":[],"lifecycles":[{}],"patches":[]}}"#, std::iter::repeat("null").take(MAX_ITEMS + 1).collect::<Vec<_>>().join(","));
        assert!(matches!(decode_scope_request(syntax(&too_many_lifecycles), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "scope lifecycle count", .. })));
        let too_many_excluded = format!(r#"{{"population":{{"kind":"all"}},"operations":[],"excluded_records":[{}],"lifecycles":[],"patches":[]}}"#, std::iter::repeat("null").take(MAX_COLLECTION + 1).collect::<Vec<_>>().join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_scope_request(syntax(&too_many_excluded), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "excluded scope record count", .. })));
        assert_eq!(cx.growth_requests(), 0, "the excluded record limit is checked before reserving its owner vector");
        assert!(matches!(decode_scope_layer(syntax("[1]"), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 1, expected: 6 })));
    }

    #[test]
    fn every_reached_request_growth_refusal_is_fixed_and_retryable() {
        let source = r#"{"population":{"kind":"selected_unfinished","records":[{"entity":"e","record":"r"}]},"operations":["a","b"],"excluded_records":[{"entity":"x","record":"y"}],"lifecycles":[{"entity":"e","completed":{"kind":"today"},"source":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}],"patches":[]}"#;
        let mut count_context = AllocationContext::new();
        let count_syntax = syntax(source);
        decode_scope_request(count_syntax, &mut count_context).unwrap();
        let growths = count_context.growth_requests();
        assert!(growths > 0);
        for n in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true);
            let result = decode_scope_request(syntax(source), &mut cx);
            assert!(matches!(result, Err(ValueDecodeError::Allocation(_))), "growth {n}/{growths}: {result:?}");
            assert!(decode_scope_request(syntax(source), &mut AllocationContext::new()).is_ok());
        }
    }

    #[test]
    fn direct_scope_decoders_fail_safely_at_each_reached_growth() {
        let cases: [(&str, fn(Value, &mut AllocationContext) -> Result<(), ValueDecodeError>); 9] = [
            (r#"{"kind":"selected_unfinished","records":[{"entity":"e","record":"r"}]}"#, |v, c| decode_scope_population(v, c).map(|_| ())),
            (r#"{"entity":"e","completed":{"kind":"today"},"source":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#, |v, c| decode_lifecycle_binding(v, c).map(|_| ())),
            (r#"{"kind":"update","action":"a","path":[0,1],"field":"f"}"#, |v, c| decode_effect_destination(v, c).map(|_| ())),
            (r#"{"destination":{"kind":"view_column","view":"v","column":"c"},"entity":"e","subject":"s","value_type":{"kind":"text"}}"#, |v, c| decode_effect_patch_request(v, c).map(|_| ())),
            (r#"{"population":{"kind":"all"},"operations":["a","b"],"excluded_records":[],"lifecycles":[],"patches":[]}"#, |v, c| decode_scope_request(v, c).map(|_| ())),
            (r#"[{"kind":"all"},["b","a"],[],[],[]]"#, |v, c| decode_scope_request(v, c).map(|_| ())),
            (r#"{"snapshot":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","revision":0,"active":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","data":{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]},"session":{"view":"v","values":{},"focused_record":null},"day":0,"decisions":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#, |v, c| decode_frozen_basis(v, c).map(|_| ())),
            (r#"{"request":{"destination":{"kind":"observable","observable":"o"},"entity":"e","subject":"s","value_type":{"kind":"text"}},"before":{"kind":"today"},"after":{"kind":"today"}}"#, |v, c| decode_effect_patch(v, c).map(|_| ())),
            (r#"{"version":1,"operation":"op","basis":{"snapshot":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","revision":0,"active":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","data":{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]},"session":{"view":"v","values":{},"focused_record":null},"day":0,"decisions":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"},"candidate":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","request":{"population":{"kind":"all"},"operations":["a"],"excluded_records":[],"lifecycles":[],"patches":[]},"patches":[]}"#, |v, c| decode_scope_layer(v, c).map(|_| ())),
        ];
        for (source, decode) in cases {
            let mut count = AllocationContext::new();
            decode(syntax(source), &mut count).unwrap();
            let growths = count.growth_requests();
            if growths == 0 { continue; }
            for n in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true);
                assert!(matches!(decode(syntax(source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {n}/{growths} in {source}");
                decode(syntax(source), &mut AllocationContext::new()).unwrap();
            }
        }
    }
}
