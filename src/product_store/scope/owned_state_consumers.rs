//! Borrowed semantic-consumer check for closed continuation marker permission.
//! This is deliberately conservative: syntactic State reads in any branch count.
use super::*;

pub(super) fn reads_state(
    app: &AppDefinition,
    state: &str,
    meter: &mut ProofMeter,
) -> AdmissionResult<bool> {
    meter.charge(1)?;
    let AppDefinition {
        version: _,
        id: _,
        label: _,
        entities,
        state: _,
        actions,
        outputs: _,
        observables,
        views,
        initial_view: _,
    } = app;
    meter.charge(1)?;
    let mut entities = entities.iter();
    loop {
        meter.charge(1)?;
        let Some(entity) = entities.next() else { break };
        meter.charge(1)?;
        let EntityDefinition {
            id: _,
            label: _,
            fields: _,
            unique: _,
            constraints,
        } = entity;
        if expressions(constraints, state, meter)? {
            return Ok(true);
        }
    }
    meter.charge(1)?;
    let mut actions = actions.iter();
    loop {
        meter.charge(1)?;
        let Some(action) = actions.next() else { break };
        meter.charge(1)?;
        let ActionDefinition {
            id: _,
            label: _,
            parameters: _,
            guards,
            steps,
            ensures,
        } = action;
        if expressions(guards, state, meter)?
            || statements(steps, state, meter)?
            || expressions(ensures, state, meter)?
        {
            return Ok(true);
        }
    }
    meter.charge(1)?;
    let mut observables = observables.iter();
    loop {
        meter.charge(1)?;
        let Some(observable) = observables.next() else {
            break;
        };
        meter.charge(1)?;
        let ObservableDefinition {
            id: _,
            label: _,
            value,
        } = observable;
        if expression(value, state, meter)? {
            return Ok(true);
        }
    }
    meter.charge(1)?;
    let mut views = views.iter();
    loop {
        meter.charge(1)?;
        let Some(view) = views.next() else { break };
        meter.charge(1)?;
        let ViewDefinition {
            id: _,
            label: _,
            kind,
            actions,
            keys: _,
        } = view;
        match kind {
            ViewKind::List {
                entity: _,
                rows,
                columns: values,
                controls: _,
                selection: _,
            } => {
                if expression(rows, state, meter)? || columns(values, state, meter)? {
                    return Ok(true);
                }
            }
            ViewKind::Detail {
                entity: _,
                record,
                columns: values,
            } => {
                if expression(record, state, meter)? || columns(values, state, meter)? {
                    return Ok(true);
                }
            }
            ViewKind::Form {
                action: _,
                fields: _,
                defaults: _,
            } => {}
        }
        meter.charge(1)?;
        let mut bindings = actions.iter();
        loop {
            meter.charge(1)?;
            let Some(binding) = bindings.next() else {
                break;
            };
            meter.charge(1)?;
            let ActionBinding {
                id: _,
                label: _,
                placement: _,
                action: _,
                arguments,
                enabled,
            } = binding;
            if values(arguments, state, meter)? || expression(enabled, state, meter)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn expressions(items: &[Expr], state: &str, meter: &mut ProofMeter) -> AdmissionResult<bool> {
    meter.charge(1)?;
    let mut items = items.iter();
    loop {
        meter.charge(1)?;
        let Some(item) = items.next() else {
            return Ok(false);
        };
        if expression(item, state, meter)? {
            return Ok(true);
        }
    }
}

fn values(
    items: &BTreeMap<Id, Expr>,
    state: &str,
    meter: &mut ProofMeter,
) -> AdmissionResult<bool> {
    meter.charge(1)?;
    let mut items = items.values();
    loop {
        meter.charge(1)?;
        let Some(item) = items.next() else {
            return Ok(false);
        };
        if expression(item, state, meter)? {
            return Ok(true);
        }
    }
}

fn columns(items: &[Column], state: &str, meter: &mut ProofMeter) -> AdmissionResult<bool> {
    meter.charge(1)?;
    let mut items = items.iter();
    loop {
        meter.charge(1)?;
        let Some(item) = items.next() else {
            return Ok(false);
        };
        meter.charge(1)?;
        let Column {
            id: _,
            label: _,
            value,
        } = item;
        if expression(value, state, meter)? {
            return Ok(true);
        }
    }
}

fn statements(items: &[Statement], state: &str, meter: &mut ProofMeter) -> AdmissionResult<bool> {
    meter.charge(1)?;
    let mut items = items.iter();
    loop {
        meter.charge(1)?;
        let Some(item) = items.next() else {
            return Ok(false);
        };
        if statement(item, state, meter)? {
            return Ok(true);
        }
    }
}

fn statement(item: &Statement, state: &str, meter: &mut ProofMeter) -> AdmissionResult<bool> {
    meter.charge(1)?;
    match item {
        Statement::Create {
            entity: _,
            values: items,
            bind: _,
        } => values(items, state, meter),
        Statement::Update {
            record,
            values: items,
        } => {
            if expression(record, state, meter)? {
                return Ok(true);
            }
            values(items, state, meter)
        }
        Statement::Archive { record } => expression(record, state, meter),
        Statement::SetState { state: _, value } => expression(value, state, meter),
        Statement::Collection {
            target,
            operation: _,
            items,
        } => {
            match target {
                CollectionTarget::State { state: _ } => {}
                CollectionTarget::Field { record, field: _ } => {
                    if expression(record, state, meter)? {
                        return Ok(true);
                    }
                }
            }
            expression(items, state, meter)
        }
        Statement::ForEach {
            items,
            binding: _,
            steps,
        } => {
            if expression(items, state, meter)? {
                return Ok(true);
            }
            statements(steps, state, meter)
        }
        Statement::Assert {
            condition,
            message: _,
        } => expression(condition, state, meter),
        Statement::Emit {
            output: _,
            items,
            binding: _,
            columns,
        } => {
            if expression(items, state, meter)? {
                return Ok(true);
            }
            values(columns, state, meter)
        }
    }
}

fn expression(item: &Expr, state: &str, meter: &mut ProofMeter) -> AdmissionResult<bool> {
    meter.charge(1)?;
    match item {
        Expr::State { state: read } => {
            let bytes = read
                .len()
                .checked_add(state.len())
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| refuse("budget", "state comparison byte count overflow"))?;
            meter.charge(bytes)?;
            Ok(read == state)
        }
        Expr::Literal {
            value_type: _,
            value: _,
        }
        | Expr::Variable { name: _ }
        | Expr::Today => Ok(false),
        Expr::Field { record, field: _ } => expression(record, state, meter),
        Expr::Query {
            entity: _,
            binding: _,
            predicate,
            sort,
            limit: _,
            include_archived: _,
        } => {
            if expression(predicate, state, meter)? {
                return Ok(true);
            }
            meter.charge(1)?;
            let mut sort = sort.iter();
            loop {
                meter.charge(1)?;
                let Some(key) = sort.next() else {
                    return Ok(false);
                };
                meter.charge(1)?;
                let SortKey {
                    value,
                    descending: _,
                } = key;
                if expression(value, state, meter)? {
                    return Ok(true);
                }
            }
        }
        Expr::Filter {
            items,
            binding: _,
            predicate,
        }
        | Expr::Any {
            items,
            binding: _,
            predicate,
        }
        | Expr::All {
            items,
            binding: _,
            predicate,
        } => {
            if expression(items, state, meter)? {
                return Ok(true);
            }
            expression(predicate, state, meter)
        }
        Expr::Map {
            items,
            binding: _,
            value,
        }
        | Expr::Contains { items, value } => {
            if expression(items, state, meter)? {
                return Ok(true);
            }
            expression(value, state, meter)
        }
        Expr::Count { items } | Expr::Sum { items } => expression(items, state, meter),
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            if expression(left, state, meter)? {
                return Ok(true);
            }
            expression(right, state, meter)
        }
        Expr::TextContains { text, search } => {
            if expression(text, state, meter)? {
                return Ok(true);
            }
            expression(search, state, meter)
        }
        Expr::Lower { value } | Expr::Not { value } => expression(value, state, meter),
        Expr::And { values } | Expr::Or { values } => expressions(values, state, meter),
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            if expression(condition, state, meter)? || expression(then_value, state, meter)? {
                return Ok(true);
            }
            expression(else_value, state, meter)
        }
        Expr::Coalesce { value, fallback } => {
            if expression(value, state, meter)? {
                return Ok(true);
            }
            expression(fallback, state, meter)
        }
        Expr::DateAdd { date, days } => {
            if expression(date, state, meter)? {
                return Ok(true);
            }
            expression(days, state, meter)
        }
        Expr::DateDifference { later, earlier } => {
            if expression(later, state, meter)? {
                return Ok(true);
            }
            expression(earlier, state, meter)
        }
    }
}
