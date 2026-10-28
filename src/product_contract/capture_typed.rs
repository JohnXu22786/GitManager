//! Value intake stays native; this paid descriptor precedes native App conversion.
use super::bounded_input::capture_payment_core as native;
use super::capture_work::Audit;
use super::*;
use std::mem::size_of;

// Pinned Serde cautious size-hint cap; independent of input/host limits.
const NATIVE_HINT_CAP_BYTES: usize = 1_048_576;

struct Conversion {
    bytes: usize,
    max_bytes: usize,
    native: u64,
}
impl Conversion {
    fn add(&mut self, n: u64, a: Audit<'_>) -> Result<()> {
        a.pay(3)?;
        self.native = self.native.checked_add(n).ok_or_else(|| a.overflow())?;
        Ok(())
    }
    fn string(&mut self, value: &str, a: Audit<'_>) -> Result<()> {
        a.pay(6)?;
        self.max_bytes = self.max_bytes.max(value.len());
        a.scaled(value.len(), 1, 10)?;
        self.bytes = self
            .bytes
            .checked_add(value.len())
            .ok_or_else(|| a.overflow())?;
        let matching = 32u64
            .checked_mul(2 + 2 * (value.len().min(32) as u64))
            .ok_or_else(|| a.overflow())?;
        self.add(
            matching
                .checked_add(native::string_copy_bytes(value.len()).ok_or_else(|| a.overflow())?)
                .ok_or_else(|| a.overflow())?,
            a,
        )
    }
    fn vector(len: usize, slot: usize, a: Audit<'_>) -> Result<(u64, u64)> {
        a.pay(64 + 4 * usize::BITS as u64)?;
        a.pay(16)?;
        let bytes = len.checked_mul(slot).ok_or_else(|| a.overflow())?;
        let requests = if bytes <= NATIVE_HINT_CAP_BYTES {
            // Exact owned hints permit n slots without growth. Keep allocation,
            // slot movement, full backing release and one excess owner envelope.
            bytes.checked_mul(4).ok_or_else(|| a.overflow())?
        } else {
            let initial = len.min(NATIVE_HINT_CAP_BYTES / slot.max(1));
            let capacity = len
                .checked_mul(2)
                .map(|n| {
                    n.max(if slot == 1 {
                        8
                    } else if slot <= 1024 {
                        4
                    } else {
                        1
                    })
                })
                .ok_or_else(|| a.overflow())?;
            initial
                .checked_add(capacity.checked_mul(3).ok_or_else(|| a.overflow())?)
                .and_then(|n| n.checked_mul(slot))
                .ok_or_else(|| a.overflow())?
        };
        let growth = native::growth_count(len).ok_or_else(|| a.overflow())?;
        a.pay(8)?;
        let fixed = growth
            .checked_mul(9)
            .and_then(|n| n.checked_add((len as u64).checked_mul(3)?))
            .and_then(|n| n.checked_add(4))
            .ok_or_else(|| a.overflow())?;
        let amount = (requests as u64)
            .checked_add(fixed)
            .ok_or_else(|| a.overflow())?;
        Ok((amount, fixed))
    }
    fn content(&mut self, len: usize, slot: usize, depth: usize, a: Audit<'_>) -> Result<()> {
        let (vector, fixed) = Self::vector(len, slot, a)?;
        a.pay(16)?;
        let excess = depth.checked_sub(1).unwrap_or(0) as u64;
        // Owned Content moves descendants: at most two temporary containers.
        // Keep the old ancestry-weighted fixed controls at depth >= 1.
        let amount = vector
            .checked_mul(2)
            .and_then(|n| n.checked_add(fixed.checked_mul(excess)?))
            .ok_or_else(|| a.overflow())?;
        self.add(amount, a)
    }
    fn map_field(key: &str, a: Audit<'_>) -> Result<bool> {
        a.pay(8 + 2 * size_of::<&str>() as u64)?;
        Ok(a.eq_str(key, "parameters")?
            || a.eq_str(key, "values")?
            || a.eq_str(key, "arguments")?
            || a.eq_str(key, "columns")?
            || a.eq_str(key, "defaults")?)
    }
    fn value(
        &mut self,
        value: &JsonValue,
        depth: usize,
        may_build_final_map: bool,
        a: Audit<'_>,
    ) -> Result<()> {
        a.pay(12)?;
        let weight = (depth + 1) as u64;
        self.add(12 * weight, a)?;
        // Every candidate final typed node may have a boxed child. This closed
        // representation bound covers Type/Expr, never an unvalidated depth cap.
        self.add((3 + 2 * size_of::<Expr>().max(size_of::<Type>())) as u64, a)?;
        match value {
            JsonValue::String(value) => self.string(value, a)?,
            JsonValue::Array(values) => {
                a.slice(values.len())?;
                self.content(
                    values.len(),
                    size_of::<JsonValue>() + 4 * size_of::<usize>(),
                    depth,
                    a,
                )?;
                a.pay(16)?;
                // Final typed Vec candidates are closed: Expr, Statement, DataValue,
                // schema/view structs. Inline width is independent of payload bytes.
                let slot = size_of::<Expr>()
                    .max(size_of::<Statement>())
                    .max(size_of::<DataValue>())
                    .max(size_of::<EntityDefinition>())
                    .max(size_of::<ViewDefinition>())
                    .max(size_of::<ActionDefinition>());
                let vector = Self::vector(values.len(), slot, a)?.0;
                let mut set = 0u64;
                for (index, value) in values.iter().enumerate() {
                    a.pay(native::INSERT_DESCRIPTOR)?;
                    let insertion = native::btree_insert::<KeyModifier, ()>(index.min(4), 1)
                        .ok_or_else(|| a.overflow())?;
                    a.pay(3)?;
                    set = set.checked_add(insertion).ok_or_else(|| a.overflow())?;
                    a.pay(4 + 2 * size_of::<bool>() as u64)?;
                    self.value(value, depth + 1, true, a)?;
                }
                // One owned array selects a final Vec or modifier set, not both.
                a.pay(2)?;
                self.add(vector.max(set), a)?;
            }
            JsonValue::Object(map) => {
                a.map_iter(map.len())?;
                self.content(
                    map.len(),
                    2 * (size_of::<JsonValue>() + 4 * size_of::<usize>()),
                    depth,
                    a,
                )?;
                self.add(
                    native::btree_iter(map.len()).ok_or_else(|| a.overflow())?,
                    a,
                )?;
                for (index, (key, value)) in map.iter().enumerate() {
                    a.pay(8)?;
                    self.string(key, a)?;
                    self.add(15 * weight, a)?; // Weighted Content key node + key child/next.
                                               // Key may be visited both as an enclosing tag and as a field.
                    self.add(32 * (2 + 2 * key.len().min(32) as u64), a)?;
                    a.pay(2)?;
                    if may_build_final_map {
                        let c = a.comparator(key.len())?;
                        a.pay(3 * native::INSERT_DESCRIPTOR)?;
                        let candidates = [
                            native::btree_insert::<Id, Type>(index, c),
                            native::btree_insert::<Id, Expr>(index, c),
                            native::btree_insert::<Id, DataValue>(index, c),
                        ];
                        // One final map type owns the complete insertion/cleanup.
                        a.pay(12)?;
                        let insertion = candidates[0]
                            .ok_or_else(|| a.overflow())?
                            .max(candidates[1].ok_or_else(|| a.overflow())?)
                            .max(candidates[2].ok_or_else(|| a.overflow())?);
                        self.add(insertion, a)?;
                    }
                    // This name classifies only the immediate child, never a subtree.
                    let child_map = Self::map_field(key, a)?;
                    a.pay(4 + 2 * size_of::<bool>() as u64)?;
                    self.value(value, depth + 1, child_map, a)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[cfg_attr(test, track_caller)]
pub(super) fn parse_app(bytes: &[u8], a: Audit<'_>) -> Result<AppDefinition> {
    #[cfg(test)]
    let _diagnostic = super::capture_work::diagnostic::enter(
        super::capture_work::diagnostic::Phase::Conversion,
    );
    a.format(107)?;
    let value = match bounded_input::parse_json_bytes_with_work(bytes, &mut |n| a.pay(n)) {
        Ok(value) => value,
        Err(bounded_input::InputWorkError::Input(error)) => {
            return Err(ContractError(error.to_string()))
        }
        Err(bounded_input::InputWorkError::Resource(error)) => return Err(error),
        Err(bounded_input::InputWorkError::ArithmeticOverflow) => return Err(a.overflow()),
    };
    convert_app(value, a)
}
// App-only original post-intake Conversion; the paid miss transfers the same
// untouched Value owner here. No admission/proof state is returned.
pub(super) fn convert_app(value: JsonValue, a: Audit<'_>) -> Result<AppDefinition> {
    a.pay(9 + 65 * (8 * size_of::<usize>() as u64) + 2 * size_of::<usize>() as u64)?;
    let mut descriptor = Conversion {
        bytes: 0,
        max_bytes: 0,
        native: 0,
    };
    a.pay(4 + 2 * size_of::<bool>() as u64)?;
    descriptor.value(&value, 0, false, a)?;
    a.pay(64 + 4 * usize::BITS as u64)?;
    let diagnostic = descriptor
        .max_bytes
        .checked_mul(6)
        .and_then(|n| n.checked_add(1024))
        .ok_or_else(|| a.overflow())?;
    a.amount(native::custom_error_bytes(diagnostic))?;
    a.format(diagnostic)?;
    let eb = size_of::<Box<str>>().max(size_of::<std::io::Error>()) + 4 * size_of::<usize>();
    a.scaled(eb, 2, 5)?;
    a.pay(descriptor.native)?;
    let app = serde_json::from_value(value).map_err(|error| ContractError(error.to_string()))?;
    a.pay(0)?;
    Ok(app)
}

// Compile-only closure of the map-placement proof. Never called at runtime.
// Owned bindings reject type drift without borrowed Deref coercions; Serde
// attributes and native feature/source changes still require explicit review.
#[allow(dead_code)]
mod schema_guard {
    use super::*;

    fn guard_type(value: Type) {
        match value {
            Type::Boolean => {},
            Type::Integer => {},
            Type::Text => {},
            Type::Date => {},
            Type::Reference { entity } => {
                let _: String = entity;
            }
            Type::List { item } => {
                let _: Box<Type> = item;
            }
            Type::Optional { item } => {
                let _: Box<Type> = item;
            }
        }
    }

    fn guard_data_value(value: DataValue) {
        match value {
            DataValue::Null => {},
            DataValue::Boolean { value } => {
                let _: bool = value;
            }
            DataValue::Integer { value } => {
                let _: i64 = value;
            }
            DataValue::Text { value } => {
                let _: String = value;
            }
            DataValue::Date { days } => {
                let _: i32 = days;
            }
            DataValue::Reference { entity, record } => {
                let _: String = entity;
                let _: String = record;
            }
            DataValue::List { item_type, items } => {
                let _: Type = item_type;
                let _: Vec<DataValue> = items;
            }
        }
    }

    fn guard_expr(value: Expr) {
        match value {
            Expr::Literal { value_type, value } => {
                let _: Type = value_type;
                let _: DataValue = value;
            }
            Expr::Variable { name } => {
                let _: String = name;
            }
            Expr::Field { record, field } => {
                let _: Box<Expr> = record;
                let _: String = field;
            }
            Expr::State { state } => {
                let _: String = state;
            }
            Expr::Today => {},
            Expr::Query { entity, binding, predicate, sort, limit, include_archived } => {
                let _: String = entity;
                let _: String = binding;
                let _: Box<Expr> = predicate;
                let _: Vec<SortKey> = sort;
                let _: usize = limit;
                let _: bool = include_archived;
            }
            Expr::Filter { items, binding, predicate } => {
                let _: Box<Expr> = items;
                let _: String = binding;
                let _: Box<Expr> = predicate;
            }
            Expr::Map { items, binding, value } => {
                let _: Box<Expr> = items;
                let _: String = binding;
                let _: Box<Expr> = value;
            }
            Expr::Count { items } => {
                let _: Box<Expr> = items;
            }
            Expr::Any { items, binding, predicate } => {
                let _: Box<Expr> = items;
                let _: String = binding;
                let _: Box<Expr> = predicate;
            }
            Expr::All { items, binding, predicate } => {
                let _: Box<Expr> = items;
                let _: String = binding;
                let _: Box<Expr> = predicate;
            }
            Expr::Sum { items } => {
                let _: Box<Expr> = items;
            }
            Expr::Contains { items, value } => {
                let _: Box<Expr> = items;
                let _: Box<Expr> = value;
            }
            Expr::Equal { left, right } => {
                let _: Box<Expr> = left;
                let _: Box<Expr> = right;
            }
            Expr::Less { left, right } => {
                let _: Box<Expr> = left;
                let _: Box<Expr> = right;
            }
            Expr::Add { left, right } => {
                let _: Box<Expr> = left;
                let _: Box<Expr> = right;
            }
            Expr::Subtract { left, right } => {
                let _: Box<Expr> = left;
                let _: Box<Expr> = right;
            }
            Expr::Concat { left, right } => {
                let _: Box<Expr> = left;
                let _: Box<Expr> = right;
            }
            Expr::TextContains { text, search } => {
                let _: Box<Expr> = text;
                let _: Box<Expr> = search;
            }
            Expr::Lower { value } => {
                let _: Box<Expr> = value;
            }
            Expr::And { values } => {
                let _: Vec<Expr> = values;
            }
            Expr::Or { values } => {
                let _: Vec<Expr> = values;
            }
            Expr::Not { value } => {
                let _: Box<Expr> = value;
            }
            Expr::If { condition, then_value, else_value } => {
                let _: Box<Expr> = condition;
                let _: Box<Expr> = then_value;
                let _: Box<Expr> = else_value;
            }
            Expr::Coalesce { value, fallback } => {
                let _: Box<Expr> = value;
                let _: Box<Expr> = fallback;
            }
            Expr::DateAdd { date, days } => {
                let _: Box<Expr> = date;
                let _: Box<Expr> = days;
            }
            Expr::DateDifference { later, earlier } => {
                let _: Box<Expr> = later;
                let _: Box<Expr> = earlier;
            }
        }
    }

    fn guard_sort_key(value: SortKey) {
        let SortKey { value, descending } = value;
        let _: Expr = value;
        let _: bool = descending;
    }

    fn guard_field_definition(value: FieldDefinition) {
        let FieldDefinition { id, label, value_type } = value;
        let _: String = id;
        let _: String = label;
        let _: Type = value_type;
    }

    fn guard_entity_definition(value: EntityDefinition) {
        let EntityDefinition { id, label, fields, unique, constraints } = value;
        let _: String = id;
        let _: String = label;
        let _: Vec<FieldDefinition> = fields;
        let _: Vec<Vec<String>> = unique;
        let _: Vec<Expr> = constraints;
    }

    fn guard_state_definition(value: StateDefinition) {
        let StateDefinition { id, label, value_type, initial } = value;
        let _: String = id;
        let _: String = label;
        let _: Type = value_type;
        let _: DataValue = initial;
    }

    fn guard_action_definition(value: ActionDefinition) {
        let ActionDefinition { id, label, parameters, guards, steps, ensures } = value;
        let _: String = id;
        let _: String = label;
        let _: BTreeMap<String, Type> = parameters;
        let _: Vec<Expr> = guards;
        let _: Vec<Statement> = steps;
        let _: Vec<Expr> = ensures;
    }

    fn guard_statement(value: Statement) {
        match value {
            Statement::Create { entity, values, bind } => {
                let _: String = entity;
                let _: BTreeMap<String, Expr> = values;
                let _: String = bind;
            }
            Statement::Update { record, values } => {
                let _: Expr = record;
                let _: BTreeMap<String, Expr> = values;
            }
            Statement::Archive { record } => {
                let _: Expr = record;
            }
            Statement::SetState { state, value } => {
                let _: String = state;
                let _: Expr = value;
            }
            Statement::Collection { target, operation, items } => {
                let _: CollectionTarget = target;
                let _: CollectionOperation = operation;
                let _: Expr = items;
            }
            Statement::ForEach { items, binding, steps } => {
                let _: Expr = items;
                let _: String = binding;
                let _: Vec<Statement> = steps;
            }
            Statement::Assert { condition, message } => {
                let _: Expr = condition;
                let _: String = message;
            }
            Statement::Emit { output, items, binding, columns } => {
                let _: String = output;
                let _: Expr = items;
                let _: String = binding;
                let _: BTreeMap<String, Expr> = columns;
            }
        }
    }

    fn guard_collection_target(value: CollectionTarget) {
        match value {
            CollectionTarget::State { state } => {
                let _: String = state;
            }
            CollectionTarget::Field { record, field } => {
                let _: Expr = record;
                let _: String = field;
            }
        }
    }

    fn guard_collection_operation(value: CollectionOperation) {
        match value {
            CollectionOperation::Insert => {},
            CollectionOperation::Remove => {},
        }
    }

    fn guard_output_format(value: OutputFormat) {
        match value {
            OutputFormat::Csv => {},
            OutputFormat::Json => {},
        }
    }

    fn guard_output_definition(value: OutputDefinition) {
        let OutputDefinition { id, label, format, columns } = value;
        let _: String = id;
        let _: String = label;
        let _: OutputFormat = format;
        let _: Vec<FieldDefinition> = columns;
    }

    fn guard_observable_definition(value: ObservableDefinition) {
        let ObservableDefinition { id, label, value } = value;
        let _: String = id;
        let _: String = label;
        let _: Expr = value;
    }

    fn guard_column(value: Column) {
        let Column { id, label, value } = value;
        let _: String = id;
        let _: String = label;
        let _: Expr = value;
    }

    fn guard_state_control(value: StateControl) {
        let StateControl { id, label, state, on_change } = value;
        let _: String = id;
        let _: String = label;
        let _: String = state;
        let _: Option<String> = on_change;
    }

    fn guard_selection_binding(value: SelectionBinding) {
        let SelectionBinding { id, state, on_change } = value;
        let _: String = id;
        let _: String = state;
        let _: Option<String> = on_change;
    }

    fn guard_form_input(value: FormInput) {
        let FormInput { parameter, label } = value;
        let _: String = parameter;
        let _: String = label;
    }

    fn guard_view_kind(value: ViewKind) {
        match value {
            ViewKind::List { entity, rows, columns, controls, selection } => {
                let _: String = entity;
                let _: Expr = rows;
                let _: Vec<Column> = columns;
                let _: Vec<StateControl> = controls;
                let _: Option<SelectionBinding> = selection;
            }
            ViewKind::Detail { entity, record, columns } => {
                let _: String = entity;
                let _: Expr = record;
                let _: Vec<Column> = columns;
            }
            ViewKind::Form { action, fields, defaults } => {
                let _: String = action;
                let _: Vec<FormInput> = fields;
                let _: BTreeMap<String, DataValue> = defaults;
            }
        }
    }

    fn guard_action_placement(value: ActionPlacement) {
        match value {
            ActionPlacement::Toolbar => {},
            ActionPlacement::Row => {},
        }
    }

    fn guard_action_binding(value: ActionBinding) {
        let ActionBinding { id, label, placement, action, arguments, enabled } = value;
        let _: String = id;
        let _: String = label;
        let _: ActionPlacement = placement;
        let _: String = action;
        let _: BTreeMap<String, Expr> = arguments;
        let _: Expr = enabled;
    }

    fn guard_key_modifier(value: KeyModifier) {
        match value {
            KeyModifier::Control => {},
            KeyModifier::Alt => {},
            KeyModifier::Shift => {},
            KeyModifier::Command => {},
        }
    }

    fn guard_key_binding(value: KeyBinding) {
        let KeyBinding { key, modifiers, binding } = value;
        let _: String = key;
        let _: BTreeSet<KeyModifier> = modifiers;
        let _: String = binding;
    }

    fn guard_view_definition(value: ViewDefinition) {
        let ViewDefinition { id, label, kind, actions, keys } = value;
        let _: String = id;
        let _: String = label;
        let _: ViewKind = kind;
        let _: Vec<ActionBinding> = actions;
        let _: Vec<KeyBinding> = keys;
    }

    fn guard_app_definition(value: AppDefinition) {
        let AppDefinition { version, id, label, entities, state, actions, outputs, observables, views, initial_view } = value;
        let _: u32 = version;
        let _: String = id;
        let _: String = label;
        let _: Vec<EntityDefinition> = entities;
        let _: Vec<StateDefinition> = state;
        let _: Vec<ActionDefinition> = actions;
        let _: Vec<OutputDefinition> = outputs;
        let _: Vec<ObservableDefinition> = observables;
        let _: Vec<ViewDefinition> = views;
        let _: String = initial_view;
    }

}

#[cfg(test)]
#[path = "capture_typed_bound_tests.rs"]
mod bound_tests;
