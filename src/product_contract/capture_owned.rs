//! Borrowed ownership descriptors for the closed capture source types.
use super::bounded_input::capture_payment_core as native;
use super::capture_work::Audit;
use super::*;
use std::mem::size_of;

pub(super) trait OwnedWork {
    fn pay_owned(&self, audit: Audit<'_>) -> Result<()>;
}
macro_rules! scalar { ($($t:ty),*) => {$ (
    impl OwnedWork for $t { fn pay_owned(&self, a: Audit<'_>) -> Result<()> { a.pay(1) } }
)*}; }
scalar!(
    bool,
    u32,
    u64,
    i32,
    i64,
    usize,
    CollectionOperation,
    OutputFormat,
    ActionPlacement,
    KeyModifier
);
impl OwnedWork for String {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        a.string(self.len())
    }
}
impl<T: OwnedWork> OwnedWork for Box<T> {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        a.pay(3)?;
        a.boxed::<T>()?;
        self.as_ref().pay_owned(a)
    }
}
impl<T: OwnedWork> OwnedWork for Option<T> {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        a.pay(2)?;
        if let Some(value) = self {
            a.pay(2)?;
            value.pay_owned(a)?;
        }
        Ok(())
    }
}
impl<T: OwnedWork> OwnedWork for Vec<T> {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(12)?;
        let n = a.usize(self.len())?;
        let s = a.usize(size_of::<T>())?;
        a.amount(
            n.checked_mul(s)
                .and_then(|x| x.checked_mul(2))
                .and_then(|x| x.checked_add(n.checked_mul(9)?))
                .and_then(|x| x.checked_add(4)),
        )?;
        a.slice(self.len())?;
        for value in self {
            value.pay_owned(a)?;
        }
        Ok(())
    }
}
impl<T: OwnedWork> OwnedWork for BTreeMap<String, T> {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.map_iter(self.len())?;
        for (key, value) in self {
            a.pay(4)?;
            key.pay_owned(a)?;
            value.pay_owned(a)?;
        }
        a.pay(native::CLONE_DESCRIPTOR)?;
        a.amount(native::btree_clone::<String, T>(self.len(), 0))
    }
}
impl OwnedWork for BTreeSet<KeyModifier> {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.map_iter(self.len())?;
        a.scaled(self.len(), 1, 0)?;
        a.pay(native::CLONE_DESCRIPTOR)?;
        a.amount(native::btree_clone::<KeyModifier, ()>(self.len(), 0))
    }
}
impl<'a> Audit<'a> {
    pub(super) fn clone_owned<T: OwnedWork + Clone>(self, value: &T) -> Result<T> {
        if self.enabled() {
            self.pay(1)?;
            value.pay_owned(self)?;
        }
        Ok(value.clone())
    }
    pub(super) fn equal_owned<T: OwnedWork + PartialEq>(self, left: &T, right: &T) -> Result<bool> {
        if self.enabled() {
            self.pay(1)?;
            left.pay_owned(self)?;
        }
        // A validated left bounds all reached right descendants: derived Eq
        // tests variants/lengths before children; no retained-right descriptor.
        Ok(left == right)
    }
}
impl OwnedWork for Type {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            Type::Boolean => {}
            Type::Integer => {}
            Type::Text => {}
            Type::Date => {}
            Type::Reference { entity } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
            }
            Type::List { item } => {
                a.pay(2)?;
                item.pay_owned(a)?;
            }
            Type::Optional { item } => {
                a.pay(2)?;
                item.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for DataValue {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            DataValue::Null => {}
            DataValue::Boolean { value } => {
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            DataValue::Integer { value } => {
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            DataValue::Text { value } => {
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            DataValue::Date { days } => {
                a.pay(2)?;
                days.pay_owned(a)?;
            }
            DataValue::Reference { entity, record } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
                a.pay(2)?;
                record.pay_owned(a)?;
            }
            DataValue::List { item_type, items } => {
                a.pay(2)?;
                item_type.pay_owned(a)?;
                a.pay(2)?;
                items.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for Expr {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            Expr::Literal { value_type, value } => {
                a.pay(2)?;
                value_type.pay_owned(a)?;
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Expr::Variable { name } => {
                a.pay(2)?;
                name.pay_owned(a)?;
            }
            Expr::Field { record, field } => {
                a.pay(2)?;
                record.pay_owned(a)?;
                a.pay(2)?;
                field.pay_owned(a)?;
            }
            Expr::State { state } => {
                a.pay(2)?;
                state.pay_owned(a)?;
            }
            Expr::Today => {}
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                include_archived,
            } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                predicate.pay_owned(a)?;
                a.pay(2)?;
                sort.pay_owned(a)?;
                a.pay(2)?;
                limit.pay_owned(a)?;
                a.pay(2)?;
                include_archived.pay_owned(a)?;
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                predicate.pay_owned(a)?;
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Expr::Count { items } => {
                a.pay(2)?;
                items.pay_owned(a)?;
            }
            Expr::Any {
                items,
                binding,
                predicate,
            } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                predicate.pay_owned(a)?;
            }
            Expr::All {
                items,
                binding,
                predicate,
            } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                predicate.pay_owned(a)?;
            }
            Expr::Sum { items } => {
                a.pay(2)?;
                items.pay_owned(a)?;
            }
            Expr::Contains { items, value } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Expr::Equal { left, right } => {
                a.pay(2)?;
                left.pay_owned(a)?;
                a.pay(2)?;
                right.pay_owned(a)?;
            }
            Expr::Less { left, right } => {
                a.pay(2)?;
                left.pay_owned(a)?;
                a.pay(2)?;
                right.pay_owned(a)?;
            }
            Expr::Add { left, right } => {
                a.pay(2)?;
                left.pay_owned(a)?;
                a.pay(2)?;
                right.pay_owned(a)?;
            }
            Expr::Subtract { left, right } => {
                a.pay(2)?;
                left.pay_owned(a)?;
                a.pay(2)?;
                right.pay_owned(a)?;
            }
            Expr::Concat { left, right } => {
                a.pay(2)?;
                left.pay_owned(a)?;
                a.pay(2)?;
                right.pay_owned(a)?;
            }
            Expr::TextContains { text, search } => {
                a.pay(2)?;
                text.pay_owned(a)?;
                a.pay(2)?;
                search.pay_owned(a)?;
            }
            Expr::Lower { value } => {
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Expr::And { values } => {
                a.pay(2)?;
                values.pay_owned(a)?;
            }
            Expr::Or { values } => {
                a.pay(2)?;
                values.pay_owned(a)?;
            }
            Expr::Not { value } => {
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                a.pay(2)?;
                condition.pay_owned(a)?;
                a.pay(2)?;
                then_value.pay_owned(a)?;
                a.pay(2)?;
                else_value.pay_owned(a)?;
            }
            Expr::Coalesce { value, fallback } => {
                a.pay(2)?;
                value.pay_owned(a)?;
                a.pay(2)?;
                fallback.pay_owned(a)?;
            }
            Expr::DateAdd { date, days } => {
                a.pay(2)?;
                date.pay_owned(a)?;
                a.pay(2)?;
                days.pay_owned(a)?;
            }
            Expr::DateDifference { later, earlier } => {
                a.pay(2)?;
                later.pay_owned(a)?;
                a.pay(2)?;
                earlier.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for SortKey {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.value.pay_owned(a)?;
        a.pay(2)?;
        self.descending.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for FieldDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.value_type.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for EntityDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.fields.pay_owned(a)?;
        a.pay(2)?;
        self.unique.pay_owned(a)?;
        a.pay(2)?;
        self.constraints.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for StateDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.value_type.pay_owned(a)?;
        a.pay(2)?;
        self.initial.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for ActionDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.parameters.pay_owned(a)?;
        a.pay(2)?;
        self.guards.pay_owned(a)?;
        a.pay(2)?;
        self.steps.pay_owned(a)?;
        a.pay(2)?;
        self.ensures.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for Statement {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            Statement::Create {
                entity,
                values,
                bind,
            } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
                a.pay(2)?;
                values.pay_owned(a)?;
                a.pay(2)?;
                bind.pay_owned(a)?;
            }
            Statement::Update { record, values } => {
                a.pay(2)?;
                record.pay_owned(a)?;
                a.pay(2)?;
                values.pay_owned(a)?;
            }
            Statement::Archive { record } => {
                a.pay(2)?;
                record.pay_owned(a)?;
            }
            Statement::SetState { state, value } => {
                a.pay(2)?;
                state.pay_owned(a)?;
                a.pay(2)?;
                value.pay_owned(a)?;
            }
            Statement::Collection {
                target,
                operation,
                items,
            } => {
                a.pay(2)?;
                target.pay_owned(a)?;
                a.pay(2)?;
                operation.pay_owned(a)?;
                a.pay(2)?;
                items.pay_owned(a)?;
            }
            Statement::ForEach {
                items,
                binding,
                steps,
            } => {
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                steps.pay_owned(a)?;
            }
            Statement::Assert { condition, message } => {
                a.pay(2)?;
                condition.pay_owned(a)?;
                a.pay(2)?;
                message.pay_owned(a)?;
            }
            Statement::Emit {
                output,
                items,
                binding,
                columns,
            } => {
                a.pay(2)?;
                output.pay_owned(a)?;
                a.pay(2)?;
                items.pay_owned(a)?;
                a.pay(2)?;
                binding.pay_owned(a)?;
                a.pay(2)?;
                columns.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for CollectionTarget {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            CollectionTarget::State { state } => {
                a.pay(2)?;
                state.pay_owned(a)?;
            }
            CollectionTarget::Field { record, field } => {
                a.pay(2)?;
                record.pay_owned(a)?;
                a.pay(2)?;
                field.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for OutputDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.format.pay_owned(a)?;
        a.pay(2)?;
        self.columns.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for ObservableDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.value.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for Column {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.value.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for StateControl {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.state.pay_owned(a)?;
        a.pay(2)?;
        self.on_change.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for SelectionBinding {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.state.pay_owned(a)?;
        a.pay(2)?;
        self.on_change.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for FormInput {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.parameter.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for ViewKind {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(2)?;
        match self {
            ViewKind::List {
                entity,
                rows,
                columns,
                controls,
                selection,
            } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
                a.pay(2)?;
                rows.pay_owned(a)?;
                a.pay(2)?;
                columns.pay_owned(a)?;
                a.pay(2)?;
                controls.pay_owned(a)?;
                a.pay(2)?;
                selection.pay_owned(a)?;
            }
            ViewKind::Detail {
                entity,
                record,
                columns,
            } => {
                a.pay(2)?;
                entity.pay_owned(a)?;
                a.pay(2)?;
                record.pay_owned(a)?;
                a.pay(2)?;
                columns.pay_owned(a)?;
            }
            ViewKind::Form {
                action,
                fields,
                defaults,
            } => {
                a.pay(2)?;
                action.pay_owned(a)?;
                a.pay(2)?;
                fields.pay_owned(a)?;
                a.pay(2)?;
                defaults.pay_owned(a)?;
            }
        }
        Ok(())
    }
}
impl OwnedWork for ActionBinding {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.placement.pay_owned(a)?;
        a.pay(2)?;
        self.action.pay_owned(a)?;
        a.pay(2)?;
        self.arguments.pay_owned(a)?;
        a.pay(2)?;
        self.enabled.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for KeyBinding {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.key.pay_owned(a)?;
        a.pay(2)?;
        self.modifiers.pay_owned(a)?;
        a.pay(2)?;
        self.binding.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for ViewDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.kind.pay_owned(a)?;
        a.pay(2)?;
        self.actions.pay_owned(a)?;
        a.pay(2)?;
        self.keys.pay_owned(a)?;
        Ok(())
    }
}
impl OwnedWork for AppDefinition {
    fn pay_owned(&self, a: Audit<'_>) -> Result<()> {
        if !a.enabled() {
            return Ok(());
        }
        a.pay(1)?;
        a.pay(2)?;
        self.version.pay_owned(a)?;
        a.pay(2)?;
        self.id.pay_owned(a)?;
        a.pay(2)?;
        self.label.pay_owned(a)?;
        a.pay(2)?;
        self.entities.pay_owned(a)?;
        a.pay(2)?;
        self.state.pay_owned(a)?;
        a.pay(2)?;
        self.actions.pay_owned(a)?;
        a.pay(2)?;
        self.outputs.pay_owned(a)?;
        a.pay(2)?;
        self.observables.pay_owned(a)?;
        a.pay(2)?;
        self.views.pay_owned(a)?;
        a.pay(2)?;
        self.initial_view.pay_owned(a)?;
        Ok(())
    }
}
