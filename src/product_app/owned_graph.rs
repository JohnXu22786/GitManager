//! Closed ownership vocabulary and borrowed planning. No parser-depth premise.
//! Every field/container/Box/Option is one token. All teardown slots are secured
//! before any copied owner exists and retained through successful root Drop.
use super::*;
use super::copy::CopyNode;
use super::equality::EqualNode;
use crate::product_contract::{Type as ContractType,DataValue as ContractDataValue};
use crate::product_alloc::{try_reserve,AllocationContext,AllocationFailure,AllocationSite,
    ordered::{OrderedMap,OrderedSet}};
use std::{marker::PhantomData,mem};

// The vocabulary is intentionally private: no downstream implementation,
// arbitrary callback, owning Clone, trait object, or additional owner type.
macro_rules! node_types{($emit:ident)=>{$emit!{
 Bool:bool,
 U32:u32,
 I64:i64,
 I32:i32,
 Usize:usize,
 String:String,
 Type:Type,
 DataValue:DataValue,
 ContractType:ContractType,
 ContractDataValue:ContractDataValue,
 BoxContractType:Box<ContractType>,
 VecContractDataValue:Vec<ContractDataValue>,
 Expr:Expr,
 SortKey:SortKey,
 FieldDefinition:FieldDefinition,
 EntityDefinition:EntityDefinition,
 StateDefinition:StateDefinition,
 ActionDefinition:ActionDefinition,
 Statement:Statement,
 CollectionTarget:CollectionTarget,
 CollectionOperation:CollectionOperation,
 OutputFormat:OutputFormat,
 OutputDefinition:OutputDefinition,
 ObservableDefinition:ObservableDefinition,
 Column:Column,
 StateControl:StateControl,
 SelectionBinding:SelectionBinding,
 FormInput:FormInput,
 ViewKind:ViewKind,
 ActionPlacement:ActionPlacement,
 ActionBinding:ActionBinding,
 KeyModifier:KeyModifier,
 KeyBinding:KeyBinding,
 ViewDefinition:ViewDefinition,
 AppDefinition:AppDefinition,
 VecDataValue:Vec<DataValue>,
 VecFieldDefinition:Vec<FieldDefinition>,
 VecStrings:Vec<Vec<String>>,
 VecString:Vec<String>,
 VecExpr:Vec<Expr>,
 VecSortKey:Vec<SortKey>,
 VecStatement:Vec<Statement>,
 VecColumn:Vec<Column>,
 VecStateControl:Vec<StateControl>,
 VecFormInput:Vec<FormInput>,
 VecActionBinding:Vec<ActionBinding>,
 VecKeyBinding:Vec<KeyBinding>,
 VecEntityDefinition:Vec<EntityDefinition>,
 VecStateDefinition:Vec<StateDefinition>,
 VecActionDefinition:Vec<ActionDefinition>,
 VecOutputDefinition:Vec<OutputDefinition>,
 VecObservableDefinition:Vec<ObservableDefinition>,
 VecViewDefinition:Vec<ViewDefinition>,
 BoxType:Box<Type>,
 BoxExpr:Box<Expr>,
 OptionString:Option<String>,
 OptionSelection:Option<SelectionBinding>,
 MapType:OrderedMap<String,Type>,
 MapExpr:OrderedMap<String,Expr>,
 MapData:OrderedMap<String,DataValue>,
 SetModifier:OrderedSet<KeyModifier>,
}};}
pub(super) use node_types;
macro_rules! schema_shapes{($structure:ident,$enumeration:ident)=>{
$enumeration!(Type{Boolean,Integer,Text,Date,Reference{entity:Id},List{item:Box<Type>},Optional{item:Box<Type>}});
$enumeration!(DataValue{Null,Boolean{value:bool},Integer{value:i64},Text{value:String},Date{days:i32},Reference{entity:Id,record:Id},List{item_type:Type,items:Vec<DataValue>}});
$enumeration!(Expr{Literal{value_type:Type,value:DataValue},Variable{name:Id},Field{record:Box<Expr>,field:Id},State{state:Id},Today,Query{entity:Id,binding:Id,predicate:Box<Expr>,sort:Vec<SortKey>,limit:usize,include_archived:bool},Filter{items:Box<Expr>,binding:Id,predicate:Box<Expr>},Map{items:Box<Expr>,binding:Id,value:Box<Expr>},Count{items:Box<Expr>},Any{items:Box<Expr>,binding:Id,predicate:Box<Expr>},All{items:Box<Expr>,binding:Id,predicate:Box<Expr>},Sum{items:Box<Expr>},Contains{items:Box<Expr>,value:Box<Expr>},Equal{left:Box<Expr>,right:Box<Expr>},Less{left:Box<Expr>,right:Box<Expr>},Add{left:Box<Expr>,right:Box<Expr>},Subtract{left:Box<Expr>,right:Box<Expr>},Concat{left:Box<Expr>,right:Box<Expr>},TextContains{text:Box<Expr>,search:Box<Expr>},Lower{value:Box<Expr>},And{values:Vec<Expr>},Or{values:Vec<Expr>},Not{value:Box<Expr>},If{condition:Box<Expr>,then_value:Box<Expr>,else_value:Box<Expr>},Coalesce{value:Box<Expr>,fallback:Box<Expr>},DateAdd{date:Box<Expr>,days:Box<Expr>},DateDifference{later:Box<Expr>,earlier:Box<Expr>}});
$structure!(SortKey{value:Expr,descending:bool});
$structure!(FieldDefinition{id:Id,label:String,value_type:Type});
$structure!(EntityDefinition{id:Id,label:String,fields:Vec<FieldDefinition>,unique:Vec<Vec<Id>>,constraints:Vec<Expr>});
$structure!(StateDefinition{id:Id,label:String,value_type:Type,initial:DataValue});
$structure!(ActionDefinition{id:Id,label:String,parameters:OrderedMap<Id, Type>,guards:Vec<Expr>,steps:Vec<Statement>,ensures:Vec<Expr>});
$enumeration!(Statement{Create{entity:Id,values:OrderedMap<Id, Expr>,bind:Id},Update{record:Expr,values:OrderedMap<Id, Expr>},Archive{record:Expr},SetState{state:Id,value:Expr},Collection{target:CollectionTarget,operation:CollectionOperation,items:Expr},ForEach{items:Expr,binding:Id,steps:Vec<Statement>},Assert{condition:Expr,message:String},Emit{output:Id,items:Expr,binding:Id,columns:OrderedMap<Id, Expr>}});
$enumeration!(CollectionTarget{State{state:Id},Field{record:Expr,field:Id}});
$enumeration!(CollectionOperation{Insert,Remove});
$enumeration!(OutputFormat{Csv,Json});
$structure!(OutputDefinition{id:Id,label:String,format:OutputFormat,columns:Vec<FieldDefinition>});
$structure!(ObservableDefinition{id:Id,label:String,value:Expr});
$structure!(Column{id:Id,label:String,value:Expr});
$structure!(StateControl{id:Id,label:String,state:Id,on_change:Option<Id>});
$structure!(SelectionBinding{id:Id,state:Id,on_change:Option<Id>});
$structure!(FormInput{parameter:Id,label:String});
$enumeration!(ViewKind{List{entity:Id,rows:Expr,columns:Vec<Column>,controls:Vec<StateControl>,selection:Option<SelectionBinding>},Detail{entity:Id,record:Expr,columns:Vec<Column>},Form{action:Id,fields:Vec<FormInput>,defaults:Values}});
$enumeration!(ActionPlacement{Toolbar,Row});
$structure!(ActionBinding{id:Id,label:String,placement:ActionPlacement,action:Id,arguments:OrderedMap<Id, Expr>,enabled:Expr});
$enumeration!(KeyModifier{Control,Alt,Shift,Command});
$structure!(KeyBinding{key:String,modifiers:OrderedSet<KeyModifier>,binding:Id});
$structure!(ViewDefinition{id:Id,label:String,kind:ViewKind,actions:Vec<ActionBinding>,keys:Vec<KeyBinding>});
$structure!(AppDefinition{version:u32,id:Id,label:String,entities:Vec<EntityDefinition>,state:Vec<StateDefinition>,actions:Vec<ActionDefinition>,outputs:Vec<OutputDefinition>,observables:Vec<ObservableDefinition>,views:Vec<ViewDefinition>,initial_view:Id});
};}
pub(super) use schema_shapes;

// Actual legacy value identities join the same sealed vocabulary. These are
// exhaustive field descriptions, not another schema or conversion boundary.
macro_rules! contract_shapes{($enumeration:ident)=>{
$enumeration!(ContractType{Boolean,Integer,Text,Date,Reference{entity:String},List{item:Box<ContractType>},Optional{item:Box<ContractType>}});
$enumeration!(ContractDataValue{Null,Boolean{value:bool},Integer{value:i64},Text{value:String},Date{days:i32},Reference{entity:String,record:String},List{item_type:ContractType,items:Vec<ContractDataValue>}});
};}
pub(super) use contract_shapes;

pub(super) trait NodeType:Sized{
 fn borrowed(&self)->Borrowed<'_>;
 fn owned(self)->Owned;
 fn take(node:Owned)->Self;
 fn staged(node:&mut Owned)->&mut Self;
}
pub(super) trait Shape:NodeType{
 fn children<'a>(&'a self,plan:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>;
 fn dismantle(self,pending:&mut Vec<Owned>);
}
// children needs the source lifetime, not an independently elided plan lifetime.
macro_rules! nodes{($($tag:ident:$ty:ty),+$(,)?)=>{
 #[derive(Clone,Copy)]pub(super) enum Borrowed<'a>{$($tag(&'a $ty)),+}
 pub(super) enum Owned{Empty,$($tag($ty)),+}
 $(impl NodeType for $ty{
  fn borrowed(&self)->Borrowed<'_>{Borrowed::$tag(self)}
  fn owned(self)->Owned{Owned::$tag(self)}
  fn take(node:Owned)->Self{match node{Owned::$tag(v)=>v,_=>unreachable!("private typed plan/slot invariant")}}
  fn staged(node:&mut Owned)->&mut Self{match node{Owned::$tag(v)=>v,_=>unreachable!("private container stage invariant")}}
 })+
 impl<'a> Borrowed<'a>{
  fn children(self,plan:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{match self{$(Self::$tag(v)=>v.children(plan,parent,cx)),+}}
  pub(super) fn start(self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>{match self{$(Self::$tag(v)=>v.start(cx)),+}}
  pub(super) fn finish(self,node:&mut Owned,children:&mut[Owned],cx:&mut AllocationContext)->Result<(),AllocationFailure>{match self{$(Self::$tag(v)=>v.finish(node,children,cx)),+}}
  pub(super) fn same_head(self,b:Self)->bool{match(self,b){$((Self::$tag(a),Self::$tag(b))=>a.same_head(b),)+_=>false}}
 }
 impl Owned{fn dismantle(self,pending:&mut Vec<Owned>){match self{Self::Empty=>(),$(Self::$tag(v)=>v.dismantle(pending)),+}}}
};}
node_types!(nodes);

#[derive(Clone,Copy)]pub(super) struct Record<'a>{
 pub(super) node:Borrowed<'a>,pub(super) parent:usize,
 pub(super) start:usize,pub(super) end:usize,
}
pub(super) struct Plan<'a>{pub(super) records:Vec<Record<'a>>}
impl<'a> Plan<'a>{
 pub(super) fn new(root:Borrowed<'a>,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut plan=Self{records:Vec::new()};plan.append(root,0,cx)?;
  let mut i=0;
  while i<plan.records.len(){
   let node=plan.records[i].node;let start=plan.records.len();
   node.children(&mut plan,i,cx)?;
   plan.records[i].start=start;plan.records[i].end=plan.records.len();i+=1;
  }
  Ok(plan)
 }
 fn append(&mut self,node:Borrowed<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{
  if self.records.len()==self.records.capacity(){
   // extent checks len + additional and bytes. No unchecked doubling.
   let additional=self.records.capacity().max(1);
   try_reserve(&mut self.records,additional,cx,AllocationSite::Vector)?;
  }
  self.records.push(Record{node,parent,start:0,end:0});Ok(())
 }
 // Declaration/index/sorted-key preorder, without a depth-sized call stack.
 pub(super) fn next(&self,mut i:usize)->Option<usize>{
  let r=self.records[i];if r.start!=r.end{return Some(r.start)}
  loop{if i==0{return None}let parent=self.records[i].parent;
   if i+1<self.records[parent].end{return Some(i+1)}i=parent;
  }
 }
}

// Exact closed field destructuring makes added/omitted schema fields a compiler
// error. Each transferred token was counted by children before ownership began.
macro_rules! structure_shape{($t:ident{$($f:ident:$ty:ty),+})=>{impl Shape for $t{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{let Self{$($f),+}=self;$(p.append($f.borrowed(),parent,cx)?;)+Ok(())}
 fn dismantle(self,p:&mut Vec<Owned>){let Self{$($f),+}=self;$(p.push($f.owned());)+}
}};}
macro_rules! enum_shape{($t:ident{$($v:ident$({$($f:ident:$ty:ty),+})?),+})=>{impl Shape for $t{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{match self{$(Self::$v$({$($f),+})?=>{$($(p.append($f.borrowed(),parent,cx)?;)+)?},)+}Ok(())}
 fn dismantle(self,p:&mut Vec<Owned>){match self{$(Self::$v$({$($f),+})?=>{$($(p.push($f.owned());)+)?},)+}}
}};}
schema_shapes!(structure_shape,enum_shape);
contract_shapes!(enum_shape);
macro_rules! scalar_shape{($($t:ty),+)=>{$(impl Shape for $t{
 fn children<'a>(&'a self,_:&mut Plan<'a>,_:usize,_:&mut AllocationContext)->Result<(),AllocationFailure>{Ok(())}
 fn dismantle(self,_:&mut Vec<Owned>){}
})+};}
scalar_shape!(bool,u32,i64,i32,usize,String);
impl<T:NodeType> Shape for Vec<T> where Self:NodeType{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{for v in self{p.append(v.borrowed(),parent,cx)?}Ok(())}
 fn dismantle(self,p:&mut Vec<Owned>){for v in self{p.push(v.owned())}}
}
impl<T:NodeType> Shape for Box<T> where Self:NodeType{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{p.append((**self).borrowed(),parent,cx)}
 fn dismantle(self,p:&mut Vec<Owned>){p.push((*self).owned())}
}
impl<T:NodeType> Shape for Option<T> where Self:NodeType{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{if let Some(v)=self{p.append(v.borrowed(),parent,cx)?}Ok(())}
 fn dismantle(self,p:&mut Vec<Owned>){if let Some(v)=self{p.push(v.owned())}}
}
impl<T:NodeType> Shape for OrderedMap<String,T> where Self:NodeType{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{for(k,v)in self.iter(){p.append(k.borrowed(),parent,cx)?;p.append(v.borrowed(),parent,cx)?}Ok(())}
 fn dismantle(self,p:&mut Vec<Owned>){for(k,v)in self.into_entries(){p.push(k.owned());p.push(v.owned())}}
}
impl Shape for OrderedSet<KeyModifier>{
 fn children<'a>(&'a self,p:&mut Plan<'a>,parent:usize,cx:&mut AllocationContext)->Result<(),AllocationFailure>{for k in self.iter(){p.append(k.borrowed(),parent,cx)?}Ok(())}
 fn dismantle(self,_:&mut Vec<Owned>){} // only closed Copy scalar keys
}

fn drain(pending:&mut Vec<Owned>){while let Some(node)=pending.pop(){node.dismantle(pending)}}
/// Private guard: every slot is initialized; no owner crosses a fallible step
/// outside these slots. The capacity proof counts ALL tokens, including edges.
pub(super) struct Stage{pub(super) nodes:Vec<Owned>,pending:Vec<Owned>}
impl Stage{
 pub(super) fn new(n:usize,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut nodes=Vec::new();let mut pending=Vec::new();
  try_reserve(&mut nodes,n,cx,AllocationSite::Vector)?;
  try_reserve(&mut pending,n,cx,AllocationSite::Vector)?;
  nodes.resize_with(n,||Owned::Empty);Ok(Self{nodes,pending})
 }
 pub(super) fn commit<T>(mut self)->OwnedAppGraph<T>{
  let tokens=self.nodes.len();
  let root=mem::replace(&mut self.nodes[0],Owned::Empty);
  OwnedAppGraph{root,pending:mem::take(&mut self.pending),tokens,marker:PhantomData}
 }
}
impl Drop for Stage{fn drop(&mut self){
 for slot in &mut self.nodes{
  let node=mem::replace(slot,Owned::Empty);
  if !matches!(node,Owned::Empty){self.pending.push(node);drain(&mut self.pending);}
 }
}}
/// A managed graph and its retained nonallocating teardown workspace. No
/// mutable access, DerefMut, raw extraction, or infallible Clone. Borrowing this
/// owner does not make a separately owned raw source safe to drop.
pub struct OwnedAppGraph<T>{root:Owned,pending:Vec<Owned>,tokens:usize,marker:PhantomData<T>}
impl<T> OwnedAppGraph<T>{
 // Credits count exact token occurrences, never allocator-provided capacity.
 // Live roots have N>=1, empty pending, and capacity>=N. Private prepared or
 // consumed roots are Empty/0 and are never exposed through as_ref.
 fn token_sum(current:usize,additional:usize,cx:&mut AllocationContext)->Result<usize,AllocationFailure>{
  cx.extent(current,additional,mem::size_of::<Owned>(),AllocationSite::Vector).map(|(n,_)|n)
 }
 fn prepare_empty(tokens:usize,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut pending=Vec::new();try_reserve(&mut pending,tokens,cx,AllocationSite::Vector)?;
  Ok(Self{root:Owned::Empty,pending,tokens:0,marker:PhantomData})
 }
 // Only the closed transfer implementation calls these, after every fallible
 // operation. install replaces an Empty root; take leaves the old guard Empty.
 fn install(&mut self,root:Owned,tokens:usize){self.root=root;self.tokens=tokens;}
 fn take_root(&mut self)->Owned{self.tokens=0;mem::replace(&mut self.root,Owned::Empty)}
 #[cfg(test)]pub(crate) fn test_transfer_state(&self)->(usize,usize,usize){
  (self.tokens,self.pending.capacity(),self.pending.as_ptr() as usize)
 }
}
impl<T> Drop for OwnedAppGraph<T>{fn drop(&mut self){
 if !matches!(self.root,Owned::Empty){
  self.pending.push(mem::replace(&mut self.root,Owned::Empty));drain(&mut self.pending);
 }
}}
#[path = "managed_value.rs"] mod managed_value;
pub(crate) use managed_value::ManagedListBuilder;
// During drain, expansion replaces a token by its original child tokens. The
// number pending cannot exceed N undischarged original tokens. All pushes use
// the capacity reserved for N; emptied Vec/map iterators and Box shells release
// shallow storage exactly once. Flat stage scanning adds only one token at a
// time to an empty pending buffer. No input-dependent cleanup allocation/panic.
macro_rules! roots{($($t:ident),+)=>{$(impl $t{
 pub fn try_copy(&self,cx:&mut AllocationContext)->Result<OwnedAppGraph<Self>,AllocationFailure>{super::copy::copy(self.borrowed(),cx)}
 pub fn try_prepare_eq<'a>(&'a self,other:&'a Self,cx:&mut AllocationContext)->Result<super::equality::PreparedAppEq<'a,Self>,AllocationFailure>{super::equality::prepare(self.borrowed(),other.borrowed(),cx)}
}
impl OwnedAppGraph<$t>{pub fn as_ref(&self)->&$t{match &self.root{Owned::$t(v)=>v,_=>unreachable!("private managed root type invariant")}}})+};}
roots!(Type,DataValue,Expr,SortKey,FieldDefinition,EntityDefinition,StateDefinition,ActionDefinition,Statement,CollectionTarget,CollectionOperation,OutputFormat,OutputDefinition,ObservableDefinition,Column,StateControl,SelectionBinding,FormInput,ViewKind,ActionPlacement,ActionBinding,KeyModifier,KeyBinding,ViewDefinition,AppDefinition);
// Concrete constructors live on our existing owner rather than on contract
// types, so one real contract identity also works across a crate boundary.
macro_rules! contract_roots{($($t:ident),+)=>{$(impl OwnedAppGraph<$t>{
 pub fn try_copy(value:&$t,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{super::copy::copy(value.borrowed(),cx)}
 pub fn try_prepare_eq<'a>(left:&'a $t,right:&'a $t,cx:&mut AllocationContext)->Result<super::equality::PreparedAppEq<'a,$t>,AllocationFailure>{super::equality::prepare(left.borrowed(),right.borrowed(),cx)}
 pub fn as_ref(&self)->&$t{match &self.root{Owned::$t(v)=>v,_=>unreachable!("private managed contract root type invariant")}}
})+};}
contract_roots!(ContractType,ContractDataValue);
#[cfg(test)]pub(crate) const BORROWED_RECORD_BYTES:usize=mem::size_of::<Record<'static>>();
#[cfg(test)]pub(crate) const OWNED_NODE_BYTES:usize=mem::size_of::<Owned>();
