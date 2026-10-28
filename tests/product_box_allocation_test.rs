#[path = "../src/product_alloc.rs"]
mod product_alloc;

use product_alloc::{try_box, try_copy_str, try_push, AllocationContext as Context,
    AllocationFailure, AllocationFailureKind as Kind, AllocationSite};
use product_alloc::ordered::OrderedMap;
use std::cell::Cell;
use std::fmt::{self, Write};
use std::mem::{align_of, size_of, size_of_val};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fault(n: usize, sticky: bool) -> Context {
    Context::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}

#[derive(Debug)]
struct Tracked<'a>(&'a Cell<usize>);
impl Drop for Tracked<'_> {
    fn drop(&mut self) { self.0.set(self.0.get() + 1); }
}

#[test]
fn over_aligned_value_is_initialized_mutable_and_dropped_once() {
    #[repr(align(256))]
    struct Aligned<'a> { value: u64, _owner: Tracked<'a> }
    let drops = Cell::new(0);
    let mut cx = Context::new();
    let mut owner = try_box(Aligned { value: 42, _owner: Tracked(&drops) }, &mut cx).unwrap();
    assert_eq!((&*owner as *const Aligned<'_> as usize) % align_of::<Aligned<'_>>(), 0);
    assert_eq!(owner.value, 42);
    owner.value = 91;
    assert_eq!(owner.value, 91);
    assert_eq!(drops.get(), 0);
    assert_eq!(cx.growth_requests(), 1);
    assert_eq!(cx.first_failure(), None);
    drop(owner);
    assert_eq!(drops.get(), 1);
}

#[test]
fn refused_box_drops_incoming_nested_owners_once() {
    let drops = Cell::new(0);
    // Fixture allocations precede the context-scoped request under test.
    let incoming = (vec![Tracked(&drops), Tracked(&drops)], String::from("kept"));
    let bytes = size_of_val(&incoming);
    let mut cx = fault(1, true);
    let error = try_box(incoming, &mut cx).unwrap_err();
    assert_eq!(error.kind, Kind::Injected);
    assert_eq!(error.site, AllocationSite::Box);
    assert_eq!(error.requested_bytes, Some(bytes));
    assert_eq!(drops.get(), 2);
    assert_eq!(cx.growth_requests(), 1);
    assert_eq!(cx.first_failure(), Some(error));
}

fn nested<'a>(drops: &'a Cell<usize>, cx: &mut Context)
    -> Result<Box<Box<Box<Tracked<'a>>>>, AllocationFailure> {
    let leaf = try_box(Tracked(drops), cx)?;
    let middle = try_box(leaf, cx)?;
    try_box(middle, cx)
}

#[test]
fn every_nested_box_request_can_fail_without_leaking_or_double_drop() {
    let drops = Cell::new(0);
    let mut normal = Context::new();
    let owner = nested(&drops, &mut normal).unwrap();
    assert_eq!(normal.growth_requests(), 3);
    assert_eq!(drops.get(), 0);
    drop(owner);
    assert_eq!(drops.get(), 1);
    for ordinal in 1..=3 {
        let drops = Cell::new(0);
        let mut cx = fault(ordinal, true);
        let error = nested(&drops, &mut cx).unwrap_err();
        assert_eq!(error.kind, Kind::Injected);
        assert_eq!(error.site, AllocationSite::Box);
        assert_eq!(cx.growth_requests(), ordinal);
        assert_eq!(cx.first_failure(), Some(error));
        assert_eq!(drops.get(), 1);
    }
    let drops = Cell::new(0);
    let mut after = fault(4, false);
    drop(nested(&drops, &mut after).unwrap());
    assert_eq!(drops.get(), 1);
    assert_eq!(after.growth_requests(), 3);
    assert_eq!(after.first_failure(), None);
}

#[test]
fn sticky_failure_refuses_later_requests_but_preserves_previous_owner() {
    let old_drops = Cell::new(0);
    let new_drops = Cell::new(0);
    let mut cx = fault(2, true);
    let owner = try_box((73_u64, Tracked(&old_drops)), &mut cx).unwrap();
    let first = try_box(Tracked(&new_drops), &mut cx).unwrap_err();
    for _ in 0..3 {
        assert_eq!(try_box(Tracked(&new_drops), &mut cx).unwrap_err(), first);
    }
    assert!(try_copy_str("later", &mut cx, AllocationSite::String).is_err());
    assert_eq!(owner.0, 73);
    assert_eq!(old_drops.get(), 0);
    assert_eq!(new_drops.get(), 4);
    assert_eq!(cx.growth_requests(), 6);
    assert_eq!(cx.first_failure(), Some(first));
    drop(owner);
    assert_eq!(old_drops.get(), 1);
    assert_eq!(new_drops.get(), 4);
}

#[test]
fn over_aligned_zst_owns_one_drop_without_request_or_injection() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    #[repr(align(512))]
    #[derive(Debug)]
    struct Zst;
    impl Drop for Zst {
        fn drop(&mut self) { DROPS.fetch_add(1, Ordering::Relaxed); }
    }
    assert_eq!(size_of::<Zst>(), 0);
    let before = DROPS.load(Ordering::Relaxed);
    let mut cx = fault(1, true);
    let owner = try_box(Zst, &mut cx).unwrap();
    let address = &*owner as *const Zst as usize;
    assert_ne!(address, 0);
    assert_eq!(address % align_of::<Zst>(), 0);
    assert_eq!(cx.growth_requests(), 0);
    assert_eq!(cx.first_failure(), None);
    assert_eq!(DROPS.load(Ordering::Relaxed), before);
    drop(owner);
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    let first = try_box(7_u64, &mut cx).unwrap_err();
    drop(try_box(Zst, &mut cx).unwrap());
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 2);
    assert_eq!(cx.growth_requests(), 1);
    assert_eq!(cx.first_failure(), Some(first));
}

#[test]
fn box_vector_and_string_share_real_request_ordinals() {
    let mut cx = fault(3, false);
    drop(try_box(1_u32, &mut cx).unwrap());
    let mut values = Vec::new();
    try_push(&mut values, 2_u64, &mut cx, AllocationSite::Vector).unwrap();
    drop(try_box((), &mut cx).unwrap());
    let error = try_copy_str("third", &mut cx, AllocationSite::String).unwrap_err();
    assert_eq!(error.site, AllocationSite::String);
    assert_eq!(cx.growth_requests(), 3);
    assert_eq!(values, [2]);
    assert_eq!(*try_box(4_u32, &mut cx).unwrap(), 4);
    assert_eq!(cx.growth_requests(), 4);
    assert_eq!(cx.first_failure(), Some(error));
}

#[test]
fn failure_and_success_contexts_are_independent() {
    let drops = Cell::new(0);
    let mut refusing = fault(1, false);
    let first = try_box(Tracked(&drops), &mut refusing).unwrap_err();
    let mut other = Context::new();
    let owner = try_box(Tracked(&drops), &mut other).unwrap();
    let resumed = try_box(Tracked(&drops), &mut refusing).unwrap();
    assert_eq!(drops.get(), 1);
    assert_eq!(refusing.growth_requests(), 2);
    assert_eq!(refusing.first_failure(), Some(first));
    assert_eq!(other.growth_requests(), 1);
    assert_eq!(other.first_failure(), None);
    drop((owner, resumed));
    assert_eq!(drops.get(), 3);
}

#[test]
fn inline_box_failure_formats_to_fixed_storage_after_refusal() {
    struct Fixed { bytes: [u8; 128], len: usize }
    impl Write for Fixed {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
            self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?
                .copy_from_slice(value.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut cx = fault(1, true);
    let first = try_box(7_u64, &mut cx).unwrap_err();
    assert!(try_box(8_u64, &mut cx).is_err());
    let mut fixed = Fixed { bytes: [0; 128], len: 0 };
    write!(&mut fixed, "{}", first).unwrap();
    assert_eq!(&fixed.bytes[..fixed.len], b"allocation failure (injected) at box: 8 bytes");
    assert_eq!(cx.first_failure(), Some(first));
    assert_eq!(cx.growth_requests(), 2);
}

fn boxed_map<'a>(drops: &'a Cell<usize>, cx: &mut Context)
    -> Result<OrderedMap<u8, Box<Tracked<'a>>>, AllocationFailure> {
    let mut map = OrderedMap::new();
    map.try_reserve(2, cx)?;
    let first = try_box(Tracked(drops), cx)?;
    map.try_insert(1, first, cx)?;
    let second = try_box(Tracked(drops), cx)?;
    map.try_insert(2, second, cx)?;
    Ok(map)
}

#[test]
fn mixed_map_box_requests_share_context_and_drop_partial_owners() {
    let drops = Cell::new(0);
    let mut normal = Context::new();
    let map = boxed_map(&drops, &mut normal).unwrap();
    assert_eq!(normal.growth_requests(), 3);
    assert_eq!(map.len(), 2);
    assert_eq!(drops.get(), 0);
    drop(map);
    assert_eq!(drops.get(), 2);
    for (index, site) in [AllocationSite::OrderedMap, AllocationSite::Box,
        AllocationSite::Box].into_iter().enumerate() {
        let drops = Cell::new(0);
        let mut cx = fault(index + 1, true);
        let error = boxed_map(&drops, &mut cx).err().unwrap();
        assert_eq!(error.kind, Kind::Injected);
        assert_eq!(error.site, site);
        assert_eq!(cx.first_failure(), Some(error));
        assert_eq!(cx.growth_requests(), index + 1);
        assert_eq!(drops.get(), index);
        assert!(try_box(Tracked(&drops), &mut cx).is_err());
        assert_eq!(drops.get(), index + 1);
        assert_eq!(cx.growth_requests(), index + 2);
        assert_eq!(cx.first_failure(), Some(error));
    }
}

#[test]
fn ordered_slot_failure_drops_box_and_sticky_replacement_keeps_one_owner() {
    let old_drops = Cell::new(0);
    let replacement_drops = Cell::new(0);
    let refused_drops = Cell::new(0);
    let mut setup = Context::new();
    let mut existing = OrderedMap::new();
    let old = try_box(Tracked(&old_drops), &mut setup).unwrap();
    existing.try_insert(7_u8, old, &mut setup).unwrap();
    let replacement = try_box(Tracked(&replacement_drops), &mut setup).unwrap();
    let capacity = existing.capacity();

    // These calls share one real module/context, not independent test copies.
    let mut cx = fault(2, true);
    let mut empty = OrderedMap::new();
    let incoming = try_box(Tracked(&refused_drops), &mut cx).unwrap();
    let first = empty.try_insert(1_u8, incoming, &mut cx).unwrap_err();
    assert_eq!(first.kind, Kind::Injected);
    assert_eq!(first.site, AllocationSite::OrderedMap);
    assert_eq!(cx.growth_requests(), 2);
    assert!(empty.is_empty());
    assert_eq!(empty.capacity(), 0);
    assert_eq!(refused_drops.get(), 1);
    assert_eq!(old_drops.get(), 0);
    let later = try_box(Tracked(&refused_drops), &mut cx).unwrap_err();
    assert_eq!(later.site, AllocationSite::Box);
    assert_eq!(refused_drops.get(), 2);
    assert_eq!(cx.first_failure(), Some(first));

    let returned = existing.try_insert(7_u8, replacement, &mut cx).unwrap().unwrap();
    assert_eq!(cx.growth_requests(), 3); // Occupied replacement did not request.
    assert_eq!(existing.capacity(), capacity);
    assert_eq!(existing.len(), 1);
    assert_eq!(old_drops.get(), 0);
    assert_eq!(replacement_drops.get(), 0);
    drop(returned);
    assert_eq!(old_drops.get(), 1);
    drop(existing);
    assert_eq!(replacement_drops.get(), 1);
    assert_eq!(refused_drops.get(), 2);
    assert_eq!(cx.first_failure(), Some(first));
}
