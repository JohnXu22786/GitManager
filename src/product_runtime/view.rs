use super::*;
use eval::{boolean, list, record, reference};
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_alloc::ordered::{OrderedMap, OrderedSet};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::fmt;

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
        let mut eval = Eval::legacy(app, &state.data, &state.session, state.day, meter);
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
                Eval::legacy(app, &state.data, &state.session, state.day, meter)
                .eval(rows, &Env::new())?,
            )?
            .1;
            (items, columns)
        }
        ViewKind::Detail {
            record, columns, ..
        } => {
            let key = Eval::legacy(app, &state.data, &state.session, state.day, meter)
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
            let value = Eval::legacy(app, &state.data, &state.session, state.day, meter)
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
    program: &ProgramSource,
    state: &State,
    point: &str,
    meter: &mut Meter,
) -> Result<Observation> {
    let app = &program.program;
    if !valid_id(point) {
        return Err(invalid("invalid observation point"));
    }
    meter.tick(1)?;
    let mut values = Values::new();
    for observable in &app.observables {
        let value = Eval::legacy(app, &state.data, &state.session, state.day, meter)
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
        value_types: program.observable_types()?,
        view_schema: program.view_schema(&view.view)?,
        view,
        outputs: state.outputs.clone(),
    };
    observation.validate()?;
    meter.tick(1)?;
    Ok(observation)
}

/// Fallible view observation over the promoted persisted owners. The output
/// mirrors Observation's wire fields while retaining sorted fallible map/set
/// storage for the next runtime-evidence stage.
#[derive(Debug, Serialize)]
pub(crate) struct DecodedRuntimeObservation {
    point: String,
    data_digest: Digest,
    session_digest: Digest,
    values: FallibleValues,
    value_types: DecodedTypeMap,
    view: DecodedViewObservation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view_schema: Option<DecodedViewSchema>,
    outputs: Vec<()>,
}

#[derive(Debug, Serialize)]
struct DecodedViewObservation {
    view: String,
    rows: Vec<DecodedPresentedRow>,
    controls: FallibleValues,
    selected: Vec<RecordRef>,
    enabled_actions: DecodedStringSet,
    form_values: FallibleValues,
}

#[derive(Debug, Serialize)]
struct DecodedPresentedRow {
    record: RecordRef,
    cells: FallibleValues,
    enabled_actions: DecodedStringSet,
}

#[derive(Debug, Serialize)]
struct DecodedViewSchema {
    entity: String,
    columns: DecodedTypeMap,
    row_actions: Option<DecodedStringMap>,
}

struct DecodedTypeMap(OrderedMap<String, Type>);
impl fmt::Debug for DecodedTypeMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}
impl Serialize for DecodedTypeMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, value)?; }
        map.end()
    }
}

struct DecodedStringMap(OrderedMap<String, String>);
impl fmt::Debug for DecodedStringMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}
impl Serialize for DecodedStringMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() { map.serialize_entry(key, value)?; }
        map.end()
    }
}

struct DecodedStringSet(OrderedSet<String>);
impl fmt::Debug for DecodedStringSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}

fn decoded_view_schema(
    program: &FallibleDecodedCapturedProgram,
    view_id: &str,
    meter: &mut Meter,
) -> Result<Option<DecodedViewSchema>> {
    use crate::product_contract::fallible_app_decode::DecodedViewKind;
    let app = program.app();
    let view = app.views.iter().find(|view| view.id == view_id)
        .ok_or_else(|| invalid("unknown view"))?;
    let (entity, columns) = match &view.kind {
        DecodedViewKind::List { entity, columns, .. }
        | DecodedViewKind::Detail { entity, columns, .. } => (entity, columns.as_slice()),
        DecodedViewKind::Form { .. } => return Ok(None),
    };
    let mut decoded_columns = OrderedMap::new();
    for column in columns {
        let value_type = crate::product_contract::decoded_expression_type_with(
            app, &column.value, "row", entity, &mut meter.allocations,
        ).map_err(decoded_app_error)?;
        let key = decoded_copy_str(&column.id, meter)?;
        decoded_columns.try_insert(key, value_type, &mut meter.allocations)
            .map_err(AdapterError::Allocation)?;
    }
    let mut row_actions = OrderedMap::new();
    for binding in view.actions.iter().filter(|binding| binding.placement == ActionPlacement::Row) {
        let key = decoded_copy_str(&binding.id, meter)?;
        let action = decoded_copy_str(&binding.action, meter)?;
        row_actions.try_insert(key, action, &mut meter.allocations)
            .map_err(AdapterError::Allocation)?;
    }
    Ok(Some(DecodedViewSchema {
        entity: decoded_copy_str(entity, meter)?,
        columns: DecodedTypeMap(decoded_columns),
        row_actions: Some(DecodedStringMap(row_actions)),
    }))
}

/// Run the actual decoded observation renderer. This is intentionally only a
/// view operation: lifecycle replay owns scenario/trace binding, while this
/// function owns the complete evaluator-backed Observation payload.
pub(super) fn observe_decoded(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    session: &FallibleDecodedSessionState,
    day: i32,
    point: &str,
    limits: RuntimeLimits,
    cancelled: Arc<AtomicBool>,
    allocations: &mut AllocationContext,
) -> Result<DecodedRuntimeObservation> {
    if !valid_id(point) { return Err(invalid("invalid observation point")); }
    program.validate_with(allocations).map_err(decoded_app_error)?;
    data.validate_with(allocations).map_err(decoded_app_error)?;
    program.validate_session_with(session, allocations).map_err(decoded_app_error)?;
    validate_day(day)?;
    limits.validate()?;
    if data.project_id() != program.project_id() {
        return Err(invalid("program and data belong to different projects"));
    }

    let owned_allocations = std::mem::replace(allocations, AllocationContext::new());
    let mut meter = Meter::with_allocations(limits.clone(), limits.fuel, cancelled, owned_allocations);
    let result = (|| {
        meter.tick(1)?;
        let app = program.app();
        let mut values = FallibleValues::new();
        for observable in &app.observables {
            let mut evaluator = Eval::fallible(app, data, session, day, &mut meter);
            let value = evaluator.eval(&observable.value, &ManagedEnv::new())?;
            meter.value(&value)?;
            decoded_insert_value(&mut values, &observable.id, value, &mut meter)?;
        }

        let view = render_decoded(program, data, session, day, &mut meter)?;
        let data_digest = data.identity_with(&mut meter.allocations).map_err(decoded_app_error)?;
        let session_digest = session.identity_with(&mut meter.allocations).map_err(|error| match error {
            crate::product_backup::fallible_codec::CodecError::Allocation(failure) => AdapterError::Allocation(failure),
            _ => invalid("observation session identity failed"),
        })?;
        let mut value_types = OrderedMap::new();
        for observable in &app.observables {
            let value_type = crate::product_contract::decoded_expression_type_empty_with(
                app, &observable.value, &mut meter.allocations,
            ).map_err(decoded_app_error)?;
            let key = decoded_copy_str(&observable.id, &mut meter)?;
            value_types.try_insert(key, value_type, &mut meter.allocations)
                .map_err(AdapterError::Allocation)?;
        }
        let view_schema = decoded_view_schema(program, &view.view, &mut meter)?;
        let observation = DecodedRuntimeObservation {
            point: decoded_copy_str(point, &mut meter)?,
            data_digest,
            session_digest,
            values,
            value_types: DecodedTypeMap(value_types),
            view,
            view_schema,
            outputs: Vec::new(),
        };
        meter.tick(1)?;
        Ok(observation)
    })();
    *allocations = meter.allocations;
    result
}
impl Serialize for DecodedStringSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.iter() { sequence.serialize_element(value)?; }
        sequence.end()
    }
}

fn decoded_app_error(error: AppValidationError) -> AdapterError {
    match error {
        AppValidationError::Contract(error) => AdapterError::Invalid(error),
        AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
    }
}
fn decoded_copy_str(value: &str, meter: &mut Meter) -> Result<String> {
    product_alloc::try_copy_str(value, &mut meter.allocations, AllocationSite::String)
        .map_err(AdapterError::Allocation)
}
fn decoded_copy_value(value: &DataValue, meter: &mut Meter) -> Result<DataValue> {
    crate::product_contract::try_clone_data_value_with(value, &mut meter.allocations)
        .map_err(AdapterError::Allocation)
}
fn decoded_ref(value: &DataValue, meter: &mut Meter) -> Result<RecordRef> {
    let DataValue::Reference { entity, record } = value else {
        return Err(failed("expected record reference"));
    };
    Ok(RecordRef {
        entity: decoded_copy_str(entity, meter)?,
        record: decoded_copy_str(record, meter)?,
    })
}
fn decoded_insert_value(values: &mut FallibleValues, key: &str, value: DataValue, meter: &mut Meter) -> Result<()> {
    let key = decoded_copy_str(key, meter)?;
    values.try_insert(key, value, &mut meter.allocations).map_err(AdapterError::Allocation)?;
    Ok(())
}
fn decoded_insert_set(set: &mut OrderedSet<String>, key: &str, meter: &mut Meter) -> Result<()> {
    let key = decoded_copy_str(key, meter)?;
    set.try_insert(key, &mut meter.allocations).map_err(AdapterError::Allocation)?;
    Ok(())
}
fn decoded_insert_unique_row(
    seen: &mut OrderedSet<(String, String)>,
    reference: &RecordRef,
    meter: &mut Meter,
    message: &'static str,
) -> Result<()> {
    let entity = decoded_copy_str(&reference.entity, meter)?;
    let record = decoded_copy_str(&reference.record, meter)?;
    if !seen.try_insert((entity, record), &mut meter.allocations).map_err(AdapterError::Allocation)? {
        return Err(invalid(message));
    }
    Ok(())
}

fn enabled_decoded(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    data: &FallibleDecodedDataSnapshot,
    session: &FallibleDecodedSessionState,
    day: i32,
    bindings: &[crate::product_contract::fallible_app_decode::DecodedActionBinding],
    placement: ActionPlacement,
    env: &dyn eval::EnvAccess,
    meter: &mut Meter,
) -> Result<DecodedStringSet> {
    let mut result = OrderedSet::new();
    for binding in bindings.iter().filter(|binding| binding.placement == placement) {
        let enabled = {
            let mut evaluator = Eval::fallible(app, data, session, day, meter);
            boolean(evaluator.eval(&binding.enabled, env)?)?
        };
        if !enabled { continue; }
        let action = app.actions.iter().find(|action| action.id == binding.action)
            .ok_or_else(|| failed("action absent"))?;
        let mut arguments = ManagedEnv::new();
        for (id, expression) in binding.arguments.iter() {
            let value = {
                let mut evaluator = Eval::fallible(app, data, session, day, meter);
                evaluator.eval(expression, env)?
            };
            check_references_decoded(&value, DataSnapshotView::from(data))?;
            let ty = action.parameters.get(id).ok_or_else(|| failed("action parameter absent"))?;
            arguments.try_bind(id, ty, &value, &mut meter.allocations)
                .map_err(AdapterError::Allocation)?;
        }
        let mut permitted = true;
        for guard in &action.guards {
            let allowed = {
                let mut evaluator = Eval::fallible(app, data, session, day, meter);
                boolean(evaluator.eval(guard, &arguments)?)?
            };
            if !allowed {
                permitted = false;
                break;
            }
        }
        if permitted { decoded_insert_set(&mut result, &binding.id, meter)?; }
    }
    Ok(DecodedStringSet(result))
}

fn render_decoded(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    session: &FallibleDecodedSessionState,
    day: i32,
    meter: &mut Meter,
) -> Result<DecodedViewObservation> {
    use crate::product_contract::fallible_app_decode::DecodedViewKind;

    let app = program.app();
    let data_view = DataSnapshotView::from(data);
    let view = app.views.iter().find(|view| view.id == session.view())
        .ok_or_else(|| invalid("session view absent"))?;
    let mut result = DecodedViewObservation {
        view: decoded_copy_str(&view.id, meter)?,
        rows: Vec::new(),
        controls: FallibleValues::new(),
        selected: Vec::new(),
        enabled_actions: enabled_decoded(
            app, data, session, day, &view.actions, ActionPlacement::Toolbar,
            &ManagedEnv::new(), meter,
        )?,
        form_values: FallibleValues::new(),
    };
    let (items, columns) = match &view.kind {
        DecodedViewKind::List { rows, columns, controls, selection, .. } => {
            for control in controls {
                let value = session.values().get(&control.state)
                    .ok_or_else(|| failed("control state absent"))?;
                meter.value(value)?;
                let copied = decoded_copy_value(value, meter)?;
                decoded_insert_value(&mut result.controls, &control.id, copied, meter)?;
            }
            if let Some(selection) = selection {
                let value = session.values().get(&selection.state)
                    .ok_or_else(|| failed("selection state absent"))?;
                meter.value(value)?;
                let (_, values) = list(decoded_copy_value(value, meter)?)?;
                let mut unique = OrderedSet::new();
                for value in values {
                    let key = decoded_ref(&value, meter)?;
                    if data_view.record(&key.entity, &key.record).is_none() {
                        return Err(invalid("record reference does not exist"));
                    }
                    decoded_insert_unique_row(&mut unique, &key, meter, "duplicate selection")?;
                    product_alloc::try_push(&mut result.selected, key, &mut meter.allocations, AllocationSite::Vector)
                        .map_err(AdapterError::Allocation)?;
                }
            }
            let mut evaluator = Eval::fallible(app, data, session, day, meter);
            let values = list(evaluator.eval(rows, &ManagedEnv::new())?)?.1;
            (values, columns.as_slice())
        }
        DecodedViewKind::Detail { record, columns, .. } => {
            let mut evaluator = Eval::fallible(app, data, session, day, meter);
            let key = evaluator.eval(record, &ManagedEnv::new())?;
            let mut values = Vec::new();
            product_alloc::try_push(&mut values, key, &mut meter.allocations, AllocationSite::Vector)
                .map_err(AdapterError::Allocation)?;
            (values, columns.as_slice())
        }
        DecodedViewKind::Form { defaults, .. } => {
            for (_, value) in defaults.iter() {
                meter.value(value)?;
                check_references_decoded(value, data_view)?;
            }
            result.form_values = defaults.try_clone_with(&mut meter.allocations)
                .map_err(AdapterError::Allocation)?;
            return Ok(result);
        }
    };
    meter.collection(items.len())?;
    let mut unique = OrderedSet::new();
    for value in items {
        meter.tick(1)?;
        let record = eval::record_view(&data_view, &value)?;
        let key = decoded_ref(&value, meter)?;
        decoded_insert_unique_row(&mut unique, &key, meter, "view produced duplicate row")?;
        let entity = decoded_copy_str(record.entity(), meter)?;
        let row_type = Type::Reference { entity };
        let mut env = ManagedEnv::new();
        env.try_bind("row", &row_type, &value, &mut meter.allocations)
            .map_err(AdapterError::Allocation)?;
        let mut cells = FallibleValues::new();
        for column in columns {
            let mut evaluator = Eval::fallible(app, data, session, day, meter);
            let value = evaluator.eval(&column.value, &env)?;
            meter.value(&value)?;
            decoded_insert_value(&mut cells, &column.id, value, meter)?;
        }
        let enabled_actions = enabled_decoded(
            app, data, session, day, &view.actions, ActionPlacement::Row, &env, meter,
        )?;
        product_alloc::try_push(
            &mut result.rows,
            DecodedPresentedRow { record: key, cells, enabled_actions },
            &mut meter.allocations,
            AllocationSite::Vector,
        ).map_err(AdapterError::Allocation)?;
    }
    Ok(result)
}
