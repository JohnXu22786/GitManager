//! Allocation-aware scalar history correspondence derivation over decoded owners.
use super::{compiler, DecodedMetadataAddition, DecodedScopeLayer, StoreError};
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    AppValidationError, DataSnapshotView, Digest, FallibleDecodedCapturedProgram,
    FallibleDecodedDataSnapshot, FallibleDataSnapshotCloneError,
};
use crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot;

#[derive(Debug)]
pub(crate) enum CorrespondenceFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for CorrespondenceFailure {
    fn from(error: StoreError) -> Self { Self::Store(error) }
}
impl From<AllocationFailure> for CorrespondenceFailure {
    fn from(error: AllocationFailure) -> Self { Self::Allocation(error) }
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct DecodedReplayFrame {
    layer: Digest,
    before: FallibleDecodedDataSnapshot,
    additions: Vec<DecodedMetadataAddition>,
}
impl DecodedReplayFrame {
    pub(crate) fn layer(&self) -> &Digest { &self.layer }
    pub(crate) fn before(&self) -> &FallibleDecodedDataSnapshot { &self.before }
    pub(crate) fn additions(&self) -> &[DecodedMetadataAddition] { &self.additions }

    pub(crate) fn copy_with(&self, cx: &mut AllocationContext) -> Result<Self, CorrespondenceFailure> {
        let layer = copy_digest(&self.layer, cx)?;
        let before = self.before.try_clone_with(cx).map_err(data_copy_error)?;
        let mut additions = Vec::new();
        product_alloc::try_reserve(&mut additions, self.additions.len(), cx, AllocationSite::Vector)?;
        for addition in &self.additions {
            let addition = addition.copy_with(cx).map_err(map_decode)?;
            product_alloc::try_push(&mut additions, addition, cx, AllocationSite::Vector)?;
        }
        Ok(Self { layer, before, additions })
    }
}

#[derive(Debug)]
pub(crate) struct DecodedVerifiedCorrespondence {
    proof: super::DecodedScopeCorrespondence,
    seed: FallibleDecodedDataSnapshot,
    frames: Vec<DecodedReplayFrame>,
    operation_seed: FallibleDecodedDataSnapshot,
}
impl DecodedVerifiedCorrespondence {
    pub(crate) fn proof(&self) -> &super::DecodedScopeCorrespondence { &self.proof }
    pub(crate) fn seed(&self) -> &FallibleDecodedDataSnapshot { &self.seed }
    pub(crate) fn frames(&self) -> &[DecodedReplayFrame] { &self.frames }
    #[cfg(test)]
    pub(crate) fn clear_frames_for_test(&mut self) { self.frames.clear(); }
    pub(crate) fn operation_seed(&self) -> &FallibleDecodedDataSnapshot { &self.operation_seed }

    pub(crate) fn new_checked(
        proof: super::DecodedScopeCorrespondence,
        seed: FallibleDecodedDataSnapshot,
        frames: Vec<DecodedReplayFrame>,
        operation_seed: FallibleDecodedDataSnapshot,
    ) -> Self { Self { proof, seed, frames, operation_seed } }

    pub(crate) fn copy_with(&self, cx: &mut AllocationContext) -> Result<Self, CorrespondenceFailure> {
        let proof = self.proof.copy_with(cx).map_err(map_decode)?;
        let seed = self.seed.try_clone_with(cx).map_err(data_copy_error)?;
        let operation_seed = self.operation_seed.try_clone_with(cx).map_err(data_copy_error)?;
        let mut frames = Vec::new();
        product_alloc::try_reserve(&mut frames, self.frames.len(), cx, AllocationSite::Vector)?;
        for frame in &self.frames {
            product_alloc::try_push(&mut frames, frame.copy_with(cx)?, cx, AllocationSite::Vector)?;
        }
        Ok(Self { proof, seed, frames, operation_seed })
    }

    pub(crate) fn bind_scenario(
        &mut self,
        original: crate::product_contract::FallibleDecodedScenarioSpec,
        projected: crate::product_contract::FallibleDecodedScenarioSpec,
        operation_seed: FallibleDecodedDataSnapshot,
        cx: &mut AllocationContext,
    ) -> Result<(), CorrespondenceFailure> {
        let identity = operation_seed.identity_with(cx).map_err(copy_error)?;
        self.proof.bind_scenario(original, projected, identity);
        self.operation_seed = operation_seed;
        Ok(())
    }
}

fn copy_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, CorrespondenceFailure> {
    let text = product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    Ok(Digest::from_owned_lower_hex(text).expect("source digest was already validated"))
}
fn copy_error(error: AppValidationError) -> CorrespondenceFailure {
    match error {
        AppValidationError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
        AppValidationError::Contract(error) => CorrespondenceFailure::Store(StoreError::Invalid(error.0)),
    }
}
fn data_copy_error(error: FallibleDataSnapshotCloneError) -> CorrespondenceFailure {
    match error {
        FallibleDataSnapshotCloneError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
        FallibleDataSnapshotCloneError::Invariant => CorrespondenceFailure::Store(StoreError::Invalid("data snapshot copy invariant".into())),
    }
}
fn map_decode(error: crate::product_contract::fallible_value_decode::ValueDecodeError) -> CorrespondenceFailure {
    match error {
        crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
        _ => CorrespondenceFailure::Store(StoreError::Invalid("correspondence frame copy invariant".into())),
    }
}
fn compiler_error(error: compiler::CompilerFailure) -> CorrespondenceFailure {
    match error {
        compiler::CompilerFailure::Store(error) => CorrespondenceFailure::Store(error),
        compiler::CompilerFailure::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
    }
}
fn initialization_error(error: super::InitializationFailure) -> CorrespondenceFailure {
    match error {
        super::InitializationFailure::Store(error) => CorrespondenceFailure::Store(error),
        super::InitializationFailure::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
    }
}
fn codec_error(error: crate::product_backup::fallible_codec::CodecError) -> CorrespondenceFailure {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
        _ => CorrespondenceFailure::Store(compiler::error("correspondence identity invariant")),
    }
}

pub(super) fn metadata_keys(
    id: &Digest,
    layer: &DecodedScopeLayer,
    entity: &str,
    keys: &mut OrderedSet<String>,
    cx: &mut AllocationContext,
) -> Result<(), CorrespondenceFailure> {
    for suffix in ["member", "sealed", "day", "origin"] {
        let key = compiler::key_fallible(id, entity, suffix, cx).map_err(compiler_error)?;
        insert_system_key(keys, key, cx)?;
    }
    for patch in layer.patches() {
        let request = patch.request();
        if request.entity == entity
            && matches!(&request.destination, super::EffectDestinationDto::ViewColumn { .. } | super::EffectDestinationDto::EmitColumn { .. })
        {
            let key = compiler::saved_key_fallible(id, patch, cx).map_err(compiler_error)?;
            insert_system_key(keys, key, cx)?;
        }
    }
    Ok(())
}

fn insert_system_key(keys: &mut OrderedSet<String>, key: String, cx: &mut AllocationContext) -> Result<(), CorrespondenceFailure> {
    if keys.len() >= crate::product_contract::MAX_COLLECTION && !keys.contains(key.as_str()) {
        return Err(compiler::error("correspondence metadata key count exceeds bounds").into());
    }
    keys.try_insert(key, cx).map_err(CorrespondenceFailure::Allocation)?;
    Ok(())
}

/// Derive a historical scalar correspondence. `prior_frames` is the ordered
/// replay-frame set selected by the caller's already verified correspondence
/// registry for this exact source revision and original data identity.
pub(crate) fn derive(
    snapshot: &DecodedProjectSnapshot,
    prior_frames: &[DecodedReplayFrame],
    source: &FallibleDecodedCapturedProgram,
    original: &FallibleDecodedDataSnapshot,
    target: &FallibleDecodedCapturedProgram,
    day: i32,
    initialized: &FallibleDecodedDataSnapshot,
    cx: &mut AllocationContext,
) -> Result<DecodedVerifiedCorrespondence, CorrespondenceFailure> {
    let working = super::replay_snapshot_fallible(snapshot, cx).map_err(|error| match error {
        super::ReplayRegistryError::Store(error) => CorrespondenceFailure::Store(error),
        super::ReplayRegistryError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
    })?;
    derive_with(&working, prior_frames, source, original, target, day, initialized, cx)
}

/// Derive using a replay registry already constructed by the caller. This is
/// used by the fallible execution context so a sequence of proof checks shares
/// one immutable replay owner instead of repeatedly rebuilding it.
pub(crate) fn derive_with(
    replay: &DecodedProjectSnapshot,
    prior_frames: &[DecodedReplayFrame],
    source: &FallibleDecodedCapturedProgram,
    original: &FallibleDecodedDataSnapshot,
    target: &FallibleDecodedCapturedProgram,
    day: i32,
    initialized: &FallibleDecodedDataSnapshot,
    cx: &mut AllocationContext,
) -> Result<DecodedVerifiedCorrespondence, CorrespondenceFailure> {
    if prior_frames.len() > super::contract::MAX_LAYERS {
        return Err(compiler::error("historical replay frame count exceeds bounds").into());
    }
    let mut working = replay.try_clone_with(cx).map_err(|error| match error {
        super::super::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Allocation(failure) => CorrespondenceFailure::Allocation(failure),
        super::super::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Invariant => CorrespondenceFailure::Store(StoreError::Invalid("replay snapshot copy invariant".into())),
    })?;
    let source_id = working.retain_program_with(source, cx).map_err(copy_error)?;
    let target_id = target.revision_with(cx).map_err(copy_error)?;
    let manifest = working.scalar_scope()
        .and_then(|scope| scope.composition(&target_id))
        .ok_or_else(|| compiler::error("historical correspondence target is not managed"))?;

    let mut frames = Vec::new();
    product_alloc::try_reserve(&mut frames, prior_frames.len(), cx, AllocationSite::Vector)?;
    for frame in prior_frames {
        product_alloc::try_push(&mut frames, frame.copy_with(cx)?, cx, AllocationSite::Vector)?;
    }
    let operation_seed = original.try_clone_with(cx).map_err(data_copy_error)?;
    let mut data = initialized.try_clone_with(cx).map_err(data_copy_error)?;

    for id in manifest.layers() {
        if frames.iter().any(|frame| frame.layer() == id) { continue; }
        let scope = working.scalar_scope().ok_or_else(|| compiler::error("historical correspondence requires scalar scope"))?;
        let layer = scope.layer(id).ok_or_else(|| compiler::error("historical correspondence layer is missing"))?;

        let mut declared = true;
        for lifecycle in layer.request().lifecycles() {
            let Some(entity) = data.schema().iter().find(|entity| entity.id == lifecycle.entity()) else { declared = false; break; };
            for suffix in ["member", "sealed"] {
                let key = compiler::key_fallible(id, lifecycle.entity(), suffix, cx).map_err(compiler_error)?;
                if !entity.fields.iter().any(|field| field.id == key) { declared = false; break; }
            }
            if !declared { break; }
        }
        let mut initialized_metadata = declared;
        if declared {
            for row in DataSnapshotView::from(&data).records() {
                if !layer.request().lifecycles().iter().any(|lifecycle| lifecycle.entity() == row.entity()) { continue; }
                let member = compiler::key_fallible(id, row.entity(), "member", cx).map_err(compiler_error)?;
                let sealed = compiler::key_fallible(id, row.entity(), "sealed", cx).map_err(compiler_error)?;
                if row.values().get(&member).is_none() || row.values().get(&sealed).is_none() { initialized_metadata = false; break; }
            }
        }
        if data.generation() >= layer.basis().data().generation()
            && day >= layer.basis().day()
            && data.events_start_with(layer.basis().data())
            && initialized_metadata
        {
            continue;
        }
        if data.generation() > layer.basis().data().generation()
            || day > layer.basis().day()
            || !layer.basis().data().events_start_with(original)
        {
            return Err(compiler::error("historical input has no actual frozen-history correspondence").into());
        }

        for row in DataSnapshotView::from(original).records() {
            let later = DataSnapshotView::from(layer.basis().data()).record(row.entity(), row.id())
                .ok_or_else(|| compiler::error("historical record is absent from the actual frozen cohort"))?;
            let mut system_keys = OrderedSet::new();
            for (layer_id, proof) in working.scalar_scope()
                .ok_or_else(|| compiler::error("historical correspondence requires scalar scope"))?.layers()
            {
                if proof.request().lifecycles().iter().any(|lifecycle| lifecycle.entity() == row.entity()) {
                    metadata_keys(layer_id, proof, row.entity(), &mut system_keys, cx)?;
                }
            }
            if later.created_program() != row.created_program()
                || later.revision() < row.revision()
                || (row.archived() && (!later.archived()
                    || row.revision() != later.revision()
                    || row.values().iter().any(|(key, value)| !system_keys.contains(key.as_str()) && later.values().get(key) != Some(value))))
            {
                return Err(compiler::error("historical record birth or archive provenance differs").into());
            }
        }

        if frames.len() >= super::contract::MAX_LAYERS {
            return Err(compiler::error("historical replay frame count exceeds bounds").into());
        }
        let before = data.try_clone_with(cx).map_err(data_copy_error)?;
        let (next, additions) = super::initialize_scalar_frame_fallible(
            &working, layer, id, &before, &source_id, target, day,
            "CapturedForHistoricalReplay", cx,
        ).map_err(initialization_error)?;
        let layer_id = copy_digest(id, cx)?;
        product_alloc::try_push(&mut frames, DecodedReplayFrame { layer: layer_id, before, additions }, cx, AllocationSite::Vector)?;
        data = next;
    }
    if frames.is_empty() {
        return Err(compiler::error("no historical layer correspondence was established").into());
    }
    data = data.merge_app_with(target.app(), cx).map_err(copy_error)?;
    let projected = data.identity_with(cx).map_err(copy_error)?;
    let operation_seed_identity = operation_seed.identity_with(cx).map_err(copy_error)?;
    let copied_source = source.try_clone_with(cx).map_err(copy_error)?;
    let copied_original = original.try_clone_with(cx).map_err(data_copy_error)?;
    let proof = super::DecodedScopeCorrespondence::new(
        copied_source,
        copied_original,
        copy_digest(&target_id, cx)?,
        day,
        projected,
        operation_seed_identity,
    );
    Ok(DecodedVerifiedCorrespondence { proof, seed: data, frames, operation_seed })
}
