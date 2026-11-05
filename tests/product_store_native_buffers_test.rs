#![cfg(any(target_os = "linux", target_os = "macos"))]
// Standalone path root: the actual allocation context appears exactly once.
#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_store/native_buffers.rs"]
mod native_buffers;

use native_buffers::*;
use product_alloc::{AllocationContext as Cx, AllocationFailure,
    AllocationFailureKind as Kind, AllocationSite as Site};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, ffi::OsStr,
    num::NonZeroUsize, os::unix::ffi::OsStrExt, path::{Component, Path, PathBuf},
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// Observation only, never a null-injecting allocator. Const TLS is primed
// before each scope. Exact target emitted/linked hooks require qualification.
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }
const RECORDS: usize = 8;
static PTR: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static SIZE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static ALIGN: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static LIVE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static USED: AtomicUsize = AtomicUsize::new(0);
static BAD: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
struct Observer;
fn observing() -> bool {
    match ACTIVE.try_with(Cell::get) {
        Ok(active) => active,
        Err(_) => { BAD.fetch_add(1, Ordering::Relaxed); false }
    }
}
fn register(p: *mut u8, size: usize, align: usize) {
    if p.is_null() { BAD.fetch_add(1, Ordering::Relaxed); return; }
    let i = USED.fetch_add(1, Ordering::Relaxed);
    if i >= RECORDS { BAD.fetch_add(1, Ordering::Relaxed); return; }
    PTR[i].store(p as usize, Ordering::Relaxed);
    SIZE[i].store(size, Ordering::Relaxed);
    ALIGN[i].store(align, Ordering::Relaxed);
    LIVE[i].store(1, Ordering::Relaxed);
}
fn release(p: *mut u8, size: usize, align: usize) {
    for i in (0..USED.load(Ordering::Relaxed).min(RECORDS)).rev() {
        if PTR[i].load(Ordering::Relaxed) == p as usize
            && LIVE[i].load(Ordering::Relaxed) == 1 {
            if SIZE[i].load(Ordering::Relaxed) != size
                || ALIGN[i].load(Ordering::Relaxed) != align {
                BAD.fetch_add(1, Ordering::Relaxed);
            }
            LIVE[i].store(0, Ordering::Relaxed);
            return;
        }
    }
    BAD.fetch_add(1, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Observer {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if observing() { register(p, l.size(), l.align()); }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if observing() { register(p, l.size(), l.align()); }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if observing() { release(p, l.size(), l.align()); }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if observing() {
            if !q.is_null() { release(p, l.size(), l.align()); }
            register(q, n, l.align());
        }
        q
    }
}
#[global_allocator]
static ALLOCATOR: Observer = Observer;
struct Watch;
impl Watch {
    fn begin() -> Self {
        ACTIVE.with(|a| a.set(false));
        USED.store(0, Ordering::Relaxed);
        BAD.store(0, Ordering::Relaxed);
        ACTIVE.with(|a| a.set(true));
        Self
    }
    fn pause(&self) { ACTIVE.with(|a| a.set(false)); }
    fn resume(&self) { ACTIVE.with(|a| a.set(true)); }
    fn clean(&self, expected_allocations: usize) {
        self.pause();
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer invariant");
        assert_eq!(USED.load(Ordering::Relaxed), expected_allocations);
    }
    fn empty(&self, expected_allocations: usize) {
        self.clean(expected_allocations);
        for i in 0..expected_allocations {
            assert_eq!(LIVE[i].load(Ordering::Relaxed), 0, "unreleased prefix");
        }
    }
}
impl Drop for Watch { fn drop(&mut self) { self.pause(); } }

#[derive(Clone, Copy)]
enum Case {
    Name(&'static [u8]),
    Copy(&'static [u8]),
    Join(&'static [u8], &'static [u8]),
    Prepared(u32, u64),
}
const LONG: &[u8] = &[b'x'; 8192];
// Exactly 32 fixtures; largest borrowed input is 8192 bytes. These are
// validation limits, not product limits. No filesystem operation is used.
const CASES: [Case; 32] = [
    Case::Name(b"foo"), Case::Name(b"foo/"), Case::Name(b"foo/."),
    Case::Name(b"foo//"), Case::Name(b"foo//."), Case::Name(b"./foo"),
    Case::Name(b"foo/.."), Case::Name(b"foo/bar"), Case::Name(b""),
    Case::Name(b"."), Case::Name(b".."), Case::Name(b"/"),
    Case::Name(b"\xff"), Case::Name(b"\xff/."), Case::Name("中é".as_bytes()),
    Case::Name(b"foo\0bar"),
    Case::Copy(b""), Case::Copy(b"/"), Case::Copy(b"a//"),
    Case::Copy(b"\xff/a\0"), Case::Copy(LONG),
    Case::Join(b"", b"foo/."), Case::Join(b"/", b"foo/"),
    Case::Join(b"//", b"foo//."), Case::Join(b"a", b"\xff"),
    Case::Join(b"a/", "中é".as_bytes()), Case::Join(b"a//", b"foo/."),
    Case::Join(b"\xff/", b"foo//"), Case::Join(b"a/./b", b"foo"),
    Case::Prepared(0, 0), Case::Prepared(1, 1), Case::Prepared(u32::MAX, u64::MAX),
];
enum Output { Name(UnixName), Path(PathBuf), Prepared(PreparedUnixNames) }
fn perform(case: Case, cx: &mut Cx) -> Result<Output, NativeBufferFailure> {
    match case {
        Case::Name(b) => try_unix_name(OsStr::from_bytes(b), cx).map(Output::Name),
        Case::Copy(b) => try_unix_path_copy(Path::new(OsStr::from_bytes(b)), cx)
            .map(Output::Path),
        Case::Join(p, n) => {
            let name = try_unix_name(OsStr::from_bytes(n), cx)?;
            try_unix_child_path(Path::new(OsStr::from_bytes(p)), &name, cx)
                .map(Output::Path)
        }
        Case::Prepared(pid, id) => try_prepared_unix_names(OsStr::new("CURRENT"),
            pid, id, cx).map(Output::Prepared),
    }
}
fn shape(bytes: &[u8]) -> bool {
    let path = Path::new(OsStr::from_bytes(bytes));
    path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)))
}
fn schedule(case: Case) -> Vec<usize> {
    // Oracle construction and decimal formatting are OUTSIDE observation.
    match case {
        Case::Name(b) if shape(b) => vec![b.len() + 1],
        Case::Name(_) => vec![],
        Case::Copy(b) if b.is_empty() => vec![],
        Case::Copy(b) => vec![b.len()],
        Case::Join(p, n) => vec![n.len() + 1,
            Path::new(OsStr::from_bytes(p)).join(OsStr::from_bytes(n))
                .as_os_str().as_bytes().len()],
        Case::Prepared(pid, id) => vec![8, format!(".pending-{pid}-{id}").len() + 1],
    }
}
fn expected(case: Case) -> Result<Vec<u8>, NativeBufferFailure> {
    match case {
        Case::Name(b) if !shape(b) || b.contains(&0) => Err(NativeBufferFailure::UnsafeName),
        Case::Name(b) | Case::Copy(b) => Ok(b.to_vec()),
        Case::Join(p, n) => Ok(Path::new(OsStr::from_bytes(p)).join(OsStr::from_bytes(n))
            .as_os_str().as_bytes().to_vec()),
        Case::Prepared(pid, id) => Ok(format!(".pending-{pid}-{id}").into_bytes()),
    }
}
fn assert_name(value: &UnixName, expected: &[u8]) {
    assert_eq!(value.as_os_str().as_bytes(), expected);
    assert_eq!(value.as_c_str().to_bytes(), expected);
    assert_eq!(value.as_c_str().to_bytes_with_nul().last(), Some(&0));
    assert_eq!(value.as_c_str().to_bytes_with_nul().len(), expected.len() + 1);
    assert_eq!(value.as_c_str().as_ptr().cast::<u8>(), value.as_os_str().as_bytes().as_ptr());
}
fn assert_result(result: &Result<Output, NativeBufferFailure>, expected: &Result<Vec<u8>, NativeBufferFailure>) {
    match (result, expected) {
        (Ok(Output::Name(value)), Ok(bytes)) => assert_name(value, bytes),
        (Ok(Output::Path(value)), Ok(bytes)) => assert_eq!(value.as_os_str().as_bytes(), bytes),
        (Ok(Output::Prepared(value)), Ok(bytes)) => {
            assert_name(value.target(), b"CURRENT");
            assert_name(value.temporary(), bytes);
        }
        (Err(actual), Err(expected)) => assert_eq!(actual, expected),
        _ => panic!("result parity"),
    }
}
fn fault(n: usize, sticky: bool) -> Cx {
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}

#[test]
fn raw_bytes_component_predicate_join_and_decimal_match_existing_operations() {
    let _lock = LOCK.lock().unwrap();
    for case in CASES {
        let want = expected(case);
        let sizes = schedule(case);
        let mut cx = Cx::new();
        let watch = Watch::begin();
        let result = perform(case, &mut cx);
        watch.clean(sizes.len());
        assert_result(&result, &want);
        assert_eq!(cx.growth_requests(), sizes.len());
        assert_eq!(cx.first_failure(), None);
        for (i, size) in sizes.iter().enumerate() {
            assert_eq!(SIZE[i].load(Ordering::Relaxed), *size);
        }
        watch.resume();
        drop(result);
        watch.empty(sizes.len());
    }
}

#[test]
fn every_reached_growth_refuses_with_exact_identity_and_releases_partial_owners() {
    let _lock = LOCK.lock().unwrap();
    let mut fault_cases = 0;
    for case in CASES {
        let sizes = schedule(case);
        for (i, bytes) in sizes.into_iter().enumerate() {
            fault_cases += 1;
            let mut cx = fault(i + 1, false);
            let watch = Watch::begin();
            let result = perform(case, &mut cx);
            watch.empty(i);
            let want = AllocationFailure { kind: Kind::Injected, site: Site::Bytes,
                requested_bytes: Some(bytes) };
            assert!(matches!(result, Err(NativeBufferFailure::Allocation(f)) if f == want));
            assert_eq!(cx.first_failure(), Some(want));
            assert_eq!(cx.growth_requests(), i + 1);
            // A consumed one-shot refusal must not become an accidental sticky policy.
            let recovered = try_unix_name(OsStr::new("later"), &mut cx).unwrap();
            assert_name(&recovered, b"later");
        }
    }
    assert_eq!(fault_cases, 35);
}

#[test]
fn sticky_failure_preserves_primary_cause_and_allows_empty_copy() {
    let _lock = LOCK.lock().unwrap();
    for case in CASES {
        let mut cx = fault(1, true);
        let watch = Watch::begin();
        let result = perform(case, &mut cx);
        drop(result);
        watch.empty(0);
        let first = cx.first_failure();
        watch.resume();
        let later = try_unix_name(OsStr::new("later"), &mut cx);
        let empty = try_unix_path_copy(Path::new(""), &mut cx);
        watch.empty(0);
        let later_error = AllocationFailure { kind: Kind::Injected, site: Site::Bytes,
            requested_bytes: Some(6) };
        assert!(matches!(later, Err(NativeBufferFailure::Allocation(f)) if f == later_error));
        assert_eq!(cx.first_failure(), first.or(Some(later_error)));
        assert!(empty.unwrap().as_os_str().is_empty());
    }
}

#[test]
fn checked_arithmetic_rejects_overflow_without_large_allocations() {
    // Pure synthetic lengths, not forged slices and not a second context.
    // Actual representable reservations go through the real context instead.
    let overflow = NativeBufferFailure::Allocation(AllocationFailure {
        kind: Kind::CapacityOverflow, site: Site::Bytes, requested_bytes: None,
    });
    assert_eq!(name_capacity(usize::MAX), Err(overflow));
    assert_eq!(child_capacity(usize::MAX, true, 1), Err(overflow));
    assert_eq!(child_capacity(usize::MAX - 1, false, 2), Err(overflow));
    assert_eq!(name_capacity(isize::MAX as usize), Ok(isize::MAX as usize + 1));
    assert_eq!(child_capacity(0, false, 0), Ok(0));
}
