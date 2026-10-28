// R05b: independent frozen Serde oracle; all comparisons walk borrowed fields.
// Oracle construction, fixture mutation and pointer tracking are test-only and
// precede injected product operations. No product serialization/backconversion.
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_json.rs"] mod product_json;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_app.rs"] mod product_app;
#[cfg(not(app_codec_aggregate))]
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "support/app_codec_oracle.rs"] mod oracle;
use crate::product_alloc::{AllocationContext as Cx, AllocationFailure, AllocationFailureKind as FailureKind,
    AllocationSite as Site, FormatFailure, ordered::OrderedMap};
use crate::product_json::{self as json, Value};
use crate::product_app::{self as app, DecodeOwned, InputMode as Mode};
use serde::{Deserialize, de::DeserializeOwned};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Deserialize)]
#[serde(tag="wrap",rename_all="snake_case")]
enum Buffered<T>{Go{value:T}}
fn ok<T>(r:Result<T,app::AppDecodeError>)->T{match r{Ok(v)=>v,Err(e)=>panic!("{}",e.try_render(&mut Cx::new()).unwrap())}}
fn bad<T>(r:Result<T,app::AppDecodeError>)->app::AppDecodeError{match r{Err(e)=>e,Ok(_)=>panic!("expected error")}}
fn value(s:&str)->Value{json::parse_json_bytes(s.as_bytes(),&mut Cx::new()).unwrap()}
static INJECTED:AtomicUsize=AtomicUsize::new(0);
static GENERATED:AtomicUsize=AtomicUsize::new(0);
fn fault(n:usize,sticky:bool)->Cx{
    assert!(INJECTED.fetch_add(1,Ordering::Relaxed)<8192,"R05b TOTAL injection cap exceeded; replan, never truncate");
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),sticky)
}
// R05a has its own unchanged 8192 guard; aggregate admission accounts for both.
trait Same<T>{fn same(&self,b:&T);}
macro_rules! scalar_same{($($t:ty),+)=>{$(impl Same<$t> for $t{fn same(&self,b:&Self){assert_eq!(self,b);}})+};}
scalar_same!(String,bool,usize,i64,i32);
impl<A:Same<B>,B> Same<Box<B>> for Box<A>{fn same(&self,b:&Box<B>){(**self).same(&**b);}}
impl<A:Same<B>,B> Same<Vec<B>> for Vec<A>{fn same(&self,b:&Vec<B>){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b);}}}
impl<A:Same<B>,B> Same<std::collections::BTreeMap<String,B>> for OrderedMap<String,A>{
    fn same(&self,b:&std::collections::BTreeMap<String,B>){assert_eq!(self.len(),b.len());for((ak,av),(bk,bv))in self.iter().zip(b){assert_eq!(ak,bk);av.same(bv);}}
}
impl Same<oracle::Type> for app::Type{
    fn same(&self,b:&oracle::Type){match self{
        Self::Boolean=>{let oracle::Type::Boolean=b else{panic!("Type::Boolean mismatch")};},
        Self::Integer=>{let oracle::Type::Integer=b else{panic!("Type::Integer mismatch")};},
        Self::Text=>{let oracle::Type::Text=b else{panic!("Type::Text mismatch")};},
        Self::Date=>{let oracle::Type::Date=b else{panic!("Type::Date mismatch")};},
        Self::Reference{entity}=>{let oracle::Type::Reference{entity:old_entity}=b else{panic!("Type::Reference mismatch")};entity.same(old_entity);},
        Self::List{item}=>{let oracle::Type::List{item:old_item}=b else{panic!("Type::List mismatch")};item.same(old_item);},
        Self::Optional{item}=>{let oracle::Type::Optional{item:old_item}=b else{panic!("Type::Optional mismatch")};item.same(old_item);},
    }}
}
impl Same<oracle::DataValue> for app::DataValue{
    fn same(&self,b:&oracle::DataValue){match self{
        Self::Null=>{let oracle::DataValue::Null=b else{panic!("DataValue::Null mismatch")};},
        Self::Boolean{value}=>{let oracle::DataValue::Boolean{value:old_value}=b else{panic!("DataValue::Boolean mismatch")};value.same(old_value);},
        Self::Integer{value}=>{let oracle::DataValue::Integer{value:old_value}=b else{panic!("DataValue::Integer mismatch")};value.same(old_value);},
        Self::Text{value}=>{let oracle::DataValue::Text{value:old_value}=b else{panic!("DataValue::Text mismatch")};value.same(old_value);},
        Self::Date{days}=>{let oracle::DataValue::Date{days:old_days}=b else{panic!("DataValue::Date mismatch")};days.same(old_days);},
        Self::Reference{entity,record}=>{let oracle::DataValue::Reference{entity:old_entity,record:old_record}=b else{panic!("DataValue::Reference mismatch")};entity.same(old_entity);record.same(old_record);},
        Self::List{item_type,items}=>{let oracle::DataValue::List{item_type:old_item_type,items:old_items}=b else{panic!("DataValue::List mismatch")};item_type.same(old_item_type);items.same(old_items);},
    }}
}
impl Same<oracle::Expr> for app::Expr{
    fn same(&self,b:&oracle::Expr){match self{
        Self::Literal{value_type,value}=>{let oracle::Expr::Literal{value_type:old_value_type,value:old_value}=b else{panic!("Expr::Literal mismatch")};value_type.same(old_value_type);value.same(old_value);},
        Self::Variable{name}=>{let oracle::Expr::Variable{name:old_name}=b else{panic!("Expr::Variable mismatch")};name.same(old_name);},
        Self::Field{record,field}=>{let oracle::Expr::Field{record:old_record,field:old_field}=b else{panic!("Expr::Field mismatch")};record.same(old_record);field.same(old_field);},
        Self::State{state}=>{let oracle::Expr::State{state:old_state}=b else{panic!("Expr::State mismatch")};state.same(old_state);},
        Self::Today=>{let oracle::Expr::Today=b else{panic!("Expr::Today mismatch")};},
        Self::Query{entity,binding,predicate,sort,limit,include_archived}=>{let oracle::Expr::Query{entity:old_entity,binding:old_binding,predicate:old_predicate,sort:old_sort,limit:old_limit,include_archived:old_include_archived}=b else{panic!("Expr::Query mismatch")};entity.same(old_entity);binding.same(old_binding);predicate.same(old_predicate);sort.same(old_sort);limit.same(old_limit);include_archived.same(old_include_archived);},
        Self::Filter{items,binding,predicate}=>{let oracle::Expr::Filter{items:old_items,binding:old_binding,predicate:old_predicate}=b else{panic!("Expr::Filter mismatch")};items.same(old_items);binding.same(old_binding);predicate.same(old_predicate);},
        Self::Map{items,binding,value}=>{let oracle::Expr::Map{items:old_items,binding:old_binding,value:old_value}=b else{panic!("Expr::Map mismatch")};items.same(old_items);binding.same(old_binding);value.same(old_value);},
        Self::Count{items}=>{let oracle::Expr::Count{items:old_items}=b else{panic!("Expr::Count mismatch")};items.same(old_items);},
        Self::Any{items,binding,predicate}=>{let oracle::Expr::Any{items:old_items,binding:old_binding,predicate:old_predicate}=b else{panic!("Expr::Any mismatch")};items.same(old_items);binding.same(old_binding);predicate.same(old_predicate);},
        Self::All{items,binding,predicate}=>{let oracle::Expr::All{items:old_items,binding:old_binding,predicate:old_predicate}=b else{panic!("Expr::All mismatch")};items.same(old_items);binding.same(old_binding);predicate.same(old_predicate);},
        Self::Sum{items}=>{let oracle::Expr::Sum{items:old_items}=b else{panic!("Expr::Sum mismatch")};items.same(old_items);},
        Self::Contains{items,value}=>{let oracle::Expr::Contains{items:old_items,value:old_value}=b else{panic!("Expr::Contains mismatch")};items.same(old_items);value.same(old_value);},
        Self::Equal{left,right}=>{let oracle::Expr::Equal{left:old_left,right:old_right}=b else{panic!("Expr::Equal mismatch")};left.same(old_left);right.same(old_right);},
        Self::Less{left,right}=>{let oracle::Expr::Less{left:old_left,right:old_right}=b else{panic!("Expr::Less mismatch")};left.same(old_left);right.same(old_right);},
        Self::Add{left,right}=>{let oracle::Expr::Add{left:old_left,right:old_right}=b else{panic!("Expr::Add mismatch")};left.same(old_left);right.same(old_right);},
        Self::Subtract{left,right}=>{let oracle::Expr::Subtract{left:old_left,right:old_right}=b else{panic!("Expr::Subtract mismatch")};left.same(old_left);right.same(old_right);},
        Self::Concat{left,right}=>{let oracle::Expr::Concat{left:old_left,right:old_right}=b else{panic!("Expr::Concat mismatch")};left.same(old_left);right.same(old_right);},
        Self::TextContains{text,search}=>{let oracle::Expr::TextContains{text:old_text,search:old_search}=b else{panic!("Expr::TextContains mismatch")};text.same(old_text);search.same(old_search);},
        Self::Lower{value}=>{let oracle::Expr::Lower{value:old_value}=b else{panic!("Expr::Lower mismatch")};value.same(old_value);},
        Self::And{values}=>{let oracle::Expr::And{values:old_values}=b else{panic!("Expr::And mismatch")};values.same(old_values);},
        Self::Or{values}=>{let oracle::Expr::Or{values:old_values}=b else{panic!("Expr::Or mismatch")};values.same(old_values);},
        Self::Not{value}=>{let oracle::Expr::Not{value:old_value}=b else{panic!("Expr::Not mismatch")};value.same(old_value);},
        Self::If{condition,then_value,else_value}=>{let oracle::Expr::If{condition:old_condition,then_value:old_then_value,else_value:old_else_value}=b else{panic!("Expr::If mismatch")};condition.same(old_condition);then_value.same(old_then_value);else_value.same(old_else_value);},
        Self::Coalesce{value,fallback}=>{let oracle::Expr::Coalesce{value:old_value,fallback:old_fallback}=b else{panic!("Expr::Coalesce mismatch")};value.same(old_value);fallback.same(old_fallback);},
        Self::DateAdd{date,days}=>{let oracle::Expr::DateAdd{date:old_date,days:old_days}=b else{panic!("Expr::DateAdd mismatch")};date.same(old_date);days.same(old_days);},
        Self::DateDifference{later,earlier}=>{let oracle::Expr::DateDifference{later:old_later,earlier:old_earlier}=b else{panic!("Expr::DateDifference mismatch")};later.same(old_later);earlier.same(old_earlier);},
    }}
}
impl Same<oracle::Statement> for app::Statement{
    fn same(&self,b:&oracle::Statement){match self{
        Self::Create{entity,values,bind}=>{let oracle::Statement::Create{entity:old_entity,values:old_values,bind:old_bind}=b else{panic!("Statement::Create mismatch")};entity.same(old_entity);values.same(old_values);bind.same(old_bind);},
        Self::Update{record,values}=>{let oracle::Statement::Update{record:old_record,values:old_values}=b else{panic!("Statement::Update mismatch")};record.same(old_record);values.same(old_values);},
        Self::Archive{record}=>{let oracle::Statement::Archive{record:old_record}=b else{panic!("Statement::Archive mismatch")};record.same(old_record);},
        Self::SetState{state,value}=>{let oracle::Statement::SetState{state:old_state,value:old_value}=b else{panic!("Statement::SetState mismatch")};state.same(old_state);value.same(old_value);},
        Self::Collection{target,operation,items}=>{let oracle::Statement::Collection{target:old_target,operation:old_operation,items:old_items}=b else{panic!("Statement::Collection mismatch")};target.same(old_target);operation.same(old_operation);items.same(old_items);},
        Self::ForEach{items,binding,steps}=>{let oracle::Statement::ForEach{items:old_items,binding:old_binding,steps:old_steps}=b else{panic!("Statement::ForEach mismatch")};items.same(old_items);binding.same(old_binding);steps.same(old_steps);},
        Self::Assert{condition,message}=>{let oracle::Statement::Assert{condition:old_condition,message:old_message}=b else{panic!("Statement::Assert mismatch")};condition.same(old_condition);message.same(old_message);},
        Self::Emit{output,items,binding,columns}=>{let oracle::Statement::Emit{output:old_output,items:old_items,binding:old_binding,columns:old_columns}=b else{panic!("Statement::Emit mismatch")};output.same(old_output);items.same(old_items);binding.same(old_binding);columns.same(old_columns);},
    }}
}
impl Same<oracle::CollectionTarget> for app::CollectionTarget{
    fn same(&self,b:&oracle::CollectionTarget){match self{
        Self::State{state}=>{let oracle::CollectionTarget::State{state:old_state}=b else{panic!("CollectionTarget::State mismatch")};state.same(old_state);},
        Self::Field{record,field}=>{let oracle::CollectionTarget::Field{record:old_record,field:old_field}=b else{panic!("CollectionTarget::Field mismatch")};record.same(old_record);field.same(old_field);},
    }}
}
impl Same<oracle::SortKey> for app::SortKey{fn same(&self,b:&oracle::SortKey){self.value.same(&b.value);self.descending.same(&b.descending);}}
impl Same<oracle::CollectionOperation> for app::CollectionOperation{fn same(&self,b:&oracle::CollectionOperation){assert!(matches!((self,b),(Self::Insert,oracle::CollectionOperation::Insert)|(Self::Remove,oracle::CollectionOperation::Remove)));}}
trait Case:DecodeOwned+Same<Self::Old>{type Old:DeserializeOwned;}
macro_rules! case{($($t:ident),+)=>{$(impl Case for app::$t{type Old=oracle::$t;})+};}
case!(Expr,SortKey,Statement,CollectionTarget);
fn bounds(v:&serde_json::Value,depth:usize,nodes:&mut usize){
    *nodes+=1;assert!(*nodes<=128);assert!(depth<=8);
    match v{serde_json::Value::Array(a)=>for v in a{bounds(v,depth+1,nodes)},serde_json::Value::Object(m)=>for v in m.values(){bounds(v,depth+1,nodes)},_=>()}
}
fn compare<T:Case>(s:&str,mode:Mode,generated:bool){
    if generated{assert!(GENERATED.fetch_add(1,Ordering::Relaxed)<512,"R05b generated cap exceeded");}
    assert!(s.len()<=16384);let old:serde_json::Value=serde_json::from_str(s).unwrap();bounds(&old,0,&mut 0);
    let expected:Result<T::Old,serde_json::Error>=match mode{
        Mode::DirectValue=>serde_json::from_value(old),
        Mode::BufferedContent=>serde_json::from_value::<Buffered<T::Old>>(serde_json::json!({"wrap":"go","value":old})).map(|Buffered::Go{value}|value),
    };
    let mut cx=Cx::new();let actual=app::decode_value::<T>(value(s),mode,&mut cx);
    match(actual,expected){
        (Ok(a),Ok(b))=>a.same(&b),
        (Err(a),Err(b))=>assert_eq!(a.try_render(&mut cx).unwrap(),b.to_string(),"{s}; {mode:?}"),
        (Err(a),Ok(_))=>panic!("unexpected {} for {s}",a.try_render(&mut cx).unwrap()),
        (Ok(_),Err(b))=>panic!("accepted {s} despite {b}"),
    }
}
fn both<T:Case>(inputs:&[&str]){for s in inputs{for m in [Mode::DirectValue,Mode::BufferedContent]{compare::<T>(s,m,false);}}}
#[derive(Clone,Copy)]enum Family{Expr,Statement,Target,Sort}
struct Fixture{family:Family,tag:Option<&'static str>,index:usize,fields:&'static[(&'static str,&'static str)]}
const FIXTURES:&[Fixture]=&[
    Fixture{family:Family::Expr,tag:Some("literal"),index:0,fields:&[("value_type",r#"["integer"]"#),("value",r#"["integer",17]"#)]},
    Fixture{family:Family::Expr,tag:Some("variable"),index:1,fields:&[("name",r#""variable-name""#)]},
    Fixture{family:Family::Expr,tag:Some("field"),index:2,fields:&[("record",r#"["variable","record"]"#),("field",r#""field-name""#)]},
    Fixture{family:Family::Expr,tag:Some("state"),index:3,fields:&[("state",r#""state-name""#)]},
    Fixture{family:Family::Expr,tag:Some("today"),index:4,fields:&[]},
    Fixture{family:Family::Expr,tag:Some("query"),index:5,fields:&[("entity",r#""entity-name""#),("binding",r#""row-name""#),("predicate",r#"["not",["today"]]"#),("sort",r#"[[["variable","sort-a"],false],{"value":["today"],"descending":true}]"#),("limit",r#"13"#),("include_archived",r#"true"#)]},
    Fixture{family:Family::Expr,tag:Some("filter"),index:6,fields:&[("items",r#"["variable","source"]"#),("binding",r#""filter-row""#),("predicate",r#"["today"]"#)]},
    Fixture{family:Family::Expr,tag:Some("map"),index:7,fields:&[("items",r#"["variable","source"]"#),("binding",r#""map-row""#),("value",r#"["today"]"#)]},
    Fixture{family:Family::Expr,tag:Some("count"),index:8,fields:&[("items",r#"["variable","count"]"#)]},
    Fixture{family:Family::Expr,tag:Some("any"),index:9,fields:&[("items",r#"["variable","source"]"#),("binding",r#""any-row""#),("predicate",r#"["today"]"#)]},
    Fixture{family:Family::Expr,tag:Some("all"),index:10,fields:&[("items",r#"["variable","source"]"#),("binding",r#""all-row""#),("predicate",r#"["today"]"#)]},
    Fixture{family:Family::Expr,tag:Some("sum"),index:11,fields:&[("items",r#"["variable","sum"]"#)]},
    Fixture{family:Family::Expr,tag:Some("contains"),index:12,fields:&[("items",r#"["variable","source"]"#),("value",r#"["state","needle"]"#)]},
    Fixture{family:Family::Expr,tag:Some("equal"),index:13,fields:&[("left",r#"["variable","left"]"#),("right",r#"["state","right"]"#)]},
    Fixture{family:Family::Expr,tag:Some("less"),index:14,fields:&[("left",r#"["variable","left"]"#),("right",r#"["state","right"]"#)]},
    Fixture{family:Family::Expr,tag:Some("add"),index:15,fields:&[("left",r#"["variable","left"]"#),("right",r#"["state","right"]"#)]},
    Fixture{family:Family::Expr,tag:Some("subtract"),index:16,fields:&[("left",r#"["variable","left"]"#),("right",r#"["state","right"]"#)]},
    Fixture{family:Family::Expr,tag:Some("concat"),index:17,fields:&[("left",r#"["variable","left"]"#),("right",r#"["state","right"]"#)]},
    Fixture{family:Family::Expr,tag:Some("text_contains"),index:18,fields:&[("text",r#"["variable","text"]"#),("search",r#"["state","search"]"#)]},
    Fixture{family:Family::Expr,tag:Some("lower"),index:19,fields:&[("value",r#"["state","lower"]"#)]},
    Fixture{family:Family::Expr,tag:Some("and"),index:20,fields:&[("values",r#"[["variable","first"],["today"]]"#)]},
    Fixture{family:Family::Expr,tag:Some("or"),index:21,fields:&[("values",r#"[["today"],["state","last"]]"#)]},
    Fixture{family:Family::Expr,tag:Some("not"),index:22,fields:&[("value",r#"["state","not"]"#)]},
    Fixture{family:Family::Expr,tag:Some("if"),index:23,fields:&[("condition",r#"["variable","condition"]"#),("then_value",r#"["state","then"]"#),("else_value",r#"["variable","else"]"#)]},
    Fixture{family:Family::Expr,tag:Some("coalesce"),index:24,fields:&[("value",r#"["state","value"]"#),("fallback",r#"["variable","fallback"]"#)]},
    Fixture{family:Family::Expr,tag:Some("date_add"),index:25,fields:&[("date",r#"["state","date"]"#),("days",r#"["variable","days"]"#)]},
    Fixture{family:Family::Expr,tag:Some("date_difference"),index:26,fields:&[("later",r#"["state","later"]"#),("earlier",r#"["variable","earlier"]"#)]},
    Fixture{family:Family::Statement,tag:Some("create"),index:0,fields:&[("entity",r#""entity""#),("values",r#"{"kind":["variable","kind-value"],"type":["today"],"value":["state","value-value"]}"#),("bind",r#""created""#)]},
    Fixture{family:Family::Statement,tag:Some("update"),index:1,fields:&[("record",r#"["variable","record"]"#),("values",r#"{"record":["state","updated"],"values":["today"]}"#)]},
    Fixture{family:Family::Statement,tag:Some("archive"),index:2,fields:&[("record",r#"["variable","archive"]"#)]},
    Fixture{family:Family::Statement,tag:Some("set_state"),index:3,fields:&[("state",r#""state""#),("value",r#"["variable","assigned"]"#)]},
    Fixture{family:Family::Statement,tag:Some("collection"),index:4,fields:&[("target",r#"["field",["variable","record"],"collection"]"#),("operation",r#"{"insert":{}}"#),("items",r#"["state","items"]"#)]},
    Fixture{family:Family::Statement,tag:Some("for_each"),index:5,fields:&[("items",r#"["state","items"]"#),("binding",r#""row""#),("steps",r#"[["archive",["variable","row"]],["set_state","s",["today"]]]"#)]},
    Fixture{family:Family::Statement,tag:Some("assert"),index:6,fields:&[("condition",r#"["today"]"#),("message",r#""message\n中`""#)]},
    Fixture{family:Family::Statement,tag:Some("emit"),index:7,fields:&[("output",r#""output""#),("items",r#"["state","items"]"#),("binding",r#""row""#),("columns",r#"{"binding":["variable","bound"],"columns":["today"]}"#)]},
    Fixture{family:Family::Target,tag:Some("state"),index:0,fields:&[("state",r#""collection-state""#)]},
    Fixture{family:Family::Target,tag:Some("field"),index:1,fields:&[("record",r#"["variable","record"]"#),("field",r#""collection-field""#)]},
    Fixture{family:Family::Sort,tag:None,index:0,fields:&[("value",r#"["variable","sort"]"#),("descending","true")]},
];
fn fixture(c:&Fixture,seq:bool,missing:Option<usize>,null_first:bool,numeric:bool)->String{
    let mut parts=Vec::new();
    if let Some(tag)=c.tag{let t=if numeric{c.index.to_string()}else{format!("\"{tag}\"")};parts.push(if seq{t}else{format!("\"kind\":{t}")});}
    for(i,(key,v))in c.fields.iter().enumerate(){
        if missing==Some(i){if seq{break}else{continue}}
        let v=if null_first&&i==0{"null"}else{v};parts.push(if seq{v.to_string()}else{format!("\"{key}\":{v}")});
    }
    let body=parts.join(",");if seq{format!("[{body}]")}else{format!("{{{body}}}")}
}
fn run_fixture(c:&Fixture,s:&str,m:Mode){match c.family{
    Family::Expr=>compare::<app::Expr>(s,m,true),Family::Statement=>compare::<app::Statement>(s,m,true),
    Family::Target=>compare::<app::CollectionTarget>(s,m,true),Family::Sort=>compare::<app::SortKey>(s,m,true),
}}
#[test]
fn all_variants_and_sort_key_map_sequence_direct_buffered(){
    assert_eq!(FIXTURES.len(),38);assert_eq!(FIXTURES.iter().map(|c|c.fields.len()).sum::<usize>(),78);
    for c in FIXTURES{for seq in [false,true]{for m in [Mode::DirectValue,Mode::BufferedContent]{run_fixture(c,&fixture(c,seq,None,false,false),m);}}}
}
#[test]
fn every_missing_map_field_and_sequence_prefix_has_exact_diagnostic(){
    // 78 map omissions +78 sequence prefixes, not arbitrary random sampling.
    for c in FIXTURES{for i in 0..c.fields.len(){
        run_fixture(c,&fixture(c,false,Some(i),false,false),Mode::DirectValue);
        run_fixture(c,&fixture(c,true,Some(i),false,false),Mode::BufferedContent);
    }}
}
#[test]
fn required_fields_never_gain_null_or_implicit_defaults(){
    for c in FIXTURES.iter().filter(|c|!c.fields.is_empty()){
        for m in [Mode::DirectValue,Mode::BufferedContent]{run_fixture(c,&fixture(c,c.index%2==0,None,true,false),m);}
    }
}
#[test]
fn every_numeric_tag_obeys_input_mode_and_declaration_index(){
    for c in FIXTURES.iter().filter(|c|c.tag.is_some()){
        for m in [Mode::DirectValue,Mode::BufferedContent]{run_fixture(c,&fixture(c,c.index%2==1,None,false,true),m);}
    }
}
#[test]
fn mixed_recursive_expr_sort_statement_graphs_are_bounded(){
    for n in 0..8{
        let child=if n%2==0{r#"["date_add",["today"],["literal",["integer"],["integer",2]]]"#}else{r#"["if",["today"],["variable","a"],["state","b"]]"#};
        let s=format!(r#"["for_each",["query","e","b",["today"],[[{child},true]],{n},false],"row",[["collection",["field",["variable","row"],"f"],"remove",["map",["state","s"],"x",{child}]]]]"#);
        for m in [Mode::DirectValue,Mode::BufferedContent]{compare::<app::Statement>(&s,m,true);}
    }
}
#[test]
fn query_usize_and_literal_integer_float_boundaries(){
    for limit in ["0","1","4294967295","4294967296","18446744073709551615","18446744073709551616","-1","-9223372036854775808","1.0","-0.0","1e30","1e-30"]{
        let s=format!(r#"["query","e","b",["today"],[],{limit},false]"#);
        for m in [Mode::DirectValue,Mode::BufferedContent]{compare::<app::Expr>(&s,m,true);}
    }
    for(limit,valid)in [("4294967295",true),("4294967296",usize::BITS>32),("18446744073709551615",usize::BITS==64)]{
        let s=format!(r#"["query","e","b",["today"],[],{limit},false]"#);
        let r=app::Expr::decode_json(s.as_bytes(),&mut Cx::new());assert_eq!(r.is_ok(),valid);
    }
    both::<app::Expr>(&[r#"["literal",["integer"],["integer",-9223372036854775808]]"#,
        r#"["literal",["integer"],["integer",9223372036854775807]]"#,
        r#"["literal",["integer"],["integer",9223372036854775808]]"#,
        r#"["literal",["date"],["date",-2147483648]]"#,r#"["literal",["date"],["date",2147483648]]"#,
        r#"["literal",["integer"],["integer",1.0]]"#]);
}

#[test]
fn sort_key_prefix_tail_and_internal_tag_error_precedence(){
    both::<app::SortKey>(&[r#"{}"#,r#"null"#,r#"[]"#,r#"[["today"]]"#,
        r#"[["today"],false,0]"#,r#"[false,false,0]"#,r#"{"value":["today"],"descending":null}"#,
        r#"{"value":[4],"descending":false}"#,r#"{"value":["today"],"descending":false,"z":0}"#]);
    both::<app::Expr>(&[r#"{"aaa":false}"#,r#"[]"#,r#"false"#,r#"{"kind":27}"#,
        r#"{"kind":18446744073709551615}"#,r#"{"kind":-1}"#,r#"{"kind":1.0}"#,r#"{"kind":-0.0}"#,
        r#"{"kind":null}"#,r#"{"kind":true}"#,r#"{"kind":[]}"#,r#"{"kind":{}}"#,
        r#"{"aaa":false,"kind":"bad`\n中","zzz":0}"#,
        r#"{"kind":"today","aaa":{"bad":0},"z":[1]}"#,r#"["today",false]"#,
        r#"["variable",false,0]"#,r#"["variable","good",0]"#]);
    both::<app::Statement>(&[r#"{"aaa":false}"#,r#"[]"#,r#"false"#,r#"{"kind":8}"#,
        r#"{"kind":-1}"#,r#"{"kind":1.0}"#,r#"{"aaa":false,"kind":"bad"}"#,
        r#"["archive",false,0]"#,r#"["archive",["today"],0]"#]);
    both::<app::CollectionTarget>(&[r#"{"aaa":false}"#,r#"[]"#,r#"null"#,r#"{"kind":2}"#,
        r#"{"kind":-1}"#,r#"{"kind":1.0}"#,r#"{"aaa":false,"kind":"bad"}"#,
        r#"["state",false,0]"#,r#"["state","s",0]"#]);
}
#[test]
fn sorted_schema_fields_choose_errors_before_missing_declaration_order(){
    both::<app::Expr>(&[r#"{"kind":"query","zzz":0,"binding":false}"#,
        r#"{"kind":"query","aaa":0,"binding":false}"#,
        r#"{"kind":"query","sort":false,"predicate":false}"#,
        r#"{"kind":"if","then_value":false,"else_value":false,"condition":false}"#,
        r#"{"kind":"if","then_value":["today"],"else_value":["today"]}"#,
        r#"{"kind":"and","values":[["today"],false,["not",["today"]]]}"#]);
    both::<app::Statement>(&[r#"{"kind":"create","values":false,"entity":false,"bind":"b"}"#,
        r#"{"kind":"create","values":{"z":false,"a":["variable",false]},"bind":"b","entity":"e"}"#,
        r#"{"kind":"emit","columns":{"kind":false},"binding":"b"}"#,
        r#"{"kind":"for_each","steps":[["archive",["today"]],false],"binding":"b","items":["today"]}"#]);
    both::<app::CollectionTarget>(&[r#"{"kind":"field","record":false,"field":false}"#,
        r#"{"kind":"field","aaa":0,"field":"f","record":false}"#]);
}
#[test]
fn arbitrary_dictionary_keys_and_nested_buffered_modes_are_not_schema_fields(){
    both::<app::Statement>(&[
        r#"["create","e",{"kind":[4],"type":["today"],"value":["today"],"values":["today"],"columns":["today"],"entity":["today"],"binding":["today"],"predicate":["today"],"record":["today"],"sort":["today"],"0":["today"],"中\n`":["today"]},"b"]"#,
        r#"["update",["today"],{}]"#,r#"["create","e",{},"b"]"#,
        r#"["emit","o",["today"],"b",{"kind":[4],"type":["today"],"value":["today"]}]"#,
        r#"["create","e",[],"b"]"#,r#"["update",["today"],null]"#,
        r#"["emit","o",["today"],"b",false]"#,
        r#"["collection",[1,[4],"f"],{"remove":{}},[4]]"#,
        r#"["collection",[0,"s"],{"remove":{"bad":0}},[4]]"#,
        r#"["collection",[0,"s"],{"remove":null,"insert":null},[4]]"#]);
    both::<app::Expr>(&[r#"["query","e","b",[4],[[[4],false]],0,false]"#,
        r#"["literal",[6,[0]],[6,[0],[[0],[1,true]]]]"#]);
}
#[test]
fn byte_entrypoints_preserve_parser_errors_and_existing_product_limits(){
    drop(ok(app::Expr::decode_json(br#"["today"]"#,&mut Cx::new())));
    drop(ok(app::Statement::decode_json(br#"["archive",["today"]]"#,&mut Cx::new())));
    drop(ok(app::CollectionTarget::decode_json(br#"["state","s"]"#,&mut Cx::new())));
    drop(ok(app::SortKey::decode_json(br#"[["today"],false]"#,&mut Cx::new())));
    let e=bad(app::Statement::decode_json(br#"["create","e",{"kind":["today"],"kind":["today"]},"b"]"#,&mut Cx::new()));
    assert!(matches!(e,app::AppDecodeError::Json(json::Error{cause:json::Cause::DuplicateObjectMember,..})));
    let e=bad(app::Expr::decode_json(b"[",&mut Cx::new()));assert!(matches!(e,app::AppDecodeError::Json(_)));
    // These fixed limit cases are not generated or fault-swept.
    let mut deep=String::from(r#"["today"]"#);for _ in 1..64{deep=format!(r#"["not",{deep}]"#);}
    drop(ok(app::Expr::decode_json(deep.as_bytes(),&mut Cx::new())));
    deep=format!(r#"["not",{deep}]"#);assert!(matches!(bad(app::Expr::decode_json(deep.as_bytes(),&mut Cx::new())),app::AppDecodeError::Json(json::Error{cause:json::Cause::NestingTooDeep,..})));
    let oversized=vec![b' ';json::MAX_INPUT_BYTES+1];
    let e=bad(app::Statement::decode_json(&oversized,&mut Cx::new()));assert!(matches!(e,app::AppDecodeError::Json(json::Error{cause:json::Cause::InputTooLarge,..})));
}
const SWEEP_QUERY:&str=r#"["query","e","b",["if",["today"],["count",["today"]],["not",["today"]]],[[["lower",["today"]],false]],7,false]"#;
const SWEEP_AND:&str=r#"["and",[["count",["today"]],["or",[["today"],["today"]]]]]"#;
const SWEEP_STEPS:&str=r#"["for_each",["today"],"row",[["create","e",{"kind":["count",["today"]],"type":["today"],"value":["today"]},"b"],["update",["today"],{"binding":["today"]}]]]"#;
const SWEEP_COLLECTION:&str=r#"["collection",["field",["field",["today"],"f"],"c"],{"insert":{}},["today"]]"#;
const OWNED_SUCCESS:&str=r#"["create","owned-entity",{"owned-a":["count",["variable","owned-inner"]],"owned-z":["variable","owned-tail"]},"owned-bind"]"#;
const OWNED_PREFIX:&str=r#"["create","owned-entity",{"owned-a":["count",["variable","owned-inner"]],"owned-z":false},"owned-bind"]"#;
const RENDER_UNKNOWN:&str=r#"{"aaa":false,"kind":"unknown`\n中"}"#;
const RENDER_FLOAT:&str=r#"["query","e","b",["today"],[],1e30,false]"#;
fn failure(e:app::AppDecodeError)->AllocationFailure{match e{
    app::AppDecodeError::Allocation(f)=>f,app::AppDecodeError::Json(json::Error{cause:json::Cause::Allocation(f),..})=>f,
    _=>panic!("allocation primary replaced"),
}}
fn sweep<T:DecodeOwned>(s:&str,parse:bool,trace:&[(Site,usize)]){
    assert!(s.len()<=4096);let mut parser=Cx::new();drop(json::parse_json_bytes(s.as_bytes(),&mut parser).unwrap());
    let prefix=if parse{parser.growth_requests()}else{0};
    for mode in [Mode::DirectValue,Mode::BufferedContent]{
        let prepare=||if parse{None}else{Some(value(s))};
        let run=|v:Option<Value>,cx:&mut Cx|{let v=match v{Some(v)=>v,None=>json::parse_json_bytes(s.as_bytes(),cx).map_err(app::AppDecodeError::Json)?};app::decode_value::<T>(v,mode,cx)};
        let prepared=prepare();let mut baseline=Cx::new();drop(ok(run(prepared,&mut baseline)));
        let count=baseline.growth_requests();assert_eq!(count,prefix+trace.len());assert!(count>0&&count<=512);
        for sticky in [false,true]{for n in 1..=count{
            let before=s.as_bytes().to_vec();let prepared=prepare();let mut cx=fault(n,sticky);let e=bad(run(prepared,&mut cx));
            if n<=prefix{assert!(matches!(e,app::AppDecodeError::Json(_)));}else{assert!(matches!(e,app::AppDecodeError::Allocation(_)));}
            let f=failure(e);assert_eq!(f.kind,FailureKind::Injected);assert_eq!(cx.first_failure(),Some(f));assert_eq!(cx.growth_requests(),n);
            if n>prefix{let(site,bytes)=trace[n-prefix-1];assert_eq!(f.site,site);assert_eq!(f.requested_bytes,Some(bytes));}
            assert_eq!(s.as_bytes(),before);assert!(f.requested_bytes.is_some());
        }}
        let prepared=prepare();let mut cx=fault(count+1,true);let out=ok(run(prepared,&mut cx));assert_eq!(cx.growth_requests(),count);
        assert!(crate::product_alloc::try_append_str(&mut String::new(),"x",&mut cx,Site::String).is_err());drop(out);assert_eq!(cx.growth_requests(),count+1);
    }
}
fn sweep_all(parse:bool){
    use std::mem::size_of;let e=size_of::<app::Expr>();let p=size_of::<(String,app::Expr)>();
    sweep::<app::Expr>(SWEEP_QUERY,parse,&[(Site::Box,e),(Site::Box,e),(Site::Box,e),(Site::Box,e),(Site::Box,e),(Site::Box,e),(Site::Vector,size_of::<app::SortKey>()),(Site::Box,e)]);
    sweep::<app::Expr>(SWEEP_AND,parse,&[(Site::Vector,2*e),(Site::Box,e),(Site::Vector,2*e)]);
    sweep::<app::Statement>(SWEEP_STEPS,parse,&[(Site::Vector,2*size_of::<app::Statement>()),(Site::OrderedMap,3*p),(Site::Box,e),(Site::OrderedMap,p)]);
    sweep::<app::Statement>(SWEEP_COLLECTION,parse,&[(Site::Box,e)]);
}
#[test]fn finite_parse_decode_fault_sweeps_share_one_context(){sweep_all(true);}
#[test]fn finite_decode_only_sweeps_check_independent_sites_and_requested_bytes(){sweep_all(false);}

// Test-only zero-allocation ownership observer. A fixed array watches existing
// String allocations during one closed decode; deallocation clears a slot before
// later allocator reuse. Normal tests do not arm it. The mutex only serializes
// registration, never runs inside the allocator callback or product operation.
struct WatchAllocator;
#[global_allocator]static ALLOCATOR:WatchAllocator=WatchAllocator;
static WATCH_LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
static WATCH_PTRS:[AtomicUsize;16]=[const{AtomicUsize::new(0)};16];
static WATCH_FREES:[AtomicUsize;16]=[const{AtomicUsize::new(0)};16];
static WATCH_REALLOCS:AtomicUsize=AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for WatchAllocator{
    unsafe fn alloc(&self,l:std::alloc::Layout)->*mut u8{unsafe{std::alloc::GlobalAlloc::alloc(&std::alloc::System,l)}}
    unsafe fn alloc_zeroed(&self,l:std::alloc::Layout)->*mut u8{unsafe{std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System,l)}}
    unsafe fn dealloc(&self,p:*mut u8,l:std::alloc::Layout){
        for(slot,count)in WATCH_PTRS.iter().zip(&WATCH_FREES){
            if slot.compare_exchange(p as usize,0,Ordering::Relaxed,Ordering::Relaxed).is_ok(){count.fetch_add(1,Ordering::Relaxed);}
        }
        unsafe{std::alloc::GlobalAlloc::dealloc(&std::alloc::System,p,l)}
    }
    unsafe fn realloc(&self,p:*mut u8,l:std::alloc::Layout,n:usize)->*mut u8{
        // Retire before System may free/reuse the old address. We deliberately
        // do not follow the result: ANY observed realloc fails this witness.
        for slot in &WATCH_PTRS{
            if slot.compare_exchange(p as usize,0,Ordering::Relaxed,Ordering::Relaxed).is_ok(){WATCH_REALLOCS.fetch_add(1,Ordering::Relaxed);}
        }
        unsafe{std::alloc::GlobalAlloc::realloc(&std::alloc::System,p,l,n)}
    }
}
// These witnesses cover selected live nonempty Strings, not all allocations or
// generic leak freedom. No raw pointer is ever dereferenced by the observer.
struct WatchScope;
impl WatchScope{
    fn arm(ptrs:&[usize])->Self{
        assert!(ptrs.len()<=16);assert!(WATCH_PTRS.iter().all(|p|p.load(Ordering::Relaxed)==0));
        for(i,p)in ptrs.iter().enumerate(){assert_ne!(*p,0);assert!(!ptrs[..i].contains(p));}
        let scope=Self; // From here every normal return, early exit or unwind resets.
        for(slot,count)in WATCH_PTRS.iter().zip(&WATCH_FREES){slot.store(0,Ordering::Relaxed);count.store(0,Ordering::Relaxed);}
        WATCH_REALLOCS.store(0,Ordering::Relaxed);
        for(slot,p)in WATCH_PTRS.iter().zip(ptrs){slot.store(*p,Ordering::Relaxed);}
        scope
    }
}
impl Drop for WatchScope{
    fn drop(&mut self){
        for(slot,count)in WATCH_PTRS.iter().zip(&WATCH_FREES){slot.store(0,Ordering::Relaxed);count.store(0,Ordering::Relaxed);}
        WATCH_REALLOCS.store(0,Ordering::Relaxed);
    }
}
fn gather_owned(v:&Value,ptrs:&mut[usize;16],n:&mut usize){
    fn push(s:&str,ptrs:&mut[usize;16],n:&mut usize){if s.starts_with("owned-"){assert!(*n<16);ptrs[*n]=s.as_ptr()as usize;*n+=1;}}
    match v{Value::String(s)=>push(s,ptrs,n),Value::Array(a)=>for v in a{gather_owned(v,ptrs,n)},
        Value::Object(m)=>for(k,v)in m.iter(){push(k,ptrs,n);gather_owned(v,ptrs,n)},_=>()}
}
#[test]
fn moved_dictionary_strings_and_partial_prefix_owners_drop_once(){
    let _guard=WATCH_LOCK.lock().unwrap();
    for(s,expected,success)in [(OWNED_SUCCESS,6,true),(OWNED_PREFIX,5,false)]{
        for sticky in [false,true]{for ordinal in 1..=3{
            let input=value(s);let mut ptrs=[0;16];let mut n=0;gather_owned(&input,&mut ptrs,&mut n);assert_eq!(n,expected);
            let _watch=WatchScope::arm(&ptrs[..n]);
            let mut cx=fault(ordinal,sticky);let output=app::decode_value::<app::Statement>(input,Mode::DirectValue,&mut cx);
            if ordinal<=2{
                let f=failure(bad(output));assert_eq!(f.site,if ordinal==1{Site::OrderedMap}else{Site::Box});
                assert_eq!(f.requested_bytes,Some(if ordinal==1{2*std::mem::size_of::<(String,app::Expr)>()}else{std::mem::size_of::<app::Expr>()}));
                assert_eq!(Some(f),cx.first_failure());assert_eq!(cx.growth_requests(),ordinal);
            }else if success{
                let out=ok(output);let app::Statement::Create{entity,values,bind}= &out else{panic!()};
                assert_eq!(entity.as_ptr()as usize,ptrs[0]);assert_eq!(bind.as_ptr()as usize,ptrs[n-1]);
                // Same owned key allocation reaches the typed dictionary.
                assert_eq!(values.iter().next().unwrap().0.as_ptr()as usize,ptrs[1]);
                assert_eq!(WATCH_FREES[..n].iter().map(|v|v.load(Ordering::Relaxed)).sum::<usize>(),0);
                drop(out);assert_eq!(cx.growth_requests(),2);
            }else{assert!(matches!(bad(output),app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),2);}
            for i in 0..n{assert_eq!(WATCH_PTRS[i].load(Ordering::Relaxed),0);assert_eq!(WATCH_FREES[i].load(Ordering::Relaxed),1);}
            assert_eq!(WATCH_REALLOCS.load(Ordering::Relaxed),0);
        }}
    }
    // This intentional unwind is outside request-local fault injection and
    // outside allocator callbacks. It proves no stale observation survives.
    let live=String::from("owned-unwind");let pointer=live.as_ptr()as usize;
    let unwind=std::panic::catch_unwind(||{let _watch=WatchScope::arm(&[pointer]);panic!("observer reset witness");});
    assert!(unwind.is_err());
    assert!(WATCH_PTRS.iter().all(|p|p.load(Ordering::Relaxed)==0));
    assert!(WATCH_FREES.iter().all(|p|p.load(Ordering::Relaxed)==0));assert_eq!(WATCH_REALLOCS.load(Ordering::Relaxed),0);
    drop(live);
}
#[test]
fn primary_error_survives_render_refusal_with_same_context_and_rollback(){
    for s in [RENDER_UNKNOWN,RENDER_FLOAT]{
        assert!(s.len()<=4096);let mut baseline=Cx::new();let primary=bad(app::Expr::decode_json(s.as_bytes(),&mut baseline));
        let prefix=baseline.growth_requests();let expected=primary.try_render(&mut baseline).unwrap();let count=baseline.growth_requests();assert!(count>prefix&&count<=512);
        // Every growth across parse, typed-prefix and render uses the same cx.
        for sticky in [false,true]{for n in 1..=count{
            let mut cx=fault(n,sticky);let primary=bad(app::Expr::decode_json(s.as_bytes(),&mut cx));
            if n<=prefix{let f=failure(primary);assert_eq!(cx.first_failure(),Some(f));}
            else{
                assert!(matches!(primary,app::AppDecodeError::Schema(_)));assert_eq!(cx.growth_requests(),prefix);
                assert!(matches!(primary.try_render(&mut cx),Err(FormatFailure::Allocation(f))if f.site==Site::Formatting&&Some(f)==cx.first_failure()));
                assert_eq!(primary.try_render(&mut Cx::new()).unwrap(),expected);
            }
            assert_eq!(cx.growth_requests(),n);
        }}
        // Decode-only errors own input strings without allocating a message.
        let input=value(s);let mut full=Cx::new();let cause=bad(app::decode_value::<app::Expr>(input,Mode::DirectValue,&mut full));
        let before=full.growth_requests();let expected=cause.try_render(&mut full).unwrap();let render=full.growth_requests()-before;
        for sticky in [false,true]{for n in 1..=render{
            let mut cx=fault(n,sticky);assert!(matches!(cause.try_render(&mut cx),Err(FormatFailure::Allocation(f))if f.site==Site::Formatting));
            assert_eq!(cx.growth_requests(),n);assert_eq!(cause.try_render(&mut Cx::new()).unwrap(),expected);
        }}
        let mut text=String::from("unchanged:");let mut cx=fault(1,true);assert!(cause.try_append_diagnostic(&mut text,&mut cx).is_err());assert_eq!(text,"unchanged:");
        let mut buffer=String::with_capacity(2048);let mut cx=fault(1,true);cause.try_append_diagnostic(&mut buffer,&mut cx).unwrap();assert_eq!(buffer,expected);assert_eq!(cx.growth_requests(),0);
        if let app::AppDecodeError::Schema(app::SchemaFailure::UnknownVariant{tag,..})=&cause{
            let ptr=tag.as_ptr();let cap=tag.capacity();let mut cx=fault(1,true);assert!(cause.try_render(&mut cx).is_err());
            assert!(matches!(&cause,app::AppDecodeError::Schema(app::SchemaFailure::UnknownVariant{tag,..})if (tag.as_ptr(),tag.capacity())==(ptr,cap)));
        }
    }
}
#[test]
fn z_finite_new_harness_budget_receipt(){
    // Exact aggregate fixture counts are static, and each dynamic call also
    // enforces its limit. Counters can be below the total under parallel Cargo;
    // the admitted serial aggregate additionally verifies the full receipt.
    assert_eq!(152+156+74+74+16+24,496);
    assert!(GENERATED.load(Ordering::Relaxed)<=512);assert!(INJECTED.load(Ordering::Relaxed)<=8192);
    println!("R05b budget: generated={} injected={}; caps=512/8192; R05a guarded separately",GENERATED.load(Ordering::Relaxed),INJECTED.load(Ordering::Relaxed));
}
