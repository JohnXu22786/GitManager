//! Test-first coverage for private, allocation-aware OwnedInventoryV2 decoding.
//! Runtime entrypoints remain unchanged in this unit.

use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::fallible_expr_decode::decode_expr;
use crate::product_contract::fallible_value_decode::{
    decode_data_value, decode_type, decode_values, JsonKind, NumericValue, ValueDecodeError,
};
use crate::product_contract::{
    DataValue, Digest, Expr, FallibleValues, Type, MAX_AST_NODES, MAX_COLLECTION, MAX_EXPR_DEPTH,
    MAX_ITEMS,
};
use crate::product_json::{Number, Value};
use crate::product_store::scope::fallible_owned_source_decode::{
    decode_owned_structural_report, DecodedOwnedStructuralReport,
};
use crate::product_store::scope::fallible_scalar_decode::FallibleScopeMap;
use crate::product_store::scope::owned_contract::OwnedSourceSide;
use serde::Serialize;

const INVENTORY_FIELDS: &[&str] = &[
    "structure", "certificates", "projections", "phases", "captured", "outcome_comparison",
];
const CERTIFICATE_FIELDS: &[&str] = &["side", "site", "rule", "identity"];
const PROJECTION_FIELDS: &[&str] = &[
    "location", "destination", "binding", "value_type", "host_value_type", "absent_value",
    "baseline", "candidate", "capture_required",
];
const VIEW_COLUMN_FIELDS: &[&str] = &["view", "column"];
const ROOT_COUNT_FIELDS: &[&str] = &["observable", "include_archived"];
const ENVIRONMENT_FIELDS: &[&str] = &["parameters"];
const ACTION_FIELDS: &[&str] = &[
    "action", "parameters", "guards", "ensures", "before_identity", "after_identity", "phases",
];
const PHASE_FIELDS: &[&str] = &["kind", "edit", "before", "after"];
const SPAN_FIELDS: &[&str] = &[
    "path", "len", "digest", "environment", "controls", "previous", "next",
];
const DESTINATIONS: &[&str] = &["view_column", "root_count"];
const PHASE_KINDS: &[&str] = &["common_subtree", "common_header", "variant"];
const SOURCE_SIDES: &[&str] = &["before", "after"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedInventoryV2 {
    structure: DecodedOwnedStructuralReport,
    certificates: Vec<DecodedCommonFactCertificate>,
    projections: Vec<DecodedOwnedProjectionRecipe>,
    phases: Vec<DecodedCommonFactAction>,
    captured: FallibleScopeMap<String, FallibleValues>,
    outcome_comparison: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DecodedCommonFactCertificate {
    side: OwnedSourceSide,
    site: String,
    rule: String,
    identity: Digest,
}

#[derive(Debug, Serialize)]
struct DecodedOwnedProjectionRecipe {
    location: String,
    destination: DecodedOwnedProjectionDestination,
    binding: String,
    value_type: Type,
    host_value_type: Type,
    absent_value: Option<DataValue>,
    baseline: Option<Expr>,
    candidate: Expr,
    capture_required: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DecodedOwnedProjectionDestination {
    ViewColumn { view: String, column: String },
    RootCount { observable: String, include_archived: bool },
}

#[derive(Debug, Serialize)]
struct DecodedCommonFactAction {
    action: String,
    parameters: FallibleScopeMap<String, Type>,
    guards: Vec<Expr>,
    ensures: Vec<Expr>,
    before_identity: Digest,
    after_identity: Digest,
    phases: Vec<DecodedCommonFactPhase>,
}

#[derive(Debug, Serialize)]
struct DecodedCommonFactPhase {
    kind: DecodedCommonFactPhaseKind,
    edit: Option<String>,
    before: DecodedCommonFactSpan,
    after: DecodedCommonFactSpan,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DecodedCommonFactPhaseKind {
    CommonSubtree,
    CommonHeader,
    Variant,
}

#[derive(Debug, Serialize)]
struct DecodedCommonFactSpan {
    path: Vec<usize>,
    len: usize,
    digest: Digest,
    environment: FallibleScopeMap<String, Type>,
    controls: Vec<Expr>,
    previous: Option<Digest>,
    next: Option<Digest>,
}

pub(crate) fn decode_owned_inventory_v2(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedInventoryV2, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedInventoryV2", INVENTORY_FIELDS, cx)?;
    let (mut structure, mut certificates, mut projections, mut phases, mut captured, mut outcome_comparison) =
        (None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "structure" => structure = Some(decode_owned_structural_report(value, cx)?),
            "certificates" => certificates = Some(decode_vec(
                value, MAX_COLLECTION, "owned inventory certificate count", cx, decode_certificate,
            )?),
            "projections" => projections = Some(decode_vec(
                value, MAX_AST_NODES, "owned inventory projection count", cx, decode_projection,
            )?),
            "phases" => phases = Some(decode_vec(
                value, MAX_ITEMS, "owned inventory action count", cx, decode_action,
            )?),
            "captured" => captured = Some(decode_captured(value, cx)?),
            "outcome_comparison" => outcome_comparison = Some(decode_vec(
                value, MAX_ITEMS, "owned outcome comparison count", cx, |v, _| string(v, "a string"),
            )?),
            _ => return Err(unknown(key, INVENTORY_FIELDS)),
        }
    }
    Ok(DecodedOwnedInventoryV2 {
        structure: required(structure, "structure")?,
        certificates: required(certificates, "certificates")?,
        projections: required(projections, "projections")?,
        phases: required(phases, "phases")?,
        captured: required(captured, "captured")?,
        outcome_comparison: required(outcome_comparison, "outcome_comparison")?,
    })
}

fn decode_certificate(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactCertificate, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactCertificate", CERTIFICATE_FIELDS, cx)?;
    let (mut side, mut site, mut rule, mut identity) = (None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "side" => side = Some(source_side(value)?),
            "site" => site = Some(string(value, "a string")?),
            "rule" => rule = Some(string(value, "a string")?),
            "identity" => identity = Some(digest(value)?),
            _ => return Err(unknown(key, CERTIFICATE_FIELDS)),
        }
    }
    Ok(DecodedCommonFactCertificate {
        side: required(side, "side")?, site: required(site, "site")?,
        rule: required(rule, "rule")?, identity: required(identity, "identity")?,
    })
}

fn decode_projection(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedProjectionRecipe, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedProjectionRecipe", PROJECTION_FIELDS, cx)?;
    let (mut location, mut projection_destination, mut binding, mut value_type, mut host_value_type,
        mut absent_value, mut baseline, mut candidate, mut capture_required) =
        (None, None, None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "location" => location = Some(string(value, "a string")?),
            "destination" => projection_destination = Some(destination(value, cx)?),
            "binding" => binding = Some(string(value, "a string")?),
            "value_type" => value_type = Some(decode_type(value, cx)?),
            "host_value_type" => host_value_type = Some(decode_type(value, cx)?),
            "absent_value" => absent_value = Some(optional_with_context(value, cx, decode_data_value)?),
            "baseline" => baseline = Some(optional_with_context(value, cx, decode_expr)?),
            "candidate" => candidate = Some(decode_expr(value, cx)?),
            "capture_required" => capture_required = Some(boolean(value)?),
            _ => return Err(unknown(key, PROJECTION_FIELDS)),
        }
    }
    Ok(DecodedOwnedProjectionRecipe {
        location: required(location, "location")?, destination: required(projection_destination, "destination")?,
        binding: required(binding, "binding")?, value_type: required(value_type, "value_type")?,
        host_value_type: required(host_value_type, "host_value_type")?, absent_value: absent_value.unwrap_or(None),
        baseline: baseline.unwrap_or(None), candidate: required(candidate, "candidate")?,
        capture_required: required(capture_required, "capture_required")?,
    })
}

fn destination(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedProjectionDestination, ValueDecodeError> {
    let mut fields = match value {
        Value::Object(fields) => fields,
        value => return Err(invalid_type(value, "internally tagged enum OwnedProjectionDestination")),
    };
    let tag = match fields.remove("kind") {
        Some(Value::String(tag)) => tag,
        Some(value) => return Err(invalid_type(value, "a string")),
        None => return Err(ValueDecodeError::MissingField("kind")),
    };
    match tag.as_str() {
        "view_column" => {
            let fields = struct_fields(Value::Object(fields), "struct variant OwnedProjectionDestination::ViewColumn", VIEW_COLUMN_FIELDS, cx)?;
            let (mut view, mut column) = (None, None);
            for (key, value) in fields {
                match key.as_str() {
                    "view" => view = Some(string(value, "a string")?),
                    "column" => column = Some(string(value, "a string")?),
                    _ => return Err(unknown(key, VIEW_COLUMN_FIELDS)),
                }
            }
            Ok(DecodedOwnedProjectionDestination::ViewColumn {
                view: required(view, "view")?, column: required(column, "column")?,
            })
        }
        "root_count" => {
            let fields = struct_fields(Value::Object(fields), "struct variant OwnedProjectionDestination::RootCount", ROOT_COUNT_FIELDS, cx)?;
            let (mut observable, mut include_archived) = (None, None);
            for (key, value) in fields {
                match key.as_str() {
                    "observable" => observable = Some(string(value, "a string")?),
                    "include_archived" => include_archived = Some(boolean(value)?),
                    _ => return Err(unknown(key, ROOT_COUNT_FIELDS)),
                }
            }
            Ok(DecodedOwnedProjectionDestination::RootCount {
                observable: required(observable, "observable")?,
                include_archived: required(include_archived, "include_archived")?,
            })
        }
        _ => Err(ValueDecodeError::UnknownVariant { tag, expected: DESTINATIONS }),
    }
}

fn decode_action(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactAction, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactAction", ACTION_FIELDS, cx)?;
    let (mut action, mut parameters, mut guards, mut ensures, mut before_identity, mut after_identity, mut phases) =
        (None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "action" => action = Some(string(value, "a string")?),
            "parameters" => parameters = Some(environment(value, cx)?),
            "guards" => guards = Some(expressions(value, MAX_ITEMS, "common fact action guard count", cx)?),
            "ensures" => ensures = Some(expressions(value, MAX_ITEMS, "common fact action ensure count", cx)?),
            "before_identity" => before_identity = Some(digest(value)?),
            "after_identity" => after_identity = Some(digest(value)?),
            "phases" => phases = Some(decode_vec(
                value, MAX_AST_NODES, "common fact action phase count", cx, decode_phase,
            )?),
            _ => return Err(unknown(key, ACTION_FIELDS)),
        }
    }
    Ok(DecodedCommonFactAction {
        action: required(action, "action")?, parameters: required(parameters, "parameters")?,
        guards: required(guards, "guards")?, ensures: required(ensures, "ensures")?,
        before_identity: required(before_identity, "before_identity")?,
        after_identity: required(after_identity, "after_identity")?, phases: required(phases, "phases")?,
    })
}

fn decode_phase(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactPhase, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactPhase", PHASE_FIELDS, cx)?;
    let (mut kind, mut edit, mut before, mut after) = (None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "kind" => kind = Some(phase_kind(value)?),
            "edit" => edit = Some(optional(value, |v| string(v, "a string"))?),
            "before" => before = Some(decode_span(value, cx)?),
            "after" => after = Some(decode_span(value, cx)?),
            _ => return Err(unknown(key, PHASE_FIELDS)),
        }
    }
    Ok(DecodedCommonFactPhase {
        kind: required(kind, "kind")?, edit: edit.unwrap_or(None),
        before: required(before, "before")?, after: required(after, "after")?,
    })
}

fn phase_kind(value: Value) -> Result<DecodedCommonFactPhaseKind, ValueDecodeError> {
    match unit_variant(value, PHASE_KINDS)? {
        "common_subtree" => Ok(DecodedCommonFactPhaseKind::CommonSubtree),
        "common_header" => Ok(DecodedCommonFactPhaseKind::CommonHeader),
        "variant" => Ok(DecodedCommonFactPhaseKind::Variant),
        _ => unreachable!("variant is checked against PHASE_KINDS"),
    }
}

fn source_side(value: Value) -> Result<OwnedSourceSide, ValueDecodeError> {
    match unit_variant(value, SOURCE_SIDES)? {
        "before" => Ok(OwnedSourceSide::Before),
        "after" => Ok(OwnedSourceSide::After),
        _ => unreachable!("variant is checked against SOURCE_SIDES"),
    }
}

fn decode_span(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactSpan, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactSpan", SPAN_FIELDS, cx)?;
    let (mut path, mut len, mut digest_value, mut env, mut controls, mut previous, mut next) =
        (None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "path" => path = Some(usize_path(value, MAX_EXPR_DEPTH + 1, "common fact span path length", cx)?),
            "len" => {
                let value = usize_value(value)?;
                if value > MAX_AST_NODES { return Err(limit("common fact span length", MAX_AST_NODES)); }
                len = Some(value);
            }
            "digest" => digest_value = Some(digest(value)?),
            "environment" => env = Some(environment(value, cx)?),
            "controls" => controls = Some(expressions(value, MAX_ITEMS, "common fact span control count", cx)?),
            "previous" => previous = Some(optional(value, digest)?),
            "next" => next = Some(optional(value, digest)?),
            _ => return Err(unknown(key, SPAN_FIELDS)),
        }
    }
    Ok(DecodedCommonFactSpan {
        path: required(path, "path")?, len: required(len, "len")?,
        digest: required(digest_value, "digest")?, environment: required(env, "environment")?,
        controls: required(controls, "controls")?, previous: previous.unwrap_or(None), next: next.unwrap_or(None),
    })
}

fn environment(value: Value, cx: &mut AllocationContext) -> Result<FallibleScopeMap<String, Type>, ValueDecodeError> {
    decode_map(value, MAX_ITEMS, "common fact environment binding count", cx, |value, cx| decode_type(value, cx))
}

fn decode_captured(value: Value, cx: &mut AllocationContext) -> Result<FallibleScopeMap<String, FallibleValues>, ValueDecodeError> {
    decode_map(value, MAX_COLLECTION, "owned inventory captured row count", cx, |value, cx| decode_values(value, cx))
}

fn decode_map<T>(
    value: Value,
    max: usize,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<FallibleScopeMap<String, T>, ValueDecodeError> {
    let entries = match value {
        Value::Object(entries) => entries,
        value => return Err(invalid_type(value, "a map")),
    };
    bounded(entries.len(), max, resource)?;
    let mut output = FallibleScopeMap::new();
    for (key, value) in entries.into_entries() {
        let value = decode(value, cx)?;
        output.insert(key, value, cx)?;
    }
    Ok(output)
}

fn expressions(value: Value, max: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<Expr>, ValueDecodeError> {
    decode_vec(value, max, resource, cx, decode_expr)
}

fn decode_vec<T>(
    value: Value,
    max: usize,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    bounded(values.len(), max, resource)?;
    let mut output = Vec::new();
    product_alloc::try_reserve(&mut output, values.len(), cx, AllocationSite::Vector)?;
    for value in values { output.push(decode(value, cx)?); }
    Ok(output)
}

fn struct_fields(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
    cx: &mut AllocationContext,
) -> Result<std::vec::IntoIter<(String, Value)>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields.into_entries()),
        Value::Array(values) => {
            if values.len() != names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let mut fields = Vec::new();
            product_alloc::try_reserve(&mut fields, names.len(), cx, AllocationSite::Vector)?;
            for (name, value) in names.iter().zip(values) {
                fields.push((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value));
            }
            Ok(fields.into_iter())
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn unit_variant(value: Value, expected: &'static [&'static str]) -> Result<&'static str, ValueDecodeError> {
    match value {
        Value::String(tag) => variant(tag, expected),
        Value::Object(mut fields) => {
            if fields.len() != 1 { return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 }); }
            let (tag, payload) = fields.into_entries().next().expect("one unit enum entry");
            let tag = variant(tag, expected)?;
            if !matches!(payload, Value::Null) { return Err(invalid_type(payload, "unit")); }
            Ok(tag)
        }
        value => Err(invalid_type(value, "a string variant identifier or map")),
    }
}

fn variant(tag: String, expected: &'static [&'static str]) -> Result<&'static str, ValueDecodeError> {
    expected.iter().copied().find(|candidate| *candidate == tag)
        .ok_or(ValueDecodeError::UnknownVariant { tag, expected })
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let text = string(value, "a string")?;
    Digest::from_owned_lower_hex(text).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}

fn usize_path(value: Value, max_len: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<usize>, ValueDecodeError> {
    let path = decode_vec(value, max_len, resource, cx, |value, _| usize_value(value))?;
    if path.iter().any(|index| *index >= MAX_AST_NODES) {
        return Err(limit("common fact span path index", MAX_AST_NODES));
    }
    Ok(path)
}

fn usize_value(value: Value) -> Result<usize, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "usize" }),
        Value::Number(Number::I64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "usize" }),
        value => Err(invalid_type(value, "usize")),
    }
}

fn boolean(value: Value) -> Result<bool, ValueDecodeError> {
    match value { Value::Bool(value) => Ok(value), value => Err(invalid_type(value, "a boolean")) }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
}

fn optional<T>(value: Value, decode: impl FnOnce(Value) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value).map(Some) }
}

fn optional_with_context<T>(
    value: Value,
    cx: &mut AllocationContext,
    decode: impl FnOnce(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value, cx).map(Some) }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn bounded(len: usize, max: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > max { Err(limit(resource, max)) } else { Ok(()) }
}

fn limit(resource: &'static str, max: usize) -> ValueDecodeError {
    ValueDecodeError::LimitExceeded { resource, limit: max }
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
    use super::{decode_owned_inventory_v2, DecodedOwnedInventoryV2};
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::{canonical_bytes, Digest, MAX_AST_NODES, MAX_COLLECTION, MAX_ITEMS};
    use crate::product_contract::fallible_value_decode::ValueDecodeError;
    use crate::product_alloc::ordered::OrderedMap;
    use crate::product_json::{self, Value};
    use crate::product_store::scope::fallible_scalar_decode::FallibleScopeMap;
    use crate::product_store::scope::owned_wire::OwnedInventoryV2;
    use serde::Serialize;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn digest(c: char) -> String { std::iter::repeat(c).take(64).collect() }
    fn literal(value: bool) -> serde_json::Value {
        serde_json::json!({
            "kind":"literal",
            "value_type":{"kind":"boolean"},
            "value":{"kind":"boolean","value":value}
        })
    }
    fn span(seed: char, index: usize) -> serde_json::Value {
        serde_json::json!({
            "path":[index], "len":1, "digest":digest(seed),
            "environment":{"record":{"kind":"reference","entity":"person"},"title":{"kind":"text"}},
            "controls":[literal(index % 2 == 0)], "previous":null, "next":digest(seed)
        })
    }
    fn phase(kind: serde_json::Value, index: usize) -> serde_json::Value {
        serde_json::json!({
            "kind":kind, "edit":null,
            "before":span(char::from_u32('a' as u32 + index as u32).unwrap(),index),
            "after":span(char::from_u32('d' as u32 + index as u32).unwrap(),index+1)
        })
    }
    fn fixture() -> serde_json::Value {
        serde_json::json!({
            "structure":{
                "before":digest('1'),"after":digest('2'),"root_entity":"person",
                "edits":[],"common_statements":[],"writers":[],"changes":[]
            },
            "certificates":[
                {"side":"before","site":"action:make/0","rule":"same-entity","identity":digest('3')},
                {"side":{"after":null},"site":"action:make/1","rule":"retained-field","identity":digest('4')}
            ],
            "projections":[
                {
                    "location":"action:make/0/value","destination":{"kind":"view_column","view":"summary","column":"title"},
                    "binding":"title","value_type":{"kind":"text"},"host_value_type":{"kind":"text"},
                    "absent_value":null,"baseline":null,"candidate":literal(true),"capture_required":true
                },
                {
                    "location":"action:count/0","destination":{"kind":"root_count","observable":"total","include_archived":false},
                    "binding":"count","value_type":{"kind":"integer"},"host_value_type":{"kind":"integer"},
                    "absent_value":{"kind":"integer","value":0},"baseline":literal(false),"candidate":literal(true),"capture_required":false
                }
            ],
            "phases":[
                {
                    "action":"make","parameters":{"record":{"kind":"reference","entity":"person"}},
                    "guards":[literal(true)],"ensures":[],"before_identity":digest('5'),"after_identity":digest('6'),
                    "phases":[phase(serde_json::json!("common_subtree"),0),phase(serde_json::json!({"common_header":null}),1),phase(serde_json::json!("variant"),2)]
                }
            ],
            "captured":{
                "record_b":{"title":{"kind":"text","value":"Bee"},"active":{"kind":"boolean","value":true}},
                "record_a":{"tags":{"kind":"list","item_type":{"kind":"text"},"items":[{"kind":"text","value":"one"},{"kind":"text","value":"two"}]}}
            },
            "outcome_comparison":["all outcomes equivalent","no archived rows changed"]
        })
    }
    fn decode(value: serde_json::Value, cx: &mut AllocationContext) -> Result<DecodedOwnedInventoryV2, ValueDecodeError> {
        decode_owned_inventory_v2(syntax(&serde_json::to_string(&value).unwrap()), cx)
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn complete_owned_inventory_graph_matches_serde_canonical_bytes() {
        let value = fixture();
        let expected: OwnedInventoryV2 = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode(value, &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        assert_eq!(decoded.captured.keys().map(String::as_str).collect::<Vec<_>>(), ["record_a", "record_b"]);
        assert_eq!(decoded.phases[0].phases.len(), 3);
    }

    #[test]
    fn inventory_sequence_defaults_sorted_maps_and_duplicate_replacement_match_serde() {
        let mut value = fixture();
        value["projections"][0].as_object_mut().unwrap().remove("baseline");
        value["projections"][0].as_object_mut().unwrap().remove("absent_value");
        value["phases"][0]["phases"][0].as_object_mut().unwrap().remove("edit");
        value["phases"][0]["phases"][0]["before"].as_object_mut().unwrap().remove("previous");
        value["phases"][0]["phases"][0]["after"].as_object_mut().unwrap().remove("next");
        let expected: OwnedInventoryV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode(value.clone(), &mut AllocationContext::new()).unwrap()));

        let sequence = serde_json::json!([value["structure"], value["certificates"], value["projections"], value["phases"], value["captured"], value["outcome_comparison"]]);
        let expected_sequence: OwnedInventoryV2 = serde_json::from_value(sequence.clone()).unwrap();
        assert_eq!(canonical(&expected_sequence), canonical(&decode(sequence, &mut AllocationContext::new()).unwrap()));

        let mut map = FallibleScopeMap::<String, Digest>::new();
        let mut cx = AllocationContext::new();
        let first = Digest::from_owned_lower_hex(digest('7')).unwrap();
        let last = Digest::from_owned_lower_hex(digest('8')).unwrap();
        map.insert("same".into(), first, &mut cx).unwrap();
        map.insert("same".into(), last.clone(), &mut cx).unwrap();
        let expected_map = std::collections::BTreeMap::from([("same".to_string(), last)]);
        assert_eq!(canonical(&expected_map), canonical(&map));
    }

    #[test]
    fn inventory_malformed_fields_variants_and_numeric_ranges_are_rejected() {
        let mut value = fixture();
        value.as_object_mut().unwrap().insert("future".into(), serde_json::json!(true));
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let mut value = fixture();
        value.as_object_mut().unwrap().remove("captured");
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("captured"))));

        let mut value = fixture();
        value["certificates"][0]["side"] = serde_json::json!("middle");
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));

        let mut value = fixture();
        value["phases"][0]["phases"][0]["before"]["len"] = serde_json::json!(MAX_AST_NODES + 1);
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));

        let mut value = fixture();
        value["projections"][0]["destination"]["kind"] = serde_json::json!("future_destination");
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));

        let mut value = fixture();
        value["projections"][0]["capture_required"] = serde_json::json!("yes");
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { .. })));

        let mut value = fixture();
        value["certificates"][0]["identity"] = serde_json::json!("not-a-digest");
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));

        let mut value = fixture();
        value["phases"][0]["phases"][0]["before"]["unexpected"] = serde_json::json!(0);
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        assert!(product_json::parse_json_bytes(br#"{"structure":null,"structure":null}"#, &mut AllocationContext::new()).is_err());
    }

    #[test]
    fn inventory_collection_and_path_limits_are_checked_before_growth() {
        let mut object = OrderedMap::new();
        let mut build_cx = AllocationContext::new();
        object.try_insert(
            "certificates".to_string(),
            Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_COLLECTION + 1).collect()),
            &mut build_cx,
        ).unwrap();
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_inventory_v2(Value::Object(object), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "owned inventory certificate count", limit: MAX_COLLECTION })));
        assert_eq!(cx.growth_requests(), 0);

        let mut value = fixture();
        value["phases"][0]["phases"][0]["before"]["path"] = serde_json::Value::Array(std::iter::repeat(serde_json::json!(0)).take(crate::product_contract::MAX_EXPR_DEPTH + 2).collect());
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));

        let mut value = fixture();
        value["phases"][0]["phases"][0]["before"]["path"][0] = serde_json::json!(MAX_AST_NODES);
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));

        let mut nested = literal(true);
        for _ in 0..=crate::product_contract::MAX_EXPR_DEPTH {
            nested = serde_json::json!({"kind":"not","value":nested});
        }
        let mut value = fixture();
        value["projections"][0]["candidate"] = nested;
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", .. })));

        let mut value = fixture();
        value["captured"] = serde_json::json!({});
        for index in 0..=MAX_COLLECTION { value["captured"][format!("r{index:05}")] = serde_json::json!({}); }
        assert!(matches!(decode(value, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "owned inventory captured row count", limit: MAX_COLLECTION })));

        let too_many = format!(r#"{{"structure":{{"before":"{}","after":"{}","root_entity":"person","edits":[],"common_statements":[],"writers":[],"changes":[]}},"certificates":[],"projections":[],"phases":[],"captured":{{}},"outcome_comparison":[{}]}}"#,
            digest('1'),digest('2'),std::iter::repeat("\"x\"").take(MAX_ITEMS+1).collect::<Vec<_>>().join(","));
        assert!(matches!(decode_owned_inventory_v2(syntax(&too_many), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "owned outcome comparison count", limit: MAX_ITEMS })));
    }

    #[test]
    fn every_owned_inventory_growth_refusal_cleans_up_and_allows_retry() {
        let value = fixture();
        let source = serde_json::to_string(&value).unwrap();
        let mut count = AllocationContext::new();
        decode_owned_inventory_v2(syntax(&source), &mut count).unwrap();
        let growths = count.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode_owned_inventory_v2(syntax(&source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retried = decode_owned_inventory_v2(syntax(&source), &mut AllocationContext::new()).unwrap();
            let model: OwnedInventoryV2 = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(canonical(&model), canonical(&retried));
        }
    }
}
