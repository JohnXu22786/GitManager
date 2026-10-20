//! Paid-row payload and container operations. Ordinary callers never enter here.
use super::*;
use std::mem::size_of;

pub(super) fn resource(cause: WorkFailure) -> AdapterError {
    match cause {
        WorkFailure::Cancelled => AdapterError::Cancelled,
        WorkFailure::BudgetExhausted(message) => exhausted(message),
    }
}
pub(super) fn contract(error: ContractWorkError) -> AdapterError {
    match error {
        ContractWorkError::Contract(error) => AdapterError::Invalid(error),
        ContractWorkError::Resource(cause) => resource(cause),
    }
}
pub(super) fn overflow() -> AdapterError { exhausted("capture work arithmetic overflow") }
pub(super) fn add(a:u64,b:u64)->Result<u64> { a.checked_add(b).ok_or_else(overflow) }
pub(super) fn mul(a:u64,b:u64)->Result<u64> { a.checked_mul(b).ok_or_else(overflow) }
pub(super) fn number(n:usize)->Result<u64> { u64::try_from(n).map_err(|_|overflow()) }
pub(super) fn pay(work:&mut dyn ContractWork,n:u64)->Result<()> {work.charge(n).map_err(resource)}
pub(super) fn string(value:&str,work:&mut dyn ContractWork)->Result<()> {
    pay(work,8)?;
    let n=capture_payment::string_copy_bytes(value.len()).map_err(contract)?;
    pay(work,n)
}
pub(super) fn eq_string(a:&str,b:&str,work:&mut dyn ContractWork)->Result<bool> {
    pay(work,8)?;
    let n=if a.len()==b.len(){add(3,mul(2,number(a.len())?)?)?}else{3};
    pay(work,n)?;
    Ok(a==b)
}
pub(super) fn search(m:usize,key_bytes:usize,work:&mut dyn ContractWork)->Result<()> {
    pay(work,20+4*u64::from(usize::BITS))?;
    let c=add(3,mul(2,number(key_bytes)?)?)?;
    let n=capture_payment::btree_search(m,c).map_err(contract)?;
    pay(work,n)
}
pub(super) fn insert<K,V>(m:usize,key_bytes:usize,work:&mut dyn ContractWork)->Result<()> {
    pay(work,104+4*u64::from(usize::BITS))?;
    let c=add(3,mul(2,number(key_bytes)?)?)?;
    let n=capture_payment::btree_insert::<K,V>(m,c).map_err(contract)?;
    pay(work,n)
}
pub(super) fn typ(t:&Type,work:&mut dyn ContractWork)->Result<()> {
    pay(work,4)?; // descriptor node/tag plus new node/cleanup
    match t {
        Type::Reference{entity}=>string(entity,work),
        Type::List{item}|Type::Optional{item}=>{
            pay(work,5+2*size_of::<Type>() as u64)?;
            typ(item,work)
        }
        _=>Ok(())
    }
}
pub(super) fn value(v:&DataValue,work:&mut dyn ContractWork)->Result<()> {
    pay(work,4)?;
    match v {
        DataValue::Text{value}=>string(value,work),
        DataValue::Reference{entity,record}=>{string(entity,work)?;string(record,work)},
        DataValue::List{item_type,items}=>{
            typ(item_type,work)?;
            pay(work,10)?;
            pay(work,add(4,mul(2*size_of::<DataValue>() as u64,number(items.len())?)?)?)?;
            for item in items {pay(work,6)?;value(item,work)?;}
            pay(work,1)
        }
        _=>Ok(())
    }
}
pub(super) fn env(env:&Env,work:&mut dyn ContractWork)->Result<()> {
    pay(work,64)?;
    let n=capture_payment::btree_clone::<Id,(Type,DataValue)>(env.len(),0).map_err(contract)?;
    pay(work,n)?;
    pay(work,8)?;
    pay(work,add(8,mul(27,number(env.len())?)?)?)?;
    for (key,(t,v)) in env {string(key,work)?;typ(t,work)?;value(v,work)?;}
    Ok(())
}
pub(super) fn bind(env:&mut Env,name:&str,t:Type,v:DataValue,work:&mut dyn ContractWork)->Result<()> {
    string(name,work)?;
    insert::<Id,(Type,DataValue)>(env.len(),name.len(),work)?;
    env.insert(name.to_owned(),(t,v));
    Ok(())
}
pub(super) fn reference(entity:&str,record:&str,work:&mut dyn ContractWork)->Result<DataValue> {
    pay(work,4)?;string(entity,work)?;string(record,work)?;
    Ok(DataValue::Reference{entity:entity.to_owned(),record:record.to_owned()})
}
pub(super) fn reference_type(entity:&str,work:&mut dyn ContractWork)->Result<Type> {
    pay(work,2)?;string(entity,work)?;Ok(Type::reference(entity))
}
pub(super) fn list_type(t:Type,work:&mut dyn ContractWork)->Result<Type> {
    pay(work,7+2*size_of::<Type>() as u64)?;Ok(Type::list(t))
}
pub(super) fn push<T>(items:&mut Vec<T>,item:T,work:&mut dyn ContractWork)->Result<()> {
    pay(work,8+size_of::<T>() as u64)?;
    if items.len()==items.capacity() {
        pay(work,12)?;
        let m=items.len();let g=m.checked_mul(2).ok_or_else(overflow)?.max(1);
        let n=add(1,mul(number(m.checked_add(g).ok_or_else(overflow)?)?,size_of::<T>() as u64)?)?;
        pay(work,n)?;
        items.reserve_exact(g-m);
    }
    items.push(item);Ok(())
}
fn type_eq_shape(t:&Type,work:&mut dyn ContractWork)->Result<()> {
    pay(work,5)?;
    match t {
        Type::Reference{entity}=>{pay(work,8)?;pay(work,add(3,mul(2,number(entity.len())?)?)?)},
        Type::List{item}|Type::Optional{item}=>{pay(work,4)?;type_eq_shape(item,work)},
        _=>Ok(())
    }
}
pub(super) fn type_equal(a:&Type,b:&Type,work:&mut dyn ContractWork)->Result<bool> {
    type_eq_shape(a,work)?;Ok(a==b)
}
fn value_eq_shape(v:&DataValue,work:&mut dyn ContractWork)->Result<()> {
    pay(work,5)?;
    match v {
        DataValue::Text{value}=>{pay(work,8)?;pay(work,add(3,mul(2,number(value.len())?)?)?)},
        DataValue::Reference{entity,record}=>{
            pay(work,16)?;pay(work,add(6,mul(2,add(number(entity.len())?,number(record.len())?)?)?)?)
        }
        DataValue::List{item_type,items}=>{
            type_eq_shape(item_type,work)?;pay(work,5)?;
            for x in items {pay(work,5)?;value_eq_shape(x,work)?;}pay(work,1)
        }
        _=>Ok(())
    }
}
pub(super) fn equal(a:&DataValue,b:&DataValue,work:&mut dyn ContractWork)->Result<bool> {
    value_eq_shape(a,work)?;Ok(a==b)
}
pub(super) fn compare(a:&DataValue,b:&DataValue,work:&mut dyn ContractWork)->Result<std::cmp::Ordering> {
    pay(work,5)?;
    match(a,b) {
        (DataValue::Text{value:a},DataValue::Text{value:b})=>{
            pay(work,8)?;pay(work,add(3,mul(2,number(a.len().min(b.len()))?)?)?)?;
            Ok(a.cmp(b))
        }
        (DataValue::Reference{entity:a,record:x},DataValue::Reference{entity:b,record:y})=>{
            pay(work,8)?;pay(work,add(3,mul(2,number(a.len().min(b.len()))?)?)?)?;
            let order=a.cmp(b);
            if order!=std::cmp::Ordering::Equal{return Ok(order)}
            pay(work,8)?;pay(work,add(3,mul(2,number(x.len().min(y.len()))?)?)?)?;
            Ok(x.cmp(y))
        }
        _=>Ok(eval::compare(a,b))
    }
}
pub(super) fn record<'s>(data:&'s DataSnapshot,value:&DataValue,work:&mut dyn ContractWork)->Result<&'s Record> {
    pay(work,2)?;
    let DataValue::Reference{entity,record}=value else{return Err(failed("expected record reference"))};
    // Original helper creates a RecordRef: fund both owners before that copy.
    pay(work,2)?;string(entity,work)?;string(record,work)?;
    let key=RecordRef{entity:entity.clone(),record:record.clone()};
    pay(work,1)?;
    for r in &data.records {
        pay(work,3)?;
        if eq_string(&r.entity,&key.entity,work)? && eq_string(&r.id,&key.record,work)? {pay(work,1)?;return Ok(r)}
    }
    pay(work,1)?;Err(invalid("record reference does not exist"))
}
