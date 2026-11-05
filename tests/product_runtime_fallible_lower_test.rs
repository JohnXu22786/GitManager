// Standalone std-only harness; exactly one allocation-context module identity.
#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_runtime/fallible_lower.rs"]
mod fallible_lower;

use fallible_lower::{try_lowercase, test_add_length};
use product_alloc::{AllocationContext as Cx, AllocationFailure,
    AllocationFailureKind as Kind, AllocationSite as Site};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, num::NonZeroUsize,
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// Observation only: never inject null into the global allocator. This is the
// accepted R09 observation pattern; its exact emitted/linked hooks need fresh
// qualification. Oracles, inputs, and assertion formatting stay outside Watch.
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
    fn clean(&self, count: usize) {
        self.pause();
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer invariant");
        assert_eq!(USED.load(Ordering::Relaxed), count);
    }
    fn empty(&self, count: usize) {
        self.clean(count);
        for i in 0..count { assert_eq!(LIVE[i].load(Ordering::Relaxed), 0); }
    }
}
impl Drop for Watch { fn drop(&mut self) { self.pause(); } }

// Each literal has an independent readable expected result as well as the old
// whole-string std oracle. No scalar-flat_map substitute is used as an oracle.
const LITERALS: &[(&str, &str)] = &[
    ("", ""), ("AbCz012", "abcz012"), ("A\0Σ\0Z", "a\0σ\0z"),
    ("Σ", "σ"), ("ΟΣ", "ος"), ("ΟΣΑ", "οσα"), ("ΣΣ", "σς"), ("ΟΣΣ", "οσς"),
    ("AΣ.A", "aσ.a"), ("AΣ:A", "aσ:a"), ("AΣ!A", "aς!a"), ("A.Σ", "a.ς"),
    ("\u{0345}Σ", "\u{0345}σ"), ("A\u{0345}Σ", "a\u{0345}ς"),
    ("AΣ\u{0345}B", "aσ\u{0345}b"),
    ("\u{0301}Σ", "\u{0301}σ"), ("Σ\u{0301}", "σ\u{0301}"),
    ("A\u{0301}Σ", "a\u{0301}ς"), ("AΣ\u{0301}", "aς\u{0301}"),
    ("A'Σ", "a'ς"), ("AΣ'A", "aσ'a"), ("A^Σ", "a^ς"),
    ("AΣ^A", "aσ^a"), ("A`Σ", "a`ς"), ("AΣ`A", "aσ`a"),
    ("A\u{200d}Σ", "a\u{200d}ς"), ("AΣ\u{200d}B", "aσ\u{200d}b"),
    ("中Σ", "中σ"), ("AΣ中", "aς中"), ("AΣ😀B", "aς😀b"),
    ("中文", "中文"), ("😀", "😀"), ("İ", "i\u{0307}"), ("I", "i"),
    ("ı", "ı"), ("ß", "ß"), ("\u{023a}", "\u{2c65}"), ("\u{023e}", "\u{2c66}"),
    ("σςΣ", "σςς"), ("Σ Σ", "σ σ"), ("AΣ\0B", "aς\0b"),
];
const TITLECASE: [u32; 31] = [
    0x1c5,0x1c8,0x1cb,0x1f2,0x1f88,0x1f89,0x1f8a,0x1f8b,0x1f8c,0x1f8d,0x1f8e,0x1f8f,
    0x1f98,0x1f99,0x1f9a,0x1f9b,0x1f9c,0x1f9d,0x1f9e,0x1f9f,
    0x1fa8,0x1fa9,0x1faa,0x1fab,0x1fac,0x1fad,0x1fae,0x1faf,0x1fbc,0x1fcc,0x1ffc,
];
// Fixed representatives, not the separately proposed complete 459-range sweep.
const CI_BOUNDARIES: [u32; 28] = [
    0xa7,0xa8,0xa9,0xac,0xad,0xae,0x2af,0x2b0,0x2b1,0x2ff,0x300,0x301,
    0x344,0x345,0x346,0x36f,0x370,0x200c,0x200d,0x200e,
    0xfe0f,0xfe10,0xe0000,0xe0001,0xe0002,0xe0100,0xe01ef,0xe01f0,
];
fn fixtures() -> Vec<String> {
    let mut out: Vec<String> = LITERALS.iter().map(|(s, _)| (*s).to_owned()).collect();
    for n in 0u8..=127 {
        let c = char::from(n);
        out.push(format!("A{c}Σ")); out.push(format!("AΣ{c}B"));
    }
    for n in TITLECASE {
        let c = char::from_u32(n).unwrap();
        out.push(format!("{c}Σ")); out.push(format!("AΣ{c}"));
    }
    for n in CI_BOUNDARIES {
        let c = char::from_u32(n).unwrap();
        out.push(format!("A{c}Σ")); out.push(format!("AΣ{c}B"));
    }
    for n in [0, 1, 31, 1024] {
        let marks = "\u{0301}".repeat(n);
        out.push(format!("{marks}Σ")); out.push(format!("A{marks}Σ"));
        out.push(format!("AΣ{marks}")); out.push(format!("AΣ{marks}B"));
        out.push("Σ".repeat(n));
        out.push(format!("A{}", "Σ\u{0301}".repeat(n)));
        out.push(format!("{}B", "Σ\u{0301}".repeat(n)));
    }
    out.push(format!("{}A", "İ".repeat(1365)));
    out.push(format!("{}AB", "İ".repeat(1365)));
    out.push("A".repeat(4096)); out.push("A".repeat(4097)); out.push("😀".repeat(1024));
    out
}
fn fail(n: usize) -> Cx { Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true) }

#[test]
fn fixed_unicode_contexts_match_old_whole_string_and_all_fault_ordinals() {
    let _lock = LOCK.lock().unwrap();
    assert_eq!(char::UNICODE_VERSION, (17, 0, 0));
    let cases = fixtures();
    assert_eq!(cases.len(), 448);
    assert_eq!(cases.iter().map(String::len).max(), Some(4097));
    // All old-oracle construction occurs before ANY fault context or Watch.
    let expected: Vec<String> = cases.iter().map(|s| s.to_lowercase()).collect();
    for (i, (_, literal)) in LITERALS.iter().enumerate() {
        assert_eq!(&expected[i], literal, "literal {i}");
    }
    assert_eq!(expected[443].len(), 4096);
    assert_eq!(expected[444].len(), 4097);
    for (i, (input, oracle)) in cases.iter().zip(&expected).enumerate() {
        let before = input.clone();
        for ordinal in 0..=2 {
            let mut cx = if ordinal == 0 { Cx::new() } else { fail(ordinal) };
            let watch = Watch::begin();
            let result = try_lowercase(input, &mut cx);
            watch.pause();
            assert_eq!(input, &before, "input changed: {i}/{ordinal}");
            let growth = usize::from(!input.is_empty());
            assert_eq!(cx.growth_requests(), growth, "growth: {i}/{ordinal}");
            if ordinal == 1 && growth == 1 {
                let error = AllocationFailure { kind: Kind::Injected, site: Site::String,
                    requested_bytes: Some(oracle.len()) };
                assert_eq!(result, Err(error), "refusal: {i}");
                assert_eq!(cx.first_failure(), Some(error));
                watch.empty(0);
            } else {
                let output = result.unwrap();
                assert_eq!(output.as_bytes(), oracle.as_bytes(), "bytes: {i}/{ordinal}");
                assert_eq!(output.len(), oracle.len());
                assert_eq!(cx.first_failure(), None);
                watch.clean(growth);
                if growth != 0 {
                    assert_eq!(LIVE[0].load(Ordering::Relaxed), 1);
                    assert_eq!(PTR[0].load(Ordering::Relaxed), output.as_ptr() as usize);
                    assert_eq!(SIZE[0].load(Ordering::Relaxed), output.capacity());
                    assert!(output.capacity() >= oracle.len());
                }
                watch.resume(); drop(output); watch.empty(growth);
            }
        }
    }
}

#[test]
fn sticky_first_failure_survives_empty_and_later_lowering() {
    let _lock = LOCK.lock().unwrap();
    let input = "İΟΣ".to_owned();
    let before = input.clone();
    let oracle = input.to_lowercase();
    let mut cx = fail(1);
    let primary = product_alloc::try_copy_str("seed", &mut cx, Site::Bytes).unwrap_err();
    let watch = Watch::begin();
    let empty = try_lowercase("", &mut cx);
    let result = try_lowercase(&input, &mut cx);
    watch.pause();
    assert_eq!(empty, Ok(String::new()));
    assert_eq!(result, Err(AllocationFailure { kind: Kind::Injected,
        site: Site::String, requested_bytes: Some(oracle.len()) }));
    assert_eq!(cx.first_failure(), Some(primary));
    assert_eq!(cx.growth_requests(), 2);
    assert_eq!(input, before);
    watch.empty(0);
}

#[test]
fn synthetic_utf8_lengths_record_capacity_failure_before_any_growth() {
    let _lock = LOCK.lock().unwrap();
    let m = isize::MAX as usize;
    for c in ['a', '\u{80}', '\u{800}', '\u{10000}'] {
        let k = c.len_utf8();
        for total in [m - k, m - k + 1, m] {
            let mut cx = fail(1);
            let watch = Watch::begin();
            let result = test_add_length(total, c, &mut cx);
            watch.pause();
            if total == m - k {
                assert_eq!(result, Ok(m)); assert_eq!(cx.first_failure(), None);
            } else {
                let error = AllocationFailure { kind: Kind::CapacityOverflow,
                    site: Site::String, requested_bytes: Some(total + k) };
                assert_eq!(result, Err(error)); assert_eq!(cx.first_failure(), Some(error));
            }
            assert_eq!(cx.growth_requests(), 0); watch.empty(0);
        }
    }
    let mut cx = fail(1);
    let primary = product_alloc::try_copy_str("seed", &mut cx, Site::Bytes).unwrap_err();
    let watch = Watch::begin();
    let result = test_add_length(m, 'a', &mut cx);
    watch.pause();
    assert_eq!(result, Err(AllocationFailure { kind: Kind::CapacityOverflow,
        site: Site::String, requested_bytes: Some(m + 1) }));
    assert_eq!(cx.first_failure(), Some(primary));
    assert_eq!(cx.growth_requests(), 1); watch.empty(0);
}
