// Actual persisted Pointer integration, deliberately independent of the store root.
// The legacy Strict visitor is a frozen test-only oracle, never a codec fallback.
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[path = "../src/product_json.rs"] mod product_json;
#[path = "../src/product_app.rs"] mod product_app;
#[path = "../src/product_digest.rs"] mod product_digest;
#[path = "../src/product_store/pointer.rs"] mod pointer;
#[path = "../src/product_store/pointer_codec.rs"] mod pointer_codec;
#[path = "../src/product_store/json.rs"] mod legacy_json;
use pointer::Pointer;
use pointer_codec::{decode_pointer, encode_pointer, PointerDecodeError};
use product_alloc::{AllocationContext as Cx, AllocationFailureKind as Kind,
    AllocationSite as Site, FormatFailure};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, num::NonZeroUsize,
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// Non-failing System observer. Constant no-Drop TLS is primed before observing;
// fresh emitted/linked hook qualification is required on the pinned Linux build.
// No hook locks, formats, asserts, allocates metadata, or injects null returns.
thread_local! { static OBSERVING: Cell<bool> = const { Cell::new(false) }; }
const RECORDS: usize = 4096;
static PTR: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static SIZE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static ALIGN: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static LIVE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static USED: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BAD: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
static INJECTED: AtomicUsize = AtomicUsize::new(0);
fn observing() -> bool {
    match OBSERVING.try_with(Cell::get) {
        Ok(v) => v, Err(_) => { BAD.fetch_add(1, Ordering::Relaxed); false }
    }
}
fn register(p: *mut u8, size: usize, align: usize) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    if p.is_null() { BAD.fetch_add(1, Ordering::Relaxed); return; }
    let i = USED.fetch_add(1, Ordering::Relaxed);
    if i >= RECORDS { BAD.fetch_add(1, Ordering::Relaxed); return; }
    PTR[i].store(p as usize, Ordering::Relaxed);
    SIZE[i].store(size, Ordering::Relaxed);
    ALIGN[i].store(align, Ordering::Relaxed);
    LIVE[i].store(1, Ordering::Relaxed);
}
fn release(p: *mut u8, layout: Layout) {
    for i in (0..USED.load(Ordering::Relaxed).min(RECORDS)).rev() {
        if PTR[i].load(Ordering::Relaxed) == p as usize && LIVE[i].load(Ordering::Relaxed) == 1 {
            if SIZE[i].load(Ordering::Relaxed) != layout.size()
                || ALIGN[i].load(Ordering::Relaxed) != layout.align() {
                BAD.fetch_add(1, Ordering::Relaxed);
            }
            LIVE[i].store(0, Ordering::Relaxed); return;
        }
    }
    BAD.fetch_add(1, Ordering::Relaxed);
}
struct Observer;
unsafe impl GlobalAlloc for Observer {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if observing() { register(p, l.size(), l.align()); } p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if observing() { register(p, l.size(), l.align()); } p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if observing() { release(p, l); } unsafe { System.dealloc(p, l); }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if observing() {
            if !q.is_null() { release(p, l); }
            register(q, n, l.align());
        } q
    }
}
#[global_allocator] static ALLOCATOR: Observer = Observer;
struct Watch;
impl Watch {
    fn begin() -> Self {
        OBSERVING.with(|v| v.set(false));
        USED.store(0, Ordering::Relaxed); CALLS.store(0, Ordering::Relaxed);
        BAD.store(0, Ordering::Relaxed); OBSERVING.with(|v| v.set(true)); Self
    }
    fn pause(&self) { OBSERVING.with(|v| v.set(false)); }
    fn resume(&self) { OBSERVING.with(|v| v.set(true)); }
    fn calls(&self) -> usize { CALLS.load(Ordering::Relaxed) }
    fn empty(&self) {
        self.pause();
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer overflow/null/layout/unknown release");
        let n = USED.load(Ordering::Relaxed); assert!(n <= RECORDS);
        for i in 0..n { assert_eq!(LIVE[i].load(Ordering::Relaxed), 0, "unreleased concrete owner"); }
    }
}
impl Drop for Watch { fn drop(&mut self) { self.pause(); } }
fn fault(n: usize, sticky: bool) -> Cx {
    assert!(INJECTED.fetch_add(1, Ordering::Relaxed) < 16384, "finite fault-call cap");
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}
const KEYS: [&str; 5] = ["magic", "version", "project_id", "revision", "object"];
fn values() -> [String; 5] {
    ["\"arbitrary magic\"".into(), "3".into(), "\"project\"".into(), "7".into(),
        format!("\"{}\"", "0".repeat(64))]
}
fn object(v: &[String; 5], order: &[usize]) -> String {
    let fields: Vec<_> = order.iter().map(|&i| format!("\"{}\":{}", KEYS[i], v[i])).collect();
    format!("{{{}}}", fields.join(","))
}
fn array(v: &[String]) -> String { format!("[{}]", v.join(",")) }
fn old(bytes: &[u8]) -> Result<Pointer, String> {
    legacy_json::parse(bytes).map_err(|e| e.to_string())
}
fn result_text(result: &Result<Pointer, PointerDecodeError>) -> Result<(), String> {
    result.as_ref().map(|_| ()).map_err(|e| e.try_render(&mut Cx::new()).unwrap())
}
fn compare(bytes: &[u8]) {
    assert!(bytes.len() <= 65536, "finite parity input cap");
    let expected = old(bytes);
    let actual = decode_pointer(bytes, &mut Cx::new());
    match (expected, actual) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b);
            assert_eq!(product_contract::canonical_bytes(&a).unwrap(), encode_pointer(&b, &mut Cx::new()).unwrap());
        }
        (Err(a), Err(b)) => assert_eq!(a, b.try_render(&mut Cx::new()).unwrap(), "input {:?}", bytes),
        (a, b) => panic!("oracle disagreement: {a:?} versus {b:?}; input {bytes:?}"),
    }
}
fn marks(p: &Pointer) -> [(usize, usize, usize); 3] {
    [(p.magic.as_ptr() as usize, p.magic.len(), p.magic.capacity()),
     (p.project_id.as_ptr() as usize, p.project_id.len(), p.project_id.capacity()),
     (p.object.as_str().as_ptr() as usize, p.object.as_str().len(), 0)]
}
fn nested(n: usize, leaf: &str) -> String { format!("{}{}{}", "[".repeat(n), leaf, "]".repeat(n)) }

mod pointer_cases {
    use super::*;

    #[test]
    fn actual_types_and_structural_semantics_are_preserved() {
        let mut v = values(); v[0] = "\"\"".into(); v[1] = "99".into();
        v[2] = serde_json::to_string(&"NOT an ID / 雪".repeat(20)).unwrap(); v[3] = "0".into();
        let bytes = object(&v, &[0, 1, 2, 3, 4]);
        let p = decode_pointer(bytes.as_bytes(), &mut Cx::new()).unwrap();
        let _: &product_contract::Digest = &p.object;
        let _: &product_contract::Id = &p.project_id;
        assert_eq!(p.version, 99); assert_eq!(p.revision, 0); assert!(p.magic.is_empty());
        assert!(!product_contract::valid_id(&p.project_id));
        assert_eq!(p, p.clone());
        assert_eq!(serde_json::to_value(&p).unwrap(), serde_json::to_value(old(bytes.as_bytes()).unwrap()).unwrap());
        compare(bytes.as_bytes()); compare(array(&v).as_bytes());
    }

    #[test]
    fn all_permutations_shapes_whitespace_and_escaped_keys() {
        let v = values(); let mut order = [0, 1, 2, 3, 4]; let mut count = 0;
        loop {
            compare(object(&v, &order).as_bytes()); count += 1;
            let Some(i) = (0..4).rev().find(|&i| order[i] < order[i + 1]) else { break; };
            let j = (i + 1..5).rev().find(|&j| order[i] < order[j]).unwrap();
            order.swap(i, j); order[i + 1..].reverse();
        }
        assert_eq!(count, 120);
        compare(format!(" \n {}\t ", array(&v)).as_bytes());
        compare(object(&v, &[4, 3, 2, 1, 0]).replace("magic", "m\\u0061gic").as_bytes());
    }

    #[test]
    fn every_missing_subset_and_null_slot_has_original_error_order() {
        let v = values();
        for mask in 0..32 {
            let order: Vec<_> = (0..5).filter(|i| mask & (1 << i) != 0).collect();
            compare(object(&v, &order).as_bytes());
        }
        for i in 0..5 {
            let mut changed = v.clone(); changed[i] = "null".into();
            compare(object(&changed, &[4, 3, 2, 1, 0]).as_bytes()); compare(array(&changed).as_bytes());
        }
    }

    #[test]
    fn every_field_kind_and_scalar_root_distinguishes_type_from_value() {
        for raw in ["null", "true", "false", "0", "-1", "4294967296", "\"x\"", "[]", "{}"] {
            compare(raw.as_bytes());
            for i in 0..5 {
                let mut v = values(); v[i] = raw.into();
                compare(object(&v, &[0, 1, 2, 3, 4]).as_bytes()); compare(array(&v).as_bytes());
            }
        }
    }

    #[test]
    fn unknown_fields_and_competing_errors_use_lexical_not_wire_order() {
        for key in ["", "A", "magib", "n", "object!", "z", "é", "a\n`\0"] {
            let key = serde_json::to_string(key).unwrap();
            let body = object(&values(), &[4, 3, 2, 1, 0]);
            compare(format!("{{{key}:{{\"nested\":[\"owned\"]}},{}", &body[1..]).as_bytes());
            compare(format!("{}, {key}:[{{\"remainder\":\"owned\"}}]}}", &body[..body.len() - 1]).as_bytes());
        }
        for order in [[0, 1, 2, 3, 4], [4, 3, 2, 1, 0]] {
            let mut v = values(); v[0] = "false".into(); v[4] = "\"invalid\"".into();
            compare(object(&v, &order).as_bytes());
            v[0] = "\"ok\"".into(); v[2] = "true".into(); compare(object(&v, &order).as_bytes());
        }
        for raw in ["{\"a\":{\"nested\":1.5}}", "{\"z\":{\"x\":0,\"x\":null}}", "{\"a\":false,\"z\":[}"] { compare(raw.as_bytes()); }
    }

    #[test]
    fn duplicates_parse_their_values_before_rejecting_decoded_keys() {
        let v = values();
        for i in 0..5 {
            for raw in ["null", "[0,1]", "1.0", "{\"x\":0,\"x\":1}", "[}"] {
                compare(format!("{{\"{}\":{},\"{}\":{raw}}}", KEYS[i], v[i], KEYS[i]).as_bytes());
            }
        }
        for raw in [
            r#"{"magic":"a","m\u0061gic":"b"}"#,
            r#"{"magic":"a","m\u0061gic":[}"#,
            r#"{"a":{"x":"one","\u0078":"two"}}"#,
            "{\n\"a\":1,\n\"a\":2\n}",
            r#"{"z":1,"z":0e0}"#,
        ] { compare(raw.as_bytes()); }
    }

    #[test]
    fn array_lengths_and_slot_surplus_precedence_match_derive() {
        let mut v = values().to_vec(); v.extend(["0".into(), "\"tail\"".into()]);
        for n in 0..=7 {
            compare(array(&v[..n]).as_bytes());
            for i in 0..n.min(5) {
                let mut changed = v[..n].to_vec(); changed[i] = "null".into();
                compare(array(&changed).as_bytes());
            }
        }
        let valid = array(&values());
        for suffix in [",1.0]", ",{\"x\":0,\"x\":1}]", ",[}]", ",0,1]"] {
            compare(format!("{}{}", &valid[..valid.len() - 1], suffix).as_bytes());
        }
    }

    #[test]
    fn integer_boundaries_and_float_lexemes_keep_original_precedence() {
        let numbers = ["0", "1", "2", "3", "99", "4294967295", "4294967296", "4294967294",
            "9223372036854775807", "9223372036854775808", "18446744073709551615", "18446744073709551616",
            "-1", "-9223372036854775808", "-9223372036854775809", "-0", "0.0", "1e0", "1E-1",
            "1e400", "1e-400", "01", "1e", "1e+", "--1", "+1", "1."];
        for raw in numbers {
            for i in [1, 3] {
                let mut v = values(); v[i] = raw.into();
                compare(object(&v, &[4, 3, 2, 1, 0]).as_bytes()); compare(array(&v).as_bytes());
            }
        }
    }

    #[test]
    fn digest_predicate_moves_or_drops_the_same_owner_without_growth() {
        let _lock = LOCK.lock().unwrap();
        let mut cases: Vec<String> = [0, 1, 63, 64, 65].iter().map(|&n| "0".repeat(n)).collect();
        cases.extend(["a".repeat(64), "f".repeat(64), "0123456789abcdef".repeat(4),
            "A".repeat(64), "g".repeat(64), format!("{}\0", "0".repeat(63)),
            "é".repeat(32), "ａ".repeat(64), "١".repeat(32)]);
        for text in cases {
            let expected = product_contract::Digest::try_from(text.clone()).is_ok();
            let mut v = values(); v[4] = serde_json::to_string(&text).unwrap();
            compare(object(&v, &[0, 1, 2, 3, 4]).as_bytes()); compare(array(&v).as_bytes());
            let watch = Watch::begin(); let mut cx = Cx::new();
            let owner = product_alloc::try_copy_str(&text, &mut cx, Site::String).unwrap();
            let address = owner.as_ptr(); let before = watch.calls();
            let digest = product_contract::Digest::from_owned_lower_hex(owner);
            let after = watch.calls(); watch.pause();
            assert_eq!(expected, digest.is_some()); assert_eq!(before, after);
            if let Some(d) = &digest { assert_eq!(d.as_str().as_ptr(), address); }
            watch.resume(); drop(digest); watch.empty();
        }
        let mut v = values(); v[4] = format!("\"{}\"", "\\u0061".repeat(64));
        compare(object(&v, &[0, 1, 2, 3, 4]).as_bytes());
    }

    #[test]
    fn unicode_escapes_truncation_and_trailing_bytes_keep_positions() {
        for raw in [r#""\ud800""#, r#""\udc00""#, r#""\ud800\udc00""#, r#""\u0000""#,
            r#""\x""#, r#""\u12""#, "\"line\nbreak\"", "{", "[", "", "true false", "{}x"] {
            compare(raw.as_bytes());
        }
        for raw in [&b"\"\xff\""[..], &b"{\"magic\":\"\xc0\xaf\"}"[..], &b"[\"\xed\xa0\x80\"]"[..]] { compare(raw); }
        let valid = object(&values(), &[0, 1, 2, 3, 4]);
        for n in [1, 2, 8, valid.len() / 2, valid.len() - 1] { compare(&valid.as_bytes()[..n]); }
        let mut v = values(); v[0] = r#""雪\ud83d\ude00\n\t\u0000""#.into();
        v[2] = serde_json::to_string(&"Not an ID! 雪".repeat(20)).unwrap();
        compare(object(&v, &[0, 1, 2, 3, 4]).as_bytes());
    }

    #[test]
    fn parser_depth_and_large_pointer_do_not_gain_proposal_or_native_caps() {
        for depth in [126, 127, 128] {
            let child = nested(depth - 1, "\"owned\"");
            compare(format!("{{\"unknown\":{child}}}").as_bytes());
            let mut v = values(); v[0] = child.clone(); compare(object(&v, &[0, 1, 2, 3, 4]).as_bytes());
            let seq = array(&values()); compare(format!("{},{child}]", &seq[..seq.len() - 1]).as_bytes());
        }
        for n in [16100, 16384, 20000] {
            let mut v = values(); v[0] = serde_json::to_string(&"m".repeat(n)).unwrap();
            let wire = object(&v, &[0, 1, 2, 3, 4]); compare(wire.as_bytes());
            assert_eq!(decode_pointer(wire.as_bytes(), &mut Cx::new()).unwrap().magic.len(), n);
        }
    }

    #[test]
    fn canonical_bytes_keep_key_order_escapes_utf8_and_integer_spelling() {
        let controls: String = (0..=31).map(char::from).collect();
        let strings = ["".to_owned(), "\"\\\n\r\t\x08\x0c/".to_owned(), "é雪😀\u{2028}".to_owned(), controls];
        for magic in &strings {
            for project in &strings {
                for (version, revision) in [(0, 0), (u32::MAX, u64::MAX), (1, 9223372036854775808)] {
                    let p = Pointer { magic: magic.clone(), version, project_id: project.clone(), revision,
                        object: product_contract::Digest::try_from("0123456789abcdef".repeat(4)).unwrap() };
                    let expected = product_contract::canonical_bytes(&p).unwrap();
                    let bytes = encode_pointer(&p, &mut Cx::new()).unwrap();
                    assert_eq!(bytes, expected); assert!(bytes.starts_with(b"{\"magic\":"));
                    assert!(!bytes.ends_with(b"\n")); compare(&bytes);
                }
            }
        }
    }

    #[test]
    fn test_only_data_domain_framing_matches_legacy_canonical_digest() {
        for wire in [object(&values(), &[4, 2, 0, 3, 1]), array(&values())] {
            let p = decode_pointer(wire.as_bytes(), &mut Cx::new()).unwrap();
            let before = p.object.clone(); let bytes = encode_pointer(&p, &mut Cx::new()).unwrap();
            let expected = product_contract::canonical_digest(product_contract::IdentityDomain::Data, &p).unwrap();
            let actual = product_digest::try_bytes_digest(product_contract::IdentityDomain::Data, &bytes).unwrap();
            assert_eq!(actual.lower_hex_bytes().as_slice(), expected.as_str().as_bytes());
            assert_eq!(p.object, before); // This is not a snapshot digest or CURRENT qualification.
        }
    }

    #[test]
    fn every_parser_growth_refusal_stops_and_cleans_concrete_owners() {
        let _lock = LOCK.lock().unwrap();
        let mut wrong = values(); wrong[0] = r#"{"owned":["first",{"nested":"second"}]}"#.into();
        let mut bad_digest = values(); bad_digest[4] = "\"invalid owned digest\"".into();
        let mut later = values(); later[1] = r#"["later",{"tail":"owner"}]"#.into();
        let fixtures = [object(&values(), &[0, 1, 2, 3, 4]), array(&values()),
            format!("{{\"a\":{},\"z\":{{\"owned\":[\"remainder\"]}}}}", nested(126, "\"leaf\"")),
            object(&wrong, &[0, 1, 2, 3, 4]), object(&bad_digest, &[0, 1, 2, 3, 4]),
            object(&later, &[0, 1, 2, 3, 4]), r#"{"magic":"prefix","magic":{"nested":["suffix"]}}"#.into()];
        assert_eq!(fixtures.len(), 7);
        for wire in fixtures {
            assert!(wire.len() <= 16384);
            let unchanged = wire.clone(); let expected = old(wire.as_bytes()).map(|_| ());
            let mut baseline = Cx::new(); drop(product_json::parse_store_json_bytes(wire.as_bytes(), &mut baseline));
            let count = baseline.growth_requests(); assert!((1..=512).contains(&count));
            for sticky in [false, true] {
                for n in 1..=count {
                    let mut oracle_cx = fault(n, sticky);
                    let expected_error = match product_json::parse_store_json_bytes(wire.as_bytes(), &mut oracle_cx) {
                        Err(e) => e, Ok(_) => panic!("parser refusal missing"),
                    };
                    let watch = Watch::begin(); let mut cx = fault(n, sticky);
                    let result = decode_pointer(wire.as_bytes(), &mut cx);
                    let calls = watch.calls(); watch.pause();
                    match &result {
                        Err(PointerDecodeError::Json(e)) => {
                            assert_eq!(*e, expected_error);
                            match e.cause {
                                product_json::Cause::Allocation(f) => { assert_eq!(f.kind, Kind::Injected); assert_eq!(cx.first_failure(), Some(f)); }
                                _ => panic!("allocation replaced by schema/syntax"),
                            }
                        }
                        _ => panic!("wrong refusal owner {result:?}"),
                    }
                    assert_eq!(cx.growth_requests(), n); assert_eq!(calls, n - 1);
                    watch.resume(); drop(result); watch.empty(); assert_eq!(wire, unchanged);
                }
                let watch = Watch::begin(); let mut cx = fault(count + 1, sticky);
                let result = decode_pointer(wire.as_bytes(), &mut cx); let calls = watch.calls(); watch.pause();
                assert_eq!(cx.growth_requests(), count); assert_eq!(cx.first_failure(), None);
                assert_eq!(calls, count); assert_eq!(result_text(&result), expected);
                watch.resume(); drop(result); watch.empty();
            }
        }
    }

    #[test]
    fn encoder_has_one_growth_and_never_changes_its_borrowed_pointer() {
        let _lock = LOCK.lock().unwrap();
        let p = old(object(&values(), &[0, 1, 2, 3, 4]).as_bytes()).unwrap();
        let identity = marks(&p); let expected = product_contract::canonical_bytes(&p).unwrap();
        for sticky in [false, true] {
            let watch = Watch::begin(); let mut cx = fault(1, sticky);
            let result = encode_pointer(&p, &mut cx); let calls = watch.calls(); watch.pause();
            match result {
                Err(product_json::canonical::EncodeError::Allocation(f)) => {
                    assert_eq!(f.kind, Kind::Injected); assert_eq!(f.site, Site::Bytes);
                    assert_eq!(f.requested_bytes, Some(expected.len())); assert_eq!(cx.first_failure(), Some(f));
                }
                _ => panic!("missing output refusal"),
            }
            assert_eq!(calls, 0); assert_eq!(cx.growth_requests(), 1); assert_eq!(marks(&p), identity);
            watch.empty();
        }
        let watch = Watch::begin(); let mut cx = fault(2, true);
        let bytes = encode_pointer(&p, &mut cx).unwrap(); let calls = watch.calls(); watch.pause();
        assert_eq!(bytes, expected); assert_eq!(calls, 1); assert_eq!(cx.growth_requests(), 1);
        assert_eq!(cx.first_failure(), None); assert_eq!(marks(&p), identity);
        watch.resume(); drop(bytes); watch.empty();
    }

    #[test]
    fn diagnostic_refusals_preserve_prefix_cause_and_all_owned_cleanup() {
        let _lock = LOCK.lock().unwrap();
        for wire in [r#"{"a\n`":"owned","z":["remainder",{"deep":"tail"}]}"#.to_owned(),
            r#"{"magic":true,"z":{"owned":"tail"}}"#.to_owned(),
            "[]".to_owned(), "null".to_owned(),
            object(&{ let mut v = values(); v[4] = "\"invalid digest\"".into(); v }, &[0, 1, 2, 3, 4])] {
            let expected = old(wire.as_bytes()).unwrap_err();
            let error = decode_pointer(wire.as_bytes(), &mut Cx::new()).unwrap_err();
            let mut rcx = Cx::new(); let mut text = "prefix:".to_owned();
            error.try_append_diagnostic(&mut text, &mut rcx).unwrap();
            assert_eq!(text, format!("prefix:{expected}"));
            let count = rcx.growth_requests(); assert!((1..=128).contains(&count));
            for sticky in [false, true] {
                for n in 1..=count {
                    let watch = Watch::begin(); let mut parse_cx = Cx::new();
                    let cause = decode_pointer(wire.as_bytes(), &mut parse_cx).unwrap_err();
                    let mut prefix_cx = Cx::new();
                    let mut prefix = product_alloc::try_copy_str("prefix:", &mut prefix_cx, Site::String).unwrap();
                    let mut render_cx = fault(n, sticky);
                    let failure = cause.try_append_diagnostic(&mut prefix, &mut render_cx);
                    let calls = watch.calls(); watch.pause();
                    match failure {
                        Err(FormatFailure::Allocation(f)) => { assert_eq!(f.kind, Kind::Injected); assert_eq!(f.site, Site::Formatting); assert_eq!(render_cx.first_failure(), Some(f)); }
                        _ => panic!("missing separate rendering refusal"),
                    }
                    assert_eq!(prefix, "prefix:"); assert_eq!(render_cx.growth_requests(), n);
                    assert_eq!(calls, parse_cx.growth_requests() + prefix_cx.growth_requests() + n - 1);
                    assert_eq!(cause.try_render(&mut Cx::new()).unwrap(), expected);
                    watch.resume(); drop(prefix); drop(cause); watch.empty();
                }
            }
        }
        // Separate contexts and completed outputs do not inherit a refusal.
        let good = object(&values(), &[0, 1, 2, 3, 4]); let mut denied = fault(1, true);
        assert!(decode_pointer(good.as_bytes(), &mut denied).is_err());
        assert!(decode_pointer(good.as_bytes(), &mut Cx::new()).is_ok());
    }
}
