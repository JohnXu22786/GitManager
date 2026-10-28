//! Sorted-vector foundation, not a std BTree API or whole-request OOM proof.
//!
//! Search is O(log n) comparisons; insert/remove move at most n entries.
//! Building n distinct entries can move O(n²) entries (also counting reserve
//! relocations), not the historical BTree tariff. No smaller input cap/new H.
//! Comparator, Borrow, iterator, copy callback and destructor work/allocations
//! must be closed separately for each protected product type. In particular,
//! arbitrary callbacks can allocate, panic, or mutate interior state.
use super::{try_reserve, AllocationContext, AllocationFailure, AllocationSite};
use std::borrow::Borrow;

/// Keys never escape mutably; all growth is explicit and fallible.
/// As for BTreeMap, a key's ordering must not change while it is stored, and
/// Borrow<Q> must agree with its Ord ordering.
pub struct OrderedMap<K, V> { entries: Vec<(K, V)> }

impl<K, V> OrderedMap<K, V> {
    pub const fn new() -> Self { Self { entries: Vec::new() } }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn capacity(&self) -> usize { self.entries.capacity() }
    /// Immutable O(1) access in lexical order; keys never escape mutably.
    pub fn get_index(&self, index: usize) -> Option<(&K, &V)> {
        self.entries.get(index).map(|(key, value)| (key, value))
    }
    /// Consume the existing sorted storage without allocation or copying.
    /// Dropping a partially consumed iterator drops its remaining owners once.
    pub fn into_entries(self) -> std::vec::IntoIter<(K, V)> { self.entries.into_iter() }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + ExactSizeIterator {
        self.entries.iter().map(|(k, v)| (k, v))
    }
    /// Additional elements, not a target capacity; overflow and ZST semantics
    /// come directly from the allocation foundation. Failure changes nothing.
    pub fn try_reserve(&mut self, additional: usize, cx: &mut AllocationContext)
        -> Result<(), AllocationFailure> {
        self.reserve_at(additional, cx, AllocationSite::OrderedMap)
    }
    fn reserve_at(&mut self, additional: usize, cx: &mut AllocationContext,
        site: AllocationSite) -> Result<(), AllocationFailure> {
        try_reserve(&mut self.entries, additional, cx, site)
    }
    fn search<Q: Ord + ?Sized>(&self, key: &Q) -> Result<usize, usize>
        where K: Borrow<Q> {
        self.entries.binary_search_by(|(stored, _)| stored.borrow().cmp(key))
    }
    pub fn get<Q: Ord + ?Sized>(&self, key: &Q) -> Option<&V>
        where K: Borrow<Q> {
        self.get_key_value(key).map(|(_, value)| value)
    }
    pub fn get_key_value<Q: Ord + ?Sized>(&self, key: &Q) -> Option<(&K, &V)>
        where K: Borrow<Q> {
        self.search(key).ok().map(|index| {
            let (key, value) = &self.entries[index];
            (key, value)
        })
    }
    pub fn get_mut<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
        where K: Borrow<Q> {
        let index = self.search(key).ok()?;
        Some(&mut self.entries[index].1)
    }
    pub fn contains_key<Q: Ord + ?Sized>(&self, key: &Q) -> bool
        where K: Borrow<Q> {
        self.search(key).is_ok()
    }
    /// Nonallocating storage removal. The returned key/value remain owned by
    /// the caller; their eventual destructors still require closure review.
    pub fn remove_entry<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<(K, V)>
        where K: Borrow<Q> {
        let index = self.search(key).ok()?;
        Some(self.entries.remove(index))
    }
    pub fn remove<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<V>
        where K: Borrow<Q> {
        self.remove_entry(key).map(|(_, value)| value)
    }
}

impl<K: Ord, V> OrderedMap<K, V> {
    /// Occupied: retain the original key, return the old value, no growth.
    /// Vacant: reserve before moving any entry, then insert without callbacks.
    /// On reservation failure contents/capacity are unchanged and the incoming
    /// key/value are dropped exactly once. No error is reformatted or replaced.
    pub fn try_insert(&mut self, key: K, value: V, cx: &mut AllocationContext)
        -> Result<Option<V>, AllocationFailure> {
        self.insert_at(key, value, cx, AllocationSite::OrderedMap)
    }
    fn insert_at(&mut self, key: K, value: V, cx: &mut AllocationContext,
        site: AllocationSite) -> Result<Option<V>, AllocationFailure> {
        match self.search(&key) {
            Ok(index) => Ok(Some(std::mem::replace(&mut self.entries[index].1, value))),
            Err(index) => {
                self.reserve_at(1, cx, site)?;
                self.entries.insert(index, (key, value));
                Ok(None)
            }
        }
    }
    /// Consume a fallible producer, retaining the first key and last value for
    /// duplicates. First producer/reservation error stops iteration and drops
    /// the local result and remaining owned input. The producer must itself be
    /// closed; IntoIterator/next are NOT claimed allocation-free generically.
    pub fn try_from_iter<I>(entries: I, cx: &mut AllocationContext)
        -> Result<Self, AllocationFailure>
        where I: IntoIterator<Item = Result<(K, V), AllocationFailure>> {
        let mut result = Self::new();
        for entry in entries {
            let (key, value) = entry?;
            result.try_insert(key, value, cx)?;
        }
        Ok(result)
    }
    /// Reserve the destination before copying children; never call Clone.
    /// Callbacks must implement the closed fallible deep copy of each field.
    /// Source storage is only borrowed, and a failure drops initialized output
    /// plus any in-progress key. Arbitrary interior-mutating callbacks are not
    /// a source-immutability guarantee. Ordering is rechecked by insertion, so
    /// even transformed/duplicate copied keys cannot break the representation.
    pub fn try_clone_with<FK, FV>(&self, cx: &mut AllocationContext,
        copy_key: FK, copy_value: FV) -> Result<Self, AllocationFailure>
        where FK: FnMut(&K, &mut AllocationContext) -> Result<K, AllocationFailure>,
              FV: FnMut(&V, &mut AllocationContext) -> Result<V, AllocationFailure> {
        self.clone_at(cx, copy_key, copy_value, AllocationSite::OrderedMap)
    }
    fn clone_at<FK, FV>(&self, cx: &mut AllocationContext,
        mut copy_key: FK, mut copy_value: FV, site: AllocationSite)
        -> Result<Self, AllocationFailure>
        where FK: FnMut(&K, &mut AllocationContext) -> Result<K, AllocationFailure>,
              FV: FnMut(&V, &mut AllocationContext) -> Result<V, AllocationFailure> {
        let mut result = Self::new();
        result.reserve_at(self.len(), cx, site)?;
        for (key, value) in &self.entries {
            let key = copy_key(key, cx)?;
            let value = copy_value(value, cx)?;
            result.insert_at(key, value, cx, site)?;
        }
        Ok(result)
    }
}

/// Set insertion retains the original equal key, like BTreeSet::insert.
pub struct OrderedSet<K> { map: OrderedMap<K, ()> }
impl<K> OrderedSet<K> {
    pub const fn new() -> Self { Self { map: OrderedMap::new() } }
    pub fn len(&self) -> usize { self.map.len() }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }
    pub fn capacity(&self) -> usize { self.map.capacity() }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &K> + ExactSizeIterator {
        self.map.iter().map(|(key, ())| key)
    }
    pub fn try_reserve(&mut self, additional: usize, cx: &mut AllocationContext)
        -> Result<(), AllocationFailure> {
        self.map.reserve_at(additional, cx, AllocationSite::OrderedSet)
    }
    pub fn get<Q: Ord + ?Sized>(&self, key: &Q) -> Option<&K> where K: Borrow<Q> {
        self.map.get_key_value(key).map(|(key, ())| key)
    }
    pub fn contains<Q: Ord + ?Sized>(&self, key: &Q) -> bool where K: Borrow<Q> {
        self.map.contains_key(key)
    }
    pub fn take<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<K> where K: Borrow<Q> {
        self.map.remove_entry(key).map(|(key, ())| key)
    }
    pub fn remove<Q: Ord + ?Sized>(&mut self, key: &Q) -> bool where K: Borrow<Q> {
        self.take(key).is_some()
    }
}
impl<K: Ord> OrderedSet<K> {
    pub fn try_insert(&mut self, key: K, cx: &mut AllocationContext)
        -> Result<bool, AllocationFailure> {
        self.map.insert_at(key, (), cx, AllocationSite::OrderedSet).map(|old| old.is_none())
    }
    /// Same producer/error/drop contract as OrderedMap::try_from_iter.
    pub fn try_from_iter<I>(keys: I, cx: &mut AllocationContext)
        -> Result<Self, AllocationFailure>
        where I: IntoIterator<Item = Result<K, AllocationFailure>> {
        let mut result = Self::new();
        for key in keys { result.try_insert(key?, cx)?; }
        Ok(result)
    }
    /// Same closed-copy/source/callback contract as OrderedMap::try_clone_with.
    pub fn try_clone_with<F>(&self, cx: &mut AllocationContext, copy_key: F)
        -> Result<Self, AllocationFailure>
        where F: FnMut(&K, &mut AllocationContext) -> Result<K, AllocationFailure> {
        Ok(Self { map: self.map.clone_at(cx, copy_key, |(), _| Ok(()), AllocationSite::OrderedSet)? })
    }
}
