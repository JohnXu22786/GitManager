//! Versioned, data-only language and evidence contracts for generated local tools.
//!
//! This module validates source; it neither executes a program nor certifies a
//! model's claims. Use the production `RuntimeAdapter` to obtain observations.
//! There is deliberately no eval, shell, URL, file-path or credential operation.

// One lexical V1 semantic prefix, with no callable semantic-only authority.
// The ordinary/count tail remains in its original scope with the Validator.
macro_rules! validate_app_semantics {
    ($this:ident, $a:ident, $validator:ident, $action_prefix_nodes:ident) => {
        $a.pay(12)?;
        #[cfg(test)]
        count_validation(|counts| counts.apps += 1);
        $a.version($this.version)?;
        $a.check_id(&$this.id)?;
        $a.nonempty(&$this.label)?;
        $a.require(
            !$this.entities.is_empty() && !$this.views.is_empty(),
            "application needs entities and views",
        )?;
        $a.unique($this.entities.iter().map(|e| e.id.as_str()))?;
        $a.unique($this.state.iter().map(|s| s.id.as_str()))?;
        $a.unique($this.actions.iter().map(|$a| $a.id.as_str()))?;
        $a.unique($this.outputs.iter().map(|o| o.id.as_str()))?;
        $a.unique($this.observables.iter().map(|o| o.id.as_str()))?;
        $a.unique($this.views.iter().map(|v| v.id.as_str()))?;
        $a.require(
            $a.find(&$this.views, &$this.initial_view, |v| &v.id)?.is_some(),
            "unknown initial view",
        )?;
        $a.pay(8)?;
        let mut $validator = Validator::new($this, $a);
        validate_entities(&$this.entities, &mut $validator)?;
        $a.slice($this.state.len())?;
        for state in &$this.state {
            $a.pay(12)?;
            $a.nonempty(&state.label)?;
            $validator.typ(&state.value_type, 0)?;
            validate_value_audited(&state.initial, &state.value_type, 0, $a)?;
        }
        $a.slice($this.outputs.len())?;
        for output in &$this.outputs {
            $a.pay(12)?;
            $a.nonempty(&output.label)?;
            $a.require(!output.columns.is_empty(), "output needs columns")?;
            $a.unique(output.columns.iter().map(|c| c.id.as_str()))?;
            $a.slice(output.columns.len())?;
        for column in &output.columns {
            $a.pay(12)?;
                $a.nonempty(&column.label)?;
                $validator.typ(&column.value_type, 0)?;
                $a.require(
                    column.value_type.is_scalar(),
                    "output columns must be scalar",
                )?;
            }
        }
        $a.slice($this.actions.len())?;
        for action in &$this.actions {
            $a.pay(12)?;
            $a.nonempty(&action.label)?;
            $a.require(!action.steps.is_empty(), "action needs steps")?;
            $a.map_iter(action.parameters.len())?;
            $a.unique(action.parameters.keys().map(String::as_str))?;
            $a.map_iter(action.parameters.len())?;
        for typ in action.parameters.values() {
            $a.pay(12)?;
                $validator.typ(typ, 0)?;
            }
            let mut env = $a.clone_owned(&action.parameters)?;
            $a.bounded(action.guards.len())?;
            $a.bounded(action.ensures.len())?;
            $a.slice(action.guards.len())?;
        for guard in &action.guards {
            $a.pay(12)?;
                $validator.expect(guard, &env, &Type::Boolean, 0, true)?;
            }
            $validator.statements(&action.steps, &mut env, 0)?;
            $a.slice(action.ensures.len())?;
        for condition in &action.ensures {
            $a.pay(12)?;
                $validator.expect(condition, &env, &Type::Boolean, 0, true)?;
            }
        }
        $a.pay(1)?;
        let $action_prefix_nodes = $validator.nodes;
        $a.slice($this.observables.len())?;
        for observable in &$this.observables {
            $a.pay(12)?;
            $a.nonempty(&observable.label)?;
            $a.pay(8)?;
            $validator.expr(&observable.value, &Environment::new(), 0, true)?;
        }
        $a.slice($this.views.len())?;
        for view in &$this.views {
            $a.pay(12)?;
            $validator.view(view)?;
        }
    };
}

#[path = "tool_proposal_input.rs"]
mod bounded_input;
#[path = "product_contract/capture_work.rs"]
mod capture_work;
#[path = "product_contract/capture_owned.rs"]
mod capture_owned;
#[path = "product_contract/capture_payment.rs"]
pub(crate) mod capture_payment;
#[path = "product_contract/capture_canonical.rs"]
mod capture_canonical;
#[path = "product_contract/capture_typed.rs"]
mod capture_typed;
#[path = "product_contract/capture_validation.rs"]
mod capture_validation;
use capture_work::Audit;
use capture_owned::OwnedWork;
pub(crate) use capture_work::{ContractWork, WorkFailure, ContractWorkError, ContractWorkResult};
pub(crate) use capture_canonical::canonical_bytes_with_work;
pub(crate) use capture_validation::{with_capture_validation, CaptureValidation, validate_value_with_work};

use ring::digest::{digest, SHA256};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValidationCounts {
    pub captures: usize,
    pub apps: usize,
    pub observables: usize,
    pub views: usize,
}
#[cfg(test)]
thread_local! {
    static VALIDATION_COUNTS: std::cell::Cell<ValidationCounts> = std::cell::Cell::new(ValidationCounts::default());
}
#[cfg(test)]
pub(crate) fn reset_validation_counts() {
    VALIDATION_COUNTS.with(|counts| counts.set(ValidationCounts::default()));
}
#[cfg(test)]
pub(crate) fn validation_counts() -> ValidationCounts {
    VALIDATION_COUNTS.with(std::cell::Cell::get)
}
#[cfg(test)]
fn count_validation(update: impl FnOnce(&mut ValidationCounts)) {
    VALIDATION_COUNTS.with(|counts| {
        let mut value = counts.get();
        update(&mut value);
        counts.set(value);
    });
}

pub const CONTRACT_VERSION: u32 = 1;
pub const LANGUAGE_VERSION: &str = "gitmanager.local-app/1";
pub const MAX_WIRE_BYTES: usize = bounded_input::MAX_INPUT_BYTES;
pub const MAX_EXPR_DEPTH: usize = 16;
pub const MAX_TYPE_DEPTH: usize = 4;
pub const MAX_AST_NODES: usize = 8192;
pub const MAX_ITEMS: usize = 256;
pub const MAX_TEXT_BYTES: usize = 4096;
pub const MAX_COLLECTION: usize = 10_000;
pub const MAX_EVENTS: usize = 50_000;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub type Id = String;
pub type Values = BTreeMap<Id, DataValue>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractError(pub String);
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}
type Result<T> = std::result::Result<T, ContractError>;
fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ContractError(message.into()))
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}
fn check_id(id: &str) -> Result<()> {
    require(valid_id(id), format!("invalid stable ID: {id}"))
}
fn text(value: &str) -> Result<()> {
    require(value.len() <= MAX_TEXT_BYTES, "text exceeds byte limit")
}
fn nonempty(value: &str) -> Result<()> {
    text(value)?;
    require(!value.trim().is_empty(), "empty required text")
}
fn bounded(len: usize) -> Result<()> {
    require(len <= MAX_ITEMS, "too many definitions or steps")
}
fn unique<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        check_id(id)?;
        require(seen.insert(id), format!("duplicate ID: {id}"))?;
    }
    bounded(seen.len())
}
fn version(version: u32) -> Result<()> {
    require(version == CONTRACT_VERSION, "unsupported contract version")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest(String);
impl Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Digest {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self> {
        require(
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid SHA-256 digest",
        )?;
        Ok(Self(value))
    }
}
impl From<Digest> for String {
    fn from(value: Digest) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDomain {
    Source,
    Program,
    SemanticProgram,
    Data,
    Schema,
    Input,
    Session,
    Observation,
    Output,
    OutputBytes,
    Request,
    Scenario,
    Decision,
    Evolution,
    Adoption,
    Evidence,
}
fn bytes_digest(domain: IdentityDomain, bytes: &[u8]) -> Digest {
    let mut input = b"gitmanager.product.v1\0".to_vec();
    input.extend(serde_json::to_vec(&domain).expect("finite enum"));
    input.push(0);
    input.extend(bytes);
    Digest(
        digest(&SHA256, &input)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// Canonical JSON uses lexically sorted object keys, preserves arrays and exact
/// typed values, and contains no whitespace. This is a versioned local format,
/// not a claim of cross-language RFC 8785 floating-point canonicalization.
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_value(value)
        .and_then(|mut value| {
            value.sort_all_objects();
            serde_json::to_vec(&value)
        })
        .map_err(|e| ContractError(e.to_string()))
}
pub fn canonical_digest<T: Serialize>(domain: IdentityDomain, value: &T) -> Result<Digest> {
    Ok(bytes_digest(domain, &canonical_bytes(value)?))
}
fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let value = bounded_input::parse_json_bytes(bytes).map_err(|e| ContractError(e.to_string()))?;
    serde_json::from_value(value).map_err(|e| ContractError(e.to_string()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Type {
    Boolean,
    Integer,
    Text,
    Date,
    Reference { entity: Id },
    List { item: Box<Type> },
    Optional { item: Box<Type> },
}
impl Type {
    pub fn list(item: Type) -> Self {
        Self::List {
            item: Box::new(item),
        }
    }
    pub fn reference(entity: impl Into<Id>) -> Self {
        Self::Reference {
            entity: entity.into(),
        }
    }
    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            Self::Boolean | Self::Integer | Self::Text | Self::Date | Self::Reference { .. }
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataValue {
    Null,
    Boolean {
        value: bool,
    },
    Integer {
        value: i64,
    },
    Text {
        value: String,
    },
    /// Gregorian day number relative to 1970-01-01; arithmetic must be checked.
    Date {
        days: i32,
    },
    Reference {
        entity: Id,
        record: Id,
    },
    List {
        item_type: Type,
        items: Vec<DataValue>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    Literal {
        value_type: Type,
        value: DataValue,
    },
    Variable {
        name: Id,
    },
    Field {
        record: Box<Expr>,
        field: Id,
    },
    State {
        state: Id,
    },
    Today,
    Query {
        entity: Id,
        binding: Id,
        predicate: Box<Expr>,
        sort: Vec<SortKey>,
        limit: usize,
        include_archived: bool,
    },
    Filter {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    Map {
        items: Box<Expr>,
        binding: Id,
        value: Box<Expr>,
    },
    Count {
        items: Box<Expr>,
    },
    Any {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    All {
        items: Box<Expr>,
        binding: Id,
        predicate: Box<Expr>,
    },
    Sum {
        items: Box<Expr>,
    },
    Contains {
        items: Box<Expr>,
        value: Box<Expr>,
    },
    Equal {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Less {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Subtract {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Concat {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    TextContains {
        text: Box<Expr>,
        search: Box<Expr>,
    },
    Lower {
        value: Box<Expr>,
    },
    And {
        values: Vec<Expr>,
    },
    Or {
        values: Vec<Expr>,
    },
    Not {
        value: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_value: Box<Expr>,
        else_value: Box<Expr>,
    },
    Coalesce {
        value: Box<Expr>,
        fallback: Box<Expr>,
    },
    DateAdd {
        date: Box<Expr>,
        days: Box<Expr>,
    },
    DateDifference {
        later: Box<Expr>,
        earlier: Box<Expr>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortKey {
    pub value: Expr,
    pub descending: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDefinition {
    pub id: Id,
    pub label: String,
    pub value_type: Type,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityDefinition {
    pub id: Id,
    pub label: String,
    pub fields: Vec<FieldDefinition>,
    /// Unique tuples among non-archived records, excluding tuples containing null.
    pub unique: Vec<Vec<Id>>,
    /// Boolean expressions with `record` bound to each non-archived row. No state.
    pub constraints: Vec<Expr>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDefinition {
    pub id: Id,
    pub label: String,
    pub value_type: Type,
    pub initial: DataValue,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDefinition {
    pub id: Id,
    pub label: String,
    pub parameters: BTreeMap<Id, Type>,
    pub guards: Vec<Expr>,
    pub steps: Vec<Statement>,
    pub ensures: Vec<Expr>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Statement {
    Create {
        entity: Id,
        values: BTreeMap<Id, Expr>,
        bind: Id,
    },
    Update {
        record: Expr,
        values: BTreeMap<Id, Expr>,
    },
    Archive {
        record: Expr,
    },
    SetState {
        state: Id,
        value: Expr,
    },
    Collection {
        target: CollectionTarget,
        operation: CollectionOperation,
        items: Expr,
    },
    ForEach {
        items: Expr,
        binding: Id,
        steps: Vec<Statement>,
    },
    Assert {
        condition: Expr,
        message: String,
    },
    Emit {
        output: Id,
        items: Expr,
        binding: Id,
        columns: BTreeMap<Id, Expr>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollectionTarget {
    State { state: Id },
    Field { record: Expr, field: Id },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionOperation {
    Insert,
    Remove,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Csv,
    Json,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputDefinition {
    pub id: Id,
    pub label: String,
    pub format: OutputFormat,
    pub columns: Vec<FieldDefinition>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservableDefinition {
    pub id: Id,
    pub label: String,
    pub value: Expr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    pub id: Id,
    pub label: String,
    pub value: Expr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateControl {
    pub id: Id,
    pub label: String,
    pub state: Id,
    pub on_change: Option<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionBinding {
    pub id: Id,
    pub state: Id,
    pub on_change: Option<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormInput {
    pub parameter: Id,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewKind {
    List {
        entity: Id,
        rows: Expr,
        columns: Vec<Column>,
        controls: Vec<StateControl>,
        selection: Option<SelectionBinding>,
    },
    Detail {
        entity: Id,
        record: Expr,
        columns: Vec<Column>,
    },
    Form {
        action: Id,
        fields: Vec<FormInput>,
        defaults: Values,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPlacement {
    Toolbar,
    Row,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
    pub id: Id,
    pub label: String,
    pub placement: ActionPlacement,
    pub action: Id,
    pub arguments: BTreeMap<Id, Expr>,
    pub enabled: Expr,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyModifier {
    Control,
    Alt,
    Shift,
    Command,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyBinding {
    pub key: String,
    pub modifiers: BTreeSet<KeyModifier>,
    pub binding: Id,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewDefinition {
    pub id: Id,
    pub label: String,
    pub kind: ViewKind,
    pub actions: Vec<ActionBinding>,
    pub keys: Vec<KeyBinding>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub version: u32,
    pub id: Id,
    pub label: String,
    pub entities: Vec<EntityDefinition>,
    pub state: Vec<StateDefinition>,
    pub actions: Vec<ActionDefinition>,
    pub outputs: Vec<OutputDefinition>,
    pub observables: Vec<ObservableDefinition>,
    pub views: Vec<ViewDefinition>,
    pub initial_view: Id,
}

struct Validator<'a, 'w> {
    entities: &'a [EntityDefinition],
    state: &'a [StateDefinition],
    actions: &'a [ActionDefinition],
    outputs: &'a [OutputDefinition],
    nodes: usize,
    audit: Audit<'w>,
}
type Environment = BTreeMap<Id, Type>;
impl<'a, 'w> Validator<'a, 'w> {
    fn new(app: &'a AppDefinition, audit: Audit<'w>) -> Self {
        Self { entities: &app.entities, state: &app.state, actions: &app.actions,
            outputs: &app.outputs, nodes: 0, audit }
    }
    fn node(&mut self, depth: usize) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        self.nodes = self.nodes.checked_add(1).ok_or_else(|| a.overflow())?;
        a.require(
            depth <= MAX_EXPR_DEPTH && self.nodes <= MAX_AST_NODES,
            "AST depth or node budget exceeded",
        )
    }
    fn entity(&self, id: &str) -> Result<&'a EntityDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.entities,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(16).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown entity: {id}")))
    }
    fn state(&self, id: &str) -> Result<&'a StateDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.state,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(15).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown state: {id}")))
    }
    fn action(&self, id: &str) -> Result<&'a ActionDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(self.actions,id,|v|&v.id)? { return Ok(value); }
        a.pay(2)?;a.format(id.len().checked_add(16).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown action: {id}")))
    }
    fn field(&self, entity: &str, field: &str) -> Result<&'a FieldDefinition> {
        let a=self.audit;
        if let Some(value)=a.find(&self.entity(entity)?.fields,field,|v|&v.id)? {return Ok(value);}
        a.pay(3)?;a.format(entity.len().checked_add(field.len()).and_then(|n|n.checked_add(16)).ok_or_else(||a.overflow())?)?;
        Err(ContractError(format!("unknown field: {entity}.{field}")))
    }
    fn typ(&self, typ: &Type, depth: usize) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.require(depth <= MAX_TYPE_DEPTH, "type nesting budget exceeded")?;
        match typ {
            Type::Reference { entity } => {
                self.entity(entity)?;
            }
            Type::List { item } => self.typ(item, depth + 1)?,
            Type::Optional { item } => {
                a.require(
                    !matches!(item.as_ref(), Type::Optional { .. }),
                    "nested optional type",
                )?;
                self.typ(item, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }
    fn bound(&self, env: &Environment, name: &str, typ: Type) -> Result<Environment> {
        let a = self.audit;
        a.pay(8)?;
        a.check_id(name)?;
        let available = !a.has(env, name)?;
        a.pay(8)?;
        if a.enabled() && available {
            a.pay(1)?;
        } else {
        a.pay(2)?;a.format(name.len().checked_add(35).ok_or_else(|| a.overflow())?)?;
        a.require_owned(available, format!("binding shadows existing variable: {name}"))?;
        }
        let mut env = a.clone_owned(env)?;
        a.insert_string(&mut env, name, typ)?;
        Ok(env)
    }
    fn expect(
        &mut self,
        expr: &Expr,
        env: &Environment,
        expected: &Type,
        depth: usize,
        state: bool,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        let actual = self.expr(expr, env, depth, state)?;
        let equal = a.type_equal(&actual, expected)?;
        a.pay(8)?;
        if a.enabled() && equal {
            a.pay(1)?;
            Ok(())
        } else {
        a.type_message(44, expected, &actual)?;
        a.require_owned(equal, format!("expression type mismatch: expected {expected:?}, found {actual:?}"))
        }
    }
    fn list_item(
        &mut self,
        expr: &Expr,
        env: &Environment,
        depth: usize,
        state: bool,
    ) -> Result<Type> {
        let a = self.audit;
        a.pay(8)?;
        match self.expr(expr, env, depth, state)? {
            Type::List { item } => Ok(*item),
            _ => Err(a.error("expected collection")?),
        }
    }
    fn record_entity(
        &mut self,
        expr: &Expr,
        env: &Environment,
        depth: usize,
        state: bool,
    ) -> Result<Id> {
        let a = self.audit;
        a.pay(8)?;
        match self.expr(expr, env, depth, state)? {
            Type::Reference { entity } => Ok(entity),
            _ => Err(a.error("expected nonoptional record reference")?),
        }
    }
    fn expr(&mut self, expr: &Expr, env: &Environment, depth: usize, state: bool) -> Result<Type> {
        let a = self.audit;
        a.pay(8)?;
        self.node(depth)?;
        let next = depth + 1;
        a.pay(12)?;
        Ok(match expr {
            Expr::Literal { value_type, value } => {
                self.typ(value_type, 0)?;
                validate_value_audited(value, value_type, 0, a)?;
                a.clone_owned(value_type)?
            }
            Expr::Variable { name } => {
                match a.get(env, name)? {
                    Some(value) => a.clone_owned(value)?,
                    None => {a.pay(2)?;a.format(name.len().checked_add(18).ok_or_else(||a.overflow())?)?;
                        return Err(ContractError(format!("unbound variable: {name}")));}
                }
            },
            Expr::Field { record, field } => {
                let entity = self.record_entity(record, env, next, state)?;
                a.clone_owned(&self.field(&entity, field)?.value_type)?
            }
            Expr::State { state: id } => {
                a.require(
                    state,
                    "durable constraints cannot reference transient state",
                )?;
                a.clone_owned(&self.state(id)?.value_type)?
            }
            Expr::Today => Type::Date,
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                ..
            } => {
                self.entity(entity)?;
                a.require(
                    *limit > 0 && *limit <= MAX_COLLECTION,
                    "query limit out of bounds",
                )?;
                a.bounded(sort.len())?;
                let env = self.bound(env, binding, a.reference_type(entity)?)?;
                self.expect(predicate, &env, &Type::Boolean, next, state)?;
                a.slice(sort.len())?;
        for key in sort {
            a.pay(8)?;
                    a.require(
                        self.expr(&key.value, &env, next, state)?.is_scalar(),
                        "sort key must be scalar",
                    )?;
                }
                a.list_type(a.reference_type(entity)?)?
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
                let item = self.list_item(items, env, next, state)?;
                let env = self.bound(env, binding, a.clone_owned(&item)?)?;
                self.expect(predicate, &env, &Type::Boolean, next, state)?;
                if matches!(expr, Expr::Filter { .. }) {
                    a.list_type(item)?
                } else {
                    Type::Boolean
                }
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let item = self.list_item(items, env, next, state)?;
                let env = self.bound(env, binding, item)?;
                let result = a.list_type(self.expr(value, &env, next, state)?)?;
                self.typ(&result, 0)?;
                result
            }
            Expr::Count { items } => {
                self.list_item(items, env, next, state)?;
                Type::Integer
            }
            Expr::Sum { items } => {
                self.expect(items, env, &a.list_type(Type::Integer)?, next, state)?;
                Type::Integer
            }
            Expr::Contains { items, value } => {
                let item = self.list_item(items, env, next, state)?;
                self.expect(value, env, &item, next, state)?;
                Type::Boolean
            }
            Expr::Equal { left, right } | Expr::Less { left, right } => {
                self.comparison_operands(left, right, env, next, state, matches!(expr, Expr::Less { .. }))?
            }
            Expr::Add { left, right } | Expr::Subtract { left, right } => {
                self.expect(left, env, &Type::Integer, next, state)?;
                self.expect(right, env, &Type::Integer, next, state)?;
                Type::Integer
            }
            Expr::Concat { left, right } => {
                self.expect(left, env, &Type::Text, next, state)?;
                self.expect(right, env, &Type::Text, next, state)?;
                Type::Text
            }
            Expr::TextContains { text, search } => {
                self.expect(text, env, &Type::Text, next, state)?;
                self.expect(search, env, &Type::Text, next, state)?;
                Type::Boolean
            }
            Expr::Lower { value } => {
                self.expect(value, env, &Type::Text, next, state)?;
                Type::Text
            }
            Expr::And { values } | Expr::Or { values } => {
                a.bounded(values.len())?;
                a.require(!values.is_empty(), "empty boolean connective")?;
                a.slice(values.len())?;
        for value in values {
            a.pay(8)?;
                    self.expect(value, env, &Type::Boolean, next, state)?;
                }
                Type::Boolean
            }
            Expr::Not { value } => {
                self.expect(value, env, &Type::Boolean, next, state)?;
                Type::Boolean
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                self.expect(condition, env, &Type::Boolean, next, state)?;
                let typ = self.expr(then_value, env, next, state)?;
                self.expect(else_value, env, &typ, next, state)?;
                typ
            }
            Expr::Coalesce { value, fallback } => {
                let Type::Optional { item } = self.expr(value, env, next, state)? else {
                    return Err(a.error("coalesce requires optional value")?);
                };
                self.expect(fallback, env, &item, next, state)?;
                *item
            }
            Expr::DateAdd { date, days } => {
                self.expect(date, env, &Type::Date, next, state)?;
                self.expect(days, env, &Type::Integer, next, state)?;
                Type::Date
            }
            Expr::DateDifference { later, earlier } => {
                self.expect(later, env, &Type::Date, next, state)?;
                self.expect(earlier, env, &Type::Date, next, state)?;
                Type::Integer
            }
        })
    }
    fn comparison_operands(&mut self, left: &Expr, right: &Expr, env: &Environment,
        depth: usize, state: bool, ordered: bool) -> Result<Type> {
        let a=self.audit;
        a.pay(6)?;
        let typ=self.expr(left, env, depth, state)?;
        if ordered {a.require(typ.is_scalar(), "ordered comparison requires scalar")?;}
        self.expect(right, env, &typ, depth, state)?;
        Ok(Type::Boolean)
    }
    fn assignment(
        &mut self,
        value: &Expr,
        env: &Environment,
        expected: &Type,
        depth: usize,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        // Storage supplies the expected optional type to each conditional
        // branch. A sealed value may be Null while the live branch computes T.
        // This does not widen If expressions outside an optional assignment.
        if let (
            Type::Optional { .. },
            Expr::If {
                condition,
                then_value,
                else_value,
            },
        ) = (expected, value)
        {
            self.node(depth)?;
            self.expect(condition, env, &Type::Boolean, depth + 1, true)?;
            self.assignment(then_value, env, expected, depth + 1)?;
            return self.assignment(else_value, env, expected, depth + 1);
        }
        let actual = self.expr(value, env, depth, true)?;
        let compatible = a.type_equal(&actual, expected)? || match expected {
            Type::Optional { item } => a.type_equal(item, &actual)?, _ => false,
        };
        a.pay(8)?;
        if a.enabled() && compatible {
            a.pay(1)?;
            Ok(())
        } else {
        a.type_message(44, expected, &actual)?;
        a.require_owned(compatible, format!("assignment type mismatch: expected {expected:?}, found {actual:?}"))
        }
    }
    fn assignments(
        &mut self,
        entity: &str,
        values: &BTreeMap<Id, Expr>,
        env: &Environment,
        depth: usize,
        create: bool,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.bounded(values.len())?;
        a.map_iter(values.len())?;
        for (field, value) in values {
            a.pay(8)?;
            let typ = &self.field(entity, field)?.value_type;
            // Optional storage accepts a computed non-null inner value as well
            // as an explicitly optional expression, without splitting an atomic
            // multi-field Create or Update into separate mutations.
            self.assignment(value, env, typ, depth)?;
        }
        if create {
            let fields = &self.entity(entity)?.fields;
            a.slice(fields.len())?;
            for field in fields {
            a.pay(8)?;
                let present = a.has(values, &field.id)? || matches!(field.value_type, Type::Optional { .. });
        a.pay(8)?;
        if a.enabled() && present {
            a.pay(1)?;
        } else {
                a.pay(2)?;a.format(field.id.len().checked_add(24).ok_or_else(||a.overflow())?)?;
                a.require_owned(present, format!("missing required field: {}", field.id))?;
        }
            }
        }
        Ok(())
    }
    fn statements(
        &mut self,
        steps: &[Statement],
        env: &mut Environment,
        depth: usize,
    ) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.bounded(steps.len())?;
        a.slice(steps.len())?;
        for step in steps {
            a.pay(8)?;
            a.pay(12)?;
            self.node(depth)?;
            match step {
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    self.entity(entity)?;
                    self.assignments(entity, values, env, depth + 1, true)?;
                    *env = self.bound(env, bind, a.reference_type(entity)?)?;
                }
                Statement::Update { record, values } => {
                    let entity = self.record_entity(record, env, depth + 1, true)?;
                    a.require(!values.is_empty(), "empty update")?;
                    self.assignments(&entity, values, env, depth + 1, false)?;
                }
                Statement::Archive { record } => {
                    self.record_entity(record, env, depth + 1, true)?;
                }
                Statement::SetState { state, value } => {
                    self.expect(value, env, &self.state(state)?.value_type, depth + 1, true)?;
                }
                Statement::Collection { target, items, .. } => {
                    let typ = match target {
                        CollectionTarget::State { state } => a.clone_owned(&self.state(state)?.value_type)?,
                        CollectionTarget::Field { record, field } => {
                            let entity = self.record_entity(record, env, depth + 1, true)?;
                            a.clone_owned(&self.field(&entity, field)?.value_type)?
                        }
                    };
                    a.require(
                        matches!(typ, Type::List { .. }),
                        "collection mutation requires list target",
                    )?;
                    self.expect(items, env, &typ, depth + 1, true)?;
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let item = self.list_item(items, env, depth + 1, true)?;
                    let mut local = self.bound(env, binding, item)?;
                    self.statements(steps, &mut local, depth + 1)?;
                }
                Statement::Assert { condition, message } => {
                    a.nonempty(message)?;
                    self.expect(condition, env, &Type::Boolean, depth + 1, true)?;
                }
                Statement::Emit {
                    output,
                    items,
                    binding,
                    columns,
                } => {
                    let output = match a.find(self.outputs, output, |o| &o.id)? {
                        Some(value) => value, None => return Err(a.error("unknown output")?),
                    };
                    let item = self.list_item(items, env, depth + 1, true)?;
                    let env = self.bound(env, binding, item)?;
                    a.require(
                        columns.len() == output.columns.len(),
                        "output columns differ from definition",
                    )?;
                    a.slice(output.columns.len())?;
        for column in &output.columns {
            a.pay(8)?;
                        let value = match a.get(columns, &column.id)? {
                            Some(value) => value, None => return Err(a.error("missing output column")?),
                        };
                        self.expect(value, &env, &column.value_type, depth + 1, true)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn columns(&mut self, columns: &[Column], env: &Environment) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.unique(columns.iter().map(|c| c.id.as_str()))?;
        a.slice(columns.len())?;
        for column in columns {
            a.pay(8)?;
            a.nonempty(&column.label)?;
            self.expr(&column.value, env, 0, true)?;
        }
        Ok(())
    }
    fn on_change(&self, action: Option<&str>, typ: &Type) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        if let Some(action) = action {
            let parameters = &self.action(action)?.parameters;
            a.string(5)?;
            if a.enabled() {typ.pay_owned(a)?;}
            capture_canonical::reserve_from_iter::<String,Type>(1, a.comparator(5)?, a)?;
            let expected = BTreeMap::from([("value".into(), typ.clone())]);
            a.require(
                a.equal_owned(parameters, &expected)?,
                "on-change action must take exactly one value parameter of the control type",
            )?;
        }
        Ok(())
    }
    fn view(&mut self, view: &ViewDefinition) -> Result<()> {
        let a = self.audit;
        a.pay(8)?;
        a.nonempty(&view.label)?;
        a.pay(8)?;
        let mut env = Environment::new();
        match &view.kind {
            ViewKind::List {
                entity,
                rows,
                columns,
                controls,
                selection,
            } => {
                self.entity(entity)?;
                self.expect(rows, &env, &a.list_type(a.reference_type(entity)?)?, 0, true)?;
                a.insert_string(&mut env, "row", a.reference_type(entity)?)?;
                self.columns(columns, &env)?;
                a.unique(
                    controls
                        .iter()
                        .map(|c| c.id.as_str())
                        .chain(selection.iter().map(|s| s.id.as_str())),
                )?;
                a.slice(controls.len())?;
        for control in controls {
            a.pay(8)?;
                    a.nonempty(&control.label)?;
                    let typ = &self.state(&control.state)?.value_type;
                    a.require(
                        typ.is_scalar()
                            || matches!(typ, Type::Optional { item } if item.is_scalar()),
                        "control must edit a scalar",
                    )?;
                    self.on_change(control.on_change.as_deref(), typ)?;
                }
                if let Some(selection) = selection {
                    let typ = &self.state(&selection.state)?.value_type;
                    a.require(
                        a.type_equal(typ, &a.list_type(a.reference_type(entity)?)?)?,
                        "selection state has wrong entity/type",
                    )?;
                    self.on_change(selection.on_change.as_deref(), typ)?;
                }
            }
            ViewKind::Detail {
                entity,
                record,
                columns,
            } => {
                self.entity(entity)?;
                self.expect(record, &env, &a.reference_type(entity)?, 0, true)?;
                a.insert_string(&mut env, "row", a.reference_type(entity)?)?;
                self.columns(columns, &env)?;
            }
            ViewKind::Form {
                action,
                fields,
                defaults,
            } => {
                let action = self.action(action)?;
                a.unique(fields.iter().map(|f| f.parameter.as_str()))?;
                a.slice(fields.len())?;
        for field in fields {
            a.pay(8)?;
                    a.nonempty(&field.label)?;
                    a.require(
                        a.has(&action.parameters, &field.parameter)?,
                        "unknown form parameter",
                    )?;
                }
                a.map_iter(defaults.len())?;
        for (id, value) in defaults {
            a.pay(8)?;
                    let typ = match a.get(&action.parameters, id)? {
                        Some(value) => value, None => return Err(a.error("unknown default parameter")?),
                    };
                    validate_value_audited(value, typ, 0, a)?;
                }
                a.map_iter(action.parameters.len())?;
        for id in action.parameters.keys() {
            a.pay(8)?;
                    a.require(
                        a.has(defaults, id)? || a.find(fields, id, |f| &f.parameter)?.is_some(),
                        "form omits required action parameter",
                    )?;
                }
            }
        }
        a.unique(view.actions.iter().map(|a| a.id.as_str()))?;
        a.slice(view.actions.len())?;
        for binding in &view.actions {
            a.pay(8)?;
            let env = if binding.placement == ActionPlacement::Row {
                a.require(
                    a.has(&env, "row")?,
                    "row action requires a list or detail view",
                )?;
                a.clone_owned(&env)?
            } else {
                { a.pay(8)?; Environment::new() }
            };
            a.nonempty(&binding.label)?;
            let action = self.action(&binding.action)?;
            a.require(
                binding.arguments.len() == action.parameters.len(),
                "action argument count mismatch",
            )?;
            a.map_iter(action.parameters.len())?;
        for (name, typ) in &action.parameters {
            a.pay(8)?;
                let value = match a.get(&binding.arguments, name)? {
                    Some(value) => value, None => return Err(a.error("missing action argument")?),
                };
                self.expect(value, &env, typ, 0, true)?;
            }
            self.expect(&binding.enabled, &env, &Type::Boolean, 0, true)?;
        }
        a.bounded(view.keys.len())?;
        a.pay(8)?;
        let mut seen = BTreeSet::new();
        a.slice(view.keys.len())?;
        for key in &view.keys {
            a.pay(8)?;
            a.scaled(key.key.len(), 4, 40)?;
            a.require(valid_key(&key.key), "unsupported keyboard key")?;
            a.insert::<(&String, &BTreeSet<KeyModifier>), ()>(seen.len(), a.key_comparator(key)?)?;
            a.require(
                seen.insert((&key.key, &key.modifiers)),
                "duplicate keyboard chord",
            )?;
            a.require(
                a.find(&view.actions, &key.binding, |v| &v.id)?.is_some(),
                "unknown keyboard action binding",
            )?;
        }
        Ok(())
    }
}
pub fn valid_key(key: &str) -> bool {
    (key.len() == 1
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
        || matches!(
            key,
            "enter"
                | "escape"
                | "space"
                | "arrow_up"
                | "arrow_down"
                | "arrow_left"
                | "arrow_right"
                | "delete"
                | "backspace"
                | "tab"
        )
}
pub fn validate_value(value: &DataValue, typ: &Type, depth: usize) -> Result<()> {
    validate_value_audited(value, typ, depth, Audit::none())
}
fn validate_value_audited(value: &DataValue, typ: &Type, depth: usize, a: Audit<'_>) -> Result<()> {
    a.pay(8)?;
    a.require(depth <= MAX_TYPE_DEPTH, "value nesting budget exceeded")?;
    if let Type::Optional { item } = typ {
        if matches!(value, DataValue::Null) {
            return Ok(());
        }
        return validate_value_audited(value, item, depth + 1, a);
    }
    match (value, typ) {
        (DataValue::Boolean { .. }, Type::Boolean) | (DataValue::Integer { .. }, Type::Integer) => {
            Ok(())
        }
        (DataValue::Text { value }, Type::Text) => a.text(value),
        (DataValue::Date { days }, Type::Date) => a.day(*days),
        (DataValue::Reference { entity, record }, Type::Reference { entity: expected }) => {
            a.check_id(entity)?;
            a.check_id(record)?;
            a.require(a.eq_str(entity, expected)?, "reference entity type mismatch")
        }
        (DataValue::List { items, item_type }, Type::List { item }) => {
            a.require(
                items.len() <= MAX_COLLECTION && a.type_equal(item_type, item)?,
                "collection bound or element type mismatch",
            )?;
            a.slice(items.len())?;
            for value in items {
                validate_value_audited(value, item, depth + 1, a)?;
            }
            Ok(())
        }
        _ => Err(a.error("value does not match declared type")?),
    }
}
pub fn validate_day(day: i32) -> Result<()> {
    require(
        (-719_162..=2_932_896).contains(&day),
        "date outside years 0001 through 9999",
    )
}
impl AppDefinition {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let app: Self = parse(bytes)?;
        app.validate()?;
        Ok(app)
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_audited(Audit::none()).map(|_| ())
    }
    fn validate_audited(&self, a: Audit<'_>) -> Result<capture_validation::CaptureFacts> {
        validate_app_semantics!(self, a, validator, action_prefix_nodes);
        // Keep the ordinary Vec alive through the original validation tail.
        let bytes;
        let canonical_len = if a.enabled() {
            capture_canonical::app_canonical_len_audited(self, a)?
        } else {
            bytes = capture_canonical::canonical_bytes_audited(self, a)?;
            bytes.len()
        };
        a.pay(3)?;
        a.require(canonical_len <= MAX_WIRE_BYTES, "program exceeds byte budget")?;
        a.pay(5)?;
        Ok(capture_validation::CaptureFacts { total_nodes: validator.nodes,
            action_prefix_nodes, canonical_len, action_count: self.actions.len() })
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Program, self)
    }
    pub fn semantic_identity(&self) -> Result<Digest> {
        self.validate()?;
        let mut app = self.clone();
        app.clear_display_labels_audited(Audit::none())?;
        canonical_digest(IdentityDomain::SemanticProgram, &app)
    }
}
impl AppDefinition {
    fn clear_display_labels_audited(&mut self,a:Audit<'_>)->Result<()> {
        a.pay(1)?;
        a.pay(1)?;
        self.label.clear();
        a.slice(self.entities.len())?;
        for entity in &mut self.entities {
            a.pay(1)?;
            entity.label.clear();
            a.slice(entity.fields.len())?;
        for field in &mut entity.fields {
                a.pay(1)?;
                field.label.clear();
            }
        }
        a.slice(self.state.len())?;
        for state in &mut self.state {
            a.pay(1)?;
            state.label.clear();
        }
        a.slice(self.actions.len())?;
        for action in &mut self.actions {
            a.pay(1)?;
            action.label.clear();
        }
        a.slice(self.outputs.len())?;
        for output in &mut self.outputs {
            a.pay(1)?;
            output.label.clear();
            a.slice(output.columns.len())?;
        for column in &mut output.columns {
                a.pay(1)?;
                column.label.clear();
            }
        }
        a.slice(self.observables.len())?;
        for observable in &mut self.observables {
            a.pay(1)?;
            observable.label.clear();
        }
        a.slice(self.views.len())?;
        for view in &mut self.views {
            a.pay(1)?;
            view.label.clear();
            a.slice(view.actions.len())?;
        for action in &mut view.actions {
                a.pay(1)?;
                action.label.clear();
            }
            match &mut view.kind {
                ViewKind::List {
                    columns, controls, ..
                } => {
                    a.slice(columns.len())?;
                    for column in columns {
                        a.pay(1)?;
                        column.label.clear();
                    }
                    a.slice(controls.len())?;
                    for control in controls {
                        a.pay(1)?;
                        control.label.clear();
                    }
                }
                ViewKind::Detail { columns, .. } => {
                    a.slice(columns.len())?;
                    for column in columns {
                        a.pay(1)?;
                        column.label.clear();
                    }
                }
                ViewKind::Form { fields, .. } => {
                    a.slice(fields.len())?;
                    for field in fields {
                        a.pay(1)?;
                        field.label.clear();
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_entities(entities: &[EntityDefinition], validator: &mut Validator<'_, '_>) -> Result<()> {
    let a = validator.audit;
    a.pay(1)?;
    a.unique(entities.iter().map(|e| e.id.as_str()))?;
    a.slice(entities.len())?;
    for entity in entities {
            a.pay(12)?;
        a.nonempty(&entity.label)?;
        a.require(!entity.fields.is_empty(), "entity needs fields")?;
        a.unique(entity.fields.iter().map(|f| f.id.as_str()))?;
        a.slice(entity.fields.len())?;
    for field in &entity.fields {
            a.pay(12)?;
            a.nonempty(&field.label)?;
            validator.typ(&field.value_type, 0)?;
        }
        a.bounded(entity.unique.len())?;
        a.pay(8)?;
        let mut tuples = BTreeSet::new();
        a.slice(entity.unique.len())?;
    for tuple in &entity.unique {
            a.pay(12)?;
            a.require(!tuple.is_empty(), "empty unique constraint")?;
            a.unique(tuple.iter().map(String::as_str))?;
            a.insert::<&Vec<Id>, ()>(tuples.len(), a.tuple_comparator(tuple)?)?;
            a.require(tuples.insert(tuple), "duplicate unique constraint")?;
            a.slice(tuple.len())?;
    for field in tuple {
            a.pay(12)?;
                let typ = &validator.field(&entity.id, field)?.value_type;
                a.require(
                    typ.is_scalar() || matches!(typ,Type::Optional { item } if item.is_scalar()),
                    "unique constraint needs scalar fields",
                )?;
            }
        }
        a.bounded(entity.constraints.len())?;
        a.string(6)?;
        a.string(entity.id.len())?;
        capture_canonical::reserve_from_iter::<String,Type>(1,a.comparator(6)?,a)?;
        let env=BTreeMap::from([("record".into(),Type::reference(&entity.id))]);
        a.slice(entity.constraints.len())?;
    for constraint in &entity.constraints {
            a.pay(12)?;
            validator.expect(constraint, &env, &Type::Boolean, 0, false)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    GeneratedApp,
    LegacyOrder,
    ExternalWeb,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub kind: ArtifactKind,
    pub version: u32,
    pub language: String,
    /// All original bytes, including whitespace, labels and source metadata.
    pub raw_digest: Digest,
    /// Canonical complete typed executable/view source; labels are included.
    pub program_digest: Digest,
    /// Only declared display labels are excluded. Not proof of equivalent behavior.
    pub semantic_digest: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Producer {
    LiveAgent {
        provider: String,
        invocation_id: String,
        session_id: Option<String>,
        request_digest: Digest,
    },
    ExternalAuthor {
        description: String,
    },
    UserAuthored,
    Fixture {
        name: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSource {
    pub task_id: Id,
    pub source_fingerprint: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub project_id: Id,
    pub program_path: String,
    pub task: Option<TaskSource>,
    pub producer: Producer,
    pub source_digest: Digest,
}
impl SourceBinding {
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none())}
    fn validate_audited(&self,a:Audit<'_>)->Result<()> {
        a.pay(6)?;
        a.check_id(&self.project_id)?;
        validate_relative_path_audited(&self.program_path,a)?;
        if let Some(task) = &self.task {
            a.check_id(&task.task_id)?;
            a.nonempty(&task.source_fingerprint)?;
        }
        self.producer.validate_audited(a)?;
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Source, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedProgram {
    pub artifact: ArtifactRef,
    pub binding: SourceBinding,
    pub source_bytes: Vec<u8>,
    pub program: AppDefinition,
}
/// An exact, owned immutable capture plus pure static observation metadata.
/// Only successful full capture validation can mint this capability. It is
/// deliberately neither serializable nor mutable, and never stores run state.
#[derive(Clone)]
pub(crate) struct PreparedProgram(Arc<PreparedProgramInner>);
struct PreparedProgramInner {
    capture: CapturedProgram,
    observable_types: BTreeMap<Id, Type>,
    view_schemas: BTreeMap<Id, Option<ViewSchema>>,
}
impl PreparedProgram {
    pub(crate) fn new(capture: &CapturedProgram) -> Result<Self> {
        capture.validate()?;
        let observable_types = capture.program.observable_types_validated()?;
        let view_schemas = capture
            .program
            .views
            .iter()
            .map(|view| {
                Ok((
                    view.id.clone(),
                    capture.program.view_schema_validated(&view.id)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self(Arc::new(PreparedProgramInner {
            capture: capture.clone(),
            observable_types,
            view_schemas,
        })))
    }
    pub(crate) fn capture(&self) -> &CapturedProgram {
        &self.0.capture
    }
    pub(crate) fn initial_session(&self) -> SessionState {
        SessionState::initial_validated(&self.0.capture.program)
    }
    pub(crate) fn validate_session(&self, session: &SessionState) -> Result<()> {
        session.validate_for_app(&self.0.capture.program)
    }
    pub(crate) fn validate_scenario(&self, scenario: &ScenarioSpec) -> Result<()> {
        scenario.validate_prefix()?;
        self.validate_session(&scenario.session)?;
        scenario.validate_suffix(&self.0.capture.program)
    }
    pub(crate) fn observable_types(&self) -> &BTreeMap<Id, Type> {
        &self.0.observable_types
    }
    pub(crate) fn view_schema(&self, view: &str) -> Result<&Option<ViewSchema>> {
        self.0
            .view_schemas
            .get(view)
            .ok_or_else(|| ContractError("unknown view".into()))
    }
}
impl CapturedProgram {
    pub fn capture(
        bytes: &[u8],
        project: &str,
        producer: Producer,
        task: Option<TaskSource>,
    ) -> Result<Self> {
        let program = AppDefinition::parse(bytes)?;
        let raw_digest = bytes_digest(IdentityDomain::Source, bytes);
        let artifact = ArtifactRef {
            kind: ArtifactKind::GeneratedApp,
            version: CONTRACT_VERSION,
            language: LANGUAGE_VERSION.into(),
            raw_digest: raw_digest.clone(),
            program_digest: program.identity()?,
            semantic_digest: program.semantic_identity()?,
        };
        let binding = SourceBinding {
            project_id: project.into(),
            program_path: "program.json".into(),
            task,
            producer,
            source_digest: raw_digest,
        };
        binding.validate()?;
        Ok(Self {
            artifact,
            binding,
            source_bytes: bytes.to_vec(),
            program,
        })
    }
    pub fn at_path(mut self, path: &str) -> Result<Self> {
        validate_relative_path(path)?;
        self.binding.program_path = path.into();
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none()).map(|_|())}
    fn validate_audited(&self,a:Audit<'_>)->Result<capture_validation::CaptureFacts> {
        a.pay(8)?;
        #[cfg(test)]
        count_validation(|counts| counts.captures += 1);
        self.binding.validate_audited(a)?;
        a.version(self.artifact.version)?;
        a.require(self.artifact.kind == ArtifactKind::GeneratedApp
            && a.eq_str(&self.artifact.language, LANGUAGE_VERSION)?, "unsupported executable artifact")?;
        a.require(self.source_bytes.len() <= MAX_WIRE_BYTES, "source exceeds byte limit")?;
        let actual: AppDefinition = if a.enabled() {
            // Both roots precede the new helper, closures, and borrowed owners.
            a.pay(capture_canonical::COMMON)?;
            a.pay(capture_canonical::SOURCE_DELTA_ROOT)?;
            return capture_canonical::validate_guarded_native_capture(self, a);
        } else {parse(&self.source_bytes)?};
        let facts=actual.validate_audited(a)?;
        a.require(a.equal_owned(&actual,&self.program)?, "source bytes do not represent captured program")?;
        let raw=capture_validation::bytes_digest_audited(IdentityDomain::Source,&self.source_bytes,a)?;
        a.require(a.eq_str(self.artifact.raw_digest.as_str(),raw.as_str())?
            && a.eq_str(self.binding.source_digest.as_str(),self.artifact.raw_digest.as_str())?, "raw source digest mismatch")?;
        a.pay(8 + 2 * std::mem::size_of::<Audit<'_>>() as u64 + 4 * std::mem::size_of::<bool>() as u64)?;
        if !a.enabled() { actual.validate_audited(a)?; }
        let bytes=capture_canonical::canonical_bytes_audited(&actual,a)?;
        let program=capture_validation::bytes_digest_audited(IdentityDomain::Program,&bytes,a)?;
        let equal=a.eq_str(self.artifact.program_digest.as_str(),program.as_str())?;
        let equal=if equal {
            a.pay(8 + 2 * std::mem::size_of::<Audit<'_>>() as u64 + 4 * std::mem::size_of::<bool>() as u64)?;
            if !a.enabled() { actual.validate_audited(a)?; }
            let mut semantic=a.clone_owned(&actual)?;
            semantic.clear_display_labels_audited(a)?;
            let bytes=capture_canonical::canonical_bytes_audited(&semantic,a)?;
            let digest=capture_validation::bytes_digest_audited(IdentityDomain::SemanticProgram,&bytes,a)?;
            a.eq_str(self.artifact.semantic_digest.as_str(),digest.as_str())?
        } else {false};
        a.require(equal,"program digest mismatch")?;
        Ok(facts)
    }
}
pub fn validate_relative_path(path: &str) -> Result<()> {validate_relative_path_audited(path,Audit::none())}
fn validate_relative_path_audited(path: &str,a:Audit<'_>)->Result<()> {
    if !a.enabled() {
        return require(!path.is_empty() && path.len() <= 1024
            && !path.contains(['\\', ':', '\0']) && !path.chars().any(char::is_control)
            && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
            "unsafe relative source path");
    }
    a.pay(8)?;
    let valid=if path.is_empty() || path.len()>1024 {false} else {
        // Native three-character search: UTF-8 decode + three candidate tests.
        a.scaled(path.len(),7,4)?;
        if path.contains(['\\', ':', '\0']) {false} else {
            // Native control predicate: decode + finite C0/C1 range checks.
            a.scaled(path.len(),6,4)?;
            if path.chars().any(char::is_control) {false} else {
                a.scaled(path.len(),1,2)?; // Original slash search bytes.
                let mut valid=true;
                for part in path.split('/') {
                    a.pay(6)?;
                    if part.is_empty() || a.eq_str(part,".")? || a.eq_str(part,"..")? {valid=false;break;}
                }
                a.pay(1)?;valid
            }
        }
    };
    a.require(valid,"unsafe relative source path")
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocus {
    pub relative_path: String,
    pub raw_digest: Digest,
    pub pointer: String,
}
impl SourceLocus {
    pub fn validate_against(&self, source: &CapturedProgram) -> Result<()> {
        source.validate()?;
        validate_relative_path(&self.relative_path)?;
        text(&self.pointer)?;
        require(
            self.relative_path == source.binding.program_path
                && self.raw_digest == source.artifact.raw_digest,
            "source locus refers to another artifact",
        )?;
        require(
            self.pointer.is_empty() || self.pointer.starts_with('/'),
            "invalid JSON pointer",
        )?;
        let bytes = self.pointer.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'~' {
                require(
                    matches!(bytes.get(i + 1), Some(b'0' | b'1')),
                    "invalid JSON pointer escape",
                )?;
            }
        }
        let value = bounded_input::parse_json_bytes(&source.source_bytes)
            .map_err(|e| ContractError(e.to_string()))?;
        require(
            value.pointer(&self.pointer).is_some(),
            "source locus does not exist",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub entity: Id,
    pub id: Id,
    pub revision: u64,
    pub created_program: Digest,
    pub archived: bool,
    pub values: Values,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordChange {
    pub entity: Id,
    pub record: Id,
    pub before: Option<Values>,
    pub after: Values,
    pub archived: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessEvent {
    pub id: Id,
    pub sequence: u64,
    pub operation_id: Id,
    pub action: Id,
    pub day: i32,
    pub program: Digest,
    pub changes: Vec<RecordChange>,
    pub outputs: Vec<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSnapshot {
    pub version: u32,
    pub project_id: Id,
    pub generation: u64,
    /// Cumulative storage schema. Retained fields/entities stay inspectable even
    /// when an older active program does not expose them. Never drop on adoption.
    pub schema: Vec<EntityDefinition>,
    pub records: Vec<Record>,
    pub events: Vec<BusinessEvent>,
}
impl DataSnapshot {
    pub fn empty(project: &str, app: &AppDefinition) -> Result<Self> {
        app.validate()?;
        check_id(project)?;
        Ok(Self {
            version: CONTRACT_VERSION,
            project_id: project.into(),
            generation: 0,
            schema: app.entities.clone(),
            records: vec![],
            events: vec![],
        })
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let data: Self = parse(bytes)?;
        data.validate()?;
        Ok(data)
    }
    pub fn validate(&self) -> Result<()> { self.validate_audited(Audit::none()) }
    fn validate_audited(&self, a: Audit<'_>) -> Result<()> {
        a.pay(8)?;
        a.version(self.version)?;
        a.check_id(&self.project_id)?;
        a.require(
            self.records.len() <= MAX_COLLECTION && self.events.len() <= MAX_EVENTS,
            "data record/event budget exceeded",
        )?;
        a.pay(8)?;
        let mut validator = Validator { entities: &self.schema, state: &[], actions: &[],
            outputs: &[], nodes: 0, audit: a };
        validate_entities(&self.schema, &mut validator)?;
        a.pay(8)?;
        let mut records = BTreeSet::new();
        a.slice(self.records.len())?;
        for record in &self.records {
            a.pay(12)?;
            a.check_id(&record.id)?;
            a.require(record.revision > 0, "record revision must be positive")?;
            a.insert::<(&str, &str), ()>(records.len(), a.pair_comparator(&record.entity, &record.id)?)?;
            a.require(
                records.insert((record.entity.as_str(), record.id.as_str())),
                "duplicate record identity",
            )?;
            validate_record_values(&validator, &record.entity, &record.values)?;
        }
        a.slice(self.records.len())?;
        for record in &self.records {
            a.pay(12)?;
            a.map_iter(record.values.len())?;
        for value in record.values.values() {
            a.pay(12)?;
                validate_references_audited(value, &records, a)?;
            }
        }
        a.slice(self.schema.len())?;
        for entity in &self.schema {
            a.pay(12)?;
            a.slice(entity.unique.len())?;
        for fields in &entity.unique {
            a.pay(12)?;
                a.pay(8)?;
                let mut tuples = BTreeSet::new();
                a.slice(self.records.len())?;
        for record in &self.records {
            a.pay(12)?;
                    if !(a.eq_str(&record.entity, &entity.id)? && !record.archived) { continue; }
                    a.pay(12)?;
                    a.scaled(fields.len(), (2 * std::mem::size_of::<&DataValue>()) as u64, 4)?;
                    if a.enabled() {
                        a.slice(fields.len())?;
                        for field in fields {
                            a.pay(5)?; // Native collect element dispatch/transfer/cleanup.
                            a.map_search(record.values.len(),field.len())?;
                        }
                    }
                    let tuple: Vec<_> = fields.iter()
                        .map(|field|record.values.get(field).unwrap_or(&DataValue::Null)).collect();
                    let mut has_null = false;
                    a.pay(1)?;
                    for value in &tuple {
            a.pay(12)?;
                        a.pay(4)?;
                        if matches!(value, DataValue::Null) { has_null = true; break; }
                    }
                    a.pay(1)?;
                    if has_null { continue; }
                    let bytes = capture_canonical::canonical_bytes_audited(&tuple, a)?;
                    a.insert::<Vec<u8>, ()>(tuples.len(), a.comparator(bytes.len())?)?;
                    a.require(tuples.insert(bytes), "duplicate unique field tuple")?;
                }
            }
        }
        a.pay(17)?;
        let mut events = BTreeSet::new();
        let mut operations = BTreeSet::new();
        let mut previous = 0;
        a.slice(self.events.len())?;
        for event in &self.events {
            a.pay(12)?;
            a.check_id(&event.id)?;
            a.check_id(&event.operation_id)?;
            a.check_id(&event.action)?;
            a.day(event.day)?;
            a.insert::<&String, ()>(events.len(), a.comparator(event.id.len())?)?;
            let new_event = events.insert(&event.id);
            let distinct = if new_event {
                a.insert::<&String, ()>(operations.len(), a.comparator(event.operation_id.len())?)?;
                operations.insert(&event.operation_id)
            } else { false };
            a.require(
                distinct,
                "duplicate event or operation receipt",
            )?;
            a.require(
                event.sequence > previous && event.sequence <= self.generation,
                "event sequence is not ordered within generation",
            )?;
            previous = event.sequence;
            a.bounded(event.changes.len())?;
            a.bounded(event.outputs.len())?;
            a.pay(8)?;
            let mut changed = BTreeSet::new();
            a.slice(event.changes.len())?;
        for change in &event.changes {
            a.pay(12)?;
                a.check_id(&change.record)?;
                a.insert::<(&String, &String), ()>(changed.len(), a.pair_comparator(&change.entity, &change.record)?)?;
                a.require(
                    changed.insert((&change.entity, &change.record)),
                    "duplicate event record change",
                )?;
                validator.entity(&change.entity)?;
                if a.enabled() {
                    a.pay(bounded_input::capture_payment_core::SEARCH_DESCRIPTOR)?;
                    a.amount(bounded_input::capture_payment_core::btree_search(records.len(), a.pair_comparator(&change.entity, &change.record)?))?;
                }
                a.require(
                    records.contains(&(change.entity.as_str(), change.record.as_str())),
                    "event record is missing",
                )?;
                // Historical maps may omit later-added fields, but never change
                // the meaning/type of values that were actually recorded.
                a.pay(8)?;
        for values in change.before.iter().chain(std::iter::once(&change.after)) {
            a.pay(12)?;
                    a.bounded(values.len())?;
                    a.map_iter(values.len())?;
        for (id, value) in values {
            a.pay(12)?;
                        validate_value_audited(value, &validator.field(&change.entity, id)?.value_type, 0, a)?;
                        validate_references_audited(value, &records, a)?;
                    }
                }
            }
        }
        capture_canonical::data_serialized_bound_audited(self, a)?;
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Data, self)
    }
    pub fn schema_identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Schema, &self.schema)
    }
}
fn validate_record_values(validator: &Validator<'_, '_>, entity: &str, values: &Values) -> Result<()> {
    let a = validator.audit;
    a.pay(4)?;
    let entity = validator.entity(entity)?;
    a.bounded(values.len())?;
    a.map_iter(values.len())?;
    for (id, value) in values {
        validate_value_audited(value, &validator.field(&entity.id, id)?.value_type, 0, a)?;
    }
    a.slice(entity.fields.len())?;
    for field in &entity.fields {
        a.require(
            a.has(values, &field.id)? || matches!(field.value_type, Type::Optional { .. }),
            "record lacks required field",
        )?;
    }
    Ok(())
}
fn validate_references(value: &DataValue, records: &BTreeSet<(&str, &str)>) -> Result<()> {
    validate_references_audited(value, records, Audit::none())
}
fn validate_references_audited(value: &DataValue, records: &BTreeSet<(&str, &str)>, a: Audit<'_>) -> Result<()> {
    a.pay(4)?;
    match value {
        DataValue::Reference { entity, record } => {
            if a.enabled() {
                a.pay(bounded_input::capture_payment_core::SEARCH_DESCRIPTOR)?;
                a.amount(bounded_input::capture_payment_core::btree_search(records.len(), a.pair_comparator(entity, record)?))?;
            }
            a.require(records.contains(&(entity.as_str(), record.as_str())), "dangling record reference")
        },
        DataValue::List { items, .. } => {
            a.slice(items.len())?;
            for item in items {
                validate_references_audited(item, records, a)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub view: Id,
    pub values: Values,
    pub focused_record: Option<RecordRef>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordRef {
    pub entity: Id,
    pub record: Id,
}
impl SessionState {
    pub fn initial(app: &AppDefinition) -> Result<Self> {
        app.validate()?;
        Ok(Self::initial_validated(app))
    }
    fn initial_validated(app: &AppDefinition) -> Self {
        Self {
            view: app.initial_view.clone(),
            values: app
                .state
                .iter()
                .map(|s| (s.id.clone(), s.initial.clone()))
                .collect(),
            focused_record: None,
        }
    }
    pub fn validate(&self, app: &AppDefinition) -> Result<()> {
        app.validate()?;
        self.validate_for_app(app)
    }
    fn validate_for_app(&self, app: &AppDefinition) -> Result<()> {
        require(
            app.views.iter().any(|v| v.id == self.view),
            "unknown session view",
        )?;
        require(
            self.values.len() == app.state.len(),
            "session state differs from program",
        )?;
        for state in &app.state {
            validate_value(
                self.values
                    .get(&state.id)
                    .ok_or_else(|| ContractError("missing session state".into()))?,
                &state.value_type,
                0,
            )?;
        }
        if let Some(record) = &self.focused_record {
            check_id(&record.record)?;
            require(
                app.entities.iter().any(|e| e.id == record.entity),
                "focused record entity is unknown",
            )?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        canonical_digest(IdentityDomain::Session, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticInput {
    Invoke {
        action: Id,
        arguments: Values,
    },
    Control {
        view: Id,
        control: Id,
        value: DataValue,
    },
    Activate {
        view: Id,
        binding: Id,
        row: Option<RecordRef>,
    },
    Submit {
        view: Id,
        arguments: Values,
    },
    Navigate {
        view: Id,
    },
    AdvanceClock {
        days: u32,
    },
    Observe {
        point: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub version: u32,
    pub id: Id,
    pub label: String,
    pub seed: DataSnapshot,
    pub session: SessionState,
    pub clock_day: i32,
    pub random_seed: u64,
    pub inputs: Vec<SemanticInput>,
    /// Independently chosen workflow-validity obligations. Failure invalidates a
    /// trial; it must not be mistaken for a discriminating behavioral difference.
    pub validity: Vec<AcceptedProperty>,
}
impl ScenarioSpec {
    pub fn identity(&self) -> Result<Digest> {
        canonical_digest(IdentityDomain::Scenario, self)
    }
    pub fn input_identity(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Input,
            &(
                &self.seed,
                &self.session,
                self.clock_day,
                self.random_seed,
                &self.inputs,
            ),
        )
    }
    /// Stable synthetic execution namespace. Full source, schema and inputs stay
    /// in run bindings; instrumentation and implementation names cannot alter
    /// IDs of earlier created records. Existing seed records retain exact IDs.
    pub fn operation_namespace(&self) -> Result<Digest> {
        self.seed.validate()?;
        validate_day(self.clock_day)?;
        let mut records: Vec<_> = self.seed.records.iter().collect();
        records.sort_by(|a, b| (&a.entity, &a.id).cmp(&(&b.entity, &b.id)));
        canonical_digest(
            IdentityDomain::Input,
            &(
                "semantic-replay/2",
                &self.seed.project_id,
                self.seed.generation,
                records,
                &self.seed.events,
                self.clock_day,
                self.random_seed,
            ),
        )
    }
    /// Shared replay-only driver. Adding/reordering action-bearing inputs changes
    /// mutation ordinals and needs explicit semantic correspondence; it is not
    /// silently treated as equivalent. This does not allocate live store IDs.
    pub fn replay_operation_ids(&self) -> Result<Vec<Id>> {
        bounded(self.inputs.len())?;
        let namespace = self.operation_namespace()?;
        let mut mutation = 0usize;
        self.inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                validate_input_shape(input)?;
                let id = if matches!(
                    input,
                    SemanticInput::Invoke { .. }
                        | SemanticInput::Control { .. }
                        | SemanticInput::Activate { .. }
                        | SemanticInput::Submit { .. }
                ) {
                    let id = format!("replay-mutation-{}-{mutation}", namespace.as_str());
                    mutation += 1;
                    id
                } else {
                    format!("replay-step-{}-{index}", namespace.as_str())
                };
                check_id(&id)?;
                Ok(id)
            })
            .collect()
    }
    pub fn validate(&self, app: &AppDefinition) -> Result<()> {
        self.validate_prefix()?;
        self.session.validate(app)?;
        self.validate_suffix(app)
    }
    fn validate_prefix(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        nonempty(&self.label)?;
        self.seed.validate()
    }
    fn validate_suffix(&self, app: &AppDefinition) -> Result<()> {
        validate_day(self.clock_day)?;
        bounded(self.inputs.len())?;
        bounded(self.validity.len())?;
        let mut points = BTreeSet::new();
        for input in &self.inputs {
            validate_input(input, app)?;
            if let SemanticInput::Observe { point } = input {
                require(points.insert(point), "duplicate observation point")?;
            }
        }
        for property in &self.validity {
            property.validate()?;
        }
        Ok(())
    }
}
pub fn validate_input(input: &SemanticInput, app: &AppDefinition) -> Result<()> {
    let validator = Validator::new(app, Audit::none());
    let view = |id: &str| {
        app.views
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| ContractError("unknown input view".into()))
    };
    let arguments = |action: &ActionDefinition, values: &Values| -> Result<()> {
        require(
            values.len() == action.parameters.len(),
            "input argument count mismatch",
        )?;
        for (id, typ) in &action.parameters {
            validate_value(
                values
                    .get(id)
                    .ok_or_else(|| ContractError("missing input argument".into()))?,
                typ,
                0,
            )?;
        }
        Ok(())
    };
    match input {
        SemanticInput::Invoke {
            action,
            arguments: values,
        } => arguments(validator.action(action)?, values),
        SemanticInput::Submit {
            view: id,
            arguments: values,
        } => {
            let ViewKind::Form { action, .. } = &view(id)?.kind else {
                return Err(ContractError("submit requires form view".into()));
            };
            arguments(validator.action(action)?, values)
        }
        SemanticInput::Control {
            view: id,
            control,
            value,
        } => {
            let ViewKind::List {
                controls,
                selection,
                ..
            } = &view(id)?.kind
            else {
                return Err(ContractError("control requires list view".into()));
            };
            let state = controls
                .iter()
                .find(|c| &c.id == control)
                .map(|c| &c.state)
                .or_else(|| {
                    selection
                        .as_ref()
                        .filter(|s| &s.id == control)
                        .map(|s| &s.state)
                })
                .ok_or_else(|| ContractError("unknown input control".into()))?;
            validate_value(value, &validator.state(state)?.value_type, 0)
        }
        SemanticInput::Activate {
            view: id,
            binding,
            row,
        } => {
            let view = view(id)?;
            let action = view
                .actions
                .iter()
                .find(|a| &a.id == binding)
                .ok_or_else(|| ContractError("unknown input action binding".into()))?;
            require(
                (action.placement == ActionPlacement::Row) == row.is_some(),
                "action row context does not match placement",
            )?;
            if let Some(row) = row {
                check_id(&row.record)?;
                let entity = match &view.kind {
                    ViewKind::List { entity, .. } | ViewKind::Detail { entity, .. } => entity,
                    _ => return Err(ContractError("form action cannot bind row".into())),
                };
                require(&row.entity == entity, "input row belongs to another entity")?;
            }
            Ok(())
        }
        SemanticInput::Navigate { view: id } => {
            view(id)?;
            Ok(())
        }
        SemanticInput::AdvanceClock { days } => {
            require(*days <= 3_652_058, "clock advance out of bounds")
        }
        SemanticInput::Observe { point } => check_id(point),
    }
}

/// Terms name stable business observables, not old functions or source offsets.
/// An unmapped term is unknown/unsupported, never an implicitly satisfied rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyTerm {
    Literal {
        value_type: Type,
        value: DataValue,
    },
    Observed {
        point: Id,
        observable: Id,
        value_type: Type,
    },
    ViewRows {
        point: Id,
        entity: Id,
    },
    /// Canonical references of displayed rows with this exact binding enabled.
    RowActionRows {
        point: Id,
        view: Id,
        entity: Id,
        binding: Id,
        action: Id,
    },
    ViewColumn {
        point: Id,
        column: Id,
        value_type: Type,
    },
    OutputCount {
        point: Id,
        output: Id,
    },
    OutputColumn {
        point: Id,
        output: Id,
        column: Id,
        value_type: Type,
    },
    Count {
        value: Box<PropertyTerm>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyPredicate {
    Equal {
        left: PropertyTerm,
        right: PropertyTerm,
    },
    Less {
        left: PropertyTerm,
        right: PropertyTerm,
    },
    And {
        values: Vec<PropertyPredicate>,
    },
    Or {
        values: Vec<PropertyPredicate>,
    },
    Not {
        value: Box<PropertyPredicate>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedProperty {
    pub id: Id,
    pub description: String,
    pub predicate: PropertyPredicate,
}
fn standalone_type(typ: &Type, depth: usize) -> Result<()> {
    require(depth <= MAX_TYPE_DEPTH, "type nesting budget exceeded")?;
    match typ {
        Type::Reference { entity } => check_id(entity),
        Type::List { item } | Type::Optional { item } => standalone_type(item, depth + 1),
        _ => Ok(()),
    }
}
impl PropertyTerm {
    fn typ(&self, depth: usize, nodes: &mut usize) -> Result<Type> {
        *nodes += 1;
        require(
            depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
            "property budget exceeded",
        )?;
        Ok(match self {
            Self::Literal { value_type, value } => {
                standalone_type(value_type, 0)?;
                validate_value(value, value_type, 0)?;
                value_type.clone()
            }
            Self::Observed {
                point,
                observable,
                value_type,
            } => {
                check_id(point)?;
                check_id(observable)?;
                standalone_type(value_type, 0)?;
                value_type.clone()
            }
            Self::ViewRows { point, entity } => {
                check_id(point)?;
                check_id(entity)?;
                Type::list(Type::reference(entity))
            }
            Self::RowActionRows {
                point,
                view,
                entity,
                binding,
                action,
            } => {
                for id in [point, view, entity, binding, action] {
                    check_id(id)?;
                }
                Type::list(Type::reference(entity))
            }
            Self::ViewColumn {
                point,
                column,
                value_type,
            } => {
                check_id(point)?;
                check_id(column)?;
                standalone_type(value_type, 1)?;
                Type::list(value_type.clone())
            }
            Self::OutputCount { point, output } => {
                check_id(point)?;
                check_id(output)?;
                Type::Integer
            }
            Self::OutputColumn {
                point,
                output,
                column,
                value_type,
            } => {
                check_id(point)?;
                check_id(output)?;
                check_id(column)?;
                standalone_type(value_type, 0)?;
                require(
                    value_type.is_scalar(),
                    "output property column must be scalar",
                )?;
                Type::list(value_type.clone())
            }
            Self::Count { value } => {
                require(
                    matches!(value.typ(depth + 1, nodes)?, Type::List { .. }),
                    "count property needs a collection",
                )?;
                Type::Integer
            }
        })
    }
}
impl PropertyPredicate {
    fn validate(&self, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        require(
            depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
            "property budget exceeded",
        )?;
        match self {
            Self::Equal { left, right } | Self::Less { left, right } => {
                let typ = left.typ(depth + 1, nodes)?;
                require(
                    typ == right.typ(depth + 1, nodes)?,
                    "property operand types differ",
                )?;
                if matches!(self, Self::Less { .. }) {
                    require(typ.is_scalar(), "ordered property requires scalar")?;
                }
            }
            Self::And { values } | Self::Or { values } => {
                bounded(values.len())?;
                require(!values.is_empty(), "empty property connective")?;
                for value in values {
                    value.validate(depth + 1, nodes)?;
                }
            }
            Self::Not { value } => value.validate(depth + 1, nodes)?,
        }
        Ok(())
    }
}
impl AcceptedProperty {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        nonempty(&self.description)?;
        self.predicate.validate(0, &mut 0)
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
    /// None means an unmapped, invalid or missing observation, never a pass.
    pub fn evaluate(&self, observations: &[Observation]) -> Option<bool> {
        self.validate().ok()?;
        unique(observations.iter().map(|o| o.point.as_str())).ok()?;
        for observation in observations {
            observation.validate().ok()?;
        }
        self.predicate.evaluate(observations)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalArtifact {
    pub output: Id,
    pub format: OutputFormat,
    pub columns: Vec<FieldDefinition>,
    pub rows: Vec<Values>,
    pub bytes: Vec<u8>,
    pub bytes_digest: Digest,
    pub digest: Digest,
}
impl LocalArtifact {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.output)?;
        validate_output_columns(&self.columns)?;
        require(
            !self.columns.is_empty()
                && self.rows.len() <= MAX_COLLECTION
                && self.bytes.len() <= MAX_OUTPUT_BYTES,
            "artifact size/columns invalid",
        )?;
        for row in &self.rows {
            require(
                row.len() == self.columns.len()
                    && self.columns.iter().all(|c| row.contains_key(&c.id)),
                "artifact row columns mismatch",
            )?;
            for column in &self.columns {
                let value = &row[&column.id];
                validate_value(value, &column.value_type, 0)?;
                require(
                    !matches!(value, DataValue::List { .. } | DataValue::Null),
                    "artifact cells must be scalar",
                )?;
            }
        }
        require(
            self.bytes_digest == Self::bytes_identity(&self.bytes),
            "artifact byte digest mismatch",
        )?;
        require(
            self.bytes == encode_output(self.format, &self.columns, &self.rows)?,
            "artifact bytes do not encode observed rows",
        )?;
        require(
            self.digest == self.receipt_identity()?,
            "artifact semantic receipt mismatch",
        )
    }
    pub fn bytes_identity(bytes: &[u8]) -> Digest {
        bytes_digest(IdentityDomain::OutputBytes, bytes)
    }
    pub fn from_rows(
        output: &str,
        format: OutputFormat,
        columns: Vec<FieldDefinition>,
        rows: Vec<Values>,
    ) -> Result<Self> {
        check_id(output)?;
        let bytes = encode_output(format, &columns, &rows)?;
        let mut artifact = Self {
            output: output.into(),
            format,
            columns,
            rows,
            bytes_digest: Self::bytes_identity(&bytes),
            digest: Self::bytes_identity(&bytes),
            bytes,
        };
        artifact.digest = artifact.receipt_identity()?;
        artifact.validate()?;
        Ok(artifact)
    }
}
fn validate_untyped_value(value: &DataValue, depth: usize) -> Result<()> {
    require(depth <= MAX_TYPE_DEPTH, "value nesting budget exceeded")?;
    match value {
        DataValue::Text { value } => text(value),
        DataValue::Date { days } => validate_day(*days),
        DataValue::Reference { entity, record } => {
            check_id(entity)?;
            check_id(record)
        }
        DataValue::List { item_type, items } => {
            standalone_type(item_type, depth + 1)?;
            require(items.len() <= MAX_COLLECTION, "collection limit exceeded")?;
            for item in items {
                validate_value(item, item_type, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentedRow {
    pub record: RecordRef,
    pub cells: Values,
    pub enabled_actions: BTreeSet<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewObservation {
    pub view: Id,
    pub rows: Vec<PresentedRow>,
    pub controls: Values,
    pub selected: Vec<RecordRef>,
    pub enabled_actions: BTreeSet<Id>,
    pub form_values: Values,
}
/// Runtime-derived type provenance for real list/detail rows, even when empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSchema {
    pub entity: Id,
    pub columns: BTreeMap<Id, Type>,
    /// Complete declarations for row bindings in the actually presented view.
    /// None is historical/untyped evidence, never an empty declaration map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_actions: Option<BTreeMap<Id, Id>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub point: Id,
    pub data_digest: Digest,
    pub session_digest: Digest,
    pub values: Values,
    pub value_types: BTreeMap<Id, Type>,
    pub view: ViewObservation,
    /// Old evidence has no typed view provenance; view properties stay unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_schema: Option<ViewSchema>,
    pub outputs: Vec<LocalArtifact>,
}
impl Observation {
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Observation, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Observed,
    NoDifferenceFound,
    Inconclusive,
    Unsupported,
    Stale,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOrigin {
    ProductionRuntime,
    LegacyRuntime,
    TestFixture,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Applied,
    Rejected,
    Failed,
    BudgetExhausted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceStep {
    pub index: usize,
    pub input: SemanticInput,
    pub before_data: Digest,
    pub after_data: Digest,
    pub before_session: Digest,
    pub after_session: Digest,
    pub outputs: Vec<Digest>,
    pub outcome: StepOutcome,
    pub diagnostic: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBinding {
    pub source: SourceBinding,
    pub artifact: ArtifactRef,
    pub data_digest: Digest,
    pub input_digest: Digest,
    pub scenario_digest: Digest,
    pub decision_digest: Digest,
    pub session_digest: Digest,
    pub runtime_version: String,
    pub driver_version: String,
}
impl RunBinding {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.artifact.validate()?;
        nonempty(&self.runtime_version)?;
        nonempty(&self.driver_version)?;
        require(
            self.source.source_digest == self.artifact.raw_digest,
            "run source/artifact mismatch",
        )
    }
    pub fn matches(&self, current: &Self) -> bool {
        self == current
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    pub fuel: u64,
    pub collection_items: usize,
    pub action_steps: usize,
    pub transaction_writes: usize,
    pub output_bytes: usize,
    pub elapsed_millis: u64,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            fuel: 100_000,
            collection_items: MAX_COLLECTION,
            action_steps: MAX_ITEMS,
            transaction_writes: MAX_ITEMS,
            output_bytes: MAX_OUTPUT_BYTES,
            elapsed_millis: 5_000,
        }
    }
}
impl RuntimeLimits {
    pub fn validate(&self) -> Result<()> {
        require(
            self.fuel > 0
                && self.fuel <= 10_000_000
                && self.collection_items > 0
                && self.collection_items <= MAX_COLLECTION
                && self.action_steps > 0
                && self.action_steps <= MAX_ITEMS
                && self.transaction_writes > 0
                && self.transaction_writes <= MAX_ITEMS
                && self.output_bytes > 0
                && self.output_bytes <= MAX_OUTPUT_BYTES
                && self.elapsed_millis > 0
                && self.elapsed_millis <= 60_000,
            "runtime budget outside host limits",
        )
    }
}
/// Host-owned execution record. Restoring this from storage only restores a
/// claim; adoption still requires exact freshness and independently executed
/// obligations. This type is intentionally absent from DevelopmentResponse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEvidence {
    pub version: u32,
    pub id: Id,
    pub origin: ExecutionOrigin,
    pub binding: RunBinding,
    pub source_after: SourceBinding,
    pub state: EvidenceState,
    pub trace: Vec<TraceStep>,
    pub observations: Vec<Observation>,
    pub limits: RuntimeLimits,
    pub errors: Vec<String>,
    pub uncovered: Vec<String>,
}
impl RunEvidence {
    pub fn is_current(&self, current: &RunBinding) -> bool {
        self.binding.matches(current)
            && self.source_after == current.source
            && self.state != EvidenceState::Stale
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evidence, self)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        self.binding.validate()?;
        self.source_after.validate()?;
        self.limits.validate()?;
        require(
            self.state != EvidenceState::NoDifferenceFound,
            "no-difference classification belongs to a completed comparison, not a single run",
        )?;
        bounded(self.trace.len())?;
        bounded(self.observations.len())?;
        bounded(self.errors.len())?;
        bounded(self.uncovered.len())?;
        for error in self.errors.iter().chain(&self.uncovered) {
            nonempty(error)?;
        }
        unique(self.observations.iter().map(|o| o.point.as_str()))?;
        require(
            self.trace.len() <= self.limits.action_steps,
            "trace exceeds selected action limit",
        )?;
        for (index, step) in self.trace.iter().enumerate() {
            validate_input_shape(&step.input)?;
            require(index == step.index, "trace index is out of order")?;
            bounded(step.outputs.len())?;
            if let Some(message) = &step.diagnostic {
                nonempty(message)?;
            }
            if index > 0 {
                let prior = &self.trace[index - 1];
                require(
                    prior.after_data == step.before_data
                        && prior.after_session == step.before_session,
                    "trace state chain is broken",
                )?;
            }
            if matches!(step.input, SemanticInput::Navigate { .. }) {
                require(
                    step.before_data == step.after_data && step.outputs.is_empty(),
                    "navigation cannot mutate durable data or emit outputs",
                )?;
            }
            if matches!(
                step.input,
                SemanticInput::Observe { .. } | SemanticInput::AdvanceClock { .. }
            ) {
                require(
                    step.before_data == step.after_data
                        && step.before_session == step.after_session
                        && step.outputs.is_empty(),
                    "observation/clock input cannot mutate data or session",
                )?;
            }
            if step.outcome != StepOutcome::Applied {
                require(
                    step.before_data == step.after_data
                        && step.before_session == step.after_session
                        && step.outputs.is_empty(),
                    "failed step claims committed effects",
                )?;
            }
        }
        if let Some(first) = self.trace.first() {
            require(
                first.before_data == self.binding.data_digest
                    && first.before_session == self.binding.session_digest,
                "trace does not start from bound seed",
            )?;
        }
        let observed_points: Vec<_> = self
            .trace
            .iter()
            .filter_map(|step| match &step.input {
                SemanticInput::Observe { point } if step.outcome == StepOutcome::Applied => {
                    Some(point.as_str())
                }
                _ => None,
            })
            .collect();
        require(
            observed_points
                == self
                    .observations
                    .iter()
                    .map(|o| o.point.as_str())
                    .collect::<Vec<_>>(),
            "observations do not match ordered trace points",
        )?;
        for observation in &self.observations {
            observation.validate()?;
            validate_observation_limits(observation, &self.limits)?;
            let trace_index = self.trace.iter().position(|s| matches!(&s.input,SemanticInput::Observe { point } if point == &observation.point)).ok_or_else(|| ContractError("observation point missing from trace".into()))?;
            let step = &self.trace[trace_index];
            require(
                observation.data_digest == step.after_data
                    && observation.session_digest == step.after_session,
                "observation belongs to another runtime state",
            )?;
            let emitted: Vec<_> = self.trace[..=trace_index]
                .iter()
                .flat_map(|s| s.outputs.iter())
                .collect();
            require(
                emitted
                    == observation
                        .outputs
                        .iter()
                        .map(|o| &o.digest)
                        .collect::<Vec<_>>(),
                "observation outputs differ from emitted artifacts",
            )?;
        }
        if self.state == EvidenceState::Observed {
            require(
                self.errors.is_empty()
                    && self.trace.iter().all(|s| s.outcome == StepOutcome::Applied)
                    && !self.observations.is_empty(),
                "observed run contains failure or lacks observations",
            )?;
            require(
                self.source_after == self.binding.source,
                "changed source cannot have current observed status",
            )?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReductionOutcome {
    InvalidScenario,
    DifferenceLost,
    DifferencePreserved,
    Inconclusive,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReductionTrial {
    pub from: Digest,
    pub reduced: Digest,
    pub operation: String,
    pub outcome: ReductionOutcome,
    pub before_run: Option<Digest>,
    pub after_run: Option<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinimalityCertificate {
    pub initial: Digest,
    pub final_scenario: Digest,
    pub deletion_operations: Vec<String>,
    pub trials: Vec<ReductionTrial>,
    pub final_single_deletions: Vec<ReductionTrial>,
    /// True only when every final permitted single deletion was independently
    /// tried and found invalid or nondiscriminating. Never global optimality.
    pub complete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DifferentialWitness {
    pub version: u32,
    pub id: Id,
    pub scenario: ScenarioSpec,
    pub before: RunEvidence,
    pub after: RunEvidence,
    pub state: EvidenceState,
    pub distinguishing_properties: Vec<AcceptedProperty>,
    pub minimization: Option<MinimalityCertificate>,
}
impl DifferentialWitness {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        self.scenario.validate_structure()?;
        self.before.validate()?;
        self.after.validate()?;
        for run in [&self.before, &self.after] {
            require(
                run.binding.data_digest == self.scenario.seed.identity()?
                    && run.binding.session_digest == self.scenario.session.identity()?,
                "witness run starts from another seed/session",
            )?;
            require(
                run.trace.iter().map(|step| &step.input).eq(self
                    .scenario
                    .inputs
                    .iter()
                    .take(run.trace.len())),
                "witness trace is not an exact executed scenario prefix",
            )?;
            if matches!(
                self.state,
                EvidenceState::Observed | EvidenceState::NoDifferenceFound
            ) || run.state == EvidenceState::Observed
            {
                require(
                    run.trace.len() == self.scenario.inputs.len(),
                    "completed witness/run did not execute the full scenario",
                )?;
            }
        }
        require(
            self.before.binding.input_digest == self.after.binding.input_digest
                && self.before.binding.input_digest == self.scenario.input_identity()?,
            "comparison does not use identical seed/input",
        )?;
        require(
            self.before.binding.scenario_digest == self.scenario.identity()?
                && self.after.binding.scenario_digest == self.scenario.identity()?,
            "witness scenario binding mismatch",
        )?;
        bounded(self.distinguishing_properties.len())?;
        for property in &self.distinguishing_properties {
            property.validate()?;
        }
        if matches!(
            self.state,
            EvidenceState::Observed | EvidenceState::NoDifferenceFound
        ) {
            require(
                self.before.state == EvidenceState::Observed
                    && self.after.state == EvidenceState::Observed,
                "comparison contains incomplete or failed execution",
            )?;
            for property in &self.scenario.validity {
                require(
                    property.evaluate(&self.before.observations) == Some(true)
                        && property.evaluate(&self.after.observations) == Some(true),
                    "witness workflow validity is false or unknown",
                )?;
            }
            let outcomes: Vec<_> = self
                .distinguishing_properties
                .iter()
                .map(|property| {
                    (
                        property.evaluate(&self.before.observations),
                        property.evaluate(&self.after.observations),
                    )
                })
                .collect();
            if self.state == EvidenceState::Observed {
                require(
                    outcomes
                        .iter()
                        .any(|pair| matches!(pair,(Some(a),Some(b)) if a != b)),
                    "no declared material property distinguishes the actual observations",
                )?;
            } else {
                require(
                    outcomes
                        .iter()
                        .all(|pair| matches!(pair,(Some(a),Some(b)) if a == b)),
                    "no-difference classification has a differing or unknown declared property",
                )?;
            }
        }
        if let Some(certificate) = &self.minimization {
            require(
                certificate.final_scenario == self.scenario.identity()?,
                "minimization describes another scenario",
            )?;
            bounded(certificate.trials.len())?;
            bounded(certificate.final_single_deletions.len())?;
            bounded(certificate.deletion_operations.len())?;
            for operation in &certificate.deletion_operations {
                nonempty(operation)?;
            }
            for trial in certificate
                .trials
                .iter()
                .chain(&certificate.final_single_deletions)
            {
                nonempty(&trial.operation)?;
            }
            if certificate.complete {
                require(
                    !certificate.deletion_operations.is_empty()
                        && certificate.final_single_deletions.iter().all(|t| {
                            t.from == certificate.final_scenario
                                && matches!(
                                    t.outcome,
                                    ReductionOutcome::InvalidScenario
                                        | ReductionOutcome::DifferenceLost
                                )
                        }),
                    "incomplete final minimization trials",
                )?;
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evidence, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMatch {
    Applies,
    Outside,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Population {
    All,
    NewWork,
    CreatedAfter { generation: u64 },
    Records { records: Vec<RecordRef> },
    Entity { entity: Id },
    Where { entity: Id, predicate: Expr },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownBoundary {
    pub id: Id,
    pub operations: BTreeSet<Id>,
    pub description: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionScope {
    pub operations: BTreeSet<Id>,
    pub population: Population,
    pub conditions: Values,
    pub excluded_records: Vec<RecordRef>,
    pub unknowns: Vec<UnknownBoundary>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeContext {
    pub operation: Id,
    pub record: Option<RecordRef>,
    pub is_new: Option<bool>,
    pub created_generation: Option<u64>,
    pub attributes: Values,
    /// Supplied only by an independently evaluated matching Where predicate.
    pub predicate_result: Option<bool>,
}
impl DecisionScope {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.operations.is_empty(),
            "decision scope needs explicit operations",
        )?;
        unique(self.operations.iter().map(String::as_str))?;
        bounded(self.conditions.len())?;
        for (id, value) in &self.conditions {
            check_id(id)?;
            validate_untyped_value(value, 0)?;
        }
        validate_record_refs(&self.excluded_records)?;
        match &self.population {
            Population::Entity { entity } => check_id(entity)?,
            Population::Where { entity, predicate } => {
                check_id(entity)?;
                // Schema/type matching is performed against the active program;
                // unresolved old fields must yield Unknown, not a broader scope.
                check_expr_shape(predicate, 0, &mut 0)?;
            }
            Population::Records { records } => {
                require(!records.is_empty(), "empty record scope")?;
                validate_record_refs(records)?;
            }
            _ => {}
        }
        unique(self.unknowns.iter().map(|u| u.id.as_str()))?;
        for unknown in &self.unknowns {
            nonempty(&unknown.description)?;
            unique(unknown.operations.iter().map(String::as_str))?;
            require(
                !unknown.operations.is_empty(),
                "unknown boundary needs affected operations",
            )?;
        }
        Ok(())
    }
    pub fn matches(&self, context: &ScopeContext) -> ScopeMatch {
        if !self.operations.contains(&context.operation) {
            return ScopeMatch::Outside;
        }
        if context
            .record
            .as_ref()
            .is_some_and(|r| self.excluded_records.contains(r))
        {
            return ScopeMatch::Outside;
        }
        let mut unknown = (!self.excluded_records.is_empty() && context.record.is_none())
            || self
                .unknowns
                .iter()
                .any(|u| u.operations.contains(&context.operation));
        for (key, value) in &self.conditions {
            match context.attributes.get(key) {
                Some(actual) if actual != value => return ScopeMatch::Outside,
                None => unknown = true,
                _ => {}
            }
        }
        let population = match &self.population {
            Population::All => Some(true),
            Population::NewWork => context.is_new,
            Population::CreatedAfter { generation } => {
                context.created_generation.map(|g| g > *generation)
            }
            Population::Records { records } => context.record.as_ref().map(|r| records.contains(r)),
            Population::Entity { entity } => context.record.as_ref().map(|r| &r.entity == entity),
            Population::Where { entity, .. } => match &context.record {
                Some(record) if &record.entity != entity => Some(false),
                Some(_) => context.predicate_result,
                None => None,
            },
        };
        match population {
            Some(false) => ScopeMatch::Outside,
            Some(true) if !unknown => ScopeMatch::Applies,
            _ => ScopeMatch::Unknown,
        }
    }
}
fn validate_record_refs(records: &[RecordRef]) -> Result<()> {
    require(
        records.len() <= MAX_COLLECTION,
        "record selection exceeds limit",
    )?;
    let mut seen = BTreeSet::new();
    for record in records {
        check_id(&record.entity)?;
        check_id(&record.record)?;
        require(
            seen.insert((&record.entity, &record.record)),
            "duplicate selected record",
        )?;
    }
    Ok(())
}
/// Syntactic AST bounds for source-independent scope predicates before a target
/// schema is known. Full typing is still required by the decision runtime.
fn check_expr_shape(expr: &Expr, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    require(
        depth <= MAX_EXPR_DEPTH && *nodes <= MAX_AST_NODES,
        "scope AST budget exceeded",
    )?;
    let next = depth + 1;
    match expr {
        Expr::Literal { value_type, value } => {
            standalone_type(value_type, 0)?;
            validate_value(value, value_type, 0)?;
        }
        Expr::Variable { name } => check_id(name)?,
        Expr::State { state } => check_id(state)?,
        Expr::Field { record, field } => {
            check_id(field)?;
            check_expr_shape(record, next, nodes)?;
        }
        Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            limit,
            ..
        } => {
            check_id(entity)?;
            check_id(binding)?;
            require(
                *limit > 0 && *limit <= MAX_COLLECTION,
                "query limit invalid",
            )?;
            check_expr_shape(predicate, next, nodes)?;
            bounded(sort.len())?;
            for key in sort {
                check_expr_shape(&key.value, next, nodes)?;
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
            check_id(binding)?;
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(predicate, next, nodes)?;
        }
        Expr::Map {
            items,
            binding,
            value,
        } => {
            check_id(binding)?;
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(value, next, nodes)?;
        }
        Expr::Count { items } | Expr::Sum { items } => check_expr_shape(items, next, nodes)?,
        Expr::Contains { items, value } => {
            check_expr_shape(items, next, nodes)?;
            check_expr_shape(value, next, nodes)?;
        }
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            check_expr_shape(left, next, nodes)?;
            check_expr_shape(right, next, nodes)?;
        }
        Expr::TextContains { text, search } => {
            check_expr_shape(text, next, nodes)?;
            check_expr_shape(search, next, nodes)?;
        }
        Expr::Lower { value } | Expr::Not { value } => check_expr_shape(value, next, nodes)?,
        Expr::And { values } | Expr::Or { values } => {
            bounded(values.len())?;
            require(!values.is_empty(), "empty boolean connective")?;
            for value in values {
                check_expr_shape(value, next, nodes)?;
            }
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            check_expr_shape(condition, next, nodes)?;
            check_expr_shape(then_value, next, nodes)?;
            check_expr_shape(else_value, next, nodes)?;
        }
        Expr::Coalesce { value, fallback } => {
            check_expr_shape(value, next, nodes)?;
            check_expr_shape(fallback, next, nodes)?;
        }
        Expr::DateAdd { date, days } => {
            check_expr_shape(date, next, nodes)?;
            check_expr_shape(days, next, nodes)?;
        }
        Expr::DateDifference { later, earlier } => {
            check_expr_shape(later, next, nodes)?;
            check_expr_shape(earlier, next, nodes)?;
        }
        Expr::Today => {}
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionOutcome {
    Accept { artifact: Digest },
    KeepCurrent,
    EitherAcceptable,
    BothNeeded,
    NeitherFits,
    Deferred,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionStatus {
    Active,
    Pending,
    Superseded {
        by: Id,
    },
    /// Terminal retained history, created only by the trusted recovery controller
    /// after compatible recovery and remaining-intention checks. The identifier
    /// binds its real adoption receipt, not a successor preference or model claim.
    Withdrawn {
        adoption: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedDecision {
    pub id: Id,
    pub revision: u64,
    pub request: String,
    pub rationale: Option<String>,
    pub scope: DecisionScope,
    pub outcome: DecisionOutcome,
    pub status: DecisionStatus,
    pub obligations: Vec<AcceptedProperty>,
    pub scenarios: Vec<Digest>,
    pub witness: Digest,
    pub supersedes: Vec<Id>,
}
impl ScopedDecision {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        require(self.revision > 0, "decision revision must be positive")?;
        nonempty(&self.request)?;
        if let Some(rationale) = &self.rationale {
            text(rationale)?;
        }
        self.scope.validate()?;
        unique(self.obligations.iter().map(|p| p.id.as_str()))?;
        for property in &self.obligations {
            property.validate()?;
        }
        bounded(self.scenarios.len())?;
        require(
            !self.scenarios.is_empty(),
            "decision lacks accepted scenario",
        )?;
        unique(self.supersedes.iter().map(String::as_str))?;
        require(
            !self.supersedes.contains(&self.id),
            "decision supersedes itself",
        )?;
        match &self.status {
            DecisionStatus::Superseded { by } => {
                check_id(by)?;
                require(by != &self.id, "decision retires itself")?;
            }
            DecisionStatus::Withdrawn { adoption } => check_id(adoption)?,
            DecisionStatus::Active | DecisionStatus::Pending => {}
        }
        if matches!(
            self.outcome,
            DecisionOutcome::BothNeeded | DecisionOutcome::NeitherFits | DecisionOutcome::Deferred
        ) {
            require(
                !matches!(self.status, DecisionStatus::Active),
                "unresolved decision cannot activate behavior",
            )?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionGraph {
    pub version: u32,
    pub revision: u64,
    pub decisions: Vec<ScopedDecision>,
}
impl DecisionGraph {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        unique(self.decisions.iter().map(|d| d.id.as_str()))?;
        let index: BTreeMap<_, _> = self.decisions.iter().map(|d| (d.id.as_str(), d)).collect();
        for decision in &self.decisions {
            decision.validate()?;
            for prior in &decision.supersedes {
                let prior = index
                    .get(prior.as_str())
                    .ok_or_else(|| ContractError("unknown superseded decision".into()))?;
                require(
                    matches!(&prior.status,DecisionStatus::Superseded { by } if by == &decision.id),
                    "supersession graph is not reciprocal",
                )?;
            }
            let mut seen = BTreeSet::new();
            let mut current = decision;
            while let DecisionStatus::Superseded { by } = &current.status {
                require(
                    seen.insert(current.id.as_str()),
                    "cyclic decision supersession",
                )?;
                let next = *index
                    .get(by.as_str())
                    .ok_or_else(|| ContractError("missing superseding decision".into()))?;
                require(
                    next.supersedes.contains(&current.id),
                    "supersession history is missing",
                )?;
                current = next;
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Decision, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Satisfied,
    Violated,
    Unknown,
    Outside,
    Unsupported,
    Stale,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCheck {
    pub decision: Id,
    pub decision_digest: Digest,
    pub property_digest: Digest,
    pub scope: ScopeMatch,
    pub state: CheckState,
    pub binding: RunBinding,
    pub evidence: Option<Digest>,
    pub explanation: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    Entity,
    Field,
    Action,
    Observable,
    Output,
    View,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticKey {
    pub kind: SemanticKind,
    pub entity: Option<Id>,
    pub id: Id,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMapping {
    pub from: SemanticKey,
    pub to: SemanticKey,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioMapping {
    pub original: Digest,
    /// Exact accepted source ArtifactRef.program_digest. An absent legacy value
    /// is usable only when the trusted host resolves the original unambiguously.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program: Option<Digest>,
    pub replacement: ScenarioSpec,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolutionProposal {
    pub version: u32,
    pub id: Id,
    pub request_digest: Digest,
    pub candidate: ArtifactRef,
    pub needs: Vec<Id>,
    pub proposed_retirement: Vec<Id>,
    pub preserved_obligations: Vec<Digest>,
    pub mappings: Vec<SemanticMapping>,
    pub scenarios: Vec<ScenarioMapping>,
}
impl EvolutionProposal {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        self.candidate.validate()?;
        check_id(&self.id)?;
        unique(self.needs.iter().map(String::as_str))?;
        unique(self.proposed_retirement.iter().map(String::as_str))?;
        require(
            !self.needs.is_empty(),
            "evolution lacks concrete accepted needs",
        )?;
        bounded(self.preserved_obligations.len())?;
        bounded(self.mappings.len())?;
        bounded(self.scenarios.len())?;
        validate_mappings(&self.mappings)?;
        let mut originals = BTreeSet::new();
        for scenario in &self.scenarios {
            require(
                originals.insert((&scenario.original, &scenario.source_program)),
                "ambiguous scenario replacement",
            )?;
            scenario.replacement.validate_structure()?;
            nonempty(&scenario.explanation)?;
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Evolution, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityState {
    Compatible,
    RetainedButUnavailable,
    Incompatible,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityReport {
    pub state: CompatibilityState,
    pub current_data: Digest,
    pub target_program: Digest,
    pub retained_records: Vec<RecordRef>,
    pub retained_events: Vec<Id>,
    pub retained_fields: Vec<SemanticKey>,
    pub issues: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdoptionPlan {
    pub version: u32,
    pub id: Id,
    pub project_id: Id,
    pub expected_generation: u64,
    pub expected_data: Digest,
    pub expected_decisions: Digest,
    pub expected_session: Digest,
    pub current_source: SourceBinding,
    pub target: ArtifactRef,
    pub scope: DecisionScope,
    pub compatibility: CompatibilityReport,
    pub required_decisions: Vec<Id>,
    pub checks: Vec<DecisionCheck>,
    pub evidence: Vec<Digest>,
    pub retire_decisions: Vec<Id>,
}
impl AdoptionPlan {
    /// Structural preparation only; the controller must compare this immutable
    /// plan to current state, rerun obligations and obtain the user's choice.
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        check_id(&self.project_id)?;
        self.current_source.validate()?;
        self.target.validate()?;
        self.scope.validate()?;
        require(
            self.project_id == self.current_source.project_id,
            "plan project/source mismatch",
        )?;
        unique(self.required_decisions.iter().map(String::as_str))?;
        unique(self.retire_decisions.iter().map(String::as_str))?;
        bounded(self.checks.len())?;
        bounded(self.evidence.len())?;
        require(
            self.compatibility.current_data == self.expected_data
                && self.compatibility.target_program == self.target.program_digest,
            "compatibility refers to different data/program",
        )?;
        self.compatibility.validate()?;
        for check in &self.checks {
            check_id(&check.decision)?;
            check.binding.validate()?;
            text(&check.explanation)?;
            if check.state == CheckState::Satisfied {
                require(
                    check.scope == ScopeMatch::Applies && check.evidence.is_some(),
                    "satisfied decision has no applicable run evidence",
                )?;
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Adoption, self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPlan {
    pub adoption: AdoptionPlan,
    pub withdraw_decisions: Vec<Id>,
    pub preserve_current_data: Digest,
    pub preserve_current_events: Digest,
}
impl RecoveryPlan {
    pub fn validate(&self) -> Result<()> {
        self.adoption.validate()?;
        unique(self.withdraw_decisions.iter().map(String::as_str))?;
        require(
            self.preserve_current_data == self.adoption.expected_data,
            "recovery must retain current data, not old snapshot",
        )
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Adoption, self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevelopmentOperation {
    Generate,
    Modify,
    Discover,
    Reconcile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    Synthetic,
    ExplicitlySelectedSanitized,
    /// Exact selected business records. This declares origin, not sanitization
    /// or authorization to send them to a development provider.
    ExplicitlySelected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedScenario {
    pub disclosure: Disclosure,
    pub scenario: ScenarioSpec,
}
/// Portable selected context, not an execution certificate or disclosure consent.
/// The host must populate this only from verified accepted packages/reruns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedSceneContext {
    pub decision: Id,
    pub source: ArtifactRef,
    pub scenario: Digest,
    pub observations: Vec<Observation>,
    pub disclosure: Disclosure,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentContext {
    pub view: Option<Id>,
    pub selected: Vec<RecordRef>,
    pub recent_inputs: Vec<SemanticInput>,
    pub data_digest: Option<Digest>,
    pub session_digest: Option<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentRequest {
    pub version: u32,
    pub id: Id,
    pub project_id: Id,
    pub operation: DevelopmentOperation,
    pub request: String,
    /// Discover: baseline, candidate, then selected accepted-history artifacts.
    /// Other operations: current source last after any historical sources.
    pub sources: Vec<CapturedProgram>,
    pub context: DevelopmentContext,
    pub examples: Vec<SelectedScenario>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_scenes: Vec<AcceptedSceneContext>,
    pub decisions: DecisionGraph,
    pub unknowns: Vec<UnknownBoundary>,
    pub required_capabilities: BTreeSet<Id>,
}
impl DevelopmentRequest {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        check_id(&self.project_id)?;
        nonempty(&self.request)?;
        bounded(self.sources.len())?;
        bounded(self.examples.len())?;
        bounded(self.context.recent_inputs.len())?;
        for input in &self.context.recent_inputs {
            validate_input_shape(input)?;
        }
        require(self.sources.len() <= 8, "too many development sources")?;
        for source in &self.sources {
            source.validate()?;
            require(
                source.binding.project_id == self.project_id,
                "request includes another project's source",
            )?;
        }
        if self.operation != DevelopmentOperation::Generate {
            require(
                !self.sources.is_empty(),
                "change request lacks actual source",
            )?;
        }
        if self.operation == DevelopmentOperation::Discover {
            require(
                self.sources.len() >= 2,
                "discovery needs an ordered baseline/candidate pair",
            )?;
            require(
                self.sources[0].program.id == self.sources[1].program.id,
                "discovery primary pair names different applications",
            )?;
            for (index, source) in self.sources.iter().enumerate().skip(2) {
                require(
                    !self.sources[..index]
                        .iter()
                        .any(|s| s.artifact == source.artifact),
                    "duplicate discovery historical source",
                )?;
                require(
                    self.accepted_scenes
                        .iter()
                        .any(|s| s.source == source.artifact),
                    "discovery historical source lacks selected accepted context",
                )?;
            }
        }
        validate_record_refs(&self.context.selected)?;
        if let Some(view) = &self.context.view {
            check_id(view)?;
        }
        self.decisions.validate()?;
        unique(self.unknowns.iter().map(|u| u.id.as_str()))?;
        unique(self.required_capabilities.iter().map(String::as_str))?;
        for unknown in &self.unknowns {
            nonempty(&unknown.description)?;
            unique(unknown.operations.iter().map(String::as_str))?;
        }
        for example in &self.examples {
            example.scenario.validate_structure()?;
        }
        bounded(self.accepted_scenes.len())?;
        serialized_bound(&self.accepted_scenes, MAX_WIRE_BYTES)?;
        let mut accepted_keys = BTreeSet::new();
        for accepted in &self.accepted_scenes {
            check_id(&accepted.decision)?;
            accepted.source.validate()?;
            let decision = self
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == accepted.decision)
                .ok_or_else(|| ContractError("accepted scene names an unknown decision".into()))?;
            require(
                decision.scenarios.contains(&accepted.scenario),
                "accepted scene is not a decision scenario",
            )?;
            let source = self
                .sources
                .iter()
                .find(|s| s.artifact == accepted.source)
                .ok_or_else(|| {
                    ContractError("accepted scene source is not captured in the request".into())
                })?;
            let selected = self
                .examples
                .iter()
                .find(|e| {
                    e.scenario.identity().ok().as_ref() == Some(&accepted.scenario)
                        && e.disclosure == accepted.disclosure
                })
                .ok_or_else(|| {
                    ContractError("accepted scene is not selected with this disclosure".into())
                })?;
            require(
                selected.scenario.seed.project_id == self.project_id,
                "accepted scene belongs to another project",
            )?;
            selected.scenario.validate(&source.program)?;
            require(
                accepted_keys.insert((
                    &accepted.decision,
                    &accepted.source.program_digest,
                    &accepted.scenario,
                )),
                "duplicate accepted scene context",
            )?;
            bounded(accepted.observations.len())?;
            let points: Vec<_> = selected
                .scenario
                .inputs
                .iter()
                .filter_map(|input| {
                    if let SemanticInput::Observe { point } = input {
                        Some(point)
                    } else {
                        None
                    }
                })
                .collect();
            require(
                !points.is_empty()
                    && points
                        == accepted
                            .observations
                            .iter()
                            .map(|o| &o.point)
                            .collect::<Vec<_>>(),
                "accepted observations do not match the selected scene",
            )?;
            let observable_types = source.program.observable_types()?;
            for observation in &accepted.observations {
                observation.validate()?;
                require(
                    observation.value_types == observable_types,
                    "accepted observable types do not match their source",
                )?;
                let expected_view = source.program.view_schema(&observation.view.view)?;
                if let Some(schema) = &observation.view_schema {
                    // Validate only supplied provenance here. Old metadata is
                    // never backfilled; exact replay still compares full receipts.
                    require(
                        expected_view.as_ref().is_some_and(|expected| {
                            expected.entity == schema.entity
                                && expected.columns == schema.columns
                                && schema.row_actions.as_ref().is_none_or(|actions| {
                                    expected.row_actions.as_ref() == Some(actions)
                                })
                        }),
                        "accepted view schema does not belong to its source",
                    )?;
                }
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<Digest> {
        self.validate()?;
        canonical_digest(IdentityDomain::Request, self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    UnresolvedChoice,
    PossibleDefect,
    RequestedChange,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceHypothesis {
    pub id: Id,
    pub statement: String,
    pub kind: HypothesisKind,
    pub action: Id,
    pub observable: Id,
    pub sources: Vec<SourceLocus>,
    pub alternatives: Vec<Id>,
    pub related_decisions: Vec<Id>,
    /// Encoded ScenarioSpec: avoids imposing dynamic record-map JSON-schema
    /// features on providers. Parsed and independently validated before running.
    pub scenario_json: String,
    pub unknowns: Vec<String>,
}
impl ChoiceHypothesis {
    pub fn scenario(&self) -> Result<ScenarioSpec> {
        let scenario: ScenarioSpec = parse(self.scenario_json.as_bytes())?;
        scenario.validate_structure()?;
        Ok(scenario)
    }
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        nonempty(&self.statement)?;
        check_id(&self.action)?;
        check_id(&self.observable)?;
        bounded(self.sources.len())?;
        require(
            !self.sources.is_empty(),
            "hypothesis lacks actual source locus",
        )?;
        for source in &self.sources {
            validate_relative_path(&source.relative_path)?;
            text(&source.pointer)?;
        }
        unique(self.alternatives.iter().map(String::as_str))?;
        require(
            self.alternatives.len() >= 2,
            "choice needs at least two executable alternatives",
        )?;
        unique(self.related_decisions.iter().map(String::as_str))?;
        bounded(self.unknowns.len())?;
        for unknown in &self.unknowns {
            nonempty(unknown)?;
        }
        self.scenario()?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedCandidate {
    pub id: Id,
    pub source_json: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentResponse {
    pub version: u32,
    pub request_digest: Digest,
    pub candidates: Vec<GeneratedCandidate>,
    pub hypotheses: Vec<ChoiceHypothesis>,
    pub evolutions: Vec<EvolutionSuggestion>,
    pub unsupported: Vec<String>,
}
impl DevelopmentResponse {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let response: Self = parse(bytes)?;
        response.validate()?;
        Ok(response)
    }
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        unique(self.candidates.iter().map(|c| c.id.as_str()))?;
        require(self.candidates.len() <= 8, "too many candidates")?;
        for candidate in &self.candidates {
            AppDefinition::parse(candidate.source_json.as_bytes())?;
        }
        unique(self.hypotheses.iter().map(|h| h.id.as_str()))?;
        for hypothesis in &self.hypotheses {
            hypothesis.validate()?;
            for id in &hypothesis.alternatives {
                require(
                    self.candidates.iter().any(|c| &c.id == id),
                    "hypothesis alternative is missing",
                )?;
            }
        }
        unique(self.evolutions.iter().map(|e| e.id.as_str()))?;
        for evolution in &self.evolutions {
            evolution.validate()?;
            require(
                self.candidates.iter().any(|c| c.id == evolution.candidate),
                "evolution candidate is missing",
            )?;
        }
        bounded(self.unsupported.len())?;
        for item in &self.unsupported {
            nonempty(item)?;
        }
        require(
            !self.candidates.is_empty() || !self.unsupported.is_empty(),
            "empty provider response",
        )
    }
    pub fn validate_for(&self, request: &DevelopmentRequest) -> Result<()> {
        self.validate()?;
        require(
            self.request_digest == request.identity()?,
            "response belongs to another request",
        )?;
        for hypothesis in &self.hypotheses {
            for locus in &hypothesis.sources {
                let source = request
                    .sources
                    .iter()
                    .find(|s| {
                        s.artifact.raw_digest == locus.raw_digest
                            && s.binding.program_path == locus.relative_path
                    })
                    .ok_or_else(|| ContractError("hypothesis source was not in request".into()))?;
                locus.validate_against(source)?;
            }
            for id in &hypothesis.related_decisions {
                require(
                    request.decisions.decisions.iter().any(|d| &d.id == id),
                    "hypothesis names unknown decision",
                )?;
            }
            let scenario = hypothesis.scenario()?;
            for id in &hypothesis.alternatives {
                let candidate = self
                    .candidates
                    .iter()
                    .find(|c| &c.id == id)
                    .ok_or_else(|| ContractError("missing alternative".into()))?;
                let app = AppDefinition::parse(candidate.source_json.as_bytes())?;
                scenario.validate(&app)?;
                require(
                    app.actions.iter().any(|a| a.id == hypothesis.action)
                        && app
                            .observables
                            .iter()
                            .any(|o| o.id == hypothesis.observable),
                    "hypothesis references missing action/observable",
                )?;
            }
        }
        for evolution in &self.evolutions {
            evolution.validate_for(request, self)?;
        }
        Ok(())
    }
}
impl ScenarioSpec {
    pub fn validate_structure(&self) -> Result<()> {
        version(self.version)?;
        check_id(&self.id)?;
        nonempty(&self.label)?;
        self.seed.validate()?;
        validate_day(self.clock_day)?;
        bounded(self.inputs.len())?;
        bounded(self.validity.len())?;
        check_id(&self.session.view)?;
        bounded(self.session.values.len())?;
        for (id, value) in &self.session.values {
            check_id(id)?;
            validate_untyped_value(value, 0)?;
        }
        if let Some(record) = &self.session.focused_record {
            validate_record_refs(std::slice::from_ref(record))?;
        }
        let mut points = BTreeSet::new();
        for input in &self.inputs {
            validate_input_shape(input)?;
            if let SemanticInput::Observe { point } = input {
                require(points.insert(point), "duplicate observation point")?;
            }
        }
        for property in &self.validity {
            property.validate()?;
        }
        Ok(())
    }
}
fn validate_values_shape(values: &Values) -> Result<()> {
    bounded(values.len())?;
    for (id, value) in values {
        check_id(id)?;
        validate_untyped_value(value, 0)?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterError {
    Invalid(ContractError),
    Unsupported(String),
    Stale(String),
    Cancelled,
    BudgetExhausted(String),
    Failed(String),
}
impl From<ContractError> for AdapterError {
    fn from(error: ContractError) -> Self {
        Self::Invalid(error)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCapabilities {
    pub adapter: Id,
    pub version: String,
    pub artifact_kinds: Vec<ArtifactKind>,
    pub features: BTreeSet<Id>,
    pub unavailable: Vec<String>,
}
/// Reserved metadata for an independently implemented external driver. Nothing
/// here starts an external server/build or claims that worktrees sandbox code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalRuntimeManifest {
    pub artifact: ArtifactRef,
    pub build_digest: Digest,
    pub dependency_lock: Digest,
    pub driver_version: String,
    pub action_mapping: Digest,
    pub seed_reset: Digest,
    pub verified_isolation_description: String,
    pub available: bool,
}
/// Auxiliary execution evidence, never serialized into source or saved output.
/// Only the local interpreter constructs these records from resolved Emit items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmissionRecord {
    record: RecordRef,
    created_program: Digest,
}
impl EmissionRecord {
    pub(crate) fn from_record(row: &Record) -> Self {
        Self {
            record: RecordRef {
                entity: row.entity.clone(),
                record: row.id.clone(),
            },
            created_program: row.created_program.clone(),
        }
    }
    pub fn record(&self) -> &RecordRef {
        &self.record
    }
    pub fn created_program(&self) -> &Digest {
        &self.created_program
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct EmissionExecution {
    before_data: Digest,
    before_session: Digest,
    input: Digest,
    day: i32,
}
/// Opaque, run-local evidence for one emission ordinal. Equal output bytes do
/// not identify participants. Absence means the entire emission is unverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedRecordParticipants {
    source: SourceBinding,
    program: ArtifactRef,
    operation: Id,
    ordinal: usize,
    output: Digest,
    records: Option<Vec<EmissionRecord>>,
    execution: Option<EmissionExecution>,
}
impl EmittedRecordParticipants {
    pub(crate) fn from_execution(
        program: &CapturedProgram,
        operation: &str,
        ordinal: usize,
        output: &LocalArtifact,
        records: Option<Vec<EmissionRecord>>,
    ) -> Self {
        Self {
            source: program.binding.clone(),
            program: program.artifact.clone(),
            operation: operation.into(),
            ordinal,
            output: output.digest.clone(),
            records,
            execution: None,
        }
    }
    pub(crate) fn bind_step(
        &mut self,
        input: &Digest,
        before_data: &Digest,
        before_session: &Digest,
        day: i32,
    ) {
        self.execution = Some(EmissionExecution {
            input: input.clone(),
            before_data: before_data.clone(),
            before_session: before_session.clone(),
            day,
        });
    }
    pub fn records(&self) -> Option<&[EmissionRecord]> {
        self.records.as_deref()
    }
    pub fn validate_for(
        &self,
        program: &CapturedProgram,
        operation: &str,
        ordinal: usize,
        output: &LocalArtifact,
        step: &TraceStep,
        day: i32,
    ) -> Result<()> {
        output.validate()?;
        require(
            self.source == program.binding
                && self.program == program.artifact
                && self.operation == operation
                && self.ordinal == ordinal
                && ordinal < MAX_ITEMS
                && self.output == output.digest
                && step.outcome == StepOutcome::Applied,
            "emission participants belong to another source, operation, ordinal or artifact",
        )?;
        let execution = self.execution.as_ref().ok_or_else(|| {
            ContractError("emission participants have no committed input step".into())
        })?;
        require(
            execution.before_data == step.before_data
                && execution.before_session == step.before_session
                && execution.day == day
                && execution.input
                    == canonical_digest(IdentityDomain::Input, &(&step.input, &program.artifact))?,
            "emission participants belong to another replay input frame",
        )?;
        if let Some(records) = &self.records {
            require(
                !records.is_empty()
                    && records.len() == output.rows.len()
                    && records.len() <= MAX_COLLECTION,
                "emission participant inventory is incomplete or exceeds bounds",
            )?;
        }
        Ok(())
    }
}

/// Local generated-app interpreter contract. It must copy initial state, apply
/// each input atomically, meter nested evaluation and expose the same production
/// views/artifacts used by daily work. Adapters cannot commit live project data.
pub trait RuntimeAdapter {
    type Run;
    /// Share cancellation with host proof checks in the same replay. Legacy
    /// adapters still enforce their own cancellation during execution.
    fn cancellation_token(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        None
    }
    fn capabilities(&self) -> RuntimeCapabilities;
    fn validate(&self, program: &CapturedProgram) -> std::result::Result<(), AdapterError>;
    fn start(
        &self,
        program: &CapturedProgram,
        data: &DataSnapshot,
        session: &SessionState,
        clock_day: i32,
        random_seed: u64,
        limits: RuntimeLimits,
    ) -> std::result::Result<Self::Run, AdapterError>;
    fn apply(
        &self,
        run: &mut Self::Run,
        input: &SemanticInput,
        operation_id: &str,
    ) -> std::result::Result<TraceStep, AdapterError>;
    fn observe(
        &self,
        run: &Self::Run,
        point: &str,
    ) -> std::result::Result<Observation, AdapterError>;
    /// Inspect already-produced immutable output receipts without evaluating
    /// expressions, observing views, or charging execution fuel.
    fn emitted_artifacts<'a>(
        &self,
        _run: &'a Self::Run,
    ) -> std::result::Result<&'a [LocalArtifact], AdapterError> {
        Err(AdapterError::Unsupported(
            "Adapter cannot inspect emitted artifact provenance".into(),
        ))
    }
    /// Fresh auxiliary evidence from actual evaluated Emit items. A legacy
    /// adapter returning None grants no additional record-scope authority.
    fn emitted_record_participants<'a>(
        &self,
        _run: &'a Self::Run,
    ) -> std::result::Result<Option<&'a [EmittedRecordParticipants]>, AdapterError> {
        Ok(None)
    }
    fn data<'a>(&self, run: &'a Self::Run) -> &'a DataSnapshot;
    fn session<'a>(&self, run: &'a Self::Run) -> &'a SessionState;
    fn compatibility(
        &self,
        program: &CapturedProgram,
        current: &DataSnapshot,
    ) -> std::result::Result<CompatibilityReport, AdapterError>;
    fn prepare_adoption(
        &self,
        plan: AdoptionPlan,
        current: &DataSnapshot,
    ) -> std::result::Result<AdoptionPlan, AdapterError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevelopmentResult {
    pub response: DevelopmentResponse,
    pub producer: Producer,
}
/// Domain bridge implemented later over the separately owned opaque transport.
/// Dispatch belongs on a worker, with prior disclosure/usage authorization.
pub trait DevelopmentProvider {
    fn develop(
        &self,
        request: &DevelopmentRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<DevelopmentResult, AdapterError>;
}
pub trait SourceAdapter {
    fn capture(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> std::result::Result<CapturedProgram, AdapterError>;
    fn current_binding(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> std::result::Result<SourceBinding, AdapterError>;
}
pub const APP_SCHEMA: &str = include_str!("../assets/product_protocol/app.schema.json");
pub const RESPONSE_SCHEMA: &str = include_str!("../assets/product_protocol/response.schema.json");

fn encode_output(
    format: OutputFormat,
    columns: &[FieldDefinition],
    rows: &[Values],
) -> Result<Vec<u8>> {
    validate_output_columns(columns)?;
    require(
        !columns.is_empty() && rows.len() <= MAX_COLLECTION,
        "invalid output dimensions",
    )?;
    let scalar = |value: &DataValue| -> Result<JsonValue> {
        validate_untyped_value(value, 0)?;
        Ok(match value {
            DataValue::Text { value } => JsonValue::String(value.clone()),
            DataValue::Boolean { value } => JsonValue::Bool(*value),
            DataValue::Integer { value } => JsonValue::Number((*value).into()),
            DataValue::Date { days } => JsonValue::String(
                chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                    .expect("epoch")
                    .checked_add_signed(chrono::Duration::days(i64::from(*days)))
                    .ok_or_else(|| ContractError("date overflow".into()))?
                    .to_string(),
            ),
            DataValue::Reference { entity, record } => {
                serde_json::json!({"entity":entity,"record":record})
            }
            _ => return Err(ContractError("output cells must be scalar".into())),
        })
    };
    let quote = |value: &str| -> String {
        if value.contains([',', '"', '\r', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.into()
        }
    };
    let mut bytes = Vec::new();
    match format {
        OutputFormat::Csv => {
            bytes.extend(
                columns
                    .iter()
                    .map(|c| quote(&csv_literal(&c.id)))
                    .collect::<Vec<_>>()
                    .join(",")
                    .as_bytes(),
            );
            bytes.extend(b"\r\n");
        }
        OutputFormat::Json => bytes.push(b'['),
    }
    for (index, row) in rows.iter().enumerate() {
        require(
            row.len() == columns.len() && columns.iter().all(|c| row.contains_key(&c.id)),
            "output row columns mismatch",
        )?;
        let mut object = BTreeMap::new();
        let mut cells = Vec::new();
        for column in columns {
            let value = &row[&column.id];
            validate_value(value, &column.value_type, 0)?;
            let json = scalar(value)?;
            if format == OutputFormat::Json {
                object.insert(&column.id, json);
            } else {
                let mut rendered = match &json {
                    JsonValue::String(s) => s.clone(),
                    _ => String::from_utf8(canonical_bytes(&json)?)
                        .map_err(|e| ContractError(e.to_string()))?,
                };
                // Quoting CSV alone does not prevent spreadsheet formula execution.
                if matches!(value, DataValue::Text { .. }) {
                    rendered = csv_literal(&rendered);
                }
                cells.push(quote(&rendered));
            }
        }
        match format {
            OutputFormat::Csv => {
                bytes.extend(cells.join(",").as_bytes());
                bytes.extend(b"\r\n");
            }
            OutputFormat::Json => {
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend(canonical_bytes(&object)?);
            }
        }
        require(
            bytes
                .len()
                .saturating_add(usize::from(format == OutputFormat::Json))
                <= MAX_OUTPUT_BYTES,
            "output byte limit exceeded",
        )?;
    }
    if format == OutputFormat::Json {
        bytes.push(b']');
    }
    require(
        bytes.len() <= MAX_OUTPUT_BYTES,
        "output byte limit exceeded",
    )?;
    Ok(bytes)
}
impl PropertyTerm {
    fn evaluate(&self, observations: &[Observation]) -> Option<DataValue> {
        let point = |id: &str| observations.iter().find(|o| o.point == id);
        match self {
            Self::Literal { value, .. } => Some(value.clone()),
            Self::Observed {
                point: id,
                observable,
                value_type,
            } => {
                let observed = point(id)?;
                if observed.value_types.get(observable)? != value_type {
                    return None;
                }
                let value = observed.values.get(observable)?;
                validate_value(value, value_type, 0).ok()?;
                Some(value.clone())
            }
            Self::ViewRows { point: id, entity } => {
                let observed = point(id)?;
                if &observed.view_schema.as_ref()?.entity != entity {
                    return None;
                }
                Some(DataValue::List {
                    item_type: Type::reference(entity),
                    items: observed
                        .view
                        .rows
                        .iter()
                        .map(|row| DataValue::Reference {
                            entity: row.record.entity.clone(),
                            record: row.record.record.clone(),
                        })
                        .collect(),
                })
            }
            Self::RowActionRows {
                point: id,
                view,
                entity,
                binding,
                action,
            } => point(id)?.row_action_rows(view, entity, binding, action),
            Self::ViewColumn {
                point: id,
                column,
                value_type,
            } => {
                let observed = point(id)?;
                if observed.view_schema.as_ref()?.columns.get(column)? != value_type {
                    return None;
                }
                let items = observed
                    .view
                    .rows
                    .iter()
                    .map(|row| {
                        let value = row.cells.get(column)?;
                        validate_value(value, value_type, 0).ok()?;
                        Some(value.clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(DataValue::List {
                    item_type: value_type.clone(),
                    items,
                })
            }
            Self::OutputCount { point: id, output } => {
                let outputs: Vec<_> = point(id)?
                    .outputs
                    .iter()
                    .filter(|a| &a.output == output)
                    .collect();
                if outputs.is_empty() {
                    return None;
                }
                let mut count = 0i64;
                for artifact in outputs {
                    artifact.validate().ok()?;
                    count = count.checked_add(i64::try_from(artifact.rows.len()).ok()?)?;
                }
                Some(DataValue::Integer { value: count })
            }
            Self::OutputColumn {
                point: id,
                output,
                column,
                value_type,
            } => {
                let outputs: Vec<_> = point(id)?
                    .outputs
                    .iter()
                    .filter(|a| &a.output == output)
                    .collect();
                if outputs.is_empty() {
                    return None;
                }
                let mut items = vec![];
                for artifact in outputs {
                    artifact.validate().ok()?;
                    if !artifact.columns.iter().any(|definition| {
                        &definition.id == column && &definition.value_type == value_type
                    }) {
                        return None;
                    }
                    for row in &artifact.rows {
                        let value = row.get(column)?;
                        validate_value(value, value_type, 0).ok()?;
                        items.push(value.clone());
                        if items.len() > MAX_COLLECTION {
                            return None;
                        }
                    }
                }
                Some(DataValue::List {
                    item_type: value_type.clone(),
                    items,
                })
            }
            Self::Count { value } => {
                let DataValue::List { items, .. } = value.evaluate(observations)? else {
                    return None;
                };
                Some(DataValue::Integer {
                    value: i64::try_from(items.len()).ok()?,
                })
            }
        }
    }
}
impl PropertyPredicate {
    fn evaluate(&self, observations: &[Observation]) -> Option<bool> {
        match self {
            Self::Equal { left, right } => {
                Some(left.evaluate(observations)? == right.evaluate(observations)?)
            }
            Self::Less { left, right } => Some(
                match (left.evaluate(observations)?, right.evaluate(observations)?) {
                    (DataValue::Integer { value: a }, DataValue::Integer { value: b }) => a < b,
                    (DataValue::Text { value: a }, DataValue::Text { value: b }) => a < b,
                    (DataValue::Date { days: a }, DataValue::Date { days: b }) => a < b,
                    (DataValue::Boolean { value: a }, DataValue::Boolean { value: b }) => !a && b,
                    (
                        DataValue::Reference {
                            entity: a,
                            record: x,
                        },
                        DataValue::Reference {
                            entity: b,
                            record: y,
                        },
                    ) => (a, x) < (b, y),
                    _ => return None,
                },
            ),
            Self::And { values } => {
                let mut unknown = false;
                for value in values {
                    match value.evaluate(observations) {
                        Some(false) => return Some(false),
                        None => unknown = true,
                        _ => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(true)
                }
            }
            Self::Or { values } => {
                let mut unknown = false;
                for value in values {
                    match value.evaluate(observations) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        _ => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(false)
                }
            }
            Self::Not { value } => value.evaluate(observations).map(|v| !v),
        }
    }
}
fn validate_view_observation(view: &ViewObservation) -> Result<()> {
    check_id(&view.view)?;
    validate_record_refs(&view.selected)?;
    validate_values_shape(&view.controls)?;
    validate_values_shape(&view.form_values)?;
    unique(view.enabled_actions.iter().map(String::as_str))?;
    require(view.rows.len() <= MAX_COLLECTION, "too many observed rows")?;
    let mut records = BTreeSet::new();
    for row in &view.rows {
        validate_record_refs(std::slice::from_ref(&row.record))?;
        require(
            records.insert((&row.record.entity, &row.record.record)),
            "duplicate observed row",
        )?;
        validate_values_shape(&row.cells)?;
        unique(row.enabled_actions.iter().map(String::as_str))?;
    }
    Ok(())
}

fn serialized_bound<T: Serialize>(value: &T, max: usize) -> Result<()> {
    struct Counter {
        size: usize,
        max: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.size = self.size.saturating_add(bytes.len());
            if self.size > self.max {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "serialized contract exceeds byte limit",
                ));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter { size: 0, max }, value).map_err(|e| ContractError(e.to_string()))
}
impl ArtifactRef {
    pub fn validate(&self) -> Result<()> {
        version(self.version)?;
        nonempty(&self.language)?;
        if self.kind == ArtifactKind::GeneratedApp {
            require(
                self.language == LANGUAGE_VERSION,
                "unsupported application language",
            )?;
        }
        Ok(())
    }
}
impl DecisionScope {
    pub fn validate_for(&self, app: &AppDefinition) -> Result<()> {
        self.validate()?;
        app.validate()?;
        let mut validator = Validator::new(app, Audit::none());
        for operation in &self.operations {
            validator.action(operation)?;
        }
        match &self.population {
            Population::Entity { entity } => {
                validator.entity(entity)?;
            }
            Population::Records { records } => {
                for record in records {
                    validator.entity(&record.entity)?;
                }
            }
            _ => {}
        }
        for excluded in &self.excluded_records {
            validator.entity(&excluded.entity)?;
        }
        if let Population::Where { entity, predicate } = &self.population {
            validator.entity(entity)?;
            validator.expect(
                predicate,
                &BTreeMap::from([("record".into(), Type::reference(entity))]),
                &Type::Boolean,
                0,
                false,
            )?;
        }
        Ok(())
    }
}
impl DevelopmentResult {
    pub fn validate_for(&self, request: &DevelopmentRequest) -> Result<()> {
        self.response.validate_for(request)?;
        self.producer.validate()?;
        if let Producer::LiveAgent { request_digest, .. } = &self.producer {
            require(
                request_digest == &request.identity()?,
                "producer belongs to another request",
            )?;
        }
        Ok(())
    }
}

impl SemanticKey {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        if let Some(entity) = &self.entity {
            check_id(entity)?;
        }
        require(
            (self.kind == SemanticKind::Field) == self.entity.is_some(),
            "only field keys require an entity namespace",
        )
    }
}
impl CompatibilityReport {
    pub fn validate(&self) -> Result<()> {
        validate_record_refs(&self.retained_records)?;
        require(
            self.retained_events.len() <= MAX_EVENTS
                && self.retained_fields.len() <= MAX_COLLECTION,
            "compatibility retention inventory exceeds bounds",
        )?;
        let mut events = BTreeSet::new();
        for event in &self.retained_events {
            check_id(event)?;
            require(events.insert(event), "duplicate retained event")?;
        }
        let mut fields = BTreeSet::new();
        for field in &self.retained_fields {
            field.validate()?;
            require(
                field.kind == SemanticKind::Field,
                "retained field inventory contains a non-field key",
            )?;
            require(
                fields.insert(canonical_bytes(field)?),
                "duplicate retained semantic key",
            )?;
        }
        bounded(self.issues.len())?;
        for issue in &self.issues {
            nonempty(issue)?;
        }
        Ok(())
    }
}

impl Producer {
    pub fn validate(&self) -> Result<()> {self.validate_audited(Audit::none())}
    fn validate_audited(&self,a:Audit<'_>)->Result<()> {
        a.pay(8)?;
        match self {
            Producer::LiveAgent {
                provider,
                invocation_id,
                session_id,
                ..
            } => {
                a.nonempty(provider)?;
                a.nonempty(invocation_id)?;
                if let Some(session) = session_id {
                    a.nonempty(session)?;
                }
            }
            Producer::ExternalAuthor { description } => a.nonempty(description)?,
            Producer::Fixture { name } => a.nonempty(name)?,
            Producer::UserAuthored => {}
        }
        Ok(())
    }
}

impl LocalArtifact {
    pub fn receipt_identity(&self) -> Result<Digest> {
        canonical_digest(
            IdentityDomain::Output,
            &(
                &self.output,
                self.format,
                &self.columns,
                &self.rows,
                &self.bytes_digest,
            ),
        )
    }
}
pub fn validate_input_shape(input: &SemanticInput) -> Result<()> {
    match input {
        SemanticInput::Invoke { action, arguments } => {
            check_id(action)?;
            validate_values_shape(arguments)
        }
        SemanticInput::Submit { view, arguments } => {
            check_id(view)?;
            validate_values_shape(arguments)
        }
        SemanticInput::Control {
            view,
            control,
            value,
        } => {
            check_id(view)?;
            check_id(control)?;
            validate_untyped_value(value, 0)
        }
        SemanticInput::Activate { view, binding, row } => {
            check_id(view)?;
            check_id(binding)?;
            if let Some(row) = row {
                validate_record_refs(std::slice::from_ref(row))?;
            }
            Ok(())
        }
        SemanticInput::Navigate { view } => check_id(view),
        SemanticInput::Observe { point } => check_id(point),
        SemanticInput::AdvanceClock { days } => {
            require(*days <= 3_652_058, "clock advance exceeds bound")
        }
    }
}
fn csv_literal(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{value}")
    } else {
        value.into()
    }
}
fn validate_observation_limits(observation: &Observation, limits: &RuntimeLimits) -> Result<()> {
    fn value_limit(value: &DataValue, limit: usize, depth: usize) -> Result<()> {
        require(
            depth <= MAX_TYPE_DEPTH,
            "observed value nesting exceeds limit",
        )?;
        if let DataValue::List { items, .. } = value {
            require(
                items.len() <= limit,
                "observed collection exceeds selected limit",
            )?;
            for item in items {
                value_limit(item, limit, depth + 1)?;
            }
        }
        Ok(())
    }
    require(
        observation.view.rows.len() <= limits.collection_items
            && observation.view.selected.len() <= limits.collection_items,
        "observed view exceeds selected collection limit",
    )?;
    for value in observation
        .values
        .values()
        .chain(observation.view.controls.values())
        .chain(observation.view.form_values.values())
        .chain(observation.view.rows.iter().flat_map(|r| r.cells.values()))
    {
        value_limit(value, limits.collection_items, 0)?;
    }
    let mut bytes = 0usize;
    for artifact in &observation.outputs {
        require(
            artifact.rows.len() <= limits.collection_items,
            "output rows exceed selected collection limit",
        )?;
        bytes = bytes.saturating_add(artifact.bytes.len());
    }
    require(
        bytes <= limits.output_bytes,
        "cumulative emitted bytes exceed selected limit",
    )
}

/// Untrusted reconciliation suggestion. Candidate IDs are resolved by the host;
/// references to captured digests are checked, never authoritative model proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolutionSuggestion {
    pub id: Id,
    pub candidate: Id,
    pub needs: Vec<Id>,
    pub proposed_retirement: Vec<Id>,
    pub preserved_obligations: Vec<Digest>,
    pub mappings: Vec<SemanticMapping>,
    pub scenarios: Vec<SuggestedScenarioMapping>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestedScenarioMapping {
    pub original: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_program: Option<Digest>,
    pub replacement_json: String,
    pub explanation: String,
}
impl SuggestedScenarioMapping {
    pub fn decode(&self) -> Result<ScenarioMapping> {
        nonempty(&self.explanation)?;
        let replacement: ScenarioSpec = parse(self.replacement_json.as_bytes())?;
        replacement.validate_structure()?;
        Ok(ScenarioMapping {
            original: self.original.clone(),
            source_program: self.source_program.clone(),
            replacement,
            explanation: self.explanation.clone(),
        })
    }
}
fn validate_mappings(mappings: &[SemanticMapping]) -> Result<()> {
    bounded(mappings.len())?;
    let mut sources = BTreeSet::new();
    for mapping in mappings {
        mapping.from.validate()?;
        mapping.to.validate()?;
        require(
            mapping.from.kind == mapping.to.kind,
            "semantic mapping changes kind",
        )?;
        require(
            sources.insert(canonical_bytes(&mapping.from)?),
            "ambiguous semantic mapping",
        )?;
    }
    Ok(())
}
impl SemanticKey {
    pub fn exists_in(&self, app: &AppDefinition) -> bool {
        if self.validate().is_err() {
            return false;
        }
        match self.kind {
            SemanticKind::Entity => app.entities.iter().any(|e| e.id == self.id),
            SemanticKind::Field => app.entities.iter().any(|e| {
                Some(&e.id) == self.entity.as_ref() && e.fields.iter().any(|f| f.id == self.id)
            }),
            SemanticKind::Action => app.actions.iter().any(|a| a.id == self.id),
            SemanticKind::Observable => app.observables.iter().any(|o| o.id == self.id),
            SemanticKind::Output => app.outputs.iter().any(|o| o.id == self.id),
            SemanticKind::View => app.views.iter().any(|v| v.id == self.id),
        }
    }
}
impl EvolutionSuggestion {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        check_id(&self.candidate)?;
        unique(self.needs.iter().map(String::as_str))?;
        require(
            !self.needs.is_empty(),
            "evolution needs accepted work examples",
        )?;
        unique(self.proposed_retirement.iter().map(String::as_str))?;
        bounded(self.preserved_obligations.len())?;
        validate_mappings(&self.mappings)?;
        bounded(self.scenarios.len())?;
        let mut originals = BTreeSet::new();
        for mapping in &self.scenarios {
            mapping.decode()?;
            require(
                originals.insert((&mapping.original, &mapping.source_program)),
                "ambiguous scenario replacement",
            )?;
        }
        Ok(())
    }
    fn validate_for(
        &self,
        request: &DevelopmentRequest,
        response: &DevelopmentResponse,
    ) -> Result<()> {
        self.validate()?;
        require(
            request.operation == DevelopmentOperation::Reconcile,
            "evolution suggestions require a reconciliation request",
        )?;
        let candidate = response
            .candidates
            .iter()
            .find(|c| c.id == self.candidate)
            .ok_or_else(|| ContractError("evolution candidate is missing".into()))?;
        let app = AppDefinition::parse(candidate.source_json.as_bytes())?;
        for need in &self.needs {
            require(
                request.decisions.decisions.iter().any(|d| &d.id == need),
                "evolution names an unknown accepted need",
            )?;
        }
        for retired in &self.proposed_retirement {
            require(
                self.needs.contains(retired)
                    && request.decisions.decisions.iter().any(|d| {
                        &d.id == retired
                            && matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                    }),
                "retirement proposals must name a current listed need",
            )?;
        }
        let obligations: BTreeSet<_> = request
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .flat_map(|d| d.obligations.iter())
            .map(AcceptedProperty::identity)
            .collect::<Result<_>>()?;
        for preserved in &self.preserved_obligations {
            require(
                obligations.contains(preserved),
                "evolution names unknown active obligation",
            )?;
        }
        for mapping in &self.mappings {
            require(
                request
                    .sources
                    .iter()
                    .any(|s| mapping.from.exists_in(&s.program))
                    && mapping.to.exists_in(&app),
                "evolution mapping references unknown source/candidate semantics",
            )?;
        }
        let mut known_scenarios: BTreeSet<_> = request
            .decisions
            .decisions
            .iter()
            .flat_map(|d| d.scenarios.iter().cloned())
            .collect();
        for example in &request.examples {
            known_scenarios.insert(example.scenario.identity()?);
        }
        for mapping in &self.scenarios {
            let accepted_sources: BTreeSet<_> = request
                .accepted_scenes
                .iter()
                .filter(|s| s.scenario == mapping.original)
                .map(|s| &s.source.program_digest)
                .collect();
            if let Some(source) = &mapping.source_program {
                require(
                    request
                        .sources
                        .iter()
                        .any(|s| &s.artifact.program_digest == source),
                    "scenario mapping names an unknown source program",
                )?;
                require(
                    accepted_sources.is_empty() || accepted_sources.contains(source),
                    "scenario mapping source does not match the accepted scene",
                )?;
            } else {
                require(
                    accepted_sources.len() <= 1,
                    "scenario mapping needs an explicit source program",
                )?;
            }
            require(
                known_scenarios.contains(&mapping.original),
                "evolution replaces an unknown scenario",
            )?;
            let mapping = mapping.decode()?;
            require(
                mapping.replacement.seed.project_id == request.project_id,
                "replacement scene belongs to another project",
            )?;
            mapping.replacement.validate(&app)?;
        }
        Ok(())
    }
}
fn validate_output_columns(columns: &[FieldDefinition]) -> Result<()> {
    unique(columns.iter().map(|column| column.id.as_str()))?;
    require(!columns.is_empty(), "output needs declared columns")?;
    for column in columns {
        nonempty(&column.label)?;
        standalone_type(&column.value_type, 0)?;
        require(
            column.value_type.is_scalar(),
            "output column type must be scalar",
        )?;
    }
    Ok(())
}

impl AppDefinition {
    /// Internal structural analysis uses the same expression typing as complete
    /// program validation. Callers must first validate the captured program and
    /// derive these lexical bindings from its parameters and statements.
    pub(crate) fn expression_type_in(
        &self,
        expression: &Expr,
        bindings: &BTreeMap<Id, Type>,
    ) -> Result<Type> {
        Validator::new(self, Audit::none())
        .expr(expression, bindings, 0, true)
    }

    /// Derive row/cell types from validated executable view expressions.
    pub fn view_schema(&self, view: &str) -> Result<Option<ViewSchema>> {
        self.validate()?;
        self.view_schema_validated(view)
    }
    fn view_schema_validated(&self, view: &str) -> Result<Option<ViewSchema>> {
        #[cfg(test)]
        count_validation(|counts| counts.views += 1);
        let view = self
            .views
            .iter()
            .find(|v| v.id == view)
            .ok_or_else(|| ContractError("unknown view".into()))?;
        let (entity, columns) = match &view.kind {
            ViewKind::List {
                entity, columns, ..
            }
            | ViewKind::Detail {
                entity, columns, ..
            } => (entity, columns),
            ViewKind::Form { .. } => return Ok(None),
        };
        let mut validator = Validator::new(self, Audit::none());
        let env = BTreeMap::from([("row".into(), Type::reference(entity))]);
        let columns = columns
            .iter()
            .map(|column| {
                Ok((
                    column.id.clone(),
                    validator.expr(&column.value, &env, 0, true)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Some(ViewSchema {
            entity: entity.clone(),
            columns,
            row_actions: Some(
                view.actions
                    .iter()
                    .filter(|binding| binding.placement == ActionPlacement::Row)
                    .map(|binding| (binding.id.clone(), binding.action.clone()))
                    .collect(),
            ),
        }))
    }
    /// Static declared types for host observations, including null/empty results.
    pub fn observable_types(&self) -> Result<BTreeMap<Id, Type>> {
        self.validate()?;
        self.observable_types_validated()
    }
    fn observable_types_validated(&self) -> Result<BTreeMap<Id, Type>> {
        #[cfg(test)]
        count_validation(|counts| counts.observables += 1);
        let mut validator = Validator::new(self, Audit::none());
        self.observables
            .iter()
            .map(|observable| {
                Ok((
                    observable.id.clone(),
                    validator.expr(&observable.value, &Environment::new(), 0, true)?,
                ))
            })
            .collect()
    }
}
impl Observation {
    pub fn validate(&self) -> Result<()> {
        check_id(&self.point)?;
        validate_values_shape(&self.values)?;
        require(
            self.values.len() == self.value_types.len(),
            "observed values lack matching declared types",
        )?;
        for (id, value_type) in &self.value_types {
            check_id(id)?;
            standalone_type(value_type, 0)?;
            let value = self
                .values
                .get(id)
                .ok_or_else(|| ContractError("declared observable has no value".into()))?;
            validate_value(value, value_type, 0)?;
        }
        validate_view_observation(&self.view)?;
        if let Some(schema) = &self.view_schema {
            check_id(&schema.entity)?;
            bounded(schema.columns.len())?;
            for (column, typ) in &schema.columns {
                check_id(column)?;
                standalone_type(typ, 0)?;
            }
            if let Some(actions) = &schema.row_actions {
                bounded(actions.len())?;
                for (binding, action) in actions {
                    check_id(binding)?;
                    check_id(action)?;
                }
                require(
                    self.view.rows.iter().all(|row| {
                        row.enabled_actions
                            .iter()
                            .all(|binding| actions.contains_key(binding))
                    }),
                    "enabled row binding lacks declared provenance",
                )?;
            }
            for row in &self.view.rows {
                require(
                    row.record.entity == schema.entity && row.cells.len() == schema.columns.len(),
                    "view row/schema mismatch",
                )?;
                for (column, typ) in &schema.columns {
                    let value = row
                        .cells
                        .get(column)
                        .ok_or_else(|| ContractError("view schema column missing".into()))?;
                    validate_value(value, typ, 0)?;
                }
            }
        }
        bounded(self.outputs.len())?;
        for output in &self.outputs {
            output.validate()?;
        }
        Ok(())
    }
}

impl Observation {
    /// Shared typed sampler for comparisons and saved-property replay. Runtime
    /// rendering has already evaluated both binding conditions and action guards.
    pub(crate) fn row_action_rows(
        &self,
        view: &str,
        entity: &str,
        binding: &str,
        action: &str,
    ) -> Option<DataValue> {
        self.validate().ok()?;
        let schema = self.view_schema.as_ref()?;
        if self.view.view != view
            || schema.entity != entity
            || schema.row_actions.as_ref()?.get(binding)? != action
        {
            return None;
        }
        let mut rows: Vec<_> = self
            .view
            .rows
            .iter()
            .filter(|row| row.enabled_actions.contains(binding))
            .map(|row| &row.record)
            .collect();
        rows.sort_by(|a, b| (&a.entity, &a.record).cmp(&(&b.entity, &b.record)));
        Some(DataValue::List {
            item_type: Type::reference(entity),
            items: rows
                .into_iter()
                .map(|row| DataValue::Reference {
                    entity: row.entity.clone(),
                    record: row.record.clone(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
#[path = "product_contract/capture_stage_tests.rs"]
mod capture_stage_tests;

#[cfg(test)]
#[path = "product_contract/capture_canonical_removal_tests.rs"]
mod capture_canonical_removal_tests;
