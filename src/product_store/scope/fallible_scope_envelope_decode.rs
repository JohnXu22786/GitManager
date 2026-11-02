//! Fallible decoding of the persisted tagged scalar/OwnedV2 scope container.
//!
//! This adapter composes the fallible state decoders and does not change the
//! production family parser or its validation path.
use crate::product_alloc::AllocationContext;
use crate::product_contract::fallible_value_decode::{JsonKind, ValueDecodeError};
use crate::product_json::Value;
use serde::Serialize;

use super::fallible_owned_history_decode::{
    decode_owned_scope_state_v2, DecodedOwnedScopeStateV2,
};
use super::fallible_scalar_decode::{decode_scope_state, DecodedScopeState};

const FAMILY_FIELDS: &[&str] = &["family", "state"];
const FAMILY_VARIANTS: &[&str] = &["scalar_v1", "owned_v2"];

#[derive(Debug, Serialize)]
#[serde(
    tag = "family",
    content = "state",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum DecodedScopeEnvelope {
    ScalarV1(DecodedScopeState),
    OwnedV2(DecodedOwnedScopeStateV2),
}

pub(crate) fn decode_scope_envelope(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedScopeEnvelope, ValueDecodeError> {
    let (family, state) = match value {
        Value::Object(fields) => {
            let mut family = None;
            let mut state = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "family" => family = Some(variant(value)?),
                    "state" => state = Some(value),
                    _ => return Err(ValueDecodeError::InvalidIdentifier { actual: key, expected: FAMILY_FIELDS }),
                }
            }
            (
                family.ok_or(ValueDecodeError::MissingField("family"))?,
                state.ok_or(ValueDecodeError::MissingField("state"))?,
            )
        }
        Value::Array(values) => {
            let actual = values.len();
            let mut values = values.into_iter();
            let family = variant(values.next().ok_or(ValueDecodeError::InvalidLength { actual, expected: 2 })?)?;
            let state = values.next().ok_or(ValueDecodeError::InvalidLength { actual, expected: 2 })?;
            if values.next().is_some() {
                return Err(ValueDecodeError::InvalidLength { actual, expected: 2 });
            }
            (family, state)
        }
        value => return Err(invalid_type(value, "an internally tagged enum")),
    };
    match family {
        Family::ScalarV1 => decode_scope_state(state, cx).map(DecodedScopeEnvelope::ScalarV1),
        Family::OwnedV2 => decode_owned_scope_state_v2(state, cx).map(DecodedScopeEnvelope::OwnedV2),
    }
}

#[derive(Clone, Copy)]
enum Family {
    ScalarV1,
    OwnedV2,
}

fn variant(value: Value) -> Result<Family, ValueDecodeError> {
    let value = match value {
        Value::String(value) => value,
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, payload) = fields.into_entries().next().expect("one unit tag was checked");
            let family = variant_name(tag)?;
            if !matches!(payload, Value::Null) {
                return Err(invalid_type(payload, "unit"));
            }
            return Ok(family);
        }
        value => return Err(invalid_type(value, "a string variant identifier or map")),
    };
    variant_name(value)
}

fn variant_name(value: String) -> Result<Family, ValueDecodeError> {
    match value.as_str() {
        "scalar_v1" => Ok(Family::ScalarV1),
        "owned_v2" => Ok(Family::OwnedV2),
        _ => Err(ValueDecodeError::UnknownVariant {
            tag: value,
            expected: FAMILY_VARIANTS,
        }),
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

#[cfg(test)]
mod tests {
    use super::{decode_scope_envelope, DecodedScopeEnvelope};
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::{canonical_bytes, MAX_ITEMS};
    use crate::product_contract::fallible_value_decode::ValueDecodeError;
    use crate::product_json::{self, Value};
    use crate::product_store::scope::contract::ScopeState;
    use crate::product_store::scope::owned_wire::{OwnedScopeStateV2, OWNED_SCOPE_VERSION};
    use crate::product_store::scope::ScopeEnvelope;
    use serde::Serialize;
    use std::num::NonZeroUsize;

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }
    fn digest(c: char) -> String { std::iter::repeat(c).take(64).collect() }
    fn scalar_state() -> ScopeState { ScopeState::default() }
    fn owned_state() -> serde_json::Value {
        let d = digest('a');
        serde_json::json!({
            "version":OWNED_SCOPE_VERSION,
            "layers":{},
            "compositions":{
                (d):{
                    "version":2,"lowering":2,
                    "basis":{"container_format":2,"family":"owned_v2","frozen":{"snapshot":digest('1'),"revision":1,"active":digest('2'),"data":{"version":1,"project_id":"p","generation":0,"schema":[],"records":[],"events":[]},"session":{"view":"main","values":{},"focused_record":null},"day":0,"decisions":digest('3')},"sources":[],"graph":{"version":1,"revision":0,"decisions":[]},"artifacts":[]},
                    "transition":"adoption","business":digest('4'),"layers":[],"active":[],"previous":null,"operation":"op","runtime":"runtime","driver":"driver","output":digest('5'),"correspondences":[]
                }
            },
            "initializations":[],"adoptions":[],"rehearsals":{},"correspondences":{}
        })
    }
    fn canonical<T: Serialize>(value: &T) -> Vec<u8> { canonical_bytes(value).unwrap() }

    #[test]
    fn both_tagged_variants_preserve_exact_canonical_bytes() {
        let scalar = ScopeEnvelope::ScalarV1(scalar_state());
        let scalar_json = serde_json::to_string(&scalar).unwrap();
        let scalar_decoded = decode_scope_envelope(syntax(&scalar_json), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&scalar), canonical(&scalar_decoded));

        let owned_value = owned_state();
        let owned_model: OwnedScopeStateV2 = serde_json::from_value(owned_value.clone()).unwrap();
        let owned = ScopeEnvelope::OwnedV2(owned_model);
        let owned_json = serde_json::to_string(&owned).unwrap();
        let owned_decoded = decode_scope_envelope(syntax(&owned_json), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&owned), canonical(&owned_decoded));
        assert!(matches!(owned_decoded, DecodedScopeEnvelope::OwnedV2(_)));

        let owned_sequence = serde_json::json!(["owned_v2", owned_state()]);
        let owned_sequence_model: ScopeEnvelope = serde_json::from_value(owned_sequence.clone()).unwrap();
        let owned_sequence_decoded = decode_scope_envelope(syntax(&owned_sequence.to_string()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&owned_sequence_model), canonical(&owned_sequence_decoded));
    }

    #[test]
    fn tag_and_content_order_is_independent_and_sequence_representation_matches_serde() {
        let scalar = ScopeEnvelope::ScalarV1(scalar_state());
        let state = serde_json::to_value(scalar).unwrap()["state"].clone();
        let reordered = format!(r#"{{"state":{},"family":"scalar_v1"}}"#, state);
        let model: ScopeEnvelope = serde_json::from_str(&reordered).unwrap();
        let decoded = decode_scope_envelope(syntax(&reordered), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&model), canonical(&decoded));

        let sequence = serde_json::json!(["scalar_v1", state]);
        let model: ScopeEnvelope = serde_json::from_value(sequence.clone()).unwrap();
        let decoded = decode_scope_envelope(syntax(&sequence.to_string()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&model), canonical(&decoded));

        let mut malformed_overlong = vec![serde_json::json!(7), serde_json::Value::Null, serde_json::Value::Null];
        let malformed_overlong = serde_json::Value::Array(std::mem::take(&mut malformed_overlong));
        assert!(matches!(decode_scope_envelope(syntax(&malformed_overlong.to_string()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "a string variant identifier or map", .. })));
    }

    #[test]
    fn tag_content_and_nested_errors_keep_fixed_classification() {
        for (source, expected) in [
            (r#"{"state":{}}"#, "family"),
            (r#"{"family":"scalar_v1"}"#, "state"),
        ] {
            let error = decode_scope_envelope(syntax(source), &mut AllocationContext::new()).unwrap_err();
            assert!(matches!(error, ValueDecodeError::MissingField(field) if field == expected));
        }
        assert!(matches!(decode_scope_envelope(syntax(r#"{"family":"future","state":{}}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let future_with_extra = serde_json::json!({"family":"future","state":{},"z":0});
        let serde_error = serde_json::from_value::<ScopeEnvelope>(future_with_extra.clone()).unwrap_err().to_string();
        assert!(serde_error.contains("unknown variant"), "Serde error was: {serde_error}");
        assert!(matches!(decode_scope_envelope(syntax(&future_with_extra.to_string()), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownVariant { .. })));
        let earlier_extra = serde_json::json!({"family":"future","state":{},"aaa":0});
        let serde_error = serde_json::from_value::<ScopeEnvelope>(earlier_extra.clone()).unwrap_err().to_string();
        assert!(serde_error.contains("invalid value"), "Serde error was: {serde_error}");
        assert!(matches!(decode_scope_envelope(syntax(&earlier_extra.to_string()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidIdentifier { .. })));
        assert!(matches!(decode_scope_envelope(syntax(r#"{"family":7,"state":{}}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "a string variant identifier or map", .. })));
        let unit_tag = serde_json::json!({"family":{"scalar_v1":null},"state":serde_json::to_value(scalar_state()).unwrap()});
        let model: ScopeEnvelope = serde_json::from_value(unit_tag.clone()).unwrap();
        let decoded = decode_scope_envelope(syntax(&unit_tag.to_string()), &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical(&model), canonical(&decoded));
        let bad_unit_tag = serde_json::json!({"family":{"scalar_v1":false},"state":serde_json::to_value(scalar_state()).unwrap()});
        assert!(serde_json::from_value::<ScopeEnvelope>(bad_unit_tag.clone()).is_err());
        assert!(matches!(decode_scope_envelope(syntax(&bad_unit_tag.to_string()), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "unit", .. })));
        assert!(matches!(decode_scope_envelope(syntax(r#"{"family":"scalar_v1","state":{},"extra":null}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidIdentifier { .. })));
        assert!(matches!(decode_scope_envelope(syntax(r#"{"family":"owned_v2","state":{"version":-1}}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        assert!(matches!(decode_scope_envelope(syntax(r#"{"family":"scalar_v1","state":null}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { .. })));
    }

    #[test]
    fn nested_collection_limit_and_every_reached_refusal_retry_cleanly() {
        let over = (0..=MAX_ITEMS).map(|i| format!("\"{:064x}\":null", i)).collect::<Vec<_>>().join(",");
        let source = format!(r#"{{"family":"owned_v2","state":{{"version":2,"layers":{{}},"compositions":{{{over}}},"initializations":[],"adoptions":[],"rehearsals":{{}},"correspondences":{{}}}}}}"#);
        assert!(matches!(decode_scope_envelope(syntax(&source), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "owned state composition count", limit: MAX_ITEMS })));

        let mut deep = owned_state();
        let mut expression = serde_json::json!({"kind":"today"});
        for _ in 0..=crate::product_contract::MAX_EXPR_DEPTH {
            expression = serde_json::json!({"kind":"count","items":expression});
        }
        deep["compositions"][digest('a')]["basis"]["frozen"]["data"]["schema"] =
            serde_json::json!([{"id":"person","label":"Person","fields":[],"unique":[],"constraints":[expression]}]);
        let deep = format!(r#"{{"family":"owned_v2","state":{}}}"#, deep);
        assert!(matches!(decode_scope_envelope(syntax(&deep), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "expression depth", limit: crate::product_contract::MAX_EXPR_DEPTH })));

        let source = format!(r#"{{"family":"owned_v2","state":{}}}"#, owned_state());
        let mut baseline = AllocationContext::new();
        let expected = decode_scope_envelope(syntax(&source), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            assert!(matches!(decode_scope_envelope(syntax(&source), &mut cx), Err(ValueDecodeError::Allocation(_))), "growth {ordinal}/{growths}");
            let retry = decode_scope_envelope(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical(&expected), canonical(&retry));
        }
    }
}
