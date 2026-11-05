//! Fallible OwnedV2 capture of the persisted projection evidence.
//!
//! The capture runs directly over decoded owners. Runtime evaluation receives
//! the single charged allocation context used by the whole capture, while the
//! enclosing proof meter pays the complete runtime reservation before each
//! row and each native projection evaluation.

use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    self, AdapterError, AppValidationError, DataSnapshotView, DataValue,
    FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot, FallibleValues,
    Expr, RecordRef, RuntimeLimits, Type,
};
use crate::product_runtime::LocalRuntime;
use crate::product_store::StoreError;

use super::common_fact_admission::{refuse, InitialLimitsProfile, ProofMeter};
use super::fallible_owned_inventory_decode::{DecodedOwnedInventoryV2, DecodedOwnedProjectionRecipe};
use super::fallible_owned_request_decode::DecodedOwnedPreviewRequest;
use super::fallible_scalar_decode::{FallibleScopeMap, ScopePopulationDto};

#[derive(Debug)]
pub(crate) enum DecodedCaptureFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<StoreError> for DecodedCaptureFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}
impl From<AllocationFailure> for DecodedCaptureFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}

fn app_error(error: AppValidationError) -> DecodedCaptureFailure {
    match error {
        AppValidationError::Allocation(failure) => DecodedCaptureFailure::Allocation(failure),
        AppValidationError::Contract(error) => {
            DecodedCaptureFailure::Store(StoreError::Runtime(AdapterError::Invalid(error)))
        }
    }
}

fn runtime_error(error: AdapterError) -> DecodedCaptureFailure {
    match error {
        AdapterError::Allocation(failure) => DecodedCaptureFailure::Allocation(failure),
        error => DecodedCaptureFailure::Store(StoreError::Runtime(error)),
    }
}

fn allocation_decode_error(error: crate::product_contract::fallible_value_decode::ValueDecodeError)
    -> DecodedCaptureFailure
{
    match error {
        crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(failure) => {
            DecodedCaptureFailure::Allocation(failure)
        }
        _ => DecodedCaptureFailure::Store(StoreError::Invalid("decoded capture map invariant".into())),
    }
}

fn charge(meter: &mut ProofMeter, units: u64) -> Result<(), DecodedCaptureFailure> {
    meter.charge(units).map_err(|error| DecodedCaptureFailure::Store(error))
}

fn paid_refusal(at: &str, rule: &str, meter: &mut ProofMeter) -> Result<StoreError, DecodedCaptureFailure> {
    super::common_fact_admission::capture_paid_refusal(at, rule, &mut |units| {
        meter.charge_cause(units).map_err(|cause| match cause {
            super::common_fact_admission::ProofChargeFailure::Cancelled => AdapterError::Cancelled,
            super::common_fact_admission::ProofChargeFailure::Elapsed => AdapterError::BudgetExhausted(
                "cumulative elapsed budget exhausted".into(),
            ),
            super::common_fact_admission::ProofChargeFailure::Fuel => AdapterError::BudgetExhausted(
                "cumulative proof/initialization budget exhausted".into(),
            ),
        })
    }).map_err(DecodedCaptureFailure::Store)
}

fn string_equal(left: &str, right: &str, meter: &mut ProofMeter) -> Result<bool, DecodedCaptureFailure> {
    charge(meter, 2)?;
    let (left_len, right_len) = (left.len(), right.len());
    charge(meter, 2)?;
    if left_len != right_len {
        charge(meter, 3)?;
        return Ok(false);
    }
    charge(meter, 6)?;
    let units = u64::try_from(left_len).ok().and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(3))
        .ok_or_else(|| DecodedCaptureFailure::Store(refuse("budget", "capture work arithmetic overflow")))?;
    charge(meter, units)?;
    Ok(left == right)
}

fn copy_string(value: &str, meter: &mut ProofMeter, cx: &mut AllocationContext)
    -> Result<String, DecodedCaptureFailure>
{
    charge(meter, 8)?;
    let units = u64::try_from(value.len()).ok().and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(3))
        .ok_or_else(|| DecodedCaptureFailure::Store(refuse("budget", "capture work arithmetic overflow")))?;
    charge(meter, units)?;
    product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into)
}

fn insert_map<K: Ord, V>(map_len: usize, key_len: usize, meter: &mut ProofMeter)
    -> Result<(), DecodedCaptureFailure>
{
    charge(meter, 8)?;
    let comparator = u64::try_from(key_len).ok().and_then(|n| n.checked_mul(2)).and_then(|n| n.checked_add(3))
        .ok_or_else(|| DecodedCaptureFailure::Store(StoreError::Runtime(
            super::common_fact_admission::capture_arithmetic_error(),
        )))?;
    const INSERT_DESCRIPTOR: u64 = 104 + 4 * usize::BITS as u64;
    charge(meter, INSERT_DESCRIPTOR)?;
    let native = product_contract::capture_payment::btree_insert::<String, V>(map_len, comparator)
        .map_err(|error| DecodedCaptureFailure::Store(StoreError::Runtime(
            super::common_fact_admission::capture_formula_error(error),
        )))?;
    charge(meter, native)
}

fn insert<V>(map: &mut FallibleScopeMap<String, V>, key: String, value: V,
    meter: &mut ProofMeter, cx: &mut AllocationContext) -> Result<(), DecodedCaptureFailure>
{
    insert_map::<String, V>(map.len(), key.len(), meter)?;
    map.insert(key, value, cx).map_err(allocation_decode_error)?;
    Ok(())
}

fn insert_value(map: &mut FallibleValues, key: String, value: DataValue,
    meter: &mut ProofMeter, cx: &mut AllocationContext) -> Result<(), DecodedCaptureFailure>
{
    insert_map::<String, DataValue>(map.len(), key.len(), meter)?;
    map.try_insert(key, value, cx).map_err(DecodedCaptureFailure::Allocation)?;
    Ok(())
}

fn account_output(value: &DataValue, output_bytes: &mut usize, meter: &mut ProofMeter)
    -> Result<(), DecodedCaptureFailure>
{
    *output_bytes = crate::product_runtime::decoded_capture_account_output(
        value, *output_bytes, &RuntimeLimits::default(), meter,
    ).map_err(runtime_error)?;
    Ok(())
}

fn evaluate(
    runtime: &LocalRuntime,
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    reference: &RecordRef,
    binding: &str,
    expression: &Expr,
    expected: &Type,
    day: i32,
    limits: RuntimeLimits,
    cx: &mut AllocationContext,
) -> Result<DataValue, DecodedCaptureFailure> {
    let allocations = std::mem::replace(cx, AllocationContext::new());
    match runtime.evaluate_decoded_record_projection_and_allocations(
        program, data, reference, binding, expression, expected, day, limits, allocations,
    ) {
        Ok((value, allocations)) => { *cx = allocations; Ok(value) }
        Err((error, allocations)) => { *cx = allocations; Err(runtime_error(error)) }
    }
}

fn selected(reference: &RecordRef, request: &DecodedOwnedPreviewRequest, meter: &mut ProofMeter)
    -> Result<bool, DecodedCaptureFailure>
{
    let ScopePopulationDto::SelectedUnfinished { records } = request.capture_population() else { return Ok(false) };
    charge(meter, 1)?;
    let mut iter = records.iter();
    loop {
        charge(meter, 1)?;
        let Some(candidate) = iter.next() else { return Ok(false) };
        charge(meter, 2)?;
        let entity = string_equal(&candidate.entity, &reference.entity, meter)?;
        charge(meter, 1)?;
        if !entity { continue; }
        charge(meter, 2)?;
        if string_equal(&candidate.record, &reference.record, meter)? { return Ok(true) }
    }
}

fn capture_row_values(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    reference: &RecordRef,
    day: i32,
    projections: &[DecodedOwnedProjectionRecipe],
    meter: &mut ProofMeter,
    output_bytes: &mut usize,
    cx: &mut AllocationContext,
) -> Result<FallibleValues, DecodedCaptureFailure> {
    charge(meter, 8)?;
    let mut values = FallibleValues::new();
    charge(meter, 1)?;
    let mut recipes = projections.iter();
    loop {
        charge(meter, 1)?;
        let Some(projection) = recipes.next() else { break };
        charge(meter, 4)?;
        let value = if let Some(expression) = projection.baseline() {
            let limits = meter.runtime().map_err(DecodedCaptureFailure::Store)?;
            let runtime = LocalRuntime::with_cancellation(meter.cancelled());
            let value = evaluate(&runtime, program, data, reference, projection.binding(), expression,
                projection.value_type(), day, limits, cx)?;
            crate::product_runtime::decoded_capture_validate_value(&value, projection.value_type(), meter)
                .map_err(runtime_error)?;
            account_output(&value, output_bytes, meter)?;
            value
        } else {
            charge(meter, 1)?;
            let Some(value) = projection.absent_value() else {
                return Err(DecodedCaptureFailure::Store(paid_refusal(projection.location(),
                    "absent baseline has no explicit availability rule", meter)?));
            };
            crate::product_runtime::decoded_capture_validate_value(value, projection.host_value_type(), meter)
                .map_err(runtime_error)?;
            account_output(value, output_bytes, meter)?;
            crate::product_runtime::decoded_capture_value_copy_cost(value, meter)
                .map_err(runtime_error)?;
            product_contract::try_clone_data_value_with(value, cx).map_err(DecodedCaptureFailure::Allocation)?
        };
        charge(meter, 1)?;
        let key = copy_string(projection.location(), meter, cx)?;
        insert_value(&mut values, key, value, meter, cx)?;
        charge(meter, 1)?;
    }
    Ok(values)
}

/// Regenerate the captured row/value map from decoded persisted owners.
pub(crate) fn capture_decoded_frame(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    day: i32,
    request: &DecodedOwnedPreviewRequest,
    projections: &[DecodedOwnedProjectionRecipe],
    meter: &mut ProofMeter,
    mut profile: Option<&mut InitialLimitsProfile>,
    cx: &mut AllocationContext,
) -> Result<FallibleScopeMap<String, FallibleValues>, DecodedCaptureFailure> {
    charge(meter, super::common_fact_admission::INITIAL_CAPTURE_PROFILE_TRANSPORT)?;
    charge(meter, 13)?;
    charge(meter, crate::product_runtime::projection_capture_entry_units::<ProofMeter>())?;
    let mut captured = FallibleScopeMap::new();
    let view = DataSnapshotView::from(data);
    let mut owners_validated = false;
    charge(meter, 1)?;
    let mut records = view.records();
    loop {
        charge(meter, 1)?;
        let Some(record) = records.next() else { break };
        charge(meter, 5)?;
        let matches = string_equal(record.entity(), request.root_entity(), meter)?;
        charge(meter, 1)?;
        if !matches { continue; }
        charge(meter, 1)?;
        charge(meter, 2)?;
        let entity = copy_string(record.entity(), meter, cx)?;
        let id = copy_string(record.id(), meter, cx)?;
        charge(meter, 4)?;
        let reference = RecordRef { entity, record: id };

        // The lifecycle predicate is the first evaluation in the legacy row
        // capture, so reserve its complete runtime before evaluating it.
        let row_limits = meter.runtime().map_err(DecodedCaptureFailure::Store)?;
        if !owners_validated {
            program.validate_with(cx).map_err(app_error)?;
            data.validate_with(cx).map_err(app_error)?;
            owners_validated = true;
        }
        let runtime = LocalRuntime::with_cancellation(meter.cancelled());
        let mut output_bytes = 0usize;
        let lifecycle = evaluate(&runtime, program, data, &reference, "record",
            request.capture_lifecycle_before(), &Type::Boolean, day, row_limits.clone(), cx)?;
        crate::product_runtime::decoded_capture_validate_value(&lifecycle, &Type::Boolean, meter)
            .map_err(runtime_error)?;
        account_output(&lifecycle, &mut output_bytes, meter)?;
        let completed = match lifecycle {
            DataValue::Boolean { value } => value,
            _ => return Err(DecodedCaptureFailure::Store(StoreError::Invalid(
                "decoded lifecycle predicate returned a non-boolean".into(),
            ))),
        };
        if let Some(profile) = profile.as_deref_mut() {
            charge(meter, 256)?;
            profile.observe_paid(&row_limits, true).map_err(|cause| DecodedCaptureFailure::Store(
                StoreError::Runtime(super::common_fact_admission::capture_resource_error(cause)),
            ))?;
        }

        charge(meter, 2)?;
        let selected = selected(&reference, request, meter)?;
        charge(meter, 1)?;
        if selected {
            charge(meter, 1)?;
            if completed {
                return Err(DecodedCaptureFailure::Store(paid_refusal("scope", "selected root is already completed", meter)?));
            }
        }
        charge(meter, 2)?;
        let values = if !record.archived() {
            charge(meter, 1)?;
            if completed {
                Some(capture_row_values(program, data, &reference, day, projections, meter, &mut output_bytes, cx)?)
            } else { None }
        } else {
            Some(capture_row_values(program, data, &reference, day, projections, meter, &mut output_bytes, cx)?)
        };
        charge(meter, 1)?;
        if let Some(values) = values {
            charge(meter, 1)?;
            let key = copy_string(record.id(), meter, cx)?;
            insert(&mut captured, key, values, meter, cx)?;
        }
    }
    charge(meter, 1)?;
    Ok(captured)
}

/// Regenerate and compare the decoded captured evidence without materializing
/// either side as a legacy map.
pub(crate) fn validate_decoded_capture(
    inventory: &DecodedOwnedInventoryV2,
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    day: i32,
    request: &DecodedOwnedPreviewRequest,
    meter: &mut ProofMeter,
    profile: Option<&mut InitialLimitsProfile>,
    cx: &mut AllocationContext,
) -> Result<bool, DecodedCaptureFailure> {
    let regenerated = capture_decoded_frame(program, data, day, request, inventory.projections(), meter, profile, cx)?;
    capture_maps_equal(&regenerated, inventory.captured(), meter)
}

fn type_equal(left: &Type, right: &Type, meter: &mut ProofMeter) -> Result<bool, DecodedCaptureFailure> {
    charge(meter, 1)?;
    Ok(match (left, right) {
        (Type::List { item: left }, Type::List { item: right }) => type_equal(left, right, meter)?,
        (Type::Optional { item: left }, Type::Optional { item: right }) => type_equal(left, right, meter)?,
        (Type::Reference { entity: left }, Type::Reference { entity: right }) => string_equal(left, right, meter)?,
        (left, right) => left == right,
    })
}

fn value_equal(left: &DataValue, right: &DataValue, meter: &mut ProofMeter) -> Result<bool, DecodedCaptureFailure> {
    charge(meter, 1)?;
    match (left, right) {
        (DataValue::Null, DataValue::Null) => Ok(true),
        (DataValue::Boolean { value: left }, DataValue::Boolean { value: right }) => {
            charge(meter, 1)?;
            Ok(left == right)
        }
        (DataValue::Integer { value: left }, DataValue::Integer { value: right }) => {
            charge(meter, 1)?;
            Ok(left == right)
        }
        (DataValue::Text { value: left }, DataValue::Text { value: right }) => string_equal(left, right, meter),
        (DataValue::Date { days: left }, DataValue::Date { days: right }) => {
            charge(meter, 1)?;
            Ok(left == right)
        }
        (DataValue::Reference { entity: left_entity, record: left_record },
            DataValue::Reference { entity: right_entity, record: right_record }) => {
            if !string_equal(left_entity, right_entity, meter)? { return Ok(false); }
            string_equal(left_record, right_record, meter)
        }
        (DataValue::List { item_type: left_type, items: left_items },
            DataValue::List { item_type: right_type, items: right_items }) => {
            if !type_equal(left_type, right_type, meter)? { return Ok(false); }
            charge(meter, 1)?;
            if left_items.len() != right_items.len() { return Ok(false); }
            charge(meter, 1)?;
            let mut left_items = left_items.iter();
            let mut right_items = right_items.iter();
            loop {
                charge(meter, 1)?;
                match (left_items.next(), right_items.next()) {
                    (None, None) => return Ok(true),
                    (Some(left), Some(right)) => if !value_equal(left, right, meter)? { return Ok(false); },
                    _ => return Ok(false),
                }
            }
        }
        _ => Ok(false),
    }
}

fn values_equal(left: &FallibleValues, right: &FallibleValues, meter: &mut ProofMeter)
    -> Result<bool, DecodedCaptureFailure>
{
    charge(meter, 1)?;
    if left.len() != right.len() { return Ok(false); }
    charge(meter, 1)?;
    let mut left = left.iter();
    let mut right = right.iter();
    loop {
        charge(meter, 1)?;
        match (left.next(), right.next()) {
            (None, None) => return Ok(true),
            (Some((left_key, left_value)), Some((right_key, right_value))) => {
                if !string_equal(left_key, right_key, meter)? || !value_equal(left_value, right_value, meter)? {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
}

pub(super) fn capture_maps_equal(left: &FallibleScopeMap<String, FallibleValues>, right: &FallibleScopeMap<String, FallibleValues>,
    meter: &mut ProofMeter) -> Result<bool, DecodedCaptureFailure>
{
    charge(meter, 1)?;
    if left.len() != right.len() { return Ok(false); }
    charge(meter, 1)?;
    let mut left = left.iter();
    let mut right = right.iter();
    loop {
        charge(meter, 1)?;
        match (left.next(), right.next()) {
            (None, None) => return Ok(true),
            (Some((left_key, left_values)), Some((right_key, right_values))) => {
                if !string_equal(left_key, right_key, meter)? || !values_equal(left_values, right_values, meter)? {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
}
