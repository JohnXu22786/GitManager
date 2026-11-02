//! Versioned, data-only language and evidence contracts for generated local tools.
//!
//! This module validates source; it neither executes a program nor certifies a
//! model's claims. Use the production `RuntimeAdapter` to obtain observations.
//! There is deliberately no eval, shell, URL, file-path or credential operation.

// One lexical V1 semantic prefix, with no callable semantic-only authority.
// The ordinary/count tail remains in its original scope with the Validator.
macro_rules! validate_app_semantics {
    ($this:ident, $a:ident, $validator:ident, $action_prefix_nodes:ident, $cx:expr) => {
        $a.pay(12)?;
        #[cfg(test)]
        count_validation(|counts| counts.apps += 1);
        $a.version($this.version)?;
        $a.check_id(&$this.id)?;
        $a.nonempty(&$this.label)?;
        $a.require(
            !$this.entities.is_empty() && !$this.views.is_empty(),
            "application needs entities and views",
        )?;
        $a.unique($this.entities.iter().map(|e| e.id.as_str()))?;
        $a.unique($this.state.iter().map(|s| s.id.as_str()))?;
        $a.unique($this.actions.iter().map(|$a| $a.id.as_str()))?;
        $a.unique($this.outputs.iter().map(|o| o.id.as_str()))?;
        $a.unique($this.observables.iter().map(|o| o.id.as_str()))?;
        $a.unique($this.views.iter().map(|v| v.id.as_str()))?;
        $a.require(
            $a.find(&$this.views, &$this.initial_view, |v| &v.id)?.is_some(),
            "unknown initial view",
        )?;
        $a.pay(8)?;
        let mut $validator = Validator::new_app(AppRef::from($this), $a, $cx);
        validate_entities(&$this.entities, &mut $validator)?;
        $a.slice($this.state.len())?;
        for state in &$this.state {
            $a.pay(12)?;
            $a.nonempty(&state.label)?;
            $validator.typ(&state.value_type, 0)?;
            validate_value_audited(&state.initial, &state.value_type, 0, $a)?;
        }
        $a.slice($this.outputs.len())?;
        for output in &$this.outputs {
            $a.pay(12)?;
            $a.nonempty(&output.label)?;
            $a.require(!output.columns.is_empty(), "output needs columns")?;
            $a.unique(output.columns.iter().map(|c| c.id.as_str()))?;
            $a.slice(output.columns.len())?;
        for column in &output.columns {
            $a.pay(12)?;
                $a.nonempty(&column.label)?;
                $validator.typ(&column.value_type, 0)?;
                $a.require(
                    column.value_type.is_scalar(),
                    "output columns must be scalar",
                )?;
            }
        }
        $a.slice($this.actions.len())?;
        for action in &$this.actions {
            $validator.action_definition(action)?;
        }
        $a.pay(1)?;
        let $action_prefix_nodes = $validator.nodes;
        $a.slice($this.observables.len())?;
        for observable in &$this.observables {
            $a.pay(12)?;
            $a.nonempty(&observable.label)?;
            $a.pay(8)?;
            let env = $validator.empty_environment();
            $validator.expr(&observable.value, &env, 0, true)?;
        }
        $a.slice($this.views.len())?;
        for view in &$this.views {
            $a.pay(12)?;
            $validator.view(view)?;
        }
    };
}

#[path = "tool_proposal_input.rs"]
mod bounded_input;
#[path = "product_contract/capture_work.rs"]
mod capture_work;
#[path = "product_contract/capture_owned.rs"]
mod capture_owned;
#[path = "product_contract/capture_payment.rs"]
pub(crate) mod capture_payment;
#[path = "product_contract/capture_canonical.rs"]
mod capture_canonical;
#[path = "product_contract/capture_typed.rs"]
mod capture_typed;
#[path = "product_contract/capture_validation.rs"]
mod capture_validation;
use capture_work::Audit;
use capture_owned::OwnedWork;
pub(crate) use capture_work::{ContractWork, WorkFailure, ContractWorkError, ContractWorkResult};
pub(crate) use capture_canonical::canonical_bytes_with_work;
pub(crate) use capture_validation::{with_capture_validation, CaptureValidation, validate_value_with_work,
    with_ordinary_source_facts, ReadyCaptureSource, SourcePreparation, SourceConsumer};

use ring::digest::{digest, SHA256};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

#[path = "product_contract/values_map.rs"]
#[allow(dead_code)]
mod values_map;
pub(crate) fn try_clone_type_with(
    value: &Type,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Type, crate::product_alloc::AllocationFailure> {
    values_map::try_clone_type(value, cx)
}
pub(crate) fn try_clone_data_value_with(
    value: &DataValue,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<DataValue, crate::product_alloc::AllocationFailure> {
    values_map::try_clone_data_value(value, cx)
}
#[path = "product_contract/fallible_value_decode.rs"]
#[allow(dead_code)]
pub(crate) mod fallible_value_decode;
#[path = "product_contract/fallible_expr_decode.rs"]
#[allow(dead_code)]
pub(crate) mod fallible_expr_decode;
#[path = "product_contract/fallible_entity_decode.rs"]
#[allow(dead_code)]
pub(crate) mod fallible_entity_decode;
#[path = "product_contract/fallible_persisted_decode.rs"]
#[allow(dead_code)]
mod fallible_persisted_decode;
#[path = "product_contract/fallible_snapshot_decode.rs"]
#[allow(dead_code)]
mod fallible_snapshot_decode;
#[path = "product_contract/fallible_snapshot_validate.rs"]
mod fallible_snapshot_validate;
pub(crate) use fallible_snapshot_validate::DataValueTuple as FallibleDataValueTuple;
/// Read-only lexical map view shared by legacy BTreeMap owners and fallible
/// sorted owners. Validation uses it without materializing either model.
pub(crate) trait StringValueMap<V> {
    type Iter<'a>: Iterator<Item = (&'a String, &'a V)>
    where
        Self: 'a,
        V: 'a;
    fn len(&self) -> usize;
    fn get(&self, key: &str) -> Option<&V>;
    fn contains_key(&self, key: &str) -> bool;
    fn iter(&self) -> Self::Iter<'_>;
}
impl<V> StringValueMap<V> for BTreeMap<String, V> {
    type Iter<'a> = std::collections::btree_map::Iter<'a, String, V> where V: 'a;
    fn len(&self) -> usize { BTreeMap::len(self) }
    fn get(&self, key: &str) -> Option<&V> { BTreeMap::get(self, key) }
    fn contains_key(&self, key: &str) -> bool { BTreeMap::contains_key(self, key) }
    fn iter(&self) -> Self::Iter<'_> { BTreeMap::iter(self) }
}
impl<V> StringValueMap<V> for fallible_app_decode::FallibleMap<V> {
    type Iter<'a> = fallible_app_decode::FallibleMapIter<'a, V> where V: 'a;
    fn len(&self) -> usize { fallible_app_decode::FallibleMap::len(self) }
    fn get(&self, key: &str) -> Option<&V> { fallible_app_decode::FallibleMap::get(self, key) }
    fn contains_key(&self, key: &str) -> bool { fallible_app_decode::FallibleMap::contains_key(self, key) }
    fn iter(&self) -> Self::Iter<'_> { fallible_app_decode::FallibleMap::iter(self) }
}
impl StringValueMap<DataValue> for FallibleValues {
    type Iter<'a> = values_map::FallibleValuesIter<'a>;
    fn len(&self) -> usize { self.len() }
    fn get(&self, key: &str) -> Option<&DataValue> { self.get(key) }
    fn contains_key(&self, key: &str) -> bool { self.contains_key(key) }
    fn iter(&self) -> Self::Iter<'_> { self.iter() }
}
/// Opaque bridge into fallible persisted data/session DTOs for sibling decoders.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FallibleDecodedDataSnapshot(fallible_snapshot_decode::DecodedDataSnapshot);
#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct FallibleEntitySchemaIdentity<'a>(&'a [EntityDefinition]);
#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct FallibleBusinessRecordsIdentity<'a>(&'a [fallible_persisted_decode::DecodedRecord]);
#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct FallibleBusinessEventsIdentity<'a>(&'a [fallible_persisted_decode::DecodedBusinessEvent]);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FallibleDataSnapshotCloneError {
    Allocation(crate::product_alloc::AllocationFailure),
    Invariant,
}
impl FallibleDecodedDataSnapshot {
    pub(crate) fn empty_with(
        project: &str,
        app: &fallible_app_decode::DecodedAppDefinition,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        app.validate_with(cx)?;
        check_id(project).map_err(AppValidationError::Contract)?;
        let project_id = crate::product_alloc::try_copy_str(project, cx, crate::product_alloc::AllocationSite::String)
            .map_err(AppValidationError::Allocation)?;
        let app = app.try_clone_with(cx)?;
        let value = fallible_snapshot_decode::DecodedDataSnapshot {
            version: CONTRACT_VERSION,
            project_id,
            generation: 0,
            schema: app.entities,
            records: Vec::new(),
            events: Vec::new(),
        };
        let snapshot = Self(value);
        snapshot.validate_with(cx)?;
        Ok(snapshot)
    }
    pub(crate) fn validate(&self) -> std::result::Result<(), ContractError> {
        self.0.validate()
    }
    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_with(cx).map_err(|error| match error {
            fallible_snapshot_validate::SnapshotValidationError::Contract(error) => AppValidationError::Contract(error),
            fallible_snapshot_validate::SnapshotValidationError::Allocation(failure) => AppValidationError::Allocation(failure),
        })
    }
    pub(crate) fn canonical_bytes_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Vec<u8>, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_bytes(self, cx)
    }
    pub(crate) fn canonical_digest_with(
        &self,
        domain: IdentityDomain,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(domain, self, cx)
    }
    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, AppValidationError> {
        self.validate_with(cx)?;
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Data, self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("data identity encoding failed".into())),
            })
    }
    pub(crate) fn schema_identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, AppValidationError> {
        self.validate_with(cx)?;
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Schema, &FallibleEntitySchemaIdentity(&self.0.schema), cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("schema identity encoding failed".into())),
            })
    }
    pub(crate) fn business_projection_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Data, &FallibleBusinessRecordsIdentity(&self.0.records), cx)
    }
    pub(crate) fn events_digest_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Data, &FallibleBusinessEventsIdentity(&self.0.events), cx)
    }
    pub(crate) fn try_clone_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, FallibleDataSnapshotCloneError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => {
                    FallibleDataSnapshotCloneError::Allocation(failure)
                }
                _ => FallibleDataSnapshotCloneError::Invariant,
            })?;
        fallible_snapshot_decode::decode_data_snapshot(value, cx)
            .map(Self)
            .map_err(|error| match error {
                fallible_value_decode::ValueDecodeError::Allocation(failure) => {
                    FallibleDataSnapshotCloneError::Allocation(failure)
                }
                _ => FallibleDataSnapshotCloneError::Invariant,
            })
    }
    pub(crate) fn merge_app_with(
        &self,
        app: &fallible_app_decode::DecodedAppDefinition,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        use crate::product_alloc::{try_push, try_reserve, AllocationSite};
        app.validate_with(cx)?;
        let mut next = self.try_clone_with(cx).map_err(|error| match error {
            FallibleDataSnapshotCloneError::Allocation(failure) => AppValidationError::Allocation(failure),
            FallibleDataSnapshotCloneError::Invariant => AppValidationError::Contract(ContractError("data snapshot copy invariant".into())),
        })?;
        let app = app.try_clone_with(cx)?;
        let missing_entities = app.entities.iter().filter(|entity| !next.0.schema.iter().any(|stored| stored.id == entity.id)).count();
        try_reserve(&mut next.0.schema, missing_entities, cx, AllocationSite::Vector).map_err(AppValidationError::Allocation)?;
        for mut entity in app.entities {
            if let Some(stored) = next.0.schema.iter_mut().find(|stored| stored.id == entity.id) {
                for field in &entity.fields {
                    if let Some(old) = stored.fields.iter().find(|old| old.id == field.id) {
                        if old.value_type != field.value_type {
                            return Err(AppValidationError::Contract(ContractError(format!("Field {}.{} changes its recorded type; a compatible forward repair is required", entity.id, field.id))));
                        }
                    }
                }
                let added = entity.fields.iter().filter(|field| !stored.fields.iter().any(|old| old.id == field.id)).count();
                try_reserve(&mut stored.fields, added, cx, AllocationSite::Vector).map_err(AppValidationError::Allocation)?;
                for field in entity.fields.drain(..) {
                    if !stored.fields.iter().any(|old| old.id == field.id) {
                        try_push(&mut stored.fields, field, cx, AllocationSite::Vector).map_err(AppValidationError::Allocation)?;
                    }
                }
                stored.label = entity.label;
                stored.unique = entity.unique;
                stored.constraints = entity.constraints;
            } else {
                try_push(&mut next.0.schema, entity, cx, AllocationSite::Vector).map_err(AppValidationError::Allocation)?;
            }
        }
        next.0.validate_with(cx).map_err(|error| match error {
            fallible_snapshot_validate::SnapshotValidationError::Contract(error) => AppValidationError::Contract(error),
            fallible_snapshot_validate::SnapshotValidationError::Allocation(failure) => AppValidationError::Allocation(failure),
        })?;
        Ok(next)
    }
    pub(crate) fn project_id(&self) -> &str { &self.0.project_id }
    pub(crate) fn generation(&self) -> u64 { self.0.generation }
    pub(crate) fn events_start_with(&self, prefix: &Self) -> bool {
        self.0.events.starts_with(&prefix.0.events)
    }
    pub(crate) fn record_count(&self) -> usize { self.0.records.len() }
    pub(crate) fn records(&self) -> &[fallible_persisted_decode::DecodedRecord] { &self.0.records }
    pub(crate) fn events(&self) -> &[fallible_persisted_decode::DecodedBusinessEvent] { &self.0.events }
    pub(crate) fn schema(&self) -> &[EntityDefinition] { &self.0.schema }
    pub(crate) fn record_producers(&self) -> impl Iterator<Item = &Digest> {
        self.0.records.iter().map(|record| &record.created_program)
    }
    pub(crate) fn event_producers(&self) -> impl Iterator<Item = &Digest> {
        self.0.events.iter().map(|event| &event.program)
    }
    pub(crate) fn output_digests(&self) -> impl Iterator<Item = &Digest> {
        self.0.events.iter().flat_map(|event| event.outputs.iter())
    }
    pub(crate) fn record_values_mut(&mut self, entity: &str, id: &str) -> Option<&mut FallibleValues> {
        self.0.records.iter_mut().find(|record| record.entity == entity && record.id == id).map(|record| &mut record.values)
    }
    pub(crate) fn business_projection_matches_after_removing<'a, I>(
        &self,
        before: &Self,
        additions: I,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<bool, FallibleDataSnapshotCloneError>
    where
        I: IntoIterator<Item = (&'a RecordRef, &'a FallibleValues)>,
    {
        let mut business = self.try_clone_with(cx)?;
        for (reference, values) in additions {
            let Some(record) = business.0.records.iter_mut().find(|record| {
                record.entity == reference.entity && record.id == reference.record
            }) else { return Ok(false) };
            for (key, _) in values.iter() { record.values.remove(key); }
        }
        Ok(business.0.records == before.0.records
            && self.0.events == before.0.events
            && self.0.generation == before.0.generation)
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FallibleDecodedSessionState(fallible_persisted_decode::DecodedSessionState);
impl FallibleDecodedSessionState {
    pub(crate) fn view(&self) -> &str { self.0.view() }
    pub(crate) fn values(&self) -> &FallibleValues { self.0.values() }
    pub(crate) fn focused_record(&self) -> Option<&RecordRef> { self.0.focused_record() }
    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Session, self, cx)
    }
    pub(crate) fn try_clone_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("session copy invariant".into())),
            })?;
        fallible_persisted_decode::decode_session_state(value, cx).map(Self).map_err(|error| match error {
            fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
            _ => AppValidationError::Contract(ContractError("session copy invariant".into())),
        })
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedCapturedProgram(fallible_capture_decode::DecodedCapturedProgram);
impl FallibleDecodedCapturedProgram {
    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_with(cx)
    }
    pub(crate) fn validate_app_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_app_with(cx)
    }
    pub(crate) fn validate_session_with(
        &self,
        session: &FallibleDecodedSessionState,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_session_with(&session.0, cx)
    }
    pub(crate) fn project_id(&self) -> &str { self.0.project_id() }
    pub(crate) fn binding(&self) -> &SourceBinding { self.0.binding() }
    pub(crate) fn artifact(&self) -> &ArtifactRef { self.0.artifact() }
    pub(crate) fn program_digest(&self) -> &Digest { self.0.program_digest() }
    pub(crate) fn app(&self) -> &fallible_app_decode::DecodedAppDefinition { self.0.app() }
    pub(crate) fn has_protected_fields(&self) -> bool {
        self.0.app().entities.iter().any(|entity| {
            entity.fields.iter().any(|field| field.id.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX))
        })
    }
    pub(crate) fn revision_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, AppValidationError> {
        self.0.revision_with(cx)
    }
    pub(crate) fn initial_session_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<FallibleDecodedSessionState, crate::product_alloc::AllocationFailure> {
        self.0.app().initial_session_with(cx).map(FallibleDecodedSessionState)
    }
    pub(crate) fn try_clone_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("captured program copy invariant".into())),
            })?;
        fallible_capture_decode::decode_captured_program(value, cx)
            .map(Self)
            .map_err(|error| match error {
                fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("captured program copy invariant".into())),
            })
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedScenarioSpec(fallible_scenario_decode::DecodedScenarioSpec);
impl FallibleDecodedScenarioSpec {
    pub(crate) fn id(&self) -> &str { self.0.id() }
    pub(crate) fn clock_day(&self) -> i32 { self.0.clock_day() }
    pub(crate) fn random_seed(&self) -> u64 { self.0.random_seed() }
    pub(crate) fn input_count(&self) -> usize { self.0.input_count() }
    pub(crate) fn has_validity(&self) -> bool { self.0.has_validity() }
    pub(crate) fn single_observe_point(&self) -> Option<&str> { self.0.single_observe_point() }
    pub(crate) fn matches_frame(&self, scenario: &Self) -> bool { self.0.matches_frame(&scenario.0) }

    pub(crate) fn validate_replay_input(
        &self,
        program: &FallibleDecodedCapturedProgram,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_replay_input(program.app())
    }

    pub(crate) fn validate_seed_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_seed_with(cx)
    }

    pub(crate) fn validate_session_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_session_with(program.app())
    }

    pub(crate) fn input_identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        self.0.input_identity_with(cx)
    }

    pub(crate) fn seed_identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        self.0.seed_identity_with(cx)
    }

    pub(crate) fn session_identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        self.0.session_identity_with(cx)
    }

    pub(crate) fn session_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<FallibleDecodedSessionState, AppValidationError> {
        self.0.session_with(cx).map(FallibleDecodedSessionState)
    }

    pub(crate) fn observe_operation_id_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<String, AppValidationError> {
        self.0.observe_operation_id_with(cx)
    }

    pub(crate) fn validate_with(
        &self,
        program: &FallibleDecodedCapturedProgram,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_with(program.app(), cx)
    }

    pub(crate) fn seed_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<FallibleDecodedDataSnapshot, AppValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self.0.seed(), cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario seed copy invariant".into())),
            })?;
        fallible_snapshot_decode::decode_data_snapshot(value, cx)
            .map(FallibleDecodedDataSnapshot)
            .map_err(|error| match error {
                fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario seed copy invariant".into())),
            })
    }

    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Scenario, self, cx)
    }

    pub(crate) fn try_clone_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario copy invariant".into())),
            })?;
        fallible_scenario_decode::decode_scenario_spec(value, cx).map(Self)
            .map_err(|error| match error {
                fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario copy invariant".into())),
            })
    }

    pub(crate) fn with_seed_and_id(
        &self,
        seed: &FallibleDecodedDataSnapshot,
        id: Option<&str>,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, AppValidationError> {
        let mut value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario projection copy invariant".into())),
            })?;
        let mut seed_value = crate::product_backup::fallible_codec::try_serialize_value(seed, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario projection seed invariant".into())),
            })?;
        let crate::product_json::Value::Object(fields) = &mut value else {
            return Err(AppValidationError::Contract(ContractError("scenario projection object invariant".into())));
        };
        let Some(seed_field) = fields.get_mut("seed") else {
            return Err(AppValidationError::Contract(ContractError("scenario projection seed field invariant".into())));
        };
        std::mem::swap(seed_field, &mut seed_value);
        if let Some(id) = id {
            let copied = crate::product_alloc::try_copy_str(id, cx, crate::product_alloc::AllocationSite::String)
                .map_err(AppValidationError::Allocation)?;
            let Some(id_field) = fields.get_mut("id") else {
                return Err(AppValidationError::Contract(ContractError("scenario projection id field invariant".into())));
            };
            *id_field = crate::product_json::Value::String(copied);
        }
        fallible_scenario_decode::decode_scenario_spec(value, cx).map(Self)
            .map_err(|error| match error {
                fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
                _ => AppValidationError::Contract(ContractError("scenario projection decode invariant".into())),
            })
    }

    pub(crate) fn scope_replay_namespace_with(
        original: &Self,
        mapped: &Self,
        source: &FallibleDecodedCapturedProgram,
        target: &FallibleDecodedCapturedProgram,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        fallible_scenario_decode::scope_replay_namespace(
            &original.0, &mapped.0, source.binding(), target.binding(), cx,
        )
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedRunEvidence(fallible_run_evidence_decode::DecodedRunEvidence);
impl crate::product_backup::fallible_codec::BackupWire for FallibleDecodedRunEvidence {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeEvidenceError {
    Contract(ContractError),
    Allocation(crate::product_alloc::AllocationFailure),
}
impl FallibleDecodedRunEvidence {
    pub(crate) fn state(&self) -> EvidenceState { self.0.state() }
    pub(crate) fn trace_count(&self) -> usize { self.0.trace_count() }
    pub(crate) fn observation_count(&self) -> usize { self.0.observation_count() }
    pub(crate) fn limits(&self) -> &RuntimeLimits { self.0.limits() }
    pub(crate) fn validate_single_observe(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), RuntimeEvidenceError> {
        self.0.validate_single_observe(cx).map_err(|error| match error {
            AppValidationError::Contract(error) => RuntimeEvidenceError::Contract(error),
            AppValidationError::Allocation(failure) => RuntimeEvidenceError::Allocation(failure),
        })
    }

    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, RuntimeEvidenceError> {
        self.validate_single_observe(cx)?;
        crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence, self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => RuntimeEvidenceError::Allocation(failure),
                _ => RuntimeEvidenceError::Contract(ContractError("runtime evidence identity encoding failed".into())),
            })
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedDecisionGraph(fallible_decision_decode::DecodedDecisionGraph);
impl FallibleDecodedDecisionGraph {
    pub(crate) fn revision(&self) -> u64 { self.0.revision }

    pub(crate) fn is_empty_checkpoint(&self) -> bool {
        self.0.revision == 0 && self.0.decisions.is_empty()
    }

    pub(crate) fn active_ids_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Vec<String>, DecisionValidationError> {
        let mut ids = Vec::new();
        crate::product_alloc::try_reserve(&mut ids, self.0.decisions.len(), cx, crate::product_alloc::AllocationSite::Vector)
            .map_err(DecisionValidationError::Allocation)?;
        for decision in &self.0.decisions {
            if decision.status == DecisionStatus::Active {
                let id = crate::product_alloc::try_copy_str(&decision.id, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(DecisionValidationError::Allocation)?;
                crate::product_alloc::try_push(&mut ids, id, cx, crate::product_alloc::AllocationSite::Vector)
                    .map_err(DecisionValidationError::Allocation)?;
            }
        }
        Ok(ids)
    }

    pub(crate) fn accepted_for_scope(
        &self,
        id: &str,
        plan: &FallibleDecodedAdoptionPlan,
        artifact: &Digest,
    ) -> bool {
        self.0.accepted_for_scope(id, plan.0.scope(), artifact)
    }

    pub(crate) fn contains_decision(&self, id: &str) -> bool {
        self.0.decisions.iter().any(|decision| decision.id == id)
    }

    pub(crate) fn matches_scope(&self, id: &str, plan: &FallibleDecodedAdoptionPlan) -> bool {
        self.0.decisions.iter().any(|decision| decision.id == id && decision.scope.same_as(plan.0.scope()))
    }

    pub(crate) fn pending_nonbinary_witness(&self, id: &str, witness: &Digest) -> bool {
        self.0.decisions.iter().any(|decision| {
            decision.id == id && &decision.witness == witness
                && matches!(decision.status, DecisionStatus::Pending | DecisionStatus::Superseded { .. })
                && matches!(decision.outcome,
                    DecisionOutcome::EitherAcceptable | DecisionOutcome::BothNeeded
                        | DecisionOutcome::NeitherFits | DecisionOutcome::Deferred)
        })
    }

    pub(crate) fn ids_matching_with(
        &self,
        ids: &[String],
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Vec<String>, DecisionValidationError> {
        let mut wanted = crate::product_alloc::ordered::OrderedSet::new();
        for id in ids {
            wanted.try_insert(id.as_str(), cx).map_err(DecisionValidationError::Allocation)?;
        }
        let mut result = Vec::new();
        crate::product_alloc::try_reserve(&mut result, ids.len(), cx, crate::product_alloc::AllocationSite::Vector)
            .map_err(DecisionValidationError::Allocation)?;
        for decision in &self.0.decisions {
            if wanted.contains(decision.id.as_str()) {
                let id = crate::product_alloc::try_copy_str(&decision.id, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(DecisionValidationError::Allocation)?;
                crate::product_alloc::try_push(&mut result, id, cx, crate::product_alloc::AllocationSite::Vector)
                    .map_err(DecisionValidationError::Allocation)?;
            }
        }
        Ok(result)
    }

    pub(crate) fn try_clone_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Self, DecisionValidationError> {
        let value = crate::product_backup::fallible_codec::try_serialize_value(self, cx)
            .map_err(|error| match error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => DecisionValidationError::Allocation(failure),
                _ => DecisionValidationError::Contract(ContractError("decision canonical encoding failed".into())),
            })?;
        let decoded = decode_decision_graph_fallible(value, cx).map_err(|error| match error {
            fallible_value_decode::ValueDecodeError::Allocation(failure) => DecisionValidationError::Allocation(failure),
            _ => DecisionValidationError::Contract(ContractError("decision copy invariant".into())),
        })?;
        Ok(decoded)
    }

    pub(crate) fn rewind_adoption_with(
        &self,
        expected: &Digest,
        retired: &[String],
        adoption: &str,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(Self, Vec<String>, Vec<String>), DecisionValidationError> {
        let (graph, active, pending) = self.0.rewind_adoption_with(expected, retired, adoption, cx)?;
        Ok((Self(graph), active, pending))
    }

    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError> {
        validate_decision_graph_with(self.0.version, &self.0.decisions, cx)
    }

    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, DecisionValidationError> {
        self.validate_with(cx)?;
        crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Decision,
            self,
            cx,
        )
        .map_err(|error| match error {
            crate::product_backup::fallible_codec::CodecError::Allocation(failure) => {
                DecisionValidationError::Allocation(failure)
            }
            _ => DecisionValidationError::Contract(ContractError("decision canonical encoding failed".into())),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DecisionValidationError {
    Contract(ContractError),
    Allocation(crate::product_alloc::AllocationFailure),
}
impl From<ContractError> for DecisionValidationError {
    fn from(value: ContractError) -> Self { Self::Contract(value) }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedLocalArtifact(fallible_local_artifact_decode::DecodedLocalArtifact);
impl FallibleDecodedLocalArtifact {
    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        self.0.validate_with(cx)
    }
    pub(crate) fn digest(&self) -> &Digest { self.0.digest() }
    pub(crate) fn byte_len(&self) -> usize { self.0.byte_len() }
}
#[derive(Debug, Serialize)]
pub(crate) struct FallibleDecodedAdoptionPlan(fallible_decision_decode::DecodedAdoptionPlan);
impl FallibleDecodedAdoptionPlan {
    pub(crate) fn id(&self) -> &str { self.0.id() }
    pub(crate) fn project_id(&self) -> &str { self.0.project_id() }
    pub(crate) fn expected_generation(&self) -> u64 { self.0.expected_generation() }
    pub(crate) fn expected_data(&self) -> &Digest { self.0.expected_data() }
    pub(crate) fn expected_decisions(&self) -> &Digest { self.0.expected_decisions() }
    pub(crate) fn expected_session(&self) -> &Digest { self.0.expected_session() }
    pub(crate) fn current_source(&self) -> &SourceBinding { self.0.current_source() }
    pub(crate) fn target(&self) -> &ArtifactRef { self.0.target() }
    pub(crate) fn required_decisions(&self) -> &[String] { self.0.required_decisions() }
    pub(crate) fn retire_decisions(&self) -> &[String] { self.0.retire_decisions() }
    pub(crate) fn evidence(&self) -> &[Digest] { self.0.evidence() }
    pub(crate) fn compatibility(&self) -> &crate::product_store::scope::DecodedCompatibilityReport { self.0.compatibility() }
    pub(crate) fn scope_matches_layer(
        &self,
        request: &crate::product_store::scope::DecodedScopeRequest,
        basis_generation: u64,
    ) -> bool {
        self.0.scope_matches_layer(request, basis_generation)
    }
    pub(crate) fn identity_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, DecisionValidationError> {
        self.0.identity_with(cx)
    }

    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError> {
        self.0.validate_with(cx)
    }
}
pub(crate) fn decode_data_snapshot_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedDataSnapshot, fallible_value_decode::ValueDecodeError> {
    fallible_snapshot_decode::decode_data_snapshot(value, cx).map(FallibleDecodedDataSnapshot)
}
pub(crate) fn decode_session_state_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedSessionState, fallible_value_decode::ValueDecodeError> {
    fallible_persisted_decode::decode_session_state(value, cx).map(FallibleDecodedSessionState)
}
pub(crate) fn decode_record_ref_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<RecordRef, fallible_value_decode::ValueDecodeError> {
    fallible_persisted_decode::decode_record_ref(value, cx)
}
pub(crate) fn decode_captured_program_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedCapturedProgram, fallible_value_decode::ValueDecodeError> {
    fallible_capture_decode::decode_captured_program(value, cx).map(FallibleDecodedCapturedProgram)
}
pub(crate) fn capture_generated_app_fallible(
    app: fallible_app_decode::DecodedAppDefinition,
    project: &str,
    producer: Producer,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedCapturedProgram, AppValidationError> {
    fallible_capture_decode::capture_generated_app(app, project, producer, cx)
        .map(FallibleDecodedCapturedProgram)
}
pub(crate) fn decode_scenario_spec_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedScenarioSpec, fallible_value_decode::ValueDecodeError> {
    fallible_scenario_decode::decode_scenario_spec(value, cx).map(FallibleDecodedScenarioSpec)
}
pub(crate) fn decode_run_evidence_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedRunEvidence, fallible_value_decode::ValueDecodeError> {
    fallible_run_evidence_decode::decode_run_evidence(value, cx).map(FallibleDecodedRunEvidence)
}
pub(crate) fn decode_run_evidence_runtime(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedRunEvidence, AppValidationError> {
    fallible_run_evidence_decode::decode_run_evidence(value, cx)
        .map(FallibleDecodedRunEvidence)
        .map_err(|error| match error {
            fallible_value_decode::ValueDecodeError::Allocation(failure) => AppValidationError::Allocation(failure),
            _ => AppValidationError::Contract(ContractError("decoded runtime evidence construction invariant".into())),
        })
}
pub(crate) fn decode_decision_graph_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedDecisionGraph, fallible_value_decode::ValueDecodeError> {
    fallible_decision_decode::decode_decision_graph(value, cx).map(FallibleDecodedDecisionGraph)
}
pub(crate) fn decode_local_artifact_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedLocalArtifact, fallible_value_decode::ValueDecodeError> {
    fallible_local_artifact_decode::decode_local_artifact(value, cx).map(FallibleDecodedLocalArtifact)
}
pub(crate) fn decode_adoption_plan_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<FallibleDecodedAdoptionPlan, fallible_value_decode::ValueDecodeError> {
    fallible_decision_decode::decode_adoption_plan(value, cx).map(FallibleDecodedAdoptionPlan)
}
pub(crate) fn decode_source_binding_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<SourceBinding, fallible_value_decode::ValueDecodeError> {
    fallible_capture_decode::decode_source_binding(value, cx)
}
pub(crate) fn decode_artifact_ref_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<ArtifactRef, fallible_value_decode::ValueDecodeError> {
    fallible_capture_decode::decode_artifact(value, cx)
}
pub(crate) fn decode_run_binding_fallible(
    value: crate::product_json::Value,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<RunBinding, fallible_value_decode::ValueDecodeError> {
    fallible_run_evidence_decode::decode_run_binding(value, cx)
}

#[path = "product_contract/fallible_decision_decode.rs"]
#[allow(dead_code)]
mod fallible_decision_decode;
impl crate::product_backup::fallible_codec::BackupWire for fallible_decision_decode::DecodedDecisionGraph {}
impl crate::product_backup::fallible_codec::BackupWire for fallible_decision_decode::DecodedAdoptionPlan {}
impl crate::product_backup::fallible_codec::BackupWire for fallible_persisted_decode::DecodedSessionState {}
#[path = "product_contract/fallible_app_decode.rs"]
#[allow(dead_code)]
pub(crate) mod fallible_app_decode;
#[path = "product_contract/fallible_capture_decode.rs"]
#[allow(dead_code)]
mod fallible_capture_decode;
pub(crate) use fallible_capture_decode::DecodedCapturedProgram as DecodedCapturedProgramWire;
#[path = "product_contract/fallible_local_artifact_decode.rs"]
#[allow(dead_code)]
mod fallible_local_artifact_decode;
pub(crate) use fallible_local_artifact_decode::{ArtifactReceipt as FallibleArtifactReceipt, JsonScalar as FallibleJsonScalar, OutputRow as FallibleOutputRow};
#[path = "product_contract/fallible_scenario_decode.rs"]
#[allow(dead_code)]
mod fallible_scenario_decode;
#[path = "product_contract/fallible_observation_decode.rs"]
#[allow(dead_code)]
mod fallible_observation_decode;
#[path = "product_contract/fallible_run_evidence_decode.rs"]
#[allow(dead_code)]
mod fallible_run_evidence_decode;
#[allow(unused_imports)]
pub(crate) use values_map::FallibleValues;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidationCounts {
    pub captures: usize,
    pub apps: usize,
    pub observables: usize,
    pub views: usize,
}
#[cfg(test)]
thread_local! {
    static VALIDATION_COUNTS: std::cell::Cell<ValidationCounts> = std::cell::Cell::new(ValidationCounts::default());
}
#[cfg(test)]
pub(crate) fn reset_validation_counts() {
    VALIDATION_COUNTS.with(|counts| counts.set(ValidationCounts::default()));
}
#[cfg(test)]
pub(crate) fn validation_counts() -> ValidationCounts {
    VALIDATION_COUNTS.with(std::cell::Cell::get)
}
#[cfg(test)]
fn count_validation(update: impl FnOnce(&mut ValidationCounts)) {
    VALIDATION_COUNTS.with(|counts| {
        let mut value = counts.get();
        update(&mut value);
        counts.set(value);
    });
}

pub const CONTRACT_VERSION: u32 = 1;
pub const LANGUAGE_VERSION: &str = "gitmanager.local-app/1";
pub const MAX_WIRE_BYTES: usize = bounded_input::MAX_INPUT_BYTES;
pub const MAX_EXPR_DEPTH: usize = 16;
pub const MAX_TYPE_DEPTH: usize = 4;
pub const MAX_AST_NODES: usize = 8192;
pub const MAX_ITEMS: usize = 256;
pub const MAX_TEXT_BYTES: usize = 4096;
pub const MAX_COLLECTION: usize = 10_000;
pub const MAX_EVENTS: usize = 50_000;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub type Id = String;
pub type Values = BTreeMap<Id, DataValue>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractError(pub String);
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AppValidationError {
    Contract(ContractError),
    Allocation(crate::product_alloc::AllocationFailure),
}
impl From<ContractError> for AppValidationError {
    fn from(value: ContractError) -> Self { Self::Contract(value) }
}
type Result<T> = std::result::Result<T, ContractError>;
fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ContractError(message.into()))
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
fn check_id(id: &str) -> Result<()> {
    require(valid_id(id), format!("invalid stable ID: {id}"))
}
fn text(value: &str) -> Result<()> {
    require(value.len() <= MAX_TEXT_BYTES, "text exceeds byte limit")
}
fn nonempty(value: &str) -> Result<()> {
    text(value)?;
    require(!value.trim().is_empty(), "empty required text")
}
fn bounded(len: usize) -> Result<()> {
    require(len <= MAX_ITEMS, "too many definitions or steps")
}
fn unique<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        check_id(id)?;
        require(seen.insert(id), format!("duplicate ID: {id}"))?;
    }
    bounded(seen.len())
}
fn version(version: u32) -> Result<()> {
    require(version == CONTRACT_VERSION, "unsupported contract version")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct Digest(String);
impl Serialize for Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        // `into = String` cloned the digest before reaching the serializer.
        // A borrowed string has identical wire bytes and permits fallible
        // serializers to account for every owned output buffer themselves.
        serializer.serialize_str(&self.0)
    }
}
impl Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate and consume the original String without allocating an error.
    pub(crate) fn from_owned_lower_hex(value: String) -> Option<Self> {
        if value.len() == 64
            && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Some(Self(value))
        } else {
            None
        }
    }
}
impl TryFrom<String> for Digest {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self> {
        require(
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid SHA-256 digest",
        )?;
        Ok(Self(value))
    }
}
impl From<Digest> for String {
    fn from(value: Digest) -> Self {
        value.0
    }
}

#[path = "product_contract/identity_domain.rs"]
mod identity_domain;
pub use self::identity_domain::IdentityDomain;
fn bytes_digest(domain: IdentityDomain, bytes: &[u8]) -> Digest {
    let mut input = b"gitmanager.product.v1\0".to_vec();
    input.extend(serde_json::to_vec(&domain).expect("finite enum"));
    input.push(0);
    input.extend(bytes);
    Digest(
        digest(&SHA256, &input)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// Canonical JSON uses lexically sorted object keys, preserves arrays and exact
/// typed values, and contains no whitespace. This is a versioned local format,
/// not a claim of cross-language RFC 8785 floating-point canonicalization.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_value(value)
        .and_then(|mut value| {
            value.sort_all_objects();
            serde_json::to_vec(&value)
        })
        .map_err(|e| ContractError(e.to_string()))
}
pub fn canonical_digest<T: Serialize>(domain: IdentityDomain, value: &T) -> Result<Digest> {
    Ok(bytes_digest(domain, &canonical_bytes(value)?))
}
/// A fresh canonical encoding, not a semantic proof or admission capability.
/// No raw-byte constructor, mutable view, extraction, clone or wire format exists.
pub(crate) struct CanonicalEvidenceBytes(Vec<u8>);
impl CanonicalEvidenceBytes {
    pub(crate) fn from_value<T: Serialize>(value: &T) -> Result<Self> {
        Ok(Self(canonical_bytes(value)?))
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn into_evidence_digest(self) -> Digest {
        // The original variable-size domain buffer, SHA-256 and hex work remain.
        bytes_digest(IdentityDomain::Evidence, &self.0)
    }
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let value = bounded_input::parse_json_bytes(bytes).map_err(|e| ContractError(e.to_string()))?;
    serde_json::from_value(value).map_err(|e| ContractError(e.to_string()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Type {
    Boolean,
    Integer,
    Text,
    Date,
    Reference { entity: Id },
    List { item: Box<Type> },
    Optional { item: Box<Type> },
}
impl Type {
    pub fn list(item: Type) -> Self {
        Self::List {
            item: Box::new(item),
        }
    }
    pub fn reference(entity: impl Into<Id>) -> Self {
        Self::Reference {
            entity: entity.into(),
        }
    }
    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            Self::Boolean | Self::Integer | Self::Text | Self::Date | Self::Reference { .. }
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataValue {
    Null,
    Boolean {
        value: bool,
    },
    Integer {
        value: i64,
    },
    Text {
        value: String,
    },
    /// Gregorian day number relative to 1970-01-01; arithmetic must be checked.
    Date {
        days: i32,
    },
    Reference {
        entity: Id,
        record: Id,
    },
    List {
        item_type: Type,
        items: Vec<DataValue>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    Literal {
        value_type: Type,
        value: DataValue,
    },
    Variable {
        name: Id,
    },
    Field {
        record: Box<Expr>,
        field: Id,
    },
    State {
        state: Id,
    },
    Today,
    Query {
        entity: Id,
        binding: Id,
        predicate: Box<Expr>,
        sort: Vec<SortKey>,
        limit: usize,
        include_archived: bool,
    },
    Filter {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    Map {
        items: Box<Expr>,
        binding: Id,
        value: Box<Expr>,
    },
    Count {
        items: Box<Expr>,
    },
    Any {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    All {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    Sum {
        items: Box<Expr>,
    },
    Contains {
        items: Box<Expr>,
        value: Box<Expr>,
    },
    Equal {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Less {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Subtract {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Concat {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    TextContains {
        text: Box<Expr>,
        search: Box<Expr>,
    },
    Lower {
        value: Box<Expr>,
    },
    And {
        values: Vec<Expr>,
    },
    Or {
        values: Vec<Expr>,
    },
    Not {
        value: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_value: Box<Expr>,
        else_value: Box<Expr>,
    },
    Coalesce {
        value: Box<Expr>,
        fallback: Box<Expr>,
    },
    DateAdd {
        date: Box<Expr>,
        days: Box<Expr>,
    },
    DateDifference {
        later: Box<Expr>,
        earlier: Box<Expr>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortKey {
    pub value: Expr,
    pub descending: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDefinition {
    pub id: Id,
    pub label: String,
    pub value_type: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDefinition {
    pub id: Id,
    pub label: String,
    pub fields: Vec<FieldDefinition>,
    /// Unique tuples among non-archived records, excluding tuples containing null.
    pub unique: Vec<Vec<Id>>,
    /// Boolean expressions with `record` bound to each non-archived row. No state.
    pub constraints: Vec<Expr>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDefinition {
    pub id: Id,
    pub label: String,
    pub value_type: Type,
    pub initial: DataValue,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDefinition {
    pub id: Id,
    pub label: String,
    pub parameters: BTreeMap<Id, Type>,
    pub guards: Vec<Expr>,
    pub steps: Vec<Statement>,
    pub ensures: Vec<Expr>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Statement {
    Create {
        entity: Id,
        values: BTreeMap<Id, Expr>,
        bind: Id,
    },
    Update {
        record: Expr,
        values: BTreeMap<Id, Expr>,
    },
    Archive {
        record: Expr,
    },
    SetState {
        state: Id,
        value: Expr,
    },
    Collection {
        target: CollectionTarget,
        operation: CollectionOperation,
        items: Expr,
    },
    ForEach {
        items: Expr,
        binding: Id,
        steps: Vec<Statement>,
    },
    Assert {
        condition: Expr,
        message: String,
    },
    Emit {
        output: Id,
        items: Expr,
        binding: Id,
        columns: BTreeMap<Id, Expr>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollectionTarget {
    State { state: Id },
    Field { record: Expr, field: Id },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionOperation {
    Insert,
    Remove,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Csv,
    Json,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputDefinition {
    pub id: Id,
    pub label: String,
    pub format: OutputFormat,
    pub columns: Vec<FieldDefinition>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservableDefinition {
    pub id: Id,
    pub label: String,
    pub value: Expr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub id: Id,
    pub label: String,
    pub value: Expr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateControl {
    pub id: Id,
    pub label: String,
    pub state: Id,
    pub on_change: Option<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionBinding {
    pub id: Id,
    pub state: Id,
    pub on_change: Option<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormInput {
    pub parameter: Id,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewKind {
    List {
        entity: Id,
        rows: Expr,
        columns: Vec<Column>,
        controls: Vec<StateControl>,
        selection: Option<SelectionBinding>,
    },
    Detail {
        entity: Id,
        record: Expr,
        columns: Vec<Column>,
    },
    Form {
        action: Id,
        fields: Vec<FormInput>,
        defaults: Values,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPlacement {
    Toolbar,
    Row,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
    pub id: Id,
    pub label: String,
    pub placement: ActionPlacement,
    pub action: Id,
    pub arguments: BTreeMap<Id, Expr>,
    pub enabled: Expr,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyModifier {
    Control,
    Alt,
    Shift,
    Command,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBinding {
    pub key: String,
    pub modifiers: BTreeSet<KeyModifier>,
    pub binding: Id,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewDefinition {
    pub id: Id,
    pub label: String,
    pub kind: ViewKind,
    pub actions: Vec<ActionBinding>,
    pub keys: Vec<KeyBinding>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub version: u32,
    pub id: Id,
    pub label: String,
    pub entities: Vec<EntityDefinition>,
    pub state: Vec<StateDefinition>,
    pub actions: Vec<ActionDefinition>,
    pub outputs: Vec<OutputDefinition>,
    pub observables: Vec<ObservableDefinition>,
    pub views: Vec<ViewDefinition>,
    pub initial_view: Id,
}

#[derive(Clone, Copy)]
pub(crate) enum AppRef<'a> {
    Legacy(&'a AppDefinition),
    Decoded(&'a fallible_app_decode::DecodedAppDefinition),
    Snapshot,
}
pub(crate) enum StringMapRef<'a, V> {
    Legacy(&'a BTreeMap<String, V>),
    Fallible(&'a fallible_app_decode::FallibleMap<V>),
}
impl<V> Copy for StringMapRef<'_, V> {}
impl<V> Clone for StringMapRef<'_, V> { fn clone(&self) -> Self { *self } }
impl<V> StringMapRef<'_, V> { pub(crate) fn is_empty(&self) -> bool { self.len()==0 } }
pub(crate) enum StringMapIter<'a, V> {
    Legacy(std::collections::btree_map::Iter<'a, String, V>),
    Fallible(fallible_app_decode::FallibleMapIter<'a, V>),
}
impl<'a, V> Iterator for StringMapIter<'a, V> {
    type Item = (&'a String, &'a V);
    fn next(&mut self) -> Option<Self::Item> { match self { Self::Legacy(i) => i.next(), Self::Fallible(i) => i.next() } }
}
impl<V> ExactSizeIterator for StringMapIter<'_, V> {}
impl<V> StringValueMap<V> for StringMapRef<'_, V> {
    type Iter<'a> = StringMapIter<'a, V> where Self: 'a, V: 'a;
    fn len(&self) -> usize { match self { Self::Legacy(m) => m.len(), Self::Fallible(m) => m.len() } }
    fn get(&self, key: &str) -> Option<&V> { match self { Self::Legacy(m) => m.get(key), Self::Fallible(m) => m.get(key) } }
    fn contains_key(&self, key: &str) -> bool { match self { Self::Legacy(m) => m.contains_key(key), Self::Fallible(m) => m.contains_key(key) } }
    fn iter(&self) -> Self::Iter<'_> { match self { Self::Legacy(m) => StringMapIter::Legacy(m.iter()), Self::Fallible(m) => StringMapIter::Fallible(m.iter()) } }
}
pub(crate) enum ActionRef<'a> {
    Legacy(&'a ActionDefinition),
    Decoded(&'a fallible_app_decode::DecodedActionDefinition),
}
pub(crate) enum ActionSlice<'a> {
    Legacy(&'a [ActionDefinition]),
    Decoded(&'a [fallible_app_decode::DecodedActionDefinition]),
}
pub(crate) enum ActionIter<'a> {
    Legacy(std::slice::Iter<'a, ActionDefinition>),
    Decoded(std::slice::Iter<'a, fallible_app_decode::DecodedActionDefinition>),
}
impl<'a> Iterator for ActionIter<'a> {
    type Item = ActionRef<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Legacy(iter) => iter.next().map(ActionRef::Legacy),
            Self::Decoded(iter) => iter.next().map(ActionRef::Decoded),
        }
    }
}
impl<'a> ActionSlice<'a> {
    pub(crate) fn iter(&self) -> ActionIter<'a> {
        match self {
            Self::Legacy(items) => ActionIter::Legacy(items.iter()),
            Self::Decoded(items) => ActionIter::Decoded(items.iter()),
        }
    }
}
impl<'a> ActionRef<'a> {
    pub(crate) fn id(&self) -> &'a str { match self { Self::Legacy(v) => &v.id, Self::Decoded(v) => &v.id } }
    pub(crate) fn label(&self) -> &'a str { match self { Self::Legacy(v) => &v.label, Self::Decoded(v) => &v.label } }
    pub(crate) fn parameters(&self) -> StringMapRef<'_, Type> { match self { Self::Legacy(v) => StringMapRef::Legacy(&v.parameters), Self::Decoded(v) => StringMapRef::Fallible(&v.parameters) } }
    pub(crate) fn guards(&self) -> &[Expr] { match self { Self::Legacy(v) => &v.guards, Self::Decoded(v) => &v.guards } }
    pub(crate) fn ensures(&self) -> &[Expr] { match self { Self::Legacy(v) => &v.ensures, Self::Decoded(v) => &v.ensures } }
    pub(crate) fn steps(&self) -> StatementSlice<'a> { match self { Self::Legacy(v) => StatementSlice::Legacy(&v.steps), Self::Decoded(v) => StatementSlice::Decoded(&v.steps) } }
}
trait ActionNode {
    fn action_id(&self) -> &str;
    fn action_label(&self) -> &str;
    fn action_parameters(&self) -> StringMapRef<'_, Type>;
    fn action_guards(&self) -> &[Expr];
    fn action_ensures(&self) -> &[Expr];
    fn action_steps(&self) -> StatementSlice<'_>;
}
impl ActionNode for ActionDefinition {
    fn action_id(&self) -> &str { &self.id }
    fn action_label(&self) -> &str { &self.label }
    fn action_parameters(&self) -> StringMapRef<'_, Type> { StringMapRef::Legacy(&self.parameters) }
    fn action_guards(&self) -> &[Expr] { &self.guards }
    fn action_ensures(&self) -> &[Expr] { &self.ensures }
    fn action_steps(&self) -> StatementSlice<'_> { StatementSlice::Legacy(&self.steps) }
}
impl ActionNode for fallible_app_decode::DecodedActionDefinition {
    fn action_id(&self) -> &str { &self.id }
    fn action_label(&self) -> &str { &self.label }
    fn action_parameters(&self) -> StringMapRef<'_, Type> { StringMapRef::Fallible(&self.parameters) }
    fn action_guards(&self) -> &[Expr] { &self.guards }
    fn action_ensures(&self) -> &[Expr] { &self.ensures }
    fn action_steps(&self) -> StatementSlice<'_> { StatementSlice::Decoded(&self.steps) }
}
impl ActionNode for ActionRef<'_> {
    fn action_id(&self) -> &str { self.id() }
    fn action_label(&self) -> &str { self.label() }
    fn action_parameters(&self) -> StringMapRef<'_, Type> { self.parameters() }
    fn action_guards(&self) -> &[Expr] { self.guards() }
    fn action_ensures(&self) -> &[Expr] { self.ensures() }
    fn action_steps(&self) -> StatementSlice<'_> { self.steps() }
}
trait ColumnNode { fn id(&self)->&str; fn label(&self)->&str; fn value(&self)->&Expr; }
impl ColumnNode for Column { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn value(&self)->&Expr{&self.value} }
impl ColumnNode for fallible_app_decode::DecodedColumn { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn value(&self)->&Expr{&self.value} }
trait ControlNode { fn id(&self)->&str; fn label(&self)->&str; fn state(&self)->&str; fn on_change(&self)->Option<&str>; }
impl ControlNode for StateControl { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn state(&self)->&str{&self.state} fn on_change(&self)->Option<&str>{self.on_change.as_deref()} }
impl ControlNode for fallible_app_decode::DecodedStateControl { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn state(&self)->&str{&self.state} fn on_change(&self)->Option<&str>{self.on_change.as_deref()} }
trait SelectionNode { fn id(&self)->&str; fn state(&self)->&str; fn on_change(&self)->Option<&str>; }
impl SelectionNode for SelectionBinding { fn id(&self)->&str{&self.id} fn state(&self)->&str{&self.state} fn on_change(&self)->Option<&str>{self.on_change.as_deref()} }
impl SelectionNode for fallible_app_decode::DecodedSelectionBinding { fn id(&self)->&str{&self.id} fn state(&self)->&str{&self.state} fn on_change(&self)->Option<&str>{self.on_change.as_deref()} }
trait FormInputNode { fn parameter(&self)->&str; fn label(&self)->&str; }
impl FormInputNode for FormInput { fn parameter(&self)->&str{&self.parameter} fn label(&self)->&str{&self.label} }
impl FormInputNode for fallible_app_decode::DecodedFormInput { fn parameter(&self)->&str{&self.parameter} fn label(&self)->&str{&self.label} }
trait BindingNode { fn id(&self)->&str; fn label(&self)->&str; fn placement(&self)->ActionPlacement; fn action(&self)->&str; fn arguments(&self)->StringMapRef<'_,Expr>; fn enabled(&self)->&Expr; }
impl BindingNode for ActionBinding { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn placement(&self)->ActionPlacement{self.placement} fn action(&self)->&str{&self.action} fn arguments(&self)->StringMapRef<'_,Expr>{StringMapRef::Legacy(&self.arguments)} fn enabled(&self)->&Expr{&self.enabled} }
impl BindingNode for fallible_app_decode::DecodedActionBinding { fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn placement(&self)->ActionPlacement{self.placement} fn action(&self)->&str{&self.action} fn arguments(&self)->StringMapRef<'_,Expr>{StringMapRef::Fallible(&self.arguments)} fn enabled(&self)->&Expr{&self.enabled} }
trait KeyNode { fn key(&self)->&str; fn mask(&self)->u8; fn modifier_count(&self)->usize; fn binding(&self)->&str; }
impl KeyNode for KeyBinding { fn key(&self)->&str{&self.key} fn mask(&self)->u8{self.modifiers.iter().fold(0,|m,v|m|match v{KeyModifier::Control=>1,KeyModifier::Alt=>2,KeyModifier::Shift=>4,KeyModifier::Command=>8})} fn modifier_count(&self)->usize{self.modifiers.len()} fn binding(&self)->&str{&self.binding} }
impl KeyNode for fallible_app_decode::DecodedKeyBinding { fn key(&self)->&str{&self.key} fn mask(&self)->u8{self.modifiers.mask()} fn modifier_count(&self)->usize{self.modifiers.len()} fn binding(&self)->&str{&self.binding} }
enum ViewKindView<'a, V: ViewNode> {
    List { entity:&'a str, rows:&'a Expr, columns:&'a [V::Column], controls:&'a [V::Control], selection:Option<&'a V::Selection> },
    Detail { entity:&'a str, record:&'a Expr, columns:&'a [V::Column] },
    Form { action:&'a str, fields:&'a [V::FormInput], defaults:&'a V::Defaults },
}
trait ViewNode {
    type Column: ColumnNode; type Control: ControlNode; type Selection: SelectionNode; type FormInput: FormInputNode;
    type Binding: BindingNode; type Key: KeyNode; type Defaults: StringValueMap<DataValue>;
    fn id(&self)->&str; fn label(&self)->&str; fn kind(&self)->ViewKindView<'_,Self> where Self:Sized;
    fn actions(&self)->&[Self::Binding]; fn keys(&self)->&[Self::Key];
    fn visit_expressions(&self, visit: &mut dyn FnMut(&Expr)) where Self: Sized {
        match self.kind() {
            ViewKindView::List { rows, columns, .. } => {
                visit(rows);
                for column in columns { visit(column.value()); }
            }
            ViewKindView::Detail { record, columns, .. } => {
                visit(record);
                for column in columns { visit(column.value()); }
            }
            ViewKindView::Form { .. } => {}
        }
        for binding in self.actions() {
            visit(binding.enabled());
            for (_, value) in binding.arguments().iter() { visit(value); }
        }
    }
    fn visit_action_expressions(&self, visit: &mut dyn FnMut(&Expr)) where Self: Sized {
        for binding in self.actions() {
            visit(binding.enabled());
            for (_, value) in binding.arguments().iter() { visit(value); }
        }
    }
}
impl ViewNode for ViewDefinition {
    type Column=Column; type Control=StateControl; type Selection=SelectionBinding; type FormInput=FormInput; type Binding=ActionBinding; type Key=KeyBinding; type Defaults=Values;
    fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn actions(&self)->&[Self::Binding]{&self.actions} fn keys(&self)->&[Self::Key]{&self.keys}
    fn kind(&self)->ViewKindView<'_,Self>{match &self.kind{ViewKind::List{entity,rows,columns,controls,selection}=>ViewKindView::List{entity,rows,columns,controls,selection:selection.as_ref()},ViewKind::Detail{entity,record,columns}=>ViewKindView::Detail{entity,record,columns},ViewKind::Form{action,fields,defaults}=>ViewKindView::Form{action,fields,defaults}}}
}
impl ViewNode for fallible_app_decode::DecodedViewDefinition {
    type Column=fallible_app_decode::DecodedColumn; type Control=fallible_app_decode::DecodedStateControl; type Selection=fallible_app_decode::DecodedSelectionBinding; type FormInput=fallible_app_decode::DecodedFormInput; type Binding=fallible_app_decode::DecodedActionBinding; type Key=fallible_app_decode::DecodedKeyBinding; type Defaults=FallibleValues;
    fn id(&self)->&str{&self.id} fn label(&self)->&str{&self.label} fn actions(&self)->&[Self::Binding]{&self.actions} fn keys(&self)->&[Self::Key]{&self.keys}
    fn kind(&self)->ViewKindView<'_,Self>{use fallible_app_decode::DecodedViewKind as K;match &self.kind{K::List{entity,rows,columns,controls,selection}=>ViewKindView::List{entity,rows,columns,controls,selection:selection.as_ref()},K::Detail{entity,record,columns}=>ViewKindView::Detail{entity,record,columns},K::Form{action,fields,defaults}=>ViewKindView::Form{action,fields,defaults}}}
}
#[derive(Clone, Copy)]
pub(crate) enum StatementSlice<'a> {
    Legacy(&'a [Statement]),
    Decoded(&'a [fallible_app_decode::DecodedStatement]),
}
#[derive(Clone, Copy)]
pub(crate) enum StatementView<'a> {
    Create { entity: &'a str, values: StringMapRef<'a, Expr>, bind: &'a str },
    Update { record: &'a Expr, values: StringMapRef<'a, Expr> },
    Archive { record: &'a Expr },
    SetState { state: &'a str, value: &'a Expr },
    Collection { target: &'a CollectionTarget, operation: CollectionOperation, items: &'a Expr },
    ForEach { items: &'a Expr, binding: &'a str, steps: StatementSlice<'a> },
    Assert { condition: &'a Expr, message: &'a str },
    Emit { output: &'a str, items: &'a Expr, binding: &'a str, columns: StringMapRef<'a, Expr> },
}
trait StatementNode { fn statement_view(&self) -> StatementView<'_>; }
impl StatementNode for Statement {
    fn statement_view(&self) -> StatementView<'_> { match self {
        Self::Create { entity, values, bind } => StatementView::Create { entity, values: StringMapRef::Legacy(values), bind },
        Self::Update { record, values } => StatementView::Update { record, values: StringMapRef::Legacy(values) },
        Self::Archive { record } => StatementView::Archive { record },
        Self::SetState { state, value } => StatementView::SetState { state, value },
        Self::Collection { target, operation, items } => StatementView::Collection { target, operation: *operation, items },
        Self::ForEach { items, binding, steps } => StatementView::ForEach { items, binding, steps: StatementSlice::Legacy(steps) },
        Self::Assert { condition, message } => StatementView::Assert { condition, message },
        Self::Emit { output, items, binding, columns } => StatementView::Emit { output, items, binding, columns: StringMapRef::Legacy(columns) },
    }}
}
impl StatementNode for fallible_app_decode::DecodedStatement {
    fn statement_view(&self) -> StatementView<'_> { use fallible_app_decode::DecodedStatement as S; match self {
        S::Create { entity, values, bind } => StatementView::Create { entity, values: StringMapRef::Fallible(values), bind },
        S::Update { record, values } => StatementView::Update { record, values: StringMapRef::Fallible(values) },
        S::Archive { record } => StatementView::Archive { record },
        S::SetState { state, value } => StatementView::SetState { state, value },
        S::Collection { target, operation, items } => StatementView::Collection { target, operation: *operation, items },
        S::ForEach { items, binding, steps } => StatementView::ForEach { items, binding, steps: StatementSlice::Decoded(steps) },
        S::Assert { condition, message } => StatementView::Assert { condition, message },
        S::Emit { output, items, binding, columns } => StatementView::Emit { output, items, binding, columns: StringMapRef::Fallible(columns) },
    }}
}
impl StatementSlice<'_> {
    fn visit(&self, f: &mut dyn FnMut(&dyn StatementNode) -> Result<()>) -> Result<()> {
        match self {
            Self::Legacy(items) => for item in *items { f(item)?; },
            Self::Decoded(items) => for item in *items { f(item)?; },
        }
        Ok(())
    }
}
impl<'a> StatementSlice<'a> {
    pub(crate) fn len(&self) -> usize { match self { Self::Legacy(v) => v.len(), Self::Decoded(v) => v.len() } }
    pub(crate) fn is_empty(&self) -> bool { self.len() == 0 }
    pub(crate) fn slice_prefix(self, end: usize) -> Self { match self { Self::Legacy(v) => Self::Legacy(&v[..end]), Self::Decoded(v) => Self::Decoded(&v[..end]) } }
    pub(crate) fn slice_range(self, start: usize, len: usize) -> Self { let end=start+len; match self { Self::Legacy(v) => Self::Legacy(&v[start..end]), Self::Decoded(v) => Self::Decoded(&v[start..end]) } }
    pub(crate) fn get(&self, index: usize) -> Option<StatementView<'a>> {
        match self {
            Self::Legacy(items) => items.get(index).map(StatementNode::statement_view),
            Self::Decoded(items) => items.get(index).map(StatementNode::statement_view),
        }
    }
    pub(crate) fn statement_digest_with(
        &self, index: usize, cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        match self {
            Self::Legacy(items) => crate::product_backup::fallible_codec::canonical_digest(
                IdentityDomain::Evidence, items.get(index).ok_or(crate::product_backup::fallible_codec::CodecError::SerializationInvariant)?, cx,
            ),
            Self::Decoded(items) => crate::product_backup::fallible_codec::canonical_digest(
                IdentityDomain::Evidence, items.get(index).ok_or(crate::product_backup::fallible_codec::CodecError::SerializationInvariant)?, cx,
            ),
        }
    }
    pub(crate) fn span_digest_with(
        &self, start: usize, len: usize, cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Digest, crate::product_backup::fallible_codec::CodecError> {
        let end = start.checked_add(len).ok_or(crate::product_backup::fallible_codec::CodecError::SerializationInvariant)?;
        match self {
            Self::Legacy(items) => crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence, &("owned-span/1", items.get(start..end).ok_or(crate::product_backup::fallible_codec::CodecError::SerializationInvariant)?), cx),
            Self::Decoded(items) => crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence, &("owned-span/1", items.get(start..end).ok_or(crate::product_backup::fallible_codec::CodecError::SerializationInvariant)?), cx),
        }
    }
}
impl<'a> AppRef<'a> {
    pub(crate) fn is_fallible(&self) -> bool { matches!(self, Self::Decoded(_)) }
    pub(crate) fn legacy(&self) -> Option<&'a AppDefinition> { match self { Self::Legacy(app) => Some(app), _ => None } }
    pub(crate) fn entities(&self) -> &'a [EntityDefinition] {
        match self {
            Self::Legacy(app) => &app.entities,
            Self::Decoded(app) => &app.entities,
            Self::Snapshot => &[],
        }
    }
    pub(crate) fn states(&self) -> &'a [StateDefinition] {
        match self {
            Self::Legacy(app) => &app.state,
            Self::Decoded(app) => &app.state,
            Self::Snapshot => &[],
        }
    }
    pub(crate) fn id(&self) -> Option<&'a str> {
        match self { Self::Legacy(app) => Some(&app.id), Self::Decoded(app) => Some(&app.id), Self::Snapshot => None }
    }
    pub(crate) fn label(&self) -> Option<&'a str> {
        match self { Self::Legacy(app) => Some(&app.label), Self::Decoded(app) => Some(&app.label), Self::Snapshot => None }
    }
    pub(crate) fn version(&self) -> Option<u32> {
        match self { Self::Legacy(app) => Some(app.version), Self::Decoded(app) => Some(app.version), Self::Snapshot => None }
    }
    pub(crate) fn initial_view(&self) -> Option<&'a str> {
        match self { Self::Legacy(app) => Some(&app.initial_view), Self::Decoded(app) => Some(&app.initial_view), Self::Snapshot => None }
    }
    pub(crate) fn observable_count(&self) -> usize {
        match self { Self::Legacy(app) => app.observables.len(), Self::Decoded(app) => app.observables.len(), Self::Snapshot => 0 }
    }
    pub(crate) fn observable(&self, index: usize) -> Option<(&'a str, &'a Expr)> {
        match self {
            Self::Legacy(app) => app.observables.get(index).map(|v| (v.id.as_str(), &v.value)),
            Self::Decoded(app) => app.observables.get(index).map(|v| (v.id.as_str(), &v.value)),
            Self::Snapshot => None,
        }
    }
    pub(crate) fn column_expression(&self, view: &str, column: &str) -> Option<(&'a str, &'a Expr)> {
        match self {
            Self::Legacy(app) => app.views.iter().find(|v| v.id == view).and_then(|view| match &view.kind {
                ViewKind::List { entity, columns, .. } | ViewKind::Detail { entity, columns, .. } => columns.iter().find(|c| c.id == column).map(|c| (entity.as_str(), &c.value)),
                ViewKind::Form { .. } => None,
            }),
            Self::Decoded(app) => app.views.iter().find(|v| v.id == view).and_then(|view| match &view.kind {
                fallible_app_decode::DecodedViewKind::List { entity, columns, .. } | fallible_app_decode::DecodedViewKind::Detail { entity, columns, .. } => columns.iter().find(|c| c.id == column).map(|c| (entity.as_str(), &c.value)),
                fallible_app_decode::DecodedViewKind::Form { .. } => None,
            }),
            Self::Snapshot => None,
        }
    }
    pub(crate) fn visit_view_action_expressions(&self, visit: &mut dyn FnMut(&Expr)) {
        match self {
            Self::Legacy(app) => for view in &app.views { view.visit_action_expressions(visit); },
            Self::Decoded(app) => for view in &app.views { view.visit_action_expressions(visit); },
            Self::Snapshot => {}
        }
    }
    pub(crate) fn visit_expressions(&self, visit: &mut dyn FnMut(&Expr)) {
        fn statements(steps: StatementSlice<'_>, visit: &mut dyn FnMut(&Expr)) {
            for index in 0..steps.len() {
                match steps.get(index).expect("bounded statement index") {
                    StatementView::Create { values, .. } => for (_, value) in values.iter() { visit(value); },
                    StatementView::Update { record, values } => {
                        visit(record);
                        for (_, value) in values.iter() { visit(value); }
                    }
                    StatementView::Archive { record } => visit(record),
                    StatementView::SetState { value, .. } => visit(value),
                    StatementView::Collection { target, items, .. } => {
                        if let CollectionTarget::Field { record, .. } = target { visit(record); }
                        visit(items);
                    }
                    StatementView::ForEach { items, steps, .. } => { visit(items); statements(steps, visit); }
                    StatementView::Assert { condition, .. } => visit(condition),
                    StatementView::Emit { items, columns, .. } => {
                        visit(items);
                        for (_, value) in columns.iter() { visit(value); }
                    }
                }
            }
        }
        match self {
            Self::Legacy(app) => {
                for entity in &app.entities { for constraint in &entity.constraints { visit(constraint); } }
                for action in &app.actions {
                    for expr in action.guards.iter().chain(&action.ensures) { visit(expr); }
                    statements(StatementSlice::Legacy(&action.steps), visit);
                }
                for observable in &app.observables { visit(&observable.value); }
                for view in &app.views { view.visit_expressions(visit); }
            }
            Self::Decoded(app) => {
                for entity in &app.entities { for constraint in &entity.constraints { visit(constraint); } }
                for action in &app.actions {
                    for expr in action.guards.iter().chain(&action.ensures) { visit(expr); }
                    statements(StatementSlice::Decoded(&action.steps), visit);
                }
                for observable in &app.observables { visit(&observable.value); }
                for view in &app.views { view.visit_expressions(visit); }
            }
            Self::Snapshot => {}
        }
    }
    pub(crate) fn entity_field_type(&self, entity: &str, field: &str) -> Option<&'a Type> {
        let entities = match self {
            Self::Legacy(app) => app.entities.as_slice(),
            Self::Decoded(app) => app.entities.as_slice(),
            Self::Snapshot => return None,
        };
        entities.iter().find(|e| e.id == entity)
            .and_then(|e| e.fields.iter().find(|f| f.id == field))
            .map(|f| &f.value_type)
    }
    pub(crate) fn state_type(&self, id: &str) -> Option<&'a Type> {
        let states = match self {
            Self::Legacy(app) => app.state.as_slice(),
            Self::Decoded(app) => app.state.as_slice(),
            Self::Snapshot => return None,
        };
        states.iter().find(|s| s.id == id).map(|s| &s.value_type)
    }
    pub(crate) fn action(&self, id: &str) -> Option<ActionRef<'a>> {
        match self {
            Self::Legacy(app) => app.actions.iter().find(|a| a.id == id).map(ActionRef::Legacy),
            Self::Decoded(app) => app.actions.iter().find(|a| a.id == id).map(ActionRef::Decoded),
            Self::Snapshot => None,
        }
    }
    pub(crate) fn actions(&self) -> ActionSlice<'a> {
        match self {
            Self::Legacy(app) => ActionSlice::Legacy(&app.actions),
            Self::Decoded(app) => ActionSlice::Decoded(&app.actions),
            Self::Snapshot => ActionSlice::Legacy(&[]),
        }
    }
    pub(crate) fn view_entity(&self, id: &str) -> Option<Option<&'a str>> {
        match self {
            Self::Legacy(app) => app.views.iter().find(|v| v.id == id).map(|v| match &v.kind {
                ViewKind::List { entity, .. } | ViewKind::Detail { entity, .. } => Some(entity.as_str()),
                ViewKind::Form { .. } => None,
            }),
            Self::Decoded(app) => app.views.iter().find(|v| v.id == id).map(|v| match &v.kind {
                fallible_app_decode::DecodedViewKind::List { entity, .. }
                | fallible_app_decode::DecodedViewKind::Detail { entity, .. } => Some(entity.as_str()),
                fallible_app_decode::DecodedViewKind::Form { .. } => None,
            }),
            Self::Snapshot => None,
        }
    }
}
impl<'a> From<&'a AppDefinition> for AppRef<'a> {
    fn from(value: &'a AppDefinition) -> Self { Self::Legacy(value) }
}
impl<'a> From<&'a fallible_app_decode::DecodedAppDefinition> for AppRef<'a> {
    fn from(value: &'a fallible_app_decode::DecodedAppDefinition) -> Self { Self::Decoded(value) }
}
enum AppEnvironment {
    Legacy(BTreeMap<Id, Type>),
    Decoded(fallible_app_decode::FallibleMap<Type>),
}
type Environment = BTreeMap<Id, Type>;
enum EnvironmentIter<'a> {
    Legacy(std::collections::btree_map::Iter<'a, Id, Type>),
    Decoded(fallible_app_decode::FallibleMapIter<'a, Type>),
}
impl<'a> Iterator for EnvironmentIter<'a> {
    type Item = (&'a String, &'a Type);
    fn next(&mut self) -> Option<Self::Item> {
        match self { Self::Legacy(iter) => iter.next(), Self::Decoded(iter) => iter.next() }
    }
}
impl ExactSizeIterator for EnvironmentIter<'_> {}
impl StringValueMap<Type> for AppEnvironment {
    type Iter<'a> = EnvironmentIter<'a> where Self: 'a, Type: 'a;
    fn len(&self) -> usize {
        match self { Self::Legacy(map) => map.len(), Self::Decoded(map) => map.len() }
    }
    fn get(&self, key: &str) -> Option<&Type> {
        match self { Self::Legacy(map) => map.get(key), Self::Decoded(map) => map.get(key) }
    }
    fn contains_key(&self, key: &str) -> bool {
        match self { Self::Legacy(map) => map.contains_key(key), Self::Decoded(map) => map.contains_key(key) }
    }
    fn iter(&self) -> Self::Iter<'_> {
        match self { Self::Legacy(map) => EnvironmentIter::Legacy(map.iter()), Self::Decoded(map) => EnvironmentIter::Decoded(map.iter()) }
    }
}
fn charge_type_map_clone<M: StringValueMap<Type>>(audit: Audit<'_>, map: &M) -> Result<()> {
    if !audit.enabled() { return Ok(()); }
    audit.pay(1)?;
    audit.map_iter(map.len())?;
    for (key, value) in map.iter() {
        audit.pay(4)?;
        key.pay_owned(audit)?;
        value.pay_owned(audit)?;
    }
    audit.pay(crate::product_contract::bounded_input::capture_payment_core::CLONE_DESCRIPTOR)?;
    audit.amount(crate::product_contract::bounded_input::capture_payment_core::btree_clone::<String, Type>(map.len(), 0))
}
impl AppEnvironment {
    fn empty(app: AppRef<'_>) -> Self {
        match app { AppRef::Decoded(_) => Self::Decoded(fallible_app_decode::FallibleMap::new()), _ => Self::Legacy(BTreeMap::new()) }
    }
}
struct Validator<'a, 'w, 'c> {
    entities: &'a [EntityDefinition],
    state: &'a [StateDefinition],
    actions: &'a [ActionDefinition],
    outputs: &'a [OutputDefinition],
    app: AppRef<'a>,
    allocation: Option<&'c mut crate::product_alloc::AllocationContext>,
    allocation_failure: Option<crate::product_alloc::AllocationFailure>,
    nodes: usize,
    audit: Audit<'w>,
}
impl<'a, 'w, 'c> Validator<'a, 'w, 'c> {
    fn new(app: &'a AppDefinition, audit: Audit<'w>) -> Self {
        Self { entities:&app.entities, state:&app.state, actions:&app.actions, outputs:&app.outputs,
            app:AppRef::Legacy(app), allocation:None, allocation_failure:None, nodes:0, audit }
    }
    fn new_app(app: AppRef<'a>, audit: Audit<'w>, allocation: &'c mut crate::product_alloc::AllocationContext) -> Self {
        let (entities,state,actions,outputs)=match app {
            AppRef::Legacy(app)=>(app.entities.as_slice(),app.state.as_slice(),app.actions.as_slice(),app.outputs.as_slice()),
            AppRef::Decoded(app)=>(app.entities.as_slice(),app.state.as_slice(),&[][..],app.outputs.as_slice()),
            AppRef::Snapshot => (&[][..],&[][..],&[][..],&[][..]),
        };
        Self { entities, state, actions, outputs, app, allocation:Some(allocation), allocation_failure:None, nodes:0, audit }
    }
    fn is_decoded(&self) -> bool { matches!(self.app, AppRef::Decoded(_)) }
    fn empty_environment(&self) -> AppEnvironment { AppEnvironment::empty(self.app) }
    fn clone_environment(&mut self, env: &AppEnvironment) -> Result<AppEnvironment> {
        let a = self.audit;
        match env {
            AppEnvironment::Legacy(map) => Ok(AppEnvironment::Legacy(a.clone_owned(map)?)),
            AppEnvironment::Decoded(map) => {
                charge_type_map_clone(a, map)?;
                let result = map.try_clone_with(self.allocation.as_deref_mut().expect("decoded app has allocation context"), |value, cx| values_map::try_clone_type(value, cx));
                match result {
                    Ok(copy) => Ok(AppEnvironment::Decoded(copy)),
                    Err(failure) => { self.allocation_failure = Some(failure); Err(ContractError(String::new())) }
                }
            }
        }
    }
    fn insert_environment(&mut self, env: &mut AppEnvironment, key: &str, value: Type) -> Result<()> {
        let a = self.audit;
        match env {
            AppEnvironment::Legacy(map) => a.insert_string(map, key, value),
            AppEnvironment::Decoded(map) => {
                a.string(key.len())?;
                a.insert::<String, Type>(map.len(), a.comparator(key.len())?)?;
                let cx = self.allocation.as_deref_mut().expect("decoded app has allocation context");
                let owned_key = crate::product_alloc::try_copy_str(key, cx, crate::product_alloc::AllocationSite::String)
                    .and_then(|key| map.try_insert(key, value, cx).map(|_| ()))
                    .map_err(|failure| { self.allocation_failure = Some(failure); ContractError(String::new()) });
                owned_key
            }
        }
    }
    fn parameter_environment(&mut self, parameters: StringMapRef<'_, Type>) -> Result<AppEnvironment> {
        let a = self.audit;
        match parameters {
            StringMapRef::Legacy(map) => Ok(AppEnvironment::Legacy(a.clone_owned(map)?)),
            StringMapRef::Fallible(map) => {
                charge_type_map_clone(a, map)?;
                let cx = self.allocation.as_deref_mut().expect("decoded app has allocation context");
                match map.try_clone_with(cx, |value, cx| values_map::try_clone_type(value, cx)) {
                    Ok(copy) => Ok(AppEnvironment::Decoded(copy)),
                    Err(failure) => { self.allocation_failure = Some(failure); Err(ContractError(String::new())) }
                }
            }
        }
    }
    fn action_definition<A: ActionNode>(&mut self, action: &A) -> Result<()> {
        let a = self.audit;
        a.pay(12)?;
        a.nonempty(action.action_label())?;
        let steps = action.action_steps();
        a.require(!steps.is_empty(), "action needs steps")?;
        let parameters = action.action_parameters();
        a.map_iter(parameters.len())?;
        a.unique(parameters.iter().map(|(key, _)| key.as_str()))?;
        a.map_iter(parameters.len())?;
        for (_, typ) in parameters.iter() { a.pay(12)?; self.typ(typ, 0)?; }
        let mut env = self.parameter_environment(parameters)?;
        let guards = action.action_guards();
        let ensures = action.action_ensures();
        a.bounded(guards.len())?;
        a.bounded(ensures.len())?;
        a.slice(guards.len())?;
        for guard in guards { a.pay(12)?; self.expect(guard, &env, &Type::Boolean, 0, true)?; }
        self.statements(steps, &mut env, 0)?;
        a.slice(ensures.len())?;
        for condition in ensures { a.pay(12)?; self.expect(condition, &env, &Type::Boolean, 0, true)?; }
        Ok(())
    }
    fn node(&mut self, depth: usize) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| a.overflow())?;
        a.require(
            depth <= MAX_EXPR_DEPTH && self.nodes <= MAX_AST_NODES,
            "AST depth or node budget exceeded",
        )
    }
    fn entity(&self, id: &str) -> Result<&'a EntityDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.entities,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(16).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown entity: {id}")))
    }
    fn state(&self, id: &str) -> Result<&'a StateDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.state,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(15).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown state: {id}")))
    }
    fn action(&self, id: &str) -> Result<&'a ActionDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.actions,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(16).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown action: {id}")))
    }
    fn action_any(&self, id: &str) -> Result<ActionRef<'a>> {
        let a=self.audit;
        let found = match self.app {
            AppRef::Legacy(app) => a.find(&app.actions,id,|v|&v.id)?.map(ActionRef::Legacy),
            AppRef::Decoded(app) => a.find(&app.actions,id,|v|&v.id)?.map(ActionRef::Decoded),
            AppRef::Snapshot => a.find(self.actions,id,|v|&v.id)?.map(ActionRef::Legacy),
        };
        if let Some(value)=found { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(16).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown action: {id}")))
    }
    fn field(&self, entity: &str, field: &str) -> Result<&'a FieldDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(&self.entity(entity)?.fields,field,|v|&v.id)? {return Ok(value);}
        a.pay(3)?;a.format(entity.len().checked_add(field.len()).and_then(|n|n.checked_add(16)).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown field: {entity}.{field}")))
    }
    fn typ(&self, typ: &Type, depth: usize) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.require(depth <= MAX_TYPE_DEPTH, "type nesting budget exceeded")?;
        match typ {
            Type::Reference { entity } => {
                self.entity(entity)?;
            }
            Type::List { item } => self.typ(item, depth + 1)?,
            Type::Optional { item } => {
                a.require(
                    !matches!(item.as_ref(), Type::Optional { .. }),
                    "nested optional type",
                )?;
                self.typ(item, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn bound_for<M: StringValueMap<Type>>(&mut self, env: &M, name: &str, typ: Type) -> Result<AppEnvironment> {
        let a = self.audit;
        a.pay(8)?;
        a.check_id(name)?;
        let available = !a.has(env, name)?;
        a.pay(8)?;
        if a.enabled() && available {
            a.pay(1)?;
        } else {
        a.pay(2)?;a.format(name.len().checked_add(35).ok_or_else(|| a.overflow())?)?;
        a.require_owned(available, format!("binding shadows existing variable: {name}"))?;
        }
        let mut env = if self.is_decoded() {
            let mut copied = fallible_app_decode::FallibleMap::new();
            charge_type_map_clone(a, env)?;
            for (key, value) in env.iter() {
                let key = crate::product_alloc::try_copy_str(key, self.allocation.as_deref_mut().expect("decoded app has allocation context"), crate::product_alloc::AllocationSite::String)
                    .map_err(|failure| { self.allocation_failure = Some(failure); ContractError(String::new()) })?;
                let value = values_map::try_clone_type(value, self.allocation.as_deref_mut().expect("decoded app has allocation context"))
                    .map_err(|failure| { self.allocation_failure = Some(failure); ContractError(String::new()) })?;
                copied.try_insert(key,value,self.allocation.as_deref_mut().expect("decoded app has allocation context"))
                    .map_err(|failure| { self.allocation_failure = Some(failure); ContractError(String::new()) })?;
            }
            AppEnvironment::Decoded(copied)
        } else {
            a.pay(1)?;
            a.map_iter(env.len())?;
            for (key,value) in env.iter() { a.pay(4)?; key.pay_owned(a)?; value.pay_owned(a)?; }
            a.pay(crate::product_contract::bounded_input::capture_payment_core::CLONE_DESCRIPTOR)?;
            a.amount(crate::product_contract::bounded_input::capture_payment_core::btree_clone::<String,Type>(env.len(),0))?;
            AppEnvironment::Legacy(env.iter().map(|(k,v)|(k.clone(),v.clone())).collect())
        };
        self.insert_environment(&mut env, name, typ)?;
        Ok(env)
    }
    fn bound(&self, env: &Environment, name: &str, typ: Type) -> Result<Environment> {
        let a=self.audit;
        a.pay(8)?;
        a.check_id(name)?;
        let available=!a.has(env,name)?;
        a.pay(8)?;
        if a.enabled() && available { a.pay(1)?; } else {
            a.pay(2)?;a.format(name.len().checked_add(35).ok_or_else(||a.overflow())?)?;
            a.require_owned(available,format!("binding shadows existing variable: {name}"))?;
        }
        let mut env=a.clone_owned(env)?;
        a.insert_string(&mut env,name,typ)?;
        Ok(env)
    }
    fn expect<M: StringValueMap<Type>>(
        &mut self,
        expr: &Expr,
        env: &M,
        expected: &Type,
        depth: usize,
        state: bool,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        let actual = self.expr(expr, env, depth, state)?;
        let equal = a.type_equal(&actual, expected)?;
        a.pay(8)?;
        if a.enabled() && equal {
            a.pay(1)?;
            Ok(())
        } else {
        a.type_message(44, expected, &actual)?;
        a.require_owned(equal, format!("expression type mismatch: expected {expected:?}, found {actual:?}"))
        }
    }
    fn list_item<M: StringValueMap<Type>>(
        &mut self,
        expr: &Expr,
        env: &M,
        depth: usize,
        state: bool,
    ) -> Result<Type> {
        let a = self.audit;
        a.pay(8)?;
        match self.expr(expr, env, depth, state)? {
            Type::List { item } => Ok(*item),
            _ => Err(a.error("expected collection")?),
        }
    }
    fn record_entity<M: StringValueMap<Type>>(
        &mut self,
        expr: &Expr,
        env: &M,
        depth: usize,
        state: bool,
    ) -> Result<Id> {
        let a = self.audit;
        a.pay(8)?;
        match self.expr(expr, env, depth, state)? {
            Type::Reference { entity } => Ok(entity),
            _ => Err(a.error("expected nonoptional record reference")?),
        }
    }
    fn expr<M: StringValueMap<Type>>(&mut self, expr: &Expr, env: &M, depth: usize, state: bool) -> Result<Type> {
        let a = self.audit;
        a.pay(8)?;
        self.node(depth)?;
        let next = depth + 1;
        a.pay(12)?;
        Ok(match expr {
            Expr::Literal { value_type, value } => {
                self.typ(value_type, 0)?;
                validate_value_audited(value, value_type, 0, a)?;
                a.clone_owned(value_type)?
            }
            Expr::Variable { name } => {
                match a.get(env, name)? {
                    Some(value) => a.clone_owned(value)?,
                    None => {a.pay(2)?;a.format(name.len().checked_add(18).ok_or_else(||a.overflow())?)?;
                        return Err(ContractError(format!("unbound variable: {name}")));}
                }
            },
            Expr::Field { record, field } => {
                let entity = self.record_entity(record, env, next, state)?;
                a.clone_owned(&self.field(&entity, field)?.value_type)?
            }
            Expr::State { state: id } => {
                a.require(
                    state,
                    "durable constraints cannot reference transient state",
                )?;
                a.clone_owned(&self.state(id)?.value_type)?
            }
            Expr::Today => Type::Date,
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                ..
            } => {
                self.entity(entity)?;
                a.require(
                    *limit > 0 && *limit <= MAX_COLLECTION,
                    "query limit out of bounds",
                )?;
                a.bounded(sort.len())?;
                let env = self.bound_for(env, binding, a.reference_type(entity)?)?;
                self.expect(predicate, &env, &Type::Boolean, next, state)?;
                a.slice(sort.len())?;
        for key in sort {
            a.pay(8)?;
                    a.require(
                        self.expr(&key.value, &env, next, state)?.is_scalar(),
                        "sort key must be scalar",
                    )?;
                }
                a.list_type(a.reference_type(entity)?)?
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            }
            | Expr::Any {
                items,
                binding,
                predicate,
            }
            | Expr::All {
                items,
                binding,
                predicate,
            } => {
                let item = self.list_item(items, env, next, state)?;
                let env = self.bound_for(env, binding, a.clone_owned(&item)?)?;
                self.expect(predicate, &env, &Type::Boolean, next, state)?;
                if matches!(expr, Expr::Filter { .. }) {
                    a.list_type(item)?
                } else {
                    Type::Boolean
                }
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let item = self.list_item(items, env, next, state)?;
                let env = self.bound_for(env, binding, item)?;
                let result = a.list_type(self.expr(value, &env, next, state)?)?;
                self.typ(&result, 0)?;
                result
            }
            Expr::Count { items } => {
                self.list_item(items, env, next, state)?;
                Type::Integer
            }
            Expr::Sum { items } => {
                self.expect(items, env, &a.list_type(Type::Integer)?, next, state)?;
                Type::Integer
            }
            Expr::Contains { items, value } => {
                let item = self.list_item(items, env, next, state)?;
                self.expect(value, env, &item, next, state)?;
                Type::Boolean
            }
            Expr::Equal { left, right } | Expr::Less { left, right } => {
                self.comparison_operands(left, right, env, next, state, matches!(expr, Expr::Less { .. }))?
            }
            Expr::Add { left, right } | Expr::Subtract { left, right } => {
                self.expect(left, env, &Type::Integer, next, state)?;
                self.expect(right, env, &Type::Integer, next, state)?;
                Type::Integer
            }
            Expr::Concat { left, right } => {
                self.expect(left, env, &Type::Text, next, state)?;
                self.expect(right, env, &Type::Text, next, state)?;
                Type::Text
            }
            Expr::TextContains { text, search } => {
                self.expect(text, env, &Type::Text, next, state)?;
                self.expect(search, env, &Type::Text, next, state)?;
                Type::Boolean
            }
            Expr::Lower { value } => {
                self.expect(value, env, &Type::Text, next, state)?;
                Type::Text
            }
            Expr::And { values } | Expr::Or { values } => {
                a.bounded(values.len())?;
                a.require(!values.is_empty(), "empty boolean connective")?;
                a.slice(values.len())?;
        for value in values {
            a.pay(8)?;
                    self.expect(value, env, &Type::Boolean, next, state)?;
                }
                Type::Boolean
            }
            Expr::Not { value } => {
                self.expect(value, env, &Type::Boolean, next, state)?;
                Type::Boolean
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                self.expect(condition, env, &Type::Boolean, next, state)?;
                let typ = self.expr(then_value, env, next, state)?;
                self.expect(else_value, env, &typ, next, state)?;
                typ
            }
            Expr::Coalesce { value, fallback } => {
                let Type::Optional { item } = self.expr(value, env, next, state)? else {
                    return Err(a.error("coalesce requires optional value")?);
                };
                self.expect(fallback, env, &item, next, state)?;
                *item
            }
            Expr::DateAdd { date, days } => {
                self.expect(date, env, &Type::Date, next, state)?;
                self.expect(days, env, &Type::Integer, next, state)?;
                Type::Date
            }
            Expr::DateDifference { later, earlier } => {
                self.expect(later, env, &Type::Date, next, state)?;
                self.expect(earlier, env, &Type::Date, next, state)?;
                Type::Integer
            }
        })
    }
    fn comparison_operands<M: StringValueMap<Type>>(&mut self, left: &Expr, right: &Expr, env: &M,
        depth: usize, state: bool, ordered: bool) -> Result<Type> {
        let a=self.audit;
        a.pay(6)?;
        let typ=self.expr(left, env, depth, state)?;
        if ordered {a.require(typ.is_scalar(), "ordered comparison requires scalar")?;}
        self.expect(right, env, &typ, depth, state)?;
        Ok(Type::Boolean)
    }
    fn assignment<M: StringValueMap<Type>>(
        &mut self,
        value: &Expr,
        env: &M,
        expected: &Type,
        depth: usize,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        // Storage supplies the expected optional type to each conditional
        // branch. A sealed value may be Null while the live branch computes T.
        // This does not widen If expressions outside an optional assignment.
        if let (
            Type::Optional { .. },
            Expr::If {
                condition,
                then_value,
                else_value,
            },
        ) = (expected, value)
        {
            self.node(depth)?;
            self.expect(condition, env, &Type::Boolean, depth + 1, true)?;
            self.assignment(then_value, env, expected, depth + 1)?;
            return self.assignment(else_value, env, expected, depth + 1);
        }
        let actual = self.expr(value, env, depth, true)?;
        let compatible = a.type_equal(&actual, expected)? || match expected {
            Type::Optional { item } => a.type_equal(item, &actual)?, _ => false,
        };
        a.pay(8)?;
        if a.enabled() && compatible {
            a.pay(1)?;
            Ok(())
        } else {
        a.type_message(44, expected, &actual)?;
        a.require_owned(compatible, format!("assignment type mismatch: expected {expected:?}, found {actual:?}"))
        }
    }
    fn assignments(
        &mut self,
        entity: &str,
        values: &impl StringValueMap<Expr>,
        env: &impl StringValueMap<Type>,
        depth: usize,
        create: bool,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.bounded(values.len())?;
        a.map_iter(values.len())?;
        for (field, value) in values.iter() {
            a.pay(8)?;
            let typ = &self.field(entity, field)?.value_type;
            // Optional storage accepts a computed non-null inner value as well
            // as an explicitly optional expression, without splitting an atomic
            // multi-field Create or Update into separate mutations.
            self.assignment(value, env, typ, depth)?;
        }
        if create {
            let fields = &self.entity(entity)?.fields;
            a.slice(fields.len())?;
            for field in fields {
            a.pay(8)?;
                let present = a.has(values, &field.id)? || matches!(field.value_type, Type::Optional { .. });
        a.pay(8)?;
        if a.enabled() && present {
            a.pay(1)?;
        } else {
                a.pay(2)?;a.format(field.id.len().checked_add(24).ok_or_else(||a.overflow())?)?;
                a.require_owned(present, format!("missing required field: {}", field.id))?;
        }
            }
        }
        Ok(())
    }
    fn statements(&mut self, steps: StatementSlice<'_>, env: &mut AppEnvironment, depth: usize) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.bounded(steps.len())?;
        a.slice(steps.len())?;
        steps.visit(&mut |step| {
            a.pay(8)?;
            a.pay(12)?;
            self.node(depth)?;
            match step.statement_view() {
                StatementView::Create { entity, values, bind } => {
                    self.entity(entity)?;
                    self.assignments(entity, &values, env, depth + 1, true)?;
                    let typ = a.reference_type(entity)?;
                    *env = self.bound_for(env, bind, typ)?;
                }
                StatementView::Update { record, values } => {
                    let entity = self.record_entity(record, env, depth + 1, true)?;
                    a.require(!values.is_empty(), "empty update")?;
                    self.assignments(&entity, &values, env, depth + 1, false)?;
                }
                StatementView::Archive { record } => { self.record_entity(record, env, depth + 1, true)?; }
                StatementView::SetState { state, value } => {
                    self.expect(value, env, &self.state(state)?.value_type, depth + 1, true)?;
                }
                StatementView::Collection { target, items, .. } => {
                    let typ = match target {
                        CollectionTarget::State { state } => a.clone_owned(&self.state(state)?.value_type)?,
                        CollectionTarget::Field { record, field } => {
                            let entity = self.record_entity(record, env, depth + 1, true)?;
                            a.clone_owned(&self.field(&entity, field)?.value_type)?
                        }
                    };
                    a.require(matches!(typ, Type::List { .. }), "collection mutation requires list target")?;
                    self.expect(items, env, &typ, depth + 1, true)?;
                }
                StatementView::ForEach { items, binding, steps } => {
                    let item = self.list_item(items, env, depth + 1, true)?;
                    let mut local = self.bound_for(env, binding, item)?;
                    self.statements(steps, &mut local, depth + 1)?;
                }
                StatementView::Assert { condition, message } => {
                    a.nonempty(message)?;
                    self.expect(condition, env, &Type::Boolean, depth + 1, true)?;
                }
                StatementView::Emit { output, items, binding, columns } => {
                    let output = match a.find(self.outputs, output, |o| &o.id)? {
                        Some(value) => value, None => return Err(a.error("unknown output")?),
                    };
                    let item = self.list_item(items, env, depth + 1, true)?;
                    let local = self.bound_for(env, binding, item)?;
                    a.require(columns.len() == output.columns.len(), "output columns differ from definition")?;
                    a.slice(output.columns.len())?;
                    for column in &output.columns {
                        a.pay(8)?;
                        let value = match a.get(&columns, &column.id)? {
                            Some(value) => value, None => return Err(a.error("missing output column")?),
                        };
                        self.expect(value, &local, &column.value_type, depth + 1, true)?;
                    }
                }
            }
            Ok(())
        })
    }
    fn columns<C: ColumnNode>(&mut self, columns: &[C], env: &AppEnvironment) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.unique(columns.iter().map(|c| c.id()))?;
        a.slice(columns.len())?;
        for column in columns {
            a.pay(8)?;
            a.nonempty(column.label())?;
            self.expr(column.value(), env, 0, true)?;
        }
        Ok(())
    }
    fn on_change(&self, action: Option<&str>, typ: &Type) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        if let Some(action_id) = action {
            let action = self.action_any(action_id)?;
            let parameters = action.parameters();
            if let ActionRef::Legacy(legacy) = action {
                a.string(5)?;
                if a.enabled() { typ.pay_owned(a)?; }
                capture_canonical::reserve_from_iter::<String,Type>(1, a.comparator(5)?, a)?;
                let expected = BTreeMap::from([("value".into(), typ.clone())]);
                a.require(a.equal_owned(&legacy.parameters, &expected)?,
                    "on-change action must take exactly one value parameter of the control type")?;
            } else {
                a.map_iter(parameters.len())?;
                let matches = parameters.len() == 1 && match a.get(&parameters, "value")? {
                    Some(value) => a.type_equal(value, typ)?, None => false,
                };
                a.require(matches, "on-change action must take exactly one value parameter of the control type")?;
            }
        }
        Ok(())
    }
    fn view<V: ViewNode>(&mut self, view: &V) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.nonempty(view.label())?;
        a.pay(8)?;
        let mut env = self.empty_environment();
        match view.kind() {
            ViewKindView::List { entity, rows, columns, controls, selection } => {
                self.entity(entity)?;
                self.expect(rows, &env, &a.list_type(a.reference_type(entity)?)?, 0, true)?;
                let row_type = a.reference_type(entity)?;
                self.insert_environment(&mut env, "row", row_type)?;
                self.columns(columns, &env)?;
                a.unique(controls.iter().map(|c|c.id()).chain(selection.iter().map(|s|s.id())))?;
                a.slice(controls.len())?;
                for control in controls {
                    a.pay(8)?;
                    a.nonempty(control.label())?;
                    let typ = &self.state(control.state())?.value_type;
                    a.require(typ.is_scalar() || matches!(typ,Type::Optional{item} if item.is_scalar()),
                        "control must edit a scalar")?;
                    self.on_change(control.on_change(), typ)?;
                }
                if let Some(selection) = selection {
                    let typ = &self.state(selection.state())?.value_type;
                    a.require(a.type_equal(typ,&a.list_type(a.reference_type(entity)?)?)?,
                        "selection state has wrong entity/type")?;
                    self.on_change(selection.on_change(), typ)?;
                }
            }
            ViewKindView::Detail { entity, record, columns } => {
                self.entity(entity)?;
                self.expect(record,&env,&a.reference_type(entity)?,0,true)?;
                let row_type = a.reference_type(entity)?;
                self.insert_environment(&mut env,"row",row_type)?;
                self.columns(columns,&env)?;
            }
            ViewKindView::Form { action: action_id, fields, defaults } => {
                let action = self.action_any(action_id)?;
                let parameters = action.parameters();
                a.unique(fields.iter().map(|f|f.parameter()))?;
                a.slice(fields.len())?;
                for field in fields {
                    a.pay(8)?;
                    a.nonempty(field.label())?;
                    a.require(a.has(&parameters,field.parameter())?,"unknown form parameter")?;
                }
                a.map_iter(defaults.len())?;
                for (id,value) in defaults.iter() {
                    a.pay(8)?;
                    let typ = match a.get(&parameters,id)? {Some(value)=>value,None=>return Err(a.error("unknown default parameter")?)};
                    validate_value_audited(value,typ,0,a)?;
                }
                a.map_iter(parameters.len())?;
                for (id,_) in parameters.iter() {
                    a.pay(8)?;
                    a.require(a.has(defaults,id)? || a.find(fields,id,|f|f.parameter())?.is_some(),
                        "form omits required action parameter")?;
                }
            }
        }
        a.unique(view.actions().iter().map(|binding|binding.id()))?;
        a.slice(view.actions().len())?;
        for binding in view.actions() {
            a.pay(8)?;
            let local_env = if binding.placement()==ActionPlacement::Row {
                a.require(a.has(&env,"row")?,"row action requires a list or detail view")?;
                self.clone_environment(&env)?
            } else { a.pay(8)?; self.empty_environment() };
            a.nonempty(binding.label())?;
            let action = self.action_any(binding.action())?;
            let parameters = action.parameters();
            let arguments = binding.arguments();
            a.require(arguments.len()==parameters.len(),"action argument count mismatch")?;
            a.map_iter(parameters.len())?;
            for (name,typ) in parameters.iter() {
                a.pay(8)?;
                let value = match a.get(&arguments,name)? {Some(value)=>value,None=>return Err(a.error("missing action argument")?)};
                self.expect(value,&local_env,typ,0,true)?;
            }
            self.expect(binding.enabled(),&local_env,&Type::Boolean,0,true)?;
        }
        a.bounded(view.keys().len())?;
        a.pay(8)?;
        let mut seen = BTreeSet::new();
        a.slice(view.keys().len())?;
        for key in view.keys() {
            a.pay(8)?;
            a.scaled(key.key().len(),4,40)?;
            a.require(valid_key(key.key()),"unsupported keyboard key")?;
            let chord=(key.key(),key.mask());
            a.insert::<(&str,u8),()>(seen.len(),a.key_comparator_parts(key.key().len(),key.modifier_count())?)?;
            a.require(seen.insert(chord),"duplicate keyboard chord")?;
            a.require(a.find(view.actions(),key.binding(),|v|v.id())?.is_some(),"unknown keyboard action binding")?;
        }
        Ok(())
    }

}
pub fn valid_key(key: &str) -> bool {
    (key.len() == 1
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
        || matches!(
            key,
            "enter"
                | "escape"
                | "space"
                | "arrow_up"
                | "arrow_down"
                | "arrow_left"
                | "arrow_right"
                | "delete"
                | "backspace"
                | "tab"
        )
}
pub fn validate_value(value: &DataValue, typ: &Type, depth: usize) -> Result<()> {
    validate_value_audited(value, typ, depth, Audit::none())
}
fn validate_value_audited(value: &DataValue, typ: &Type, depth: usize, a: Audit<'_>) -> Result<()> {
    a.pay(8)?;
    a.require(depth <= MAX_TYPE_DEPTH, "value nesting budget exceeded")?;
    if let Type::Optional { item } = typ {
        if matches!(value, DataValue::Null) {
            return Ok(());
        }
        return validate_value_audited(value, item, depth + 1, a);
    }
    match (value, typ) {
        (DataValue::Boolean { .. }, Type::Boolean) | (DataValue::Integer { .. }, Type::Integer) => {
            Ok(())
        }
        (DataValue::Text { value }, Type::Text) => a.text(value),
        (DataValue::Date { days }, Type::Date) => a.day(*days),
        (DataValue::Reference { entity, record }, Type::Reference { entity: expected }) => {
            a.check_id(entity)?;
            a.check_id(record)?;
            a.require(a.eq_str(entity, expected)?, "reference entity type mismatch")
        }
        (DataValue::List { items, item_type }, Type::List { item }) => {
            a.require(
                items.len() <= MAX_COLLECTION && a.type_equal(item_type, item)?,
                "collection bound or element type mismatch",
            )?;
            a.slice(items.len())?;
            for value in items {
                validate_value_audited(value, item, depth + 1, a)?;
            }
            Ok(())
        }
        _ => Err(a.error("value does not match declared type")?),
    }
}
pub fn validate_day(day: i32) -> Result<()> {
    require(
        (-719_162..=2_932_896).contains(&day),
        "date outside years 0001 through 9999",
    )
}
impl AppDefinition {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let app: Self = parse(bytes)?;
        app.validate()?;
        Ok(app)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_audited(Audit::none()).map(|_| ())
    }
    fn validate_audited(&self, a: Audit<'_>) -> Result<capture_validation::CaptureFacts> {
        let mut app_validation_cx = crate::product_alloc::AllocationContext::new();
        validate_app_semantics!(self, a, validator, action_prefix_nodes, &mut app_validation_cx);
        // Keep the ordinary Vec alive through the original validation tail.
        let bytes;
        let canonical_len = if a.enabled() {
            capture_canonical::app_canonical_len_audited(self, a)?
        } else {
            bytes = capture_canonical::canonical_bytes_audited(self, a)?;
            bytes.len()
        };
        a.pay(3)?;
        a.require(canonical_len <= MAX_WIRE_BYTES, "program exceeds byte budget")?;
        a.pay(5)?;
        Ok(capture_validation::CaptureFacts { total_nodes: validator.nodes,
            action_prefix_nodes, canonical_len, action_count: self.actions.len() })
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Program, self)
    }
    pub fn semantic_identity(&self) -> Result<Digest> {
        self.validate()?;
        let mut app = self.clone();
        app.clear_display_labels_audited(Audit::none())?;
        canonical_digest(IdentityDomain::SemanticProgram, &app)
    }
}

pub(crate) fn decoded_expression_type_with(
    app: &fallible_app_decode::DecodedAppDefinition,
    expression: &Expr,
    binding: &str,
    entity: &str,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Type, AppValidationError> {
    let mut env = fallible_app_decode::FallibleMap::new();
    let key = crate::product_alloc::try_copy_str(binding, cx, crate::product_alloc::AllocationSite::String)
        .map_err(AppValidationError::Allocation)?;
    let entity = crate::product_alloc::try_copy_str(entity, cx, crate::product_alloc::AllocationSite::String)
        .map_err(AppValidationError::Allocation)?;
    let reference = Type::Reference { entity };
    let reference = values_map::try_clone_type(&reference, cx).map_err(AppValidationError::Allocation)?;
    env.try_insert(key, reference, cx).map_err(AppValidationError::Allocation)?;
    let mut validator = Validator::new_app(AppRef::Decoded(app), Audit::none(), cx);
    let result = validator.expr(expression, &env, 0, true);
    if let Some(failure) = validator.allocation_failure.take() {
        return Err(AppValidationError::Allocation(failure));
    }
    result.map_err(AppValidationError::Contract)
}

/// Type-check a decoded expression in the empty environment, preserving the
/// allocation accounting used by view observables and other host projections.
pub(crate) fn decoded_expression_type_empty_with(
    app: &fallible_app_decode::DecodedAppDefinition,
    expression: &Expr,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Type, AppValidationError> {
    let env = fallible_app_decode::FallibleMap::new();
    let mut validator = Validator::new_app(AppRef::Decoded(app), Audit::none(), cx);
    let result = validator.expr(expression, &env, 0, true);
    if let Some(failure) = validator.allocation_failure.take() {
        return Err(AppValidationError::Allocation(failure));
    }
    result.map_err(AppValidationError::Contract)
}

/// Type-check a decoded expression against an already-owned lexical map.
/// Callers which build nested statement scopes can share one charged ordered
/// environment instead of cloning/reconstructing a one-binding map per read.
pub(crate) fn decoded_expression_type_in_with(
    app: &fallible_app_decode::DecodedAppDefinition,
    expression: &Expr,
    bindings: &impl StringValueMap<Type>,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Type, AppValidationError> {
    let mut validator = Validator::new_app(AppRef::Decoded(app), Audit::none(), cx);
    let result = validator.expr(expression, bindings, 0, true);
    if let Some(failure) = validator.allocation_failure.take() {
        return Err(AppValidationError::Allocation(failure));
    }
    result.map_err(AppValidationError::Contract)
}

pub(crate) fn validate_decoded_app(
    app: &fallible_app_decode::DecodedAppDefinition,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), AppValidationError> {
    let audit = Audit::none();
    let semantic = (|| -> std::result::Result<(usize, usize), AppValidationError> {
        validate_app_semantics!(app, audit, validator, action_prefix_nodes, cx);
        let nodes = validator.nodes;
        drop(validator);
        Ok((nodes, action_prefix_nodes))
    })();
    if let Some(failure) = cx.first_failure() {
        return Err(AppValidationError::Allocation(failure));
    }
    semantic?;
    let bytes = crate::product_backup::fallible_codec::canonical_bytes(app, cx).map_err(|error| match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AppValidationError::Allocation(failure),
        _ => AppValidationError::Contract(ContractError("program canonical encoding failed".into())),
    })?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(AppValidationError::Contract(ContractError("program exceeds byte budget".into())));
    }
    Ok(())
}
impl AppDefinition {
    fn clear_display_labels_audited(&mut self,a:Audit<'_>)->Result<()> {
        a.pay(1)?;
        a.pay(1)?;
        self.label.clear();
        a.slice(self.entities.len())?;
        for entity in &mut self.entities {
            a.pay(1)?;
            entity.label.clear();
            a.slice(entity.fields.len())?;
        for field in &mut entity.fields {
                a.pay(1)?;
                field.label.clear();
            }
        }
        a.slice(self.state.len())?;
        for state in &mut self.state {
            a.pay(1)?;
            state.label.clear();
        }
        a.slice(self.actions.len())?;
        for action in &mut self.actions {
            a.pay(1)?;
            action.label.clear();
        }
        a.slice(self.outputs.len())?;
        for output in &mut self.outputs {
            a.pay(1)?;
            output.label.clear();
            a.slice(output.columns.len())?;
        for column in &mut output.columns {
                a.pay(1)?;
                column.label.clear();
            }
        }
        a.slice(self.observables.len())?;
        for observable in &mut self.observables {
            a.pay(1)?;
            observable.label.clear();
        }
        a.slice(self.views.len())?;
        for view in &mut self.views {
            a.pay(1)?;
            view.label.clear();
            a.slice(view.actions.len())?;
        for action in &mut view.actions {
                a.pay(1)?;
                action.label.clear();
            }
            match &mut view.kind {
                ViewKind::List {
                    columns, controls, ..
                } => {
                    a.slice(columns.len())?;
                    for column in columns {
                        a.pay(1)?;
                        column.label.clear();
                    }
                    a.slice(controls.len())?;
                    for control in controls {
                        a.pay(1)?;
                        control.label.clear();
                    }
                }
                ViewKind::Detail { columns, .. } => {
                    a.slice(columns.len())?;
                    for column in columns {
                        a.pay(1)?;
                        column.label.clear();
                    }
                }
                ViewKind::Form { fields, .. } => {
                    a.slice(fields.len())?;
                    for field in fields {
                        a.pay(1)?;
                        field.label.clear();
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_entities(entities: &[EntityDefinition], validator: &mut Validator<'_, '_, '_>) -> Result<()> {
    let a = validator.audit;
    a.pay(1)?;
    a.unique(entities.iter().map(|e| e.id.as_str()))?;
    a.slice(entities.len())?;
    for entity in entities {
            a.pay(12)?;
        a.nonempty(&entity.label)?;
        a.require(!entity.fields.is_empty(), "entity needs fields")?;
        a.unique(entity.fields.iter().map(|f| f.id.as_str()))?;
        a.slice(entity.fields.len())?;
    for field in &entity.fields {
            a.pay(12)?;
            a.nonempty(&field.label)?;
            validator.typ(&field.value_type, 0)?;
        }
        a.bounded(entity.unique.len())?;
        a.pay(8)?;
        let mut tuples = BTreeSet::new();
        a.slice(entity.unique.len())?;
    for tuple in &entity.unique {
            a.pay(12)?;
            a.require(!tuple.is_empty(), "empty unique constraint")?;
            a.unique(tuple.iter().map(String::as_str))?;
            a.insert::<&Vec<Id>, ()>(tuples.len(), a.tuple_comparator(tuple)?)?;
            a.require(tuples.insert(tuple), "duplicate unique constraint")?;
            a.slice(tuple.len())?;
    for field in tuple {
            a.pay(12)?;
                let typ = &validator.field(&entity.id, field)?.value_type;
                a.require(
                    typ.is_scalar() || matches!(typ,Type::Optional { item } if item.is_scalar()),
                    "unique constraint needs scalar fields",
                )?;
            }
        }
        a.bounded(entity.constraints.len())?;
        a.string(6)?;
        a.string(entity.id.len())?;
        capture_canonical::reserve_from_iter::<String,Type>(1,a.comparator(6)?,a)?;
        let env=BTreeMap::from([("record".into(),Type::reference(&entity.id))]);
        a.slice(entity.constraints.len())?;
    for constraint in &entity.constraints {
            a.pay(12)?;
            validator.expect(constraint, &env, &Type::Boolean, 0, false)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    GeneratedApp,
    LegacyOrder,
    ExternalWeb,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub kind: ArtifactKind,
    pub version: u32,
    pub language: String,
    /// All original bytes, including whitespace, labels and source metadata.
    pub raw_digest: Digest,
    /// Canonical complete typed executable/view source; labels are included.
    pub program_digest: Digest,
    /// Only declared display labels are excluded. Not proof of equivalent behavior.
    pub semantic_digest: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Producer {
    LiveAgent {
        provider: String,
        invocation_id: String,
        session_id: Option<String>,
        request_digest: Digest,
    },
    ExternalAuthor {
        description: String,
    },
    UserAuthored,
    Fixture {
        name: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSource {
    pub task_id: Id,
    pub source_fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub project_id: Id,
    pub program_path: String,
    pub task: Option<TaskSource>,
    pub producer: Producer,
    pub source_digest: Digest,
}
impl SourceBinding {
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none())}
    fn validate_audited(&self,a:Audit<'_>)->Result<()> {
        a.pay(6)?;
        a.check_id(&self.project_id)?;
        validate_relative_path_audited(&self.program_path,a)?;
        if let Some(task) = &self.task {
            a.check_id(&task.task_id)?;
            a.nonempty(&task.source_fingerprint)?;
        }
        self.producer.validate_audited(a)?;
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Source, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedProgram {
    pub artifact: ArtifactRef,
    pub binding: SourceBinding,
    pub source_bytes: Vec<u8>,
    pub program: AppDefinition,
}
/// An exact, owned immutable capture plus pure static observation metadata.
/// Only successful full capture validation can mint this capability. It is
/// deliberately neither serializable nor mutable, and never stores run state.
#[derive(Clone)]
pub(crate) struct PreparedProgram(Arc<PreparedProgramInner>);
struct PreparedProgramInner {
    capture: CapturedProgram,
    observable_types: BTreeMap<Id, Type>,
    view_schemas: BTreeMap<Id, Option<ViewSchema>>,
}
impl PreparedProgram {
    pub(crate) fn new(capture: &CapturedProgram) -> Result<Self> {
        capture.validate()?;
        let observable_types = capture.program.observable_types_validated()?;
        let view_schemas = capture
            .program
            .views
            .iter()
            .map(|view| {
                Ok((
                    view.id.clone(),
                    capture.program.view_schema_validated(&view.id)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self(Arc::new(PreparedProgramInner {
            capture: capture.clone(),
            observable_types,
            view_schemas,
        })))
    }
    pub(crate) fn capture(&self) -> &CapturedProgram {
        &self.0.capture
    }
    pub(crate) fn initial_session(&self) -> SessionState {
        SessionState::initial_validated(&self.0.capture.program)
    }
    pub(crate) fn validate_session(&self, session: &SessionState) -> Result<()> {
        session.validate_for_app(&self.0.capture.program)
    }
    pub(crate) fn validate_scenario(&self, scenario: &ScenarioSpec) -> Result<()> {
        scenario.validate_prefix()?;
        self.validate_session(&scenario.session)?;
        scenario.validate_suffix(&self.0.capture.program)
    }
    pub(crate) fn observable_types(&self) -> &BTreeMap<Id, Type> {
        &self.0.observable_types
    }
    pub(crate) fn view_schema(&self, view: &str) -> Result<&Option<ViewSchema>> {
        self.0
            .view_schemas
            .get(view)
            .ok_or_else(|| ContractError("unknown view".into()))
    }
}
impl CapturedProgram {
    pub fn capture(
        bytes: &[u8],
        project: &str,
        producer: Producer,
        task: Option<TaskSource>,
    ) -> Result<Self> {
        let program = AppDefinition::parse(bytes)?;
        let raw_digest = bytes_digest(IdentityDomain::Source, bytes);
        let artifact = ArtifactRef {
            kind: ArtifactKind::GeneratedApp,
            version: CONTRACT_VERSION,
            language: LANGUAGE_VERSION.into(),
            raw_digest: raw_digest.clone(),
            program_digest: program.identity()?,
            semantic_digest: program.semantic_identity()?,
        };
        let binding = SourceBinding {
            project_id: project.into(),
            program_path: "program.json".into(),
            task,
            producer,
            source_digest: raw_digest,
        };
        binding.validate()?;
        Ok(Self {
            artifact,
            binding,
            source_bytes: bytes.to_vec(),
            program,
        })
    }
    pub fn at_path(mut self, path: &str) -> Result<Self> {
        validate_relative_path(path)?;
        self.binding.program_path = path.into();
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none()).map(|_|())}
    fn validate_audited(&self,a:Audit<'_>)->Result<capture_validation::CaptureFacts> {
        a.pay(8)?;
        #[cfg(test)]
        count_validation(|counts| counts.captures += 1);
        self.binding.validate_audited(a)?;
        a.version(self.artifact.version)?;
        a.require(self.artifact.kind == ArtifactKind::GeneratedApp
            && a.eq_str(&self.artifact.language, LANGUAGE_VERSION)?, "unsupported executable artifact")?;
        a.require(self.source_bytes.len() <= MAX_WIRE_BYTES, "source exceeds byte limit")?;
        let actual: AppDefinition = if a.enabled() {
            // Both roots precede the new helper, closures, and borrowed owners.
            a.pay(capture_canonical::COMMON)?;
            a.pay(capture_canonical::SOURCE_DELTA_ROOT)?;
            return capture_canonical::validate_guarded_native_capture(self, a);
        } else {parse(&self.source_bytes)?};
        let facts=actual.validate_audited(a)?;
        a.require(a.equal_owned(&actual,&self.program)?, "source bytes do not represent captured program")?;
        let raw=capture_validation::bytes_digest_audited(IdentityDomain::Source,&self.source_bytes,a)?;
        a.require(a.eq_str(self.artifact.raw_digest.as_str(),raw.as_str())?
            && a.eq_str(self.binding.source_digest.as_str(),self.artifact.raw_digest.as_str())?, "raw source digest mismatch")?;
        a.pay(8 + 2 * std::mem::size_of::<Audit<'_>>() as u64 + 4 * std::mem::size_of::<bool>() as u64)?;
        if !a.enabled() { actual.validate_audited(a)?; }
        let bytes=capture_canonical::canonical_bytes_audited(&actual,a)?;
        let program=capture_validation::bytes_digest_audited(IdentityDomain::Program,&bytes,a)?;
        let equal=a.eq_str(self.artifact.program_digest.as_str(),program.as_str())?;
        let equal=if equal {
            a.pay(8 + 2 * std::mem::size_of::<Audit<'_>>() as u64 + 4 * std::mem::size_of::<bool>() as u64)?;
            if !a.enabled() { actual.validate_audited(a)?; }
            let mut semantic=a.clone_owned(&actual)?;
            semantic.clear_display_labels_audited(a)?;
            let bytes=capture_canonical::canonical_bytes_audited(&semantic,a)?;
            let digest=capture_validation::bytes_digest_audited(IdentityDomain::SemanticProgram,&bytes,a)?;
            a.eq_str(self.artifact.semantic_digest.as_str(),digest.as_str())?
        } else {false};
        a.require(equal,"program digest mismatch")?;
        Ok(facts)
    }
}
pub fn validate_relative_path(path: &str) -> Result<()> {validate_relative_path_audited(path,Audit::none())}
fn validate_relative_path_audited(path: &str,a:Audit<'_>)->Result<()> {
    if !a.enabled() {
        return require(!path.is_empty() && path.len() <= 1024
            && !path.contains(['\\', ':', '\0']) && !path.chars().any(char::is_control)
            && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
            "unsafe relative source path");
    }
    a.pay(8)?;
    let valid=if path.is_empty() || path.len()>1024 {false} else {
        // Native three-character search: UTF-8 decode + three candidate tests.
        a.scaled(path.len(),7,4)?;
        if path.contains(['\\', ':', '\0']) {false} else {
            // Native control predicate: decode + finite C0/C1 range checks.
            a.scaled(path.len(),6,4)?;
            if path.chars().any(char::is_control) {false} else {
                a.scaled(path.len(),1,2)?; // Original slash search bytes.
                let mut valid=true;
                for part in path.split('/') {
                    a.pay(6)?;
                    if part.is_empty() || a.eq_str(part,".")? || a.eq_str(part,"..")? {valid=false;break;}
                }
                a.pay(1)?;valid
            }
        }
    };
    a.require(valid,"unsafe relative source path")
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocus {
    pub relative_path: String,
    pub raw_digest: Digest,
    pub pointer: String,
}
impl SourceLocus {
    pub fn validate_against(&self, source: &CapturedProgram) -> Result<()> {
        source.validate()?;
        validate_relative_path(&self.relative_path)?;
        text(&self.pointer)?;
        require(
            self.relative_path == source.binding.program_path
                && self.raw_digest == source.artifact.raw_digest,
            "source locus refers to another artifact",
        )?;
        require(
            self.pointer.is_empty() || self.pointer.starts_with('/'),
            "invalid JSON pointer",
        )?;
        let bytes = self.pointer.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'~' {
                require(
                    matches!(bytes.get(i + 1), Some(b'0' | b'1')),
                    "invalid JSON pointer escape",
                )?;
            }
        }
        let value = bounded_input::parse_json_bytes(&source.source_bytes)
            .map_err(|e| ContractError(e.to_string()))?;
        require(
            value.pointer(&self.pointer).is_some(),
            "source locus does not exist",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub entity: Id,
    pub id: Id,
    pub revision: u64,
    pub created_program: Digest,
    pub archived: bool,
    pub values: Values,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordChange {
    pub entity: Id,
    pub record: Id,
    pub before: Option<Values>,
    pub after: Values,
    pub archived: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessEvent {
    pub id: Id,
    pub sequence: u64,
    pub operation_id: Id,
    pub action: Id,
    pub day: i32,
    pub program: Digest,
    pub changes: Vec<RecordChange>,
    pub outputs: Vec<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSnapshot {
    pub version: u32,
    pub project_id: Id,
    pub generation: u64,
    /// Cumulative storage schema. Retained fields/entities stay inspectable even
    /// when an older active program does not expose them. Never drop on adoption.
    pub schema: Vec<EntityDefinition>,
    pub records: Vec<Record>,
    pub events: Vec<BusinessEvent>,
}
impl DataSnapshot {
    pub fn empty(project: &str, app: &AppDefinition) -> Result<Self> {
        app.validate()?;
        check_id(project)?;
        Ok(Self {
            version: CONTRACT_VERSION,
            project_id: project.into(),
            generation: 0,
            schema: app.entities.clone(),
            records: vec![],
            events: vec![],
        })
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let data: Self = parse(bytes)?;
        data.validate()?;
        Ok(data)
    }
    pub fn validate(&self) -> Result<()> { self.validate_audited(Audit::none()) }
    fn validate_audited(&self, a: Audit<'_>) -> Result<()> {
        fallible_snapshot_validate::validate(self, a)
    }

    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Data, self)
    }
    pub fn schema_identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Schema, &self.schema)
    }
}

/// Borrowed read-only view over either persisted snapshot owner. Runtime
/// proof evaluation uses this to share the same evaluator without converting
/// fallible `Values` back into BTreeMap-backed legacy records.
#[derive(Clone, Copy)]
pub(crate) enum DataSnapshotView<'a> {
    Legacy(&'a DataSnapshot),
    Fallible(&'a FallibleDecodedDataSnapshot),
    FallibleHistorical { data: &'a FallibleDecodedDataSnapshot, override_record: DataRecordView<'a>, values: &'a FallibleValues, archived: bool },
}

#[derive(Clone, Copy)]
pub(crate) struct DataRecordView<'a> {
    inner: DataRecordKind<'a>,
}
#[derive(Clone, Copy)]
enum DataRecordKind<'a> {
    Legacy(&'a Record),
    Fallible(&'a fallible_persisted_decode::DecodedRecord),
    FallibleOverride(&'a fallible_persisted_decode::DecodedRecord, &'a FallibleValues, bool),
}

#[derive(Clone, Copy)]
pub(crate) enum DataValuesView<'a> {
    Legacy(&'a Values),
    Fallible(&'a FallibleValues),
}

pub(crate) struct DataRecordIter<'a>(DataRecordIterKind<'a>);
enum DataRecordIterKind<'a> {
    Legacy(std::slice::Iter<'a, Record>),
    Fallible(std::slice::Iter<'a, fallible_persisted_decode::DecodedRecord>),
    FallibleHistorical { iter: std::slice::Iter<'a, fallible_persisted_decode::DecodedRecord>, override_record: DataRecordView<'a>, values: &'a FallibleValues, archived: bool },
}

impl<'a> Iterator for DataRecordIter<'a> {
    type Item = DataRecordView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            DataRecordIterKind::Legacy(iter) => iter.next().map(|row| DataRecordView { inner: DataRecordKind::Legacy(row) }),
            DataRecordIterKind::Fallible(iter) => iter.next().map(|row| DataRecordView { inner: DataRecordKind::Fallible(row) }),
            DataRecordIterKind::FallibleHistorical { iter, override_record, values, archived } => iter.next().map(|row| {
                // The historical overlay has already resolved its target record.
                // Compare owners by address here so each evaluator row step does
                // constant work instead of rescanning long entity/record IDs.
                let view = DataRecordView { inner: DataRecordKind::Fallible(row) };
                if view.same_fallible_record(override_record) { DataRecordView { inner: DataRecordKind::FallibleOverride(row, values, *archived) } }
                else { view }
            }),
        }
    }
}

pub(crate) enum DataValueIter<'a> {
    Legacy(std::collections::btree_map::Iter<'a, String, DataValue>),
    Fallible(values_map::FallibleValuesIter<'a>),
}
impl<'a> Iterator for DataValueIter<'a> {
    type Item = (&'a String, &'a DataValue);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Legacy(iter) => iter.next(),
            Self::Fallible(iter) => iter.next(),
        }
    }
}
impl ExactSizeIterator for DataValueIter<'_> {}

impl<'a> DataSnapshotView<'a> {
    pub(crate) fn legacy(&self) -> Option<&'a DataSnapshot> { match self { Self::Legacy(data) => Some(data), _ => None } }
    pub(crate) fn fallible_historical(data: &'a FallibleDecodedDataSnapshot, override_record: DataRecordView<'a>, values: &'a FallibleValues, archived: bool) -> Self {
        Self::FallibleHistorical { data, override_record, values, archived }
    }
    pub(crate) fn project_id(&self) -> &str {
        match self { Self::Legacy(data) => &data.project_id, Self::Fallible(data) | Self::FallibleHistorical { data, .. } => data.project_id() }
    }
    pub(crate) fn generation(&self) -> u64 {
        match self { Self::Legacy(data) => data.generation, Self::Fallible(data) | Self::FallibleHistorical { data, .. } => data.generation() }
    }
    pub(crate) fn record_count(&self) -> usize {
        match self { Self::Legacy(data) => data.records.len(), Self::Fallible(data) | Self::FallibleHistorical { data, .. } => data.records().len() }
    }
    pub(crate) fn schema(&self) -> &[EntityDefinition] {
        match self { Self::Legacy(data) => &data.schema, Self::Fallible(data) | Self::FallibleHistorical { data, .. } => data.schema() }
    }
    pub(crate) fn records(&self) -> DataRecordIter<'a> {
        match self {
            Self::Legacy(data) => DataRecordIter(DataRecordIterKind::Legacy(data.records.iter())),
            Self::Fallible(data) => DataRecordIter(DataRecordIterKind::Fallible(data.0.records.iter())),
            Self::FallibleHistorical { data, override_record, values, archived } => DataRecordIter(DataRecordIterKind::FallibleHistorical { iter: data.0.records.iter(), override_record: *override_record, values, archived: *archived }),
        }
    }
    pub(crate) fn record(&self, entity: &str, id: &str) -> Option<DataRecordView<'a>> {
        self.records().find(|record| record.entity() == entity && record.id() == id)
    }
    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), AppValidationError> {
        match self {
            Self::Legacy(data) => data.validate().map_err(AppValidationError::Contract),
            Self::Fallible(data) | Self::FallibleHistorical { data, .. } => data.validate_with(cx),
        }
    }
}
impl<'a> From<&'a DataSnapshot> for DataSnapshotView<'a> {
    fn from(value: &'a DataSnapshot) -> Self { Self::Legacy(value) }
}
impl<'a> From<&'a FallibleDecodedDataSnapshot> for DataSnapshotView<'a> {
    fn from(value: &'a FallibleDecodedDataSnapshot) -> Self { Self::Fallible(value) }
}
impl<'a> DataRecordView<'a> {
    pub(crate) fn entity(&self) -> &str {
        match self.inner { DataRecordKind::Legacy(row) => &row.entity, DataRecordKind::Fallible(row) | DataRecordKind::FallibleOverride(row, _, _) => &row.entity }
    }
    pub(crate) fn id(&self) -> &str {
        match self.inner { DataRecordKind::Legacy(row) => &row.id, DataRecordKind::Fallible(row) | DataRecordKind::FallibleOverride(row, _, _) => &row.id }
    }
    pub(crate) fn archived(&self) -> bool {
        match self.inner { DataRecordKind::Legacy(row) => row.archived, DataRecordKind::Fallible(row) => row.archived, DataRecordKind::FallibleOverride(_, _, archived) => archived }
    }
    pub(crate) fn revision(&self) -> u64 {
        match self.inner { DataRecordKind::Legacy(row) => row.revision, DataRecordKind::Fallible(row) | DataRecordKind::FallibleOverride(row, _, _) => row.revision }
    }
    pub(crate) fn created_program(&self) -> &'a Digest {
        match self.inner { DataRecordKind::Legacy(row) => &row.created_program, DataRecordKind::Fallible(row) | DataRecordKind::FallibleOverride(row, _, _) => &row.created_program }
    }
    pub(crate) fn values(&self) -> DataValuesView<'_> {
        match self.inner { DataRecordKind::Legacy(row) => DataValuesView::Legacy(&row.values), DataRecordKind::Fallible(row) => DataValuesView::Fallible(&row.values), DataRecordKind::FallibleOverride(_, values, _) => DataValuesView::Fallible(values) }
    }
    fn same_fallible_record(&self, other: &Self) -> bool {
        match self.inner {
            DataRecordKind::Fallible(row) | DataRecordKind::FallibleOverride(row, _, _) => match other.inner {
                DataRecordKind::Fallible(other) | DataRecordKind::FallibleOverride(other, _, _) => std::ptr::eq(row, other),
                DataRecordKind::Legacy(_) => false,
            },
            DataRecordKind::Legacy(_) => false,
        }
    }
}
impl<'a> DataValuesView<'a> {
    pub(crate) fn len(&self) -> usize {
        match self { Self::Legacy(values) => values.len(), Self::Fallible(values) => values.len() }
    }
    pub(crate) fn get(&self, key: &str) -> Option<&'a DataValue> {
        match self { Self::Legacy(values) => values.get(key), Self::Fallible(values) => values.get(key) }
    }
    pub(crate) fn iter(&self) -> DataValueIter<'a> {
        match self { Self::Legacy(values) => DataValueIter::Legacy(values.iter()), Self::Fallible(values) => DataValueIter::Fallible(values.iter()) }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SessionStateView<'a> {
    Legacy(&'a SessionState),
    Fallible(&'a FallibleDecodedSessionState),
}
impl<'a> SessionStateView<'a> {
    pub(crate) fn legacy(&self) -> Option<&'a SessionState> { match self { Self::Legacy(state) => Some(state), _ => None } }
    pub(crate) fn values(&self) -> DataValuesView<'a> {
        match self {
            Self::Legacy(session) => DataValuesView::Legacy(&session.values),
            Self::Fallible(session) => DataValuesView::Fallible(&session.0.values),
        }
    }
}
impl<'a> From<&'a SessionState> for SessionStateView<'a> {
    fn from(value: &'a SessionState) -> Self { Self::Legacy(value) }
}
impl<'a> From<&'a FallibleDecodedSessionState> for SessionStateView<'a> {
    fn from(value: &'a FallibleDecodedSessionState) -> Self { Self::Fallible(value) }
}
trait RecordLookup {
    fn len(&self) -> usize;
    fn contains_pair(&self, entity: &str, record: &str) -> bool;
}
impl RecordLookup for BTreeSet<(&str, &str)> {
    fn len(&self) -> usize { BTreeSet::len(self) }
    fn contains_pair(&self, entity: &str, record: &str) -> bool { self.contains(&(entity, record)) }
}
impl RecordLookup for crate::product_alloc::ordered::OrderedSet<(&str, &str)> {
    fn len(&self) -> usize { crate::product_alloc::ordered::OrderedSet::len(self) }
    fn contains_pair(&self, entity: &str, record: &str) -> bool { self.contains(&(entity, record)) }
}
fn validate_references(value: &DataValue, records: &BTreeSet<(&str, &str)>) -> Result<()> {
    validate_references_audited(value, records, Audit::none())
}
fn validate_references_audited<R: RecordLookup>(value: &DataValue, records: &R, a: Audit<'_>) -> Result<()> {
    a.pay(4)?;
    match value {
        DataValue::Reference { entity, record } => {
            if a.enabled() {
                a.pay(bounded_input::capture_payment_core::SEARCH_DESCRIPTOR)?;
                a.amount(bounded_input::capture_payment_core::btree_search(records.len(), a.pair_comparator(entity, record)?))?;
            }
            a.require(records.contains_pair(entity, record), "dangling record reference")
        },
        DataValue::List { items, .. } => {
            a.slice(items.len())?;
            for item in items {
                validate_references_audited(item, records, a)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub view: Id,
    pub values: Values,
    pub focused_record: Option<RecordRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRef {
    pub entity: Id,
    pub record: Id,
}
impl SessionState {
    pub fn initial(app: &AppDefinition) -> Result<Self> {
        app.validate()?;
        Ok(Self::initial_validated(app))
    }
    fn initial_validated(app: &AppDefinition) -> Self {
        Self {
            view: app.initial_view.clone(),
            values: app
                .state
                .iter()
                .map(|s| (s.id.clone(), s.initial.clone()))
                .collect(),
            focused_record: None,
        }
    }
    pub fn validate(&self, app: &AppDefinition) -> Result<()> {
        app.validate()?;
        self.validate_for_app(app)
    }
    fn validate_for_app(&self, app: &AppDefinition) -> Result<()> {
        require(
            app.views.iter().any(|v| v.id == self.view),
            "unknown session view",
        )?;
        require(
            self.values.len() == app.state.len(),
            "session state differs from program",
        )?;
        for state in &app.state {
            validate_value(
                self.values
                    .get(&state.id)
                    .ok_or_else(|| ContractError("missing session state".into()))?,
                &state.value_type,
                0,
            )?;
        }
        if let Some(record) = &self.focused_record {
            check_id(&record.record)?;
            require(
                app.entities.iter().any(|e| e.id == record.entity),
                "focused record entity is unknown",
            )?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        canonical_digest(IdentityDomain::Session, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticInput {
    Invoke {
        action: Id,
        arguments: Values,
    },
    Control {
        view: Id,
        control: Id,
        value: DataValue,
    },
    Activate {
        view: Id,
        binding: Id,
        row: Option<RecordRef>,
    },
    Submit {
        view: Id,
        arguments: Values,
    },
    Navigate {
        view: Id,
    },
    AdvanceClock {
        days: u32,
    },
    Observe {
        point: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub version: u32,
    pub id: Id,
    pub label: String,
    pub seed: DataSnapshot,
    pub session: SessionState,
    pub clock_day: i32,
    pub random_seed: u64,
    pub inputs: Vec<SemanticInput>,
    /// Independently chosen workflow-validity obligations. Failure invalidates a
    /// trial; it must not be mistaken for a discriminating behavioral difference.
    pub validity: Vec<AcceptedProperty>,
}
impl ScenarioSpec {
    pub fn identity(&self) -> Result<Digest> {
        canonical_digest(IdentityDomain::Scenario, self)
    }
    pub fn input_identity(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Input,
            &(
                &self.seed,
                &self.session,
                self.clock_day,
                self.random_seed,
                &self.inputs,
            ),
        )
    }
    /// Stable synthetic execution namespace. Full source, schema and inputs stay
    /// in run bindings; instrumentation and implementation names cannot alter
    /// IDs of earlier created records. Existing seed records retain exact IDs.
    pub fn operation_namespace(&self) -> Result<Digest> {
        self.seed.validate()?;
        validate_day(self.clock_day)?;
        let mut records: Vec<_> = self.seed.records.iter().collect();
        records.sort_by(|a, b| (&a.entity, &a.id).cmp(&(&b.entity, &b.id)));
        canonical_digest(
            IdentityDomain::Input,
            &(
                "semantic-replay/2",
                &self.seed.project_id,
                self.seed.generation,
                records,
                &self.seed.events,
                self.clock_day,
                self.random_seed,
            ),
        )
    }
    /// Shared replay-only driver. Adding/reordering action-bearing inputs changes
    /// mutation ordinals and needs explicit semantic correspondence; it is not
    /// silently treated as equivalent. This does not allocate live store IDs.
    pub fn replay_operation_ids(&self) -> Result<Vec<Id>> {
        bounded(self.inputs.len())?;
        let namespace = self.operation_namespace()?;
        let mut mutation = 0usize;
        self.inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                validate_input_shape(input)?;
                let id = if matches!(
                    input,
                    SemanticInput::Invoke { .. }
                        | SemanticInput::Control { .. }
                        | SemanticInput::Activate { .. }
                        | SemanticInput::Submit { .. }
                ) {
                    let id = format!("replay-mutation-{}-{mutation}", namespace.as_str());
                    mutation += 1;
                    id
                } else {
                    format!("replay-step-{}-{index}", namespace.as_str())
                };
                check_id(&id)?;
                Ok(id)
            })
            .collect()
    }
    pub fn validate(&self, app: &AppDefinition) -> Result<()> {
        self.validate_prefix()?;
        self.session.validate(app)?;
        self.validate_suffix(app)
    }
    fn validate_prefix(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        nonempty(&self.label)?;
        self.seed.validate()
    }
    fn validate_suffix(&self, app: &AppDefinition) -> Result<()> {
        validate_day(self.clock_day)?;
        bounded(self.inputs.len())?;
        bounded(self.validity.len())?;
        let mut points = BTreeSet::new();
        for input in &self.inputs {
            validate_input(input, app)?;
            if let SemanticInput::Observe { point } = input {
                require(points.insert(point), "duplicate observation point")?;
            }
        }
        for property in &self.validity {
            property.validate()?;
        }
        Ok(())
    }
}
pub fn validate_input(input: &SemanticInput, app: &AppDefinition) -> Result<()> {
    let validator = Validator::new(app, Audit::none());
    let view = |id: &str| {
        app.views
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| ContractError("unknown input view".into()))
    };
    let arguments = |action: &ActionDefinition, values: &Values| -> Result<()> {
        require(
            values.len() == action.parameters.len(),
            "input argument count mismatch",
        )?;
        for (id, typ) in &action.parameters {
            validate_value(
                values
                    .get(id)
                    .ok_or_else(|| ContractError("missing input argument".into()))?,
                typ,
                0,
            )?;
        }
        Ok(())
    };
    match input {
        SemanticInput::Invoke {
            action,
            arguments: values,
        } => arguments(validator.action(action)?, values),
        SemanticInput::Submit {
            view: id,
            arguments: values,
        } => {
            let ViewKind::Form { action, .. } = &view(id)?.kind else {
                return Err(ContractError("submit requires form view".into()));
            };
            arguments(validator.action(action)?, values)
        }
        SemanticInput::Control {
            view: id,
            control,
            value,
        } => {
            let ViewKind::List {
                controls,
                selection,
                ..
            } = &view(id)?.kind
            else {
                return Err(ContractError("control requires list view".into()));
            };
            let state = controls
                .iter()
                .find(|c| &c.id == control)
                .map(|c| &c.state)
                .or_else(|| {
                    selection
                        .as_ref()
                        .filter(|s| &s.id == control)
                        .map(|s| &s.state)
                })
                .ok_or_else(|| ContractError("unknown input control".into()))?;
            validate_value(value, &validator.state(state)?.value_type, 0)
        }
        SemanticInput::Activate {
            view: id,
            binding,
            row,
        } => {
            let view = view(id)?;
            let action = view
                .actions
                .iter()
                .find(|a| &a.id == binding)
                .ok_or_else(|| ContractError("unknown input action binding".into()))?;
            require(
                (action.placement == ActionPlacement::Row) == row.is_some(),
                "action row context does not match placement",
            )?;
            if let Some(row) = row {
                check_id(&row.record)?;
                let entity = match &view.kind {
                    ViewKind::List { entity, .. } | ViewKind::Detail { entity, .. } => entity,
                    _ => return Err(ContractError("form action cannot bind row".into())),
                };
                require(&row.entity == entity, "input row belongs to another entity")?;
            }
            Ok(())
        }
        SemanticInput::Navigate { view: id } => {
            view(id)?;
            Ok(())
        }
        SemanticInput::AdvanceClock { days } => {
            require(*days <= 3_652_058, "clock advance out of bounds")
        }
        SemanticInput::Observe { point } => check_id(point),
    }
}

/// Terms name stable business observables, not old functions or source offsets.
/// An unmapped term is unknown/unsupported, never an implicitly satisfied rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyTerm {
    Literal {
        value_type: Type,
        value: DataValue,
    },
    Observed {
        point: Id,
        observable: Id,
        value_type: Type,
    },
    ViewRows {
        point: Id,
        entity: Id,
    },
    /// Canonical references of displayed rows with this exact binding enabled.
    RowActionRows {
        point: Id,
        view: Id,
        entity: Id,
        binding: Id,
        action: Id,
    },
    ViewColumn {
        point: Id,
        column: Id,
        value_type: Type,
    },
    OutputCount {
        point: Id,
        output: Id,
    },
    OutputColumn {
        point: Id,
        output: Id,
        column: Id,
        value_type: Type,
    },
    Count {
        value: Box<PropertyTerm>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyPredicate {
    Equal {
        left: PropertyTerm,
        right: PropertyTerm,
    },
    Less {
        left: PropertyTerm,
        right: PropertyTerm,
    },
    And {
        values: Vec<PropertyPredicate>,
    },
    Or {
        values: Vec<PropertyPredicate>,
    },
    Not {
        value: Box<PropertyPredicate>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedProperty {
    pub id: Id,
    pub description: String,
    pub predicate: PropertyPredicate,
}
fn standalone_type(typ: &Type, depth: usize) -> Result<()> {
    require(depth <= MAX_TYPE_DEPTH, "type nesting budget exceeded")?;
    match typ {
        Type::Reference { entity } => check_id(entity),
        Type::List { item } | Type::Optional { item } => standalone_type(item, depth + 1),
        _ => Ok(()),
    }
}
impl PropertyTerm {
    fn typ(&self, depth: usize, nodes: &mut usize) -> Result<Type> {
        *nodes += 1;
        require(
            depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
            "property budget exceeded",
        )?;
        Ok(match self {
            Self::Literal { value_type, value } => {
                standalone_type(value_type, 0)?;
                validate_value(value, value_type, 0)?;
                value_type.clone()
            }
            Self::Observed {
                point,
                observable,
                value_type,
            } => {
                check_id(point)?;
                check_id(observable)?;
                standalone_type(value_type, 0)?;
                value_type.clone()
            }
            Self::ViewRows { point, entity } => {
                check_id(point)?;
                check_id(entity)?;
                Type::list(Type::reference(entity))
            }
            Self::RowActionRows {
                point,
                view,
                entity,
                binding,
                action,
            } => {
                for id in [point, view, entity, binding, action] {
                    check_id(id)?;
                }
                Type::list(Type::reference(entity))
            }
            Self::ViewColumn {
                point,
                column,
                value_type,
            } => {
                check_id(point)?;
                check_id(column)?;
                standalone_type(value_type, 1)?;
                Type::list(value_type.clone())
            }
            Self::OutputCount { point, output } => {
                check_id(point)?;
                check_id(output)?;
                Type::Integer
            }
            Self::OutputColumn {
                point,
                output,
                column,
                value_type,
            } => {
                check_id(point)?;
                check_id(output)?;
                check_id(column)?;
                standalone_type(value_type, 0)?;
                require(
                    value_type.is_scalar(),
                    "output property column must be scalar",
                )?;
                Type::list(value_type.clone())
            }
            Self::Count { value } => {
                require(
                    matches!(value.typ(depth + 1, nodes)?, Type::List { .. }),
                    "count property needs a collection",
                )?;
                Type::Integer
            }
        })
    }
}
impl PropertyPredicate {
    fn validate(&self, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        require(
            depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
            "property budget exceeded",
        )?;
        match self {
            Self::Equal { left, right } | Self::Less { left, right } => {
                let typ = left.typ(depth + 1, nodes)?;
                require(
                    typ == right.typ(depth + 1, nodes)?,
                    "property operand types differ",
                )?;
                if matches!(self, Self::Less { .. }) {
                    require(typ.is_scalar(), "ordered property requires scalar")?;
                }
            }
            Self::And { values } | Self::Or { values } => {
                bounded(values.len())?;
                require(!values.is_empty(), "empty property connective")?;
                for value in values {
                    value.validate(depth + 1, nodes)?;
                }
            }
            Self::Not { value } => value.validate(depth + 1, nodes)?,
        }
        Ok(())
    }
}
impl AcceptedProperty {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        nonempty(&self.description)?;
        self.predicate.validate(0, &mut 0)
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
    /// None means an unmapped, invalid or missing observation, never a pass.
    pub fn evaluate(&self, observations: &[Observation]) -> Option<bool> {
        self.validate().ok()?;
        unique(observations.iter().map(|o| o.point.as_str())).ok()?;
        for observation in observations {
            observation.validate().ok()?;
        }
        self.predicate.evaluate(observations)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalArtifact {
    pub output: Id,
    pub format: OutputFormat,
    pub columns: Vec<FieldDefinition>,
    pub rows: Vec<Values>,
    pub bytes: Vec<u8>,
    pub bytes_digest: Digest,
    pub digest: Digest,
}
impl LocalArtifact {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.output)?;
        validate_output_columns(&self.columns)?;
        require(
            !self.columns.is_empty()
                && self.rows.len() <= MAX_COLLECTION
                && self.bytes.len() <= MAX_OUTPUT_BYTES,
            "artifact size/columns invalid",
        )?;
        for row in &self.rows {
            require(
                row.len() == self.columns.len()
                    && self.columns.iter().all(|c| row.contains_key(&c.id)),
                "artifact row columns mismatch",
            )?;
            for column in &self.columns {
                let value = &row[&column.id];
                validate_value(value, &column.value_type, 0)?;
                require(
                    !matches!(value, DataValue::List { .. } | DataValue::Null),
                    "artifact cells must be scalar",
                )?;
            }
        }
        require(
            self.bytes_digest == Self::bytes_identity(&self.bytes),
            "artifact byte digest mismatch",
        )?;
        require(
            self.bytes == encode_output(self.format, &self.columns, &self.rows)?,
            "artifact bytes do not encode observed rows",
        )?;
        require(
            self.digest == self.receipt_identity()?,
            "artifact semantic receipt mismatch",
        )
    }
    pub fn bytes_identity(bytes: &[u8]) -> Digest {
        bytes_digest(IdentityDomain::OutputBytes, bytes)
    }
    pub fn from_rows(
        output: &str,
        format: OutputFormat,
        columns: Vec<FieldDefinition>,
        rows: Vec<Values>,
    ) -> Result<Self> {
        check_id(output)?;
        let bytes = encode_output(format, &columns, &rows)?;
        let mut artifact = Self {
            output: output.into(),
            format,
            columns,
            rows,
            bytes_digest: Self::bytes_identity(&bytes),
            digest: Self::bytes_identity(&bytes),
            bytes,
        };
        artifact.digest = artifact.receipt_identity()?;
        artifact.validate()?;
        Ok(artifact)
    }
}
fn validate_untyped_value(value: &DataValue, depth: usize) -> Result<()> {
    require(depth <= MAX_TYPE_DEPTH, "value nesting budget exceeded")?;
    match value {
        DataValue::Text { value } => text(value),
        DataValue::Date { days } => validate_day(*days),
        DataValue::Reference { entity, record } => {
            check_id(entity)?;
            check_id(record)
        }
        DataValue::List { item_type, items } => {
            standalone_type(item_type, depth + 1)?;
            require(items.len() <= MAX_COLLECTION, "collection limit exceeded")?;
            for item in items {
                validate_value(item, item_type, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentedRow {
    pub record: RecordRef,
    pub cells: Values,
    pub enabled_actions: BTreeSet<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewObservation {
    pub view: Id,
    pub rows: Vec<PresentedRow>,
    pub controls: Values,
    pub selected: Vec<RecordRef>,
    pub enabled_actions: BTreeSet<Id>,
    pub form_values: Values,
}
/// Runtime-derived type provenance for real list/detail rows, even when empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSchema {
    pub entity: Id,
    pub columns: BTreeMap<Id, Type>,
    /// Complete declarations for row bindings in the actually presented view.
    /// None is historical/untyped evidence, never an empty declaration map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_actions: Option<BTreeMap<Id, Id>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub point: Id,
    pub data_digest: Digest,
    pub session_digest: Digest,
    pub values: Values,
    pub value_types: BTreeMap<Id, Type>,
    pub view: ViewObservation,
    /// Old evidence has no typed view provenance; view properties stay unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_schema: Option<ViewSchema>,
    pub outputs: Vec<LocalArtifact>,
}
impl Observation {
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Observation, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Observed,
    NoDifferenceFound,
    Inconclusive,
    Unsupported,
    Stale,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOrigin {
    ProductionRuntime,
    LegacyRuntime,
    TestFixture,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Applied,
    Rejected,
    Failed,
    BudgetExhausted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStep {
    pub index: usize,
    pub input: SemanticInput,
    pub before_data: Digest,
    pub after_data: Digest,
    pub before_session: Digest,
    pub after_session: Digest,
    pub outputs: Vec<Digest>,
    pub outcome: StepOutcome,
    pub diagnostic: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub source: SourceBinding,
    pub artifact: ArtifactRef,
    pub data_digest: Digest,
    pub input_digest: Digest,
    pub scenario_digest: Digest,
    pub decision_digest: Digest,
    pub session_digest: Digest,
    pub runtime_version: String,
    pub driver_version: String,
}
impl RunBinding {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.artifact.validate()?;
        nonempty(&self.runtime_version)?;
        nonempty(&self.driver_version)?;
        require(
            self.source.source_digest == self.artifact.raw_digest,
            "run source/artifact mismatch",
        )
    }
    pub fn matches(&self, current: &Self) -> bool {
        self == current
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    pub fuel: u64,
    pub collection_items: usize,
    pub action_steps: usize,
    pub transaction_writes: usize,
    pub output_bytes: usize,
    pub elapsed_millis: u64,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            fuel: 100_000,
            collection_items: MAX_COLLECTION,
            action_steps: MAX_ITEMS,
            transaction_writes: MAX_ITEMS,
            output_bytes: MAX_OUTPUT_BYTES,
            elapsed_millis: 5_000,
        }
    }
}
impl RuntimeLimits {
    pub fn validate(&self) -> Result<()> {
        require(
            self.fuel > 0
                && self.fuel <= 10_000_000
                && self.collection_items > 0
                && self.collection_items <= MAX_COLLECTION
                && self.action_steps > 0
                && self.action_steps <= MAX_ITEMS
                && self.transaction_writes > 0
                && self.transaction_writes <= MAX_ITEMS
                && self.output_bytes > 0
                && self.output_bytes <= MAX_OUTPUT_BYTES
                && self.elapsed_millis > 0
                && self.elapsed_millis <= 60_000,
            "runtime budget outside host limits",
        )
    }
}
/// Host-owned execution record. Restoring this from storage only restores a
/// claim; adoption still requires exact freshness and independently executed
/// obligations. This type is intentionally absent from DevelopmentResponse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEvidence {
    pub version: u32,
    pub id: Id,
    pub origin: ExecutionOrigin,
    pub binding: RunBinding,
    pub source_after: SourceBinding,
    pub state: EvidenceState,
    pub trace: Vec<TraceStep>,
    pub observations: Vec<Observation>,
    pub limits: RuntimeLimits,
    pub errors: Vec<String>,
    pub uncovered: Vec<String>,
}
impl RunEvidence {
    pub fn is_current(&self, current: &RunBinding) -> bool {
        self.binding.matches(current)
            && self.source_after == current.source
            && self.state != EvidenceState::Stale
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evidence, self)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        self.binding.validate()?;
        self.source_after.validate()?;
        self.limits.validate()?;
        require(
            self.state != EvidenceState::NoDifferenceFound,
            "no-difference classification belongs to a completed comparison, not a single run",
        )?;
        bounded(self.trace.len())?;
        bounded(self.observations.len())?;
        bounded(self.errors.len())?;
        bounded(self.uncovered.len())?;
        for error in self.errors.iter().chain(&self.uncovered) {
            nonempty(error)?;
        }
        unique(self.observations.iter().map(|o| o.point.as_str()))?;
        require(
            self.trace.len() <= self.limits.action_steps,
            "trace exceeds selected action limit",
        )?;
        for (index, step) in self.trace.iter().enumerate() {
            validate_input_shape(&step.input)?;
            require(index == step.index, "trace index is out of order")?;
            bounded(step.outputs.len())?;
            if let Some(message) = &step.diagnostic {
                nonempty(message)?;
            }
            if index > 0 {
                let prior = &self.trace[index - 1];
                require(
                    prior.after_data == step.before_data
                        && prior.after_session == step.before_session,
                    "trace state chain is broken",
                )?;
            }
            if matches!(step.input, SemanticInput::Navigate { .. }) {
                require(
                    step.before_data == step.after_data && step.outputs.is_empty(),
                    "navigation cannot mutate durable data or emit outputs",
                )?;
            }
            if matches!(
                step.input,
                SemanticInput::Observe { .. } | SemanticInput::AdvanceClock { .. }
            ) {
                require(
                    step.before_data == step.after_data
                        && step.before_session == step.after_session
                        && step.outputs.is_empty(),
                    "observation/clock input cannot mutate data or session",
                )?;
            }
            if step.outcome != StepOutcome::Applied {
                require(
                    step.before_data == step.after_data
                        && step.before_session == step.after_session
                        && step.outputs.is_empty(),
                    "failed step claims committed effects",
                )?;
            }
        }
        if let Some(first) = self.trace.first() {
            require(
                first.before_data == self.binding.data_digest
                    && first.before_session == self.binding.session_digest,
                "trace does not start from bound seed",
            )?;
        }
        let observed_points: Vec<_> = self
            .trace
            .iter()
            .filter_map(|step| match &step.input {
                SemanticInput::Observe { point } if step.outcome == StepOutcome::Applied => {
                    Some(point.as_str())
                }
                _ => None,
            })
            .collect();
        require(
            observed_points
                == self
                    .observations
                    .iter()
                    .map(|o| o.point.as_str())
                    .collect::<Vec<_>>(),
            "observations do not match ordered trace points",
        )?;
        for observation in &self.observations {
            observation.validate()?;
            validate_observation_limits(observation, &self.limits)?;
            let trace_index = self.trace.iter().position(|s| matches!(&s.input,SemanticInput::Observe { point } if point == &observation.point)).ok_or_else(|| ContractError("observation point missing from trace".into()))?;
            let step = &self.trace[trace_index];
            require(
                observation.data_digest == step.after_data
                    && observation.session_digest == step.after_session,
                "observation belongs to another runtime state",
            )?;
            let emitted: Vec<_> = self.trace[..=trace_index]
                .iter()
                .flat_map(|s| s.outputs.iter())
                .collect();
            require(
                emitted
                    == observation
                        .outputs
                        .iter()
                        .map(|o| &o.digest)
                        .collect::<Vec<_>>(),
                "observation outputs differ from emitted artifacts",
            )?;
        }
        if self.state == EvidenceState::Observed {
            require(
                self.errors.is_empty()
                    && self.trace.iter().all(|s| s.outcome == StepOutcome::Applied)
                    && !self.observations.is_empty(),
                "observed run contains failure or lacks observations",
            )?;
            require(
                self.source_after == self.binding.source,
                "changed source cannot have current observed status",
            )?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReductionOutcome {
    InvalidScenario,
    DifferenceLost,
    DifferencePreserved,
    Inconclusive,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReductionTrial {
    pub from: Digest,
    pub reduced: Digest,
    pub operation: String,
    pub outcome: ReductionOutcome,
    pub before_run: Option<Digest>,
    pub after_run: Option<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinimalityCertificate {
    pub initial: Digest,
    pub final_scenario: Digest,
    pub deletion_operations: Vec<String>,
    pub trials: Vec<ReductionTrial>,
    pub final_single_deletions: Vec<ReductionTrial>,
    /// True only when every final permitted single deletion was independently
    /// tried and found invalid or nondiscriminating. Never global optimality.
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DifferentialWitness {
    pub version: u32,
    pub id: Id,
    pub scenario: ScenarioSpec,
    pub before: RunEvidence,
    pub after: RunEvidence,
    pub state: EvidenceState,
    pub distinguishing_properties: Vec<AcceptedProperty>,
    pub minimization: Option<MinimalityCertificate>,
}
impl DifferentialWitness {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        self.scenario.validate_structure()?;
        self.before.validate()?;
        self.after.validate()?;
        for run in [&self.before, &self.after] {
            require(
                run.binding.data_digest == self.scenario.seed.identity()?
                    && run.binding.session_digest == self.scenario.session.identity()?,
                "witness run starts from another seed/session",
            )?;
            require(
                run.trace.iter().map(|step| &step.input).eq(self
                    .scenario
                    .inputs
                    .iter()
                    .take(run.trace.len())),
                "witness trace is not an exact executed scenario prefix",
            )?;
            if matches!(
                self.state,
                EvidenceState::Observed | EvidenceState::NoDifferenceFound
            ) || run.state == EvidenceState::Observed
            {
                require(
                    run.trace.len() == self.scenario.inputs.len(),
                    "completed witness/run did not execute the full scenario",
                )?;
            }
        }
        require(
            self.before.binding.input_digest == self.after.binding.input_digest
                && self.before.binding.input_digest == self.scenario.input_identity()?,
            "comparison does not use identical seed/input",
        )?;
        require(
            self.before.binding.scenario_digest == self.scenario.identity()?
                && self.after.binding.scenario_digest == self.scenario.identity()?,
            "witness scenario binding mismatch",
        )?;
        bounded(self.distinguishing_properties.len())?;
        for property in &self.distinguishing_properties {
            property.validate()?;
        }
        if matches!(
            self.state,
            EvidenceState::Observed | EvidenceState::NoDifferenceFound
        ) {
            require(
                self.before.state == EvidenceState::Observed
                    && self.after.state == EvidenceState::Observed,
                "comparison contains incomplete or failed execution",
            )?;
            for property in &self.scenario.validity {
                require(
                    property.evaluate(&self.before.observations) == Some(true)
                        && property.evaluate(&self.after.observations) == Some(true),
                    "witness workflow validity is false or unknown",
                )?;
            }
            let outcomes: Vec<_> = self
                .distinguishing_properties
                .iter()
                .map(|property| {
                    (
                        property.evaluate(&self.before.observations),
                        property.evaluate(&self.after.observations),
                    )
                })
                .collect();
            if self.state == EvidenceState::Observed {
                require(
                    outcomes
                        .iter()
                        .any(|pair| matches!(pair,(Some(a),Some(b)) if a != b)),
                    "no declared material property distinguishes the actual observations",
                )?;
            } else {
                require(
                    outcomes
                        .iter()
                        .all(|pair| matches!(pair,(Some(a),Some(b)) if a == b)),
                    "no-difference classification has a differing or unknown declared property",
                )?;
            }
        }
        if let Some(certificate) = &self.minimization {
            require(
                certificate.final_scenario == self.scenario.identity()?,
                "minimization describes another scenario",
            )?;
            bounded(certificate.trials.len())?;
            bounded(certificate.final_single_deletions.len())?;
            bounded(certificate.deletion_operations.len())?;
            for operation in &certificate.deletion_operations {
                nonempty(operation)?;
            }
            for trial in certificate
                .trials
                .iter()
                .chain(&certificate.final_single_deletions)
            {
                nonempty(&trial.operation)?;
            }
            if certificate.complete {
                require(
                    !certificate.deletion_operations.is_empty()
                        && certificate.final_single_deletions.iter().all(|t| {
                            t.from == certificate.final_scenario
                                && matches!(
                                    t.outcome,
                                    ReductionOutcome::InvalidScenario
                                        | ReductionOutcome::DifferenceLost
                                )
                        }),
                    "incomplete final minimization trials",
                )?;
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evidence, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMatch {
    Applies,
    Outside,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Population {
    All,
    NewWork,
    CreatedAfter { generation: u64 },
    Records { records: Vec<RecordRef> },
    Entity { entity: Id },
    Where { entity: Id, predicate: Expr },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownBoundary {
    pub id: Id,
    pub operations: BTreeSet<Id>,
    pub description: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionScope {
    pub operations: BTreeSet<Id>,
    pub population: Population,
    pub conditions: Values,
    pub excluded_records: Vec<RecordRef>,
    pub unknowns: Vec<UnknownBoundary>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeContext {
    pub operation: Id,
    pub record: Option<RecordRef>,
    pub is_new: Option<bool>,
    pub created_generation: Option<u64>,
    pub attributes: Values,
    /// Supplied only by an independently evaluated matching Where predicate.
    pub predicate_result: Option<bool>,
}
impl DecisionScope {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.operations.is_empty(),
            "decision scope needs explicit operations",
        )?;
        unique(self.operations.iter().map(String::as_str))?;
        bounded(self.conditions.len())?;
        for (id, value) in &self.conditions {
            check_id(id)?;
            validate_untyped_value(value, 0)?;
        }
        validate_record_refs(&self.excluded_records)?;
        match &self.population {
            Population::Entity { entity } => check_id(entity)?,
            Population::Where { entity, predicate } => {
                check_id(entity)?;
                // Schema/type matching is performed against the active program;
                // unresolved old fields must yield Unknown, not a broader scope.
                check_expr_shape(predicate, 0, &mut 0)?;
            }
            Population::Records { records } => {
                require(!records.is_empty(), "empty record scope")?;
                validate_record_refs(records)?;
            }
            _ => {}
        }
        unique(self.unknowns.iter().map(|u| u.id.as_str()))?;
        for unknown in &self.unknowns {
            nonempty(&unknown.description)?;
            unique(unknown.operations.iter().map(String::as_str))?;
            require(
                !unknown.operations.is_empty(),
                "unknown boundary needs affected operations",
            )?;
        }
        Ok(())
    }
    pub fn matches(&self, context: &ScopeContext) -> ScopeMatch {
        if !self.operations.contains(&context.operation) {
            return ScopeMatch::Outside;
        }
        if context
            .record
            .as_ref()
            .is_some_and(|r| self.excluded_records.contains(r))
        {
            return ScopeMatch::Outside;
        }
        let mut unknown = (!self.excluded_records.is_empty() && context.record.is_none())
            || self
                .unknowns
                .iter()
                .any(|u| u.operations.contains(&context.operation));
        for (key, value) in &self.conditions {
            match context.attributes.get(key) {
                Some(actual) if actual != value => return ScopeMatch::Outside,
                None => unknown = true,
                _ => {}
            }
        }
        let population = match &self.population {
            Population::All => Some(true),
            Population::NewWork => context.is_new,
            Population::CreatedAfter { generation } => {
                context.created_generation.map(|g| g > *generation)
            }
            Population::Records { records } => context.record.as_ref().map(|r| records.contains(r)),
            Population::Entity { entity } => context.record.as_ref().map(|r| &r.entity == entity),
            Population::Where { entity, .. } => match &context.record {
                Some(record) if &record.entity != entity => Some(false),
                Some(_) => context.predicate_result,
                None => None,
            },
        };
        match population {
            Some(false) => ScopeMatch::Outside,
            Some(true) if !unknown => ScopeMatch::Applies,
            _ => ScopeMatch::Unknown,
        }
    }
}
fn validate_record_refs(records: &[RecordRef]) -> Result<()> {
    require(
        records.len() <= MAX_COLLECTION,
        "record selection exceeds limit",
    )?;
    let mut seen = BTreeSet::new();
    for record in records {
        check_id(&record.entity)?;
        check_id(&record.record)?;
        require(
            seen.insert((&record.entity, &record.record)),
            "duplicate selected record",
        )?;
    }
    Ok(())
}
/// Syntactic AST bounds for source-independent scope predicates before a target
/// schema is known. Full typing is still required by the decision runtime.
fn check_expr_shape(expr: &Expr, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    require(
        depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
        "scope AST budget exceeded",
    )?;
    let next = depth + 1;
    match expr {
        Expr::Literal { value_type, value } => {
            standalone_type(value_type, 0)?;
            validate_value(value, value_type, 0)?;
        }
        Expr::Variable { name } => check_id(name)?,
        Expr::State { state } => check_id(state)?,
        Expr::Field { record, field } => {
            check_id(field)?;
            check_expr_shape(record, next, nodes)?;
        }
        Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            limit,
            ..
        } => {
            check_id(entity)?;
            check_id(binding)?;
            require(
                *limit > 0 && *limit <= MAX_COLLECTION,
                "query limit invalid",
            )?;
            check_expr_shape(predicate, next, nodes)?;
            bounded(sort.len())?;
            for key in sort {
                check_expr_shape(&key.value, next, nodes)?;
            }
        }
        Expr::Filter {
            items,
            binding,
            predicate,
        }
        | Expr::Any {
            items,
            binding,
            predicate,
        }
        | Expr::All {
            items,
            binding,
            predicate,
        } => {
            check_id(binding)?;
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(predicate, next, nodes)?;
        }
        Expr::Map {
            items,
            binding,
            value,
        } => {
            check_id(binding)?;
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(value, next, nodes)?;
        }
        Expr::Count { items } | Expr::Sum { items } => check_expr_shape(items, next, nodes)?,
        Expr::Contains { items, value } => {
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(value, next, nodes)?;
        }
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            check_expr_shape(left, next, nodes)?;
            check_expr_shape(right, next, nodes)?;
        }
        Expr::TextContains { text, search } => {
            check_expr_shape(text, next, nodes)?;
            check_expr_shape(search, next, nodes)?;
        }
        Expr::Lower { value } | Expr::Not { value } => check_expr_shape(value, next, nodes)?,
        Expr::And { values } | Expr::Or { values } => {
            bounded(values.len())?;
            require(!values.is_empty(), "empty boolean connective")?;
            for value in values {
                check_expr_shape(value, next, nodes)?;
            }
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            check_expr_shape(condition, next, nodes)?;
            check_expr_shape(then_value, next, nodes)?;
            check_expr_shape(else_value, next, nodes)?;
        }
        Expr::Coalesce { value, fallback } => {
            check_expr_shape(value, next, nodes)?;
            check_expr_shape(fallback, next, nodes)?;
        }
        Expr::DateAdd { date, days } => {
            check_expr_shape(date, next, nodes)?;
            check_expr_shape(days, next, nodes)?;
        }
        Expr::DateDifference { later, earlier } => {
            check_expr_shape(later, next, nodes)?;
            check_expr_shape(earlier, next, nodes)?;
        }
        Expr::Today => {}
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionOutcome {
    Accept { artifact: Digest },
    KeepCurrent,
    EitherAcceptable,
    BothNeeded,
    NeitherFits,
    Deferred,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionStatus {
    Active,
    Pending,
    Superseded {
        by: Id,
    },
    /// Terminal retained history, created only by the trusted recovery controller
    /// after compatible recovery and remaining-intention checks. The identifier
    /// binds its real adoption receipt, not a successor preference or model claim.
    Withdrawn {
        adoption: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedDecision {
    pub id: Id,
    pub revision: u64,
    pub request: String,
    pub rationale: Option<String>,
    pub scope: DecisionScope,
    pub outcome: DecisionOutcome,
    pub status: DecisionStatus,
    pub obligations: Vec<AcceptedProperty>,
    pub scenarios: Vec<Digest>,
    pub witness: Digest,
    pub supersedes: Vec<Id>,
}
impl ScopedDecision {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        require(self.revision > 0, "decision revision must be positive")?;
        nonempty(&self.request)?;
        if let Some(rationale) = &self.rationale {
            text(rationale)?;
        }
        self.scope.validate()?;
        unique(self.obligations.iter().map(|p| p.id.as_str()))?;
        for property in &self.obligations {
            property.validate()?;
        }
        bounded(self.scenarios.len())?;
        require(
            !self.scenarios.is_empty(),
            "decision lacks accepted scenario",
        )?;
        unique(self.supersedes.iter().map(String::as_str))?;
        require(
            !self.supersedes.contains(&self.id),
            "decision supersedes itself",
        )?;
        match &self.status {
            DecisionStatus::Superseded { by } => {
                check_id(by)?;
                require(by != &self.id, "decision retires itself")?;
            }
            DecisionStatus::Withdrawn { adoption } => check_id(adoption)?,
            DecisionStatus::Active | DecisionStatus::Pending => {}
        }
        if matches!(
            self.outcome,
            DecisionOutcome::BothNeeded | DecisionOutcome::NeitherFits | DecisionOutcome::Deferred
        ) {
            require(
                !matches!(self.status, DecisionStatus::Active),
                "unresolved decision cannot activate behavior",
            )?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionGraph {
    pub version: u32,
    pub revision: u64,
    pub decisions: Vec<ScopedDecision>,
}
impl DecisionGraph {
    pub fn validate(&self) -> Result<()> {
        let mut cx = crate::product_alloc::AllocationContext::new();
        self.validate_with(&mut cx).map_err(decision_contract_error)
    }
    pub(crate) fn validate_with(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError> {
        validate_decision_graph_with(self.version, &self.decisions, cx)
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
}

trait DecisionGraphNode {
    fn id(&self) -> &str;
    fn supersedes(&self) -> &[String];
    fn superseding_id(&self) -> Option<&str>;
    fn validate_node(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError>;
}

impl DecisionGraphNode for ScopedDecision {
    fn id(&self) -> &str { &self.id }
    fn supersedes(&self) -> &[String] { &self.supersedes }
    fn superseding_id(&self) -> Option<&str> {
        match &self.status { DecisionStatus::Superseded { by } => Some(by), _ => None }
    }
    fn validate_node(
        &self,
        _cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError> {
        self.validate().map_err(DecisionValidationError::Contract)
    }
}

impl DecisionGraphNode for fallible_decision_decode::DecodedScopedDecision {
    fn id(&self) -> &str { &self.id }
    fn supersedes(&self) -> &[String] { &self.supersedes }
    fn superseding_id(&self) -> Option<&str> {
        match &self.status { DecisionStatus::Superseded { by } => Some(by), _ => None }
    }
    fn validate_node(
        &self,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<(), DecisionValidationError> {
        validate_decoded_decision(self, cx)
    }
}

fn decision_contract_error(error: DecisionValidationError) -> ContractError {
    match error {
        DecisionValidationError::Contract(error) => error,
        DecisionValidationError::Allocation(_) => ContractError("decision validation allocation failed".into()),
    }
}

fn validate_decision_graph_with<D: DecisionGraphNode>(
    graph_version: u32,
    decisions: &[D],
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), DecisionValidationError> {
    version(graph_version)?;
    let mut index = crate::product_alloc::ordered::OrderedMap::<&str, usize>::new();
    for (position, decision) in decisions.iter().enumerate() {
        check_id(decision.id())?;
        if index.try_insert(decision.id(), position, cx)
            .map_err(DecisionValidationError::Allocation)?.is_some()
        {
            return Err(ContractError(format!("duplicate ID: {}", decision.id())).into());
        }
    }
    bounded(index.len())?;

    for decision in decisions {
        decision.validate_node(cx)?;
        for prior_id in decision.supersedes() {
            let prior_position = *index.get(prior_id.as_str())
                .ok_or_else(|| ContractError("unknown superseded decision".into()))?;
            let prior = &decisions[prior_position];
            require(
                matches!(prior.superseding_id(), Some(by) if by == decision.id()),
                "supersession graph is not reciprocal",
            )?;
        }
        let mut seen = crate::product_alloc::ordered::OrderedSet::<&str>::new();
        let mut current = decision;
        while let Some(by) = current.superseding_id() {
            require(
                seen.try_insert(current.id(), cx).map_err(DecisionValidationError::Allocation)?,
                "cyclic decision supersession",
            )?;
            let next_position = *index.get(by)
                .ok_or_else(|| ContractError("missing superseding decision".into()))?;
            let next = &decisions[next_position];
            require(
                next.supersedes().iter().any(|id| id == current.id()),
                "supersession history is missing",
            )?;
            current = next;
        }
    }
    Ok(())
}

fn unique_with<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), DecisionValidationError> {
    let mut seen = crate::product_alloc::ordered::OrderedSet::new();
    for id in ids {
        check_id(id)?;
        if !seen.try_insert(id, cx).map_err(DecisionValidationError::Allocation)? {
            return Err(ContractError(format!("duplicate ID: {id}")).into());
        }
    }
    bounded(seen.len())?;
    Ok(())
}

fn validate_decoded_decision(
    decision: &fallible_decision_decode::DecodedScopedDecision,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), DecisionValidationError> {
    check_id(&decision.id)?;
    require(decision.revision > 0, "decision revision must be positive")?;
    nonempty(&decision.request)?;
    if let Some(rationale) = &decision.rationale { text(rationale)?; }

    validate_decoded_decision_scope(&decision.scope, cx)?;
    unique_with(decision.obligations.iter().map(|property| property.id.as_str()), cx)?;
    for property in &decision.obligations { property.validate()?; }
    bounded(decision.scenarios.len())?;
    require(!decision.scenarios.is_empty(), "decision lacks accepted scenario")?;
    unique_with(decision.supersedes.iter().map(String::as_str), cx)?;
    require(!decision.supersedes.contains(&decision.id), "decision supersedes itself")?;
    match &decision.status {
        DecisionStatus::Superseded { by } => {
            check_id(by)?;
            require(by != &decision.id, "decision retires itself")?;
        }
        DecisionStatus::Withdrawn { adoption } => check_id(adoption)?,
        DecisionStatus::Active | DecisionStatus::Pending => {}
    }
    if matches!(decision.outcome, DecisionOutcome::BothNeeded | DecisionOutcome::NeitherFits | DecisionOutcome::Deferred) {
        require(!matches!(decision.status, DecisionStatus::Active), "unresolved decision cannot activate behavior")?;
    }
    Ok(())
}

fn validate_decoded_decision_scope(
    scope: &fallible_decision_decode::DecodedDecisionScope,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), DecisionValidationError> {
    require(!scope.operations.is_empty(), "decision scope needs explicit operations")?;
    unique_with(scope.operations.iter().map(String::as_str), cx)?;
    bounded(scope.conditions.len())?;
    for (id, value) in scope.conditions.iter() {
        check_id(id)?;
        validate_untyped_value(value, 0)?;
    }
    validate_record_refs_with(&scope.excluded_records, cx)?;
    match &scope.population {
        Population::Entity { entity } => check_id(entity)?,
        Population::Where { entity, predicate } => {
            check_id(entity)?;
            check_expr_shape(predicate, 0, &mut 0)?;
        }
        Population::Records { records } => {
            require(!records.is_empty(), "empty record scope")?;
            validate_record_refs_with(records, cx)?;
        }
        Population::All | Population::NewWork | Population::CreatedAfter { .. } => {}
    }
    unique_with(scope.unknowns.iter().map(|unknown| unknown.id.as_str()), cx)?;
    for unknown in &scope.unknowns {
        nonempty(&unknown.description)?;
        unique_with(unknown.operations.iter().map(String::as_str), cx)?;
        require(!unknown.operations.is_empty(), "unknown boundary needs affected operations")?;
    }

    Ok(())
}

fn validate_record_refs_with(
    records: &[RecordRef],
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), DecisionValidationError> {
    require(records.len() <= MAX_COLLECTION, "record selection exceeds limit")?;
    let mut seen = crate::product_alloc::ordered::OrderedSet::new();
    for record in records {
        check_id(&record.entity)?;
        check_id(&record.record)?;
        if !seen.try_insert((record.entity.as_str(), record.record.as_str()), cx)
            .map_err(DecisionValidationError::Allocation)?
        {
            return Err(ContractError("duplicate selected record".into()).into());
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Satisfied,
    Violated,
    Unknown,
    Outside,
    Unsupported,
    Stale,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCheck {
    pub decision: Id,
    pub decision_digest: Digest,
    pub property_digest: Digest,
    pub scope: ScopeMatch,
    pub state: CheckState,
    pub binding: RunBinding,
    pub evidence: Option<Digest>,
    pub explanation: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    Entity,
    Field,
    Action,
    Observable,
    Output,
    View,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticKey {
    pub kind: SemanticKind,
    pub entity: Option<Id>,
    pub id: Id,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMapping {
    pub from: SemanticKey,
    pub to: SemanticKey,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioMapping {
    pub original: Digest,
    /// Exact accepted source ArtifactRef.program_digest. An absent legacy value
    /// is usable only when the trusted host resolves the original unambiguously.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program: Option<Digest>,
    pub replacement: ScenarioSpec,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolutionProposal {
    pub version: u32,
    pub id: Id,
    pub request_digest: Digest,
    pub candidate: ArtifactRef,
    pub needs: Vec<Id>,
    pub proposed_retirement: Vec<Id>,
    pub preserved_obligations: Vec<Digest>,
    pub mappings: Vec<SemanticMapping>,
    pub scenarios: Vec<ScenarioMapping>,
}
impl EvolutionProposal {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        self.candidate.validate()?;
        check_id(&self.id)?;
        unique(self.needs.iter().map(String::as_str))?;
        unique(self.proposed_retirement.iter().map(String::as_str))?;
        require(
            !self.needs.is_empty(),
            "evolution lacks concrete accepted needs",
        )?;
        bounded(self.preserved_obligations.len())?;
        bounded(self.mappings.len())?;
        bounded(self.scenarios.len())?;
        validate_mappings(&self.mappings)?;
        let mut originals = BTreeSet::new();
        for scenario in &self.scenarios {
            require(
                originals.insert((&scenario.original, &scenario.source_program)),
                "ambiguous scenario replacement",
            )?;
            scenario.replacement.validate_structure()?;
            nonempty(&scenario.explanation)?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evolution, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityState {
    Compatible,
    RetainedButUnavailable,
    Incompatible,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityReport {
    pub state: CompatibilityState,
    pub current_data: Digest,
    pub target_program: Digest,
    pub retained_records: Vec<RecordRef>,
    pub retained_events: Vec<Id>,
    pub retained_fields: Vec<SemanticKey>,
    pub issues: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionPlan {
    pub version: u32,
    pub id: Id,
    pub project_id: Id,
    pub expected_generation: u64,
    pub expected_data: Digest,
    pub expected_decisions: Digest,
    pub expected_session: Digest,
    pub current_source: SourceBinding,
    pub target: ArtifactRef,
    pub scope: DecisionScope,
    pub compatibility: CompatibilityReport,
    pub required_decisions: Vec<Id>,
    pub checks: Vec<DecisionCheck>,
    pub evidence: Vec<Digest>,
    pub retire_decisions: Vec<Id>,
}
impl AdoptionPlan {
    /// Structural preparation only; the controller must compare this immutable
    /// plan to current state, rerun obligations and obtain the user's choice.
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        check_id(&self.project_id)?;
        self.current_source.validate()?;
        self.target.validate()?;
        self.scope.validate()?;
        require(
            self.project_id == self.current_source.project_id,
            "plan project/source mismatch",
        )?;
        unique(self.required_decisions.iter().map(String::as_str))?;
        unique(self.retire_decisions.iter().map(String::as_str))?;
        bounded(self.checks.len())?;
        bounded(self.evidence.len())?;
        require(
            self.compatibility.current_data == self.expected_data
                && self.compatibility.target_program == self.target.program_digest,
            "compatibility refers to different data/program",
        )?;
        self.compatibility.validate()?;
        for check in &self.checks {
            check_id(&check.decision)?;
            check.binding.validate()?;
            text(&check.explanation)?;
            if check.state == CheckState::Satisfied {
                require(
                    check.scope == ScopeMatch::Applies && check.evidence.is_some(),
                    "satisfied decision has no applicable run evidence",
                )?;
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Adoption, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPlan {
    pub adoption: AdoptionPlan,
    pub withdraw_decisions: Vec<Id>,
    pub preserve_current_data: Digest,
    pub preserve_current_events: Digest,
}
impl RecoveryPlan {
    pub fn validate(&self) -> Result<()> {
        self.adoption.validate()?;
        unique(self.withdraw_decisions.iter().map(String::as_str))?;
        require(
            self.preserve_current_data == self.adoption.expected_data,
            "recovery must retain current data, not old snapshot",
        )
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Adoption, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevelopmentOperation {
    Generate,
    Modify,
    Discover,
    Reconcile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    Synthetic,
    ExplicitlySelectedSanitized,
    /// Exact selected business records. This declares origin, not sanitization
    /// or authorization to send them to a development provider.
    ExplicitlySelected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedScenario {
    pub disclosure: Disclosure,
    pub scenario: ScenarioSpec,
}
/// Portable selected context, not an execution certificate or disclosure consent.
/// The host must populate this only from verified accepted packages/reruns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedSceneContext {
    pub decision: Id,
    pub source: ArtifactRef,
    pub scenario: Digest,
    pub observations: Vec<Observation>,
    pub disclosure: Disclosure,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentContext {
    pub view: Option<Id>,
    pub selected: Vec<RecordRef>,
    pub recent_inputs: Vec<SemanticInput>,
    pub data_digest: Option<Digest>,
    pub session_digest: Option<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentRequest {
    pub version: u32,
    pub id: Id,
    pub project_id: Id,
    pub operation: DevelopmentOperation,
    pub request: String,
    /// Discover: baseline, candidate, then selected accepted-history artifacts.
    /// Other operations: current source last after any historical sources.
    pub sources: Vec<CapturedProgram>,
    pub context: DevelopmentContext,
    pub examples: Vec<SelectedScenario>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_scenes: Vec<AcceptedSceneContext>,
    pub decisions: DecisionGraph,
    pub unknowns: Vec<UnknownBoundary>,
    pub required_capabilities: BTreeSet<Id>,
}
impl DevelopmentRequest {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        check_id(&self.project_id)?;
        nonempty(&self.request)?;
        bounded(self.sources.len())?;
        bounded(self.examples.len())?;
        bounded(self.context.recent_inputs.len())?;
        for input in &self.context.recent_inputs {
            validate_input_shape(input)?;
        }
        require(self.sources.len() <= 8, "too many development sources")?;
        for source in &self.sources {
            source.validate()?;
            require(
                source.binding.project_id == self.project_id,
                "request includes another project's source",
            )?;
        }
        if self.operation != DevelopmentOperation::Generate {
            require(
                !self.sources.is_empty(),
                "change request lacks actual source",
            )?;
        }
        if self.operation == DevelopmentOperation::Discover {
            require(
                self.sources.len() >= 2,
                "discovery needs an ordered baseline/candidate pair",
            )?;
            require(
                self.sources[0].program.id == self.sources[1].program.id,
                "discovery primary pair names different applications",
            )?;
            for (index, source) in self.sources.iter().enumerate().skip(2) {
                require(
                    !self.sources[..index]
                        .iter()
                        .any(|s| s.artifact == source.artifact),
                    "duplicate discovery historical source",
                )?;
                require(
                    self.accepted_scenes
                        .iter()
                        .any(|s| s.source == source.artifact),
                    "discovery historical source lacks selected accepted context",
                )?;
            }
        }
        validate_record_refs(&self.context.selected)?;
        if let Some(view) = &self.context.view {
            check_id(view)?;
        }
        self.decisions.validate()?;
        unique(self.unknowns.iter().map(|u| u.id.as_str()))?;
        unique(self.required_capabilities.iter().map(String::as_str))?;
        for unknown in &self.unknowns {
            nonempty(&unknown.description)?;
            unique(unknown.operations.iter().map(String::as_str))?;
        }
        for example in &self.examples {
            example.scenario.validate_structure()?;
        }
        bounded(self.accepted_scenes.len())?;
        serialized_bound(&self.accepted_scenes, MAX_WIRE_BYTES)?;
        let mut accepted_keys = BTreeSet::new();
        for accepted in &self.accepted_scenes {
            check_id(&accepted.decision)?;
            accepted.source.validate()?;
            let decision = self
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == accepted.decision)
                .ok_or_else(|| ContractError("accepted scene names an unknown decision".into()))?;
            require(
                decision.scenarios.contains(&accepted.scenario),
                "accepted scene is not a decision scenario",
            )?;
            let source = self
                .sources
                .iter()
                .find(|s| s.artifact == accepted.source)
                .ok_or_else(|| {
                    ContractError("accepted scene source is not captured in the request".into())
                })?;
            let selected = self
                .examples
                .iter()
                .find(|e| {
                    e.scenario.identity().ok().as_ref() == Some(&accepted.scenario)
                        && e.disclosure == accepted.disclosure
                })
                .ok_or_else(|| {
                    ContractError("accepted scene is not selected with this disclosure".into())
                })?;
            require(
                selected.scenario.seed.project_id == self.project_id,
                "accepted scene belongs to another project",
            )?;
            selected.scenario.validate(&source.program)?;
            require(
                accepted_keys.insert((
                    &accepted.decision,
                    &accepted.source.program_digest,
                    &accepted.scenario,
                )),
                "duplicate accepted scene context",
            )?;
            bounded(accepted.observations.len())?;
            let points: Vec<_> = selected
                .scenario
                .inputs
                .iter()
                .filter_map(|input| {
                    if let SemanticInput::Observe { point } = input {
                        Some(point)
                    } else {
                        None
                    }
                })
                .collect();
            require(
                !points.is_empty()
                    && points
                        == accepted
                            .observations
                            .iter()
                            .map(|o| &o.point)
                            .collect::<Vec<_>>(),
                "accepted observations do not match the selected scene",
            )?;
            let observable_types = source.program.observable_types()?;
            for observation in &accepted.observations {
                observation.validate()?;
                require(
                    observation.value_types == observable_types,
                    "accepted observable types do not match their source",
                )?;
                let expected_view = source.program.view_schema(&observation.view.view)?;
                if let Some(schema) = &observation.view_schema {
                    // Validate only supplied provenance here. Old metadata is
                    // never backfilled; exact replay still compares full receipts.
                    require(
                        expected_view.as_ref().is_some_and(|expected| {
                            expected.entity == schema.entity
                                && expected.columns == schema.columns
                                && schema.row_actions.as_ref().is_none_or(|actions| {
                                    expected.row_actions.as_ref() == Some(actions)
                                })
                        }),
                        "accepted view schema does not belong to its source",
                    )?;
                }
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Request, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    UnresolvedChoice,
    PossibleDefect,
    RequestedChange,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceHypothesis {
    pub id: Id,
    pub statement: String,
    pub kind: HypothesisKind,
    pub action: Id,
    pub observable: Id,
    pub sources: Vec<SourceLocus>,
    pub alternatives: Vec<Id>,
    pub related_decisions: Vec<Id>,
    /// Encoded ScenarioSpec: avoids imposing dynamic record-map JSON-schema
    /// features on providers. Parsed and independently validated before running.
    pub scenario_json: String,
    pub unknowns: Vec<String>,
}
impl ChoiceHypothesis {
    pub fn scenario(&self) -> Result<ScenarioSpec> {
        let scenario: ScenarioSpec = parse(self.scenario_json.as_bytes())?;
        scenario.validate_structure()?;
        Ok(scenario)
    }
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        nonempty(&self.statement)?;
        check_id(&self.action)?;
        check_id(&self.observable)?;
        bounded(self.sources.len())?;
        require(
            !self.sources.is_empty(),
            "hypothesis lacks actual source locus",
        )?;
        for source in &self.sources {
            validate_relative_path(&source.relative_path)?;
            text(&source.pointer)?;
        }
        unique(self.alternatives.iter().map(String::as_str))?;
        require(
            self.alternatives.len() >= 2,
            "choice needs at least two executable alternatives",
        )?;
        unique(self.related_decisions.iter().map(String::as_str))?;
        bounded(self.unknowns.len())?;
        for unknown in &self.unknowns {
            nonempty(unknown)?;
        }
        self.scenario()?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedCandidate {
    pub id: Id,
    pub source_json: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentResponse {
    pub version: u32,
    pub request_digest: Digest,
    pub candidates: Vec<GeneratedCandidate>,
    pub hypotheses: Vec<ChoiceHypothesis>,
    pub evolutions: Vec<EvolutionSuggestion>,
    pub unsupported: Vec<String>,
}
impl DevelopmentResponse {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let response: Self = parse(bytes)?;
        response.validate()?;
        Ok(response)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        unique(self.candidates.iter().map(|c| c.id.as_str()))?;
        require(self.candidates.len() <= 8, "too many candidates")?;
        for candidate in &self.candidates {
            AppDefinition::parse(candidate.source_json.as_bytes())?;
        }
        unique(self.hypotheses.iter().map(|h| h.id.as_str()))?;
        for hypothesis in &self.hypotheses {
            hypothesis.validate()?;
            for id in &hypothesis.alternatives {
                require(
                    self.candidates.iter().any(|c| &c.id == id),
                    "hypothesis alternative is missing",
                )?;
            }
        }
        unique(self.evolutions.iter().map(|e| e.id.as_str()))?;
        for evolution in &self.evolutions {
            evolution.validate()?;
            require(
                self.candidates.iter().any(|c| c.id == evolution.candidate),
                "evolution candidate is missing",
            )?;
        }
        bounded(self.unsupported.len())?;
        for item in &self.unsupported {
            nonempty(item)?;
        }
        require(
            !self.candidates.is_empty() || !self.unsupported.is_empty(),
            "empty provider response",
        )
    }
    pub fn validate_for(&self, request: &DevelopmentRequest) -> Result<()> {
        self.validate()?;
        require(
            self.request_digest == request.identity()?,
            "response belongs to another request",
        )?;
        for hypothesis in &self.hypotheses {
            for locus in &hypothesis.sources {
                let source = request
                    .sources
                    .iter()
                    .find(|s| {
                        s.artifact.raw_digest == locus.raw_digest
                            && s.binding.program_path == locus.relative_path
                    })
                    .ok_or_else(|| ContractError("hypothesis source was not in request".into()))?;
                locus.validate_against(source)?;
            }
            for id in &hypothesis.related_decisions {
                require(
                    request.decisions.decisions.iter().any(|d| &d.id == id),
                    "hypothesis names unknown decision",
                )?;
            }
            let scenario = hypothesis.scenario()?;
            for id in &hypothesis.alternatives {
                let candidate = self
                    .candidates
                    .iter()
                    .find(|c| &c.id == id)
                    .ok_or_else(|| ContractError("missing alternative".into()))?;
                let app = AppDefinition::parse(candidate.source_json.as_bytes())?;
                scenario.validate(&app)?;
                require(
                    app.actions.iter().any(|a| a.id == hypothesis.action)
                        && app
                            .observables
                            .iter()
                            .any(|o| o.id == hypothesis.observable),
                    "hypothesis references missing action/observable",
                )?;
            }
        }
        for evolution in &self.evolutions {
            evolution.validate_for(request, self)?;
        }
        Ok(())
    }
}
impl ScenarioSpec {
    pub fn validate_structure(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        nonempty(&self.label)?;
        self.seed.validate()?;
        validate_day(self.clock_day)?;
        bounded(self.inputs.len())?;
        bounded(self.validity.len())?;
        check_id(&self.session.view)?;
        bounded(self.session.values.len())?;
        for (id, value) in &self.session.values {
            check_id(id)?;
            validate_untyped_value(value, 0)?;
        }
        if let Some(record) = &self.session.focused_record {
            validate_record_refs(std::slice::from_ref(record))?;
        }
        let mut points = BTreeSet::new();
        for input in &self.inputs {
            validate_input_shape(input)?;
            if let SemanticInput::Observe { point } = input {
                require(points.insert(point), "duplicate observation point")?;
            }
        }
        for property in &self.validity {
            property.validate()?;
        }
        Ok(())
    }
}
fn validate_values_shape(values: &Values) -> Result<()> {
    bounded(values.len())?;
    for (id, value) in values {
        check_id(id)?;
        validate_untyped_value(value, 0)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterError {
    Invalid(ContractError),
    Allocation(crate::product_alloc::AllocationFailure),
    Unsupported(String),
    Stale(String),
    Cancelled,
    BudgetExhausted(String),
    Failed(String),
}
impl From<ContractError> for AdapterError {
    fn from(error: ContractError) -> Self {
        Self::Invalid(error)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCapabilities {
    pub adapter: Id,
    pub version: String,
    pub artifact_kinds: Vec<ArtifactKind>,
    pub features: BTreeSet<Id>,
    pub unavailable: Vec<String>,
}
/// Reserved metadata for an independently implemented external driver. Nothing
/// here starts an external server/build or claims that worktrees sandbox code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalRuntimeManifest {
    pub artifact: ArtifactRef,
    pub build_digest: Digest,
    pub dependency_lock: Digest,
    pub driver_version: String,
    pub action_mapping: Digest,
    pub seed_reset: Digest,
    pub verified_isolation_description: String,
    pub available: bool,
}
/// Auxiliary execution evidence, never serialized into source or saved output.
/// Only the local interpreter constructs these records from resolved Emit items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmissionRecord {
    record: RecordRef,
    created_program: Digest,
}
impl EmissionRecord {
    pub(crate) fn from_record(row: &Record) -> Self {
        Self {
            record: RecordRef {
                entity: row.entity.clone(),
                record: row.id.clone(),
            },
            created_program: row.created_program.clone(),
        }
    }
    pub fn record(&self) -> &RecordRef {
        &self.record
    }
    pub fn created_program(&self) -> &Digest {
        &self.created_program
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct EmissionExecution {
    before_data: Digest,
    before_session: Digest,
    input: Digest,
    day: i32,
}
/// Opaque, run-local evidence for one emission ordinal. Equal output bytes do
/// not identify participants. Absence means the entire emission is unverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedRecordParticipants {
    source: SourceBinding,
    program: ArtifactRef,
    operation: Id,
    ordinal: usize,
    output: Digest,
    records: Option<Vec<EmissionRecord>>,
    execution: Option<EmissionExecution>,
}
impl EmittedRecordParticipants {
    pub(crate) fn from_execution(
        program: &CapturedProgram,
        operation: &str,
        ordinal: usize,
        output: &LocalArtifact,
        records: Option<Vec<EmissionRecord>>,
    ) -> Self {
        Self {
            source: program.binding.clone(),
            program: program.artifact.clone(),
            operation: operation.into(),
            ordinal,
            output: output.digest.clone(),
            records,
            execution: None,
        }
    }
    pub(crate) fn bind_step(
        &mut self,
        input: &Digest,
        before_data: &Digest,
        before_session: &Digest,
        day: i32,
    ) {
        self.execution = Some(EmissionExecution {
            input: input.clone(),
            before_data: before_data.clone(),
            before_session: before_session.clone(),
            day,
        });
    }
    pub fn records(&self) -> Option<&[EmissionRecord]> {
        self.records.as_deref()
    }
    pub fn validate_for(
        &self,
        program: &CapturedProgram,
        operation: &str,
        ordinal: usize,
        output: &LocalArtifact,
        step: &TraceStep,
        day: i32,
    ) -> Result<()> {
        output.validate()?;
        require(
            self.source == program.binding
                && self.program == program.artifact
                && self.operation == operation
                && self.ordinal == ordinal
                && ordinal < MAX_ITEMS
                && self.output == output.digest
                && step.outcome == StepOutcome::Applied,
            "emission participants belong to another source, operation, ordinal or artifact",
        )?;
        let execution = self.execution.as_ref().ok_or_else(|| {
            ContractError("emission participants have no committed input step".into())
        })?;
        require(
            execution.before_data == step.before_data
                && execution.before_session == step.before_session
                && execution.day == day
                && execution.input
                    == canonical_digest(IdentityDomain::Input, &(&step.input, &program.artifact))?,
            "emission participants belong to another replay input frame",
        )?;
        if let Some(records) = &self.records {
            require(
                !records.is_empty()
                    && records.len() == output.rows.len()
                    && records.len() <= MAX_COLLECTION,
                "emission participant inventory is incomplete or exceeds bounds",
            )?;
        }
        Ok(())
    }
}

/// Local generated-app interpreter contract. It must copy initial state, apply
/// each input atomically, meter nested evaluation and expose the same production
/// views/artifacts used by daily work. Adapters cannot commit live project data.
pub trait RuntimeAdapter {
    type Run;
    /// Share cancellation with host proof checks in the same replay. Legacy
    /// adapters still enforce their own cancellation during execution.
    fn cancellation_token(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        None
    }
    fn capabilities(&self) -> RuntimeCapabilities;
    fn validate(&self, program: &CapturedProgram) -> std::result::Result<(), AdapterError>;
    fn start(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        session: &SessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
    ) -> std::result::Result<Self::Run, AdapterError>;
    fn apply(
        &self,
        run: &mut Self::Run,
        input: &SemanticInput,
        operation_id: &str,
    ) -> std::result::Result<TraceStep, AdapterError>;
    fn observe(
        &self,
        run: &Self::Run,
        point: &str,
    ) -> std::result::Result<Observation, AdapterError>;
    /// Inspect already-produced immutable output receipts without evaluating
    /// expressions, observing views, or charging execution fuel.
    fn emitted_artifacts<'a>(
        &self,
        _run: &'a Self::Run,
    ) -> std::result::Result<&'a [LocalArtifact], AdapterError> {
        Err(AdapterError::Unsupported(
            "Adapter cannot inspect emitted artifact provenance".into(),
        ))
    }
    /// Fresh auxiliary evidence from actual evaluated Emit items. A legacy
    /// adapter returning None grants no additional record-scope authority.
    fn emitted_record_participants<'a>(
        &self,
        _run: &'a Self::Run,
    ) -> std::result::Result<Option<&'a [EmittedRecordParticipants]>, AdapterError> {
        Ok(None)
    }
    fn data<'a>(&self, run: &'a Self::Run) -> &'a DataSnapshot;
    fn session<'a>(&self, run: &'a Self::Run) -> &'a SessionState;
    fn compatibility(
        &self,
        program: &CapturedProgram,
        current: &DataSnapshot,
    ) -> std::result::Result<CompatibilityReport, AdapterError>;
    fn prepare_adoption(
        &self,
        plan: AdoptionPlan,
        current: &DataSnapshot,
    ) -> std::result::Result<AdoptionPlan, AdapterError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevelopmentResult {
    pub response: DevelopmentResponse,
    pub producer: Producer,
}
/// Domain bridge implemented later over the separately owned opaque transport.
/// Dispatch belongs on a worker, with prior disclosure/usage authorization.
pub trait DevelopmentProvider {
    fn develop(
        &self,
        request: &DevelopmentRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<DevelopmentResult, AdapterError>;
}
pub trait SourceAdapter {
    fn capture(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> std::result::Result<CapturedProgram, AdapterError>;
    fn current_binding(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> std::result::Result<SourceBinding, AdapterError>;
}
pub const APP_SCHEMA: &str = include_str!("../assets/product_protocol/app.schema.json");
pub const RESPONSE_SCHEMA: &str = include_str!("../assets/product_protocol/response.schema.json");

fn encode_output(
    format: OutputFormat,
    columns: &[FieldDefinition],
    rows: &[Values],
) -> Result<Vec<u8>> {
    validate_output_columns(columns)?;
    require(
        !columns.is_empty() && rows.len() <= MAX_COLLECTION,
        "invalid output dimensions",
    )?;
    let scalar = |value: &DataValue| -> Result<JsonValue> {
        validate_untyped_value(value, 0)?;
        Ok(match value {
            DataValue::Text { value } => JsonValue::String(value.clone()),
            DataValue::Boolean { value } => JsonValue::Bool(*value),
            DataValue::Integer { value } => JsonValue::Number((*value).into()),
            DataValue::Date { days } => JsonValue::String(
                chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                    .expect("epoch")
                    .checked_add_signed(chrono::Duration::days(i64::from(*days)))
                    .ok_or_else(|| ContractError("date overflow".into()))?
                    .to_string(),
            ),
            DataValue::Reference { entity, record } => {
                serde_json::json!({"entity":entity,"record":record})
            }
            _ => return Err(ContractError("output cells must be scalar".into())),
        })
    };
    let quote = |value: &str| -> String {
        if value.contains([',', '"', '\r', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.into()
        }
    };
    let mut bytes = Vec::new();
    match format {
        OutputFormat::Csv => {
            bytes.extend(
                columns
                    .iter()
                    .map(|c| quote(&csv_literal(&c.id)))
                    .collect::<Vec<_>>()
                    .join(",")
                    .as_bytes(),
            );
            bytes.extend(b"\r\n");
        }
        OutputFormat::Json => bytes.push(b'['),
    }
    for (index, row) in rows.iter().enumerate() {
        require(
            row.len() == columns.len() && columns.iter().all(|c| row.contains_key(&c.id)),
            "output row columns mismatch",
        )?;
        let mut object = BTreeMap::new();
        let mut cells = Vec::new();
        for column in columns {
            let value = &row[&column.id];
            validate_value(value, &column.value_type, 0)?;
            let json = scalar(value)?;
            if format == OutputFormat::Json {
                object.insert(&column.id, json);
            } else {
                let mut rendered = match &json {
                    JsonValue::String(s) => s.clone(),
                    _ => String::from_utf8(canonical_bytes(&json)?)
                        .map_err(|e| ContractError(e.to_string()))?,
                };
                // Quoting CSV alone does not prevent spreadsheet formula execution.
                if matches!(value, DataValue::Text { .. }) {
                    rendered = csv_literal(&rendered);
                }
                cells.push(quote(&rendered));
            }
        }
        match format {
            OutputFormat::Csv => {
                bytes.extend(cells.join(",").as_bytes());
                bytes.extend(b"\r\n");
            }
            OutputFormat::Json => {
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend(canonical_bytes(&object)?);
            }
        }
        require(
            bytes
                .len()
                .saturating_add(usize::from(format == OutputFormat::Json))
                <= MAX_OUTPUT_BYTES,
            "output byte limit exceeded",
        )?;
    }
    if format == OutputFormat::Json {
        bytes.push(b']');
    }
    require(
        bytes.len() <= MAX_OUTPUT_BYTES,
        "output byte limit exceeded",
    )?;
    Ok(bytes)
}
impl PropertyTerm {
    fn evaluate(&self, observations: &[Observation]) -> Option<DataValue> {
        let point = |id: &str| observations.iter().find(|o| o.point == id);
        match self {
            Self::Literal { value, .. } => Some(value.clone()),
            Self::Observed {
                point: id,
                observable,
                value_type,
            } => {
                let observed = point(id)?;
                if observed.value_types.get(observable)? != value_type {
                    return None;
                }
                let value = observed.values.get(observable)?;
                validate_value(value, value_type, 0).ok()?;
                Some(value.clone())
            }
            Self::ViewRows { point: id, entity } => {
                let observed = point(id)?;
                if &observed.view_schema.as_ref()?.entity != entity {
                    return None;
                }
                Some(DataValue::List {
                    item_type: Type::reference(entity),
                    items: observed
                        .view
                        .rows
                        .iter()
                        .map(|row| DataValue::Reference {
                            entity: row.record.entity.clone(),
                            record: row.record.record.clone(),
                        })
                        .collect(),
                })
            }
            Self::RowActionRows {
                point: id,
                view,
                entity,
                binding,
                action,
            } => point(id)?.row_action_rows(view, entity, binding, action),
            Self::ViewColumn {
                point: id,
                column,
                value_type,
            } => {
                let observed = point(id)?;
                if observed.view_schema.as_ref()?.columns.get(column)? != value_type {
                    return None;
                }
                let items = observed
                    .view
                    .rows
                    .iter()
                    .map(|row| {
                        let value = row.cells.get(column)?;
                        validate_value(value, value_type, 0).ok()?;
                        Some(value.clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(DataValue::List {
                    item_type: value_type.clone(),
                    items,
                })
            }
            Self::OutputCount { point: id, output } => {
                let outputs: Vec<_> = point(id)?
                    .outputs
                    .iter()
                    .filter(|a| &a.output == output)
                    .collect();
                if outputs.is_empty() {
                    return None;
                }
                let mut count = 0i64;
                for artifact in outputs {
                    artifact.validate().ok()?;
                    count = count.checked_add(i64::try_from(artifact.rows.len()).ok()?)?;
                }
                Some(DataValue::Integer { value: count })
            }
            Self::OutputColumn {
                point: id,
                output,
                column,
                value_type,
            } => {
                let outputs: Vec<_> = point(id)?
                    .outputs
                    .iter()
                    .filter(|a| &a.output == output)
                    .collect();
                if outputs.is_empty() {
                    return None;
                }
                let mut items = vec![];
                for artifact in outputs {
                    artifact.validate().ok()?;
                    if !artifact.columns.iter().any(|definition| {
                        &definition.id == column && &definition.value_type == value_type
                    }) {
                        return None;
                    }
                    for row in &artifact.rows {
                        let value = row.get(column)?;
                        validate_value(value, value_type, 0).ok()?;
                        items.push(value.clone());
                        if items.len() > MAX_COLLECTION {
                            return None;
                        }
                    }
                }
                Some(DataValue::List {
                    item_type: value_type.clone(),
                    items,
                })
            }
            Self::Count { value } => {
                let DataValue::List { items, .. } = value.evaluate(observations)? else {
                    return None;
                };
                Some(DataValue::Integer {
                    value: i64::try_from(items.len()).ok()?,
                })
            }
        }
    }
}
impl PropertyPredicate {
    fn evaluate(&self, observations: &[Observation]) -> Option<bool> {
        match self {
            Self::Equal { left, right } => {
                Some(left.evaluate(observations)? == right.evaluate(observations)?)
            }
            Self::Less { left, right } => Some(
                match (left.evaluate(observations)?, right.evaluate(observations)?) {
                    (DataValue::Integer { value: a }, DataValue::Integer { value: b }) => a < b,
                    (DataValue::Text { value: a }, DataValue::Text { value: b }) => a < b,
                    (DataValue::Date { days: a }, DataValue::Date { days: b }) => a < b,
                    (DataValue::Boolean { value: a }, DataValue::Boolean { value: b }) => !a && b,
                    (
                        DataValue::Reference {
                            entity: a,
                            record: x,
                        },
                        DataValue::Reference {
                            entity: b,
                            record: y,
                        },
                    ) => (a, x) < (b, y),
                    _ => return None,
                },
            ),
            Self::And { values } => {
                let mut unknown = false;
                for value in values {
                    match value.evaluate(observations) {
                        Some(false) => return Some(false),
                        None => unknown = true,
                        _ => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(true)
                }
            }
            Self::Or { values } => {
                let mut unknown = false;
                for value in values {
                    match value.evaluate(observations) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        _ => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(false)
                }
            }
            Self::Not { value } => value.evaluate(observations).map(|v| !v),
        }
    }
}
fn validate_view_observation(view: &ViewObservation) -> Result<()> {
    check_id(&view.view)?;
    validate_record_refs(&view.selected)?;
    validate_values_shape(&view.controls)?;
    validate_values_shape(&view.form_values)?;
    unique(view.enabled_actions.iter().map(String::as_str))?;
    require(view.rows.len() <= MAX_COLLECTION, "too many observed rows")?;
    let mut records = BTreeSet::new();
    for row in &view.rows {
        validate_record_refs(std::slice::from_ref(&row.record))?;
        require(
            records.insert((&row.record.entity, &row.record.record)),
            "duplicate observed row",
        )?;
        validate_values_shape(&row.cells)?;
        unique(row.enabled_actions.iter().map(String::as_str))?;
    }
    Ok(())
}

fn serialized_bound<T: Serialize>(value: &T, max: usize) -> Result<()> {
    struct Counter {
        size: usize,
        max: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.size = self.size.saturating_add(bytes.len());
            if self.size > self.max {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "serialized contract exceeds byte limit",
                ));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter { size: 0, max }, value).map_err(|e| ContractError(e.to_string()))
}
impl ArtifactRef {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        nonempty(&self.language)?;
        if self.kind == ArtifactKind::GeneratedApp {
            require(
                self.language == LANGUAGE_VERSION,
                "unsupported application language",
            )?;
        }
        Ok(())
    }
}
impl DecisionScope {
    pub fn validate_for(&self, app: &AppDefinition) -> Result<()> {
        self.validate()?;
        app.validate()?;
        let mut validator = Validator::new(app, Audit::none());
        for operation in &self.operations {
            validator.action(operation)?;
        }
        match &self.population {
            Population::Entity { entity } => {
                validator.entity(entity)?;
            }
            Population::Records { records } => {
                for record in records {
                    validator.entity(&record.entity)?;
                }
            }
            _ => {}
        }
        for excluded in &self.excluded_records {
            validator.entity(&excluded.entity)?;
        }
        if let Population::Where { entity, predicate } = &self.population {
            validator.entity(entity)?;
            validator.expect(
                predicate,
                &BTreeMap::from([("record".into(), Type::reference(entity))]),
                &Type::Boolean,
                0,
                false,
            )?;
        }
        Ok(())
    }
}
impl DevelopmentResult {
    pub fn validate_for(&self, request: &DevelopmentRequest) -> Result<()> {
        self.response.validate_for(request)?;
        self.producer.validate()?;
        if let Producer::LiveAgent { request_digest, .. } = &self.producer {
            require(
                request_digest == &request.identity()?,
                "producer belongs to another request",
            )?;
        }
        Ok(())
    }
}

impl SemanticKey {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        if let Some(entity) = &self.entity {
            check_id(entity)?;
        }
        require(
            (self.kind == SemanticKind::Field) == self.entity.is_some(),
            "only field keys require an entity namespace",
        )
    }
}
impl CompatibilityReport {
    pub fn validate(&self) -> Result<()> {
        validate_record_refs(&self.retained_records)?;
        require(
            self.retained_events.len() <= MAX_EVENTS
                && self.retained_fields.len() <= MAX_COLLECTION,
            "compatibility retention inventory exceeds bounds",
        )?;
        let mut events = BTreeSet::new();
        for event in &self.retained_events {
            check_id(event)?;
            require(events.insert(event), "duplicate retained event")?;
        }
        let mut fields = BTreeSet::new();
        for field in &self.retained_fields {
            field.validate()?;
            require(
                field.kind == SemanticKind::Field,
                "retained field inventory contains a non-field key",
            )?;
            require(
                fields.insert(canonical_bytes(field)?),
                "duplicate retained semantic key",
            )?;
        }
        bounded(self.issues.len())?;
        for issue in &self.issues {
            nonempty(issue)?;
        }
        Ok(())
    }
}

impl Producer {
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none())}
    fn validate_audited(&self,a:Audit<'_>)->Result<()> {
        a.pay(8)?;
        match self {
            Producer::LiveAgent {
                provider,
                invocation_id,
                session_id,
                ..
            } => {
                a.nonempty(provider)?;
                a.nonempty(invocation_id)?;
                if let Some(session) = session_id {
                    a.nonempty(session)?;
                }
            }
            Producer::ExternalAuthor { description } => a.nonempty(description)?,
            Producer::Fixture { name } => a.nonempty(name)?,
            Producer::UserAuthored => {}
        }
        Ok(())
    }
}

impl LocalArtifact {
    pub fn receipt_identity(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Output,
            &(
                &self.output,
                self.format,
                &self.columns,
                &self.rows,
                &self.bytes_digest,
            ),
        )
    }
}
pub fn validate_input_shape(input: &SemanticInput) -> Result<()> {
    match input {
        SemanticInput::Invoke { action, arguments } => {
            check_id(action)?;
            validate_values_shape(arguments)
        }
        SemanticInput::Submit { view, arguments } => {
            check_id(view)?;
            validate_values_shape(arguments)
        }
        SemanticInput::Control {
            view,
            control,
            value,
        } => {
            check_id(view)?;
            check_id(control)?;
            validate_untyped_value(value, 0)
        }
        SemanticInput::Activate { view, binding, row } => {
            check_id(view)?;
            check_id(binding)?;
            if let Some(row) = row {
                validate_record_refs(std::slice::from_ref(row))?;
            }
            Ok(())
        }
        SemanticInput::Navigate { view } => check_id(view),
        SemanticInput::Observe { point } => check_id(point),
        SemanticInput::AdvanceClock { days } => {
            require(*days <= 3_652_058, "clock advance exceeds bound")
        }
    }
}
fn csv_literal(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{value}")
    } else {
        value.into()
    }
}
fn validate_observation_limits(observation: &Observation, limits: &RuntimeLimits) -> Result<()> {
    fn value_limit(value: &DataValue, limit: usize, depth: usize) -> Result<()> {
        require(
            depth <= MAX_TYPE_DEPTH,
            "observed value nesting exceeds limit",
        )?;
        if let DataValue::List { items, .. } = value {
            require(
                items.len() <= limit,
                "observed collection exceeds selected limit",
            )?;
            for item in items {
                value_limit(item, limit, depth + 1)?;
            }
        }
        Ok(())
    }
    require(
        observation.view.rows.len() <= limits.collection_items
            && observation.view.selected.len() <= limits.collection_items,
        "observed view exceeds selected collection limit",
    )?;
    for value in observation
        .values
        .values()
        .chain(observation.view.controls.values())
        .chain(observation.view.form_values.values())
        .chain(observation.view.rows.iter().flat_map(|r| r.cells.values()))
    {
        value_limit(value, limits.collection_items, 0)?;
    }
    let mut bytes = 0usize;
    for artifact in &observation.outputs {
        require(
            artifact.rows.len() <= limits.collection_items,
            "output rows exceed selected collection limit",
        )?;
        bytes = bytes.saturating_add(artifact.bytes.len());
    }
    require(
        bytes <= limits.output_bytes,
        "cumulative emitted bytes exceed selected limit",
    )
}

/// Untrusted reconciliation suggestion. Candidate IDs are resolved by the host;
/// references to captured digests are checked, never authoritative model proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolutionSuggestion {
    pub id: Id,
    pub candidate: Id,
    pub needs: Vec<Id>,
    pub proposed_retirement: Vec<Id>,
    pub preserved_obligations: Vec<Digest>,
    pub mappings: Vec<SemanticMapping>,
    pub scenarios: Vec<SuggestedScenarioMapping>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestedScenarioMapping {
    pub original: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program: Option<Digest>,
    pub replacement_json: String,
    pub explanation: String,
}
impl SuggestedScenarioMapping {
    pub fn decode(&self) -> Result<ScenarioMapping> {
        nonempty(&self.explanation)?;
        let replacement: ScenarioSpec = parse(self.replacement_json.as_bytes())?;
        replacement.validate_structure()?;
        Ok(ScenarioMapping {
            original: self.original.clone(),
            source_program: self.source_program.clone(),
            replacement,
            explanation: self.explanation.clone(),
        })
    }
}
fn validate_mappings(mappings: &[SemanticMapping]) -> Result<()> {
    bounded(mappings.len())?;
    let mut sources = BTreeSet::new();
    for mapping in mappings {
        mapping.from.validate()?;
        mapping.to.validate()?;
        require(
            mapping.from.kind == mapping.to.kind,
            "semantic mapping changes kind",
        )?;
        require(
            sources.insert(canonical_bytes(&mapping.from)?),
            "ambiguous semantic mapping",
        )?;
    }
    Ok(())
}
impl SemanticKey {
    pub fn exists_in(&self, app: &AppDefinition) -> bool {
        if self.validate().is_err() {
            return false;
        }
        match self.kind {
            SemanticKind::Entity => app.entities.iter().any(|e| e.id == self.id),
            SemanticKind::Field => app.entities.iter().any(|e| {
                Some(&e.id) == self.entity.as_ref() && e.fields.iter().any(|f| f.id == self.id)
            }),
            SemanticKind::Action => app.actions.iter().any(|a| a.id == self.id),
            SemanticKind::Observable => app.observables.iter().any(|o| o.id == self.id),
            SemanticKind::Output => app.outputs.iter().any(|o| o.id == self.id),
            SemanticKind::View => app.views.iter().any(|v| v.id == self.id),
        }
    }
}
impl EvolutionSuggestion {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        check_id(&self.candidate)?;
        unique(self.needs.iter().map(String::as_str))?;
        require(
            !self.needs.is_empty(),
            "evolution needs accepted work examples",
        )?;
        unique(self.proposed_retirement.iter().map(String::as_str))?;
        bounded(self.preserved_obligations.len())?;
        validate_mappings(&self.mappings)?;
        bounded(self.scenarios.len())?;
        let mut originals = BTreeSet::new();
        for mapping in &self.scenarios {
            mapping.decode()?;
            require(
                originals.insert((&mapping.original, &mapping.source_program)),
                "ambiguous scenario replacement",
            )?;
        }
        Ok(())
    }
    fn validate_for(
        &self,
        request: &DevelopmentRequest,
        response: &DevelopmentResponse,
    ) -> Result<()> {
        self.validate()?;
        require(
            request.operation == DevelopmentOperation::Reconcile,
            "evolution suggestions require a reconciliation request",
        )?;
        let candidate = response
            .candidates
            .iter()
            .find(|c| c.id == self.candidate)
            .ok_or_else(|| ContractError("evolution candidate is missing".into()))?;
        let app = AppDefinition::parse(candidate.source_json.as_bytes())?;
        for need in &self.needs {
            require(
                request.decisions.decisions.iter().any(|d| &d.id == need),
                "evolution names an unknown accepted need",
            )?;
        }
        for retired in &self.proposed_retirement {
            require(
                self.needs.contains(retired)
                    && request.decisions.decisions.iter().any(|d| {
                        &d.id == retired
                            && matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                    }),
                "retirement proposals must name a current listed need",
            )?;
        }
        let obligations: BTreeSet<_> = request
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .flat_map(|d| d.obligations.iter())
            .map(AcceptedProperty::identity)
            .collect::<Result<_>>()?;
        for preserved in &self.preserved_obligations {
            require(
                obligations.contains(preserved),
                "evolution names unknown active obligation",
            )?;
        }
        for mapping in &self.mappings {
            require(
                request
                    .sources
                    .iter()
                    .any(|s| mapping.from.exists_in(&s.program))
                    && mapping.to.exists_in(&app),
                "evolution mapping references unknown source/candidate semantics",
            )?;
        }
        let mut known_scenarios: BTreeSet<_> = request
            .decisions
            .decisions
            .iter()
            .flat_map(|d| d.scenarios.iter().cloned())
            .collect();
        for example in &request.examples {
            known_scenarios.insert(example.scenario.identity()?);
        }
        for mapping in &self.scenarios {
            let accepted_sources: BTreeSet<_> = request
                .accepted_scenes
                .iter()
                .filter(|s| s.scenario == mapping.original)
                .map(|s| &s.source.program_digest)
                .collect();
            if let Some(source) = &mapping.source_program {
                require(
                    request
                        .sources
                        .iter()
                        .any(|s| &s.artifact.program_digest == source),
                    "scenario mapping names an unknown source program",
                )?;
                require(
                    accepted_sources.is_empty() || accepted_sources.contains(source),
                    "scenario mapping source does not match the accepted scene",
                )?;
            } else {
                require(
                    accepted_sources.len() <= 1,
                    "scenario mapping needs an explicit source program",
                )?;
            }
            require(
                known_scenarios.contains(&mapping.original),
                "evolution replaces an unknown scenario",
            )?;
            let mapping = mapping.decode()?;
            require(
                mapping.replacement.seed.project_id == request.project_id,
                "replacement scene belongs to another project",
            )?;
            mapping.replacement.validate(&app)?;
        }
        Ok(())
    }
}
fn validate_output_columns(columns: &[FieldDefinition]) -> Result<()> {
    unique(columns.iter().map(|column| column.id.as_str()))?;
    require(!columns.is_empty(), "output needs declared columns")?;
    for column in columns {
        nonempty(&column.label)?;
        standalone_type(&column.value_type, 0)?;
        require(
            column.value_type.is_scalar(),
            "output column type must be scalar",
        )?;
    }
    Ok(())
}

impl AppDefinition {
    /// Internal structural analysis uses the same expression typing as complete
    /// program validation. Callers must first validate the captured program and
    /// derive these lexical bindings from its parameters and statements.
    pub(crate) fn expression_type_in(
        &self,
        expression: &Expr,
        bindings: &BTreeMap<Id, Type>,
    ) -> Result<Type> {
        Validator::new(self, Audit::none())
        .expr(expression, bindings, 0, true)
    }

    /// Derive row/cell types from validated executable view expressions.
    pub fn view_schema(&self, view: &str) -> Result<Option<ViewSchema>> {
        self.validate()?;
        self.view_schema_validated(view)
    }
    fn view_schema_validated(&self, view: &str) -> Result<Option<ViewSchema>> {
        #[cfg(test)]
        count_validation(|counts| counts.views += 1);
        let view = self
            .views
            .iter()
            .find(|v| v.id == view)
            .ok_or_else(|| ContractError("unknown view".into()))?;
        let (entity, columns) = match &view.kind {
            ViewKind::List {
                entity, columns, ..
            }
            | ViewKind::Detail {
                entity, columns, ..
            } => (entity, columns),
            ViewKind::Form { .. } => return Ok(None),
        };
        let mut validator = Validator::new(self, Audit::none());
        let env = BTreeMap::from([("row".into(), Type::reference(entity))]);
        let columns = columns
            .iter()
            .map(|column| {
                Ok((
                    column.id.clone(),
                    validator.expr(&column.value, &env, 0, true)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Some(ViewSchema {
            entity: entity.clone(),
            columns,
            row_actions: Some(
                view.actions
                    .iter()
                    .filter(|binding| binding.placement == ActionPlacement::Row)
                    .map(|binding| (binding.id.clone(), binding.action.clone()))
                    .collect(),
            ),
        }))
    }
    /// Static declared types for host observations, including null/empty results.
    pub fn observable_types(&self) -> Result<BTreeMap<Id, Type>> {
        self.validate()?;
        self.observable_types_validated()
    }
    fn observable_types_validated(&self) -> Result<BTreeMap<Id, Type>> {
        #[cfg(test)]
        count_validation(|counts| counts.observables += 1);
        let mut validator = Validator::new(self, Audit::none());
        self.observables
            .iter()
            .map(|observable| {
                Ok((
                    observable.id.clone(),
                    validator.expr(&observable.value, &BTreeMap::new(), 0, true)?,
                ))
            })
            .collect()
    }
}
impl Observation {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.point)?;
        validate_values_shape(&self.values)?;
        require(
            self.values.len() == self.value_types.len(),
            "observed values lack matching declared types",
        )?;
        for (id, value_type) in &self.value_types {
            check_id(id)?;
            standalone_type(value_type, 0)?;
            let value = self
                .values
                .get(id)
                .ok_or_else(|| ContractError("declared observable has no value".into()))?;
            validate_value(value, value_type, 0)?;
        }
        validate_view_observation(&self.view)?;
        if let Some(schema) = &self.view_schema {
            check_id(&schema.entity)?;
            bounded(schema.columns.len())?;
            for (column, typ) in &schema.columns {
                check_id(column)?;
                standalone_type(typ, 0)?;
            }
            if let Some(actions) = &schema.row_actions {
                bounded(actions.len())?;
                for (binding, action) in actions {
                    check_id(binding)?;
                    check_id(action)?;
                }
                require(
                    self.view.rows.iter().all(|row| {
                        row.enabled_actions
                            .iter()
                            .all(|binding| actions.contains_key(binding))
                    }),
                    "enabled row binding lacks declared provenance",
                )?;
            }
            for row in &self.view.rows {
                require(
                    row.record.entity == schema.entity && row.cells.len() == schema.columns.len(),
                    "view row/schema mismatch",
                )?;
                for (column, typ) in &schema.columns {
                    let value = row
                        .cells
                        .get(column)
                        .ok_or_else(|| ContractError("view schema column missing".into()))?;
                    validate_value(value, typ, 0)?;
                }
            }
        }
        bounded(self.outputs.len())?;
        for output in &self.outputs {
            output.validate()?;
        }
        Ok(())
    }
}

impl Observation {
    /// Shared typed sampler for comparisons and saved-property replay. Runtime
    /// rendering has already evaluated both binding conditions and action guards.
    pub(crate) fn row_action_rows(
        &self,
        view: &str,
        entity: &str,
        binding: &str,
        action: &str,
    ) -> Option<DataValue> {
        self.validate().ok()?;
        let schema = self.view_schema.as_ref()?;
        if self.view.view != view
            || schema.entity != entity
            || schema.row_actions.as_ref()?.get(binding)? != action
        {
            return None;
        }
        let mut rows: Vec<_> = self
            .view
            .rows
            .iter()
            .filter(|row| row.enabled_actions.contains(binding))
            .map(|row| &row.record)
            .collect();
        rows.sort_by(|a, b| (&a.entity, &a.record).cmp(&(&b.entity, &b.record)));
        Some(DataValue::List {
            item_type: Type::reference(entity),
            items: rows
                .into_iter()
                .map(|row| DataValue::Reference {
                    entity: row.entity.clone(),
                    record: row.record.clone(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
#[path = "product_contract/capture_stage_tests.rs"]
mod capture_stage_tests;

#[cfg(test)]
#[path = "product_contract/capture_canonical_removal_tests.rs"]
mod capture_canonical_removal_tests;

#[cfg(test)]
#[path = "product_contract/retained_source_controls.rs"]
mod retained_source_controls;
