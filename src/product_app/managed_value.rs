//! Closed actual-contract construction and accumulation. Every move commits
//! only after both teardown credit and payload storage have been secured.
use super::*;
use crate::product_alloc::{try_box,try_copy_str};

impl OwnedAppGraph<ContractType>{
 pub(crate) fn try_boolean(cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(1,cx)?;out.install(ContractType::Boolean.owned(),1);Ok(out)
 }
 pub(crate) fn try_integer(cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(1,cx)?;out.install(ContractType::Integer.owned(),1);Ok(out)
 }
 pub(crate) fn try_text(cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(1,cx)?;out.install(ContractType::Text.owned(),1);Ok(out)
 }
 pub(crate) fn try_date(cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(1,cx)?;out.install(ContractType::Date.owned(),1);Ok(out)
 }
 pub(crate) fn try_reference(entity:&str,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(2,cx)?;
  let entity=try_copy_str(entity,cx,AllocationSite::String)?;
  out.install(ContractType::Reference{entity}.owned(),2);Ok(out)
 }
 pub(crate) fn try_list(mut item:Self,cx:&mut AllocationContext)->Result<Self,(AllocationFailure,Self)>{
  let tokens=match Self::token_sum(item.tokens,2,cx){Ok(n)=>n,Err(e)=>return Err((e,item))};
  let mut out=match Self::prepare_empty(tokens,cx){Ok(out)=>out,Err(e)=>return Err((e,item))};
  // A failed raw Box allocation may drop only this shallow shell, never item.
  let mut shell=match try_box(ContractType::Boolean,cx){Ok(shell)=>shell,Err(e)=>return Err((e,item))};
  // Infallible commit frontier: the source remains guarded until this point.
  *shell=ContractType::take(item.take_root());
  out.install(ContractType::List{item:shell}.owned(),tokens);Ok(out)
 }
 pub(crate) fn into_list_item(mut self)->Result<Self,Self>{
  if !matches!(self.as_ref(),ContractType::List{..}){return Err(self)}
  let tokens=self.tokens.checked_sub(2).expect("private List credit invariant");
  let ContractType::List{item}=ContractType::take(self.take_root()) else{unreachable!("checked List tag")};
  // Moving from the Box releases only its empty shell. The same retained
  // workspace already covers the child; no allocation or planning is needed.
  self.install((*item).owned(),tokens);Ok(self)
 }
}
impl OwnedAppGraph<ContractDataValue>{
 pub(crate) fn try_null(cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(1,cx)?;out.install(ContractDataValue::Null.owned(),1);Ok(out)
 }
 pub(crate) fn try_boolean(value:bool,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(2,cx)?;out.install(ContractDataValue::Boolean{value}.owned(),2);Ok(out)
 }
 pub(crate) fn try_integer(value:i64,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(2,cx)?;out.install(ContractDataValue::Integer{value}.owned(),2);Ok(out)
 }
 pub(crate) fn try_date(days:i32,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(2,cx)?;out.install(ContractDataValue::Date{days}.owned(),2);Ok(out)
 }
 pub(crate) fn try_text(value:&str,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(2,cx)?;
  let value=try_copy_str(value,cx,AllocationSite::String)?;
  out.install(ContractDataValue::Text{value}.owned(),2);Ok(out)
 }
 pub(crate) fn try_reference(entity:&str,record:&str,cx:&mut AllocationContext)->Result<Self,AllocationFailure>{
  let mut out=Self::prepare_empty(3,cx)?;
  let entity=try_copy_str(entity,cx,AllocationSite::String)?;
  let record=try_copy_str(record,cx,AllocationSite::String)?;
  out.install(ContractDataValue::Reference{entity,record}.owned(),3);Ok(out)
 }
 pub(crate) fn try_take_text(value:String,cx:&mut AllocationContext)->Result<Self,(AllocationFailure,String)>{
  let mut out=match Self::prepare_empty(2,cx){Ok(out)=>out,Err(e)=>return Err((e,value))};
  out.install(ContractDataValue::Text{value}.owned(),2);Ok(out)
 }
}

// A facade over the one existing owner, not a second graph or teardown engine.
// A capacity hint is not a limit. Partial accumulation is always a complete
// managed List; callers may inspect, finish, or drop it after any refusal.
pub(crate) struct ManagedListBuilder{owner:OwnedAppGraph<ContractDataValue>}
impl ManagedListBuilder{
 pub(crate) fn try_new(mut item_type:OwnedAppGraph<ContractType>,reserve_items:usize,cx:&mut AllocationContext)
  ->Result<Self,(AllocationFailure,OwnedAppGraph<ContractType>)>{
  let tokens=match OwnedAppGraph::<ContractDataValue>::token_sum(item_type.tokens,2,cx){Ok(n)=>n,Err(e)=>return Err((e,item_type))};
  let mut owner=match OwnedAppGraph::prepare_empty(tokens,cx){Ok(owner)=>owner,Err(e)=>return Err((e,item_type))};
  let mut items=Vec::new();
  if let Err(e)=try_reserve(&mut items,reserve_items,cx,AllocationSite::Vector){return Err((e,item_type))}
  let item_type=ContractType::take(item_type.take_root());
  owner.install(ContractDataValue::List{item_type,items}.owned(),tokens);Ok(Self{owner})
 }
 pub(crate) fn try_push(&mut self,mut item:OwnedAppGraph<ContractDataValue>,cx:&mut AllocationContext)
  ->Result<(),(AllocationFailure,OwnedAppGraph<ContractDataValue>)>{
  let tokens=match OwnedAppGraph::<ContractDataValue>::token_sum(self.owner.tokens,item.tokens,cx){Ok(n)=>n,Err(e)=>return Err((e,item))};
  let ContractDataValue::List{items,..}=self.owner.as_ref() else{unreachable!("private builder List invariant")};
  // Validate both arithmetic extents before either guard is emptied, including
  // payload length/bytes before a teardown reservation can change capacity.
  if let Err(e)=cx.extent(items.len(),1,mem::size_of::<ContractDataValue>(),AllocationSite::Vector){return Err((e,item))}
  if let Err(e)=try_reserve(&mut self.owner.pending,tokens,cx,AllocationSite::Vector){return Err((e,item))}
  let ContractDataValue::List{items,..}=ContractDataValue::staged(&mut self.owner.root) else{unreachable!("private builder List invariant")};
  if let Err(e)=try_reserve(items,1,cx,AllocationSite::Vector){return Err((e,item))}
  // All later work is a shallow move into reserved storage. No naked deep
  // value crosses a fallible call; the old empty source releases only workspace.
  items.push(ContractDataValue::take(item.take_root()));self.owner.tokens=tokens;Ok(())
 }
 pub(crate) fn as_ref(&self)->&ContractDataValue{self.owner.as_ref()}
 pub(crate) fn finish(self)->OwnedAppGraph<ContractDataValue>{self.owner}
 #[cfg(test)]pub(crate) fn test_transfer_state(&self)->(usize,usize,usize){self.owner.test_transfer_state()}
}
