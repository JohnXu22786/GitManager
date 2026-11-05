//! Allocation-aware scalar scope metadata initialization over decoded owners.
use super::{compiler, EffectDestinationDto, ScopePopulationDto, StoreError};
use super::fallible_scalar_decode::{DecodedMetadataAddition, DecodedMetadataInitializationReceipt, DecodedScopeLayer};
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    AdapterError, AppValidationError, DataRecordView, DataSnapshotView, DataValue, Digest, FallibleDecodedCapturedProgram,
    FallibleDecodedDataSnapshot, FallibleDataSnapshotCloneError, FallibleValues,
    RecordRef, RuntimeLimits, Type,
};
use crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot;
use crate::product_runtime::LocalRuntime;

#[derive(Debug)]
pub(crate) enum InitializationFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for InitializationFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}
impl From<AllocationFailure> for InitializationFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}

fn app_error(error: AppValidationError) -> InitializationFailure {
    match error {
        AppValidationError::Allocation(failure) => InitializationFailure::Allocation(failure),
        AppValidationError::Contract(error) => InitializationFailure::Store(StoreError::Invalid(error.0)),
    }
}
fn clone_error(error: FallibleDataSnapshotCloneError) -> InitializationFailure {
    match error {
        FallibleDataSnapshotCloneError::Allocation(failure) => InitializationFailure::Allocation(failure),
        FallibleDataSnapshotCloneError::Invariant => InitializationFailure::Store(StoreError::Invalid("data snapshot copy invariant".into())),
    }
}
fn codec_error(error: crate::product_backup::fallible_codec::CodecError) -> InitializationFailure {
    match error {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => InitializationFailure::Allocation(failure),
        _ => InitializationFailure::Store(StoreError::Invalid("initialization identity encoding failed".into())),
    }
}
fn compiler_error(error: compiler::CompilerFailure) -> InitializationFailure {
    match error {
        compiler::CompilerFailure::Store(error) => InitializationFailure::Store(error),
        compiler::CompilerFailure::Allocation(failure) => InitializationFailure::Allocation(failure),
    }
}
fn runtime_error(error: AdapterError) -> InitializationFailure {
    match error {
        AdapterError::Allocation(failure) => InitializationFailure::Allocation(failure),
        error => InitializationFailure::Store(StoreError::Runtime(error)),
    }
}
fn copied_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, InitializationFailure> {
    let text = product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    Ok(Digest::from_owned_lower_hex(text).expect("source digest was already validated"))
}
fn copied_record_ref(row: DataRecordView<'_>, cx: &mut AllocationContext) -> Result<RecordRef, InitializationFailure> {
    let entity = product_alloc::try_copy_str(row.entity(), cx, AllocationSite::String)?;
    let record = product_alloc::try_copy_str(row.id(), cx, AllocationSite::String)?;
    Ok(RecordRef { entity, record })
}
fn initial_member(layer: &DecodedScopeLayer, row: &RecordRef) -> bool {
    if layer.request().excluded_records.iter().any(|excluded| excluded == row) { return false; }
    match &layer.request().population {
        ScopePopulationDto::All => true,
        ScopePopulationDto::FutureWork => false,
        ScopePopulationDto::SelectedUnfinished { records } => records.iter().any(|selected| selected == row),
    }
}
fn is_projection(destination: &EffectDestinationDto) -> bool {
    matches!(destination, EffectDestinationDto::ViewColumn { .. } | EffectDestinationDto::EmitColumn { .. })
}

/// Produce the exact initial protected metadata frame without mutating any
/// input owner. All output, key, expression and runtime allocation work shares
/// the caller's single allocation context.
pub(crate) fn initialize(
    snapshot: &DecodedProjectSnapshot,
    layer: &DecodedScopeLayer,
    target: &FallibleDecodedCapturedProgram,
    cx: &mut AllocationContext,
) -> Result<(FallibleDecodedDataSnapshot, Vec<DecodedMetadataAddition>), InitializationFailure> {
    let id = layer.identity_with(cx).map_err(codec_error)?;
    let before = layer.basis().data();
    initialize_frame(snapshot, layer, &id, before,
        layer.basis().active(), target, layer.basis().day(), "CapturedAtAdoption", cx)
}

pub(crate) fn initialization_receipt(
    snapshot: &DecodedProjectSnapshot,
    layer: &DecodedScopeLayer,
    before: &FallibleDecodedDataSnapshot,
    after: &FallibleDecodedDataSnapshot,
    additions: &[DecodedMetadataAddition],
    cx: &mut AllocationContext,
) -> Result<DecodedMetadataInitializationReceipt, InitializationFailure> {
    let id = layer.identity_with(cx).map_err(codec_error)?;
    // Keep the legacy order: receipt identities are computed only after the
    // complete frame has validated and proved to retain all business facts.
    let operation = product_alloc::try_copy_str(layer.operation(), cx, AllocationSite::String)?;
    let before_data = before.identity_with(cx).map_err(app_error)?;
    let after_data = after.identity_with(cx).map_err(app_error)?;
    let before_schema = before.schema_identity_with(cx).map_err(app_error)?;
    let after_schema = after.schema_identity_with(cx).map_err(app_error)?;
    let business_projection = before.business_projection_with(cx).map_err(codec_error)?;
    let original_events = before.events_digest_with(cx).map_err(codec_error)?;
    let capture = snapshot.retained_source_program_with(layer.basis().active(), cx)
        .map_err(app_error)?
        .ok_or_else(|| StoreError::Invalid("retained source revision missing".into()))?;
    let capture_program = copied_digest(capture.program_digest(), cx)?;
    let prior_snapshot = copied_digest(layer.basis().snapshot(), cx)?;
    let adoption_revision = layer.basis().revision().checked_add(1)
        .ok_or_else(|| StoreError::Invalid("revision exhausted".into()))?;
    let mut copied_additions = Vec::new();
    product_alloc::try_reserve(&mut copied_additions, additions.len(), cx, AllocationSite::Vector)?;
    for addition in additions {
        let entity = product_alloc::try_copy_str(&addition.record().entity, cx, AllocationSite::String)?;
        let record = product_alloc::try_copy_str(&addition.record().record, cx, AllocationSite::String)?;
        let record = RecordRef { entity, record };
        let values = addition.values().try_clone_with(cx)?;
        product_alloc::try_push(&mut copied_additions, DecodedMetadataAddition::new(record, values), cx, AllocationSite::Vector)?;
    }
    Ok(DecodedMetadataInitializationReceipt::new(
        operation, id, before_data, after_data, before_schema, after_schema, copied_additions,
        business_projection, original_events, before.record_count(), layer.basis().day(),
        capture_program, prior_snapshot, adoption_revision,
    ))
}

/// Shared frame helper for adoption initialization and historical replay.
pub(crate) fn initialize_frame(
    snapshot: &DecodedProjectSnapshot,
    layer: &DecodedScopeLayer,
    id: &Digest,
    before: &FallibleDecodedDataSnapshot,
    source: &Digest,
    target: &FallibleDecodedCapturedProgram,
    day: i32,
    origin: &str,
    cx: &mut AllocationContext,
) -> Result<(FallibleDecodedDataSnapshot, Vec<DecodedMetadataAddition>), InitializationFailure> {
    let mut after = before.merge_app_with(target.app(), cx).map_err(app_error)?;
    let baseline = snapshot.retained_source_program_with(source, cx)
        .map_err(app_error)?
        .ok_or_else(|| StoreError::Invalid("retained source revision missing".into()))?;

    // Projection destinations belong to the pre-instrumentation envelope app;
    // the captured composition additionally rewrites actions and provenance.
    let envelope_baseline = if let Some(manifest) = snapshot.scalar_scope().and_then(|scope| scope.composition(source)) {
        Some(compiler::envelope_app_fallible(snapshot, manifest, cx).map_err(compiler_error)?)
    } else { None };
    let baseline_app = envelope_baseline.as_ref().unwrap_or_else(|| baseline.app());

    let rows = DataSnapshotView::from(before).records();
    let mut additions = Vec::new();
    product_alloc::try_reserve(&mut additions, before.record_count(), cx, AllocationSite::Vector)?;
    for row in rows {
        let Some(lifecycle) = layer.request().lifecycles().iter().find(|lifecycle| lifecycle.entity() == row.entity()) else {
            continue;
        };
        let reference = copied_record_ref(row, cx)?;
        let sealed = if row.archived() { true } else {
            let value = LocalRuntime::default().evaluate_decoded_record_projection_and_allocations(
                baseline, before, &reference, "record", lifecycle.completed(), &Type::Boolean,
                day, RuntimeLimits::default(), std::mem::replace(cx, AllocationContext::new()),
            );
            let value = match value {
                Ok((value, allocations)) => { *cx = allocations; value }
                Err((error, allocations)) => { *cx = allocations; return Err(runtime_error(error).into()); }
            };
            match value {
                DataValue::Boolean { value } => value,
                _ => return Err(StoreError::Invalid("completion predicate is not Boolean".into()).into()),
            }
        };
        let mut values = FallibleValues::new();
        let member = compiler::key_fallible(id, row.entity(), "member", cx).map_err(compiler_error)?;
        values.try_insert(member, DataValue::Boolean { value: initial_member(layer, &reference) }, cx)?;
        let sealed_key = compiler::key_fallible(id, row.entity(), "sealed", cx).map_err(compiler_error)?;
        values.try_insert(sealed_key, DataValue::Boolean { value: sealed }, cx)?;
        if sealed {
            let day_key = compiler::key_fallible(id, row.entity(), "day", cx).map_err(compiler_error)?;
            values.try_insert(day_key, DataValue::Date { days: day }, cx)?;
            let origin_key = compiler::key_fallible(id, row.entity(), "origin", cx).map_err(compiler_error)?;
            let origin_value = product_alloc::try_copy_str(origin, cx, AllocationSite::String)?;
            values.try_insert(origin_key, DataValue::Text { value: origin_value }, cx)?;
            for patch in layer.patches() {
                let request = patch.request();
                if request.entity != row.entity() || !is_projection(&request.destination) { continue; }
                let expression = compiler::slot_fallible_ref(baseline_app, &request.destination)
                    .map_err(InitializationFailure::from)?;
                let evaluated = LocalRuntime::default().evaluate_decoded_record_projection_and_allocations(
                    baseline, before, &reference, &request.subject, expression, &request.value_type,
                    day, RuntimeLimits::default(), std::mem::replace(cx, AllocationContext::new()),
                );
                let evaluated = match evaluated {
                    Ok((value, allocations)) => { *cx = allocations; value }
                    Err((error, allocations)) => { *cx = allocations; return Err(runtime_error(error).into()); }
                };
                let value_key = compiler::saved_key_fallible(id, patch, cx).map_err(compiler_error)?;
                values.try_insert(value_key, evaluated, cx)?;
            }
        }
        if values.iter().any(|(key, _)| row.values().get(key).is_some()) {
            return Err(StoreError::Invalid("new system field collides with stored data".into()).into());
        }
        let changed = after.record_values_mut(row.entity(), row.id())
            .ok_or_else(|| StoreError::Invalid("initialization lost a record".into()))?;
        for (key, value) in values.iter() {
            let key = product_alloc::try_copy_str(key, cx, AllocationSite::String)?;
            let value = crate::product_contract::try_clone_data_value_with(value, cx)?;
            changed.try_insert(key, value, cx)?;
        }
        product_alloc::try_push(&mut additions, DecodedMetadataAddition::new(reference, values), cx, AllocationSite::Vector)?;
    }
    after.validate_with(cx).map_err(app_error)?;
    let unchanged = after.business_projection_matches_after_removing(
        before,
        additions.iter().map(|addition| (addition.record(), addition.values())),
        cx,
    ).map_err(clone_error)?;
    if !unchanged {
        return Err(StoreError::Invalid("initialization changed existing business facts".into()).into());
    }
    Ok((after, additions))
}
