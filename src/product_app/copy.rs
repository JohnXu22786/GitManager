//! Allocate-before-owner-transfer copy. Enter reservations preserve semantic
//! reserve-before-child order despite postorder parent assembly. No depth cap.
use super::*;
use super::owned_graph::{Borrowed,Owned,NodeType,Plan,Stage,schema_shapes,contract_shapes};
use crate::product_contract::{Type as ContractType,DataValue as ContractDataValue};
use crate::product_alloc::{try_box,try_copy_str,try_reserve,AllocationContext,
 AllocationFailure,AllocationSite,ordered::{OrderedMap,OrderedSet}};
use std::mem;

pub(super) trait CopyNode:NodeType{
 fn start(&self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>;
 fn finish(&self,node:&mut Owned,children:&mut[Owned],cx:&mut AllocationContext)->Result<(),AllocationFailure>;
}
fn take<T:NodeType>(node:&mut Owned)->T{T::take(mem::replace(node,Owned::Empty))}
fn field<T:NodeType>(children:&mut std::slice::IterMut<'_,Owned>)->T{
 // A private plan supplies exactly the fields from the same exhaustive shape.
 take(children.next().expect("private plan field arity invariant"))
}
macro_rules! structure_copy{($t:ident{$($f:ident:$ty:ty),+})=>{impl CopyNode for $t{
 fn start(&self,_:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok(Owned::Empty)}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{
  let Self{$($f:_),+}=self;let mut children=children.iter_mut();
  *node=Self{$($f:field::<$ty>(&mut children)),+}.owned();Ok(())
 }
}};}
macro_rules! enum_copy{($t:ident{$($v:ident$({$($f:ident:$ty:ty),+})?),+})=>{impl CopyNode for $t{
 fn start(&self,_:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok(Owned::Empty)}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{
  let mut children=children.iter_mut();
  *node=match self{$(Self::$v$({$($f:_),+})?=>Self::$v$({$($f:field::<$ty>(&mut children)),+})?,)+}.owned();Ok(())
 }
}};}
schema_shapes!(structure_copy,enum_copy);
contract_shapes!(enum_copy);
macro_rules! scalar_copy{($($t:ty),+)=>{$(impl CopyNode for $t{
 fn start(&self,_:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok((*self).owned())}
 fn finish(&self,_:&mut Owned,_:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{Ok(())}
})+};}
scalar_copy!(bool,u32,i64,i32,usize);
impl CopyNode for String{
 fn start(&self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok(try_copy_str(self,cx,AllocationSite::String)?.owned())}
 fn finish(&self,_:&mut Owned,_:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{Ok(())}
}
impl<T:NodeType> CopyNode for Vec<T> where Self:NodeType{
 fn start(&self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>{let mut v=Self::new();try_reserve(&mut v,self.len(),cx,AllocationSite::Vector)?;Ok(v.owned())}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{
  let v=Self::staged(node);for child in children{v.push(take::<T>(child))}Ok(())
 }
}
impl<T:NodeType> CopyNode for Option<T> where Self:NodeType{
 fn start(&self,_:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok(Owned::Empty)}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],_:&mut AllocationContext)->Result<(),AllocationFailure>{
  *node=match self{None=>None,Some(_)=>Some(take::<T>(&mut children[0]))}.owned();Ok(())
 }
}
macro_rules! box_copy{($t:ty,$shallow:expr)=>{impl CopyNode for Box<$t>{
 fn start(&self,_:&mut AllocationContext)->Result<Owned,AllocationFailure>{Ok(Owned::Empty)}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],cx:&mut AllocationContext)->Result<(),AllocationFailure>{
  // Child remains guarded on refusal. Allocation sees only a shallow sentinel.
  let mut shell=try_box($shallow,cx)?;*shell=take::<$t>(&mut children[0]);
  *node=shell.owned();Ok(())
 }
}};}
box_copy!(Type,Type::Boolean);box_copy!(Expr,Expr::Today);
box_copy!(ContractType,ContractType::Boolean);
impl<T:NodeType> CopyNode for OrderedMap<String,T> where Self:NodeType{
 fn start(&self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>{let mut m=Self::new();m.try_reserve(self.len(),cx)?;Ok(m.owned())}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],cx:&mut AllocationContext)->Result<(),AllocationFailure>{
  let m=Self::staged(node);
  for entry in children.chunks_exact_mut(2){let key=take::<String>(&mut entry[0]);let value=take::<T>(&mut entry[1]);
   // Local no-refusal proof: start reserved final source length (and checked its
   // extent). This loop inserts exactly that many unique byte-identical keys
   // in sorted order. Each current len+1 <= final length <= capacity, so the
   // foundation's reserve does not request growth/overflow, even sticky cx.
   // String Ord is closed/nonallocating. No replacement/deep temporary Drop.
   // The map itself remains in its guarded parent slot throughout the loop.
   m.try_insert(key,value,cx)?;
  }Ok(())
 }
}
impl CopyNode for OrderedSet<KeyModifier>{
 fn start(&self,cx:&mut AllocationContext)->Result<Owned,AllocationFailure>{let mut s=Self::new();s.try_reserve(self.len(),cx)?;Ok(s.owned())}
 fn finish(&self,node:&mut Owned,children:&mut[Owned],cx:&mut AllocationContext)->Result<(),AllocationFailure>{
  let s=Self::staged(node);for child in children{s.try_insert(take::<KeyModifier>(child),cx)?;}Ok(())
 }
}
pub(super) fn copy<T>(root:Borrowed<'_>,cx:&mut AllocationContext)->Result<OwnedAppGraph<T>,AllocationFailure>{
 let plan=Plan::new(root,cx)?;let mut stage=Stage::new(plan.records.len(),cx)?;
 let mut i=0;
 loop{
  let r=plan.records[i];stage.nodes[i]=r.node.start(cx)?;
  if r.start!=r.end{i=r.start;continue}
  loop{
   let r=plan.records[i];
   // BFS records give every child a larger index than its parent; direct
   // children occupy this exact contiguous range, even for empty leaves.
   let (parents,children)=stage.nodes.split_at_mut(r.start);
   r.node.finish(&mut parents[i],&mut children[..r.end-r.start],cx)?;
   if i==0{return Ok(stage.commit())}
   let parent=r.parent;
   if i+1<plan.records[parent].end{i+=1;break}i=parent;
  }
 }
}
