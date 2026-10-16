//! Closed-use certificates for unchanged explicit-root history appends.
//! These private values discharge exact analysis sites, never runtime admission,
//! lifecycle sealing, preparation, or counterfactual history equivalence.
use super::{OwnedRead, OwnedStatementKind, OwnedStructuralReport};
use crate::product_contract::*;
use crate::product_store::ProjectSnapshot;
use serde::Serialize;
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, ContractError>;
type Env = BTreeMap<Id, Type>;
fn error(at: &str, reason: &str) -> ContractError {
    ContractError(format!("Owned common operation: {at}: {reason}"))
}
/// Diagnostic inventory only; deliberately not deserializable proof authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedCommonOperation {
    pub action: Id,
    pub action_identity: Digest,
    pub root_parameter: Id,
    pub relation: Id,
    pub owner_field: Id,
    pub append_path: Vec<usize>,
    pub root_update_paths: Vec<Vec<usize>>,
    pub bookkeeping_field: Option<Id>,
    pub reset_path: Option<Vec<usize>>,
    pub reset_key: Option<Id>,
    /// An exact admission predicate, not a verified completed/sealed predicate.
    pub admission_guard: Expr,
    pub permitted_uses: Vec<OwnedCommonUse>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedCommonUse {
    pub location: String,
    pub reason: String,
}
struct Operation {
    report: OwnedCommonOperation,
    input: Option<Id>,
    history: Vec<Id>,
}
/// Constructed afresh from authenticated source and retained snapshot identity.
/// No caller can supply this value or reuse a serialized diagnostic report.
pub(super) struct VerifiedCommonOperations {
    _snapshot: Digest,
    _before: Digest,
    _after: Digest,
    operations: Vec<Operation>,
}
fn scalar(ty: &Type) -> bool {
    matches!(ty, Type::Boolean | Type::Integer | Type::Text | Type::Date)
}
fn variable(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Variable { name: n } if n == name)
}
fn literal_true(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal {
            value_type: Type::Boolean,
            value: DataValue::Boolean { value: true }
        }
    )
}
fn empty_text(expr: &Expr) -> bool {
    matches!(expr, Expr::Literal { value_type: Type::Text, value: DataValue::Text { value } } if value.is_empty())
}
fn nonempty(expr: &Expr, value: impl Fn(&Expr) -> bool) -> bool {
    matches!(expr, Expr::Not { value: inner } if matches!(inner.as_ref(), Expr::Equal { left, right } if value(left) && empty_text(right)))
}
// Explicit primitive leaves and operators only. Reference-valued fields,
// nested owner dereferences, state, queries, and binder transformations fail.
fn local(expr: &Expr, root: &str, env: &Env, app: &AppDefinition) -> bool {
    if !app.expression_type_in(expr, env).is_ok_and(|t| scalar(&t)) {
        return false;
    }
    match expr {
        Expr::Literal { value_type, .. } => scalar(value_type),
        Expr::Variable { name } => env.get(name).is_some_and(scalar),
        Expr::Today => true,
        Expr::Field { record, .. } => variable(record, root),
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            local(left, root, env, app) && local(right, root, env, app)
        }
        Expr::TextContains { text, search } => {
            local(text, root, env, app) && local(search, root, env, app)
        }
        Expr::Lower { value } | Expr::Not { value } => local(value, root, env, app),
        Expr::And { values } | Expr::Or { values } => {
            values.iter().all(|e| local(e, root, env, app))
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => [condition, then_value, else_value]
            .iter()
            .all(|e| local(e, root, env, app)),
        Expr::DateAdd { date, days } => local(date, root, env, app) && local(days, root, env, app),
        Expr::DateDifference { later, earlier } => {
            local(later, root, env, app) && local(earlier, root, env, app)
        }
        _ => false,
    }
}
fn bookkeeping(expr: &Expr, relation: &str) -> bool {
    let Expr::Add { left, right } = expr else {
        return false;
    };
    if !matches!(
        right.as_ref(),
        Expr::Literal {
            value_type: Type::Integer,
            value: DataValue::Integer { value: 1 }
        }
    ) {
        return false;
    }
    let Expr::Count { items } = left.as_ref() else {
        return false;
    };
    matches!(items.as_ref(), Expr::Query { entity, predicate, sort, limit, include_archived: true, .. }
        if entity == relation && literal_true(predicate) && sort.is_empty() && *limit == MAX_COLLECTION)
}
fn footprint(
    action: &ActionDefinition,
    before: &AppDefinition,
    after: &AppDefinition,
    snapshot: &ProjectSnapshot,
    structure: &OwnedStructuralReport,
) -> Option<Operation> {
    if after.actions.iter().find(|a| a.id == action.id) != Some(action)
        || structure
            .edits
            .iter()
            .any(|e| e.before.action == action.id || e.after.action == action.id)
        || !action.ensures.is_empty()
    {
        return None;
    }
    let roots: Vec<_> = action
        .parameters
        .iter()
        .filter(|(_, t)| **t == Type::reference(&structure.root_entity))
        .collect();
    if roots.len() != 1 {
        return None;
    }
    let root = roots[0].0;
    if action
        .parameters
        .iter()
        .any(|(name, ty)| name != root && !scalar(ty))
    {
        return None;
    }
    if !action
        .guards
        .iter()
        .all(|e| local(e, root, &action.parameters, before))
    {
        return None;
    }
    let admission = action.guards.iter().find(|e| matches!(e, Expr::Not { value } if matches!(value.as_ref(), Expr::Equal { left, right }
        if matches!(left.as_ref(), Expr::Field { record, .. } if variable(record, root)) && matches!(right.as_ref(), Expr::Literal { value_type, .. } if scalar(value_type)))))?.clone();
    let creates: Vec<_> = action
        .steps
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Statement::Create { entity, values, .. } => Some((i, entity, values)),
            _ => None,
        })
        .collect();
    if creates.len() != 1 {
        return None;
    }
    let (append, relation, values) = creates[0];
    if relation == &structure.root_entity {
        return None;
    }
    let entity = before.entities.iter().find(|e| &e.id == relation)?;
    if after.entities.iter().find(|e| &e.id == relation) != Some(entity)
        || snapshot.data.schema.iter().find(|e| &e.id == relation) != Some(entity)
        || !entity.unique.is_empty()
        || !entity.constraints.is_empty()
    {
        return None;
    }
    let owners: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| f.value_type == Type::reference(&structure.root_entity))
        .collect();
    if owners.len() != 1 {
        return None;
    }
    let owner = &owners[0].id;
    if !values.get(owner).is_some_and(|v| variable(v, root))
        || entity
            .fields
            .iter()
            .any(|f| &f.id != owner && !scalar(&f.value_type))
    {
        return None;
    }
    let mut count = None;
    for (field, value) in values {
        if field == owner {
            continue;
        }
        if bookkeeping(value, relation) {
            if count.replace(field.clone()).is_some() {
                return None;
            }
        } else if !local(value, root, &action.parameters, before) {
            return None;
        }
    }
    let mut updates = vec![];
    let mut reset = None;
    let mut input = None;
    for (index, step) in action.steps.iter().enumerate() {
        match step {
            Statement::Create { .. } => {}
            Statement::Update { record, values }
                if variable(record, root)
                    && values
                        .values()
                        .all(|v| local(v, root, &action.parameters, before)) =>
            {
                updates.push(vec![index])
            }
            Statement::SetState { state, value }
                if index + 1 == action.steps.len() && empty_text(value) =>
            {
                let definition = before.state.iter().find(|s| &s.id == state)?;
                if definition.value_type != Type::Text
                    || definition.initial
                        != (DataValue::Text {
                            value: String::new(),
                        })
                    || after.state.iter().find(|s| &s.id == state) != Some(definition)
                {
                    return None;
                }
                let inputs: Vec<_> = action
                    .parameters
                    .iter()
                    .filter(|(name, ty)| {
                        **ty == Type::Text
                            && action
                                .guards
                                .iter()
                                .any(|g| nonempty(g, |e| variable(e, name)))
                    })
                    .collect();
                if inputs.len() != 1 {
                    return None;
                }
                input = Some(inputs[0].0.clone());
                reset = Some((index, state.clone()));
            }
            _ => return None,
        }
    }
    if updates.is_empty() {
        return None;
    }
    Some(Operation {
        report: OwnedCommonOperation {
            action: action.id.clone(),
            action_identity: canonical_digest(
                IdentityDomain::Evidence,
                &("owned-common-operation/1", action),
            )
            .ok()?,
            root_parameter: root.clone(),
            relation: relation.clone(),
            owner_field: owner.clone(),
            append_path: vec![append],
            root_update_paths: updates,
            bookkeeping_field: count,
            reset_path: reset.as_ref().map(|(i, _)| vec![*i]),
            reset_key: reset.map(|(_, s)| s),
            admission_guard: admission,
            permitted_uses: vec![],
        },
        input,
        history: vec![],
    })
}
fn contains_relation(ty: &Type, relation: &str) -> bool {
    match ty {
        Type::Reference { entity } => entity == relation,
        Type::Optional { item } | Type::List { item } => contains_relation(item, relation),
        _ => false,
    }
}
struct Inventory<'a> {
    app: &'a AppDefinition,
    operation: &'a OwnedCommonOperation,
}
impl Inventory<'_> {
    // Every Expr alternative is enumerated so new language forms cannot silently
    // inherit permission. Type inspection closes aliases and reference wrappers.
    fn expression(&self, expr: &Expr, env: &Env, at: &str) -> Result<()> {
        if matches!(expr, Expr::State { state } if self.operation.reset_key.as_ref() == Some(state))
        {
            return Err(error(at, "reset input has an unsupported consumer"));
        }
        if contains_relation(
            &self.app.expression_type_in(expr, env)?,
            &self.operation.relation,
        ) {
            return Err(error(
                at,
                "history membership/reference has an unsupported consumer",
            ));
        }
        let visit = |e: &Expr, suffix: &str| self.expression(e, env, &format!("{at}.{suffix}"));
        match expr {
            Expr::Literal { .. } | Expr::Variable { .. } | Expr::State { .. } | Expr::Today => {
                Ok(())
            }
            Expr::Field { record, .. } => visit(record, "record"),
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                ..
            } => {
                let mut inner = env.clone();
                inner.insert(binding.clone(), Type::reference(entity));
                self.expression(predicate, &inner, &format!("{at}.predicate"))?;
                for (i, key) in sort.iter().enumerate() {
                    self.expression(&key.value, &inner, &format!("{at}.sort[{i}]"))?;
                }
                Ok(())
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
                visit(items, "items")?;
                let Type::List { item } = self.app.expression_type_in(items, env)? else {
                    unreachable!()
                };
                let mut inner = env.clone();
                inner.insert(binding.clone(), *item);
                self.expression(predicate, &inner, &format!("{at}.predicate"))
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                visit(items, "items")?;
                let Type::List { item } = self.app.expression_type_in(items, env)? else {
                    unreachable!()
                };
                let mut inner = env.clone();
                inner.insert(binding.clone(), *item);
                self.expression(value, &inner, &format!("{at}.value"))
            }
            Expr::Count { items } | Expr::Sum { items } => visit(items, "items"),
            Expr::Contains { items, value } => {
                visit(items, "items")?;
                visit(value, "value")
            }
            Expr::Equal { left, right }
            | Expr::Less { left, right }
            | Expr::Add { left, right }
            | Expr::Subtract { left, right }
            | Expr::Concat { left, right } => {
                visit(left, "left")?;
                visit(right, "right")
            }
            Expr::TextContains { text, search } => {
                visit(text, "text")?;
                visit(search, "search")
            }
            Expr::Lower { value } | Expr::Not { value } => visit(value, "value"),
            Expr::And { values } | Expr::Or { values } => {
                for (i, e) in values.iter().enumerate() {
                    visit(e, &format!("operand[{i}]"))?;
                }
                Ok(())
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                visit(condition, "condition")?;
                visit(then_value, "then")?;
                visit(else_value, "else")
            }
            Expr::Coalesce { value, fallback } => {
                visit(value, "value")?;
                visit(fallback, "fallback")
            }
            Expr::DateAdd { date, days } => {
                visit(date, "date")?;
                visit(days, "days")
            }
            Expr::DateDifference { later, earlier } => {
                visit(later, "later")?;
                visit(earlier, "earlier")
            }
        }
    }
    fn statements(&self, steps: &[Statement], env: &mut Env, at: &str) -> Result<()> {
        for (i, step) in steps.iter().enumerate() {
            let at = format!("{at}[{i}]");
            match step {
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    for (f, e) in values {
                        self.expression(e, env, &format!("{at}.{f}"))?;
                    }
                    env.insert(bind.clone(), Type::reference(entity));
                }
                Statement::Update { record, values } => {
                    self.expression(record, env, &format!("{at}.target"))?;
                    for (f, e) in values {
                        self.expression(e, env, &format!("{at}.{f}"))?;
                    }
                }
                Statement::Archive { record } => {
                    self.expression(record, env, &format!("{at}.target"))?
                }
                Statement::SetState { state, value } => {
                    if self.operation.reset_key.as_ref() == Some(state) {
                        return Err(error(&at, "reset input has another action writer"));
                    }
                    self.expression(value, env, &format!("{at}.value"))?;
                }
                Statement::Collection { target, items, .. } => {
                    match target {
                        CollectionTarget::State { state }
                            if self.operation.reset_key.as_ref() == Some(state) =>
                        {
                            return Err(error(&at, "reset input has a collection writer"))
                        }
                        CollectionTarget::Field { record, field } => self.expression(
                            &Expr::Field {
                                record: Box::new(record.clone()),
                                field: field.clone(),
                            },
                            env,
                            &format!("{at}.target"),
                        )?,
                        _ => {}
                    }
                    self.expression(items, env, &format!("{at}.items"))?;
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    self.expression(items, env, &format!("{at}.items"))?;
                    let Type::List { item } = self.app.expression_type_in(items, env)? else {
                        unreachable!()
                    };
                    let mut inner = env.clone();
                    inner.insert(binding.clone(), *item);
                    self.statements(steps, &mut inner, &at)?;
                }
                Statement::Assert { condition, .. } => {
                    self.expression(condition, env, &format!("{at}.condition"))?
                }
                Statement::Emit {
                    items,
                    binding,
                    columns,
                    ..
                } => {
                    self.expression(items, env, &format!("{at}.items"))?;
                    let Type::List { item } = self.app.expression_type_in(items, env)? else {
                        unreachable!()
                    };
                    let mut inner = env.clone();
                    inner.insert(binding.clone(), *item);
                    for (c, e) in columns {
                        self.expression(e, &inner, &format!("{at}.{c}"))?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn terminal(view: &ViewDefinition, relation: &str) -> bool {
    if !view.actions.is_empty() || !view.keys.is_empty() {
        return false;
    }
    let ViewKind::List {
        entity,
        rows,
        columns,
        controls,
        selection,
    } = &view.kind
    else {
        return false;
    };
    let Expr::Query {
        entity: queried,
        binding,
        predicate,
        sort,
        ..
    } = rows
    else {
        return false;
    };
    entity == relation
        && queried == relation
        && literal_true(predicate)
        && controls.is_empty()
        && selection.is_none()
        && sort
            .iter()
            .all(|s| matches!(&s.value, Expr::Field { record, .. } if variable(record, binding)))
        && columns
            .iter()
            .all(|c| matches!(&c.value, Expr::Field { record, .. } if variable(record, "row")))
}
fn input_enabled(expr: &Expr, key: &str, env: &Env, app: &AppDefinition) -> bool {
    nonempty(expr, |e| matches!(e, Expr::State { state } if state == key))
        || match expr {
            Expr::And { values } => values.iter().all(|e| input_enabled(e, key, env, app)),
            _ => local(expr, "row", env, app),
        }
}
fn closure(
    app: &AppDefinition,
    other: &AppDefinition,
    structure: &OwnedStructuralReport,
    operation: &mut Operation,
) -> Result<()> {
    let op = &operation.report;
    for writer in &structure.writers {
        if writer.entity == op.relation
            && (writer.action != op.action
                || writer.path != op.append_path
                || writer.kind != OwnedStatementKind::Create)
        {
            return Err(error(
                &format!("{} {:?}", writer.action, writer.path),
                "history relation has another producer or a mutation",
            ));
        }
    }
    let inventory = Inventory { app, operation: op };
    for action in &app.actions {
        if action.id == op.action {
            continue;
        } // Complete footprint checked, not a syntax-only bypass.
          // Invoke validates every argument reference, even an unused one.
        for (name, ty) in &action.parameters {
            if contains_relation(ty, &op.relation) {
                return Err(error(
                    &format!("{} parameter {name}", action.id),
                    "history reference admission is an unsupported consumer",
                ));
            }
        }
        let mut env = action.parameters.clone();
        for (i, g) in action.guards.iter().enumerate() {
            inventory.expression(g, &env, &format!("{} guard[{i}]", action.id))?;
        }
        inventory.statements(&action.steps, &mut env, &action.id)?;
        for (i, e) in action.ensures.iter().enumerate() {
            inventory.expression(e, &env, &format!("{} ensure[{i}]", action.id))?;
        }
    }
    for entity in &app.entities {
        let env = [("record".into(), Type::reference(&entity.id))].into();
        for (i, e) in entity.constraints.iter().enumerate() {
            inventory.expression(e, &env, &format!("entity {} constraint[{i}]", entity.id))?;
        }
    }
    for observable in &app.observables {
        inventory.expression(
            &observable.value,
            &Env::new(),
            &format!("observable {}", observable.id),
        )?;
    }
    let mut uses = vec![];
    let mut history = vec![];
    for view in &app.views {
        let previous = other.views.iter().find(|v| v.id == view.id);
        if terminal(view, &op.relation) && previous == Some(view) {
            history.push(view.id.clone());
            uses.push(OwnedCommonUse {
                location: format!("view {}", view.id),
                reason: "unchanged inert stored history, including actual membership and ordering"
                    .into(),
            });
            continue;
        }
        let mut env = Env::new();
        match &view.kind {
            ViewKind::List {
                entity,
                rows,
                columns,
                controls,
                selection,
            } => {
                inventory.expression(rows, &env, &format!("view {} rows", view.id))?;
                env.insert("row".into(), Type::reference(entity));
                for column in columns {
                    inventory.expression(
                        &column.value,
                        &env,
                        &format!("view {} column {}", view.id, column.id),
                    )?;
                }
                for control in controls {
                    let state = app
                        .state
                        .iter()
                        .find(|s| s.id == control.state)
                        .expect("validated control state");
                    // Control edits validate references before any callback.
                    if contains_relation(&state.value_type, &op.relation) {
                        return Err(error(
                            &format!("view {} control {}", view.id, control.id),
                            "history reference input is outside the terminal boundary",
                        ));
                    }
                    if op.reset_key.as_ref() == Some(&control.state) {
                        let unchanged = previous.is_some_and(|v| matches!(&v.kind, ViewKind::List { controls, .. } if controls.contains(control)));
                        if !unchanged
                            || control.on_change.is_some()
                            || entity != &structure.root_entity
                        {
                            return Err(error(&view.id,"reset input control is not an unchanged callback-free root control"));
                        }
                        uses.push(OwnedCommonUse {
                            location: format!("view {} control {}", view.id, control.id),
                            reason: "unchanged ephemeral input control without callback".into(),
                        });
                    }
                }
                if selection
                    .as_ref()
                    .is_some_and(|s| op.reset_key.as_ref() == Some(&s.state))
                {
                    return Err(error(&view.id, "reset input cannot select records"));
                }
            }
            ViewKind::Detail {
                entity,
                record,
                columns,
            } => {
                inventory.expression(record, &env, &format!("view {} record", view.id))?;
                env.insert("row".into(), Type::reference(entity));
                for column in columns {
                    inventory.expression(
                        &column.value,
                        &env,
                        &format!("view {} column {}", view.id, column.id),
                    )?;
                }
            }
            ViewKind::Form {
                action, defaults, ..
            } => {
                let action = app
                    .actions
                    .iter()
                    .find(|a| &a.id == action)
                    .expect("validated form action");
                for (name, value) in defaults {
                    inventory.expression(
                        &Expr::Literal {
                            value_type: action.parameters[name].clone(),
                            value: value.clone(),
                        },
                        &env,
                        &format!("view {} default {name}", view.id),
                    )?;
                }
            }
        }
        for binding in &view.actions {
            let empty = Env::new();
            let binding_env = if binding.placement == ActionPlacement::Row {
                &env
            } else {
                &empty
            };
            let allowed_input = op.reset_key.as_ref().zip(operation.input.as_ref()).is_some_and(|(key,input)| {
                binding.action == op.action && binding.placement == ActionPlacement::Row
                    && matches!(&view.kind,ViewKind::List { entity, .. } if entity == &structure.root_entity)
                    && binding.arguments.get(&op.root_parameter).is_some_and(|e| variable(e,"row"))
                    && binding.arguments.get(input).is_some_and(|e| matches!(e, Expr::State { state } if state == key))
                    && previous.is_some_and(|v| v.actions.contains(binding))
                    && input_enabled(&binding.enabled,key,binding_env,app)
            });
            if !allowed_input {
                inventory.expression(
                    &binding.enabled,
                    binding_env,
                    &format!("view {} binding {} enabled", view.id, binding.id),
                )?;
            }
            for (name, expr) in &binding.arguments {
                if allowed_input && Some(name) == operation.input.as_ref() {
                    continue;
                }
                inventory.expression(
                    expr,
                    binding_env,
                    &format!("view {} binding {} argument {name}", view.id, binding.id),
                )?;
            }
            if allowed_input {
                uses.push(OwnedCommonUse {
                    location: format!("view {} binding {}", view.id, binding.id),
                    reason:
                        "unchanged exact presented-root invocation, scalar input and nonempty check"
                            .into(),
                });
            }
        }
    }
    operation.history = history;
    operation.report.permitted_uses = uses;
    Ok(())
}
impl VerifiedCommonOperations {
    pub(super) fn derive(
        snapshot: &ProjectSnapshot,
        after: &AppDefinition,
        structure: &OwnedStructuralReport,
        basis: &Digest,
    ) -> Result<Self> {
        let before = &snapshot
            .program()
            .map_err(|e| error("snapshot", &e.to_string()))?
            .program;
        let mut operations = vec![];
        for action in &before.actions {
            let Some(mut operation) = footprint(action, before, after, snapshot, structure) else {
                continue;
            };
            closure(before, after, structure, &mut operation)?;
            closure(after, before, structure, &mut operation)?;
            let op = &mut operation.report;
            op.permitted_uses.push(OwnedCommonUse {
                location: format!("{} {:?}", op.action, op.append_path),
                reason: "single append with immutable required exact-root owner".into(),
            });
            if let Some(field) = &op.bookkeeping_field {
                op.permitted_uses.push(OwnedCommonUse {
                    location: format!("{} {:?}.{field}", op.action, op.append_path),
                    reason: "unchanged Count(all retained history) + 1 at this append field only"
                        .into(),
                });
            }
            if let Some(path) = &op.reset_path {
                op.permitted_uses.push(OwnedCommonUse {
                    location: format!("{} {path:?}", op.action),
                    reason: "trailing literal ephemeral reset after ordinary successful admission"
                        .into(),
                });
            }
            operations.push(operation);
        }
        Ok(Self {
            _snapshot: basis.clone(),
            _before: structure.before.clone(),
            _after: structure.after.clone(),
            operations,
        })
    }
    pub(super) fn reports(&self) -> Vec<OwnedCommonOperation> {
        self.operations.iter().map(|o| o.report.clone()).collect()
    }
    pub(super) fn append(&self, action: &str, path: &[usize]) -> Option<(&str, &str)> {
        self.operations
            .iter()
            .find(|o| o.report.action == action && o.report.append_path == path)
            .map(|o| {
                (
                    o.report.root_parameter.as_str(),
                    o.report.owner_field.as_str(),
                )
            })
    }
    pub(super) fn reset(&self, action: &str, path: &[usize]) -> Option<&str> {
        self.operations
            .iter()
            .find(|o| o.report.action == action && o.report.reset_path.as_deref() == Some(path))
            .map(|o| o.report.root_parameter.as_str())
    }
    pub(super) fn history_reads(
        &self,
        view: &ViewDefinition,
        side: super::OwnedSourceSide,
    ) -> Option<Vec<OwnedRead>> {
        let op = self
            .operations
            .iter()
            .find(|o| o.history.contains(&view.id))?;
        let ViewKind::List { rows, columns, .. } = &view.kind else {
            unreachable!()
        };
        let Expr::Query { sort, .. } = rows else {
            unreachable!()
        };
        Some(
            columns
                .iter()
                .map(|c| (&c.value, format!("column {}", c.id)))
                .chain(
                    sort.iter()
                        .enumerate()
                        .map(|(i, s)| (&s.value, format!("sort[{i}]"))),
                )
                .map(|(expr, location)| {
                    let Expr::Field { field, .. } = expr else {
                        unreachable!()
                    };
                    OwnedRead {
                        location: format!("{side:?} view {} {location}", view.id),
                        entity: op.report.relation.clone(),
                        field: field.clone(),
                    }
                })
                .collect(),
        )
    }
}
