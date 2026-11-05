//! Fallible structural decoding for the recursive Product Contract Expr graph.
//!
//! This module consumes the bounded `product_json` tree, transfers owned
//! strings without copying, and reserves every Box/Vec growth through the
//! caller's AllocationContext. Semantic validation remains a later phase.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};

use super::fallible_value_decode::{decode_data_value, decode_type, JsonKind, NumericValue, ValueDecodeError};
use super::{Expr, SortKey, MAX_AST_NODES, MAX_EXPR_DEPTH, MAX_ITEMS};

const EXPR_VARIANTS: &[&str] = &[
    "literal", "variable", "field", "state", "today", "query", "filter", "map", "count",
    "any", "all", "sum", "contains", "equal", "less", "add", "subtract", "concat",
    "text_contains", "lower", "and", "or", "not", "if", "coalesce", "date_add",
    "date_difference",
];
const LITERAL_FIELDS: &[&str] = &["value", "value_type"];
const VARIABLE_FIELDS: &[&str] = &["name"];
const FIELD_FIELDS: &[&str] = &["field", "record"];
const STATE_FIELDS: &[&str] = &["state"];
const QUERY_FIELDS: &[&str] = &["binding", "entity", "include_archived", "limit", "predicate", "sort"];
const FILTER_FIELDS: &[&str] = &["binding", "items", "predicate"];
const MAP_FIELDS: &[&str] = &["binding", "items", "value"];
const ITEMS_FIELDS: &[&str] = &["items"];
const ANY_FIELDS: &[&str] = &["binding", "items", "predicate"];
const CONTAINS_FIELDS: &[&str] = &["items", "value"];
const BINARY_FIELDS: &[&str] = &["left", "right"];
const TEXT_CONTAINS_FIELDS: &[&str] = &["search", "text"];
const VALUE_FIELDS: &[&str] = &["value"];
const VALUES_FIELDS: &[&str] = &["values"];
const IF_FIELDS: &[&str] = &["condition", "else_value", "then_value"];
const COALESCE_FIELDS: &[&str] = &["fallback", "value"];
const DATE_ADD_FIELDS: &[&str] = &["date", "days"];
const DATE_DIFFERENCE_FIELDS: &[&str] = &["earlier", "later"];
const SORT_KEY_FIELDS: &[&str] = &["descending", "value"];
const SORT_KEY_SEQUENCE_FIELDS: &[&str] = &["value", "descending"];

/// Decode one expression and enforce the existing Product Contract AST bounds
/// before constructing each node. Depth is zero-based, like check_expr_shape.
pub(crate) fn decode_expr(value: Value, cx: &mut AllocationContext) -> Result<Expr, ValueDecodeError> {
    let mut nodes = 0usize;
    decode_expr_with_budget(value, 0, &mut nodes, cx)
}

pub(crate) fn decode_expr_with_budget(
    value: Value,
    depth: usize,
    nodes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<Expr, ValueDecodeError> {
    decode_expr_at(value, depth, nodes, cx)
}

fn decode_expr_at(
    value: Value,
    depth: usize,
    nodes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<Expr, ValueDecodeError> {
    if depth > MAX_EXPR_DEPTH {
        return Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: MAX_EXPR_DEPTH });
    }
    *nodes = nodes.checked_add(1).ok_or(ValueDecodeError::LimitExceeded {
        resource: "expression node count", limit: MAX_AST_NODES,
    })?;
    if *nodes > MAX_AST_NODES {
        return Err(ValueDecodeError::LimitExceeded { resource: "expression node count", limit: MAX_AST_NODES });
    }

    let (tag, fields) = tagged_expr(value)?;
    match tag.as_str() {
        "literal" => {
            let mut value_type = None;
            let mut literal = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "value" => literal = Some(decode_data_value(value, cx)?),
                    "value_type" => value_type = Some(decode_type(value, cx)?),
                    _ => return Err(unknown_field(key, LITERAL_FIELDS)),
                }
            }
            Ok(Expr::Literal {
                value_type: required(value_type, "value_type")?,
                value: required(literal, "value")?,
            })
        }
        "variable" => decode_single_string(fields, "name", VARIABLE_FIELDS).map(|name| Expr::Variable { name }),
        "field" => {
            let mut field = None;
            let mut record = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "field" => field = Some(string(value, "a string")?),
                    "record" => record = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, FIELD_FIELDS)),
                }
            }
            Ok(Expr::Field {
                record: required(record, "record")?,
                field: required(field, "field")?,
            })
        }
        "state" => decode_single_string(fields, "state", STATE_FIELDS).map(|state| Expr::State { state }),
        "today" => unit_expr(fields),
        "query" => {
            let mut binding = None;
            let mut entity = None;
            let mut include_archived = None;
            let mut limit = None;
            let mut predicate = None;
            let mut sort = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "binding" => binding = Some(string(value, "a string")?),
                    "entity" => entity = Some(string(value, "a string")?),
                    "include_archived" => include_archived = Some(boolean(value)?),
                    "limit" => limit = Some(integer_usize(value)?),
                    "predicate" => predicate = Some(boxed_expr(value, depth, nodes, cx)?),
                    "sort" => sort = Some(sort_keys(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, QUERY_FIELDS)),
                }
            }
            Ok(Expr::Query {
                entity: required(entity, "entity")?,
                binding: required(binding, "binding")?,
                predicate: required(predicate, "predicate")?,
                sort: required(sort, "sort")?,
                limit: required(limit, "limit")?,
                include_archived: required(include_archived, "include_archived")?,
            })
        }
        "filter" => {
            let mut binding = None;
            let mut items = None;
            let mut predicate = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "binding" => binding = Some(string(value, "a string")?),
                    "items" => items = Some(boxed_expr(value, depth, nodes, cx)?),
                    "predicate" => predicate = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, FILTER_FIELDS)),
                }
            }
            Ok(Expr::Filter {
                items: required(items, "items")?,
                binding: required(binding, "binding")?,
                predicate: required(predicate, "predicate")?,
            })
        }
        "map" => {
            let mut binding = None;
            let mut items = None;
            let mut result = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "binding" => binding = Some(string(value, "a string")?),
                    "items" => items = Some(boxed_expr(value, depth, nodes, cx)?),
                    "value" => result = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, MAP_FIELDS)),
                }
            }
            Ok(Expr::Map {
                items: required(items, "items")?,
                binding: required(binding, "binding")?,
                value: required(result, "value")?,
            })
        }
        "count" => decode_single_box(fields, "items", ITEMS_FIELDS, depth, nodes, cx)
            .map(|items| Expr::Count { items }),
        "any" | "all" => {
            let mut binding = None;
            let mut items = None;
            let mut predicate = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "binding" => binding = Some(string(value, "a string")?),
                    "items" => items = Some(boxed_expr(value, depth, nodes, cx)?),
                    "predicate" => predicate = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, ANY_FIELDS)),
                }
            }
            let binding = required(binding, "binding")?;
            let items = required(items, "items")?;
            let predicate = required(predicate, "predicate")?;
            if tag == "any" {
                Ok(Expr::Any { items, binding, predicate })
            } else {
                Ok(Expr::All { items, binding, predicate })
            }
        }
        "sum" => decode_single_box(fields, "items", ITEMS_FIELDS, depth, nodes, cx)
            .map(|items| Expr::Sum { items }),
        "contains" => decode_binary(fields, CONTAINS_FIELDS, "items", "value", depth, nodes, cx)
            .map(|(items, value)| Expr::Contains { items, value }),
        "equal" => decode_binary(fields, BINARY_FIELDS, "left", "right", depth, nodes, cx)
            .map(|(left, right)| Expr::Equal { left, right }),
        "less" => decode_binary(fields, BINARY_FIELDS, "left", "right", depth, nodes, cx)
            .map(|(left, right)| Expr::Less { left, right }),
        "add" => decode_binary(fields, BINARY_FIELDS, "left", "right", depth, nodes, cx)
            .map(|(left, right)| Expr::Add { left, right }),
        "subtract" => decode_binary(fields, BINARY_FIELDS, "left", "right", depth, nodes, cx)
            .map(|(left, right)| Expr::Subtract { left, right }),
        "concat" => decode_binary(fields, BINARY_FIELDS, "left", "right", depth, nodes, cx)
            .map(|(left, right)| Expr::Concat { left, right }),
        "text_contains" => {
            let mut search = None;
            let mut text = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "search" => search = Some(boxed_expr(value, depth, nodes, cx)?),
                    "text" => text = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, TEXT_CONTAINS_FIELDS)),
                }
            }
            Ok(Expr::TextContains { text: required(text, "text")?, search: required(search, "search")? })
        }
        "lower" => decode_single_box(fields, "value", VALUE_FIELDS, depth, nodes, cx)
            .map(|value| Expr::Lower { value }),
        "and" | "or" => {
            let values = decode_expr_sequence_field(fields, "values", VALUES_FIELDS, depth, nodes, cx)?;
            if tag == "and" { Ok(Expr::And { values }) } else { Ok(Expr::Or { values }) }
        }
        "not" => decode_single_box(fields, "value", VALUE_FIELDS, depth, nodes, cx)
            .map(|value| Expr::Not { value }),
        "if" => {
            let mut condition = None;
            let mut else_value = None;
            let mut then_value = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "condition" => condition = Some(boxed_expr(value, depth, nodes, cx)?),
                    "else_value" => else_value = Some(boxed_expr(value, depth, nodes, cx)?),
                    "then_value" => then_value = Some(boxed_expr(value, depth, nodes, cx)?),
                    _ => return Err(unknown_field(key, IF_FIELDS)),
                }
            }
            Ok(Expr::If {
                condition: required(condition, "condition")?,
                then_value: required(then_value, "then_value")?,
                else_value: required(else_value, "else_value")?,
            })
        }
        "coalesce" => decode_binary(fields, COALESCE_FIELDS, "value", "fallback", depth, nodes, cx)
            .map(|(value, fallback)| Expr::Coalesce { value, fallback }),
        "date_add" => decode_binary(fields, DATE_ADD_FIELDS, "date", "days", depth, nodes, cx)
            .map(|(date, days)| Expr::DateAdd { date, days }),
        "date_difference" => decode_binary(fields, DATE_DIFFERENCE_FIELDS, "later", "earlier", depth, nodes, cx)
            .map(|(later, earlier)| Expr::DateDifference { later, earlier }),
        _ => unreachable!("tag comes from the closed Expr variant table"),
    }
}

fn tagged_expr(value: Value) -> Result<(String, crate::product_alloc::ordered::OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = match value {
        Value::Object(fields) => fields,
        value => return Err(invalid_type(value, "internally tagged enum Expr")),
    };
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = match tag {
        Value::String(tag) => tag,
        value => return Err(invalid_type(value, "a string variant identifier")),
    };
    if EXPR_VARIANTS.iter().any(|variant| *variant == tag) {
        Ok((tag, fields))
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: EXPR_VARIANTS })
    }
}

fn boxed_expr(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Box<Expr>, ValueDecodeError> {
    let value = decode_expr_at(value, depth + 1, nodes, cx)?;
    Ok(product_alloc::try_box(value, cx)?)
}

fn expr_sequence(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Vec<Expr>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    ensure_item_count(values.len(), "expression item count")?;
    ensure_possible_node_count(values.len(), *nodes)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_expr_at(value, depth + 1, nodes, cx)?);
    }
    Ok(decoded)
}

fn sort_keys(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<Vec<SortKey>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    ensure_item_count(values.len(), "sort key count")?;
    ensure_possible_node_count(values.len(), *nodes)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(sort_key(value, depth, nodes, cx)?);
    }
    Ok(decoded)
}

fn sort_key(value: Value, depth: usize, nodes: &mut usize, cx: &mut AllocationContext) -> Result<SortKey, ValueDecodeError> {
    let fields = match value {
        Value::Object(fields) => fields,
        Value::Array(values) => {
            let names = SORT_KEY_SEQUENCE_FIELDS;
            if values.len() != names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let count = values.len().min(names.len());
            let mut fields = crate::product_alloc::ordered::OrderedMap::new();
            fields.try_reserve(count, cx)?;
            for (name, value) in names.iter().take(count).zip(values) {
                let key = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(key, value, cx)?;
            }
            fields
        }
        value => return Err(invalid_type(value, "a struct SortKey")),
    };
    let mut decoded_value = None;
    let mut descending = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "descending" => descending = Some(boolean(value)?),
            "value" => decoded_value = Some(decode_expr_at(value, depth + 1, nodes, cx)?),
            _ => return Err(unknown_field(key, SORT_KEY_FIELDS)),
        }
    }
    Ok(SortKey { value: required(decoded_value, "value")?, descending: required(descending, "descending")? })
}

fn decode_single_string(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<String, ValueDecodeError> {
    let mut result = None;
    for (key, value) in fields.into_entries() {
        if key == field { result = Some(string(value, "a string")?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(result, field)
}

fn decode_single_box(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    depth: usize,
    nodes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<Box<Expr>, ValueDecodeError> {
    let mut result = None;
    for (key, value) in fields.into_entries() {
        if key == field { result = Some(boxed_expr(value, depth, nodes, cx)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(result, field)
}

fn decode_expr_sequence_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    depth: usize,
    nodes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<Vec<Expr>, ValueDecodeError> {
    let mut result = None;
    for (key, value) in fields.into_entries() {
        if key == field { result = Some(expr_sequence(value, depth, nodes, cx)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(result, field)
}

fn decode_binary(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    expected: &'static [&'static str],
    left_field: &'static str,
    right_field: &'static str,
    depth: usize,
    nodes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<(Box<Expr>, Box<Expr>), ValueDecodeError> {
    let mut left = None;
    let mut right = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            name if name == left_field => left = Some(boxed_expr(value, depth, nodes, cx)?),
            name if name == right_field => right = Some(boxed_expr(value, depth, nodes, cx)?),
            _ => return Err(unknown_field(key, expected)),
        }
    }
    Ok((required(left, left_field)?, required(right, right_field)?))
}

fn unit_expr(fields: crate::product_alloc::ordered::OrderedMap<String, Value>) -> Result<Expr, ValueDecodeError> {
    if let Some((key, _)) = fields.into_entries().next() {
        Err(unknown_field(key, &[]))
    } else {
        Ok(Expr::Today)
    }
}

fn ensure_possible_node_count(additional: usize, nodes: usize) -> Result<(), ValueDecodeError> {
    if additional > MAX_AST_NODES.saturating_sub(nodes) {
        Err(ValueDecodeError::LimitExceeded { resource: "expression node count", limit: MAX_AST_NODES })
    } else {
        Ok(())
    }
}

fn ensure_item_count(count: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if count > MAX_ITEMS {
        Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS })
    } else {
        Ok(())
    }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn boolean(value: Value) -> Result<bool, ValueDecodeError> {
    match value {
        Value::Bool(value) => Ok(value),
        value => Err(invalid_type(value, "a boolean")),
    }
}

fn integer_usize(value: Value) -> Result<usize, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => usize::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "usize" }
        }),
        Value::Number(Number::I64(value)) => usize::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "usize" }
        }),
        value => Err(invalid_type(value, "usize")),
    }
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
    use crate::product_json;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
        serde_json::from_str(source).unwrap()
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
        assert!(matches!(failure.site, crate::product_alloc::AllocationSite::Box | crate::product_alloc::AllocationSite::Vector));
    }

    fn canonical_expr(source: &str) {
        let expected: Expr = typed(source);
        let mut cx = AllocationContext::new();
        let decoded = decode_expr(syntax(source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn every_expr_variant_matches_serde_canonical_bytes() {
        for source in [
            r#"{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"雪"}}"#,
            r#"{"kind":"variable","name":"x"}"#,
            r#"{"kind":"field","record":{"kind":"today"},"field":"f"}"#,
            r#"{"kind":"state","state":"s"}"#,
            r#"{"kind":"today"}"#,
            r#"{"kind":"query","entity":"e","binding":"b","predicate":{"kind":"today"},"sort":[{"value":{"kind":"today"},"descending":true}],"limit":2,"include_archived":false}"#,
            r#"{"kind":"filter","items":{"kind":"today"},"binding":"b","predicate":{"kind":"today"}}"#,
            r#"{"kind":"map","items":{"kind":"today"},"binding":"b","value":{"kind":"today"}}"#,
            r#"{"kind":"count","items":{"kind":"today"}}"#,
            r#"{"kind":"any","items":{"kind":"today"},"binding":"b","predicate":{"kind":"today"}}"#,
            r#"{"kind":"all","items":{"kind":"today"},"binding":"b","predicate":{"kind":"today"}}"#,
            r#"{"kind":"sum","items":{"kind":"today"}}"#,
            r#"{"kind":"contains","items":{"kind":"today"},"value":{"kind":"today"}}"#,
            r#"{"kind":"equal","left":{"kind":"today"},"right":{"kind":"today"}}"#,
            r#"{"kind":"less","left":{"kind":"today"},"right":{"kind":"today"}}"#,
            r#"{"kind":"add","left":{"kind":"today"},"right":{"kind":"today"}}"#,
            r#"{"kind":"subtract","left":{"kind":"today"},"right":{"kind":"today"}}"#,
            r#"{"kind":"concat","left":{"kind":"today"},"right":{"kind":"today"}}"#,
            r#"{"kind":"text_contains","text":{"kind":"today"},"search":{"kind":"today"}}"#,
            r#"{"kind":"lower","value":{"kind":"today"}}"#,
            r#"{"kind":"and","values":[{"kind":"today"}]}"#,
            r#"{"kind":"or","values":[{"kind":"today"}]}"#,
            r#"{"kind":"not","value":{"kind":"today"}}"#,
            r#"{"kind":"if","condition":{"kind":"today"},"then_value":{"kind":"today"},"else_value":{"kind":"today"}}"#,
            r#"{"kind":"coalesce","value":{"kind":"today"},"fallback":{"kind":"today"}}"#,
            r#"{"kind":"date_add","date":{"kind":"today"},"days":{"kind":"today"}}"#,
            r#"{"kind":"date_difference","later":{"kind":"today"},"earlier":{"kind":"today"}}"#,
        ] {
            canonical_expr(source);
        }
    }

    #[test]
    fn nested_expr_allocation_refusal_cleans_and_retries_at_each_growth() {
        let source = r#"{"kind":"query","entity":"e","binding":"b","predicate":{"kind":"and","values":[{"kind":"not","value":{"kind":"variable","name":"x"}},{"kind":"today"}]},"sort":[{"value":{"kind":"field","record":{"kind":"today"},"field":"f"},"descending":false}],"limit":2,"include_archived":true}"#;
        let expected: Expr = typed(source);
        let mut cx = AllocationContext::new();
        assert_eq!(decode_expr(syntax(source), &mut cx).unwrap(), expected);
        let growths = cx.growth_requests();
        assert!(growths >= 5);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_expr(syntax(source), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_expr(syntax(source), &mut retry).unwrap(), expected);
        }
    }

    #[test]
    fn decoder_rejects_unknown_malformed_and_out_of_range_fields() {
        assert!(matches!(decode_expr(syntax(r#"{"kind":"future"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"today","extra":1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"query","entity":"e","binding":"b","predicate":{"kind":"today"},"sort":[],"limit":-1,"include_archived":false}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "usize", .. })));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"query","entity":"e","binding":"b","predicate":{},"sort":[],"limit":1,"include_archived":false}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("kind"))));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"variable","name":1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { .. })));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"query","entity":"e","binding":"b","predicate":{"kind":"today"},"sort":[{"value":{"kind":"today"},"descending":1}],"limit":1,"include_archived":false}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { .. })));
        assert!(matches!(decode_expr(syntax(r#"{"kind":"not","value":{"kind":"today"},"unused":null}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
    }

    #[test]
    fn expression_and_sort_collections_are_bounded_before_reserve_or_decode() {
        let invalid_members = vec!["false"; MAX_ITEMS + 1].join(",");
        let expression = format!(r#"{{"kind":"and","values":[{invalid_members}]}}"#);
        assert!(matches!(decode_expr(syntax(&expression), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression item count", limit: MAX_ITEMS })));

        let invalid_keys = vec!["false"; MAX_ITEMS + 1].join(",");
        let query = format!(r#"{{"kind":"query","entity":"e","binding":"b","predicate":{{"kind":"today"}},"sort":[{invalid_keys}],"limit":1,"include_archived":false}}"#);
        assert!(matches!(decode_expr(syntax(&query), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "sort key count", limit: MAX_ITEMS })));
    }

    #[test]
    fn expression_depth_and_node_limits_match_contract_bounds() {
        fn nested_not(depth: usize) -> String {
            let mut source = String::from(r#"{"kind":"today"}"#);
            for _ in 0..depth {
                source = format!(r#"{{"kind":"not","value":{source}}}"#);
            }
            source
        }
        let mut cx = AllocationContext::new();
        assert!(decode_expr(syntax(&nested_not(MAX_EXPR_DEPTH)), &mut cx).is_ok());
        assert!(matches!(decode_expr(syntax(&nested_not(MAX_EXPR_DEPTH + 1)), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: MAX_EXPR_DEPTH })));

        let group = || format!(r#"{{"kind":"and","values":[{}]}}"#, vec![r#"{"kind":"today"}"#; 63].join(","));
        let below = format!(r#"{{"kind":"and","values":[{}]}}"#, vec![group(); 127].join(","));
        assert!(decode_expr(syntax(&below), &mut AllocationContext::new()).is_ok());
        let above = format!(r#"{{"kind":"and","values":[{}]}}"#, vec![group(); 128].join(","));
        assert!(matches!(decode_expr(syntax(&above), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression node count", limit: MAX_AST_NODES })));
    }

    #[test]
    fn owned_strings_transfer_without_unmetered_copy() {
        let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        assert_eq!(decode_expr(syntax(r#"{"kind":"variable","name":"é"}"#), &mut cx).unwrap(), Expr::Variable { name: "é".into() });
        assert_eq!(cx.growth_requests(), 0);
    }
}
