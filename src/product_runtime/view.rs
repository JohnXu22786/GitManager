use super::*;
use eval::{boolean, list, record, reference};

fn enabled(
    app: &AppDefinition,
    state: &State,
    bindings: &[ActionBinding],
    placement: ActionPlacement,
    env: &Env,
    meter: &mut Meter,
) -> Result<BTreeSet<Id>> {
    let mut result = BTreeSet::new();
    for binding in bindings.iter().filter(|b| b.placement == placement) {
        let mut eval = Eval {
            app,
            data: &state.data,
            session: &state.session,
            day: state.day,
            meter,
        };
        if !boolean(eval.eval(&binding.enabled, env)?)? {
            continue;
        }
        let action = app
            .actions
            .iter()
            .find(|a| a.id == binding.action)
            .ok_or_else(|| failed("action absent"))?;
        let mut arguments = Env::new();
        for (id, expr) in &binding.arguments {
            let value = eval.eval(expr, env)?;
            check_references(&value, &state.data)?;
            arguments.insert(id.clone(), (action.parameters[id].clone(), value));
        }
        let mut permitted = true;
        for guard in &action.guards {
            if !boolean(eval.eval(guard, &arguments)?)? {
                permitted = false;
                break;
            }
        }
        if permitted {
            result.insert(binding.id.clone());
        }
    }
    Ok(result)
}
pub(super) fn render(
    app: &AppDefinition,
    state: &State,
    meter: &mut Meter,
) -> Result<ViewObservation> {
    let view = app
        .views
        .iter()
        .find(|v| v.id == state.session.view)
        .ok_or_else(|| invalid("session view absent"))?;
    let mut result = ViewObservation {
        view: view.id.clone(),
        rows: vec![],
        controls: Values::new(),
        selected: vec![],
        enabled_actions: enabled(
            app,
            state,
            &view.actions,
            ActionPlacement::Toolbar,
            &Env::new(),
            meter,
        )?,
        form_values: Values::new(),
    };
    let (items, columns) = match &view.kind {
        ViewKind::List {
            rows,
            columns,
            controls,
            selection,
            ..
        } => {
            for control in controls {
                let value = state
                    .session
                    .values
                    .get(&control.state)
                    .ok_or_else(|| failed("control state absent"))?;
                meter.value(value)?;
                result.controls.insert(control.id.clone(), value.clone());
            }
            if let Some(selection) = selection {
                let value = state
                    .session
                    .values
                    .get(&selection.state)
                    .ok_or_else(|| failed("selection state absent"))?;
                meter.value(value)?;
                let (_, items) = list(value.clone())?;
                let mut unique = BTreeSet::new();
                for value in items {
                    let key = reference(&value)?;
                    record(&state.data, &value)?;
                    if !unique.insert((key.entity.clone(), key.record.clone())) {
                        return Err(invalid("duplicate selection"));
                    }
                    result.selected.push(key);
                }
            }
            let items = list(
                Eval {
                    app,
                    data: &state.data,
                    session: &state.session,
                    day: state.day,
                    meter,
                }
                .eval(rows, &Env::new())?,
            )?
            .1;
            (items, columns)
        }
        ViewKind::Detail {
            record, columns, ..
        } => {
            let key = Eval {
                app,
                data: &state.data,
                session: &state.session,
                day: state.day,
                meter,
            }
            .eval(record, &Env::new())?;
            (vec![key], columns)
        }
        ViewKind::Form { defaults, .. } => {
            for value in defaults.values() {
                meter.value(value)?;
                check_references(value, &state.data)?;
            }
            result.form_values = defaults.clone();
            return Ok(result);
        }
    };
    meter.collection(items.len())?;
    let mut unique = BTreeSet::new();
    for value in items {
        meter.tick(1)?;
        record(&state.data, &value)?;
        let key = reference(&value)?;
        if !unique.insert((key.entity.clone(), key.record.clone())) {
            return Err(invalid("view produced duplicate row"));
        }
        let env = BTreeMap::from([("row".into(), (Type::reference(&key.entity), value))]);
        let mut cells = Values::new();
        for column in columns {
            let value = Eval {
                app,
                data: &state.data,
                session: &state.session,
                day: state.day,
                meter,
            }
            .eval(&column.value, &env)?;
            meter.value(&value)?;
            cells.insert(column.id.clone(), value);
        }
        let enabled_actions =
            enabled(app, state, &view.actions, ActionPlacement::Row, &env, meter)?;
        result.rows.push(PresentedRow {
            record: key,
            cells,
            enabled_actions,
        });
    }
    Ok(result)
}
pub(super) fn observe(
    app: &AppDefinition,
    state: &State,
    point: &str,
    meter: &mut Meter,
) -> Result<Observation> {
    if !valid_id(point) {
        return Err(invalid("invalid observation point"));
    }
    meter.tick(1)?;
    let mut values = Values::new();
    for observable in &app.observables {
        let value = Eval {
            app,
            data: &state.data,
            session: &state.session,
            day: state.day,
            meter,
        }
        .eval(&observable.value, &Env::new())?;
        meter.value(&value)?;
        values.insert(observable.id.clone(), value);
    }
    let view = render(app, state, meter)?;
    let observation = Observation {
        point: point.into(),
        data_digest: state.data.identity()?,
        session_digest: state.session.identity()?,
        values,
        value_types: app.observable_types()?,
        view,
        outputs: state.outputs.clone(),
    };
    observation.validate()?;
    meter.tick(1)?;
    Ok(observation)
}
