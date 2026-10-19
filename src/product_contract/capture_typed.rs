//! Value intake stays native; this paid descriptor precedes native App conversion.
use super::bounded_input::capture_payment_core as native;
use super::capture_work::Audit;
use super::*;
use std::mem::size_of;

struct Conversion {
    bytes: usize,
    native: u64,
}
impl Conversion {
    fn add(&mut self, n: u64, a: Audit<'_>) -> Result<()> {
        a.pay(3)?;
        self.native = self.native.checked_add(n).ok_or_else(|| a.overflow())?;
        Ok(())
    }
    fn string(&mut self, value: &str, a: Audit<'_>) -> Result<()> {
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
        let initial = len.min(MAX_WIRE_BYTES / slot.max(1));
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
        let requests = initial
            .checked_add(capacity.checked_mul(3).ok_or_else(|| a.overflow())?)
            .and_then(|n| n.checked_mul(slot))
            .ok_or_else(|| a.overflow())?;
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
    fn value(&mut self, value: &JsonValue, depth: usize, a: Audit<'_>) -> Result<()> {
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
                    self.value(value, depth + 1, a)?;
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
                    self.value(value, depth + 1, a)?;
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
    a.pay(8 + 65 * (8 * size_of::<usize>() as u64))?;
    let mut descriptor = Conversion {
        bytes: 0,
        native: 0,
    };
    descriptor.value(&value, 0, a)?;
    a.pay(64 + 4 * usize::BITS as u64)?;
    let diagnostic = descriptor
        .bytes
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

#[cfg(test)]
#[path = "capture_typed_bound_tests.rs"]
mod bound_tests;
