//! Decoded assembly of the common-fact pair proof.
//!
//! This keeps the verifier input graph decoded from capture/request/inventory
//! owners. Retained transition evidence is not part of this path yet, so the
//! decoded action rules continue to refuse nonempty SetState markers.
use super::common_fact_admission::{OwnedProjectionDestination, OwnedProjectionRecipe, ProofMeter};
use super::fallible_common_fact_inventory::{CommonFactInventoryFailure, DecodedCommonFactInventory};
use super::fallible_common_fact_rules::{decoded_rule_projections, lifecycle_criteria, DecodedCommonFactCertificate, DecodedCommonFactRules};
use super::fallible_common_fact_schemas::{self, DecodedOwnedSchemaKind};
use super::fallible_owned_request_decode::DecodedOwnedPreviewRequest;
use super::owned_contract::{OwnedSourceSide, OwnedStructuralChangeKind};
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationSite, FormatFailure};
use crate::product_contract::{DataValue, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot};
use crate::product_store::StoreError;
use std::fmt;

#[derive(Debug)]
pub(crate) struct DecodedCommonFactPairProof {
    pub(crate) certificates: Vec<DecodedCommonFactCertificate>,
    pub(crate) projections: Vec<OwnedProjectionRecipe>,
    pub(crate) comparison: Vec<String>,
}

fn app_error(cx: &mut AllocationContext, at: &str, rule: &str) -> CommonFactInventoryFailure {
    let mut text = String::new();
    for part in [format_args!("Common factual admission: "), format_args!("{at}"), format_args!(": "), format_args!("{rule}")] {
        match product_alloc::try_append_fmt(&mut text, part, cx, AllocationSite::Formatting) {
            Ok(()) => (),
            Err(FormatFailure::Allocation(value)) => return CommonFactInventoryFailure::Allocation(value),
            Err(FormatFailure::Formatter) => return CommonFactInventoryFailure::Store(StoreError::Invalid("Common factual admission: formatting invariant".into())),
        }
    }
    CommonFactInventoryFailure::Store(StoreError::Invalid(text))
}
fn copy(value: &str, cx: &mut AllocationContext) -> Result<String, CommonFactInventoryFailure> {
    product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into)
}
fn format_owned(cx: &mut AllocationContext, args: fmt::Arguments<'_>) -> Result<String, CommonFactInventoryFailure> {
    let mut value = String::new();
    product_alloc::try_append_fmt(&mut value, args, cx, AllocationSite::Formatting).map_err(|failure| match failure {
        FormatFailure::Allocation(value) => CommonFactInventoryFailure::Allocation(value),
        FormatFailure::Formatter => CommonFactInventoryFailure::Store(StoreError::Invalid("Common factual admission: formatting invariant".into())),
    })?;
    Ok(value)
}
fn push<T>(values: &mut Vec<T>, value: T, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
    product_alloc::try_push(values, value, cx, AllocationSite::Vector).map_err(Into::into)
}
fn add_policy(
    policy: &mut OrderedSet<(String, String)>,
    entity: &str,
    field: &str,
    cx: &mut AllocationContext,
) -> Result<(), CommonFactInventoryFailure> {
    policy.try_insert((copy(entity, cx)?, copy(field, cx)?), cx).map(|_| ()).map_err(Into::into)
}
fn copy_data_value(value: &DataValue, cx: &mut AllocationContext) -> Result<DataValue, CommonFactInventoryFailure> {
    crate::product_contract::try_clone_data_value_with(value, cx).map_err(Into::into)
}
fn merge_policy(
    policy: &mut OrderedSet<(String, String)>,
    discovered: &OrderedSet<(String, String)>,
    cx: &mut AllocationContext,
) -> Result<bool, CommonFactInventoryFailure> {
    let mut changed = false;
    for (entity, field) in discovered.iter() {
        changed |= policy.try_insert((copy(entity, cx)?, copy(field, cx)?), cx)?;
    }
    Ok(changed)
}
fn edited(writer: &super::fallible_owned_source_decode::DecodedOwnedWriter, inventory: &DecodedCommonFactInventory) -> bool {
    inventory.structure.edits.iter().any(|edit| {
        let endpoint = if writer.side == OwnedSourceSide::Before { &edit.before } else { &edit.after };
        endpoint.action == writer.action
            && writer.path.len() > endpoint.list_path.len()
            && writer.path.starts_with(&endpoint.list_path)
            && endpoint.start.checked_add(endpoint.len).is_some_and(|end| {
                (endpoint.start..end).contains(&writer.path[endpoint.list_path.len()])
            })
    })
}

/// Verify the before/after pair using only decoded owners and fallible output
/// allocation. No retained transition capability is accepted by this API.
pub(crate) fn verify_pair_fallible(
    before: &FallibleDecodedCapturedProgram,
    after: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    request: &DecodedOwnedPreviewRequest,
    inventory: &DecodedCommonFactInventory,
    meter: &mut ProofMeter,
    cx: &mut AllocationContext,
) -> Result<DecodedCommonFactPairProof, CommonFactInventoryFailure> {
    let (before_field, before_value) = lifecycle_criteria(&request.inventory().lifecycle.before, cx)?;
    let (after_field, after_value) = lifecycle_criteria(&request.inventory().lifecycle.after, cx)?;
    if before_field != after_field || before_value != after_value {
        return Err(app_error(cx, "lifecycle", "before/after completion meaning differs"));
    }
    for edit in &inventory.structure.edits {
        if edit.before.root.is_none() && edit.before.len > 0 && edit.after.len > 0 {
            return Err(app_error(cx, &edit.id, "paired variant lacks an exact single-root correspondence; split at a proved root frontier"));
        }
    }

    let kinds = fallible_common_fact_schemas::schemas(before, data, after, request, inventory, cx)
        .map_err(|failure| match failure {
            super::fallible_common_fact_schemas::DecodedSchemaFailure::Allocation(value) => CommonFactInventoryFailure::Allocation(value),
            super::fallible_common_fact_schemas::DecodedSchemaFailure::Refusal(value) => CommonFactInventoryFailure::Store(value),
        })?;
    if data.records().iter().filter(|record| !record.archived()).any(|record| record.revision().checked_add(10_000_000).is_none()) {
        return Err(app_error(cx, "retained revisions", "insufficient revision range for the bounded runtime run"));
    }
    let all_roots_active = !data.records().iter().any(|record| record.entity() == inventory.structure.root_entity && record.archived());

    let mut policy = OrderedSet::new();
    for writer in &inventory.structure.writers {
        let Some(field) = writer.field.as_deref() else { continue; };
        if edited(writer, inventory)
            && (writer.entity == inventory.structure.root_entity
                || kinds.get(&writer.entity).is_some_and(|kind| matches!(kind, DecodedOwnedSchemaKind::Aux { owner } if owner != field)))
        {
            add_policy(&mut policy, &writer.entity, field, cx)?;
        }
    }
    // The predicate's raw lifecycle field remains tainted outside exact
    // accepted equality occurrences.
    add_policy(&mut policy, &inventory.structure.root_entity, before_field, cx)?;

    let mut certificates = Vec::new();
    loop {
        certificates.clear();
        let mut discovered = OrderedSet::new();
        for (app, side) in [(before, OwnedSourceSide::Before), (after, OwnedSourceSide::After)] {
            let mut rules = DecodedCommonFactRules::from_inventory(
                crate::product_contract::AppRef::Decoded(app.app()), &kinds, inventory,
                before_field, copy_data_value(before_value, cx)?, side, &policy,
                all_roots_active, meter,
            );
            rules.actions(cx)?;
            rules.observations(cx)?;
            let (side_certificates, side_discovered) = rules.take_outputs();
            product_alloc::try_reserve(&mut certificates, side_certificates.len(), cx, AllocationSite::Vector)?;
            certificates.extend(side_certificates);
            merge_policy(&mut discovered, &side_discovered, cx)?;
        }
        if !merge_policy(&mut policy, &discovered, cx)? { break; }
        meter.charge(1)?;
    }

    for hazard in &inventory.identity_hazards {
        if !matches!(kinds.get(&hazard.relation), Some(DecodedOwnedSchemaKind::History { .. })) {
            let at = format_owned(cx, format_args!("{} {:?}", hazard.action, hazard.common_create))?;
            return Err(app_error(cx, &at, "fresh identity shift reaches a business-consumed relation"));
        }
    }
    let projections = decoded_rule_projections(
        crate::product_contract::AppRef::Decoded(before.app()),
        crate::product_contract::AppRef::Decoded(after.app()),
        &inventory.structure.root_entity, cx,
    )?;
    for change in &inventory.structure.changes {
        let covered = match change.kind {
            OwnedStructuralChangeKind::ColumnAdded | OwnedStructuralChangeKind::ColumnChanged => projections.iter().any(|recipe| matches!(&recipe.destination,
                OwnedProjectionDestination::ViewColumn { view, column } if Some(view) == change.container.as_ref() && column == &change.id)),
            OwnedStructuralChangeKind::ObservableAdded | OwnedStructuralChangeKind::ObservableChanged => projections.iter().any(|recipe| matches!(&recipe.destination,
                OwnedProjectionDestination::RootCount { observable, .. } if observable == &change.id)),
            OwnedStructuralChangeKind::ApplicationLabelChanged | OwnedStructuralChangeKind::EntityAdded => true,
        };
        if !covered { return Err(app_error(cx, &change.id, "changed observation has no supported exact projection recipe")); }
    }

    let mut comparison = Vec::new();
    if !inventory.identity_hazards.is_empty() {
        push(&mut comparison, copy("unsupported: fresh history row IDs can differ; inert-use/order proof does not establish exact accepted observation equality", cx)?, cx)?;
    }
    if projections.iter().any(|recipe| recipe.baseline.is_none() && matches!(recipe.destination, OwnedProjectionDestination::ViewColumn { .. })) {
        push(&mut comparison, copy("unsupported: added Boolean uses Optional<Boolean> with explicit Null absence; raw Boolean/schema equality is not claimed", cx)?, cx)?;
    }
    if projections.iter().any(|recipe| recipe.baseline.is_none() && matches!(recipe.destination, OwnedProjectionDestination::RootCount { .. })) {
        push(&mut comparison, copy("unsupported: added count has no baseline observation; excluded/pre-existing sealed roots contribute zero by the disclosed eligible-member rule", cx)?, cx)?;
    }
    if comparison.is_empty() {
        push(&mut comparison, copy("not performed: runtime outcome comparison belongs to isolated execution, not static admission", cx)?, cx)?;
    }
    let units = inventory.uses.len().checked_add(inventory.structure.writers.len()).and_then(|value| value.checked_add(certificates.len()))
        .and_then(|value| u64::try_from(value).ok()).ok_or_else(|| app_error(cx, "proof meter", "final proof charge overflow"))?;
    meter.charge(units)?;
    Ok(DecodedCommonFactPairProof { certificates, projections, comparison })
}
