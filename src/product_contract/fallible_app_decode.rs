//! Fallible structural decoding for the persisted Product Contract App graph.
//!
//! The DTO mirrors the derived Serde wire format while replacing BTree maps and
//! sets with ordered fallible owners. Semantic AppDefinition validation stays
//! with the existing caller.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::{OrderedMap, OrderedSet};
use crate::product_json::{Number, Value};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use super::fallible_entity_decode::decode_field_definition;
use super::fallible_expr_decode::decode_expr_with_budget;
use super::fallible_value_decode::{decode_data_value, decode_type, decode_values, JsonKind, NumericValue, ValueDecodeError};
use super::{
    ActionPlacement, CollectionOperation, CollectionTarget, EntityDefinition, Expr, FallibleValues,
    KeyModifier, ObservableDefinition, OutputDefinition,
    OutputFormat, StateDefinition, Type, MAX_AST_NODES, MAX_EXPR_DEPTH, MAX_ITEMS,
};

const APP_FIELDS: &[&str] = &["version", "id", "label", "entities", "state", "actions", "outputs", "observables", "views", "initial_view"];
const STATE_FIELDS: &[&str] = &["id", "label", "value_type", "initial"];
const ACTION_FIELDS: &[&str] = &["id", "label", "parameters", "guards", "steps", "ensures"];
const OUTPUT_FIELDS: &[&str] = &["id", "label", "format", "columns"];
const OBSERVABLE_FIELDS: &[&str] = &["id", "label", "value"];
const VIEW_FIELDS: &[&str] = &["id", "label", "kind", "actions", "keys"];
const STATEMENT_VARIANTS: &[&str] = &["create", "update", "archive", "set_state", "collection", "for_each", "assert", "emit"];
const CREATE_FIELDS: &[&str] = &["entity", "values", "bind"];
const UPDATE_FIELDS: &[&str] = &["record", "values"];
const ARCHIVE_FIELDS: &[&str] = &["record"];
const SET_STATE_FIELDS: &[&str] = &["state", "value"];
const COLLECTION_FIELDS: &[&str] = &["target", "operation", "items"];
const FOREACH_FIELDS: &[&str] = &["items", "binding", "steps"];
const ASSERT_FIELDS: &[&str] = &["condition", "message"];
const EMIT_FIELDS: &[&str] = &["output", "items", "binding", "columns"];
const COLLECTION_TARGET_VARIANTS: &[&str] = &["state", "field"];
const COLLECTION_STATE_FIELDS: &[&str] = &["state"];
const COLLECTION_FIELD_FIELDS: &[&str] = &["record", "field"];
const VIEW_KIND_VARIANTS: &[&str] = &["list", "detail", "form"];
const VIEW_LIST_FIELDS: &[&str] = &["entity", "rows", "columns", "controls", "selection"];
const VIEW_DETAIL_FIELDS: &[&str] = &["entity", "record", "columns"];
const VIEW_FORM_FIELDS: &[&str] = &["action", "fields", "defaults"];
const COLUMN_FIELDS: &[&str] = &["id", "label", "value"];
const STATE_CONTROL_FIELDS: &[&str] = &["id", "label", "state", "on_change"];
const SELECTION_FIELDS: &[&str] = &["id", "state", "on_change"];
const FORM_INPUT_FIELDS: &[&str] = &["parameter", "label"];
const ACTION_BINDING_FIELDS: &[&str] = &["id", "label", "placement", "action", "arguments", "enabled"];
const KEY_BINDING_FIELDS: &[&str] = &["key", "modifiers", "binding"];
const ACTION_PLACEMENTS: &[&str] = &["toolbar", "row"];
const OUTPUT_FORMATS: &[&str] = &["csv", "json"];
const COLLECTION_OPERATIONS: &[&str] = &["insert", "remove"];
const KEY_MODIFIERS: &[&str] = &["control", "alt", "shift", "command"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedAppDefinition {
    pub(super) version: u32,
    pub(super) id: String,
    pub(super) label: String,
    pub(super) entities: Vec<EntityDefinition>,
    pub(super) state: Vec<StateDefinition>,
    pub(super) actions: Vec<DecodedActionDefinition>,
    pub(super) outputs: Vec<OutputDefinition>,
    pub(super) observables: Vec<ObservableDefinition>,
    pub(super) views: Vec<DecodedViewDefinition>,
    pub(super) initial_view: String,
}
impl DecodedAppDefinition {
    pub(crate) fn validate_session_with(
        &self,
        session: &super::fallible_persisted_decode::DecodedSessionState,
    ) -> Result<(), super::ContractError> {
        super::require(self.views.iter().any(|view| view.id == session.view()), "unknown session view")?;
        super::require(session.values().len() == self.state.len(), "session state differs from program")?;
        for state in &self.state {
            super::validate_value(
                session.values().get(&state.id).ok_or_else(|| super::ContractError("missing session state".into()))?,
                &state.value_type,
                0,
            )?;
        }
        if let Some(record) = session.focused_record() {
            super::check_id(&record.record)?;
            super::require(self.entities.iter().any(|entity| entity.id == record.entity), "focused record entity is unknown")?;
        }
        Ok(())
    }

    pub(crate) fn validate_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<(), super::AppValidationError> {
        super::validate_decoded_app(self, cx)
    }
    pub(crate) fn try_clone_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<Self, super::AppValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => super::AppValidationError::Allocation(failure),
                _ => super::AppValidationError::Contract(super::ContractError("program copy invariant".into())),
            })?;
        decode_app_definition(value, cx).map_err(|error| match error {
            ValueDecodeError::Allocation(failure) => super::AppValidationError::Allocation(failure),
            _ => super::AppValidationError::Contract(super::ContractError("program copy invariant".into())),
        })
    }
    pub(crate) fn clear_display_labels(&mut self) {
        self.label.clear();
        for entity in &mut self.entities {
            entity.label.clear();
            for field in &mut entity.fields { field.label.clear(); }
        }
        for state in &mut self.state { state.label.clear(); }
        for action in &mut self.actions { action.label.clear(); }
        for output in &mut self.outputs {
            output.label.clear();
            for column in &mut output.columns { column.label.clear(); }
        }
        for observable in &mut self.observables { observable.label.clear(); }
        for view in &mut self.views {
            view.label.clear();
            for action in &mut view.actions { action.label.clear(); }
            match &mut view.kind {
                DecodedViewKind::List { columns, controls, .. } => {
                    for column in columns { column.label.clear(); }
                    for control in controls { control.label.clear(); }
                }
                DecodedViewKind::Detail { columns, .. } => for column in columns { column.label.clear(); },
                DecodedViewKind::Form { fields, .. } => for field in fields { field.label.clear(); },
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedActionDefinition {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) parameters: FallibleMap<Type>,
    pub(super) guards: Vec<Expr>,
    pub(super) steps: Vec<DecodedStatement>,
    pub(super) ensures: Vec<Expr>,
}

pub(super) struct FallibleMap<V>(OrderedMap<String, V>);
pub(super) struct FallibleMapIter<'a, V>(std::slice::Iter<'a, (String, V)>);
impl<'a, V> Iterator for FallibleMapIter<'a, V> {
    type Item = (&'a String, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, value)| (key, value))
    }
    fn size_hint(&self) -> (usize, Option<usize>) { self.0.size_hint() }
}
impl<V> ExactSizeIterator for FallibleMapIter<'_, V> {}
impl<V> FallibleMap<V> {
    pub(super) const fn new() -> Self { Self(OrderedMap::new()) }
    pub(super) fn len(&self) -> usize { self.0.len() }
    pub(super) fn is_empty(&self) -> bool { self.0.is_empty() }
    pub(super) fn get(&self, key: &str) -> Option<&V> { self.0.get(key) }
    pub(super) fn contains_key(&self, key: &str) -> bool { self.0.contains_key(key) }
    pub(super) fn iter(&self) -> FallibleMapIter<'_, V> { FallibleMapIter(self.0.iter_entries()) }
    pub(super) fn try_insert(
        &mut self,
        key: String,
        value: V,
        cx: &mut AllocationContext,
    ) -> Result<Option<V>, crate::product_alloc::AllocationFailure> {
        self.0.try_insert(key, value, cx)
    }
    pub(super) fn try_clone_with<F>(
        &self,
        cx: &mut AllocationContext,
        copy_value: F,
    ) -> Result<Self, crate::product_alloc::AllocationFailure>
    where
        F: FnMut(&V, &mut AllocationContext) -> Result<V, crate::product_alloc::AllocationFailure>,
    {
        self.0
            .try_clone_with(
                cx,
                |key, cx| product_alloc::try_copy_str(key, cx, AllocationSite::String),
                copy_value,
            )
            .map(Self)
    }
}
impl<V: std::fmt::Debug> std::fmt::Debug for FallibleMap<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}
impl<V: Serialize> Serialize for FallibleMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

pub(super) struct FallibleKeyModifiers(pub(super) OrderedSet<KeyModifier>);
impl FallibleKeyModifiers {
    pub(super) fn len(&self) -> usize { self.0.len() }
    pub(super) fn mask(&self) -> u8 {
        self.0.iter().fold(0, |mask, value| mask | match value {
            KeyModifier::Control => 1, KeyModifier::Alt => 2, KeyModifier::Shift => 4, KeyModifier::Command => 8,
        })
    }
}
impl std::fmt::Debug for FallibleKeyModifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}
impl Serialize for FallibleKeyModifiers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.iter() {
            sequence.serialize_element(value)?;
        }
        sequence.end()
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum DecodedStatement {
    Create { entity: String, values: FallibleMap<Expr>, bind: String },
    Update { record: Expr, values: FallibleMap<Expr> },
    Archive { record: Expr },
    SetState { state: String, value: Expr },
    Collection { target: CollectionTarget, operation: CollectionOperation, items: Expr },
    ForEach { items: Expr, binding: String, steps: Vec<DecodedStatement> },
    Assert { condition: Expr, message: String },
    Emit { output: String, items: Expr, binding: String, columns: FallibleMap<Expr> },
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedViewDefinition {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) kind: DecodedViewKind,
    pub(super) actions: Vec<DecodedActionBinding>,
    pub(super) keys: Vec<DecodedKeyBinding>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum DecodedViewKind {
    List {
        entity: String,
        rows: Expr,
        columns: Vec<DecodedColumn>,
        controls: Vec<DecodedStateControl>,
        selection: Option<DecodedSelectionBinding>,
    },
    Detail { entity: String, record: Expr, columns: Vec<DecodedColumn> },
    Form { action: String, fields: Vec<DecodedFormInput>, defaults: FallibleValues },
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedColumn { pub(super) id: String, pub(super) label: String, pub(super) value: Expr }
#[derive(Debug, Serialize)]
pub(super) struct DecodedStateControl { pub(super) id: String, pub(super) label: String, pub(super) state: String, pub(super) on_change: Option<String> }
#[derive(Debug, Serialize)]
pub(super) struct DecodedSelectionBinding { pub(super) id: String, pub(super) state: String, pub(super) on_change: Option<String> }
#[derive(Debug, Serialize)]
pub(super) struct DecodedFormInput { pub(super) parameter: String, pub(super) label: String }
#[derive(Debug, Serialize)]
pub(super) struct DecodedActionBinding {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) placement: ActionPlacement,
    pub(super) action: String,
    pub(super) arguments: FallibleMap<Expr>,
    pub(super) enabled: Expr,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedKeyBinding { pub(super) key: String, pub(super) modifiers: FallibleKeyModifiers, pub(super) binding: String }

pub(crate) fn decode_app_definition(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedAppDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct AppDefinition", APP_FIELDS, cx)?;
    let mut version = None;
    let mut id = None;
    let mut label = None;
    let mut decoded_entities = None;
    let mut state = None;
    let mut decoded_actions = None;
    let mut decoded_outputs = None;
    let mut decoded_observables = None;
    let mut decoded_views = None;
    let mut initial_view = None;
    let mut nodes = 0usize;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "version" => version = Some(integer_u32(value)?),
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "entities" => decoded_entities = Some(entities(value, cx, &mut nodes)?),
            "state" => state = Some(states(value, cx, &mut nodes)?),
            "actions" => decoded_actions = Some(actions(value, cx, &mut nodes)?),
            "outputs" => decoded_outputs = Some(outputs(value, cx)?),
            "observables" => decoded_observables = Some(observables(value, cx, &mut nodes)?),
            "views" => decoded_views = Some(views(value, cx, &mut nodes)?),
            "initial_view" => initial_view = Some(string(value, "a string")?),
            _ => return Err(unknown_field(key, APP_FIELDS)),
        }
    }
    Ok(DecodedAppDefinition {
        version: required(version, "version")?, id: required(id, "id")?,
        label: required(label, "label")?, entities: required(decoded_entities, "entities")?,
        state: required(state, "state")?, actions: required(decoded_actions, "actions")?,
        outputs: required(decoded_outputs, "outputs")?, observables: required(decoded_observables, "observables")?,
        views: required(decoded_views, "views")?, initial_view: required(initial_view, "initial_view")?,
    })
}

fn entities(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<EntityDefinition>, ValueDecodeError> {
    sequence(value, "entity definition count", cx, nodes, |v, cx, nodes| super::fallible_entity_decode::decode_entity_definition_with_budget(v, cx, nodes))
}

fn states(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<StateDefinition>, ValueDecodeError> {
    sequence(value, "state definition count", cx, nodes, |v, cx, nodes| decode_state(v, cx, nodes))
}

fn decode_state(value: Value, cx: &mut AllocationContext, _nodes: &mut usize) -> Result<StateDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct StateDefinition", STATE_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut value_type = None; let mut initial = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "value_type" => value_type = Some(decode_type(value, cx)?),
            "initial" => initial = Some(decode_data_value(value, cx)?),
            _ => return Err(unknown_field(key, STATE_FIELDS)),
        }
    }
    Ok(StateDefinition { id: required(id, "id")?, label: required(label, "label")?, value_type: required(value_type, "value_type")?, initial: required(initial, "initial")? })
}

fn actions(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<DecodedActionDefinition>, ValueDecodeError> {
    sequence(value, "action definition count", cx, nodes, |v, cx, nodes| decode_action(v, cx, nodes))
}

fn decode_action(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<DecodedActionDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ActionDefinition", ACTION_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut parameters = None; let mut guards = None; let mut steps = None; let mut ensures = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "parameters" => parameters = Some(map(value, cx, decode_type)?),
            "guards" => guards = Some(expressions(value, 0, nodes, cx)?),
            "steps" => steps = Some(statements(value, 0, nodes, cx)?),
            "ensures" => ensures = Some(expressions(value, 0, nodes, cx)?),
            _ => return Err(unknown_field(key, ACTION_FIELDS)),
        }
    }
    Ok(DecodedActionDefinition { id: required(id, "id")?, label: required(label, "label")?, parameters: required(parameters, "parameters")?, guards: required(guards, "guards")?, steps: required(steps, "steps")?, ensures: required(ensures, "ensures")? })
}

fn outputs(value: Value, cx: &mut AllocationContext) -> Result<Vec<OutputDefinition>, ValueDecodeError> {
    let mut nodes = 0;
    sequence(value, "output definition count", cx, &mut nodes, |v, cx, _| decode_output(v, cx))
}

fn decode_output(value: Value, cx: &mut AllocationContext) -> Result<OutputDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OutputDefinition", OUTPUT_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut format = None; let mut columns = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "format" => format = Some(output_format(value)?),
            "columns" => columns = Some(sequence(value, "output column count", cx, &mut 0, |v, cx, _| decode_field_definition(v, cx))?),
            _ => return Err(unknown_field(key, OUTPUT_FIELDS)),
        }
    }
    Ok(OutputDefinition { id: required(id, "id")?, label: required(label, "label")?, format: required(format, "format")?, columns: required(columns, "columns")? })
}

fn observables(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<ObservableDefinition>, ValueDecodeError> {
    sequence(value, "observable definition count", cx, nodes, |v, cx, nodes| decode_observable(v, cx, nodes))
}

fn decode_observable(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<ObservableDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ObservableDefinition", OBSERVABLE_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut expression = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "value" => expression = Some(expr(value, 0, nodes, cx)?),
            _ => return Err(unknown_field(key, OBSERVABLE_FIELDS)),
        }
    }
    Ok(ObservableDefinition { id: required(id, "id")?, label: required(label, "label")?, value: required(expression, "value")? })
}

fn views(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<DecodedViewDefinition>, ValueDecodeError> {
    sequence(value, "view definition count", cx, nodes, |v, cx, nodes| decode_view(v, cx, nodes))
}

fn decode_view(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<DecodedViewDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ViewDefinition", VIEW_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut kind = None; let mut actions = None; let mut keys = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "kind" => kind = Some(view_kind(value, cx, nodes)?),
            "actions" => actions = Some(sequence(value, "view action binding count", cx, nodes, |v, cx, nodes| action_binding(v, cx, nodes))?),
            "keys" => keys = Some(sequence(value, "view key binding count", cx, nodes, |v, cx, _| key_binding(v, cx))?),
            _ => return Err(unknown_field(key, VIEW_FIELDS)),
        }
    }
    Ok(DecodedViewDefinition { id: required(id, "id")?, label: required(label, "label")?, kind: required(kind, "kind")?, actions: required(actions, "actions")?, keys: required(keys, "keys")? })
}

fn view_kind(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<DecodedViewKind, ValueDecodeError> {
    let (tag, fields) = tagged(value, VIEW_KIND_VARIANTS)?;
    match tag.as_str() {
        "list" => {
            let mut entity = None; let mut rows = None; let mut columns = None; let mut controls = None; let mut selection = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "entity" => entity = Some(string(value, "a string")?),
                    "rows" => rows = Some(expr(value, 0, nodes, cx)?),
                    "columns" => columns = Some(sequence(value, "view column count", cx, nodes, |v, cx, nodes| column(v, cx, nodes))?),
                    "controls" => controls = Some(sequence(value, "view control count", cx, nodes, |v, cx, _| state_control(v, cx))?),
                    "selection" => selection = Some(option(value, cx, |v, cx| selection_binding(v, cx))?),
                    _ => return Err(unknown_field(key, VIEW_LIST_FIELDS)),
                }
            }
            Ok(DecodedViewKind::List { entity: required(entity, "entity")?, rows: required(rows, "rows")?, columns: required(columns, "columns")?, controls: required(controls, "controls")?, selection: selection.unwrap_or(None) })
        }
        "detail" => {
            let mut entity = None; let mut record = None; let mut columns = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "entity" => entity = Some(string(value, "a string")?),
                    "record" => record = Some(expr(value, 0, nodes, cx)?),
                    "columns" => columns = Some(sequence(value, "view column count", cx, nodes, |v, cx, nodes| column(v, cx, nodes))?),
                    _ => return Err(unknown_field(key, VIEW_DETAIL_FIELDS)),
                }
            }
            Ok(DecodedViewKind::Detail { entity: required(entity, "entity")?, record: required(record, "record")?, columns: required(columns, "columns")? })
        }
        "form" => {
            let mut action = None; let mut fields_values = None; let mut defaults = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "action" => action = Some(string(value, "a string")?),
                    "fields" => fields_values = Some(sequence(value, "form input count", cx, nodes, |v, cx, _nodes| form_input(v, cx))?),
                    "defaults" => defaults = Some(decode_values(value, cx)?),
                    _ => return Err(unknown_field(key, VIEW_FORM_FIELDS)),
                }
            }
            Ok(DecodedViewKind::Form { action: required(action, "action")?, fields: required(fields_values, "fields")?, defaults: required(defaults, "defaults")? })
        }
        _ => unreachable!("view variant list is complete"),
    }
}

fn column(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<DecodedColumn, ValueDecodeError> {
    let fields = struct_fields(value, "a struct Column", COLUMN_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut expression = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "value" => expression = Some(expr(value, 0, nodes, cx)?),
            _ => return Err(unknown_field(key, COLUMN_FIELDS)),
        }
    }
    Ok(DecodedColumn { id: required(id, "id")?, label: required(label, "label")?, value: required(expression, "value")? })
}

fn state_control(value: Value, cx: &mut AllocationContext) -> Result<DecodedStateControl, ValueDecodeError> {
    let fields = struct_fields(value, "a struct StateControl", STATE_CONTROL_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut state = None; let mut on_change = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "state" => state = Some(string(value, "a string")?),
            "on_change" => on_change = Some(option(value, cx, |v, _cx| string(v, "a string"))?),
            _ => return Err(unknown_field(key, STATE_CONTROL_FIELDS)),
        }
    }
    Ok(DecodedStateControl { id: required(id, "id")?, label: required(label, "label")?, state: required(state, "state")?, on_change: on_change.unwrap_or(None) })
}

fn selection_binding(value: Value, cx: &mut AllocationContext) -> Result<DecodedSelectionBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct SelectionBinding", SELECTION_FIELDS, cx)?;
    let mut id = None; let mut state = None; let mut on_change = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "state" => state = Some(string(value, "a string")?),
            "on_change" => on_change = Some(match value { Value::Null => None, value => Some(string(value, "a string")?) }),
            _ => return Err(unknown_field(key, SELECTION_FIELDS)),
        }
    }
    Ok(DecodedSelectionBinding { id: required(id, "id")?, state: required(state, "state")?, on_change: on_change.unwrap_or(None) })
}

fn form_input(value: Value, cx: &mut AllocationContext) -> Result<DecodedFormInput, ValueDecodeError> {
    let fields = struct_fields(value, "a struct FormInput", FORM_INPUT_FIELDS, cx)?;
    let mut parameter = None; let mut label = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "parameter" => parameter = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            _ => return Err(unknown_field(key, FORM_INPUT_FIELDS)),
        }
    }
    Ok(DecodedFormInput { parameter: required(parameter, "parameter")?, label: required(label, "label")? })
}

fn action_binding(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<DecodedActionBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct ActionBinding", ACTION_BINDING_FIELDS, cx)?;
    let mut id = None; let mut label = None; let mut placement = None; let mut action = None; let mut arguments = None; let mut enabled = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "placement" => placement = Some(action_placement(value)?),
            "action" => action = Some(string(value, "a string")?),
            "arguments" => arguments = Some(map(value, cx, |v, cx| decode_expr_with_budget(v, 0, nodes, cx))?),
            "enabled" => enabled = Some(expr(value, 0, nodes, cx)?),
            _ => return Err(unknown_field(key, ACTION_BINDING_FIELDS)),
        }
    }
    Ok(DecodedActionBinding { id: required(id, "id")?, label: required(label, "label")?, placement: required(placement, "placement")?, action: required(action, "action")?, arguments: required(arguments, "arguments")?, enabled: required(enabled, "enabled")? })
}

fn key_binding(value: Value, cx: &mut AllocationContext) -> Result<DecodedKeyBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct KeyBinding", KEY_BINDING_FIELDS, cx)?;
    let mut key = None; let mut modifiers = None; let mut binding = None;
    for (field, value) in fields.into_entries() {
        match field.as_str() {
            "key" => key = Some(string(value, "a string")?),
            "modifiers" => modifiers = Some(key_modifiers(value, cx)?),
            "binding" => binding = Some(string(value, "a string")?),
            _ => return Err(unknown_field(field, KEY_BINDING_FIELDS)),
        }
    }
    Ok(DecodedKeyBinding { key: required(key, "key")?, modifiers: required(modifiers, "modifiers")?, binding: required(binding, "binding")? })
}

fn key_modifiers(value: Value, cx: &mut AllocationContext) -> Result<FallibleKeyModifiers, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), "key modifier count")?;
    let mut decoded = OrderedSet::new();
    for value in values {
        let tag = string(value, "a string variant identifier")?;
        let modifier = match tag.as_str() {
            "control" => KeyModifier::Control,
            "alt" => KeyModifier::Alt,
            "shift" => KeyModifier::Shift,
            "command" => KeyModifier::Command,
            _ => return Err(ValueDecodeError::UnknownVariant { tag, expected: KEY_MODIFIERS }),
        };
        decoded.try_insert(modifier, cx)?;
    }
    Ok(FallibleKeyModifiers(decoded))
}

fn statements(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Vec<DecodedStatement>, ValueDecodeError> {
    sequence(value, "statement count", cx, nodes, |v, cx, nodes| statement(v, depth, nodes, cx))
}

fn statement(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<DecodedStatement, ValueDecodeError> {
    if depth > MAX_EXPR_DEPTH {
        return Err(ValueDecodeError::LimitExceeded { resource: "statement depth", limit: MAX_EXPR_DEPTH });
    }
    count_node(nodes)?;
    let (tag, fields) = tagged(value, STATEMENT_VARIANTS)?;
    match tag.as_str() {
        "create" => {
            let mut entity = None; let mut values = None; let mut bind = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "entity" => entity = Some(string(value, "a string")?),
                "values" => values = Some(assignment_map(value, cx, nodes, depth)?),
                "bind" => bind = Some(string(value, "a string")?),
                _ => return Err(unknown_field(key, CREATE_FIELDS)),
            }}
            Ok(DecodedStatement::Create { entity: required(entity, "entity")?, values: required(values, "values")?, bind: required(bind, "bind")? })
        }
        "update" => {
            let mut record = None; let mut values = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "record" => record = Some(expr(value, depth + 1, nodes, cx)?),
                "values" => values = Some(assignment_map(value, cx, nodes, depth)?),
                _ => return Err(unknown_field(key, UPDATE_FIELDS)),
            }}
            Ok(DecodedStatement::Update { record: required(record, "record")?, values: required(values, "values")? })
        }
        "archive" => {
            let mut record = None;
            for (key, value) in fields.into_entries() { if key == "record" { record = Some(expr(value, depth + 1, nodes, cx)?); } else { return Err(unknown_field(key, ARCHIVE_FIELDS)); } }
            Ok(DecodedStatement::Archive { record: required(record, "record")? })
        }
        "set_state" => {
            let mut state = None; let mut expression = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "state" => state = Some(string(value, "a string")?),
                "value" => expression = Some(expr(value, depth + 1, nodes, cx)?),
                _ => return Err(unknown_field(key, SET_STATE_FIELDS)),
            }}
            Ok(DecodedStatement::SetState { state: required(state, "state")?, value: required(expression, "value")? })
        }
        "collection" => {
            let mut target = None; let mut operation = None; let mut items = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "target" => target = Some(collection_target(value, depth + 1, nodes, cx)?),
                "operation" => operation = Some(collection_operation(value)?),
                "items" => items = Some(expr(value, depth + 1, nodes, cx)?),
                _ => return Err(unknown_field(key, COLLECTION_FIELDS)),
            }}
            Ok(DecodedStatement::Collection { target: required(target, "target")?, operation: required(operation, "operation")?, items: required(items, "items")? })
        }
        "for_each" => {
            let mut items = None; let mut binding = None; let mut steps = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "items" => items = Some(expr(value, depth + 1, nodes, cx)?),
                "binding" => binding = Some(string(value, "a string")?),
                "steps" => steps = Some(statements(value, depth + 1, nodes, cx)?),
                _ => return Err(unknown_field(key, FOREACH_FIELDS)),
            }}
            Ok(DecodedStatement::ForEach { items: required(items, "items")?, binding: required(binding, "binding")?, steps: required(steps, "steps")? })
        }
        "assert" => {
            let mut condition = None; let mut message = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "condition" => condition = Some(expr(value, depth + 1, nodes, cx)?),
                "message" => message = Some(string(value, "a string")?),
                _ => return Err(unknown_field(key, ASSERT_FIELDS)),
            }}
            Ok(DecodedStatement::Assert { condition: required(condition, "condition")?, message: required(message, "message")? })
        }
        "emit" => {
            let mut output = None; let mut items = None; let mut binding = None; let mut columns = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "output" => output = Some(string(value, "a string")?),
                "items" => items = Some(expr(value, depth + 1, nodes, cx)?),
                "binding" => binding = Some(string(value, "a string")?),
                "columns" => columns = Some(map(value, cx, |v, cx| decode_expr_with_budget(v, depth + 1, nodes, cx))?),
                _ => return Err(unknown_field(key, EMIT_FIELDS)),
            }}
            Ok(DecodedStatement::Emit { output: required(output, "output")?, items: required(items, "items")?, binding: required(binding, "binding")?, columns: required(columns, "columns")? })
        }
        _ => unreachable!("statement variant list is complete"),
    }
}

fn collection_target(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<CollectionTarget, ValueDecodeError> {
    let (tag, fields) = tagged(value, COLLECTION_TARGET_VARIANTS)?;
    match tag.as_str() {
        "state" => {
            let mut state = None;
            for (key, value) in fields.into_entries() { if key == "state" { state = Some(string(value, "a string")?); } else { return Err(unknown_field(key, COLLECTION_STATE_FIELDS)); } }
            Ok(CollectionTarget::State { state: required(state, "state")? })
        }
        "field" => {
            let mut record = None; let mut field = None;
            for (key, value) in fields.into_entries() { match key.as_str() {
                "record" => record = Some(expr(value, depth, nodes, cx)?),
                "field" => field = Some(string(value, "a string")?),
                _ => return Err(unknown_field(key, COLLECTION_FIELD_FIELDS)),
            }}
            Ok(CollectionTarget::Field { record: required(record, "record")?, field: required(field, "field")? })
        }
        _ => unreachable!("collection target variant list is complete"),
    }
}

fn expressions(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Vec<Expr>, ValueDecodeError> {
    sequence(value, "expression count", cx, nodes, |v, cx, nodes| expr(v, depth, nodes, cx))
}
fn expr(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Expr, ValueDecodeError> {
    decode_expr_with_budget(value, depth, nodes, cx)
}

fn map<V>(value: Value, cx: &mut AllocationContext, mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<V, ValueDecodeError>) -> Result<FallibleMap<V>, ValueDecodeError> {
    let fields = match value { Value::Object(fields) => fields, value => return Err(invalid_type(value, "a map")) };
    bounded(fields.len(), "map entry count")?;
    let mut result = OrderedMap::new();
    result.try_reserve(fields.len(), cx)?;
    for (key, value) in fields.into_entries() {
        let value = decode(value, cx)?;
        result.try_insert(key, value, cx)?;
    }
    Ok(FallibleMap(result))
}

fn assignment_map(value: Value, cx: &mut AllocationContext, nodes: &mut usize, depth: usize) -> Result<FallibleMap<Expr>, ValueDecodeError> {
    let len = match &value { Value::Object(fields) => fields.len(), value => return Err(ValueDecodeError::InvalidType { actual: json_kind(value), expected: "a map" }) };
    bounded(len, "assignment count")?;
    map(value, cx, |v, cx| decode_expr_with_budget(v, depth + 1, nodes, cx))
}

fn sequence<T>(value: Value, resource: &'static str, cx: &mut AllocationContext, nodes: &mut usize, mut decode: impl FnMut(Value, &mut AllocationContext, &mut usize) -> Result<T, ValueDecodeError>) -> Result<Vec<T>, ValueDecodeError> {
    let values = array(value)?;
    bounded(values.len(), resource)?;
    let mut result = Vec::new();
    product_alloc::try_reserve(&mut result, values.len(), cx, AllocationSite::Vector)?;
    for value in values { result.push(decode(value, cx, nodes)?); }
    Ok(result)
}

fn tagged(value: Value, variants: &'static [&'static str]) -> Result<(String, OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = object(value, "an internally tagged enum")?;
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = string(tag, "a string variant identifier")?;
    if variants.contains(&tag.as_str()) { Ok((tag, fields)) }
    else { Err(ValueDecodeError::UnknownVariant { tag, expected: variants }) }
}

fn output_format(value: Value) -> Result<OutputFormat, ValueDecodeError> {
    let tag = string(value, "a string variant identifier")?;
    match tag.as_str() { "csv" => Ok(OutputFormat::Csv), "json" => Ok(OutputFormat::Json), _ => Err(ValueDecodeError::UnknownVariant { tag, expected: OUTPUT_FORMATS }) }
}
fn collection_operation(value: Value) -> Result<CollectionOperation, ValueDecodeError> {
    let tag = string(value, "a string variant identifier")?;
    match tag.as_str() { "insert" => Ok(CollectionOperation::Insert), "remove" => Ok(CollectionOperation::Remove), _ => Err(ValueDecodeError::UnknownVariant { tag, expected: COLLECTION_OPERATIONS }) }
}
fn action_placement(value: Value) -> Result<ActionPlacement, ValueDecodeError> {
    let tag = string(value, "a string variant identifier")?;
    match tag.as_str() { "toolbar" => Ok(ActionPlacement::Toolbar), "row" => Ok(ActionPlacement::Row), _ => Err(ValueDecodeError::UnknownVariant { tag, expected: ACTION_PLACEMENTS }) }
}

fn option<T>(value: Value, cx: &mut AllocationContext, decode: impl FnOnce(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value, cx).map(Some) }
}
fn object(value: Value, expected: &'static str) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value { Value::Object(fields) => Ok(fields), value => Err(invalid_type(value, expected)) }
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Array(values) => {
            if values.len() != names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let count = values.len().min(names.len());
            let mut fields = OrderedMap::new();
            fields.try_reserve(count, cx)?;
            for (name, value) in names.iter().take(count).zip(values) {
                let key = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(key, value, cx)?;
            }
            Ok(fields)
        }
        value => Err(invalid_type(value, expected)),
    }
}
fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value { Value::Array(values) => Ok(values), value => Err(invalid_type(value, "a sequence")) }
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
}
fn integer_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}
fn count_node(nodes: &mut usize) -> Result<(), ValueDecodeError> {
    *nodes = nodes.checked_add(1).ok_or(ValueDecodeError::LimitExceeded { resource: "application AST node count", limit: MAX_AST_NODES })?;
    if *nodes > MAX_AST_NODES { Err(ValueDecodeError::LimitExceeded { resource: "application AST node count", limit: MAX_AST_NODES }) } else { Ok(()) }
}
fn bounded(len: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > MAX_ITEMS { Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS }) } else { Ok(()) }
}
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    ValueDecodeError::InvalidType { actual: json_kind(&value), expected }
}
fn json_kind(value: &Value) -> JsonKind {
    match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::AppDefinition;
    use crate::product_alloc::AllocationFailureKind;
    use crate::product_json::{self, Cause};
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed(source: &str) -> AppDefinition {
        serde_json::from_str(source).unwrap()
    }

    fn full_app() -> String {
        r#"{
          "version":1,"id":"app","label":"Application 雪",
          "entities":[{"id":"person","label":"Person","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[["name"]],"constraints":[]}],
          "state":[{"id":"members","label":"Members","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"initial":{"kind":"list","item_type":{"kind":"reference","entity":"person"},"items":[]}}],
          "actions":[{"id":"save","label":"Save","parameters":{"name":{"kind":"text"}},"guards":[{"kind":"today"}],"steps":[
            {"kind":"create","entity":"person","values":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}},"bind":"record"},
            {"kind":"update","record":{"kind":"variable","name":"record"},"values":{"name":{"kind":"variable","name":"name"}}},
            {"kind":"archive","record":{"kind":"variable","name":"record"}},
            {"kind":"set_state","state":"members","value":{"kind":"variable","name":"members"}},
            {"kind":"collection","target":{"kind":"state","state":"members"},"operation":"insert","items":{"kind":"variable","name":"members"}},
            {"kind":"collection","target":{"kind":"field","record":{"kind":"variable","name":"record"},"field":"name"},"operation":"remove","items":{"kind":"variable","name":"members"}},
            {"kind":"for_each","items":{"kind":"variable","name":"members"},"binding":"member","steps":[{"kind":"assert","condition":{"kind":"today"},"message":"ok"}]},
            {"kind":"assert","condition":{"kind":"today"},"message":"ok"},
            {"kind":"emit","output":"report","items":{"kind":"variable","name":"members"},"binding":"member","columns":{"name":{"kind":"variable","name":"name"}}}
          ],"ensures":[]}],
          "outputs":[{"id":"report","label":"Report","format":"csv","columns":[{"id":"name","label":"Name","value_type":{"kind":"text"}}]},{"id":"json","label":"JSON","format":"json","columns":[]}],
          "observables":[{"id":"count","label":"Count","value":{"kind":"count","items":{"kind":"variable","name":"members"}}}],
          "views":[
            {"id":"list","label":"List","kind":{"kind":"list","entity":"person","rows":{"kind":"query","entity":"person","binding":"row","predicate":{"kind":"today"},"sort":[],"limit":10,"include_archived":false},"columns":[{"id":"name","label":"Name","value":{"kind":"today"}}],"controls":[{"id":"filter","label":"Filter","state":"members","on_change":"save"}],"selection":{"id":"selected","state":"members"}},"actions":[{"id":"save-row","label":"Save","placement":"row","action":"save","arguments":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}},"enabled":{"kind":"today"}}],"keys":[{"key":"K","modifiers":["shift","control","shift"],"binding":"save-row"},{"key":"J","modifiers":["alt","command"],"binding":"save-row"}]},
            {"id":"detail","label":"Detail","kind":{"kind":"detail","entity":"person","record":{"kind":"today"},"columns":[{"id":"name","label":"Name","value":{"kind":"today"}}]},"actions":[],"keys":[]},
            {"id":"form","label":"Form","kind":{"kind":"form","action":"save","fields":[{"parameter":"name","label":"Name"}],"defaults":{"enabled":{"kind":"boolean","value":true}}},"actions":[],"keys":[]}
          ],"initial_view":"list"
        }"#.to_owned()
    }

    fn valid_app_for_validation() -> String {
        full_app()
            .replace(
                r#"{"id":"json","label":"JSON","format":"json","columns":[]}"#,
                r#"{"id":"json","label":"JSON","format":"json","columns":[{"id":"name","label":"Name","value_type":{"kind":"text"}}]}"#,
            )
            .replace(r#"{"kind":"today"}"#, r#"{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}"#)
            .replace(r#""record":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}"#,
                     r#""record":{"kind":"literal","value_type":{"kind":"reference","entity":"person"},"value":{"kind":"reference","entity":"person","record":"ada"}}"#)
            .replace(r#"{"kind":"variable","name":"members"}"#,
                     r#"{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":{"kind":"list","item_type":{"kind":"reference","entity":"person"},"items":[]}}"#)
            .replace(r#""columns":{"name":{"kind":"variable","name":"name"}}"#,
                     r#""columns":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}}"#)
            .replace(r#""target":{"kind":"field","record":{"kind":"variable","name":"record"},"field":"name"}"#,
                     r#""target":{"kind":"state","state":"members"}"#)
            .replace(r#""on_change":"save""#, r#""on_change":null"#)
            .replace(r#""controls":[{"id":"filter","label":"Filter","state":"members","on_change":null}]"#, r#""controls":[]"#)
            .replace(r#""key":"K""#, r#""key":"k""#)
            .replace(r#""key":"J""#, r#""key":"j""#)
            .replace(r#""defaults":{"enabled":{"kind":"boolean","value":true}}"#,
                     r#""defaults":{"name":{"kind":"text","value":"Ada"}}"#)
    }

    fn canonical(source: &str) {
        let expected = typed(source);
        let mut cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(source), &mut cx).unwrap();
        assert_eq!(
            super::super::canonical_bytes(&expected).unwrap(),
            super::super::canonical_bytes(&decoded).unwrap()
        );
    }

    #[test]
    fn full_app_graph_and_all_view_kinds_match_serde_canonical_bytes() {
        canonical(&full_app());
        let expected = typed(&full_app());
        assert_eq!(expected.views.len(), 3);
        assert!(matches!(expected.views[0].kind, super::super::ViewKind::List { .. }));
        assert!(matches!(expected.views[1].kind, super::super::ViewKind::Detail { .. }));
        assert!(matches!(expected.views[2].kind, super::super::ViewKind::Form { .. }));
        assert_eq!(expected.views[0].keys[0].modifiers.len(), 2);
    }

    #[test]
    fn fallible_app_validation_matches_legacy_semantics_and_error_order() {
        let source = valid_app_for_validation();
        let legacy = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(&source), &mut cx).unwrap();
        assert_eq!(
            legacy.validate(),
            super::super::validate_decoded_app(&decoded, &mut cx).map_err(|error| match error {
                super::super::AppValidationError::Contract(error) => error,
                super::super::AppValidationError::Allocation(failure) => panic!("unexpected allocation refusal: {failure:?}"),
            }),
        );

        for (source, expected_error) in [
            (source.replace("\"initial_view\":\"list\"", "\"initial_view\":\"missing\""), "unknown initial view"),
            (source.replace("\"bind\":\"record\"", "\"bind\":\"name\""), "binding shadows existing variable: name"),
            (source.replace(r#"{"kind":"variable","name":"name"}"#, r#"{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}"#), "expected Text"),
        ] {
            let legacy = match typed(&source).validate() {
                Ok(()) => panic!("expected invalid source for {expected_error}"),
                Err(error) => error,
            };
            let mut cx = AllocationContext::new();
            let decoded = decode_app_definition(syntax(&source), &mut cx).unwrap();
            let actual = match super::super::validate_decoded_app(&decoded, &mut cx).unwrap_err() {
                super::super::AppValidationError::Contract(error) => error,
                super::super::AppValidationError::Allocation(failure) => panic!("unexpected allocation refusal: {failure:?}"),
            };
            assert_eq!(actual, legacy);
            assert!(actual.0.contains(expected_error), "{}", actual.0);
        }
    }

    #[test]
    fn fallible_validation_environment_refusal_is_typed_and_retryable() {
        let source = valid_app_for_validation();
        let mut decode_cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(&source), &mut decode_cx).unwrap();
        let before = super::super::canonical_bytes(&decoded).unwrap();
        let mut baseline = AllocationContext::new();
        super::super::validate_decoded_app(&decoded, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0, "validation must reserve fallible lexical environments");
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = super::super::validate_decoded_app(&decoded, &mut refused).unwrap_err();
            let super::super::AppValidationError::Allocation(failure) = error else {
                panic!("expected original allocation failure at growth {nth}: {error:?}");
            };
            assert_eq!(Some(failure), refused.first_failure());
            assert_eq!(failure.kind, AllocationFailureKind::Injected);
            assert_eq!(before, super::super::canonical_bytes(&decoded).unwrap());
            let mut retry = AllocationContext::new();
            super::super::validate_decoded_app(&decoded, &mut retry).unwrap();
        }
    }

    #[test]
    fn derived_serde_struct_sequences_and_length_rules_are_preserved() {
        let source = r#"[1,"app","App",[["person","Person",[["name","Name",{"kind":"text"}]],[ ["name"] ],[]]],[["members","Members",{"kind":"list","item":{"kind":"reference","entity":"person"}},{"kind":"list","item_type":{"kind":"reference","entity":"person"},"items":[]}]],[],[],[],[],"list"]"#;
        let expected: AppDefinition = serde_json::from_str(source).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(source), &mut cx).unwrap();
        assert_eq!(
            super::super::canonical_bytes(&expected).unwrap(),
            super::super::canonical_bytes(&decoded).unwrap()
        );
        let mut baseline = AllocationContext::new();
        let baseline_value = decode_app_definition(syntax(source), &mut baseline).unwrap();
        let baseline_bytes = super::super::canonical_bytes(&baseline_value).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths >= 15);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = decode_app_definition(syntax(source), &mut cx).unwrap_err();
            let ValueDecodeError::Allocation(failure) = error else { panic!("expected sequence owner refusal at growth {nth}") };
            assert_eq!(Some(failure), cx.first_failure());
            assert_eq!(failure.kind, AllocationFailureKind::Injected);
            let mut retry = AllocationContext::new();
            let decoded = decode_app_definition(syntax(source), &mut retry).unwrap();
            assert_eq!(baseline_bytes, super::super::canonical_bytes(&decoded).unwrap());
        }
        let extra = format!("{},null]", &source[..source.len() - 1]);
        assert!(serde_json::from_str::<AppDefinition>(&extra).is_err());
        assert!(matches!(decode_app_definition(syntax(&extra), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 11, expected: 10 })));

        let sort_source = r#"{"kind":"query","entity":"e","binding":"r","predicate":{"kind":"today"},"sort":[[{"kind":"today"},true]],"limit":1,"include_archived":false}"#;
        let expected_expr: Expr = serde_json::from_str(sort_source).unwrap();
        let mut cx = AllocationContext::new();
        let decoded_expr = super::super::fallible_expr_decode::decode_expr(syntax(sort_source), &mut cx).unwrap();
        assert_eq!(
            super::super::canonical_bytes(&expected_expr).unwrap(),
            super::super::canonical_bytes(&decoded_expr).unwrap()
        );
        let extra_sort = sort_source.replace(r#""sort":[[{"kind":"today"},true]]"#, r#""sort":[[{"kind":"today"},true,"ignored"]]"#);
        assert!(serde_json::from_str::<Expr>(&extra_sort).is_err());
        assert!(matches!(super::super::fallible_expr_decode::decode_expr(syntax(&extra_sort), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 3, expected: 2 })));

        let optional_tail = r#"{"version":1,"id":"a","label":"A","entities":[],"state":[],"actions":[],"outputs":[],"observables":[],"views":[{"id":"v","label":"V","kind":{"kind":"list","entity":"e","rows":{"kind":"today"},"columns":[["c","C",{"kind":"today"}]],"controls":[["ctrl","Control","s",null]],"selection":["sel","s",null]},"actions":[["bind","B","toolbar","act",{}, {"kind":"today"}]],"keys":[["K",["shift","shift"],"bind"]]}],"initial_view":"v"}"#;
        let omitted_optional_tail = optional_tail
            .replace(r#"["ctrl","Control","s",null]"#, r#"["ctrl","Control","s"]"#)
            .replace(r#"["sel","s",null]"#, r#"["sel","s"]"#);
        assert!(serde_json::from_str::<AppDefinition>(&omitted_optional_tail).is_err());
        assert!(matches!(decode_app_definition(syntax(&omitted_optional_tail), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 3, expected: 4 })));
        let expected: AppDefinition = serde_json::from_str(optional_tail).unwrap();
        let mut cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(optional_tail), &mut cx).unwrap();
        assert_eq!(
            super::super::canonical_bytes(&expected).unwrap(),
            super::super::canonical_bytes(&decoded).unwrap()
        );
    }

    #[test]
    fn malformed_unknown_duplicate_and_numeric_fields_are_rejected() {
        let source = full_app()
            .replace(r#"{"id":"filter","label":"Filter","state":"members","on_change":"save"}"#, r#"["filter","Filter","members","save"]"#)
            .replace(r#"{"id":"selected","state":"members"}"#, r#"["selected","members",null]"#);
        let missing = source.replace(",\"initial_view\":\"list\"", "");
        assert!(matches!(decode_app_definition(syntax(&missing), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("initial_view"))));
        let unknown = source.replace("\"initial_view\":\"list\"", "\"z_future\":null,\"initial_view\":\"list\"");
        let error = decode_app_definition(syntax(&unknown), &mut AllocationContext::new()).unwrap_err();
        let ValueDecodeError::UnknownField { expected, .. } = error else { panic!("expected unknown field") };
        assert_eq!(expected, APP_FIELDS);
        assert!(matches!(decode_app_definition(syntax(r#"{"version":-1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        assert!(matches!(decode_app_definition(syntax(r#"{"version":4294967296}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        let bad_variant = source.replace("\"kind\":\"form\"", "\"kind\":\"future\"");
        assert!(matches!(decode_app_definition(syntax(&bad_variant), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let bad_type = source.replace("\"version\":1", "\"version\":true");
        assert!(matches!(decode_app_definition(syntax(&bad_type), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { actual: JsonKind::Boolean, .. })));

        let mut cx = AllocationContext::new();
        let duplicate = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate AppDefinition member was accepted"),
        };
        assert_eq!(duplicate.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn duplicate_set_values_deduplicate_and_map_member_duplicates_reject() {
        let source = full_app();
        canonical(&source);
        let repeated_modifiers = vec!["\"shift\""; MAX_ITEMS + 1].join(",");
        let oversized_modifiers = source.replace(r#""modifiers":["shift","control","shift"]"#, &format!(r#""modifiers":[{repeated_modifiers}]"#));
        assert!(matches!(decode_app_definition(syntax(&oversized_modifiers), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "key modifier count", limit: MAX_ITEMS })));
        let duplicate_map = source.replace("\"name\":{\"kind\":\"text\"}", "\"name\":{\"kind\":\"text\"},\"name\":{\"kind\":\"integer\"}");
        let mut cx = AllocationContext::new();
        let error = match product_json::parse_json_bytes(duplicate_map.as_bytes(), &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate AppDefinition map member was accepted"),
        };
        assert_eq!(error.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn allocation_refusal_at_each_reached_owner_growth_cleans_and_retries() {
        let source = full_app();
        let expected = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_app_definition(syntax(&source), &mut cx).unwrap();
        let bytes = super::super::canonical_bytes(&decoded).unwrap();
        let growths = cx.growth_requests();
        assert!(growths >= 20, "expected broad nested owner coverage, got {growths}");
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = decode_app_definition(syntax(&source), &mut cx).unwrap_err();
            let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocator refusal at growth {nth}") };
            assert_eq!(Some(failure), cx.first_failure());
            assert_eq!(failure.kind, AllocationFailureKind::Injected);
            let mut retry = AllocationContext::new();
            let retried = decode_app_definition(syntax(&source), &mut retry).unwrap();
            assert_eq!(bytes, super::super::canonical_bytes(&retried).unwrap());
            assert_eq!(bytes, super::super::canonical_bytes(&expected).unwrap());
        }
    }

    #[test]
    fn collection_and_global_expression_budgets_are_enforced() {
        let entity = r#"{"id":"e","label":"E","fields":[],"unique":[],"constraints":[]}"#;
        let entities = format!("[{}]", vec![entity; MAX_ITEMS].join(","));
        let source = format!(r#"{{"version":1,"id":"a","label":"A","entities":{entities},"state":[],"actions":[],"outputs":[],"observables":[],"views":[],"initial_view":"v"}}"#);
        assert!(decode_app_definition(syntax(&source), &mut AllocationContext::new()).is_ok());
        let entities_over = format!("[{}]", vec![entity; MAX_ITEMS + 1].join(","));
        let source = format!(r#"{{"version":1,"id":"a","label":"A","entities":{entities_over},"state":[],"actions":[],"outputs":[],"observables":[],"views":[],"initial_view":"v"}}"#);
        assert!(matches!(decode_app_definition(syntax(&source), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "entity definition count", limit: MAX_ITEMS })));

        let app_with_nodes = |actions: usize| {
            let guards = vec![r#"{"kind":"today"}"#; MAX_ITEMS - 1].join(",");
            let actions = (0..actions).map(|i| format!(r#"{{"id":"a{i}","label":"A","parameters":{{}},"guards":[{guards}],"steps":[],"ensures":[]}}"#)).collect::<Vec<_>>().join(",");
            format!(r#"{{"version":1,"id":"a","label":"A","entities":[],"state":[],"actions":[{actions}],"outputs":[],"observables":[],"views":[],"initial_view":"v"}}"#)
        };
        assert!(decode_app_definition(syntax(&app_with_nodes(32)), &mut AllocationContext::new()).is_ok());
        assert!(matches!(decode_app_definition(syntax(&app_with_nodes(33)), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression node count", limit: MAX_AST_NODES })));
    }

    #[test]
    fn create_and_update_assignment_maps_are_bounded_before_value_decode() {
        let entries = (0..=MAX_ITEMS).map(|i| format!(r#""field{i}":false"#)).collect::<Vec<_>>().join(",");
        let oversized = format!("{{{entries}}}");
        let source = full_app().replace(
            r#"{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}}"#,
            &oversized,
        );
        let error = decode_app_definition(syntax(&source), &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, ValueDecodeError::LimitExceeded { resource: "assignment count", limit: MAX_ITEMS }));

        let source = full_app().replace(
            r#""parameters":{"name":{"kind":"text"}}"#,
            &format!(r#""parameters":{oversized}"#),
        );
        let error = decode_app_definition(syntax(&source), &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, ValueDecodeError::LimitExceeded { resource: "map entry count", limit: MAX_ITEMS }));

        let defaults = format!("{{{entries}}}");
        let source = full_app().replace(
            r#""defaults":{"enabled":{"kind":"boolean","value":true}}"#,
            &format!(r#""defaults":{defaults}"#),
        );
        let error = decode_app_definition(syntax(&source), &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, ValueDecodeError::LimitExceeded { resource: "value count", limit: MAX_ITEMS }));
    }

    #[test]
    fn nested_statement_depth_boundary_is_preserved() {
        let wrap = |depth: usize| {
            let mut body = r#"{"kind":"assert","condition":{"kind":"today"},"message":"ok"}"#.to_owned();
            for i in 0..depth {
                body = format!(r#"{{"kind":"for_each","items":{{"kind":"today"}},"binding":"b{i}","steps":[{body}]}}"#);
            }
            body
        };
        let app = |depth| format!(r#"{{"version":1,"id":"a","label":"A","entities":[],"state":[],"actions":[{{"id":"x","label":"X","parameters":{{}},"guards":[],"steps":[{}],"ensures":[]}}],"outputs":[],"observables":[],"views":[],"initial_view":"v"}}"#, wrap(depth));
        assert!(decode_app_definition(syntax(&app(MAX_EXPR_DEPTH - 1)), &mut AllocationContext::new()).is_ok());
        assert!(matches!(decode_app_definition(syntax(&app(MAX_EXPR_DEPTH)), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: MAX_EXPR_DEPTH })));
        assert!(matches!(decode_app_definition(syntax(&app(MAX_EXPR_DEPTH + 1)), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: MAX_EXPR_DEPTH })));
    }
}
