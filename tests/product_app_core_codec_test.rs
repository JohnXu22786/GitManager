// Independent frozen Serde oracle is never part of an injected product operation.
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_json.rs"] mod product_json;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_app.rs"] mod product_app;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "support/app_codec_oracle.rs"] mod oracle;
use crate::product_alloc::{AllocationContext as Cx, AllocationFailure, AllocationSite as Site,
    AllocationFailureKind as FailureKind, FormatFailure, ordered::OrderedMap};
use crate::product_json::{self as json, Value, Number};
use crate::product_app::{self as app, InputMode as Mode, DecodeOwned};
use serde::{Deserialize, de::DeserializeOwned};
use std::cell::Cell;
use std::num::NonZeroUsize;

#[derive(Deserialize)]
#[serde(tag = "wrap", rename_all = "snake_case")]
enum Buffered<T> { Go { value: T } }
fn ok<T>(r: Result<T, app::AppDecodeError>) -> T {
    match r { Ok(v) => v, Err(e) => panic!("{}", e.try_render(&mut Cx::new()).unwrap()) }
}
fn bad<T>(r: Result<T, app::AppDecodeError>) -> app::AppDecodeError {
    match r { Err(e) => e, Ok(_) => panic!("expected schema/allocation error") }
}
static INJECTED_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn fault(n: usize, sticky: bool) -> Cx {
    let prior=INJECTED_CALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    assert!(prior<8192,"aggregate R05a injection admission cap exceeded; replan, never truncate");
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(), sticky)
}
fn value(input: &str) -> Value { json::parse_json_bytes(input.as_bytes(), &mut Cx::new()).unwrap() }
trait Case: DecodeOwned {
    type Old: DeserializeOwned;
    fn same(&self, old: &Self::Old);
}
fn same_type(a: &app::Type, b: &oracle::Type) {
    use app::Type as A; use oracle::Type as B;
    match (a,b) {
        (A::Boolean,B::Boolean)|(A::Integer,B::Integer)|(A::Text,B::Text)|(A::Date,B::Date)=>(),
        (A::Reference{entity:a},B::Reference{entity:b})=>assert_eq!(a,b),
        (A::List{item:a},B::List{item:b})|(A::Optional{item:a},B::Optional{item:b})=>same_type(a,b),
        _=>panic!("Type variant mismatch"),
    }
}
fn same_data(a: &app::DataValue, b: &oracle::DataValue) {
    use app::DataValue as A; use oracle::DataValue as B;
    match (a,b) {
        (A::Null,B::Null)=>(),
        (A::Boolean{value:a},B::Boolean{value:b})=>assert_eq!(a,b),
        (A::Integer{value:a},B::Integer{value:b})=>assert_eq!(a,b),
        (A::Text{value:a},B::Text{value:b})=>assert_eq!(a,b),
        (A::Date{days:a},B::Date{days:b})=>assert_eq!(a,b),
        (A::Reference{entity:a,record:c},B::Reference{entity:b,record:d})=>{assert_eq!(a,b);assert_eq!(c,d);},
        (A::List{item_type:a,items:c},B::List{item_type:b,items:d})=>{
            same_type(a,b);assert_eq!(c.len(),d.len());for(a,b)in c.iter().zip(d){same_data(a,b);}
        },
        _=>panic!("DataValue variant mismatch"),
    }
}
impl Case for app::Type { type Old=oracle::Type; fn same(&self,b:&Self::Old){same_type(self,b);} }
impl Case for app::DataValue { type Old=oracle::DataValue; fn same(&self,b:&Self::Old){same_data(self,b);} }
impl Case for app::FieldDefinition { type Old=oracle::FieldDefinition; fn same(&self,b:&Self::Old){
    assert_eq!(self.id,b.id);assert_eq!(self.label,b.label);same_type(&self.value_type,&b.value_type);
} }
impl Case for app::StateControl { type Old=oracle::StateControl; fn same(&self,b:&Self::Old){
    assert_eq!(self.id,b.id);assert_eq!(self.label,b.label);assert_eq!(self.state,b.state);assert_eq!(self.on_change,b.on_change);
} }
macro_rules! unit_case {
    ($t:ident { $($v:ident),+ }) => {
        impl Case for app::$t { type Old=oracle::$t; fn same(&self,b:&Self::Old){
            assert!(matches!((self,b),$((app::$t::$v,oracle::$t::$v))|+));
        } }
    };
}
unit_case!(CollectionOperation{Insert,Remove}); unit_case!(OutputFormat{Csv,Json});
unit_case!(ActionPlacement{Toolbar,Row}); unit_case!(KeyModifier{Control,Alt,Shift,Command});
impl Case for app::KeyBinding { type Old=oracle::KeyBinding; fn same(&self,b:&Self::Old){
    assert_eq!(self.key,b.key);assert_eq!(self.binding,b.binding);assert_eq!(self.modifiers.len(),b.modifiers.len());
    for(a,b)in self.modifiers.iter().zip(&b.modifiers){a.same(b);}
} }
macro_rules! scalar_case { ($($t:ty),+) => {$ (impl Case for $t {type Old=$t;fn same(&self,b:&Self::Old){assert_eq!(self,b);}})+}; }
scalar_case!(bool,String,i64,i32,u32,usize);
fn compare<T:Case>(input:&str,mode:Mode) {
    assert!(input.len()<=16384);
    let old:serde_json::Value=serde_json::from_str(input).unwrap();
    let expected:Result<T::Old,serde_json::Error>=match mode {
        Mode::DirectValue=>serde_json::from_value(old),
        Mode::BufferedContent=>serde_json::from_value::<Buffered<T::Old>>(serde_json::json!({"wrap":"go","value":old})).map(|Buffered::Go{value}|value),
    };
    let mut cx=Cx::new();
    let actual=app::decode_value::<T>(value(input),mode,&mut cx);
    match(actual,expected){
        (Ok(a),Ok(b))=>a.same(&b),
        (Err(a),Err(b))=>assert_eq!(a.try_render(&mut cx).unwrap(),b.to_string(),"{input}; {mode:?}"),
        (Err(a),Ok(_))=>panic!("unexpected error for {input}: {}",a.try_render(&mut cx).unwrap()),
        (Ok(_),Err(b))=>panic!("accepted {input} despite {b}"),
    }
}
fn both<T:Case>(inputs:&[&str]) {for input in inputs {for mode in [Mode::DirectValue,Mode::BufferedContent]{compare::<T>(input,mode);}}}

#[test]
fn all_twenty_four_variants_maps_sequences_and_payloads() {
    both::<app::Type>(&[r#"{"kind":"boolean"}"#,r#"["boolean"]"#,r#"{"kind":"integer"}"#,r#"["integer"]"#,
        r#"{"kind":"text"}"#,r#"["text"]"#,r#"{"kind":"date"}"#,r#"["date"]"#,
        r#"{"kind":"reference","entity":"e"}"#,r#"["reference","e"]"#,
        r#"{"kind":"list","item":{"kind":"text"}}"#,r#"["list",["text"]]"#,
        r#"{"kind":"optional","item":{"kind":"boolean"}}"#,r#"["optional",["boolean"]]"#]);
    both::<app::DataValue>(&[r#"{"kind":"null"}"#,r#"["null"]"#,
        r#"{"kind":"boolean","value":true}"#,r#"["boolean",false]"#,
        r#"{"kind":"integer","value":-2}"#,r#"["integer",3]"#,
        r#"{"kind":"text","value":"中\u0000`\""}"#,r#"["text","é"]"#,
        r#"{"kind":"date","days":4}"#,r#"["date",-5]"#,
        r#"{"kind":"reference","entity":"e","record":"r"}"#,r#"["reference","e","r"]"#,
        r#"{"kind":"list","item_type":{"kind":"integer"},"items":[{"kind":"integer","value":7}]}"#,
        r#"["list",["text"],[["text","s"],["null"]]]"#]);
    both::<app::CollectionOperation>(&[r#""insert""#,r#"{"insert":null}"#,r#""remove""#,r#"{"remove":null}"#]);
    both::<app::OutputFormat>(&[r#""csv""#,r#"{"csv":null}"#,r#""json""#,r#"{"json":null}"#]);
    both::<app::ActionPlacement>(&[r#""toolbar""#,r#"{"toolbar":null}"#,r#""row""#,r#"{"row":null}"#]);
    both::<app::KeyModifier>(&[r#""control""#,r#"{"control":null}"#,r#""alt""#,r#"{"alt":null}"#,
        r#""shift""#,r#"{"shift":null}"#,r#""command""#,r#"{"command":null}"#]);
}
#[test]
fn direct_buffered_numeric_internal_tags_and_error_priority() {
    both::<app::Type>(&[r#"{"kind":0}"#,r#"[0]"#,r#"{"kind":6,"item":{"kind":0}}"#,
        r#"{"kind":7}"#,r#"{"kind":18446744073709551615}"#,r#"{"kind":-1}"#,r#"{"kind":1.0}"#,
        r#"{"kind":-0.0}"#,r#"{"kind":true}"#,r#"{"kind":null}"#,r#"{"kind":[]}"#,r#"{"kind":{}}"#,
        r#"{"kind":"list","item":{"kind":0}}"#,r#"["list",[6,[0]]]"#,
        r#"{"aaa":false,"kind":"nonesuch","zzz":true}"#,r#"{"aaa":false}"#,r#"[]"#,
        r#"{"kind":"list","aaa":0,"item":{"kind":"boolean"}}"#,
        r#"{"item":false,"kind":"list","zzz":0}"#,r#"{"kind":"boolean","garbage":[1,2]}"#,
        r#"["boolean",1]"#,r#"["reference","e",2]"#,r#"["reference",false,2]"#,r#"["reference"]"#,r#"["list"]"#]);
    both::<app::DataValue>(&[r#"{"kind":6,"item_type":{"kind":2},"items":[{"kind":3,"value":"x"}]}"#,
        r#"{"kind":"reference"}"#,r#"{"kind":"list","items":[]}"#,r#"["list",["text"]]"#,
        r#"{"kind":"null","bogus":0}"#,r#"["null",0]"#]);
}
#[test]
fn external_unit_payload_modes_and_map_cardinality_precedence() {
    both::<app::KeyModifier>(&[r#"{"control":{}}"#,r#"{"control":{"a":0}}"#,r#"{"control":[]}"#,
        r#"{"control":0}"#,r#"{"control":false}"#,r#"{}"#,r#"{"z":0,"control":null}"#,
        r#""wat`\n""#,r#"0"#,r#"[]"#,r#"null"#,r#"true"#]);
    both::<app::OutputFormat>(&[r#""other""#,r#"{"csv":{}}"#,r#"{"csv":"x"}"#]);
    both::<app::CollectionOperation>(&[r#""other""#,r#"{"insert":{}}"#]);
    both::<app::ActionPlacement>(&[r#""other""#,r#"{"row":{}}"#]);
}
#[test]
fn struct_order_missing_option_and_sequence_error_parity() {
    both::<app::FieldDefinition>(&[r#"{"id":"i","label":"l","value_type":{"kind":"text"}}"#,
        r#"["i","l",["text"]]"#,r#"{}"#,r#"{"label":false}"#,r#"{"z":0,"a":0}"#,
        r#"{"id":false,"a":0}"#,r#"{"id":false,"z":0}"#,r#"[]"#,r#"["i"]"#,
        r#"["i","l"]"#,r#"["i","l",["text"],0]"#,r#"[false,"l",["text"],0]"#,r#"null"#]);
    both::<app::StateControl>(&[r#"{"id":"i","label":"l","state":"s"}"#,
        r#"{"id":"i","label":"l","state":"s","on_change":null}"#,
        r#"{"id":"i","label":"l","state":"s","on_change":"c"}"#,
        r#"["i","l","s",null]"#,r#"["i","l","s","c"]"#,r#"["i","l","s"]"#,
        r#"["i","l","s",null,0]"#,r#"{"on_change":false}"#,r#"{}"#,r#"[]"#]);
}
#[test]
fn numeric_endpoints_overflow_float_and_wrong_scalar_kinds() {
    let integers=["-9223372036854775808","9223372036854775807","9223372036854775808","18446744073709551615",
        "-2147483649","-2147483648","2147483647","2147483648","4294967295","4294967296","-1","0","1.0","-0.0"];
    for s in integers {both::<i64>(&[s]);both::<i32>(&[s]);both::<u32>(&[s]);both::<usize>(&[s]);}
    for s in ["null","false",r#""中\n\u0001\"`""#,"[]","{}"] {
        both::<i64>(&[s]);both::<i32>(&[s]);both::<u32>(&[s]);both::<usize>(&[s]);both::<bool>(&[s]);both::<String>(&[s]);
    }
    both::<String>(&["1.0","-0.0","1e30","1e-30"]);
    // Exact JSON diagnostic backend, including bit patterns not expressible as JSON.
    // 32 fixed boundaries + 96 deterministic raw bits, each in both modes = 256.
    // Together with the nested corpus: 304 generated cases, <=512; scalar depth/nodes 1.
    let fixed:[f64;32]=[0.0,-0.0,1.0,-1.0,f64::from_bits(1),-f64::from_bits(1),
        f64::from_bits(0x000f_ffff_ffff_ffff),-f64::from_bits(0x000f_ffff_ffff_ffff),
        f64::MIN_POSITIVE,-f64::MIN_POSITIVE,f64::MAX,-f64::MAX,1e-6,1e-5,1e15,1e16,
        -1e-6,-1e-5,-1e15,-1e16,1e30,1e-30,0.1,-0.1,f64::INFINITY,f64::NEG_INFINITY,
        f64::NAN,f64::from_bits(0xfff8_0000_0000_0001),f64::from_bits(0x7ff0_0000_0000_0001),
        f64::from_bits(0x3fef_ffff_ffff_ffff),f64::from_bits(0x3ff0_0000_0000_0001),1.2345678901234567];
    let mut bits=0x4d59_5df4_d0f3_3173u64;
    let mut cases=0;
    for i in 0..128 {
        let v=if i<fixed.len(){fixed[i]}else{bits=bits.wrapping_mul(6364136223846793005).wrapping_add(1);f64::from_bits(bits)};
        // Oracle construction and storage preparation precede the injected operation.
        let expected=<serde_json::Error as serde::de::Error>::invalid_type(serde::de::Unexpected::Float(v),&"a string").to_string();
        for mode in [Mode::DirectValue,Mode::BufferedContent] {
            let mut text=String::with_capacity(256);let mut cx=fault(1,true);
            let e=bad(app::decode_value::<String>(Value::Number(Number::F64(v)),mode,&mut cx));
            assert_eq!(cx.growth_requests(),0);
            e.try_append_diagnostic(&mut text,&mut cx).unwrap();
            assert_eq!(text,expected,"bits {:016x}; {mode:?}",v.to_bits());
            assert_eq!(cx.growth_requests(),0);
            assert!(matches!(e.try_render(&mut cx),Err(FormatFailure::Allocation(f))if f.site==Site::Formatting&&Some(f)==cx.first_failure()));
            assert_eq!(cx.growth_requests(),1);
            assert!(matches!(&e,app::AppDecodeError::Schema(app::SchemaFailure::InvalidType{actual:app::UnexpectedOwned::F64(n),..})if n.to_bits()==v.to_bits()));
            cases+=1;
        }
    }
    assert_eq!(cases,256);assert!(cases+48<=512);
    assert_eq!(ok(app::decode_value::<usize>(value("4294967295"),Mode::DirectValue,&mut Cx::new())) as u64,4294967295);
    if usize::BITS==32 {assert!(app::decode_value::<usize>(value("4294967296"),Mode::DirectValue,&mut Cx::new()).is_err());}
    else {assert_eq!(ok(app::decode_value::<usize>(value("4294967296"),Mode::DirectValue,&mut Cx::new())) as u64,4294967296);}
    both::<app::DataValue>(&[r#"["integer",9223372036854775808]"#,r#"["date",2147483648]"#,r#"["integer",1.0]"#]);
}
#[test]
fn key_modifier_set_order_duplicates_and_later_invalid_member() {
    both::<app::KeyBinding>(&[r#"{"key":"x","binding":"b","modifiers":[]}"#,
        r#"["x",["command","shift","control","alt","control"],"b"]"#,
        r#"["x",["control","control","bad"],"b"]"#,r#"["x",["control",{"alt":{}}],"b"]"#,
        r#"{"key":"x","binding":"b"}"#,r#"["x",false,"b"]"#]);
    let b=ok(app::KeyBinding::decode_json(br#"["x",["command","shift","control","alt"],"b"]"#,&mut Cx::new()));
    assert_eq!(b.modifiers.iter().copied().collect::<Vec<_>>(),[app::KeyModifier::Control,app::KeyModifier::Alt,app::KeyModifier::Shift,app::KeyModifier::Command]);
}
#[test]
fn bounded_generated_nested_and_malformed_corpus() {
    // Finite declared construction: 8 depths, 3 shapes, 2 modes = 48 generated cases.
    let mut n=0;
    for depth in 0..8 {
        let mut v=serde_json::json!({"kind":"text"});
        for _ in 0..depth {v=serde_json::json!({"kind":"list","item":v});}
        for variant in 0..3 {
            let mut v=v.clone();
            if variant==1 {v.as_object_mut().unwrap().insert("aaa".into(),serde_json::json!(false));}
            if variant==2 {v.as_object_mut().unwrap().insert("kind".into(),serde_json::json!(7));}
            let s=serde_json::to_string(&v).unwrap();assert!(s.len()<=16384);assert!(depth+1<=8);
            for mode in [Mode::DirectValue,Mode::BufferedContent]{compare::<app::Type>(&s,mode);n+=1;}
        }
    }
    assert_eq!(n,48);assert!(n<=512);
}
#[test]
fn strings_vec_and_pairs_move_without_growth() {
    let mut input=String::with_capacity(103);input.push_str("owned 中");let ptr=input.as_ptr();let cap=input.capacity();
    let mut cx=fault(1,true);let out=ok(app::decode_value::<String>(Value::String(input),Mode::DirectValue,&mut cx));
    assert_eq!((out.as_ptr(),out.capacity()),(ptr,cap));assert_eq!(cx.growth_requests(),0);
    let mut m=OrderedMap::new();let mut k=String::with_capacity(90);k.push_str("entity");let kp=k.as_ptr();
    let mut v=String::with_capacity(94);v.push_str("e");let vp=v.as_ptr();let vc=v.capacity();
    m.try_insert(k,Value::String(v),&mut Cx::new()).unwrap();
    m.try_insert("kind".into(),Value::String("reference".into()),&mut Cx::new()).unwrap();
    let out=ok(app::decode_value::<app::Type>(Value::Object(m),Mode::DirectValue,&mut cx));
    match out {app::Type::Reference{entity}=>assert_eq!((entity.as_ptr(),entity.capacity()),(vp,vc)),_=>panic!()}
    let mut m=OrderedMap::new();let k=String::from("key");let p=k.as_ptr();m.try_insert(k,9,&mut Cx::new()).unwrap();
    let mut it=m.into_entries();assert_eq!(it.len(),1);let(k,v)=it.next().unwrap();assert_eq!(k.as_ptr(),p);assert_eq!(v,9);
    assert_ne!(kp,std::ptr::null());assert_eq!(cx.growth_requests(),0);
    // Array payload iterator is moved, not rebuilt; empty typed Vec needs no allocation.
    let v=Value::Array(vec![Value::String("list".into()),value(r#"{"kind":"text"}"#),Value::Array(Vec::new())]);
    assert!(matches!(ok(app::decode_value::<app::DataValue>(v,Mode::DirectValue,&mut cx)),app::DataValue::List{items,..} if items.capacity()==0));
    assert_eq!(cx.growth_requests(),0);
}
#[test]
fn consumed_ordered_iterator_partial_drop_exactly_once() {
    struct Spy<'a>(&'a Cell<usize>);impl Drop for Spy<'_>{fn drop(&mut self){self.0.set(self.0.get()+1);}}
    let drops=Cell::new(0);let mut m=OrderedMap::new();let mut cx=Cx::new();
    for key in [3,1,2]{m.try_insert(key,Spy(&drops),&mut cx).unwrap();}
    let n=cx.growth_requests();let mut it=m.into_entries();assert_eq!(it.len(),3);
    let (k,v)=it.next().unwrap();assert_eq!(k,1);assert_eq!(drops.get(),0);drop(v);assert_eq!(drops.get(),1);
    let(k,v)=it.next_back().unwrap();assert_eq!(k,3);drop(v);assert_eq!(drops.get(),2);drop(it);assert_eq!(drops.get(),3);
    assert_eq!(cx.growth_requests(),n);
}
#[test]
fn schema_errors_own_offending_strings_without_growth() {
    for mode in [Mode::DirectValue,Mode::BufferedContent] {
        let mut s=String::with_capacity(80);s.push_str("unknown`\n中");let p=s.as_ptr();let c=s.capacity();
        let mut cx=fault(1,true);let e=bad(app::decode_value::<app::KeyModifier>(Value::String(s),mode,&mut cx));
        match &e {app::AppDecodeError::Schema(app::SchemaFailure::UnknownVariant{tag,..})=>assert_eq!((tag.as_ptr(),tag.capacity()),(p,c)),_=>panic!()}
        assert_eq!(cx.growth_requests(),0);drop(e);assert_eq!(cx.growth_requests(),0);
        let s=String::from("bad scalar\n中");let ptr=s.as_ptr();let cap=s.capacity();
        let e=bad(app::decode_value::<bool>(Value::String(s),mode,&mut cx));
        assert!(matches!(&e,app::AppDecodeError::Schema(app::SchemaFailure::InvalidType{actual:app::UnexpectedOwned::String(s),..})if(s.as_ptr(),s.capacity())==(ptr,cap)));
        let key=String::from("aaa\nunknown");let ptr=key.as_ptr();let cap=key.capacity();let mut map=OrderedMap::new();map.try_insert(key,Value::Null,&mut Cx::new()).unwrap();
        let e=bad(app::decode_value::<app::FieldDefinition>(Value::Object(map),mode,&mut cx));
        assert!(matches!(&e,app::AppDecodeError::Schema(app::SchemaFailure::UnknownField{key,..})if(key.as_ptr(),key.capacity())==(ptr,cap)));
        assert_eq!(cx.growth_requests(),0);

        let mut cx=fault(1,true);let e=bad(app::decode_value::<app::FieldDefinition>(value(r#"{"id":"ok","label":false}"#),mode,&mut cx));
        assert!(matches!(e,app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),0);
    }
}
const SWEEP_TYPE:&str=r#"{"kind":"list","item":{"kind":"optional","item":{"kind":"reference","entity":"e"}}}"#;
const SWEEP_DATA:&str=r#"{"kind":"list","item_type":{"kind":"list","item":{"kind":"text"}},"items":[{"kind":"text","value":"first"},{"kind":"list","item_type":{"kind":"text"},"items":[{"kind":"text","value":"second"}]}]}"#;
const SWEEP_KEYS:&str=r#"{"key":"x","binding":"b","modifiers":["shift","control","alt","command","control"]}"#;
fn failure(e:app::AppDecodeError)->AllocationFailure {match e{
    app::AppDecodeError::Allocation(f)=>f,app::AppDecodeError::Json(json::Error{cause:json::Cause::Allocation(f),..})=>f,
    _=>panic!("allocation cause replaced"),
}}
fn sweep<T:DecodeOwned>(s:&str,parse:bool,total:&mut usize,trace:&[(Site,usize)]){
    assert!(s.len()<=4096);
    for mode in [Mode::DirectValue,Mode::BufferedContent]{
        let run=|prepared:Option<Value>,cx:&mut Cx|{let v=match prepared{Some(v)=>v,None=>json::parse_json_bytes(s.as_bytes(),cx).map_err(app::AppDecodeError::Json)?};app::decode_value::<T>(v,mode,cx)};
        let prepare=||if parse{None}else{Some(value(s))};
        let prepared=prepare();let mut full=Cx::new();drop(ok(run(prepared,&mut full)));let count=full.growth_requests();assert!(count>0&&count<=512);
        let mut parser=Cx::new();if parse{drop(json::parse_json_bytes(s.as_bytes(),&mut parser).unwrap());}
        let prefix=parser.growth_requests();assert_eq!(count,prefix+trace.len());
        for sticky in [false,true]{for n in 1..=count{
            let before=s.as_bytes().to_vec();let prepared=prepare();let mut cx=fault(n,sticky);let e=bad(run(prepared,&mut cx));
            if n<=prefix{assert!(matches!(&e,app::AppDecodeError::Json(json::Error{cause:json::Cause::Allocation(_),..})));}
            else{assert!(matches!(&e,app::AppDecodeError::Allocation(_)));}
            let f=failure(e);
            if n>prefix{let(site,bytes)=trace[n-prefix-1];assert_eq!(f.site,site);assert_eq!(f.requested_bytes,Some(bytes));}
            assert_eq!(f.kind,FailureKind::Injected);assert_eq!(cx.first_failure(),Some(f));assert_eq!(cx.growth_requests(),n);
            assert_eq!(s.as_bytes(),before);assert!(f.requested_bytes.is_some());*total+=1;assert!(*total<=8192);
        }}
        let prepared=prepare();let mut after=fault(count+1,true);let out=ok(run(prepared,&mut after));assert_eq!(after.growth_requests(),count);
        let mut refused_text=String::new();assert!(crate::product_alloc::try_append_str(&mut refused_text,"x",&mut after,Site::String).is_err());
        drop(out);assert_eq!(after.growth_requests(),count+1);
    }
}
fn sweep_all(parse:bool){
    use std::mem::size_of;
    let mut n=0;
    sweep::<app::Type>(SWEEP_TYPE,parse,&mut n,&[(Site::Box,size_of::<app::Type>());2]);
    sweep::<app::DataValue>(SWEEP_DATA,parse,&mut n,&[(Site::Box,size_of::<app::Type>()),(Site::Vector,2*size_of::<app::DataValue>()),(Site::Vector,size_of::<app::DataValue>())]);
    let set_slot=size_of::<(app::KeyModifier,())>();
    sweep::<app::KeyBinding>(SWEEP_KEYS,parse,&mut n,&[(Site::OrderedSet,set_slot),(Site::OrderedSet,2*set_slot),(Site::OrderedSet,3*set_slot),(Site::OrderedSet,4*set_slot)]);
    assert!(n>0&&n<=8192);
}
#[test]
fn finite_full_parse_decode_fault_sweeps_same_context(){sweep_all(true);}
#[test]
fn finite_decode_only_fault_sweeps_boxes_vectors_and_sets(){sweep_all(false);}
#[test]
fn primary_schema_cause_survives_every_render_refusal_and_rollback(){
    for s in [r#"{"id":"i","label":"l","unknown`\n中":0}"#,r#"{"id":"i","label":1.0}"#]{
        let mut full=Cx::new();let e=bad(app::decode_value::<app::FieldDefinition>(value(s),Mode::DirectValue,&mut full));
        assert_eq!(full.growth_requests(),0);let expected=e.try_render(&mut full).unwrap();let count=full.growth_requests();assert!(count>0&&count<=512);
        for sticky in [false,true]{for n in 1..=count{
            let mut cx=fault(n,sticky);let failure=e.try_render(&mut cx).unwrap_err();
            assert!(matches!(failure,FormatFailure::Allocation(f) if f.site==Site::Formatting&&Some(f)==cx.first_failure()));
            assert_eq!(cx.growth_requests(),n);assert_eq!(e.try_render(&mut Cx::new()).unwrap(),expected);
        }}
        let mut text=String::from("unchanged:");let mut cx=fault(1,true);
        assert!(e.try_append_diagnostic(&mut text,&mut cx).is_err());assert_eq!(text,"unchanged:");
        // Primary schema + rendering share the same ordinal stream as parse.
        let mut baseline=Cx::new();let primary=bad(app::FieldDefinition::decode_json(s.as_bytes(),&mut baseline));
        let parsed=baseline.growth_requests();let expected=primary.try_render(&mut baseline).unwrap();let all=baseline.growth_requests();assert!(all<=512);
        for sticky in [false,true]{for n in 1..=all{
            let mut cx=fault(n,sticky);let primary=bad(app::FieldDefinition::decode_json(s.as_bytes(),&mut cx));
            if n<=parsed {let f=failure(primary);assert_eq!(Some(f),cx.first_failure());}
            else {
                assert!(matches!(primary,app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),parsed);
                assert!(matches!(primary.try_render(&mut cx),Err(FormatFailure::Allocation(_))));
                assert_eq!(primary.try_render(&mut Cx::new()).unwrap(),expected);
            }
            assert_eq!(cx.growth_requests(),n);
        }}
        struct Fixed{bytes:[u8;1024],len:usize}
        impl std::fmt::Write for Fixed{fn write_str(&mut self,s:&str)->std::fmt::Result{let end=self.len+s.len();if end>self.bytes.len(){return Err(std::fmt::Error)}self.bytes[self.len..end].copy_from_slice(s.as_bytes());self.len=end;Ok(())}}
        let mut fixed=Fixed{bytes:[0;1024],len:0};
        if let app::AppDecodeError::Schema(schema)=&e {use std::fmt::Write;write!(&mut fixed,"{schema}").unwrap();}else{panic!()}
        assert_eq!(&fixed.bytes[..fixed.len],expected.as_bytes());

    }
}
#[test]
fn parser_limits_are_real_fixed_cases_and_errors_remain_typed(){
    let mut s=String::from("\"");s.extend(std::iter::repeat_n('x',json::MAX_INPUT_BYTES-2));s.push('"');
    assert_eq!(s.len(),json::MAX_INPUT_BYTES);let out=ok(app::decode_value::<String>(value(&s),Mode::DirectValue,&mut Cx::new()));assert_eq!(out.len(),s.len()-2);
    s.push(' ');let mut cx=fault(1,true);let e=bad(app::Type::decode_json(s.as_bytes(),&mut cx));assert!(matches!(e,app::AppDecodeError::Json(json::Error{cause:json::Cause::InputTooLarge,..})));assert_eq!(cx.growth_requests(),0);
    let mut s=String::from(r#"{"kind":"text"}"#);for _ in 1..64{s=format!(r#"{{"kind":"list","item":{s}}}"#);}
    drop(ok(app::Type::decode_json(s.as_bytes(),&mut Cx::new())));
    s=format!(r#"{{"kind":"list","item":{s}}}"#);let e=bad(app::Type::decode_json(s.as_bytes(),&mut Cx::new()));assert!(matches!(e,app::AppDecodeError::Json(json::Error{cause:json::Cause::NestingTooDeep,..})));
    let e=bad(app::Type::decode_json(br#"{"kind":"text","kind":"text"}"#,&mut Cx::new()));assert!(matches!(e,app::AppDecodeError::Json(json::Error{cause:json::Cause::DuplicateObjectMember,..})));
}
