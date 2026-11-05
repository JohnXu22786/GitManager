// Test-only exact frozen copy of src/product_store/json.rs, including its
// generic parse/from_value mapping. Never a production fallback.
// Original SHA256: 0c632afcc88145181486fa108bf1b48bea3d1b4d01b9299513fbcadb2702799d
#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_json.rs"]
mod product_json;
use crate::product_alloc::{AllocationContext, AllocationFailureKind, AllocationSite};
use self::product_json::{Cause, Error, Number, SyntaxCode, Value};
use std::fmt::{self, Write};
use std::num::NonZeroUsize;
mod oracle {
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Strict, E> {
                Err(E::custom(
                    "floating point values are not part of the store format",
                ))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<Strict>()? {
                    values.push(value.0);
                }
                Ok(Strict(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, Strict>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(Strict(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}
pub(super) fn parse<T: de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let Strict(value) = serde_json::from_slice(bytes)?;
    serde_json::from_value(value)
}

}
fn parse(input: &[u8], cx: &mut AllocationContext) -> Result<Value, Error> {
    product_json::parse_store_json_bytes(input, cx)
}
fn error(input: &[u8]) -> Error {
    match parse(input, &mut AllocationContext::new()) {
        Err(e) => e, Ok(_) => panic!("expected error"),
    }
}
fn same_value(actual: &Value, expected: &serde_json::Value) {
    use serde_json::Value as S;
    match (actual, expected) {
        (Value::Null, S::Null) => (),
        (Value::Bool(a), S::Bool(b)) => assert_eq!(a,b),
        (Value::String(a), S::String(b)) => assert_eq!(a,b),
        (Value::Number(Number::U64(a)), S::Number(b)) => {
            assert!(!b.is_f64()); assert_eq!(Some(*a), b.as_u64());
        }
        (Value::Number(Number::I64(a)), S::Number(b)) => {
            assert!(!b.is_f64()); assert_eq!(Some(*a), b.as_i64());
        }
        (Value::Array(a), S::Array(b)) => {
            assert_eq!(a.len(),b.len());
            for (a,b) in a.iter().zip(b) { same_value(a,b); }
        }
        (Value::Object(a), S::Object(b)) => {
            assert_eq!(a.len(),b.len());
            for ((ak,av),(bk,bv)) in a.iter().zip(b) {
                assert_eq!(ak,bk); same_value(av,bv);
            }
        }
        _ => panic!("different value categories"),
    }
}
fn compare(input: &[u8]) {
    let expected = oracle::parse::<serde_json::Value>(input);
    let actual = parse(input, &mut AllocationContext::new());
    match (actual,expected) {
        (Ok(a),Ok(b)) => same_value(&a,&b),
        (Err(a),Err(b)) => {
            assert_eq!((a.line,a.column),(b.line(),b.column()),"{input:?}");
            assert_eq!(a.to_string(),b.to_string(),"{input:?}");
            assert!(matches!(a.cause, Cause::StoreSyntax(_) |
                Cause::StoreDuplicateObjectMember | Cause::StoreFloatNotAllowed));
        }
        (Err(a),Ok(_)) => panic!("new rejected {input:?}: {a}"),
        (Ok(_),Err(b)) => panic!("new accepted {input:?}: {b}"),
    }
}
#[test]
fn store_scalars_collections_and_integer_boundaries_match_strict() {
    for input in ["null", "true", "false", "0", "-1", "9223372036854775808",
        "-9223372036854775808", "18446744073709551615", "[]", "{}",
        r#"{"z":[0,true,null,"中🚀"],"a":{"x":1},"Z":[],"e\u0301":{},"é":""}"#,
        r#"["a\nb\t\r\b\f\\\/\"", "\u0000", "\ud83d\ude80"]"#] {
        compare(input.as_bytes());
    }
}
#[test]
fn store_float_rejection_follows_lexical_range_and_negative_zero() {
    for number in ["-0", "0.0", "-0.0", "1e0", "0e2147483648", "-0e2147483648",
        "-1e-9999999999", "18446744073709551616", "-9223372036854775809",
        "1.7976931348623157e308", "1.7976931348623159e308", "1e2147483648",
        "1e9999999999x", "0e9999999999x", "1.", "1e", "1e+", "01", "-01"] {
        for (before,after) in [("",""),("[\n",",?]"),("{\"k\":"," } trailing")] {
            compare(format!("{before}{number}{after}").as_bytes());
        }
    }
    for number in ["-0","-0.0","0e9999999999","-1e-9999999999"] {
        assert_eq!(error(number.as_bytes()).cause,Cause::StoreFloatNotAllowed);
    }
    assert_eq!(error(b"1e9999999999").cause,Cause::StoreSyntax(SyntaxCode::NumberOutOfRange));
}
#[test]
fn store_duplicate_value_precedence_and_decoded_keys_match_strict() {
    for input in [r#"{"x":0,"x":1}"#, r#"{"x":0,"x":?}"#, r#"{"x":0,"x":1.0}"#,
        r#"{"x":0,"x":1e9999999999}"#, r#"{"x":0,"x":{"a":1,"a":?}}"#,
        r#"{"x":0,"x":{"a":1,"a":2}}"#, r#"{"x":0,"x":[0,]}"#,
        r#"{"x":0,"\u0078":[]}"#, r#"{"🚀":0,"\ud83d\ude80":{}}"#,
        r#"{"":0,"":null}"#, r#"{"x":0,"x"}"#, r#"{"x":0,"x""#,
        r#"{"x":0,"x":0,?}"#, r#"{"x":0,"x":0 garbage}"#,
        r#"{"x":0,"x":{"a":1.0}}"#, r#"[{"x":0,"x":[]},?]"#] {
        compare(input.as_bytes());
    }
    assert!(matches!(error(br#"{"x":0,"x":?}"#).cause,Cause::StoreSyntax(_)));
    assert_eq!(error(br#"{"x":0,"x":-0}"#).cause,Cause::StoreFloatNotAllowed);
    assert_eq!(error(br#"{"x":0,"x":[]}"#).cause,Cause::StoreDuplicateObjectMember);
}
#[test]
fn store_custom_error_end_map_positions_are_exact() {
    for (input, expected) in [
        ("{\"x\":0,\"x\":1}", "duplicate JSON member at line 1 column 13"),
        ("{\"x\":0,\"x\":1 \n }", "duplicate JSON member at line 2 column 2"),
        ("{\"x\":0,\"x\":1 \n ,0}", "duplicate JSON member at line 2 column 1"),
        ("{\"x\":0,\"x\":1 \n ?}", "duplicate JSON member at line 2 column 1"),
        ("{\"x\":0,\"x\":1 \n ", "duplicate JSON member at line 2 column 1"),
        ("[\n -0 \n ]", "floating point values are not part of the store format at line 2 column 3"),
    ] {
        compare(input.as_bytes());
        assert_eq!(error(input.as_bytes()).to_string(),expected);
    }
    for input in ["[ {\"x\":0,\"x\":[]} \n ]", "{\"o\":{\"x\":0,\"x\":1} ,?}",
        "{\n\"中\":0,\n\"中\":1\r\n}", "{\"x\":0,\"x\":true}\n junk"] {
        compare(input.as_bytes());
    }
}
#[test]
fn store_recursion_boundary_remains_serde_not_proposal() {
    for depth in [0,1,63,64,65,126,127,128,129,192] {
        for scalar in ["0", "[]", "{}", "?", "-0"] {
            compare(format!("{}{}{}","[".repeat(depth),scalar,"]".repeat(depth)).as_bytes());
            compare(format!("{}{}{}","{\"k\":".repeat(depth),scalar,"}".repeat(depth)).as_bytes());
        }
        compare(format!("{}{}","[".repeat(depth)," ").as_bytes());
    }
    let accepted=format!("{}0{}","[".repeat(127),"]".repeat(127));
    assert!(parse(accepted.as_bytes(),&mut AllocationContext::new()).is_ok());
    let rejected=format!("{}0{}","[".repeat(128),"]".repeat(128));
    let e=error(rejected.as_bytes());
    assert_eq!(e.cause,Cause::StoreSyntax(SyntaxCode::RecursionLimitExceeded));
    assert_eq!((e.line,e.column),(1,128));
}
#[test]
fn store_input_bound_stays_with_caller_not_proposal_cap() {
    let input=format!("\"{}\"","x".repeat(product_json::MAX_INPUT_BYTES+1));
    compare(input.as_bytes());
    let proposal=product_json::parse_json_bytes(input.as_bytes(),&mut AllocationContext::new());
    assert!(matches!(proposal,Err(Error{cause:Cause::InputTooLarge,..})));
    let mut whitespace=vec![b' ';product_json::MAX_INPUT_BYTES+1];
    whitespace.push(b'?'); compare(&whitespace);
    assert_eq!(error(&whitespace).cause,Cause::StoreSyntax(SyntaxCode::ExpectedSomeValue));
}
#[test]
fn store_malformed_utf8_escapes_trailing_input_and_positions_match() {
    for input in [&b""[..],b" \r\n\t",b"[",b"{",b"[1,]",b"[1 2]",b"[1,",b"{x:1}",
        br#"{"x":1,}"#,br#"{"x"}"#,br#"{"x""y"}"#,br#"{"x":1 "y":2}"#,
        b"null false",b"true!",b"nul",b"nux",b"NaN",b"Infinity",b"\xef\xbb\xbfnull",
        b"\"\xff\"",b"\"\xc0\x80\"",b"\"\xed\xa0\x80\"",b"null\xff",b"\"\xff\\q\"",
        b"\"\xff\\u1234\"",b"\"\xff\n\"",b"\"\xff",b"\"\xc3\\n\xa9\"",
        br#""\q""#,br#""\u""#,br#""\u12""#,br#""\u12xz""#,br#""\uDC00""#,
        br#""\ud800""#,br#""\ud800x""#,br#""\ud800\x""#,br#""\ud800\u1234""#,
        br#""\ud800\uD800""#,br#""\ud800\u""#,b"\"\\",b"\"\\ud800",b"\"\\ud800\\",
        b"\"raw\nline\"",b"[\r\n 1,\n 2,\n ]",b"// comment\nnull",b"\x0b",b"\x0c"] {
        compare(input);
    }
    for byte in 0..=255u8 {
        compare(&[byte]); compare(&[b'"',byte,b'"']);
        compare(&[b'"',b'\\',byte,b'"']);
        compare(&[b'"',b'\\',b'u',b'0',byte,b'0',b'0',b'"']);
    }
}
fn next(seed:&mut u64)->u64 {
    *seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *seed
}
fn generated(seed:&mut u64,depth:u8)->String {
    match next(seed)%if depth==0 {5}else{7} {
        0=>"null".into(), 1=>(next(seed)&1==0).to_string(),
        2=>next(seed).to_string(),
        3=>[r#""中\u0000🚀""#,r#""\ud83d\ude80""#,r#""\"\\\n""#][next(seed) as usize%3].into(),
        4=>format!("{}e{}",next(seed),next(seed)%700),
        5=>format!("[{},{}]",generated(seed,depth-1),generated(seed,depth-1)),
        _=>format!("{{\"z\":{},\"a\":{}}}",generated(seed,depth-1),generated(seed,depth-1)),
    }
}
#[test]
fn store_bounded_generated_mutated_and_random_cases_match() {
    let mut seed=0x5240_4b27_2026u64;
    for _ in 0..1200 {
        let text=generated(&mut seed,4); assert!(text.len()<4096); compare(text.as_bytes());
        let mut bytes=text.into_bytes(); let i=next(&mut seed) as usize%bytes.len();
        bytes[i]=(next(&mut seed)>>32) as u8; compare(&bytes);
        bytes.truncate(next(&mut seed) as usize%bytes.len()); compare(&bytes);
    }
    const ALPHABET:&[u8]=b"{}[],:\"\\truefalsn0123456789.eE+- \r\n\t\x00\xff";
    for _ in 0..2000 {
        let mut bytes=[0u8;80]; let len=next(&mut seed) as usize%bytes.len();
        for b in &mut bytes[..len] { *b=ALPHABET[next(&mut seed) as usize%ALPHABET.len()]; }
        compare(&bytes[..len]);
    }
}
fn sweep(input:&[u8]) {
    compare(input);
    let mut normal=AllocationContext::new(); drop(parse(input,&mut normal));
    let requests=normal.growth_requests();
    for sticky in [false,true] {
        for nth in 1..=requests {
            let mut cx=AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(),sticky);
            let failure=match parse(input,&mut cx) { Err(e)=>e,Ok(_)=>panic!("refusal ignored") };
            let Cause::Allocation(a)=failure.cause else {panic!("refusal masked: {failure}")};
            assert_eq!(a.kind,AllocationFailureKind::Injected);
            assert_eq!(cx.growth_requests(),nth); assert_eq!(cx.first_failure(),Some(a));
            let later=crate::product_alloc::try_copy_str("later",&mut cx,AllocationSite::String);
            assert_eq!(later.is_err(),sticky); assert_eq!(cx.first_failure(),Some(a));
        }
    }
    let mut cx=AllocationContext::fail_nth_growth(NonZeroUsize::new(requests+1).unwrap(),true);
    assert!(!matches!(parse(input,&mut cx),Err(Error{cause:Cause::Allocation(_),..})));
    assert_eq!(cx.growth_requests(),requests);
}
#[test]
fn store_every_growth_sticky_failure_and_partial_cleanup_stop_immediately() {
    for input in [r#"null"#,r#"-0"#,r#""""#,r#""中🚀""#,r#""a\nb\u4e2d\ud83d\ude80""#,
        r#"["a",{"z":[0,1,"b"],"a":{"k":"v"}},null]"#,
        r#"{"z":1,"y":2,"x":3,"w":4,"v":5,"u":6,"t":7,"s":8}"#,
        r#"{"x":"first","x":"second"}"#,r#"{"x":"first","x":["owned",?]}"#,
        r#"{"x":"first","x":{"nested":"text","f":1.0}}"#,
        r#""a\nb\uDC00""#,r#"{"a":"text"} !"#] { sweep(input.as_bytes()); }
    sweep(format!("{}[\"leaf\",{{\"k\":\"value\"}}]{}","[\"owned\",".repeat(125),"]".repeat(125)).as_bytes());
    sweep(format!("{}?{}","{\"key\":[\"owned\",".repeat(63),"]}".repeat(63)).as_bytes());
}
struct Fixed {bytes:[u8;256],len:usize}
impl fmt::Write for Fixed {
    fn write_str(&mut self,text:&str)->fmt::Result {
        let end=self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let out=self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        out.copy_from_slice(text.as_bytes()); self.len=end; Ok(())
    }
}
#[test]
fn store_inline_causes_survive_refused_diagnostics() {
    for input in [&b"-0"[..],br#"{"x":1,"x":2}"#,b"[?"] {
        let original=error(input);
        let mut cx=AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(),true);
        let mut dynamic=String::new();
        assert!(crate::product_alloc::try_append_fmt(&mut dynamic,format_args!("{original}"),
            &mut cx,AllocationSite::Formatting).is_err());
        assert!(dynamic.is_empty());
        let mut fixed=Fixed{bytes:[0;256],len:0}; write!(&mut fixed,"{original}").unwrap();
        assert_eq!(&fixed.bytes[..fixed.len],original.to_string().as_bytes());
        assert_eq!(original,error(input));
    }
}
