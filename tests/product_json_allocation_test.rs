// Re-run every original intake assertion unchanged, including its reader tests.
// These are TEST-ONLY infallible oracles, never a production error fallback.
include!("tool_proposal_input_test.rs");
#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_json.rs"]
mod product_json;

use product_alloc::{AllocationContext, AllocationFailureKind, AllocationSite};
use product_json::{Cause, Error, Number as PNumber, Value as PValue};
use std::fmt::{self, Write};
use std::num::NonZeroUsize;

fn parse(input: &[u8], cx: &mut AllocationContext) -> Result<PValue, Error> {
    product_json::parse_json_bytes(input, cx)
}
fn refused(n: usize, sticky: bool) -> AllocationContext {
    AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}
fn error(result: Result<PValue, Error>) -> Error {
    match result { Err(error) => error, Ok(_) => panic!("expected failure") }
}
fn same_value(actual: &PValue, expected: &Value) {
    match (actual, expected) {
        (PValue::Null, Value::Null) => (),
        (PValue::Bool(a), Value::Bool(b)) => assert_eq!(a, b),
        (PValue::String(a), Value::String(b)) => assert_eq!(a, b),
        (PValue::Number(a), Value::Number(b)) => match a {
            PNumber::U64(n) => { assert!(!b.is_f64()); assert_eq!(Some(*n), b.as_u64()); }
            PNumber::I64(n) => { assert!(!b.is_f64()); assert_eq!(Some(*n), b.as_i64()); }
            PNumber::F64(n) => { assert!(b.is_f64()); assert_eq!(n.to_bits(), b.as_f64().unwrap().to_bits()); }
        },
        (PValue::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) { same_value(a, b); }
        }
        (PValue::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for ((ak, av), (bk, bv)) in a.iter().zip(b.iter()) {
                assert_eq!(ak, bk); same_value(av, bv);
            }
        }
        _ => panic!("different value variants"),
    }
}
fn compare(input: &[u8]) {
    // Oracle construction and diagnostics do not run under injected allocation.
    let expected = parse_json_bytes(input);
    let actual = parse(input, &mut AllocationContext::new());
    match (actual, expected) {
        (Ok(a), Ok(b)) => same_value(&a, &b),
        (Err(a), Err(b)) => {
            match &b {
                InputError::InputTooLarge => assert_eq!(a.cause, Cause::InputTooLarge),
                InputError::NestingTooDeep => assert_eq!(a.cause, Cause::NestingTooDeep),
                InputError::DuplicateObjectMember => assert_eq!(a.cause, Cause::DuplicateObjectMember),
                InputError::InvalidJson(b) => {
                    assert!(matches!(a.cause, Cause::Syntax(_)));
                    assert_eq!((a.line, a.column), (b.line(), b.column()), "{input:?}");
                }
                _ => panic!("byte slice oracle returned non-syntax error"),
            }
            assert_eq!(a.to_string(), b.to_string(), "{input:?}");
        }
        (Err(a), Ok(_)) => panic!("new parser rejected {input:?}: {a}"),
        (Ok(_), Err(b)) => panic!("new parser accepted {input:?}: {b}"),
    }
}

#[test]
fn differential_ordinary_values_ordered_keys_and_numeric_bits() {
    for input in [
        r#"null"#, r#" true \t"#, r#"false"#, r#"[]"#, r#"{}"#,
        r#"{"z":[0,-0,1.0,1e0,-1,-9223372036854775808,18446744073709551615],"a":"中文 🚀","passed":true,"path":"../../data"}"#,
        r#"{"first":{"x":1},"second":{"x":2},"é":3,"e\u0301":4,"X":5,"x":6}"#,
        r#"["", "a\nb\t\r\b\f\\\/\"", "\u0000", "\u4e2d\u6587", "\uD83D\uDE80"]"#,
    ] { compare(input.as_bytes()); }
}

#[test]
fn differential_malformed_unicode_positions_and_error_precedence() {
    let cases: &[&[u8]] = &[
        b"", b" \r\n\t", b"[", b"{", b"[1,]", b"[1 2]", b"[1,", b"{x:1}",
        br#"{"x":1,}"#, br#"{"x"}"#, br#"{"x""y"}"#, br#"{"x":1 "y":2}"#,
        br#"{"x":1,"x":?}"#, br#"{"x":1,"\u0078"}"#, br#"{"":1,"":]}"#,
        b"null false", b"true!", b"nul", b"nux", b"NaN", b"Infinity", b"\xef\xbb\xbfnull",
        b"\"\xff\"", b"\"\xc0\x80\"", b"\"\xed\xa0\x80\"", b"null\xff", b"\"\xff\\q\"",
        b"\"\xff\\u1234\"", b"\"\xff\n\"", b"\"\xff", b"\"\xc3\\n\xa9\"",
        br#""\q""#, br#""\u""#, br#""\u12""#, br#""\u12xz""#, br#""\uDC00""#,
        br#""\ud800""#, br#""\ud800x""#, br#""\ud800\x""#, br#""\ud800\u1234""#,
        br#""\ud800\uD800""#, br#""\ud800\u""#, b"\"\\", b"\"\\ud800", b"\"\\ud800\\",
        b"\"raw\nline\"", b"[\r\n 1,\n 2,\n ]", "{\n\"中\":1,\n?}".as_bytes(),
        b"01", b"-01", b"-", b"-x", b"1.", b"1.e2", b"1e", b"1e+", b"1e-?",
        b"1e9999999999x", b"0e9999999999x", b"-0e-9999999999", b"1e-9999999999x",
        b"[\n1e9999999999]", b"// comment\nnull", b"\x0b", b"\x0c",
    ];
    for input in cases { compare(input); }
    // All one-byte values in/outside strings and escape positions (incl. NUL).
    for byte in 0..=255u8 {
        compare(&[byte]); compare(&[b'"', byte, b'"']);
        compare(&[b'"', b'\\', byte, b'"']);
        compare(&[b'"', b'\\', b'u', b'0', byte, b'0', b'0', b'"']);
    }
}

#[test]
fn differential_numeric_rounding_overflow_underflow_negative_zero() {
    let values = ["0", "-0", "0.0", "-0.0", "0e0", "-0e0", "-1e-400",
        "9223372036854775807", "9223372036854775808", "-9223372036854775808",
        "-9223372036854775809", "18446744073709551615", "18446744073709551616",
        "9007199254740993.0", "1.23456789012345678901234567890123456789",
        "18446744073709551615.123", "18446744073709551616.123",
        "1.7976931348623157e308", "1.7976931348623159e308", "2.2250738585072014e-308",
        "2.2250738585072011e-308", "4.9406564584124654e-324", "2.4703282292062327e-324",
        "0e2147483648", "-0e2147483648", "1e2147483648", "1e-2147483648",
        "1e2147483647", "1e-2147483647", "0.0000000000000000000000000000000001"];
    for value in values { compare(value.as_bytes()); }
    for sign in ["", "-"] {
        for mantissa in ["0", "1", "17976931348623157", "18446744073709551615", "18446744073709551616", "999999999999999999999999999999"] {
            for exponent in [-1000, -400, -325, -324, -323, -309, -308, -307, -1, 0, 1, 289, 290, 307, 308, 309, 1000] {
                compare(format!("{sign}{mantissa}e{exponent}").as_bytes());
            }
        }
    }
    for value in ["-0", "-0.0", "-0e9999999999", "-1e-9999999999"] {
        match parse(value.as_bytes(), &mut refused(1, true)).unwrap() {
            PValue::Number(PNumber::F64(n)) => assert_eq!(n.to_bits(), (-0.0_f64).to_bits()),
            _ => panic!("negative zero lost"),
        }
    }
}

#[test]
fn differential_caps_depth_and_decoded_duplicate_before_invalid_value() {
    let mut exact = format!("\"{}\"", "é".repeat((MAX_INPUT_BYTES - 2) / 2)).into_bytes();
    exact.resize(MAX_INPUT_BYTES, b' ');
    compare(&exact);
    exact.push(b' '); exact[0] = 255;
    let mut cx = refused(1, true);
    assert_eq!(error(parse(&exact, &mut cx)).cause, Cause::InputTooLarge);
    assert_eq!(cx.growth_requests(), 0);
    compare(&exact);
    for depth in [0, 1, 63, 64, 65, 70] {
        for scalar in ["0", "\"text\"", "{}", "[]"] {
            compare(format!("{}{}{}", "[".repeat(depth), scalar, "]".repeat(depth)).as_bytes());
        }
        let text = format!("{}0{}", "{\"k\":".repeat(depth), "}".repeat(depth));
        compare(text.as_bytes());
    }
    for input in [r#"{"x":0,"\u0078":?}"#, r#"{"🚀":0,"\ud83d\ude80":{bad}"#,
                  r#"{"x":0,"x""#, r#"{"x":0,"x":1e9999999999}"#] {
        assert_eq!(error(parse(input.as_bytes(), &mut AllocationContext::new())).cause,
                   Cause::DuplicateObjectMember);
        compare(input.as_bytes());
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}
fn generated(seed: &mut u64, depth: u8) -> String {
    let choice = next(seed) % if depth == 0 { 5 } else { 7 };
    match choice {
        0 => "null".into(), 1 => (next(seed) & 1 == 0).to_string(),
        2 => format!("{}{}e{}", if next(seed)&1==0 {""}else{"-"}, next(seed), (next(seed)%700) as i32 - 350),
        3 => [r#""中\u0000🚀""#, r#""\uD83D\uDE80""#, r#""\"\\\n""#][next(seed) as usize%3].into(),
        4 => format!("{}.{}", next(seed), next(seed)),
        5 => format!("[{},{}]", generated(seed,depth-1), generated(seed,depth-1)),
        _ => format!("{{\"z\":{},\"a\":{}}}", generated(seed,depth-1), generated(seed,depth-1)),
    }
}
#[test]
fn differential_bounded_lcg_generated_and_mutated_documents() {
    let mut seed = 0x5240_4a27_2026_u64;
    for _ in 0..1200 {
        let document = generated(&mut seed, 4);
        assert!(document.len() < 4096);
        compare(document.as_bytes());
        let mut bytes = document.into_bytes();
        let index = next(&mut seed) as usize % bytes.len();
        bytes[index] = (next(&mut seed) >> 32) as u8;
        compare(&bytes);
        bytes.truncate(next(&mut seed) as usize % bytes.len());
        compare(&bytes);
    }
    const ALPHABET: &[u8] = b"{}[],:\"\\truefalsn0123456789.eE+- \r\n\t\x00\xff";
    for _ in 0..2000 {
        let mut bytes = [0u8; 80];
        let len = next(&mut seed) as usize % bytes.len();
        for b in &mut bytes[..len] { *b = ALPHABET[next(&mut seed) as usize % ALPHABET.len()]; }
        compare(&bytes[..len]);
    }
}

fn sweep(input: &[u8]) {
    compare(input); // Allocating oracle is outside faulted request.
    let mut normal = AllocationContext::new();
    drop(parse(input, &mut normal));
    let requests = normal.growth_requests();
    for sticky in [false, true] {
        for nth in 1..=requests {
            let mut cx = refused(nth, sticky);
            let failure = error(parse(input, &mut cx));
            let Cause::Allocation(allocation) = failure.cause else { panic!("resource failure was masked: {failure}") };
            assert_eq!(allocation.kind, AllocationFailureKind::Injected);
            assert_eq!(cx.growth_requests(), nth, "no retry or fall-through after refusal");
            assert_eq!(cx.first_failure(), Some(allocation));
            let later = product_alloc::try_copy_str("later", &mut cx, AllocationSite::String);
            assert_eq!(later.is_err(), sticky);
            assert_eq!(cx.first_failure(), Some(allocation));
        }
    }
    let mut cx = refused(requests + 1, true);
    let later = parse(input, &mut cx);
    assert!(!matches!(later, Err(Error { cause: Cause::Allocation(_), .. })));
    assert_eq!(cx.growth_requests(), requests);
}
#[test]
fn every_actual_growth_and_sticky_refusal_propagates_without_fallback() {
    for input in [r#"null"#, r#"-0"#, r#""""#, r#""中🚀""#, r#""a\nb\u4e2d\ud83d\ude80""#,
        r#"["a",{"z":[0,1,"b"],"a":{"k":"v"}},null]"#,
        r#"{"z":1,"y":2,"x":3,"w":4,"v":5,"u":6,"t":7,"s":8,"r":9,"q":10,"p":11,"o":12}"#,
        r#"{"x":"first","x":?}"#, r#"["text",{"a":[0,1,2]},?]"#,
        r#""a\nb\uDC00""#, r#"{"a":"text"} !"#] { sweep(input.as_bytes()); }
    // Partial recursive owners and accumulated siblings are dropped on unwind.
    let deepest = format!("{}[\"leaf\",{{\"k\":\"value\"}}]{}", "[\"owned\",".repeat(62), "]".repeat(62));
    sweep(deepest.as_bytes());
    let invalid = format!("{}?{}", "{\"key\":[\"owned\",".repeat(31), "]}".repeat(31));
    sweep(invalid.as_bytes());
}

struct Fixed { bytes: [u8; 512], len: usize }
impl Write for Fixed {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(text.as_bytes());
        self.len = end; Ok(())
    }
}
#[test]
fn inline_errors_keep_original_cause_when_later_rendering_is_refused() {
    let mut cx = refused(1, true);
    let first = error(parse(br#""owned""#, &mut cx));
    let snapshot = first;
    let mut fixed = Fixed { bytes: [0; 512], len: 0 };
    write!(&mut fixed, "{first}").unwrap();
    assert!(fixed.len > 0);
    let mut dynamic = String::new();
    assert!(product_alloc::try_append_fmt(&mut dynamic, format_args!("{first}"),
        &mut cx, AllocationSite::Formatting).is_err());
    assert_eq!(snapshot, first);
    let Cause::Allocation(a) = first.cause else { panic!("wrong cause") };
    assert_eq!(cx.first_failure(), Some(a));
    assert!(dynamic.is_empty());
    let semantic = error(parse(b"?", &mut cx));
    assert!(matches!(semantic.cause, Cause::Syntax(_)));
    assert!(product_alloc::try_append_fmt(&mut dynamic, format_args!("{semantic}"),
        &mut cx, AllocationSite::Formatting).is_err());
    assert!(matches!(semantic.cause, Cause::Syntax(_)));
    assert_eq!(semantic.to_string(), "invalid JSON: expected value at line 1 column 1");
}
