//! Fallible DTO decoders for owned source request and structural reports.
//! These helpers only decode bounded syntax trees and do not change production routing.
use crate::product_contract::fallible_value_decode::{decode_type, JsonKind, NumericValue, ValueDecodeError};
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::{Digest, Type, MAX_AST_NODES, MAX_COLLECTION, MAX_EXPR_DEPTH, MAX_ITEMS};
use crate::product_json::{Number, Value};
use serde::Serialize;
use super::owned_contract::{OwnedSourceSide, OwnedStatementKind, OwnedStructuralChangeKind};

const SOURCE_REQUEST_FIELDS: &[&str] = &["before", "after", "root_entity", "edits"];
const SPAN_EDIT_FIELDS: &[&str] = &["id", "before", "after"];
const ROOT_BINDING_FIELDS: &[&str] = &["name", "value_type"];
const SPAN_ENDPOINT_FIELDS: &[&str] = &[
    "source", "action", "list_path", "start", "len", "kinds", "digest", "previous",
    "next", "root",
];
const STRUCTURAL_CHANGE_FIELDS: &[&str] = &[
    "kind", "container", "id", "before", "after", "before_type", "after_type",
];
const COMMON_STATEMENT_FIELDS: &[&str] = &["action", "before", "after", "kind"];
const WRITER_FIELDS: &[&str] = &[
    "side", "source", "action", "path", "kind", "entity", "field", "value_type",
];
const STRUCTURAL_REPORT_FIELDS: &[&str] = &[
    "before", "after", "root_entity", "edits", "common_statements", "writers", "changes",
];
const STATEMENT_KINDS: &[&str] = &[
    "create", "update", "archive", "set_state", "collection", "for_each", "assert", "emit",
];
const SOURCE_SIDES: &[&str] = &["before", "after"];
const CHANGE_KINDS: &[&str] = &[
    "application_label_changed", "entity_added", "column_added", "column_changed",
    "observable_added", "observable_changed",
];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedSourceRequest {
    pub(super) before: Digest,
    pub(super) after: Digest,
    pub(super) root_entity: String,
    pub(super) edits: Vec<DecodedOwnedSpanEdit>,
}
impl DecodedOwnedSourceRequest {
    pub(crate) fn before(&self) -> &Digest { &self.before }
    pub(crate) fn after(&self) -> &Digest { &self.after }
    pub(crate) fn root_entity(&self) -> &str { &self.root_entity }
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedSpanEdit {
    pub(super) id: String,
    pub(super) before: DecodedOwnedSpanEndpoint,
    pub(super) after: DecodedOwnedSpanEndpoint,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedRootBinding {
    pub(super) name: String,
    pub(super) value_type: Type,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedSpanEndpoint {
    pub(super) source: Digest,
    pub(super) action: String,
    pub(super) list_path: Vec<usize>,
    pub(super) start: usize,
    pub(super) len: usize,
    pub(super) kinds: Vec<OwnedStatementKind>,
    pub(super) digest: Digest,
    pub(super) previous: Option<Digest>,
    pub(super) next: Option<Digest>,
    pub(super) root: Option<DecodedOwnedRootBinding>,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedStructuralChange {
    pub(super) kind: OwnedStructuralChangeKind,
    pub(super) container: Option<String>,
    pub(super) id: String,
    pub(super) before: Option<Digest>,
    pub(super) after: Digest,
    pub(super) before_type: Option<Type>,
    pub(super) after_type: Option<Type>,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedCommonStatement {
    pub(super) action: String,
    pub(super) before: Vec<usize>,
    pub(super) after: Vec<usize>,
    pub(super) kind: OwnedStatementKind,
}
#[derive(Debug, Serialize)]
pub(super) struct DecodedOwnedWriter {
    pub(super) side: OwnedSourceSide,
    pub(super) source: Digest,
    pub(super) action: String,
    pub(super) path: Vec<usize>,
    pub(super) kind: OwnedStatementKind,
    pub(super) entity: String,
    pub(super) field: Option<String>,
    pub(super) value_type: Option<Type>,
}
#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedStructuralReport {
    pub(super) before: Digest,
    pub(super) after: Digest,
    pub(super) root_entity: String,
    pub(super) edits: Vec<DecodedOwnedSpanEdit>,
    pub(super) common_statements: Vec<DecodedOwnedCommonStatement>,
    pub(super) writers: Vec<DecodedOwnedWriter>,
    pub(super) changes: Vec<DecodedOwnedStructuralChange>,
}

pub(crate) fn decode_owned_source_request(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedSourceRequest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedSourceRequest", SOURCE_REQUEST_FIELDS, cx)?;
    let (mut before, mut after, mut root_entity, mut edits) = (None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "before" => before = Some(digest_value(value)?),
            "after" => after = Some(digest_value(value)?),
            "root_entity" => root_entity = Some(string(value, "a string")?),
            "edits" => edits = Some(decode_vec(value, MAX_ITEMS, "owned source edit count", cx, decode_owned_span_edit)?),
            _ => return Err(unknown(key, SOURCE_REQUEST_FIELDS)),
        }
    }
    Ok(DecodedOwnedSourceRequest {
        before: required(before, "before")?, after: required(after, "after")?,
        root_entity: required(root_entity, "root_entity")?, edits: required(edits, "edits")?,
    })
}

fn decode_owned_span_edit(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedSpanEdit, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedSpanEdit", SPAN_EDIT_FIELDS, cx)?;
    let (mut id, mut before, mut after) = (None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "before" => before = Some(decode_owned_span_endpoint(value, cx)?),
            "after" => after = Some(decode_owned_span_endpoint(value, cx)?),
            _ => return Err(unknown(key, SPAN_EDIT_FIELDS)),
        }
    }
    Ok(DecodedOwnedSpanEdit { id: required(id, "id")?, before: required(before, "before")?, after: required(after, "after")? })
}

fn decode_owned_root_binding(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedRootBinding, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedRootBinding", ROOT_BINDING_FIELDS, cx)?;
    let (mut name, mut value_type) = (None, None);
    for (key, value) in fields {
        match key.as_str() {
            "name" => name = Some(string(value, "a string")?),
            "value_type" => value_type = Some(decode_type(value, cx)?),
            _ => return Err(unknown(key, ROOT_BINDING_FIELDS)),
        }
    }
    Ok(DecodedOwnedRootBinding { name: required(name, "name")?, value_type: required(value_type, "value_type")? })
}

fn decode_owned_span_endpoint(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedSpanEndpoint, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedSpanEndpoint", SPAN_ENDPOINT_FIELDS, cx)?;
    let (mut source, mut action, mut list_path, mut start, mut len, mut kinds, mut digest, mut previous, mut next, mut root) =
        (None, None, None, None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "source" => source = Some(digest_value(value)?),
            "action" => action = Some(string(value, "a string")?),
            "list_path" => list_path = Some(usize_path(value, MAX_EXPR_DEPTH, "owned span list path length", cx)?),
            "start" => start = Some(usize_value(value)?),
            "len" => len = Some(usize_value(value)?),
            "kinds" => kinds = Some(decode_vec(value, MAX_ITEMS, "owned span statement kind count", cx, |v, _| decode_owned_statement_kind(v))?),
            "digest" => digest = Some(digest_value(value)?),
            "previous" => previous = Some(optional(value, digest_value)?),
            "next" => next = Some(optional(value, digest_value)?),
            "root" => root = Some(optional_with_context(value, cx, decode_owned_root_binding)?),
            _ => return Err(unknown(key, SPAN_ENDPOINT_FIELDS)),
        }
    }
    let start = required(start, "start")?;
    let len = required(len, "len")?;
    if start > MAX_AST_NODES {
        return Err(limit("owned span start", MAX_AST_NODES));
    }
    bounded(len, MAX_ITEMS, "owned span length")?;
    if start.checked_add(len).filter(|end| *end <= MAX_AST_NODES).is_none() {
        return Err(limit("owned span end", MAX_AST_NODES));
    }
    Ok(DecodedOwnedSpanEndpoint {
        source: required(source, "source")?, action: required(action, "action")?,
        list_path: required(list_path, "list_path")?, start, len,
        kinds: required(kinds, "kinds")?, digest: required(digest, "digest")?,
        previous: previous.unwrap_or(None), next: next.unwrap_or(None), root: root.unwrap_or(None),
    })
}

fn decode_owned_statement_kind(value: Value) -> Result<OwnedStatementKind, ValueDecodeError> {
    match variant(value, STATEMENT_KINDS)? {
        "create" => Ok(OwnedStatementKind::Create), "update" => Ok(OwnedStatementKind::Update),
        "archive" => Ok(OwnedStatementKind::Archive), "set_state" => Ok(OwnedStatementKind::SetState),
        "collection" => Ok(OwnedStatementKind::Collection), "for_each" => Ok(OwnedStatementKind::ForEach),
        "assert" => Ok(OwnedStatementKind::Assert), "emit" => Ok(OwnedStatementKind::Emit),
        _ => unreachable!("variant is checked against STATEMENT_KINDS"),
    }
}
fn decode_owned_source_side(value: Value) -> Result<OwnedSourceSide, ValueDecodeError> {
    match variant(value, SOURCE_SIDES)? {
        "before" => Ok(OwnedSourceSide::Before), "after" => Ok(OwnedSourceSide::After),
        _ => unreachable!("variant is checked against SOURCE_SIDES"),
    }
}
fn decode_owned_structural_change_kind(value: Value) -> Result<OwnedStructuralChangeKind, ValueDecodeError> {
    match variant(value, CHANGE_KINDS)? {
        "application_label_changed" => Ok(OwnedStructuralChangeKind::ApplicationLabelChanged),
        "entity_added" => Ok(OwnedStructuralChangeKind::EntityAdded),
        "column_added" => Ok(OwnedStructuralChangeKind::ColumnAdded),
        "column_changed" => Ok(OwnedStructuralChangeKind::ColumnChanged),
        "observable_added" => Ok(OwnedStructuralChangeKind::ObservableAdded),
        "observable_changed" => Ok(OwnedStructuralChangeKind::ObservableChanged),
        _ => unreachable!("variant is checked against CHANGE_KINDS"),
    }
}

fn decode_owned_structural_change(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedStructuralChange, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedStructuralChange", STRUCTURAL_CHANGE_FIELDS, cx)?;
    let (mut kind, mut container, mut id, mut before, mut after, mut before_type, mut after_type) =
        (None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "kind" => kind = Some(decode_owned_structural_change_kind(value)?),
            "container" => container = Some(optional(value, |v| string(v, "a string"))?),
            "id" => id = Some(string(value, "a string")?),
            "before" => before = Some(optional(value, digest_value)?),
            "after" => after = Some(digest_value(value)?),
            "before_type" => before_type = Some(optional_with_context(value, cx, decode_type)?),
            "after_type" => after_type = Some(optional_with_context(value, cx, decode_type)?),
            _ => return Err(unknown(key, STRUCTURAL_CHANGE_FIELDS)),
        }
    }
    Ok(DecodedOwnedStructuralChange {
        kind: required(kind, "kind")?, container: container.unwrap_or(None), id: required(id, "id")?,
        before: before.unwrap_or(None), after: required(after, "after")?,
        before_type: before_type.unwrap_or(None), after_type: after_type.unwrap_or(None),
    })
}

fn decode_owned_common_statement(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedCommonStatement, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedCommonStatement", COMMON_STATEMENT_FIELDS, cx)?;
    let (mut action, mut before, mut after, mut kind) = (None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "action" => action = Some(string(value, "a string")?),
            "before" => before = Some(usize_path(value, MAX_EXPR_DEPTH + 1, "owned common statement path length", cx)?),
            "after" => after = Some(usize_path(value, MAX_EXPR_DEPTH + 1, "owned common statement path length", cx)?),
            "kind" => kind = Some(decode_owned_statement_kind(value)?),
            _ => return Err(unknown(key, COMMON_STATEMENT_FIELDS)),
        }
    }
    Ok(DecodedOwnedCommonStatement { action: required(action, "action")?, before: required(before, "before")?, after: required(after, "after")?, kind: required(kind, "kind")? })
}

fn decode_owned_writer(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedWriter, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedWriter", WRITER_FIELDS, cx)?;
    let (mut side, mut source, mut action, mut path, mut kind, mut entity, mut field, mut value_type) =
        (None, None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "side" => side = Some(decode_owned_source_side(value)?),
            "source" => source = Some(digest_value(value)?),
            "action" => action = Some(string(value, "a string")?),
            "path" => path = Some(usize_path(value, MAX_EXPR_DEPTH + 1, "owned writer path length", cx)?),
            "kind" => kind = Some(decode_owned_statement_kind(value)?),
            "entity" => entity = Some(string(value, "a string")?),
            "field" => field = Some(optional(value, |v| string(v, "a string"))?),
            "value_type" => value_type = Some(optional_with_context(value, cx, decode_type)?),
            _ => return Err(unknown(key, WRITER_FIELDS)),
        }
    }
    Ok(DecodedOwnedWriter {
        side: required(side, "side")?, source: required(source, "source")?, action: required(action, "action")?,
        path: required(path, "path")?, kind: required(kind, "kind")?, entity: required(entity, "entity")?,
        field: field.unwrap_or(None), value_type: value_type.unwrap_or(None),
    })
}

pub(crate) fn decode_owned_structural_report(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedStructuralReport, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedStructuralReport", STRUCTURAL_REPORT_FIELDS, cx)?;
    let (mut before, mut after, mut root_entity, mut edits, mut common_statements, mut writers, mut changes) =
        (None, None, None, None, None, None, None);
    for (key, value) in fields {
        match key.as_str() {
            "before" => before = Some(digest_value(value)?),
            "after" => after = Some(digest_value(value)?),
            "root_entity" => root_entity = Some(string(value, "a string")?),
            "edits" => edits = Some(decode_vec(value, MAX_ITEMS, "owned structural edit count", cx, decode_owned_span_edit)?),
            "common_statements" => common_statements = Some(decode_vec(value, MAX_AST_NODES, "owned common statement count", cx, decode_owned_common_statement)?),
            "writers" => writers = Some(decode_vec(value, MAX_COLLECTION, "owned writer count", cx, decode_owned_writer)?),
            "changes" => changes = Some(decode_vec(value, MAX_COLLECTION, "owned structural change count", cx, decode_owned_structural_change)?),
            _ => return Err(unknown(key, STRUCTURAL_REPORT_FIELDS)),
        }
    }
    Ok(DecodedOwnedStructuralReport {
        before: required(before, "before")?, after: required(after, "after")?,
        root_entity: required(root_entity, "root_entity")?, edits: required(edits, "edits")?,
        common_statements: required(common_statements, "common_statements")?,
        writers: required(writers, "writers")?, changes: required(changes, "changes")?,
    })
}

fn decode_vec<T>(value: Value, limit: usize, resource: &'static str, cx: &mut AllocationContext, mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value { Value::Array(values) => values, value => return Err(invalid_type(value, "a sequence")) };
    bounded(values.len(), limit, resource)?;
    let mut out = Vec::new();
    product_alloc::try_reserve(&mut out, values.len(), cx, AllocationSite::Vector)?;
    for value in values { out.push(decode(value, cx)?); }
    Ok(out)
}
fn usize_path(value: Value, max_len: usize, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<usize>, ValueDecodeError> {
    let path = decode_vec(value, max_len, resource, cx, |value, _| usize_value(value))?;
    if path.iter().any(|index| *index >= MAX_AST_NODES) {
        return Err(limit("owned source path index", MAX_AST_NODES));
    }
    Ok(path)
}
fn optional<T>(value: Value, decode: impl FnOnce(Value) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value).map(Some) }
}
fn optional_with_context<T>(value: Value, cx: &mut AllocationContext, decode: impl FnOnce(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Option<T>, ValueDecodeError> {
    match value { Value::Null => Ok(None), value => decode(value, cx).map(Some) }
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<std::vec::IntoIter<(String, Value)>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields.into_entries()),
        Value::Array(values) => {
            if values.len() != names.len() { return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() }); }
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
fn variant(value: Value, expected: &'static [&'static str]) -> Result<&'static str, ValueDecodeError> {
    let name = match value { Value::String(name) => name, value => return Err(invalid_type(value, "a string variant identifier")) };
    expected.iter().copied().find(|candidate| *candidate == name).ok_or(ValueDecodeError::UnknownVariant { tag: name, expected })
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) }
}
fn digest_value(value: Value) -> Result<Digest, ValueDecodeError> {
    Digest::from_owned_lower_hex(string(value, "a string")?).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}
fn usize_value(value: Value) -> Result<usize, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "usize" }),
        Value::Number(Number::I64(value)) => usize::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "usize" }),
        value => Err(invalid_type(value, "usize")),
    }
}
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null, Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String, Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}
fn bounded(len: usize, max: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > max { Err(limit(resource, max)) } else { Ok(()) }
}
fn limit(resource: &'static str, limit: usize) -> ValueDecodeError {
    ValueDecodeError::LimitExceeded { resource, limit }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::canonical_bytes;
    use crate::product_json;
    use crate::product_store::scope::owned_contract::{
        OwnedCommonStatement, OwnedRootBinding, OwnedSourceRequest, OwnedSourceSide,
        OwnedSpanEdit, OwnedSpanEndpoint, OwnedStatementKind, OwnedStructuralChange,
        OwnedStructuralChangeKind, OwnedStructuralReport, OwnedWriter,
    };
    use std::num::NonZeroUsize;
    use std::result::Result;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }
    fn d(byte: char) -> String { std::iter::repeat(byte).take(64).collect() }

    fn endpoint(source: &str, side: &str, start: usize, len: usize) -> serde_json::Value {
        let kinds = ["create", "update"].into_iter().take(len).collect::<Vec<_>>();
        serde_json::json!({
            "source": source,
            "action": "save",
            "list_path": [1, 0],
            "start": start,
            "len": len,
            "kinds": kinds,
            "digest": d(if side == "before" { 'a' } else { 'b' }),
            "previous": null,
            "next": d(if side == "before" { 'c' } else { 'd' }),
            "root": {"name":"row", "value_type":{"kind":"reference", "entity":"person"}}
        })
    }

    fn graph_value() -> serde_json::Value {
        let before = d('1');
        let after = d('2');
        serde_json::json!({
            "request": {
                "before": before,
                "after": after,
                "root_entity": "person",
                "edits": [{
                    "id":"edit_1",
                    "before":endpoint(&before, "before", 3, 2),
                    "after":endpoint(&after, "after", 4, 2)
                }]
            },
            "report": {
                "before":before,
                "after":after,
                "root_entity":"person",
                "edits":[{
                    "id":"edit_1",
                    "before":endpoint(&before, "before", 3, 2),
                    "after":endpoint(&after, "after", 4, 2)
                }],
                "common_statements":[
                    {"action":"save","before":[0,1],"after":[2,3],"kind":"for_each"},
                    {"action":"save","before":[0,1,2],"after":[2,3,4],"kind":"assert"}
                ],
                "writers":[
                    {"side":"before","source":before,"action":"save","path":[0,1],"kind":"update","entity":"person","field":"name","value_type":{"kind":"text"}},
                    {"side":"after","source":after,"action":"save","path":[2,3],"kind":"archive","entity":"person","field":null,"value_type":null}
                ],
                "changes":[
                    {"kind":"application_label_changed","container":null,"id":"app","before":before,"after":after,"before_type":null,"after_type":null},
                    {"kind":"column_changed","container":"list","id":"name","before":null,"after":after,"before_type":{"kind":"optional","item":{"kind":"text"}},"after_type":{"kind":"text"}}
                ]
            }
        })
    }

    #[test]
    fn owned_source_request_and_nested_span_graph_match_canonical_serde_bytes() {
        let value = graph_value()["request"].clone();
        let expected: OwnedSourceRequest = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode_owned_source_request(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
    }

    #[test]
    fn owned_structural_report_and_each_persisted_leaf_match_canonical_bytes() {
        let value = graph_value()["report"].clone();
        let expected: OwnedStructuralReport = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode_owned_structural_report(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));

        let endpoint_value = value["edits"][0]["before"].clone();
        let expected_endpoint: OwnedSpanEndpoint = serde_json::from_value(endpoint_value.clone()).unwrap();
        let decoded_endpoint = decode_owned_span_endpoint(syntax(&serde_json::to_string(&endpoint_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_endpoint), canonical(&decoded_endpoint));

        let edit_value = value["edits"][0].clone();
        let expected_edit: OwnedSpanEdit = serde_json::from_value(edit_value.clone()).unwrap();
        let decoded_edit = decode_owned_span_edit(syntax(&serde_json::to_string(&edit_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_edit), canonical(&decoded_edit));

        let root_value = serde_json::json!({"name":"item", "value_type":{"kind":"optional", "item":{"kind":"list", "item":{"kind":"integer"}}}});
        let expected_root: OwnedRootBinding = serde_json::from_value(root_value.clone()).unwrap();
        let decoded_root = decode_owned_root_binding(syntax(&serde_json::to_string(&root_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_root), canonical(&decoded_root));

        let common_value = value["common_statements"][0].clone();
        let expected_common: OwnedCommonStatement = serde_json::from_value(common_value.clone()).unwrap();
        let decoded_common = decode_owned_common_statement(syntax(&serde_json::to_string(&common_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_common), canonical(&decoded_common));

        let writer_value = value["writers"][0].clone();
        let expected_writer: OwnedWriter = serde_json::from_value(writer_value.clone()).unwrap();
        let decoded_writer = decode_owned_writer(syntax(&serde_json::to_string(&writer_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_writer), canonical(&decoded_writer));

        let optional_value = serde_json::json!({"kind":"entity_added","id":"person","after":d('5')});
        let expected_optional: OwnedStructuralChange = serde_json::from_value(optional_value.clone()).unwrap();
        let decoded_optional = decode_owned_structural_change(syntax(&serde_json::to_string(&optional_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_optional), canonical(&decoded_optional));
        let optional_writer = serde_json::json!({"side":"after","source":d('6'),"action":"save","path":[],"kind":"archive","entity":"person"});
        let expected_optional_writer: OwnedWriter = serde_json::from_value(optional_writer.clone()).unwrap();
        let decoded_optional_writer = decode_owned_writer(syntax(&serde_json::to_string(&optional_writer).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_optional_writer), canonical(&decoded_optional_writer));

        let change_value = value["changes"][1].clone();
        let expected_change: OwnedStructuralChange = serde_json::from_value(change_value.clone()).unwrap();
        let decoded_change = decode_owned_structural_change(syntax(&serde_json::to_string(&change_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_change), canonical(&decoded_change));
    }

    #[test]
    fn every_owned_structural_enum_variant_matches_serde_and_rejects_unknowns() {
        for name in ["create", "update", "archive", "set_state", "collection", "for_each", "assert", "emit"] {
            let expected: OwnedStatementKind = serde_json::from_str(&format!("\"{name}\"")).unwrap();
            let decoded = decode_owned_statement_kind(syntax(&format!("\"{name}\""))).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
        }
        for (name, expected) in [("before", OwnedSourceSide::Before), ("after", OwnedSourceSide::After)] {
            let decoded = decode_owned_source_side(syntax(&format!("\"{name}\""))).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
        }
        for name in ["application_label_changed", "entity_added", "column_added", "column_changed", "observable_added", "observable_changed"] {
            let expected: OwnedStructuralChangeKind = serde_json::from_str(&format!("\"{name}\"")).unwrap();
            let decoded = decode_owned_structural_change_kind(syntax(&format!("\"{name}\""))).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
        }
        assert!(matches!(decode_owned_statement_kind(syntax("\"future\"")), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_owned_source_side(syntax("\"future\"")), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_owned_structural_change_kind(syntax("\"future\"")), Err(ValueDecodeError::UnknownVariant { .. })));
    }

    #[test]
    fn optional_fields_sequence_shapes_and_field_errors_match_serde() {
        let source = d('3');
        let value = serde_json::json!({
            "source":source,"action":"save","list_path":[],"start":0,"len":0,"kinds":[],"digest":d('4')
        });
        let expected: OwnedSpanEndpoint = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode_owned_span_endpoint(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        assert_eq!(expected.previous, None);
        assert_eq!(expected.next, None);
        assert_eq!(expected.root, None);

        let sequence = serde_json::json!([d('1'),d('2'),"person",[]]);
        let expected_request: OwnedSourceRequest = serde_json::from_value(sequence.clone()).unwrap();
        let decoded_request = decode_owned_source_request(syntax(&serde_json::to_string(&sequence).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_request), canonical(&decoded_request));

        for raw in [
            r#"{"before":"0000000000000000000000000000000000000000000000000000000000000000","after":"0000000000000000000000000000000000000000000000000000000000000000","root_entity":"root","edits":[],"extra":1}"#,
            r#"{"before":"bad","after":"0000000000000000000000000000000000000000000000000000000000000000","root_entity":"root","edits":[]}"#,
            r#"{"before":"0000000000000000000000000000000000000000000000000000000000000000","after":"0000000000000000000000000000000000000000000000000000000000000000","edits":[]}"#,
        ] {
            assert!(decode_owned_source_request(syntax(raw), &mut AllocationContext::new()).is_err());
        }
        assert!(product_json::parse_json_bytes(br#"{"before":"x","before":"y"}"#, &mut AllocationContext::new()).is_err());
    }

    #[test]
    fn path_numeric_and_collection_bounds_are_checked_before_growth() {
        let mut value = serde_json::json!({"source":d('1'),"action":"a","list_path":[],"start":0,"len":0,"kinds":[],"digest":d('2')});
        value["list_path"] = serde_json::json!(vec![0; crate::product_contract::MAX_EXPR_DEPTH + 1]);
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_span_endpoint(syntax(&serde_json::to_string(&value).unwrap()), &mut cx), Err(ValueDecodeError::LimitExceeded { .. })));
        assert_eq!(cx.growth_requests(), 0);

        let mut value = serde_json::json!({"source":d('1'),"action":"a","list_path":[],"start":0,"len":0,"kinds":[],"digest":d('2')});
        value["start"] = serde_json::json!(usize::MAX);
        assert!(decode_owned_span_endpoint(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).is_err());
        let mut value = serde_json::json!({"source":d('1'),"action":"a","list_path":[],"start":usize::MAX,"len":1,"kinds":[],"digest":d('2')});
        assert!(decode_owned_span_endpoint(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).is_err());
        let mut value = serde_json::json!({"source":d('1'),"action":"a","list_path":[],"start":0,"len":crate::product_contract::MAX_ITEMS + 1,"kinds":[],"digest":d('2')});
        assert!(matches!(decode_owned_span_endpoint(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));
        let unrepresentable_usize = format!(r#"{{"source":"{}","action":"a","list_path":[],"start":18446744073709551616,"len":0,"kinds":[],"digest":"{}"}}"#, d('1'), d('2'));
        assert!(decode_owned_span_endpoint(syntax(&unrepresentable_usize), &mut AllocationContext::new()).is_err());

        let too_many_edits = serde_json::json!({"before":d('1'),"after":d('2'),"root_entity":"r","edits":vec![serde_json::Value::Null; crate::product_contract::MAX_ITEMS + 1]});
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_source_request(syntax(&serde_json::to_string(&too_many_edits).unwrap()), &mut cx), Err(ValueDecodeError::LimitExceeded { .. })));
        assert_eq!(cx.growth_requests(), 0);

        let too_many_writers = serde_json::json!({"before":d('1'),"after":d('2'),"root_entity":"r","edits":[],"common_statements":[],"writers":vec![serde_json::Value::Null; crate::product_contract::MAX_COLLECTION + 1],"changes":[]});
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_structural_report(syntax(&serde_json::to_string(&too_many_writers).unwrap()), &mut cx), Err(ValueDecodeError::LimitExceeded { .. })));
        assert_eq!(cx.growth_requests(), 0);
    }

    #[test]
    fn every_decoder_growth_refusal_is_retryable_and_cleanup_safe() {
        let values = [
            serde_json::to_string(&graph_value()["request"]).unwrap(),
            serde_json::to_string(&graph_value()["report"]).unwrap(),
            serde_json::to_string(&serde_json::json!({"name":"root","value_type":{"kind":"optional","item":{"kind":"reference","entity":"person"}}})).unwrap(),
            format!(r#"["{}","{}","root",[]]"#, d('1'), d('2')),
        ];
        let decoders: [fn(Value, &mut AllocationContext) -> Result<(), ValueDecodeError>; 4] = [
            |v,c| decode_owned_source_request(v,c).map(|_|()),
            |v,c| decode_owned_structural_report(v,c).map(|_|()),
            |v,c| decode_owned_root_binding(v,c).map(|_|()),
            |v,c| decode_owned_source_request(v,c).map(|_|()),
        ];
        for (source, decode) in values.into_iter().zip(decoders) {
            let mut count = AllocationContext::new();
            decode(syntax(&source), &mut count).unwrap();
            let growths = count.growth_requests();
            assert!(growths > 0);
            for nth in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
                assert!(matches!(decode(syntax(&source), &mut cx), Err(ValueDecodeError::Allocation(_))), "refusal at growth {nth}/{growths}");
                decode(syntax(&source), &mut AllocationContext::new()).unwrap();
            }
        }
    }
}
