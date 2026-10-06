//! Deterministic lowering into the existing typed language. No second runtime.
use super::*;

pub(super) fn error(message: &str) -> StoreError {
    StoreError::Invalid(format!("Scoped change unsupported: {message}. Keep current work and prepare a compatible forward repair"))
}
pub(super) fn literal(value: DataValue, value_type: Type) -> Expr {
    Expr::Literal { value_type, value }
}
pub(super) fn boolean(value: bool) -> Expr {
    literal(DataValue::Boolean { value }, Type::Boolean)
}
pub(super) fn variable(name: &str) -> Expr {
    Expr::Variable { name: name.into() }
}
pub(super) fn field(subject: &str, id: &str) -> Expr {
    Expr::Field {
        record: Box::new(variable(subject)),
        field: id.into(),
    }
}
pub(super) fn flag(subject: &str, id: &str) -> Expr {
    Expr::Coalesce {
        value: Box::new(field(subject, id)),
        fallback: Box::new(boolean(false)),
    }
}
pub(super) fn conditional(condition: Expr, then_value: Expr, else_value: Expr) -> Expr {
    Expr::If {
        condition: Box::new(condition),
        then_value: Box::new(then_value),
        else_value: Box::new(else_value),
    }
}
pub(super) fn key(layer: &Digest, entity: &str, suffix: &str) -> String {
    let digest = canonical_digest(IdentityDomain::Adoption, &(layer, entity, suffix))
        .expect("bounded serializable key");
    format!("{RESERVED_PREFIX}{}_{suffix}", &digest.as_str()[..24])
}
pub(super) fn saved_key(layer: &Digest, patch: &EffectPatch) -> Result<Id> {
    let hash = canonical_digest(IdentityDomain::Adoption, &patch.request.destination)?;
    Ok(key(
        layer,
        &patch.request.entity,
        &format!("value_{}", &hash.as_str()[..16]),
    ))
}
pub(super) fn optional(value: &Type) -> Type {
    match value {
        Type::Optional { .. } => value.clone(),
        _ => Type::Optional {
            item: Box::new(value.clone()),
        },
    }
}
fn inert(typ: &Type) -> Result<Expr> {
    Ok(match typ {
        Type::Boolean => boolean(false),
        Type::Integer => literal(DataValue::Integer { value: 0 }, Type::Integer),
        Type::Text => literal(
            DataValue::Text {
                value: String::new(),
            },
            Type::Text,
        ),
        Type::Date => literal(DataValue::Date { days: 0 }, Type::Date),
        _ => {
            return Err(error(
                "protected result must be a scalar value, optionally nullable",
            ))
        }
    })
}
pub(super) fn projection(destination: &EffectDestination) -> bool {
    matches!(
        destination,
        EffectDestination::ViewColumn { .. } | EffectDestination::EmitColumn { .. }
    )
}
pub(super) fn action(destination: &EffectDestination) -> Option<&str> {
    match destination {
        EffectDestination::Update { action, .. }
        | EffectDestination::CreateValue { action, .. }
        | EffectDestination::EmitColumn { action, .. }
        | EffectDestination::EmitItems { action, .. } => Some(action),
        _ => None,
    }
}
fn step<'a>(steps: &'a mut [Statement], path: &[usize]) -> Result<&'a mut Statement> {
    if path.is_empty() || path.len() > MAX_EXPR_DEPTH {
        return Err(error("invalid statement path"));
    }
    let value = steps
        .get_mut(path[0])
        .ok_or_else(|| error("statement path is absent"))?;
    if path.len() == 1 {
        return Ok(value);
    }
    if let Statement::ForEach { steps, .. } = value {
        step(steps, &path[1..])
    } else {
        Err(error("statement path does not traverse a loop"))
    }
}
pub(super) fn slot<'a>(
    app: &'a mut AppDefinition,
    destination: &EffectDestination,
) -> Result<&'a mut Expr> {
    match destination {
        EffectDestination::ViewColumn { view, column } => {
            let v = app
                .views
                .iter_mut()
                .find(|v| &v.id == view)
                .ok_or_else(|| error("view is absent"))?;
            let columns = match &mut v.kind {
                ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => columns,
                _ => return Err(error("form is not a row projection")),
            };
            Ok(&mut columns
                .iter_mut()
                .find(|c| &c.id == column)
                .ok_or_else(|| error("view column is absent"))?
                .value)
        }
        EffectDestination::Observable { observable } => Ok(&mut app
            .observables
            .iter_mut()
            .find(|o| &o.id == observable)
            .ok_or_else(|| error("observable is absent"))?
            .value),
        _ => {
            let (id, path) = match destination {
                EffectDestination::Update { action, path, .. }
                | EffectDestination::CreateValue { action, path, .. }
                | EffectDestination::EmitColumn { action, path, .. }
                | EffectDestination::EmitItems { action, path } => (action, path),
                _ => unreachable!(),
            };
            let a = app
                .actions
                .iter_mut()
                .find(|a| &a.id == id)
                .ok_or_else(|| error("action is absent"))?;
            match (step(&mut a.steps, path)?, destination) {
                (Statement::Update { values, .. }, EffectDestination::Update { field, .. })
                | (
                    Statement::Create { values, .. },
                    EffectDestination::CreateValue { field, .. },
                ) => values
                    .get_mut(field)
                    .ok_or_else(|| error("assignment is absent")),
                (Statement::Emit { columns, .. }, EffectDestination::EmitColumn { column, .. }) => {
                    columns
                        .get_mut(column)
                        .ok_or_else(|| error("output column is absent"))
                }
                (Statement::Emit { items, .. }, EffectDestination::EmitItems { .. }) => Ok(items),
                _ => Err(error("effect destination has the wrong statement kind")),
            }
        }
    }
}
fn list_entity(expr: &Expr, app: &AppDefinition, env: &BTreeMap<Id, Type>) -> Option<Id> {
    match expr {
        Expr::Query { entity, .. } => Some(entity.clone()),
        Expr::Filter { items, .. } => list_entity(items, app, env),
        Expr::State { state } => app
            .state
            .iter()
            .find(|s| &s.id == state)
            .and_then(|s| type_entity(&s.value_type)),
        Expr::Variable { name } => env.get(name).and_then(type_entity),
        Expr::Literal { value_type, .. } => type_entity(value_type),
        _ => None,
    }
}
fn type_entity(typ: &Type) -> Option<Id> {
    if let Type::List { item } = typ {
        if let Type::Reference { entity } = item.as_ref() {
            return Some(entity.clone());
        }
    }
    None
}
fn locate_context(app: &AppDefinition, request: &EffectPatchRequest) -> Result<BTreeMap<Id, Type>> {
    if let EffectDestination::ViewColumn { view, .. } = &request.destination {
        let v = app
            .views
            .iter()
            .find(|v| &v.id == view)
            .ok_or_else(|| error("view missing"))?;
        let entity = match &v.kind {
            ViewKind::List { entity, .. } | ViewKind::Detail { entity, .. } => entity,
            _ => return Err(error("form projection")),
        };
        if entity != &request.entity || request.subject != "row" {
            return Err(error("view subject is not its own row"));
        }
        return Ok(BTreeMap::from([("row".into(), Type::reference(entity))]));
    }
    if matches!(request.destination, EffectDestination::Observable { .. }) {
        return Ok(BTreeMap::new());
    }
    let (id, path) = match &request.destination {
        EffectDestination::Update { action, path, .. }
        | EffectDestination::CreateValue { action, path, .. }
        | EffectDestination::EmitColumn { action, path, .. }
        | EffectDestination::EmitItems { action, path } => (action, path),
        _ => unreachable!(),
    };
    let a = app
        .actions
        .iter()
        .find(|a| &a.id == id)
        .ok_or_else(|| error("action missing"))?;
    let mut env = a.parameters.clone();
    let mut steps = &a.steps;
    for (depth, index) in path.iter().enumerate() {
        // Earlier Create bindings are genuine local references, never guessed.
        for previous in steps.iter().take(*index) {
            if let Statement::Create { entity, bind, .. } = previous {
                env.insert(bind.clone(), Type::reference(entity));
            }
        }
        let s = steps
            .get(*index)
            .ok_or_else(|| error("statement missing"))?;
        if depth + 1 < path.len() {
            if let Statement::ForEach {
                items,
                binding,
                steps: inner,
            } = s
            {
                let entity = list_entity(items, app, &env)
                    .ok_or_else(|| error("loop subject cannot be proven"))?;
                env.insert(binding.clone(), Type::reference(entity));
                steps = inner;
                continue;
            } else {
                return Err(error("invalid nested effect path"));
            }
        }
        match s {
            Statement::Update { record, .. } => {
                if record != &variable(&request.subject)
                    || env.get(&request.subject) != Some(&Type::reference(&request.entity))
                {
                    return Err(error("update writes a different subject"));
                }
            }
            Statement::Create { entity, .. } => {
                if entity != &request.entity {
                    return Err(error("create entity mismatch"));
                }
            }
            Statement::Emit { items, binding, .. } => {
                if !matches!(request.destination, EffectDestination::EmitItems { .. })
                    && (binding != &request.subject
                        || list_entity(items, app, &env).as_ref() != Some(&request.entity))
                {
                    return Err(error("output subject cannot be proven"));
                }
                env.insert(binding.clone(), Type::reference(&request.entity));
            }
            _ => return Err(error("unsupported effect statement")),
        }
    }
    Ok(env)
}
/// Check reads structurally, before any host instrumentation exists.
pub(super) fn own_row(
    expr: &Expr,
    subject: &str,
    entity: &str,
    app: &AppDefinition,
    env: &BTreeMap<Id, Type>,
    allow_day: bool,
) -> Result<()> {
    match expr {
        Expr::Literal { value_type, .. } => {
            if !matches!(
                value_type,
                Type::Boolean | Type::Integer | Type::Text | Type::Date | Type::Optional { .. }
            ) {
                return Err(error("non-scalar literal in record-local effect"));
            }
        }
        Expr::Variable { name } => {
            if name == subject
                || !env.get(name).is_some_and(|t| {
                    matches!(t, Type::Boolean | Type::Integer | Type::Text | Type::Date)
                })
            {
                return Err(error("unproved scalar parameter"));
            }
        }
        Expr::Field { record, field } => {
            if record.as_ref() != &variable(subject) || field.starts_with(RESERVED_PREFIX) {
                return Err(error("foreign or protected field read"));
            }
            let typ = &app
                .entities
                .iter()
                .find(|e| e.id == entity)
                .and_then(|e| e.fields.iter().find(|f| &f.id == field))
                .ok_or_else(|| error("unknown subject field"))?
                .value_type;
            if matches!(typ, Type::Reference { .. } | Type::List { .. }) {
                return Err(error("cross-record or collection field read"));
            }
        }
        Expr::Today => {
            if !allow_day {
                return Err(error("clock-sensitive completion predicate"));
            }
        }
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            own_row(left, subject, entity, app, env, allow_day)?;
            own_row(right, subject, entity, app, env, allow_day)?;
        }
        Expr::TextContains { text, search } => {
            own_row(text, subject, entity, app, env, allow_day)?;
            own_row(search, subject, entity, app, env, allow_day)?;
        }
        Expr::DateAdd { date, days } => {
            own_row(date, subject, entity, app, env, allow_day)?;
            own_row(days, subject, entity, app, env, allow_day)?;
        }
        Expr::DateDifference { later, earlier } => {
            own_row(later, subject, entity, app, env, allow_day)?;
            own_row(earlier, subject, entity, app, env, allow_day)?;
        }
        Expr::Not { value } | Expr::Lower { value } => {
            own_row(value, subject, entity, app, env, allow_day)?
        }
        Expr::And { values } | Expr::Or { values } => {
            for value in values {
                own_row(value, subject, entity, app, env, allow_day)?;
            }
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            own_row(condition, subject, entity, app, env, allow_day)?;
            own_row(then_value, subject, entity, app, env, allow_day)?;
            own_row(else_value, subject, entity, app, env, allow_day)?;
        }
        Expr::Coalesce { value, fallback } => {
            own_row(value, subject, entity, app, env, allow_day)?;
            own_row(fallback, subject, entity, app, env, allow_day)?;
        }
        _ => {
            return Err(error(
                "global query, aggregation, session or cross-record effect",
            ))
        }
    }
    Ok(())
}
pub(super) fn reject_reserved(app: &AppDefinition) -> Result<()> {
    fn contains(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::String(s) => s.starts_with(RESERVED_PREFIX),
            serde_json::Value::Array(a) => a.iter().any(contains),
            serde_json::Value::Object(m) => m
                .iter()
                .any(|(k, v)| k.starts_with(RESERVED_PREFIX) || contains(v)),
            _ => false,
        }
    }
    if contains(&serde_json::to_value(app)?) {
        Err(error(
            "authored source uses the reserved metadata namespace",
        ))
    } else {
        Ok(())
    }
}
fn normalize_labels(app: &mut AppDefinition) {
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
        for field in &mut output.columns {
            field.label.clear();
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

fn mentions(expr: &serde_json::Value, field: &str) -> bool {
    match expr {
        serde_json::Value::Object(m) => {
            m.get("kind").and_then(|v| v.as_str()) == Some("field")
                && m.get("field").and_then(|v| v.as_str()) == Some(field)
                || m.values().any(|v| mentions(v, field))
        }
        serde_json::Value::Array(a) => a.iter().any(|v| mentions(v, field)),
        _ => false,
    }
}
fn dangerous_dependency(value: &serde_json::Value, field: &str) -> bool {
    match value {
        serde_json::Value::Object(m) => {
            if m.get("kind").and_then(|v| v.as_str()) == Some("field")
                && m.get("field").and_then(|v| v.as_str()) == Some(field)
                && m.get("record")
                    .and_then(|v| v.get("kind"))
                    .and_then(|v| v.as_str())
                    != Some("variable")
            {
                return true;
            }
            let kind = m.get("kind").and_then(|v| v.as_str());
            if matches!(
                kind,
                Some("query" | "filter" | "any" | "all" | "contains" | "set_state" | "collection")
            ) && mentions(value, field)
            {
                return true;
            }
            m.values().any(|v| dangerous_dependency(v, field))
        }
        serde_json::Value::Array(a) => a.iter().any(|v| dangerous_dependency(v, field)),
        _ => false,
    }
}

fn foreign_read(value: &Expr, changed: &str, subject: Option<&str>) -> bool {
    fn visit(v: &serde_json::Value, changed: &str, subject: Option<&str>) -> bool {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("kind").and_then(|v| v.as_str()) == Some("field")
                    && m.get("field").and_then(|v| v.as_str()) == Some(changed)
                {
                    let record = m.get("record");
                    if record.and_then(|v| v.get("kind")).and_then(|v| v.as_str())
                        != Some("variable")
                        || record.and_then(|v| v.get("name")).and_then(|v| v.as_str()) != subject
                    {
                        return true;
                    }
                }
                m.values().any(|v| visit(v, changed, subject))
            }
            serde_json::Value::Array(a) => a.iter().any(|v| visit(v, changed, subject)),
            _ => false,
        }
    }
    visit(
        &serde_json::to_value(value).expect("typed expression"),
        changed,
        subject,
    )
}
fn sink_dependencies(steps: &[Statement], changed: &str) -> bool {
    for step in steps {
        match step {
            Statement::Update { record, values } => {
                let subject = match record {
                    Expr::Variable { name } => Some(name.as_str()),
                    _ => None,
                };
                if values.values().any(|v| foreign_read(v, changed, subject)) {
                    return true;
                }
            }
            Statement::Create { values, .. } => {
                if values.values().any(|v| foreign_read(v, changed, None)) {
                    return true;
                }
            }
            Statement::SetState { value, .. } => {
                if foreign_read(value, changed, None) {
                    return true;
                }
            }
            Statement::Assert { condition, .. } => {
                if foreign_read(condition, changed, None) {
                    return true;
                }
            }
            Statement::ForEach { steps, .. } => {
                if sink_dependencies(steps, changed) {
                    return true;
                }
            }
            Statement::Emit {
                binding, columns, ..
            } => {
                if columns
                    .values()
                    .any(|v| foreign_read(v, changed, Some(binding)))
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
fn check_dependencies(app: &AppDefinition, entity_id: &str, changed: &str) -> Result<()> {
    let entity = app
        .entities
        .iter()
        .find(|e| e.id == entity_id)
        .ok_or_else(|| error("effect entity absent"))?;
    if entity.unique.iter().flatten().any(|f| f == changed)
        || entity
            .constraints
            .iter()
            .any(|e| mentions(&serde_json::to_value(e).expect("typed expression"), changed))
        || dangerous_dependency(&serde_json::to_value(app)?, changed)
        || app.actions.iter().any(|a| {
            sink_dependencies(&a.steps, changed)
                || a.guards
                    .iter()
                    .chain(&a.ensures)
                    .any(|e| foreign_read(e, changed, None))
        })
    {
        return Err(error("changed durable value influences a foreign-row write, shared selection, state, constraint or uniqueness rule"));
    }
    Ok(())
}
fn check_completion_order(app: &AppDefinition) -> Result<()> {
    fn visit(steps: &[Statement], mut emitted: bool) -> Result<bool> {
        for step in steps {
            match step {
        Statement::Emit{..}=>emitted=true,
        Statement::ForEach{steps,..}=>{let nested=visit(steps,emitted)?;emitted|=nested;},
        Statement::Create{..}|Statement::Update{..}|Statement::Archive{..}|Statement::Collection{target:CollectionTarget::Field{..},..} if emitted=>return Err(error("a business write follows output; completion capture requires a compatible final-output action")),
        _=>{}
    }
        }
        Ok(emitted)
    }
    for action in &app.actions {
        visit(&action.steps, false)?;
    }
    Ok(())
}
fn check_output_coverage(app: &AppDefinition, patches: &[EffectPatchRequest]) -> Result<()> {
    fn sites(
        action: &str,
        steps: &[Statement],
        prefix: &[usize],
        out: &mut Vec<(Id, Vec<usize>, Id)>,
    ) {
        for (index, step) in steps.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(index);
            match step {
                Statement::Emit { output, .. } => out.push((action.into(), path, output.clone())),
                Statement::ForEach { steps, .. } => sites(action, steps, &path, out),
                _ => {}
            }
        }
    }
    let mut emitted = vec![];
    for action in &app.actions {
        sites(&action.id, &action.steps, &[], &mut emitted);
    }
    for patch in patches {
        if let EffectDestination::EmitColumn {
            action,
            path,
            column,
        } = &patch.destination
        {
            let (_, _, output) = emitted
                .iter()
                .find(|(a, p, _)| a == action && p == path)
                .ok_or_else(|| error("protected output site absent"))?;
            for (other_action, other_path, other_output) in &emitted {
                if other_output==output&&!patches.iter().any(|p|p.entity==patch.entity&&matches!(&p.destination,EffectDestination::EmitColumn{action,path,column:other_column} if action==other_action&&path==other_path&&other_column==column)) {
            return Err(error("a shared output has an unprotected emission; separate its output definition or explicitly cover every affected result"));
        }
            }
        }
    }
    Ok(())
}
pub(super) fn derive_patches(
    baseline: &AppDefinition,
    candidate: &AppDefinition,
    request: &ScopeRequest,
) -> Result<Vec<EffectPatch>> {
    reject_reserved(candidate)?;
    if request.patches.is_empty()
        || request.patches.len() > MAX_ITEMS
        || request.lifecycles.len() > MAX_ITEMS
    {
        return Err(error("empty or oversized effect specification"));
    }
    if !request.lifecycles.is_empty() {
        check_completion_order(baseline)?;
        check_completion_order(candidate)?;
    }
    check_output_coverage(baseline, &request.patches)?;
    check_output_coverage(candidate, &request.patches)?;
    let mut masked = baseline.clone();
    let mut seen = BTreeSet::new();
    let mut patches = vec![];
    for p in &request.patches {
        if !seen.insert(canonical_bytes(&p.destination)?) {
            return Err(error("duplicate effect destination"));
        }
        if let Some(action) = action(&p.destination) {
            if !request.operations.contains(action) {
                return Err(error("effect falls outside the named operations"));
            }
        }
        let env = locate_context(baseline, p)?;
        locate_context(candidate, p)?;
        let before = slot(&mut baseline.clone(), &p.destination)?.clone();
        let after = slot(&mut candidate.clone(), &p.destination)?.clone();
        let global = matches!(
            p.destination,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        );
        if global {
            if request.population != ScopePopulation::All || !request.excluded_records.is_empty() {
                return Err(error(
                    "global operation expression needs the whole population",
                ));
            }
        } else {
            own_row(&before, &p.subject, &p.entity, baseline, &env, true)?;
            own_row(&after, &p.subject, &p.entity, candidate, &env, true)?;
            if !request.lifecycles.iter().any(|l| l.entity == p.entity) {
                return Err(error("row effect needs an explicit completed-work meaning"));
            }
            let typ = match &p.value_type {
                Type::Optional { item } => item.as_ref(),
                typ => typ,
            };
            inert(typ)?;
            if projection(&p.destination)
                && env
                    .iter()
                    .any(|(id, _)| id != &p.subject && expression_variable(&after, id))
            {
                return Err(error("saved projection depends on an action parameter"));
            }
        }
        if let EffectDestination::Update { field, .. }
        | EffectDestination::CreateValue { field, .. } = &p.destination
        {
            for app in [baseline, candidate] {
                check_dependencies(app, &p.entity, field)?;
            }
        }
        *slot(&mut masked, &p.destination)? = after.clone();
        patches.push(EffectPatch {
            request: p.clone(),
            before,
            after,
        });
    }
    let mut target = candidate.clone();
    normalize_labels(&mut masked);
    normalize_labels(&mut target);
    if masked != target {
        return Err(error(
            "partial adoption changes the action/view shell outside enumerated effect expressions",
        ));
    }
    Ok(patches)
}
fn expression_variable(expr: &Expr, id: &str) -> bool {
    fn visit(v: &serde_json::Value, id: &str) -> bool {
        match v {
            serde_json::Value::Object(m) => {
                m.get("kind").and_then(|v| v.as_str()) == Some("variable")
                    && m.get("name").and_then(|v| v.as_str()) == Some(id)
                    || m.values().any(|v| visit(v, id))
            }
            serde_json::Value::Array(a) => a.iter().any(|v| visit(v, id)),
            _ => false,
        }
    }
    visit(&serde_json::to_value(expr).expect("typed expression"), id)
}
pub(super) fn rebind(expr: &Expr, from: &str, to: &str) -> Result<Expr> {
    // Accepted record-local expressions contain no binders; there is no capture.
    fn visit(v: &mut serde_json::Value, from: &str, to: &str) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("kind").and_then(|v| v.as_str()) == Some("variable")
                    && m.get("name").and_then(|v| v.as_str()) == Some(from)
                {
                    m.insert("name".into(), to.into());
                }
                for child in m.values_mut() {
                    visit(child, from, to)
                }
            }
            serde_json::Value::Array(a) => {
                for child in a {
                    visit(child, from, to)
                }
            }
            _ => {}
        }
    }
    let mut v = serde_json::to_value(expr)?;
    visit(&mut v, from, to);
    Ok(serde_json::from_value(v)?)
}
pub(super) fn effective_patch(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    id: &Digest,
    index: usize,
) -> Result<EffectPatch> {
    let original = snapshot
        .scope
        .layers
        .get(id)
        .and_then(|l| l.patches.get(index))
        .ok_or_else(|| error("mapped layer patch absent"))?;
    let mut patch = original.clone();
    if let Some(mapping) = manifest
        .rewrites
        .iter()
        .find(|m| &m.layer == id && m.patch == index)
    {
        let source = program(snapshot, &mapping.to_source)?;
        if std::mem::discriminant(&mapping.to)
            != std::mem::discriminant(&original.request.destination)
        {
            return Err(error("mapping changes the kind of a protected result"));
        }
        patch.before = rebind(&patch.before, &patch.request.subject, &mapping.subject)?;
        patch.request.destination = mapping.to.clone();
        patch.request.subject = mapping.subject.clone();
        patch.after = slot(&mut source.program.clone(), &mapping.to)?.clone();
        let env = locate_context(&source.program, &patch.request)?;
        if !matches!(
            mapping.to,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        ) {
            own_row(
                &patch.after,
                &patch.request.subject,
                &patch.request.entity,
                &source.program,
                &env,
                true,
            )?;
        }
    }
    Ok(patch)
}
pub(super) fn validate_mappings(
    snapshot: &ProjectSnapshot,
    previous: &CompositionManifest,
    candidate: &CapturedProgram,
    mappings: &[ScopeSlotMapping],
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for mapping in mappings {
        if mapping.from_source != previous.output
            || mapping.to_source != revision(candidate)?
            || !previous.layers.contains(&mapping.layer)
            || !seen.insert((&mapping.layer, mapping.patch))
        {
            return Err(error("ambiguous, stale or foreign protected-slot mapping"));
        }
        let current = effective_patch(snapshot, previous, &mapping.layer, mapping.patch)?;
        if mapping.from != current.request.destination {
            return Err(error(
                "protected-slot mapping does not name the current source slot",
            ));
        }
        if !destinations.insert(canonical_bytes(&mapping.to)?) {
            return Err(error(
                "overlapping protected slots need an explicit compatible reconciliation",
            ));
        }
        let mut p = current.request.clone();
        p.destination = mapping.to.clone();
        p.subject = mapping.subject.clone();
        let env = locate_context(&candidate.program, &p)?;
        let expression = slot(&mut candidate.program.clone(), &mapping.to)?.clone();
        if !matches!(
            mapping.to,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        ) {
            own_row(
                &expression,
                &p.subject,
                &p.entity,
                &candidate.program,
                &env,
                true,
            )?;
        }
    }
    if previous
        .layers
        .iter()
        .any(|id| !snapshot.scope.layers[id].request.lifecycles.is_empty())
    {
        check_completion_order(&candidate.program)?;
    }
    for id in &previous.layers {
        let mapped: Vec<_> = mappings
            .iter()
            .filter(|m| &m.layer == id)
            .map(|m| {
                let mut p = snapshot.scope.layers[id].patches[m.patch].request.clone();
                p.destination = m.to.clone();
                p.subject = m.subject.clone();
                p
            })
            .collect();
        check_output_coverage(&candidate.program, &mapped)?;
        for p in &mapped {
            if let EffectDestination::Update { field, .. }
            | EffectDestination::CreateValue { field, .. } = &p.destination
            {
                check_dependencies(&candidate.program, &p.entity, field)?;
            }
        }
    }
    let expected: usize = previous
        .layers
        .iter()
        .map(|id| snapshot.scope.layers[id].patches.len())
        .sum();
    if mappings.len() != expected {
        return Err(error(
            "every protected history and derived-write slot needs a source-qualified mapping",
        ));
    }
    Ok(())
}

pub(super) fn business_program(snapshot: &ProjectSnapshot) -> Result<AppDefinition> {
    if let Some(manifest) = snapshot.scope.compositions.get(&snapshot.active_revision) {
        let mut app = program(snapshot, &manifest.business)?.program.clone();
        let mut seen = BTreeSet::new();
        for id in &manifest.layers {
            let layer = &snapshot.scope.layers[id];
            for index in 0..layer.patches.len() {
                let patch = effective_patch(snapshot, manifest, id, index)?;
                let key = canonical_bytes(&patch.request.destination)?;
                if seen.insert(key) {
                    *slot(&mut app, &patch.request.destination)? = patch.before.clone();
                }
                if manifest.active.contains(id) {
                    *slot(&mut app, &patch.request.destination)? = patch.after.clone();
                }
            }
        }
        Ok(app)
    } else {
        Ok(snapshot.program()?.program.clone())
    }
}

pub(super) fn envelope_app(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<AppDefinition> {
    let business = program(snapshot, &manifest.business)?;
    reject_reserved(&business.program)?;
    let mut app = business.program.clone();
    let mut seen = BTreeSet::new();
    // Reset a shared destination exactly once to its earliest retained meaning.
    for id in &manifest.layers {
        let layer = snapshot
            .scope
            .layers
            .get(id)
            .ok_or_else(|| error("missing layer"))?;
        for index in 0..layer.patches.len() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            if seen.insert(canonical_bytes(&patch.request.destination)?) {
                *slot(&mut app, &patch.request.destination)? = patch.before.clone();
            }
        }
    }
    for id in &manifest.layers {
        let layer = &snapshot.scope.layers[id];
        let active = manifest.active.contains(id);
        for lifecycle in &layer.request.lifecycles {
            let entity = app
                .entities
                .iter_mut()
                .find(|e| e.id == lifecycle.entity)
                .ok_or_else(|| error("protected entity removed"))?;
            entity.constraints.push(Expr::Or {
                values: vec![
                    Expr::Not {
                        value: Box::new(flag("record", &key(id, &lifecycle.entity, "sealed"))),
                    },
                    lifecycle.completed.clone(),
                ],
            });
            for (suffix, typ, label) in [
                ("member", Type::Boolean, "Scope membership"),
                ("sealed", Type::Boolean, "Preserved result"),
                ("day", Type::Date, "Result capture day"),
                ("origin", Type::Text, "Result provenance"),
            ] {
                entity.fields.push(FieldDefinition {
                    id: key(id, &lifecycle.entity, suffix),
                    label: label.into(),
                    value_type: optional(&typ),
                });
            }
        }
        for (index, original) in layer.patches.iter().enumerate() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            let p = &patch.request;
            let old = slot(&mut app, &p.destination)?.clone();
            let global = matches!(
                p.destination,
                EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
            );
            let mut live = if !active {
                old.clone()
            } else if global {
                patch.after.clone()
            } else if matches!(p.destination, EffectDestination::CreateValue { .. }) {
                if matches!(
                    layer.request.population,
                    ScopePopulation::FutureWork | ScopePopulation::All
                ) {
                    patch.after.clone()
                } else {
                    old.clone()
                }
            } else {
                conditional(
                    flag(&p.subject, &key(id, &p.entity, "member")),
                    patch.after.clone(),
                    old.clone(),
                )
            };
            if projection(&p.destination) {
                let saved = saved_key(id, original)?;
                let sealed = key(id, &p.entity, "sealed");
                let entity = app
                    .entities
                    .iter_mut()
                    .find(|e| e.id == p.entity)
                    .ok_or_else(|| error("protected entity absent"))?;
                entity.fields.push(FieldDefinition {
                    id: saved.clone(),
                    label: "Preserved completed result".into(),
                    value_type: optional(&p.value_type),
                });
                let read = if matches!(p.value_type, Type::Optional { .. }) {
                    field(&p.subject, &saved)
                } else {
                    entity.constraints.push(Expr::Or {
                        values: vec![
                            Expr::Not {
                                value: Box::new(flag("record", &sealed)),
                            },
                            Expr::Not {
                                value: Box::new(Expr::Equal {
                                    left: Box::new(field("record", &saved)),
                                    right: Box::new(literal(
                                        DataValue::Null,
                                        optional(&p.value_type),
                                    )),
                                }),
                            },
                        ],
                    });
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &saved)),
                        fallback: Box::new(inert(&p.value_type)?),
                    }
                };
                live = conditional(flag(&p.subject, &sealed), read, live);
            } else if let EffectDestination::Update { field: target, .. } = &p.destination {
                live = conditional(
                    flag(&p.subject, &key(id, &p.entity, "sealed")),
                    field(&p.subject, target),
                    live,
                );
            }
            *slot(&mut app, &p.destination)? = live;
        }
    }
    Ok(app)
}
pub(super) fn compile(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<CapturedProgram> {
    let mut app = envelope_app(snapshot, manifest)?;
    let projected = app.clone();
    for action in &mut app.actions {
        action.steps = instrument(
            &action.steps,
            &projected,
            snapshot,
            manifest,
            &action.parameters,
        )?;
        action
            .steps
            .extend(sealing_passes(&projected, snapshot, manifest, None)?);
    }
    add_provenance(&mut app, snapshot, manifest)?;
    app.validate()?;
    let input = canonical_digest(
        IdentityDomain::Adoption,
        &(
            &manifest.business,
            &manifest.layers,
            &manifest.active,
            &manifest.rewrites,
            &manifest.previous,
            &manifest.operation,
            COMPILER_VERSION,
        ),
    )?;
    Ok(CapturedProgram::capture(
        &canonical_bytes(&app)?,
        &snapshot.data.project_id,
        Producer::ExternalAuthor {
            description: format!(
                "Verified host scope composition v{COMPILER_VERSION}: {}",
                input.as_str()
            ),
        },
        None,
    )?)
}
fn instrument(
    steps: &[Statement],
    app: &AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    environment: &BTreeMap<Id, Type>,
) -> Result<Vec<Statement>> {
    let mut environment = environment.clone();
    let mut result = vec![];
    for original in steps {
        let mut s = original.clone();
        match &mut s {
            Statement::Create { entity, values, .. } => {
                for id in &manifest.layers {
                    let layer = &snapshot.scope.layers[id];
                    if layer.request.lifecycles.iter().any(|l| &l.entity == entity) {
                        values.insert(
                            key(id, entity, "member"),
                            boolean(
                                manifest.active.contains(id)
                                    && matches!(
                                        layer.request.population,
                                        ScopePopulation::FutureWork | ScopePopulation::All
                                    ),
                            ),
                        );
                        values.insert(key(id, entity, "sealed"), boolean(false));
                    }
                }
            }
            Statement::Archive { record } => {
                let entity = match record {
                    Expr::Variable { name } => match environment.get(name) {
                        Some(Type::Reference { entity }) => entity.clone(),
                        _ => return Err(error("archive subject cannot be proven")),
                    },
                    _ => return Err(error("archive subject cannot be proven")),
                };
                result.extend(sealing_passes(
                    app,
                    snapshot,
                    manifest,
                    Some((record, &entity)),
                )?)
            }
            Statement::Emit { .. } => result.extend(sealing_passes(app, snapshot, manifest, None)?),
            Statement::ForEach {
                steps,
                items,
                binding,
            } => {
                let entity = list_entity(items, app, &environment)
                    .ok_or_else(|| error("loop subject cannot be proven for completion capture"))?;
                let mut nested = environment.clone();
                nested.insert(binding.clone(), Type::reference(entity));
                *steps = instrument(steps, app, snapshot, manifest, &nested)?
            }
            _ => {}
        }
        if let Statement::Create { entity, bind, .. } = &s {
            environment.insert(bind.clone(), Type::reference(entity));
        }
        result.push(s);
    }
    Ok(result)
}
fn sealing_passes(
    app: &AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    archive: Option<(&Expr, &str)>,
) -> Result<Vec<Statement>> {
    let mut result = vec![];
    for id in &manifest.layers {
        let layer = &snapshot.scope.layers[id];
        for lifecycle in &layer.request.lifecycles {
            let binding = key(id, &lifecycle.entity, "seal_row");
            // Archive supports the same explicit own-row reference grammar. Cross-
            // entity archive programs are rejected rather than inventing a cast.
            let condition = if let Some((record, entity)) = archive {
                if entity != lifecycle.entity {
                    continue;
                }
                Expr::Equal {
                    left: Box::new(variable(&binding)),
                    right: Box::new(record.clone()),
                }
            } else {
                rebind(&lifecycle.completed, "record", &binding)?
            };
            let query = Expr::Query {
                entity: lifecycle.entity.clone(),
                binding: binding.clone(),
                predicate: Box::new(Expr::And {
                    values: vec![
                        Expr::Not {
                            value: Box::new(flag(&binding, &key(id, &lifecycle.entity, "sealed"))),
                        },
                        condition,
                    ],
                }),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            };
            // Expr assignments (not data values) all read the pre-seal row together.
            let mut assignments = BTreeMap::new();
            for (index, original) in layer.patches.iter().enumerate() {
                let patch = effective_patch(snapshot, manifest, id, index)?;
                if patch.request.entity == lifecycle.entity
                    && projection(&patch.request.destination)
                {
                    let expression = slot(&mut app.clone(), &patch.request.destination)?.clone();
                    assignments.insert(
                        saved_key(id, original)?,
                        rebind(&expression, &patch.request.subject, &binding)?,
                    );
                }
            }
            assignments.insert(key(id, &lifecycle.entity, "sealed"), boolean(true));
            assignments.insert(key(id, &lifecycle.entity, "day"), Expr::Today);
            assignments.insert(
                key(id, &lifecycle.entity, "origin"),
                literal(
                    DataValue::Text {
                        value: if archive.is_some() {
                            "ObservedAtArchive"
                        } else {
                            "ObservedAtCompletion"
                        }
                        .into(),
                    },
                    Type::Text,
                ),
            );
            // Keep the pass an ordinary bounded atomic language transaction.
            result.push(Statement::ForEach {
                items: query,
                binding: binding.clone(),
                steps: vec![Statement::Update {
                    record: variable(&binding),
                    values: assignments,
                }],
            });
        }
    }
    Ok(result)
}
fn add_provenance(
    app: &mut AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<()> {
    // Schema-carried provenance survives exported bytes, including mixed live
    // and saved rows; it is not merely a transient host caption.
    for id in &manifest.layers {
        let layer = &snapshot.scope.layers[id];
        for index in 0..layer.patches.len() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            let p = &patch.request;
            if !projection(&p.destination) {
                continue;
            }
            let origin = key(id, &p.entity, "origin");
            let day = key(id, &p.entity, "day");
            let values = [
                (
                    origin.clone(),
                    "Result provenance",
                    Type::Text,
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &origin)),
                        fallback: Box::new(literal(
                            DataValue::Text {
                                value: "CurrentCalculation".into(),
                            },
                            Type::Text,
                        )),
                    },
                ),
                (
                    day.clone(),
                    "Result day",
                    Type::Date,
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &day)),
                        fallback: Box::new(Expr::Today),
                    },
                ),
            ];
            match &p.destination {
                EffectDestination::ViewColumn { view, .. } => {
                    let v = app
                        .views
                        .iter_mut()
                        .find(|v| &v.id == view)
                        .ok_or_else(|| error("view removed"))?;
                    let columns = match &mut v.kind {
                        ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => {
                            columns
                        }
                        _ => unreachable!(),
                    };
                    for (id, label, _, value) in values {
                        if !columns.iter().any(|c| c.id == id) {
                            columns.push(Column {
                                id,
                                label: label.into(),
                                value,
                            });
                        }
                    }
                }
                EffectDestination::EmitColumn { action, path, .. } => {
                    // Instrumentation changed statement positions, so locate every
                    // original matching output/binding rather than reusing indices.
                    let mut source = program(snapshot, &manifest.business)?.program.clone();
                    let a = source
                        .actions
                        .iter_mut()
                        .find(|a| &a.id == action)
                        .ok_or_else(|| error("output action absent"))?;
                    let output = if let Statement::Emit { output, .. } = step(&mut a.steps, path)? {
                        output.clone()
                    } else {
                        return Err(error("output slot absent"));
                    };
                    let definition = app
                        .outputs
                        .iter_mut()
                        .find(|o| o.id == output)
                        .ok_or_else(|| error("output definition absent"))?;
                    for (id, label, typ, _) in &values {
                        if !definition.columns.iter().any(|c| &c.id == id) {
                            definition.columns.push(FieldDefinition {
                                id: id.clone(),
                                label: label.to_string(),
                                value_type: typ.clone(),
                            });
                        }
                    }
                    fn append(
                        steps: &mut [Statement],
                        output: &str,
                        subject: &str,
                        values: &[(Id, &str, Type, Expr); 2],
                    ) -> Result<()> {
                        for step in steps {
                            match step {
                                Statement::Emit {
                                    output: id,
                                    binding,
                                    columns,
                                    ..
                                } if id == output => {
                                    for (id, _, _, expr) in values {
                                        columns.insert(id.clone(), rebind(expr, subject, binding)?);
                                    }
                                }
                                Statement::ForEach { steps, .. } => {
                                    append(steps, output, subject, values)?
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    }
                    for action in &mut app.actions {
                        append(&mut action.steps, &output, &p.subject, &values)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
