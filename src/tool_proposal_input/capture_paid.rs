use super::capture_payment_core as payment;
use super::*;
use std::cell::RefCell;
use std::mem::size_of;

#[derive(Debug)]
pub(crate) enum InputWorkError<E> {
    Input(InputError),
    Resource(E),
    ArithmeticOverflow,
}

pub(super) struct LexicalEnvelope {
    pub(super) raw_len: usize,
    pub(super) container_depth: usize,
    pub(super) spans: usize,
    pub(super) string_bytes: usize,
    pub(super) escaped_bytes: usize,
    pub(super) max_escaped: usize,
    pub(super) escapes: usize,
}

fn add(a: usize, b: usize) -> Result<usize, ()> {
    a.checked_add(b).ok_or(())
}

pub(super) fn lexical_envelope_with_work<E>(
    bytes: &[u8],
    work: &mut impl FnMut(u64) -> Result<(), E>,
) -> Result<LexicalEnvelope, InputWorkError<E>> {
    work(10).map_err(InputWorkError::Resource)?;
    let mut shape = LexicalEnvelope {
        raw_len: bytes.len(),
        container_depth: 0,
        spans: 0,
        string_bytes: 0,
        escaped_bytes: 0,
        max_escaped: 0,
        escapes: 0,
    };
    let (mut inside, mut escaped, mut has_escape) = (false, false, false);
    let (mut depth, mut span) = (0usize, 0usize);
    for &byte in bytes {
        work(18).map_err(InputWorkError::Resource)?;
        let overflow = || InputWorkError::ArithmeticOverflow;
        if inside {
            if !escaped && byte == b'"' {
                inside = false;
                if has_escape {
                    shape.escaped_bytes = add(shape.escaped_bytes, span).map_err(|_| overflow())?;
                    shape.max_escaped = shape.max_escaped.max(span);
                }
            } else {
                span = add(span, 1).map_err(|_| overflow())?;
                shape.string_bytes = add(shape.string_bytes, 1).map_err(|_| overflow())?;
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                    has_escape = true;
                    shape.escapes = add(shape.escapes, 1).map_err(|_| overflow())?;
                }
            }
        } else if byte == b'"' {
            inside = true;
            escaped = false;
            has_escape = false;
            span = 0;
            shape.spans = add(shape.spans, 1).map_err(|_| overflow())?;
        } else if byte == b'[' || byte == b'{' {
            depth = add(depth, 1).map_err(|_| overflow())?;
            shape.container_depth = shape.container_depth.max(depth.min(65));
        } else if byte == b']' || byte == b'}' {
            depth = depth.saturating_sub(1);
        }
    }
    work(9).map_err(InputWorkError::Resource)?;
    if inside && has_escape {
        shape.escaped_bytes = shape
            .escaped_bytes
            .checked_add(span)
            .ok_or(InputWorkError::ArithmeticOverflow)?;
        shape.max_escaped = shape.max_escaped.max(span);
    }
    Ok(shape)
}

fn tokenizer_reserve(s: &LexicalEnvelope) -> Option<u64> {
    let n = u64::try_from(s.raw_len).ok()?;
    let a = u64::try_from(s.container_depth).ok()?;
    let c = if s.escapes == 0 {
        0
    } else {
        s.max_escaped.checked_mul(2)?.max(8)
    };
    let g = if c == 0 {
        0
    } else {
        payment::growth_count(s.max_escaped)?
    };
    let eb = size_of::<Box<str>>()
        .max(size_of::<io::Error>())
        .checked_add(size_of::<usize>().checked_mul(4)?)?;
    let mut cost = n.checked_mul(42)?;
    for amount in [
        u64::try_from(s.string_bytes).ok()?.checked_mul(3)?,
        u64::try_from(s.escaped_bytes).ok()?,
        u64::try_from(c).ok()?.checked_mul(3)?,
        g.checked_mul(9)?,
        u64::try_from(s.spans).ok()?.checked_mul(4)?,
        a.checked_add(2)?.checked_mul(n)?.checked_mul(2)?,
        a.checked_add(2)?
            .checked_mul(u64::try_from(eb).ok()?.checked_mul(2)?.checked_add(5)?)?,
        2646,
        a.checked_add(1)?.checked_mul(
            8u64.checked_add(u64::try_from(size_of::<usize>()).ok()?.checked_mul(8)?)?,
        )?,
        8,
    ] {
        cost = cost.checked_add(amount)?;
    }
    Some(cost)
}

pub(super) trait IntakeCharge {
    fn charge(&mut self, units: u64) -> Result<(), ()>;
    fn overflow(&mut self);
}
struct Transport<'a, E, F> {
    callback: &'a mut F,
    first: Option<E>,
    overflow: bool,
}
impl<E, F: FnMut(u64) -> Result<(), E>> IntakeCharge for Transport<'_, E, F> {
    fn charge(&mut self, units: u64) -> Result<(), ()> {
        if self.first.is_some() || self.overflow {
            return Err(());
        }
        match (self.callback)(units) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.first = Some(error);
                Err(())
            }
        }
    }
    fn overflow(&mut self) {
        if self.first.is_none() {
            self.overflow = true;
        }
    }
}

pub(crate) fn parse_json_bytes_with_work<E: Clone>(
    bytes: &[u8],
    work: &mut impl FnMut(u64) -> Result<(), E>,
) -> Result<Value, InputWorkError<E>> {
    work(0).map_err(InputWorkError::Resource)?;
    work(2).map_err(InputWorkError::Resource)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(InputWorkError::Input(InputError::InputTooLarge));
    }
    let shape = lexical_envelope_with_work(bytes, work)?;
    work(96 + 4 * usize::BITS as u64).map_err(InputWorkError::Resource)?;
    let reserve = tokenizer_reserve(&shape).ok_or(InputWorkError::ArithmeticOverflow)?;
    work(reserve).map_err(InputWorkError::Resource)?;
    let transport = RefCell::new(Transport {
        callback: work,
        first: None,
        overflow: false,
    });
    let mut violation = None;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = ValueSeed {
        depth: 0,
        violation: &mut violation,
        paid: Some(&transport),
    }
    .deserialize(&mut deserializer);
    let value = value.and_then(|value| {
        deserializer.end()?;
        Ok(value)
    });
    let mut transport = transport.into_inner();
    if let Some(error) = transport.first.take() {
        return Err(InputWorkError::Resource(error));
    }
    if transport.overflow {
        return Err(InputWorkError::ArithmeticOverflow);
    }
    if let Some(error) = violation {
        return Err(InputWorkError::Input(error));
    }
    // Native semantic errors precede the new completion poll.
    let value = value.map_err(|error| InputWorkError::Input(InputError::InvalidJson(error)))?;
    (transport.callback)(0).map_err(InputWorkError::Resource)?;
    Ok(value)
}

impl ValueSeed<'_, '_> {
    pub(super) fn pay<E: de::Error>(&self, units: u64) -> Result<(), E> {
        if let Some(work) = self.paid {
            work.borrow_mut()
                .charge(units)
                .map_err(|()| E::custom("capture work refused"))?;
        }
        Ok(())
    }
    pub(super) fn reserve<E: de::Error>(&self, amount: Option<u64>) -> Result<(), E> {
        if self.paid.is_none() {
            return Ok(());
        }
        match amount {
            Some(n) => self.pay(n),
            None => {
                self.paid
                    .expect("checked paid branch")
                    .borrow_mut()
                    .overflow();
                Err(E::custom("capture work arithmetic overflow"))
            }
        }
    }
    pub(super) fn copy_string<E: de::Error>(&self, bytes: usize) -> Result<(), E> {
        if self.paid.is_some() {
            self.pay(8)?;
            self.reserve(payment::string_copy_bytes(bytes))?;
        }
        Ok(())
    }
    pub(super) fn map_search<E: de::Error>(&self, len: usize, key: &str) -> Result<(), E> {
        if self.paid.is_some() {
            self.pay(payment::SEARCH_DESCRIPTOR + 8)?;
            let n = u64::try_from(key.len())
                .ok()
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(3));
            self.reserve(n.and_then(|c| payment::btree_search(len, c)))?;
        }
        Ok(())
    }
    pub(super) fn map_insert<E: de::Error>(&self, len: usize, key: &str) -> Result<(), E> {
        if self.paid.is_some() {
            self.pay(payment::INSERT_DESCRIPTOR + 8)?;
            let n = u64::try_from(key.len())
                .ok()
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(3));
            self.reserve(n.and_then(|c| payment::btree_insert::<String, Value>(len, c)))?;
        }
        Ok(())
    }
    pub(super) fn push<E: de::Error>(
        &self,
        values: &mut Vec<Value>,
        value: Value,
    ) -> Result<(), E> {
        if self.paid.is_some() {
            self.pay(16)?;
            let m = values.len();
            let size = size_of::<Value>();
            let capacity = if m == values.capacity() {
                m.checked_mul(2).map(|n| n.max(1))
            } else {
                Some(0)
            };
            let amount = capacity.and_then(|g| {
                let growth = if g == 0 {
                    Some(0)
                } else {
                    m.checked_add(g)?.checked_mul(size)?.checked_add(1)
                }?;
                u64::try_from(growth.checked_add(size)?.checked_add(8)?).ok()
            });
            self.reserve(amount)?;
            if m == values.capacity() {
                let target = capacity.expect("capacity checked before native effect");
                let layout = match std::alloc::Layout::array::<Value>(target) {
                    Ok(layout) => layout,
                    Err(_) => {
                        self.paid
                            .expect("checked paid branch")
                            .borrow_mut()
                            .overflow();
                        return Err(E::custom("capture work arithmetic overflow"));
                    }
                };
                // Numeric capacity failure was excluded above. Preserve the old
                // infallible Vec allocation-failure boundary rather than inventing
                // a new owned input diagnostic after a successful work reservation.
                if values.try_reserve_exact(target - m).is_err() {
                    std::alloc::handle_alloc_error(layout);
                }
            }
        }
        values.push(value);
        Ok(())
    }
}
