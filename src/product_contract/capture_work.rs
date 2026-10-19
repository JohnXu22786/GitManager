//! Capture-route work transport. Ordinary validation uses `Audit::none()`.
use super::bounded_input::capture_payment_core as native;
use super::*;
use std::cell::RefCell;
use std::mem::size_of;

pub(crate) trait ContractWork {
    fn charge(&mut self, units: u64) -> std::result::Result<(), WorkFailure>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkFailure {
    Cancelled,
    BudgetExhausted(&'static str),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContractWorkError {
    Contract(ContractError),
    Resource(WorkFailure),
}
pub(crate) type ContractWorkResult<T> = std::result::Result<T, ContractWorkError>;
impl From<ContractError> for ContractWorkError {
    fn from(error: ContractError) -> Self {
        Self::Contract(error)
    }
}

pub(super) struct WorkState<'a> {
    work: RefCell<&'a mut dyn ContractWork>,
    first: RefCell<Option<WorkFailure>>,
}
impl<'a> WorkState<'a> {
    pub(super) fn new(work: &'a mut dyn ContractWork) -> Self {
        Self {
            work: RefCell::new(work),
            first: RefCell::new(None),
        }
    }
    pub(super) fn finish<T>(&self, result: Result<T>) -> ContractWorkResult<T> {
        if let Some(error) = self.first.borrow_mut().take() {
            Err(ContractWorkError::Resource(error))
        } else {
            result.map_err(ContractWorkError::Contract)
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Audit<'a>(pub(super) Option<&'a WorkState<'a>>);
impl<'a> Audit<'a> {
    pub(super) fn none() -> Self {
        Self(None)
    }
    pub(super) fn enabled(self) -> bool {
        self.0.is_some()
    }
    pub(super) fn pay(self, units: u64) -> Result<()> {
        if let Some(state) = self.0 {
            if state.first.borrow().is_some() {
                return Err(ContractError(String::new()));
            }
            if let Err(error) = state.work.borrow_mut().charge(units) {
                *state.first.borrow_mut() = Some(error);
                // Empty transport owner: the typed first cause is retained separately.
                return Err(ContractError(String::new()));
            }
        }
        Ok(())
    }
    pub(super) fn overflow(self) -> ContractError {
        if let Some(state) = self.0 {
            let mut first = state.first.borrow_mut();
            if first.is_none() {
                *first = Some(WorkFailure::BudgetExhausted(
                    "capture work arithmetic overflow",
                ));
            }
        }
        ContractError(String::new())
    }
    pub(super) fn amount(self, n: Option<u64>) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(n.ok_or_else(|| self.overflow())?)
    }
    pub(super) fn usize(self, n: usize) -> Result<u64> {
        u64::try_from(n).map_err(|_| self.overflow())
    }
    pub(super) fn scaled(self, n: usize, factor: u64, fixed: u64) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(5)?;
        self.amount(
            self.usize(n)?
                .checked_mul(factor)
                .and_then(|n| n.checked_add(fixed)),
        )
    }
    pub(super) fn string(self, n: usize) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(8)?;
        self.amount(native::string_copy_bytes(n))
    }
    pub(super) fn format(self, n: usize) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(64 + 4 * usize::BITS as u64)?;
        self.amount(native::format_bytes(n))?;
        // One retained first-error copy, unused on successful eager formatting.
        self.string(n)
    }
    pub(super) fn error(self, message: &str) -> Result<ContractError> {
        self.pay(2)?;
        self.string(message.len())?;
        self.string(message.len())?;
        Ok(ContractError(message.into()))
    }
    pub(super) fn require(self, condition: bool, message: &str) -> Result<()> {
        self.pay(1)?;
        if condition {
            Ok(())
        } else {
            Err(self.error(message)?)
        }
    }
    pub(super) fn require_owned(self, condition: bool, message: String) -> Result<()> {
        self.pay(1)?;
        if condition {
            Ok(())
        } else {
            Err(ContractError(message))
        }
    }
    pub(super) fn check_id(self, id: &str) -> Result<()> {
        if !self.enabled() {
            return check_id(id);
        }
        self.pay(4)?;
        let valid = if id.is_empty() || id.len() > 128 {
            false
        } else {
            let mut valid = true;
            self.pay(1)?;
            for byte in id.bytes() {
                self.pay(9)?;
                if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)) {
                    valid = false;
                    break;
                }
            }
            self.pay(1)?;
            valid
        };
        self.pay(2)?;
        let bound = id.len().checked_add(19).ok_or_else(|| self.overflow())?;
        self.format(bound)?;
        self.require_owned(valid, format!("invalid stable ID: {id}"))
    }
    pub(super) fn text(self, value: &str) -> Result<()> {
        self.pay(3)?;
        self.require(value.len() <= MAX_TEXT_BYTES, "text exceeds byte limit")
    }
    pub(super) fn nonempty(self, value: &str) -> Result<()> {
        self.text(value)?;
        // Two UTF-8/whitespace interface passes, each byte plus classification.
        self.scaled(value.len(), 4, 6)?;
        self.require(!value.trim().is_empty(), "empty required text")
    }
    pub(super) fn bounded(self, len: usize) -> Result<()> {
        self.pay(2)?;
        self.require(len <= MAX_ITEMS, "too many definitions or steps")
    }
    pub(super) fn version(self, version: u32) -> Result<()> {
        self.pay(2)?;
        self.require(version == CONTRACT_VERSION, "unsupported contract version")
    }
    pub(super) fn day(self, day: i32) -> Result<()> {
        self.pay(4)?;
        self.require(
            (-719_162..=2_932_896).contains(&day),
            "date outside years 0001 through 9999",
        )
    }
    pub(super) fn eq_str(self, left: &str, right: &str) -> Result<bool> {
        self.pay(3)?;
        if left.len() != right.len() {
            return Ok(false);
        }
        self.scaled(left.len(), 2, 0)?;
        Ok(left == right)
    }
    pub(super) fn comparator(self, bytes: usize) -> Result<u64> {
        if !self.enabled() {
            return Ok(0);
        }
        self.pay(6)?;
        self.usize(bytes)?
            .checked_mul(2)
            .and_then(|n| n.checked_add(3))
            .ok_or_else(|| self.overflow())
    }
    pub(super) fn map_search(self, len: usize, bytes: usize) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(native::SEARCH_DESCRIPTOR)?;
        let c = self.comparator(bytes)?;
        self.amount(native::btree_search(len, c))
    }
    pub(super) fn insert<K, V>(self, len: usize, comparator: u64) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(native::INSERT_DESCRIPTOR)?;
        self.amount(native::btree_insert::<K, V>(len, comparator))
    }
    pub(super) fn map_iter(self, len: usize) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(8)?;
        self.amount(native::btree_iter(len))
    }
    pub(super) fn slice(self, len: usize) -> Result<()> {
        // Iterator and terminal, each reached next and child edge.
        self.scaled(len, 3, 2)
    }
    pub(super) fn unique<'s>(self, ids: impl IntoIterator<Item = &'s str>) -> Result<()> {
        self.pay(9)?;
        let mut seen = BTreeSet::new();
        self.pay(1)?;
        for id in ids {
            self.pay(3)?;
            self.check_id(id)?;
            self.insert::<&str, ()>(seen.len(), self.comparator(id.len())?)?;
            let inserted = seen.insert(id);
            self.pay(2)?;
            self.format(id.len().checked_add(14).ok_or_else(|| self.overflow())?)?;
            self.require_owned(inserted, format!("duplicate ID: {id}"))?;
        }
        self.pay(1)?;
        self.bounded(seen.len())
    }
    pub(super) fn boxed<T>(self) -> Result<()> {
        self.scaled(size_of::<T>(), 2, 3)
    }
    pub(super) fn list_type(self, item: Type) -> Result<Type> {
        self.boxed::<Type>()?;
        Ok(Type::list(item))
    }
    pub(super) fn reference_type(self, entity: &str) -> Result<Type> {
        self.pay(2)?;
        self.string(entity.len())?;
        Ok(Type::reference(entity))
    }
    pub(super) fn type_debug(self, value: &Type) -> Result<usize> {
        self.pay(4)?;
        let child = match value {
            Type::Reference { entity } => {
                self.scaled(entity.len(), 1, 2)?;
                entity.len().checked_mul(6).ok_or_else(|| self.overflow())?
            }
            Type::List { item } | Type::Optional { item } => {
                self.pay(2)?;
                self.type_debug(item)?
            }
            _ => 0,
        };
        child.checked_add(32).ok_or_else(|| self.overflow())
    }
    pub(super) fn type_equal(self, left: &Type, right: &Type) -> Result<bool> {
        if !self.enabled() {
            return Ok(left == right);
        }
        self.pay(4)?;
        Ok(match (left, right) {
            (Type::Reference { entity: a }, Type::Reference { entity: b }) => self.eq_str(a, b)?,
            (Type::List { item: a }, Type::List { item: b })
            | (Type::Optional { item: a }, Type::Optional { item: b }) => {
                self.pay(2)?;
                self.type_equal(a, b)?
            }
            _ => left == right,
        })
    }
}
impl<'a> Audit<'a> {
    pub(super) fn get<'s, T>(
        self,
        map: &'s BTreeMap<String, T>,
        key: &str,
    ) -> Result<Option<&'s T>> {
        self.map_search(map.len(), key.len())?;
        Ok(map.get(key))
    }
    pub(super) fn has<T>(self, map: &BTreeMap<String, T>, key: &str) -> Result<bool> {
        self.map_search(map.len(), key.len())?;
        Ok(map.contains_key(key))
    }
    pub(super) fn insert_string<T>(
        self,
        map: &mut BTreeMap<String, T>,
        key: &str,
        value: T,
    ) -> Result<()> {
        self.string(key.len())?;
        self.insert::<String, T>(map.len(), self.comparator(key.len())?)?;
        map.insert(key.into(), value);
        Ok(())
    }
    pub(super) fn find<'s, T>(
        self,
        values: &'s [T],
        id: &str,
        key: impl Fn(&T) -> &str,
    ) -> Result<Option<&'s T>> {
        self.pay(1)?;
        for value in values {
            self.pay(3)?;
            if self.eq_str(key(value), id)? {
                return Ok(Some(value));
            }
        }
        self.pay(1)?;
        Ok(None)
    }
    pub(super) fn type_message(self, prefix: usize, expected: &Type, actual: &Type) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        self.pay(3)?;
        let n = self
            .type_debug(expected)?
            .checked_add(self.type_debug(actual)?)
            .and_then(|n| n.checked_add(prefix))
            .ok_or_else(|| self.overflow())?;
        self.format(n)
    }
    pub(super) fn tuple_comparator(self, tuple: &Vec<Id>) -> Result<u64> {
        if !self.enabled() {
            return Ok(0);
        }
        self.slice(tuple.len())?;
        let mut c = 3u64;
        for id in tuple {
            self.pay(3)?;
            c = c
                .checked_add(self.comparator(id.len())?)
                .and_then(|n| n.checked_add(3))
                .ok_or_else(|| self.overflow())?;
        }
        Ok(c)
    }
    pub(super) fn pair_comparator(self, left: &str, right: &str) -> Result<u64> {
        if !self.enabled() {
            return Ok(0);
        }
        self.pay(3)?;
        self.comparator(left.len())?
            .checked_add(self.comparator(right.len())?)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| self.overflow())
    }
    pub(super) fn key_comparator(self, key: &KeyBinding) -> Result<u64> {
        if !self.enabled() {
            return Ok(0);
        }
        self.pay(24)?;
        self.comparator(key.key.len())?
            .checked_add(native::btree_iter(key.modifiers.len()).ok_or_else(|| self.overflow())?)
            .and_then(|n| n.checked_add(native::btree_iter(4)?))
            .and_then(|n| n.checked_add((key.modifiers.len() as u64).checked_mul(4)?))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| self.overflow())
    }
}
