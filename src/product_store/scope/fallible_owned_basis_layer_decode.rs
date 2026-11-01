//! Fallible structural decoding for persisted OwnedBasisV2 and OwnedLayerV2.
//!
//! The DTOs compose already bounded scope and Product Contract leaves without
//! changing the store's production parser or validation route.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_contract::{
    decode_decision_graph_fallible, decode_local_artifact_fallible,
    Digest, FallibleDecodedDecisionGraph, FallibleDecodedLocalArtifact,
    MAX_ITEMS,
};
use crate::product_contract::fallible_value_decode::{JsonKind, NumericValue, ValueDecodeError};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_owned_inventory_decode::{decode_owned_inventory_v2, DecodedOwnedInventoryV2};
use super::fallible_owned_lifecycle_decode::{decode_owned_lifecycle_v2, DecodedOwnedLifecycleV2};
use super::fallible_owned_request_decode::{decode_owned_preview_request, DecodedOwnedPreviewRequest};
use super::fallible_scalar_decode::{decode_frozen_basis, DecodedFrozenBasis};

const BASIS_FIELDS: &[&str] = &["container_format", "family", "frozen", "sources", "graph", "artifacts"];
const LAYER_FIELDS: &[&str] = &["version", "lowering", "operation", "basis", "candidate", "request", "lifecycle", "inventory"];
const BASIS_FAMILIES: &[&str] = &["scalar_empty", "owned_v2"];

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedBasisV2 {
    container_format: u32,
    family: DecodedOwnedBasisFamilyV2,
    frozen: DecodedFrozenBasis,
    sources: Vec<Digest>,
    graph: FallibleDecodedDecisionGraph,
    artifacts: Vec<FallibleDecodedLocalArtifact>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DecodedOwnedBasisFamilyV2 {
    ScalarEmpty,
    OwnedV2,
}

#[derive(Debug, Serialize)]
pub(crate) struct DecodedOwnedLayerV2 {
    version: u32,
    lowering: u32,
    operation: String,
    basis: DecodedOwnedBasisV2,
    candidate: Digest,
    request: DecodedOwnedPreviewRequest,
    lifecycle: DecodedOwnedLifecycleV2,
    inventory: DecodedOwnedInventoryV2,
}

pub(crate) fn decode_owned_basis_v2(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedBasisV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedBasisV2", BASIS_FIELDS)?;
    let (mut container_format, mut family, mut frozen, mut sources, mut graph, mut artifacts) =
        (None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "container_format" => container_format = Some(u32_value(value)?),
            "family" => family = Some(basis_family(value)?),
            "frozen" => frozen = Some(decode_frozen_basis(value, cx)?),
            "sources" => sources = Some(digest_vec(value, "owned basis source count", cx)?),
            "graph" => graph = Some(decode_decision_graph_fallible(value, cx)?),
            "artifacts" => artifacts = Some(decode_vec(value, "owned basis artifact count", cx, |v, cx| decode_local_artifact_fallible(v, cx))?),
            _ => return Err(unknown(key, BASIS_FIELDS)),
        }
    }
    Ok(DecodedOwnedBasisV2 {
        container_format: required(container_format, "container_format")?,
        family: required(family, "family")?,
        frozen: required(frozen, "frozen")?,
        sources: required(sources, "sources")?,
        graph: required(graph, "graph")?,
        artifacts: required(artifacts, "artifacts")?,
    })
}

pub(crate) fn decode_owned_layer_v2(value: Value, cx: &mut AllocationContext) -> Result<DecodedOwnedLayerV2, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct OwnedLayerV2", LAYER_FIELDS)?;
    let (mut version, mut lowering, mut operation, mut basis, mut candidate, mut request, mut lifecycle, mut inventory) =
        (None, None, None, None, None, None, None, None);
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(u32_value(value)?),
            "lowering" => lowering = Some(u32_value(value)?),
            "operation" => operation = Some(string(value, "a string")?),
            "basis" => basis = Some(decode_owned_basis_v2(value, cx)?),
            "candidate" => candidate = Some(digest(value)?),
            "request" => request = Some(decode_owned_preview_request(value, cx)?),
            "lifecycle" => lifecycle = Some(decode_owned_lifecycle_v2(value, cx)?),
            "inventory" => inventory = Some(decode_owned_inventory_v2(value, cx)?),
            _ => return Err(unknown(key, LAYER_FIELDS)),
        }
    }
    Ok(DecodedOwnedLayerV2 {
        version: required(version, "version")?,
        lowering: required(lowering, "lowering")?,
        operation: required(operation, "operation")?,
        basis: required(basis, "basis")?,
        candidate: required(candidate, "candidate")?,
        request: required(request, "request")?,
        lifecycle: required(lifecycle, "lifecycle")?,
        inventory: required(inventory, "inventory")?,
    })
}

fn basis_family(value: Value) -> Result<DecodedOwnedBasisFamilyV2, ValueDecodeError> {
    match unit_variant(value, BASIS_FAMILIES, "unit enum OwnedBasisFamilyV2")?.as_str() {
        "scalar_empty" => Ok(DecodedOwnedBasisFamilyV2::ScalarEmpty),
        "owned_v2" => Ok(DecodedOwnedBasisFamilyV2::OwnedV2),
        _ => unreachable!("variant was checked"),
    }
}

fn digest_vec(value: Value, resource: &'static str, cx: &mut AllocationContext) -> Result<Vec<Digest>, ValueDecodeError> {
    decode_vec(value, resource, cx, |value, _| digest(value))
}

fn decode_vec<T>(
    value: Value,
    resource: &'static str,
    cx: &mut AllocationContext,
    mut decode: impl FnMut(Value, &mut AllocationContext) -> Result<T, ValueDecodeError>,
) -> Result<Vec<T>, ValueDecodeError> {
    let values = match value {
        Value::Array(values) => values,
        value => return Err(invalid_type(value, "a sequence")),
    };
    if values.len() > MAX_ITEMS {
        return Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS });
    }
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode(value, cx)?);
    }
    Ok(decoded)
}

fn unit_variant(value: Value, variants: &'static [&'static str], expected: &'static str) -> Result<String, ValueDecodeError> {
    let tag = match value {
        Value::String(tag) => tag,
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, payload) = fields.into_entries().next().expect("one enum map entry was checked");
            if !variants.contains(&tag.as_str()) {
                return Err(ValueDecodeError::UnknownVariant { tag, expected: variants });
            }
            match payload {
                Value::Null => tag,
                payload => return Err(invalid_type(payload, "unit")),
            }
        }
        value => return Err(invalid_type(value, expected)),
    };
    if variants.contains(&tag.as_str()) {
        Ok(tag)
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: variants })
    }
}

enum StructFields {
    Object(std::vec::IntoIter<(String, Value)>),
    Sequence {
        names: &'static [&'static str],
        values: std::vec::IntoIter<Value>,
        index: usize,
        actual: usize,
    },
}

impl StructFields {
    fn next(&mut self, cx: &mut AllocationContext) -> Result<Option<(String, Value)>, ValueDecodeError> {
        match self {
            Self::Object(fields) => Ok(fields.next()),
            Self::Sequence { names, values, index, actual } => {
                if let Some(name) = names.get(*index) {
                    let Some(value) = values.next() else {
                        return Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() });
                    };
                    *index += 1;
                    Ok(Some((product_alloc::try_copy_str(name, cx, AllocationSite::String)?, value)))
                } else if values.next().is_some() {
                    Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() })
                } else {
                    Ok(None)
                }
            }
        }
    }
}

fn struct_fields(value: Value, expected: &'static str, names: &'static [&'static str]) -> Result<StructFields, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(StructFields::Object(fields.into_entries())),
        Value::Array(values) => {
            let actual = values.len();
            Ok(StructFields::Sequence { names, values: values.into_iter(), index: 0, actual })
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    Digest::from_owned_lower_hex(string(value, "a lowercase 64-character digest")?)
        .ok_or(ValueDecodeError::InvalidFormat("invalid lowercase digest"))
}
fn string(value: Value, expected: &'static str) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, expected)),
    }
}
fn u32_value(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> { value.ok_or(ValueDecodeError::MissingField(field)) }
fn unknown(key: String, expected: &'static [&'static str]) -> ValueDecodeError { ValueDecodeError::UnknownField { key, expected } }
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
    use crate::product_contract::{canonical_bytes, MAX_ITEMS};
    use crate::product_json;
    use crate::product_store::scope::{OwnedBasisV2, OwnedLayerV2};
    use serde_json::{json, Value as SerdeValue};
    use std::num::NonZeroUsize;

    fn digest(ch: char) -> String { std::iter::repeat(ch).take(64).collect() }
    fn artifact() -> SerdeValue {
        json!({"output":"export","format":"json","columns":[{"id":"title","label":"Title","value_type":{"kind":"text"}}],
            "rows":[{"title":{"kind":"text","value":"hello"}}],"bytes":[123,125],"bytes_digest":digest('8'),"digest":digest('9')})
    }
    fn basis(family: &str) -> SerdeValue {
        json!({"container_format":2,"family":family,
            "frozen":{"snapshot":digest('1'),"revision":3,"active":digest('2'),
                "data":{"version":1,"project_id":"project","generation":4,"schema":[],"records":[],"events":[]},
                "session":{"view":"main","values":{},"focused_record":null},"day":-2,"decisions":digest('3')},
            "sources":[digest('4'),digest('4')],"graph":{"version":1,"revision":0,"decisions":[]},"artifacts":[artifact()]})
    }
    fn preview_request() -> SerdeValue {
        let d = |c| digest(c);
        json!({"inventory":{"snapshot":d('1'),"sources":{"before":d('4'),"after":d('5'),"root_entity":"person","edits":[]},
                "lifecycle":{"entity":"person","before":{"kind":"today"},"after":{"kind":"today"},"meaning":"ok"}},
            "auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":["write_b","write_a","write_b"],
            "availability":{"nullable_added_boolean":null},"purpose":"basis layer fixture"})
    }
    fn lifecycle(origin: &str) -> SerdeValue {
        let d = |c| digest(c);
        json!({"version":2,"witnesses":[],"evidence":[],"disclosure":d('6'),"event":d('7'),"origin":origin})
    }
    fn inventory() -> SerdeValue {
        let d = |c| digest(c);
        json!({"structure":{"before":d('1'),"after":d('2'),"root_entity":"person","edits":[],"common_statements":[],"writers":[],"changes":[]},
            "certificates":[],"projections":[],"phases":[],"captured":{},"outcome_comparison":[]})
    }
    fn layer(family: &str, origin: &str) -> SerdeValue {
        json!({"version":2,"lowering":2,"operation":"operation","basis":basis(family),"candidate":digest('a'),
            "request":preview_request(),"lifecycle":lifecycle(origin),"inventory":inventory()})
    }
    fn syntax(value: &SerdeValue) -> Value {
        product_json::parse_json_bytes(&serde_json::to_vec(value).unwrap(), &mut AllocationContext::new()).unwrap()
    }
    fn struct_sequence(value: &mut SerdeValue, names: &[&str]) {
        let object = value.as_object().expect("fixture struct object");
        let sequence = names.iter().map(|name| object[*name].clone()).collect();
        *value = SerdeValue::Array(sequence);
    }
    fn basis_sequence() -> SerdeValue {
        let mut value = basis("owned_v2");
        struct_sequence(&mut value["frozen"], &["snapshot", "revision", "active", "data", "session", "day", "decisions"]);
        struct_sequence(&mut value["graph"], &["version", "revision", "decisions"]);
        struct_sequence(&mut value["artifacts"][0]["columns"][0], &["id", "label", "value_type"]);
        struct_sequence(&mut value["artifacts"][0], &["output", "format", "columns", "rows", "bytes", "bytes_digest", "digest"]);
        struct_sequence(&mut value, &["container_format", "family", "frozen", "sources", "graph", "artifacts"]);
        value
    }
    fn decode_basis(value: &SerdeValue, cx: &mut AllocationContext) -> Result<DecodedOwnedBasisV2, ValueDecodeError> { decode_owned_basis_v2(syntax(value), cx) }
    fn decode_layer(value: &SerdeValue, cx: &mut AllocationContext) -> Result<DecodedOwnedLayerV2, ValueDecodeError> { decode_owned_layer_v2(syntax(value), cx) }
    fn canonical(value: &impl Serialize) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn owned_basis_families_and_nested_artifacts_match_canonical_serde() {
        for family in ["scalar_empty", "owned_v2"] {
            let value = basis(family);
            let expected: OwnedBasisV2 = serde_json::from_value(value.clone()).unwrap();
            let decoded = decode_basis(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
            assert_eq!(decoded.sources.len(), 2, "source sequence preserves duplicate entries");
        }
    }

    #[test]
    fn owned_layer_composes_basis_request_lifecycle_and_inventory_exactly() {
        for (family, origin) in [("owned_v2", "explicit_host_ui"), ("scalar_empty", "synthetic_operator_fixture")] {
            let value = layer(family, origin);
            let expected: OwnedLayerV2 = serde_json::from_value(value.clone()).unwrap();
            let decoded = decode_layer(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&decoded));
            assert_eq!(decoded.basis.sources.len(), 2);
        }
    }

    #[test]
    fn struct_sequences_and_enum_map_forms_match_serde() {
        let mut value = basis("owned_v2");
        value["family"] = json!({"owned_v2":null});
        value["graph"] = json!([1,0,[]]);
        let expected: OwnedBasisV2 = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode_basis(&value, &mut AllocationContext::new()).unwrap()));

        let value = basis("owned_v2");
        let fields = ["container_format","family","frozen","sources","graph","artifacts"];
        let mut sequence = value.clone();
        struct_sequence(&mut sequence, &fields);
        let expected: OwnedBasisV2 = serde_json::from_value(sequence.clone()).unwrap();
        let decoded = decode_owned_basis_v2(syntax(&sequence), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decoded));

        let value = layer("owned_v2", "explicit_host_ui");
        let fields = ["version","lowering","operation","basis","candidate","request","lifecycle","inventory"];
        let mut sequence = value.clone();
        struct_sequence(&mut sequence, &fields);
        let expected: OwnedLayerV2 = serde_json::from_value(sequence.clone()).unwrap();
        assert_eq!(canonical(&expected), canonical(&decode_layer(&sequence, &mut AllocationContext::new()).unwrap()));
    }

    #[test]
    fn malformed_unknown_duplicate_numeric_and_collection_limits_are_typed() {
        let mut value = basis("future");
        assert!(matches!(decode_basis(&value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        value["family"] = json!("owned_v2");
        value["container_format"] = json!(u64::from(u32::MAX) + 1);
        assert!(matches!(decode_basis(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        let mut value = layer("owned_v2", "explicit_host_ui");
        value["future"] = json!(true);
        assert!(matches!(decode_layer(&value, &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { .. })));
        value = layer("owned_v2", "explicit_host_ui");
        value["lowering"] = json!(-1);
        assert!(matches!(decode_layer(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        let duplicate = match product_json::parse_json_bytes(br#"{"family":"scalar_empty","family":"owned_v2"}"#, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate member was accepted"),
        };
        assert_eq!(duplicate.cause, product_json::Cause::DuplicateObjectMember);
        let too_many = Value::Array(std::iter::repeat_with(|| Value::String(digest('b'))).take(MAX_ITEMS + 1).collect());
        let mut cx = AllocationContext::new();
        assert!(matches!(digest_vec(too_many, "owned basis source count", &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "owned basis source count", limit: MAX_ITEMS })));
        assert_eq!(cx.growth_requests(), 0);
        let too_many_artifacts = Value::Array(std::iter::repeat_with(|| Value::Null).take(MAX_ITEMS + 1).collect());
        assert!(matches!(decode_vec(too_many_artifacts, "owned basis artifact count", &mut cx, |_, _| Ok(())), Err(ValueDecodeError::LimitExceeded { resource: "owned basis artifact count", limit: MAX_ITEMS })));
        assert_eq!(cx.growth_requests(), 0);
    }

    #[test]
    fn struct_sequence_errors_follow_declared_field_order() {
        let mut value = layer("owned_v2", "explicit_host_ui");
        value["version"] = json!("bad version");
        value["lowering"] = json!(-1);
        struct_sequence(&mut value, &["version","lowering","operation","basis","candidate","request","lifecycle","inventory"]);
        assert!(matches!(decode_layer(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));

        if let SerdeValue::Array(fields) = &mut value {
            fields.push(json!("extra"));
        }
        assert!(matches!(decode_layer(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));
    }

    #[test]
    fn nested_frozen_basis_errors_precede_sequence_length_mismatch() {
        let mut value = basis("owned_v2");
        let frozen = value["frozen"].as_object().unwrap();
        let mut frozen_sequence: Vec<_> = ["snapshot", "revision", "active", "data", "session", "day", "decisions"]
            .iter().map(|name| frozen[*name].clone()).collect();
        frozen_sequence[0] = json!(false);
        frozen_sequence.push(json!(null));
        value["frozen"] = SerdeValue::Array(frozen_sequence);
        struct_sequence(&mut value, &["container_format", "family", "frozen", "sources", "graph", "artifacts"]);
        assert!(matches!(decode_basis(&value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "a string", .. })));
    }

    #[test]
    fn allocation_refusal_at_every_nested_growth_cleans_and_retries() {
        let value = layer("owned_v2", "synthetic_operator_fixture");
        let mut baseline = AllocationContext::new();
        let expected: OwnedLayerV2 = serde_json::from_value(value.clone()).unwrap();
        decode_layer(&value, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode_layer(&value, &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retry = decode_layer(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&retry));
        }
        let mut failed = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        assert!(matches!(decode_basis(&basis("owned_v2"), &mut failed), Err(ValueDecodeError::Allocation(failure)) if failure.kind == AllocationFailureKind::Injected));
    }

    #[test]
    fn allocation_refusal_at_each_struct_sequence_key_cleans_and_retries() {
        let value = basis_sequence();
        let expected: OwnedBasisV2 = serde_json::from_value(value.clone()).unwrap();
        let mut baseline = AllocationContext::new();
        decode_basis(&value, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode_basis(&value, &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retry = decode_basis(&value, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&retry));
        }
    }
}
