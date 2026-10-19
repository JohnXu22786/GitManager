//! Closed ordinary-source continuation over an authenticated current owned frame.
//! There is deliberately no initializer, DTO conversion, or public constructor.
use super::common_fact_admission::{refuse, AdmissionResult, ProofMeter};
use super::owned_lowering::{self, Metadata};
use super::*;
use serde::Serialize;

#[path = "owned_state_consumers.rs"]
mod state_consumers;

/// Local rule capability: only this verifying constructor can create it. It is
/// neither serialized nor returned as a preparation/publication permission.
struct ClosedContinuationRules {
    snapshot: Digest,
    before: Digest,
    after: Digest,
    proofs: Vec<VerifiedSourceContinuation>,
    markers: BTreeSet<Vec<u8>>,
}
impl ClosedContinuationRules {
    #[allow(clippy::too_many_arguments)]
    fn verify(
        original: &PreparedOwnedPreview,
        predecessor_before: &CapturedProgram,
        predecessor_after: &CapturedProgram,
        predecessor_sources: &OwnedSourceRequest,
        before: &CapturedProgram,
        after: &CapturedProgram,
        request: &OwnedPreviewRequest,
        inventory: &CommonFactInventory,
        seed: &Dependencies,
        controlled: &Dependencies,
        meter: &mut ProofMeter,
    ) -> AdmissionResult<Self> {
        meter.charge(1)?;
        let sources = &request.inventory.sources;
        if inventory.snapshot != request.inventory.snapshot
            || inventory.structure.before != revision(before)?
            || inventory.structure.after != revision(after)?
            || sources.before != inventory.structure.before
            || sources.after != inventory.structure.after
        {
            return Err(refuse(
                "continuation rules",
                "current frame/source inventory mismatch",
            ));
        }
        let original_sources = &original.proof.request.inventory.sources;
        let mut proofs = Vec::new();
        for (old, new, old_sources, side) in [
            (
                original.proof.snapshot.program()?,
                before,
                original_sources,
                OwnedSourceSide::Before,
            ),
            (
                &original.proof.candidate,
                after,
                original_sources,
                OwnedSourceSide::After,
            ),
            (
                predecessor_before,
                before,
                predecessor_sources,
                OwnedSourceSide::Before,
            ),
            (
                predecessor_after,
                after,
                predecessor_sources,
                OwnedSourceSide::After,
            ),
        ] {
            proofs.push(continuation(
                old,
                new,
                old_sources,
                sources,
                side,
                seed,
                controlled,
                meter,
            )?);
        }
        let mut markers =
            collect_common_state_markers(&before.program, &after.program, sources, meter)?;
        for (app, side) in [
            (&before.program, OwnedSourceSide::Before),
            (&after.program, OwnedSourceSide::After),
        ] {
            for action in &app.actions {
                meter.charge(1)?;
                let Some(step) = action.steps.last() else {
                    continue;
                };
                let Some((state, _)) = bounded_state_marker(step) else {
                    continue;
                };
                let dependency = Dependency::State(state.into());
                if proofs.iter().any(|p| {
                    p.protected.contains(&dependency) || p.influenced.contains(&dependency)
                }) {
                    let key =
                        state_marker_key(side, &action.id, &[action.steps.len() - 1], step, meter)?;
                    markers.remove(&key);
                }
            }
        }
        Ok(Self {
            snapshot: inventory.snapshot.clone(),
            before: inventory.structure.before.clone(),
            after: inventory.structure.after.clone(),
            proofs,
            markers,
        })
    }
}
fn bounded_state_marker(step: &Statement) -> Option<(&str, &str)> {
    match step {
        Statement::SetState {
            state,
            value:
                Expr::Literal {
                    value_type: Type::Text,
                    value: DataValue::Text { value },
                },
        } if !value.is_empty() && value.len() <= MAX_TEXT_BYTES => Some((state, value)),
        _ => None,
    }
}
fn state_marker_key(
    side: OwnedSourceSide,
    action: &str,
    path: &[usize],
    step: &Statement,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<u8>> {
    let (state, text) = bounded_state_marker(step)
        .ok_or_else(|| refuse("continuation marker", "not a bounded nonempty text literal"))?;
    meter.charge(
        u64::try_from(text.len()).map_err(|_| refuse("budget", "text byte count overflow"))?,
    )?;
    charged_bytes(&(side, action, path, state, step), meter)
}
/// Plain syntax candidates only. No caller can turn this set into a capability.
fn collect_common_state_markers(
    before: &AppDefinition,
    after: &AppDefinition,
    sources: &OwnedSourceRequest,
    meter: &mut ProofMeter,
) -> AdmissionResult<BTreeSet<Vec<u8>>> {
    let mut output = BTreeSet::new();
    let mut candidate_actions = BTreeMap::new();
    for action in &after.actions {
        meter.charge(1)?;
        candidate_actions.insert(action.id.as_str(), action);
    }
    for action in &before.actions {
        meter.charge(1)?;
        let Some(other) = candidate_actions.get(action.id.as_str()) else {
            continue;
        };
        let (Some(step), Some(next)) = (action.steps.last(), other.steps.last()) else {
            continue;
        };
        let Some((state, _)) = bounded_state_marker(step) else {
            continue;
        };
        if step != next
            || ![before, after].iter().all(|app| {
                app.state
                    .iter()
                    .any(|s| s.id == state && s.value_type == Type::Text)
            })
        {
            continue;
        }
        let sites = [
            (OwnedSourceSide::Before, action.steps.len() - 1),
            (OwnedSourceSide::After, other.steps.len() - 1),
        ];
        let mut common = true;
        for (side, index) in sites {
            for edit in &sources.edits {
                meter.charge(1)?;
                let ep = if side == OwnedSourceSide::Before {
                    &edit.before
                } else {
                    &edit.after
                };
                if ep.action == action.id && inside(&[index], ep) {
                    common = false;
                }
            }
        }
        if common
            && !state_consumers::reads_state(before, state, meter)?
            && !state_consumers::reads_state(after, state, meter)?
        {
            for (side, index) in sites {
                output.insert(state_marker_key(side, &action.id, &[index], step, meter)?);
            }
        }
    }
    Ok(output)
}

/// Permission to retain schemas comes from the reconstructed original compiler,
/// not from recognizing an auxiliary or protected field by its name.
pub(super) struct RetainedOwned {
    baseline: Vec<EntityDefinition>,
    candidate: Vec<EntityDefinition>,
    storage: Vec<EntityDefinition>,
    auxiliary: Vec<OwnedAuxiliary>,
    continuation_rules: ClosedContinuationRules,
}
impl RetainedOwned {
    fn new(
        original: &PreparedOwnedPreview,
        continuation_rules: ClosedContinuationRules,
    ) -> AdmissionResult<Self> {
        Ok(Self {
            baseline: original.proof.snapshot.program()?.program.entities.clone(),
            candidate: original.proof.candidate.program.entities.clone(),
            storage: original.initialized.schema.clone(),
            auxiliary: original.proof.request.auxiliary.clone(),
            continuation_rules,
        })
    }
    pub(super) fn permits_state_marker(
        &self,
        inventory: &CommonFactInventory,
        side: OwnedSourceSide,
        action: &str,
        path: &[usize],
        step: &Statement,
        meter: &mut ProofMeter,
    ) -> AdmissionResult<bool> {
        let closed = &self.continuation_rules;
        if inventory.snapshot != closed.snapshot
            || inventory.structure.before != closed.before
            || inventory.structure.after != closed.after
            || bounded_state_marker(step).is_none()
        {
            return Ok(false);
        }
        let key = state_marker_key(side, action, path, step, meter)?;
        Ok(closed.markers.contains(&key))
    }
    pub(super) fn verify_schema(
        &self,
        before: &AppDefinition,
        data: &DataSnapshot,
        after: &AppDefinition,
        request: &OwnedPreviewRequest,
    ) -> AdmissionResult<()> {
        if before.entities != self.baseline
            || after.entities != self.candidate
            || data.schema != self.storage
            || request.auxiliary != self.auxiliary
        {
            return Err(refuse(
                "continuation schema",
                "original business/owner/storage layout changed",
            ));
        }
        // Includes archived and auxiliary rows, protected cells, uniqueness and
        // real revisions. No reduced frame is substituted for this validation.
        data.validate()?;
        Ok(())
    }
}

/// Typed dependency distinctions are intentionally more conservative than alias
/// analysis: two references to the same entity may alias. No spelling equality
/// is used to prove that two receivers refer to different rows.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
enum Dependency {
    Field(Id, Id),
    Membership(Id),
    Order(Id),
    Unique(Id),
    State(Id),
    Clock,
    Binding(Id, Digest),
    /// A particular Create-produced identity, unlike a shared receiver binder.
    CreatedIdentity(Id, Digest),
    CreationOrdinal,
}
type Dependencies = BTreeSet<Dependency>;
type Types = BTreeMap<Id, Type>;
type Origins = BTreeMap<Id, Digest>;

fn identity<T: Serialize + ?Sized>(value: &T) -> AdmissionResult<Digest> {
    Ok(canonical_digest(IdentityDomain::Evidence, &value)?)
}
fn entity(typ: &Type) -> AdmissionResult<&str> {
    if let Type::Reference { entity } = typ {
        Ok(entity)
    } else {
        Err(refuse(
            "continuation receiver",
            "unknown or non-reference alias",
        ))
    }
}
fn bind(
    app: &AppDefinition,
    expr: &Expr,
    name: &str,
    kind: &str,
    types: &Types,
    origins: &Origins,
) -> AdmissionResult<(Types, Origins)> {
    let typ = match app.expression_type_in(expr, types)? {
        Type::List { item } => *item,
        _ => return Err(refuse(name, "non-list lexical origin")),
    };
    let mut t = types.clone();
    let mut o = origins.clone();
    o.insert(name.into(), identity(&(kind, name, expr, types, origins))?);
    t.insert(name.into(), typ);
    Ok((t, o))
}
fn dependencies(
    app: &AppDefinition,
    expr: &Expr,
    types: &Types,
    origins: &Origins,
    out: &mut Dependencies,
    meter: &mut ProofMeter,
) -> AdmissionResult<()> {
    meter.charge(1)?;
    match expr {
        Expr::Literal { .. } => {}
        Expr::Variable { name } => {
            let origin = origins
                .get(name)
                .ok_or_else(|| refuse(name, "unresolved lexical origin"))?;
            out.insert(Dependency::Binding(name.clone(), origin.clone()));
        }
        Expr::State { state } => {
            out.insert(Dependency::State(state.clone()));
        }
        Expr::Today => {
            out.insert(Dependency::Clock);
        }
        Expr::Field { record, field } => {
            let typ = app.expression_type_in(record, types)?;
            let e = entity(&typ)?;
            out.insert(Dependency::Field(e.into(), field.clone()));
            dependencies(app, record, types, origins, out, meter)?;
        }
        Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            ..
        } => {
            out.insert(Dependency::Membership(entity.clone()));
            out.insert(Dependency::Order(entity.clone()));
            out.insert(Dependency::Unique(entity.clone()));
            let mut t = types.clone();
            let mut o = origins.clone();
            t.insert(binding.clone(), Type::reference(entity));
            o.insert(
                binding.clone(),
                identity(&("query", binding, expr, types, origins))?,
            );
            dependencies(app, predicate, &t, &o, out, meter)?;
            for key in sort {
                dependencies(app, &key.value, &t, &o, out, meter)?;
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
            dependencies(app, items, types, origins, out, meter)?;
            let kind = match expr {
                Expr::Filter { .. } => "filter",
                Expr::Any { .. } => "any",
                _ => "all",
            };
            let (t, o) = bind(app, items, binding, kind, types, origins)?;
            dependencies(app, predicate, &t, &o, out, meter)?;
        }
        Expr::Map {
            items,
            binding,
            value,
        } => {
            dependencies(app, items, types, origins, out, meter)?;
            let (t, o) = bind(app, items, binding, "map", types, origins)?;
            dependencies(app, value, &t, &o, out, meter)?;
        }
        Expr::Count { items } | Expr::Sum { items } => {
            dependencies(app, items, types, origins, out, meter)?
        }
        Expr::Contains { items, value } => {
            dependencies(app, items, types, origins, out, meter)?;
            dependencies(app, value, types, origins, out, meter)?;
        }
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            dependencies(app, left, types, origins, out, meter)?;
            dependencies(app, right, types, origins, out, meter)?;
        }
        Expr::TextContains { text, search } => {
            dependencies(app, text, types, origins, out, meter)?;
            dependencies(app, search, types, origins, out, meter)?;
        }
        Expr::Lower { value } | Expr::Not { value } => {
            dependencies(app, value, types, origins, out, meter)?
        }
        Expr::And { values } | Expr::Or { values } => {
            for value in values {
                dependencies(app, value, types, origins, out, meter)?;
            }
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            for value in [condition, then_value, else_value] {
                dependencies(app, value, types, origins, out, meter)?;
            }
        }
        Expr::Coalesce { value, fallback } => {
            dependencies(app, value, types, origins, out, meter)?;
            dependencies(app, fallback, types, origins, out, meter)?;
        }
        Expr::DateAdd { date, days } => {
            dependencies(app, date, types, origins, out, meter)?;
            dependencies(app, days, types, origins, out, meter)?;
        }
        Expr::DateDifference { later, earlier } => {
            dependencies(app, later, types, origins, out, meter)?;
            dependencies(app, earlier, types, origins, out, meter)?;
        }
    }
    Ok(())
}

#[derive(Clone, Serialize)]
struct Node {
    path: Vec<usize>,
    /// Complete lexical origins/controls, excluding path indices. A disjoint
    /// insertion may relocate a site but cannot change its binder or header.
    context: Digest,
    statement: Statement,
    reads: Dependencies,
    writes: Dependencies,
    edit: Option<Id>,
    barrier: bool,
}
fn inside(path: &[usize], endpoint: &OwnedSpanEndpoint) -> bool {
    path.starts_with(&endpoint.list_path)
        && path
            .get(endpoint.list_path.len())
            .is_some_and(|i| endpoint.start <= *i && *i < endpoint.start + endpoint.len)
}
fn trace(
    app: &AppDefinition,
    action: &ActionDefinition,
    sources: &OwnedSourceRequest,
    side: OwnedSourceSide,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<Node>> {
    let origins: Origins = action
        .parameters
        .iter()
        .map(|(name, typ)| {
            Ok((
                name.clone(),
                identity(&("parameter", &action.id, name, typ))?,
            ))
        })
        .collect::<AdmissionResult<_>>()?;
    let mut final_types = action.parameters.clone();
    let mut final_origins = origins.clone();
    for step in &action.steps {
        if let Statement::Create { entity, bind, .. } = step {
            let origin = identity(&("create", bind, step, &final_origins, &action.guards))?;
            final_types.insert(bind.clone(), Type::reference(entity));
            final_origins.insert(bind.clone(), origin);
        }
    }
    let mut ensures = Dependencies::new();
    for expr in &action.ensures {
        dependencies(app, expr, &final_types, &final_origins, &mut ensures, meter)?;
    }
    let mut nodes = Vec::new();
    walk(
        app,
        action,
        &action.steps,
        &[],
        &action.guards,
        &ensures,
        &mut action.parameters.clone(),
        &mut origins.clone(),
        sources,
        side,
        &mut nodes,
        meter,
    )?;
    Ok(nodes)
}
#[allow(clippy::too_many_arguments)]
fn walk(
    app: &AppDefinition,
    action: &ActionDefinition,
    steps: &[Statement],
    path: &[usize],
    controls: &[Expr],
    ensures: &Dependencies,
    types: &mut Types,
    origins: &mut Origins,
    sources: &OwnedSourceRequest,
    side: OwnedSourceSide,
    out: &mut Vec<Node>,
    meter: &mut ProofMeter,
) -> AdmissionResult<()> {
    for (index, step) in steps.iter().enumerate() {
        meter.charge(1)?;
        let mut at = path.to_vec();
        at.push(index);
        let edit = sources
            .edits
            .iter()
            .find(|e| {
                let endpoint = if side == OwnedSourceSide::Before {
                    &e.before
                } else {
                    &e.after
                };
                endpoint.action == action.id && inside(&at, endpoint)
            })
            .map(|e| e.id.clone());
        let context = identity(&(
            &action.id,
            &action.parameters,
            &action.guards,
            &action.ensures,
            controls,
            &*types,
            &*origins,
        ))?;
        let mut reads = ensures.clone();
        let mut writes = Dependencies::new();
        for e in controls {
            dependencies(app, e, types, origins, &mut reads, meter)?;
        }
        let mut bind_create = None;
        let mut header = step.clone();
        let mut nested = None;
        let mut barrier = false;
        match step {
            Statement::Create {
                entity,
                values,
                bind,
            } => {
                for (field, value) in values {
                    dependencies(app, value, types, origins, &mut reads, meter)?;
                    writes.insert(Dependency::Field(entity.clone(), field.clone()));
                }
                writes.extend([
                    Dependency::Membership(entity.clone()),
                    Dependency::Order(entity.clone()),
                    Dependency::Unique(entity.clone()),
                    Dependency::CreationOrdinal,
                ]);
                reads.insert(Dependency::CreationOrdinal);
                let origin = identity(&("create", bind, step, &*origins, controls))?;
                writes.insert(Dependency::CreatedIdentity(bind.clone(), origin.clone()));
                bind_create = Some((bind.clone(), entity.clone(), origin));
            }
            Statement::Update { record, values } => {
                dependencies(app, record, types, origins, &mut reads, meter)?;
                let typ = app.expression_type_in(record, types)?;
                let e = entity(&typ)?.to_string();
                for (field, value) in values {
                    writes.insert(Dependency::Field(e.clone(), field.clone()));
                    dependencies(app, value, types, origins, &mut reads, meter)?;
                }
            }
            Statement::SetState { state, value } => {
                writes.insert(Dependency::State(state.clone()));
                dependencies(app, value, types, origins, &mut reads, meter)?;
            }
            Statement::ForEach {
                items,
                binding,
                steps,
            } => {
                dependencies(app, items, types, origins, &mut reads, meter)?;
                header = Statement::ForEach {
                    items: items.clone(),
                    binding: binding.clone(),
                    steps: vec![],
                };
                let (t, o) = bind(app, items, binding, "for_each", types, origins)?;
                let mut c = controls.to_vec();
                c.push(items.clone());
                nested = Some((steps, t, o, c));
            }
            Statement::Assert { condition, .. } => {
                barrier = true;
                dependencies(app, condition, types, origins, &mut reads, meter)?;
            }
            Statement::Archive { .. } | Statement::Collection { .. } | Statement::Emit { .. } => {
                return Err(refuse(
                    &format!("{} {at:?}", action.id),
                    "unknown continuation mutation/output barrier",
                ));
            }
        }
        out.push(Node {
            path: at.clone(),
            context,
            statement: header,
            reads,
            writes,
            edit,
            barrier,
        });
        if let Some((steps, mut t, mut o, c)) = nested {
            walk(
                app, action, steps, &at, &c, ensures, &mut t, &mut o, sources, side, out, meter,
            )?;
        }
        if let Some((name, e, origin)) = bind_create {
            types.insert(name.clone(), Type::reference(e));
            origins.insert(name, origin);
        }
    }
    Ok(())
}

fn same_shell(
    old: &AppDefinition,
    new: &AppDefinition,
    side: OwnedSourceSide,
    root: &str,
) -> AdmissionResult<()> {
    if old.id != new.id
        || old.version != new.version
        || old.entities != new.entities
        || old.state != new.state
        || old.outputs != new.outputs
        || old.initial_view != new.initial_view
        || old.actions.len() != new.actions.len()
    {
        return Err(refuse(
            "continuation",
            "business schema, state, output or action inventory changed",
        ));
    }
    // Baseline destinations include their complete selection/control headers.
    // Candidate projection expressions may change, but no other view/control or
    // non-root observation can silently become a new policy surface.
    let shell = |app: &AppDefinition| {
        let mut views = app.views.clone();
        let mut observables = app.observables.clone();
        if side == OwnedSourceSide::After {
            for view in &mut views {
                if let ViewKind::List {
                    entity, columns, ..
                } = &mut view.kind
                {
                    if entity == root {
                        for column in columns {
                            column.value = owned_lowering::literal(
                                DataValue::Boolean { value: false },
                                Type::Boolean,
                            );
                        }
                    }
                }
            }
            for observable in &mut observables {
                if let Expr::Count { items } = &mut observable.value {
                    if let Expr::Query {
                        entity, predicate, ..
                    } = items.as_mut()
                    {
                        if entity == root {
                            **predicate = owned_lowering::literal(
                                DataValue::Boolean { value: false },
                                Type::Boolean,
                            );
                        }
                    }
                }
            }
        }
        (views, observables)
    };
    if shell(old) != shell(new) {
        return Err(refuse(
            "continuation observation",
            "baseline read or complete view/query/control surface changed",
        ));
    }
    for action in &old.actions {
        let next = new
            .actions
            .iter()
            .find(|a| a.id == action.id)
            .ok_or_else(|| refuse(&action.id, "original action missing"))?;
        if action.parameters != next.parameters
            || action.guards != next.guards
            || action.ensures != next.ensures
        {
            return Err(refuse(
                &action.id,
                "parameter, guard or successful-execution condition changed",
            ));
        }
    }
    Ok(())
}
fn endpoint<'a>(
    action: &'a ActionDefinition,
    at: &OwnedSpanEndpoint,
) -> AdmissionResult<&'a [Statement]> {
    let mut steps = action.steps.as_slice();
    for index in &at.list_path {
        let Some(Statement::ForEach { steps: inner, .. }) = steps.get(*index) else {
            return Err(refuse(&action.id, "invalid continuation endpoint ancestry"));
        };
        steps = inner;
    }
    steps
        .get(
            at.start
                ..at.start
                    .checked_add(at.len)
                    .ok_or_else(|| refuse(&action.id, "endpoint overflow"))?,
        )
        .ok_or_else(|| refuse(&action.id, "continuation endpoint outside action"))
}
fn projection_key(recipe: &OwnedProjectionRecipe) -> AdmissionResult<Vec<u8>> {
    Ok(canonical_bytes(&recipe.destination)?)
}
fn alias_key(recipe: &OwnedProjectionRecipe) -> AdmissionResult<Vec<u8>> {
    Ok(canonical_bytes(&(
        &recipe.binding,
        &recipe.value_type,
        &recipe.host_value_type,
        &recipe.baseline,
        &recipe.candidate,
        &recipe.absent_value,
    ))?)
}
fn reorder_projections(
    original: &[OwnedProjectionRecipe],
    current: Vec<OwnedProjectionRecipe>,
    layout: &Metadata,
) -> AdmissionResult<Vec<OwnedProjectionRecipe>> {
    let mut map = BTreeMap::new();
    for p in current {
        if map.insert(projection_key(&p)?, p).is_some() {
            return Err(refuse("projection", "duplicate destination"));
        }
    }
    let mut ordered = Vec::new();
    for p in original {
        let next = map
            .remove(&projection_key(p)?)
            .ok_or_else(|| refuse(&p.location, "original saved destination missing"))?;
        if p.binding != next.binding
            || p.value_type != next.value_type
            || p.host_value_type != next.host_value_type
            || p.absent_value != next.absent_value
            || p.baseline != next.baseline
            || p.capture_required != next.capture_required
        {
            return Err(refuse(
                &p.location,
                "baseline/type/presence/capture contract changed",
            ));
        }
        ordered.push(next);
    }
    if !map.is_empty() || ordered.len() != layout.saved.len() {
        return Err(refuse("projection", "extra or missing saved cell"));
    }
    for i in 0..ordered.len() {
        for j in 0..i {
            if (layout.saved[i] == layout.saved[j])
                != (alias_key(&ordered[i])? == alias_key(&ordered[j])?)
            {
                return Err(refuse(
                    &ordered[i].location,
                    "original capture alias group split or merged",
                ));
            }
        }
    }
    Ok(ordered)
}

/// This result is minted only after comparing the full structured closure.
struct VerifiedSourceContinuation {
    identity: Digest,
    protected: Dependencies,
    influenced: Dependencies,
}
fn charged_bytes<T: Serialize + ?Sized>(
    value: &T,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<u8>> {
    meter.charge(1)?;
    let bytes = canonical_bytes(&value)?;
    let count = u64::try_from(bytes.len())
        .map_err(|_| refuse("budget", "canonical byte count overflow"))?;
    meter.charge(count)?;
    Ok(bytes)
}
fn materially_influenced(read: &Dependency, influenced: &Dependencies) -> bool {
    match read {
        // A shared lexical receiver is association evidence, not policy taint.
        // Only an actually produced private identity can flow through a binder.
        Dependency::Binding(name, origin) => {
            influenced.contains(&Dependency::CreatedIdentity(name.clone(), origin.clone()))
        }
        _ => influenced.contains(read),
    }
}
#[derive(Clone, Serialize)]
enum ProjectedEffect {
    Gap {
        edit: Id,
        context: Digest,
    },
    Step(Vec<u8>),
    Loop {
        header: Vec<u8>,
        body: Vec<ProjectedEffect>,
    },
}
fn node_signature(
    n: &Node,
    side: OwnedSourceSide,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<u8>> {
    if side == OwnedSourceSide::After && n.edit.is_some() {
        charged_bytes(
            &(
                &n.context,
                &n.edit,
                &n.writes,
                statement_receiver(&n.statement),
            ),
            meter,
        )
    } else {
        charged_bytes(&(&n.context, &n.edit, &n.statement), meter)
    }
}
#[allow(clippy::too_many_arguments)]
fn project_list(
    steps: &[Statement],
    path: &[usize],
    nodes: &BTreeMap<Vec<usize>, &Node>,
    gaps: &BTreeMap<Vec<usize>, BTreeMap<usize, Vec<ProjectedEffect>>>,
    side: OwnedSourceSide,
    protected: &Dependencies,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<ProjectedEffect>> {
    let mut result = Vec::new();
    for i in 0..=steps.len() {
        meter.charge(1)?;
        if let Some(at) = gaps.get(path).and_then(|positions| positions.get(&i)) {
            result.extend(at.iter().cloned());
        }
        let Some(step) = steps.get(i) else { break };
        let mut at = path.to_vec();
        at.push(i);
        let n = nodes
            .get(&at)
            .ok_or_else(|| refuse("continuation", "missing typed trace node"))?;
        let relevant = n.edit.is_some() || n.barrier || !n.writes.is_disjoint(protected);
        if let Statement::ForEach { steps: inner, .. } = step {
            let body = project_list(inner, &at, nodes, gaps, side, protected, meter)?;
            if relevant || !body.is_empty() {
                result.push(ProjectedEffect::Loop {
                    header: node_signature(n, side, meter)?,
                    body,
                });
            }
        } else if relevant {
            result.push(ProjectedEffect::Step(node_signature(n, side, meter)?));
        }
    }
    Ok(result)
}
fn projected_trace(
    app: &AppDefinition,
    action: &str,
    nodes: &[Node],
    sources: &OwnedSourceRequest,
    side: OwnedSourceSide,
    protected: &Dependencies,
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<u8>> {
    let action = app
        .actions
        .iter()
        .find(|a| a.id == action)
        .ok_or_else(|| refuse(action, "missing source action"))?;
    let mut index = BTreeMap::new();
    for node in nodes {
        meter.charge(1)?;
        if index.insert(node.path.clone(), node).is_some() {
            return Err(refuse(&action.id, "duplicate typed trace node"));
        }
    }
    let mut gaps: BTreeMap<Vec<usize>, BTreeMap<usize, Vec<ProjectedEffect>>> = BTreeMap::new();
    let mut edits: Vec<_> = sources.edits.iter().collect();
    edits.sort_by(|a, b| a.id.cmp(&b.id));
    for edit in edits {
        meter.charge(1)?;
        let ep = if side == OwnedSourceSide::Before {
            &edit.before
        } else {
            &edit.after
        };
        if ep.action == action.id && ep.len == 0 {
            gaps.entry(ep.list_path.clone())
                .or_default()
                .entry(ep.start)
                .or_default()
                .push(ProjectedEffect::Gap {
                    edit: edit.id.clone(),
                    context: endpoint_context(app, action, ep)?,
                });
        }
    }
    let forest = project_list(&action.steps, &[], &index, &gaps, side, protected, meter)?;
    charged_bytes(&forest, meter)
}

/// Freeze the complete relevant effect sequence, including every possible
/// loop-carried predecessor. Iterating globally is conservative: it deliberately
/// refuses ambiguous aliasing rather than assuming a later statement is harmless.
fn continuation(
    old: &CapturedProgram,
    new: &CapturedProgram,
    old_sources: &OwnedSourceRequest,
    new_sources: &OwnedSourceRequest,
    side: OwnedSourceSide,
    seed: &Dependencies,
    controlled: &Dependencies,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedSourceContinuation> {
    same_shell(&old.program, &new.program, side, &old_sources.root_entity)?;
    let old_ids: BTreeSet<_> = old_sources.edits.iter().map(|e| &e.id).collect();
    let new_ids: BTreeSet<_> = new_sources.edits.iter().map(|e| &e.id).collect();
    if old_ids != new_ids || new_ids.len() != new_sources.edits.len() {
        return Err(refuse(
            "logical edits",
            "missing, extra or repeated original logical slot",
        ));
    }
    let mut old_traces = BTreeMap::new();
    let mut new_traces = BTreeMap::new();
    for a in &old.program.actions {
        let b = new
            .program
            .actions
            .iter()
            .find(|b| b.id == a.id)
            .ok_or_else(|| refuse(&a.id, "missing action"))?;
        old_traces.insert(
            a.id.clone(),
            trace(&old.program, a, old_sources, side, meter)?,
        );
        new_traces.insert(
            a.id.clone(),
            trace(&new.program, b, new_sources, side, meter)?,
        );
    }
    for edit in &old_sources.edits {
        let next = new_sources
            .edits
            .iter()
            .find(|e| e.id == edit.id)
            .ok_or_else(|| refuse(&edit.id, "logical edit missing"))?;
        let (a, b) = if side == OwnedSourceSide::Before {
            (&edit.before, &next.before)
        } else {
            (&edit.after, &next.after)
        };
        if a.action != b.action || a.root != b.root {
            return Err(refuse(
                &edit.id,
                "logical edit retargeted to another action/receiver",
            ));
        }
        let old_action = old
            .program
            .actions
            .iter()
            .find(|x| x.id == a.action)
            .ok_or_else(|| refuse(&edit.id, "original action missing"))?;
        let new_action = new
            .program
            .actions
            .iter()
            .find(|x| x.id == b.action)
            .ok_or_else(|| refuse(&edit.id, "new action missing"))?;
        if side == OwnedSourceSide::Before && endpoint(old_action, a)? != endpoint(new_action, b)? {
            return Err(refuse(&edit.id, "original baseline variant changed"));
        }
        // Empty gap endpoints have no statement node: authenticate their full
        // entering environment/control from independently regenerated spans.
        let old_context = endpoint_context(&old.program, old_action, a)?;
        let new_context = endpoint_context(&new.program, new_action, b)?;
        if old_context != new_context {
            return Err(refuse(&edit.id, "lexical receiver/control origin changed"));
        }
    }
    let mut closed = seed.clone();
    let mut influenced: Dependencies = controlled
        .iter()
        .filter(|d| !matches!(d, Dependency::Binding(..)))
        .cloned()
        .collect();
    closed.extend(influenced.iter().cloned());
    loop {
        let previous = (closed.len(), influenced.len());
        for nodes in old_traces.values().chain(new_traces.values()) {
            for n in nodes {
                meter.charge(1)?;
                if n.reads
                    .iter()
                    .any(|read| materially_influenced(read, &influenced))
                {
                    influenced.extend(n.writes.iter().cloned());
                    closed.extend(n.writes.iter().cloned());
                }
                if n.edit.is_some() || n.barrier || !n.writes.is_disjoint(&closed) {
                    closed.extend(n.reads.iter().cloned());
                    closed.extend(n.writes.iter().cloned());
                }
            }
        }
        if (closed.len(), influenced.len()) == previous {
            break;
        }
    }
    let mut frozen = Vec::new();
    for (action, old_nodes) in &old_traces {
        let a = projected_trace(
            &old.program,
            action,
            old_nodes,
            old_sources,
            side,
            &closed,
            meter,
        )?;
        let b = projected_trace(
            &new.program,
            action,
            &new_traces[action],
            new_sources,
            side,
            &closed,
            meter,
        )?;
        if a != b {
            return Err(refuse(
                action,
                "original writer/dependency/barrier sequence changed",
            ));
        }
        frozen.push((action, a));
    }
    let evidence = (
        "owned-baseline-continuation/3",
        revision(old)?,
        revision(new)?,
        side,
        &closed,
        &influenced,
        frozen,
    );
    let bytes = charged_bytes(&evidence, meter)?;
    // canonical_digest is the public domain-separated hash route. Its second
    // canonical traversal is real work, not a reuse of the temporary byte Vec.
    meter.charge(
        u64::try_from(bytes.len())
            .map_err(|_| refuse("budget", "canonical byte count overflow"))?,
    )?;
    drop(bytes);
    let commitment = identity(&evidence)?;
    drop(evidence);
    Ok(VerifiedSourceContinuation {
        identity: commitment,
        protected: closed,
        influenced,
    })
}

fn statement_receiver(step: &Statement) -> Option<&Expr> {
    match step {
        Statement::Update { record, .. } => Some(record),
        Statement::ForEach { items, .. } => Some(items),
        _ => None,
    }
}
fn endpoint_context(
    app: &AppDefinition,
    action: &ActionDefinition,
    at: &OwnedSpanEndpoint,
) -> AdmissionResult<Digest> {
    let mut types = action.parameters.clone();
    let mut origins: Origins = action
        .parameters
        .iter()
        .map(|(name, typ)| {
            Ok((
                name.clone(),
                identity(&("parameter", &action.id, name, typ))?,
            ))
        })
        .collect::<AdmissionResult<_>>()?;
    let mut controls = action.guards.clone();
    let mut steps = action.steps.as_slice();
    for (level, index) in at
        .list_path
        .iter()
        .chain(std::iter::once(&at.start))
        .enumerate()
    {
        for step in steps
            .get(..*index)
            .ok_or_else(|| refuse(&action.id, "bad endpoint context"))?
        {
            if let Statement::Create { entity, bind, .. } = step {
                let origin = identity(&("create", bind, step, &origins, &controls))?;
                types.insert(bind.clone(), Type::reference(entity));
                origins.insert(bind.clone(), origin);
            }
        }
        if level == at.list_path.len() {
            break;
        }
        let Some(Statement::ForEach {
            items,
            binding,
            steps: inner,
        }) = steps.get(*index)
        else {
            return Err(refuse(&action.id, "non-loop endpoint context"));
        };
        (types, origins) = bind(app, items, binding, "for_each", &types, &origins)?;
        controls.push(items.clone());
        steps = inner;
    }
    identity(&(
        &action.id,
        &action.parameters,
        &action.guards,
        &action.ensures,
        controls,
        types,
        origins,
    ))
}

pub(super) struct VerifiedOwnedContinuation {
    current: ProjectSnapshot,
    before: CapturedProgram,
    after: CapturedProgram,
    request: OwnedPreviewRequest,
    report: CommonFactAdmissionReport,
    layout: Metadata,
    storage_fields: Vec<FieldDefinition>,
    layer: Digest,
    initialization: Digest,
    continuation: Vec<Digest>,
}
impl VerifiedOwnedContinuation {
    pub(super) fn current(&self) -> &ProjectSnapshot {
        &self.current
    }
    pub(super) fn before(&self) -> &CapturedProgram {
        &self.before
    }
    pub(super) fn after(&self) -> &CapturedProgram {
        &self.after
    }
    pub(super) fn request(&self) -> &OwnedPreviewRequest {
        &self.request
    }
    pub(super) fn report(&self) -> &CommonFactAdmissionReport {
        &self.report
    }
    pub(super) fn projections(&self) -> &[OwnedProjectionRecipe] {
        &self.report.projections
    }
    pub(super) fn layout(&self) -> &Metadata {
        &self.layout
    }
    pub(super) fn storage_fields(&self) -> &[FieldDefinition] {
        &self.storage_fields
    }
    pub(super) fn layer(&self) -> &Digest {
        &self.layer
    }
    pub(super) fn original_layer(&self) -> &Digest {
        &self.layer
    }
    pub(super) fn original_initialization(&self) -> &Digest {
        &self.initialization
    }
    pub(super) fn initialization(&self) -> &Digest {
        &self.initialization
    }
    pub(super) fn continuation_identity(&self) -> AdmissionResult<Digest> {
        identity(&self.continuation)
    }
    pub(super) fn completed(&self, row: &Record) -> AdmissionResult<bool> {
        let (field, value) = lifecycle_parts(&self.request.inventory.lifecycle.before)?;
        Ok(row.values.get(field) == Some(value))
    }
    pub(super) fn projection_present(&self, p: &OwnedProjectionRecipe, active: bool) -> bool {
        active || p.baseline.is_some()
    }
    pub(super) fn capture_expression(
        &self,
        p: &OwnedProjectionRecipe,
        binding: &str,
        active: bool,
    ) -> AdmissionResult<Expr> {
        capture_expression(&self.layout, p, binding, active)
    }
}
pub(super) fn capture_expression(
    layout: &Metadata,
    p: &OwnedProjectionRecipe,
    binding: &str,
    active: bool,
) -> AdmissionResult<Expr> {
    if active {
        return layout.live(p, binding);
    }
    Ok(match &p.baseline {
        Some(e) => owned_lowering::rebind(e, &p.binding, binding),
        None => owned_lowering::literal(
            owned_lowering::empty(&p.host_value_type)?,
            p.host_value_type.clone(),
        ),
    })
}
fn lifecycle_parts(expr: &Expr) -> AdmissionResult<(&str, &DataValue)> {
    let Expr::Equal { left, right } = expr else {
        return Err(refuse("lifecycle", "unsupported predicate"));
    };
    match (left.as_ref(), right.as_ref()) {
        (Expr::Field { record, field }, Expr::Literal { value, .. }) if matches!(record.as_ref(),Expr::Variable{name} if name=="record") => {
            Ok((field, value))
        }
        _ => Err(refuse(
            "lifecycle",
            "not exact own-row completion predicate",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn verify(
    current: &ProjectSnapshot,
    original: &PreparedOwnedPreview,
    layer: &OwnedLayerV2,
    initialization: &OwnedInitializationV2,
    predecessor_before: &CapturedProgram,
    predecessor_after: &CapturedProgram,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedContinuation> {
    meter.charge(1)?;
    let state = current.owned_scope()?;
    let layer_id = layer.identity()?;
    let init_id = initialization.identity()?;
    let tail = state
        .adoptions
        .last()
        .and_then(|r| state.compositions.get(&r.composition))
        .ok_or_else(|| refuse("continuation", "missing committed predecessor"))?;
    let initial_output = state
        .adoptions
        .first()
        .and_then(|r| state.compositions.get(&r.composition))
        .map(|m| &m.output);
    if original.metadata.revision.is_none()
        || initial_output != Some(&revision(&original.target)?)
        || state.layers.len() != 1
        || state.layers.get(&layer_id) != Some(layer)
        || state.initializations.len() != 1
        || state.initializations.first() != Some(initialization)
        || tail.output != current.active_revision
        || tail.layers != vec![layer_id.clone()]
        || layer.request != original.proof.request
        || layer.candidate != revision(&original.proof.candidate)?
        || initialization.layer != layer_id
        || initialization.after_data != original.initialized.identity()?
        || initialization.before_data != original.proof.snapshot.data.identity()?
        || original.metadata.root != sources.root_entity
    {
        return Err(refuse(
            "continuation",
            "original layer/initializer or immediate committed predecessor mismatch",
        ));
    }
    let predecessor_sources = if tail.correspondences.is_empty() {
        &layer.request.inventory.sources
    } else {
        if tail.correspondences.len() != 1 {
            return Err(refuse("continuation", "ambiguous effective pair"));
        }
        &tail.correspondences[0].sources
    };
    if predecessor_sources.before != revision(predecessor_before)?
        || predecessor_sources.after != revision(predecessor_after)?
        || tail.business != revision(predecessor_after)?
    {
        return Err(refuse(
            "continuation",
            "wrong immediate predecessor ordinary pair",
        ));
    }
    for source in [before, after] {
        meter.charge(source.source_bytes.len() as u64)?;
        source.validate()?;
        super::compiler::reject_reserved(&source.program)?;
        if source.binding.project_id != current.data.project_id
            || source.program.id != original.proof.candidate.program.id
        {
            return Err(refuse("continuation", "foreign ordinary program"));
        }
    }
    let mut request = layer.request.clone();
    request.inventory.snapshot = canonical_digest(IdentityDomain::Data, current)?;
    request.inventory.sources = sources.clone();
    let inventory =
        super::common_fact_inventory::derive_pair(current, before, after, &request.inventory)?;
    // Every original policy writer is a seed, even if copied into both new
    // programs and thus absent from their newly proposed difference inventory.
    let original_inventory = super::common_fact_inventory::derive(
        &original.proof.snapshot,
        &original.proof.candidate,
        &original.proof.request.inventory,
    )?;
    let mut seed = Dependencies::new();
    for writer in &original_inventory.structure.writers {
        let edited = original_inventory.structure.edits.iter().any(|e| {
            let ep = if writer.side == OwnedSourceSide::Before {
                &e.before
            } else {
                &e.after
            };
            ep.action == writer.action && inside(&writer.path, ep)
        });
        if edited {
            if let Some(field) = &writer.field {
                seed.insert(Dependency::Field(writer.entity.clone(), field.clone()));
            }
        }
    }
    for p in &original.proof.report.projections {
        if let Some(expr) = &p.baseline {
            let types = Types::from([(p.binding.clone(), Type::reference(&sources.root_entity))]);
            let origins = Origins::from([(
                p.binding.clone(),
                identity(&("projection", &p.destination, &p.binding))?,
            )]);
            dependencies(&before.program, expr, &types, &origins, &mut seed, meter)?;
        }
    }
    let (completed, _) = lifecycle_parts(&request.inventory.lifecycle.before)?;
    seed.insert(Dependency::Field(
        sources.root_entity.clone(),
        completed.into(),
    ));
    let mut controlled = Dependencies::new();
    for (source, side) in [
        (original.proof.snapshot.program()?, OwnedSourceSide::Before),
        (&original.proof.candidate, OwnedSourceSide::After),
    ] {
        for action in &source.program.actions {
            for node in trace(
                &source.program,
                action,
                &original.proof.request.inventory.sources,
                side,
                meter,
            )? {
                if node.edit.is_some() {
                    controlled.extend(node.writes);
                }
            }
        }
    }
    seed.extend(controlled.iter().cloned());
    let closed = ClosedContinuationRules::verify(
        original,
        predecessor_before,
        predecessor_after,
        predecessor_sources,
        before,
        after,
        &request,
        &inventory,
        &seed,
        &controlled,
        meter,
    )?;
    let continuation_ids = closed
        .proofs
        .iter()
        .map(|proof| proof.identity.clone())
        .collect();
    let retained = RetainedOwned::new(original, closed)?;
    let (certificates, projections, comparison) = super::common_fact_rules::verify_pair_frame(
        before,
        after,
        &request,
        &inventory,
        &current.data,
        Some(&retained),
        meter,
    )?;
    let projections = reorder_projections(
        &original.proof.report.projections,
        projections,
        &original.metadata,
    )?;
    let report = CommonFactAdmissionReport {
        version: 1,
        snapshot: request.inventory.snapshot.clone(),
        before: inventory.structure.before,
        after: inventory.structure.after,
        decisions: current.decisions.identity()?,
        acceptance: original.proof.report.acceptance.clone(),
        certificates,
        projections,
        phases: inventory.actions,
        outcome_comparison: comparison,
        capture_presence_required: true,
        durable_adoption_supported: false,
    };
    Ok(VerifiedOwnedContinuation {
        current: current.clone(),
        before: before.clone(),
        after: after.clone(),
        request,
        report,
        layout: original.metadata.clone(),
        storage_fields: original.metadata.fields(&original.proof),
        layer: layer_id,
        initialization: init_id,
        continuation: continuation_ids,
    })
}

/// Read-only source-qualified facts, never caller-constructible write authority.
/// A valid old P4 proof may yield an empty surface: unsupported retirement does
/// not retroactively make its original acceptance corrupt.
pub(crate) struct VerifiedOwnedPolicySurface {
    source: Digest,
    original_layer: Digest,
    baseline: Digest,
    candidate: Digest,
    counters: BTreeMap<Id, i64>,
    root: Id,
    member: Id,
    sealed: Id,
    schema: Vec<EntityDefinition>,
    policy_fields: BTreeMap<Id, BTreeSet<Id>>,
}
impl VerifiedOwnedPolicySurface {
    pub(crate) fn qualify_observable(&self, id: &str) -> Option<i64> {
        self.counters.get(id).copied()
    }
    pub(crate) fn source(&self) -> &Digest {
        &self.source
    }
    pub(crate) fn original_layer(&self) -> &Digest {
        &self.original_layer
    }
    pub(crate) fn baseline(&self) -> &Digest {
        &self.baseline
    }
    pub(crate) fn candidate(&self) -> &Digest {
        &self.candidate
    }
    pub(crate) fn validate_frame(&self, data: &DataSnapshot) -> AdmissionResult<()> {
        if data.schema != self.schema {
            return Err(refuse(
                "policy frame",
                "accepted protected storage differs from original layout",
            ));
        }
        for row in data.records.iter().filter(|r| r.entity == self.root) {
            let Some(DataValue::Boolean { value: member }) = row.values.get(&self.member) else {
                return Err(refuse(&row.id, "missing exact member cell"));
            };
            let Some(DataValue::Boolean { value: sealed }) = row.values.get(&self.sealed) else {
                return Err(refuse(&row.id, "missing exact sealed cell"));
            };
            verify_policy_cells(row, *member, *sealed, &self.policy_fields)?;
        }
        Ok(())
    }
}
fn group_policy_cells(entries: Vec<(Id, Id)>) -> BTreeMap<Id, BTreeSet<Id>> {
    let mut fields: BTreeMap<Id, BTreeSet<Id>> = BTreeMap::new();
    for (field, saved) in entries {
        fields.entry(field).or_default().insert(saved);
    }
    fields
}
fn verify_policy_cells(
    row: &Record,
    member: bool,
    sealed: bool,
    fields: &BTreeMap<Id, BTreeSet<Id>>,
) -> AdmissionResult<()> {
    if row.archived {
        return Ok(());
    }
    for (field, saved) in fields {
        if (!sealed
            && !member
            && row.values.get(field) != Some(&DataValue::Boolean { value: false }))
            || (sealed
                && saved
                    .iter()
                    .any(|key| row.values.get(key) != Some(&DataValue::Boolean { value: false })))
        {
            return Err(refuse(
                &row.id,
                "nonmember or sealed policy contribution lacks neutral producer history",
            ));
        }
    }
    Ok(())
}
fn neutral_original_rows(
    rows: &[Record],
    root: &str,
    completed: (&str, &DataValue),
    field: &str,
) -> bool {
    rows.iter()
        .filter(|r| {
            r.entity == root && !r.archived && r.values.get(completed.0) != Some(completed.1)
        })
        .all(|r| r.values.get(field) == Some(&DataValue::Boolean { value: false }))
}
fn qualifying_count<'a>(
    expression: &'a Expr,
    root: &str,
    field: &str,
    lifecycle: &Expr,
) -> Option<&'a str> {
    let Expr::Count { items } = expression else {
        return None;
    };
    let Expr::Query {
        entity,
        binding,
        predicate,
        sort,
        limit,
        include_archived,
    } = items.as_ref()
    else {
        return None;
    };
    if entity != root || !sort.is_empty() || *limit != MAX_COLLECTION || *include_archived {
        return None;
    }
    let expected = Expr::And {
        values: vec![
            Expr::Not {
                value: Box::new(owned_lowering::rebind(lifecycle, "record", binding)),
            },
            Expr::Field {
                record: Box::new(Expr::Variable {
                    name: binding.clone(),
                }),
                field: field.into(),
            },
        ],
    };
    (predicate.as_ref() == &expected).then_some(binding.as_str())
}
fn policy_field_sources(
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    field: &str,
    meter: &mut ProofMeter,
) -> AdmissionResult<bool> {
    let root = &sources.root_entity;
    let mut writes = [BTreeMap::<(Id, Vec<u8>), Vec<Expr>>::new(), BTreeMap::new()];
    for (index, (source, side)) in [
        (before, OwnedSourceSide::Before),
        (after, OwnedSourceSide::After),
    ]
    .into_iter()
    .enumerate()
    {
        for action in &source.program.actions {
            for n in trace(&source.program, action, sources, side, meter)? {
                if !n
                    .writes
                    .contains(&Dependency::Field(root.clone(), field.into()))
                {
                    continue;
                }
                match &n.statement {
                    Statement::Create { entity, values, .. } if entity == root => {
                        if n.edit.is_some()
                            || values.get(field)
                                != Some(&owned_lowering::literal(
                                    DataValue::Boolean { value: false },
                                    Type::Boolean,
                                ))
                        {
                            return Ok(false);
                        }
                    }
                    Statement::Update { record, values } => {
                        let Some(id) = n.edit.as_ref() else {
                            return Ok(false);
                        };
                        let Some(edit) = sources.edits.iter().find(|e| &e.id == id) else {
                            return Ok(false);
                        };
                        let endpoint = if side == OwnedSourceSide::Before {
                            &edit.before
                        } else {
                            &edit.after
                        };
                        let Some(binding) = &endpoint.root else {
                            return Ok(false);
                        };
                        if binding.value_type != Type::reference(root)
                            || !matches!(record,Expr::Variable{name} if name==&binding.name)
                        {
                            return Ok(false);
                        }
                        let Some(value) = values.get(field) else {
                            return Ok(false);
                        };
                        if side == OwnedSourceSide::Before
                            && value
                                != &owned_lowering::literal(
                                    DataValue::Boolean { value: false },
                                    Type::Boolean,
                                )
                        {
                            return Ok(false);
                        }
                        writes[index]
                            .entry((id.clone(), canonical_bytes(record)?))
                            .or_default()
                            .push(value.clone());
                    }
                    _ => return Ok(false),
                }
            }
        }
    }
    if writes[0].is_empty() || writes[0].keys().ne(writes[1].keys()) {
        return Ok(false);
    }
    let mut genuine = false;
    for (key, old) in &writes[0] {
        let new = &writes[1][key];
        if old.len() != new.len() {
            return Ok(false);
        }
        genuine |= old != new;
    }
    Ok(genuine)
}
fn protected_count(
    target: &CapturedProgram,
    observable: &str,
    layout: &Metadata,
    recipe: &OwnedProjectionRecipe,
    index: usize,
) -> AdmissionResult<bool> {
    let Some(o) = target
        .program
        .observables
        .iter()
        .find(|o| o.id == observable)
    else {
        return Ok(false);
    };
    let Expr::Count { items } = &o.value else {
        return Ok(false);
    };
    let Expr::Query {
        entity,
        binding,
        predicate,
        sort,
        limit,
        include_archived,
    } = items.as_ref()
    else {
        return Ok(false);
    };
    if entity != &layout.root
        || binding != &recipe.binding
        || !sort.is_empty()
        || *limit != MAX_COLLECTION
        || *include_archived
    {
        return Ok(false);
    }
    let field = |id: &str| Expr::Field {
        record: Box::new(Expr::Variable {
            name: binding.clone(),
        }),
        field: id.into(),
    };
    let Some(saved) = layout.saved.get(index) else {
        return Ok(false);
    };
    let expected = Expr::If {
        condition: Box::new(field(&layout.sealed)),
        then_value: Box::new(field(saved)),
        else_value: Box::new(layout.live(recipe, binding)?),
    };
    Ok(predicate.as_ref() == &expected)
}

/// `accepted` is supplied only by the checked execution registry, after complete
/// producer-prefix validation. The current value of F never establishes origin.
pub(super) fn derive_policy_surface(
    original: &PreparedOwnedPreview,
    layer: &OwnedLayerV2,
    accepted: Option<(&VerifiedOwnedContinuation, &CapturedProgram)>,
    meter: &mut ProofMeter,
) -> AdmissionResult<VerifiedOwnedPolicySurface> {
    let layer_id = layer.identity()?;
    if original.proof.request != layer.request
        || revision(&original.proof.candidate)? != layer.candidate
    {
        return Err(refuse(
            "policy ownership",
            "wrong reconstructed original layer",
        ));
    }
    let old_before = original.proof.snapshot.program()?;
    let old_after = &original.proof.candidate;
    let (before, after, request, projections, target) = match accepted {
        Some((p, target)) => {
            if p.layer() != &layer_id {
                return Err(refuse("policy ownership", "foreign accepted producer"));
            }
            (p.before(), p.after(), p.request(), p.projections(), target)
        }
        None => (
            old_before,
            old_after,
            &original.proof.request,
            original.proof.report.projections.as_slice(),
            &original.target,
        ),
    };
    let root = &layer.request.inventory.sources.root_entity;
    let life = &layer.request.inventory.lifecycle.before;
    let completed = lifecycle_parts(life)?;
    let max = match &layer.request.population {
        ScopePopulation::All | ScopePopulation::FutureWork => MAX_COLLECTION as i64,
        ScopePopulation::SelectedUnfinished { .. } => i64::try_from(
            original
                .initialized
                .records
                .iter()
                .filter(|r| {
                    r.entity == *root
                        && !r.archived
                        && r.values.get(&original.metadata.member)
                            == Some(&DataValue::Boolean { value: true })
                        && r.values.get(&original.metadata.sealed)
                            == Some(&DataValue::Boolean { value: false })
                })
                .count(),
        )
        .map_err(|_| refuse("policy ownership", "population bound overflow"))?,
    };
    let mut counters = BTreeMap::new();
    let mut policy_fields = Vec::new();
    let definition = old_before
        .program
        .entities
        .iter()
        .find(|e| &e.id == root)
        .ok_or_else(|| refuse("policy ownership", "original root missing"))?;
    for field in &definition.fields {
        meter.charge(1)?;
        if field.value_type != Type::Boolean || field.id == completed.0 || max == 0 {
            continue;
        }
        if !neutral_original_rows(
            &original.proof.snapshot.data.records,
            root,
            completed,
            &field.id,
        ) {
            continue;
        }
        if !policy_field_sources(
            old_before,
            old_after,
            &layer.request.inventory.sources,
            &field.id,
            meter,
        )? || !policy_field_sources(before, after, &request.inventory.sources, &field.id, meter)?
        {
            continue;
        }
        for (index, p) in projections.iter().enumerate() {
            let OwnedProjectionDestination::RootCount {
                observable,
                include_archived: false,
            } = &p.destination
            else {
                continue;
            };
            meter.charge(1)?;
            let old = original
                .proof
                .report
                .projections
                .iter()
                .find(|q| q.destination == p.destination);
            let Some(old) = old else {
                continue;
            };
            if old.value_type != Type::Boolean
                || old.host_value_type != Type::Boolean
                || old.baseline.is_none()
                || p.value_type != Type::Boolean
                || p.host_value_type != Type::Boolean
                || p.baseline.is_none()
            {
                continue;
            }
            let mut exact = true;
            for source in [old_before, old_after, before, after] {
                let Some(o) = source
                    .program
                    .observables
                    .iter()
                    .find(|o| &o.id == observable)
                else {
                    exact = false;
                    break;
                };
                if qualifying_count(&o.value, root, &field.id, life) != Some(p.binding.as_str())
                    || source.program.expression_type_in(&o.value, &Types::new())? != Type::Integer
                {
                    exact = false;
                    break;
                }
            }
            if !exact || !protected_count(target, observable, &original.metadata, p, index)? {
                continue;
            }
            // The original already-sealed selector is a genuine false capture,
            // not a current-value guess. Historical producer validation checks
            // every later seal at the actual post-completion barrier.
            let key = &original.metadata.saved[index];
            if original
                .initialized
                .records
                .iter()
                .filter(|r| {
                    r.entity == *root
                        && !r.archived
                        && r.values.get(&original.metadata.sealed)
                            == Some(&DataValue::Boolean { value: true })
                })
                .any(|r| r.values.get(key) != Some(&DataValue::Boolean { value: false }))
            {
                continue;
            }
            counters.insert(observable.clone(), max);
            policy_fields.push((field.id.clone(), key.clone()));
        }
    }
    meter.charge(original.proof.snapshot.data.records.len() as u64)?;
    meter.charge(
        u64::try_from(policy_fields.len())
            .map_err(|_| refuse("budget", "policy cell count overflow"))?,
    )?;
    let policy_fields = group_policy_cells(policy_fields);
    let surface = VerifiedOwnedPolicySurface {
        source: revision(target)?,
        original_layer: layer_id,
        baseline: revision(before)?,
        candidate: revision(after)?,
        counters,
        root: root.clone(),
        member: original.metadata.member.clone(),
        sealed: original.metadata.sealed.clone(),
        schema: original.initialized.schema.clone(),
        policy_fields,
    };
    surface.validate_frame(&original.initialized)?;
    if let Some((continuation, _)) = accepted {
        surface.validate_frame(&continuation.current().data)?;
    }
    Ok(surface)
}

#[cfg(test)]
#[path = "owned_continuation_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "owned_continuation_amendment_tests.rs"]
mod amendment_tests;
