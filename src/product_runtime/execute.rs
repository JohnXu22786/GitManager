use super::*;
use eval::{boolean, list, record, reference, value_ref};

fn evaluate(
    app: &AppDefinition,
    state: &State,
    expr: &Expr,
    env: &Env,
    meter: &mut Meter,
) -> Result<DataValue> {
    Eval {
        app,
        data: &state.data,
        session: &state.session,
        day: state.day,
        meter,
    }
    .eval(expr, env)
}
fn assignments(
    app: &AppDefinition,
    state: &State,
    values: &BTreeMap<Id, Expr>,
    env: &Env,
    meter: &mut Meter,
) -> Result<Values> {
    values
        .iter()
        .map(|(id, expr)| Ok((id.clone(), evaluate(app, state, expr, env, meter)?)))
        .collect()
}
fn update(
    state: &mut State,
    key: &DataValue,
    values: Values,
    archive: bool,
    meter: &mut Meter,
) -> Result<()> {
    meter.write()?;
    meter.tick(state.data.records.len() as u64)?;
    let target = reference(key)?;
    let row = state
        .data
        .records
        .iter_mut()
        .find(|r| r.entity == target.entity && r.id == target.record)
        .ok_or_else(|| invalid("record does not exist"))?;
    if row.archived {
        return Err(invalid("archived historical records cannot be modified"));
    }
    row.values.extend(values);
    row.archived = archive;
    row.revision = row
        .revision
        .checked_add(1)
        .ok_or_else(|| failed("record revision overflow"))?;
    Ok(())
}
struct Transaction<'a> {
    program: &'a CapturedProgram,
    operation: &'a str,
    seed: u64,
    created: usize,
}
impl Transaction<'_> {
    fn statements(
        &mut self,
        state: &mut State,
        steps: &[Statement],
        env: &mut Env,
        meter: &mut Meter,
    ) -> Result<()> {
        let app = &self.program.program;
        for step in steps {
            meter.step()?;
            match step {
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    let values = assignments(app, state, values, env, meter)?;
                    meter.write()?;
                    meter.collection(state.data.records.len() + 1)?;
                    self.created += 1;
                    let digest = canonical_digest(
                        IdentityDomain::Input,
                        &(self.seed, self.operation, entity, self.created),
                    )?;
                    let id = format!("record-{}", &digest.as_str()[..32]);
                    if state
                        .data
                        .records
                        .iter()
                        .any(|r| r.entity == *entity && r.id == id)
                    {
                        return Err(invalid("generated record identity collision"));
                    }
                    let row = Record {
                        entity: entity.clone(),
                        id,
                        revision: 1,
                        created_program: self.program.artifact.program_digest.clone(),
                        archived: false,
                        values,
                    };
                    env.insert(bind.clone(), (Type::reference(entity), value_ref(&row)));
                    state.data.records.push(row);
                }
                Statement::Update { record, values } => {
                    let key = evaluate(app, state, record, env, meter)?;
                    let values = assignments(app, state, values, env, meter)?;
                    update(state, &key, values, false, meter)?;
                }
                Statement::Archive { record } => {
                    let key = evaluate(app, state, record, env, meter)?;
                    update(state, &key, Values::new(), true, meter)?;
                }
                Statement::SetState { state: id, value } => {
                    let value = evaluate(app, state, value, env, meter)?;
                    meter.write()?;
                    state.session.values.insert(id.clone(), value);
                }
                Statement::Collection {
                    target,
                    operation,
                    items,
                } => {
                    let (_, items) = list(evaluate(app, state, items, env, meter)?)?;
                    let (key, field, old) = match target {
                        CollectionTarget::State { state: id } => (
                            None,
                            id.clone(),
                            state
                                .session
                                .values
                                .get(id)
                                .cloned()
                                .ok_or_else(|| failed("state absent"))?,
                        ),
                        CollectionTarget::Field {
                            record: expr,
                            field,
                        } => {
                            let key = evaluate(app, state, expr, env, meter)?;
                            let value = record(&state.data, &key)?
                                .values
                                .get(field)
                                .cloned()
                                .ok_or_else(|| invalid("collection field absent"))?;
                            (Some(key), field.clone(), value)
                        }
                    };
                    let (item_type, mut previous) = list(old)?;
                    match operation {
                        CollectionOperation::Insert => {
                            for item in items {
                                meter.tick(previous.len() as u64 + 1)?;
                                if !previous.contains(&item) {
                                    meter.collection(previous.len() + 1)?;
                                    previous.push(item);
                                }
                            }
                        }
                        CollectionOperation::Remove => {
                            let mut retained = Vec::new();
                            for item in previous {
                                meter.tick(items.len() as u64 + 1)?;
                                if !items.contains(&item) {
                                    retained.push(item);
                                }
                            }
                            previous = retained;
                        }
                    }
                    let value = DataValue::List {
                        item_type,
                        items: previous,
                    };
                    if let Some(key) = key {
                        update(state, &key, BTreeMap::from([(field, value)]), false, meter)?;
                    } else {
                        meter.write()?;
                        state.session.values.insert(field, value);
                    }
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let (typ, items) = list(evaluate(app, state, items, env, meter)?)?;
                    for item in items {
                        meter.tick(1)?;
                        let mut local = env.clone();
                        local.insert(binding.clone(), (typ.clone(), item));
                        self.statements(state, steps, &mut local, meter)?;
                    }
                }
                Statement::Assert { condition, message } => {
                    if !boolean(evaluate(app, state, condition, env, meter)?)? {
                        return Err(invalid(message));
                    }
                }
                Statement::Emit {
                    output,
                    items,
                    binding,
                    columns,
                } => {
                    let definition = app
                        .outputs
                        .iter()
                        .find(|o| &o.id == output)
                        .ok_or_else(|| failed("output absent"))?;
                    let (typ, items) = list(evaluate(app, state, items, env, meter)?)?;
                    let mut rows = Vec::new();
                    let header = LocalArtifact::from_rows(
                        output,
                        definition.format,
                        definition.columns.clone(),
                        vec![],
                    )?;
                    let prior_bytes = state.outputs.iter().map(|a| a.bytes.len()).sum::<usize>();
                    let mut encoded_bytes = header.bytes.len();
                    if prior_bytes.saturating_add(encoded_bytes) > meter.limits.output_bytes {
                        return Err(exhausted("cumulative output limit"));
                    }
                    for item in items {
                        meter.tick(1)?;
                        let mut local = env.clone();
                        local.insert(binding.clone(), (typ.clone(), item));
                        let row = assignments(app, state, columns, &local, meter)?;
                        for value in row.values() {
                            meter.value(value)?;
                        }
                        // Measure one bounded row through the authoritative
                        // encoder before accumulating it. This prevents a wide
                        // repeated projection allocating an oversized artifact.
                        let single = LocalArtifact::from_rows(
                            output,
                            definition.format,
                            definition.columns.clone(),
                            vec![row.clone()],
                        )
                        .map_err(|_| exhausted("output row exceeds encoder limits"))?;
                        meter.tick(single.bytes.len() as u64)?;
                        encoded_bytes =
                            encoded_bytes.saturating_add(single.bytes.len() - header.bytes.len());
                        if definition.format == OutputFormat::Json && !rows.is_empty() {
                            encoded_bytes = encoded_bytes.saturating_add(1);
                        }
                        if prior_bytes.saturating_add(encoded_bytes) > meter.limits.output_bytes {
                            return Err(exhausted("cumulative output limit"));
                        }
                        rows.push(row);
                    }
                    let artifact = LocalArtifact::from_rows(
                        output,
                        definition.format,
                        definition.columns.clone(),
                        rows,
                    )?;
                    meter.tick(artifact.bytes.len() as u64)?;
                    let total = state
                        .outputs
                        .iter()
                        .map(|a| a.bytes.len())
                        .sum::<usize>()
                        .saturating_add(artifact.bytes.len());
                    if total > meter.limits.output_bytes || state.outputs.len() >= MAX_ITEMS {
                        return Err(exhausted("cumulative output limit"));
                    }
                    state.outputs.push(artifact);
                }
            }
        }
        Ok(())
    }
}
pub(super) fn constraints(
    program: &CapturedProgram,
    state: &State,
    meter: &mut Meter,
) -> Result<()> {
    let app = &program.program;
    meter.collection(state.data.records.len())?;
    for row in &state.data.records {
        meter.tick(1)?;
        for value in row.values.values() {
            meter.value(value)?;
        }
        if row.archived {
            continue;
        }
        let Some(entity) = app.entities.iter().find(|e| e.id == row.entity) else {
            continue;
        };
        let env = BTreeMap::from([(
            "record".into(),
            (Type::reference(&row.entity), value_ref(row)),
        )]);
        for constraint in &entity.constraints {
            if !boolean(evaluate(app, state, constraint, &env, meter)?)? {
                return Err(invalid(&format!("Entity constraint failed: {}", entity.id)));
            }
        }
    }
    state.data.validate()?;
    state.session.validate(app)?;
    for value in state.session.values.values() {
        meter.value(value)?;
        check_references(value, &state.data)?;
    }
    meter.tick(1)?;
    Ok(())
}
fn action(
    program: &CapturedProgram,
    state: &mut State,
    id: &str,
    arguments: &Values,
    operation: &str,
    seed: u64,
    meter: &mut Meter,
) -> Result<()> {
    let app = &program.program;
    let definition = app
        .actions
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| invalid("unknown action"))?;
    validate_input(
        &SemanticInput::Invoke {
            action: id.into(),
            arguments: arguments.clone(),
        },
        app,
    )?;
    let mut env = Env::new();
    for (id, value) in arguments {
        meter.value(value)?;
        check_references(value, &state.data)?;
        env.insert(
            id.clone(),
            (definition.parameters[id].clone(), value.clone()),
        );
    }
    for guard in &definition.guards {
        if !boolean(evaluate(app, state, guard, &env, meter)?)? {
            return Err(invalid(&format!("Action guard rejected: {id}")));
        }
    }
    let previous = state.data.clone();
    let output_start = state.outputs.len();
    state.data = merged_data(program, &state.data)?;
    Transaction {
        program,
        operation,
        seed,
        created: 0,
    }
    .statements(state, &definition.steps, &mut env, meter)?;
    for ensure in &definition.ensures {
        if !boolean(evaluate(app, state, ensure, &env, meter)?)? {
            return Err(invalid(&format!("Action postcondition rejected: {id}")));
        }
    }
    state.data.generation = state
        .data
        .generation
        .checked_add(1)
        .ok_or_else(|| failed("data generation overflow"))?;
    let mut changes = Vec::new();
    for row in &state.data.records {
        meter.tick(previous.records.len() as u64 + 1)?;
        let before = previous
            .records
            .iter()
            .find(|r| r.entity == row.entity && r.id == row.id);
        if before != Some(row) {
            changes.push(RecordChange {
                entity: row.entity.clone(),
                record: row.id.clone(),
                before: before.map(|r| r.values.clone()),
                after: row.values.clone(),
                archived: row.archived,
            });
        }
    }
    let event_id = canonical_digest(
        IdentityDomain::Input,
        &(operation, &program.artifact.program_digest),
    )?;
    state.data.events.push(BusinessEvent {
        id: format!("event-{}", &event_id.as_str()[..32]),
        sequence: state.data.generation,
        operation_id: operation.into(),
        action: id.into(),
        day: state.day,
        program: program.artifact.program_digest.clone(),
        changes,
        outputs: state.outputs[output_start..]
            .iter()
            .map(|a| a.digest.clone())
            .collect(),
    });
    constraints(program, state, meter)
}
pub(super) fn input(
    program: &CapturedProgram,
    state: &mut State,
    input: &SemanticInput,
    operation: &str,
    seed: u64,
    meter: &mut Meter,
) -> Result<()> {
    let app = &program.program;
    match input {
        SemanticInput::Invoke {
            action: id,
            arguments,
        } => action(program, state, id, arguments, operation, seed, meter),
        SemanticInput::Navigate { view } => {
            state.session.view = view.clone();
            state.session.focused_record = None;
            Ok(())
        }
        SemanticInput::AdvanceClock { days } => {
            let next = i64::from(state.day) + i64::from(*days);
            let next = i32::try_from(next).map_err(|_| failed("date overflow"))?;
            validate_day(next)?;
            state.day = next;
            // A saved clock must satisfy the same constraints as reopening it.
            // The caller discards this entire tentative state on rejection.
            constraints(program, state, meter)
        }
        SemanticInput::Observe { .. } => Ok(()),
        SemanticInput::Submit { view, arguments } => {
            if &state.session.view != view {
                return Err(invalid("form is not the active view"));
            }
            let view = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let ViewKind::Form { action: id, .. } = &view.kind else {
                return Err(invalid("not a form"));
            };
            action(program, state, id, arguments, operation, seed, meter)
        }
        SemanticInput::Control {
            view,
            control,
            value,
        } => {
            if &state.session.view != view {
                return Err(invalid("control is not in the active view"));
            }
            meter.value(value)?;
            check_references(value, &state.data)?;
            let definition = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let ViewKind::List {
                controls,
                selection,
                ..
            } = &definition.kind
            else {
                return Err(invalid("not a list control"));
            };
            let (id, on_change) = if let Some(control) = controls.iter().find(|c| &c.id == control)
            {
                (&control.state, &control.on_change)
            } else {
                let selection = selection
                    .as_ref()
                    .filter(|s| &s.id == control)
                    .ok_or_else(|| invalid("unknown control"))?;
                let (_, items) = list(value.clone())?;
                let mut unique = BTreeSet::new();
                for item in items {
                    if !unique.insert(canonical_bytes(&item)?) {
                        return Err(invalid("duplicate selected record"));
                    }
                }
                (&selection.state, &selection.on_change)
            };
            meter.write()?;
            state.session.values.insert(id.clone(), value.clone());
            if let Some(id) = on_change {
                action(
                    program,
                    state,
                    id,
                    &BTreeMap::from([("value".into(), value.clone())]),
                    operation,
                    seed,
                    meter,
                )?;
            }
            state.session.validate(app)?;
            Ok(())
        }
        SemanticInput::Activate { view, binding, row } => {
            if &state.session.view != view {
                return Err(invalid("binding is not in the active view"));
            }
            let presented = super::view::render(app, state, meter)?;
            let definition = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let binding = definition
                .actions
                .iter()
                .find(|b| &b.id == binding)
                .ok_or_else(|| invalid("unknown binding"))?;
            let mut env = Env::new();
            if let Some(row) = row {
                let presented = presented
                    .rows
                    .iter()
                    .find(|r| &r.record == row)
                    .ok_or_else(|| invalid("row is not visible"))?;
                if !presented.enabled_actions.contains(&binding.id) {
                    return Err(invalid("row action is disabled"));
                }
                env.insert(
                    "row".into(),
                    (
                        Type::reference(&row.entity),
                        DataValue::Reference {
                            entity: row.entity.clone(),
                            record: row.record.clone(),
                        },
                    ),
                );
            } else if !presented.enabled_actions.contains(&binding.id) {
                return Err(invalid("action is disabled"));
            }
            let arguments = assignments(app, state, &binding.arguments, &env, meter)?;
            action(
                program,
                state,
                &binding.action,
                &arguments,
                operation,
                seed,
                meter,
            )
        }
    }
}
