//! Bounded structural decoders for Product Contract value leaves.
//!
//! Inputs are the closed tree produced by `product_json`; these helpers do not
//! perform semantic validation or route production parsing in this unit.
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_json::{Number, Value};

use super::{DataValue, FallibleValues, Type, MAX_COLLECTION, MAX_TYPE_DEPTH};

const TYPE_VARIANTS: &[&str] = &["boolean", "integer", "text", "date", "reference", "list", "optional"];
const VALUE_VARIANTS: &[&str] = &["null", "boolean", "integer", "text", "date", "reference", "list"];
const NO_FIELDS: &[&str] = &[];
const REFERENCE_FIELDS: &[&str] = &["entity"];
const LIST_TYPE_FIELDS: &[&str] = &["item"];
const OPTIONAL_TYPE_FIELDS: &[&str] = &["item"];
const BOOLEAN_VALUE_FIELDS: &[&str] = &["value"];
const INTEGER_VALUE_FIELDS: &[&str] = &["value"];
const TEXT_VALUE_FIELDS: &[&str] = &["value"];
const DATE_VALUE_FIELDS: &[&str] = &["days"];
const REFERENCE_VALUE_FIELDS: &[&str] = &["entity", "record"];
const LIST_VALUE_FIELDS: &[&str] = &["item_type", "items"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsonKind {
    Null,
    Boolean,
    UnsignedInteger,
    SignedInteger,
    FloatingPoint,
    String,
    Sequence,
    Map,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericValue {
    Unsigned(u64),
    Signed(i64),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ValueDecodeError {
    Allocation(AllocationFailure),
    LimitExceeded { resource: &'static str, limit: usize },
    MissingField(&'static str),
    UnknownField { key: String, expected: &'static [&'static str] },
    UnknownVariant { tag: String, expected: &'static [&'static str] },
    InvalidFormat(&'static str),
    InvalidType { actual: JsonKind, expected: &'static str },
    InvalidValue { actual: NumericValue, expected: &'static str },
}

impl From<AllocationFailure> for ValueDecodeError {
    fn from(value: AllocationFailure) -> Self {
        Self::Allocation(value)
    }
}

pub(super) fn decode_type(value: Value, cx: &mut AllocationContext) -> Result<Type, ValueDecodeError> {
    decode_type_at(value, cx, 0)
}

fn decode_type_at(value: Value, cx: &mut AllocationContext, depth: usize) -> Result<Type, ValueDecodeError> {
    if depth > MAX_TYPE_DEPTH {
        return Err(ValueDecodeError::LimitExceeded { resource: "type nesting depth", limit: MAX_TYPE_DEPTH });
    }
    let (tag, fields) = tagged(value, TYPE_VARIANTS, "internally tagged enum Type")?;
    match tag.as_str() {
        "boolean" => unit(fields, NO_FIELDS).map(|()| Type::Boolean),
        "integer" => unit(fields, NO_FIELDS).map(|()| Type::Integer),
        "text" => unit(fields, NO_FIELDS).map(|()| Type::Text),
        "date" => unit(fields, NO_FIELDS).map(|()| Type::Date),
        "reference" => {
            let entity = string_field(fields, "entity", REFERENCE_FIELDS)?;
            Ok(Type::Reference { entity })
        }
        "list" => {
            let item = type_field(fields, "item", LIST_TYPE_FIELDS, cx, depth + 1)?;
            Ok(Type::List { item })
        }
        "optional" => {
            let item = type_field(fields, "item", OPTIONAL_TYPE_FIELDS, cx, depth + 1)?;
            Ok(Type::Optional { item })
        }
        _ => unreachable!("tag comes from the closed Type variant table"),
    }
}

pub(super) fn decode_data_value(value: Value, cx: &mut AllocationContext) -> Result<DataValue, ValueDecodeError> {
    let (tag, fields) = tagged(value, VALUE_VARIANTS, "internally tagged enum DataValue")?;
    match tag.as_str() {
        "null" => unit(fields, NO_FIELDS).map(|()| DataValue::Null),
        "boolean" => bool_field(fields, "value", BOOLEAN_VALUE_FIELDS).map(|value| DataValue::Boolean { value }),
        "integer" => i64_field(fields, "value", INTEGER_VALUE_FIELDS).map(|value| DataValue::Integer { value }),
        "text" => string_field(fields, "value", TEXT_VALUE_FIELDS).map(|value| DataValue::Text { value }),
        "date" => i32_field(fields, "days", DATE_VALUE_FIELDS).map(|days| DataValue::Date { days }),
        "reference" => {
            let mut entity = None;
            let mut record = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "entity" => entity = Some(string(value, "a string")?),
                    "record" => record = Some(string(value, "a string")?),
                    _ => return Err(ValueDecodeError::UnknownField { key, expected: REFERENCE_VALUE_FIELDS }),
                }
            }
            Ok(DataValue::Reference {
                entity: entity.ok_or(ValueDecodeError::MissingField("entity"))?,
                record: record.ok_or(ValueDecodeError::MissingField("record"))?,
            })
        }
        "list" => {
            let mut item_type = None;
            let mut items = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "item_type" => item_type = Some(decode_type(value, cx)?),
                    "items" => items = Some(data_values(value, cx)?),
                    _ => return Err(ValueDecodeError::UnknownField { key, expected: LIST_VALUE_FIELDS }),
                }
            }
            Ok(DataValue::List {
                item_type: item_type.ok_or(ValueDecodeError::MissingField("item_type"))?,
                items: items.ok_or(ValueDecodeError::MissingField("items"))?,
            })
        }
        _ => unreachable!("tag comes from the closed DataValue variant table"),
    }
}

pub(super) fn decode_values(value: Value, cx: &mut AllocationContext) -> Result<FallibleValues, ValueDecodeError> {
    let entries = match value {
        Value::Object(entries) => entries,
        value => return Err(invalid_type(value, "a map")),
    };
    let mut decoded = FallibleValues::new();
    for (key, value) in entries.into_entries() {
        let value = decode_data_value(value, cx)?;
        decoded.try_insert(key, value, cx)?;
    }
    Ok(decoded)
}

fn tagged(value: Value, expected: &'static [&'static str], expected_type: &'static str)
    -> Result<(String, crate::product_alloc::ordered::OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = match value {
        Value::Object(fields) => fields,
        value => return Err(invalid_type(value, expected_type)),
    };
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = match tag {
        Value::String(tag) => tag,
        tag => return Err(invalid_type(tag, "a string variant identifier")),
    };
    if expected.iter().any(|name| *name == tag) {
        Ok((tag, fields))
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected })
    }
}

fn unit(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    expected: &'static [&'static str],
) -> Result<(), ValueDecodeError> {
    if let Some((key, _)) = fields.into_entries().next() {
        Err(ValueDecodeError::UnknownField { key, expected })
    } else {
        Ok(())
    }
}

fn type_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    cx: &mut AllocationContext,
    depth: usize,
) -> Result<Box<Type>, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field {
            let item = decode_type_at(value, cx, depth)?;
            decoded = Some(product_alloc::try_box(item, cx)?);
        } else {
            return Err(ValueDecodeError::UnknownField { key, expected });
        }
    }
    decoded.ok_or(ValueDecodeError::MissingField(field))
}

fn string_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<String, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field {
            decoded = Some(string(value, "a string")?);
        } else {
            return Err(ValueDecodeError::UnknownField { key, expected });
        }
    }
    decoded.ok_or(ValueDecodeError::MissingField(field))
}

fn bool_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<bool, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field {
            decoded = Some(match value {
                Value::Bool(value) => value,
                value => return Err(invalid_type(value, "a boolean")),
            });
        } else {
            return Err(ValueDecodeError::UnknownField { key, expected });
        }
    }
    decoded.ok_or(ValueDecodeError::MissingField(field))
}

fn i64_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<i64, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field {
            decoded = Some(integer_i64(value)?);
        } else {
            return Err(ValueDecodeError::UnknownField { key, expected });
        }
    }
    decoded.ok_or(ValueDecodeError::MissingField(field))
}

fn i32_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<i32, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field {
            decoded = Some(integer_i32(value)?);
        } else {
            return Err(ValueDecodeError::UnknownField { key, expected });
        }
    }
    decoded.ok_or(ValueDecodeError::MissingField(field))
}

fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}

fn integer_i64(value: Value) -> Result<i64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i64::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i64" }
        }),
        Value::Number(Number::I64(value)) => Ok(value),
        value => Err(invalid_type(value, "i64")),
    }
}

fn integer_i32(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i32" }
        }),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "i32" }
        }),
        value => Err(invalid_type(value, "i32")),
    }
}

fn data_values(value: Value, cx: &mut AllocationContext) -> Result<Vec<DataValue>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    if values.len() > MAX_COLLECTION {
        return Err(ValueDecodeError::LimitExceeded { resource: "list item count", limit: MAX_COLLECTION });
    }
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_data_value(value, cx)?);
    }
    Ok(decoded)
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
    use crate::product_alloc::{AllocationFailureKind, AllocationSite};
    use crate::product_json::{self, Cause};
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
        serde_json::from_str(source).unwrap()
    }

    fn allocation(error: ValueDecodeError, cx: &AllocationContext, site: AllocationSite) {
        let ValueDecodeError::Allocation(failure) = error else {
            panic!("expected allocation refusal")
        };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
        assert_eq!(failure.site, site);
    }

    #[test]
    fn type_leaf_matches_serde_and_refuses_each_box_growth() {
        let source = r#"{"kind":"optional","item":{"kind":"list","item":{"kind":"reference","entity":"person"}}}"#;
        let expected: Type = typed(source);

        let mut cx = AllocationContext::new();
        let decoded = decode_type(syntax(source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        let growths = cx.growth_requests();
        assert_eq!(growths, 2);

        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_type(syntax(source), &mut cx).unwrap_err(), &cx, AllocationSite::Box);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_type(syntax(source), &mut retry).unwrap(), expected);
        }
    }

    #[test]
    fn recursive_data_values_match_serde_and_refuse_every_list_growth() {
        let source = r#"{"kind":"list","item_type":{"kind":"optional","item":{"kind":"text"}},"items":[{"kind":"text","value":"雪\n"},{"kind":"list","item_type":{"kind":"integer"},"items":[{"kind":"integer","value":-9}]}]}"#;
        let expected: DataValue = typed(source);

        let mut cx = AllocationContext::new();
        let decoded = decode_data_value(syntax(source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        let growths = cx.growth_requests();
        assert_eq!(growths, 3); // one Box and both list vectors

        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = decode_data_value(syntax(source), &mut cx).unwrap_err();
            let site = match nth {
                1 => AllocationSite::Box,
                2 | 3 => AllocationSite::Vector,
                _ => unreachable!(),
            };
            allocation(error, &cx, site);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_data_value(syntax(source), &mut retry).unwrap(), expected);
        }
    }

    #[test]
    fn values_map_is_sorted_wire_compatible_and_refuses_each_growth() {
        let source = r#"{"zeta":{"kind":"text","value":"last"},"alpha":{"kind":"reference","entity":"person","record":"p1"}}"#;
        let expected: BTreeMap<String, DataValue> = typed(source);

        let mut cx = AllocationContext::new();
        let decoded = decode_values(syntax(source), &mut cx).unwrap();
        assert_eq!(decoded.iter().map(|(key, _)| key.as_str()).collect::<Vec<_>>(), ["alpha", "zeta"]);
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert_eq!(cx.growth_requests(), 2);

        for nth in 1..=2 {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_values(syntax(source), &mut cx).unwrap_err(), &cx, AllocationSite::OrderedMap);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_values(syntax(source), &mut retry).unwrap().len(), 2);
        }
    }

    #[test]
    fn duplicate_members_are_rejected_before_typed_value_decode() {
        let mut cx = AllocationContext::new();
        let error = match product_json::parse_json_bytes(
            br#"{"same":{"kind":"null"},"same":{"kind":"boolean"}}"#,
            &mut cx,
        ) {
            Err(error) => error,
            Ok(_) => panic!("duplicate object key was accepted"),
        };
        assert_eq!(error.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn wrong_shapes_unknown_fields_variants_and_integer_ranges_are_typed_errors() {
        assert!(matches!(decode_type(syntax(r#"{"kind":"future"}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"future"}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_type(syntax(r#"{"kind":"text","extra":1}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"boolean","value":1}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidType { actual: JsonKind::UnsignedInteger, .. })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"integer","value":9223372036854775808}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(_), expected: "i64" })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"date","days":2147483648}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(_), expected: "i32" })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"list","item_type":{"kind":"text"},"items":false}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidType { actual: JsonKind::Boolean, expected: "a sequence" })));
        assert!(matches!(decode_values(syntax(r#"[]"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::InvalidType { actual: JsonKind::Sequence, expected: "a map" })));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"reference","entity":"person"}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::MissingField("record"))));
        assert!(matches!(decode_data_value(syntax(r#"{"kind":"text","extra":1,"value":"ok"}"#), &mut AllocationContext::new()),
            Err(ValueDecodeError::UnknownField { .. })));
    }

    #[test]
    fn all_scalar_and_unit_variant_shapes_match_serde() {
        for source in [
            r#"{"kind":"boolean"}"#,
            r#"{"kind":"integer"}"#,
            r#"{"kind":"text"}"#,
            r#"{"kind":"date"}"#,
            r#"{"kind":"reference","entity":"person"}"#,
            r#"{"kind":"list","item":{"kind":"text"}}"#,
            r#"{"kind":"optional","item":{"kind":"integer"}}"#,
        ] {
            let expected: Type = typed(source);
            let mut cx = AllocationContext::new();
            let decoded = decode_type(syntax(source), &mut cx).unwrap();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        }

        for source in [
            r#"{"kind":"null"}"#,
            r#"{"kind":"boolean","value":true}"#,
            r#"{"kind":"integer","value":-9223372036854775808}"#,
            r#"{"kind":"text","value":"café"}"#,
            r#"{"kind":"date","days":-2147483648}"#,
            r#"{"kind":"reference","entity":"person","record":"p1"}"#,
            r#"{"kind":"list","item_type":{"kind":"text"},"items":[]}"#,
        ] {
            let expected: DataValue = typed(source);
            let mut cx = AllocationContext::new();
            let decoded = decode_data_value(syntax(source), &mut cx).unwrap();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        }
    }
}
