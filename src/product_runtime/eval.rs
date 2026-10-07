use super::*;
use std::cmp::Ordering as Compare;

pub(super) type Env = BTreeMap<Id, (Type, DataValue)>;
pub(super) struct Eval<'a> {
    pub app: &'a AppDefinition,
    pub data: &'a DataSnapshot,
    pub session: &'a SessionState,
    pub day: i32,
    pub meter: &'a mut Meter,
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

impl Eval<'_> {
    fn literal_references(&mut self, value: &DataValue) -> Result<()> {
        self.meter.tick(1)?;
        match value {
            DataValue::Reference { .. } => {
                self.meter.tick(self.data.records.len() as u64)?;
                record(self.data, value)?;
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
    pub(super) fn typ(&mut self, expr: &Expr, env: &Env) -> Result<Type> {
        self.meter.tick(1)?;
        Ok(match expr {
            Expr::Literal { value_type, .. } => value_type.clone(),
            Expr::Variable { name } => env
                .get(name)
                .ok_or_else(|| failed("missing variable type"))?
                .0
                .clone(),
            Expr::State { state } => self
                .app
                .state
                .iter()
                .find(|s| &s.id == state)
                .ok_or_else(|| failed("missing state type"))?
                .value_type
                .clone(),
            Expr::Field { record, field } => {
                let Type::Reference { entity } = self.typ(record, env)? else {
                    return Err(failed("invalid field type"));
                };
                self.app
                    .entities
                    .iter()
                    .find(|e| e.id == entity)
                    .and_then(|e| e.fields.iter().find(|f| &f.id == field))
                    .ok_or_else(|| failed("missing field type"))?
                    .value_type
                    .clone()
            }
            Expr::Query { entity, .. } => Type::list(Type::reference(entity)),
            Expr::Filter { items, .. } => self.typ(items, env)?,
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let Type::List { item } = self.typ(items, env)? else {
                    return Err(failed("invalid map type"));
                };
                let mut local = env.clone();
                local.insert(binding.clone(), (*item, DataValue::Null));
                Type::list(self.typ(value, &local)?)
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
    pub fn eval(&mut self, expr: &Expr, env: &Env) -> Result<DataValue> {
        self.meter.tick(1)?;
        let value = match expr {
            Expr::Literal { value, .. } => {
                self.meter.value(value)?;
                self.literal_references(value)?;
                value.clone()
            }
            Expr::Variable { name } => {
                let value = &env.get(name).ok_or_else(|| failed("unbound variable"))?.1;
                self.meter.value(value)?;
                value.clone()
            }
            Expr::Field {
                record: expr,
                field,
            } => {
                let key = self.eval(expr, env)?;
                self.meter.tick(self.data.records.len() as u64)?;
                let value = record(self.data, &key)?
                    .values
                    .get(field)
                    .cloned()
                    .unwrap_or(DataValue::Null);
                self.meter.value(&value)?;
                value
            }
            Expr::State { state } => {
                let value = self
                    .session
                    .values
                    .get(state)
                    .ok_or_else(|| failed("missing session value"))?;
                self.meter.value(value)?;
                value.clone()
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
                for record in &self.data.records {
                    self.meter.tick(1)?;
                    if &record.entity == entity && (!record.archived || *include_archived) {
                        candidates.push(record);
                    }
                }
                self.meter.collection(candidates.len())?;
                let comparisons = candidates.len().saturating_mul(
                    (usize::BITS - candidates.len().max(1).leading_zeros()) as usize,
                );
                self.meter.tick(comparisons as u64)?;
                candidates.sort_by(|a, b| a.id.cmp(&b.id));
                // Predicate and key failures must follow ID order too, not just
                // successful result rows after the final explicit-key sort.
                let mut rows = Vec::new();
                for record in candidates {
                    self.meter.tick(1)?;
                    let value = value_ref(record);
                    let mut local = env.clone();
                    local.insert(binding.clone(), (Type::reference(entity), value.clone()));
                    if boolean(self.eval(predicate, &local)?)? {
                        let keys = sort
                            .iter()
                            .map(|k| self.eval(&k.value, &local))
                            .collect::<Result<Vec<_>>>()?;
                        rows.push((record.id.clone(), keys, value));
                        self.meter.collection(rows.len())?;
                    }
                }
                // Charge a deterministic upper bound before the bounded stable sort.
                let comparisons = rows
                    .len()
                    .saturating_mul((usize::BITS - rows.len().max(1).leading_zeros()) as usize)
                    .saturating_mul(sort.len().max(1));
                self.meter.tick(comparisons as u64)?;
                rows.sort_by(|a, b| {
                    for (i, key) in sort.iter().enumerate() {
                        let order = compare(&a.1[i], &b.1[i]);
                        if order != Compare::Equal {
                            return if key.descending {
                                order.reverse()
                            } else {
                                order
                            };
                        }
                    }
                    a.0.cmp(&b.0)
                });
                self.meter.tick(1)?;
                DataValue::List {
                    item_type: Type::reference(entity),
                    items: rows.into_iter().take(*limit).map(|r| r.2).collect(),
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
                let (typ, items) = list(self.eval(items, env)?)?;
                let mut kept = Vec::new();
                let all = matches!(expr, Expr::All { .. });
                let any = matches!(expr, Expr::Any { .. });
                let mut result = all;
                for item in items {
                    self.meter.tick(1)?;
                    let mut local = env.clone();
                    local.insert(binding.clone(), (typ.clone(), item.clone()));
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
                        kept.push(item);
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
                let mut local = env.clone();
                local.insert(binding.clone(), (typ.clone(), DataValue::Null));
                let result_type = self.typ(value, &local)?;
                let mut mapped = Vec::new();
                for item in items {
                    self.meter.tick(1)?;
                    local.insert(binding.clone(), (typ.clone(), item));
                    mapped.push(self.eval(value, &local)?);
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
                a.push_str(&b);
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
                let value = value.to_lowercase();
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
