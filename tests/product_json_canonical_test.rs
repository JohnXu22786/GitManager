// New R06a fault/case budgets are independent of inherited R05a8192/304.
#[cfg(not(canonical_aggregate))]
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[cfg(not(canonical_aggregate))]
#[path = "../src/product_json.rs"] mod product_json;
#[path = "support/canonical_json_native_oracle.rs"] mod oracle;
use crate::product_alloc::{ordered::OrderedMap, AllocationContext as Cx,
    AllocationFailureKind, AllocationSite};
use crate::product_json::{Number, Value, canonical::{try_canonical_bytes, EncodeError}};
use std::fmt::{self, Write};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
static FAULT_CALLS: AtomicUsize = AtomicUsize::new(0);
const MAX_FAULT_CALLS: usize = 128;
const NUMERIC_BITS_CASES: usize = 8192;
const GENERATED_TREES: usize = 128;
fn fault(n: usize) -> Cx {
    assert!(FAULT_CALLS.fetch_add(1, Ordering::Relaxed) < MAX_FAULT_CALLS);
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), true)
}
fn encode(value: &Value) -> Vec<u8> { try_canonical_bytes(value, &mut Cx::new()).unwrap() }
fn object(entries: Vec<(&str, Value)>) -> Value {
    let mut map = OrderedMap::new();
    for (k, v) in entries { map.try_insert(k.into(), v, &mut Cx::new()).unwrap(); }
    Value::Object(map)
}
// Bounded shallow fixture conversion only, before any injected operation.
fn native(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(v) => (*v).into(), Value::String(v) => v.clone().into(),
        Value::Number(Number::U64(v)) => (*v).into(),
        Value::Number(Number::I64(v)) => (*v).into(),
        Value::Number(Number::F64(v)) => (*v).into(),
        Value::Array(a) => a.iter().map(native).collect(),
        Value::Object(m) => m.iter().map(|(k,v)| (k.clone(),native(v))).collect(),
    }
}
fn compare(value: &Value) {
    let independent = native(value);
    let expected = oracle::canonical_bytes(&independent).unwrap();
    assert_eq!(encode(value), expected);
}
#[test]
fn canonical_scalars_empty_nested_and_lexical_order() {
    for (v, bytes) in [(Value::Null,b"null".as_slice()), (Value::Bool(true),b"true"),
        (Value::Bool(false),b"false"), (Value::Array(vec![]),b"[]"),
        (object(vec![]),b"{}"), (Value::String(String::new()),b"\"\"")] {
        assert_eq!(encode(&v),bytes); compare(&v);
    }
    let value = object(vec![("z",Value::Null),("a",Value::Array(vec![
        object(vec![("z",Value::Bool(false)),("a",Value::Bool(true))]),Value::Null]))]);
    assert_eq!(encode(&value),br#"{"a":[{"a":true,"z":false},null],"z":null}"#);
    compare(&value);
}
#[test]
fn canonical_every_escape_and_unicode_key_bytes() {
    let all: String = (0u8..=31).map(char::from).collect();
    let v=Value::String(all.clone());
    assert_eq!(encode(&v),br#""\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e\u000f\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f""#);
    compare(&v);
    let raw="\"\\/\u{7f}é e\u{301} 中 🚀 \u{2028}\u{2029}<>&'";
    compare(&Value::String(raw.into()));
    let value=object(vec![("🚀",Value::Null),("中",Value::Null),("é",Value::Null),
        ("e\u{301}",Value::Null),("aa",Value::Null),("a",Value::Null),("",Value::Null),
        (&all,Value::String(raw.into()))]);
    compare(&value);
    let keys=object(vec![("é",Value::Null),("e\u{301}",Value::Null),("aa",Value::Null),
        ("a",Value::Null),("",Value::Null)]);
    assert_eq!(encode(&keys),"{\"\":null,\"a\":null,\"aa\":null,\"e\u{301}\":null,\"é\":null}".as_bytes());
}
#[test]
fn canonical_integer_float_boundaries_and_nonfinite() {
    for n in [0,1,9,10,u64::MAX,i64::MAX as u64,9007199254740993] {
        compare(&Value::Number(Number::U64(n)));
    }
    for n in [i64::MIN,i64::MAX,-1,-10,0,1] { compare(&Value::Number(Number::I64(n))); }
    for n in [0.0,-0.0,1.0,-1.0,1e-7,1e-6,1e-5,1e15,1e16,1e20,1e21,
        f64::MAX,f64::MIN,f64::MIN_POSITIVE,f64::from_bits(1),f64::from_bits(0x000f_ffff_ffff_ffff),
        f64::INFINITY,f64::NEG_INFINITY,f64::NAN,f64::from_bits(0xfff8_0000_0000_0001)] {
        compare(&Value::Number(Number::F64(n)));
        // Original generic f64 API itself, not just a normalized Value.
        assert_eq!(encode(&Value::Number(Number::F64(n))),oracle::canonical_bytes(&n).unwrap());
        if n.is_finite() && n != 0.0 {
            for bits in [n.to_bits()-1,n.to_bits()+1] { compare(&Value::Number(Number::F64(f64::from_bits(bits)))); }
        }
    }
    assert_eq!(encode(&Value::Number(Number::F64(-0.0))),b"-0.0");
    assert_eq!(encode(&Value::Number(Number::F64(f64::INFINITY))),b"null");
}
fn next(seed: &mut u64) -> u64 { *seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *seed }
fn generated(seed: &mut u64, depth: u8) -> Value {
    match next(seed)%if depth==0 {5} else {7} {
        0=>Value::Null,1=>Value::Bool(next(seed)&1==0),
        2=>Value::Number(Number::F64(f64::from_bits(next(seed)))),
        3=>Value::Number(Number::U64(next(seed))),4=>Value::String("中\0🚀\\\n".into()),
        5=>Value::Array(vec![generated(seed,depth-1),generated(seed,depth-1)]),
        _=>object(vec![("z",generated(seed,depth-1)),("a",generated(seed,depth-1))]),
    }
}
#[test]
fn canonical_bounded_numeric_bits_and_generated_trees() {
    let mut seed=0x06a0_2026_1027;
    for _ in 0..NUMERIC_BITS_CASES { compare(&Value::Number(Number::F64(f64::from_bits(next(&mut seed))))); }
    for _ in 0..GENERATED_TREES { let v=generated(&mut seed,4); assert!(encode(&v).len()<4096); compare(&v); }
}
fn sweep(value: &Value, expected: &[u8], has_workspace: bool) {
    let mut cx=Cx::new(); assert_eq!(try_canonical_bytes(value,&mut cx).unwrap(),expected);
    let requests=cx.growth_requests(); assert!((1..=32).contains(&requests));
    assert_eq!(requests>1,has_workspace);
    for nth in 1..=requests {
        let mut cx=fault(nth);
        let failure=match try_canonical_bytes(value,&mut cx) {
            Err(EncodeError::Allocation(e))=>e, _=>panic!("expected original allocation failure"),
        };
        assert_eq!(failure.kind,AllocationFailureKind::Injected);
        assert_eq!(failure.site,if nth==requests {AllocationSite::Bytes} else {AllocationSite::Vector});
        if nth==requests { assert_eq!(failure.requested_bytes,Some(expected.len())); }
        assert_eq!(cx.first_failure(),Some(failure)); assert_eq!(cx.growth_requests(),nth);
        assert_eq!(encode(value),expected,"source survives refusal and same-source retry succeeds");
    }
}
#[test]
fn canonical_each_workspace_and_output_refusal_preserves_source() {
    let value=object(vec![("z",Value::String("é\n".repeat(32))),
        ("a",Value::Array(vec![object(vec![("x",Value::Array(vec![Value::Null]))])]))]);
    let expected=oracle::canonical_bytes(&native(&value)).unwrap();
    sweep(&value,&expected,true); compare(&value);
    let scalar=Value::String("\0é\"".into());
    sweep(&scalar,&oracle::canonical_bytes(&native(&scalar)).unwrap(),false);
}
#[test]
fn canonical_wide_map_long_string_single_output_request() {
    let mut m=OrderedMap::new();
    for n in (0..1024).rev() { m.try_insert(format!("key{n:04}"),Value::Bool(true),&mut Cx::new()).unwrap(); }
    let value=Value::Object(m); let mut cx=Cx::new();
    let encoded=try_canonical_bytes(&value,&mut cx).unwrap();
    assert_eq!(encoded,oracle::canonical_bytes(&native(&value)).unwrap());
    assert_eq!(cx.growth_requests(),2,"one frame and one output request, not one per member");
    let value=Value::String("é\0\\".repeat(16384)); let mut cx=Cx::new();
    assert_eq!(try_canonical_bytes(&value,&mut cx).unwrap(),oracle::canonical_bytes(&native(&value)).unwrap());
    assert_eq!(cx.growth_requests(),1,"one output request, not one per byte");
}
// A single-child chain. Drop also runs on assertion unwind and
// dismantles each owner before moving to its only child. No leak/forget or
// claim that general owned Value destruction has become nonrecursive.
struct DeepOwner(Option<Value>);
impl Drop for DeepOwner {
    fn drop(&mut self) {
        let mut current=self.0.take();
        while let Some(v)=current {
            current=match v {
                Value::Array(mut a)=>a.pop(),
                Value::Object(m)=>m.into_entries().next().map(|(_,v)|v),
                _=>None,
            };
        }
    }
}
#[test]
fn canonical_deep_borrowed_chain_with_iterative_owner_cleanup() {
    const DEPTH:usize=4096;
    let mut owner=DeepOwner(Some(Value::Null));
    for _ in 0..DEPTH {
        // Allocate the next shell before removing the child from its guard.
        let mut shell=Vec::with_capacity(1);
        shell.push(owner.0.take().unwrap());
        owner.0=Some(Value::Array(shell));
    }
    let mut expected=Vec::new();
    for _ in 0..DEPTH { expected.push(b'['); }
    expected.extend_from_slice(b"null");
    for _ in 0..DEPTH { expected.push(b']'); }
    // No recursive oracle/native conversion is used on the deep source.
    let value=owner.0.as_ref().unwrap(); sweep(value,&expected,true);
    let mut cx=Cx::new(); assert_eq!(try_canonical_bytes(value,&mut cx).unwrap(),expected);
    assert!(cx.growth_requests()<=15,"geometric frame storage plus exactly one output request");
}
struct Fixed { data:[u8;128], len:usize }
impl Write for Fixed {
    fn write_str(&mut self,s:&str)->fmt::Result {
        let end=self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.data.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes()); self.len=end; Ok(())
    }
}
#[test]
fn canonical_inline_error_display_after_refusal() {
    let mut cx=fault(1); let error=try_canonical_bytes(&Value::Null,&mut cx).unwrap_err();
    let mut fixed=Fixed{data:[0;128],len:0}; write!(&mut fixed,"{error}").unwrap();
    assert_eq!(&fixed.data[..fixed.len],b"allocation failure (injected) at bytes: 4 bytes");
    assert_eq!(cx.growth_requests(),1);
}
