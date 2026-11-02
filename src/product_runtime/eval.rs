use super::*;
use std::cmp::Ordering as Compare;
use super::owned_env::ManagedEnv;
use super::product_alloc::AllocationContext;
use super::query_order::{try_sort_candidate_ids, try_sort_rows, QueryOrderError};

pub(super) type Env = BTreeMap<Id, (Type, DataValue)>;
pub(super) trait EnvAccess {
    fn len(&self) -> usize;
    fn get(&self, name: &str) -> Option<(&Type, &DataValue)>;
    fn visit(&self, visit: &mut dyn FnMut(&str, &Type, &DataValue) -> Result<()>) -> Result<()>;
    fn try_managed_copy(&self, allocations: &mut AllocationContext)
        -> std::result::Result<ManagedEnv, super::product_alloc::AllocationFailure>;
}
impl EnvAccess for Env {
    fn len(&self) -> usize { BTreeMap::len(self) }
    fn get(&self, name: &str) -> Option<(&Type, &DataValue)> {
        BTreeMap::get(self, name).map(|(ty, value)| (ty, value))
    }
    fn visit(&self, visit: &mut dyn FnMut(&str, &Type, &DataValue) -> Result<()>) -> Result<()> {
        for (key, (ty, value)) in self { visit(key, ty, value)?; }
        Ok(())
    }
    fn try_managed_copy(&self, allocations: &mut AllocationContext)
        -> std::result::Result<ManagedEnv, super::product_alloc::AllocationFailure> {
        ManagedEnv::try_from_iter(
            self.iter().map(|(key, (ty, value))| (key.as_str(), ty, value)),
            allocations,
        )
    }
}
impl EnvAccess for ManagedEnv {
    fn len(&self) -> usize { ManagedEnv::len(self) }
    fn get(&self, name: &str) -> Option<(&Type, &DataValue)> { ManagedEnv::get(self, name) }
    fn visit(&self, visit: &mut dyn FnMut(&str, &Type, &DataValue) -> Result<()>) -> Result<()> {
        for (key, ty, value) in self.iter() { visit(key, ty, value)?; }
        Ok(())
    }
    fn try_managed_copy(&self, allocations: &mut AllocationContext)
        -> std::result::Result<ManagedEnv, super::product_alloc::AllocationFailure> {
        ManagedEnv::try_copy(self, allocations)
    }
}
pub(super) struct Eval<'a> {
    pub app: AppRef<'a>,
    pub data: DataSnapshotView<'a>,
    pub session: SessionStateView<'a>,
    pub day: i32,
    pub meter: &'a mut Meter,
}
impl<'a> Eval<'a> {
    pub(super) fn legacy(
        app: &'a AppDefinition,
        data: &'a DataSnapshot,
        session: &'a SessionState,
        day: i32,
        meter: &'a mut Meter,
    ) -> Self {
        Self { app: AppRef::from(app), data: DataSnapshotView::from(data),
            session: SessionStateView::from(session), day, meter }
    }
    pub(super) fn fallible(
        app: &'a crate::product_contract::fallible_app_decode::DecodedAppDefinition,
        data: &'a FallibleDecodedDataSnapshot,
        session: &'a FallibleDecodedSessionState,
        day: i32,
        meter: &'a mut Meter,
    ) -> Self {
        Self::fallible_view(app, DataSnapshotView::from(data), session, day, meter)
    }
    pub(super) fn fallible_view(
        app: &'a crate::product_contract::fallible_app_decode::DecodedAppDefinition,
        data: DataSnapshotView<'a>,
        session: &'a FallibleDecodedSessionState,
        day: i32,
        meter: &'a mut Meter,
    ) -> Self {
        Self { app: AppRef::from(app), data,
            session: SessionStateView::from(session), day, meter }
    }
    pub(super) fn view(app: AppRef<'a>, data: DataSnapshotView<'a>, session: SessionStateView<'a>, day: i32, meter: &'a mut Meter) -> Self {
        Self { app, data, session, day, meter }
    }
    fn try_clone_value(&mut self, value: &DataValue) -> Result<DataValue> {
        if self.app.is_fallible() {
            crate::product_contract::try_clone_data_value_with(value, &mut self.meter.allocations)
                .map_err(AdapterError::Allocation)
        } else { Ok(value.clone()) }
    }
    fn try_clone_type(&mut self, value: &Type) -> Result<Type> {
        if self.app.is_fallible() {
            crate::product_contract::try_clone_type_with(value, &mut self.meter.allocations)
                .map_err(AdapterError::Allocation)
        } else { Ok(value.clone()) }
    }
    fn try_list_type(&mut self, item: Type) -> Result<Type> {
        if self.app.is_fallible() {
            let item = super::product_alloc::try_box(item, &mut self.meter.allocations)
                .map_err(AdapterError::Allocation)?;
            Ok(Type::List { item })
        } else { Ok(Type::list(item)) }
    }
    fn try_reference_type(&mut self, entity: &str) -> Result<Type> {
        if self.app.is_fallible() {
            let entity = super::product_alloc::try_copy_str(entity, &mut self.meter.allocations,
                super::product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?;
            Ok(Type::Reference { entity })
        } else { Ok(Type::reference(entity)) }
    }
    fn reference_value(&mut self, record: DataRecordView<'_>) -> Result<DataValue> {
        if self.app.is_fallible() {
            let entity = super::product_alloc::try_copy_str(record.entity(), &mut self.meter.allocations,
                super::product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?;
            let id = super::product_alloc::try_copy_str(record.id(), &mut self.meter.allocations,
                super::product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?;
            Ok(DataValue::Reference { entity, record: id })
        } else { Ok(value_ref_view(record)) }
    }
    fn try_push<T>(&mut self, values: &mut Vec<T>, value: T) -> Result<()> {
        if self.app.is_fallible() {
            super::product_alloc::try_push(values, value, &mut self.meter.allocations,
                super::product_alloc::AllocationSite::Vector).map_err(AdapterError::Allocation)
        } else { values.push(value); Ok(()) }
    }
}

pub(super) fn boolean(value: DataValue) -> Result<bool> {
    match value {
        DataValue::Boolean { value } => Ok(value),
        _ => Err(failed("expected boolean")),
    }
}
pub(super) fn integer(value: DataValue) -> Result<i64> {
    match value {
        DataValue::Integer { value } => Ok(value),
        _ => Err(failed("expected integer")),
    }
}
pub(super) fn text(value: DataValue) -> Result<String> {
    match value {
        DataValue::Text { value } => Ok(value),
        _ => Err(failed("expected text")),
    }
}
pub(super) fn day(value: DataValue) -> Result<i32> {
    match value {
        DataValue::Date { days } => Ok(days),
        _ => Err(failed("expected date")),
    }
}
pub(super) fn list(value: DataValue) -> Result<(Type, Vec<DataValue>)> {
    match value {
        DataValue::List { item_type, items } => Ok((item_type, items)),
        _ => Err(failed("expected list")),
    }
}
pub(super) fn reference(value: &DataValue) -> Result<RecordRef> {
    match value {
        DataValue::Reference { entity, record } => Ok(RecordRef {
            entity: entity.clone(),
            record: record.clone(),
        }),
        _ => Err(failed("expected record reference")),
    }
}
pub(super) fn record<'a>(data: &'a DataSnapshot, value: &DataValue) -> Result<&'a Record> {
    let key = reference(value)?;
    data.records
        .iter()
        .find(|r| r.entity == key.entity && r.id == key.record)
        .ok_or_else(|| invalid("record reference does not exist"))
}
pub(super) fn record_view<'a>(data: &'a DataSnapshotView<'a>, value: &DataValue) -> Result<DataRecordView<'a>> {
    let DataValue::Reference { entity, record } = value else { return Err(failed("expected record reference")); };
    data.record(entity, record).ok_or_else(|| invalid("record reference does not exist"))
}
pub(super) fn compare(a: &DataValue, b: &DataValue) -> Compare {
    match (a, b) {
        (DataValue::Boolean { value: a }, DataValue::Boolean { value: b }) => a.cmp(b),
        (DataValue::Integer { value: a }, DataValue::Integer { value: b }) => a.cmp(b),
        (DataValue::Text { value: a }, DataValue::Text { value: b }) => a.cmp(b),
        (DataValue::Date { days: a }, DataValue::Date { days: b }) => a.cmp(b),
        (
            DataValue::Reference {
                entity: a,
                record: x,
            },
            DataValue::Reference {
                entity: b,
                record: y,
            },
        ) => (a, x).cmp(&(b, y)),
        _ => Compare::Equal, // Static validation disallows incomparable operands.
    }
}
pub(super) fn value_ref(r: &Record) -> DataValue {
    DataValue::Reference {
        entity: r.entity.clone(),
        record: r.id.clone(),
    }
}
pub(super) fn value_ref_view(r: DataRecordView<'_>) -> DataValue {
    DataValue::Reference { entity: r.entity().to_owned(), record: r.id().to_owned() }
}

fn query_order_error(error: QueryOrderError, fallible: bool) -> AdapterError {
    match error {
        QueryOrderError::Allocation(failure) if fallible => AdapterError::Allocation(failure),
        QueryOrderError::Allocation(_) => failed("query ordering allocation failed"),
        QueryOrderError::MissingKeySlots { .. } => failed("query ordering key slots missing"),
    }
}
fn binding_error(failure: super::product_alloc::AllocationFailure, fallible: bool) -> AdapterError {
    if fallible { AdapterError::Allocation(failure) } else { failed("expression binding allocation failed") }
}

impl Eval<'_> {
    fn literal_references(&mut self, value: &DataValue) -> Result<()> {
        self.meter.tick(1)?;
        match value {
            DataValue::Reference { .. } => {
                self.meter.tick(self.data.record_count() as u64)?;
                record_view(&self.data, value)?;
            }
            DataValue::List { items, .. } => {
                for value in items {
                    self.literal_references(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Static result types are needed even when map/filter receives no values.
    pub(super) fn typ(&mut self, expr: &Expr, env: &dyn EnvAccess) -> Result<Type> {
        if self.meter.row.is_some() { return self.typ_paid(expr, env); }
        self.meter.tick(1)?;
        Ok(match expr {
            Expr::Literal { value_type, .. } => self.try_clone_type(value_type)?,
            Expr::Variable { name } => {
                let value = env.get(name)
                    .ok_or_else(|| failed("missing variable type"))?.0;
                self.try_clone_type(value)?
            }
            Expr::State { state } => {
                let app = self.app;
                let value = app.states()
                .iter()
                .find(|s| &s.id == state)
                .ok_or_else(|| failed("missing state type"))?;
                self.try_clone_type(&value.value_type)?
            }
            Expr::Field { record, field } => {
                let Type::Reference { entity } = self.typ(record, env)? else {
                    return Err(failed("invalid field type"));
                };
                let app = self.app;
                let value = app
                    .entities()
                    .iter()
                    .find(|e| e.id == entity)
                    .and_then(|e| e.fields.iter().find(|f| &f.id == field))
                    .ok_or_else(|| failed("missing field type"))?;
                self.try_clone_type(&value.value_type)?
            }
            Expr::Query { entity, .. } => {
                let reference = self.try_reference_type(entity)?;
                self.try_list_type(reference)?
            }
            Expr::Filter { items, .. } => self.typ(items, env)?,
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let Type::List { item } = self.typ(items, env)? else {
                    return Err(failed("invalid map type"));
                };
                let fallible = self.app.is_fallible();
                let mut local = env.try_managed_copy(&mut self.meter.allocations)
                    .map_err(|failure| binding_error(failure, fallible))?;
                let type_lease = local.try_bind_leased(binding, &item, &DataValue::Null, &mut self.meter.allocations)
                    .map_err(|failure| binding_error(failure, fallible))?;
                let result_type = self.typ(value, &local)?;
                drop(type_lease);
                self.try_list_type(result_type)?
            }
            Expr::If { then_value, .. } => self.typ(then_value, env)?,
            Expr::Coalesce { fallback, .. } => self.typ(fallback, env)?,
            Expr::Today | Expr::DateAdd { .. } => Type::Date,
            Expr::Count { .. }
            | Expr::Sum { .. }
            | Expr::Add { .. }
            | Expr::Subtract { .. }
            | Expr::DateDifference { .. } => Type::Integer,
            Expr::Concat { .. } | Expr::Lower { .. } => Type::Text,
            _ => Type::Boolean,
        })
    }
    pub fn eval(&mut self, expr: &Expr, env: &dyn EnvAccess) -> Result<DataValue> {
        if self.meter.row.is_some() { return self.eval_paid(expr, env); }
        self.meter.tick(1)?;
        let value = match expr {
            Expr::Literal { value, .. } => {
                self.meter.value(value)?;
                self.literal_references(value)?;
                self.try_clone_value(value)?
            }
            Expr::Variable { name } => {
                let value = &env.get(name).ok_or_else(|| failed("unbound variable"))?.1;
                self.meter.value(value)?;
                self.try_clone_value(value)?
            }
            Expr::Field {
                record: expr,
                field,
            } => {
                let key = self.eval(expr, env)?;
                self.meter.tick(self.data.record_count() as u64)?;
                let data = self.data;
                let row = record_view(&data, &key)?;
                let found = row.values().get(field);
                let value = match found { Some(value) => self.try_clone_value(value)?, None => DataValue::Null };
                self.meter.value(&value)?;
                value
            }
            Expr::State { state } => {
                let session = self.session;
                let values = session.values();
                let found = values
                    .get(state)
                    .ok_or_else(|| failed("missing session value"))?;
                self.meter.value(found)?;
                self.try_clone_value(found)?
            }
            Expr::Today => DataValue::Date { days: self.day },
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                include_archived,
            } => {
                let mut candidates = Vec::new();
                let data = self.data;
                for record in data.records() {
                    self.meter.tick(1)?;
                    if record.entity() == entity && (!record.archived() || *include_archived) {
                        self.try_push(&mut candidates, record)?;
                    }
                }
                self.meter.collection(candidates.len())?;
                let comparisons = candidates.len().saturating_mul(
                    (usize::BITS - candidates.len().max(1).leading_zeros()) as usize,
                );
                self.meter.tick(comparisons as u64)?;
                try_sort_candidate_ids(&mut candidates, &mut self.meter.allocations)
                    .map_err(|error| query_order_error(QueryOrderError::Allocation(error), self.app.is_fallible()))?;
                // Predicate and key failures must follow ID order too, not just
                // successful result rows after the final explicit-key sort.
                let mut rows = Vec::new();
                for record in candidates {
                    self.meter.tick(1)?;
                    let value = self.reference_value(record)?;
                    let fallible = self.app.is_fallible();
                    let mut local = env.try_managed_copy(&mut self.meter.allocations)
                        .map_err(|failure| binding_error(failure, fallible))?;
                    local.try_bind_reference(binding, entity, record.entity(), record.id(), &mut self.meter.allocations)
                        .map_err(|failure| binding_error(failure, fallible))?;
                    if boolean(self.eval(predicate, &local)?)? {
                        let mut keys = Vec::new();
                        for key in sort { let value = self.eval(&key.value, &local)?; self.try_push(&mut keys, value)?; }
                        let record_id = if self.app.is_fallible() {
                            super::product_alloc::try_copy_str(record.id(), &mut self.meter.allocations,
                                super::product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?
                        } else { record.id().to_owned() };
                        self.try_push(&mut rows, (record_id, keys, value))?;
                        self.meter.collection(rows.len())?;
                    }
                }
                // Charge a deterministic upper bound before the bounded stable sort.
                let comparisons = rows
                    .len()
                    .saturating_mul((usize::BITS - rows.len().max(1).leading_zeros()) as usize)
                    .saturating_mul(sort.len().max(1));
                self.meter.tick(comparisons as u64)?;
                try_sort_rows(&mut rows, sort, &mut self.meter.allocations)
                    .map_err(|error| query_order_error(error, self.app.is_fallible()))?;
                self.meter.tick(1)?;
                let item_type = self.try_reference_type(entity)?;
                let mut items = Vec::new();
                for (_, _, value) in rows.into_iter().take(*limit) { self.try_push(&mut items, value)?; }
                DataValue::List { item_type, items }
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
                let (typ, items) = list(self.eval(items, env)?)?;
                let mut kept = Vec::new();
                let all = matches!(expr, Expr::All { .. });
                let any = matches!(expr, Expr::Any { .. });
                let mut result = all;
                for item in items {
                    self.meter.tick(1)?;
                    let fallible = self.app.is_fallible();
                    let mut local = env.try_managed_copy(&mut self.meter.allocations)
                        .map_err(|failure| binding_error(failure, fallible))?;
                    local.try_bind(binding, &typ, &item, &mut self.meter.allocations)
                        .map_err(|failure| binding_error(failure, fallible))?;
                    let yes = boolean(self.eval(predicate, &local)?)?;
                    if any && yes {
                        result = true;
                        break;
                    }
                    if all && !yes {
                        result = false;
                        break;
                    }
                    if yes {
                        self.try_push(&mut kept, item)?;
                    }
                }
                if all || any {
                    DataValue::Boolean { value: result }
                } else {
                    DataValue::List {
                        item_type: typ,
                        items: kept,
                    }
                }
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let (typ, items) = list(self.eval(items, env)?)?;
                let fallible = self.app.is_fallible();
                let mut local = env.try_managed_copy(&mut self.meter.allocations)
                    .map_err(|failure| binding_error(failure, fallible))?;
                let type_lease = local.try_bind_leased(binding, &typ, &DataValue::Null, &mut self.meter.allocations)
                    .map_err(|failure| binding_error(failure, fallible))?;
                let result_type = self.typ(value, &local)?;
                drop(type_lease);
                let mut mapped = Vec::new();
                for item in items {
                    self.meter.tick(1)?;
                    let _lease = local.try_bind_leased(binding, &typ, &item, &mut self.meter.allocations)
                        .map_err(|failure| binding_error(failure, fallible))?;
                    let mapped_value = self.eval(value, &local)?;
                    self.try_push(&mut mapped, mapped_value)?;
                }
                DataValue::List {
                    item_type: result_type,
                    items: mapped,
                }
            }
            Expr::Count { items } => DataValue::Integer {
                value: list(self.eval(items, env)?)?.1.len() as i64,
            },
            Expr::Sum { items } => {
                let mut sum = 0i64;
                for value in list(self.eval(items, env)?)?.1 {
                    self.meter.tick(1)?;
                    sum = sum
                        .checked_add(integer(value)?)
                        .ok_or_else(|| failed("integer overflow"))?;
                }
                DataValue::Integer { value: sum }
            }
            Expr::Contains { items, value } => {
                let (_, items) = list(self.eval(items, env)?)?;
                let value = self.eval(value, env)?;
                self.meter.tick(items.len() as u64)?;
                DataValue::Boolean {
                    value: items.contains(&value),
                }
            }
            Expr::Equal { left, right } => {
                let left = self.eval(left, env)?;
                let right = self.eval(right, env)?;
                self.meter.value(&left)?;
                self.meter.value(&right)?;
                DataValue::Boolean {
                    value: left == right,
                }
            }
            Expr::Less { left, right } => {
                let left = self.eval(left, env)?;
                let right = self.eval(right, env)?;
                DataValue::Boolean {
                    value: compare(&left, &right) == Compare::Less,
                }
            }
            Expr::Add { left, right } | Expr::Subtract { left, right } => {
                let a = integer(self.eval(left, env)?)?;
                let b = integer(self.eval(right, env)?)?;
                DataValue::Integer {
                    value: if matches!(expr, Expr::Add { .. }) {
                        a.checked_add(b)
                    } else {
                        a.checked_sub(b)
                    }
                    .ok_or_else(|| failed("integer overflow"))?,
                }
            }
            Expr::Concat { left, right } => {
                let mut a = text(self.eval(left, env)?)?;
                let b = text(self.eval(right, env)?)?;
                if a.len().saturating_add(b.len()) > MAX_TEXT_BYTES {
                    return Err(exhausted("text byte limit"));
                }
                self.meter.tick((a.len() + b.len()) as u64)?;
                if self.app.is_fallible() {
                    super::product_alloc::try_append_str(&mut a, &b, &mut self.meter.allocations,
                        super::product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?;
                } else { a.push_str(&b); }
                DataValue::Text { value: a }
            }
            Expr::TextContains {
                text: source,
                search,
            } => {
                let a = text(self.eval(source, env)?)?;
                let b = text(self.eval(search, env)?)?;
                self.meter.tick((a.len() + b.len()) as u64)?;
                DataValue::Boolean {
                    value: a.contains(&b),
                }
            }
            Expr::Lower { value } => {
                let value = text(self.eval(value, env)?)?;
                self.meter.tick(value.len() as u64)?;
                let mut allocations = if self.app.is_fallible() { None } else { Some(AllocationContext::new()) };
                let fallible = self.app.is_fallible();
                let value = super::fallible_lower::try_lowercase(&value, allocations.as_mut().unwrap_or(&mut self.meter.allocations))
                    .map_err(|failure| if fallible { AdapterError::Allocation(failure) } else { failed("lowercase allocation failed") })?;
                if value.len() > MAX_TEXT_BYTES {
                    return Err(exhausted("text byte limit"));
                }
                DataValue::Text { value }
            }
            Expr::And { values } | Expr::Or { values } => {
                let is_and = matches!(expr, Expr::And { .. });
                let mut result = is_and;
                for value in values {
                    let value = boolean(self.eval(value, env)?)?;
                    if value != is_and {
                        result = value;
                        break;
                    }
                }
                DataValue::Boolean { value: result }
            }
            Expr::Not { value } => DataValue::Boolean {
                value: !boolean(self.eval(value, env)?)?,
            },
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                if boolean(self.eval(condition, env)?)? {
                    self.eval(then_value, env)?
                } else {
                    self.eval(else_value, env)?
                }
            }
            Expr::Coalesce { value, fallback } => {
                let value = self.eval(value, env)?;
                if value == DataValue::Null {
                    self.eval(fallback, env)?
                } else {
                    value
                }
            }
            Expr::DateAdd { date, days } => {
                let date = day(self.eval(date, env)?)?;
                let days = integer(self.eval(days, env)?)?;
                let value = i64::from(date)
                    .checked_add(days)
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or_else(|| failed("date overflow"))?;
                validate_day(value)?;
                DataValue::Date { days: value }
            }
            Expr::DateDifference { later, earlier } => DataValue::Integer {
                value: i64::from(day(self.eval(later, env)?)?)
                    - i64::from(day(self.eval(earlier, env)?)?),
            },
        };
        self.meter.tick(1)?;
        Ok(value)
    }
}
