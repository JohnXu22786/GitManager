// Standalone root: never aggregate with a root that defines another allocator.
#[path = "../src/product_alloc.rs"] mod product_alloc;
// Preserve and rerun the directly relevant original regressions unchanged.
#[cfg(test)]
#[path = "product_allocation_test.rs"] mod foundation;
#[cfg(test)]
#[path = "product_box_allocation_test.rs"] mod boxes;
#[cfg(shared_managed)]
#[path = "../src/product_json.rs"] mod product_json;
#[cfg(shared_managed)]
#[path = "../src/product_app.rs"] mod product_app;
#[cfg(shared_managed)]
#[path = "../src/product_contract.rs"] mod product_contract;
use product_alloc::{AllocationContext as Cx, AllocationFailureKind as Kind,
    AllocationSite as Site, Shared, try_box, try_copy_str};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, num::NonZeroUsize,
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// No TLS, callbacks, formatting, locks, or allocations in these hooks. Tests
// serialize capture windows with LOCK and the admitted --test-threads=1 runner.
// Mode 3 follows only selected addresses while scoped worker threads run; their
// runtime allocations are not claimed as product allocations or cleanup work.
const RECORDS: usize = 8192;
static PTR: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static SIZE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static ALIGN: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static LIVE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static MODE: AtomicUsize = AtomicUsize::new(0);
static USED: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BAD: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
struct Observer;
fn record(p: *mut u8, l: Layout) {
    let mode = MODE.load(Ordering::Relaxed);
    if mode == 0 || mode == 3 { return; }
    CALLS.fetch_add(1, Ordering::Relaxed);
    if mode == 2 || p.is_null() { BAD.fetch_add(1, Ordering::Relaxed); return; }
    let i = USED.fetch_add(1, Ordering::Relaxed);
    if i >= RECORDS { BAD.fetch_add(1, Ordering::Relaxed); return; }
    PTR[i].store(p as usize, Ordering::Relaxed);
    SIZE[i].store(l.size(), Ordering::Relaxed);
    ALIGN[i].store(l.align(), Ordering::Relaxed);
    LIVE[i].store(1, Ordering::Relaxed);
}
fn release(p: *mut u8, l: Layout) {
    let mode = MODE.load(Ordering::Relaxed);
    if mode == 0 { return; }
    let n = USED.load(Ordering::Relaxed).min(RECORDS);
    for i in (0..n).rev() {
        if PTR[i].load(Ordering::Relaxed) == p as usize {
            if LIVE[i].swap(0, Ordering::Relaxed) != 1
                || SIZE[i].load(Ordering::Relaxed) != l.size()
                || ALIGN[i].load(Ordering::Relaxed) != l.align() {
                BAD.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
    }
    if mode != 3 { BAD.fetch_add(1, Ordering::Relaxed); }
}
unsafe impl GlobalAlloc for Observer {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) }; record(p, l); p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) }; record(p, l); p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        release(p, l); unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() { release(p, l); }
        // GlobalAlloc's caller supplies a valid new allocation extent.
        record(q, unsafe { Layout::from_size_align_unchecked(n, l.align()) }); q
    }
}
#[global_allocator] static ALLOCATOR: Observer = Observer;
struct Watch;
impl Watch {
    fn begin() -> Self {
        MODE.store(0, Ordering::Relaxed); USED.store(0, Ordering::Relaxed);
        CALLS.store(0, Ordering::Relaxed); BAD.store(0, Ordering::Relaxed);
        MODE.store(1, Ordering::Relaxed); Self
    }
    fn no_growth(&self) { MODE.store(2, Ordering::Relaxed); }
    fn selected_only(&self) { MODE.store(3, Ordering::Relaxed); }
    fn empty(&self) -> usize {
        MODE.store(0, Ordering::Relaxed);
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer overflow/null/layout/release/growth");
        let n = USED.load(Ordering::Relaxed); assert!(n <= RECORDS);
        for i in 0..n { assert_eq!(LIVE[i].load(Ordering::Relaxed), 0); }
        n
    }
}
impl Drop for Watch { fn drop(&mut self) { MODE.store(0, Ordering::Relaxed); } }
fn fault(n: usize) -> Cx { Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true) }
struct Tracked<'a>(&'a Cell<usize>);
impl Drop for Tracked<'_> { fn drop(&mut self) { self.0.set(self.0.get() + 1); } }

#[test]
fn refused_single_and_pair_publish_nothing_drop_once_and_preserve_old_owner() {
    let _lock = LOCK.lock().unwrap();
    let old = Shared::try_new(17_u64, &mut Cx::new()).unwrap();
    for pair in [false, true] {
        let drops = Cell::new(0); let mut cx = fault(1);
        let watch = Watch::begin();
        let error = if pair { Shared::try_pair(Tracked(&drops), &mut cx).err().unwrap() }
            else { Shared::try_new(Tracked(&drops), &mut cx).err().unwrap() };
        watch.no_growth();
        assert!(try_copy_str("later", &mut cx, Site::String).is_err());
        assert!(Shared::try_new(Tracked(&drops), &mut cx).is_err());
        assert_eq!(watch.empty(), 0);
        assert_eq!(error.kind, Kind::Injected); assert_eq!(error.site, Site::Box);
        assert!(error.requested_bytes.unwrap() > 0);
        assert_eq!(drops.get(), 2); assert_eq!(cx.growth_requests(), 3);
        assert_eq!(cx.first_failure(), Some(error)); assert_eq!(*old, 17);
    }
}

#[test]
fn zst_still_has_fallible_real_header_and_one_payload_drop() {
    let _lock = LOCK.lock().unwrap();
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Zst;
    impl Drop for Zst { fn drop(&mut self) { DROPS.fetch_add(1, Ordering::Relaxed); } }
    let before = DROPS.load(Ordering::Relaxed);
    let watch = Watch::begin(); let mut cx = fault(1);
    assert!(Shared::try_new(Zst, &mut cx).is_err());
    assert_eq!(cx.growth_requests(), 1); assert_eq!(watch.empty(), 0);
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 1);
    let watch = Watch::begin(); let mut cx = Cx::new();
    let owner = Shared::try_new(Zst, &mut cx).unwrap();
    assert_eq!(USED.load(Ordering::Relaxed), 1);
    assert!(SIZE[0].load(Ordering::Relaxed) >= std::mem::size_of::<usize>());
    watch.no_growth(); drop(owner); assert_eq!(watch.empty(), 1);
    assert_eq!(DROPS.load(Ordering::Relaxed), before + 2);
    assert_eq!(cx.growth_requests(), 1);
}

#[test]
fn aligned_single_and_both_pair_orders_release_matching_header_once() {
    let _lock = LOCK.lock().unwrap();
    #[repr(align(512))] struct Aligned<'a> { value: u64, _drop: Tracked<'a> }
    for order in 0..3 {
        let drops = Cell::new(0); let mut cx = Cx::new(); let watch = Watch::begin();
        let value = Aligned { value: 73, _drop: Tracked(&drops) };
        if order == 0 {
            let a = Shared::try_new(value, &mut cx).unwrap();
            assert_eq!((&*a as *const Aligned<'_> as usize) % 512, 0);
            assert_eq!(a.value, 73); watch.no_growth(); drop(a);
        } else {
            let (a, b) = Shared::try_pair(value, &mut cx).unwrap();
            assert!(Shared::ptr_eq(&a, &b));
            assert!(std::ptr::eq(&*a, &*b)); watch.no_growth();
            let survivor = if order == 1 { drop(a); b } else { drop(b); a };
            assert_eq!(drops.get(), 0); assert_eq!(LIVE[0].load(Ordering::Relaxed), 1);
            assert_eq!(survivor.value, 73); drop(survivor);
        }
        assert_eq!(watch.empty(), 1); assert_eq!(drops.get(), 1);
        assert_eq!(cx.growth_requests(), 1);
    }
}

#[test]
fn equal_values_are_distinct_allocations_and_contexts_are_independent() {
    let _lock = LOCK.lock().unwrap(); let mut cx = fault(2);
    let a = Shared::try_new(5_u64, &mut cx).unwrap();
    assert!(Shared::try_pair(5_u64, &mut cx).is_err());
    let b = Shared::try_new(5_u64, &mut Cx::new()).unwrap();
    assert!(!Shared::ptr_eq(&a, &b)); assert_eq!(*a, *b);
    assert_eq!(cx.growth_requests(), 2);
}

#[test]
fn pair_two_thread_handoff_keeps_payload_until_unique_final_release() {
    let _lock = LOCK.lock().unwrap();
    struct Payload<'a> { drops: &'a AtomicUsize, seen: AtomicUsize }
    impl Drop for Payload<'_> { fn drop(&mut self) {
        assert_eq!(self.seen.load(Ordering::Relaxed), 3);
        self.drops.fetch_add(1, Ordering::Relaxed);
    } }
    let drops = AtomicUsize::new(0); let watch = Watch::begin();
    let (a, b) = Shared::try_pair(Payload { drops: &drops, seen: AtomicUsize::new(0) },
        &mut Cx::new()).unwrap();
    watch.selected_only();
    std::thread::scope(|s| {
        let x = s.spawn(move || { a.seen.fetch_or(1, Ordering::Relaxed); drop(a); });
        let y = s.spawn(move || { b.seen.fetch_or(2, Ordering::Relaxed); drop(b); });
        x.join().unwrap(); y.join().unwrap();
    });
    assert_eq!(watch.empty(), 1); assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[cfg(shared_managed)]
#[test]
fn managed_deep_graph_header_failure_and_success_keep_teardown_workspace() {
    let _lock = LOCK.lock().unwrap();
    use product_app::Type;
    // Raw fixture is independently owned; dismantle it iteratively below.
    let mut source = Type::Boolean;
    for _ in 0..2048 { source = Type::List { item: Box::new(source) }; }
    let mut probe = Cx::new(); let graph = source.try_copy(&mut probe).unwrap();
    let requests = probe.growth_requests(); assert!(requests <= 4096); drop(graph);
    // First, middle, final child allocation, and header refusal. This is a
    // fixed four-case deep cleanup sample, not every allocation ordinal.
    for ordinal in [1, requests / 2, requests, requests + 1] {
        let mut cx = fault(ordinal); let watch = Watch::begin();
        let result = source.try_copy(&mut cx).and_then(|g| Shared::try_pair(g, &mut cx));
        assert!(result.is_err()); watch.no_growth();
        assert!(try_box(1_u8, &mut cx).is_err()); drop(result);
        // Each successful pre-refusal nonzero growth makes one allocation.
        // Any balanced allocation during failure cleanup adds a record too.
        assert_eq!(watch.empty(), ordinal - 1);
        assert_eq!(cx.growth_requests(), ordinal + 1);
    }
    let mut cx = fault(requests + 2); let watch = Watch::begin();
    let graph = source.try_copy(&mut cx).unwrap();
    let (a, b) = Shared::try_pair(graph, &mut cx).unwrap();
    assert!(Shared::ptr_eq(&a, &b));
    assert!(try_box(1_u8, &mut cx).is_err()); watch.no_growth();
    drop(a);
    let mut node = b.as_ref(); let mut depth = 0;
    while let Type::List { item } = node { depth += 1; node = item; }
    assert_eq!(depth, 2048); assert!(matches!(node, Type::Boolean));
    drop(b); watch.empty(); assert_eq!(cx.growth_requests(), requests + 2);
    loop { match source { Type::List { item } => source = *item, Type::Boolean => break,
        _ => unreachable!() } }
}

// Metadata-only compile boundaries use this same root without --test. There is
// deliberately no test-side facade that grants production raw/retain access.
#[cfg(shared_positive)]
fn positive_bounds() {
    fn both<T: Send + Sync>() {} both::<Shared<AtomicUsize>>();
    both::<Shared<dyn Send + Sync>>();
    let (a, b) = Shared::try_pair(3_u64, &mut Cx::new()).unwrap();
    let _: &u64 = &a; let _: bool = Shared::ptr_eq(&a, &b);
}
#[cfg(shared_negative_clone)]
fn no_clone() { fn need<T: Clone>() {} need::<Shared<u64>>(); }
#[cfg(shared_negative_cell_sync)]
fn no_cell_sync() { fn need<T: Sync>() {} need::<Shared<Cell<u64>>>(); }
#[cfg(shared_negative_cell_send)]
fn no_cell_send() { fn need<T: Send>() {} need::<Shared<Cell<u64>>>(); }
#[cfg(shared_negative_rc_send)]
fn no_rc_send() { fn need<T: Send>() {} need::<Shared<std::rc::Rc<u64>>>(); }
#[cfg(shared_negative_raw)]
fn no_raw() { let a = Shared::try_new(1, &mut Cx::new()).unwrap(); let _ = a.ptr; }
#[cfg(shared_negative_retain)]
fn no_retain() { let a = Shared::try_new(1, &mut Cx::new()).unwrap(); let _ = unsafe { a.retain_bounded() }; }
#[cfg(shared_negative_mut)]
fn no_mut() { let mut a = Shared::try_new(1, &mut Cx::new()).unwrap(); *a = 2; }
#[cfg(shared_negative_extract)]
fn no_extract() { let a = Shared::try_new(String::new(), &mut Cx::new()).unwrap(); drop(*a); }
