//! Closed, call-local actual-Value admission. No proof or reusable output API
//! escapes this body. The source-first relation does not validate semantics.
use super::*;

// These compile-time widths cover the finite live-slot plan, not measured CPU
// stack use. Changing helper chains/slots requires revisiting the plan as well.
const P: usize = size_of::<usize>();
const A: usize = size_of::<Audit<'static>>();
const R: usize = size_of::<Result<bool>>();
const STR: usize = size_of::<&'static str>();
const J: usize = size_of::<JsonValue>();
const LMAP: usize = size_of::<serde_json::map::Iter<'static>>();
const LSLICE: usize = size_of::<std::slice::Iter<'static, JsonValue>>();
const SET: usize = size_of::<std::collections::btree_set::Iter<'static, KeyModifier>>();
macro_rules! maximum {
    ($only:expr) => { $only };
    ($head:expr, $($tail:expr),+) => {{
        const REST: usize = maximum!($($tail),+);
        if $head > REST { $head } else { REST }
    }};
}
const RMAP: usize = maximum!(
    size_of::<std::collections::btree_map::Iter<'static, String, Type>>(),
    size_of::<std::collections::btree_map::Iter<'static, String, Expr>>(),
    size_of::<std::collections::btree_map::Iter<'static, String, DataValue>>()
);
const RSLICE: usize = maximum!(
    size_of::<std::slice::Iter<'static, ActionBinding>>(),
    size_of::<std::slice::Iter<'static, ActionDefinition>>(),
    size_of::<std::slice::Iter<'static, Column>>(),
    size_of::<std::slice::Iter<'static, DataValue>>(),
    size_of::<std::slice::Iter<'static, EntityDefinition>>(),
    size_of::<std::slice::Iter<'static, Expr>>(),
    size_of::<std::slice::Iter<'static, FieldDefinition>>(),
    size_of::<std::slice::Iter<'static, FormInput>>(),
    size_of::<std::slice::Iter<'static, Id>>(),
    size_of::<std::slice::Iter<'static, KeyBinding>>(),
    size_of::<std::slice::Iter<'static, ObservableDefinition>>(),
    size_of::<std::slice::Iter<'static, OutputDefinition>>(),
    size_of::<std::slice::Iter<'static, SortKey>>(),
    size_of::<std::slice::Iter<'static, StateControl>>(),
    size_of::<std::slice::Iter<'static, StateDefinition>>(),
    size_of::<std::slice::Iter<'static, Statement>>(),
    size_of::<std::slice::Iter<'static, Vec<Id>>>(),
    size_of::<std::slice::Iter<'static, ViewDefinition>>()
);
const COMPARE_FRAME: usize = 64
    + 24 * P
    + 4 * A
    + 4 * R
    + 2 * STR
    + size_of::<serde_json::Number>()
    + maximum!(LMAP + RMAP, LSLICE + RSLICE, LSLICE + SET);
const COMPARE_ROOT: usize = 32 + 2 * J + 4 * A + 2 * R + 4 * P + 4 * size_of::<bool>();
const COMPARE_STACK_ROOT: usize = 4 * 65 * COMPARE_FRAME + COMPARE_ROOT;
const MUT_SLICE: usize = size_of::<std::slice::IterMut<'static, JsonValue>>();
const MUT_OPT: usize = size_of::<Option<&'static mut JsonValue>>();
const MUTATION_FRAME: usize = 64 + 16 * P + 4 * A + 4 * R + 2 * STR + 2 * MUT_SLICE + 4 * MUT_OPT;
const MUTATION_ROOT: usize = 24 + 2 * A + 2 * R + 4 * P + 2 * MUT_OPT;
const MUTATION_STACK_ROOT: usize = 4 * 8 * MUTATION_FRAME + MUTATION_ROOT;
const SER: usize = size_of::<serde_json::Serializer<&'static mut Vec<u8>>>();
const COMPOUND: usize = size_of::<
    serde_json::ser::Compound<'static, &'static mut Vec<u8>, serde_json::ser::CompactFormatter>,
>();
const OUTPUT_FRAME: usize = maximum!(
    64 + 64 * P,
    size_of::<Shape<'static>>()
        + 2 * size_of::<Container<'static, 'static>>()
        + COMPOUND
        + 2 * SER
        + LMAP
        + LSLICE
        + 4 * size_of::<Result<()>>()
        + 12 * P
        + 64
);
const ERROR_WIDTH: usize = maximum!(size_of::<Box<str>>(), size_of::<std::io::Error>()) + 4 * P;
const OUTPUT_ROOT: usize = 12
    + 2 * size_of::<Vec<u8>>()
    + 2 * SER
    + 2 * size_of::<serde_json::Result<Vec<u8>>>()
    + 2 * size_of::<Result<Vec<u8>>>()
    + 2 * size_of::<serde_json::Error>()
    + (5 + 2 * ERROR_WIDTH)
    + 72;
const OUTPUT_STACK_ROOT: usize = 4 * 74 * OUTPUT_FRAME + OUTPUT_ROOT;

// The reviewed bounded plan is admitted only on targets where these widths fit
// its slots. Formula changes cannot silently spend the preview's spare budget.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<Shape<'static>>() == 64);
    assert!(size_of::<Container<'static, 'static>>() == 24);
    assert!(size_of::<Option<u64>>() == 16);
    assert!(COMPARE_FRAME == 576 && COMPARE_ROOT == 212);
    assert!(MUTATION_FRAME == 416 && MUTATION_ROOT == 136);
    assert!(OUTPUT_FRAME == 576 && OUTPUT_ROOT == 377);
};

pub(in super::super) fn validate_closed_value_capture(
    capture: &CapturedProgram,
    a: Audit<'_>,
) -> Result<capture_validation::CaptureFacts> {
    macro_rules! relation_result {
        ($a:ident, $value:expr) => {{
            $a.pay(2)?;
            return Ok($value);
        }};
    }
    fn rel_null(v: &JsonValue, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        a.pay(2)?;
        relation_result!(a, matches!(v, JsonValue::Null));
    }
    fn rel_string(v: &JsonValue, p: &str, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::String(s) = v else {
            relation_result!(a, false);
        };
        a.pay(4)?;
        let equal = a.eq_str(s, p)?;
        relation_result!(a, equal);
    }
    // Probe all source entries through terminal before borrowing any retained
    // variant payload. Its iterator is dropped before the complete field pass.
    fn tag_matches(
        m: &serde_json::Map<String, JsonValue>,
        index: usize,
        tag: &str,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(6)?;
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        let mut at = 0usize;
        let mut found = false;
        loop {
            a.pay(6)?;
            let next = cursor.next();
            if let Some((key, value)) = next {
                if at == index {
                    found = a.eq_str(key, "kind")? && rel_string(value, tag, a)?;
                }
                at = at.checked_add(1).ok_or_else(|| a.overflow())?;
            } else {
                break;
            }
        }
        a.pay(4)?;
        Ok(found)
    }
    macro_rules! relation_field {
        ($a:ident, $cursor:ident, $key:literal, $value:ident, $body:expr) => {{
            $a.pay(8)?;
            let Some((key, $value)) = $cursor.next() else {
                relation_result!($a, false);
            };
            if !$a.eq_str(key, $key)? {
                relation_result!($a, false);
            }
            if !$body {
                relation_result!($a, false);
            }
        }};
    }

    // Only after this same immutable object's complete successful tag probe.
    // Keep the second traversal and exact key check; the String leaf is proven.
    macro_rules! checked_kind_field {
        ($a:ident, $cursor:ident) => {{
            $a.pay(8)?;
            let Some((key, _)) = $cursor.next() else {
                relation_result!($a, false);
            };
            if !$a.eq_str(key, "kind")? {
                relation_result!($a, false);
            }
        }};
    }

    fn rel_type(v: &JsonValue, p: &Type, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            Type::Boolean => (1, 0, "boolean"),
            Type::Integer => (1, 0, "integer"),
            Type::Text => (1, 0, "text"),
            Type::Date => (1, 0, "date"),
            Type::Reference { .. } => (2, 1, "reference"),
            Type::List { .. } => (2, 1, "list"),
            Type::Optional { .. } => (2, 1, "optional"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            Type::Boolean => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::Integer => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::Text => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::Date => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::Reference { entity } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::List { item } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "item", source_child, {
                    a.pay(2)?;
                    rel_type(source_child, (item).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Type::Optional { item } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "item", source_child, {
                    a.pay(2)?;
                    rel_type(source_child, (item).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_datavalue(v: &JsonValue, p: &DataValue, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            DataValue::Null => (1, 0, "null"),
            DataValue::Boolean { .. } => (2, 0, "boolean"),
            DataValue::Integer { .. } => (2, 0, "integer"),
            DataValue::Text { .. } => (2, 0, "text"),
            DataValue::Date { .. } => (2, 1, "date"),
            DataValue::Reference { .. } => (3, 1, "reference"),
            DataValue::List { .. } => (3, 2, "list"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            DataValue::Null => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::Boolean { value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "value",
                    source_child,
                    rel_bool(source_child, value, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::Integer { value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "value",
                    source_child,
                    rel_i64(source_child, value, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::Text { value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "value",
                    source_child,
                    rel_string(source_child, value, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::Date { days } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "days",
                    source_child,
                    rel_i32(source_child, days, a)?
                );
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::Reference { entity, record } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "record",
                    source_child,
                    rel_string(source_child, record, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            DataValue::List { item_type, items } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "item_type",
                    source_child,
                    rel_type(source_child, item_type, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "items",
                    source_child,
                    rel_vec_datavalue(source_child, items, a)?
                );
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_expr(v: &JsonValue, p: &Expr, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            Expr::Literal { .. } => (3, 0, "literal"),
            Expr::Variable { .. } => (2, 0, "variable"),
            Expr::Field { .. } => (3, 1, "field"),
            Expr::State { .. } => (2, 0, "state"),
            Expr::Today => (1, 0, "today"),
            Expr::Query { .. } => (7, 3, "query"),
            Expr::Filter { .. } => (4, 2, "filter"),
            Expr::Map { .. } => (4, 2, "map"),
            Expr::Count { .. } => (2, 1, "count"),
            Expr::Any { .. } => (4, 2, "any"),
            Expr::All { .. } => (4, 2, "all"),
            Expr::Sum { .. } => (2, 1, "sum"),
            Expr::Contains { .. } => (3, 1, "contains"),
            Expr::Equal { .. } => (3, 0, "equal"),
            Expr::Less { .. } => (3, 0, "less"),
            Expr::Add { .. } => (3, 0, "add"),
            Expr::Subtract { .. } => (3, 0, "subtract"),
            Expr::Concat { .. } => (3, 0, "concat"),
            Expr::TextContains { .. } => (3, 0, "text_contains"),
            Expr::Lower { .. } => (2, 0, "lower"),
            Expr::And { .. } => (2, 0, "and"),
            Expr::Or { .. } => (2, 0, "or"),
            Expr::Not { .. } => (2, 0, "not"),
            Expr::If { .. } => (4, 2, "if"),
            Expr::Coalesce { .. } => (3, 1, "coalesce"),
            Expr::DateAdd { .. } => (3, 2, "date_add"),
            Expr::DateDifference { .. } => (3, 1, "date_difference"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            Expr::Literal { value_type, value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "value",
                    source_child,
                    rel_datavalue(source_child, value, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "value_type",
                    source_child,
                    rel_type(source_child, value_type, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Variable { name } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "name",
                    source_child,
                    rel_string(source_child, name, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Field { record, field } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "field",
                    source_child,
                    rel_string(source_child, field, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "record", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (record).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::State { state } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "state",
                    source_child,
                    rel_string(source_child, state, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Today => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                include_archived,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "include_archived",
                    source_child,
                    rel_bool(source_child, include_archived, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "limit",
                    source_child,
                    rel_usize(source_child, limit, a)?
                );
                relation_field!(a, cursor, "predicate", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (predicate).as_ref(), a)?
                });
                relation_field!(
                    a,
                    cursor,
                    "sort",
                    source_child,
                    rel_vec_sortkey(source_child, sort, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "predicate", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (predicate).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Count { items } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Any {
                items,
                binding,
                predicate,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "predicate", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (predicate).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::All {
                items,
                binding,
                predicate,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "predicate", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (predicate).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Sum { items } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Contains { items, value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "items", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (items).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Equal { left, right } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "left", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (left).as_ref(), a)?
                });
                relation_field!(a, cursor, "right", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (right).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Less { left, right } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "left", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (left).as_ref(), a)?
                });
                relation_field!(a, cursor, "right", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (right).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Add { left, right } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "left", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (left).as_ref(), a)?
                });
                relation_field!(a, cursor, "right", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (right).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Subtract { left, right } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "left", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (left).as_ref(), a)?
                });
                relation_field!(a, cursor, "right", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (right).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Concat { left, right } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "left", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (left).as_ref(), a)?
                });
                relation_field!(a, cursor, "right", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (right).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::TextContains { text, search } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "search", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (search).as_ref(), a)?
                });
                relation_field!(a, cursor, "text", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (text).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Lower { value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::And { values } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "values",
                    source_child,
                    rel_vec_expr(source_child, values, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Or { values } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "values",
                    source_child,
                    rel_vec_expr(source_child, values, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Not { value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "condition", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (condition).as_ref(), a)?
                });
                relation_field!(a, cursor, "else_value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (else_value).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "then_value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (then_value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::Coalesce { value, fallback } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "fallback", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (fallback).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "value", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (value).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::DateAdd { date, days } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "date", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (date).as_ref(), a)?
                });
                relation_field!(a, cursor, "days", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (days).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Expr::DateDifference { later, earlier } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(a, cursor, "earlier", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (earlier).as_ref(), a)?
                });
                checked_kind_field!(a, cursor);
                relation_field!(a, cursor, "later", source_child, {
                    a.pay(2)?;
                    rel_expr(source_child, (later).as_ref(), a)?
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_sortkey(v: &JsonValue, p: &SortKey, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 2 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "descending",
            source_child,
            rel_bool(source_child, &p.descending, a)?
        );
        relation_field!(
            a,
            cursor,
            "value",
            source_child,
            rel_expr(source_child, &p.value, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_fielddefinition(v: &JsonValue, p: &FieldDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 3 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "value_type",
            source_child,
            rel_type(source_child, &p.value_type, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_entitydefinition(v: &JsonValue, p: &EntityDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 5 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "constraints",
            source_child,
            rel_vec_expr(source_child, &p.constraints, a)?
        );
        relation_field!(
            a,
            cursor,
            "fields",
            source_child,
            rel_vec_fielddefinition(source_child, &p.fields, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "unique",
            source_child,
            rel_vec_vec_id(source_child, &p.unique, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_statedefinition(v: &JsonValue, p: &StateDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 4 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "initial",
            source_child,
            rel_datavalue(source_child, &p.initial, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "value_type",
            source_child,
            rel_type(source_child, &p.value_type, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_actiondefinition(v: &JsonValue, p: &ActionDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 6 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "ensures",
            source_child,
            rel_vec_expr(source_child, &p.ensures, a)?
        );
        relation_field!(
            a,
            cursor,
            "guards",
            source_child,
            rel_vec_expr(source_child, &p.guards, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "parameters",
            source_child,
            rel_btreemap_id_type(source_child, &p.parameters, a)?
        );
        relation_field!(
            a,
            cursor,
            "steps",
            source_child,
            rel_vec_statement(source_child, &p.steps, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_statement(v: &JsonValue, p: &Statement, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            Statement::Create { .. } => (4, 2, "create"),
            Statement::Update { .. } => (3, 0, "update"),
            Statement::Archive { .. } => (2, 0, "archive"),
            Statement::SetState { .. } => (3, 0, "set_state"),
            Statement::Collection { .. } => (4, 1, "collection"),
            Statement::ForEach { .. } => (4, 2, "for_each"),
            Statement::Assert { .. } => (3, 1, "assert"),
            Statement::Emit { .. } => (5, 3, "emit"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            Statement::Create {
                entity,
                values,
                bind,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "bind",
                    source_child,
                    rel_string(source_child, bind, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "values",
                    source_child,
                    rel_btreemap_id_expr(source_child, values, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::Update { record, values } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "record",
                    source_child,
                    rel_expr(source_child, record, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "values",
                    source_child,
                    rel_btreemap_id_expr(source_child, values, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::Archive { record } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "record",
                    source_child,
                    rel_expr(source_child, record, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::SetState { state, value } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "state",
                    source_child,
                    rel_string(source_child, state, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "value",
                    source_child,
                    rel_expr(source_child, value, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::Collection {
                target,
                operation,
                items,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "items",
                    source_child,
                    rel_expr(source_child, items, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "operation",
                    source_child,
                    rel_collectionoperation(source_child, operation, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "target",
                    source_child,
                    rel_collectiontarget(source_child, target, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::ForEach {
                items,
                binding,
                steps,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "items",
                    source_child,
                    rel_expr(source_child, items, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "steps",
                    source_child,
                    rel_vec_statement(source_child, steps, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::Assert { condition, message } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "condition",
                    source_child,
                    rel_expr(source_child, condition, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "message",
                    source_child,
                    rel_string(source_child, message, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            Statement::Emit {
                output,
                items,
                binding,
                columns,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "binding",
                    source_child,
                    rel_string(source_child, binding, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "columns",
                    source_child,
                    rel_btreemap_id_expr(source_child, columns, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "items",
                    source_child,
                    rel_expr(source_child, items, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "output",
                    source_child,
                    rel_string(source_child, output, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_collectiontarget(v: &JsonValue, p: &CollectionTarget, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            CollectionTarget::State { .. } => (2, 0, "state"),
            CollectionTarget::Field { .. } => (3, 1, "field"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            CollectionTarget::State { state } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "state",
                    source_child,
                    rel_string(source_child, state, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            CollectionTarget::Field { record, field } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "field",
                    source_child,
                    rel_string(source_child, field, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "record",
                    source_child,
                    rel_expr(source_child, record, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_collectionoperation(
        v: &JsonValue,
        p: &CollectionOperation,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::String(s) = v else {
            relation_result!(a, false);
        };
        a.pay(6)?;
        let tag = match p {
            CollectionOperation::Insert => "insert",
            CollectionOperation::Remove => "remove",
        };
        let equal = a.eq_str(s, tag)?;
        relation_result!(a, equal);
    }

    fn rel_outputformat(v: &JsonValue, p: &OutputFormat, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::String(s) = v else {
            relation_result!(a, false);
        };
        a.pay(6)?;
        let tag = match p {
            OutputFormat::Csv => "csv",
            OutputFormat::Json => "json",
        };
        let equal = a.eq_str(s, tag)?;
        relation_result!(a, equal);
    }

    fn rel_outputdefinition(v: &JsonValue, p: &OutputDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 4 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "columns",
            source_child,
            rel_vec_fielddefinition(source_child, &p.columns, a)?
        );
        relation_field!(
            a,
            cursor,
            "format",
            source_child,
            rel_outputformat(source_child, &p.format, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_observabledefinition(
        v: &JsonValue,
        p: &ObservableDefinition,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 3 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "value",
            source_child,
            rel_expr(source_child, &p.value, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_column(v: &JsonValue, p: &Column, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 3 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "value",
            source_child,
            rel_expr(source_child, &p.value, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_statecontrol(v: &JsonValue, p: &StateControl, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 4 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(a, cursor, "on_change", source_child, {
            a.pay(4)?;
            match &p.on_change {
                Some(child) => rel_string(source_child, child, a)?,
                None => rel_null(source_child, a)?,
            }
        });
        relation_field!(
            a,
            cursor,
            "state",
            source_child,
            rel_string(source_child, &p.state, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_selectionbinding(v: &JsonValue, p: &SelectionBinding, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 3 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(a, cursor, "on_change", source_child, {
            a.pay(4)?;
            match &p.on_change {
                Some(child) => rel_string(source_child, child, a)?,
                None => rel_null(source_child, a)?,
            }
        });
        relation_field!(
            a,
            cursor,
            "state",
            source_child,
            rel_string(source_child, &p.state, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_forminput(v: &JsonValue, p: &FormInput, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 2 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "parameter",
            source_child,
            rel_string(source_child, &p.parameter, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_viewkind(v: &JsonValue, p: &ViewKind, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        let (count, index, tag) = match p {
            ViewKind::List { .. } => (6, 3, "list"),
            ViewKind::Detail { .. } => (4, 2, "detail"),
            ViewKind::Form { .. } => (4, 3, "form"),
        };
        if m.len() != count {
            relation_result!(a, false);
        }
        if !tag_matches(m, index, tag, a)? {
            relation_result!(a, false);
        }
        match p {
            ViewKind::List {
                entity,
                rows,
                columns,
                controls,
                selection,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "columns",
                    source_child,
                    rel_vec_column(source_child, columns, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "controls",
                    source_child,
                    rel_vec_statecontrol(source_child, controls, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "rows",
                    source_child,
                    rel_expr(source_child, rows, a)?
                );
                relation_field!(a, cursor, "selection", source_child, {
                    a.pay(4)?;
                    match selection {
                        Some(child) => rel_selectionbinding(source_child, child, a)?,
                        None => rel_null(source_child, a)?,
                    }
                });
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            ViewKind::Detail {
                entity,
                record,
                columns,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "columns",
                    source_child,
                    rel_vec_column(source_child, columns, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "entity",
                    source_child,
                    rel_string(source_child, entity, a)?
                );
                checked_kind_field!(a, cursor);
                relation_field!(
                    a,
                    cursor,
                    "record",
                    source_child,
                    rel_expr(source_child, record, a)?
                );
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
            ViewKind::Form {
                action,
                fields,
                defaults,
            } => {
                a.map_iter(m.len())?;
                let mut cursor = m.iter();
                relation_field!(
                    a,
                    cursor,
                    "action",
                    source_child,
                    rel_string(source_child, action, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "defaults",
                    source_child,
                    rel_btreemap_id_datavalue(source_child, defaults, a)?
                );
                relation_field!(
                    a,
                    cursor,
                    "fields",
                    source_child,
                    rel_vec_forminput(source_child, fields, a)?
                );
                checked_kind_field!(a, cursor);
                a.pay(4)?;
                let end = cursor.next().is_none();
                relation_result!(a, end);
            }
        }
    }

    fn rel_actionplacement(v: &JsonValue, p: &ActionPlacement, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::String(s) = v else {
            relation_result!(a, false);
        };
        a.pay(6)?;
        let tag = match p {
            ActionPlacement::Toolbar => "toolbar",
            ActionPlacement::Row => "row",
        };
        let equal = a.eq_str(s, tag)?;
        relation_result!(a, equal);
    }

    fn rel_actionbinding(v: &JsonValue, p: &ActionBinding, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 6 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "action",
            source_child,
            rel_string(source_child, &p.action, a)?
        );
        relation_field!(
            a,
            cursor,
            "arguments",
            source_child,
            rel_btreemap_id_expr(source_child, &p.arguments, a)?
        );
        relation_field!(
            a,
            cursor,
            "enabled",
            source_child,
            rel_expr(source_child, &p.enabled, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "placement",
            source_child,
            rel_actionplacement(source_child, &p.placement, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_keymodifier(v: &JsonValue, p: &KeyModifier, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::String(s) = v else {
            relation_result!(a, false);
        };
        a.pay(6)?;
        let tag = match p {
            KeyModifier::Control => "control",
            KeyModifier::Alt => "alt",
            KeyModifier::Shift => "shift",
            KeyModifier::Command => "command",
        };
        let equal = a.eq_str(s, tag)?;
        relation_result!(a, equal);
    }

    fn rel_keybinding(v: &JsonValue, p: &KeyBinding, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 3 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "binding",
            source_child,
            rel_string(source_child, &p.binding, a)?
        );
        relation_field!(
            a,
            cursor,
            "key",
            source_child,
            rel_string(source_child, &p.key, a)?
        );
        relation_field!(
            a,
            cursor,
            "modifiers",
            source_child,
            rel_btreeset_keymodifier(source_child, &p.modifiers, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_viewdefinition(v: &JsonValue, p: &ViewDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 5 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "actions",
            source_child,
            rel_vec_actionbinding(source_child, &p.actions, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "keys",
            source_child,
            rel_vec_keybinding(source_child, &p.keys, a)?
        );
        relation_field!(
            a,
            cursor,
            "kind",
            source_child,
            rel_viewkind(source_child, &p.kind, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_appdefinition(v: &JsonValue, p: &AppDefinition, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(m) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if m.len() != 10 {
            relation_result!(a, false);
        }
        a.map_iter(m.len())?;
        let mut cursor = m.iter();
        relation_field!(
            a,
            cursor,
            "actions",
            source_child,
            rel_vec_actiondefinition(source_child, &p.actions, a)?
        );
        relation_field!(
            a,
            cursor,
            "entities",
            source_child,
            rel_vec_entitydefinition(source_child, &p.entities, a)?
        );
        relation_field!(
            a,
            cursor,
            "id",
            source_child,
            rel_string(source_child, &p.id, a)?
        );
        relation_field!(
            a,
            cursor,
            "initial_view",
            source_child,
            rel_string(source_child, &p.initial_view, a)?
        );
        relation_field!(
            a,
            cursor,
            "label",
            source_child,
            rel_string(source_child, &p.label, a)?
        );
        relation_field!(
            a,
            cursor,
            "observables",
            source_child,
            rel_vec_observabledefinition(source_child, &p.observables, a)?
        );
        relation_field!(
            a,
            cursor,
            "outputs",
            source_child,
            rel_vec_outputdefinition(source_child, &p.outputs, a)?
        );
        relation_field!(
            a,
            cursor,
            "state",
            source_child,
            rel_vec_statedefinition(source_child, &p.state, a)?
        );
        relation_field!(
            a,
            cursor,
            "version",
            source_child,
            rel_u32(source_child, &p.version, a)?
        );
        relation_field!(
            a,
            cursor,
            "views",
            source_child,
            rel_vec_viewdefinition(source_child, &p.views, a)?
        );
        a.pay(4)?;
        let end = cursor.next().is_none();
        relation_result!(a, end);
    }

    fn rel_btreemap_id_datavalue(
        v: &JsonValue,
        p: &BTreeMap<Id, DataValue>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.map_iter(source.len())?;
        a.map_iter(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(6)?;
            match (lefts.next(), rights.next()) {
                (Some((lk, left)), Some((rk, right))) => {
                    a.pay(4)?;
                    if !a.eq_str(lk, rk)? {
                        relation_result!(a, false);
                    }
                    if !rel_datavalue(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_btreemap_id_expr(v: &JsonValue, p: &BTreeMap<Id, Expr>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.map_iter(source.len())?;
        a.map_iter(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(6)?;
            match (lefts.next(), rights.next()) {
                (Some((lk, left)), Some((rk, right))) => {
                    a.pay(4)?;
                    if !a.eq_str(lk, rk)? {
                        relation_result!(a, false);
                    }
                    if !rel_expr(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_btreemap_id_type(v: &JsonValue, p: &BTreeMap<Id, Type>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Object(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.map_iter(source.len())?;
        a.map_iter(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(6)?;
            match (lefts.next(), rights.next()) {
                (Some((lk, left)), Some((rk, right))) => {
                    a.pay(4)?;
                    if !a.eq_str(lk, rk)? {
                        relation_result!(a, false);
                    }
                    if !rel_type(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_btreeset_keymodifier(
        v: &JsonValue,
        p: &BTreeSet<KeyModifier>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.map_iter(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(6)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_keymodifier(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_actionbinding(v: &JsonValue, p: &Vec<ActionBinding>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_actionbinding(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_actiondefinition(
        v: &JsonValue,
        p: &Vec<ActionDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_actiondefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_column(v: &JsonValue, p: &Vec<Column>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_column(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_datavalue(v: &JsonValue, p: &Vec<DataValue>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_datavalue(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_entitydefinition(
        v: &JsonValue,
        p: &Vec<EntityDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_entitydefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_expr(v: &JsonValue, p: &Vec<Expr>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_expr(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_fielddefinition(
        v: &JsonValue,
        p: &Vec<FieldDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_fielddefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_forminput(v: &JsonValue, p: &Vec<FormInput>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_forminput(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_keybinding(v: &JsonValue, p: &Vec<KeyBinding>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_keybinding(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_observabledefinition(
        v: &JsonValue,
        p: &Vec<ObservableDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_observabledefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_outputdefinition(
        v: &JsonValue,
        p: &Vec<OutputDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_outputdefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_sortkey(v: &JsonValue, p: &Vec<SortKey>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_sortkey(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_statecontrol(v: &JsonValue, p: &Vec<StateControl>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_statecontrol(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_statedefinition(
        v: &JsonValue,
        p: &Vec<StateDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_statedefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_statement(v: &JsonValue, p: &Vec<Statement>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_statement(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_vec_id(v: &JsonValue, p: &Vec<Vec<Id>>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_vec_id(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_id(v: &JsonValue, p: &Vec<Id>, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_string(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_vec_viewdefinition(
        v: &JsonValue,
        p: &Vec<ViewDefinition>,
        a: Audit<'_>,
    ) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Array(source) = v else {
            relation_result!(a, false);
        };
        a.pay(8)?;
        if source.len() != p.len() {
            relation_result!(a, false);
        }
        a.slice(source.len())?;
        a.slice(p.len())?;
        let mut lefts = source.iter();
        let mut rights = p.iter();
        // Split the pair packet at the native next boundary: terminal funding
        // is paid before both next calls; child funding only on a real pair.
        loop {
            a.pay(4)?;
            match (lefts.next(), rights.next()) {
                (Some(left), Some(right)) => {
                    a.pay(2)?;
                    if !rel_viewdefinition(left, right, a)? {
                        relation_result!(a, false);
                    }
                }
                (None, None) => {
                    relation_result!(a, true);
                }
                _ => {
                    relation_result!(a, false);
                }
            }
        }
    }

    fn rel_bool(v: &JsonValue, p: &bool, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Bool(s) = v else {
            relation_result!(a, false);
        };
        a.pay(4)?;
        relation_result!(a, *s == *p);
    }

    fn rel_i32(v: &JsonValue, p: &i32, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Number(s) = v else {
            relation_result!(a, false);
        };
        a.pay(12)?;
        let n = serde_json::Number::from(*p);
        let equal = *s == n;
        relation_result!(a, equal);
    }

    fn rel_i64(v: &JsonValue, p: &i64, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Number(s) = v else {
            relation_result!(a, false);
        };
        a.pay(12)?;
        let n = serde_json::Number::from(*p);
        let equal = *s == n;
        relation_result!(a, equal);
    }

    fn rel_u32(v: &JsonValue, p: &u32, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Number(s) = v else {
            relation_result!(a, false);
        };
        a.pay(12)?;
        let n = serde_json::Number::from(*p);
        let equal = *s == n;
        relation_result!(a, equal);
    }

    fn rel_usize(v: &JsonValue, p: &usize, a: Audit<'_>) -> Result<bool> {
        a.pay(12)?;
        let JsonValue::Number(s) = v else {
            relation_result!(a, false);
        };
        a.pay(12)?;
        let n = serde_json::Number::from(*p);
        let equal = *s == n;
        relation_result!(a, equal);
    }

    // This is the original complete intake, before any retained-App traversal.
    a.format(107)?;
    let mut value =
        match bounded_input::parse_json_bytes_with_work(&capture.source_bytes, &mut |n| a.pay(n)) {
            Ok(value) => value,
            Err(bounded_input::InputWorkError::Input(error)) => {
                return Err(ContractError(error.to_string()))
            }
            Err(bounded_input::InputWorkError::Resource(error)) => return Err(error),
            Err(bounded_input::InputWorkError::ArithmeticOverflow) => return Err(a.overflow()),
        };
    a.pay(96)?;
    a.pay(COMPARE_STACK_ROOT as u64)?;
    a.pay(6)?;
    let equal = rel_appdefinition(&value, &capture.program, a)?;
    a.pay(0)?;
    a.pay(4)?;
    if equal {
        fn clear_field<'v>(
            m: &'v mut serde_json::Map<String, JsonValue>,
            key: &str,
            a: Audit<'_>,
        ) -> Result<&'v mut JsonValue> {
            a.pay(8)?;
            a.map_search(m.len(), key.len())?;
            match m.get_mut(key) {
                Some(value) => Ok(value),
                None => Err(a.error("capture Value shape changed")?),
            }
        }
        fn clear_label(m: &mut serde_json::Map<String, JsonValue>, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            let value = clear_field(m, "label", a)?;
            a.pay(8)?;
            let JsonValue::String(s) = value else {
                return Err(a.error("capture Value shape changed")?);
            };
            a.pay(64)?;
            let mut len = u64::try_from(s.len()).map_err(|_| a.overflow())?;
            let mut encoded = 0u64;
            let mut escaped = 0u64;
            for byte in s.bytes() {
                a.pay(14)?;
                let n = match byte {
                    b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                    0..=31 => 6,
                    _ => 1,
                };
                encoded = encoded.checked_add(n).ok_or_else(|| a.overflow())?;
                if n != 1 {
                    escaped = escaped.checked_add(1).ok_or_else(|| a.overflow())?;
                }
            }
            a.pay(64)?;
            #[cfg(test)]
            semantic_shapes::alter_delta(&mut len, &mut encoded,
                &mut raw.bytes, &mut raw.native_base, &mut raw.escaped);
            let removed = len.checked_mul(6).and_then(|n| n.checked_add(encoded))
                .ok_or_else(|| a.overflow())?;
            let bytes = raw.bytes.checked_sub(encoded).ok_or_else(|| a.overflow())?;
            let native = raw.native_base.checked_sub(removed).ok_or_else(|| a.overflow())?;
            let remaining_escaped = raw.escaped.checked_sub(escaped).ok_or_else(|| a.overflow())?;
            raw.bytes = bytes;
            raw.native_base = native;
            raw.escaped = remaining_escaped;
            a.pay(8)?;
            s.clear();
            Ok(())
        }
        macro_rules! clear_array {
            ($a:ident, $map:ident, $key:literal, $child:ident, $raw:ident) => {{
                let value = clear_field($map, $key, $a)?;
                $a.pay(8)?;
                let JsonValue::Array(values) = value else {
                    return Err($a.error("capture Value shape changed")?);
                };
                $a.slice(values.len())?;
                for item in values.iter_mut() {
                    $a.pay(4)?;
                    $child(item, $a, $raw)?;
                }
                $a.pay(2)?;
            }};
        }
        fn clear_app(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            clear_label(m, a, raw)?;
            clear_array!(a, m, "entities", clear_entity, raw);
            clear_array!(a, m, "state", clear_leaf, raw);
            clear_array!(a, m, "actions", clear_leaf, raw);
            clear_array!(a, m, "outputs", clear_output, raw);
            clear_array!(a, m, "observables", clear_leaf, raw);
            clear_array!(a, m, "views", clear_view, raw);
            a.pay(2)?;
            Ok(())
        }
        fn clear_entity(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            clear_label(m, a, raw)?;
            clear_array!(a, m, "fields", clear_leaf, raw);
            a.pay(2)?;
            Ok(())
        }
        fn clear_output(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            clear_label(m, a, raw)?;
            clear_array!(a, m, "columns", clear_leaf, raw);
            a.pay(2)?;
            Ok(())
        }
        fn clear_view(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            clear_label(m, a, raw)?;
            clear_array!(a, m, "actions", clear_leaf, raw);
            clear_view_kind(clear_field(m, "kind", a)?, a, raw)?;
            a.pay(2)?;
            Ok(())
        }
        fn clear_leaf(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            clear_label(m, a, raw)?;
            a.pay(2)?;
            Ok(())
        }
        fn clear_view_kind(v: &mut JsonValue, a: Audit<'_>, raw: &mut RawOutputShape) -> Result<()> {
            a.pay(8)?;
            let JsonValue::Object(m) = v else {
                return Err(a.error("capture Value shape changed")?);
            };
            let tag = clear_field(m, "kind", a)?;
            a.pay(8)?;
            let JsonValue::String(tag) = tag else {
                return Err(a.error("capture Value shape changed")?);
            };
            a.pay(2)?;
            if a.eq_str(tag, "list")? {
                clear_array!(a, m, "columns", clear_leaf, raw);
                clear_array!(a, m, "controls", clear_leaf, raw);
            } else {
                a.pay(2)?;
                if a.eq_str(tag, "detail")? {
                    clear_array!(a, m, "columns", clear_leaf, raw);
                } else {
                    a.pay(2)?;
                    if a.eq_str(tag, "form")? {
                        clear_array!(a, m, "fields", clear_leaf, raw);
                    } else {
                        return Err(a.error("capture Value shape changed")?);
                    }
                }
            }
            a.pay(2)?;
            Ok(())
        }
        // These exclusive borrowed states cannot attach to another Value, be
        // cloned, or escape this successful equal branch. No native-tail total
        // is reused: each output keeps its own complete reservation and Vec.
        struct RawOutputShape {
            bytes: u64,
            nodes: u64,
            pairs: u64,
            native_base: u64,
            escaped: u64,
            digest_copies: u64,
        }
        struct ProgramOutput<'v, 'w> {
            value: &'v mut JsonValue,
            a: Audit<'w>,
            raw: RawOutputShape,
        }
        struct SemanticOutput<'v, 'w> {
            value: &'v mut JsonValue,
            a: Audit<'w>,
            raw: RawOutputShape,
        }
        const DELTA_FRAME: usize = 8 * size_of::<u64>() + 4 * size_of::<usize>()
            + 4 * size_of::<&mut RawOutputShape>() + size_of::<std::slice::Iter<'static, u8>>();
        const DELTA_ROOT: usize = 4 * (2 * size_of::<RawOutputShape>()
            + size_of::<ProgramOutput<'static, 'static>>()
            + size_of::<SemanticOutput<'static, 'static>>()
            + 8 * size_of::<usize>() + 4 * size_of::<Result<()>>());
        #[cfg(target_pointer_width = "64")]
        const _: () = {
            assert!(size_of::<RawOutputShape>() == 48);
            assert!(size_of::<ProgramOutput<'static, 'static>>() == 64);
            assert!(size_of::<SemanticOutput<'static, 'static>>() == 64);
            assert!(DELTA_FRAME == 144 && DELTA_ROOT == 1536);
        };
        // This pending-length phase exists only in this complete relation arm.
        // It cannot attach arbitrary bytes, clone, serialize, or leave this call.
        struct PendingLength<'c, 'v, 'w> {
            capture: &'c CapturedProgram,
            value: &'v mut JsonValue,
            a: Audit<'w>,
            total_nodes: usize,
            action_prefix_nodes: usize,
            action_count: usize,
        }
        // Positive owner/transport coverage is separate from DELTA_ROOT. The
        // full new tuple and Result carriers are counted, not just their delta.
        const BOUND_ROOT: usize = 4 * (
            2 * size_of::<PendingLength<'static, 'static, 'static>>()
            + 2 * size_of::<capture_validation::CaptureFacts>()
            + 8 * size_of::<usize>() + 4 * size_of::<Result<()>>()
            + size_of::<(Vec<u8>, ProgramOutput<'static, 'static>, capture_validation::CaptureFacts)>()
            + size_of::<Result<(Vec<u8>, ProgramOutput<'static, 'static>, capture_validation::CaptureFacts)>>()
        );
        #[cfg(target_pointer_width = "64")]
        const _: () = {
            assert!(size_of::<PendingLength<'static, 'static, 'static>>() == 48);
            assert!(size_of::<capture_validation::CaptureFacts>() == 32);
            assert!(BOUND_ROOT == 2240);
        };
        impl<'v, 'w> PendingLength<'_, 'v, 'w> {
        fn first_value_output(self)
            -> Result<(Vec<u8>, ProgramOutput<'v, 'w>, capture_validation::CaptureFacts)>
        {
            let Self { capture: _, value, a, total_nodes, action_prefix_nodes, action_count } = self;
            a.pay(0)?;
            a.pay(96)?;
            a.pay(OUTPUT_STACK_ROOT as u64)?;
            let mut shape = Shape::new(a)?;
            a.pay(1)?;
            shape.canonical = false;
            a.pay(1)?;
            shape.simple_app = true;
            value.serialize(&mut shape)?;
            a.pay(DELTA_ROOT as u64)?;
            a.pay(64)?;
            let raw = RawOutputShape {
                bytes: shape.bytes, nodes: shape.nodes, pairs: shape.pairs,
                native_base: shape.native, escaped: shape.escaped,
                digest_copies: shape.digest_copies,
            };
            #[cfg(test)]
            semantic_shapes::record([raw.bytes, raw.nodes, raw.pairs, raw.native_base,
                raw.escaped, raw.digest_copies]);
            shape.reserve_output()?;
            let bytes = serde_json::to_vec(&*value).map_err(|error| ContractError(error.to_string()))?;
            #[cfg(test)]
            native_outputs::record(&bytes);
            a.pay(0)?;
            // Read the first actual native Vec before any label mutation. The
            // fixed control packet pays the consume/reborrow/len/transfer work;
            // the original facts5 is charged at actual construction, not early.
            a.pay(128)?;
            let canonical_len = bytes.len();
            a.pay(5)?;
            let facts = capture_validation::CaptureFacts {
                total_nodes, action_prefix_nodes, canonical_len, action_count,
            };
            Ok((bytes, ProgramOutput { value, a, raw }, facts))
        }
        }
        impl<'v, 'w> ProgramOutput<'v, 'w> {
            fn clear_labels(self) -> Result<SemanticOutput<'v, 'w>> {
                self.a.pay(64)?;
                let Self { value, a, mut raw } = self;
                a.pay(96)?;
                a.pay(MUTATION_STACK_ROOT as u64)?;
                a.pay((4 * 8 * DELTA_FRAME) as u64)?;
                clear_app(value, a, &mut raw)?;
                a.pay(0)?;
                a.pay(64)?;
                Ok(SemanticOutput { value, a, raw })
            }
        }
        impl SemanticOutput<'_, '_> {
            fn write(self) -> Result<Vec<u8>> {
                self.a.pay(0)?;
                self.a.pay(96)?;
                self.a.pay(OUTPUT_STACK_ROOT as u64)?;
                let mut shape = Shape::new(self.a)?;
                self.a.pay(1)?;
                shape.canonical = false;
                self.a.pay(1)?;
                shape.simple_app = true;
                self.a.pay(64)?;
                let Self { value, a, raw } = self;
                shape.bytes = raw.bytes;
                shape.nodes = raw.nodes;
                shape.pairs = raw.pairs;
                shape.native = raw.native_base;
                shape.escaped = raw.escaped;
                shape.digest_copies = raw.digest_copies;
                #[cfg(test)]
                semantic_shapes::record([shape.bytes, shape.nodes, shape.pairs, shape.native,
                    shape.escaped, shape.digest_copies]);
                shape.reserve_output()?;
                let bytes = serde_json::to_vec(&*value).map_err(|error| ContractError(error.to_string()))?;
                #[cfg(test)]
                native_outputs::record(&bytes);
                a.pay(0)?;
                Ok(bytes)
            }
        }

        // Prepay every new root owner and fixed handoff before constructing it.
        // Constant checked target layouts fail compilation on arithmetic overflow.
        a.pay(128)?;
        a.pay(BOUND_ROOT as u64)?;
        let pending = {
            let app = &capture.program;
            validate_app_semantics!(app, a, validator, action_prefix_nodes);
            // Complete same-Value equality excludes floats/coercion and schema
            // expansion. Native compact length is <= this actual immutable raw
            // source length. This establishes only the V1 budget predicate.
            // Keep the removed count's entry/success cancellation opportunities.
            a.pay(0)?;
            a.pay(128)?;
            let source_len = capture.source_bytes.len();
            a.pay(0)?;
            a.pay(3)?;
            a.require(source_len <= MAX_WIRE_BYTES, "program exceeds byte budget")?;
            PendingLength { capture, value: &mut value, a, total_nodes: validator.nodes,
                action_prefix_nodes, action_count: app.actions.len() }
        };
        // The Validator drops here, after the original semantic tail. No pending
        // facts can publish, and no exact canonical length has been invented.
        // Successful equality continuation, not a synthetic self-comparison.
        a.pay(1)?;
        let raw = capture_validation::bytes_digest_audited(
            IdentityDomain::Source,
            &pending.capture.source_bytes,
            a,
        )?;
        a.require(
            a.eq_str(pending.capture.artifact.raw_digest.as_str(), raw.as_str())?
                && a.eq_str(
                    pending.capture.binding.source_digest.as_str(),
                    pending.capture.artifact.raw_digest.as_str(),
                )?,
            "raw source digest mismatch",
        )?;
        a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
        if !a.enabled() {
            pending.capture.program.validate_audited(a)?;
        }
        let (bytes, program_output, facts) = pending.first_value_output()?;
        let program = capture_validation::bytes_digest_audited(IdentityDomain::Program, &bytes, a)?;
        let equal = a.eq_str(capture.artifact.program_digest.as_str(), program.as_str())?;
        let equal = if equal {
            a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
            if !a.enabled() {
                capture.program.validate_audited(a)?;
            }
            let semantic_output = program_output.clear_labels()?;
            // Program bytes remain owned and live throughout this fresh output.
            let semantic_bytes = semantic_output.write()?;
            let digest = capture_validation::bytes_digest_audited(
                IdentityDomain::SemanticProgram,
                &semantic_bytes,
                a,
            )?;
            a.eq_str(capture.artifact.semantic_digest.as_str(), digest.as_str())?
        } else {
            false
        };
        a.require(equal, "program digest mismatch")?;
        return Ok(facts);
    }
    // False is not a validation result: keep the complete native Conversion,
    // genuine V1, independent typed equality, and old canonical fallback order.
    let actual = capture_typed::convert_app(value, a)?;
    let facts = actual.validate_audited(a)?;
    a.require(
        a.equal_owned(&actual, &capture.program)?,
        "source bytes do not represent captured program",
    )?;
    let raw =
        capture_validation::bytes_digest_audited(IdentityDomain::Source, &capture.source_bytes, a)?;
    a.require(
        a.eq_str(capture.artifact.raw_digest.as_str(), raw.as_str())?
            && a.eq_str(
                capture.binding.source_digest.as_str(),
                capture.artifact.raw_digest.as_str(),
            )?,
        "raw source digest mismatch",
    )?;
    a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
    if !a.enabled() {
        actual.validate_audited(a)?;
    }
    let bytes = canonical_bytes_audited(&actual, a)?;
    let program = capture_validation::bytes_digest_audited(IdentityDomain::Program, &bytes, a)?;
    let equal = a.eq_str(capture.artifact.program_digest.as_str(), program.as_str())?;
    let equal = if equal {
        a.pay(8 + 2 * size_of::<Audit<'_>>() as u64 + 4 * size_of::<bool>() as u64)?;
        if !a.enabled() {
            actual.validate_audited(a)?;
        }
        let mut semantic = a.clone_owned(&actual)?;
        semantic.clear_display_labels_audited(a)?;
        let bytes = canonical_bytes_audited(&semantic, a)?;
        let digest =
            capture_validation::bytes_digest_audited(IdentityDomain::SemanticProgram, &bytes, a)?;
        a.eq_str(capture.artifact.semantic_digest.as_str(), digest.as_str())?
    } else {
        false
    };
    a.require(equal, "program digest mismatch")?;
    Ok(facts)
}

// Test-only scoped capture of the two actual native Vec outputs. There is no
// production observer, generic serializer hook, authority, or output API.
#[cfg(test)]
pub(super) mod native_outputs {
    use super::*;
    thread_local! {
        static OUTPUTS: std::cell::RefCell<Option<Vec<(Vec<u8>,usize,usize)>>> = const { std::cell::RefCell::new(None) };
    }
    pub(super) fn record(bytes: &Vec<u8>) {
        OUTPUTS.with(|slot| {
            if let Some(outputs) = slot.borrow_mut().as_mut() {
                outputs.push((bytes.clone(), bytes.as_ptr() as usize, bytes.capacity()));
            }
        });
    }
    pub(in super::super) fn begin() {
        OUTPUTS.with(|slot| assert!(slot.borrow_mut().replace(Vec::new()).is_none()));
    }
    pub(in super::super) fn take() -> Vec<(Vec<u8>, usize, usize)> {
        OUTPUTS.with(|slot| slot.borrow_mut().take().unwrap())
    }
}

// Test-only numeric observations and corruption faults. Neither provides an
// attach/constructor, output bytes, payment credit or production authority.
#[cfg(test)]
pub(super) mod semantic_shapes {
    thread_local! {
        static SHAPES: std::cell::RefCell<Option<Vec<[u64; 6]>>> = const { std::cell::RefCell::new(None) };
        static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    }
    pub(super) fn record(shape: [u64; 6]) {
        SHAPES.with(|slot| { if let Some(values) = slot.borrow_mut().as_mut() { values.push(shape); } });
    }
    pub(in super::super) fn begin() { SHAPES.with(|slot| assert!(slot.borrow_mut().replace(vec![]).is_none())); }
    pub(in super::super) fn take() -> Vec<[u64; 6]> { SHAPES.with(|slot| slot.borrow_mut().take().unwrap()) }
    pub(in super::super) fn fault(fault: &'static str) { FAULT.with(|slot| assert!(slot.replace(Some(fault)).is_none())); }
    pub(in super::super) fn clear_fault() { FAULT.with(|slot| slot.set(None)); }
    pub(super) fn alter_delta(len: &mut u64, encoded: &mut u64, bytes: &mut u64, native: &mut u64, escaped: &mut u64) {
        FAULT.with(|slot| match slot.take() {
            Some("bytes") => *bytes = 0,
            Some("native") => *native = 0,
            Some("escaped") => *escaped = 0,
            Some("multiply") => *len = u64::MAX,
            Some("add") => { *len = 1; *encoded = u64::MAX; },
            Some(other) => panic!("unknown fault {other}"),
            None => (),
        });
    }
}
