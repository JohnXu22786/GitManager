//! Private, bounded structural decoding for the scalar scope request/layer graph.
//!
//! These DTOs are intentionally not connected to production persistence yet.
//! They compose the existing Product Contract fallible decoders and retain
//! ordered fallible ownership where the persisted model uses BTreeSet.
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationSite};
use crate::product_contract::fallible_expr_decode::decode_expr;
use crate::product_contract::fallible_value_decode::{decode_type, decode_values, JsonKind, NumericValue, ValueDecodeError};
use crate::product_contract::{decode_data_snapshot_fallible, decode_record_ref_fallible, decode_session_state_fallible, Digest, Expr, FallibleDecodedDataSnapshot, FallibleDecodedSessionState, FallibleValues, RecordRef, Type, MAX_COLLECTION, MAX_EXPR_DEPTH, MAX_ITEMS};
use super::contract::ScopeTransition;
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
const TRANSITIONS: &[&str] = &["adoption", "withdrawal", "evolution"];
const SLOT_MAPPING_FIELDS: &[&str] = &["layer", "patch", "from_source", "from", "to_source", "to", "subject"];
const COMPOSITION_FIELDS: &[&str] = &["version", "basis", "transition", "rewrites", "business", "layers", "active", "previous", "operation", "runtime", "driver", "output"];
const ADDITION_FIELDS: &[&str] = &["record", "values"];
const INITIALIZATION_FIELDS: &[&str] = &["operation", "layer", "before_data", "after_data", "before_schema", "after_schema", "additions", "business_projection", "original_events", "original_records", "capture_day", "capture_program", "prior_snapshot", "adoption_revision"];
const MAX_SCOPE_REWRITES: usize = super::contract::MAX_LAYERS * MAX_ITEMS;

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

pub(crate) struct FallibleDigestSet(OrderedSet<Digest>);
impl std::fmt::Debug for FallibleDigestSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}
impl FallibleDigestSet {
    fn new() -> Self { Self(OrderedSet::new()) }
    fn insert(&mut self, value: Digest, limit: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<(), ValueDecodeError> {
        if !self.0.contains(&value) && self.0.len() == limit {
            return Err(ValueDecodeError::LimitExceeded { resource, limit });
        }
        self.0.try_insert(value, cx)?;
        Ok(())
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Digest> { self.0.iter() }
}
impl PartialEq for FallibleDigestSet {
    fn eq(&self, other: &Self) -> bool { self.0.iter().eq(other.0.iter()) }
}
impl Eq for FallibleDigestSet {}
impl Serialize for FallibleDigestSet {
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
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedScopeSlotMapping {
    layer: Digest, patch: usize, from_source: Digest, from: EffectDestinationDto,
    to_source: Digest, to: EffectDestinationDto, subject: String,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedCompositionManifest {
    version: u32, basis: DecodedFrozenBasis, transition: ScopeTransition,
    rewrites: Vec<DecodedScopeSlotMapping>, business: Digest, layers: Vec<Digest>,
    active: FallibleDigestSet, previous: Option<Digest>, operation: String,
    runtime: String, driver: String, output: Digest,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedMetadataAddition { record: RecordRef, values: FallibleValues }
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedMetadataInitializationReceipt {
    operation: String, layer: Digest, before_data: Digest, after_data: Digest,
    before_schema: Digest, after_schema: Digest, additions: Vec<DecodedMetadataAddition>,
    business_projection: Digest, original_events: Digest, original_records: usize,
    capture_day: i32, capture_program: Digest, prior_snapshot: Digest, adoption_revision: u64,
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

pub(crate) fn decode_scope_transition(value: Value) -> Result<ScopeTransition, ValueDecodeError> {
    let name = string(value, "a string variant identifier")?;
    match name.as_str() {
        "adoption" => Ok(ScopeTransition::Adoption),
        "withdrawal" => Ok(ScopeTransition::Withdrawal),
        "evolution" => Ok(ScopeTransition::Evolution),
        _ => Err(ValueDecodeError::UnknownVariant { tag: name, expected: TRANSITIONS }),
    }
}

pub(crate) fn decode_scope_slot_mapping(value: Value, cx: &mut AllocationContext) -> Result<DecodedScopeSlotMapping, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ScopeSlotMapping", SLOT_MAPPING_FIELDS, cx)?;
    let (mut layer, mut patch, mut from_source, mut from, mut to_source, mut to, mut subject) = (None, None, None, None, None, None, None);
    for (key, value) in fields { match key.as_str() {
        "layer" => layer = Some(digest(value)?), "patch" => patch = Some(usize_value(value)?),
        "from_source" => from_source = Some(digest(value)?), "from" => from = Some(decode_effect_destination(value, cx)?),
        "to_source" => to_source = Some(digest(value)?), "to" => to = Some(decode_effect_destination(value, cx)?),
        "subject" => subject = Some(string(value, "a string")?), _ => return Err(unknown(key, SLOT_MAPPING_FIELDS)),
    }}
    Ok(DecodedScopeSlotMapping { layer: required(layer, "layer")?, patch: required(patch, "patch")?, from_source: required(from_source, "from_source")?, from: required(from, "from")?, to_source: required(to_source, "to_source")?, to: required(to, "to")?, subject: required(subject, "subject")? })
}

pub(crate) fn decode_composition_manifest(value: Value, cx: &mut AllocationContext) -> Result<DecodedCompositionManifest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CompositionManifest", COMPOSITION_FIELDS, cx)?;
    let (mut version, mut basis, mut transition, mut rewrites, mut business, mut layers, mut active, mut previous, mut operation, mut runtime, mut driver, mut output) = (None, None, None, None, None, None, None, None, None, None, None, None);
    for (key, value) in fields { match key.as_str() {
        "version" => version = Some(u32_value(value)?), "basis" => basis = Some(decode_frozen_basis(value, cx)?),
        "transition" => transition = Some(decode_scope_transition(value)?),
        "rewrites" => rewrites = Some(slot_mappings(value, cx)?), "business" => business = Some(digest(value)?),
        "layers" => layers = Some(digest_vec(value, super::contract::MAX_LAYERS, "composition layer count", cx)?),
        "active" => active = Some(digest_set(value, super::contract::MAX_LAYERS, "active composition layer count", cx)?),
        "previous" => previous = Some(optional_digest(value)?), "operation" => operation = Some(string(value, "a string")?),
        "runtime" => runtime = Some(string(value, "a string")?), "driver" => driver = Some(string(value, "a string")?),
        "output" => output = Some(digest(value)?), _ => return Err(unknown(key, COMPOSITION_FIELDS)),
    }}
    Ok(DecodedCompositionManifest { version: required(version, "version")?, basis: required(basis, "basis")?, transition: required(transition, "transition")?, rewrites: required(rewrites, "rewrites")?, business: required(business, "business")?, layers: required(layers, "layers")?, active: required(active, "active")?, previous: previous.unwrap_or(None), operation: required(operation, "operation")?, runtime: required(runtime, "runtime")?, driver: required(driver, "driver")?, output: required(output, "output")? })
}

pub(crate) fn decode_metadata_addition(value: Value, cx: &mut AllocationContext) -> Result<DecodedMetadataAddition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct MetadataAddition", ADDITION_FIELDS, cx)?;
    let (mut record, mut values) = (None, None);
    for (key, value) in fields { match key.as_str() {
        "record" => record = Some(decode_record_ref_fallible(value, cx)?),
        "values" => values = Some(decode_values(value, cx)?), _ => return Err(unknown(key, ADDITION_FIELDS)),
    }}
    Ok(DecodedMetadataAddition { record: required(record, "record")?, values: required(values, "values")? })
}

pub(crate) fn decode_metadata_initialization_receipt(value: Value, cx: &mut AllocationContext) -> Result<DecodedMetadataInitializationReceipt, ValueDecodeError> {
    let fields = struct_fields(value, "a struct MetadataInitializationReceipt", INITIALIZATION_FIELDS, cx)?;
    let (mut operation, mut layer, mut before_data, mut after_data, mut before_schema, mut after_schema, mut additions, mut business_projection, mut original_events, mut original_records, mut capture_day, mut capture_program, mut prior_snapshot, mut adoption_revision) = (None, None, None, None, None, None, None, None, None, None, None, None, None, None);
    for (key, value) in fields { match key.as_str() {
        "operation" => operation = Some(string(value, "a string")?), "layer" => layer = Some(digest(value)?),
        "before_data" => before_data = Some(digest(value)?), "after_data" => after_data = Some(digest(value)?),
        "before_schema" => before_schema = Some(digest(value)?), "after_schema" => after_schema = Some(digest(value)?),
        "additions" => additions = Some(metadata_additions(value, cx)?),
        "business_projection" => business_projection = Some(digest(value)?), "original_events" => original_events = Some(digest(value)?),
        "original_records" => original_records = Some(usize_value(value)?), "capture_day" => capture_day = Some(i32_value(value)?),
        "capture_program" => capture_program = Some(digest(value)?), "prior_snapshot" => prior_snapshot = Some(digest(value)?),
        "adoption_revision" => adoption_revision = Some(u64_value(value)?), _ => return Err(unknown(key, INITIALIZATION_FIELDS)),
    }}
    let original_records = required(original_records, "original_records")?;
    bounded(original_records, MAX_COLLECTION, "initialization original record count")?;
    Ok(DecodedMetadataInitializationReceipt { operation: required(operation, "operation")?, layer: required(layer, "layer")?, before_data: required(before_data, "before_data")?, after_data: required(after_data, "after_data")?, before_schema: required(before_schema, "before_schema")?, after_schema: required(after_schema, "after_schema")?, additions: required(additions, "additions")?, business_projection: required(business_projection, "business_projection")?, original_events: required(original_events, "original_events")?, original_records, capture_day: required(capture_day, "capture_day")?, capture_program: required(capture_program, "capture_program")?, prior_snapshot: required(prior_snapshot, "prior_snapshot")?, adoption_revision: required(adoption_revision, "adoption_revision")? })
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
fn slot_mappings(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedScopeSlotMapping>, ValueDecodeError> {
    decode_vec(value, MAX_SCOPE_REWRITES, "scope slot mapping count", cx, decode_scope_slot_mapping)
}
fn metadata_additions(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedMetadataAddition>, ValueDecodeError> {
    decode_vec(value, MAX_COLLECTION, "metadata addition count", cx, decode_metadata_addition)
}
fn digest_vec(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<Digest>, ValueDecodeError> {
    decode_vec(value, limit, resource, cx, |value, _| digest(value))
}
fn digest_set(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<FallibleDigestSet, ValueDecodeError> {
    let values = array(value)?; let mut set = FallibleDigestSet::new();
    for value in values { set.insert(digest(value)?, limit, resource, cx)?; }
    Ok(set)
}
fn optional_digest(value: Value) -> Result<Option<Digest>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => digest(value).map(Some) }
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
    use super::super::contract::{CompositionManifest as ModelCompositionManifest, EffectDestination, EffectPatch as ModelEffectPatch, EffectPatchRequest, LifecycleBinding as ModelLifecycleBinding, MetadataAddition as ModelMetadataAddition, MetadataInitializationReceipt as ModelMetadataInitializationReceipt, ScopeLayer as ModelScopeLayer, ScopePopulation as ModelPopulation, ScopeRequest as ModelScopeRequest, ScopeSlotMapping as ModelScopeSlotMapping, ScopeTransition};
    use crate::product_json;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }

    fn digest_text() -> &'static str { "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" }

    fn basis_text(d: &str) -> String {
        format!(r#"{{"snapshot":"{d}","revision":4,"active":"{d}","data":{{"version":1,"project_id":"project","generation":2,"schema":[],"records":[],"events":[]}},"session":{{"view":"main","values":{{}},"focused_record":null}},"day":-3,"decisions":"{d}"}}"#)
    }

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

    #[test]
    fn slot_mapping_and_composition_manifest_match_canonical_serde_bytes() {
        let d = digest_text();
        let mapping_source = format!(r#"{{"layer":"{d}","patch":2,"from_source":"{d}","from":{{"kind":"update","action":"save","path":[1,3],"field":"name"}},"to_source":"{d}","to":{{"kind":"view_column","view":"summary","column":"name"}},"subject":"person/r1"}}"#);
        let expected: ModelScopeSlotMapping = serde_json::from_str(&mapping_source).unwrap();
        let actual = decode_scope_slot_mapping(syntax(&mapping_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));
        let mapping_sequence = format!(r#"["{d}",2,"{d}",{{"kind":"observable","observable":"x"}},"{d}",{{"kind":"observable","observable":"y"}},"person/r1"]"#);
        let expected_sequence: ModelScopeSlotMapping = serde_json::from_str(&mapping_sequence).unwrap();
        let actual_sequence = decode_scope_slot_mapping(syntax(&mapping_sequence), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_sequence), canonical(&actual_sequence));

        for transition in [ScopeTransition::Adoption, ScopeTransition::Withdrawal, ScopeTransition::Evolution] {
            let transition_json = serde_json::to_string(&transition).unwrap();
            let source = format!(r#"{{"version":1,"basis":{},"transition":{},"rewrites":[{}],"business":"{d}","layers":["{d}","{d}"],"active":["{d}","{d}"],"previous":"{d}","operation":"op","runtime":"engine","driver":"host","output":"{d}"}}"#,
                basis_text(d), transition_json, mapping_source);
            let expected: ModelCompositionManifest = serde_json::from_str(&source).unwrap();
            let actual = decode_composition_manifest(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(actual.active.iter().count(), 1, "duplicate BTreeSet entries collapse");
            assert_eq!(canonical(&expected), canonical(&actual));
        }

        let omitted_previous = format!(r#"{{"version":1,"basis":{},"transition":"adoption","rewrites":[],"business":"{d}","layers":[],"active":[],"operation":"op","runtime":"engine","driver":"host","output":"{d}"}}"#, basis_text(d));
        let expected: ModelCompositionManifest = serde_json::from_str(&omitted_previous).unwrap();
        let actual = decode_composition_manifest(syntax(&omitted_previous), &mut AllocationContext::new()).unwrap();
        assert_eq!(expected.previous, None);
        assert_eq!(canonical(&expected), canonical(&actual));
    }

    #[test]
    fn metadata_addition_and_initialization_receipt_match_canonical_bytes() {
        let d = digest_text();
        let addition_source = format!(r#"{{"record":{{"entity":"person","record":"r1"}},"values":{{"label":{{"kind":"text","value":"Zoë"}},"active":{{"kind":"boolean","value":true}}}}}}"#);
        let expected_addition: ModelMetadataAddition = serde_json::from_str(&addition_source).unwrap();
        let actual_addition = decode_metadata_addition(syntax(&addition_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_addition), canonical(&actual_addition));
        let addition_sequence = r#"[["person","r1"],{"label":{"kind":"text","value":"Zoë"},"active":{"kind":"boolean","value":true}}]"#;
        let expected_sequence: ModelMetadataAddition = serde_json::from_str(addition_sequence).unwrap();
        let actual_sequence = decode_metadata_addition(syntax(addition_sequence), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_sequence), canonical(&actual_sequence));

        let source = format!(r#"{{"operation":"initialize","layer":"{d}","before_data":"{d}","after_data":"{d}","before_schema":"{d}","after_schema":"{d}","additions":[{addition_source}],"business_projection":"{d}","original_events":"{d}","original_records":1,"capture_day":-5,"capture_program":"{d}","prior_snapshot":"{d}","adoption_revision":9}}"#);
        let expected: ModelMetadataInitializationReceipt = serde_json::from_str(&source).unwrap();
        let actual = decode_metadata_initialization_receipt(syntax(&source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&actual));
    }

    #[test]
    fn receipt_decoders_reject_invalid_fields_ranges_and_bounds() {
        let d = digest_text();
        let source = format!(r#"{{"version":1,"basis":{},"transition":"adoption","rewrites":[],"business":"{d}","layers":[],"active":[],"operation":"op","runtime":"engine","driver":"host","output":"{d}"}}"#, basis_text(d));
        let mut unknown = serde_json::from_str::<serde_json::Value>(&source).unwrap();
        unknown.as_object_mut().unwrap().insert("z_unknown".into(), serde_json::json!(1));
        let unknown = serde_json::to_string(&unknown).unwrap();
        assert!(matches!(decode_composition_manifest(syntax(&unknown), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_scope_transition(syntax("\"new_transition\"")), Err(ValueDecodeError::UnknownVariant { .. })));
        let invalid_patch = format!(r#"{{"layer":"{d}","patch":-1,"from_source":"{d}","from":{{"kind":"observable","observable":"x"}},"to_source":"{d}","to":{{"kind":"observable","observable":"y"}},"subject":"s"}}"#);
        assert!(matches!(decode_scope_slot_mapping(syntax(&invalid_patch), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { .. })));
        let too_many_layers = format!(r#"{{"version":1,"basis":{},"transition":"adoption","rewrites":[],"business":"{d}","layers":[{}],"active":[],"operation":"op","runtime":"engine","driver":"host","output":"{d}"}}"#,
            basis_text(d), std::iter::repeat(format!("\"{d}\"")).take(super::super::contract::MAX_LAYERS + 1).collect::<Vec<_>>().join(","));
        assert!(matches!(decode_composition_manifest(syntax(&too_many_layers), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "composition layer count", .. })));

        let too_many_records = format!(r#"{{"operation":"op","layer":"{d}","before_data":"{d}","after_data":"{d}","before_schema":"{d}","after_schema":"{d}","additions":[],"business_projection":"{d}","original_events":"{d}","original_records":{},"capture_day":0,"capture_program":"{d}","prior_snapshot":"{d}","adoption_revision":0}}"#,
            MAX_COLLECTION + 1);
        assert!(matches!(decode_metadata_initialization_receipt(syntax(&too_many_records), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "initialization original record count", .. })));
        let too_many_additions = format!(r#"{{"operation":"op","layer":"{d}","before_data":"{d}","after_data":"{d}","before_schema":"{d}","after_schema":"{d}","additions":[{}],"business_projection":"{d}","original_events":"{d}","original_records":0,"capture_day":0,"capture_program":"{d}","prior_snapshot":"{d}","adoption_revision":0}}"#,
            std::iter::repeat("null").take(MAX_COLLECTION + 1).collect::<Vec<_>>().join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_metadata_initialization_receipt(syntax(&too_many_additions), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "metadata addition count", .. })));
        assert_eq!(cx.growth_requests(), 0, "addition count is checked before vector reservation");
        assert!(product_json::parse_json_bytes(br#"{"patch":0,"patch":1}"#, &mut AllocationContext::new()).is_err(), "duplicate object keys remain rejected by the bounded syntax parser");
    }

    #[test]
    fn every_reached_receipt_growth_refusal_is_fixed_and_retryable() {
        let d = digest_text();
        let basis = basis_text(d);
        let mapping = format!(r#"{{"layer":"{d}","patch":0,"from_source":"{d}","from":{{"kind":"update","action":"a","path":[1],"field":"x"}},"to_source":"{d}","to":{{"kind":"observable","observable":"y"}},"subject":"s"}}"#);
        let addition = r#"{"record":{"entity":"person","record":"r1"},"values":{"name":{"kind":"text","value":"Ada"}}}"#;
        let manifest = format!(r#"{{"version":1,"basis":{basis},"transition":"adoption","rewrites":[{mapping}],"business":"{d}","layers":["{d}"],"active":["{d}","{d}"],"previous":null,"operation":"op","runtime":"runtime","driver":"driver","output":"{d}"}}"#);
        let receipt = format!(r#"{{"operation":"op","layer":"{d}","before_data":"{d}","after_data":"{d}","before_schema":"{d}","after_schema":"{d}","additions":[{addition}],"business_projection":"{d}","original_events":"{d}","original_records":1,"capture_day":1,"capture_program":"{d}","prior_snapshot":"{d}","adoption_revision":1}}"#);
        let cases: [(&str, fn(Value, &mut AllocationContext) -> Result<(), ValueDecodeError>); 4] = [
            (&mapping, |v, c| decode_scope_slot_mapping(v, c).map(|_| ())),
            (&manifest, |v, c| decode_composition_manifest(v, c).map(|_| ())),
            (addition, |v, c| decode_metadata_addition(v, c).map(|_| ())),
            (&receipt, |v, c| decode_metadata_initialization_receipt(v, c).map(|_| ())),
        ];
        for (source, decode) in cases {
            let mut count = AllocationContext::new();
            decode(syntax(source), &mut count).unwrap();
            let growths = count.growth_requests();
            assert!(growths > 0, "fixture contains no decoder growth: {source}");
            for n in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true);
                assert!(matches!(decode(syntax(source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {n}/{growths} in {source}");
                decode(syntax(source), &mut AllocationContext::new()).unwrap();
            }
        }
    }
}
