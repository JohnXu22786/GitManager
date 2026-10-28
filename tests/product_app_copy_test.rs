// R05d: separate executable. Never include in app_codec_aggregate (one allocator each).
// Oracle declarations are frozen historical inputs; no production conversion uses them.
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[path = "../src/product_json.rs"] mod product_json;
#[path = "../src/product_app.rs"] mod product_app;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "support/app_codec_oracle.rs"] mod oracle;
use product_app as app;
use product_alloc::{AllocationContext as Cx, AllocationFailure, AllocationFailureKind as Kind,
    AllocationSite as Site, ordered::{OrderedMap,OrderedSet}};
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,mem::size_of,num::NonZeroUsize,
    sync::{Mutex,atomic::{AtomicUsize,Ordering}}};

// Constant-initialized, no-Drop TLS is primed BEFORE observation. Hooks never
// lock, format, allocate metadata, call thread::current, assert, or return null.
// Any TLS access error is recorded as BAD, never silently accepted as inactive.
thread_local! { static OBSERVE: Cell<bool> = const { Cell::new(false) }; }
const RECORDS:usize=32768;
static PTR:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static SIZE:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static LIVE:[AtomicUsize;RECORDS]=[const{AtomicUsize::new(0)};RECORDS];
static USED:AtomicUsize=AtomicUsize::new(0);
static CALLS:AtomicUsize=AtomicUsize::new(0);
static BAD:AtomicUsize=AtomicUsize::new(0);
static LOCK:Mutex<()>=Mutex::new(());
struct Observer;
fn observing()->bool{match OBSERVE.try_with(Cell::get){Ok(active)=>active,Err(_)=>{BAD.fetch_add(1,Ordering::Relaxed);false}}}
fn register(p:*mut u8,n:usize){
    CALLS.fetch_add(1,Ordering::Relaxed);
    if p.is_null(){BAD.fetch_add(1,Ordering::Relaxed);return}
    let i=USED.fetch_add(1,Ordering::Relaxed);
    if i>=RECORDS{BAD.fetch_add(1,Ordering::Relaxed);return}
    PTR[i].store(p as usize,Ordering::Relaxed);SIZE[i].store(n,Ordering::Relaxed);LIVE[i].store(1,Ordering::Relaxed);
}
fn release(p:*mut u8){
    let n=USED.load(Ordering::Relaxed).min(RECORDS);
    for i in (0..n).rev(){if PTR[i].load(Ordering::Relaxed)==p as usize && LIVE[i].load(Ordering::Relaxed)==1{
        LIVE[i].store(0,Ordering::Relaxed);return
    }}
    BAD.fetch_add(1,Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Observer{
    unsafe fn alloc(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc(l)};if observing(){register(p,l.size())}p}
    unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8{let p=unsafe{System.alloc_zeroed(l)};if observing(){register(p,l.size())}p}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){if observing(){release(p)}unsafe{System.dealloc(p,l)}}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{
        let q=unsafe{System.realloc(p,l,n)};if observing(){if !q.is_null(){release(p)}register(q,n)}q
    }
}
#[global_allocator]static ALLOCATOR:Observer=Observer;
struct Watch;
impl Watch{
    fn begin()->Self{OBSERVE.with(|a|a.set(false));USED.store(0,Ordering::Relaxed);CALLS.store(0,Ordering::Relaxed);BAD.store(0,Ordering::Relaxed);OBSERVE.with(|a|a.set(true));Self}
    fn pause(&self){OBSERVE.with(|a|a.set(false))}
    fn resume(&self){OBSERVE.with(|a|a.set(true))}
    fn clean(&self){self.pause();assert_eq!(BAD.load(Ordering::Relaxed),0,"observer overflow/unknown or duplicate release/null allocation");}
    fn empty(&self){self.clean();for i in 0..USED.load(Ordering::Relaxed){assert_eq!(LIVE[i].load(Ordering::Relaxed),0,"new allocation was not released exactly once");}}
}
impl Drop for Watch{fn drop(&mut self){self.pause()}}
static INJECTED:AtomicUsize=AtomicUsize::new(0);
static PAIRS:AtomicUsize=AtomicUsize::new(0);
fn fault(n:usize,sticky:bool)->Cx{assert!(INJECTED.fetch_add(1,Ordering::Relaxed)<16384);Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),sticky)}
#[derive(Debug,PartialEq,Eq)]struct Mark(usize,usize,usize);
type Schedule=Vec<(Site,usize)>;
trait Walk{
    type Old;
    fn same(&self,old:&Self::Old);
    fn marks(&self,out:&mut Vec<Mark>);
    fn tokens(&self)->usize;
    fn payload(&self,out:&mut Schedule);
}
macro_rules! scalar_walk{($($t:ty),+)=>{$(impl Walk for $t{type Old=Self;
 fn same(&self,b:&Self){assert_eq!(self,b)} fn marks(&self,_:&mut Vec<Mark>){}
 fn tokens(&self)->usize{1} fn payload(&self,_:&mut Schedule){}
})+};}
scalar_walk!(bool,u32,i64,i32,usize);
impl Walk for String{type Old=String;fn same(&self,b:&String){assert_eq!(self,b)}
 fn marks(&self,o:&mut Vec<Mark>){if self.capacity()>0{o.push(Mark(self.as_ptr() as usize,self.capacity(),self.len()))}}
 fn tokens(&self)->usize{1}fn payload(&self,o:&mut Schedule){if !self.is_empty(){o.push((Site::String,self.len()))}}
}
impl<T:Walk> Walk for Vec<T>{type Old=Vec<T::Old>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){if self.capacity()>0{o.push(Mark(self.as_ptr() as usize,self.capacity(),self.len()))}for a in self{a.marks(o)}}
 fn tokens(&self)->usize{1+self.iter().map(Walk::tokens).sum::<usize>()}
 fn payload(&self,o:&mut Schedule){if !self.is_empty(){o.push((Site::Vector,self.len()*size_of::<T>()))}for a in self{a.payload(o)}}
}
impl<T:Walk> Walk for Box<T>{type Old=Box<T::Old>;
 fn same(&self,b:&Self::Old){(**self).same(b)}
 fn marks(&self,o:&mut Vec<Mark>){o.push(Mark((&**self)as *const T as usize,1,size_of::<T>()));(**self).marks(o)}
 fn tokens(&self)->usize{1+(**self).tokens()}
 fn payload(&self,o:&mut Schedule){(**self).payload(o);o.push((Site::Box,size_of::<T>()))}
}
impl<T:Walk> Walk for Option<T>{type Old=Option<T::Old>;
 fn same(&self,b:&Self::Old){match(self,b){(Some(a),Some(b))=>a.same(b),(None,None)=>(),_=>panic!("option")}}
 fn marks(&self,o:&mut Vec<Mark>){if let Some(v)=self{v.marks(o)}}
 fn tokens(&self)->usize{1+self.as_ref().map(Walk::tokens).unwrap_or(0)}
 fn payload(&self,o:&mut Schedule){if let Some(v)=self{v.payload(o)}}
}
impl<T:Walk> Walk for OrderedMap<String,T>{type Old=std::collections::BTreeMap<String,T::Old>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for((k,v),(a,b))in self.iter().zip(b){k.same(a);v.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){for(k,v)in self.iter(){o.push(Mark(k as *const String as usize,self.capacity(),self.len()));k.marks(o);v.marks(o)}}
 fn tokens(&self)->usize{1+self.iter().map(|(k,v)|k.tokens()+v.tokens()).sum::<usize>()}
 fn payload(&self,o:&mut Schedule){if !self.is_empty(){o.push((Site::OrderedMap,self.len()*size_of::<(String,T)>()))}for(k,v)in self.iter(){k.payload(o);v.payload(o)}}
}
impl Walk for OrderedSet<app::KeyModifier>{type Old=std::collections::BTreeSet<oracle::KeyModifier>;
 fn same(&self,b:&Self::Old){assert_eq!(self.len(),b.len());for(a,b)in self.iter().zip(b){a.same(b)}}
 fn marks(&self,o:&mut Vec<Mark>){for k in self.iter(){o.push(Mark(k as *const _ as usize,self.capacity(),self.len()))}}
 fn tokens(&self)->usize{1+self.len()}
 fn payload(&self,o:&mut Schedule){if !self.is_empty(){o.push((Site::OrderedSet,self.len()*size_of::<(app::KeyModifier,())>()))}}
}
macro_rules! structure_walk{($t:ident{$($f:ident),+})=>{impl Walk for app::$t{type Old=oracle::$t;
 fn same(&self,b:&Self::Old){let Self{$($f),+}=self; $($f.same(&b.$f);)+}
 fn marks(&self,o:&mut Vec<Mark>){let Self{$($f),+}=self; $($f.marks(o);)+}
 fn tokens(&self)->usize{let Self{$($f),+}=self;1$(+$f.tokens())+}
 fn payload(&self,o:&mut Schedule){let Self{$($f),+}=self; $($f.payload(o);)+}
}};}
macro_rules! enum_walk{($t:ident{$($v:ident$({$($f:ident:$old:ident),+})?),+})=>{impl Walk for app::$t{type Old=oracle::$t;
 fn same(&self,b:&Self::Old){match(self,b){$((Self::$v$({$($f),+})?,oracle::$t::$v$({$($f:$old),+})?)=>{$($($f.same($old);)+)?},)+_=>panic!("variant mismatch")}}
 fn marks(&self,o:&mut Vec<Mark>){match self{$(Self::$v$({$($f),+})?=>{$($($f.marks(o);)+)?},)+}}
 fn tokens(&self)->usize{match self{$(Self::$v$({$($f),+})?=>1$($(+$f.tokens())+)? ,)+}}
 fn payload(&self,o:&mut Schedule){match self{$(Self::$v$({$($f),+})?=>{$($($f.payload(o);)+)?},)+}}
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
trait Case:Walk+Sized where Self::Old:serde::de::DeserializeOwned+Clone+Eq{
 fn decode(s:&str)->Self;
 fn copy(&self,cx:&mut Cx)->Result<app::OwnedAppGraph<Self>,AllocationFailure>;
 fn view(g:&app::OwnedAppGraph<Self>)->&Self;
 fn prepare<'a>(&'a self,b:&'a Self,cx:&mut Cx)->Result<app::PreparedAppEq<'a,Self>,AllocationFailure>;
}
macro_rules! cases{($($t:ident),+)=>{$(impl Case for app::$t{
 fn decode(s:&str)->Self{Self::decode_json(s.as_bytes(),&mut Cx::new()).unwrap_or_else(|_|panic!("fixture decode"))}
 fn copy(&self,cx:&mut Cx)->Result<app::OwnedAppGraph<Self>,AllocationFailure>{self.try_copy(cx)}
 fn view(g:&app::OwnedAppGraph<Self>)->&Self{g.as_ref()}
 fn prepare<'a>(&'a self,b:&'a Self,cx:&mut Cx)->Result<app::PreparedAppEq<'a,Self>,AllocationFailure>{self.try_prepare_eq(b,cx)}
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
fn bounds(v:&serde_json::Value,d:usize,n:&mut usize){*n+=1;assert!(*n<=128 && d<=8);match v{serde_json::Value::Array(a)=>for x in a{bounds(x,d+1,n)},serde_json::Value::Object(a)=>for x in a.values(){bounds(x,d+1,n)},_=>()}}
fn pair<T:Case>(a_wire:&str,b_wire:&str)where T::Old:serde::de::DeserializeOwned+Clone+Eq{
 assert!(PAIRS.fetch_add(1,Ordering::Relaxed)<768);for s in [a_wire,b_wire]{assert!(s.len()<=16384);bounds(&serde_json::from_str(s).unwrap(),0,&mut 0)}
 let a=T::decode(a_wire);let b=T::decode(b_wire);
 let oa:T::Old=serde_json::from_str(a_wire).unwrap();let ob:T::Old=serde_json::from_str(b_wire).unwrap();let cloned=oa.clone();
 let mut before=Vec::new();a.marks(&mut before);
 let copied=a.copy(&mut Cx::new()).unwrap();T::view(&copied).same(&cloned);
 let mut after=Vec::new();a.marks(&mut after);assert_eq!(before,after);
 let mut owned=Vec::new();T::view(&copied).marks(&mut owned);
 for p in &owned{assert!(before.iter().all(|q|p.0!=q.0),"source and copy share owner storage")}
 let mut eq=a.prepare(&b,&mut Cx::new()).unwrap();let mut self_eq=a.prepare(&a,&mut Cx::new()).unwrap();
 let watch=Watch::begin();let actual=eq.equal();let again=eq.equal();let identity=self_eq.equal();watch.clean();assert_eq!(CALLS.load(Ordering::Relaxed),0);assert_eq!(actual,oa==ob);assert_eq!(again,actual);assert!(identity);
}
fn dispatch(f:Family,a:&str,b:&str){match f{
Family::Type=>pair::<app::Type>(a,b),
Family::DataValue=>pair::<app::DataValue>(a,b),
Family::Expr=>pair::<app::Expr>(a,b),
Family::SortKey=>pair::<app::SortKey>(a,b),
Family::FieldDefinition=>pair::<app::FieldDefinition>(a,b),
Family::EntityDefinition=>pair::<app::EntityDefinition>(a,b),
Family::StateDefinition=>pair::<app::StateDefinition>(a,b),
Family::ActionDefinition=>pair::<app::ActionDefinition>(a,b),
Family::Statement=>pair::<app::Statement>(a,b),
Family::CollectionTarget=>pair::<app::CollectionTarget>(a,b),
Family::CollectionOperation=>pair::<app::CollectionOperation>(a,b),
Family::OutputFormat=>pair::<app::OutputFormat>(a,b),
Family::OutputDefinition=>pair::<app::OutputDefinition>(a,b),
Family::ObservableDefinition=>pair::<app::ObservableDefinition>(a,b),
Family::Column=>pair::<app::Column>(a,b),
Family::StateControl=>pair::<app::StateControl>(a,b),
Family::SelectionBinding=>pair::<app::SelectionBinding>(a,b),
Family::FormInput=>pair::<app::FormInput>(a,b),
Family::ViewKind=>pair::<app::ViewKind>(a,b),
Family::ActionPlacement=>pair::<app::ActionPlacement>(a,b),
Family::ActionBinding=>pair::<app::ActionBinding>(a,b),
Family::KeyModifier=>pair::<app::KeyModifier>(a,b),
Family::KeyBinding=>pair::<app::KeyBinding>(a,b),
Family::ViewDefinition=>pair::<app::ViewDefinition>(a,b),
Family::AppDefinition=>pair::<app::AppDefinition>(a,b),
}}
#[test]fn all_roots_and_each_field_against_independent_clone_eq(){
 let _serial=LOCK.lock().unwrap();let pair_start=PAIRS.load(Ordering::Relaxed);assert_eq!(FIXTURES.len(),79);assert_eq!(FIXTURES.iter().map(|f|f.changes.len()).sum::<usize>(),161);
 for f in FIXTURES{dispatch(f.family,f.wire,f.wire);for (field,other)in f.changes{let mut changed:serde_json::Value=serde_json::from_str(f.wire).unwrap();changed[*field]=serde_json::from_str(other).unwrap();dispatch(f.family,f.wire,&changed.to_string());}}
 // Every distinct variant also differs from its neighbor in its own family.
 for window in FIXTURES.windows(2){if std::mem::discriminant(&window[0].family)==std::mem::discriminant(&window[1].family){dispatch(window[0].family,window[0].wire,window[1].wire)}}
 assert_eq!(PAIRS.load(Ordering::Relaxed)-pair_start,294);
}

// Independent count + target-layout schedule, never obtained from first_failure.
// Mirror only the documented geometric workspace policy; actual Vec capacities
// are read from a separately allocated witness before protected observation.
fn plan_schedule(n:usize)->Schedule{
 let mut witness:Vec<[u8;app::BORROWED_RECORD_BYTES]>=Vec::new();let mut out=Vec::new();
 for _ in 0..n{if witness.len()==witness.capacity(){let additional=witness.capacity().max(1);out.push((Site::Vector,(witness.len()+additional)*app::BORROWED_RECORD_BYTES));witness.try_reserve_exact(additional).unwrap();}witness.push([0;app::BORROWED_RECORD_BYTES]);}out
}
fn copy_schedule<T:Walk>(v:&T)->Schedule{
 let n=v.tokens();let mut out=plan_schedule(n);
 out.push((Site::Vector,n*app::OWNED_NODE_BYTES));out.push((Site::Vector,n*app::OWNED_NODE_BYTES));v.payload(&mut out);out
}
fn eq_schedule<T:Walk>(a:&T,b:&T)->Schedule{let mut out=plan_schedule(a.tokens());out.extend(plan_schedule(b.tokens()));out}
fn injected_error(e:AllocationFailure,cx:&Cx,ordinal:usize,expected:(Site,usize)){
 assert_eq!(cx.growth_requests(),ordinal);assert_eq!(e,AllocationFailure{kind:Kind::Injected,site:expected.0,requested_bytes:Some(expected.1)});assert_eq!(cx.first_failure(),Some(e));
}
fn failure<T>(r:Result<T,AllocationFailure>)->AllocationFailure{match r{Err(e)=>e,Ok(_)=>panic!("expected refusal")}}
fn check_source<T:Walk>(v:&T,old:&T::Old,before:&[Mark]){v.same(old);let mut now=Vec::new();v.marks(&mut now);assert_eq!(now,before)}
fn sweep<T:Case>(wire:&str)where T::Old:serde::de::DeserializeOwned+Clone+Eq{
 assert!(wire.len()<=4096);let source=T::decode(wire);let other=T::decode(wire);let old:T::Old=serde_json::from_str(wire).unwrap();let mut before=Vec::new();source.marks(&mut before);
 let copy=copy_schedule(&source);let eq=eq_schedule(&source,&other);assert!(copy.len()<=512 && eq.len()<=512);
 // Successful ownership is live until managed Drop; ALL allocations observed.
 let mut cx=Cx::new();let watch=Watch::begin();let result=source.copy(&mut cx).unwrap();watch.clean();assert_eq!(cx.growth_requests(),copy.len());assert_eq!(CALLS.load(Ordering::Relaxed),copy.len());for(i,expected)in copy.iter().enumerate(){assert_eq!(SIZE[i].load(Ordering::Relaxed),expected.1)}assert!(LIVE[..USED.load(Ordering::Relaxed)].iter().any(|n|n.load(Ordering::Relaxed)==1));
 T::view(&result).same(&old);watch.resume();drop(result);watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),copy.len());check_source(&source,&old,&before);
 let mut cx=Cx::new();let prepared=source.prepare(&other,&mut cx).unwrap();assert_eq!(cx.growth_requests(),eq.len());drop(prepared);
 for sticky in [false,true]{
  // Every copy/preparation ordinal, both directly and after a same-cx prefix.
  for prefix in [false,true]{
   for (i,&expected)in copy.iter().enumerate(){let n=i+1+usize::from(prefix);let mut cx=fault(n,sticky);
    let prefix_owner=if prefix{Some(product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap())}else{None};
    let watch=Watch::begin();let e=failure(source.copy(&mut cx));watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),i);injected_error(e,&cx,n,expected);drop(prefix_owner);check_source(&source,&old,&before);
   }
   for (i,&expected)in eq.iter().enumerate(){let n=i+1+usize::from(prefix);let mut cx=fault(n,sticky);
    let prefix_owner=if prefix{Some(product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap())}else{None};
    let watch=Watch::begin();let e=failure(source.prepare(&other,&mut cx));watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),i);injected_error(e,&cx,n,expected);drop(prefix_owner);check_source(&source,&old,&before);
   }
  }
  // Failure in the prefix prevents the operation, retaining the same first cause.
  let mut cx=fault(1,sticky);let e=failure(product_alloc::try_copy_str("prefix",&mut cx,Site::String));injected_error(e,&cx,1,(Site::String,6));assert_eq!(cx.growth_requests(),1);
  // Copy then preparation share ordinals. Already completed deep owners remain
  // protected during every second-operation preparation refusal.
  for(i,&expected)in eq.iter().enumerate(){let n=copy.len()+i+1;let mut cx=fault(n,sticky);let watch=Watch::begin();let first=source.copy(&mut cx).unwrap();let e=failure(source.prepare(&other,&mut cx));drop(first);watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),n-1);injected_error(e,&cx,n,expected)}
  // N+1 refusal occurs AFTER a successful copy; drop does not use/reset cx.
  let n=copy.len()+1;let mut cx=fault(n,sticky);let watch=Watch::begin();let owned=source.copy(&mut cx).unwrap();let e=failure(product_alloc::try_copy_str("later",&mut cx,Site::String));drop(owned);watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),copy.len());injected_error(e,&cx,n,(Site::String,5));
 }
}
#[test]fn every_growth_ordinal_context_prefix_and_allocation_lifetime(){
 let _serial=LOCK.lock().unwrap();let injection_start=INJECTED.load(Ordering::Relaxed);
 // Eight finite fixtures: map key-before-value, Vec prefix, both Box shells,
 // two/three-Box siblings, scalar set, and completed siblings/late App fields.
 sweep::<app::Type>(r#"{"kind":"optional","item":{"kind":"list","item":{"kind":"reference","entity":"e"}}}"#);
 sweep::<app::DataValue>(r#"{"kind":"list","item_type":{"kind":"text"},"items":[{"kind":"text","value":"a"},{"kind":"text","value":"b"}]}"#);
 sweep::<app::Expr>(r#"{"kind":"query","entity":"e","binding":"b","predicate":{"kind":"not","value":{"kind":"today"}},"sort":[{"value":{"kind":"variable","name":"x"},"descending":true}],"limit":3,"include_archived":false}"#);
 sweep::<app::Expr>(r#"{"kind":"if","condition":{"kind":"variable","name":"a"},"then_value":{"kind":"field","record":{"kind":"today"},"field":"f"},"else_value":{"kind":"state","state":"s"}}"#);
 sweep::<app::Statement>(r#"{"kind":"emit","output":"o","items":{"kind":"today"},"binding":"b","columns":{"a":{"kind":"variable","name":"v"},"z":{"kind":"not","value":{"kind":"today"}}}}"#);
 let key=FIXTURES.iter().find(|f|matches!(f.family,Family::KeyBinding)).unwrap();sweep::<app::KeyBinding>(key.wire);
 let action=FIXTURES.iter().find(|f|matches!(f.family,Family::ActionDefinition)).unwrap();sweep::<app::ActionDefinition>(action.wire);
 let app=FIXTURES.iter().find(|f|matches!(f.family,Family::AppDefinition)).unwrap();sweep::<app::AppDefinition>(app.wire);
 assert_eq!(INJECTED.load(Ordering::Relaxed)-injection_start,1136);
}
#[test]fn order_capacity_options_and_primitive_edges(){
 let _serial=LOCK.lock().unwrap();let pair_start=PAIRS.load(Ordering::Relaxed);
 for(a,b)in [
  (r#"{"kind":"and","values":[{"kind":"today"},{"kind":"state","state":"s"}]}"#,r#"{"kind":"and","values":[{"kind":"state","state":"s"},{"kind":"today"}]}"#),
  (r#"{"kind":"and","values":[]}"#,r#"{"kind":"and","values":[{"kind":"today"}]}"#),
  (r#"{"kind":"variable","name":""}"#,r#"{"kind":"variable","name":"\u0000"}"#),
  (r#"{"kind":"variable","name":"é"}"#,r#"{"kind":"variable","name":"é"}"#),
 ]{pair::<app::Expr>(a,b)}
 for(a,b)in [("-9223372036854775808","9223372036854775807"),("0","-1")]{pair::<app::DataValue>(&format!("{{\"kind\":\"integer\",\"value\":{a}}}"),&format!("{{\"kind\":\"integer\",\"value\":{b}}}"))}
 pair::<app::DataValue>(r#"{"kind":"date","days":-2147483648}"#,r#"{"kind":"date","days":2147483647}"#);
 pair::<app::StateControl>(r#"{"id":"i","label":"l","state":"s","on_change":null}"#,r#"{"id":"i","label":"l","state":"s","on_change":""}"#);
 pair::<app::Statement>(r#"{"kind":"create","entity":"e","values":{"z":{"kind":"today"},"a":{"kind":"state","state":"s"}},"bind":"b"}"#,r#"{"kind":"create","entity":"e","values":{"a":{"kind":"state","state":"s"},"z":{"kind":"today"}},"bind":"b"}"#);
 // Same keys/cardinality force comparison into map values and set members.
 pair::<app::ActionDefinition>(r#"{"id":"i","label":"l","parameters":{"k":{"kind":"reference","entity":"a"}},"guards":[],"steps":[],"ensures":[]}"#,r#"{"id":"i","label":"l","parameters":{"k":{"kind":"reference","entity":"b"}},"guards":[],"steps":[],"ensures":[]}"#);
 pair::<app::Statement>(r#"{"kind":"create","entity":"e","values":{"k":{"kind":"variable","name":"a"}},"bind":"b"}"#,r#"{"kind":"create","entity":"e","values":{"k":{"kind":"variable","name":"b"}},"bind":"b"}"#);
 pair::<app::ViewKind>(r#"{"kind":"form","action":"a","fields":[],"defaults":{"k":{"kind":"text","value":"a"}}}"#,r#"{"kind":"form","action":"a","fields":[],"defaults":{"k":{"kind":"text","value":"b"}}}"#);
 pair::<app::KeyBinding>(r#"{"key":"K","modifiers":["control","alt"],"binding":"B"}"#,r#"{"key":"K","modifiers":["control","shift"],"binding":"B"}"#);
 let q=FIXTURES.iter().find(|f|matches!(f.family,Family::Expr)&&f.wire.contains("\"limit\"")).unwrap();let mut wide:serde_json::Value=serde_json::from_str(q.wire).unwrap();wide["limit"]=serde_json::json!(usize::MAX);pair::<app::Expr>(q.wire,&wide.to_string());
 let root=FIXTURES.iter().find(|f|matches!(f.family,Family::AppDefinition)).unwrap();let mut wide:serde_json::Value=serde_json::from_str(root.wire).unwrap();wide["version"]=serde_json::json!(u32::MAX);pair::<app::AppDefinition>(root.wire,&wide.to_string());
 let mut small=OrderedMap::new();let mut spare=OrderedMap::new();spare.try_reserve(19,&mut Cx::new()).unwrap();
 for k in ["a","z"]{small.try_insert(k.to_owned(),app::Expr::Today,&mut Cx::new()).unwrap();}
 for k in ["z","a"]{spare.try_insert(k.to_owned(),app::Expr::Today,&mut Cx::new()).unwrap();}
 let left=app::Statement::Create{entity:String::new(),values:small,bind:String::new()};let right=app::Statement::Create{entity:String::new(),values:spare,bind:String::new()};
 let mut map_eq=left.try_prepare_eq(&right,&mut Cx::new()).unwrap();let watch=Watch::begin();let equal=map_eq.equal();watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),0);assert!(equal);drop(map_eq);
 let mut a=app::KeyBinding{key:String::from("K"),modifiers:OrderedSet::new(),binding:String::from("B")};
 let mut b=app::KeyBinding{key:String::with_capacity(127),modifiers:OrderedSet::new(),binding:String::with_capacity(111)};b.key.push('K');b.binding.push('B');
 for v in [app::KeyModifier::Control,app::KeyModifier::Command]{a.modifiers.try_insert(v,&mut Cx::new()).unwrap();}
 b.modifiers.try_reserve(8,&mut Cx::new()).unwrap();for v in [app::KeyModifier::Command,app::KeyModifier::Control]{b.modifiers.try_insert(v,&mut Cx::new()).unwrap();}
 let mut eq=a.try_prepare_eq(&b,&mut Cx::new()).unwrap();let watch=Watch::begin();let yes=eq.equal();watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),0);assert!(yes);drop(eq);
 let copy=a.try_copy(&mut Cx::new()).unwrap();a.key.clear();a.binding.push_str(" changed");a.modifiers.remove(&app::KeyModifier::Control);
 assert_eq!(copy.as_ref().key,"K");assert_eq!(copy.as_ref().binding,"B");assert_eq!(copy.as_ref().modifiers.len(),2);
 // Vec capacity and String capacities are not value semantics.
 let x=app::Expr::And{values:vec![app::Expr::Today]};let mut spare=Vec::with_capacity(17);spare.push(app::Expr::Today);let y=app::Expr::And{values:spare};assert!(x.try_prepare_eq(&y,&mut Cx::new()).unwrap().equal());
 assert_eq!(PAIRS.load(Ordering::Relaxed)-pair_start,15);
}

// Independent, iterative fixture-only source owner and borrowed oracle. Neither
// production teardown nor recursive Clone/Drop/Walk is used for these sources.
enum DeepNode{T(app::Type),D(app::DataValue),E(app::Expr),S(app::Statement)}
struct DeepSource{root:Option<DeepNode>,pending:Vec<DeepNode>}
impl DeepSource{fn new(root:DeepNode)->Self{Self{root:Some(root),pending:Vec::with_capacity(8192)}}}
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
fn deep_run<T:Case>(source:&T,reference:for<'a>fn(&'a T)->DeepRef<'a>)where T::Old:serde::de::DeserializeOwned+Clone+Eq{
 let(before,marks,payload)=deep_info(reference(source));let n=before.len();assert!(n<=8192);
 let mut schedule=plan_schedule(n);schedule.push((Site::Vector,n*app::OWNED_NODE_BYTES));schedule.push((Site::Vector,n*app::OWNED_NODE_BYTES));schedule.extend(payload);
 let mut cx=Cx::new();let watch=Watch::begin();let owned=source.copy(&mut cx).unwrap();watch.clean();assert_eq!(cx.growth_requests(),schedule.len());
 let(after,copied_marks,_)=deep_info(reference(T::view(&owned)));assert_eq!(before,after);for p in &copied_marks{assert!(marks.iter().all(|q|p.0!=q.0))}
 let mut prepared=source.prepare(T::view(&owned),&mut Cx::new()).unwrap();let prior=CALLS.load(Ordering::Relaxed);watch.resume();let equal=prepared.equal();watch.clean();assert_eq!(CALLS.load(Ordering::Relaxed),prior);assert!(equal);drop(prepared);drop(after);drop(copied_marks);
 watch.resume();drop(owned);watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),prior);
 for sticky in [false,true]{for ordinal in [1,schedule.len()/2,schedule.len()]{let mut cx=fault(ordinal,sticky);let watch=Watch::begin();let e=failure(source.copy(&mut cx));watch.empty();assert_eq!(CALLS.load(Ordering::Relaxed),ordinal-1);injected_error(e,&cx,ordinal,schedule[ordinal-1]);let(after,after_marks,_)=deep_info(reference(source));assert_eq!(before,after);assert_eq!(marks,after_marks)}}
}
#[test]fn deep_type_data_expr_sort_and_statement_ownership(){
 let _serial=LOCK.lock().unwrap();let injection_start=INJECTED.load(Ordering::Relaxed);
 let mut t=app::Type::Reference{entity:String::from("leaf")};for _ in 0..2048{t=app::Type::Optional{item:Box::new(t)}}let source=DeepSource::new(DeepNode::T(t));if let Some(DeepNode::T(t))=&source.root{deep_run(t,|v|DeepRef::T(v))}drop(source);
 let mut d=app::DataValue::Null;for _ in 0..2048{d=app::DataValue::List{item_type:app::Type::Text,items:vec![d]}}if let app::DataValue::List{items,..}=&mut d{items.push(app::DataValue::Null)}let source=DeepSource::new(DeepNode::D(d));if let Some(DeepNode::D(d))=&source.root{deep_run(d,|v|DeepRef::D(v))}drop(source);
 let mut e=app::Expr::Today;for _ in 0..2048{e=app::Expr::Not{value:Box::new(e)}}let e=app::Expr::Query{entity:String::new(),binding:String::new(),predicate:Box::new(app::Expr::Today),sort:vec![app::SortKey{value:e,descending:true},app::SortKey{value:app::Expr::And{values:vec![app::Expr::Today,app::Expr::State{state:String::from("late")}]},descending:false}],limit:17,include_archived:true};let source=DeepSource::new(DeepNode::E(e));if let Some(DeepNode::E(e))=&source.root{deep_run(e,|v|DeepRef::E(v))}drop(source);
 // 2046 recursive edges + late sibling = 8189 ownership tokens, not a product cap.
 let mut s=app::Statement::Archive{record:app::Expr::Today};for _ in 0..2046{s=app::Statement::ForEach{items:app::Expr::Today,binding:String::new(),steps:vec![s]}}if let app::Statement::ForEach{steps,..}=&mut s{steps.push(app::Statement::Archive{record:app::Expr::State{state:String::from("late")}})}let source=DeepSource::new(DeepNode::S(s));if let Some(DeepNode::S(s))=&source.root{deep_run(s,|v|DeepRef::S(v))}drop(source);
 assert_eq!(INJECTED.load(Ordering::Relaxed)-injection_start,24);
}
