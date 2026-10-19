//! Read-only source/basis-bound inventory. Nothing in this module is a verifier,
//! certificate, preparation input, executable permission, or persisted format.
use super::owned_contract::*;
use crate::product_contract::*;
use crate::product_store::{ProductStore, ProjectSnapshot, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Env = BTreeMap<Id, Type>;
pub(super) type Result<T> = std::result::Result<T, StoreError>;
pub(super) fn error(reason: &str) -> StoreError {
    StoreError::Invalid(format!("Common fact inventory: {reason}"))
}
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<Digest> {
    Ok(canonical_digest(IdentityDomain::Evidence, &value)?)
}

/// Explicit opt-in suggestions; never inherited by OwnedEffectRequest/scalar-v1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactInventoryRequest {
    pub snapshot: Digest,
    pub sources: OwnedSourceRequest,
    pub lifecycle: CommonFactLifecycleSuggestion,
}
/// Even a well-typed predicate and a plausible meaning remain untrusted. There
/// is deliberately no caller-supplied "accepted" flag or completed-field guess.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactLifecycleSuggestion {
    pub entity: Id,
    /// Own-row Boolean expressions using the exact binder `record`.
    pub before: Expr,
    pub after: Expr,
    pub meaning: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommonFactPhaseKind {
    CommonSubtree,
    CommonHeader,
    Variant,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactSpan {
    /// Last index is the start of this slice; len=0 denotes an explicit gap.
    pub path: Vec<usize>,
    pub len: usize,
    pub digest: Digest,
    pub environment: Env,
    /// Action guards followed by outer-to-inner ForEach selection expressions.
    pub controls: Vec<Expr>,
    pub previous: Option<Digest>,
    pub next: Option<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactPhase {
    pub kind: CommonFactPhaseKind,
    pub edit: Option<Id>,
    pub before: CommonFactSpan,
    pub after: CommonFactSpan,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactAction {
    pub action: Id,
    pub parameters: Env,
    pub guards: Vec<Expr>,
    pub ensures: Vec<Expr>,
    pub before_identity: Digest,
    pub after_identity: Digest,
    /// Preorder: a common header precedes its nested phases. A complete common
    /// subtree is maximal and is not expanded into duplicate executable phases.
    pub phases: Vec<CommonFactPhase>,
}
/// Exhaustive typed use sites, including non-expression ingress. The expression
/// and environment preserve exact reference chains, not just the target type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactUse {
    pub side: OwnedSourceSide,
    pub source: Digest,
    pub location: String,
    pub kind: String,
    pub value_type: Option<Type>,
    pub expression: Option<Expr>,
    pub environment: Env,
    pub entity: Option<Id>,
    pub field: Option<Id>,
    pub state: Option<Id>,
    /// Exact non-expression descriptor (parameter, control, key, schema, etc.).
    pub details: serde_json::Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactReference {
    pub side: OwnedSourceSide,
    pub entity: Id,
    pub field: Id,
    pub target_entity: Id,
    pub required: bool,
    pub matches_retained_schema: bool,
    pub unique: Vec<Vec<Id>>,
    pub producers: Vec<OwnedWriter>,
    /// Empty means no mutation route observed, never verified ownership.
    pub mutations: Vec<OwnedWriter>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactIdentityHazard {
    pub side: OwnedSourceSide,
    pub action: Id,
    pub variant_create: Vec<usize>,
    pub common_create: Vec<usize>,
    pub relation: Id,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactObligation {
    pub side: Option<OwnedSourceSide>,
    pub location: String,
    pub code: String,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactResources {
    pub total_record_cap: usize,
    pub stored_records: usize,
    pub includes_archived_and_auxiliary: bool,
    pub ordinary_defaults: RuntimeLimits,
    pub max_query_limit_proves_order_independence: bool,
    pub failures_commit_fuel_and_trace: bool,
}
/// Diagnostic output only. None of the report types implement Deserialize, and
/// no preparation/adoption code consumes them. Success means inventory obtained.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommonFactInventory {
    pub snapshot: Digest,
    pub retained_data: Digest,
    pub retained_schema: Vec<EntityDefinition>,
    pub before_schema: Vec<EntityDefinition>,
    pub after_schema: Vec<EntityDefinition>,
    pub structure: OwnedStructuralReport,
    pub lifecycle: CommonFactLifecycleSuggestion,
    pub lifecycle_writers: Vec<OwnedWriter>,
    pub actions: Vec<CommonFactAction>,
    pub uses: Vec<CommonFactUse>,
    pub references: Vec<CommonFactReference>,
    pub identity_hazards: Vec<CommonFactIdentityHazard>,
    pub obligations: Vec<CommonFactObligation>,
    pub resources: CommonFactResources,
}
pub(super) fn obligation(
    result: &mut Vec<CommonFactObligation>,
    side: Option<OwnedSourceSide>,
    at: &str,
    code: &str,
    reason: &str,
) {
    result.push(CommonFactObligation {
        side,
        location: at.into(),
        code: code.into(),
        reason: reason.into(),
    });
}
pub(super) fn relation(typ: &Type) -> Option<&str> {
    match typ {
        Type::Reference { entity } => Some(entity),
        Type::List { item } | Type::Optional { item } => relation(item),
        _ => None,
    }
}
pub(super) fn item(app: &AppDefinition, expr: &Expr, env: &Env) -> Result<Type> {
    match app.expression_type_in(expr, env)? {
        Type::List { item } => Ok(*item),
        _ => Err(error("expected validated collection")),
    }
}
fn creates(env: &mut Env, steps: &[Statement]) {
    for step in steps {
        if let Statement::Create { entity, bind, .. } = step {
            env.insert(bind.clone(), Type::reference(entity));
        }
    }
}
fn span(
    app: &AppDefinition,
    action: &ActionDefinition,
    path: &[usize],
    len: usize,
) -> Result<CommonFactSpan> {
    let (start, parents) = path.split_last().ok_or_else(|| error("empty phase path"))?;
    let mut steps = action.steps.as_slice();
    let mut environment = action.parameters.clone();
    let mut controls = action.guards.clone();
    for index in parents {
        creates(&mut environment, &steps[..*index]);
        let Statement::ForEach {
            items,
            binding,
            steps: inner,
        } = &steps[*index]
        else {
            return Err(error("phase path is not a validated loop"));
        };
        environment.insert(binding.clone(), item(app, items, &environment)?);
        controls.push(items.clone());
        steps = inner;
    }
    creates(&mut environment, &steps[..*start]);
    Ok(CommonFactSpan {
        path: path.into(),
        len,
        digest: digest(&("common-fact-span/1", &steps[*start..*start + len]))?,
        environment,
        controls,
        previous: start
            .checked_sub(1)
            .map(|i| digest(&steps[i]))
            .transpose()?,
        next: steps.get(*start + len).map(digest).transpose()?,
    })
}
fn statement<'a>(action: &'a ActionDefinition, path: &[usize]) -> &'a Statement {
    let mut steps = action.steps.as_slice();
    for index in &path[..path.len() - 1] {
        let Statement::ForEach { steps: inner, .. } = &steps[*index] else {
            unreachable!("resolved path")
        };
        steps = inner;
    }
    &steps[path[path.len() - 1]]
}
fn phases(
    before: &AppDefinition,
    after: &AppDefinition,
    a: &ActionDefinition,
    b: &ActionDefinition,
    structure: &OwnedStructuralReport,
    steps: &[Statement],
    path: &[usize],
    output: &mut Vec<CommonFactPhase>,
) -> Result<()> {
    let mut index = 0;
    while index <= steps.len() {
        if let Some(edit) = structure.edits.iter().find(|e| {
            e.before.action == a.id && e.before.list_path == path && e.before.start == index
        }) {
            let mut old = edit.before.list_path.clone();
            old.push(edit.before.start);
            let mut new = edit.after.list_path.clone();
            new.push(edit.after.start);
            output.push(CommonFactPhase {
                kind: CommonFactPhaseKind::Variant,
                edit: Some(edit.id.clone()),
                before: span(before, a, &old, edit.before.len)?,
                after: span(after, b, &new, edit.after.len)?,
            });
            if edit.before.len > 0 {
                index += edit.before.len;
                continue;
            }
        }
        if index == steps.len() {
            break;
        }
        let mut current = path.to_vec();
        current.push(index);
        let common = structure
            .common_statements
            .iter()
            .find(|s| s.action == a.id && s.before == current)
            .ok_or_else(|| error("unresolved common phase"))?;
        let nested_variant = structure
            .edits
            .iter()
            .any(|e| e.before.action == a.id && e.before.list_path.starts_with(&current));
        let complete = steps[index] == *statement(b, &common.after) && !nested_variant;
        output.push(CommonFactPhase {
            kind: if complete {
                CommonFactPhaseKind::CommonSubtree
            } else {
                CommonFactPhaseKind::CommonHeader
            },
            edit: None,
            before: span(before, a, &current, 1)?,
            after: span(after, b, &common.after, 1)?,
        });
        if !complete {
            let Statement::ForEach { steps: inner, .. } = &steps[index] else {
                return Err(error("different common leaf"));
            };
            phases(before, after, a, b, structure, inner, &current, output)?;
        }
        index += 1;
    }
    Ok(())
}
fn inside(writer: &OwnedWriter, endpoint: &OwnedSpanEndpoint) -> bool {
    endpoint.action == writer.action
        && writer.path.starts_with(&endpoint.list_path)
        && writer
            .path
            .get(endpoint.list_path.len())
            .is_some_and(|i| (endpoint.start..endpoint.start + endpoint.len).contains(i))
}
fn edited(writer: &OwnedWriter, structure: &OwnedStructuralReport) -> bool {
    structure.edits.iter().any(|e| {
        inside(
            writer,
            match writer.side {
                OwnedSourceSide::Before => &e.before,
                OwnedSourceSide::After => &e.after,
            },
        )
    })
}
fn identity_hazards(structure: &OwnedStructuralReport) -> Vec<CommonFactIdentityHazard> {
    let mut output = vec![];
    for side in [OwnedSourceSide::Before, OwnedSourceSide::After] {
        let variant: BTreeSet<_> = structure
            .writers
            .iter()
            .filter(|w| {
                w.side == side && w.kind == OwnedStatementKind::Create && edited(w, structure)
            })
            .map(|w| (&w.action, &w.path))
            .collect();
        for common in structure
            .common_statements
            .iter()
            .filter(|s| s.kind == OwnedStatementKind::Create)
        {
            let path = match side {
                OwnedSourceSide::Before => &common.before,
                OwnedSourceSide::After => &common.after,
            };
            for (action, prior) in &variant {
                // A syntactically later create in the same loop can precede this
                // common create on the next iteration. Do not assume one pass.
                let repeated = (1..path.len()).any(|n| prior.starts_with(&path[..n]));
                if **action == common.action && (*prior < path || repeated) {
                    let entity = &structure
                        .writers
                        .iter()
                        .find(|w| {
                            w.side == side
                                && w.action == common.action
                                && w.path == *path
                                && w.kind == OwnedStatementKind::Create
                        })
                        .expect("common create writer")
                        .entity;
                    output.push(CommonFactIdentityHazard {side,action:common.action.clone(),variant_create:(*prior).clone(),common_create:path.clone(),relation:entity.clone(),reason:"Action-wide creation ordinal may differ, including across loop iterations; actual IDs/provenance and every identity/order consumer require closure".into()});
                }
            }
        }
    }
    output
}
pub(super) fn derive(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &CommonFactInventoryRequest,
) -> Result<CommonFactInventory> {
    let basis = canonical_digest(IdentityDomain::Data, snapshot)?;
    if basis != request.snapshot || snapshot.active_revision != request.sources.before {
        return Err(error("stale authenticated snapshot/source basis"));
    }
    derive_pair_inner(snapshot, snapshot.program()?, candidate, request, basis)
}
/// Explicit ordinary pair on the actual protected frame. The returned inventory
/// remains diagnostic; only the closed continuation verifier can consume it.
pub(super) fn derive_pair(
    snapshot: &ProjectSnapshot,
    baseline: &CapturedProgram,
    candidate: &CapturedProgram,
    request: &CommonFactInventoryRequest,
) -> Result<CommonFactInventory> {
    let basis = canonical_digest(IdentityDomain::Data, snapshot)?;
    if basis != request.snapshot {
        return Err(error("stale authenticated current frame"));
    }
    derive_pair_inner(snapshot, baseline, candidate, request, basis)
}
fn derive_pair_inner(
    snapshot: &ProjectSnapshot,
    baseline: &CapturedProgram,
    candidate: &CapturedProgram,
    request: &CommonFactInventoryRequest,
    basis: Digest,
) -> Result<CommonFactInventory> {
    let before = &baseline.program;
    let after = &candidate.program;
    let structure = super::analyze_owned_sources(baseline, candidate, &request.sources)?;
    if request.lifecycle.entity != structure.root_entity
        || request.lifecycle.meaning.trim().is_empty()
        || request.lifecycle.meaning.len() > MAX_TEXT_BYTES
    {
        return Err(error(
            "lifecycle suggestion must name the exact root and a bounded explicit meaning",
        ));
    }
    let env = Env::from([("record".into(), Type::reference(&structure.root_entity))]);
    for (app, predicate) in [
        (before, &request.lifecycle.before),
        (after, &request.lifecycle.after),
    ] {
        if app.expression_type_in(predicate, &env)? != Type::Boolean {
            return Err(error("lifecycle suggestion is not Boolean"));
        }
        super::compiler::own_row(
            predicate,
            "record",
            &structure.root_entity,
            app,
            &Env::new(),
            false,
        )?;
    }
    let mut uses = vec![];
    let mut lifecycle_origins = vec![];
    let mut obligations = vec![];
    for (app, source, side, predicate) in [
        (
            before,
            &structure.before,
            OwnedSourceSide::Before,
            &request.lifecycle.before,
        ),
        (
            after,
            &structure.after,
            OwnedSourceSide::After,
            &request.lifecycle.after,
        ),
    ] {
        lifecycle_origins.push(super::common_fact_uses::inventory(
            app,
            source,
            side,
            &structure.root_entity,
            predicate,
            &mut uses,
            &mut obligations,
        )?);
    }
    obligation(&mut obligations,None,"lifecycle","lifecycle_meaning","The source/basis-bound own-row suggestion is untrusted: business meaning, exact admission occurrences, preservation, and completed-history sealing remain unproved");
    let mut lifecycle_writers = vec![];
    for writer in &structure.writers {
        let lifecycle_field = lifecycle_origins
            .iter()
            .flat_map(|origin| &uses[origin.clone()])
            .any(|u| {
                u.side == writer.side
                    && u.kind == "field"
                    && u.entity.as_ref() == Some(&writer.entity)
                    && u.field == writer.field
            });
        if writer.entity == structure.root_entity
            && (lifecycle_field || writer.kind == OwnedStatementKind::Archive)
        {
            lifecycle_writers.push(writer.clone());
            let code = if edited(writer, &structure) {
                "mixed_lifecycle_transition"
            } else {
                "lifecycle_writer"
            };
            obligation(&mut obligations,Some(writer.side),&format!("{} {:?}",writer.action,writer.path),code,"Every writer, including unchanged create/begin/complete paths, needs lifecycle classification; an owned span in a mixed action must preserve lifecycle truth, and equal values do not prove sealing");
        }
    }
    let mut references = vec![];
    for (app, side) in [
        (before, OwnedSourceSide::Before),
        (after, OwnedSourceSide::After),
    ] {
        for entity in &app.entities {
            for field in &entity.fields {
                let Some(target) = relation(&field.value_type) else {
                    continue;
                };
                let writers: Vec<_> = structure
                    .writers
                    .iter()
                    .filter(|w| {
                        w.side == side
                            && w.entity == entity.id
                            && (w.field.as_ref() == Some(&field.id)
                                || w.kind == OwnedStatementKind::Archive)
                    })
                    .cloned()
                    .collect();
                let (producers, mutations): (Vec<_>, Vec<_>) = writers
                    .into_iter()
                    .partition(|w| w.kind == OwnedStatementKind::Create);
                for w in &mutations {
                    obligation(&mut obligations,Some(side),&format!("{} {:?}",w.action,w.path),"reference_mutation","Reference reassignment, collection mutation, or archival prevents an immutable-owner proof, including unchanged and Invoke-only actions");
                }
                obligation(&mut obligations,Some(side),&format!("entity {} field {}",entity.id,field.id),"reference_ownership","Typed reference is only an immutable-owner candidate: exact Create provenance, aliases, retained rows, uniqueness, and all mutation routes require proof; shared references confer no write authority");
                references.push(CommonFactReference {
                    side,
                    entity: entity.id.clone(),
                    field: field.id.clone(),
                    target_entity: target.into(),
                    required: matches!(field.value_type, Type::Reference { .. }),
                    matches_retained_schema: snapshot
                        .data
                        .schema
                        .iter()
                        .find(|e| e.id == entity.id)
                        == Some(entity),
                    unique: entity.unique.clone(),
                    producers,
                    mutations,
                });
            }
        }
    }
    for entity in &snapshot.data.schema {
        if before.entities.iter().find(|e| e.id == entity.id) != Some(entity)
            || after.entities.iter().find(|e| e.id == entity.id) != Some(entity)
        {
            obligation(&mut obligations,None,&format!("retained entity {}",entity.id),"retained_schema","Cumulative retained schema differs from a source definition; source equality cannot establish runtime compatibility or retained uniqueness");
        }
        for (i, _) in entity.unique.iter().enumerate() {
            obligation(&mut obligations,None,&format!("retained entity {} unique[{i}]",entity.id),"retained_uniqueness","Retained uniqueness is a global transaction admission dependency; owner partitioning must be proved from the exact tuple and retained references");
        }
    }
    let mut actions = vec![];
    for a in &before.actions {
        let b = after
            .actions
            .iter()
            .find(|b| b.id == a.id)
            .expect("resolved action");
        let mut result = vec![];
        phases(before, after, a, b, &structure, &a.steps, &[], &mut result)?;
        actions.push(CommonFactAction {
            action: a.id.clone(),
            parameters: a.parameters.clone(),
            guards: a.guards.clone(),
            ensures: a.ensures.clone(),
            before_identity: digest(a)?,
            after_identity: digest(b)?,
            phases: result,
        });
        obligation(&mut obligations,None,&a.id,"resource_atomicity","Common phases must run once in authored order; ordinary fuel/collection/statement/write/output/elapsed/cancellation failures must not commit business/session/output prefixes, while fuel and trace/outcome may change");
    }
    let identity_hazards = identity_hazards(&structure);
    for h in &identity_hazards {
        obligation(
            &mut obligations,
            Some(h.side),
            &format!("{} {:?}", h.action, h.common_create),
            "fresh_identity",
            &h.reason,
        );
    }
    Ok(CommonFactInventory {
        snapshot: basis,
        retained_data: snapshot.data.identity()?,
        retained_schema: snapshot.data.schema.clone(),
        before_schema: before.entities.clone(),
        after_schema: after.entities.clone(),
        structure,
        lifecycle: request.lifecycle.clone(),
        lifecycle_writers,
        actions,
        uses,
        references,
        identity_hazards,
        obligations,
        resources: CommonFactResources {
            total_record_cap: MAX_COLLECTION,
            stored_records: snapshot.data.records.len(),
            includes_archived_and_auxiliary: true,
            ordinary_defaults: RuntimeLimits::default(),
            max_query_limit_proves_order_independence: false,
            failures_commit_fuel_and_trace: true,
        },
    })
}
impl ProductStore {
    /// Re-derive diagnostic inventories from the currently validated store and
    /// exact sources. Does not call or relax the effect verifier, or write state.
    pub fn inventory_common_fact_phases(
        &self,
        candidate: &CapturedProgram,
        request: &CommonFactInventoryRequest,
    ) -> Result<CommonFactInventory> {
        derive(&self.load()?, candidate, request)
    }
}
