//! Closed capture-local validation owner. It cannot detach from its actual host.
use super::bounded_input::capture_payment_core as native;
use super::capture_work::{Audit, WorkState};
use super::*;
use serde::ser::SerializeMap;
use std::mem::size_of;

pub(super) struct CaptureFacts {
    pub(super) total_nodes: usize,
    pub(super) action_prefix_nodes: usize,
    pub(super) canonical_len: usize,
    pub(super) action_count: usize,
}
pub(crate) struct CaptureValidation<'h, 's, H: ContractWork + ?Sized> {
    pub(super) program: &'s CapturedProgram,
    pub(super) data: &'s DataSnapshot,
    host: &'h mut H,
    pub(super) facts: Option<CaptureFacts>,
    frame_valid: bool,
    first_failure: Option<ContractWorkError>,
}
// Lexical expansion preserves the ordinary entrance without a new wrapper.
macro_rules! capture_validation_body {
    ($program:ident, $data:ident, $host:ident, $body:ident, $facts:expr) => {{
    let mut context = CaptureValidation {
        program: $program,
        data: $data,
        host: $host,
        facts: $facts,
        frame_valid: false,
        first_failure: None,
    };
    let result = $body(&mut context);
    if let Some(error) = context.first_failure.take() {
        return Err(E::from(error));
    }
    // Caller Err is not a successful native/callback region and receives no poll.
    let value = result?;
    context
        .$host
        .charge(0)
        .map_err(|e| E::from(ContractWorkError::Resource(e)))?;
    Ok(value)
    }};
}
pub(crate) fn with_capture_validation<'s, H, R, E>(
    program: &'s CapturedProgram,
    data: &'s DataSnapshot,
    host: &mut H,
    body: impl for<'h> FnOnce(&mut CaptureValidation<'h, 's, H>) -> std::result::Result<R, E>,
) -> std::result::Result<R, E>
where
    H: ContractWork + ?Sized,
    E: From<ContractWorkError>,
{
    host.charge(0)
        .map_err(|e| E::from(ContractWorkError::Resource(e)))?;
    host.charge(10 + size_of::<CaptureValidation<'_, '_, H>>() as u64)
        .map_err(|e| E::from(ContractWorkError::Resource(e)))?;
    capture_validation_body!(program, data, host, body, None)
}
// Only the closed retained store chain implements these production protocols.
// The lifetime-generic consumer cannot return the factory's facts/host borrow.
pub(crate) trait SourcePreparation<H: ContractWork + ?Sized> {
    type Prepared;
    type Error: From<ContractError>;
    fn prepare(self, host: &mut H) -> std::result::Result<Self::Prepared, Self::Error>;
}
pub(crate) trait SourceConsumer<'s, H: ContractWork + ?Sized, P> {
    type Output;
    type Error;
    fn consume<'g>(self, ready: ReadyCaptureSource<'g, 's, H>, prepared: P)
        -> std::result::Result<Self::Output, Self::Error>;
}
pub(crate) struct ReadyCaptureSource<'g, 's, H: ContractWork + ?Sized> {
    source: &'s CapturedProgram,
    facts: &'g CaptureFacts,
    host: &'g mut H,
}
pub(crate) fn with_ordinary_source_facts<'s, H, P, C>(
    source: &'s CapturedProgram, host: &mut H, prepare: P, consume: C,
) -> std::result::Result<C::Output, P::Error>
where
    H: ContractWork + ?Sized,
    P: SourcePreparation<H>,
    C: SourceConsumer<'s, H, P::Prepared, Error = P::Error>,
{
    // Replaces only the original analyzer before.validate() position.
    let facts = source.validate_audited(Audit::none())?;
    let prepared = prepare.prepare(host)?;
    consume.consume(ReadyCaptureSource { source, facts: &facts, host }, prepared)
}
impl<'g, 's, H: ContractWork + ?Sized> ReadyCaptureSource<'g, 's, H> {
    pub(crate) const FACT_BYTES: usize = size_of::<CaptureFacts>();
    pub(crate) const OPTIONAL_FACT_BYTES: usize = size_of::<Option<CaptureFacts>>();
    pub(crate) const FACT_RESULT_BYTES: usize = size_of::<Result<CaptureFacts>>();
    pub(crate) fn source(&self) -> &'s CapturedProgram { self.source }
    pub(crate) fn into_capture_validation<W, R, E>(
        self, data: &'s DataSnapshot, entry: u64,
        wrap: impl FnOnce(&'g mut H) -> W,
        body: impl for<'v> FnOnce(&mut CaptureValidation<'v, 's, W>) -> std::result::Result<R, E>,
    ) -> std::result::Result<R, E>
    where W: ContractWork, E: From<ContractWorkError>,
    {
        self.host.charge(entry).map_err(|e| E::from(ContractWorkError::Resource(e)))?;
        let mut wrapped = wrap(self.host);
        let host = &mut wrapped;
        host.charge(0).map_err(|e| E::from(ContractWorkError::Resource(e)))?;
        host.charge(10 + size_of::<CaptureValidation<'_, '_, W>>() as u64)
            .map_err(|e| E::from(ContractWorkError::Resource(e)))?;
        let program = self.source;
        capture_validation_body!(program, data, host, body, Some(CaptureFacts {
            total_nodes: self.facts.total_nodes,
            action_prefix_nodes: self.facts.action_prefix_nodes,
            canonical_len: self.facts.canonical_len,
            action_count: self.facts.action_count,
        }))
    }
}

struct Forward<'h, H: ?Sized>(&'h mut H);
impl<H: ContractWork + ?Sized> ContractWork for Forward<'_, H> {
    fn charge(&mut self, n: u64) -> std::result::Result<(), WorkFailure> {
        self.0.charge(n)
    }
}
impl<H: ContractWork + ?Sized> CaptureValidation<'_, '_, H> {
    fn retain<T>(&mut self, result: ContractWorkResult<T>) -> ContractWorkResult<T> {
        if let Err(error) = &result {
            // Semantic A+B was reserved at its origin; fixed Resource needs no String.
            if self.first_failure.is_none() {
                self.first_failure = Some(error.clone());
            }
        }
        result
    }
    fn run_host<T>(&mut self, body: impl FnOnce(Audit<'_>) -> Result<T>) -> ContractWorkResult<T> {
        let header = self
            .host
            .charge(8 + size_of::<Forward<'_, H>>() as u64 + size_of::<WorkState<'_>>() as u64)
            .map_err(ContractWorkError::Resource);
        if let Err(error) = header {
            return self.retain(Err(error));
        }
        let result = {
            let mut forward = Forward(&mut *self.host);
            let state = WorkState::new(&mut forward);
            state.finish(body(Audit(Some(&state))))
        };
        self.retain(result)
    }
    pub(crate) fn with_host<R>(&mut self, body: impl FnOnce(&mut H) -> R) -> R {
        body(self.host)
    }
    pub(crate) fn ensure_source_and_data(&mut self) -> ContractWorkResult<()> {
        // Production propagates immediately; only one swallow + finalization is supported.
        let program = self.program;
        let data = self.data;
        let result = self.host.charge(4).map_err(ContractWorkError::Resource);
        self.retain(result)?;
        if self.facts.is_none() {
            let facts = self.run_host(|a| program.validate_audited(a))?;
            let result = self.host.charge(5).map_err(ContractWorkError::Resource);
            self.retain(result)?;
            self.facts = Some(facts);
        }
        if !self.frame_valid {
            self.run_host(|a| data.validate_audited(a))?;
            let result = self.host.charge(1).map_err(ContractWorkError::Resource);
            self.retain(result)?;
            self.frame_valid = true;
        }
        Ok(())
    }
    pub(crate) fn initial_session(
        &mut self,
        row_work: &mut dyn ContractWork,
    ) -> ContractWorkResult<SessionState> {
        self.ensure_source_and_data()?;
        let header = row_work
            .charge(0)
            .and_then(|()| row_work.charge(8 + size_of::<WorkState<'_>>() as u64))
            .map_err(ContractWorkError::Resource);
        self.retain(header)?;
        let state = WorkState::new(row_work);
        let a = Audit(Some(&state));
        let result = (|| {
            a.pay(0)?;
            a.pay(8 + size_of::<SessionState>() as u64)?;
            a.string(self.program.program.initial_view.len())?;
            let items = &self.program.program.state;
            a.slice(items.len())?;
            let mut max_key = 0usize;
            for item in items {
                a.pay(3)?;
                a.string(item.id.len())?;
                item.initial.pay_owned(a)?;
                max_key = max_key.max(item.id.len());
            }
            capture_canonical::reserve_from_iter::<String, DataValue>(
                items.len(),
                a.comparator(max_key)?,
                a,
            )?;
            let session = SessionState::initial_validated(&self.program.program);
            a.pay(0)?;
            Ok(session)
        })();
        let result = state.finish(result);
        self.retain(result)
    }
    pub(crate) fn validate_extension(
        &mut self,
        entity: &str,
        binding: &str,
        expr: &Expr,
    ) -> ContractWorkResult<()> {
        self.ensure_source_and_data()?;
        let program = &self.program.program;
        let facts = self
            .facts
            .as_ref()
            .expect("successful source before static extension");
        // Copy only scalar measurements into this local call; no escaping proof/token.
        let (total, prefix, len, count) = (
            facts.total_nodes,
            facts.action_prefix_nodes,
            facts.canonical_len,
            facts.action_count,
        );
        self.run_host(|a| {
            extension(
                program,
                (total, prefix, len, count),
                entity,
                binding,
                expr,
                a,
            )
        })
    }
}
pub(crate) fn validate_value_with_work(
    value: &DataValue,
    expected: &Type,
    depth: usize,
    work: &mut dyn ContractWork,
) -> ContractWorkResult<()> {
    work.charge(0).map_err(ContractWorkError::Resource)?;
    work.charge(8 + size_of::<WorkState<'_>>() as u64)
        .map_err(ContractWorkError::Resource)?;
    let state = WorkState::new(work);
    let a = Audit(Some(&state));
    let result = (|| {
        a.pay(0)?;
        validate_value_audited(value, expected, depth, a)?;
        a.pay(0)
    })();
    state.finish(result)
}

pub(super) fn bytes_digest_audited(
    domain: IdentityDomain,
    bytes: &[u8],
    a: Audit<'_>,
) -> Result<Digest> {
    if !a.enabled() {
        return Ok(bytes_digest(domain, bytes));
    }
    a.pay(0)?;
    a.pay(96)?;
    let domain_bytes = match domain {
        IdentityDomain::Source => 8,
        IdentityDomain::Program => 9,
        IdentityDomain::SemanticProgram => 18,
        _ => return Err(a.error("unsupported capture digest domain")?),
    };
    let framed = 22usize
        .checked_add(domain_bytes)
        .and_then(|n| n.checked_add(1))
        .and_then(|n| n.checked_add(bytes.len()))
        .ok_or_else(|| a.overflow())?;
    // Original initial22 Vec, domain serializer128, and the three original extends.
    a.pay(4 + 44 + 128 + 5 * domain_bytes as u64 + 32)?;
    let mut len = 22usize;
    let mut capacity = 22usize;
    for amount in [domain_bytes, 1, bytes.len()] {
        a.pay(16)?;
        let needed = len.checked_add(amount).ok_or_else(|| a.overflow())?;
        if needed > capacity {
            let next = capacity
                .checked_mul(2)
                .map(|n| n.max(needed).max(8))
                .ok_or_else(|| a.overflow())?;
            a.scaled(capacity, 1, 1)?;
            a.scaled(next, 1, 1)?;
            capacity = next;
        }
        a.scaled(amount, 1, 4)?;
        len = needed;
    }
    let blocks = framed
        .checked_add(9)
        .and_then(|n| n.checked_add(63))
        .map(|n| n / 64)
        .ok_or_else(|| a.overflow())?;
    // Hash input, state, remainder, padding and one bounded compression interface per block.
    a.scaled(framed, 1, 8 * 4 + 128 + 63 + 16 + 64)?;
    a.scaled(blocks, 1, 0)?;
    // Original 32 per-byte native format! strings, then native String collection.
    a.pay(64 + 4 * usize::BITS as u64)?;
    let format = native::format_bytes(2).ok_or_else(|| a.overflow())?;
    a.amount(format.checked_mul(32))?;
    a.pay(12)?;
    // String FromIterator grows byte capacity8,16,32,64: requested120, relocation56.
    a.pay(64 + 120 + 56 + 4 * 9 + 32 * 4 + 8)?;
    let result = bytes_digest(domain, bytes);
    a.pay(0)?;
    Ok(result)
}

#[derive(Serialize)]
pub(super) struct SyntheticAction<'a> {
    id: &'a str,
    label: &'static str,
    parameters: Parameters<'a>,
    guards: [(); 0],
    steps: [SyntheticAssert<'a>; 1],
    ensures: [(); 0],
}
struct Parameters<'a> {
    binding: &'a str,
    entity: &'a str,
}
#[derive(Serialize)]
struct Reference<'a> {
    kind: &'static str,
    entity: &'a str,
}
impl Serialize for Parameters<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry(
            self.binding,
            &Reference {
                kind: "reference",
                entity: self.entity,
            },
        )?;
        map.end()
    }
}
#[derive(Serialize)]
struct SyntheticAssert<'a> {
    kind: &'static str,
    condition: SyntheticEqual<'a>,
    message: &'static str,
}
#[derive(Serialize)]
struct SyntheticEqual<'a> {
    kind: &'static str,
    left: &'a Expr,
    right: &'a Expr,
}
// The one-entry Parameters map is a borrowed custom serializer, not a BTree.
// Describe its actual fields without the generic map's BTree iterator reserve.
impl capture_canonical::ManualShape for Parameters<'_> {
    fn describe(&self, s: &mut capture_canonical::Shape<'_>) -> Result<()> {
        let mut object = s.object()?;
        object.field(self.binding, &Reference { kind: "reference", entity: self.entity })?;
        object.end()
    }
}
impl SyntheticAction<'_> {
    pub(super) fn describe(&self, s: &mut capture_canonical::Shape<'_>) -> Result<()> {
        let mut object = s.object()?;
        object.field("id", &self.id)?;
        object.field("label", &self.label)?;
        object.manual_field("parameters", &self.parameters)?;
        object.field("guards", &self.guards)?;
        object.field("steps", &self.steps)?;
        object.field("ensures", &self.ensures)?;
        object.end()
    }
}
fn extension(
    app: &AppDefinition,
    facts: (usize, usize, usize, usize),
    entity: &str,
    binding: &str,
    expr: &Expr,
    a: Audit<'_>,
) -> Result<()> {
    a.pay(0)?;
    a.pay(16)?;
    let (total, prefix, wire, actions) = facts;
    a.string("gm_scope_projection".len())?;
    let mut id = "gm_scope_projection".to_owned();
    while a.find(&app.actions, &id, |action| &action.id)?.is_some() {
        a.pay(16)?;
        if id.len() == id.capacity() {
            let next = id
                .capacity()
                .checked_mul(2)
                .map(|n| n.max(8))
                .ok_or_else(|| a.overflow())?;
            a.scaled(id.len(), 1, 1)?;
            a.scaled(next, 1, 1)?;
        }
        a.pay(6)?;
        id.push('_');
    }
    a.check_id(&id)?;
    a.pay(2)?;
    a.bounded(actions.checked_add(1).ok_or_else(|| a.overflow())?)?;
    a.nonempty("Host projection type check")?;
    a.require(true, "action needs steps")?;
    a.pay(8)?;
    let mut parameters = BTreeMap::new();
    a.insert_string(&mut parameters, binding, a.reference_type(entity)?)?;
    a.map_iter(parameters.len())?;
    a.unique(parameters.keys().map(String::as_str))?;
    let mut validator = Validator::new(app, a);
    validator.nodes = prefix;
    a.map_iter(parameters.len())?;
    for typ in parameters.values() {
        validator.typ(typ, 0)?;
    }
    let env = a.clone_owned(&parameters)?;
    a.bounded(0)?;
    a.bounded(0)?;
    a.bounded(1)?;
    a.slice(1)?;
    a.pay(8)?;
    validator.node(0)?;
    a.nonempty("Projection type check")?;
    a.pay(16)?; // Original expect + Expr dispatch before Equal's node.
    validator.node(1)?;
    // Both actual depth2 occurrences use the SAME original Validator and node counter.
    let actual = validator.comparison_operands(expr, expr, &env, 2, true, false)?;
    let expected = Type::Boolean;
    let equal = a.type_equal(&actual, &expected)?;
    a.type_message(44, &expected, &actual)?;
    a.require_owned(
        equal,
        format!("expression type mismatch: expected {expected:?}, found {actual:?}"),
    )?;
    a.pay(6)?;
    let suffix = total.checked_sub(prefix).ok_or_else(|| a.overflow())?;
    let augmented = validator
        .nodes
        .checked_add(suffix)
        .ok_or_else(|| a.overflow())?;
    a.require(
        augmented <= MAX_AST_NODES,
        "AST depth or node budget exceeded",
    )?;
    a.pay(16 + size_of::<SyntheticAction<'_>>() as u64 + 4 * size_of::<usize>() as u64)?;
    let synthetic = SyntheticAction {
        id: &id,
        label: "Host projection type check",
        parameters: Parameters { binding, entity },
        guards: [],
        ensures: [],
        steps: [SyntheticAssert {
            kind: "assert",
            condition: SyntheticEqual {
                kind: "equal",
                left: expr,
                right: expr,
            },
            message: "Projection type check",
        }],
    };
    // Ordinary output retains its original lifetime through the wire-limit tail.
    // Only the audited path removes the Value tree and final Vec.
    a.pay(32 + 2 * size_of::<Result<usize>>() as u64 + 2 * size_of::<usize>() as u64)?;
    let bytes;
    let synthetic_len = if a.enabled() {
        capture_canonical::synthetic_canonical_len_audited(&synthetic, a)?
    } else {
        bytes = capture_canonical::canonical_bytes_audited(&synthetic, a)?;
        bytes.len()
    };
    a.pay(6)?;
    let len = wire
        .checked_add(synthetic_len)
        .and_then(|n| n.checked_add(usize::from(actions > 0)))
        .ok_or_else(|| a.overflow())?;
    a.require(len <= MAX_WIRE_BYTES, "program exceeds byte budget")?;
    a.pay(0)
}


// A paid, concrete height walk, used only to reserve this action's two later
// serialization stacks. These are actual logical frame/lifetime slots, never
// heap allocations: self/a plus six field references, six scalar temporaries,
// one iterator and its Option, three result and two unit-result carriers.
const SYNTHETIC_HEIGHT_FRAME: usize = 2 * (
    8 * size_of::<usize>() + 6 * size_of::<usize>()
    + size_of::<std::slice::Iter<'static, Expr>>() + size_of::<Option<&Expr>>()
    + 3 * size_of::<Result<usize>>() + 2 * size_of::<Result<()>>()
    + size_of::<Option<usize>>()
);
impl SyntheticAction<'_> {
    pub(super) fn serialization_height(&self, a: Audit<'_>) -> Result<usize> {
        a.pay(SYNTHETIC_HEIGHT_FRAME as u64 + 96)?;
        let SyntheticAction { id: _, label: _, parameters: _, guards: [], steps: [step], ensures: [] } = self;
        let SyntheticAssert { kind: _, condition: SyntheticEqual { kind: _, left, right }, message: _ } = step;
        // action / steps / assert / equal. Parameters / Reference has height3.
        // Visit BOTH occurrences, even though this lexical owner borrows one Expr.
        let left = synthetic_expr_height(left, a)?;
        let right = synthetic_expr_height(right, a)?;
        left.max(right).checked_add(4).ok_or_else(|| a.overflow())
    }
}
fn synthetic_expr_height(expr: &Expr, a: Audit<'_>) -> Result<usize> {
    a.pay(SYNTHETIC_HEIGHT_FRAME as u64 + 96)?;
    let child = match expr {
        Expr::Literal { value_type, value } => {
            let typ = synthetic_type_height(value_type, a)?;
            typ.max(synthetic_value_height(value, a)?)
        }
        Expr::Variable { name: _ } | Expr::State { state: _ } | Expr::Today => 0,
        Expr::Field { record, field: _ } => synthetic_expr_height(record, a)?,
        Expr::Query { entity: _, binding: _, predicate, sort, limit: _, include_archived: _ } => {
            let mut height = synthetic_expr_height(predicate, a)?.max(1);
            let mut iter = sort.iter();
            loop {
                a.pay(32)?; // next, Option, SortKey borrow, max and checked add
                let Some(SortKey { value, descending: _ }) = iter.next() else { break };
                let next = synthetic_expr_height(value, a)?.checked_add(2).ok_or_else(|| a.overflow())?;
                height = height.max(next); // sort array + SortKey object
            }
            height
        }
        Expr::Filter { items, binding: _, predicate } | Expr::Any { items, binding: _, predicate }
        | Expr::All { items, binding: _, predicate } => {
            let items = synthetic_expr_height(items, a)?;
            items.max(synthetic_expr_height(predicate, a)?)
        }
        Expr::Map { items, binding: _, value } | Expr::Contains { items, value } => {
            let items = synthetic_expr_height(items, a)?;
            items.max(synthetic_expr_height(value, a)?)
        }
        Expr::Count { items } | Expr::Sum { items } => synthetic_expr_height(items, a)?,
        Expr::Equal { left, right } | Expr::Less { left, right } | Expr::Add { left, right }
        | Expr::Subtract { left, right } | Expr::Concat { left, right } => {
            let left = synthetic_expr_height(left, a)?;
            left.max(synthetic_expr_height(right, a)?)
        }
        Expr::TextContains { text, search } => {
            let text = synthetic_expr_height(text, a)?;
            text.max(synthetic_expr_height(search, a)?)
        }
        Expr::Lower { value } | Expr::Not { value } => synthetic_expr_height(value, a)?,
        Expr::And { values } | Expr::Or { values } => {
            let mut height = 0usize;
            let mut iter = values.iter();
            loop {
                a.pay(32)?;
                let Some(value) = iter.next() else { break };
                height = height.max(synthetic_expr_height(value, a)?);
            }
            height.checked_add(1).ok_or_else(|| a.overflow())? // values array, even empty
        }
        Expr::If { condition, then_value, else_value } => {
            let condition = synthetic_expr_height(condition, a)?;
            let then_value = synthetic_expr_height(then_value, a)?;
            condition.max(then_value).max(synthetic_expr_height(else_value, a)?)
        }
        Expr::Coalesce { value, fallback } => {
            let value = synthetic_expr_height(value, a)?;
            value.max(synthetic_expr_height(fallback, a)?)
        }
        Expr::DateAdd { date, days } => {
            let date = synthetic_expr_height(date, a)?;
            date.max(synthetic_expr_height(days, a)?)
        }
        Expr::DateDifference { later, earlier } => {
            let later = synthetic_expr_height(later, a)?;
            later.max(synthetic_expr_height(earlier, a)?)
        }
    };
    child.checked_add(1).ok_or_else(|| a.overflow()) // internally tagged object
}
fn synthetic_type_height(typ: &Type, a: Audit<'_>) -> Result<usize> {
    a.pay(SYNTHETIC_HEIGHT_FRAME as u64 + 96)?;
    let child = match typ {
        Type::Boolean | Type::Integer | Type::Text | Type::Date | Type::Reference { entity: _ } => 0,
        Type::List { item } | Type::Optional { item } => synthetic_type_height(item, a)?,
    };
    child.checked_add(1).ok_or_else(|| a.overflow())
}
fn synthetic_value_height(value: &DataValue, a: Audit<'_>) -> Result<usize> {
    a.pay(SYNTHETIC_HEIGHT_FRAME as u64 + 96)?;
    let child = match value {
        DataValue::Null | DataValue::Boolean { value: _ } | DataValue::Integer { value: _ }
        | DataValue::Text { value: _ } | DataValue::Date { days: _ }
        | DataValue::Reference { entity: _, record: _ } => 0,
        DataValue::List { item_type, items } => {
            let mut height = synthetic_type_height(item_type, a)?.max(1);
            let mut iter = items.iter();
            loop {
                a.pay(32)?;
                let Some(value) = iter.next() else { break };
                let next = synthetic_value_height(value, a)?.checked_add(1).ok_or_else(|| a.overflow())?;
                height = height.max(next); // items array, including its empty case
            }
            height
        }
    };
    child.checked_add(1).ok_or_else(|| a.overflow())
}

#[cfg(test)]
#[path = "capture_synthetic_extension_tests.rs"]
mod synthetic_extension_tests;
