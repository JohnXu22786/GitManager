//! Bounded typed lowering of ordinary authored programs. This supplies a
//! suggestion to the store compiler, never an alternative validation authority.
use crate::product_contract::*;
use crate::product_store::{scope::*, ProjectSnapshot};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Clone)]
struct Site {
    patch: EffectPatchRequest,
    value: Expr,
    output: Option<Id>,
}
#[derive(Clone)]
pub(super) struct Analysis {
    pub patches: Vec<EffectPatchRequest>,
    pub operations: BTreeSet<Id>,
    pub entities: BTreeSet<Id>,
    pub structural: bool,
}
fn key(d: &EffectDestination) -> Result<Vec<u8>> {
    canonical_bytes(d).map_err(err)
}
fn reference_type(expr: &Expr, app: &AppDefinition, env: &BTreeMap<Id, Type>) -> Option<Type> {
    match expr {
        Expr::Variable { name } => env.get(name).cloned(),
        Expr::State { state } => app
            .state
            .iter()
            .find(|s| &s.id == state)
            .map(|s| s.value_type.clone()),
        Expr::Field { record, field } => match reference_type(record, app, env)? {
            Type::Reference { entity } => field_type(app, &entity, field).ok(),
            _ => None,
        },
        Expr::Query { entity, .. } => Some(Type::list(Type::reference(entity))),
        Expr::Filter { items, .. } => reference_type(items, app, env),
        Expr::Literal { value_type, .. } => Some(value_type.clone()),
        Expr::If {
            then_value,
            else_value,
            ..
        } => {
            let a = reference_type(then_value, app, env)?;
            (Some(&a) == reference_type(else_value, app, env).as_ref()).then_some(a)
        }
        _ => None,
    }
}
fn list_entity(expr: &Expr, app: &AppDefinition, env: &BTreeMap<Id, Type>) -> Option<Id> {
    match reference_type(expr, app, env)? {
        Type::List { item } => match *item {
            Type::Reference { entity } => Some(entity),
            _ => None,
        },
        _ => None,
    }
}
fn field_type(app: &AppDefinition, entity: &str, field: &str) -> Result<Type> {
    app.entities
        .iter()
        .find(|e| e.id == entity)
        .and_then(|e| e.fields.iter().find(|f| f.id == field))
        .map(|f| f.value_type.clone())
        .ok_or_else(|| "The changed field has no compatible declared type".into())
}
fn sites(app: &AppDefinition) -> Result<BTreeMap<Vec<u8>, Site>> {
    app.validate().map_err(err)?;
    let mut result = BTreeMap::new();
    fn insert(
        result: &mut BTreeMap<Vec<u8>, Site>,
        patch: EffectPatchRequest,
        value: &Expr,
        output: Option<Id>,
    ) -> Result<()> {
        if result.len() >= MAX_ITEMS * MAX_ITEMS {
            return Err("The design exceeds the bounded change adapter".into());
        }
        if result
            .insert(
                key(&patch.destination)?,
                Site {
                    patch,
                    value: value.clone(),
                    output,
                },
            )
            .is_some()
        {
            return Err("Ambiguous effect destination".into());
        }
        Ok(())
    }
    fn steps(
        app: &AppDefinition,
        action: &Id,
        body: &[Statement],
        prefix: &[usize],
        mut env: BTreeMap<Id, Type>,
        out: &mut BTreeMap<Vec<u8>, Site>,
    ) -> Result<()> {
        for (i, step) in body.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(i);
            match step {
                Statement::Update {
                    record: Expr::Variable { name },
                    values,
                } => {
                    if let Some(Type::Reference { entity }) = env.get(name) {
                        for (field, value) in values {
                            insert(
                                out,
                                EffectPatchRequest {
                                    destination: EffectDestination::Update {
                                        action: action.clone(),
                                        path: path.clone(),
                                        field: field.clone(),
                                    },
                                    entity: entity.clone(),
                                    subject: name.clone(),
                                    value_type: field_type(app, entity, field)?,
                                },
                                value,
                                None,
                            )?;
                        }
                    }
                }
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    for (field, value) in values {
                        insert(
                            out,
                            EffectPatchRequest {
                                destination: EffectDestination::CreateValue {
                                    action: action.clone(),
                                    path: path.clone(),
                                    field: field.clone(),
                                },
                                entity: entity.clone(),
                                subject: bind.clone(),
                                value_type: field_type(app, entity, field)?,
                            },
                            value,
                            None,
                        )?;
                    }
                    env.insert(bind.clone(), Type::reference(entity));
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps: inner,
                } => {
                    if let Some(entity) = list_entity(items, app, &env) {
                        let mut nested = env.clone();
                        nested.insert(binding.clone(), Type::reference(entity));
                        steps(app, action, inner, &path, nested, out)?;
                    }
                }
                Statement::Emit {
                    output,
                    items,
                    binding,
                    columns,
                } => {
                    if let Some(entity) = list_entity(items, app, &env) {
                        insert(
                            out,
                            EffectPatchRequest {
                                destination: EffectDestination::EmitItems {
                                    action: action.clone(),
                                    path: path.clone(),
                                },
                                entity: entity.clone(),
                                subject: binding.clone(),
                                value_type: Type::list(Type::reference(&entity)),
                            },
                            items,
                            Some(output.clone()),
                        )?;
                        let definition = app
                            .outputs
                            .iter()
                            .find(|o| &o.id == output)
                            .ok_or("Missing output definition")?;
                        for (column, value) in columns {
                            let typ = definition
                                .columns
                                .iter()
                                .find(|c| &c.id == column)
                                .ok_or("Missing output column")?
                                .value_type
                                .clone();
                            insert(
                                out,
                                EffectPatchRequest {
                                    destination: EffectDestination::EmitColumn {
                                        action: action.clone(),
                                        path: path.clone(),
                                        column: column.clone(),
                                    },
                                    entity: entity.clone(),
                                    subject: binding.clone(),
                                    value_type: typ,
                                },
                                value,
                                Some(output.clone()),
                            )?;
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
    for action in &app.actions {
        steps(
            app,
            &action.id,
            &action.steps,
            &[],
            action.parameters.clone(),
            &mut result,
        )?;
    }
    for view in &app.views {
        let columns = match &view.kind {
            ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => columns,
            _ => continue,
        };
        let schema = app
            .view_schema(&view.id)
            .map_err(err)?
            .ok_or("Missing row schema")?;
        for column in columns {
            insert(
                &mut result,
                EffectPatchRequest {
                    destination: EffectDestination::ViewColumn {
                        view: view.id.clone(),
                        column: column.id.clone(),
                    },
                    entity: schema.entity.clone(),
                    subject: "row".into(),
                    value_type: schema.columns[&column.id].clone(),
                },
                &column.value,
                None,
            )?;
        }
    }
    let types = app.observable_types().map_err(err)?;
    for observable in &app.observables {
        insert(
            &mut result,
            EffectPatchRequest {
                destination: EffectDestination::Observable {
                    observable: observable.id.clone(),
                },
                entity: "global".into(),
                subject: "global".into(),
                value_type: types[&observable.id].clone(),
            },
            &observable.value,
            None,
        )?;
    }
    Ok(result)
}
fn body_slot<'a>(body: &'a mut [Statement], path: &[usize]) -> Result<&'a mut Statement> {
    let (index, tail) = path.split_first().ok_or("Empty change path")?;
    let step = body.get_mut(*index).ok_or("Changed statement is absent")?;
    if tail.is_empty() {
        return Ok(step);
    }
    if let Statement::ForEach { steps, .. } = step {
        body_slot(steps, tail)
    } else {
        Err("Changed loop is absent".into())
    }
}
fn replace(app: &mut AppDefinition, destination: &EffectDestination, value: Expr) -> Result<()> {
    match destination {
        EffectDestination::ViewColumn { view, column } => {
            let v = app
                .views
                .iter_mut()
                .find(|v| &v.id == view)
                .ok_or("Changed view is absent")?;
            let columns = match &mut v.kind {
                ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => columns,
                _ => return Err("Changed view is not a row view".into()),
            };
            columns
                .iter_mut()
                .find(|c| &c.id == column)
                .ok_or("Changed column is absent")?
                .value = value;
        }
        EffectDestination::Observable { observable } => {
            app.observables
                .iter_mut()
                .find(|o| &o.id == observable)
                .ok_or("Changed result is absent")?
                .value = value
        }
        destination => {
            let (action, path) = match destination {
                EffectDestination::Update { action, path, .. }
                | EffectDestination::CreateValue { action, path, .. }
                | EffectDestination::EmitColumn { action, path, .. }
                | EffectDestination::EmitItems { action, path } => (action, path),
                _ => unreachable!(),
            };
            let action = app
                .actions
                .iter_mut()
                .find(|a| &a.id == action)
                .ok_or("Changed action is absent")?;
            match (body_slot(&mut action.steps, path)?, destination) {
                (Statement::Update { values, .. }, EffectDestination::Update { field, .. })
                | (
                    Statement::Create { values, .. },
                    EffectDestination::CreateValue { field, .. },
                ) => {
                    *values.get_mut(field).ok_or("Changed value is absent")? = value;
                }
                (Statement::Emit { columns, .. }, EffectDestination::EmitColumn { column, .. }) => {
                    *columns.get_mut(column).ok_or("Changed output is absent")? = value;
                }
                (Statement::Emit { items, .. }, EffectDestination::EmitItems { .. }) => {
                    *items = value
                }
                _ => return Err("The action design changed".into()),
            }
        }
    }
    Ok(())
}
fn labels(app: &mut AppDefinition) {
    app.label.clear();
    for entity in &mut app.entities {
        entity.label.clear();
        for field in &mut entity.fields {
            field.label.clear();
        }
    }
    for state in &mut app.state {
        state.label.clear();
    }
    for action in &mut app.actions {
        action.label.clear();
    }
    for output in &mut app.outputs {
        output.label.clear();
        for column in &mut output.columns {
            column.label.clear();
        }
    }
    for observable in &mut app.observables {
        observable.label.clear();
    }
    for view in &mut app.views {
        view.label.clear();
        for action in &mut view.actions {
            action.label.clear();
        }
        match &mut view.kind {
            ViewKind::List {
                columns, controls, ..
            } => {
                for column in columns {
                    column.label.clear();
                }
                for control in controls {
                    control.label.clear();
                }
            }
            ViewKind::Detail { columns, .. } => {
                for column in columns {
                    column.label.clear();
                }
            }
            ViewKind::Form { fields, .. } => {
                for field in fields {
                    field.label.clear();
                }
            }
        }
    }
}
pub(super) fn baseline(snapshot: &ProjectSnapshot) -> Result<CapturedProgram> {
    Ok(snapshot
        .editable_scope_context()
        .map_err(err)?
        .map(|e| e.editable)
        .unwrap_or(snapshot.program().map_err(err)?.clone()))
}
pub(super) fn analyze(snapshot: &ProjectSnapshot, candidate: &CapturedProgram) -> Result<Analysis> {
    candidate.validate().map_err(err)?;
    if candidate.binding.project_id != snapshot.data.project_id {
        return Err("The returned design belongs to another tool".into());
    }
    let base = baseline(snapshot)?;
    let before = sites(&base.program)?;
    let after = sites(&candidate.program)?;
    let mut masked = base.program.clone();
    let mut chosen = BTreeSet::new();
    for (k, new) in &after {
        if let Some(old) = before.get(k) {
            if old.patch == new.patch && old.value != new.value {
                replace(&mut masked, &new.patch.destination, new.value.clone())?;
                chosen.insert(k.clone());
            }
        }
    }
    let mut target = candidate.program.clone();
    labels(&mut masked);
    labels(&mut target);
    let structural = masked != target;
    // Coverage closure preserves every other writer of a newly protected field
    // and every emission site of a protected output column.
    let changed: Vec<_> = chosen.iter().map(|k| &after[k]).collect();
    for (k, site) in &after {
        if changed
            .iter()
            .any(|p| match (&p.patch.destination, &site.patch.destination) {
                (
                    EffectDestination::Update { field: a, .. }
                    | EffectDestination::CreateValue { field: a, .. },
                    EffectDestination::Update { field: b, .. },
                ) => p.patch.entity == site.patch.entity && a == b,
                (
                    EffectDestination::EmitColumn { column: a, .. },
                    EffectDestination::EmitColumn { column: b, .. },
                ) => p.output == site.output && a == b && p.patch.entity == site.patch.entity,
                _ => false,
            })
        {
            chosen.insert(k.clone());
        }
    }
    let patches: Vec<_> = chosen
        .into_iter()
        .map(|k| after[&k].patch.clone())
        .collect();
    let operations = patches
        .iter()
        .filter_map(|p| match &p.destination {
            EffectDestination::Update { action, .. }
            | EffectDestination::CreateValue { action, .. }
            | EffectDestination::EmitColumn { action, .. }
            | EffectDestination::EmitItems { action, .. } => Some(action.clone()),
            _ => None,
        })
        .collect();
    let entities = patches
        .iter()
        .filter(|p| {
            !matches!(
                p.destination,
                EffectDestination::Observable { .. } | EffectDestination::EmitItems { .. }
            )
        })
        .map(|p| p.entity.clone())
        .collect();
    Ok(Analysis {
        patches,
        operations,
        entities,
        structural,
    })
}
pub(super) fn retained_lifecycles(snapshot: &ProjectSnapshot) -> Result<Vec<LifecycleBinding>> {
    let mut result: BTreeMap<Id, LifecycleBinding> = BTreeMap::new();
    for layer in snapshot.scope.layers.values() {
        for lifecycle in &layer.request.lifecycles {
            let mut lifecycle = lifecycle.clone();
            lifecycle.source = snapshot.active_revision.clone();
            if result
                .get(&lifecycle.entity)
                .is_some_and(|old| old.completed != lifecycle.completed)
            {
                return Err(
                    "Saved finished-work meanings disagree; repair the design before changing it"
                        .into(),
                );
            }
            result.insert(lifecycle.entity.clone(), lifecycle);
        }
    }
    Ok(result.into_values().collect())
}
pub(super) fn request(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    population: ScopePopulation,
    explicit: Vec<LifecycleBinding>,
    experienced: BTreeSet<Id>,
) -> Result<ScopeRequest> {
    let analysis = analyze(snapshot, candidate)?;
    if analysis.structural {
        return Err("This changes the tool design and needs a whole-design rehearsal".into());
    }
    if analysis.patches.is_empty() {
        return Err("This changes no scoped business effect".into());
    }
    let mut lifecycles = retained_lifecycles(snapshot)?;
    for lifecycle in explicit {
        if lifecycle.source != snapshot.active_revision {
            return Err("The finished-work answer belongs to an earlier tool".into());
        }
        if let Some(prior) = lifecycles.iter().find(|p| p.entity == lifecycle.entity) {
            if prior.completed != lifecycle.completed {
                return Err("The established finished-work meaning must stay exact".into());
            }
        } else {
            lifecycles.push(lifecycle)
        }
    }
    lifecycles.retain(|l| analysis.entities.contains(&l.entity));
    if analysis
        .entities
        .iter()
        .any(|e| !lifecycles.iter().any(|l| &l.entity == e))
    {
        return Err("Show when this work should count as finished, or explicitly say it has no finished state".into());
    }
    let operations = analysis.operations.union(&experienced).cloned().collect();
    Ok(ScopeRequest {
        population,
        operations,
        excluded_records: vec![],
        lifecycles,
        patches: analysis.patches,
    })
}
/// Identity destinations are accepted only when the actual typed slot remains
/// unique, with its kind, entity, subject and value type preserved.
pub(super) fn slot_mappings(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
) -> Result<Vec<ScopeSlotMapping>> {
    let Some(editable) = snapshot.editable_scope_context().map_err(err)? else {
        return Ok(vec![]);
    };
    let candidate_sites = sites(&candidate.program)?;
    let to_source = canonical_digest(IdentityDomain::Source, candidate).map_err(err)?;
    editable.slots.into_iter().map(|slot|{
        let site=candidate_sites.get(&key(&slot.destination)?).ok_or("The new design moved a protected result; a verified result mapping is needed")?;
        if site.patch.entity!=slot.entity||site.patch.subject!=slot.subject||site.patch.value_type!=slot.value_type{return Err("The new design changed a protected result's meaning; it needs a compatible design".into())}
        Ok(ScopeSlotMapping{layer:slot.layer,patch:slot.patch,from_source:editable.compiled_source.clone(),from:slot.destination,to_source:to_source.clone(),to:site.patch.destination.clone(),subject:site.patch.subject.clone()})
    }).collect()
}
