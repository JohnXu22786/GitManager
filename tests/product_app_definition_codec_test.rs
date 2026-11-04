// R05c test-only extension of the frozen R05b suite. The aggregate nests this
// file in that suite; standalone integration-test builds include it once here.
// Private borrowed walkers and the ONE System observer are reused, not copied.
#[cfg(not(app_codec_aggregate))]
include!("product_app_expr_codec_test.rs");

mod definition {
use super::{app,oracle,Same,Buffered,ok,bad,value,bounds,failure,
    WatchScope,WATCH_LOCK,WATCH_PTRS,WATCH_FREES,WATCH_REALLOCS,gather_owned};
use crate::product_alloc::{AllocationContext as Cx,AllocationSite as Site,
    AllocationFailureKind as FailureKind,FormatFailure,ordered::OrderedSet};
use crate::product_json::{self as json,Value};
use app::{DecodeOwned,InputMode as Mode};
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicUsize,Ordering};
use std::num::NonZeroUsize;
use std::mem::size_of;
static INJECTED:AtomicUsize=AtomicUsize::new(0);
static GENERATED:AtomicUsize=AtomicUsize::new(0);
fn fault(n:usize,sticky:bool)->Cx{
    assert!(INJECTED.fetch_add(1,Ordering::Relaxed)<8192,"R05c suite-wide injection cap exceeded; revise admission, never truncate");
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),sticky)
}
impl Same<u32> for u32{fn same(&self,b:&u32){assert_eq!(self,b);}}
impl<A:Same<B>,B> Same<Option<B>> for Option<A>{
    fn same(&self,b:&Option<B>){match(self,b){(Some(a),Some(b))=>a.same(b),(None,None)=>(),_=>panic!("Option mismatch")}}
}
impl Same<oracle::KeyModifier> for app::KeyModifier{
    fn same(&self,b:&oracle::KeyModifier){assert!(matches!((self,b),
        (Self::Control,oracle::KeyModifier::Control)|(Self::Alt,oracle::KeyModifier::Alt)|
        (Self::Shift,oracle::KeyModifier::Shift)|(Self::Command,oracle::KeyModifier::Command)));}
}
impl Same<std::collections::BTreeSet<oracle::KeyModifier>> for OrderedSet<app::KeyModifier>{
    fn same(&self,b:&std::collections::BTreeSet<oracle::KeyModifier>){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b);}}
}
impl Same<oracle::OutputFormat> for app::OutputFormat{fn same(&self,b:&oracle::OutputFormat){assert!(matches!((self,b),(Self::Csv,oracle::OutputFormat::Csv)|(Self::Json,oracle::OutputFormat::Json)));}}
impl Same<oracle::ActionPlacement> for app::ActionPlacement{fn same(&self,b:&oracle::ActionPlacement){assert!(matches!((self,b),(Self::Toolbar,oracle::ActionPlacement::Toolbar)|(Self::Row,oracle::ActionPlacement::Row)));}}
macro_rules! structure_same{($name:ident{$($f:ident),+})=>{
    impl Same<oracle::$name> for app::$name{fn same(&self,b:&oracle::$name){$(self.$f.same(&b.$f);)+}}
};}
structure_same!(EntityDefinition{id,label,fields,unique,constraints});
structure_same!(StateDefinition{id,label,value_type,initial});
structure_same!(ActionDefinition{id,label,parameters,guards,steps,ensures});
structure_same!(OutputDefinition{id,label,format,columns});
structure_same!(ObservableDefinition{id,label,value});
structure_same!(Column{id,label,value});
structure_same!(SelectionBinding{id,state,on_change});
structure_same!(FormInput{parameter,label});
structure_same!(ActionBinding{id,label,placement,action,arguments,enabled});
structure_same!(ViewDefinition{id,label,kind,actions,keys});
structure_same!(AppDefinition{version,id,label,entities,state,actions,outputs,observables,views,initial_view});
structure_same!(FieldDefinition{id,label,value_type});
structure_same!(StateControl{id,label,state,on_change});
structure_same!(KeyBinding{key,modifiers,binding});
impl Same<oracle::ViewKind> for app::ViewKind{fn same(&self,b:&oracle::ViewKind){match self{
Self::List{entity,rows,columns,controls,selection}=>{let oracle::ViewKind::List{entity:old_entity,rows:old_rows,columns:old_columns,controls:old_controls,selection:old_selection}=b else{panic!("ViewKind mismatch")};entity.same(old_entity);rows.same(old_rows);columns.same(old_columns);controls.same(old_controls);selection.same(old_selection);},
Self::Detail{entity,record,columns}=>{let oracle::ViewKind::Detail{entity:old_entity,record:old_record,columns:old_columns}=b else{panic!("ViewKind mismatch")};entity.same(old_entity);record.same(old_record);columns.same(old_columns);},
Self::Form{action,fields,defaults}=>{let oracle::ViewKind::Form{action:old_action,fields:old_fields,defaults:old_defaults}=b else{panic!("ViewKind mismatch")};action.same(old_action);fields.same(old_fields);defaults.same(old_defaults);},
}}}
trait Case:DecodeOwned+Same<Self::Old>{type Old:DeserializeOwned;}
impl Case for app::EntityDefinition{type Old=oracle::EntityDefinition;}
impl Case for app::StateDefinition{type Old=oracle::StateDefinition;}
impl Case for app::ActionDefinition{type Old=oracle::ActionDefinition;}
impl Case for app::OutputDefinition{type Old=oracle::OutputDefinition;}
impl Case for app::ObservableDefinition{type Old=oracle::ObservableDefinition;}
impl Case for app::Column{type Old=oracle::Column;}
impl Case for app::SelectionBinding{type Old=oracle::SelectionBinding;}
impl Case for app::FormInput{type Old=oracle::FormInput;}
impl Case for app::ActionBinding{type Old=oracle::ActionBinding;}
impl Case for app::ViewDefinition{type Old=oracle::ViewDefinition;}
impl Case for app::AppDefinition{type Old=oracle::AppDefinition;}
impl Case for app::ViewKind{type Old=oracle::ViewKind;}
fn compare<T:Case>(s:&str,mode:Mode,generated:bool){
    if generated{assert!(GENERATED.fetch_add(1,Ordering::Relaxed)<512,"R05c generated cap exceeded");}
    assert!(s.len()<=16384);let old:serde_json::Value=serde_json::from_str(s).unwrap();bounds(&old,0,&mut 0);
    let expected:Result<T::Old,serde_json::Error>=match mode{
        Mode::DirectValue=>serde_json::from_value(old),
        Mode::BufferedContent=>serde_json::from_value::<Buffered<T::Old>>(serde_json::json!({"wrap":"go","value":old})).map(|Buffered::Go{value}|value),
    };
    let mut cx=Cx::new();let actual=app::decode_value::<T>(value(s),mode,&mut cx);
    match(actual,expected){(Ok(a),Ok(b))=>a.same(&b),
        (Err(a),Err(b))=>assert_eq!(a.try_render(&mut cx).unwrap(),b.to_string(),"{s}; {mode:?}"),
        (Err(a),Ok(_))=>panic!("unexpected {} for {s}",a.try_render(&mut cx).unwrap()),
        (Ok(_),Err(b))=>panic!("accepted {s} despite {b}"),}
}
fn both<T:Case>(inputs:&[&str]){for s in inputs{for m in [Mode::DirectValue,Mode::BufferedContent]{compare::<T>(s,m,false);}}}
#[derive(Clone,Copy)]enum Family{EntityDefinition,StateDefinition,ActionDefinition,OutputDefinition,ObservableDefinition,Column,SelectionBinding,FormInput,ViewKind,ActionBinding,ViewDefinition,AppDefinition}
struct Fixture{family:Family,tag:Option<&'static str>,index:usize,fields:&'static[(&'static str,&'static str)]}
const FIXTURES:&[Fixture]=&[
Fixture{family:Family::EntityDefinition,tag:None,index:0,fields:&[("id",r#""entity""#),("label",r#""Entity""#),("fields",r#"[["field","Field",["list",["text"]]]]"#),("unique",r#"[["kind","type"],["value","default"]]"#),("constraints",r#"[["not",["today"]]]"#)]},
Fixture{family:Family::StateDefinition,tag:None,index:0,fields:&[("id",r#""state""#),("label",r#""State""#),("value_type",r#"["optional",["date"]]"#),("initial",r#"["date",-2147483648]"#)]},
Fixture{family:Family::ActionDefinition,tag:None,index:0,fields:&[("id",r#""action""#),("label",r#""Action""#),("parameters",r#"{"default":["boolean"],"kind":["optional",["text"]],"type":["reference","e"],"value":["integer"]}"#),("guards",r#"[["today"]]"#),("steps",r#"[["collection",["state","s"],"insert",["today"]]]"#),("ensures",r#"[["not",["today"]]]"#)]},
Fixture{family:Family::OutputDefinition,tag:None,index:0,fields:&[("id",r#""output""#),("label",r#""Output""#),("format",r#""csv""#),("columns",r#"[["field","Field",["text"]]]"#)]},
Fixture{family:Family::ObservableDefinition,tag:None,index:0,fields:&[("id",r#""observable""#),("label",r#""Observable""#),("value",r#"["count",["today"]]"#)]},
Fixture{family:Family::Column,tag:None,index:0,fields:&[("id",r#""column""#),("label",r#""Column""#),("value",r#"["field",["variable","r"],"f"]"#)]},
Fixture{family:Family::SelectionBinding,tag:None,index:0,fields:&[("id",r#""selection""#),("state",r#""state""#),("on_change",r#""changed""#)]},
Fixture{family:Family::FormInput,tag:None,index:0,fields:&[("parameter",r#""parameter""#),("label",r#""Input""#)]},
Fixture{family:Family::ViewKind,tag:Some("list"),index:0,fields:&[("entity",r#""entity""#),("rows",r#"["today"]"#),("columns",r#"[["column","Column",["today"]]]"#),("controls",r#"[["control","Control","s","changed"]]"#),("selection",r#"["selection","s","changed"]"#)]},
Fixture{family:Family::ViewKind,tag:Some("detail"),index:1,fields:&[("entity",r#""entity""#),("record",r#"["today"]"#),("columns",r#"[["column","Column",["today"]]]"#)]},
Fixture{family:Family::ViewKind,tag:Some("form"),index:2,fields:&[("action",r#""action""#),("fields",r#"[["p","Parameter"]]"#),("defaults",r#"{"default":["null"],"kind":["text","text"],"type":["integer",7],"value":["boolean",true]}"#)]},
Fixture{family:Family::ActionBinding,tag:None,index:0,fields:&[("id",r#""binding""#),("label",r#""Binding""#),("placement",r#""row""#),("action",r#""action""#),("arguments",r#"{"default":["today"],"kind":["variable","v"],"type":["today"],"value":["state","s"]}"#),("enabled",r#"["today"]"#)]},
Fixture{family:Family::ViewDefinition,tag:None,index:0,fields:&[("id",r#""view""#),("label",r#""View""#),("kind",r#"["detail","e",["today"],[]]"#),("actions",r#"[["bind","Bind","toolbar","a",{},["today"]]]"#),("keys",r#"[["K",["command","alt","control","shift","control"],"bind"]]"#)]},
Fixture{family:Family::AppDefinition,tag:None,index:0,fields:&[("version",r#"1"#),("id",r#""app""#),("label",r#""App""#),("entities",r#"[["e","E",[],[],[]]]"#),("state",r#"[["s","S",["text"],["text","hello"]]]"#),("actions",r#"[["a","A",{},[],[],[]]]"#),("outputs",r#"[["o","O","json",[]]]"#),("observables",r#"[["obs","Obs",["today"]]]"#),("views",r#"[["v","V",["form","a",[],{}],[],[]]]"#),("initial_view",r#""v""#)]},
];
fn dispatch(c:&Fixture,s:&str,m:Mode){match c.family{
Family::EntityDefinition=>compare::<app::EntityDefinition>(s,m,true),
Family::StateDefinition=>compare::<app::StateDefinition>(s,m,true),
Family::ActionDefinition=>compare::<app::ActionDefinition>(s,m,true),
Family::OutputDefinition=>compare::<app::OutputDefinition>(s,m,true),
Family::ObservableDefinition=>compare::<app::ObservableDefinition>(s,m,true),
Family::Column=>compare::<app::Column>(s,m,true),
Family::SelectionBinding=>compare::<app::SelectionBinding>(s,m,true),
Family::FormInput=>compare::<app::FormInput>(s,m,true),
Family::ActionBinding=>compare::<app::ActionBinding>(s,m,true),
Family::ViewDefinition=>compare::<app::ViewDefinition>(s,m,true),
Family::AppDefinition=>compare::<app::AppDefinition>(s,m,true),
Family::ViewKind=>compare::<app::ViewKind>(s,m,true),
}}
fn fixture(c:&Fixture,seq:bool,missing:Option<usize>,null:Option<usize>,numeric:bool)->String{
    let mut parts=Vec::new();
    if let Some(tag)=c.tag{let t=if numeric{c.index.to_string()}else{format!("\"{tag}\"")};parts.push(if seq{t}else{format!("\"kind\":{t}")});}
    for(i,(key,v))in c.fields.iter().enumerate(){if missing==Some(i){if seq{break}else{continue}}
        let v=if null==Some(i){"null"}else{v};parts.push(if seq{v.to_string()}else{format!("\"{key}\":{v}")});}
    let body=parts.join(",");if seq{format!("[{body}]")}else{format!("{{{body}}}")}
}
#[test]
fn every_definition_field_and_view_variant_map_sequence_direct_buffered(){
    assert_eq!(FIXTURES.len(),14);assert_eq!(FIXTURES.iter().map(|f|f.fields.len()).sum::<usize>(),62);
    for c in FIXTURES{for seq in [false,true]{for m in [Mode::DirectValue,Mode::BufferedContent]{dispatch(c,&fixture(c,seq,None,None,false),m);}}}
}
#[test]
fn all_missing_fields_and_short_sequences_preserve_exact_error_order(){
    for c in FIXTURES{for i in 0..c.fields.len(){
        dispatch(c,&fixture(c,false,Some(i),None,false),Mode::DirectValue);
        dispatch(c,&fixture(c,true,Some(i),None,false),Mode::BufferedContent);
    }}
}
#[test]
fn null_at_every_field_in_both_shapes_and_modes_preserves_defaults(){
    for c in FIXTURES{for i in 0..c.fields.len(){for seq in [false,true]{for m in [Mode::DirectValue,Mode::BufferedContent]{
        dispatch(c,&fixture(c,seq,None,Some(i),false),m);
    }}}}
}
#[test]
fn each_definition_unknown_field_and_excess_sequence_matches_oracle(){
    for c in FIXTURES{for seq in [false,true]{let mut v:serde_json::Value=serde_json::from_str(&fixture(c,seq,None,None,false)).unwrap();
        if seq{v.as_array_mut().unwrap().push(serde_json::Value::Bool(false));}else{v.as_object_mut().unwrap().insert("zzz".into(),serde_json::Value::Bool(false));}
        let s=v.to_string();for m in [Mode::DirectValue,Mode::BufferedContent]{dispatch(c,&s,m);}
    }}
}
#[test]
fn all_view_numeric_discriminants_and_bad_tags_preserve_modes(){
    for c in FIXTURES.iter().filter(|c|c.tag.is_some()){for seq in [false,true]{for m in [Mode::DirectValue,Mode::BufferedContent]{dispatch(c,&fixture(c,seq,None,None,true),m);}}}
    both::<app::ViewKind>(&[r#"{"kind":3}"#,r#"{"kind":18446744073709551615}"#,r#"{"kind":-1}"#,r#"{"kind":1.0}"#,
        r#"{"kind":-0.0}"#,r#"{"kind":null}"#,r#"{"kind":true}"#,r#"{"kind":[]}"#,r#"{"kind":{}}"#,
        r#"{"aaa":false,"kind":"bad`\n中","zzz":0}"#,r#"{"aaa":false}"#,r#"[]"#,r#"false"#]);
}
#[test]
fn optional_selection_and_on_change_are_map_defaults_not_sequence_defaults(){
    both::<app::SelectionBinding>(&[r#"{"id":"s","state":"x"}"#,r#"{"id":"s","state":"x","on_change":null}"#,
        r#"{"id":"s","state":"x","on_change":false}"#,r#"["s","x"]"#,r#"["s","x",null]"#,
        r#"["s","x","action",false]"#,r#"["s",false,null,false]"#]);
    both::<app::ViewKind>(&[r#"{"kind":"list","entity":"e","rows":["today"],"columns":[],"controls":[]}"#,
        r#"["list","e",["today"],[],[],null]"#,r#"["list","e",["today"],[],[]]"#,
        r#"["list","e",["today"],[],[],{"id":"s","state":"x"}]"#,
        r#"["list","e",["today"],[],[],{"id":"s","state":"x","on_change":false}]"#]);
}
#[test]
fn dictionary_schema_names_nested_boxes_vectors_and_modes_are_structural(){
    both::<app::ActionDefinition>(&[r#"["a","A",{"default":["integer"],"kind":[0],"type":["optional",[2]],"value":["text"]},[],[],[]]"#,
        r#"["a","A",[],[],[],[]]"#,r#"["a","A",{"a":["list",["text"]],"z":false},[],[],[]]"#]);
    both::<app::ViewKind>(&[r#"["form","a",[],{"kind":[0],"default":[6,[2],[[3,"text"]]],"type":["date",-2147483648],"value":["integer",9223372036854775807]}]"#,
        r#"["form","a",[],null]"#,r#"["form","a",[],[]]"#,
        r#"["list","e",[4],[],[["c","C","s",null]],["sel","s",null]]"#]);
    both::<app::ViewDefinition>(&[r#"["v","V",["detail","e",[4],[]],[["b","B",{"row":{}},"a",{"default":["today"],"kind":[4]},[4]]],[["K",["shift","control","command","alt","shift"],"b"]]]"#,
        r#"["v","V",["form","a",[],{}],[],[["K",["control","bad"],"b"]]]"#]);
    both::<app::EntityDefinition>(&[r#"["e","E",[],[["kind"],[],["type","value","default"]],[]]"#,
        r#"["e","E",[],[["good"],["moved",false]],[]]"#,r#"["e","E",[],[null],[]]"#]);
}
#[test]
fn app_ten_fields_and_all_nested_families_remain_unvalidated(){
    let c=&FIXTURES[13];
    for seq in [false,true]{let s=fixture(c,seq,None,None,false);both::<app::AppDefinition>(&[&s]);
        let old:oracle::AppDefinition=serde_json::from_str(&s).unwrap();
        let mut cx=Cx::new();ok(app::AppDefinition::decode_json(s.as_bytes(),&mut cx)).same(&old);
    }
    // Full App owns every remaining family, including modifiers and selections.
    both::<app::AppDefinition>(&[r#"[0,"","",[["e","E",[["f","F",["optional",["text"]]]],[["f"]],[]]],[["s","S",["text"],["text","x"]]],[["a","A",{},[],[["collection",["state","s"],"remove",["today"]]],[]]],[["o","O","csv",[]]],[["obs","Obs",["query","e","r",["today"],[[["today"],false]],1,false]]],[["v","V",["list","e",["today"],[["c","C",["today"]]],[["control","Control","s",null]],["sel","s",null]],[["b","B","row","a",{},["today"]]],[["K",["control","command"],"b"]]]],"missing"]"#]);
    // Zero/empty identifiers and unresolved links are decoding successes, not validation.
    drop(ok(app::AppDefinition::decode_json(br#"[0,"","",[],[],[],[],[],[],""]"#,&mut Cx::new())));
}
#[test]
fn numeric_ranges_wrong_types_and_late_owned_prefix_errors_match(){
    for version in ["0","4294967295","4294967296","-1","18446744073709551615","18446744073709551616","1.0","-0.0","1e30","1e-30","false","null","[]","{}","\"wrong\""]{
        let s=format!(r#"[{version},"app","App",[],[],[],[],[],[],"v"]"#);both::<app::AppDefinition>(&[&s]);
    }
    both::<app::StateDefinition>(&[r#"["s","S",["integer"],["integer",-9223372036854775808]]"#,
        r#"["s","S",["integer"],["integer",9223372036854775808]]"#,r#"["s","S",["date"],["date",2147483648]]"#,
        r#"["s","S",["date"],["date",-2147483649]]"#,r#"["s","S",["integer"],["integer",-0.0]]"#]);
    both::<app::AppDefinition>(&[r#"{"zzz":false,"version":false,"actions":false}"#,r#"{"aaa":false,"actions":false}"#,
        r#"{"views":false,"version":false}"#,r#"{"version":1,"id":"moved","label":"moved"}"#,
        r#"[1,"moved","moved",[],[],[],[],[],[],false,0]"#,r#"[1,"moved","moved",[],[],[],[],[],[],"v",0]"#]);
    both::<app::ActionBinding>(&[r#"{"placement":false,"arguments":{"z":false,"a":["today"]},"action":"a"}"#]);
    both::<app::ViewKind>(&[r#"{"kind":"list","zzz":0,"columns":false}"#,r#"{"kind":"list","aaa":0,"columns":false}"#,
        r#"["form","moved",[["p","P"]],{"a":["text","moved"],"z":false}]"#]);
}
#[test]
fn app_byte_entrypoint_preserves_syntax_cause_positions_and_policy(){
    for s in [b"[".as_slice(),br#"{"version":1,"version":2}"#,b"\xff",b"[0,]",b"{} trailing"]{
        let expected=json::parse_json_bytes(s,&mut Cx::new()).err().unwrap();
        assert!(matches!(bad(app::AppDefinition::decode_json(s,&mut Cx::new())),app::AppDecodeError::Json(e)if e==expected));
    }
    let oversized=vec![b' ';json::MAX_INPUT_BYTES+1];
    assert!(matches!(bad(app::AppDefinition::decode_json(&oversized,&mut Cx::new())),app::AppDecodeError::Json(json::Error{cause:json::Cause::InputTooLarge,..})));
    let deep=format!("{}0{}","[".repeat(65),"]".repeat(65));
    assert!(matches!(bad(app::AppDefinition::decode_json(deep.as_bytes(),&mut Cx::new())),app::AppDecodeError::Json(json::Error{cause:json::Cause::NestingTooDeep,..})));
}
const SWEEP_ENTITY:&str=r#"["e","E",[["f","F",["list",["text"]]]],[["a","b"],["c"]],[["not",["today"]]]]"#;
const SWEEP_ACTION:&str=r#"["a","A",{"kind":["optional",["integer"]],"type":["text"]},[["not",["today"]]],[["create","e",{"value":["count",["today"]]},"x"]],[["today"]]]"#;
const SWEEP_VIEW:&str=r#"["v","V",["list","e",["today"],[["c","C",["today"]]],[["ctrl","Ctrl","s",null]],["sel","s","a"]],[["b","B","row","a",{"kind":["count",["today"]]},["today"]]],[["K",["command","control","command"],"b"]]]"#;
const SWEEP_APP:&str=r#"[1,"app","App",[["e","E",[],[],[]]],[["s","S",["text"],["text","hello"]]],[["a","A",{},[],[],[]]],[["o","O","csv",[]]],[["obs","Obs",["today"]]],[["v","V",["detail","e",["today"],[]],[],[]]],"v"]"#;
const OWNED_FORM:&str=r#"["form","owned-action",[["owned-parameter","owned-label"]],{"owned-a":["list",["list",["text"]],[["text","owned-child"]]],"owned-z":["text","owned-tail"]}]"#;
const OWNED_PREFIX:&str=r#"["form","owned-action",[["owned-parameter","owned-label"]],{"owned-a":["list",["list",["text"]],[["text","owned-child"]]],"owned-z":false}]"#;
const RENDER_UNKNOWN:&str=r#"{"owned-unknown":false}"#;
const RENDER_FLOAT:&str=r#"[1e+30,"app","App",[],[],[],[],[],[],"v"]"#;
// Independent parser schedule for these fixed ASCII fixtures: each nonempty
// string is one exact reserve; each array/map member reserves its next slot.
// Fixtures have no escapes/duplicates, and map source order is checked against
// this oracle walk before injection. This is NOT recorded from first_failure.
fn parser_trace(s:&str)->Vec<(Site,usize)>{
    assert!(s.len()<=4096&&s.is_ascii()&&!s.contains('\\'));
    let v:serde_json::Value=serde_json::from_str(s).unwrap();
    assert_eq!(v.to_string(),s,"sweep objects must already be in sorted source order");
    fn walk(v:&serde_json::Value,t:&mut Vec<(Site,usize)>){match v{
        serde_json::Value::String(s)=>if !s.is_empty(){t.push((Site::String,s.len()));},
        serde_json::Value::Array(a)=>for(i,v)in a.iter().enumerate(){walk(v,t);t.push((Site::Vector,(i+1)*size_of::<Value>()));},
        serde_json::Value::Object(m)=>for(i,(k,v))in m.iter().enumerate(){if !k.is_empty(){t.push((Site::String,k.len()));}walk(v,t);t.push((Site::OrderedMap,(i+1)*size_of::<(String,Value)>()));},
        _=>(),
    }}
    let mut trace=Vec::new();walk(&v,&mut trace);trace
}
fn sweep<T:DecodeOwned>(s:&str,parse:bool,typed:&[(Site,usize)]){
    let syntax=parser_trace(s);let mut trace=if parse{syntax.clone()}else{Vec::new()};trace.extend_from_slice(typed);
    assert!(!trace.is_empty()&&trace.len()<=512);
    for mode in [Mode::DirectValue,Mode::BufferedContent]{
        let prepare=||if parse{None}else{Some(value(s))};
        let run=|v:Option<Value>,cx:&mut Cx|{let v=match v{Some(v)=>v,None=>json::parse_json_bytes(s.as_bytes(),cx).map_err(app::AppDecodeError::Json)?};app::decode_value::<T>(v,mode,cx)};
        let mut baseline=Cx::new();drop(ok(run(prepare(),&mut baseline)));assert_eq!(baseline.growth_requests(),trace.len());
        for sticky in [false,true]{for(n,&(site,bytes))in trace.iter().enumerate(){
            let before=s.as_bytes().to_vec();let input=prepare();let mut cx=fault(n+1,sticky);let e=bad(run(input,&mut cx));
            assert_eq!(matches!(e,app::AppDecodeError::Json(_)),parse&&n<syntax.len());
            let f=failure(e);assert_eq!((f.kind,f.site,f.requested_bytes),(FailureKind::Injected,site,Some(bytes)));
            assert_eq!(cx.first_failure(),Some(f));assert_eq!(cx.growth_requests(),n+1);assert_eq!(before,s.as_bytes());
        }}
        let mut cx=fault(trace.len()+1,true);let out=ok(run(prepare(),&mut cx));
        assert_eq!(cx.growth_requests(),trace.len());
        assert!(crate::product_alloc::try_append_str(&mut String::new(),"x",&mut cx,Site::String).is_err());drop(out);
        assert_eq!(cx.growth_requests(),trace.len()+1);
    }
}
fn sweep_all(parse:bool){
    let e=size_of::<app::Expr>();let t=size_of::<app::Type>();
    sweep::<app::EntityDefinition>(SWEEP_ENTITY,parse,&[(Site::Vector,size_of::<app::FieldDefinition>()),(Site::Box,t),
        (Site::Vector,2*size_of::<Vec<String>>()),(Site::Vector,2*size_of::<String>()),(Site::Vector,size_of::<String>()),(Site::Vector,e),(Site::Box,e)]);
    sweep::<app::ActionDefinition>(SWEEP_ACTION,parse,&[(Site::OrderedMap,2*size_of::<(String,app::Type)>()),(Site::Box,t),(Site::Vector,e),(Site::Box,e),
        (Site::Vector,size_of::<app::Statement>()),(Site::OrderedMap,size_of::<(String,app::Expr)>()),(Site::Box,e),(Site::Vector,e)]);
    sweep::<app::ViewDefinition>(SWEEP_VIEW,parse,&[(Site::Vector,size_of::<app::Column>()),(Site::Vector,size_of::<app::StateControl>()),
        (Site::Vector,size_of::<app::ActionBinding>()),(Site::OrderedMap,size_of::<(String,app::Expr)>()),(Site::Box,e),
        (Site::Vector,size_of::<app::KeyBinding>()),(Site::OrderedSet,size_of::<(app::KeyModifier,())>()),(Site::OrderedSet,2*size_of::<(app::KeyModifier,())>())]);
    sweep::<app::AppDefinition>(SWEEP_APP,parse,&[(Site::Vector,size_of::<app::EntityDefinition>()),(Site::Vector,size_of::<app::StateDefinition>()),
        (Site::Vector,size_of::<app::ActionDefinition>()),(Site::Vector,size_of::<app::OutputDefinition>()),(Site::Vector,size_of::<app::ObservableDefinition>()),(Site::Vector,size_of::<app::ViewDefinition>())]);
    sweep::<app::ViewKind>(OWNED_FORM,parse,&[(Site::Vector,size_of::<app::FormInput>()),(Site::OrderedMap,2*size_of::<(String,app::DataValue)>()),(Site::Box,t),(Site::Vector,size_of::<app::DataValue>())]);
}
#[test]fn every_parse_decode_growth_has_independent_site_byte_trace(){sweep_all(true);}
#[test]fn every_decode_only_growth_has_independent_site_byte_trace(){sweep_all(false);}
#[test]
fn moved_form_keys_values_and_malformed_owned_prefix_drop_exactly_once(){
    let _guard=WATCH_LOCK.lock().unwrap();
    for(s,expected,success)in [(OWNED_FORM,7,true),(OWNED_PREFIX,6,false)]{for sticky in [false,true]{for ordinal in 1..=5{
        let input=value(s);let mut ptrs=[0;16];let mut n=0;gather_owned(&input,&mut ptrs,&mut n);assert_eq!(n,expected);
        let _watch=WatchScope::arm(&ptrs[..n]);let mut cx=fault(ordinal,sticky);
        let result=app::decode_value::<app::ViewKind>(input,Mode::DirectValue,&mut cx);
        if ordinal<=4{let f=failure(bad(result));let trace=[(Site::Vector,size_of::<app::FormInput>()),(Site::OrderedMap,2*size_of::<(String,app::DataValue)>()),(Site::Box,size_of::<app::Type>()),(Site::Vector,size_of::<app::DataValue>())];
            assert_eq!((f.site,f.requested_bytes),(trace[ordinal-1].0,Some(trace[ordinal-1].1)));assert_eq!(cx.growth_requests(),ordinal);
        }else if success{let out=ok(result);let app::ViewKind::Form{action,fields,defaults}=&out else{panic!()};
            assert_eq!(action.as_ptr()as usize,ptrs[0]);assert_eq!(fields[0].parameter.as_ptr()as usize,ptrs[1]);assert_eq!(fields[0].label.as_ptr()as usize,ptrs[2]);
            let(k,v)=defaults.iter().next().unwrap();assert_eq!(k.as_ptr()as usize,ptrs[3]);
            let app::DataValue::List{items,..}=v else{panic!()};let app::DataValue::Text{value}=&items[0] else{panic!()};assert_eq!(value.as_ptr()as usize,ptrs[4]);
            assert!(WATCH_FREES[..n].iter().all(|v|v.load(Ordering::Relaxed)==0));
            assert!(crate::product_alloc::try_append_str(&mut String::new(),"x",&mut cx,Site::String).is_err());drop(out);
        }else{assert!(matches!(bad(result),app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),4);}
        for i in 0..n{assert_eq!(WATCH_PTRS[i].load(Ordering::Relaxed),0);assert_eq!(WATCH_FREES[i].load(Ordering::Relaxed),1);}assert_eq!(WATCH_REALLOCS.load(Ordering::Relaxed),0);
    }}}
}
// Explicit fixed diagnostic fragments independently price every rendering
// growth. They are not obtained from a successful/failing product renderer.
fn diagnostic_trace(s:&str)->(String,Vec<(Site,usize)>){
    let mut parts=Vec::new();
    if s==RENDER_UNKNOWN{
        parts.extend_from_slice(&["unknown ","field"," `","owned-unknown","`, ","expected one of "]);
        for(i,name)in ["version","id","label","entities","state","actions","outputs","observables","views","initial_view"].iter().enumerate(){
            if i>0{parts.push(", ");}parts.push("`");parts.push(name);parts.push("`");
        }
    }else{assert_eq!(s,RENDER_FLOAT);parts.extend_from_slice(&["invalid type: ","floating point `","1e+30","`",", expected ","u32"]);}
    let mut bytes=0;let trace=parts.iter().map(|s|{bytes+=s.len();(Site::Formatting,bytes)}).collect();
    (parts.concat(),trace)
}
#[test]
fn primary_owned_error_survives_all_render_refusals_and_drops_once(){
    for s in [RENDER_UNKNOWN,RENDER_FLOAT]{
        let syntax=parser_trace(s);let(expected,render_trace)=diagnostic_trace(s);let mut baseline=Cx::new();let primary=bad(app::AppDefinition::decode_json(s.as_bytes(),&mut baseline));
        assert_eq!(baseline.growth_requests(),syntax.len());assert_eq!(primary.try_render(&mut baseline).unwrap(),expected);
        let total=syntax.len()+render_trace.len();assert_eq!(baseline.growth_requests(),total);assert!(total<=512);
        for sticky in [false,true]{for n in 1..=total{
            let original=s.as_bytes().to_vec();let mut cx=fault(n,sticky);let primary=bad(app::AppDefinition::decode_json(s.as_bytes(),&mut cx));
            if n<=syntax.len(){let f=failure(primary);assert_eq!((f.site,f.requested_bytes),(syntax[n-1].0,Some(syntax[n-1].1)));assert_eq!(cx.first_failure(),Some(f));}
            else{assert!(matches!(primary,app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),syntax.len());
                let(site,bytes)=render_trace[n-syntax.len()-1];
                assert!(matches!(primary.try_render(&mut cx),Err(FormatFailure::Allocation(f))if (f.site,f.requested_bytes)==(site,Some(bytes))&&Some(f)==cx.first_failure()));
                assert_eq!(primary.try_render(&mut Cx::new()).unwrap(),expected);}
            assert_eq!(cx.growth_requests(),n);assert_eq!(s.as_bytes(),original);
        }}
        let input=value(s);let mut cx=Cx::new();let cause=bad(app::decode_value::<app::AppDefinition>(input,Mode::DirectValue,&mut cx));assert_eq!(cx.growth_requests(),0);
        let render=total-syntax.len();for sticky in [false,true]{for n in 1..=render{let mut cx=fault(n,sticky);
            let(site,bytes)=render_trace[n-1];assert!(matches!(cause.try_render(&mut cx),Err(FormatFailure::Allocation(f))if (f.site,f.requested_bytes)==(site,Some(bytes))));assert_eq!(cx.growth_requests(),n);
            assert_eq!(cause.try_render(&mut Cx::new()).unwrap(),expected);}}
        let mut text=String::from("unchanged:");assert!(cause.try_append_diagnostic(&mut text,&mut fault(1,true)).is_err());assert_eq!(text,"unchanged:");
        let mut buffer=String::with_capacity(2048);let mut cx=fault(1,true);cause.try_append_diagnostic(&mut buffer,&mut cx).unwrap();assert_eq!(buffer,expected);assert_eq!(cx.growth_requests(),0);
    }
    let _guard=WATCH_LOCK.lock().unwrap();let input=value(RENDER_UNKNOWN);let mut ptrs=[0;16];let mut n=0;gather_owned(&input,&mut ptrs,&mut n);assert_eq!(n,1);
    let _watch=WatchScope::arm(&ptrs[..n]);let mut cx=fault(1,true);let cause=bad(app::decode_value::<app::AppDefinition>(input,Mode::DirectValue,&mut cx));
    assert_eq!(cx.growth_requests(),0);let app::AppDecodeError::Schema(app::SchemaFailure::UnknownField{key,..})=&cause else{panic!()};
    let cap=key.capacity();assert_eq!(key.as_ptr()as usize,ptrs[0]);assert!(cause.try_render(&mut cx).is_err());
    assert!(matches!(&cause,app::AppDecodeError::Schema(app::SchemaFailure::UnknownField{key,..})if key.as_ptr()as usize==ptrs[0]&&key.capacity()==cap));
    assert_eq!(WATCH_FREES[0].load(Ordering::Relaxed),0);drop(cause);assert_eq!(WATCH_FREES[0].load(Ordering::Relaxed),1);assert_eq!(WATCH_REALLOCS.load(Ordering::Relaxed),0);
}
#[test]
fn z_new_suite_budget_receipt(){
    assert_eq!(56+124+248+56+12,496);assert!(GENERATED.load(Ordering::Relaxed)<=512);assert!(INJECTED.load(Ordering::Relaxed)<=8192);
    println!("R05c budget: generated={} injected={}; caps=512/8192; R05a/R05b guarded separately",GENERATED.load(Ordering::Relaxed),INJECTED.load(Ordering::Relaxed));
}
}
