//! Exhaustive diagnostic traversal. No use is granted an exemption here.
use super::common_fact_inventory::*;
use super::OwnedSourceSide;
use crate::product_contract::*;
use serde::Serialize;
use serde_json::json;

struct Visitor<'a> {
    app: &'a AppDefinition,
    source: &'a Digest,
    side: OwnedSourceSide,
    root: &'a str,
    uses: &'a mut Vec<CommonFactUse>,
    obligations: &'a mut Vec<CommonFactObligation>,
}
impl Visitor<'_> {
    fn need(&mut self, at: &str, code: &str, reason: &str) {
        obligation(self.obligations, Some(self.side), at, code, reason);
    }
    fn descriptor<T: Serialize>(
        &mut self,
        at: &str,
        kind: &str,
        ty: Option<Type>,
        env: &Env,
        details: &T,
    ) -> Result<()> {
        let entity = ty.as_ref().and_then(relation).map(String::from);
        self.uses.push(CommonFactUse {
            side: self.side,
            source: self.source.clone(),
            location: at.into(),
            kind: kind.into(),
            value_type: ty,
            expression: None,
            environment: env.clone(),
            entity,
            field: None,
            state: None,
            details: serde_json::to_value(details)?,
        });
        Ok(())
    }
    fn expr(&mut self, expr: &Expr, env: &Env, at: &str) -> Result<()> {
        let ty = self.app.expression_type_in(expr, env)?;
        let mut entity = relation(&ty).map(String::from);
        let mut field = None;
        let mut state = None;
        // Enumerated deliberately: a new language form cannot silently disappear
        // from membership, identity, control, input, or reference-use inventory.
        let kind = match expr {
            Expr::Literal { .. } => "literal",
            Expr::Variable { .. } => "variable",
            Expr::Field { record, field: id } => {
                entity = relation(&self.app.expression_type_in(record, env)?).map(String::from);
                field = Some(id.clone());
                "field"
            }
            Expr::State { state: id } => {
                state = Some(id.clone());
                "state"
            }
            Expr::Today => "today",
            Expr::Query { entity: id, .. } => {
                entity = Some(id.clone());
                "query"
            }
            Expr::Filter { .. } => "filter",
            Expr::Map { .. } => "map",
            Expr::Count { .. } => "count",
            Expr::Any { .. } => "any",
            Expr::All { .. } => "all",
            Expr::Sum { .. } => "sum",
            Expr::Contains { .. } => "contains",
            Expr::Equal { left, .. } => {
                if relation(&self.app.expression_type_in(left, env)?).is_some() {
                    "reference_equality"
                } else {
                    "equal"
                }
            }
            Expr::Less { .. } => "less",
            Expr::Add { .. } => "add",
            Expr::Subtract { .. } => "subtract",
            Expr::Concat { .. } => "concat",
            Expr::TextContains { .. } => "text_contains",
            Expr::Lower { .. } => "lower",
            Expr::And { .. } => "and",
            Expr::Or { .. } => "or",
            Expr::Not { .. } => "not",
            Expr::If { .. } => "if",
            Expr::Coalesce { .. } => "coalesce",
            Expr::DateAdd { .. } => "date_add",
            Expr::DateDifference { .. } => "date_difference",
        };
        self.uses.push(CommonFactUse {
            side: self.side,
            source: self.source.clone(),
            location: at.into(),
            kind: kind.into(),
            value_type: Some(ty),
            expression: Some(expr.clone()),
            environment: env.clone(),
            entity,
            field,
            state,
            details: serde_json::Value::Null,
        });
        match expr {
            Expr::Literal { .. } | Expr::Variable { .. } | Expr::State { .. } | Expr::Today => {}
            Expr::Field { record, .. } => self.expr(record, env, &format!("{at}.record"))?,
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                ..
            } => {
                self.need(at,"identity_order","Relation membership, archive inclusion, predicate/error order, ID ordering/final tie-break, and limit are dependencies; even an untruncated loop can have order-sensitive shared writes");
                self.need(at,"cross_root_coupling","Query membership and consumers need exact root-correlation and immutable-owner closure; an entity type or a shared common subtree does not confine effects to the named input root");
                let mut inner = env.clone();
                inner.insert(binding.clone(), Type::reference(entity));
                self.expr(predicate, &inner, &format!("{at}.predicate"))?;
                for (i, key) in sort.iter().enumerate() {
                    self.expr(&key.value, &inner, &format!("{at}.sort[{i}]"))?;
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
                self.expr(items, env, &format!("{at}.items"))?;
                let mut inner = env.clone();
                inner.insert(binding.clone(), item(self.app, items, env)?);
                self.expr(predicate, &inner, &format!("{at}.predicate"))?;
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                self.expr(items, env, &format!("{at}.items"))?;
                let mut inner = env.clone();
                inner.insert(binding.clone(), item(self.app, items, env)?);
                self.expr(value, &inner, &format!("{at}.value"))?;
            }
            Expr::Count { items } | Expr::Sum { items } => {
                self.expr(items, env, &format!("{at}.items"))?
            }
            Expr::Contains { items, value } => {
                self.expr(items, env, &format!("{at}.items"))?;
                self.expr(value, env, &format!("{at}.value"))?;
            }
            Expr::Equal { left, right }
            | Expr::Less { left, right }
            | Expr::Add { left, right }
            | Expr::Subtract { left, right }
            | Expr::Concat { left, right } => {
                self.expr(left, env, &format!("{at}.left"))?;
                self.expr(right, env, &format!("{at}.right"))?;
            }
            Expr::TextContains { text, search } => {
                self.expr(text, env, &format!("{at}.text"))?;
                self.expr(search, env, &format!("{at}.search"))?;
            }
            Expr::Lower { value } | Expr::Not { value } => {
                self.expr(value, env, &format!("{at}.value"))?
            }
            Expr::And { values } | Expr::Or { values } => {
                for (i, value) in values.iter().enumerate() {
                    self.expr(value, env, &format!("{at}.operand[{i}]"))?;
                }
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                self.expr(condition, env, &format!("{at}.condition"))?;
                self.expr(then_value, env, &format!("{at}.then"))?;
                self.expr(else_value, env, &format!("{at}.else"))?;
            }
            Expr::Coalesce { value, fallback } => {
                self.expr(value, env, &format!("{at}.value"))?;
                self.expr(fallback, env, &format!("{at}.fallback"))?;
            }
            Expr::DateAdd { date, days } => {
                self.expr(date, env, &format!("{at}.date"))?;
                self.expr(days, env, &format!("{at}.days"))?;
            }
            Expr::DateDifference { later, earlier } => {
                self.expr(later, env, &format!("{at}.later"))?;
                self.expr(earlier, env, &format!("{at}.earlier"))?;
            }
        }
        Ok(())
    }
    fn write(&mut self, entity: &str, at: &str) {
        if entity != self.root {
            self.need(at,"shared_effect","Create/update/archive/collection effects on an existing or added non-root relation need exact producer/control/target closure; lifecycle or structural equality grants no shared-write authority");
        } else {
            self.need(at,"cross_root_coupling","Root effects require exact correlation of the recorded target, values, guards, and enclosing selections; same-entity parameters, aliases, or common source do not prove they identify the same root, even without a query");
        }
    }
    fn statements(
        &mut self,
        action: &str,
        steps: &[Statement],
        path: &[usize],
        env: &mut Env,
    ) -> Result<()> {
        for (i, step) in steps.iter().enumerate() {
            let mut current = path.to_vec();
            current.push(i);
            let at = format!("{action} {current:?}");
            match step {
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    self.descriptor(
                        &at,
                        "create",
                        Some(Type::reference(entity)),
                        env,
                        &json!({"entity":entity,"bind":bind}),
                    )?;
                    self.write(entity, &at);
                    if entity == self.root {
                        self.need(&at,"root_membership","Root Create changes membership and initial legacy/lifecycle values; all hidden producers, scalar input provenance, and retained identity validation remain dependencies");
                    }
                    for (field, value) in values {
                        self.expr(value, env, &format!("{at}.value.{field}"))?;
                    }
                    env.insert(bind.clone(), Type::reference(entity));
                }
                Statement::Update { record, values } => {
                    let ty = self.app.expression_type_in(record, env)?;
                    self.write(
                        relation(&ty).ok_or_else(|| error("validated target lacks entity"))?,
                        &at,
                    );
                    self.expr(record, env, &format!("{at}.target"))?;
                    for (field, value) in values {
                        self.expr(value, env, &format!("{at}.value.{field}"))?;
                    }
                }
                Statement::Archive { record } => {
                    let ty = self.app.expression_type_in(record, env)?;
                    self.write(
                        relation(&ty).ok_or_else(|| error("validated target lacks entity"))?,
                        &at,
                    );
                    self.need(&at,"membership_mutation","Archival changes active relation membership and invalidates an immutable owner/lifecycle assumption");
                    self.expr(record, env, &format!("{at}.target"))?;
                }
                Statement::SetState { state, value } => {
                    self.descriptor(
                        &at,
                        "state_write",
                        Some(self.app.expression_type_in(value, env)?),
                        env,
                        &json!({"state":state}),
                    )?;
                    self.uses.last_mut().unwrap().state = Some(state.clone());
                    self.need(&at,"reset_closure","A state reset/write needs fresh whole-pair input, ingress, producer, consumer, and failure-order closure; an earlier common-operation certificate does not authorize it");
                    self.expr(value, env, &format!("{at}.value"))?;
                }
                Statement::Collection {
                    target,
                    operation,
                    items,
                } => {
                    self.descriptor(
                        &at,
                        "collection",
                        None,
                        env,
                        &json!({"target":target,"operation":operation}),
                    )?;
                    match target {
                        CollectionTarget::State { state } => {
                            self.uses.last_mut().unwrap().state = Some(state.clone());
                            self.need(&at,"reset_closure","Collection mutation is a state writer and requires complete input/reference closure");
                        }
                        CollectionTarget::Field { record, field } => {
                            let ty = self.app.expression_type_in(record, env)?;
                            self.write(
                                relation(&ty).ok_or_else(|| {
                                    error("validated collection target lacks entity")
                                })?,
                                &at,
                            );
                            self.expr(
                                &Expr::Field {
                                    record: Box::new(record.clone()),
                                    field: field.clone(),
                                },
                                env,
                                &format!("{at}.target"),
                            )?;
                        }
                    }
                    self.expr(items, env, &format!("{at}.items"))?;
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    self.descriptor(&at, "for_each", None, env, &json!({"binding":binding}))?;
                    self.need(&at,"phase_reachability","Selection is evaluated once and nested queries see current transactional state in authored iteration order; variant work cannot change common selection/reachability or justify scope pushdown");
                    self.expr(items, env, &format!("{at}.items"))?;
                    let mut inner = env.clone();
                    inner.insert(binding.clone(), item(self.app, items, env)?);
                    self.statements(action, steps, &current, &mut inner)?;
                }
                Statement::Assert { condition, .. } => {
                    self.need(&at,"global_admission","Ordinary assertions can reject the entire transaction; policy-dependent business rejection is not a resource exception");
                    self.expr(condition, env, &format!("{at}.condition"))?;
                }
                Statement::Emit {
                    output,
                    items,
                    binding,
                    columns,
                } => {
                    self.descriptor(
                        &at,
                        "emit",
                        None,
                        env,
                        &json!({"output":output,"binding":binding}),
                    )?;
                    self.need(&at,"output_closure","Stored/completed outputs and ordered emitted rows require separate preservation and identity-use proofs");
                    self.expr(items, env, &format!("{at}.items"))?;
                    let mut inner = env.clone();
                    inner.insert(binding.clone(), item(self.app, items, env)?);
                    for (column, value) in columns {
                        self.expr(value, &inner, &format!("{at}.column.{column}"))?;
                    }
                }
            }
        }
        Ok(())
    }
    fn reference_input(&mut self, at: &str, ty: &Type) {
        if relation(ty).is_some() {
            self.need(at,"reference_admission","Runtime validates input references, including optional/list wrappers and unused parameters; exact identity and retained reference closure are required");
        }
    }
    fn program(&mut self) -> Result<()> {
        let empty = Env::new();
        for entity in &self.app.entities {
            for field in &entity.fields {
                self.descriptor(
                    &format!("entity {} field {}", entity.id, field.id),
                    "schema_field",
                    Some(field.value_type.clone()),
                    &empty,
                    field,
                )?;
            }
            let env = Env::from([("record".into(), Type::reference(&entity.id))]);
            for (i, unique) in entity.unique.iter().enumerate() {
                let at = format!("entity {} unique[{i}]", entity.id);
                self.descriptor(&at, "unique", None, &env, unique)?;
                self.need(&at,"global_uniqueness","Exact uniqueness tuple controls transaction admission and may couple roots; global common keys are not owner partitions");
            }
            for (i, constraint) in entity.constraints.iter().enumerate() {
                let at = format!("entity {} constraint[{i}]", entity.id);
                self.need(&at,"global_admission","Even unchanged unrelated-entity constraints determine transaction admission; policy-dependent constraints remain unsupported");
                self.expr(constraint, &env, &at)?;
            }
        }
        for state in &self.app.state {
            let at = format!("state {} initial", state.id);
            self.descriptor(
                &at,
                "state_initial",
                Some(state.value_type.clone()),
                &empty,
                state,
            )?;
            self.uses.last_mut().unwrap().state = Some(state.id.clone());
            self.reference_input(&at, &state.value_type);
            self.expr(
                &Expr::Literal {
                    value_type: state.value_type.clone(),
                    value: state.initial.clone(),
                },
                &empty,
                &at,
            )?;
        }
        for action in &self.app.actions {
            for (name, ty) in &action.parameters {
                let at = format!("{} parameter {name}", action.id);
                self.descriptor(
                    &at,
                    "parameter",
                    Some(ty.clone()),
                    &action.parameters,
                    &json!({"action":action.id,"parameter":name}),
                )?;
                self.reference_input(&at, ty);
            }
            let mut env = action.parameters.clone();
            for (i, guard) in action.guards.iter().enumerate() {
                let at = format!("{} guard[{i}]", action.id);
                self.need(&at,"admission_reachability","Ordinary admission and non-error common-phase reachability must be independent of varying policy state at equal common facts/lifecycle; active-status guards are not lifecycle authority");
                self.expr(guard, &env, &at)?;
            }
            self.statements(&action.id, &action.steps, &[], &mut env)?;
            for (i, ensure) in action.ensures.iter().enumerate() {
                let at = format!("{} ensure[{i}]", action.id);
                self.need(&at,"global_admission","Action ensures can reject the whole transaction; policy-dependent ensures remain unsupported even when unchanged or owner-local");
                self.expr(ensure, &env, &at)?;
            }
        }
        for output in &self.app.outputs {
            self.descriptor(
                &format!("output {}", output.id),
                "output_definition",
                None,
                &empty,
                output,
            )?;
        }
        for observable in &self.app.observables {
            self.expr(
                &observable.value,
                &empty,
                &format!("observable {}", observable.id),
            )?;
        }
        for view in &self.app.views {
            let mut env = Env::new();
            match &view.kind {
                ViewKind::List {
                    entity,
                    rows,
                    columns,
                    controls,
                    selection,
                } => {
                    self.expr(rows, &env, &format!("view {} rows", view.id))?;
                    env.insert("row".into(), Type::reference(entity));
                    for column in columns {
                        self.expr(
                            &column.value,
                            &env,
                            &format!("view {} column {}", view.id, column.id),
                        )?;
                    }
                    for control in controls {
                        let at = format!("view {} control {}", view.id, control.id);
                        let ty = &self
                            .app
                            .state
                            .iter()
                            .find(|s| s.id == control.state)
                            .expect("validated control")
                            .value_type;
                        self.descriptor(&at, "control", Some(ty.clone()), &env, control)?;
                        self.uses.last_mut().unwrap().state = Some(control.state.clone());
                        self.reference_input(&at, ty);
                        self.need(&at,"input_argument_closure","Control edits and on-change callbacks are ingress; state validation and automatic argument/target derivation need closure");
                    }
                    if let Some(selection) = selection {
                        let at = format!("view {} selection {}", view.id, selection.id);
                        let ty = &self
                            .app
                            .state
                            .iter()
                            .find(|s| s.id == selection.state)
                            .expect("validated selection")
                            .value_type;
                        self.descriptor(&at, "selection", Some(ty.clone()), &env, selection)?;
                        self.uses.last_mut().unwrap().state = Some(selection.state.clone());
                        self.reference_input(&at, ty);
                        self.need(&at,"input_argument_closure","Explicit user selection is actual input, not equivalent future choices; automatic selection callbacks and state/reference validation require closure");
                    }
                }
                ViewKind::Detail {
                    entity,
                    record,
                    columns,
                } => {
                    self.expr(record, &env, &format!("view {} record", view.id))?;
                    env.insert("row".into(), Type::reference(entity));
                    for column in columns {
                        self.expr(
                            &column.value,
                            &env,
                            &format!("view {} column {}", view.id, column.id),
                        )?;
                    }
                }
                ViewKind::Form {
                    action,
                    fields,
                    defaults,
                } => {
                    let action = self
                        .app
                        .actions
                        .iter()
                        .find(|a| &a.id == action)
                        .expect("validated form");
                    self.descriptor(
                        &format!("view {} form", view.id),
                        "form",
                        None,
                        &env,
                        &json!({"action":action.id}),
                    )?;
                    for field in fields {
                        let at = format!("view {} input {}", view.id, field.parameter);
                        let ty = &action.parameters[&field.parameter];
                        self.descriptor(&at, "form_input", Some(ty.clone()), &env, field)?;
                        self.reference_input(&at, ty);
                    }
                    for (name, value) in defaults {
                        let at = format!("view {} default {name}", view.id);
                        self.expr(
                            &Expr::Literal {
                                value_type: action.parameters[name].clone(),
                                value: value.clone(),
                            },
                            &env,
                            &at,
                        )?;
                        self.reference_input(&at, &action.parameters[name]);
                    }
                }
            }
            for binding in &view.actions {
                let binding_env = if binding.placement == ActionPlacement::Row {
                    &env
                } else {
                    &empty
                };
                let at = format!("view {} binding {}", view.id, binding.id);
                self.descriptor(
                    &at,
                    "action_binding",
                    None,
                    binding_env,
                    &json!({"action":binding.action,"placement":binding.placement}),
                )?;
                self.need(&format!("{at} enabled"),"input_argument_closure","Action enabling is an executable ingress condition; exact state, policy, callback, and reference dependencies require closure even for zero-argument actions");
                self.expr(&binding.enabled, binding_env, &format!("{at} enabled"))?;
                for (name, expr) in &binding.arguments {
                    let at = format!("{at} argument {name}");
                    self.need(&at,"input_argument_closure","Scalar parameter typing does not prove independent user input: policy/state/alias-derived arguments and targets need complete producer/consumer closure");
                    self.expr(expr, binding_env, &at)?;
                }
            }
            for (i, key) in view.keys.iter().enumerate() {
                self.descriptor(
                    &format!("view {} key[{i}]", view.id),
                    "key",
                    None,
                    &env,
                    key,
                )?;
            }
        }
        Ok(())
    }
}
pub(super) fn inventory(
    app: &AppDefinition,
    source: &Digest,
    side: OwnedSourceSide,
    root: &str,
    lifecycle: &Expr,
    uses: &mut Vec<CommonFactUse>,
    obligations: &mut Vec<CommonFactObligation>,
) -> Result<std::ops::Range<usize>> {
    let mut visitor = Visitor {
        app,
        source,
        side,
        root,
        uses,
        obligations,
    };
    visitor.program()?;
    // Preserve the actual traversal origin, independently of authored IDs or
    // human-readable location labels.
    let start = visitor.uses.len();
    visitor.expr(
        lifecycle,
        &Env::from([("record".into(), Type::reference(root))]),
        "lifecycle predicate",
    )?;
    Ok(start..visitor.uses.len())
}
