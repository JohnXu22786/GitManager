//! Allocation-aware structural decoding for the persisted intention bundle.
//!
//! This remains a private leaf; production backup intake and validation are
//! intentionally unchanged in this unit.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::{Digest, MAX_ITEMS};
use crate::product_contract::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use crate::product_json::{Number, Value};

use super::{BundledObject, IntentionBundle, MAX_BUNDLE_BYTES};

const BUNDLE_FIELDS: &[&str] = &["version", "snapshot", "objects"];
const OBJECT_FIELDS: &[&str] = &["digest", "bytes"];
const MAX_BUNDLE_OBJECTS: usize = MAX_ITEMS * 2;

pub(super) fn decode_intention_bundle(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<IntentionBundle, ValueDecodeError> {
    decode_intention_bundle_with_limits(value, cx, MAX_BUNDLE_OBJECTS, MAX_BUNDLE_BYTES)
}

fn decode_intention_bundle_with_limits(
    value: Value,
    cx: &mut AllocationContext,
    max_objects: usize,
    max_bundle_bytes: usize,
) -> Result<IntentionBundle, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct IntentionBundle", BUNDLE_FIELDS)?;
    let sequence_len = fields.sequence_len;
    let mut version = None;
    let mut snapshot = None;
    let mut objects = None;
    let mut byte_budget = max_bundle_bytes;
    for (key, value) in fields.iter.by_ref() {
        match key.as_str() {
            "version" => version = Some(integer_u32(value)?),
            "snapshot" => snapshot = Some(digest(value)?),
            "objects" => objects = Some(bundled_objects(value, cx, max_objects, &mut byte_budget)?),
            _ => return Err(key.unknown_field(BUNDLE_FIELDS)),
        }
    }
    check_sequence_length(sequence_len, BUNDLE_FIELDS.len())?;
    Ok(IntentionBundle {
        version: required(version, "version")?,
        snapshot: required(snapshot, "snapshot")?,
        objects: required(objects, "objects")?,
    })
}

fn bundled_objects(
    value: Value,
    cx: &mut AllocationContext,
    max_objects: usize,
    byte_budget: &mut usize,
) -> Result<Vec<BundledObject>, ValueDecodeError> {
    let values = array(value)?;
    if values.len() > max_objects {
        return Err(ValueDecodeError::LimitExceeded {
            resource: "intention object count",
            limit: max_objects,
        });
    }
    let mut objects = Vec::new();
    product_alloc::try_reserve(&mut objects, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        let object = bundled_object(value, cx, byte_budget)?;
        product_alloc::try_push(&mut objects, object, cx, AllocationSite::Vector)?;
    }
    Ok(objects)
}

fn bundled_object(
    value: Value,
    cx: &mut AllocationContext,
    byte_budget: &mut usize,
) -> Result<BundledObject, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct BundledObject", OBJECT_FIELDS)?;
    let sequence_len = fields.sequence_len;
    let mut object_digest = None;
    let mut bytes = None;
    for (key, value) in fields.iter.by_ref() {
        match key.as_str() {
            "digest" => object_digest = Some(digest(value)?),
            "bytes" => bytes = Some(byte_sequence(value, cx, byte_budget)?),
            _ => return Err(key.unknown_field(OBJECT_FIELDS)),
        }
    }
    check_sequence_length(sequence_len, OBJECT_FIELDS.len())?;
    Ok(BundledObject {
        digest: required(object_digest, "digest")?,
        bytes: required(bytes, "bytes")?,
    })
}

fn byte_sequence(
    value: Value,
    cx: &mut AllocationContext,
    byte_budget: &mut usize,
) -> Result<Vec<u8>, ValueDecodeError> {
    let values = array(value)?;
    if values.len() > *byte_budget {
        return Err(ValueDecodeError::LimitExceeded {
            resource: "intention bundle byte count",
            limit: MAX_BUNDLE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    product_alloc::try_reserve(&mut bytes, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        let byte = integer_u8(value)?;
        product_alloc::try_push(&mut bytes, byte, cx, AllocationSite::Vector)?;
    }
    *byte_budget -= bytes.len();
    Ok(bytes)
}

struct ParsedFields {
    iter: FieldIter,
    sequence_len: Option<usize>,
}

enum FieldIter {
    Map(std::vec::IntoIter<(String, Value)>),
    Sequence { names: &'static [&'static str], values: std::vec::IntoIter<Value>, index: usize },
}

enum FieldKey {
    Static(&'static str),
    Owned(String),
}

impl FieldKey {
    fn as_str(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Owned(value) => value,
        }
    }

    fn unknown_field(self, expected: &'static [&'static str]) -> ValueDecodeError {
        match self {
            Self::Owned(key) => ValueDecodeError::UnknownField { key, expected },
            Self::Static(_) => unreachable!("sequence field names come from the known schema"),
        }
    }
}

impl Iterator for FieldIter {
    type Item = (FieldKey, Value);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Map(fields) => fields.next().map(|(key, value)| (FieldKey::Owned(key), value)),
            Self::Sequence { names, values, index } => {
                let name = names.get(*index)?;
                let value = values.next()?;
                *index += 1;
                Some((FieldKey::Static(name), value))
            }
        }
    }
}

fn struct_fields(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
) -> Result<ParsedFields, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(ParsedFields { iter: FieldIter::Map(fields.into_entries()), sequence_len: None }),
        Value::Array(values) => {
            let sequence_len = values.len();
            Ok(ParsedFields {
                iter: FieldIter::Sequence { names, values: values.into_iter(), index: 0 },
                sequence_len: Some(sequence_len),
            })
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn check_sequence_length(actual: Option<usize>, expected: usize) -> Result<(), ValueDecodeError> {
    if let Some(actual) = actual {
        if actual != expected {
            return Err(ValueDecodeError::InvalidLength { actual, expected });
        }
    }
    Ok(())
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value)?;
    Digest::from_owned_lower_hex(value)
        .ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}

fn string(value: Value) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}

fn integer_u8(value: Value) -> Result<u8, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u8::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u8" }
        }),
        Value::Number(Number::I64(value)) => u8::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u8" }
        }),
        value => Err(invalid_type(value, "u8")),
    }
}

fn integer_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }
        }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }
        }),
        value => Err(invalid_type(value, "u32")),
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

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_contract::{canonical_bytes, canonical_digest, IdentityDomain};
    use crate::product_json;
    use std::num::NonZeroUsize;

    const SNAPSHOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OBJECT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn fixture(objects: &str) -> String {
        format!(r#"{{"version":1,"snapshot":"{SNAPSHOT}","objects":{objects}}}"#)
    }

    fn bundle(objects: Vec<(Digest, Vec<u8>)>) -> IntentionBundle {
        IntentionBundle {
            version: 1,
            snapshot: Digest::try_from(SNAPSHOT.to_owned()).unwrap(),
            objects: objects.into_iter().map(|(digest, bytes)| BundledObject { digest, bytes }).collect(),
        }
    }

    fn assert_allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(cx.first_failure(), Some(failure));
    }

    #[test]
    fn empty_and_populated_bundle_round_trip_bytes_and_digest() {
        for (source, expected) in [
            (fixture("[]"), bundle(vec![])),
            (fixture(&format!(r#"[{{"digest":"{OBJECT}","bytes":[0,1,255]}}]"#)),
             bundle(vec![(Digest::try_from(OBJECT.to_owned()).unwrap(), vec![0, 1, 255])])),
        ] {
            let decoded = decode_intention_bundle(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical_bytes(&decoded).unwrap(), canonical_bytes(&expected).unwrap());
            assert_eq!(canonical_digest(IdentityDomain::Evidence, &decoded).unwrap(), canonical_digest(IdentityDomain::Evidence, &expected).unwrap());
        }
    }

    #[test]
    fn sequence_struct_forms_match_serde() {
        let source = format!(r#"[1,"{SNAPSHOT}",[["{OBJECT}",[0,1,255]]]]"#);
        let expected = bundle(vec![(Digest::try_from(OBJECT.to_owned()).unwrap(), vec![0, 1, 255])]);
        let decoded = decode_intention_bundle(syntax(&source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical_bytes(&decoded).unwrap(), canonical_bytes(&expected).unwrap());
        assert_eq!(canonical_digest(IdentityDomain::Evidence, &decoded).unwrap(), canonical_digest(IdentityDomain::Evidence, &expected).unwrap());
        assert!(matches!(decode_intention_bundle(syntax(&format!(r#"[1,"{SNAPSHOT}",[["{OBJECT}",[0]]]]"#)), &mut AllocationContext::new()), Ok(_)));
        assert!(matches!(decode_intention_bundle(syntax(&format!(r#"[1,"{SNAPSHOT}",[["{OBJECT}"]]]"#)), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 1, expected: 2 })));
        assert!(matches!(decode_intention_bundle(syntax(&format!(r#"[1,"{SNAPSHOT}"]"#)), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 2, expected: 3 })));
        assert!(matches!(decode_intention_bundle(syntax("[null]"), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));
        assert!(matches!(decode_intention_bundle(syntax(&format!(r#"[null,"{SNAPSHOT}",[],4]"#)), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));
    }

    #[test]
    fn malformed_unknown_missing_and_duplicate_inputs_are_rejected() {
        assert!(matches!(decode_intention_bundle(syntax(r#"{"version":1,"snapshot":"bad","objects":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));
        assert!(matches!(decode_intention_bundle(syntax(r#"{"version":1,"snapshot":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","objects":[],"extra":null}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        assert!(matches!(decode_intention_bundle(syntax(r#"{"version":1,"objects":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("snapshot"))));
        assert!(matches!(decode_intention_bundle(syntax(r#"{"version":1,"snapshot":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","objects":[{"digest":"bad","bytes":[]}] }"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));
        assert!(matches!(decode_intention_bundle(syntax(r#"{"version":1,"snapshot":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","objects":[{"digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}] }"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("bytes"))));
        assert!(matches!(decode_intention_bundle(syntax(&fixture(&format!(r#"[{{"digest":"{OBJECT}","bytes":[-1]}}]"#))), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u8", .. })));
        assert!(matches!(decode_intention_bundle(syntax(&fixture(&format!(r#"[{{"digest":"{OBJECT}","bytes":[256]}}]"#))), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u8", .. })));
        let duplicate = format!(r#"{{"version":1,"snapshot":"{SNAPSHOT}","snapshot":"{SNAPSHOT}","objects":[]}}"#);
        assert!(product_json::parse_json_bytes(duplicate.as_bytes(), &mut AllocationContext::new()).is_err());
    }

    #[test]
    fn object_count_and_bundle_byte_budget_are_bounded_before_growth() {
        let too_many = Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_BUNDLE_OBJECTS + 1).collect());
        let source = Value::Object({
            let mut fields = crate::product_alloc::ordered::OrderedMap::new();
            let mut cx = AllocationContext::new();
            fields.try_insert("version".to_owned(), Value::Number(Number::U64(1)), &mut cx).unwrap();
            fields.try_insert("snapshot".to_owned(), Value::String(SNAPSHOT.to_owned()), &mut cx).unwrap();
            fields.try_insert("objects".to_owned(), too_many, &mut cx).unwrap();
            fields
        });
        assert!(matches!(decode_intention_bundle(source, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "intention object count", limit: MAX_BUNDLE_OBJECTS })));

        let small_budget = syntax(&fixture(&format!(r#"[{{"digest":"{OBJECT}","bytes":[0,1]}}]"#)));
        assert!(matches!(decode_intention_bundle_with_limits(small_budget, &mut AllocationContext::new(), MAX_BUNDLE_OBJECTS, 1), Err(ValueDecodeError::LimitExceeded { resource: "intention bundle byte count", limit: MAX_BUNDLE_BYTES })));
    }

    #[test]
    fn parsed_bundle_input_obeys_the_json_depth_boundary() {
        let nested = |count: usize| format!("{}0{}", "[".repeat(count), "]".repeat(count));
        let at_limit = format!(r#"{{"version":1,"snapshot":"{SNAPSHOT}","objects":[],"extra":{}}}"#, nested(product_json::MAX_NESTING_DEPTH - 1));
        let parsed = syntax(&at_limit);
        assert!(matches!(decode_intention_bundle(parsed, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));

        let over_limit = format!(r#"{{"version":1,"snapshot":"{SNAPSHOT}","objects":[],"extra":{}}}"#, nested(product_json::MAX_NESTING_DEPTH));
        let error = match product_json::parse_json_bytes(over_limit.as_bytes(), &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("over-depth JSON must be refused"),
        };
        assert_eq!(error.cause, product_json::Cause::NestingTooDeep);
    }

    #[test]
    fn each_decoder_growth_refusal_is_fixed_and_retryable() {
        let source = fixture(&format!(r#"[{{"digest":"{OBJECT}","bytes":[0,1,255]}}]"#));
        let parsed = syntax(&source);
        let mut baseline = AllocationContext::new();
        let expected = decode_intention_bundle(parsed, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        let canonical = canonical_bytes(&expected).unwrap();
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_intention_bundle(syntax(&source), &mut cx).unwrap_err(), &cx);
            let retry = decode_intention_bundle(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical_bytes(&retry).unwrap(), canonical);
        }

        let sequence = format!(r#"[1,"{SNAPSHOT}",[["{OBJECT}",[0]]]]"#);
        let parsed = syntax(&sequence);
        let mut baseline = AllocationContext::new();
        decode_intention_bundle(parsed, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert_allocation(decode_intention_bundle(syntax(&sequence), &mut cx).unwrap_err(), &cx);
        }
    }
}
