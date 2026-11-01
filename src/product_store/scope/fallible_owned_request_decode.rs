//! Allocation-aware DTO decoding for owned preview/effect request graphs.
//! This module does not change production request routing.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::fallible_expr_decode::decode_expr;
use crate::product_contract::fallible_value_decode::{JsonKind, ValueDecodeError};
use crate::product_contract::{decode_record_ref_fallible, Digest, Expr, RecordRef, MAX_COLLECTION, MAX_ITEMS, MAX_TEXT_BYTES};
use crate::product_json::{Number, Value};
use serde::Serialize;
use super::fallible_owned_source_decode::{decode_owned_source_request, DecodedOwnedSourceRequest};
use super::fallible_scalar_decode::{decode_operation_set, decode_scope_population, FallibleIdSet, ScopePopulationDto};

const INVENTORY_REQUEST_FIELDS: &[&str] = &["snapshot", "sources", "lifecycle"];
const LIFECYCLE_SUGGESTION_FIELDS: &[&str] = &["entity", "before", "after", "meaning"];
const PREVIEW_REQUEST_FIELDS: &[&str] = &["inventory", "auxiliary", "population", "excluded_records", "operations", "availability", "purpose"];
const EFFECT_REQUEST_FIELDS: &[&str] = &["snapshot", "sources", "auxiliary"];
const AUXILIARY_FIELDS: &[&str] = &["entity", "owner_field"];
const AVAILABILITY_VARIANTS: &[&str] = &["nullable_added_boolean"];

#[derive(Debug, Serialize)]
struct DecodedCommonFactLifecycleSuggestion { entity: String, before: Expr, after: Expr, meaning: String }
#[derive(Debug, Serialize)]
struct DecodedCommonFactInventoryRequest { snapshot: Digest, sources: DecodedOwnedSourceRequest, lifecycle: DecodedCommonFactLifecycleSuggestion }
#[derive(Debug, Serialize)]
struct DecodedOwnedAuxiliary { entity: String, owner_field: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DecodedOwnedPreviewAvailability { NullableAddedBoolean }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedPreviewRequest {
    inventory: DecodedCommonFactInventoryRequest, auxiliary: Vec<DecodedOwnedAuxiliary>,
    population: ScopePopulationDto, excluded_records: Vec<RecordRef>, operations: FallibleIdSet,
    availability: DecodedOwnedPreviewAvailability, purpose: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedEffectRequest { snapshot: Digest, sources: DecodedOwnedSourceRequest, auxiliary: Vec<DecodedOwnedAuxiliary> }

fn decode_owned_preview_availability(value: Value) -> Result<DecodedOwnedPreviewAvailability, ValueDecodeError> {
    match value {
        Value::String(variant) => preview_availability_variant(variant),
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (variant, value) = fields.into_entries().next().expect("one enum map entry");
            let decoded = preview_availability_variant(variant)?;
            if !matches!(value, Value::Null) {
                return Err(invalid_type(value, "unit"));
            }
            Ok(decoded)
        }
        value => Err(invalid_type(value, "a string variant identifier or map")),
    }
}

fn preview_availability_variant(variant: String) -> Result<DecodedOwnedPreviewAvailability, ValueDecodeError> {
    if AVAILABILITY_VARIANTS.contains(&variant.as_str()) {
        Ok(DecodedOwnedPreviewAvailability::NullableAddedBoolean)
    } else {
        Err(ValueDecodeError::UnknownVariant { tag: variant, expected: AVAILABILITY_VARIANTS })
    }
}

fn decode_common_fact_lifecycle_suggestion(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactLifecycleSuggestion, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactLifecycleSuggestion", LIFECYCLE_SUGGESTION_FIELDS, cx)?;
    let (mut entity, mut before, mut after, mut meaning) = (None, None, None, None);
    for (key, value) in fields { match key.as_str() {
        "entity" => entity = Some(string(value, "a string")?), "before" => before = Some(decode_expr(value, cx)?),
        "after" => after = Some(decode_expr(value, cx)?),
        "meaning" => { let value = string(value, "a string")?; if value.len() > MAX_TEXT_BYTES { return Err(limit("common fact lifecycle meaning bytes", MAX_TEXT_BYTES)); } meaning = Some(value); },
        _ => return Err(unknown(key, LIFECYCLE_SUGGESTION_FIELDS)),
    }}
    Ok(DecodedCommonFactLifecycleSuggestion { entity: required(entity, "entity")?, before: required(before, "before")?, after: required(after, "after")?, meaning: required(meaning, "meaning")? })
}

fn decode_common_fact_inventory_request(value: Value, cx: &mut AllocationContext) -> Result<DecodedCommonFactInventoryRequest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct CommonFactInventoryRequest", INVENTORY_REQUEST_FIELDS, cx)?;
    let (mut snapshot, mut sources, mut lifecycle) = (None, None, None);
    for (key, value) in fields { match key.as_str() {
        "snapshot" => snapshot = Some(digest(value)?), "sources" => sources = Some(decode_owned_source_request(value, cx)?),
        "lifecycle" => lifecycle = Some(decode_common_fact_lifecycle_suggestion(value, cx)?),
        _ => return Err(unknown(key, INVENTORY_REQUEST_FIELDS)),
    }}
    Ok(DecodedCommonFactInventoryRequest { snapshot: required(snapshot, "snapshot")?, sources: required(sources, "sources")?, lifecycle: required(lifecycle, "lifecycle")? })
}

fn decode_owned_auxiliary(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedAuxiliary, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedAuxiliary", AUXILIARY_FIELDS, cx)?;
    let (mut entity, mut owner_field) = (None, None);
    for (key, value) in fields { match key.as_str() {
        "entity" => entity = Some(string(value, "a string")?), "owner_field" => owner_field = Some(string(value, "a string")?),
        _ => return Err(unknown(key, AUXILIARY_FIELDS)),
    }}
    Ok(DecodedOwnedAuxiliary { entity: required(entity, "entity")?, owner_field: required(owner_field, "owner_field")? })
}

pub(crate) fn decode_owned_preview_request(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedPreviewRequest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedPreviewRequest", PREVIEW_REQUEST_FIELDS, cx)?;
    let (mut inventory, mut auxiliary, mut population, mut excluded_records, mut operations, mut availability, mut purpose) = (None, None, None, None, None, None, None);
    for (key, value) in fields { match key.as_str() {
        "inventory" => inventory = Some(decode_common_fact_inventory_request(value, cx)?), "auxiliary" => auxiliary = Some(auxiliaries(value, cx)?),
        "population" => population = Some(decode_scope_population(value, cx)?), "excluded_records" => excluded_records = Some(record_refs(value, cx)?),
        "operations" => operations = Some(decode_operation_set(value, cx)?), "availability" => availability = Some(decode_owned_preview_availability(value)?),
        "purpose" => { let value = string(value, "a string")?; if value.len() > MAX_TEXT_BYTES { return Err(limit("owned preview purpose bytes", MAX_TEXT_BYTES)); } purpose = Some(value); },
        _ => return Err(unknown(key, PREVIEW_REQUEST_FIELDS)),
    }}
    Ok(DecodedOwnedPreviewRequest { inventory: required(inventory, "inventory")?, auxiliary: required(auxiliary, "auxiliary")?, population: required(population, "population")?, excluded_records: required(excluded_records, "excluded_records")?, operations: required(operations, "operations")?, availability: required(availability, "availability")?, purpose: required(purpose, "purpose")? })
}

pub(crate) fn decode_owned_effect_request(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedEffectRequest, ValueDecodeError> {
    let fields = struct_fields(value, "a struct OwnedEffectRequest", EFFECT_REQUEST_FIELDS, cx)?;
    let (mut snapshot, mut sources, mut auxiliary) = (None, None, None);
    for (key, value) in fields { match key.as_str() {
        "snapshot" => snapshot = Some(digest(value)?), "sources" => sources = Some(decode_owned_source_request(value, cx)?),
        "auxiliary" => auxiliary = Some(auxiliaries(value, cx)?), _ => return Err(unknown(key, EFFECT_REQUEST_FIELDS)),
    }}
    Ok(DecodedOwnedEffectRequest { snapshot: required(snapshot, "snapshot")?, sources: required(sources, "sources")?, auxiliary: required(auxiliary, "auxiliary")? })
}

fn auxiliaries(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedOwnedAuxiliary>, ValueDecodeError> {
    decode_vec(value, MAX_ITEMS, "owned auxiliary declaration count", cx, decode_owned_auxiliary)
}
fn record_refs(value: Value, cx: &mut AllocationContext) -> Result<Vec<RecordRef>, ValueDecodeError> {
    decode_vec(value, MAX_COLLECTION, "owned preview excluded record count", cx, decode_record_ref_fallible)
}
fn decode_vec<T>(value: Value, max: usize, resource: &'static str, cx: &mut AllocationContext, mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value { Value::Array(values) => values, value => return Err(invalid_type(value, "a sequence")) };
    bounded(values.len(), max, resource)?;
    let mut out = Vec::new(); product_alloc::try_reserve(&mut out, values.len(), cx, AllocationSite::Vector)?;
    for value in values { out.push(decode(value, cx)?); } Ok(out)
}
fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str], cx: &mut AllocationContext) -> Result<std::vec::IntoIter<(String, Value)>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields.into_entries()),
        Value::Array(values) => {
            if values.len() != names.len() { return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() }); }
            let mut fields = Vec::new(); product_alloc::try_reserve(&mut fields, names.len(), cx, AllocationSite::Vector)?;
            for (name, value) in names.iter().zip(values) { fields.push((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value)); }
            Ok(fields.into_iter())
        }
        value => Err(invalid_type(value, expected)),
    }
}
fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    Digest::from_owned_lower_hex(string(value, "a string")?).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> { match value { Value::String(value) => Ok(value), value => Err(invalid_type(value, expected)) } }
fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value { Value::Null => JsonKind::Null, Value::Bool(_) => JsonKind::Boolean, Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger, Value::Number(Number::I64(_)) => JsonKind::SignedInteger, Value::Number(Number::F64(_)) => JsonKind::FloatingPoint, Value::String(_) => JsonKind::String, Value::Array(_) => JsonKind::Sequence, Value::Object(_) => JsonKind::Map };
    ValueDecodeError::InvalidType { actual, expected }
}
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn bounded(len: usize, max: usize, resource: &'static str) -> Result<(), ValueDecodeError> { if len > max { Err(limit(resource, max)) } else { Ok(()) } }
fn limit(resource: &'static str, max: usize) -> ValueDecodeError { ValueDecodeError::LimitExceeded { resource, limit: max } }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::{canonical_bytes, fallible_value_decode::ValueDecodeError};
    use crate::product_json::{self, Value};
    use crate::product_store::scope::common_fact_admission::{OwnedPreviewAvailability, OwnedPreviewRequest};
    use crate::product_store::scope::common_fact_inventory::{CommonFactInventoryRequest, CommonFactLifecycleSuggestion};
    use crate::product_store::scope::owned_effects::{OwnedAuxiliary, OwnedEffectRequest};
    use crate::product_store::scope::fallible_owned_source_decode::decode_owned_source_request;
    use serde::Serialize;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }
    fn digest(ch: char) -> String { std::iter::repeat(ch).take(64).collect() }
    fn source_request() -> serde_json::Value {
        serde_json::json!({"before":digest('1'),"after":digest('2'),"root_entity":"person","edits":[]})
    }
    fn lifecycle() -> serde_json::Value {
        let predicate = serde_json::json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}});
        serde_json::json!({"entity":"person","before":predicate,"after":predicate,"meaning":"The row is complete."})
    }
    fn auxiliary() -> serde_json::Value { serde_json::json!({"entity":"person_note","owner_field":"owner_id"}) }
    fn nested_value() -> serde_json::Value {
        serde_json::json!({
            "inventory":{"snapshot":digest('3'),"sources":source_request(),"lifecycle":lifecycle()},
            "auxiliary":[auxiliary()],
            "population":{"kind":"selected_unfinished","records":[{"entity":"person","record":"p1"}]},
            "excluded_records":[{"entity":"person","record":"p2"}],
            "operations":["write_z","write_a","write_z"],
            "availability":"nullable_added_boolean",
            "purpose":"preview independently"
        })
    }

    #[test]
    fn owned_preview_and_effect_requests_match_canonical_serde_bytes() {
        let value = nested_value();
        let expected: OwnedPreviewRequest = serde_json::from_value(value.clone()).unwrap();
        let decoded = decode_owned_preview_request(syntax(&serde_json::to_string(&value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));
        assert_eq!(decoded.operations.iter().map(String::as_str).collect::<Vec<_>>(), ["write_a", "write_z"]);
        let expected_v2: crate::product_store::scope::owned_wire::OwnedScopeRequestV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected_v2), canonical(&decoded));

        let effect_value = serde_json::json!({"snapshot":digest('4'),"sources":source_request(),"auxiliary":[auxiliary()]});
        let expected_effect: OwnedEffectRequest = serde_json::from_value(effect_value.clone()).unwrap();
        let decoded_effect = decode_owned_effect_request(syntax(&serde_json::to_string(&effect_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_effect), canonical(&decoded_effect));
    }

    #[test]
    fn each_owned_preview_request_leaf_matches_canonical_serde_bytes() {
        let availability: OwnedPreviewAvailability = serde_json::from_str("\"nullable_added_boolean\"").unwrap();
        assert_eq!(canonical(&availability), canonical(&decode_owned_preview_availability(syntax("\"nullable_added_boolean\"")).unwrap()));
        let map_availability = serde_json::json!({"nullable_added_boolean":null});
        let expected_map: OwnedPreviewAvailability = serde_json::from_value(map_availability.clone()).unwrap();
        assert_eq!(canonical(&expected_map), canonical(&decode_owned_preview_availability(syntax(&serde_json::to_string(&map_availability).unwrap())).unwrap()));
        assert!(matches!(decode_owned_preview_availability(syntax(r#"{"future":null}"#)), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(decode_owned_preview_availability(syntax(r#"{"nullable_added_boolean":false}"#)).is_err());
        let source_value = source_request();
        let expected_source: crate::product_store::scope::fallible_owned_source_decode::DecodedOwnedSourceRequest = decode_owned_source_request(syntax(&serde_json::to_string(&source_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_source), canonical(&decode_owned_source_request(syntax(&serde_json::to_string(&source_value).unwrap()), &mut AllocationContext::new()).unwrap()));

        let lifecycle_value = lifecycle();
        let expected_lifecycle: CommonFactLifecycleSuggestion = serde_json::from_value(lifecycle_value.clone()).unwrap();
        let decoded_lifecycle = decode_common_fact_lifecycle_suggestion(syntax(&serde_json::to_string(&lifecycle_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_lifecycle), canonical(&decoded_lifecycle));
        let inventory_value = serde_json::json!({"snapshot":digest('3'),"sources":source_value,"lifecycle":lifecycle_value});
        let expected_inventory: CommonFactInventoryRequest = serde_json::from_value(inventory_value.clone()).unwrap();
        let decoded_inventory = decode_common_fact_inventory_request(syntax(&serde_json::to_string(&inventory_value).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_inventory), canonical(&decoded_inventory));

        let auxiliary_value = auxiliary();
        let expected_auxiliary: OwnedAuxiliary = serde_json::from_value(auxiliary_value.clone()).unwrap();
        assert_eq!(canonical(&expected_auxiliary), canonical(&decode_owned_auxiliary(syntax(&serde_json::to_string(&auxiliary_value).unwrap()), &mut AllocationContext::new()).unwrap()));
    }

    #[test]
    fn optional_defaults_sequence_forms_and_duplicate_operation_semantics_match_serde() {
        let source = r#"{"inventory":{"snapshot":"3333333333333333333333333333333333333333333333333333333333333333","sources":{"before":"1111111111111111111111111111111111111111111111111111111111111111","after":"2222222222222222222222222222222222222222222222222222222222222222","root_entity":"person","edits":[]},"lifecycle":{"entity":"person","before":{"kind":"today"},"after":{"kind":"today"},"meaning":"ok"}},"auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":["b","a","b"],"availability":"nullable_added_boolean","purpose":"preview"}"#;
        let expected: OwnedPreviewRequest = serde_json::from_str(source).unwrap();
        let decoded = decode_owned_preview_request(syntax(source), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));

        let sequence = serde_json::json!([
            [digest('3'),[digest('1'),digest('2'),"person",[]],["person",{"kind":"today"},{"kind":"today"},"ok"]],
            [],{"kind":"all"},[],["b","a","b"],"nullable_added_boolean","purpose"
        ]);
        let expected_sequence: OwnedPreviewRequest = serde_json::from_value(sequence.clone()).unwrap();
        let decoded_sequence = decode_owned_preview_request(syntax(&serde_json::to_string(&sequence).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_sequence), canonical(&decoded_sequence));
        let request_sequence = serde_json::json!([digest('1'),digest('2'),"person",[]]);
        let expected_source: crate::product_store::scope::owned_contract::OwnedSourceRequest = serde_json::from_value(request_sequence.clone()).unwrap();
        let decoded_source = decode_owned_source_request(syntax(&serde_json::to_string(&request_sequence).unwrap()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected_source), canonical(&decoded_source));
    }

    #[test]
    fn malformed_unknown_wrong_type_and_collection_bounds_are_rejected() {
        assert!(matches!(decode_owned_preview_availability(syntax("\"future\"")), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(decode_owned_preview_request(syntax(r#"{"inventory":null,"auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":[],"availability":"nullable_added_boolean","purpose":"p","extra":0}"#), &mut AllocationContext::new()).is_err());
        assert!(decode_owned_preview_request(syntax(r#"{"inventory":{"snapshot":"bad"},"auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":[],"availability":"nullable_added_boolean","purpose":"p"}"#), &mut AllocationContext::new()).is_err());
        assert!(decode_common_fact_lifecycle_suggestion(syntax(r#"{"entity":"person","before":false,"after":{"kind":"today"},"meaning":"ok"}"#), &mut AllocationContext::new()).is_err());
        let mut missing_purpose = nested_value();
        missing_purpose.as_object_mut().unwrap().remove("purpose");
        assert!(matches!(decode_owned_preview_request(syntax(&serde_json::to_string(&missing_purpose).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("purpose"))));
        let overlong_purpose = serde_json::json!({"inventory":{"snapshot":digest('3'),"sources":source_request(),"lifecycle":lifecycle()},"auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":[],"availability":"nullable_added_boolean","purpose":"x".repeat(crate::product_contract::MAX_TEXT_BYTES+1)});
        assert!(matches!(decode_owned_preview_request(syntax(&serde_json::to_string(&overlong_purpose).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));
        let overlong_meaning = serde_json::json!({"entity":"person","before":{"kind":"today"},"after":{"kind":"today"},"meaning":"é".repeat(crate::product_contract::MAX_TEXT_BYTES)});
        assert!(matches!(decode_common_fact_lifecycle_suggestion(syntax(&serde_json::to_string(&overlong_meaning).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));
        assert!(product_json::parse_json_bytes(br#"{"snapshot":"a","snapshot":"b"}"#, &mut AllocationContext::new()).is_err());

        let too_many_aux = serde_json::json!({"snapshot":digest('4'),"sources":source_request(),"auxiliary":vec![auxiliary(); crate::product_contract::MAX_ITEMS+1]});
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_effect_request(syntax(&serde_json::to_string(&too_many_aux).unwrap()), &mut cx), Err(ValueDecodeError::LimitExceeded { .. })));
        assert_eq!(cx.growth_requests(), 0);
        let excluded = (0..crate::product_contract::MAX_COLLECTION+1).map(|_| serde_json::json!({"entity":"person","record":"p"})).collect::<Vec<_>>();
        let too_many_excluded = serde_json::json!({"inventory":{"snapshot":digest('3'),"sources":source_request(),"lifecycle":lifecycle()},"auxiliary":[],"population":{"kind":"all"},"excluded_records":excluded,"operations":[],"availability":"nullable_added_boolean","purpose":"p"});
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_owned_preview_request(syntax(&serde_json::to_string(&too_many_excluded).unwrap()), &mut cx), Err(ValueDecodeError::LimitExceeded { .. })));
        assert_eq!(cx.growth_requests(), 0);
        let operations = serde_json::Value::Array((0..crate::product_contract::MAX_ITEMS+1).map(|n| serde_json::json!(format!("operation_{n}"))).collect());
        assert!(matches!(decode_operation_set(syntax(&serde_json::to_string(&operations).unwrap()), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { .. })));
    }

    #[test]
    fn every_reached_owned_request_growth_refusal_is_retryable_and_cleanup_safe() {
        let values = [
            nested_value(),
            serde_json::json!({"snapshot":digest('4'),"sources":source_request(),"auxiliary":[auxiliary()]}),
            serde_json::json!({"snapshot":digest('3'),"sources":source_request(),"lifecycle":lifecycle()}),
            serde_json::json!([
                [digest('3'),[digest('1'),digest('2'),"person",[]],["person",{"kind":"today"},{"kind":"today"},"ok"]],
                [],{"kind":"all"},[],["b","a","b"],"nullable_added_boolean","purpose"
            ]),
        ];
        let decoders: [fn(Value, &mut AllocationContext) -> Result<(), ValueDecodeError>; 4] = [
            |v,c| decode_owned_preview_request(v,c).map(|_|()),
            |v,c| decode_owned_effect_request(v,c).map(|_|()),
            |v,c| decode_common_fact_inventory_request(v,c).map(|_|()),
            |v,c| decode_owned_preview_request(v,c).map(|_|()),
        ];
        for (value, decode) in values.into_iter().zip(decoders) {
            let source = serde_json::to_string(&value).unwrap();
            let mut count = AllocationContext::new();
            decode(syntax(&source), &mut count).unwrap();
            let growths = count.growth_requests();
            if growths == 0 { continue; }
            for nth in 1..=growths {
                let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
                assert!(matches!(decode(syntax(&source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {nth}/{growths}");
                decode(syntax(&source), &mut AllocationContext::new()).unwrap();
            }
        }
    }
}
