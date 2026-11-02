//! Allocation-aware construction of the scalar replay snapshot registry.
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_store::{fallible_project_snapshot_decode::DecodedProjectSnapshot, StoreError};
use crate::product_contract::{Digest, MAX_ITEMS};
use crate::product_store::scope::contract::MAX_LAYERS;

#[derive(Debug)]
pub(crate) enum ReplayRegistryError {
    Store(StoreError),
    Allocation(AllocationFailure),
}

impl From<StoreError> for ReplayRegistryError {
    fn from(error: StoreError) -> Self { Self::Store(error) }
}
impl From<AllocationFailure> for ReplayRegistryError {
    fn from(error: AllocationFailure) -> Self { Self::Allocation(error) }
}

/// Create the private replay registry using bounded owner copies.
pub(crate) fn replay_snapshot_fallible(
    snapshot: &DecodedProjectSnapshot,
    cx: &mut AllocationContext,
) -> Result<DecodedProjectSnapshot, ReplayRegistryError> {
    let mut replay = snapshot.try_clone_with(cx).map_err(|error| match error {
        super::super::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Allocation(failure) => ReplayRegistryError::Allocation(failure),
        super::super::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Invariant => ReplayRegistryError::Store(StoreError::Invalid("replay snapshot copy invariant".into())),
    })?;
    let source_scope = snapshot.scalar_scope().ok_or_else(|| ReplayRegistryError::Store(StoreError::Invalid("replay requires scalar scope".into())))?;
    let target_scope = replay.scalar_scope_mut().ok_or_else(|| ReplayRegistryError::Store(StoreError::Invalid("replay requires scalar scope".into())))?;

    for (id, proof) in source_scope.rehearsals() {
        if let Some(layer) = proof.layer() {
            let identity = layer.identity_with(cx).map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => ReplayRegistryError::Allocation(failure),
                _ => ReplayRegistryError::Store(StoreError::Invalid("rehearsal layer identity invariant".into())),
            })?;
            if target_scope.has_replay_layer(&identity) {
                return Err(StoreError::Invalid("rehearsal layer identity collision".into()).into());
            }
            if target_scope.replay_layer_count() >= MAX_LAYERS {
                return Err(StoreError::Invalid("scope-state layer count limit reached".into()).into());
            }
            let layer = layer.copy_with(cx).map_err(map_decode)?;
            target_scope.insert_replay_layer(identity, layer, cx).map_err(map_decode)?;
        }

        if let Some(receipt) = proof.initialization() {
            if target_scope.initialization_count() >= MAX_LAYERS {
                return Err(StoreError::Invalid("scope-state initialization receipt count limit reached".into()).into());
            }
            let receipt = receipt.copy_with(cx).map_err(map_decode)?;
            target_scope.push_replay_initialization(receipt, cx).map_err(ReplayRegistryError::Allocation)?;
        }

        if !target_scope.has_replay_composition(id) {
            if target_scope.composition_count() >= MAX_ITEMS {
                return Err(StoreError::Invalid("scope-state composition count limit reached".into()).into());
            }
            let key_text = product_alloc::try_copy_str(id.as_str(), cx, AllocationSite::String)?;
            let key = Digest::from_owned_lower_hex(key_text).ok_or_else(|| ReplayRegistryError::Store(StoreError::Invalid("rehearsal composition key invariant".into())))?;
            let manifest = proof.manifest().copy_with(cx).map_err(map_decode)?;
            target_scope.insert_replay_composition(key, manifest, cx).map_err(map_decode)?;
        }
    }
    Ok(replay)
}

fn map_decode(error: crate::product_contract::fallible_value_decode::ValueDecodeError) -> ReplayRegistryError {
    use crate::product_contract::fallible_value_decode::ValueDecodeError;
    match error {
        ValueDecodeError::Allocation(failure) => ReplayRegistryError::Allocation(failure),
        _ => ReplayRegistryError::Store(StoreError::Invalid("replay registry owner invariant".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::canonical_bytes;
    use crate::product_json::{self};
    use crate::product_store::{fallible_project_snapshot_decode::decode_project_snapshot, ProjectSnapshot};
    use crate::product_store::scope::contract::{ScopeLayer, COMPILER_VERSION};
    use serde_json::{json, Map, Value};
    use std::num::NonZeroUsize;

    const D0: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn root(rehearsals: Map<String, Value>) -> Value {
        json!({
            "magic":"gitmanager.generated-project", "version":3, "revision":9,
            "programs":[], "active_revision":D0,
            "data":{"version":1,"project_id":"project","generation":0,"schema":[],"records":[],"events":[]},
            "session":{"view":"main","values":{},"focused_record":null},
            "clock_day":0, "decisions":{"version":1,"revision":0,"decisions":[]},
            "adoptions":[],"operations":{},"artifacts":[],
            "scope":{"family":"scalar_v1","state":{
                "version":1,"layers":{},"compositions":{},"initializations":[],"adoptions":[],
                "rehearsals":rehearsals,"correspondences":{}
            }}
        })
    }

    fn proof(output: &str, suffix: &str) -> (Value, String, Value) {
        let basis = json!({
            "snapshot":D0,"revision":1,"active":D0,
            "data":{"version":1,"project_id":"project","generation":0,"schema":[],"records":[],"events":[]},
            "session":{"view":"main","values":{},"focused_record":null},"day":0,"decisions":D0
        });
        let layer = json!({
            "version":COMPILER_VERSION,"operation":format!("layer-{suffix}"),"basis":basis,
            "candidate":D0,
            "request":{"population":{"kind":"all"},"operations":["read"],"excluded_records":[],"lifecycles":[],"patches":[]},
            "patches":[]
        });
        let layer_model: ScopeLayer = serde_json::from_value(layer.clone()).unwrap();
        let layer_id = layer_model.identity().unwrap().as_str().to_owned();
        let basis = serde_json::to_value(&layer_model.basis).unwrap();
        let manifest = json!({
            "version":COMPILER_VERSION,"basis":basis,"transition":"adoption","rewrites":[],
            "business":D0,"layers":[layer_id],"active":[layer_id],"previous":null,
            "operation":format!("composition-{suffix}"),"runtime":"runtime","driver":"driver","output":output
        });
        let receipt = json!({
            "operation":format!("initialize-{suffix}"),"layer":layer_id,
            "before_data":D0,"after_data":D0,"before_schema":D0,"after_schema":D0,
            "additions":[],"business_projection":D0,"original_events":D0,"original_records":0,
            "capture_day":0,"capture_program":D0,"prior_snapshot":D0,"adoption_revision":2
        });
        let proof = json!({
            "version":COMPILER_VERSION,"manifest":manifest,"seed":D0,"layer":layer,"initialization":receipt,
            "compatibility":{"state":"compatible","current_data":D0,"target_program":D0,"retained_records":[],"retained_events":[],"retained_fields":[],"issues":[]},
            "recorded_by":"recording","recorded_revision":2,"witnesses":{}
        });
        (proof, layer_id, layer)
    }

    fn decoded(value: &Value) -> DecodedProjectSnapshot {
        let syntax = product_json::parse_json_bytes(&serde_json::to_vec(value).unwrap(), &mut AllocationContext::new()).unwrap();
        decode_project_snapshot(syntax, &mut AllocationContext::new()).unwrap()
    }

    fn replay_oracle(value: &Value) -> (ProjectSnapshot, DecodedProjectSnapshot) {
        let legacy: ProjectSnapshot = serde_json::from_value(value.clone()).unwrap();
        let replay = super::super::rehearsal::replay_snapshot(&legacy).unwrap();
        (replay, decoded(value))
    }

    fn bytes(snapshot: &DecodedProjectSnapshot) -> Vec<u8> {
        snapshot.canonical_bytes_with(&mut AllocationContext::new()).unwrap()
    }

    #[test]
    fn empty_single_and_paired_replay_registries_match_legacy_bytes() {
        let empty = root(Map::new());
        let (expected, actual) = replay_oracle(&empty);
        let replay = replay_snapshot_fallible(&actual, &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical_bytes(&expected).unwrap(), bytes(&replay));

        for count in [1, 2] {
            let mut rehearsals = Map::new();
            for index in 0..count {
                let output = format!("{:064x}", index + 1);
                let (proof, _, _) = proof(&output, &format!("{index}"));
                rehearsals.insert(output, proof);
            }
            let value = root(rehearsals);
            let (expected, actual) = replay_oracle(&value);
            let replay = replay_snapshot_fallible(&actual, &mut AllocationContext::new()).unwrap();
            assert_eq!(canonical_bytes(&expected).unwrap(), bytes(&replay), "{count} rehearsal(s)");
        }
    }

    #[test]
    fn replay_registry_refusal_is_typed_retryable_and_input_is_unchanged() {
        let mut rehearsals = Map::new();
        let (proof, _, _) = proof(D0, "single");
        rehearsals.insert(D0.to_owned(), proof);
        let value = root(rehearsals);
        let actual = decoded(&value);
        let original = bytes(&actual);
        let mut baseline = AllocationContext::new();
        let expected = replay_snapshot_fallible(&actual, &mut baseline).unwrap();
        let expected = bytes(&expected);
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            let failure = replay_snapshot_fallible(&actual, &mut cx).unwrap_err();
            let ReplayRegistryError::Allocation(failure) = failure else { panic!("expected allocation refusal at {ordinal}/{growths}: {failure:?}") };
            assert_eq!(cx.first_failure(), Some(failure));
            assert_eq!(bytes(&actual), original);
            let retry = replay_snapshot_fallible(&actual, &mut AllocationContext::new()).unwrap();
            assert_eq!(bytes(&retry), expected);
        }
    }

    #[test]
    fn replay_registry_rejects_layer_collision_and_limit_without_mutating_input() {
        let mut rehearsals = Map::new();
        let (proof_value, layer_id, layer) = proof(D0, "collision");
        rehearsals.insert(D0.to_owned(), proof_value);
        let mut value = root(rehearsals);
        value["scope"]["state"]["layers"] = json!({layer_id:layer});
        let actual = decoded(&value);
        let before = bytes(&actual);
        assert!(matches!(replay_snapshot_fallible(&actual, &mut AllocationContext::new()), Err(ReplayRegistryError::Store(_))));
        assert_eq!(bytes(&actual), before);

        let mut rehearsals = Map::new();
        let (proof_value, _, layer) = proof(D0, "limit");
        rehearsals.insert(D0.to_owned(), proof_value);
        let mut layers = Map::new();
        for index in 0..crate::product_store::scope::contract::MAX_LAYERS {
            layers.insert(format!("{:064x}", index + 1), layer.clone());
        }
        let mut value = root(rehearsals);
        value["scope"]["state"]["layers"] = Value::Object(layers);
        let actual = decoded(&value);
        let before = bytes(&actual);
        assert!(matches!(replay_snapshot_fallible(&actual, &mut AllocationContext::new()), Err(ReplayRegistryError::Store(_))));
        assert_eq!(bytes(&actual), before);
    }

    #[test]
    fn replay_registry_preserves_an_existing_composition_entry() {
        let mut rehearsals = Map::new();
        let (mut proof_value, _, _) = proof(D0, "existing-composition");
        let existing_manifest = proof_value["manifest"].clone();
        rehearsals.insert(D0.to_owned(), proof_value);
        let mut value = root(rehearsals);
        value["scope"]["state"]["compositions"] = json!({D0: existing_manifest});
        let (expected, actual) = replay_oracle(&value);
        let replay = replay_snapshot_fallible(&actual, &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical_bytes(&expected).unwrap(), bytes(&replay));
    }
}
