//! Fallible runtime-authenticated acceptance of the persisted OwnedV2
//! lifecycle witness event. Persisted hashes are only commitments; this
//! private capability is returned after rerunning both Observe witnesses and
//! their lifecycle predicates against decoded owners.
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    AdapterError, AppValidationError, DataSnapshotView, DataValue, FallibleDecodedCapturedProgram,
    FallibleDecodedRunEvidence, IdentityDomain, RuntimeLimits, Type,
};
use crate::product_runtime::LocalRuntime;
use crate::product_store::{fallible_project_snapshot_decode::DecodedProjectSnapshot, StoreError};
use crate::product_backup::fallible_codec::{self, CodecError};
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

use super::common_fact_admission::{refuse, InitialLimitsProfile, ProofMeter};
use super::{DecodedOwnedLayerV2, DecodedOwnedLifecycleV2, DecodedOwnedPreviewRequest};

const PROFILE_ROOT: usize = 4 * (std::mem::size_of::<Option<RuntimeLimits>>()
    + std::mem::size_of::<RuntimeLimits>() + 8 * std::mem::size_of::<usize>()
    + 4 * std::mem::size_of::<Option<&mut InitialLimitsProfile>>());

#[derive(Debug)]
pub(crate) enum LifecycleAcceptanceFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for LifecycleAcceptanceFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}
impl From<AllocationFailure> for LifecycleAcceptanceFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}

/// Opaque in-memory proof that a retained lifecycle event was bound to this
/// exact decoded snapshot, request, source, candidate and regenerated runtime
/// evidence. Its fields cannot be constructed from persisted digests.
pub(crate) struct FallibleAcceptedPreviewLifecycle {
    snapshot: crate::product_contract::Digest,
    request: crate::product_contract::Digest,
    source: crate::product_contract::Digest,
    candidate: crate::product_contract::Digest,
    layer: crate::product_contract::Digest,
    disclosure: crate::product_contract::Digest,
    event: crate::product_contract::Digest,
    origin: &'static str,
}

impl FallibleAcceptedPreviewLifecycle {
    pub(crate) fn identity(&self) -> &crate::product_contract::Digest { &self.event }

    pub(crate) fn reconstruct(
        snapshot: &DecodedProjectSnapshot,
        layer: &DecodedOwnedLayerV2,
        candidate: &FallibleDecodedCapturedProgram,
        request: &DecodedOwnedPreviewRequest,
        meter: &mut ProofMeter,
        profile: Option<&mut InitialLimitsProfile>,
        cx: &mut AllocationContext,
    ) -> Result<Self, LifecycleAcceptanceFailure> {
        let lifecycle = layer.lifecycle();
        if lifecycle.version() != super::OWNED_SCOPE_VERSION {
            return Err(refuse("lifecycle", "unsupported retained acceptance version").into());
        }
        let origin = lifecycle.origin_label();
        let disclosure = disclosure_identity(snapshot, candidate, request, lifecycle, cx)?;
        if disclosure != *lifecycle.disclosure() {
            return Err(refuse("lifecycle", "retained disclosure/event binding differs").into());
        }
        let event = acceptance_event_identity(lifecycle.disclosure(), origin, cx)?;
        if event != *lifecycle.event() {
            return Err(refuse("lifecycle", "retained disclosure/event binding differs").into());
        }

        // The retained common-fact request is a source/frame contract as well
        // as a digest-bound payload. Check those claims before minting a
        // capability: an attacker can recompute the public disclosure/event
        // hashes for a stale request, so the hashes alone do not authenticate
        // its snapshot or candidate source.
        let snapshot_digest = snapshot_identity(snapshot, cx)?;
        let source_digest = candidate_revision_for_snapshot(snapshot, request, cx)?;
        let candidate_digest = candidate.revision_with(cx).map_err(app_error)?;
        if request.snapshot() != &snapshot_digest || request.source_after() != &candidate_digest {
            return Err(refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis").into());
        }

        let accepted = Self {
            snapshot: copy_digest(&snapshot_digest, cx)?,
            request: copy_digest(&request_identity(request, cx)?, cx)?,
            source: copy_digest(&source_digest, cx)?,
            candidate: copy_digest(&candidate_digest, cx)?,
            layer: copy_digest(&layer_identity(layer, cx)?, cx)?,
            disclosure: copy_digest(&disclosure, cx)?,
            event: copy_digest(&event, cx)?,
            origin,
        };
        accepted.verify_profiled(snapshot, layer, candidate, request, meter, profile, cx)?;
        Ok(accepted)
    }

    pub(crate) fn verify_profiled(
        &self,
        snapshot: &DecodedProjectSnapshot,
        layer: &DecodedOwnedLayerV2,
        candidate: &FallibleDecodedCapturedProgram,
        request: &DecodedOwnedPreviewRequest,
        meter: &mut ProofMeter,
        profile: Option<&mut InitialLimitsProfile>,
        cx: &mut AllocationContext,
    ) -> Result<(), LifecycleAcceptanceFailure> {
        const VERIFY_FRAME: u64 = super::owned_lifecycle::AcceptedPreviewLifecycle::PROFILE_VERIFY_FRAME;
        meter.charge(VERIFY_FRAME)?;
        let lifecycle = layer.lifecycle();
        if snapshot_identity(snapshot, cx)? != self.snapshot
            || request_identity(request, cx)? != self.request
            || request.snapshot() != &self.snapshot
            || candidate_revision_for_snapshot(snapshot, request, cx)? != self.source
            || candidate.revision_with(cx).map_err(app_error)? != self.candidate
            || request.source_after() != &self.candidate
            || *layer.candidate() != self.candidate
            || layer_identity(layer, cx)? != self.layer
            || !same_wire(layer.request(), request, cx)?
        {
            return Err(refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis").into());
        }
        if lifecycle.origin_label() != self.origin
            || lifecycle.disclosure() != &self.disclosure
            || lifecycle.event() != &self.event
        {
            return Err(refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis").into());
        }

        let regenerated = replay(snapshot, candidate, request, lifecycle, meter, profile, cx)?;
        if !same_evidence(&regenerated, lifecycle.evidence(), cx)?
            || disclosure_identity(snapshot, candidate, request, lifecycle, cx)? != self.disclosure
            || acceptance_event_identity(&self.disclosure, self.origin, cx)? != self.event
        {
            return Err(refuse("lifecycle acceptance", "independent supporting scene regeneration differs").into());
        }
        Ok(())
    }
}

fn codec_error(error: CodecError) -> LifecycleAcceptanceFailure {
    match error {
        CodecError::Allocation(failure) => LifecycleAcceptanceFailure::Allocation(failure),
        _ => LifecycleAcceptanceFailure::Store(StoreError::Invalid("lifecycle identity invariant".into())),
    }
}
fn app_error(error: AppValidationError) -> LifecycleAcceptanceFailure {
    match error {
        AppValidationError::Allocation(failure) => LifecycleAcceptanceFailure::Allocation(failure),
        AppValidationError::Contract(error) => LifecycleAcceptanceFailure::Store(StoreError::Invalid(error.0)),
    }
}
fn runtime_error(error: AdapterError) -> LifecycleAcceptanceFailure {
    match error {
        AdapterError::Allocation(failure) => LifecycleAcceptanceFailure::Allocation(failure),
        error => LifecycleAcceptanceFailure::Store(StoreError::Runtime(error)),
    }
}
fn snapshot_identity(
    snapshot: &DecodedProjectSnapshot,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    fallible_codec::canonical_digest(IdentityDomain::Data, snapshot, cx).map_err(codec_error)
}
fn layer_identity(
    layer: &DecodedOwnedLayerV2,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    fallible_codec::canonical_digest(IdentityDomain::Adoption, &LayerIdentity(layer), cx).map_err(codec_error)
}
struct LayerIdentity<'a>(&'a DecodedOwnedLayerV2);
impl Serialize for LayerIdentity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element("owned-scope-layer/2")?;
        tuple.serialize_element(self.0)?;
        tuple.end()
    }
}
impl fallible_codec::BackupWire for LayerIdentity<'_> {}
fn request_identity(
    request: &DecodedOwnedPreviewRequest,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    request.identity_with(cx).map_err(codec_error)
}
fn candidate_revision_for_snapshot(
    snapshot: &DecodedProjectSnapshot,
    request: &DecodedOwnedPreviewRequest,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    let source = snapshot.active_revision();
    if request.source_before() != source {
        return Err(refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis").into());
    }
    let source_program = snapshot.retained_source_program_with(source, cx)
        .map_err(app_error)?.ok_or_else(|| refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis"))?;
    source_program.revision_with(cx).map_err(app_error)
}
fn disclosure_identity(
    snapshot: &DecodedProjectSnapshot,
    candidate: &FallibleDecodedCapturedProgram,
    request: &DecodedOwnedPreviewRequest,
    lifecycle: &DecodedOwnedLifecycleV2,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    fallible_codec::canonical_digest(
        IdentityDomain::Evidence,
        &DisclosureIdentity(snapshot, candidate, request, lifecycle),
        cx,
    ).map_err(codec_error)
}
fn acceptance_event_identity(
    disclosure: &crate::product_contract::Digest,
    origin: &str,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    fallible_codec::canonical_digest(
        IdentityDomain::Evidence,
        &AcceptanceEventIdentity(disclosure, origin),
        cx,
    ).map_err(codec_error)
}
struct DisclosureIdentity<'a>(
    &'a DecodedProjectSnapshot,
    &'a FallibleDecodedCapturedProgram,
    &'a DecodedOwnedPreviewRequest,
    &'a DecodedOwnedLifecycleV2,
);
impl Serialize for DisclosureIdentity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(6)?;
        tuple.serialize_element("preview-lifecycle-disclosure/1")?;
        tuple.serialize_element(self.0)?;
        tuple.serialize_element(self.1)?;
        tuple.serialize_element(self.2)?;
        tuple.serialize_element(self.3.witnesses())?;
        tuple.serialize_element(self.3.evidence())?;
        tuple.end()
    }
}
impl fallible_codec::BackupWire for DisclosureIdentity<'_> {}
struct AcceptanceEventIdentity<'a>(&'a crate::product_contract::Digest, &'a str);
impl Serialize for AcceptanceEventIdentity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(3)?;
        tuple.serialize_element("host-lifecycle-acceptance/1")?;
        tuple.serialize_element(self.0)?;
        tuple.serialize_element(self.1)?;
        tuple.end()
    }
}
impl fallible_codec::BackupWire for AcceptanceEventIdentity<'_> {}
fn copy_digest(
    value: &crate::product_contract::Digest,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::Digest, LifecycleAcceptanceFailure> {
    let owned = product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    crate::product_contract::Digest::from_owned_lower_hex(owned)
        .ok_or_else(|| LifecycleAcceptanceFailure::Store(StoreError::Invalid("lifecycle digest copy invariant".into())))
}
fn same_wire<T: fallible_codec::BackupWire + ?Sized>(
    left: &T,
    right: &T,
    cx: &mut AllocationContext,
) -> Result<bool, LifecycleAcceptanceFailure> {
    let left = fallible_codec::canonical_bytes(left, cx).map_err(codec_error)?;
    let right = fallible_codec::canonical_bytes(right, cx).map_err(codec_error)?;
    Ok(left == right)
}
fn same_evidence(
    actual: &[FallibleDecodedRunEvidence],
    expected: &[FallibleDecodedRunEvidence],
    cx: &mut AllocationContext,
) -> Result<bool, LifecycleAcceptanceFailure> {
    if actual.len() != expected.len() { return Ok(false); }
    for (actual, expected) in actual.iter().zip(expected) {
        if !same_wire(actual, expected, cx)? { return Ok(false); }
    }
    Ok(true)
}

fn replay(
    snapshot: &DecodedProjectSnapshot,
    candidate: &FallibleDecodedCapturedProgram,
    request: &DecodedOwnedPreviewRequest,
    lifecycle: &DecodedOwnedLifecycleV2,
    meter: &mut ProofMeter,
    mut profile: Option<&mut InitialLimitsProfile>,
    cx: &mut AllocationContext,
) -> Result<Vec<FallibleDecodedRunEvidence>, LifecycleAcceptanceFailure> {
    meter.charge(PROFILE_ROOT as u64 + 32)?;
    let witnesses = lifecycle.witnesses();
    if witnesses.len() != 2
        || witnesses[0].side() != super::owned_contract::OwnedSourceSide::Before
        || witnesses[1].side() != super::owned_contract::OwnedSourceSide::After
    {
        return Err(refuse("lifecycle witnesses", "exact before and after supporting scenes required").into());
    }
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    let mut result = Vec::new();
    product_alloc::try_reserve(&mut result, 2, cx, AllocationSite::Vector)?;
    for witness in witnesses {
        let scenario = witness.scenario();
        let current_data = snapshot.data().identity_with(cx).map_err(app_error)?;
        let scenario_data = scenario.seed_identity_with(cx).map_err(codec_error)?;
        if scenario_data != current_data {
            return Err(refuse("lifecycle witness", "supporting scene is not the exact current frame/observation").into());
        }
        let current_session = snapshot.session().identity_with(cx).map_err(codec_error)?;
        let scenario_session = scenario.session_identity_with(cx).map_err(codec_error)?;
        if scenario_session != current_session {
            return Err(refuse("lifecycle witness", "supporting scene is not the exact current frame/observation").into());
        }
        if scenario.clock_day() != snapshot.clock_day()
            || scenario.input_count() != 1
            || scenario.single_observe_point().is_none()
            || scenario.has_validity()
        {
            return Err(refuse("lifecycle witness", "supporting scene is not the exact current frame/observation").into());
        }
        let completed = witness.completed();
        let unfinished = witness.unfinished();
        if completed == unfinished
            || completed.entity != request.root_entity()
            || unfinished.entity != request.root_entity()
        {
            return Err(refuse("lifecycle witness", "distinct exact-root completed and unfinished witnesses required").into());
        }
        let (program, predicate) = match witness.side() {
            super::owned_contract::OwnedSourceSide::Before => {
                let before = snapshot.captured_program_by_revision_with(snapshot.active_revision(), cx)
                    .map_err(app_error)?
                    .ok_or_else(|| refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis"))?;
                if before.revision_with(cx).map_err(app_error)? != *request.source_before() {
                    return Err(refuse("lifecycle acceptance", "stale source, snapshot, policy, meaning or decision basis").into());
                }
                (before, request.lifecycle_predicates().0)
            }
            super::owned_contract::OwnedSourceSide::After => (candidate, request.lifecycle_predicates().1),
        };
        let run_limits = meter.runtime()?;
        let run = runtime.replay_decoded_observe_with(
            program, scenario, snapshot.decisions(), run_limits, "lifecycle-support", cx,
        ).map_err(runtime_error)?;
        meter.charge(2)?;
        if let Some(profile) = profile.as_deref_mut() { profile.observe(run.limits(), false, meter)?; }
        if run.state() != crate::product_contract::EvidenceState::Observed
            || run.observation_count() != 1
            || run.trace_count() != 1
        {
            return Err(refuse("lifecycle witness", "concrete LocalRuntime did not complete supporting observation").into());
        }

        let data = snapshot.data().merge_app_with(program.app(), cx).map_err(app_error)?;
        for (row, wanted) in [(completed, true), (unfinished, false)] {
            let row_view = DataSnapshotView::from(&data).record(&row.entity, &row.record);
            if !row_view.is_some_and(|row| !row.archived()) {
                return Err(refuse("lifecycle witness", "active witness row missing").into());
            }
            let limits = meter.runtime()?;
            meter.charge(2)?;
            let profiled_limits = if profile.is_some() { Some(limits.clone()) } else { None };
            if profile.is_some() {
                meter.charge(64)?;
            }
            let (actual, new_allocations) = runtime.evaluate_decoded_record_projection_and_allocations(
                program, &data, row, "record", predicate, &Type::Boolean,
                snapshot.clock_day(), limits, std::mem::replace(cx, AllocationContext::new()),
            ).map_err(|(error, allocations)| {
                *cx = allocations;
                runtime_error(error)
            })?;
            *cx = new_allocations;
            meter.charge(2)?;
            if let (Some(profile), Some(limits)) = (profile.as_deref_mut(), profiled_limits.as_ref()) {
                profile.observe(limits, false, meter)?;
            }
            if actual != (DataValue::Boolean { value: wanted }) {
                return Err(refuse("lifecycle witness", "replayed completed/unfinished interpretation disagrees").into());
            }
        }
        product_alloc::try_push(&mut result, run, cx, AllocationSite::Vector)?;
        meter.charge(1)?;
    }
    Ok(result)
}
