//! Exact source correspondence only. This module never evaluates expressions,
//! lowers effects, initializes protected data, or constructs preparation handles.
use super::owned_contract::*;
use crate::product_contract::*;
use serde::Serialize;
use serde_json::Value;
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
fn json<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| error(e.to_string()))
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
fn contains_variable(value: &Value, name: &str) -> bool {
    match value {
        Value::Object(fields) => {
            (fields.get("kind").and_then(Value::as_str) == Some("variable")
                && fields.get("name").and_then(Value::as_str) == Some(name))
                || fields.values().any(|v| contains_variable(v, name))
        }
        Value::Array(values) => values.iter().any(|v| contains_variable(v, name)),
        _ => false,
    }
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
    let tail = json(&steps[end..])?;
    let ensures = if endpoint.list_path.is_empty() {
        json(&action.ensures)?
    } else {
        Value::Null
    };
    for statement in span {
        if let Statement::Create { bind, .. } = statement {
            require(
                !contains_variable(&tail, bind) && !contains_variable(&ensures, bind),
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

#[derive(Debug)]
enum Skeleton {
    Slot(Id),
    Common {
        path: Vec<usize>,
        kind: OwnedStatementKind,
        header: Value,
        body: Vec<Skeleton>,
    },
}
fn skeleton(
    steps: &[Statement],
    path: &[usize],
    endpoints: &[(&Id, &OwnedSpanEndpoint)],
) -> Result<Vec<Skeleton>> {
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
        let statement = &steps[index];
        let mut current = path.to_vec();
        current.push(index);
        let mut header = json(statement)?;
        let body = if let Statement::ForEach { steps, .. } = statement {
            header
                .as_object_mut()
                .expect("typed statement object")
                .remove("steps");
            skeleton(steps, &current, endpoints)?
        } else {
            vec![]
        };
        result.push(Skeleton::Common {
            path: current,
            kind: OwnedStatementKind::of(statement),
            header,
            body,
        });
        index += 1;
    }
    Ok(result)
}
fn match_skeleton(
    action: &str,
    before: &[Skeleton],
    after: &[Skeleton],
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
fn columns(view: &ViewDefinition) -> Option<&Vec<Column>> {
    match &view.kind {
        ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => Some(columns),
        _ => None,
    }
}
fn replace_columns(view: &mut ViewDefinition, replacement: Vec<Column>) {
    match &mut view.kind {
        ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => *columns = replacement,
        _ => {}
    }
}
fn change<T: Serialize>(
    kind: OwnedStructuralChangeKind,
    container: Option<Id>,
    id: Id,
    before: Option<&T>,
    after: &T,
    before_type: Option<Type>,
    after_type: Option<Type>,
) -> Result<OwnedStructuralChange> {
    Ok(OwnedStructuralChange {
        kind,
        container,
        id,
        before: before.map(digest).transpose()?,
        after: digest(after)?,
        before_type,
        after_type,
    })
}
fn passive_changes(
    before: &AppDefinition,
    after: &AppDefinition,
) -> Result<Vec<OwnedStructuralChange>> {
    let mut shell = after.clone();
    let mut baseline = before.clone();
    let mut changes = vec![];
    if before.label != after.label {
        changes.push(change(
            OwnedStructuralChangeKind::ApplicationLabelChanged,
            None,
            before.id.clone(),
            Some(&before.label),
            &after.label,
            None,
            None,
        )?);
        shell.label = before.label.clone();
    }
    for entity in &after.entities {
        if !before.entities.iter().any(|e| e.id == entity.id) {
            changes.push(change(
                OwnedStructuralChangeKind::EntityAdded,
                None,
                entity.id.clone(),
                None,
                entity,
                None,
                None,
            )?);
        }
    }
    shell
        .entities
        .retain(|entity| before.entities.iter().any(|e| e.id == entity.id));
    for view in &after.views {
        let Some(previous) = before.views.iter().find(|v| v.id == view.id) else {
            continue;
        };
        let (Some(old), Some(new)) = (columns(previous), columns(view)) else {
            continue;
        };
        let old_types = before
            .view_schema(&previous.id)?
            .expect("column view has a schema")
            .columns;
        let new_types = after
            .view_schema(&view.id)?
            .expect("column view has a schema")
            .columns;
        let mut retained = vec![];
        for column in new {
            let prior = old.iter().find(|c| c.id == column.id);
            if prior != Some(column) {
                changes.push(change(
                    if prior.is_some() {
                        OwnedStructuralChangeKind::ColumnChanged
                    } else {
                        OwnedStructuralChangeKind::ColumnAdded
                    },
                    Some(view.id.clone()),
                    column.id.clone(),
                    prior,
                    column,
                    prior.and_then(|c| old_types.get(&c.id)).cloned(),
                    new_types.get(&column.id).cloned(),
                )?);
            }
            if let Some(prior) = prior {
                retained.push(prior.clone());
            }
        }
        replace_columns(
            shell
                .views
                .iter_mut()
                .find(|v| v.id == view.id)
                .expect("existing view"),
            retained,
        );
    }
    let old_types = before.observable_types()?;
    let new_types = after.observable_types()?;
    let mut retained = vec![];
    for observable in &after.observables {
        let prior = before.observables.iter().find(|o| o.id == observable.id);
        if prior != Some(observable) {
            changes.push(change(
                if prior.is_some() {
                    OwnedStructuralChangeKind::ObservableChanged
                } else {
                    OwnedStructuralChangeKind::ObservableAdded
                },
                None,
                observable.id.clone(),
                prior,
                observable,
                prior.and_then(|o| old_types.get(&o.id)).cloned(),
                new_types.get(&observable.id).cloned(),
            )?);
        }
        if let Some(prior) = prior {
            retained.push(prior.clone());
        }
    }
    shell.observables = retained;
    for action in &mut shell.actions {
        action.steps.clear();
    }
    for action in &mut baseline.actions {
        action.steps.clear();
    }
    require(
        shell == baseline,
        "unclassified schema, action header, view, output, state, or ordering change",
    )?;
    Ok(changes)
}
fn record_entity(app: &AppDefinition, expression: &Expr, env: &Environment) -> Result<Id> {
    match app.expression_type_in(expression, env)? {
        Type::Reference { entity } => Ok(entity),
        _ => Err(error("writer target is not a typed record")),
    }
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
    for (index, statement) in steps.iter().enumerate() {
        let mut current = path.to_vec();
        current.push(index);
        let (entity, fields) = match statement {
            // Creation also writes omitted optional fields as null.
            Statement::Create { entity, .. } => (
                Some(entity.clone()),
                app.entities
                    .iter()
                    .find(|e| &e.id == entity)
                    .ok_or_else(|| error("created entity missing"))?
                    .fields
                    .iter()
                    .map(|field| field.id.clone())
                    .collect::<Vec<_>>(),
            ),
            Statement::Update { record, values } => (
                Some(record_entity(app, record, env)?),
                values.keys().cloned().collect(),
            ),
            Statement::Archive { record } => (Some(record_entity(app, record, env)?), vec![]),
            Statement::Collection {
                target: CollectionTarget::Field { record, field },
                ..
            } => (Some(record_entity(app, record, env)?), vec![field.clone()]),
            _ => (None, vec![]),
        };
        if let Some(entity) = entity {
            let definition = app
                .entities
                .iter()
                .find(|e| e.id == entity)
                .ok_or_else(|| error("writer entity missing"))?;
            let targets: Vec<Option<Id>> = if matches!(statement, Statement::Archive { .. }) {
                vec![None]
            } else {
                fields.into_iter().map(Some).collect()
            };
            for field in targets {
                let value_type = field
                    .as_ref()
                    .map(|id| {
                        definition
                            .fields
                            .iter()
                            .find(|f| &f.id == id)
                            .map(|f| f.value_type.clone())
                            .ok_or_else(|| error("writer field missing"))
                    })
                    .transpose()?;
                result.push(OwnedWriter {
                    side,
                    source: source.clone(),
                    action: action.into(),
                    path: current.clone(),
                    kind: OwnedStatementKind::of(statement),
                    entity: entity.clone(),
                    field,
                    value_type,
                });
            }
        }
        match statement {
            Statement::Create { entity, bind, .. } => {
                env.insert(bind.clone(), Type::reference(entity));
            }
            Statement::ForEach {
                items,
                binding,
                steps,
            } => {
                let mut inner = env.clone();
                inner.insert(binding.clone(), list_item(app, items, env)?);
                writers(
                    app, source, side, action, steps, &current, &mut inner, result,
                )?;
            }
            _ => {}
        }
    }
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
            &skeleton(&action.steps, &[], &old)?,
            &skeleton(&candidate.steps, &[], &new)?,
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
