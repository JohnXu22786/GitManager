//! Fallible ordered storage for the Product Contract `Values` migration.
//!
//! This wrapper deliberately has no `Clone`, `FromIterator`, or `Deserialize`
//! implementation: each could hide an unmetered allocation. Callers must use
//! `try_insert`/`try_from_iter` and carry the allocation failure unchanged.
use crate::product_alloc::{ordered::OrderedMap, AllocationContext, AllocationFailure};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::fmt;

use super::{DataValue, Id};

/// Sorted, fallibly grown storage for Product Contract record/session values.
/// Iteration and serialization use the same lexical key order as `BTreeMap`.
pub(crate) struct FallibleValues(OrderedMap<Id, DataValue>);

impl FallibleValues {
    pub(crate) const fn new() -> Self {
        Self(OrderedMap::new())
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn get(&self, id: &str) -> Option<&DataValue> {
        self.0.get(id)
    }

    pub(crate) fn get_mut(&mut self, id: &str) -> Option<&mut DataValue> {
        self.0.get_mut(id)
    }

    pub(crate) fn contains_key(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = (&Id, &DataValue)> + ExactSizeIterator {
        self.0.iter()
    }

    /// Occupied keys retain the original key and replace the value, matching
    /// `BTreeMap::insert`; vacant-key storage is reserved before mutation.
    /// This accounts for the map's storage only; callers must construct the
    /// owned key/value through their own fallible paths.
    pub(crate) fn try_insert(
        &mut self,
        id: Id,
        value: DataValue,
        cx: &mut AllocationContext,
    ) -> Result<Option<DataValue>, AllocationFailure> {
        self.0.try_insert(id, value, cx)
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<DataValue> {
        self.0.remove(id)
    }

    pub(crate) fn remove_entry(&mut self, id: &str) -> Option<(Id, DataValue)> {
        self.0.remove_entry(id)
    }

    /// Consume fallible producers without introducing an infallible
    /// `FromIterator` path. Duplicate keys retain the first key and last value,
    /// matching `BTreeMap` construction from an ordered iterator. Iterator
    /// creation/advancement is caller work and must itself be allocation-safe.
    pub(crate) fn try_from_iter<I>(
        entries: I,
        cx: &mut AllocationContext,
    ) -> Result<Self, AllocationFailure>
    where
        I: IntoIterator<Item = Result<(Id, DataValue), AllocationFailure>>,
    {
        OrderedMap::try_from_iter(entries, cx).map(Self)
    }
}

impl Serialize for FallibleValues {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (id, value) in self.iter() {
            map.serialize_entry(id, value)?;
        }
        map.end()
    }
}

impl fmt::Debug for FallibleValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl PartialEq for FallibleValues {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self.iter().zip(other.iter()).all(|((ak, av), (bk, bv))| ak == bk && av == bv)
    }
}

impl Eq for FallibleValues {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    fn text(value: &str) -> DataValue {
        DataValue::Text { value: value.to_owned() }
    }

    #[test]
    fn sorted_iteration_and_canonical_bytes_match_btree_values() {
        let mut legacy = BTreeMap::new();
        legacy.insert("z-last".to_owned(), text("plain"));
        legacy.insert("a-first".to_owned(), text("雪\n\""));
        legacy.insert("m-middle".to_owned(), DataValue::List {
            item_type: super::super::Type::Text,
            items: vec![text("one"), text("two")],
        });

        let mut cx = AllocationContext::new();
        let values = FallibleValues::try_from_iter(
            legacy.iter().map(|(id, value)| Ok((id.clone(), value.clone()))),
            &mut cx,
        ).unwrap();

        let keys: Vec<_> = values.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(keys, ["a-first", "m-middle", "z-last"]);
        assert_eq!(
            super::super::canonical_bytes(&legacy).unwrap(),
            super::super::canonical_bytes(&values).unwrap(),
        );
        assert_eq!(values.len(), 3);
        assert!(!values.is_empty());
    }

    #[test]
    fn equality_matches_key_value_semantics_independent_of_insertion_order() {
        let mut left = FallibleValues::new();
        let mut right = FallibleValues::new();
        let mut left_cx = AllocationContext::new();
        let mut right_cx = AllocationContext::new();
        left.try_insert("b".to_owned(), text("two"), &mut left_cx).unwrap();
        left.try_insert("a".to_owned(), text("one"), &mut left_cx).unwrap();
        right.try_insert("a".to_owned(), text("one"), &mut right_cx).unwrap();
        right.try_insert("b".to_owned(), text("two"), &mut right_cx).unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn duplicate_insert_replaces_without_growth_and_preserves_map_semantics() {
        let mut values = FallibleValues::new();
        let mut cx = AllocationContext::new();
        let mut original_key = String::with_capacity(32);
        original_key.push_str("same");
        assert!(values.try_insert(original_key, text("first"), &mut cx).unwrap().is_none());
        let requests = cx.growth_requests();
        assert_eq!(values.try_insert("same".to_owned(), text("last"), &mut cx).unwrap(), Some(text("first")));
        assert_eq!(cx.growth_requests(), requests);
        assert_eq!(values.get("same"), Some(&text("last")));
        assert!(values.iter().next().unwrap().0.capacity() >= 32);
        assert!(values.contains_key("same"));
        assert_eq!(values.remove_entry("same"), Some(("same".to_owned(), text("last"))));
        assert!(values.is_empty());
        assert_eq!(values.remove("missing"), None);
    }

    #[test]
    fn each_reached_map_growth_refusal_preserves_input_failure_and_allows_retry() {
        let input = || vec![
            Ok(("a".to_owned(), text("one"))),
            Ok(("b".to_owned(), text("two"))),
            Ok(("c".to_owned(), text("three"))),
        ];

        let mut count_cx = AllocationContext::new();
        let full = FallibleValues::try_from_iter(input(), &mut count_cx).unwrap();
        let growths = count_cx.growth_requests();
        assert_eq!(growths, 3);
        drop(full);

        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = FallibleValues::try_from_iter(input(), &mut cx).unwrap_err();
            assert_eq!(Some(error), cx.first_failure());
            assert_eq!(error.site, crate::product_alloc::AllocationSite::OrderedMap);

            // The refused partial map has already been dropped by the failed
            // construction; a fresh attempt with a fresh context succeeds.
            let mut retry_cx = AllocationContext::new();
            let retry = FallibleValues::try_from_iter(input(), &mut retry_cx).unwrap();
            assert_eq!(retry.len(), 3);
        }
    }

    #[test]
    fn failed_single_insert_keeps_existing_entries_unchanged() {
        let mut values = FallibleValues::new();
        let mut cx = AllocationContext::new();
        values.try_insert("kept".to_owned(), text("stable"), &mut cx).unwrap();

        let mut fail = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        let error = values.try_insert("new".to_owned(), text("refused"), &mut fail).unwrap_err();
        assert_eq!(Some(error), fail.first_failure());
        assert_eq!(values.len(), 1);
        assert_eq!(values.get("kept"), Some(&text("stable")));
        assert_eq!(values.get("new"), None);

        let mut retry = AllocationContext::new();
        values.try_insert("new".to_owned(), text("accepted"), &mut retry).unwrap();
        assert_eq!(values.len(), 2);
    }
}
