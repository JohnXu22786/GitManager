//! Preparation allocates; equal() does not. Immutable source borrows keep both
//! flat plans valid. Direct field/tag comparison, never serialization or hashes.
use super::*;
use super::owned_graph::{Borrowed,NodeType,Plan,schema_shapes,contract_shapes};
use crate::product_contract::{Type as ContractType,DataValue as ContractDataValue};
use crate::product_alloc::{AllocationContext,AllocationFailure,ordered::{OrderedMap,OrderedSet}};
use std::marker::PhantomData;

pub(super) trait EqualNode:NodeType{fn same_head(&self,other:&Self)->bool;}
macro_rules! structure_equal{($t:ident{$($f:ident:$ty:ty),+})=>{impl EqualNode for $t{
 fn same_head(&self,_:&Self)->bool{true} // all fields have separate plan tokens
}};}
macro_rules! enum_equal{($t:ident{$($v:ident$({$($f:ident:$ty:ty),+})?),+})=>{impl EqualNode for $t{
 fn same_head(&self,other:&Self)->bool{std::mem::discriminant(self)==std::mem::discriminant(other)}
}};}
schema_shapes!(structure_equal,enum_equal);
contract_shapes!(enum_equal);
macro_rules! scalar_equal{($($t:ty),+)=>{$(impl EqualNode for $t{fn same_head(&self,b:&Self)->bool{self==b}})+};}
scalar_equal!(bool,u32,i64,i32,usize,String);
impl<T:NodeType> EqualNode for Vec<T> where Self:NodeType{fn same_head(&self,b:&Self)->bool{self.len()==b.len()}}
impl<T:NodeType> EqualNode for Box<T> where Self:NodeType{fn same_head(&self,_:&Self)->bool{true}}
impl<T:NodeType> EqualNode for Option<T> where Self:NodeType{fn same_head(&self,b:&Self)->bool{self.is_some()==b.is_some()}}
impl<T:NodeType> EqualNode for OrderedMap<String,T> where Self:NodeType{fn same_head(&self,b:&Self)->bool{self.len()==b.len()}}
impl EqualNode for OrderedSet<KeyModifier>{fn same_head(&self,b:&Self)->bool{self.len()==b.len()}}
/// A fallibly prepared immutable comparison. Only equal(), not preparation, is
/// allocation-free. No external scratch API or context-free recursive Eq facade.
pub struct PreparedAppEq<'a,T>{left:Plan<'a>,right:Plan<'a>,marker:PhantomData<&'a T>}
impl<T> PreparedAppEq<'_,T>{
 pub fn equal(&mut self)->bool{
  let(mut a,mut b)=(0,0);
  loop{
   if !self.left.records[a].node.same_head(self.right.records[b].node){return false}
   match(self.left.next(a),self.right.next(b)){
    (Some(x),Some(y))=>{a=x;b=y},(None,None)=>return true,_=>return false,
   }
  }
 }
}
pub(super) fn prepare<'a,T>(a:Borrowed<'a>,b:Borrowed<'a>,cx:&mut AllocationContext)->Result<PreparedAppEq<'a,T>,AllocationFailure>{
 let left=Plan::new(a,cx)?;let right=Plan::new(b,cx)?;
 Ok(PreparedAppEq{left,right,marker:PhantomData})
}
