// R06c standalone only: one System-forwarding observer; no allocator failure injection.
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[path = "../src/product_json.rs"] mod product_json;
#[path = "../src/product_app.rs"] mod product_app;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "support/app_codec_oracle.rs"] mod oracle;
#[path = "support/canonical_json_native_oracle.rs"] mod canonical_oracle;
use product_app as app;
use product_alloc::{AllocationContext as Cx, AllocationFailure, AllocationFailureKind as Kind,
    AllocationSite as Site, ordered::{OrderedMap,OrderedSet}};
use product_json::canonical::EncodeError;
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,mem::size_of,num::NonZeroUsize,
    sync::{Mutex,atomic::{AtomicUsize,Ordering}}};

// Const native TLS is primed outside observation; emitted AND linked hooks need
// qualification for this exact binary/target. No locks, dynamic records, output,
// thread::current, assertions, callbacks, or null injection inside allocator hooks.
thread_local! { static OBSERVE: Cell<bool> = const { Cell::new(false) }; }
const RECORDS:usize=128;
static PTR:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static SIZE:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static ALIGN:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static LIVE:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static KIND:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static USED:AtomicUsize=AtomicUsize::new(0);
static BAD:AtomicUsize=AtomicUsize::new(0);
static LOCK:Mutex<()>=Mutex::new(());
struct Observer;
fn observing()->bool{match OBSERVE.try_with(Cell::get){Ok(active)=>active,Err(_)=>{BAD.fetch_add(1,Ordering::Relaxed);false}}}
fn register(p:*mut u8,n:usize,a:usize,k:usize){
 if p.is_null(){BAD.fetch_add(1,Ordering::Relaxed);return}
 let i=USED.fetch_add(1,Ordering::Relaxed);
 if i>=RECORDS{BAD.fetch_add(1,Ordering::Relaxed);return}
 PTR[i].store(p as usize,Ordering::Relaxed);SIZE[i].store(n,Ordering::Relaxed);
 ALIGN[i].store(a,Ordering::Relaxed);KIND[i].store(k,Ordering::Relaxed);LIVE[i].store(1,Ordering::Relaxed);
}
fn release(p:*mut u8,n:usize,a:usize){
 let count=USED.load(Ordering::Relaxed).min(RECORDS);
 for i in (0..count).rev(){if PTR[i].load(Ordering::Relaxed)==p as usize && LIVE[i].load(Ordering::Relaxed)==1{
  if SIZE[i].load(Ordering::Relaxed)!=n || ALIGN[i].load(Ordering::Relaxed)!=a {BAD.fetch_add(1,Ordering::Relaxed);}
  LIVE[i].store(0,Ordering::Relaxed);return
 }}
 BAD.fetch_add(1,Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Observer{
 unsafe fn alloc(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc(l)};if observing(){register(p,l.size(),l.align(),1)}p}
 unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc_zeroed(l)};if observing(){register(p,l.size(),l.align(),2)}p}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){if observing(){release(p,l.size(),l.align())}unsafe{System.dealloc(p,l)}}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{
  let q=unsafe{System.realloc(p,l,n)};if observing(){if !q.is_null(){release(p,l.size(),l.align())}register(q,n,l.align(),3)}q
 }
}
#[global_allocator]static ALLOCATOR:Observer=Observer;
struct Watch;
impl Watch{
 fn begin()->Self{OBSERVE.with(|a|a.set(false));USED.store(0,Ordering::Relaxed);BAD.store(0,Ordering::Relaxed);OBSERVE.with(|a|a.set(true));Self}
 fn pause(&self){OBSERVE.with(|a|a.set(false))}
 fn resume(&self){OBSERVE.with(|a|a.set(true))}
 fn clean(&self){self.pause();assert_eq!(BAD.load(Ordering::Relaxed),0,"observer overflow/foreign or duplicate release/null allocation");}
 fn empty(&self){self.clean();for i in 0..USED.load(Ordering::Relaxed){assert_eq!(LIVE[i].load(Ordering::Relaxed),0,"prefix allocation not released once");}}
 fn schedule(&self,s:&[(Site,usize)]){self.clean();assert_eq!(USED.load(Ordering::Relaxed),s.len());for(i,&(_,n))in s.iter().enumerate(){assert_eq!(SIZE[i].load(Ordering::Relaxed),n);assert!(matches!(KIND[i].load(Ordering::Relaxed),1|2|3));}}
}
impl Drop for Watch{fn drop(&mut self){self.pause()}}
static INJECTED:AtomicUsize=AtomicUsize::new(0);
static CASES:AtomicUsize=AtomicUsize::new(0);
fn fault(n:usize,sticky:bool)->Cx{assert!(INJECTED.fetch_add(1,Ordering::Relaxed)<8192);Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),sticky)}
#[derive(Debug,PartialEq,Eq)]struct Mark(usize,usize,usize);
type Schedule=Vec<(Site,usize)>;
trait Walk{
    type Old;
    fn same(&self,old:&Self::Old);
    fn marks(&self,out:&mut Vec<Mark>);
    fn tokens(&self)->usize;
}
macro_rules! scalar_walk{($($t:ty),+)=>{$(impl Walk for $t{type Old=Self;
 fn same(&self,b:&Self){assert_eq!(self,b)} fn marks(&self,_:&mut Vec<Mark>){}
 fn tokens(&self)->usize{1}
})+};}
scalar_walk!(bool,u32,i64,i32,usize);
impl Walk for String{type Old=String;fn same(&self,b:&String){assert_eq!(self,b)}
 fn marks(&self,o:&mut Vec<Mark>){if self.capacity()>0{o.push(Mark(self.as_ptr() as usize,self.capacity(),self.len()))}}
 fn tokens(&self)->usize{1}
}
impl<T:Walk> Walk for Vec<T>{type Old=Vec<T::Old>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){if self.capacity()>0{o.push(Mark(self.as_ptr() as usize,self.capacity(),self.len()))}for a in self{a.marks(o)}}
 fn tokens(&self)->usize{1+self.iter().map(Walk::tokens).sum::<usize>()}
}
impl<T:Walk> Walk for Box<T>{type Old=Box<T::Old>;
 fn same(&self,b:&Self::Old){(**self).same(b)}
 fn marks(&self,o:&mut Vec<Mark>){o.push(Mark((&**self)as *const T as usize,1,size_of::<T>()));(**self).marks(o)}
 fn tokens(&self)->usize{1+(**self).tokens()}
}
impl<T:Walk> Walk for Option<T>{type Old=Option<T::Old>;
 fn same(&self,b:&Self::Old){match(self,b){(Some(a),Some(b))=>a.same(b),(None,None)=>(),_=>panic!("option")}}
 fn marks(&self,o:&mut Vec<Mark>){if let Some(v)=self{v.marks(o)}}
 fn tokens(&self)->usize{1+self.as_ref().map(Walk::tokens).unwrap_or(0)}
}
impl<T:Walk> Walk for OrderedMap<String,T>{type Old=std::collections::BTreeMap<String,T::Old>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for((k,v),(a,b))in self.iter().zip(b){k.same(a);v.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){for(k,v)in self.iter(){o.push(Mark(k as *const String as usize,self.capacity(),self.len()));k.marks(o);v.marks(o)}}
 fn tokens(&self)->usize{1+self.iter().map(|(k,v)|k.tokens()+v.tokens()).sum::<usize>()}
}
impl Walk for OrderedSet<app::KeyModifier>{type Old=std::collections::BTreeSet<oracle::KeyModifier>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){for k in self.iter(){o.push(Mark(k as *const _ as usize,self.capacity(),self.len()))}}
 fn tokens(&self)->usize{1+self.len()}
}
macro_rules! structure_walk{($t:ident{$($f:ident),+})=>{impl Walk for app::$t{type Old=oracle::$t;
 fn same(&self,b:&Self::Old){let Self{$($f),+}=self; $($f.same(&b.$f);)+}
 fn marks(&self,o:&mut Vec<Mark>){let Self{$($f),+}=self; $($f.marks(o);)+}
 fn tokens(&self)->usize{let Self{$($f),+}=self;1$(+$f.tokens())+}
}};}
macro_rules! enum_walk{($t:ident{$($v:ident$({$($f:ident:$old:ident),+})?),+})=>{impl Walk for app::$t{type Old=oracle::$t;
 fn same(&self,b:&Self::Old){match(self,b){$((Self::$v$({$($f),+})?,oracle::$t::$v$({$($f:$old),+})?)=>{$($($f.same($old);)+)?},)+_=>panic!("variant mismatch")}}
 fn marks(&self,o:&mut Vec<Mark>){match self{$(Self::$v$({$($f),+})?=>{$($($f.marks(o);)+)?},)+}}
 fn tokens(&self)->usize{match self{$(Self::$v$({$($f),+})?=>1$($(+$f.tokens())+)? ,)+}}
}};}
enum_walk!(Type{Boolean,Integer,Text,Date,Reference{entity:old_entity},List{item:old_item},Optional{item:old_item}});
enum_walk!(DataValue{Null,Boolean{value:old_value},Integer{value:old_value},Text{value:old_value},Date{days:old_days},Reference{entity:old_entity,record:old_record},List{item_type:old_item_type,items:old_items}});
enum_walk!(Expr{Literal{value_type:old_value_type,value:old_value},Variable{name:old_name},Field{record:old_record,field:old_field},State{state:old_state},Today,Query{entity:old_entity,binding:old_binding,predicate:old_predicate,sort:old_sort,limit:old_limit,include_archived:old_include_archived},Filter{items:old_items,binding:old_binding,predicate:old_predicate},Map{items:old_items,binding:old_binding,value:old_value},Count{items:old_items},Any{items:old_items,binding:old_binding,predicate:old_predicate},All{items:old_items,binding:old_binding,predicate:old_predicate},Sum{items:old_items},Contains{items:old_items,value:old_value},Equal{left:old_left,right:old_right},Less{left:old_left,right:old_right},Add{left:old_left,right:old_right},Subtract{left:old_left,right:old_right},Concat{left:old_left,right:old_right},TextContains{text:old_text,search:old_search},Lower{value:old_value},And{values:old_values},Or{values:old_values},Not{value:old_value},If{condition:old_condition,then_value:old_then_value,else_value:old_else_value},Coalesce{value:old_value,fallback:old_fallback},DateAdd{date:old_date,days:old_days},DateDifference{later:old_later,earlier:old_earlier}});
structure_walk!(SortKey{value,descending});
structure_walk!(FieldDefinition{id,label,value_type});
structure_walk!(EntityDefinition{id,label,fields,unique,constraints});
structure_walk!(StateDefinition{id,label,value_type,initial});
structure_walk!(ActionDefinition{id,label,parameters,guards,steps,ensures});
enum_walk!(Statement{Create{entity:old_entity,values:old_values,bind:old_bind},Update{record:old_record,values:old_values},Archive{record:old_record},SetState{state:old_state,value:old_value},Collection{target:old_target,operation:old_operation,items:old_items},ForEach{items:old_items,binding:old_binding,steps:old_steps},Assert{condition:old_condition,message:old_message},Emit{output:old_output,items:old_items,binding:old_binding,columns:old_columns}});
enum_walk!(CollectionTarget{State{state:old_state},Field{record:old_record,field:old_field}});
enum_walk!(CollectionOperation{Insert,Remove});
enum_walk!(OutputFormat{Csv,Json});
structure_walk!(OutputDefinition{id,label,format,columns});
structure_walk!(ObservableDefinition{id,label,value});
structure_walk!(Column{id,label,value});
structure_walk!(StateControl{id,label,state,on_change});
structure_walk!(SelectionBinding{id,state,on_change});
structure_walk!(FormInput{parameter,label});
enum_walk!(ViewKind{List{entity:old_entity,rows:old_rows,columns:old_columns,controls:old_controls,selection:old_selection},Detail{entity:old_entity,record:old_record,columns:old_columns},Form{action:old_action,fields:old_fields,defaults:old_defaults}});
enum_walk!(ActionPlacement{Toolbar,Row});
structure_walk!(ActionBinding{id,label,placement,action,arguments,enabled});
enum_walk!(KeyModifier{Control,Alt,Shift,Command});
structure_walk!(KeyBinding{key,modifiers,binding});
structure_walk!(ViewDefinition{id,label,kind,actions,keys});
structure_walk!(AppDefinition{version,id,label,entities,state,actions,outputs,observables,views,initial_view});
trait Case:Walk+Sized where Self::Old:serde::de::DeserializeOwned+serde::Serialize{
 fn decode(s:&str)->Self;
 fn encode(&self,cx:&mut Cx)->Result<Vec<u8>,EncodeError>;
}
macro_rules! cases{($($t:ident),+)=>{$(impl Case for app::$t{
 fn decode(s:&str)->Self{Self::decode_json(s.as_bytes(),&mut Cx::new()).unwrap_or_else(|_|panic!("fixture decode"))}
 fn encode(&self,cx:&mut Cx)->Result<Vec<u8>,EncodeError>{self.try_canonical_bytes(cx)}
})+};}
cases!(Type,DataValue,Expr,SortKey,FieldDefinition,EntityDefinition,StateDefinition,ActionDefinition,Statement,CollectionTarget,CollectionOperation,OutputFormat,OutputDefinition,ObservableDefinition,Column,StateControl,SelectionBinding,FormInput,ViewKind,ActionPlacement,ActionBinding,KeyModifier,KeyBinding,ViewDefinition,AppDefinition);
#[derive(Clone,Copy,Debug)]enum Family{Type,DataValue,Expr,SortKey,FieldDefinition,EntityDefinition,StateDefinition,ActionDefinition,Statement,CollectionTarget,CollectionOperation,OutputFormat,OutputDefinition,ObservableDefinition,Column,StateControl,SelectionBinding,FormInput,ViewKind,ActionPlacement,ActionBinding,KeyModifier,KeyBinding,ViewDefinition,AppDefinition}
struct Fixture{family:Family,wire:&'static str,changes:&'static[(&'static str,&'static str)]}
const FIXTURES:&[Fixture]=&[
Fixture{family:Family::Type,wire:r###"{"kind":"boolean"}"###,changes:&[]},
Fixture{family:Family::Type,wire:r###"{"kind":"integer"}"###,changes:&[]},
Fixture{family:Family::Type,wire:r###"{"kind":"text"}"###,changes:&[]},
Fixture{family:Family::Type,wire:r###"{"kind":"date"}"###,changes:&[]},
Fixture{family:Family::Type,wire:r###"{"kind":"reference","entity":"é\\\"\n中"}"###,changes:&[("entity",r###""other""###)]},
Fixture{family:Family::Type,wire:r###"{"kind":"list","item":{"kind":"text"}}"###,changes:&[("item",r###"{"kind":"integer"}"###)]},
Fixture{family:Family::Type,wire:r###"{"kind":"optional","item":{"kind":"text"}}"###,changes:&[("item",r###"{"kind":"integer"}"###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"null"}"###,changes:&[]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"boolean","value":true}"###,changes:&[("value",r###"false"###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"integer","value":-9223372036854775808}"###,changes:&[("value",r###"-1"###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"text","value":"é\\\"\n中"}"###,changes:&[("value",r###""other""###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"date","days":-2147483648}"###,changes:&[("days",r###"-1"###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"reference","entity":"é\\\"\n中","record":"é\\\"\n中"}"###,changes:&[("entity",r###""other""###),("record",r###""other""###)]},
Fixture{family:Family::DataValue,wire:r###"{"kind":"list","item_type":{"kind":"text"},"items":[{"kind":"text","value":"é\\\"\n中"}]}"###,changes:&[("item_type",r###"{"kind":"integer"}"###),("items",r###"[]"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"é\\\"\n中"}}"###,changes:&[("value_type",r###"{"kind":"integer"}"###),("value",r###"{"kind":"integer","value":-1}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"variable","name":"é\\\"\n中"}"###,changes:&[("name",r###""other""###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"field","record":{"kind":"variable","name":"é\\\"\n中"},"field":"é\\\"\n中"}"###,changes:&[("record",r###"{"kind":"state","state":"other"}"###),("field",r###""other""###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"state","state":"é\\\"\n中"}"###,changes:&[("state",r###""other""###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"today"}"###,changes:&[]},
Fixture{family:Family::Expr,wire:r###"{"kind":"query","entity":"é\\\"\n中","binding":"é\\\"\n中","predicate":{"kind":"variable","name":"é\\\"\n中"},"sort":[{"value":{"kind":"variable","name":"é\\\"\n中"},"descending":true}],"limit":1,"include_archived":true}"###,changes:&[("entity",r###""other""###),("binding",r###""other""###),("predicate",r###"{"kind":"state","state":"other"}"###),("sort",r###"[]"###),("limit",r###"2"###),("include_archived",r###"false"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"filter","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","predicate":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("predicate",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"map","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"count","items":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"any","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","predicate":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("predicate",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"all","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","predicate":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("predicate",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"sum","items":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"contains","items":{"kind":"variable","name":"é\\\"\n中"},"value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"equal","left":{"kind":"variable","name":"é\\\"\n中"},"right":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("left",r###"{"kind":"state","state":"other"}"###),("right",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"less","left":{"kind":"variable","name":"é\\\"\n中"},"right":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("left",r###"{"kind":"state","state":"other"}"###),("right",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"add","left":{"kind":"variable","name":"é\\\"\n中"},"right":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("left",r###"{"kind":"state","state":"other"}"###),("right",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"subtract","left":{"kind":"variable","name":"é\\\"\n中"},"right":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("left",r###"{"kind":"state","state":"other"}"###),("right",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"concat","left":{"kind":"variable","name":"é\\\"\n中"},"right":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("left",r###"{"kind":"state","state":"other"}"###),("right",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"text_contains","text":{"kind":"variable","name":"é\\\"\n中"},"search":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("text",r###"{"kind":"state","state":"other"}"###),("search",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"lower","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"and","values":[{"kind":"variable","name":"é\\\"\n中"}]}"###,changes:&[("values",r###"[]"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"or","values":[{"kind":"variable","name":"é\\\"\n中"}]}"###,changes:&[("values",r###"[]"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"not","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"if","condition":{"kind":"variable","name":"é\\\"\n中"},"then_value":{"kind":"variable","name":"é\\\"\n中"},"else_value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("condition",r###"{"kind":"state","state":"other"}"###),("then_value",r###"{"kind":"state","state":"other"}"###),("else_value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"coalesce","value":{"kind":"variable","name":"é\\\"\n中"},"fallback":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("value",r###"{"kind":"state","state":"other"}"###),("fallback",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"date_add","date":{"kind":"variable","name":"é\\\"\n中"},"days":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("date",r###"{"kind":"state","state":"other"}"###),("days",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Expr,wire:r###"{"kind":"date_difference","later":{"kind":"variable","name":"é\\\"\n中"},"earlier":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("later",r###"{"kind":"state","state":"other"}"###),("earlier",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::SortKey,wire:r###"{"value":{"kind":"variable","name":"é\\\"\n中"},"descending":true}"###,changes:&[("value",r###"{"kind":"state","state":"other"}"###),("descending",r###"false"###)]},
Fixture{family:Family::FieldDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"}}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("value_type",r###"{"kind":"integer"}"###)]},
Fixture{family:Family::EntityDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","fields":[{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"}}],"unique":[["é\\\"\n中"]],"constraints":[{"kind":"variable","name":"é\\\"\n中"}]}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("fields",r###"[]"###),("unique",r###"[]"###),("constraints",r###"[]"###)]},
Fixture{family:Family::StateDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"},"initial":{"kind":"text","value":"é\\\"\n中"}}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("value_type",r###"{"kind":"integer"}"###),("initial",r###"{"kind":"integer","value":-1}"###)]},
Fixture{family:Family::ActionDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","parameters":{"k":{"kind":"text"}},"guards":[{"kind":"variable","name":"é\\\"\n中"}],"steps":[{"kind":"archive","record":{"kind":"variable","name":"é\\\"\n中"}}],"ensures":[{"kind":"variable","name":"é\\\"\n中"}]}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("parameters",r###"{"z":{"kind":"text"}}"###),("guards",r###"[]"###),("steps",r###"[]"###),("ensures",r###"[]"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"create","entity":"é\\\"\n中","values":{"k":{"kind":"variable","name":"é\\\"\n中"}},"bind":"é\\\"\n中"}"###,changes:&[("entity",r###""other""###),("values",r###"{"z":{"kind":"variable","name":"é\\\"\n中"}}"###),("bind",r###""other""###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"update","record":{"kind":"variable","name":"é\\\"\n中"},"values":{"k":{"kind":"variable","name":"é\\\"\n中"}}}"###,changes:&[("record",r###"{"kind":"state","state":"other"}"###),("values",r###"{"z":{"kind":"variable","name":"é\\\"\n中"}}"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"archive","record":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("record",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"set_state","state":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("state",r###""other""###),("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"collection","target":{"kind":"state","state":"é\\\"\n中"},"operation":"insert","items":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("target",r###"{"kind":"field","record":{"kind":"state","state":"other"},"field":"other"}"###),("operation",r###""remove""###),("items",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"for_each","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","steps":[{"kind":"archive","record":{"kind":"variable","name":"é\\\"\n中"}}]}"###,changes:&[("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("steps",r###"[]"###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"assert","condition":{"kind":"variable","name":"é\\\"\n中"},"message":"é\\\"\n中"}"###,changes:&[("condition",r###"{"kind":"state","state":"other"}"###),("message",r###""other""###)]},
Fixture{family:Family::Statement,wire:r###"{"kind":"emit","output":"é\\\"\n中","items":{"kind":"variable","name":"é\\\"\n中"},"binding":"é\\\"\n中","columns":{"k":{"kind":"variable","name":"é\\\"\n中"}}}"###,changes:&[("output",r###""other""###),("items",r###"{"kind":"state","state":"other"}"###),("binding",r###""other""###),("columns",r###"{"z":{"kind":"variable","name":"é\\\"\n中"}}"###)]},
Fixture{family:Family::CollectionTarget,wire:r###"{"kind":"state","state":"é\\\"\n中"}"###,changes:&[("state",r###""other""###)]},
Fixture{family:Family::CollectionTarget,wire:r###"{"kind":"field","record":{"kind":"variable","name":"é\\\"\n中"},"field":"é\\\"\n中"}"###,changes:&[("record",r###"{"kind":"state","state":"other"}"###),("field",r###""other""###)]},
Fixture{family:Family::CollectionOperation,wire:r###""insert""###,changes:&[]},
Fixture{family:Family::CollectionOperation,wire:r###""remove""###,changes:&[]},
Fixture{family:Family::OutputFormat,wire:r###""csv""###,changes:&[]},
Fixture{family:Family::OutputFormat,wire:r###""json""###,changes:&[]},
Fixture{family:Family::OutputDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","format":"csv","columns":[{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"}}]}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("format",r###""json""###),("columns",r###"[]"###)]},
Fixture{family:Family::ObservableDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::Column,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("value",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::StateControl,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","state":"é\\\"\n中","on_change":"é\\\"\n中"}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("state",r###""other""###),("on_change",r###"null"###)]},
Fixture{family:Family::SelectionBinding,wire:r###"{"id":"é\\\"\n中","state":"é\\\"\n中","on_change":"é\\\"\n中"}"###,changes:&[("id",r###""other""###),("state",r###""other""###),("on_change",r###"null"###)]},
Fixture{family:Family::FormInput,wire:r###"{"parameter":"é\\\"\n中","label":"é\\\"\n中"}"###,changes:&[("parameter",r###""other""###),("label",r###""other""###)]},
Fixture{family:Family::ViewKind,wire:r###"{"kind":"list","entity":"é\\\"\n中","rows":{"kind":"variable","name":"é\\\"\n中"},"columns":[{"id":"é\\\"\n中","label":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}],"controls":[{"id":"é\\\"\n中","label":"é\\\"\n中","state":"é\\\"\n中","on_change":"é\\\"\n中"}],"selection":{"id":"é\\\"\n中","state":"é\\\"\n中","on_change":"é\\\"\n中"}}"###,changes:&[("entity",r###""other""###),("rows",r###"{"kind":"state","state":"other"}"###),("columns",r###"[]"###),("controls",r###"[]"###),("selection",r###"null"###)]},
Fixture{family:Family::ViewKind,wire:r###"{"kind":"detail","entity":"é\\\"\n中","record":{"kind":"variable","name":"é\\\"\n中"},"columns":[{"id":"é\\\"\n中","label":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}]}"###,changes:&[("entity",r###""other""###),("record",r###"{"kind":"state","state":"other"}"###),("columns",r###"[]"###)]},
Fixture{family:Family::ViewKind,wire:r###"{"kind":"form","action":"é\\\"\n中","fields":[{"parameter":"é\\\"\n中","label":"é\\\"\n中"}],"defaults":{"k":{"kind":"text","value":"é\\\"\n中"}}}"###,changes:&[("action",r###""other""###),("fields",r###"[]"###),("defaults",r###"{"z":{"kind":"text","value":"é\\\"\n中"}}"###)]},
Fixture{family:Family::ActionPlacement,wire:r###""toolbar""###,changes:&[]},
Fixture{family:Family::ActionPlacement,wire:r###""row""###,changes:&[]},
Fixture{family:Family::ActionBinding,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","placement":"toolbar","action":"é\\\"\n中","arguments":{"k":{"kind":"variable","name":"é\\\"\n中"}},"enabled":{"kind":"variable","name":"é\\\"\n中"}}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("placement",r###""row""###),("action",r###""other""###),("arguments",r###"{"z":{"kind":"variable","name":"é\\\"\n中"}}"###),("enabled",r###"{"kind":"state","state":"other"}"###)]},
Fixture{family:Family::KeyModifier,wire:r###""control""###,changes:&[]},
Fixture{family:Family::KeyModifier,wire:r###""alt""###,changes:&[]},
Fixture{family:Family::KeyModifier,wire:r###""shift""###,changes:&[]},
Fixture{family:Family::KeyModifier,wire:r###""command""###,changes:&[]},
Fixture{family:Family::KeyBinding,wire:r###"{"key":"é\\\"\n中","modifiers":["control","alt"],"binding":"é\\\"\n中"}"###,changes:&[("key",r###""other""###),("modifiers",r###"["shift"]"###),("binding",r###""other""###)]},
Fixture{family:Family::ViewDefinition,wire:r###"{"id":"é\\\"\n中","label":"é\\\"\n中","kind":{"kind":"form","action":"é\\\"\n中","fields":[{"parameter":"é\\\"\n中","label":"é\\\"\n中"}],"defaults":{"k":{"kind":"text","value":"é\\\"\n中"}}},"actions":[{"id":"é\\\"\n中","label":"é\\\"\n中","placement":"toolbar","action":"é\\\"\n中","arguments":{"k":{"kind":"variable","name":"é\\\"\n中"}},"enabled":{"kind":"variable","name":"é\\\"\n中"}}],"keys":[{"key":"é\\\"\n中","modifiers":["control","alt"],"binding":"é\\\"\n中"}]}"###,changes:&[("id",r###""other""###),("label",r###""other""###),("kind",r###"{"kind":"detail","entity":"other","record":{"kind":"state","state":"other"},"columns":[]}"###),("actions",r###"[]"###),("keys",r###"[]"###)]},
Fixture{family:Family::AppDefinition,wire:r###"{"version":1,"id":"é\\\"\n中","label":"é\\\"\n中","entities":[{"id":"é\\\"\n中","label":"é\\\"\n中","fields":[{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"}}],"unique":[["é\\\"\n中"]],"constraints":[{"kind":"variable","name":"é\\\"\n中"}]}],"state":[{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"},"initial":{"kind":"text","value":"é\\\"\n中"}}],"actions":[{"id":"é\\\"\n中","label":"é\\\"\n中","parameters":{"k":{"kind":"text"}},"guards":[{"kind":"variable","name":"é\\\"\n中"}],"steps":[{"kind":"archive","record":{"kind":"variable","name":"é\\\"\n中"}}],"ensures":[{"kind":"variable","name":"é\\\"\n中"}]}],"outputs":[{"id":"é\\\"\n中","label":"é\\\"\n中","format":"csv","columns":[{"id":"é\\\"\n中","label":"é\\\"\n中","value_type":{"kind":"text"}}]}],"observables":[{"id":"é\\\"\n中","label":"é\\\"\n中","value":{"kind":"variable","name":"é\\\"\n中"}}],"views":[{"id":"é\\\"\n中","label":"é\\\"\n中","kind":{"kind":"form","action":"é\\\"\n中","fields":[{"parameter":"é\\\"\n中","label":"é\\\"\n中"}],"defaults":{"k":{"kind":"text","value":"é\\\"\n中"}}},"actions":[{"id":"é\\\"\n中","label":"é\\\"\n中","placement":"toolbar","action":"é\\\"\n中","arguments":{"k":{"kind":"variable","name":"é\\\"\n中"}},"enabled":{"kind":"variable","name":"é\\\"\n中"}}],"keys":[{"key":"é\\\"\n中","modifiers":["control","alt"],"binding":"é\\\"\n中"}]}],"initial_view":"é\\\"\n中"}"###,changes:&[("version",r###"2"###),("id",r###""other""###),("label",r###""other""###),("entities",r###"[]"###),("state",r###"[]"###),("actions",r###"[]"###),("outputs",r###"[]"###),("observables",r###"[]"###),("views",r###"[]"###),("initial_view",r###""other""###)]},
];
// Independent oracle container depth: nonempty objects/arrays need an active
// cursor; the closed modifier set is emitted inline (at most four scalars).
fn depth(v:&serde_json::Value)->usize{match v{
 serde_json::Value::Object(m) if !m.is_empty()=>1+m.iter().map(|(k,v)|{
  if k=="modifiers" && m.len()==3 && m.get("key").is_some_and(|x|x.is_string()) && m.get("binding").is_some_and(|x|x.is_string()){0}else{depth(v)}
 }).max().unwrap_or(0),
 serde_json::Value::Array(a) if !a.is_empty()=>1+a.iter().map(depth).max().unwrap_or(0),_=>0,
}}
fn bounds(v:&serde_json::Value,d:usize,n:&mut usize){*n+=1;assert!(*n<=128 && d<=8);match v{serde_json::Value::Array(a)=>for x in a{bounds(x,d+1,n)},serde_json::Value::Object(a)=>for x in a.values(){bounds(x,d+1,n)},_=>()}}
fn schedule(d:usize,len:usize)->Schedule{
 let mut result=Vec::new();let mut capacity=0;
 while capacity<d{capacity=if capacity==0{1}else{capacity*2};result.push((Site::Vector,capacity*app::ENCODE_FRAME_BYTES))}
 result.push((Site::Bytes,len));assert!(result.len()<=32);result
}
fn success<T:Case>(source:&T,expected:&[u8],requests:&Schedule)where T::Old:serde::de::DeserializeOwned+serde::Serialize{
 let mut cx=Cx::new();let watch=Watch::begin();let result=source.encode(&mut cx);watch.clean();
 let bytes=result.unwrap();assert_eq!(bytes,expected);assert_eq!(cx.growth_requests(),requests.len());assert_eq!(cx.first_failure(),None);watch.schedule(requests);
 // Only the returned byte owner may survive the operation, until caller Drop.
 let used=USED.load(Ordering::Relaxed);assert!(used>0);
 for i in 0..used{assert_eq!(LIVE[i].load(Ordering::Relaxed),usize::from(i+1==used));}
 assert_eq!(PTR[used-1].load(Ordering::Relaxed),bytes.as_ptr()as usize);
 watch.resume();drop(bytes);watch.empty();assert_eq!(USED.load(Ordering::Relaxed),used);
}
fn injected(e:EncodeError,cx:&Cx,ordinal:usize,want:(Site,usize)){
 let expected=AllocationFailure{kind:Kind::Injected,site:want.0,requested_bytes:Some(want.1)};
 assert_eq!(e,EncodeError::Allocation(expected));assert_eq!(cx.growth_requests(),ordinal);assert_eq!(cx.first_failure(),Some(expected));
}
fn sweep<T:Case>(source:&T,old:&T::Old,expected:&[u8],requests:&Schedule)where T::Old:serde::de::DeserializeOwned+serde::Serialize{
 let mut before=Vec::new();source.marks(&mut before);
 for sticky in [false,true]{for (i,&want)in requests.iter().enumerate(){
  let mut cx=fault(i+1,sticky);let watch=Watch::begin();let result=source.encode(&mut cx);watch.empty();
  injected(result.unwrap_err(),&cx,i+1,want);watch.schedule(&requests[..i]);
  source.same(old);let mut after=Vec::new();source.marks(&mut after);assert_eq!(before,after);
 }}
 success(source,expected,requests);source.same(old);let mut after=Vec::new();source.marks(&mut after);assert_eq!(before,after);
}
fn parity<T:Case>(wire:&str,failures:bool)->Vec<u8>where T::Old:serde::de::DeserializeOwned+serde::Serialize{
 assert!(CASES.fetch_add(1,Ordering::Relaxed)<768);assert!(wire.len()<=16384);
 let value:serde_json::Value=serde_json::from_str(wire).unwrap();bounds(&value,0,&mut 0);
 let source=T::decode(wire);assert!(source.tokens()<=128);
 let old:T::Old=serde_json::from_str(wire).unwrap();let expected=canonical_oracle::canonical_bytes(&old).unwrap();assert!(expected.len()<=16384);
 let requests=schedule(depth(&serde_json::to_value(&old).unwrap()),expected.len());
 let mut before=Vec::new();source.marks(&mut before);
 success(&source,&expected,&requests);
 if failures{sweep(&source,&old,&expected,&requests)}
 source.same(&old);let mut after=Vec::new();source.marks(&mut after);assert_eq!(before,after);expected
}
macro_rules! dispatch_cases{($($t:ident),+)=>{fn dispatch(f:Family,wire:&str,sweep:bool)->Vec<u8>{match f{$(Family::$t=>parity::<app::$t>(wire,sweep)),+}}};}
dispatch_cases!(Type,DataValue,Expr,SortKey,FieldDefinition,EntityDefinition,StateDefinition,ActionDefinition,Statement,CollectionTarget,CollectionOperation,OutputFormat,OutputDefinition,ObservableDefinition,Column,StateControl,SelectionBinding,FormInput,ViewKind,ActionPlacement,ActionBinding,KeyModifier,KeyBinding,ViewDefinition,AppDefinition);
fn fixture(f:Family,tag:Option<&str>)->serde_json::Value{
 let found=FIXTURES.iter().find(|x|std::mem::discriminant(&x.family)==std::mem::discriminant(&f) && tag.map_or(true,|tag|serde_json::from_str::<serde_json::Value>(x.wire).unwrap()["kind"]==tag)).unwrap();
 serde_json::from_str(found.wire).unwrap()
}
#[test]fn all_79_shapes_161_field_mutations_and_every_growth(){
 let _serial=LOCK.lock().unwrap();let start=CASES.load(Ordering::Relaxed);let faults=INJECTED.load(Ordering::Relaxed);let mut fields=0;
 assert_eq!(FIXTURES.len(),79);
 for f in FIXTURES{
  let base=dispatch(f.family,f.wire,true);
  for &(name,changed)in f.changes{
   let mut wire:serde_json::Value=serde_json::from_str(f.wire).unwrap();
   assert!(wire.as_object().unwrap().contains_key(name));wire[name]=serde_json::from_str(changed).unwrap();
   let after=dispatch(f.family,&wire.to_string(),false);assert_ne!(base,after,"lost {name}");fields+=1;
  }
 }
 assert_eq!(fields,161);assert_eq!(CASES.load(Ordering::Relaxed)-start,240);assert_eq!(INJECTED.load(Ordering::Relaxed)-faults,434);
}
// Mutate only the selected container; minimize unrelated App siblings so every
// shallow case fits the frozen 128-token envelope, including double View nodes.
#[test]fn all_18_vectors_order_and_empty_single_multiple(){
 let _serial=LOCK.lock().unwrap();let start=CASES.load(Ordering::Relaxed);
 let shapes=[
 (Family::DataValue,Some("list"),"/items"),(Family::EntityDefinition,None,"/fields"),
 (Family::EntityDefinition,None,"/unique"),(Family::EntityDefinition,None,"/unique/0"),
 (Family::Expr,Some("and"),"/values"),(Family::Expr,Some("query"),"/sort"),
 (Family::ActionDefinition,None,"/steps"),(Family::ViewKind,Some("list"),"/columns"),
 (Family::ViewKind,Some("list"),"/controls"),(Family::ViewKind,Some("form"),"/fields"),
 (Family::ViewDefinition,None,"/actions"),(Family::ViewDefinition,None,"/keys"),
 (Family::AppDefinition,None,"/entities"),(Family::AppDefinition,None,"/state"),
 (Family::AppDefinition,None,"/actions"),(Family::AppDefinition,None,"/outputs"),
 (Family::AppDefinition,None,"/observables"),(Family::AppDefinition,None,"/views")];
 for(f,tag,path)in shapes{
  let mut v=fixture(f,tag);let first=v.pointer(path).unwrap()[0].clone();let mut second=first.clone();
  if let Some(s)=second.as_str(){second=serde_json::Value::String(format!("{s}2"))}
  else if let Some(a)=second.as_array_mut(){a.push(serde_json::json!("second"))}
  else{let m=second.as_object_mut().unwrap();let key=if m.contains_key("id"){"id"}else if m.contains_key("binding"){"binding"}else if m.contains_key("parameter"){"parameter"}else if m.contains_key("value"){"value"}else if m.contains_key("record"){"record"}else{"kind"};
   if key=="value" || key=="record"{m.insert(key.into(),serde_json::json!({"kind":"today"}));}
   else if key=="kind"{second=serde_json::json!({"kind":"today"});}
   else{m.insert(key.into(),serde_json::json!("second"));}}
  // DataValue cannot use Expr::Today; its fixture is text, and value is scalar.
  if matches!(f,Family::DataValue){second=serde_json::json!({"kind":"text","value":"second"});}
  if matches!(f,Family::AppDefinition){for k in ["entities","state","actions","outputs","observables","views"]{v[k]=serde_json::json!([])}}
  let mut enc=Vec::new();
  for a in [vec![],vec![first.clone()],vec![first.clone(),second.clone()]]{
   *v.pointer_mut(path).unwrap()=serde_json::Value::Array(a);enc.push(dispatch(f,&v.to_string(),false));
  }
  assert_ne!(enc[0],enc[1]);assert_ne!(enc[1],enc[2]);
  *v.pointer_mut(path).unwrap()=serde_json::json!([second,first]);assert_ne!(enc[2],dispatch(f,&v.to_string(),false));
 }
 assert_eq!(CASES.load(Ordering::Relaxed)-start,72);
}
#[test]fn maps_boxes_options_and_modifier_declaration_order(){
 let _serial=LOCK.lock().unwrap();let start=CASES.load(Ordering::Relaxed);
 for(f,tag,path)in [(Family::ActionDefinition,None,"parameters"),(Family::Statement,Some("create"),"values"),(Family::ViewKind,Some("form"),"defaults")]{
  let mut v=fixture(f,tag);let item=v[path].as_object().unwrap().values().next().unwrap().clone();
  for keys in [&[][..],&["kind"][..],&["label","kind"][..]]{let mut m=serde_json::Map::new();for k in keys{m.insert((*k).into(),item.clone());}v[path]=m.into();dispatch(f,&v.to_string(),false);}
 }
 for(f,tag)in [(Family::Type,"optional"),(Family::Expr,"not")]{let key=if matches!(f,Family::Type){"item"}else{"value"};let mut v=if matches!(f,Family::Type){serde_json::json!({"kind":"text"})}else{serde_json::json!({"kind":"today"})};for _ in 0..3{dispatch(f,&v.to_string(),false);v=serde_json::json!({"kind":tag,key:v});}}
 for(f,tag,key)in [(Family::StateControl,None,"on_change"),(Family::SelectionBinding,None,"on_change"),(Family::ViewKind,Some("list"),"selection")]{
  let mut v=fixture(f,tag);let some=dispatch(f,&v.to_string(),false);v[key]=serde_json::Value::Null;let null=dispatch(f,&v.to_string(),false);v.as_object_mut().unwrap().remove(key);assert_eq!(null,dispatch(f,&v.to_string(),false));assert_ne!(some,null);
 }
 for bits in 0..16{
  let mut a=app::KeyBinding{key:String::new(),binding:String::new(),modifiers:OrderedSet::new()};let mut b=app::KeyBinding{key:String::new(),binding:String::new(),modifiers:OrderedSet::new()};
  let mods=[app::KeyModifier::Control,app::KeyModifier::Alt,app::KeyModifier::Shift,app::KeyModifier::Command];let words=["control","alt","shift","command"];
  let mut expected=String::from("{\"binding\":\"\",\"key\":\"\",\"modifiers\":[");let mut comma=false;
  for i in 0..4{if bits&(1<<i)!=0{a.modifiers.try_insert(mods[i],&mut Cx::new()).unwrap();if comma{expected.push(',')}expected.push('"');expected.push_str(words[i]);expected.push('"');comma=true;}}
  b.modifiers.try_reserve(9,&mut Cx::new()).unwrap();for i in (0..4).rev(){if bits&(1<<i)!=0{b.modifiers.try_insert(mods[i],&mut Cx::new()).unwrap();}}
  expected.push_str("]}");let s=schedule(1,expected.len());success(&a,expected.as_bytes(),&s);success(&b,expected.as_bytes(),&s);
 }
 // Real insertion history and spare capacity in all three concrete map shapes.
 macro_rules! insertion{($ty:ty,$value:expr,$root:expr)=>{{
  let mut a:OrderedMap<String,$ty>=OrderedMap::new();let mut b:OrderedMap<String,$ty>=OrderedMap::new();b.try_reserve(17,&mut Cx::new()).unwrap();
  for k in ["","kind","label","é"]{a.try_insert(k.into(),$value,&mut Cx::new()).unwrap();}
  for k in ["é","label","kind",""]{b.try_insert(k.into(),$value,&mut Cx::new()).unwrap();}
  let make=$root;let a=make(a);let b=make(b);assert_eq!(a.encode(&mut Cx::new()).unwrap(),b.encode(&mut Cx::new()).unwrap());
 }}}
 insertion!(app::Type,app::Type::Text,|parameters|app::ActionDefinition{id:String::new(),label:String::new(),parameters,guards:vec![],steps:vec![],ensures:vec![]});
 insertion!(app::Expr,app::Expr::Today,|values|app::Statement::Create{entity:String::new(),values,bind:String::new()});
 insertion!(app::DataValue,app::DataValue::Null,|defaults|app::ViewKind::Form{action:String::new(),fields:vec![],defaults});
 assert_eq!(CASES.load(Ordering::Relaxed)-start,24);
}
#[test]fn exact_literals_strings_keys_integer_widths_and_unvalidated_data(){
 let _serial=LOCK.lock().unwrap();let start=CASES.load(Ordering::Relaxed);
 for(f,wire,expected)in [
  (Family::StateControl,r#"{"state":"","on_change":null,"label":"L","id":""}"#,r#"{"id":"","label":"L","on_change":null,"state":""}"#),
  (Family::Type,r#"{"kind":"reference","entity":"bad id"}"#,r#"{"entity":"bad id","kind":"reference"}"#),
  (Family::ViewDefinition,r#"{"id":"","label":"L","kind":{"kind":"list","entity":"","rows":{"kind":"today"},"columns":[],"controls":[],"selection":null},"actions":[],"keys":[]}"#,r#"{"actions":[],"id":"","keys":[],"kind":{"columns":[],"controls":[],"entity":"","kind":"list","rows":{"kind":"today"},"selection":null},"label":"L"}"#),
 ]{assert_eq!(dispatch(f,wire,false),expected.as_bytes())}
 let mut text=String::new();for b in 0..32u8{text.push(char::from(b))}text.push_str("\"\\/\u{7f}e\u{301}é😀\u{2028}\u{2029}<>&");
 let escaped=r#"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e\u000f\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f\"\\/"#;
 let wire=serde_json::json!({"kind":"text","value":text}).to_string();let got=parity::<app::DataValue>(&wire,false);let expected=format!("{{\"kind\":\"text\",\"value\":\"{escaped}\u{7f}e\u{301}é😀\u{2028}\u{2029}<>&\"}}");assert_eq!(got,expected.as_bytes());
 for(f,tag,key)in [(Family::ActionDefinition,None,"parameters"),(Family::Statement,Some("create"),"values"),(Family::ViewKind,Some("form"),"defaults")]{let mut v=fixture(f,tag);let item=v[key].as_object().unwrap().values().next().unwrap().clone();let mut m=serde_json::Map::new();for s in ["","a","aa","kind","label","value","é","é","😀","\n","\"","\\",text.as_str()]{m.insert(s.into(),item.clone());}v[key]=m.into();dispatch(f,&v.to_string(),false);}
 for n in [i64::MIN,-9007199254740993,-1,0,1,9007199254740993,i64::MAX]{parity::<app::DataValue>(&format!("{{\"kind\":\"integer\",\"value\":{n}}}"),false);}
 for n in [i32::MIN,-1,0,1,i32::MAX]{parity::<app::DataValue>(&format!("{{\"kind\":\"date\",\"days\":{n}}}"),false);}
 for n in [0,1,u32::MAX]{let mut v=fixture(Family::AppDefinition,None);v["version"]=serde_json::json!(n);parity::<app::AppDefinition>(&v.to_string(),false);}
 for n in [0,1,usize::MAX]{let mut v=fixture(Family::Expr,Some("query"));v["limit"]=serde_json::json!(n);parity::<app::Expr>(&v.to_string(),false);}
 for b in [false,true]{parity::<app::DataValue>(&format!("{{\"kind\":\"boolean\",\"value\":{b}}}"),false);}
 // Exact key sorting is before escaping: LF precedes quote, backslash, then a.
 let v=r#"{"kind":"form","action":"","fields":[],"defaults":{"a":{"kind":"null"},"\\":{"kind":"null"},"\"":{"kind":"null"},"\n":{"kind":"null"}}}"#;
 assert_eq!(parity::<app::ViewKind>(v,false),br#"{"action":"","defaults":{"\n":{"kind":"null"},"\"":{"kind":"null"},"\\":{"kind":"null"},"a":{"kind":"null"}},"fields":[],"kind":"form"}"#);
 assert_eq!(CASES.load(Ordering::Relaxed)-start,28);
}
#[test]fn accepted_sequences_tags_and_unit_extras_normalize(){
 let _serial=LOCK.lock().unwrap();let start=CASES.load(Ordering::Relaxed);
 for(f,wire,literal)in [
  (Family::FieldDefinition,r#"["i","label",{"kind":"text"}]"#,r#"{"id":"i","label":"label","value_type":{"kind":"text"}}"#),
  (Family::SortKey,r#"[{"kind":"today"},false]"#,r#"{"descending":false,"value":{"kind":"today"}}"#),
  (Family::StateControl,r#"["i","label","s",null]"#,r#"{"id":"i","label":"label","on_change":null,"state":"s"}"#),
  (Family::Type,r#"{"kind":"text","ignored":{"x":1}}"#,r#"{"kind":"text"}"#),
  (Family::Expr,r#"{"kind":"today","unused":[1]}"#,r#"{"kind":"today"}"#),
  (Family::OutputFormat,r#"{"csv":null}"#,r#""csv""#),
  (Family::ActionPlacement,r#"{"row":null}"#,r#""row""#),
  (Family::CollectionOperation,r#"{"remove":null}"#,r#""remove""#),
  (Family::KeyModifier,r#"{"command":null}"#,r#""command""#),
 ]{assert_eq!(dispatch(f,wire,false),literal.as_bytes())}
 assert_eq!(CASES.load(Ordering::Relaxed)-start,9);
}
fn stress<T:Case>(source:T,old:T::Old)where T::Old:serde::de::DeserializeOwned+serde::Serialize{
 let expected=canonical_oracle::canonical_bytes(&old).unwrap();assert!(expected.len()<=2*1024*1024);
 let requests=schedule(depth(&serde_json::to_value(&old).unwrap()),expected.len());sweep(&source,&old,&expected,&requests);
}
#[test]fn eight_stress_fixtures_include_unbounded_text_and_width(){
 let _serial=LOCK.lock().unwrap();let faults=INJECTED.load(Ordering::Relaxed);
 let text="x".repeat(1024*1024+1);stress(app::DataValue::Text{value:text.clone()},oracle::DataValue::Text{value:text});
 stress(app::Expr::And{values:(0..1024).map(|_|app::Expr::Today).collect()},oracle::Expr::And{values:(0..1024).map(|_|oracle::Expr::Today).collect()});
 for(f,tag,key)in [(Family::ActionDefinition,None,"parameters"),(Family::Statement,Some("create"),"values"),(Family::ViewKind,Some("form"),"defaults")]{
  let mut v=fixture(f,tag);let item=v[key].as_object().unwrap().values().next().unwrap().clone();let mut map=serde_json::Map::new();for i in (0..64).rev(){map.insert(format!("k{i:03}"),item.clone());}v[key]=map.into();let wire=v.to_string();
  match f{Family::ActionDefinition=>stress(app::ActionDefinition::decode(&wire),serde_json::from_str::<oracle::ActionDefinition>(&wire).unwrap()),Family::Statement=>stress(app::Statement::decode(&wire),serde_json::from_str::<oracle::Statement>(&wire).unwrap()),Family::ViewKind=>stress(app::ViewKind::decode(&wire),serde_json::from_str::<oracle::ViewKind>(&wire).unwrap()),_=>unreachable!()}
 }
 let mut v=fixture(Family::Expr,Some("query"));let item=v["sort"][0].clone();v["sort"]=serde_json::Value::Array(vec![item;128]);let wire=v.to_string();stress(app::Expr::decode(&wire),serde_json::from_str::<oracle::Expr>(&wire).unwrap());
 let mut t=app::Type::Text;let mut old=oracle::Type::Text;for _ in 0..24{t=app::Type::Optional{item:Box::new(t)};old=oracle::Type::Optional{item:Box::new(old)}}stress(t,old);
 let mut v=fixture(Family::AppDefinition,None);let item=v["actions"][0].clone();v["actions"]=serde_json::Value::Array(vec![item;128]);let wire=v.to_string();stress(app::AppDefinition::decode(&wire),serde_json::from_str::<oracle::AppDefinition>(&wire).unwrap());
 assert_eq!(INJECTED.load(Ordering::Relaxed)-faults,68);
}
#[test]fn prefix_sequential_contexts_first_failure_and_managed_borrow(){
 let _serial=LOCK.lock().unwrap();let faults=INJECTED.load(Ordering::Relaxed);let wire=r#"{"kind":"and","values":[{"kind":"today"},{"kind":"not","value":{"kind":"today"}}]}"#;
 let source=app::Expr::decode(wire);let old:oracle::Expr=serde_json::from_str(wire).unwrap();let expected=canonical_oracle::canonical_bytes(&old).unwrap();let requests=schedule(4,expected.len());
 for sticky in [false,true]{
  for(i,&want)in requests.iter().enumerate(){
   let mut cx=fault(i+2,sticky);let prefix=product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap();
   let watch=Watch::begin();let result=source.encode(&mut cx);watch.empty();injected(result.unwrap_err(),&cx,i+2,want);watch.schedule(&requests[..i]);drop(prefix);
  }
  // Completed first output remains live while every second-operation request is refused.
  for(i,&want)in requests.iter().enumerate(){
   let n=requests.len()+i+1;let mut cx=fault(n,sticky);let watch=Watch::begin();
   let first=source.encode(&mut cx);watch.clean();let first=first.unwrap();watch.resume();let result=source.encode(&mut cx);watch.clean();
   injected(result.unwrap_err(),&cx,n,want);assert_eq!(first,expected);let mut prefix=requests.clone();prefix.extend_from_slice(&requests[..i]);watch.schedule(&prefix);
   watch.resume();drop(first);watch.empty();
  }
  // A prior capacity failure, not an encoder-created context, retains priority.
  let mut cx=fault(usize::MAX,sticky);let mut prefix:Vec<u8>=Vec::new();
  let cause=product_alloc::try_reserve(&mut prefix,usize::MAX,&mut cx,Site::String).unwrap_err();assert_eq!(cause.kind,Kind::CapacityOverflow);
  let watch=Watch::begin();let result=source.encode(&mut cx);watch.clean();
  if sticky{assert!(result.is_err());assert_eq!(cx.growth_requests(),1);watch.empty()}else{let bytes=result.unwrap();assert_eq!(bytes,expected);watch.resume();drop(bytes);watch.empty()}
  assert_eq!(cx.first_failure(),Some(cause));
  let mut cx=fault(1,sticky);let result=product_alloc::try_copy_str("prefix",&mut cx,Site::String);assert!(result.is_err());assert_eq!(cx.growth_requests(),1);
 }
 for sticky in [false,true]{
  let mut cx=fault(1,sticky);let watch=Watch::begin();let first=source.encode(&mut cx);watch.empty();let cause=cx.first_failure();assert!(first.is_err());
  let watch=Watch::begin();let again=source.encode(&mut cx);watch.clean();
  if sticky{assert!(again.is_err());assert_eq!(cx.growth_requests(),2);watch.empty()}
  else{let bytes=again.unwrap();assert_eq!(bytes,expected);assert_eq!(cx.growth_requests(),1+requests.len());watch.resume();drop(bytes);watch.empty()}
  assert_eq!(cx.first_failure(),cause);
 }
 assert_eq!(INJECTED.load(Ordering::Relaxed)-faults,22);
 let owner=source.try_copy(&mut Cx::new()).unwrap();success(owner.as_ref(),&expected,&requests);
 // No production raw extraction is needed. Source retains its content/capacity.
 source.same(&old);
}

// Fixture guards exist before construction. Only known fixture variants are
// accepted; shallow pending storage is preallocated for the finite census.
enum DeepNode{T(app::Type),D(app::DataValue),E(app::Expr),S(app::Statement)}
struct DeepSource{root:Option<DeepNode>,pending:Vec<DeepNode>}
impl DeepSource{fn new()->Self{Self{root:None,pending:Vec::with_capacity(16384)}}}
impl Drop for DeepSource{fn drop(&mut self){
 if let Some(root)=self.root.take(){self.pending.push(root)}
 while let Some(n)=self.pending.pop(){match n{
  DeepNode::T(t)=>match t{app::Type::List{item}|app::Type::Optional{item}=>self.pending.push(DeepNode::T(*item)),app::Type::Boolean|app::Type::Integer|app::Type::Text|app::Type::Date|app::Type::Reference{..}=>()},
  DeepNode::D(d)=>match d{app::DataValue::List{item_type,items}=>{self.pending.push(DeepNode::T(item_type));for x in items{self.pending.push(DeepNode::D(x))}},app::DataValue::Null=>(),_=>panic!("fixture-only DataValue destructor")},
  DeepNode::E(e)=>match e{
   app::Expr::Not{value}=>self.pending.push(DeepNode::E(*value)),
   app::Expr::And{values}=>for x in values{self.pending.push(DeepNode::E(x))},
   app::Expr::Query{predicate,sort,..}=>{self.pending.push(DeepNode::E(*predicate));for s in sort{self.pending.push(DeepNode::E(s.value))}},
   app::Expr::Today|app::Expr::State{..}=>(),_=>panic!("fixture-only Expr destructor")},
  DeepNode::S(s)=>match s{app::Statement::Archive{record}=>self.pending.push(DeepNode::E(record)),app::Statement::ForEach{items,steps,..}=>{self.pending.push(DeepNode::E(items));for x in steps{self.pending.push(DeepNode::S(x))}},_=>panic!("fixture-only Statement destructor")},
 }}
}}
#[derive(Clone,Copy)]enum DeepRef<'a>{T(&'a app::Type),D(&'a app::DataValue),E(&'a app::Expr),S(&'a app::Statement),K(&'a app::SortKey),BT(&'a Box<app::Type>),BE(&'a Box<app::Expr>),VD(&'a Vec<app::DataValue>),VE(&'a Vec<app::Expr>),VS(&'a Vec<app::Statement>),VK(&'a Vec<app::SortKey>),Text(&'a String),Bool(bool),U(usize),Request(Site,usize)}
#[derive(Debug,PartialEq,Eq)]struct Atom<'a>(u8,usize,Option<&'a str>);
fn deep_info(root:DeepRef<'_>)->(Vec<Atom<'_>>,Vec<Mark>,Schedule){
 let mut todo=vec![root];let mut atoms=Vec::new();let mut marks=Vec::new();let mut payload=Vec::new();
 while let Some(n)=todo.pop(){match n{
  DeepRef::Request(site,bytes)=>{payload.push((site,bytes));continue},
  DeepRef::T(t)=>match t{
   app::Type::Boolean=>atoms.push(Atom(0,0,None)),app::Type::Integer=>atoms.push(Atom(1,0,None)),app::Type::Text=>atoms.push(Atom(2,0,None)),app::Type::Date=>atoms.push(Atom(3,0,None)),
   app::Type::Reference{entity}=>{atoms.push(Atom(4,0,None));todo.push(DeepRef::Text(entity))},
   app::Type::List{item}=>{atoms.push(Atom(5,0,None));todo.push(DeepRef::BT(item))},app::Type::Optional{item}=>{atoms.push(Atom(6,0,None));todo.push(DeepRef::BT(item))}},
  DeepRef::D(d)=>match d{app::DataValue::Null=>atoms.push(Atom(7,0,None)),app::DataValue::List{item_type,items}=>{atoms.push(Atom(8,0,None));todo.push(DeepRef::VD(items));todo.push(DeepRef::T(item_type))},_=>panic!("fixture data")},
  DeepRef::E(e)=>match e{
   app::Expr::Today=>atoms.push(Atom(9,0,None)),app::Expr::State{state}=>{atoms.push(Atom(10,0,None));todo.push(DeepRef::Text(state))},
   app::Expr::Not{value}=>{atoms.push(Atom(11,0,None));todo.push(DeepRef::BE(value))},
   app::Expr::And{values}=>{atoms.push(Atom(12,0,None));todo.push(DeepRef::VE(values))},
   app::Expr::Query{entity,binding,predicate,sort,limit,include_archived}=>{atoms.push(Atom(13,0,None));todo.push(DeepRef::Bool(*include_archived));todo.push(DeepRef::U(*limit));todo.push(DeepRef::VK(sort));todo.push(DeepRef::BE(predicate));todo.push(DeepRef::Text(binding));todo.push(DeepRef::Text(entity))},_=>panic!("fixture expr")},
  DeepRef::S(s)=>match s{app::Statement::Archive{record}=>{atoms.push(Atom(14,0,None));todo.push(DeepRef::E(record))},app::Statement::ForEach{items,binding,steps}=>{atoms.push(Atom(15,0,None));todo.push(DeepRef::VS(steps));todo.push(DeepRef::Text(binding));todo.push(DeepRef::E(items))},_=>panic!("fixture statement")},
  DeepRef::K(k)=>{atoms.push(Atom(16,0,None));todo.push(DeepRef::Bool(k.descending));todo.push(DeepRef::E(&k.value))},
  DeepRef::BT(b)=>{atoms.push(Atom(17,0,None));marks.push(Mark((&**b)as *const _ as usize,1,size_of::<app::Type>()));todo.push(DeepRef::Request(Site::Box,size_of::<app::Type>()));todo.push(DeepRef::T(b))},
  DeepRef::BE(b)=>{atoms.push(Atom(18,0,None));marks.push(Mark((&**b)as *const _ as usize,1,size_of::<app::Expr>()));todo.push(DeepRef::Request(Site::Box,size_of::<app::Expr>()));todo.push(DeepRef::E(b))},
  DeepRef::VD(v)=>{atoms.push(Atom(19,v.len(),None));if v.capacity()>0{marks.push(Mark(v.as_ptr()as usize,v.capacity(),v.len()))}for x in v.iter().rev(){todo.push(DeepRef::D(x))}if !v.is_empty(){todo.push(DeepRef::Request(Site::Vector,v.len()*size_of::<app::DataValue>()))}},
  DeepRef::VE(v)=>{atoms.push(Atom(20,v.len(),None));if v.capacity()>0{marks.push(Mark(v.as_ptr()as usize,v.capacity(),v.len()))}for x in v.iter().rev(){todo.push(DeepRef::E(x))}if !v.is_empty(){todo.push(DeepRef::Request(Site::Vector,v.len()*size_of::<app::Expr>()))}},
  DeepRef::VS(v)=>{atoms.push(Atom(21,v.len(),None));if v.capacity()>0{marks.push(Mark(v.as_ptr()as usize,v.capacity(),v.len()))}for x in v.iter().rev(){todo.push(DeepRef::S(x))}if !v.is_empty(){todo.push(DeepRef::Request(Site::Vector,v.len()*size_of::<app::Statement>()))}},
  DeepRef::VK(v)=>{atoms.push(Atom(22,v.len(),None));if v.capacity()>0{marks.push(Mark(v.as_ptr()as usize,v.capacity(),v.len()))}for x in v.iter().rev(){todo.push(DeepRef::K(x))}if !v.is_empty(){todo.push(DeepRef::Request(Site::Vector,v.len()*size_of::<app::SortKey>()))}},
  DeepRef::Text(s)=>{atoms.push(Atom(23,0,Some(s)));if s.capacity()>0{marks.push(Mark(s.as_ptr()as usize,s.capacity(),s.len()))}if !s.is_empty(){payload.push((Site::String,s.len()))}},
  DeepRef::Bool(b)=>atoms.push(Atom(24,usize::from(b),None)),DeepRef::U(n)=>atoms.push(Atom(25,n,None)),
 }}(atoms,marks,payload)
}
fn deep_run<T:Case>(source:&T,reference:for<'a>fn(&'a T)->DeepRef<'a>,expected:&[u8],d:usize)where T::Old:serde::de::DeserializeOwned+serde::Serialize{
 let(before,marks,_)=deep_info(reference(source));assert!(before.len()<=16384);assert!(expected.len()<=2*1024*1024);let requests=schedule(d,expected.len());
 success(source,expected,&requests);
 for sticky in [false,true]{for (i,&want)in requests.iter().enumerate(){
  let mut cx=fault(i+1,sticky);let watch=Watch::begin();let result=source.encode(&mut cx);watch.empty();injected(result.unwrap_err(),&cx,i+1,want);watch.schedule(&requests[..i]);
  let(after,after_marks,_)=deep_info(reference(source));assert_eq!(before,after);assert_eq!(marks,after_marks);
 }}
 success(source,expected,&requests);let(after,after_marks,_)=deep_info(reference(source));assert_eq!(before,after);assert_eq!(marks,after_marks);
}
#[test]fn four_depth_2048_chains_literal_bytes_and_all_refusals(){
 let _serial=LOCK.lock().unwrap();let faults=INJECTED.load(Ordering::Relaxed);
 let mut source=DeepSource::new();source.root=Some(DeepNode::T(app::Type::Reference{entity:"leaf".into()}));
 for _ in 0..2048{if let Some(DeepNode::T(t))=source.root.take(){source.root=Some(DeepNode::T(app::Type::Optional{item:Box::new(t)}));}else{panic!("fixture")}}
 let expected=format!("{}{}{}",r#"{"item":"#.repeat(2048),r#"{"entity":"leaf","kind":"reference"}"#,r#","kind":"optional"}"#.repeat(2048));
 if let Some(DeepNode::T(t))=&source.root{deep_run(t,|v|DeepRef::T(v),expected.as_bytes(),2049)}drop(source);
 let mut source=DeepSource::new();source.root=Some(DeepNode::D(app::DataValue::Null));
 for _ in 0..2048{if let Some(DeepNode::D(d))=source.root.take(){source.root=Some(DeepNode::D(app::DataValue::List{item_type:app::Type::Text,items:vec![d]}));}else{panic!("fixture")}}
 let expected=format!("{}{}{}",r#"{"item_type":{"kind":"text"},"items":["#.repeat(2048),r#"{"kind":"null"}"#,r#"],"kind":"list"}"#.repeat(2048));
 if let Some(DeepNode::D(d))=&source.root{deep_run(d,|v|DeepRef::D(v),expected.as_bytes(),4097)}drop(source);
 let mut source=DeepSource::new();source.root=Some(DeepNode::E(app::Expr::Today));
 for i in 0..2048{if let Some(DeepNode::E(e))=source.root.take(){source.root=Some(DeepNode::E(match i%3{
  0=>app::Expr::Not{value:Box::new(e)},1=>app::Expr::And{values:vec![e]},
  _=>app::Expr::Query{entity:String::new(),binding:String::new(),predicate:Box::new(app::Expr::Today),sort:vec![app::SortKey{value:e,descending:false}],limit:0,include_archived:false},
 }));}else{panic!("fixture")}}
 let mut expected=String::new();for i in (0..2048).rev(){expected.push_str(match i%3{0=>r#"{"kind":"not","value":"#,1=>r#"{"kind":"and","values":["#,_=>r#"{"binding":"","entity":"","include_archived":false,"kind":"query","limit":0,"predicate":{"kind":"today"},"sort":[{"descending":false,"value":"#})}
 expected.push_str(r#"{"kind":"today"}"#);for i in 0..2048{expected.push_str(match i%3{0=>"}",1=>"]}",_=>"}]}"})}
 if let Some(DeepNode::E(e))=&source.root{deep_run(e,|v|DeepRef::E(v),expected.as_bytes(),4096)}
 // The same deep fixture is dismantled during unwind, not a fifth fixture.
 let unwind=std::panic::catch_unwind(move||{let _guard=source;panic!("fixture unwind")});assert!(unwind.is_err());
 let mut source=DeepSource::new();source.root=Some(DeepNode::S(app::Statement::Archive{record:app::Expr::Today}));
 for _ in 0..2048{if let Some(DeepNode::S(s))=source.root.take(){source.root=Some(DeepNode::S(app::Statement::ForEach{items:app::Expr::Today,binding:String::new(),steps:vec![s]}));}else{panic!("fixture")}}
 let expected=format!("{}{}{}",r#"{"binding":"","items":{"kind":"today"},"kind":"for_each","steps":["#.repeat(2048),r#"{"kind":"archive","record":{"kind":"today"}}"#,r#"]}"#.repeat(2048));
 if let Some(DeepNode::S(s))=&source.root{deep_run(s,|v|DeepRef::S(v),expected.as_bytes(),4098)}drop(source);
 assert_eq!(INJECTED.load(Ordering::Relaxed)-faults,116);

}
#[test]fn refused_error_formats_to_fixed_storage_without_growth(){
 use std::fmt::Write;
 struct Fixed{bytes:[u8;256],n:usize}
 impl Write for Fixed{fn write_str(&mut self,s:&str)->std::fmt::Result{let end=self.n.checked_add(s.len()).ok_or(std::fmt::Error)?;self.bytes.get_mut(self.n..end).ok_or(std::fmt::Error)?.copy_from_slice(s.as_bytes());self.n=end;Ok(())}}
 let _serial=LOCK.lock().unwrap();let source=app::Type::Boolean;let mut cx=fault(1,true);let e=source.encode(&mut cx).unwrap_err();let mut fixed=Fixed{bytes:[0;256],n:0};
 let watch=Watch::begin();let result=write!(&mut fixed,"{e}");watch.empty();assert!(result.is_ok());assert_eq!(USED.load(Ordering::Relaxed),0);assert_eq!(cx.growth_requests(),1);assert!(std::str::from_utf8(&fixed.bytes[..fixed.n]).unwrap().contains("injected"));
}
