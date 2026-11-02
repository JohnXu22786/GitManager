//! Allocation-aware scalar history verifier over the promoted decoded owners.
//!
//! This module is deliberately separate from the legacy validator so both
//! implementations remain available for parity testing during the model
//! cutover. No decoded value is materialized as a legacy BTreeMap model.
use crate::product_alloc::{ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{AppValidationError, DecisionValidationError, Digest, FallibleDecodedDecisionGraph, IdentityDomain};
use crate::product_store::{fallible_project_snapshot_decode::DecodedProjectSnapshot, StoreError};
use serde::ser::{SerializeMap, SerializeTuple};
use serde::{Serialize, Serializer};

struct StringWire(&'static str);
impl Serialize for StringWire {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(self.0) }
}
impl crate::product_backup::fallible_codec::BackupWire for StringWire {}

struct RehearsalRequestWire<'a>(&'a Digest, &'a super::DecodedCompositionManifest, &'a crate::product_contract::FallibleDecodedCapturedProgram, &'a crate::product_contract::FallibleDecodedCapturedProgram, &'a Digest, &'a super::DecodedCompatibilityReport, &'a [String]);
impl Serialize for RehearsalRequestWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(8)?;
        tuple.serialize_element("managed-rehearsal/1")?;
        tuple.serialize_element(self.0)?; tuple.serialize_element(self.1)?;
        tuple.serialize_element(self.2)?; tuple.serialize_element(self.3)?;
        tuple.serialize_element(self.4)?; tuple.serialize_element(self.5)?;
        tuple.serialize_element(self.6)?; tuple.end()
    }
}
impl crate::product_backup::fallible_codec::BackupWire for RehearsalRequestWire<'_> {}

struct PairRequestWire<'a>(&'a [Digest]);
impl Serialize for PairRequestWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element("managed-evolution-pair/1")?;
        tuple.serialize_element(self.0)?; tuple.end()
    }
}
impl crate::product_backup::fallible_codec::BackupWire for PairRequestWire<'_> {}

struct RecordingRequestWire<'a>(&'a crate::product_contract::FallibleDecodedAdoptionPlan, &'a crate::product_contract::FallibleDecodedCapturedProgram, &'a FallibleDecodedDecisionGraph, &'a Digest);
impl Serialize for RecordingRequestWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(5)?;
        tuple.serialize_element("record-managed-rehearsal/1")?;
        tuple.serialize_element(self.0)?; tuple.serialize_element(self.1)?;
        tuple.serialize_element(self.2)?; tuple.serialize_element(self.3)?; tuple.end()
    }
}
impl crate::product_backup::fallible_codec::BackupWire for RecordingRequestWire<'_> {}

struct CorrespondenceInventory<'a>(&'a OrderedMap<Digest, super::DecodedScopeCorrespondence>);
impl Serialize for CorrespondenceInventory<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, value)?; }
        map.end()
    }
}

struct CorrespondenceRequestWire<'a>(&'a Digest, &'a OrderedMap<Digest, super::DecodedScopeCorrespondence>);
impl Serialize for CorrespondenceRequestWire<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(3)?;
        tuple.serialize_element("scope-correspondence/1")?;
        tuple.serialize_element(self.0)?;
        tuple.serialize_element(&CorrespondenceInventory(self.1))?;
        tuple.end()
    }
}
impl crate::product_backup::fallible_codec::BackupWire for CorrespondenceRequestWire<'_> {}

#[derive(Debug)]
pub(crate) enum ScalarValidationFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for ScalarValidationFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}
impl From<AllocationFailure> for ScalarValidationFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}

fn error(message: &'static str) -> ScalarValidationFailure {
    super::compiler::error(message).into()
}
fn decision_error(error: DecisionValidationError) -> ScalarValidationFailure {
    match error {
        DecisionValidationError::Contract(error) => ScalarValidationFailure::Store(StoreError::Invalid(error.0)),
        DecisionValidationError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}
fn app_error(error: AppValidationError) -> ScalarValidationFailure {
    match error {
        AppValidationError::Contract(error) => ScalarValidationFailure::Store(StoreError::Invalid(error.0)),
        AppValidationError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}
fn codec_error(failure: crate::product_backup::fallible_codec::CodecError) -> ScalarValidationFailure {
    match failure {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
        _ => error("scope history canonical identity invariant"),
    }
}
fn decode_error(failure: crate::product_contract::fallible_value_decode::ValueDecodeError) -> ScalarValidationFailure {
    match failure {
        crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
        _ => error("scope proof copy invariant"),
    }
}
fn copy_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, ScalarValidationFailure> {
    let value = crate::product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    Ok(Digest::from_owned_lower_hex(value).expect("validated digest remains valid"))
}

struct DecisionBirths {
    active: OrderedSet<String>,
    pending: OrderedSet<String>,
}
impl DecisionBirths {
    fn contains_active(&self, id: &str) -> bool { self.active.contains(id) }
    fn contains_pending(&self, id: &str) -> bool { self.pending.contains(id) }
}

/// Recover the exact decision graph before each adoption. This reverse walk
/// is the authorization source for scalar decision births; it is not inferred
/// from current status labels alone.
fn decision_births(
    snapshot: &DecodedProjectSnapshot,
    cx: &mut AllocationContext,
) -> Result<OrderedMap<u64, DecisionBirths>, ScalarValidationFailure> {
    let mut graph = snapshot.decisions().try_clone_with(cx).map_err(decision_error)?;
    let mut births = OrderedMap::new();
    for adoption in snapshot.adoptions().iter().rev() {
        verify_recording_request(snapshot, adoption, &graph, cx)?;
        let active = graph.active_ids_with(cx).map_err(decision_error)?;
        let mut required = OrderedSet::new();
        for id in adoption.plan().required_decisions() {
            required.try_insert(id.as_str(), cx)?;
        }
        if active.len() != required.len() || active.iter().any(|id| !required.contains(id.as_str())) {
            return Err(error("adoption requirements differ from its exact committed graph"));
        }
        let (rewound, active_ids, pending_ids) = graph.rewind_adoption_with(
            adoption.plan().expected_decisions(), adoption.plan().retire_decisions(),
            adoption.plan().id(), cx,
        ).map_err(decision_error)?;
        let mut active = OrderedSet::new();
        for id in active_ids { active.try_insert(id, cx)?; }
        let mut pending = OrderedSet::new();
        for id in pending_ids { pending.try_insert(id, cx)?; }
        graph = rewound;
        births.try_insert(adoption.revision(), DecisionBirths { active, pending }, cx)?;
    }
    if !graph.is_empty_checkpoint() {
        return Err(error("decision graph lacks its original empty checkpoint"));
    }
    Ok(births)
}

fn verify_recording_request(
    snapshot: &DecodedProjectSnapshot,
    receipt: &crate::product_store::fallible_project_snapshot_decode::DecodedAdoptionReceipt,
    graph: &FallibleDecodedDecisionGraph,
    cx: &mut AllocationContext,
) -> Result<(), ScalarValidationFailure> {
    let scope = snapshot.scalar_scope().ok_or_else(|| error("scalar scope envelope is required"))?;
    let mut proofs = Vec::new();
    crate::product_alloc::try_reserve(&mut proofs, scope.rehearsals().size_hint().0, cx, AllocationSite::Vector)?;
    for (_, proof) in scope.rehearsals() {
        if proof.recorded_by() == receipt.plan().id() {
            crate::product_alloc::try_push(&mut proofs, proof, cx, AllocationSite::Vector)?;
        }
    }
    let pair_tag = crate::product_backup::fallible_codec::canonical_digest(
        IdentityDomain::Evidence, &StringWire("managed-evolution-pair/1"), cx,
    ).map_err(codec_error)?;
    let mut tag_positions = Vec::new();
    crate::product_alloc::try_reserve(&mut tag_positions, receipt.plan().evidence().len(), cx, AllocationSite::Vector)?;
    for (index, evidence) in receipt.plan().evidence().iter().enumerate() {
        if evidence == &pair_tag {
            crate::product_alloc::try_push(&mut tag_positions, index, cx, AllocationSite::Vector)?;
        }
    }
    let binding = match tag_positions.as_slice() {
        [] => None,
        [index] if *index > 0 && index.checked_add(2) == Some(receipt.plan().evidence().len()) => receipt.plan().evidence().last(),
        _ => return Err(error("paired recording tag is duplicated, misplaced or incomplete")),
    };
    if binding.is_none() && proofs.len() < 2 { return Ok(()); }
    if proofs.len() != 2 || binding.is_none()
        || proofs.iter().any(|proof| proof.manifest().transition() != super::ScopeTransition::Evolution)
    {
        return Err(error("paired recording is missing a tag or one of its exact proofs"));
    }
    let first = proofs[0];
    if first.witness_count() != 1 || proofs.iter().any(|proof| {
        proof.manifest().basis() != first.manifest().basis()
            || proof.recorded_revision() != first.recorded_revision()
            || !witness_maps_equal(proof, first)
    }) {
        return Err(error("paired rehearsal proofs differ in basis or exact recording birth"));
    }
    let mut witness_ids = Vec::new();
    crate::product_alloc::try_reserve(&mut witness_ids, first.witness_count(), cx, AllocationSite::Vector)?;
    for (id, _) in first.witnesses() {
        let id = crate::product_alloc::try_copy_str(id, cx, AllocationSite::String)?;
        crate::product_alloc::try_push(&mut witness_ids, id, cx, AllocationSite::Vector)?;
    }
    let decisions = graph.ids_matching_with(&witness_ids, cx).map_err(decision_error)?;
    let mut requests = Vec::new();
    crate::product_alloc::try_reserve(&mut requests, proofs.len(), cx, AllocationSite::Vector)?;
    for proof in &proofs {
        let business = snapshot.retained_source_program_with(proof.manifest().business(), cx)
            .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
        let output = snapshot.retained_source_program_with(proof.manifest().output(), cx)
            .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
        let request = crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Adoption,
            &RehearsalRequestWire(proof.manifest().basis().snapshot(), proof.manifest(), business, output, proof.seed(), proof.compatibility(), &decisions),
            cx,
        ).map_err(codec_error)?;
        crate::product_alloc::try_push(&mut requests, request, cx, AllocationSite::Vector)?;
    }
    requests.sort_unstable();
    let proof_request = crate::product_backup::fallible_codec::canonical_digest(
        IdentityDomain::Adoption, &PairRequestWire(&requests), cx,
    ).map_err(codec_error)?;
    if binding != Some(&proof_request) {
        return Err(error("paired recording proof inventory differs from its tagged evidence"));
    }
    let target = snapshot.retained_source_program_with(receipt.active(), cx)
        .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
    let mut request = crate::product_backup::fallible_codec::canonical_digest(
        IdentityDomain::Adoption, &RecordingRequestWire(receipt.plan(), target, graph, &proof_request), cx,
    ).map_err(codec_error)?;
    let mut correspondences = OrderedMap::new();
    for (id, proof) in scope.correspondences() {
        if proof.operation() == receipt.plan().id() {
            let id = copy_digest(id, cx)?;
            let proof = proof.proof().copy_with(cx).map_err(decode_error)?;
            correspondences.try_insert(id, proof, cx)?;
        }
    }
    if !correspondences.is_empty() {
        request = crate::product_backup::fallible_codec::canonical_digest(
            IdentityDomain::Adoption, &CorrespondenceRequestWire(&request, &correspondences), cx,
        ).map_err(codec_error)?;
    }
    if snapshot.operation_request(receipt.plan().id()) != Some(&request) {
        return Err(error("rehearsal or decision recording differs from its exact operation receipt"));
    }
    Ok(())
}

fn witness_maps_equal(
    left: &super::DecodedManagedRehearsal,
    right: &super::DecodedManagedRehearsal,
) -> bool {
    left.witnesses().eq(right.witnesses())
}

fn validate_history(snapshot: &DecodedProjectSnapshot, replay: super::FallibleScalarReplayContext, cx: &mut AllocationContext) -> Result<(), ScalarValidationFailure> {
    // The legacy verifier validates complete live history after recording and
    // correspondence receipts. Check it before the replay-context seed gate so
    // corruption in current rows keeps the legacy's specific failure reason.
    replay.verify_all_live_layers(snapshot.data(), snapshot.clock_day(), cx).map_err(replay_error)?;
    let active = snapshot.retained_source_program_with(snapshot.active_revision(), cx)
        .map_err(app_error)?.ok_or_else(|| error("active source missing"))?;
    replay.verify_seed(active, snapshot.data(), snapshot.clock_day(), cx).map_err(replay_error)
}

fn replay_error(failure: super::ScalarReplayFailure) -> ScalarValidationFailure {
    match failure {
        super::ScalarReplayFailure::Store(error) => ScalarValidationFailure::Store(error),
        super::ScalarReplayFailure::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}

fn validate_recorded_proofs(snapshot: &DecodedProjectSnapshot, cx: &mut AllocationContext) -> Result<super::FallibleScalarReplayContext, ScalarValidationFailure> {
    let state = snapshot.scalar_scope().ok_or_else(|| error("scalar scope envelope is required"))?;
    if state.rehearsals().count() > crate::product_contract::MAX_ITEMS {
        return Err(error("rehearsal inventory exceeds bounds"));
    }
    for (output, proof) in state.rehearsals() {
        let manifest = proof.manifest();
        if proof.version() != super::COMPILER_VERSION || manifest.version() != super::COMPILER_VERSION
            || manifest.output() != output || manifest.runtime() != super::RUNTIME_VERSION
            || manifest.driver() != super::DRIVER_VERSION
            || manifest.basis().revision().checked_add(1) != Some(proof.recorded_revision())
            || proof.recorded_revision() > snapshot.revision() || proof.witness_count() == 0
            || proof.witness_count() > crate::product_contract::MAX_ITEMS
            || !crate::product_contract::valid_id(manifest.operation())
            || !crate::product_contract::valid_id(proof.recorded_by())
        { return Err(error("invalid retained rehearsal identity or transition")); }
        let previous = state.composition(manifest.basis().active());
        if manifest.previous() != previous.map(|_| manifest.basis().active()) {
            return Err(error("rehearsal predecessor differs from its adopted basis"));
        }
        manifest.basis().data().validate_with(cx).map_err(app_error)?;
        crate::product_contract::validate_day(manifest.basis().day()).map_err(|error| StoreError::Invalid(error.0))?;
        let baseline = snapshot.retained_source_program_with(manifest.basis().active(), cx)
            .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
        baseline.validate_session_with(manifest.basis().session(), cx).map_err(app_error)?;
        let candidate = snapshot.retained_source_program_with(manifest.business(), cx)
            .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
        if has_reserved_namespace(candidate, cx)? { return Err(error("authored source uses the reserved metadata namespace")); }
        if candidate.project_id() != snapshot.data().project_id() || manifest.basis().data().project_id() != snapshot.data().project_id() {
            return Err(error("rehearsal belongs to another project"));
        }
        let target = snapshot.retained_source_program_with(output, cx)
            .map_err(app_error)?.ok_or_else(|| error("retained program revision missing"))?;
        let working = snapshot.with_scalar_basis(manifest.basis().active(), manifest.basis().data(), manifest.basis().session(),
            manifest.basis().day(), manifest.basis().revision(), cx).map_err(project_copy_error)?;
        let (mut working, seed) = match manifest.transition() {
            super::ScopeTransition::Evolution => {
                let previous = previous.ok_or_else(|| error("evolution rehearsal has no adopted predecessor"))?;
                if proof.layer().is_some() || proof.initialization().is_some()
                    || manifest.layers() != previous.layers()
                    || manifest.active_layers().ne(previous.active_layers())
                { return Err(error("evolution rehearsal changed the adopted layer set")); }
                super::compiler::validate_mappings_fallible(snapshot, previous, candidate, manifest.rewrites(), cx).map_err(compiler_error)?;
                (working, manifest.basis().data().merge_app_with(target.app(), cx).map_err(app_error)?)
            }
            super::ScopeTransition::Adoption => {
                let layer = proof.layer().ok_or_else(|| error("prospective layer proof missing"))?;
                let id = layer.identity_with(cx).map_err(codec_error)?;
                let mut expected_layers = Vec::new();
                let mut expected_active = OrderedSet::new();
                if let Some(previous) = previous {
                    crate::product_alloc::try_reserve(&mut expected_layers, previous.layers().len().saturating_add(1), cx, AllocationSite::Vector)?;
                    for prior in previous.layers() { crate::product_alloc::try_push(&mut expected_layers, copy_digest(prior, cx)?, cx, AllocationSite::Vector)?; }
                    for prior in previous.active_layers() { expected_active.try_insert(copy_digest(prior, cx)?, cx)?; }
                }
                crate::product_alloc::try_push(&mut expected_layers, copy_digest(&id, cx)?, cx, AllocationSite::Vector)?;
                expected_active.try_insert(copy_digest(&id, cx)?, cx)?;
                if layer.version() != super::COMPILER_VERSION || layer.basis() != manifest.basis()
                    || layer.operation() != manifest.operation() || layer.candidate() != manifest.business()
                    || manifest.layers().len() > super::MAX_LAYERS
                    || manifest.layers() != expected_layers.as_slice() || manifest.active_len() != expected_active.len()
                    || manifest.active_layers().any(|active| !expected_active.contains(active))
                    || state.layer(&id).is_some()
                    || manifest.rewrites() != previous.map(|prior| prior.rewrites()).unwrap_or(&[])
                { return Err(error("prospective layer differs from its exact prepared envelope")); }
                let mut working = working;
                working.scalar_scope_mut().ok_or_else(|| error("scalar scope envelope is required"))?.retain_replay_layers(previous.map(|prior| prior.layers()).unwrap_or(&[]), cx)?;
                request_valid(&working, layer.request(), cx)?;
                let before = super::compiler::business_program_fallible(&working, cx).map_err(compiler_error)?;
                let producer = crate::product_contract::Producer::Fixture { name: crate::product_alloc::try_copy_str("scope-history-validation", cx, AllocationSite::String)? };
                let before = crate::product_contract::capture_generated_app_fallible(before, snapshot.data().project_id(), producer, cx).map_err(app_error)?;
                let patches = super::compiler::derive_patches_fallible(&before, candidate, layer.request(), cx).map_err(compiler_error)?;
                if patches != layer.patches() { return Err(error("prospective layer patches do not independently regenerate")); }
                let inserted = working.scalar_scope_mut().unwrap().insert_replay_layer(copy_digest(&id, cx)?, layer.copy_with(cx).map_err(decode_error)?, cx).map_err(decode_error)?;
                if !inserted { return Err(error("prospective layer differs from its exact prepared envelope")); }
                let (seed, additions) = super::initialize_scalar_fallible(&working, layer, target, cx).map_err(initialization_error)?;
                let receipt = super::scalar_initialization_receipt(&working, layer, layer.basis().data(), &seed, &additions, cx).map_err(initialization_error)?;
                if proof.initialization() != Some(&receipt) { return Err(error("prospective initialization does not independently regenerate")); }
                (working, seed)
            }
            super::ScopeTransition::Withdrawal => return Err(error("withdrawal cannot be retained as rehearsal authority")),
        };
        let compiled = super::compiler::compile_fallible(&working, manifest, cx).map_err(compiler_error)?;
        if crate::product_backup::fallible_codec::canonical_bytes(&compiled, cx).map_err(codec_error)?
            != crate::product_backup::fallible_codec::canonical_bytes(target, cx).map_err(codec_error)?
        { return Err(error("rehearsal executable does not independently regenerate")); }
        if seed.identity_with(cx).map_err(app_error)? != *proof.seed() { return Err(error("rehearsal seed lacks exact provenance")); }
        let compatibility = super::compatibility_report_fallible(target, &seed, manifest.basis().day(), cx).map_err(compatibility_error)?;
        if compatibility.state() != crate::product_contract::CompatibilityState::Compatible
            || crate::product_backup::fallible_codec::canonical_bytes(&compatibility, cx).map_err(codec_error)?
                != crate::product_backup::fallible_codec::canonical_bytes(proof.compatibility(), cx).map_err(codec_error)?
        { return Err(error("rehearsal compatibility does not independently regenerate")); }
        let session = target.initial_session_with(cx)?;
        super::validate_initial_runtime_fallible(target, &seed, &session, manifest.basis().day(), crate::product_contract::RuntimeLimits::default(), cx)
            .map_err(compatibility_error)?;
        let receipt = snapshot.adoptions().iter().find(|receipt| receipt.plan().id() == proof.recorded_by())
            .ok_or_else(|| error("rehearsal has no recording receipt"))?;
        if receipt.revision() != proof.recorded_revision() || receipt.previous() != manifest.basis().active()
            || receipt.active() != manifest.basis().active() || receipt.plan().target() != baseline.artifact()
            || receipt.plan().current_source() != baseline.binding()
            || receipt.plan().expected_data() != &manifest.basis().data().identity_with(cx).map_err(app_error)?
            || receipt.plan().expected_generation() != manifest.basis().data().generation()
            || receipt.plan().expected_session() != &manifest.basis().session().identity_with(cx).map_err(codec_error)?
            || receipt.plan().expected_decisions() != manifest.basis().decisions()
        { return Err(error("rehearsal recording changed the active source or frozen basis")); }
        for (id, witness) in proof.witnesses() {
            if !snapshot.decisions().pending_nonbinary_witness(id, witness)
                || receipt.plan().required_decisions().iter().any(|required| required == id)
            { return Err(error("rehearsal witness link is missing or was relabeled as active authority")); }
        }
    }
    for (id, receipt) in state.correspondences() {
        let proof = receipt.proof();
        if proof.identity_with(cx).map_err(codec_error)? != *id || !crate::product_contract::valid_id(receipt.operation())
            || receipt.revision() > snapshot.revision()
        { return Err(error("invalid historical correspondence identity")); }
        let recorded = snapshot.adoptions().iter().find(|adoption| adoption.plan().id() == receipt.operation())
            .ok_or_else(|| error("historical correspondence has no atomic recording receipt"))?;
        let target = snapshot.retained_source_program_with(proof.target(), cx).map_err(app_error)?
            .ok_or_else(|| error("retained program revision missing"))?;
        if recorded.revision() != receipt.revision()
            || ((recorded.active() != proof.target() || recorded.plan().target() != target.artifact())
                && !state.rehearsals().any(|(_, rehearsal)| rehearsal.recorded_by() == receipt.operation() && rehearsal.manifest().output() == proof.target()))
        { return Err(error("historical correspondence is not reachable from its recording target")); }
    }
    super::FallibleScalarReplayContext::registry(snapshot, cx).map_err(replay_error)?
        .with_correspondences(cx).map_err(replay_error)
}

fn project_copy_error(failure: crate::product_store::fallible_project_snapshot_decode::ProjectSnapshotCloneError) -> ScalarValidationFailure {
    match failure {
        crate::product_store::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
        crate::product_store::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Invariant => error("basis snapshot copy invariant"),
    }
}

fn compiler_error(failure: super::compiler::CompilerFailure) -> ScalarValidationFailure {
    match failure {
        super::compiler::CompilerFailure::Store(error) => ScalarValidationFailure::Store(error),
        super::compiler::CompilerFailure::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}
fn compatibility_error(failure: super::CompatibilityFailure) -> ScalarValidationFailure {
    match failure {
        super::CompatibilityFailure::Store(error) => ScalarValidationFailure::Store(error),
        super::CompatibilityFailure::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}
fn initialization_error(failure: super::InitializationFailure) -> ScalarValidationFailure {
    match failure {
        super::InitializationFailure::Store(error) => ScalarValidationFailure::Store(error),
        super::InitializationFailure::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
    }
}
fn has_reserved_namespace(
    program: &crate::product_contract::FallibleDecodedCapturedProgram,
    cx: &mut AllocationContext,
) -> Result<bool, ScalarValidationFailure> {
    fn has_reserved(value: &crate::product_json::Value) -> bool {
        use crate::product_json::Value;
        match value {
            Value::String(value) => value.starts_with("gm_scope_"),
            Value::Array(values) => values.iter().any(has_reserved),
            Value::Object(values) => values.iter().any(|(key, value)| key.starts_with("gm_scope_") || has_reserved(value)),
            _ => false,
        }
    }
    let value = crate::product_backup::fallible_codec::try_serialize_value(program.app(), cx).map_err(codec_error)?;
    Ok(has_reserved(&value))
}

fn request_valid(
    snapshot: &DecodedProjectSnapshot,
    request: &super::DecodedScopeRequest,
    cx: &mut AllocationContext,
) -> Result<(), ScalarValidationFailure> {
    let lifecycles = request.lifecycles();
    let excluded = request.excluded_records();
    if request.operations().count() == 0 || request.operations().count() > crate::product_contract::MAX_ITEMS
        || excluded.len() > crate::product_contract::MAX_COLLECTION
    {
        return Err(error("invalid scope bounds"));
    }
    let active = snapshot.retained_source_program_with(snapshot.active_revision(), cx).map_err(app_error)?
        .ok_or_else(|| error("active source missing"))?;
    for operation in request.operations() {
        if !active.app().actions.iter().any(|action| &action.id == operation) {
            return Err(error("unknown scope operation"));
        }
    }
    let mut entities = OrderedSet::new();
    for lifecycle in lifecycles {
        if lifecycle.source() != snapshot.active_revision() || !entities.try_insert(lifecycle.entity(), cx)? {
            return Err(error("stale or duplicate completion binding"));
        }
        if snapshot.scalar_scope().is_some_and(|scope| scope.layers().any(|(_, layer)| {
            layer.request().lifecycles().iter().any(|prior| prior.entity() == lifecycle.entity() && prior.completed() != lifecycle.completed())
        })) {
            return Err(error("completion meaning differs from protected entity history; lifecycle redefinition requires an explicit preservation design"));
        }
        let empty_env = crate::product_contract::fallible_app_decode::FallibleMap::<crate::product_contract::Type>::new();
        super::compiler::own_row(lifecycle.completed(), "record", lifecycle.entity(), crate::product_contract::AppRef::from(active.app()), &empty_env, false)
            .map_err(ScalarValidationFailure::Store)?;
    }
    let mut check_refs = |refs: &[crate::product_contract::RecordRef]| -> Result<(), ScalarValidationFailure> {
        let mut seen = OrderedSet::new();
        let view = crate::product_contract::DataSnapshotView::from(snapshot.data());
        for reference in refs {
            if !seen.try_insert((reference.entity.as_str(), reference.record.as_str()), cx)?
                || !view.records().any(|row| row.entity() == reference.entity && row.id() == reference.record) {
                return Err(error("missing or duplicate frozen record"));
            }
        }
        Ok(())
    };
    check_refs(excluded)?;
    if let super::ScopePopulationDto::SelectedUnfinished { records } = request.population() {
        if records.is_empty() || records.len() > crate::product_contract::MAX_COLLECTION {
            return Err(error("selected unfinished population is empty or too large"));
        }
        check_refs(records)?;
        let view = crate::product_contract::DataSnapshotView::from(snapshot.data());
        for reference in records {
            let row = view.records().find(|row| row.entity() == reference.entity && row.id() == reference.record)
                .ok_or_else(|| error("missing or duplicate frozen record"))?;
            let lifecycle = lifecycles.iter().find(|lifecycle| lifecycle.entity() == row.entity())
                .ok_or_else(|| error("selected record has no completed-work meaning"))?;
            let sealed = if row.archived() { true } else {
                let runtime = crate::product_runtime::LocalRuntime::default();
                let value = runtime.evaluate_decoded_record_projection_and_allocations(
                    active, snapshot.data(), reference, "record", lifecycle.completed(), &crate::product_contract::Type::Boolean,
                    snapshot.clock_day(), crate::product_contract::RuntimeLimits::default(), std::mem::replace(cx, AllocationContext::new()),
                );
                let value = match value {
                    Ok((value, allocations)) => { *cx = allocations; value }
                    Err((error, allocations)) => { *cx = allocations; return Err(ScalarValidationFailure::Store(StoreError::Runtime(error))); }
                };
                match value { crate::product_contract::DataValue::Boolean { value } => value, _ => return Err(error("completion predicate is not Boolean")) }
            };
            if sealed || excluded.iter().any(|excluded| excluded == reference) {
                return Err(error(if sealed { "selected work is already completed or archived" } else { "selected work is also excluded" }));
            }
        }
    }
    Ok(())
}

fn validate_layers(
    snapshot: &DecodedProjectSnapshot,
    _births: &OrderedMap<u64, DecisionBirths>,
    cx: &mut AllocationContext,
) -> Result<(), ScalarValidationFailure> {
    let state = snapshot.scalar_scope().ok_or_else(|| error("scalar scope envelope is required"))?;
    for (id, layer) in state.layers() {
        if layer.version() != super::COMPILER_VERSION || layer.identity_with(cx).map_err(codec_error)? != *id || layer.operation().is_empty() {
            return Err(error("layer identity mismatch"));
        }
        layer.basis().data().validate_with(cx).map_err(app_error)?;
        if layer.basis().data().project_id() != snapshot.data().project_id()
            || layer.basis().revision() >= snapshot.revision()
        { return Err(error("foreign or future scope basis")); }
        let candidate = snapshot.retained_source_program_with(layer.candidate(), cx).map_err(app_error)?
            .ok_or_else(|| error("retained program revision missing"))?;
        if has_reserved_namespace(candidate, cx)? { return Err(error("authored source uses the reserved metadata namespace")); }
        let basis = snapshot.with_scalar_basis(
            layer.basis().active(), layer.basis().data(), layer.basis().session(), layer.basis().day(), layer.basis().revision(), cx,
        ).map_err(|copy_failure| match copy_failure {
            crate::product_store::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
            crate::product_store::fallible_project_snapshot_decode::ProjectSnapshotCloneError::Invariant => error("basis snapshot copy invariant"),
        })?;
        request_valid(&basis, layer.request(), cx)?;
        let before_app = super::compiler::business_program_fallible(&basis, cx).map_err(compiler_error)?;
        let producer = crate::product_contract::Producer::Fixture {
            name: crate::product_alloc::try_copy_str("scope-history-validation", cx, AllocationSite::String)?,
        };
        let before = crate::product_contract::capture_generated_app_fallible(before_app, snapshot.data().project_id(), producer, cx).map_err(app_error)?;
        let actual_patches = super::compiler::derive_patches_fallible(&before, candidate, layer.request(), cx).map_err(compiler_error)?;
        if actual_patches != layer.patches() { return Err(error("layer patch proof does not independently regenerate")); }

        let receipt = state.initializations().iter().find(|receipt| receipt.layer() == id)
            .ok_or_else(|| error("layer has no initialization"))?;
        let adoption = snapshot.adoptions().iter().find(|adoption| adoption.plan().id() == layer.operation())
            .ok_or_else(|| error("layer has no atomic adoption"))?;
        let expected_data = layer.basis().data().identity_with(cx).map_err(app_error)?;
        let expected_session = layer.basis().session().identity_with(cx).map_err(codec_error)?;
        if adoption.previous() != layer.basis().active()
            || adoption.revision() != receipt.adoption_revision()
            || adoption.plan().expected_data() != &expected_data
            || adoption.plan().expected_generation() != layer.basis().data().generation()
            || adoption.plan().expected_session() != &expected_session
            || adoption.plan().expected_decisions() != layer.basis().decisions()
            || !adoption.plan().scope_matches_layer(layer.request(), layer.basis().data().generation())
        { return Err(error("layer/adoption basis linkage differs")); }
        let target = snapshot.retained_source_program_with(adoption.active(), cx).map_err(app_error)?
            .ok_or_else(|| error("retained program revision missing"))?;
        let (after, additions) = super::initialize_scalar_fallible(snapshot, layer, target, cx).map_err(initialization_error)?;
        let regenerated = super::scalar_initialization_receipt(snapshot, layer, layer.basis().data(), &after, &additions, cx)
            .map_err(initialization_error)?;
        let actual = crate::product_backup::fallible_codec::canonical_bytes(receipt, cx).map_err(codec_error)?;
        let expected = crate::product_backup::fallible_codec::canonical_bytes(&regenerated, cx).map_err(codec_error)?;
        if actual != expected { return Err(error("metadata initialization does not independently regenerate")); }
    }
    Ok(())
}

fn validate_compositions(
    snapshot: &DecodedProjectSnapshot,
    history: &AdoptionHistory,
    cx: &mut AllocationContext,
) -> Result<(), ScalarValidationFailure> {
    let state = snapshot.scalar_scope().ok_or_else(|| error("scalar scope envelope is required"))?;
    for (output, manifest) in state.compositions() {
        if manifest.version() != super::COMPILER_VERSION
            || manifest.output() != output
            || manifest.layers().len() > super::MAX_LAYERS
            || manifest.layers().is_empty()
            || manifest.runtime() != super::RUNTIME_VERSION
            || manifest.driver() != super::DRIVER_VERSION
        { return Err(error("unsupported composition or stale runtime")); }
        let mut unique = OrderedSet::new();
        for layer in manifest.layers() {
            if !unique.try_insert(layer, cx)? { return Err(error("ambiguous active layer inventory")); }
        }
        if manifest.active_layers().any(|id| !manifest.layers().contains(id)) {
            return Err(error("ambiguous active layer inventory"));
        }
        let expected_previous = state.composition(manifest.basis().active()).map(|_| manifest.basis().active());
        if manifest.previous() != expected_previous {
            return Err(error("composition predecessor differs from its exact adopted basis"));
        }
        if let Some(previous_id) = manifest.previous() {
            let prior = state.composition(previous_id).ok_or_else(|| error("composition predecessor missing"))?;
            if !manifest.layers().starts_with(prior.layers()) { return Err(error("composition removed protected history")); }
        }
        manifest.basis().data().validate_with(cx).map_err(app_error)?;
        if manifest.basis().revision() >= snapshot.revision()
            || manifest.basis().data().project_id() != snapshot.data().project_id()
        { return Err(error("invalid composition basis")); }
        match manifest.transition() {
            super::ScopeTransition::Evolution => {
                let prior = manifest.previous().and_then(|id| state.composition(id))
                    .ok_or_else(|| error("evolution predecessor missing"))?;
                if !manifest.same_layer_inventory_as(prior) { return Err(error("evolution changed independent active scope layers")); }
                let candidate = snapshot.retained_source_program_with(manifest.business(), cx).map_err(app_error)?
                    .ok_or_else(|| error("retained program revision missing"))?;
                super::compiler::validate_mappings_fallible(snapshot, prior, candidate, manifest.rewrites(), cx)
                    .map_err(compiler_error)?;
            }
            super::ScopeTransition::Withdrawal => {
                let prior = manifest.previous().and_then(|id| state.composition(id))
                    .ok_or_else(|| error("withdrawal predecessor missing"))?;
                if manifest.rewrites() != prior.rewrites() { return Err(error("withdrawal changed protected mappings")); }
            }
            super::ScopeTransition::Adoption => {
                let prior = manifest.previous().and_then(|id| state.composition(id));
                if manifest.rewrites() != prior.map(|previous| previous.rewrites()).unwrap_or(&[]) {
                    return Err(error("scoped adoption changed prior mappings"));
                }
                let new = state.layers().find(|(_, layer)| layer.operation() == manifest.operation())
                    .map(|(_, layer)| layer).ok_or_else(|| error("adoption layer missing"))?;
                if manifest.business() != new.candidate() || manifest.basis() != new.basis() {
                    return Err(error("adoption composition differs from its exact layer candidate or basis"));
                }
                let id = new.identity_with(cx).map_err(codec_error)?;
                let expected_len = prior.map_or(0, |previous| previous.layers().len()) + 1;
                if manifest.layers().len() != expected_len
                    || prior.is_some_and(|previous| !manifest.layers().starts_with(previous.layers()))
                    || manifest.layers().last() != Some(&id)
                    || manifest.active_len() != prior.map_or(0, |previous| previous.active_len()) + 1
                    || prior.is_some_and(|previous| previous.active_layers().any(|layer| !manifest.active_contains(layer)))
                    || !manifest.active_contains(&id)
                { return Err(error("adoption changed an independent layer")); }
            }
        }
        let target = snapshot.retained_source_program_with(output, cx).map_err(app_error)?
            .ok_or_else(|| error("retained program revision missing"))?;
        let compiled = super::compiler::compile_fallible(snapshot, manifest, cx).map_err(compiler_error)?;
        let compiled_bytes = crate::product_backup::fallible_codec::canonical_bytes(&compiled, cx).map_err(codec_error)?;
        let target_bytes = crate::product_backup::fallible_codec::canonical_bytes(target, cx).map_err(codec_error)?;
        if compiled_bytes != target_bytes { return Err(error("compiled source differs from independently regenerated envelope")); }

        let (adoption_index, scoped_receipt_index) = history.links.get(output).copied()
            .ok_or_else(|| error("composition lacks its validated history link"))?;
        let linked = &snapshot.adoptions()[adoption_index];
        let matching = &state.adoptions()[scoped_receipt_index];
        let frozen_report = super::compatibility_report_fallible(target, manifest.basis().data(), manifest.basis().day(), cx)
            .map_err(compatibility_error)?;
        let frozen_bytes = crate::product_backup::fallible_codec::canonical_bytes(&frozen_report, cx).map_err(codec_error)?;
        let expected_frozen = crate::product_backup::fallible_codec::canonical_bytes(linked.plan().compatibility(), cx).map_err(codec_error)?;
        if frozen_bytes != expected_frozen { return Err(error("adoption compatibility differs from its frozen input")); }

        let layer = state.layers().find(|(_, layer)| layer.operation() == manifest.operation()).map(|(_, layer)| layer);
        let data = if let Some(layer) = layer {
            if manifest.transition() != super::ScopeTransition::Adoption { return Err(error("layer receipt attached to the wrong transition")); }
            let (data, additions) = super::initialize_scalar_fallible(snapshot, layer, target, cx).map_err(initialization_error)?;
            let receipt = super::scalar_initialization_receipt(snapshot, layer, layer.basis().data(), &data, &additions, cx).map_err(initialization_error)?;
            let identity = crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Adoption, &receipt, cx).map_err(codec_error)?;
            if matching.initialization() != Some(&identity) { return Err(error("initialization receipt link mismatch")); }
            data
        } else {
            if matching.initialization().is_some() { return Err(error("withdrawal must not initialize metadata")); }
            let previous = manifest.previous().and_then(|id| state.composition(id)).ok_or_else(|| error("withdrawal predecessor missing"))?;
            if manifest.transition() == super::ScopeTransition::Withdrawal
                && (manifest.business() != previous.business()
                    || manifest.layers() != previous.layers()
                    || !manifest.active_is_subset_of(previous)
                    || manifest.active_len() == previous.active_len())
            { return Err(error("withdrawal changed independent layers or source")); }
            manifest.basis().data().merge_app_with(target.app(), cx).map_err(app_error)?
        };
        let report = super::compatibility_report_fallible(target, &data, manifest.basis().day(), cx)
            .map_err(compatibility_error)?;
        if report.state() != crate::product_contract::CompatibilityState::Compatible
            || crate::product_backup::fallible_codec::canonical_bytes(&report, cx).map_err(codec_error)?
                != crate::product_backup::fallible_codec::canonical_bytes(matching.initialized_compatibility(), cx).map_err(codec_error)?
        { return Err(error("initialized-data compatibility receipt mismatch")); }
        let session = target.initial_session_with(cx)?;
        super::validate_initial_runtime_fallible(
            target,
            &data,
            &session,
            manifest.basis().day(),
            crate::product_contract::RuntimeLimits::default(),
            cx,
        )
            .map_err(compatibility_error)?;
    }
    Ok(())
}

struct AdoptionHistory {
    /// Composition output revision -> (ordered adoption index, scoped receipt index).
    links: OrderedMap<Digest, (usize, usize)>,
    /// Exact graph checkpoints reconstructed by the legacy-ordered reverse walk.
    births: OrderedMap<u64, DecisionBirths>,
}

fn adoption_history(
    snapshot: &DecodedProjectSnapshot,
    cx: &mut AllocationContext,
) -> Result<AdoptionHistory, ScalarValidationFailure> {
    let state = snapshot.scalar_scope().ok_or_else(|| error("scalar scope envelope is required"))?;
    let mut history = AdoptionHistory { links: OrderedMap::new(), births: OrderedMap::new() };
    if state.layers().next().is_none() && state.rehearsals().next().is_none()
        && state.correspondences().next().is_none() {
        return Ok(history);
    }

    let mut sources: OrderedMap<Digest, &crate::product_contract::FallibleDecodedCapturedProgram> = OrderedMap::new();
    for source in snapshot.programs() {
        sources.try_insert(source.revision_with(cx).map_err(app_error)?, source, cx)?;
    }

    let mut manifests: OrderedMap<Digest, Digest> = OrderedMap::new();
    for (output, manifest) in state.compositions() {
        let identity = manifest.identity_with(cx).map_err(codec_error)?;
        if manifests.try_insert(identity, copy_digest(output, cx)?, cx)?.is_some() {
            return Err(error("ambiguous composition identity"));
        }
    }
    let mut receipts: OrderedMap<u64, usize> = OrderedMap::new();
    for (index, receipt) in state.adoptions().iter().enumerate() {
        if receipts.try_insert(receipt.revision(), index, cx)?.is_some() {
            return Err(error("duplicate scoped adoption revision link"));
        }
    }

    // Match the legacy order: first index source/manifest/receipt identities,
    // then reconstruct decision checkpoints before traversing adoption links.
    let births = decision_births(snapshot, cx)?;

    let first = snapshot.programs().first().ok_or_else(|| error("initial source missing"))?;
    let mut active = first.revision_with(cx).map_err(app_error)?;
    let mut operations = OrderedSet::new();
    for (adoption_index, linked) in snapshot.adoptions().iter().enumerate() {
        let previous = sources.get(&active).copied()
            .ok_or_else(|| error("prior active capture missing"))?;
        let target = sources.get(linked.active()).copied()
            .ok_or_else(|| error("adopted capture missing"))?;
        let plan = linked.plan();
        if linked.previous() != &active
            || plan.current_source() != previous.binding()
            || plan.target() != target.artifact()
            || !operations.try_insert(plan.id(), cx)?
            || snapshot.operation_revision(plan.id()) != Some(linked.revision())
        {
            return Err(error("adoption is inconsistent with its ordered source and operation history"));
        }
        if let Some(receipt_index) = receipts.get(&linked.revision()).copied() {
            let receipt = &state.adoptions()[receipt_index];
            let output = manifests.get(receipt.composition()).ok_or_else(|| error("scoped receipt has no composition"))?;
            let manifest = state.composition(output).ok_or_else(|| error("scoped receipt composition missing"))?;
            let expected_data = manifest.basis().data().identity_with(cx).map_err(app_error)?;
            let expected_session = manifest.basis().session().identity_with(cx).map_err(|failure| match failure {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => ScalarValidationFailure::Allocation(failure),
                _ => error("scope history session identity invariant"),
            })?;
            if linked.active() != output
                || plan.identity_with(cx).map_err(decision_error)? != *receipt.plan()
                || plan.id() != manifest.operation()
                || manifest.basis().active() != &active
                || plan.expected_data() != &expected_data
                || plan.expected_generation() != manifest.basis().data().generation()
                || plan.expected_session() != &expected_session
                || plan.expected_decisions() != manifest.basis().decisions()
                || manifest.basis().revision().checked_add(1) != Some(linked.revision())
                || history.links.try_insert(copy_digest(output, cx)?, (adoption_index, receipt_index), cx)?.is_some()
            {
                return Err(error("scoped plan differs from its exact history checkpoint"));
            }
            let born = births.get(&linked.revision()).ok_or_else(|| error("scoped decision birth checkpoint missing"))?;
            let mut expected = OrderedSet::new();
            for id in born.active.iter() {
                if snapshot.decisions().matches_scope(id, plan) {
                    expected.try_insert(id.as_str(), cx)?;
                }
            }
            let mut actual = OrderedSet::new();
            for id in receipt.decisions() {
                if !snapshot.decisions().contains_decision(id) {
                    return Err(error("scoped receipt decision missing"));
                }
                if !actual.try_insert(id.as_str(), cx)?
                    || !snapshot.decisions().accepted_for_scope(id, plan, target.program_digest()) {
                    return Err(error("scoped decision differs from its exact accepted artifact"));
                }
            }
            if actual.len() != expected.len() || actual.iter().any(|id| !expected.contains(*id)) {
                return Err(error("scoped receipt differs from the exact newly activated intention set"));
            }
        } else if linked.active() != &active
            && (state.composition(&active).is_some() || state.composition(linked.active()).is_some()) {
            return Err(error("managed source change has no scoped adoption receipt"));
        }
        active = copy_digest(linked.active(), cx)?;
    }

    let mut rehearsal_operations: OrderedMap<&str, Vec<&super::DecodedManagedRehearsal>> = OrderedMap::new();
    for (_, proof) in state.rehearsals() {
        let born = births.get(&proof.recorded_revision())
            .ok_or_else(|| error("rehearsal witnesses differ from their exact recording decision births"))?;
        if proof.witness_count() != born.pending.len()
            || proof.witnesses().any(|(id, witness)| {
                !born.pending.contains(id.as_str())
                    || !snapshot.decisions().pending_nonbinary_witness(id, witness)
            })
        { return Err(error("rehearsal witnesses differ from their exact recording decision births")); }
        if !rehearsal_operations.contains_key(proof.recorded_by()) {
            rehearsal_operations.try_insert(proof.recorded_by(), Vec::new(), cx)?;
        }
        crate::product_alloc::try_push(rehearsal_operations.get_mut(proof.recorded_by()).expect("group inserted"), proof, cx, AllocationSite::Vector)?;
    }
    for (_, proofs) in rehearsal_operations.iter() {
        if proofs.len() > 2 || (proofs.len() == 2 && proofs.iter().any(|proof| {
            proof.manifest().transition() != super::ScopeTransition::Evolution
                || proof.manifest().basis() != proofs[0].manifest().basis()
                || proof.manifest().layers() != proofs[0].manifest().layers()
                || proof.manifest().active_layers().ne(proofs[0].manifest().active_layers())
                || !witness_maps_equal(proof, proofs[0])
                || proof.recorded_revision() != proofs[0].recorded_revision()
        })) { return Err(error("paired rehearsal has ambiguous recording or preservation authority")); }
    }

    if active != *snapshot.active_revision()
        || history.links.len() != state.compositions().count()
        || history.links.len() != receipts.len()
    {
        return Err(error("active source or scoped inventory differs from the ordered history tail"));
    }
    history.births = births;
    Ok(history)
}

/// Full scalar-history validation entrypoint. Kept private to ProductStore
/// until CURRENT and backup roots are switched to the decoded owner graph.
pub(crate) fn validate(
    snapshot: &DecodedProjectSnapshot,
    cx: &mut AllocationContext,
) -> Result<(), ScalarValidationFailure> {
    let Some(state) = snapshot.scalar_scope() else {
        return Err(error("scalar scope envelope is required"));
    };
    if state.version() != super::COMPILER_VERSION
        || state.layers().count() > super::MAX_LAYERS
        || state.compositions().count() > crate::product_contract::MAX_ITEMS
        || state.initializations().len() != state.layers().count()
        || state.adoptions().len() != state.compositions().count()
        || state.rehearsals().count() > crate::product_contract::MAX_ITEMS
        || state.correspondences().count() > crate::product_contract::MAX_ITEMS
    {
        return Err(error("invalid scope-state version or inventory"));
    }
    let history = adoption_history(snapshot, cx)?;
    for (_, manifest) in state.compositions() {
        if manifest.layers().iter().any(|id| state.layer(id).is_none()) {
            return Err(error("composition references a missing layer proof"));
        }
    }
    for (_, proof) in state.rehearsals() {
        let own = proof.layer().map(|layer| layer.identity_with(cx).map_err(codec_error)).transpose()?;
        if proof.manifest().layers().iter().any(|id| state.layer(id).is_none() && own.as_ref() != Some(id)) {
            return Err(error("rehearsal references a missing layer proof"));
        }
    }
    let mut declared: OrderedMap<(&str, &str), &crate::product_contract::Type> = OrderedMap::new();
    for program in snapshot.programs() {
        let revision = program.revision_with(cx).map_err(app_error)?;
        if state.composition(&revision).is_some() {
            for entity in &program.app().entities {
                for field in &entity.fields {
                    if field.id.starts_with("gm_scope_") {
                        if let Some(previous) = declared.try_insert((&entity.id, &field.id), &field.value_type, cx)? {
                            if previous != &field.value_type { return Err(error("protected field type changed")); }
                        }
                    }
                }
            }
        } else if !state.rehearsals().any(|(id, _)| id == &revision)
            && has_reserved_namespace(program, cx)?
        {
            return Err(error("authored source uses the reserved metadata namespace"));
        }
    }
    for entity in snapshot.data().schema() {
        for field in &entity.fields {
            if field.id.starts_with("gm_scope_")
                && declared.get(&(entity.id.as_str(), field.id.as_str())) != Some(&&field.value_type)
            {
                return Err(error("unregistered protected storage field"));
            }
        }
    }
    if state.layers().next().is_none() {
        if state.compositions().next().is_some() || !state.initializations().is_empty() || !state.adoptions().is_empty() {
            return Err(error("orphan composition metadata"));
        }
        let active = snapshot.retained_source_program_with(snapshot.active_revision(), cx).map_err(app_error)?
            .ok_or_else(|| error("active source missing"))?;
        if has_reserved_namespace(active, cx)? { return Err(error("authored source uses the reserved metadata namespace")); }
        // Rehearsal and correspondence proof validation follows the same
        // no-layer branch ordering as the legacy verifier.
        let _ = validate_recorded_proofs(snapshot, cx)?;
        return Ok(());
    }
    if state.composition(snapshot.active_revision()).is_none() {
        return Err(error("active source has lost its protected history envelope"));
    }
    let mut initialized = OrderedSet::new();
    let mut operations = OrderedSet::new();
    for receipt in state.initializations() {
        if !initialized.try_insert(receipt.layer(), cx)? || !operations.try_insert(receipt.operation(), cx)? {
            return Err(error("duplicate metadata initialization"));
        }
    }

    validate_layers(snapshot, &history.births, cx)?;
    validate_compositions(snapshot, &history, cx)?;
    let replay = validate_recorded_proofs(snapshot, cx)?;
    validate_history(snapshot, replay, cx)?;
    Ok(())
}

