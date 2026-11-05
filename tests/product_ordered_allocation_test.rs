#[path = "../src/product_alloc.rs"]
mod product_alloc;
// Exercise the unchanged R01 regression cases in the same admitted binary.
#[path = "product_allocation_test.rs"]
mod foundation_tests;

use product_alloc::ordered::{OrderedMap, OrderedSet};
use product_alloc::{try_copy_str, AllocationContext as Context, AllocationFailure,
    AllocationFailureKind as Kind, AllocationSite as Site};
use std::borrow::Borrow;
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

fn fault(n: usize, sticky: bool) -> Context {
    Context::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}
fn pairs(map: &OrderedMap<i32, i32>) -> Vec<(i32, i32)> {
    map.iter().map(|(k, v)| (*k, *v)).collect()
}
fn map_oracle(input: &[i32]) {
    let mut map = OrderedMap::new();
    let mut oracle = BTreeMap::new();
    let mut cx = Context::new();
    for (index, &key) in input.iter().enumerate() {
        let value = index as i32;
        assert_eq!(map.try_insert(key, value, &mut cx).unwrap(), oracle.insert(key, value));
        assert_eq!(pairs(&map), oracle.iter().map(|(&k, &v)| (k, v)).collect::<Vec<_>>());
        for query in -2..14 {
            assert_eq!(map.get(&query), oracle.get(&query));
            assert_eq!(map.contains_key(&query), oracle.contains_key(&query));
        }
    }
    assert_eq!(map.len(), oracle.len());
    assert_eq!(map.is_empty(), oracle.is_empty());
    for &key in input.iter().rev() {
        if let Some(value) = map.get_mut(&key) { *value += 1; }
        if let Some(value) = oracle.get_mut(&key) { *value += 1; }
        assert_eq!(map.remove_entry(&key), oracle.remove_entry(&key));
    }
    assert!(map.is_empty());
}

#[test]
fn map_matches_btree_for_boundaries_and_small_sequences() {
    for n in [0, 1, 11, 12] {
        let mut input: Vec<i32> = (0..n).collect();
        map_oracle(&input);
        input.reverse();
        map_oracle(&input);
    }
    for length in 0..=4 {
        for mut encoding in 0..3_usize.pow(length) {
            let mut input = Vec::new();
            for _ in 0..length { input.push((encoding % 3) as i32 - 1); encoding /= 3; }
            map_oracle(&input);
        }
    }
}

#[test]
fn set_matches_btree_for_order_duplicates_and_removal() {
    for n in [0, 1, 11, 12] {
        for reverse in [false, true] {
            let mut input: Vec<i32> = (0..n).chain(0..n).collect();
            if reverse { input.reverse(); }
            let mut set = OrderedSet::new();
            let mut oracle = BTreeSet::new();
            let mut cx = Context::new();
            for key in input {
                assert_eq!(set.try_insert(key, &mut cx).unwrap(), oracle.insert(key));
                assert_eq!(set.iter().copied().collect::<Vec<_>>(), oracle.iter().copied().collect::<Vec<_>>());
            }
            assert_eq!(set.len(), oracle.len());
            assert_eq!(set.is_empty(), oracle.is_empty());
            for key in -1..=n {
                assert_eq!(set.contains(&key), oracle.contains(&key));
                assert_eq!(set.get(&key), oracle.get(&key));
                assert_eq!(set.take(&key), oracle.take(&key));
                assert_eq!(set.remove(&key), oracle.remove(&key));
            }
            assert!(set.is_empty());
        }
    }
}

#[derive(Debug)]
struct Key<'a> { text: &'static str, identity: usize, drops: &'a Cell<usize> }
impl PartialEq for Key<'_> { fn eq(&self, other: &Self) -> bool { self.text == other.text } }
impl Eq for Key<'_> {}
impl PartialOrd for Key<'_> { fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) } }
impl Ord for Key<'_> { fn cmp(&self, other: &Self) -> Ordering { self.text.cmp(other.text) } }
impl Borrow<str> for Key<'_> { fn borrow(&self) -> &str { self.text } }
impl Drop for Key<'_> { fn drop(&mut self) { self.drops.set(self.drops.get() + 1); } }
fn key<'a>(text: &'static str, identity: usize, drops: &'a Cell<usize>) -> Key<'a> {
    Key { text, identity, drops }
}

#[test]
fn borrowed_queries_and_occupied_insert_preserve_original_key() {
    let drops = Cell::new(0);
    let mut normal = Context::new();
    let mut refusing = fault(1, true);
    let mut map = OrderedMap::new();
    map.try_insert(key("中", 1, &drops), 7, &mut normal).unwrap();
    let cap = map.capacity();
    assert_eq!(map.try_insert(key("中", 2, &drops), 9, &mut refusing).unwrap(), Some(7));
    assert_eq!(map.get_key_value("中").map(|(k, v)| (k.identity, *v)), Some((1, 9)));
    assert_eq!(map.get("missing"), None);
    assert_eq!(map.capacity(), cap);
    assert_eq!(drops.get(), 1);
    assert_eq!(refusing.growth_requests(), 0);
    assert_eq!(map.remove("中"), Some(9));
    assert_eq!(drops.get(), 2);
    let mut set = OrderedSet::new();
    set.try_insert(key("é", 3, &drops), &mut normal).unwrap();
    let cap = set.capacity();
    assert!(!set.try_insert(key("é", 4, &drops), &mut refusing).unwrap());
    assert_eq!(set.get("é").unwrap().identity, 3);
    assert_eq!(set.capacity(), cap);
    assert_eq!(refusing.growth_requests(), 0);
    drop(set);
    assert_eq!(drops.get(), 4);
    let mut words = OrderedMap::new();
    words.try_insert(String::from("borrowed"), 42, &mut normal).unwrap();
    assert_eq!(words.get("borrowed"), Some(&42));
    let strings = OrderedSet::try_from_iter([Ok(String::from("borrowed"))], &mut normal).unwrap();
    assert!(strings.contains("borrowed"));
}

#[test]
fn spare_capacity_removal_empty_copy_and_zst_do_not_request_growth() {
    let mut normal = Context::new();
    let mut refusing = fault(1, true);
    let mut map = OrderedMap::new();
    map.try_reserve(12, &mut normal).unwrap();
    for key in 0..12 { map.try_insert(key, key, &mut refusing).unwrap(); }
    assert_eq!(map.remove(&5), Some(5));
    map.try_insert(20, 20, &mut refusing).unwrap();
    map.try_reserve(0, &mut refusing).unwrap();
    let mut set = OrderedSet::new();
    set.try_reserve(12, &mut normal).unwrap();
    for key in 0..12 { set.try_insert(key, &mut refusing).unwrap(); }
    assert!(set.remove(&5));
    set.try_insert(20, &mut refusing).unwrap();
    set.try_reserve(0, &mut refusing).unwrap();
    let mut units = OrderedMap::new();
    units.try_insert((), (), &mut refusing).unwrap();
    units.try_insert((), (), &mut refusing).unwrap();
    let mut unit_set = OrderedSet::new();
    unit_set.try_insert((), &mut refusing).unwrap();
    unit_set.try_insert((), &mut refusing).unwrap();
    let empty = OrderedMap::<i32, i32>::new();
    assert!(empty.try_clone_with(&mut refusing, |_, _| panic!("empty key"), |_, _| panic!("empty value")).unwrap().is_empty());
    assert!(OrderedSet::<i32>::new().try_clone_with(&mut refusing, |_, _| panic!("empty set")).unwrap().is_empty());
    assert_eq!(refusing.growth_requests(), 0);
    let failure = OrderedMap::new().try_insert(1, 2, &mut refusing).unwrap_err();
    assert_eq!(failure.site, Site::OrderedMap);
    assert_eq!(refusing.growth_requests(), 1);
}

struct Tracked<'a>(&'a Cell<usize>);
impl Drop for Tracked<'_> { fn drop(&mut self) { self.0.set(self.0.get() + 1); } }

#[test]
fn failed_new_slot_preserves_contents_capacity_and_owned_drops() {
    let old_keys = Cell::new(0);
    let new_keys = Cell::new(0);
    let old_values = Cell::new(0);
    let new_values = Cell::new(0);
    let mut normal = Context::new();
    let mut map = OrderedMap::new();
    map.try_insert(key("old", 1, &old_keys), vec![Tracked(&old_values)], &mut normal).unwrap();
    // Fill actual capacity; do not assume the allocator returns the requested size.
    let fillers = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
    assert!(map.capacity() <= fillers.len());
    for &name in fillers.iter().take(map.capacity() - 1) {
        map.try_insert(key(name, 1, &old_keys), vec![Tracked(&old_values)], &mut normal).unwrap();
    }
    let cap = map.capacity();
    let ptr = map.get_key_value("old").unwrap().0 as *const Key<'_>;
    let mut cx = fault(1, true);
    let first = map.try_insert(key("new", 2, &new_keys), vec![Tracked(&new_values), Tracked(&new_values)], &mut cx).err().unwrap();
    assert_eq!(first.kind, Kind::Injected);
    assert_eq!(first.site, Site::OrderedMap);
    assert_eq!(first.requested_bytes, Some((cap + 1) * std::mem::size_of::<(Key<'_>, Vec<Tracked<'_>>)>())); 
    assert_eq!((map.len(), map.capacity()), (cap, cap));
    assert_eq!(map.get_key_value("old").unwrap().0 as *const Key<'_>, ptr);
    assert!(map.get("new").is_none());
    assert_eq!((old_keys.get(), old_values.get(), new_keys.get(), new_values.get()), (0, 0, 1, 2));
    let mut set = OrderedSet::new();
    set.try_insert(key("old", 3, &old_keys), &mut normal).unwrap();
    for &name in fillers.iter().take(set.capacity() - 1) { set.try_insert(key(name, 3, &old_keys), &mut normal).unwrap(); }
    let set_cap = set.capacity();
    let second = set.try_insert(key("new", 4, &new_keys), &mut cx).unwrap_err();
    assert_eq!(second.site, Site::OrderedSet);
    assert_eq!((set.len(), set.capacity()), (set_cap, set_cap));
    assert_eq!(set.get("old").unwrap().identity, 3);
    assert_eq!(new_keys.get(), 2);
    assert_eq!(cx.first_failure(), Some(first));
    assert_eq!(cx.growth_requests(), 2);
    drop(map);
    drop(set);
    assert_eq!(old_keys.get(), cap + set_cap);
    assert_eq!(old_values.get(), cap);
}

#[test]
fn bulk_build_sweeps_every_actual_growth_and_sticky_failure() {
    let input = [(3, 30), (1, 10), (2, 20), (1, 11), (0, 0)];
    let mut normal = Context::new();
    let map = OrderedMap::try_from_iter(input.map(Ok), &mut normal).unwrap();
    assert_eq!(pairs(&map), vec![(0, 0), (1, 11), (2, 20), (3, 30)]);
    let requests = normal.growth_requests();
    assert!(requests > 0);
    for sticky in [false, true] {
        for nth in 1..=requests {
            let mut cx = fault(nth, sticky);
            let first = OrderedMap::try_from_iter(input.map(Ok), &mut cx).err().unwrap();
            assert_eq!(first.site, Site::OrderedMap);
            assert_eq!(first.kind, Kind::Injected);
            assert_eq!(cx.growth_requests(), nth);
            assert_eq!(cx.first_failure(), Some(first));
            let later = try_copy_str("later", &mut cx, Site::String);
            assert_eq!(later.is_err(), sticky);
            assert_eq!(cx.first_failure(), Some(first));
        }
    }
    OrderedMap::try_from_iter(input.map(Ok), &mut fault(requests + 1, false)).unwrap();
    let mut normal = Context::new();
    let set = OrderedSet::try_from_iter(input.map(|(k, _)| Ok(k)), &mut normal).unwrap();
    assert_eq!(set.iter().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    for nth in 1..=normal.growth_requests() {
        let mut cx = fault(nth, true);
        let first = OrderedSet::try_from_iter(input.map(|(k, _)| Ok(k)), &mut cx).err().unwrap();
        assert_eq!(first.site, Site::OrderedSet);
        assert_eq!(cx.growth_requests(), nth);
        assert!(try_copy_str("later", &mut cx, Site::String).is_err());
        assert_eq!(cx.first_failure(), Some(first));
    }
}

#[test]
fn deep_copy_sweeps_nested_allocations_without_changing_source() {
    let mut normal = Context::new();
    let source = OrderedMap::try_from_iter([
        Ok((String::from("a"), String::from("中"))),
        Ok((String::from("b"), String::from("🚀"))),
    ], &mut normal).unwrap();
    let snapshot: Vec<_> = source.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let cap = source.capacity();
    let mut success = Context::new();
    let copy = source.try_clone_with(&mut success,
        |k, cx| try_copy_str(k, cx, Site::String),
        |v, cx| try_copy_str(v, cx, Site::String)).unwrap();
    assert_eq!(copy.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>(), snapshot);
    assert_eq!(success.growth_requests(), 5); // destination + two key/value pairs
    for nth in 1..=success.growth_requests() {
        let mut cx = fault(nth, true);
        let first = source.try_clone_with(&mut cx,
            |k, cx| try_copy_str(k, cx, Site::String),
            |v, cx| try_copy_str(v, cx, Site::String)).err().unwrap();
        assert_eq!(cx.growth_requests(), nth);
        assert_eq!(cx.first_failure(), Some(first));
        assert_eq!(source.capacity(), cap);
        assert_eq!(source.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>(), snapshot);
        assert!(try_copy_str("later", &mut cx, Site::String).is_err());
        assert_eq!(cx.first_failure(), Some(first));
    }
    let set = OrderedSet::try_from_iter([Ok(String::from("a")), Ok(String::from("b"))], &mut normal).unwrap();
    let mut success = Context::new();
    let copy = set.try_clone_with(&mut success, |k, cx| try_copy_str(k, cx, Site::String)).unwrap();
    assert_eq!(copy.iter().map(String::as_str).collect::<Vec<_>>(), vec!["a", "b"]);
    for nth in 1..=success.growth_requests() {
        let mut cx = fault(nth, true);
        let first = set.try_clone_with(&mut cx, |k, cx| try_copy_str(k, cx, Site::String)).err().unwrap();
        assert_eq!(cx.first_failure(), Some(first));
        assert_eq!(cx.growth_requests(), nth);
        assert_eq!(set.iter().map(String::as_str).collect::<Vec<_>>(), vec!["a", "b"]);
    }
}

#[test]
fn failed_copy_drops_initialized_prefix_and_in_progress_key_once() {
    struct Value<'a> { text: String, _owner: Tracked<'a> }
    let source_keys = Cell::new(0);
    let copied_keys = Cell::new(0);
    let source_values = Cell::new(0);
    let copied_values = Cell::new(0);
    let mut normal = Context::new();
    let source = OrderedMap::try_from_iter(["a", "b"].map(|text|
        Ok((key(text, 1, &source_keys), Value { text: String::from(text), _owner: Tracked(&source_values) }))), &mut normal).unwrap();
    for nth in 1..=3 {
        copied_keys.set(0);
        copied_values.set(0);
        let mut cx = fault(nth, true);
        assert!(source.try_clone_with(&mut cx,
            |k, _| Ok(key(k.text, 2, &copied_keys)),
            |v, cx| Ok(Value { text: try_copy_str(&v.text, cx, Site::String)?, _owner: Tracked(&copied_values) })).is_err());
        assert_eq!(copied_keys.get(), nth - 1);
        assert_eq!(copied_values.get(), nth.saturating_sub(2));
        assert_eq!((source_keys.get(), source_values.get()), (0, 0));
        assert_eq!(source.len(), 2);
    }
    let set = OrderedSet::try_from_iter([Ok(key("a", 1, &source_keys)), Ok(key("b", 1, &source_keys))], &mut normal).unwrap();
    copied_keys.set(0);
    let mut cx = fault(3, true);
    assert!(set.try_clone_with(&mut cx, |k, cx| {
        let _temporary = try_copy_str(k.text, cx, Site::String)?;
        Ok(key(k.text, 2, &copied_keys))
    }).is_err());
    assert_eq!(copied_keys.get(), 1);
    assert_eq!(source_keys.get(), 0);
    drop(source);
    drop(set);
    assert_eq!((source_keys.get(), source_values.get()), (4, 2));
}

#[test]
fn bulk_producer_error_stops_and_drops_prefix_and_remaining_inputs() {
    let drops = Cell::new(0);
    let mut cx = fault(1, true);
    let first = try_copy_str("failure", &mut cx, Site::String).unwrap_err();
    let mut normal = Context::new();
    let mut visited = 0;
    let input = [Ok((1, vec![Tracked(&drops)])), Err(first), Ok((2, vec![Tracked(&drops)]))];
    let result = OrderedMap::try_from_iter(input.into_iter().inspect(|_| visited += 1), &mut normal);
    assert_eq!(result.err(), Some(first));
    assert_eq!(visited, 2);
    assert_eq!(drops.get(), 2);
    let set_drops = Cell::new(0);
    let input = [Ok(key("a", 1, &set_drops)), Err(first), Ok(key("b", 1, &set_drops))];
    assert_eq!(OrderedSet::try_from_iter(input, &mut normal).err(), Some(first));
    assert_eq!(set_drops.get(), 2);
}

#[test]
fn reserve_overflow_and_transformed_copy_keep_ordering_invariants() {
    let mut normal = Context::new();
    let mut map = OrderedMap::try_from_iter([Ok((1_i32, 10_i32)), Ok((2, 20))], &mut normal).unwrap();
    let cap = map.capacity();
    let mut cx = fault(1, true);
    let first = map.try_reserve(usize::MAX, &mut cx).unwrap_err();
    assert_eq!(first.kind, Kind::CapacityOverflow);
    assert_eq!(first.site, Site::OrderedMap);
    assert_eq!(cx.growth_requests(), 0);
    assert_eq!(map.capacity(), cap);
    assert_eq!(pairs(&map), vec![(1, 10), (2, 20)]);
    let mut set = OrderedSet::try_from_iter([Ok(1_i32)], &mut normal).unwrap();
    let cap = set.capacity();
    assert_eq!(set.try_reserve(usize::MAX, &mut cx).unwrap_err().site, Site::OrderedSet);
    assert_eq!(set.capacity(), cap);
    assert_eq!(cx.first_failure(), Some(first));
    let reversed = map.try_clone_with(&mut normal, |k, _| Ok(-k), |v, _| Ok(*v)).unwrap();
    assert_eq!(pairs(&reversed), vec![(-2, 20), (-1, 10)]);
    let collapsed = map.try_clone_with(&mut normal, |_, _| Ok(0), |v, _| Ok(*v)).unwrap();
    assert_eq!(pairs(&collapsed), vec![(0, 20)]);
}
