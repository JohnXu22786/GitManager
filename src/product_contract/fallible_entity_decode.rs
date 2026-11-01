//! Fallible structural decoding for Product Contract entity declarations.
//!
//! This slice is private and intentionally does not route production parsing.
//! It transfers owned string fields from the bounded JSON tree and reserves
//! every decoder-owned Vec before filling it.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::Value;

use super::fallible_expr_decode::decode_expr_with_budget;
use super::fallible_value_decode::{decode_type, JsonKind, ValueDecodeError};
use super::{EntityDefinition, Expr, FieldDefinition, MAX_ITEMS};

const FIELD_DEFINITION_FIELDS: &[&str] = &["id", "label", "value_type"];
const ENTITY_DEFINITION_FIELDS: &[&str] = &["id", "label", "fields", "unique", "constraints"];

pub(crate) fn decode_field_definition(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<FieldDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct FieldDefinition", FIELD_DEFINITION_FIELDS, cx)?;
    let mut id = None;
    let mut label = None;
    let mut value_type = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "value_type" => value_type = Some(decode_type(value, cx)?),
            _ => return Err(unknown_field(key, FIELD_DEFINITION_FIELDS)),
        }
    }
    Ok(FieldDefinition {
        id: required(id, "id")?,
        label: required(label, "label")?,
        value_type: required(value_type, "value_type")?,
    })
}

pub(crate) fn decode_entity_definition(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<EntityDefinition, ValueDecodeError> {
    decode_entity_definition_with_budget(value, cx, &mut 0)
}

pub(crate) fn decode_entity_definition_with_budget(
    value: Value,
    cx: &mut AllocationContext,
    nodes: &mut usize,
) -> Result<EntityDefinition, ValueDecodeError> {
    let fields = struct_fields(value, "a struct EntityDefinition", ENTITY_DEFINITION_FIELDS, cx)?;
    let mut constraints = None;
    let mut declared_fields = None;
    let mut id = None;
    let mut label = None;
    let mut unique = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "constraints" => constraints = Some(exprs(value, cx, nodes)?),
            "fields" => declared_fields = Some(field_definitions(value, cx)?),
            "id" => id = Some(string(value, "a string")?),
            "label" => label = Some(string(value, "a string")?),
            "unique" => unique = Some(unique_tuples(value, cx)?),
            _ => return Err(unknown_field(key, ENTITY_DEFINITION_FIELDS)),
        }
    }
    Ok(EntityDefinition {
        id: required(id, "id")?,
        label: required(label, "label")?,
        fields: required(declared_fields, "fields")?,
        unique: required(unique, "unique")?,
        constraints: required(constraints, "constraints")?,
    })
}

fn field_definitions(value: Value, cx: &mut AllocationContext) -> Result<Vec<FieldDefinition>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), "field definition count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_field_definition(value, cx)?);
    }
    Ok(decoded)
}

fn unique_tuples(value: Value, cx: &mut AllocationContext) -> Result<Vec<Vec<String>>, ValueDecodeError> {
    let tuples = array(value)?;
    bounded_length(tuples.len(), "unique constraint count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, tuples.len(), cx, AllocationSite::Vector)?;
    for tuple in tuples {
        let values = array(tuple)?;
        bounded_length(values.len(), "unique constraint field count")?;
        let mut fields = Vec::new();
        product_alloc::try_reserve(&mut fields, values.len(), cx, AllocationSite::Vector)?;
        for value in values {
            fields.push(string(value, "a string")?);
        }
        decoded.push(fields);
    }
    Ok(decoded)
}

fn exprs(value: Value, cx: &mut AllocationContext, nodes: &mut usize) -> Result<Vec<Expr>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), "entity constraint count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_expr_with_budget(value, 0, nodes, cx)?);
    }
    Ok(decoded)
}

fn struct_fields(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
    cx: &mut AllocationContext,
) -> Result<crate::product_alloc::ordered::OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        Value::Array(values) => {
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
            Ok(fields)
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(crate::product_json::Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(crate::product_json::Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(crate::product_json::Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn bounded_length(len: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > MAX_ITEMS {
        Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::{AllocationFailureKind, AllocationSite};
    use crate::product_json::{self, Cause};
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
        assert!(matches!(failure.site, AllocationSite::Vector | AllocationSite::Box));
    }

    #[test]
    fn field_and_entity_shapes_match_serde_canonical_bytes() {
        let field_source = r#"{"id":"name","label":"名前","value_type":{"kind":"optional","item":{"kind":"list","item":{"kind":"text"}}}}"#;
        let expected_field: FieldDefinition = typed(field_source);
        let mut cx = AllocationContext::new();
        let decoded_field = decode_field_definition(syntax(field_source), &mut cx).unwrap();
        assert_eq!(decoded_field, expected_field);
        assert_eq!(super::super::canonical_bytes(&expected_field).unwrap(), super::super::canonical_bytes(&decoded_field).unwrap());

        let entity_source = r#"{"id":"person","label":"Person","fields":[{"id":"name","label":"名字","value_type":{"kind":"text"}},{"id":"age","label":"Age","value_type":{"kind":"integer"}}],"unique":[["name"],["age","name"]],"constraints":[{"kind":"equal","left":{"kind":"field","record":{"kind":"variable","name":"record"},"field":"name"},"right":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}}]}"#;
        let expected_entity: EntityDefinition = typed(entity_source);
        let mut cx = AllocationContext::new();
        let decoded_entity = decode_entity_definition(syntax(entity_source), &mut cx).unwrap();
        assert_eq!(decoded_entity, expected_entity);
        assert_eq!(super::super::canonical_bytes(&expected_entity).unwrap(), super::super::canonical_bytes(&decoded_entity).unwrap());

        let field_with_tail = r#"["name","Name",{"kind":"text"},"ignored"]"#;
        assert!(serde_json::from_str::<FieldDefinition>(field_with_tail).is_err());
        assert!(matches!(decode_field_definition(syntax(field_with_tail), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 4, expected: 3 })));

        let entity_with_tail = r#"["person","Person",[],[],[],"ignored"]"#;
        assert!(serde_json::from_str::<EntityDefinition>(entity_with_tail).is_err());
        assert!(matches!(decode_entity_definition(syntax(entity_with_tail), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 6, expected: 5 })));
    }

    #[test]
    fn refusal_at_every_reached_vector_and_nested_box_growth_cleans_and_retries() {
        let source = r#"{"id":"person","label":"Person","fields":[{"id":"age","label":"Age","value_type":{"kind":"optional","item":{"kind":"integer"}}}],"unique":[["age"],["person","age"]],"constraints":[{"kind":"not","value":{"kind":"equal","left":{"kind":"field","record":{"kind":"variable","name":"record"},"field":"age"},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":0}}}}]}"#;
        let expected: EntityDefinition = typed(source);
        let mut cx = AllocationContext::new();
        assert_eq!(decode_entity_definition(syntax(source), &mut cx).unwrap(), expected);
        let growths = cx.growth_requests();
        assert!(growths >= 8);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_entity_definition(syntax(source), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_entity_definition(syntax(source), &mut retry).unwrap(), expected);
        }
    }

    #[test]
    fn malformed_unknown_wrong_type_and_duplicate_object_members_are_rejected() {
        assert!(matches!(decode_field_definition(syntax(r#"{"id":"x","label":"X"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("value_type"))));
        assert!(matches!(decode_field_definition(syntax(r#"{"id":"x","label":"X","value_type":{"kind":"text"},"extra":null}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_field_definition(syntax(r#"[]"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 0, expected: 3 })));
        assert!(matches!(decode_entity_definition(syntax(r#"{"id":false}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { actual: JsonKind::Boolean, .. })));
        let error = decode_entity_definition(syntax(r#"{"constraints":[],"fields":[],"id":"e","label":"E","unique":[],"extra":1}"#), &mut AllocationContext::new()).unwrap_err();
        let ValueDecodeError::UnknownField { expected, .. } = error else { panic!("expected unknown field") };
        assert_eq!(expected, &["id", "label", "fields", "unique", "constraints"]);
        let mut cx = AllocationContext::new();
        let error = match product_json::parse_json_bytes(br#"{"id":"a","id":"b"}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate object member was accepted"),
        };
        assert_eq!(error.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn collection_limits_match_contract_item_bound() {
        let fields = format!(r#"{{"id":"e","label":"E","fields":[{}],"unique":[],"constraints":[]}}"#, vec![r#"{"id":"x","label":"X","value_type":{"kind":"text"}}"#; MAX_ITEMS].join(","));
        assert!(decode_entity_definition(syntax(&fields), &mut AllocationContext::new()).is_ok());
        let fields_over = format!(r#"{{"id":"e","label":"E","fields":[{}],"unique":[],"constraints":[]}}"#, vec![r#"{"id":"x","label":"X","value_type":{"kind":"text"}}"#; MAX_ITEMS + 1].join(","));
        assert!(matches!(decode_entity_definition(syntax(&fields_over), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "field definition count", limit: MAX_ITEMS })));

        let constraints = format!(r#"{{"id":"e","label":"E","fields":[],"unique":[],"constraints":[{}]}}"#, vec![r#"{"kind":"today"}"#; MAX_ITEMS + 1].join(","));
        assert!(matches!(decode_entity_definition(syntax(&constraints), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "entity constraint count", limit: MAX_ITEMS })));

        let tuples = format!(r#"{{"id":"e","label":"E","fields":[],"unique":[{}],"constraints":[]}}"#, vec![r#"["f"]"#; MAX_ITEMS].join(","));
        assert!(decode_entity_definition(syntax(&tuples), &mut AllocationContext::new()).is_ok());
        let tuples_over = format!(r#"{{"id":"e","label":"E","fields":[],"unique":[{}],"constraints":[]}}"#, vec![r#"["f"]"#; MAX_ITEMS + 1].join(","));
        assert!(matches!(decode_entity_definition(syntax(&tuples_over), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "unique constraint count", limit: MAX_ITEMS })));

        fn nested_not(depth: usize) -> String {
            let mut source = String::from(r#"{"kind":"today"}"#);
            for _ in 0..depth {
                source = format!(r#"{{"kind":"not","value":{source}}}"#);
            }
            source
        }
        let at_depth = format!(r#"{{"id":"e","label":"E","fields":[],"unique":[],"constraints":[{}]}}"#, nested_not(super::super::MAX_EXPR_DEPTH));
        assert!(decode_entity_definition(syntax(&at_depth), &mut AllocationContext::new()).is_ok());
        let over_depth = format!(r#"{{"id":"e","label":"E","fields":[],"unique":[],"constraints":[{}]}}"#, nested_not(super::super::MAX_EXPR_DEPTH + 1));
        assert!(matches!(decode_entity_definition(syntax(&over_depth), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", .. })));
    }

    #[test]
    fn owned_string_fields_transfer_without_allocation() {
        let source = r#"{"id":"f","label":"雪","value_type":{"kind":"text"}}"#;
        let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        let decoded = decode_field_definition(syntax(source), &mut cx).unwrap();
        assert_eq!(decoded.label, "雪");
        assert_eq!(cx.growth_requests(), 0);
    }
}
