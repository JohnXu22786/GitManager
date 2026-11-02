//! Nonserializable, allocation-aware scalar replay context over decoded owners.
use crate::product_alloc::{self, ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{AppValidationError, Digest, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot};
use crate::product_store::{fallible_project_snapshot_decode::{DecodedProjectSnapshot, ProjectSnapshotCloneError}, PointerCodecFailure, StoreError};

use super::{
    compiler, derive_correspondence_with_fallible, replay_snapshot_fallible,
    CorrespondenceFailure, DecodedReplayFrame, DecodedScopeCorrespondence,
    DecodedVerifiedCorrespondence, FallibleDecodedScopeState, ReplayRegistryError,
};

#[derive(Debug)]
pub(crate) enum ScalarReplayFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for ScalarReplayFailure {
    fn from(error: StoreError) -> Self { Self::Store(error) }
}
impl From<AllocationFailure> for ScalarReplayFailure {
    fn from(error: AllocationFailure) -> Self { Self::Allocation(error) }
}
fn correspondence_error(error: CorrespondenceFailure) -> ScalarReplayFailure {
    match error {
        CorrespondenceFailure::Store(error) => ScalarReplayFailure::Store(error),
        CorrespondenceFailure::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
    }
}
fn store_error(error: StoreError) -> ScalarReplayFailure {
    match error {
        StoreError::PointerCodec(PointerCodecFailure::Allocation(failure)) => ScalarReplayFailure::Allocation(failure),
        error => ScalarReplayFailure::Store(error),
    }
}
fn app_error(error: AppValidationError) -> ScalarReplayFailure {
    match error {
        AppValidationError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        AppValidationError::Contract(error) => ScalarReplayFailure::Store(StoreError::Invalid(error.0)),
    }
}
fn copy_error(error: ProjectSnapshotCloneError) -> ScalarReplayFailure {
    match error {
        ProjectSnapshotCloneError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        ProjectSnapshotCloneError::Invariant => ScalarReplayFailure::Store(StoreError::Invalid("replay snapshot copy invariant".into())),
    }
}
fn data_clone_error(error: crate::product_contract::FallibleDataSnapshotCloneError) -> ScalarReplayFailure {
    match error {
        crate::product_contract::FallibleDataSnapshotCloneError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        crate::product_contract::FallibleDataSnapshotCloneError::Invariant => ScalarReplayFailure::Store(StoreError::Invalid("data snapshot copy invariant".into())),
    }
}
fn initialization_error(error: super::InitializationFailure) -> ScalarReplayFailure {
    match error {
        super::InitializationFailure::Store(error) => ScalarReplayFailure::Store(error),
        super::InitializationFailure::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
    }
}
fn codec_error(error: crate::product_backup::fallible_codec::CodecError) -> ScalarReplayFailure {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        _ => ScalarReplayFailure::Store(StoreError::Invalid("correspondence identity invariant".into())),
    }
}
fn decode_error(error: crate::product_contract::fallible_value_decode::ValueDecodeError) -> ScalarReplayFailure {
    match error {
        crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        _ => ScalarReplayFailure::Store(StoreError::Invalid("correspondence copy invariant".into())),
    }
}
fn compiler_failure(error: compiler::CompilerFailure) -> ScalarReplayFailure {
    match error {
        compiler::CompilerFailure::Store(error) => ScalarReplayFailure::Store(error),
        compiler::CompilerFailure::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
    }
}
fn runtime_failure(error: crate::product_contract::AdapterError) -> ScalarReplayFailure {
    match error {
        crate::product_contract::AdapterError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
        crate::product_contract::AdapterError::Invalid(error) => ScalarReplayFailure::Store(StoreError::Invalid(error.0)),
        _ => ScalarReplayFailure::Store(compiler::error("history projection evaluation failed")),
    }
}
fn data_bytes(value: &FallibleDecodedDataSnapshot, cx: &mut AllocationContext) -> Result<Vec<u8>, ScalarReplayFailure> {
    crate::product_backup::fallible_codec::canonical_bytes(value, cx).map_err(codec_error)
}
fn has_protected_fields(program: &FallibleDecodedCapturedProgram) -> bool {
    program.app().entities.iter().any(|entity| entity.fields.iter().any(|field| field.id.starts_with("gm_scope_")))
}
fn decoded_app_has_reserved_namespace(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    cx: &mut AllocationContext,
) -> Result<bool, ScalarReplayFailure> {
    fn contains(value: &crate::product_json::Value) -> bool {
        use crate::product_json::Value;
        match value {
            Value::String(value) => value.starts_with("gm_scope_"),
            Value::Array(values) => values.iter().any(contains),
            Value::Object(values) => values.iter().any(|(key, value)| key.starts_with("gm_scope_") || contains(value)),
            _ => false,
        }
    }
    let value = crate::product_backup::fallible_codec::try_serialize_value(app, cx).map_err(codec_error)?;
    Ok(contains(&value))
}
fn copy_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, ScalarReplayFailure> {
    let text = product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    Ok(Digest::from_owned_lower_hex(text).expect("stored digest is valid"))
}

/// Immutable-input scalar replay registry and its independently regenerated
/// correspondence proofs. It deliberately has no serialization implementation.
pub(crate) struct FallibleScalarReplayContext {
    snapshot: DecodedProjectSnapshot,
    adoption_target: Digest,
    projected_seeds: OrderedSet<Digest>,
    correspondences: OrderedMap<Digest, DecodedVerifiedCorrespondence>,
}

impl FallibleScalarReplayContext {
    /// Validate the decoded container and construct the exact rehearsal replay
    /// registry before exposing any proof or projected-seed view.
    pub(crate) fn registry(
        snapshot: &DecodedProjectSnapshot,
        cx: &mut AllocationContext,
    ) -> Result<Self, ScalarReplayFailure> {
        snapshot.validate_container_with(cx).map_err(store_error)?;
        if snapshot.scalar_scope().is_none() {
            return Err(compiler::error("scalar replay context requires scalar scope").into());
        }
        let replay = replay_snapshot_fallible(snapshot, cx).map_err(replay_error)?;
        let mut projected_seeds = OrderedSet::new();
        if let Some(scope) = snapshot.scalar_scope() {
            for (_, rehearsal) in scope.rehearsals() {
                let seed = copy_digest(rehearsal.seed(), cx)?;
                projected_seeds.try_insert(seed, cx)?;
            }
        }
        Ok(Self {
            adoption_target: copy_digest(snapshot.active_revision(), cx)?,
            snapshot: replay,
            projected_seeds,
            correspondences: OrderedMap::new(),
        })
    }

    /// Rebuild and authenticate every persisted scalar correspondence in
    /// dependency order. Semantic failures are deferred while later proofs may
    /// establish their input frames; AllocationContext refusal is never hidden.
    pub(crate) fn with_correspondences(
        mut self,
        cx: &mut AllocationContext,
    ) -> Result<Self, ScalarReplayFailure> {
        let scope = self.snapshot.scalar_scope().ok_or_else(|| compiler::error("scalar replay context requires scalar scope"))?;
        if scope.correspondences().size_hint().0 > crate::product_contract::MAX_ITEMS {
            return Err(compiler::error("correspondence inventory exceeds bounds").into());
        }
        let mut pending = OrderedMap::new();
        for (id, receipt) in scope.correspondences() {
            if pending.len() >= crate::product_contract::MAX_ITEMS {
                return Err(compiler::error("correspondence inventory exceeds bounds").into());
            }
            let id = copy_digest(id, cx)?;
            let proof = receipt.proof().copy_with(cx).map_err(decode_error)?;
            pending.try_insert(id, proof, cx)?;
        }
        while !pending.is_empty() {
            let mut progressed = false;
            let mut index = 0;
            while index < pending.len() {
                let (key, proof_ref) = pending.get_index(index).expect("pending index is valid");
                let id = copy_digest(key, cx)?;
                let proof = proof_ref.copy_with(cx).map_err(decode_error)?;
                let proof_id = proof.identity_with(cx).map_err(codec_error)?;
                if proof_id != id {
                    return Err(compiler::error("correspondence digest changed").into());
                }
                let source = proof.source();
                let target = self.snapshot.captured_program_by_revision_with(proof.target(), cx)
                    .map_err(app_error)?
                    .ok_or_else(|| compiler::error("retained program revision missing"))?;
                let original_id = proof.original().identity_with(cx).map_err(app_error)?;
                let mapped = proof.original().merge_app_with(target.app(), cx).map_err(app_error)?;

                if let Some(pair) = proof.scenario() {
                    if pair.original().seed_with(cx).map_err(app_error)?.identity_with(cx).map_err(app_error)? != original_id
                        || pair.original().clock_day() != proof.day()
                    {
                        return Err(compiler::error("correspondence scenario differs from its original input").into());
                    }
                    let mapped_scenario = pair.projected().with_seed_and_id(&mapped, None, cx).map_err(app_error)?;
                    match self.project_scenario(source, pair.original(), target, &mapped_scenario, cx) {
                        Ok((context, actual)) => {
                            self = context;
                            let actual_bytes = scenario_bytes(&actual, cx)?;
                            let expected_bytes = scenario_bytes(pair.projected(), cx)?;
                            if actual_bytes != expected_bytes {
                                return Err(compiler::error("projected scenario does not regenerate").into());
                            }
                            let Some(checked) = self.correspondences.get(&proof_id) else { index += 1; continue };
                            if proof_bytes(checked.proof(), cx)? != proof_bytes(&proof, cx)? {
                                return Err(compiler::error("correspondence does not independently regenerate").into());
                            }
                        }
                        Err(ScalarReplayFailure::Allocation(failure)) => return Err(ScalarReplayFailure::Allocation(failure)),
                        Err(ScalarReplayFailure::Store(_)) => { index += 1; continue; }
                    }
                } else {
                    match self.project_seed(source, proof.original(), target, &mapped, proof.day(), cx) {
                        Ok((context, _)) => {
                            self = context;
                            let Some(checked) = self.correspondences.get(&proof_id) else { index += 1; continue };
                            if proof_bytes(checked.proof(), cx)? != proof_bytes(&proof, cx)? {
                                return Err(compiler::error("correspondence does not independently regenerate").into());
                            }
                        }
                        Err(ScalarReplayFailure::Allocation(failure)) => return Err(ScalarReplayFailure::Allocation(failure)),
                        Err(ScalarReplayFailure::Store(_)) => { index += 1; continue; }
                    }
                }
                pending.remove(&id);
                progressed = true;
                // `remove` shifted the next lexical entry into this slot.
            }
            if !progressed {
                return Err(compiler::error("historical correspondence is forged, cyclic or lacks an authenticated original input").into());
            }
        }
        Ok(self)
    }

    pub(crate) fn projected_seed(&self, seed: &Digest) -> bool { self.projected_seeds.contains(seed) }

    pub(crate) fn frames_for_source_seed(
        &self,
        source_revision: &Digest,
        seed: &Digest,
    ) -> Option<&[DecodedReplayFrame]> {
        self.correspondences.iter().find(|(_, checked)| {
            checked.proof().target() == source_revision && checked.proof().projected() == seed
        }).map(|(_, checked)| checked.frames())
    }

    pub(crate) fn correspondence_proofs(&self) -> impl Iterator<Item = (&Digest, &DecodedScopeCorrespondence)> {
        self.correspondences.iter().map(|(id, checked)| (id, checked.proof()))
    }

    pub(crate) fn snapshot(&self) -> &DecodedProjectSnapshot { &self.snapshot }

    #[cfg(test)]
    pub(crate) fn strip_history_evidence_for_test(&mut self) {
        if let Some(scope) = self.snapshot.scalar_scope_mut() {
            scope.clear_initializations_for_test();
        }
        for (_, checked) in self.correspondences.iter_entries_mut() {
            checked.clear_frames_for_test();
        }
    }

    pub(crate) fn project_seed(
        &self,
        source: &FallibleDecodedCapturedProgram,
        original: &FallibleDecodedDataSnapshot,
        target: &FallibleDecodedCapturedProgram,
        mapped: &FallibleDecodedDataSnapshot,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<(Self, FallibleDecodedDataSnapshot), ScalarReplayFailure> {
        self.verify_seed(source, original, day, cx)?;
        let original_id = original.identity_with(cx).map_err(app_error)?;
        let authenticated = self.known_seed(&original_id, cx)?;
        if has_protected_fields(target) && !authenticated {
            return Err(compiler::error("an unverified raw seed cannot grant scope metadata authority").into());
        }
        let expected = original.merge_app_with(target.app(), cx).map_err(app_error)?;
        if data_bytes(&expected, cx)? != data_bytes(mapped, cx)? {
            return Err(compiler::error("scope scene mapping changed protected seed facts").into());
        }

        let mut initialized = original.try_clone_with(cx).map_err(data_clone_error)?;
        let target_id = target.revision_with(cx).map_err(app_error)?;
        let scope = self.snapshot.scalar_scope().ok_or_else(|| compiler::error("scalar replay context requires scalar scope"))?;
        let mut lineage = Vec::new();
        let mut seen = OrderedSet::new();
        let mut next = Some(copy_digest(&target_id, cx)?);
        while let Some(id) = next {
            if !seen.try_insert(copy_digest(&id, cx)?, cx)? {
                return Err(compiler::error("cyclic scope projection lineage").into());
            }
            let Some(manifest) = scope.composition(&id) else { break };
            if lineage.len() >= super::contract::MAX_LAYERS {
                return Err(compiler::error("scope projection lineage exceeds bounds").into());
            }
            product_alloc::try_reserve(&mut lineage, 1, cx, AllocationSite::Vector)?;
            lineage.push(copy_digest(&id, cx)?);
            next = manifest.previous().map(|previous| copy_digest(previous, cx)).transpose()?;
        }
        for id in lineage.iter().rev() {
            let manifest = scope.composition(id).expect("lineage entry was present");
            if initialized.identity_with(cx).map_err(app_error)? != manifest.basis().data().identity_with(cx).map_err(app_error)?
                || (manifest.transition() == super::ScopeTransition::Adoption && day != manifest.basis().day())
            { continue; }
            let transition_target = self.snapshot.captured_program_by_revision_with(manifest.output(), cx)
                .map_err(app_error)?
                .ok_or_else(|| compiler::error("retained program revision missing"))?;
            initialized = match manifest.transition() {
                super::ScopeTransition::Adoption => {
                    let layer_id = manifest.layers().last().ok_or_else(|| compiler::error("initialization layer missing"))?;
                    let layer = scope.layer(layer_id).ok_or_else(|| compiler::error("initialization layer missing"))?;
                    let (after, additions) = super::initialize_scalar_fallible(&self.snapshot, layer, transition_target, cx)
                        .map_err(initialization_error)?;
                    let receipt = super::scalar_initialization_receipt(&self.snapshot, layer, layer.basis().data(), &after, &additions, cx)
                        .map_err(initialization_error)?;
                    let receipt_bytes = crate::product_backup::fallible_codec::canonical_bytes(&receipt, cx).map_err(codec_error)?;
                    let mut receipt_found = false;
                    for stored in scope.initializations() {
                        let stored_bytes = crate::product_backup::fallible_codec::canonical_bytes(stored, cx).map_err(codec_error)?;
                        if stored_bytes == receipt_bytes { receipt_found = true; break; }
                    }
                    if !receipt_found {
                        return Err(compiler::error("initialization projection differs from retained receipt").into());
                    }
                    after
                }
                super::ScopeTransition::Evolution | super::ScopeTransition::Withdrawal => {
                    initialized.merge_app_with(transition_target.app(), cx).map_err(app_error)?
                }
            };
        }

        let mut result = self.try_clone_with(cx)?;
        if authenticated {
            let projected = initialized.identity_with(cx).map_err(app_error)?;
            result.projected_seeds.try_insert(copy_digest(&projected, cx)?, cx)?;
        }
        if let Err(error) = result.verify_seed(target, &initialized, day, cx) {
            match error {
                ScalarReplayFailure::Allocation(failure) => return Err(ScalarReplayFailure::Allocation(failure)),
                ScalarReplayFailure::Store(_) => {
                    let source_id = source.revision_with(cx).map_err(app_error)?;
                    let prior_frames = self.frames_for_source_seed(&source_id, &original_id).unwrap_or(&[]);
                    match derive_correspondence_with_fallible(&self.snapshot, prior_frames, source, original, target, day, &initialized, cx) {
                        Ok(checked) => {
                            initialized = checked.seed().try_clone_with(cx).map_err(data_clone_error)?;
                            result.insert_correspondence(checked, cx)?;
                            let projected_id = initialized.identity_with(cx).map_err(app_error)?;
                            if !result.projected_seeds.contains(&projected_id) {
                                result.projected_seeds.try_insert(copy_digest(&projected_id, cx)?, cx)?;
                            }
                        }
                        Err(error) => return Err(correspondence_error(error)),
                    }
                }
            }
        } else if data_bytes(&initialized, cx)? != data_bytes(original, cx)? {
            let operation_seed = original.try_clone_with(cx).map_err(data_clone_error)?;
            let operation_seed_id = operation_seed.identity_with(cx).map_err(app_error)?;
            let proof = DecodedScopeCorrespondence::new(
                source.try_clone_with(cx).map_err(app_error)?,
                original.try_clone_with(cx).map_err(data_clone_error)?,
                copy_digest(&target_id, cx)?, day,
                initialized.identity_with(cx).map_err(app_error)?,
                operation_seed_id,
            );
            let frames = self.frames_for_source_seed(&target_id, &initialized.identity_with(cx).map_err(app_error)?)
                .unwrap_or(&[]);
            let mut copied_frames = Vec::new();
            product_alloc::try_reserve(&mut copied_frames, frames.len(), cx, AllocationSite::Vector)?;
            for frame in frames {
                product_alloc::try_push(&mut copied_frames, frame.copy_with(cx).map_err(correspondence_error)?, cx, AllocationSite::Vector)?;
            }
            result.insert_correspondence(DecodedVerifiedCorrespondence::new_checked(
                proof, initialized.try_clone_with(cx).map_err(data_clone_error)?, copied_frames, operation_seed,
            ), cx)?;
        }
        result.verify_seed(target, &initialized, day, cx)?;
        Ok((result, initialized))
    }

    pub(crate) fn project_scenario(
        &self,
        source: &FallibleDecodedCapturedProgram,
        original: &crate::product_contract::FallibleDecodedScenarioSpec,
        target: &FallibleDecodedCapturedProgram,
        mapped: &crate::product_contract::FallibleDecodedScenarioSpec,
        cx: &mut AllocationContext,
    ) -> Result<(Self, crate::product_contract::FallibleDecodedScenarioSpec), ScalarReplayFailure> {
        original.validate_with(source, cx).map_err(app_error)?;
        mapped.validate_with(target, cx).map_err(app_error)?;
        if original.clock_day() != mapped.clock_day() || original.random_seed() != mapped.random_seed() {
            return Err(compiler::error("historical correspondence changed the replay clock or seed").into());
        }
        let original_seed = original.seed_with(cx).map_err(app_error)?;
        let mapped_seed = mapped.seed_with(cx).map_err(app_error)?;
        let (mut context, seed) = self.project_seed(source, &original_seed, target, &mapped_seed, mapped.clock_day(), cx)?;
        let original_id = original_seed.identity_with(cx).map_err(app_error)?;
        let projected_id = seed.identity_with(cx).map_err(app_error)?;
        let source_id = source.revision_with(cx).map_err(app_error)?;
        let target_id = target.revision_with(cx).map_err(app_error)?;
        let mut actual = mapped.with_seed_and_id(&seed, None, cx).map_err(app_error)?;
        let mut matching = None;
        for (id, checked) in context.correspondences.iter() {
            if checked.proof().source().revision_with(cx).map_err(app_error)? == source_id
                && checked.proof().target() == &target_id
                && checked.proof().projected() == &projected_id
                && checked.proof().original().identity_with(cx).map_err(app_error)? == original_id
            {
                matching = Some((copy_digest(id, cx)?, checked.copy_with(cx).map_err(correspondence_error)?));
                break;
            }
        }
        if let Some((checked_id, mut checked)) = matching {
            let namespace = crate::product_contract::FallibleDecodedScenarioSpec::scope_replay_namespace_with(
                original, mapped, source, target, cx,
            ).map_err(codec_error)?;
            let mut synthetic = product_alloc::try_copy_str("scope-replay-", cx, AllocationSite::String)?;
            product_alloc::try_append_fmt(&mut synthetic, format_args!("{}", &namespace.as_str()[..40]), cx, AllocationSite::String)
                .map_err(|failure| match failure {
                    product_alloc::FormatFailure::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
                    product_alloc::FormatFailure::Formatter => ScalarReplayFailure::Store(StoreError::Invalid("scenario namespace formatting invariant".into())),
                })?;
            actual = mapped.with_seed_and_id(&seed, Some(&synthetic), cx).map_err(app_error)?;
            let operation_seed = self.operation_seed_for_scenario(source, original, cx)?;
            checked.bind_scenario(
                original.try_clone_with(cx).map_err(app_error)?,
                actual.try_clone_with(cx).map_err(app_error)?, operation_seed, cx,
            ).map_err(correspondence_error)?;
            let regenerated_id = checked.proof().identity_with(cx).map_err(codec_error)?;
            if regenerated_id != checked_id {
                context.insert_correspondence(checked, cx)?;
            }
        }
        Ok((context, actual))
    }

    pub(crate) fn has_scenario_correspondence(
        &self,
        scenario: &crate::product_contract::FallibleDecodedScenarioSpec,
    ) -> bool {
        self.correspondences.iter().any(|(_, checked)| {
            checked.proof().scenario().is_some_and(|pair| pair.matches_frame(scenario))
        })
    }

    fn operation_seed_for_scenario(
        &self,
        source: &FallibleDecodedCapturedProgram,
        scenario: &crate::product_contract::FallibleDecodedScenarioSpec,
        cx: &mut AllocationContext,
    ) -> Result<FallibleDecodedDataSnapshot, ScalarReplayFailure> {
        source.validate_with(cx).map_err(app_error)?;
        for (_, checked) in self.correspondences.iter() {
            let Some(pair) = checked.proof().scenario() else { continue };
            if pair.matches_frame(scenario) {
                return checked.operation_seed().try_clone_with(cx).map_err(data_clone_error);
            }
        }
        for (_, checked) in self.correspondences.iter() {
            if checked.proof().scenario().is_some_and(|pair| pair.projected().id() == scenario.id()) {
                return Err(compiler::error("projected scenario changed its authenticated input frame; operation identity is unverified").into());
            }
        }
        scenario.seed_with(cx).map_err(app_error)
    }

    fn try_clone_with(&self, cx: &mut AllocationContext) -> Result<Self, ScalarReplayFailure> {
        let snapshot = self.snapshot.try_clone_with(cx).map_err(copy_error)?;
        let mut projected_seeds = OrderedSet::new();
        projected_seeds.try_reserve(self.projected_seeds.len(), cx)?;
        for digest in self.projected_seeds.iter() {
            projected_seeds.try_insert(copy_digest(digest, cx)?, cx)?;
        }
        let mut correspondences = OrderedMap::new();
        correspondences.try_reserve(self.correspondences.len(), cx)?;
        for (id, checked) in self.correspondences.iter() {
            correspondences.try_insert(copy_digest(id, cx)?, checked.copy_with(cx).map_err(correspondence_error)?, cx)?;
        }
        Ok(Self { snapshot, adoption_target: copy_digest(&self.adoption_target, cx)?, projected_seeds, correspondences })
    }

    pub(crate) fn verify_seed(
        &self,
        target: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<(), ScalarReplayFailure> {
        target.validate_with(cx).map_err(app_error)?;
        data.validate_with(cx).map_err(app_error)?;
        crate::product_contract::validate_day(day).map_err(|error| StoreError::Invalid(error.0))?;
        if target.project_id() != self.snapshot.data().project_id() || data.project_id() != self.snapshot.data().project_id() {
            return Err(compiler::error("scope replay belongs to another project").into());
        }
        let revision = target.revision_with(cx).map_err(app_error)?;
        if self.snapshot.scalar_scope().and_then(|scope| scope.composition(&revision)).is_none() {
            if decoded_app_has_reserved_namespace(target.app(), cx)? {
                return Err(compiler::error("authored source uses the reserved metadata namespace").into());
            }
            return Ok(());
        }
        let retained = self.snapshot.captured_program_by_revision_with(&revision, cx)
            .map_err(app_error)?
            .ok_or_else(|| compiler::error("retained program revision missing"))?;
        if crate::product_backup::fallible_codec::canonical_bytes(retained, cx).map_err(codec_error)?
            != crate::product_backup::fallible_codec::canonical_bytes(target, cx).map_err(codec_error)?
        {
            return Err(compiler::error("scope source or project identity mismatch").into());
        }
        let target_has_layers = self.snapshot.scalar_scope()
            .and_then(|scope| scope.composition(&revision))
            .is_some_and(|manifest| !manifest.layers().is_empty());
        if has_protected_fields(target) && target_has_layers {
            let digest = data.identity_with(cx).map_err(app_error)?;
            if !self.known_seed(&digest, cx)? {
                return Err(compiler::error("metadata-bearing scene seed lacks an authenticated frozen snapshot or exact initialization receipt").into());
            }
        }
        let digest = data.identity_with(cx).map_err(app_error)?;
        if !self.known_seed_for_target(data, &digest, &revision, day, cx)? {
            return Err(compiler::error("managed scene seed is not authenticated for its target history").into());
        }
        Ok(())
    }

    /// Check seed identity only against the requested managed target's own
    /// composition lineage and proofs. The broader `known_seed` predicate is
    /// used to decide whether raw protected metadata has any authority at all;
    /// it must not let a basis from an unrelated composition authenticate this
    /// target's replay state.
    fn known_seed_for_target(
        &self,
        data: &FallibleDecodedDataSnapshot,
        digest: &Digest,
        target_revision: &Digest,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<bool, ScalarReplayFailure> {
        let Some(scope) = self.snapshot.scalar_scope() else { return Ok(false) };
        let Some(manifest) = scope.composition(target_revision) else { return Ok(false) };
        if manifest.layers().is_empty() {
            // Legacy verify_history_layers is vacuous for an unlayered managed
            // target. The caller has already validated data, day, project and
            // exact retained-program identity; preserve its arbitrary-seed rule.
            return Ok(true);
        }

        // A current live snapshot can be authenticated by its complete current
        // identity. A stored receipt alone is not authority: validate its
        // regenerated additions and every later event against the rows.
        let target_program = self.snapshot.captured_program_by_revision_with(target_revision, cx)
            .map_err(app_error)?.ok_or_else(|| compiler::error("retained program revision missing"))?;
        let mut live_frame = true;
        for layer_id in manifest.layers() {
            if !self.verify_layer_history(data, scope, layer_id, None, target_program, day, cx)? {
                live_frame = false;
                break;
            }
        }
        if live_frame {
            if self.snapshot.active_revision() == target_revision
                && self.snapshot.data().identity_with(cx).map_err(app_error)? == *digest
            {
                return Ok(true);
            }
            if let Some(final_layer) = manifest.layers().last() {
                if scope.initializations().iter().any(|receipt| receipt.layer() == final_layer && receipt.after_data() == digest) {
                    return Ok(true);
                }
            }
            let target_manifest = crate::product_backup::fallible_codec::canonical_bytes(manifest, cx).map_err(codec_error)?;
            for (_, rehearsal) in scope.rehearsals() {
                let rehearsal_manifest = rehearsal.manifest();
                if rehearsal.seed() != digest
                    || rehearsal_manifest.output() != target_revision
                    || rehearsal_manifest.transition() != super::ScopeTransition::Evolution
                    || rehearsal.layer().is_some()
                    || rehearsal.initialization().is_some()
                    || rehearsal.version() != super::contract::COMPILER_VERSION
                    || rehearsal_manifest.previous() != Some(rehearsal_manifest.basis().active())
                    || rehearsal_manifest.basis().revision().checked_add(1) != Some(rehearsal.recorded_revision())
                {
                    continue;
                }
                if crate::product_backup::fallible_codec::canonical_bytes(rehearsal_manifest, cx).map_err(codec_error)? != target_manifest {
                    continue;
                }
                let Some(predecessor) = scope.composition(rehearsal_manifest.basis().active()) else { continue };
                if predecessor.output() != rehearsal_manifest.basis().active()
                    || !rehearsal_manifest.same_layer_inventory_as(predecessor)
                {
                    continue;
                }
                let Some(candidate) = self.snapshot.captured_program_by_revision_with(rehearsal_manifest.business(), cx).map_err(app_error)? else {
                    continue;
                };
                match compiler::validate_mappings_fallible(
                    &self.snapshot, predecessor, candidate, rehearsal_manifest.rewrites(), cx,
                ) {
                    Ok(()) => {}
                    Err(compiler::CompilerFailure::Allocation(failure)) => return Err(ScalarReplayFailure::Allocation(failure)),
                    Err(compiler::CompilerFailure::Store(_)) => continue,
                }
                let regenerated_target = match compiler::compile_fallible(&self.snapshot, rehearsal_manifest, cx) {
                    Ok(program) => program,
                    Err(compiler::CompilerFailure::Allocation(failure)) => return Err(ScalarReplayFailure::Allocation(failure)),
                    Err(compiler::CompilerFailure::Store(_)) => continue,
                };
                if crate::product_backup::fallible_codec::canonical_bytes(&regenerated_target, cx).map_err(codec_error)?
                    != crate::product_backup::fallible_codec::canonical_bytes(target_program, cx).map_err(codec_error)?
                {
                    continue;
                }
                // A retained rehearsal's digest is only a claim. Rebuild the
                // Evolution seed from this exact target and frozen basis so a
                // forged or cross-composition rehearsal cannot authenticate
                // arbitrary business data.
                let regenerated_seed = rehearsal_manifest.basis().data()
                    .merge_app_with(target_program.app(), cx).map_err(app_error)?;
                if regenerated_seed.identity_with(cx).map_err(app_error)? == *digest {
                    return Ok(true);
                }
            }
        }
        for (_, checked) in self.correspondences.iter() {
            if checked.proof().target() == target_revision
                && checked.proof().projected() == digest
                && day >= checked.proof().day()
                && data.generation() >= checked.seed().generation()
                && data.events_start_with(checked.seed())
            {
                let mut complete_frames = true;
                for layer in manifest.layers() {
                    let Some(frame) = checked.frames().iter().find(|frame| frame.layer() == layer) else {
                        complete_frames = false;
                        break;
                    };
                    if !self.verify_layer_history(data, scope, layer, Some(frame), target_program, day, cx)? {
                        complete_frames = false;
                        break;
                    }
                }
                if complete_frames { return Ok(true); }
            }
        }
        Ok(false)
    }

    fn verify_layer_history(
        &self,
        data: &FallibleDecodedDataSnapshot,
        scope: &FallibleDecodedScopeState,
        layer_id: &Digest,
        frame: Option<&DecodedReplayFrame>,
        target_program: &FallibleDecodedCapturedProgram,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<bool, ScalarReplayFailure> {
        let layer = scope.layer(layer_id).ok_or_else(|| compiler::error("history layer proof missing"))?;
        let Some(receipt) = scope.initializations().iter().find(|receipt| receipt.layer() == layer_id) else {
            return Ok(false);
        };
        let regenerated_live_additions;
        let (basis, additions) = if let Some(frame) = frame {
            if frame.layer() != layer_id
                || !frame.before().events_start_with(layer.basis().data())
                || frame.before().generation() < layer.basis().data().generation()
            {
                return Ok(false);
            }
            (frame.before(), frame.additions())
        } else {
            // The live path is trusted only if its receipt can be regenerated
            // from the frozen basis and the exact adoption target recorded by
            // this receipt. Later adoptions may inherit the same layer but
            // have a different app schema, so layer membership alone is not
            // enough to select the regeneration program.
            let initialization_target = scope.compositions().find_map(|(_, manifest)| {
                (manifest.transition() == super::ScopeTransition::Adoption
                    && manifest.layers().contains(layer_id)
                    && manifest.basis().snapshot() == receipt.prior_snapshot()
                    && manifest.basis().active() == layer.basis().active()
                    && manifest.basis().revision().checked_add(1) == Some(receipt.adoption_revision()))
                    .then_some(manifest.output())
            });
            let Some(initialization_target) = initialization_target else { return Ok(false) };
            let Some(initialization_source) = self.snapshot.captured_program_by_revision_with(layer.basis().active(), cx)
                .map_err(app_error)? else { return Ok(false) };
            if initialization_source.program_digest() != receipt.capture_program() {
                return Ok(false);
            }
            let initialization_program = self.snapshot.captured_program_by_revision_with(initialization_target, cx)
                .map_err(app_error)?.ok_or_else(|| compiler::error("initialization target program missing"))?;
            let (initialized, regenerated_additions) = super::initialize_scalar_fallible(&self.snapshot, layer, initialization_program, cx)
                .map_err(initialization_error)?;
            let regenerated = super::scalar_initialization_receipt(
                &self.snapshot, layer, layer.basis().data(), &initialized, &regenerated_additions, cx,
            ).map_err(initialization_error)?;
            if crate::product_backup::fallible_codec::canonical_bytes(&regenerated, cx).map_err(codec_error)?
                != crate::product_backup::fallible_codec::canonical_bytes(receipt, cx).map_err(codec_error)?
            {
                return Ok(false);
            }
            // The receipt output itself must be an exact prefix of the live
            // state; later records/events are checked below from its frozen
            // basis and regenerated additions.
            if !data.events_start_with(&initialized) || data.generation() < initialized.generation() {
                return Ok(false);
            }
            regenerated_live_additions = regenerated_additions;
            (layer.basis().data(), regenerated_live_additions.as_slice())
        };
        if !data.events_start_with(basis) || data.generation() < basis.generation() || day < layer.basis().day() {
            return Ok(false);
        }
        // Metadata keys contain an entity component, so construct them for
        // each lifecycle row below.
        for lifecycle in layer.request().lifecycles() {
            let mut row_keys = OrderedSet::new();
            super::fallible_correspondence::metadata_keys(layer_id, layer, lifecycle.entity(), &mut row_keys, cx)
                .map_err(correspondence_error)?;
            let member = compiler::key_fallible(layer_id, lifecycle.entity(), "member", cx).map_err(compiler_failure)?;
            let sealed = compiler::key_fallible(layer_id, lifecycle.entity(), "sealed", cx).map_err(compiler_failure)?;
            for row in data.records().iter().filter(|row| row.entity() == lifecycle.entity()) {
                let before_row = basis.records().iter().find(|record| record.entity() == row.entity() && record.id() == row.id());
                let row_ref = (row.entity(), row.id());
                if before_row.is_some_and(|original| {
                    original.created_program() != row.created_program()
                        || row.revision() < original.revision()
                        || (original.archived() && (!row.archived() || row.revision() != original.revision()))
                }) {
                    return Ok(false);
                }
                let mut expected: Option<&crate::product_contract::FallibleValues> = if before_row.is_some() {
                    let Some(addition) = additions.iter().find(|addition| addition.record().entity == row_ref.0 && addition.record().record == row_ref.1) else {
                        return Ok(false);
                    };
                    Some(addition.values())
                } else { None };
                let mut business = before_row.map(|record| record.values());
                let mut archived = before_row.is_some_and(|record| record.archived());
                let mut births = 0usize;
                for event in data.events().iter().filter(|event| event.sequence() > basis.generation()) {
                    for change in event.changes().iter().filter(|change| change.entity() == row.entity() && change.record() == row.id()) {
                        if archived || !same_business(change.before(), business) {
                            return Ok(false);
                        }
                        let Some(source) = self.producer_composition(event.program(), cx)? else { return Ok(false) };
                        let source_program = self.snapshot.captured_program_by_revision_with(source.output(), cx)
                            .map_err(app_error)?.ok_or_else(|| compiler::error("retained program revision missing"))?;
                        let before_meta = change.before();
                        let after_meta = change.after();
                        if before_meta.is_none() {
                            births += 1;
                            if before_row.is_some() || births != 1 || row.created_program() != event.program() {
                                return Ok(false);
                            }
                            let stamped = source.active_contains(layer_id)
                                && matches!(layer.request().population, super::ScopePopulationDto::FutureWork | super::ScopePopulationDto::All);
                            if after_meta.get(&member) != Some(&crate::product_contract::DataValue::Boolean { value: stamped })
                                || after_meta.get(&sealed) != Some(&crate::product_contract::DataValue::Boolean { value: false })
                                || row_keys.iter().any(|key| key != &member && key != &sealed && after_meta.get(key).is_some())
                            { return Ok(false); }
                            expected = Some(after_meta);
                        } else {
                            let Some(prior) = expected else { return Ok(false) };
                            if !same_metadata(before_meta.expect("checked Some"), prior, &row_keys) {
                                return Ok(false);
                            }
                            if before_meta.expect("checked Some").get(&member) != after_meta.get(&member) {
                                return Ok(false);
                            }
                            let was_sealed = prior.get(&sealed) == Some(&crate::product_contract::DataValue::Boolean { value: true });
                            let is_sealed = after_meta.get(&sealed) == Some(&crate::product_contract::DataValue::Boolean { value: true });
                            if was_sealed && !same_metadata(after_meta, prior, &row_keys) { return Ok(false); }
                            if !is_sealed && !same_metadata(after_meta, prior, &row_keys) { return Ok(false); }
                            if was_sealed {
                                for patch in layer.patches() {
                                    if patch.request().entity != row.entity() { continue; }
                                    if let super::EffectDestinationDto::Update { field, .. }
                                        | super::EffectDestinationDto::CreateValue { field, .. } = &patch.request().destination
                                    {
                                        if before_meta.expect("checked Some").get(field).unwrap_or(&crate::product_contract::DataValue::Null)
                                            != after_meta.get(field).unwrap_or(&crate::product_contract::DataValue::Null)
                                        { return Ok(false); }
                                    }
                                }
                            }
                            if is_sealed && !was_sealed {
                                let day_key = compiler::key_fallible(layer_id, row.entity(), "day", cx).map_err(compiler_failure)?;
                                let origin_key = compiler::key_fallible(layer_id, row.entity(), "origin", cx).map_err(compiler_failure)?;
                                let completed = if change.archived() { false } else {
                                    self.completed_projection(source_program, data, row.entity(), row.id(), after_meta, false, lifecycle, event.day(), cx)?
                                };
                                let archive_origin = matches!(after_meta.get(&origin_key), Some(crate::product_contract::DataValue::Text { value }) if value == "ObservedAtArchive");
                                if after_meta.get(&day_key) != Some(&crate::product_contract::DataValue::Date { days: event.day() })
                                    || (archive_origin && !change.archived())
                                    || (!archive_origin && (change.archived() || !completed
                                        || !matches!(after_meta.get(&origin_key), Some(crate::product_contract::DataValue::Text { value }) if value == "ObservedAtCompletion")))
                                { return Ok(false); }
                                if !source.layers().contains(layer_id) { return Ok(false); }
                                let mut projection_values = after_meta.try_clone_with(cx).map_err(ScalarReplayFailure::Allocation)?;
                                for other_id in source.layers() {
                                    let Some(other_layer) = scope.layer(other_id) else { continue };
                                    if !other_layer.request().lifecycles().iter().any(|item| item.entity() == row.entity()) { continue; }
                                    let other_sealed = compiler::key_fallible(other_id, row.entity(), "sealed", cx).map_err(compiler_failure)?;
                                    let prior_sealed = before_meta.expect("checked Some").get(&other_sealed)
                                        == Some(&crate::product_contract::DataValue::Boolean { value: true });
                                    if !prior_sealed {
                                        let mut other_keys = OrderedSet::new();
                                        super::fallible_correspondence::metadata_keys(other_id, other_layer, row.entity(), &mut other_keys, cx)
                                            .map_err(correspondence_error)?;
                                        let other_member = compiler::key_fallible(other_id, row.entity(), "member", cx).map_err(compiler_failure)?;
                                        for key in other_keys.iter() {
                                            if key != &other_member { projection_values.remove(key); }
                                        }
                                        let key = product_alloc::try_copy_str(&other_sealed, cx, AllocationSite::String)?;
                                        projection_values.try_insert(key, crate::product_contract::DataValue::Boolean { value: false }, cx)?;
                                    }
                                }
                                let producer_app = compiler::envelope_app_fallible(&self.snapshot, source, cx)
                                    .map_err(compiler_failure)?;
                                for (index, original_patch) in layer.patches().iter().enumerate() {
                                    if original_patch.request().entity != row.entity()
                                        || !matches!(original_patch.request().destination, super::EffectDestinationDto::ViewColumn { .. } | super::EffectDestinationDto::EmitColumn { .. }) { continue; }
                                    let effective = compiler::effective_patch_fallible(&self.snapshot, source, layer_id, index, cx)
                                        .map_err(compiler_failure)?;
                                    let saved = compiler::saved_key_fallible(layer_id, original_patch, cx).map_err(compiler_failure)?;
                                    let Some(value) = after_meta.get(&saved) else { return Ok(false) };
                                    crate::product_contract::validate_value(value, &original_patch.request().value_type, 0)
                                        .map_err(|error| ScalarReplayFailure::Store(StoreError::Invalid(error.0)))?;
                                    let expression = compiler::slot_fallible_ref(&producer_app, &effective.request.destination)
                                        .map_err(ScalarReplayFailure::Store)?;
                                    let observed = self.historical_projection(
                                        source_program, data, row.entity(), row.id(), &projection_values,
                                        change.archived(), &effective.request.subject, expression,
                                        &effective.request.value_type, event.day(), cx,
                                    )?;
                                    if value != &observed { return Ok(false); }
                                }
                            }
                            expected = Some(after_meta);
                        }
                        let sealed_now = expected.is_some_and(|values| values.get(&sealed) == Some(&crate::product_contract::DataValue::Boolean { value: true }));
                        let completed = if change.archived() { false } else {
                            self.completed_projection(source_program, data, row.entity(), row.id(), change.after(), false, lifecycle, event.day(), cx)?
                        };
                        if (change.archived() || completed) && !sealed_now { return Ok(false); }
                        business = Some(after_meta);
                        archived = change.archived();
                    }
                }
                let Some(expected) = expected else { return Ok(false) };
                if !same_metadata(row.values(), expected, &row_keys)
                    || !same_business(Some(row.values()), business)
                    || archived != row.archived()
                    || (before_row.is_none() && births != 1)
                { return Ok(false); }
                let is_sealed = expected.get(&sealed) == Some(&crate::product_contract::DataValue::Boolean { value: true });
                let completed = if row.archived() { false } else {
                    self.completed_projection(target_program, data, row.entity(), row.id(), row.values(), row.archived(), lifecycle, day, cx)?
                };
                if (is_sealed && !row.archived() && !completed)
                    || (!is_sealed && (row.archived() || completed))
                { return Ok(false); }
            }
            for original in basis.records().iter().filter(|record| record.entity() == lifecycle.entity()) {
                if !data.records().iter().any(|record| record.entity() == original.entity() && record.id() == original.id()) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn completed_projection(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        entity: &str,
        record: &str,
        values: &crate::product_contract::FallibleValues,
        archived: bool,
        lifecycle: &super::fallible_scalar_decode::LifecycleBindingDto,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<bool, ScalarReplayFailure> {
        match self.historical_projection(program, data, entity, record, values, archived,
            "record", lifecycle.completed(), &crate::product_contract::Type::Boolean, day, cx)?
        {
            crate::product_contract::DataValue::Boolean { value } => Ok(value),
            _ => Err(compiler::error("completion predicate is not Boolean").into()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn historical_projection(
        &self,
        program: &FallibleDecodedCapturedProgram,
        data: &FallibleDecodedDataSnapshot,
        entity: &str,
        record: &str,
        values: &crate::product_contract::FallibleValues,
        archived: bool,
        binding: &str,
        expression: &crate::product_contract::Expr,
        expected: &crate::product_contract::Type,
        day: i32,
        cx: &mut AllocationContext,
    ) -> Result<crate::product_contract::DataValue, ScalarReplayFailure> {
        let row = crate::product_contract::RecordRef {
            entity: product_alloc::try_copy_str(entity, cx, AllocationSite::String)?,
            record: product_alloc::try_copy_str(record, cx, AllocationSite::String)?,
        };
        let allocations = std::mem::replace(cx, AllocationContext::new());
        let result = crate::product_runtime::LocalRuntime::default()
            .evaluate_decoded_historical_record_projection_and_allocations(
                program, data, &row, values, archived, binding, expression, expected, day,
                crate::product_contract::RuntimeLimits::default(), allocations,
            );
        let (value, allocations) = match result {
            Ok(result) => result,
            Err((error, allocations)) => {
                *cx = allocations;
                return Err(runtime_failure(error));
            }
        };
        *cx = allocations;
        Ok(value)
    }

    fn producer_composition(
        &self,
        program_digest: &Digest,
        cx: &mut AllocationContext,
    ) -> Result<Option<&super::DecodedCompositionManifest>, ScalarReplayFailure> {
        for program in self.snapshot.programs() {
            if program.program_digest() != program_digest { continue; }
            let revision = program.revision_with(cx).map_err(app_error)?;
            if let Some(scope) = self.snapshot.scalar_scope() {
                for (_, manifest) in scope.compositions() {
                    if manifest.output() == &revision { return Ok(Some(manifest)); }
                }
            }
        }
        Ok(None)
    }

    fn known_seed(&self, digest: &Digest, cx: &mut AllocationContext) -> Result<bool, ScalarReplayFailure> {
        if self.projected_seeds.contains(digest) || self.snapshot.data().identity_with(cx).map_err(app_error)? == *digest {
            return Ok(true);
        }
        if self.snapshot.has_adoption_expected_data(digest) {
            return Ok(true);
        }
        let Some(scope) = self.snapshot.scalar_scope() else { return Ok(false) };
        for receipt in scope.initializations() {
            if receipt.after_data() == digest { return Ok(true); }
        }
        for (_, manifest) in scope.compositions() {
            if manifest.basis().data().identity_with(cx).map_err(app_error)? == *digest { return Ok(true); }
            if matches!(manifest.transition(), super::ScopeTransition::Evolution | super::ScopeTransition::Withdrawal) {
                let target = self.snapshot.captured_program_by_revision_with(manifest.output(), cx).map_err(app_error)?
                    .ok_or_else(|| compiler::error("retained program revision missing"))?;
                if manifest.basis().data().merge_app_with(target.app(), cx).map_err(app_error)?.identity_with(cx).map_err(app_error)? == *digest {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub(crate) fn insert_correspondence(
        &mut self,
        checked: DecodedVerifiedCorrespondence,
        cx: &mut AllocationContext,
    ) -> Result<(), ScalarReplayFailure> {
        let id = checked.proof().identity_with(cx).map_err(codec_error)?;
        if self.correspondences.len() >= crate::product_contract::MAX_ITEMS && !self.correspondences.contains_key(&id) {
            return Err(compiler::error("correspondence inventory exceeds bounds").into());
        }
        for (_, prior) in self.correspondences.iter() {
            let same_projected = prior.proof().projected() == checked.proof().projected();
            let differing_frames = prior.proof().target() == checked.proof().target()
                && prior.frames() != checked.frames();
            let different_scenario_operation = if same_projected {
                match (prior.proof().scenario(), checked.proof().scenario()) {
                    (Some(prior_pair), Some(checked_pair)) => {
                        scenario_bytes(prior_pair.projected(), cx)? == scenario_bytes(checked_pair.projected(), cx)?
                            && prior.proof().operation_seed() != checked.proof().operation_seed()
                    }
                    _ => false,
                }
            } else { false };
            if same_projected && (differing_frames || different_scenario_operation) {
                return Err(compiler::error("ambiguous historical correspondence").into());
            }
        }
        let seed = copy_digest(checked.proof().projected(), cx)?;
        self.projected_seeds.try_insert(seed, cx)?;
        self.correspondences.try_insert(id, checked, cx)?;
        Ok(())
    }
}

fn same_metadata(
    actual: &crate::product_contract::FallibleValues,
    expected: &crate::product_contract::FallibleValues,
    keys: &OrderedSet<String>,
) -> bool {
    keys.iter().all(|key| actual.get(key) == expected.get(key))
}

fn same_business(
    actual: Option<&crate::product_contract::FallibleValues>,
    expected: Option<&crate::product_contract::FallibleValues>,
) -> bool {
    match (actual, expected) {
        (None, None) => true,
        (Some(actual), Some(expected)) => {
            actual.iter().filter(|(key, _)| !key.starts_with("gm_scope_")).all(|(key, value)| expected.get(key) == Some(value))
                && expected.iter().filter(|(key, _)| !key.starts_with("gm_scope_")).all(|(key, value)| actual.get(key) == Some(value))
        }
        _ => false,
    }
}

fn replay_error(error: ReplayRegistryError) -> ScalarReplayFailure {
    match error {
        ReplayRegistryError::Store(error) => ScalarReplayFailure::Store(error),
        ReplayRegistryError::Allocation(failure) => ScalarReplayFailure::Allocation(failure),
    }
}
fn proof_bytes(
    proof: &DecodedScopeCorrespondence,
    cx: &mut AllocationContext,
) -> Result<Vec<u8>, ScalarReplayFailure> {
    crate::product_backup::fallible_codec::canonical_bytes(proof, cx).map_err(codec_error)
}
fn scenario_bytes(
    scenario: &crate::product_contract::FallibleDecodedScenarioSpec,
    cx: &mut AllocationContext,
) -> Result<Vec<u8>, ScalarReplayFailure> {
    crate::product_backup::fallible_codec::canonical_bytes(scenario, cx).map_err(codec_error)
}
