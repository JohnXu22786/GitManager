//! Allocation-aware schema-kind classification for decoded common-fact inputs.
//! This is deliberately separate from the legacy rule path until the decoded
//! verifier is ready to be wired into production.
use super::fallible_common_fact_inventory::DecodedCommonFactInventory;
use super::fallible_owned_request_decode::DecodedOwnedPreviewRequest;
use crate::product_alloc::{self, ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationFailure, AllocationSite, FormatFailure};
use crate::product_contract::{DataValue, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot, StringValueMap, Type};
use crate::product_contract::fallible_app_decode::DecodedStatement;
use crate::product_store::StoreError;

#[derive(Debug)]
pub(super) enum DecodedSchemaFailure {
    Allocation(AllocationFailure),
    Refusal(StoreError),
}
impl From<AllocationFailure> for DecodedSchemaFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DecodedOwnedSchemaKind {
    Root,
    Aux { owner: String },
    Link { owner: String },
    Registry { key: String },
    History { owner: Option<String>, sequence: String, archived: bool },
}
impl DecodedOwnedSchemaKind {
    pub(super) fn owner(&self) -> Option<&str> {
        match self {
            Self::Aux { owner } | Self::Link { owner } => Some(owner),
            Self::History { owner, .. } => owner.as_deref(),
            _ => None,
        }
    }
    fn auxiliary(&self) -> bool { matches!(self, Self::Aux { .. }) }
}

pub(super) struct DecodedOwnedSchemaKinds(OrderedMap<String, DecodedOwnedSchemaKind>);
impl DecodedOwnedSchemaKinds {
    pub(super) fn get(&self, entity: &str) -> Option<&DecodedOwnedSchemaKind> { self.0.get(entity) }
    pub(super) fn iter(&self) -> impl Iterator<Item = (&String, &DecodedOwnedSchemaKind)> { self.0.iter() }
}

fn refusal(cx: &mut AllocationContext, at: &str, rule: &str) -> DecodedSchemaFailure {
    let mut text = String::new();
    for part in [format_args!("Common factual admission: "), format_args!("{at}"), format_args!(": "), format_args!("{rule}")] {
        match product_alloc::try_append_fmt(&mut text, part, cx, AllocationSite::Formatting) {
            Ok(()) => (),
            Err(FormatFailure::Allocation(failure)) => return DecodedSchemaFailure::Allocation(failure),
            Err(FormatFailure::Formatter) => return DecodedSchemaFailure::Refusal(StoreError::Invalid("Common factual admission: formatting invariant".into())),
        }
    }
    DecodedSchemaFailure::Refusal(StoreError::Invalid(text))
}
fn entity_refusal(cx: &mut AllocationContext, entity: &str, rule: &str) -> DecodedSchemaFailure {
    let mut at = String::new();
    match product_alloc::try_append_fmt(&mut at, format_args!("entity {entity}"), cx, AllocationSite::Formatting) {
        Ok(()) => refusal(cx, &at, rule),
        Err(FormatFailure::Allocation(failure)) => DecodedSchemaFailure::Allocation(failure),
        Err(FormatFailure::Formatter) => DecodedSchemaFailure::Refusal(StoreError::Invalid("Common factual admission: formatting invariant".into())),
    }
}
fn copy(value: &str, cx: &mut AllocationContext) -> Result<String, DecodedSchemaFailure> {
    product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into)
}
fn push<T>(values: &mut Vec<T>, value: T, cx: &mut AllocationContext) -> Result<(), DecodedSchemaFailure> {
    product_alloc::try_push(values, value, cx, AllocationSite::Vector).map_err(Into::into)
}
fn relation(ty: &Type) -> Option<&str> {
    match ty {
        Type::Reference { entity } => Some(entity),
        Type::Optional { item } | Type::List { item } => relation(item),
        _ => None,
    }
}
fn bookkeeping(expr: &crate::product_contract::Expr) -> Option<(&str, &str, bool)> {
    use crate::product_contract::Expr;
    let Expr::Add { left, right } = expr else { return None; };
    if !matches!(right.as_ref(), Expr::Literal { value: DataValue::Integer { value: 1 }, .. }) { return None; }
    let Expr::Count { items } = left.as_ref() else { return None; };
    let Expr::Query { entity, binding, predicate, sort, limit, include_archived } = items.as_ref() else { return None; };
    if !matches!(predicate.as_ref(), Expr::Literal { value: DataValue::Boolean { value: true }, .. })
        || !sort.is_empty() || *limit != crate::product_contract::MAX_COLLECTION { return None; }
    Some((entity, binding, *include_archived))
}
fn is_required_reference(ty: &Type, target: &str) -> bool {
    matches!(ty, Type::Reference { entity } if entity == target)
}
fn visit_statements(steps: &[DecodedStatement], visit: &mut impl FnMut(&DecodedStatement) -> Result<(), DecodedSchemaFailure>) -> Result<(), DecodedSchemaFailure> {
    for step in steps {
        visit(step)?;
        if let DecodedStatement::ForEach { steps, .. } = step { visit_statements(steps, visit)?; }
    }
    Ok(())
}
fn each_create<F>(program: &FallibleDecodedCapturedProgram, entity: &str, visit: &mut F) -> Result<(), DecodedSchemaFailure>
where F: FnMut(&crate::product_contract::fallible_app_decode::FallibleMap<crate::product_contract::Expr>) -> Result<(), DecodedSchemaFailure>,
{
    for action in &program.app().actions {
        visit_statements(&action.steps, &mut |step| {
            if let DecodedStatement::Create { entity: target, values, .. } = step {
                if target == entity { visit(values)?; }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Classify decoded after-schema entities using only decoded owner views.
/// The retained branch intentionally does not exist here: its exact-source
/// contract remains in the legacy verifier until that verifier is decoded.
pub(super) fn schemas(
    before: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    after: &FallibleDecodedCapturedProgram,
    request: &DecodedOwnedPreviewRequest,
    inventory: &DecodedCommonFactInventory,
    cx: &mut AllocationContext,
) -> Result<DecodedOwnedSchemaKinds, DecodedSchemaFailure> {
    let root = request.root_entity();
    for entity in data.schema() {
        if before.app().entities.iter().find(|candidate| candidate.id == entity.id) != Some(entity)
            || after.app().entities.iter().find(|candidate| candidate.id == entity.id) != Some(entity)
        {
            return Err(entity_refusal(cx, &entity.id, "retained schema differs from both exact sources"));
        }
    }

    let mut result = DecodedOwnedSchemaKinds(OrderedMap::new());
    let mut declared = OrderedSet::new();
    for (entity, _) in request.auxiliary() {
        let key = copy(entity, cx)?;
        if !declared.try_insert(key, cx)? { return Err(refusal(cx, entity, "duplicate auxiliary declaration")); }
    }

    for entity in &after.app().entities {
        if !entity.constraints.is_empty() {
            return Err(entity_refusal(cx, &entity.id, "schema constraints are outside the total admission grammar"));
        }
        let mut refs = Vec::new();
        for field in &entity.fields {
            if matches!(field.value_type, Type::Reference { .. }) { push(&mut refs, field, cx)?; }
        }
        if entity.fields.iter().any(|field| relation(&field.value_type).is_some() && !matches!(field.value_type, Type::Reference { .. })) {
            return Err(refusal(cx, &entity.id, "optional/list references have unsupported ownership provenance"));
        }
        let owner_field = refs.iter().find(|field| is_required_reference(&field.value_type, root)).map(|field| field.id.as_str());
        let kind = if entity.id == root {
            if !entity.unique.is_empty() || !refs.is_empty() {
                return Err(refusal(cx, &entity.id, "root uniqueness or reference ownership is unsupported"));
            }
            DecodedOwnedSchemaKind::Root
        } else if let Some((_, declared_owner)) = request.auxiliary().find(|(aux_entity, _)| *aux_entity == entity.id) {
            if before.app().entities.iter().any(|candidate| candidate.id == entity.id)
                || data.schema().iter().any(|candidate| candidate.id == entity.id)
                || owner_field != Some(declared_owner)
                || refs.len() != 1
                || entity.unique.len() != 1
                || entity.unique[0].len() != 1
                || entity.unique[0][0] != declared_owner
            {
                return Err(refusal(cx, &entity.id, "auxiliary must be fresh with one immutable required owner and owner-only uniqueness"));
            }
            DecodedOwnedSchemaKind::Aux { owner: copy(declared_owner, cx)? }
        } else if let Some(owner) = owner_field.filter(|_| !entity.unique.is_empty()) {
            let mut reference_fields = OrderedSet::new();
            for field in &refs { reference_fields.try_insert(copy(&field.id, cx)?, cx)?; }
            let mut unique_fields = OrderedSet::new();
            for field in &entity.unique[0] { unique_fields.try_insert(copy(field, cx)?, cx)?; }
            if refs.iter().filter(|field| is_required_reference(&field.value_type, root)).count() != 1
                || entity.unique.len() != 1
                || unique_fields.len() != entity.unique[0].len()
                || unique_fields.len() != reference_fields.len()
                || unique_fields.iter().any(|field| !reference_fields.contains(field.as_str()))
            {
                return Err(refusal(cx, &entity.id, "owner-partitioned uniqueness must contain every exact required reference"));
            }
            if entity.fields.iter().any(|field| !reference_fields.contains(field.id.as_str()) && field.value_type != Type::Boolean) {
                return Err(refusal(cx, &entity.id, "only Boolean non-owner link state is supported"));
            }
            DecodedOwnedSchemaKind::Link { owner: copy(owner, cx)? }
        } else if !entity.unique.is_empty() {
            if !refs.is_empty() || entity.unique.len() != 1 || entity.unique[0].len() != 1 {
                return Err(refusal(cx, &entity.id, "unsupported global uniqueness"));
            }
            let key = &entity.unique[0][0];
            if !entity.fields.iter().any(|field| field.id == *key && field.value_type == Type::Text) {
                return Err(refusal(cx, &entity.id, "registry key must be required text"));
            }
            DecodedOwnedSchemaKind::Registry { key: copy(key, cx)? }
        } else {
            let mut sequence: Option<String> = None;
            let mut archived_mode = None;
            let mut creates = 0usize;
            for program in [before, after] {
                each_create(program, &entity.id, &mut |values| {
                    creates += 1;
                    let mut found = None;
                    let mut matches = 0;
                    for (field, value) in values.iter() {
                        if let Some((relation, _, archived)) = bookkeeping(value) {
                            if relation == entity.id { matches += 1; found = Some((field.as_str(), archived)); }
                        }
                    }
                    if matches != 1 { return Err(refusal(cx, &entity.id, "terminal history needs one bounded count-plus-one producer field")); }
                    let (field, archived) = found.expect("one match");
                    if sequence.as_deref().is_some_and(|old| old != field) || archived_mode.is_some_and(|old| old != archived) {
                        return Err(refusal(cx, &entity.id, "history producers disagree on sequence/count population"));
                    }
                    sequence = Some(copy(field, cx)?);
                    archived_mode = Some(archived);
                    Ok(())
                })?;
            }
            if creates == 0 || refs.len() != 1 { return Err(refusal(cx, &entity.id, "unknown relation/producer grammar")); }
            DecodedOwnedSchemaKind::History {
                owner: owner_field.map(|value| copy(value, cx)).transpose()?,
                sequence: sequence.expect("history producer sequence"),
                archived: archived_mode.expect("history producer mode"),
            }
        };
        result.0.try_insert(copy(&entity.id, cx)?, kind, cx)?;
    }

    for (entity, _) in request.auxiliary() {
        if !result.get(entity).is_some_and(DecodedOwnedSchemaKind::auxiliary) {
            return Err(refusal(cx, entity, "undeclared auxiliary addition"));
        }
    }
    for entity in &after.app().entities {
        if !before.app().entities.iter().any(|candidate| candidate.id == entity.id) && !declared.contains(entity.id.as_str()) {
            return Err(refusal(cx, &entity.id, "every added relation needs exact auxiliary ownership"));
        }
    }
    for reference in &inventory.references {
        if !reference.required || !reference.mutations.is_empty() {
            let mut at = String::new();
            for part in [format_args!("{}", reference.entity), format_args!("."), format_args!("{}", reference.field)] {
                match product_alloc::try_append_fmt(&mut at, part, cx, AllocationSite::Formatting) {
                    Ok(()) => (),
                    Err(FormatFailure::Allocation(failure)) => return Err(DecodedSchemaFailure::Allocation(failure)),
                    Err(FormatFailure::Formatter) => return Err(DecodedSchemaFailure::Refusal(StoreError::Invalid("Common factual admission: formatting invariant".into()))),
                }
            }
            return Err(refusal(cx, &at, "all reference assignment/archive/collection routes must be immutable"));
        }
        if let Some(kind) = result.get(&reference.entity) {
            if matches!(result.get(&reference.target_entity), Some(DecodedOwnedSchemaKind::History { .. } | DecodedOwnedSchemaKind::Aux { .. })) {
                return Err(refusal(cx, &reference.entity, "references to terminal history/private auxiliary relations are unsupported"));
            }
            if kind.owner() == Some(reference.field.as_str()) && reference.target_entity != root {
                return Err(refusal(cx, &reference.entity, "owner reference is not the exact root"));
            }
        }
    }
    for (entity, kind) in result.iter() {
        if let DecodedOwnedSchemaKind::History { sequence, archived, .. } = kind {
            if !archived && data.records().iter().any(|row| row.entity() == entity && row.archived()) {
                return Err(refusal(cx, entity, "active-only sequence producer has retained archived identity/order ambiguity"));
            }
            let mut row_count = 0usize;
            let mut numbers = OrderedSet::new();
            for row in data.records() {
                if row.entity() != entity || !(*archived || !row.archived()) { continue; }
                row_count += 1;
                let Some(DataValue::Integer { value }) = row.values().get(sequence) else {
                    return Err(refusal(cx, entity, "retained sequence missing or mistyped"));
                };
                numbers.try_insert(*value, cx)?;
            }
            if numbers.len() != row_count || !numbers.iter().copied().eq(1..=row_count as i64) {
                return Err(refusal(cx, entity, "retained sequence must be exactly distinct 1..count; fresh/retained ties are unsupported"));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::*;
    use crate::product_store::{AdoptionReceipt, OperationReceipt, ProjectSnapshot};
    use crate::product_store::scope::common_fact_admission::{OwnedPreviewAvailability, OwnedPreviewRequest};
    use crate::product_store::scope::fallible_common_fact_inventory::derive_pair_fallible;
    use crate::product_store::scope::common_fact_inventory::CommonFactInventory;
    use crate::product_store::scope::fallible_owned_lifecycle_acceptance::FallibleAcceptedPreviewLifecycle;
    use super::super::common_fact_inventory::{CommonFactInventoryRequest, CommonFactLifecycleSuggestion};
    use super::super::owned_contract::{OwnedSourceRequest, OwnedSourceSide, OwnedStatementKind};
    use std::collections::BTreeMap;

    fn fixture_with(mutator: impl FnOnce(&mut AppDefinition, &mut AppDefinition)) -> (AppDefinition, AppDefinition, CapturedProgram, CapturedProgram, DataSnapshot, OwnedPreviewRequest, CommonFactInventory, FallibleDecodedCapturedProgram, FallibleDecodedCapturedProgram, crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot, DecodedOwnedPreviewRequest, DecodedCommonFactInventory) {
        fn literal(value_type: Type, value: DataValue) -> Expr { Expr::Literal { value_type, value } }
        fn app(with_auxiliary: bool) -> AppDefinition {
        let mut entities = vec![
            EntityDefinition { id: "job".into(), label: "Job".into(), fields: vec![FieldDefinition { id: "done".into(), label: "Done".into(), value_type: Type::Boolean }], unique: vec![], constraints: vec![] },
            EntityDefinition { id: "edge".into(), label: "Edge".into(), fields: vec![
                FieldDefinition { id: "job_ref".into(), label: "Job".into(), value_type: Type::Reference { entity: "job".into() } },
                FieldDefinition { id: "registry_ref".into(), label: "Registry".into(), value_type: Type::Reference { entity: "registry".into() } },
                FieldDefinition { id: "enabled".into(), label: "Enabled".into(), value_type: Type::Boolean },
            ], unique: vec![vec!["job_ref".into(), "registry_ref".into()]], constraints: vec![] },
            EntityDefinition { id: "registry".into(), label: "Registry".into(), fields: vec![FieldDefinition { id: "key".into(), label: "Key".into(), value_type: Type::Text }], unique: vec![vec!["key".into()]], constraints: vec![] },
            EntityDefinition { id: "history".into(), label: "History".into(), fields: vec![
                FieldDefinition { id: "job_ref".into(), label: "Job".into(), value_type: Type::Reference { entity: "job".into() } },
                FieldDefinition { id: "sequence".into(), label: "Sequence".into(), value_type: Type::Integer },
                FieldDefinition { id: "counter".into(), label: "Counter".into(), value_type: Type::Integer },
            ], unique: vec![], constraints: vec![] },
        ];
        if with_auxiliary {
            entities.push(EntityDefinition { id: "scratch".into(), label: "Scratch".into(), fields: vec![FieldDefinition { id: "job_ref".into(), label: "Job".into(), value_type: Type::Reference { entity: "job".into() } }], unique: vec![vec!["job_ref".into()]], constraints: vec![] });
        }
        let query = Expr::Query { entity: "history".into(), binding: "row".into(), predicate: Box::new(literal(Type::Boolean, DataValue::Boolean { value: true })), sort: vec![], limit: MAX_COLLECTION, include_archived: false };
        let sequence = Expr::Add { left: Box::new(Expr::Count { items: Box::new(query) }), right: Box::new(literal(Type::Integer, DataValue::Integer { value: 1 })) };
        AppDefinition {
            version: CONTRACT_VERSION, id: "schema-app".into(), label: "Schema".into(),
            entities,
            state: vec![], actions: vec![ActionDefinition { id: "record".into(), label: "Record".into(), parameters: BTreeMap::from([("job".into(), Type::Reference { entity: "job".into() })]), guards: vec![], steps: vec![Statement::Create { entity: "history".into(), bind: "created".into(), values: BTreeMap::from([("job_ref".into(), Expr::Variable { name: "job".into() }), ("sequence".into(), sequence), ("counter".into(), literal(Type::Integer, DataValue::Integer { value: 0 }))]) }], ensures: vec![] }], outputs: vec![], observables: vec![],
            views: vec![ViewDefinition { id: "jobs".into(), label: "Jobs".into(), kind: ViewKind::List {
                entity: "job".into(), rows: Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(literal(Type::Boolean, DataValue::Boolean { value: true })), sort: vec![], limit: MAX_COLLECTION, include_archived: false },
                columns: vec![], controls: vec![], selection: None,
            }, actions: vec![], keys: vec![] }], initial_view: "jobs".into(),
        }
        }
        let mut app_before = app(false);
        let mut app_after = app(true);
        mutator(&mut app_before, &mut app_after);
        let bytes = canonical_bytes(&app_before).unwrap();
        let legacy_before = CapturedProgram::capture(&bytes, "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let legacy_after = CapturedProgram::capture(&canonical_bytes(&app_after).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let before_revision = crate::product_store::revision(&legacy_before).unwrap();
        let snapshot = ProjectSnapshot {
            magic: crate::product_store::MAGIC.into(), version: crate::product_store::FORMAT, revision: 0,
            programs: vec![legacy_before.clone()], active_revision: before_revision.clone(),
            data: DataSnapshot::empty("schema-project", &app_before).unwrap(),
            session: SessionState::initial(&app_before).unwrap(), clock_day: 0,
            decisions: DecisionGraph { version: CONTRACT_VERSION, revision: 0, decisions: vec![] },
            adoptions: Vec::<AdoptionReceipt>::new(), operations: BTreeMap::<String, OperationReceipt>::new(), artifacts: vec![],
            scope: super::super::ScopeEnvelope::ScalarV1(super::super::ScopeState::default()),
        };
        let inventory_request = CommonFactInventoryRequest {
            snapshot: canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            sources: OwnedSourceRequest { before: before_revision, after: crate::product_store::revision(&legacy_after).unwrap(), root_entity: "job".into(), edits: vec![] },
            lifecycle: CommonFactLifecycleSuggestion {
                entity: "job".into(), before: Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "done".into() },
                after: Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "done".into() }, meaning: "done".into(),
            },
        };
        let legacy_request = OwnedPreviewRequest {
            inventory: inventory_request.clone(),
            auxiliary: vec![super::super::owned_effects::OwnedAuxiliary { entity: "scratch".into(), owner_field: "job_ref".into() }],
            population: super::super::contract::ScopePopulation::All, excluded_records: vec![], operations: std::collections::BTreeSet::new(),
            availability: OwnedPreviewAvailability::NullableAddedBoolean, purpose: "schema".into(),
        };
        let legacy_inventory = super::super::common_fact_inventory::derive_pair(&snapshot, &legacy_before, &legacy_after, &inventory_request).unwrap();
        let mut cx = AllocationContext::new();
        let decoded_snapshot = crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(
            crate::product_json::parse_json_bytes(&serde_json::to_vec(&snapshot).unwrap(), &mut cx).unwrap(), &mut cx,
        ).unwrap();
        let decoded_before = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_before).unwrap(), &mut cx).unwrap(), &mut cx).unwrap();
        let decoded_after = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_after).unwrap(), &mut cx).unwrap(), &mut cx).unwrap();
        let request_value = serde_json::to_value(&legacy_request).unwrap();
        let request = super::super::fallible_owned_request_decode::decode_owned_preview_request(
            crate::product_json::parse_json_bytes(&serde_json::to_vec(&request_value).unwrap(), &mut cx).unwrap(), &mut cx,
        ).unwrap();
        let accepted = FallibleAcceptedPreviewLifecycle::test_inventory_binding(&decoded_snapshot, &decoded_before, &decoded_after, &request, &mut cx).unwrap();
        let inventory = derive_pair_fallible(&decoded_snapshot, &decoded_before, &decoded_after, &request, &accepted, &mut cx).unwrap();
        (app_before, app_after, legacy_before, legacy_after, snapshot.data.clone(), legacy_request, legacy_inventory, decoded_before, decoded_after, decoded_snapshot, request, inventory)
    }

    fn fixture() -> (AppDefinition, AppDefinition, CapturedProgram, CapturedProgram, DataSnapshot, OwnedPreviewRequest, CommonFactInventory, FallibleDecodedCapturedProgram, FallibleDecodedCapturedProgram, crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot, DecodedOwnedPreviewRequest, DecodedCommonFactInventory) {
        fixture_with(|_, _| {})
    }

    fn assert_refusal(mutator: impl FnOnce(&mut AppDefinition, &mut AppDefinition), sync_data_schema: bool, expected: &str) {
        let (mut app_before, mut app_after, _, _, mut legacy_data, legacy_request, legacy_inventory, _, _, _snapshot, request, inventory) = fixture();
        mutator(&mut app_before, &mut app_after);
        if sync_data_schema { legacy_data.schema = app_before.entities.clone(); }
        let legacy_before = CapturedProgram::capture(&canonical_bytes(&app_before).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let legacy_after = CapturedProgram::capture(&canonical_bytes(&app_after).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let mut decode_cx = AllocationContext::new();
        let before = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_before).unwrap(), &mut decode_cx).unwrap(), &mut decode_cx).unwrap();
        let after = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_after).unwrap(), &mut decode_cx).unwrap(), &mut decode_cx).unwrap();
        let decoded_data = decode_data_snapshot_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_data).unwrap(), &mut decode_cx).unwrap(), &mut decode_cx).unwrap();
        let legacy = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded = match schemas(&before, &decoded_data, &after, &request, &inventory, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("invalid schema was accepted"),
        };
        let legacy_text = match legacy { crate::product_store::StoreError::Invalid(message) => message, other => panic!("unexpected legacy error: {other:?}") };
        let decoded_text = match decoded { DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) => message, other => panic!("unexpected decoded error: {other:?}") };
        assert_eq!(legacy_text, expected);
        assert_eq!(decoded_text, expected);
    }

    fn assert_data_refusal(records: Vec<Record>, expected: &str) {
        let (_app_before, _app_after, legacy_before, legacy_after, mut legacy_data, legacy_request, legacy_inventory, before, after, _snapshot, request, inventory) = fixture();
        legacy_data.records = records;
        let mut cx = AllocationContext::new();
        let decoded_data = decode_data_snapshot_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_data).unwrap(), &mut cx).unwrap(), &mut cx).unwrap();
        let legacy = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded = match schemas(&before, &decoded_data, &after, &request, &inventory, &mut AllocationContext::new()) { Err(error) => error, Ok(_) => panic!("invalid retained history accepted") };
        assert!(matches!(legacy, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(decoded, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));
    }

    fn assert_reference_refusal(reference: super::super::common_fact_inventory::CommonFactReference, decoded_reference: super::super::fallible_common_fact_inventory::DecodedCommonFactReference, expected: &str) {
        let (_app_before, _app_after, legacy_before, legacy_after, legacy_data, legacy_request, mut legacy_inventory, before, after, snapshot, request, mut inventory) = fixture();
        legacy_inventory.references.push(reference);
        inventory.references.push(decoded_reference);
        let legacy = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded = match schemas(&before, snapshot.data(), &after, &request, &inventory, &mut AllocationContext::new()) { Err(error) => error, Ok(_) => panic!("invalid reference route accepted") };
        assert!(matches!(legacy, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(decoded, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));
    }

    fn history_record(id: &str, archived: bool, sequence: Option<DataValue>) -> Record {
        let mut values = BTreeMap::from([("job_ref".into(), DataValue::Reference { entity: "job".into(), record: "job-1".into() })]);
        if let Some(sequence) = sequence { values.insert("sequence".into(), sequence); }
        Record { entity: "history".into(), id: id.into(), revision: 1, created_program: Digest::from_owned_lower_hex("0".repeat(64)).unwrap(), archived, values }
    }

    #[test]
    fn decoded_schema_kinds_and_every_allocation_failure_retry() {
        let (_app_before, _app_after, legacy_before, legacy_after, legacy_data, legacy_request, legacy_inventory, before, after, snapshot, request, inventory) = fixture();
        let before_bytes = [
            crate::product_backup::fallible_codec::canonical_bytes(&before, &mut AllocationContext::new()).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&after, &mut AllocationContext::new()).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&snapshot, &mut AllocationContext::new()).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&request, &mut AllocationContext::new()).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&inventory, &mut AllocationContext::new()).unwrap(),
        ];
        let mut count = AllocationContext::new();
        let kinds = schemas(&before, snapshot.data(), &after, &request, &inventory, &mut count).unwrap();
        let legacy = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap();
        assert!(matches!(kinds.get("job"), Some(DecodedOwnedSchemaKind::Root)));
        assert!(matches!(kinds.get("scratch"), Some(DecodedOwnedSchemaKind::Aux { owner }) if owner == "job_ref"));
        assert!(matches!(kinds.get("edge"), Some(DecodedOwnedSchemaKind::Link { owner }) if owner == "job_ref"));
        assert!(matches!(kinds.get("registry"), Some(DecodedOwnedSchemaKind::Registry { key }) if key == "key"));
        assert!(matches!(kinds.get("history"), Some(DecodedOwnedSchemaKind::History { owner: Some(owner), sequence, archived: false }) if owner == "job_ref" && sequence == "sequence"));
        assert_eq!(kinds.iter().map(|(entity, _)| entity.as_str()).collect::<Vec<_>>(), vec!["edge", "history", "job", "registry", "scratch"]);
        assert_eq!(legacy.get("job").map(|kind| matches!(kind, super::super::common_fact_rules::Kind::Root)), Some(true));
        assert_eq!(legacy.get("scratch").and_then(|kind| match kind { super::super::common_fact_rules::Kind::Aux { owner } => Some(owner.as_str()), _ => None }), Some("job_ref"));
        assert_eq!(legacy.get("edge").and_then(|kind| match kind { super::super::common_fact_rules::Kind::Link { owner } => Some(owner.as_str()), _ => None }), Some("job_ref"));
        assert_eq!(legacy.get("registry").and_then(|kind| match kind { super::super::common_fact_rules::Kind::Registry { key } => Some(key.as_str()), _ => None }), Some("key"));
        assert_eq!(legacy.get("history").and_then(|kind| match kind { super::super::common_fact_rules::Kind::History { owner, sequence, archived } if owner.as_deref() == Some("job_ref") && sequence == "sequence" && !archived => Some(true), _ => None }), Some(true));
        let growths = count.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(nth).unwrap(), true);
            let refusal = match schemas(&before, snapshot.data(), &after, &request, &inventory, &mut refused) {
                Err(DecodedSchemaFailure::Allocation(failure)) => failure,
                _ => panic!("growth refusal {nth} did not preserve allocation classification"),
            };
            assert_eq!(refused.first_failure(), Some(refusal), "growth refusal {nth} preserves original allocation cause");
            let mut retry = AllocationContext::new();
            let retried = schemas(&before, snapshot.data(), &after, &request, &inventory, &mut retry).unwrap();
            assert_eq!(retried.0.len(), kinds.0.len());
            assert!(retried.iter().zip(kinds.iter()).all(|((key, kind), (expected_key, expected_kind))| key == expected_key && kind == expected_kind));
            let after_bytes = [
                crate::product_backup::fallible_codec::canonical_bytes(&before, &mut AllocationContext::new()).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(&after, &mut AllocationContext::new()).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(&snapshot, &mut AllocationContext::new()).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(&request, &mut AllocationContext::new()).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(&inventory, &mut AllocationContext::new()).unwrap(),
            ];
            assert_eq!(before_bytes, after_bytes);
        }
    }

    #[test]
    fn decoded_schema_kinds_duplicate_auxiliary_refusal_keeps_legacy_text() {
        let (_app_before, mut app_after, legacy_before, _legacy_after, legacy_data, mut legacy_request, legacy_inventory, before, _after, snapshot, _request, inventory) = fixture();
        app_after.entities[4].constraints.push(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } });
        let legacy_after = CapturedProgram::capture(&canonical_bytes(&app_after).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let mut decode_cx = AllocationContext::new();
        let after = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_after).unwrap(), &mut decode_cx).unwrap(), &mut decode_cx).unwrap();
        let auxiliary = legacy_request.auxiliary[0].clone();
        legacy_request.auxiliary.push(auxiliary);
        let value = serde_json::to_value(&legacy_request).unwrap();
        let mut cx = AllocationContext::new();
        let duplicate = super::super::fallible_owned_request_decode::decode_owned_preview_request(
            crate::product_json::parse_json_bytes(&serde_json::to_vec(&value).unwrap(), &mut cx).unwrap(), &mut cx,
        ).unwrap();
        let error = match schemas(&before, snapshot.data(), &after, &duplicate, &inventory, &mut AllocationContext::new()) {
            Err(error) => error,
            Ok(_) => panic!("duplicate auxiliary declaration was accepted"),
        };
        let expected = "Common factual admission: scratch: duplicate auxiliary declaration";
        let legacy_error = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        assert!(matches!(legacy_error, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(error, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));
    }

    #[test]
    fn decoded_schema_kinds_auxiliary_freshness_and_declaration_refusals_match_legacy() {
        let (_app_before, _app_after, legacy_before, legacy_after, legacy_data, mut legacy_request, legacy_inventory, before, after, snapshot, _request, inventory) = fixture();
        legacy_request.auxiliary.clear();
        let mut cx = AllocationContext::new();
        let missing = super::super::fallible_owned_request_decode::decode_owned_preview_request(
            crate::product_json::parse_json_bytes(&serde_json::to_vec(&legacy_request).unwrap(), &mut cx).unwrap(), &mut cx,
        ).unwrap();
        let expected = "Common factual admission: scratch: every added relation needs exact auxiliary ownership";
        let legacy_error = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded_error = match schemas(&before, snapshot.data(), &after, &missing, &inventory, &mut AllocationContext::new()) { Err(value) => value, Ok(_) => panic!("missing auxiliary declaration accepted") };
        assert!(matches!(legacy_error, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(decoded_error, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));

        let (app_before, mut app_after, legacy_before, _legacy_after, legacy_data, legacy_request, legacy_inventory, before, _after, snapshot, request, inventory) = fixture();
        app_after.entities.retain(|entity| entity.id != "scratch");
        let absent = CapturedProgram::capture(&canonical_bytes(&app_after).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let mut cx = AllocationContext::new();
        let decoded_absent = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&absent).unwrap(), &mut cx).unwrap(), &mut cx).unwrap();
        let expected = "Common factual admission: scratch: undeclared auxiliary addition";
        let legacy_error = super::super::common_fact_rules::schemas(&legacy_before.program, &legacy_data, &absent.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded_error = match schemas(&before, snapshot.data(), &decoded_absent, &request, &inventory, &mut AllocationContext::new()) { Err(value) => value, Ok(_) => panic!("missing auxiliary entity accepted") };
        assert!(matches!(legacy_error, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(decoded_error, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));

        let (mut app_before, app_after, _legacy_before, legacy_after, legacy_data, legacy_request, legacy_inventory, _before, after, snapshot, request, inventory) = fixture();
        app_before.entities.push(app_after.entities.iter().find(|entity| entity.id == "scratch").unwrap().clone());
        let retained_aux = CapturedProgram::capture(&canonical_bytes(&app_before).unwrap(), "schema-project", Producer::Fixture { name: "schema".into() }, None).unwrap();
        let mut cx = AllocationContext::new();
        let decoded_retained_aux = decode_captured_program_fallible(crate::product_json::parse_json_bytes(&serde_json::to_vec(&retained_aux).unwrap(), &mut cx).unwrap(), &mut cx).unwrap();
        let expected = "Common factual admission: scratch: auxiliary must be fresh with one immutable required owner and owner-only uniqueness";
        let legacy_error = super::super::common_fact_rules::schemas(&retained_aux.program, &legacy_data, &legacy_after.program, &legacy_request, &legacy_inventory, None).unwrap_err();
        let decoded_error = match schemas(&decoded_retained_aux, snapshot.data(), &after, &request, &inventory, &mut AllocationContext::new()) { Err(value) => value, Ok(_) => panic!("retained auxiliary accepted") };
        assert!(matches!(legacy_error, crate::product_store::StoreError::Invalid(message) if message == expected));
        assert!(matches!(decoded_error, DecodedSchemaFailure::Refusal(crate::product_store::StoreError::Invalid(message)) if message == expected));
    }

    #[test]
    fn decoded_schema_kinds_schema_and_owner_refusals_match_legacy_text_and_precedence() {
        assert_refusal(|before, after| {
            before.entities[1].fields[0].value_type = Type::Optional { item: Box::new(Type::Reference { entity: "job".into() }) };
            after.entities[1].fields[0].value_type = before.entities[1].fields[0].value_type.clone();
        }, true, "Common factual admission: edge: optional/list references have unsupported ownership provenance");
        assert_refusal(|before, after| {
            let field = FieldDefinition { id: "extra".into(), label: "Extra".into(), value_type: Type::List { item: Box::new(Type::Reference { entity: "job".into() }) } };
            before.entities[1].fields.push(field.clone());
            after.entities[1].fields.push(field);
        }, true, "Common factual admission: edge: optional/list references have unsupported ownership provenance");
        assert_refusal(|_, after| {
            after.entities[4].fields[0].value_type = Type::Reference { entity: "registry".into() };
        }, false, "Common factual admission: scratch: auxiliary must be fresh with one immutable required owner and owner-only uniqueness");
        assert_refusal(|_, after| {
            after.entities[4].unique.clear();
        }, false, "Common factual admission: scratch: auxiliary must be fresh with one immutable required owner and owner-only uniqueness");
        assert_refusal(|_, after| {
            after.entities[4].constraints.push(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } });
        }, false, "Common factual admission: entity scratch: schema constraints are outside the total admission grammar");
        assert_refusal(|before, after| {
            before.entities[1].unique = vec![vec!["job_ref".into()]];
            after.entities[1].unique = before.entities[1].unique.clone();
        }, true, "Common factual admission: edge: owner-partitioned uniqueness must contain every exact required reference");
        assert_refusal(|before, after| {
            before.entities[1].fields[2].value_type = Type::Text;
            after.entities[1].fields[2].value_type = Type::Text;
        }, true, "Common factual admission: edge: only Boolean non-owner link state is supported");
        assert_refusal(|before, after| {
            before.entities[2].fields[0].value_type = Type::Boolean;
            after.entities[2].fields[0].value_type = Type::Boolean;
        }, true, "Common factual admission: registry: registry key must be required text");
        assert_refusal(|before, after| {
            before.entities[0].fields[0].value_type = Type::Text;
            after.entities[0].fields[0].value_type = Type::Text;
            before.entities[0].unique = vec![vec!["done".into()]];
            after.entities[0].unique = before.entities[0].unique.clone();
        }, true, "Common factual admission: job: root uniqueness or reference ownership is unsupported");
        assert_refusal(|before, after| {
            for app in [before, after] {
                let Statement::Create { values, .. } = &mut app.actions[0].steps[0] else { panic!("fixture create") };
                let Expr::Add { left, .. } = values.get_mut("sequence").unwrap() else { panic!("fixture sequence") };
                let Expr::Count { items } = left.as_mut() else { panic!("fixture count") };
                let Expr::Query { limit, .. } = items.as_mut() else { panic!("fixture query") };
                *limit = 1;
            }
        }, false, "Common factual admission: history: terminal history needs one bounded count-plus-one producer field");
        assert_refusal(|before, after| {
            for app in [before, after] {
                let Statement::Create { values, .. } = &mut app.actions[0].steps[0] else { panic!("fixture create") };
                let sequence = values.get("sequence").unwrap().clone();
                values.insert("counter".into(), sequence);
            }
        }, false, "Common factual admission: history: terminal history needs one bounded count-plus-one producer field");
        assert_refusal(|before, after| {
            before.actions.clear();
            after.actions.clear();
        }, false, "Common factual admission: history: unknown relation/producer grammar");
        assert_refusal(|_, after| {
            let Statement::Create { values, .. } = &mut after.actions[0].steps[0] else { panic!("fixture create") };
            let Expr::Add { left, .. } = values.get_mut("sequence").unwrap() else { panic!("fixture sequence") };
            let Expr::Count { items } = left.as_mut() else { panic!("fixture count") };
            let Expr::Query { include_archived, .. } = items.as_mut() else { panic!("fixture query") };
            *include_archived = true;
        }, false, "Common factual admission: history: history producers disagree on sequence/count population");
        assert_refusal(|_, after| {
            after.entities[1].label = "Changed Edge".into();
        }, false, "Common factual admission: entity edge: retained schema differs from both exact sources");
    }

    #[test]
    fn decoded_schema_kinds_history_retained_rows_match_legacy_refusal_order() {
        assert_data_refusal(vec![history_record("archived", true, Some(DataValue::Integer { value: 1 }))], "Common factual admission: history: active-only sequence producer has retained archived identity/order ambiguity");
        assert_data_refusal(vec![history_record("missing", false, None)], "Common factual admission: history: retained sequence missing or mistyped");
        assert_data_refusal(vec![history_record("mistyped", false, Some(DataValue::Text { value: "1".into() }))], "Common factual admission: history: retained sequence missing or mistyped");
        assert_data_refusal(vec![history_record("one", false, Some(DataValue::Integer { value: 1 })), history_record("two", false, Some(DataValue::Integer { value: 1 }))], "Common factual admission: history: retained sequence must be exactly distinct 1..count; fresh/retained ties are unsupported");
        assert_data_refusal(vec![history_record("one", false, Some(DataValue::Integer { value: 1 })), history_record("three", false, Some(DataValue::Integer { value: 3 }))], "Common factual admission: history: retained sequence must be exactly distinct 1..count; fresh/retained ties are unsupported");
    }

    #[test]
    fn decoded_schema_kinds_reference_route_refusals_match_legacy() {
        let reference = |field: &str, target: &str, required: bool, mutations: Vec<super::super::owned_contract::OwnedWriter>| super::super::common_fact_inventory::CommonFactReference {
            side: OwnedSourceSide::After, entity: "edge".into(), field: field.into(), target_entity: target.into(), required, matches_retained_schema: true, unique: vec![], producers: vec![], mutations,
        };
        let decoded_reference = |field: &str, target: &str, required: bool, mutations: Vec<super::super::fallible_owned_source_decode::DecodedOwnedWriter>| super::super::fallible_common_fact_inventory::DecodedCommonFactReference {
            side: OwnedSourceSide::After, entity: "edge".into(), field: field.into(), target_entity: target.into(), required, matches_retained_schema: true, unique: vec![], producers: vec![], mutations,
        };
        assert_reference_refusal(reference("job_ref", "job", false, vec![]), decoded_reference("job_ref", "job", false, vec![]), "Common factual admission: edge.job_ref: all reference assignment/archive/collection routes must be immutable");
        let writer = super::super::owned_contract::OwnedWriter { side: OwnedSourceSide::After, source: Digest::from_owned_lower_hex("0".repeat(64)).unwrap(), action: "mutate".into(), path: vec![], kind: OwnedStatementKind::Update, entity: "edge".into(), field: Some("job_ref".into()), value_type: Some(Type::reference("job")) };
        let decoded_writer = super::super::fallible_owned_source_decode::DecodedOwnedWriter { side: OwnedSourceSide::After, source: Digest::from_owned_lower_hex("0".repeat(64)).unwrap(), action: "mutate".into(), path: vec![], kind: OwnedStatementKind::Update, entity: "edge".into(), field: Some("job_ref".into()), value_type: Some(Type::reference("job")) };
        assert_reference_refusal(reference("job_ref", "job", true, vec![writer]), decoded_reference("job_ref", "job", true, vec![decoded_writer]), "Common factual admission: edge.job_ref: all reference assignment/archive/collection routes must be immutable");
        assert_reference_refusal(reference("registry_ref", "history", true, vec![]), decoded_reference("registry_ref", "history", true, vec![]), "Common factual admission: edge: references to terminal history/private auxiliary relations are unsupported");
        assert_reference_refusal(reference("registry_ref", "scratch", true, vec![]), decoded_reference("registry_ref", "scratch", true, vec![]), "Common factual admission: edge: references to terminal history/private auxiliary relations are unsupported");
        assert_reference_refusal(reference("job_ref", "registry", true, vec![]), decoded_reference("job_ref", "registry", true, vec![]), "Common factual admission: edge: owner reference is not the exact root");
    }
}
