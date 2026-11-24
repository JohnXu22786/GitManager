//! Exact source correspondence only. This module never evaluates expressions,
//! lowers effects, initializes protected data, or constructs preparation handles.
use super::owned_contract::*;
use crate::product_contract::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, ContractError>;
type Environment = BTreeMap<Id, Type>;
fn error(message: impl Into<String>) -> ContractError {
    ContractError(format!("Owned source structure: {}", message.into()))
}
fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(error(message))
    }
}
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<Digest> {
    canonical_digest(IdentityDomain::Evidence, &value)
}
fn list_item(app: &AppDefinition, items: &Expr, env: &Environment) -> Result<Type> {
    match app.expression_type_in(items, env)? {
        Type::List { item } => Ok(*item),
        _ => Err(error("loop does not have a typed collection")),
    }
}
fn extend_creates(env: &mut Environment, steps: &[Statement]) {
    for step in steps {
        if let Statement::Create { entity, bind, .. } = step {
            env.insert(bind.clone(), Type::reference(entity));
        }
    }
}
struct Resolved<'a> {
    steps: &'a [Statement],
    env: Environment,
}
fn resolve<'a>(
    source: &'a CapturedProgram,
    identity: &Digest,
    root_entity: &str,
    endpoint: &OwnedSpanEndpoint,
) -> Result<Resolved<'a>> {
    let location = format!(
        "{} {:?} at {}",
        endpoint.action, endpoint.list_path, endpoint.start
    );
    require(
        &endpoint.source == identity,
        format!("stale source at {location}"),
    )?;
    require(
        endpoint.list_path.len() <= MAX_EXPR_DEPTH && endpoint.len <= MAX_ITEMS,
        format!("span bounds exceeded at {location}"),
    )?;
    let action = source
        .program
        .actions
        .iter()
        .find(|a| a.id == endpoint.action)
        .ok_or_else(|| error(format!("unknown action at {location}")))?;
    let mut steps = action.steps.as_slice();
    let mut env = action.parameters.clone();
    for index in &endpoint.list_path {
        let step = steps
            .get(*index)
            .ok_or_else(|| error(format!("missing list path at {location}")))?;
        extend_creates(&mut env, &steps[..*index]);
        let Statement::ForEach {
            items,
            binding,
            steps: nested,
        } = step
        else {
            return Err(error(format!("list path is not a ForEach at {location}")));
        };
        env.insert(binding.clone(), list_item(&source.program, items, &env)?);
        steps = nested;
    }
    let end = endpoint
        .start
        .checked_add(endpoint.len)
        .filter(|end| *end <= steps.len())
        .ok_or_else(|| error(format!("span outside statement list at {location}")))?;
    extend_creates(&mut env, &steps[..endpoint.start]);
    let span = &steps[endpoint.start..end];
    require(
        endpoint.kinds == span.iter().map(OwnedStatementKind::of).collect::<Vec<_>>(),
        format!("statement kind mismatch at {location}"),
    )?;
    require(
        endpoint.digest == digest(&("owned-span/1", span))?,
        format!("span digest mismatch at {location}"),
    )?;
    let previous = endpoint
        .start
        .checked_sub(1)
        .map(|i| digest(&steps[i]))
        .transpose()?;
    let next = steps.get(end).map(digest).transpose()?;
    require(
        endpoint.previous == previous && endpoint.next == next,
        format!("neighbor anchor mismatch at {location}"),
    )?;
    if let Some(root) = &endpoint.root {
        require(
            root.value_type == Type::reference(root_entity)
                && env.get(&root.name) == Some(&root.value_type),
            format!("root binder or type mismatch at {location}"),
        )?;
    }
    // Create extends its containing list's scope. Guarding a selected span would
    // instead make these bindings local, so no use may escape that span.
    let tail = crate::product_contract::StatementSlice::Legacy(&steps[end..]);
    let ensures = if endpoint.list_path.is_empty() { action.ensures.as_slice() } else { &[] };
    for statement in span {
        if let Statement::Create { bind, .. } = statement {
            require(
                !super::owned_source_shared::statements_contain_variable(tail, bind)
                    && !ensures.iter().any(|expr| super::owned_source_shared::expression_contains_variable(expr, bind)),
                format!("Create binder {bind} escapes span at {location}"),
            )?;
        }
    }
    Ok(Resolved { steps: span, env })
}
fn overlaps(a: &OwnedSpanEndpoint, b: &OwnedSpanEndpoint) -> bool {
    if a.action != b.action {
        return false;
    }
    if a.list_path == b.list_path {
        return a.start == b.start
            || (a.start < b.start && b.start < a.start + a.len)
            || (b.start < a.start && a.start < b.start + b.len);
    }
    // A selected parent statement covers all of its descendant lists, including
    // empty gaps. A gap itself covers no descendant statement.
    if b.list_path.starts_with(&a.list_path) {
        let child = b.list_path[a.list_path.len()];
        return a.start <= child && child < a.start + a.len;
    }
    if a.list_path.starts_with(&b.list_path) {
        return overlaps(b, a);
    }
    false
}

pub(super) enum StatementHeader<'a> {
    Create { entity: &'a str, values: crate::product_contract::StringMapRef<'a, Expr>, bind: &'a str },
    Update { record: &'a Expr, values: crate::product_contract::StringMapRef<'a, Expr> },
    Archive { record: &'a Expr },
    SetState { state: &'a str, value: &'a Expr },
    Collection { target: &'a CollectionTarget, operation: CollectionOperation, items: &'a Expr },
    ForEach { items: &'a Expr, binding: &'a str },
    Assert { condition: &'a Expr, message: &'a str },
    Emit { output: &'a str, items: &'a Expr, binding: &'a str, columns: crate::product_contract::StringMapRef<'a, Expr> },
}
fn same_map(a: &crate::product_contract::StringMapRef<'_, Expr>, b: &crate::product_contract::StringMapRef<'_, Expr>) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|((ak,av),(bk,bv))| ak == bk && av == bv)
}
impl PartialEq for StatementHeader<'_> {
    fn eq(&self, other: &Self) -> bool {
        use StatementHeader as H;
        match (self,other) {
            (H::Create{entity:a,values:av,bind:ab},H::Create{entity:b,values:bv,bind:bb}) => a==b && ab==bb && same_map(av,bv),
            (H::Update{record:a,values:av},H::Update{record:b,values:bv}) => a==b && same_map(av,bv),
            (H::Archive{record:a},H::Archive{record:b}) => a==b,
            (H::SetState{state:a,value:av},H::SetState{state:b,value:bv}) => a==b && av==bv,
            (H::Collection{target:a,operation:ao,items:ai},H::Collection{target:b,operation:bo,items:bi}) => a==b && ao==bo && ai==bi,
            (H::ForEach{items:a,binding:ab},H::ForEach{items:b,binding:bb}) => a==b && ab==bb,
            (H::Assert{condition:a,message:am},H::Assert{condition:b,message:bm}) => a==b && am==bm,
            (H::Emit{output:a,items:ai,binding:ab,columns:ac},H::Emit{output:b,items:bi,binding:bb,columns:bc}) => a==b && ai==bi && ab==bb && same_map(ac,bc),
            _ => false,
        }
    }
}
impl Eq for StatementHeader<'_> {}
pub(super) fn statement_header<'a>(view: crate::product_contract::StatementView<'a>) -> StatementHeader<'a> {
    use crate::product_contract::StatementView as S;
    match view {
        S::Create{entity,values,bind} => StatementHeader::Create{entity,values,bind},
        S::Update{record,values} => StatementHeader::Update{record,values},
        S::Archive{record} => StatementHeader::Archive{record},
        S::SetState{state,value} => StatementHeader::SetState{state,value},
        S::Collection{target,operation,items} => StatementHeader::Collection{target,operation,items},
        S::ForEach{items,binding,..} => StatementHeader::ForEach{items,binding},
        S::Assert{condition,message} => StatementHeader::Assert{condition,message},
        S::Emit{output,items,binding,columns} => StatementHeader::Emit{output,items,binding,columns},
    }
}
enum Skeleton<'a> {
    Slot(Id),
    Common {
        path: Vec<usize>,
        kind: OwnedStatementKind,
        header: StatementHeader<'a>,
        body: Vec<Skeleton<'a>>,
    },
}
fn skeleton<'a>(
    steps: crate::product_contract::StatementSlice<'a>,
    path: &[usize],
    endpoints: &[(&Id, &OwnedSpanEndpoint)],
) -> Result<Vec<Skeleton<'a>>> {
    let mut result = vec![];
    let mut index = 0;
    while index <= steps.len() {
        if let Some((id, endpoint)) = endpoints
            .iter()
            .find(|(_, e)| e.list_path == path && e.start == index)
        {
            result.push(Skeleton::Slot((*id).clone()));
            if endpoint.len > 0 {
                index += endpoint.len;
                continue;
            }
        }
        if index == steps.len() {
            break;
        }
        let statement = steps.get(index).expect("bounded statement index");
        let mut current = path.to_vec();
        current.push(index);
        let (kind,body) = match statement {
            crate::product_contract::StatementView::ForEach{steps,..} =>
                (OwnedStatementKind::ForEach,skeleton(steps,&current,endpoints)?),
            crate::product_contract::StatementView::Create{..} => (OwnedStatementKind::Create,vec![]),
            crate::product_contract::StatementView::Update{..} => (OwnedStatementKind::Update,vec![]),
            crate::product_contract::StatementView::Archive{..} => (OwnedStatementKind::Archive,vec![]),
            crate::product_contract::StatementView::SetState{..} => (OwnedStatementKind::SetState,vec![]),
            crate::product_contract::StatementView::Collection{..} => (OwnedStatementKind::Collection,vec![]),
            crate::product_contract::StatementView::Assert{..} => (OwnedStatementKind::Assert,vec![]),
            crate::product_contract::StatementView::Emit{..} => (OwnedStatementKind::Emit,vec![]),
        };
        result.push(Skeleton::Common {
            path: current,
            kind,
            header: statement_header(statement),
            body,
        });
        index += 1;
    }
    Ok(result)
}
fn match_skeleton(
    action: &str,
    before: &[Skeleton<'_>],
    after: &[Skeleton<'_>],
    common: &mut Vec<OwnedCommonStatement>,
) -> Result<()> {
    require(
        before.len() == after.len(),
        format!("uncovered or reordered statements in action {action}"),
    )?;
    for (a, b) in before.iter().zip(after) {
        match (a, b) {
            (Skeleton::Slot(a), Skeleton::Slot(b)) if a == b => {}
            (
                Skeleton::Common {
                    path: ap,
                    kind,
                    header: ah,
                    body: ab,
                },
                Skeleton::Common {
                    path: bp,
                    header: bh,
                    body: bb,
                    ..
                },
            ) if ah == bh => {
                common.push(OwnedCommonStatement {
                    action: action.into(),
                    before: ap.clone(),
                    after: bp.clone(),
                    kind: *kind,
                });
                match_skeleton(action, ab, bb, common)?;
            }
            _ => {
                return Err(error(format!(
                    "uncovered or reordered statement/binder in action {action}"
                )))
            }
        }
    }
    Ok(())
}
fn passive_changes(
    before: &AppDefinition,
    after: &AppDefinition,
) -> Result<Vec<OwnedStructuralChange>> {
    use crate::product_alloc::AllocationContext;
    let mut cx = AllocationContext::new();
    let changes = super::fallible_owned_source_analysis::passive_changes(
        AppRef::Legacy(before), AppRef::Legacy(after), &mut cx,
    ).map_err(|failure| match failure {
        super::fallible_owned_source_analysis::SourceAnalysisFailure::Contract(error) => error,
        super::fallible_owned_source_analysis::SourceAnalysisFailure::Allocation(failure) => error(failure.to_string()),
    })?;
    let mut out = Vec::new();
    crate::product_alloc::try_reserve(&mut out, changes.len(), &mut cx, crate::product_alloc::AllocationSite::Vector)
        .map_err(|failure| error(failure.to_string()))?;
    for change in changes {
        out.push(OwnedStructuralChange {
            kind: change.kind,
            container: change.container,
            id: change.id,
            before: change.before,
            after: change.after,
            before_type: change.before_type,
            after_type: change.after_type,
        });
    }
    Ok(out)
}
fn writers(
    app: &AppDefinition,
    source: &Digest,
    side: OwnedSourceSide,
    action: &str,
    steps: &[Statement],
    path: &[usize],
    env: &mut Environment,
    result: &mut Vec<OwnedWriter>,
) -> Result<()> {
    use crate::product_alloc::{AllocationContext, AllocationSite};
    use super::owned_source_shared::LexicalEnvironment;
    let mut cx = AllocationContext::new();
    let mut shared_env = LexicalEnvironment::Legacy(std::mem::take(env));
    let mut decoded = Vec::new();
    super::fallible_owned_source_analysis::writers_for(
        AppRef::Legacy(app), source, side, action, StatementSlice::Legacy(steps), path,
        &mut shared_env, &mut decoded, &mut cx,
    ).map_err(|failure| match failure {
        super::fallible_owned_source_analysis::SourceAnalysisFailure::Contract(error) => error,
        super::fallible_owned_source_analysis::SourceAnalysisFailure::Allocation(failure) => error(failure.to_string()),
    })?;
    crate::product_alloc::try_reserve(result, decoded.len(), &mut cx, AllocationSite::Vector)
        .map_err(|failure| error(failure.to_string()))?;
    for writer in decoded {
        result.push(OwnedWriter {
            side: writer.side, source: writer.source, action: writer.action, path: writer.path,
            kind: writer.kind, entity: writer.entity, field: writer.field, value_type: writer.value_type,
        });
    }
    let LexicalEnvironment::Legacy(updated) = shared_env else { unreachable!("legacy writer environment remains legacy") };
    *env = updated;
    Ok(())
}

macro_rules! analyze_owned_source_tail {
    ($before:ident, $after:ident, $request:ident) => {{
    $after.validate()?;
    require(
        $request.before == canonical_digest(IdentityDomain::Source, $before)?
            && $request.after == canonical_digest(IdentityDomain::Source, $after)?,
        "stale exact source identity",
    )?;
    require(
        $before.binding.project_id == $after.binding.project_id,
        "source projects differ",
    )?;
    require(
        $before
            .program
            .entities
            .iter()
            .any(|e| e.id == $request.root_entity)
            && $after
                .program
                .entities
                .iter()
                .any(|e| e.id == $request.root_entity),
        "root entity is missing",
    )?;
    require($request.edits.len() <= MAX_ITEMS, "too many edit spans")?;
    let mut ids = BTreeSet::new();
    for (index, edit) in $request.edits.iter().enumerate() {
        require(
            valid_id(&edit.id) && ids.insert(&edit.id),
            "invalid or duplicate edit ID",
        )?;
        require(
            edit.before.action == edit.after.action && (edit.before.len > 0 || edit.after.len > 0),
            format!("cross-action or empty correspondence {}", edit.id),
        )?;
        let old = resolve($before, &$request.before, &$request.root_entity, &edit.before)?;
        let new = resolve($after, &$request.after, &$request.root_entity, &edit.after)?;
        require(
            old.env == new.env && edit.before.root == edit.after.root,
            format!("unsupported author binder correspondence in {}", edit.id),
        )?;
        if !old.steps.is_empty() && !new.steps.is_empty() {
            require(
                super::owned_bindings::correspond(
                    &$before.program,
                    old.steps,
                    &$after.program,
                    new.steps,
                    &old.env,
                )?,
                format!(
                    "unsupported typed lexical binder correspondence in {}",
                    edit.id
                ),
            )?;
        }
        for prior in &$request.edits[..index] {
            require(
                !overlaps(&prior.before, &edit.before) && !overlaps(&prior.after, &edit.after),
                format!("overlapping edit spans {} and {}", prior.id, edit.id),
            )?;
        }
    }
    let mut common_statements = vec![];
    for action in &$before.program.actions {
        let candidate = $after
            .program
            .actions
            .iter()
            .find(|a| a.id == action.id)
            .ok_or_else(|| error(format!("removed action {}", action.id)))?;
        let old: Vec<_> = $request
            .edits
            .iter()
            .filter(|e| e.before.action == action.id)
            .map(|e| (&e.id, &e.before))
            .collect();
        let new: Vec<_> = $request
            .edits
            .iter()
            .filter(|e| e.after.action == action.id)
            .map(|e| (&e.id, &e.after))
            .collect();
        match_skeleton(
            &action.id,
            &skeleton(crate::product_contract::StatementSlice::Legacy(&action.steps), &[], &old)?,
            &skeleton(crate::product_contract::StatementSlice::Legacy(&candidate.steps), &[], &new)?,
            &mut common_statements,
        )?;
    }
    let changes = passive_changes(&$before.program, &$after.program)?;
    let mut inventory = vec![];
    for (source, identity, side) in [
        ($before, &$request.before, OwnedSourceSide::Before),
        ($after, &$request.after, OwnedSourceSide::After),
    ] {
        for action in &source.program.actions {
            writers(
                &source.program,
                identity,
                side,
                &action.id,
                &action.steps,
                &[],
                &mut action.parameters.clone(),
                &mut inventory,
            )?;
        }
    }
    Ok::<_, ContractError>(OwnedStructuralReport {
        before: $request.before.clone(),
        after: $request.after.clone(),
        root_entity: $request.root_entity.clone(),
        edits: $request.edits.clone(),
        common_statements,
        writers: inventory,
        changes,
    })

    }};
}

/// Resolve suggestions into a complete, ordered structural report. Success is
/// not evidence of partition independence, ownership, valid completed history,
/// fresh auxiliary storage, or executable/adoptable scoped behavior.
pub fn analyze_owned_sources(
    before: &CapturedProgram,
    after: &CapturedProgram,
    request: &OwnedSourceRequest,
) -> Result<OwnedStructuralReport> {
    before.validate()?;
    analyze_owned_source_tail!(before, after, request)
}

pub(super) struct StructuralTail<'s> {
    before: &'s CapturedProgram,
    after: &'s CapturedProgram,
    request: &'s OwnedSourceRequest,
    inventory: super::common_fact_inventory::InventoryTail<'s>,
}
impl SourcePreparation<super::common_fact_admission::ProofMeter> for StructuralTail<'_> {
    type Prepared = super::common_fact_admission::PreparedRetainedCapture;
    type Error = crate::product_store::StoreError;
    fn prepare(self, meter: &mut super::common_fact_admission::ProofMeter) -> super::common_fact_admission::AdmissionResult<Self::Prepared> {
        let Self { before, after, request, inventory } = self;
        let structure = analyze_owned_source_tail!(before, after, request)?;
        inventory.prepare(structure, meter)
    }
}
pub(super) fn analyze_retained_sources<'s>(
    before: &'s CapturedProgram, after: &'s CapturedProgram, request: &'s OwnedSourceRequest,
    meter: &mut super::common_fact_admission::ProofMeter,
    inventory: super::common_fact_inventory::InventoryTail<'s>,
    consume: super::common_fact_admission::CaptureTail<'s>,
) -> super::common_fact_admission::AdmissionResult<super::common_fact_admission::CapturedRetainedCapture> {
    with_ordinary_source_facts(before, meter,
        StructuralTail { before, after, request, inventory }, consume)
}
