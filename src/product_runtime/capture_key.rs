//! Closed, directly owned capture-key graph. No serde, hash, derived deep
//! equality/clone or caller-supplied length participates in production identity.
use super::*;
use super::capture_cost as cost;
use super::capture_cost::pay;
use std::cmp::Ordering as KeyOrder;
use std::mem::size_of;

pub(super) struct OwnedCaptureKey {
    entity:String,
    binding:String,
    expression:Expr,
    expected:Type,
    limits:RuntimeLimits,
    wire_len:usize,
}
// Caller prepays before entering key, including the Option/result transport
// through prepare/finish/publish and abandonment on every early-return path.
pub(super) const KEY_FRAME:u64=64+4*(size_of::<&str>()*2+size_of::<&Expr>()+size_of::<&Type>()+size_of::<&RuntimeLimits>()+size_of::<&mut dyn ContractWork>()+size_of::<OwnedCaptureKey>()+size_of::<Option<OwnedCaptureKey>>()*3+size_of::<Result<Option<OwnedCaptureKey>>>()+size_of::<AdapterError>()+size_of::<usize>()*2+size_of::<u64>()+size_of::<String>()*2+size_of::<Expr>()+size_of::<Type>()+size_of::<RuntimeLimits>()+size_of::<u64>()) as u64;
macro_rules! invoke {
    ($work:expr,$frame:expr,$call:expr)=>{{pay($work,$frame)?;$call}};
}
// Arithmetic occurs only after its positive packet. Failure uses the existing
// static resource cause and never needs a successful debit for cleanup.
macro_rules! wire_add {
    ($work:expr,$a:expr,$b:expr)=>{{let rhs=$b;pay($work,32)?;($a).checked_add(rhs).ok_or_else(cost::overflow)?}};
}
const LENGTH_TEXT_FRAME:u64=64+4*(size_of::<&str>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<std::slice::Iter<'_,u8>>()+size_of::<&u8>()+size_of::<u64>()*3) as u64;
fn length_text(a:&str,work:&mut dyn ContractWork)->Result<u64>{
    pay(work,32)?;let mut n=2u64;
    for byte in a.as_bytes(){pay(work,16)?;let width=match byte{b'"'|b'\\'|8|9|10|12|13=>2,0..=31=>6,_=>1};n=n.checked_add(width).ok_or_else(cost::overflow)?;}
    pay(work,16)?;Ok(n)
}
const LENGTH_UNSIGNED_FRAME:u64=64+4*(size_of::<&u64>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()*2) as u64;
fn length_unsigned(a:&u64,work:&mut dyn ContractWork)->Result<u64>{
    pay(work,32)?;let mut value=*a;let mut n=1u64;
    loop{pay(work,16)?;if value<10{break}value/=10;n=n.checked_add(1).ok_or_else(cost::overflow)?;}
    Ok(n)
}
const LENGTH_USIZE_FRAME:u64=64+4*(size_of::<&usize>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()) as u64;
fn length_usize(a:&usize,work:&mut dyn ContractWork)->Result<u64>{pay(work,16)?;let n=u64::try_from(*a).map_err(|_|cost::overflow())?;invoke!(work,LENGTH_UNSIGNED_FRAME,length_unsigned(&n,work))}
const LENGTH_SIGNED_FRAME:u64=64+4*(size_of::<&i64>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()*2+size_of::<u64>()) as u64;
fn length_signed(a:&i64,work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;let magnitude=a.unsigned_abs();let n=invoke!(work,LENGTH_UNSIGNED_FRAME,length_unsigned(&magnitude,work))?;Ok(wire_add!(work,n,u64::from(*a<0)))}
const LENGTH_DATE_FRAME:u64=64+4*(size_of::<&i32>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<i64>()) as u64;
fn length_date(a:&i32,work:&mut dyn ContractWork)->Result<u64>{pay(work,16)?;let n=i64::from(*a);invoke!(work,LENGTH_SIGNED_FRAME,length_signed(&n,work))}
const COPY_TEXT_FRAME:u64=64+4*(size_of::<&str>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<String>>()+size_of::<AdapterError>()+size_of::<String>()+size_of::<u64>()) as u64;
fn copy_text(a:&str,work:&mut dyn ContractWork)->Result<String>{
    pay(work,32)?;let debit=capture_payment::string_copy_bytes(a.len()).map_err(cost::contract)?;pay(work,debit)?;Ok(a.to_owned())
}
const COMPARE_TEXT_FRAME:u64=64+4*(size_of::<&str>()*2+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<std::ops::Range<usize>>()+size_of::<usize>()+size_of::<KeyOrder>()) as u64;
fn compare_text(a:&str,b:&str,work:&mut dyn ContractWork)->Result<KeyOrder>{
    pay(work,32)?;for i in 0..a.len().min(b.len()){pay(work,16)?;let order=a.as_bytes()[i].cmp(&b.as_bytes()[i]);if order!=KeyOrder::Equal{return Ok(order)}}pay(work,16)?;Ok(a.len().cmp(&b.len()))
}

// Evaluated at compile time only; never a runtime graph operation.
const fn max_arm(arms:&[usize])->usize {let mut i=0;let mut n=0;while i<arms.len(){if arms[i]>n{n=arms[i]}i+=1;}n}

fn text_shape(s:&str,work:&mut dyn ContractWork)->Result<bool>{pay(work,3)?;Ok(s.len()<=MAX_TEXT_BYTES)}
fn type_shape(t:&Type,depth:usize,work:&mut dyn ContractWork)->Result<bool>{
    pay(work,8)?;
    if depth>MAX_TYPE_DEPTH{return Ok(false)}
    match t{
        Type::Reference{entity}=>text_shape(entity,work),
        Type::List{item}|Type::Optional{item}=>{pay(work,3)?;type_shape(item,depth+1,work)},
        _=>Ok(true)
    }
}
fn value_shape(v:&DataValue,depth:usize,work:&mut dyn ContractWork)->Result<bool>{
    pay(work,8)?;
    if depth>MAX_TYPE_DEPTH{return Ok(false)}
    match v{
        DataValue::Text{value}=>text_shape(value,work),
        DataValue::Reference{entity,record}=>Ok(text_shape(entity,work)?&&text_shape(record,work)?),
        DataValue::List{item_type,items}=>{
            pay(work,4)?;
            if items.len()>MAX_COLLECTION||!type_shape(item_type,0,work)?{return Ok(false)}
            pay(work,1)?;
            for item in items{pay(work,4)?;if !value_shape(item,depth+1,work)?{return Ok(false)}}
            pay(work,1)?;Ok(true)
        }
        _=>Ok(true)
    }
}
fn expr_shape(e:&Expr,depth:usize,nodes:&mut usize,work:&mut dyn ContractWork)->Result<bool>{
    pay(work,10)?;
    if depth>MAX_EXPR_DEPTH||*nodes>=MAX_AST_NODES{return Ok(false)}
    *nodes+=1;
    let mut child=|e:&Expr|{pay(work,3)?;expr_shape(e,depth+1,nodes,work)};
    match e{
        Expr::Literal{value_type,value}=>Ok(type_shape(value_type,0,work)?&&value_shape(value,0,work)?),
        Expr::Variable{name}=>text_shape(name,work),Expr::State{state}=>text_shape(state,work),Expr::Today=>Ok(true),
        Expr::Field{record,field}=>{if !child(record)?{return Ok(false)}text_shape(field,work)},
        Expr::Query{entity,binding,predicate,sort,..}=>{
            if !child(predicate)?{return Ok(false)}
            if !text_shape(entity,work)?||!text_shape(binding,work)?{return Ok(false)}
            pay(work,3)?;if sort.len()>MAX_ITEMS{return Ok(false)}
            for key in sort{pay(work,4)?;if !expr_shape(&key.value,depth+1,nodes,work)?{return Ok(false)}}
            pay(work,1)?;Ok(true)
        }
        Expr::Filter{items,binding,predicate}|Expr::Any{items,binding,predicate}|Expr::All{items,binding,predicate}=>{
            if !child(items)?||!child(predicate)?{return Ok(false)}text_shape(binding,work)
        }
        Expr::Map{items,binding,value}=>{if !child(items)?||!child(value)?{return Ok(false)}text_shape(binding,work)},
        Expr::Count{items}|Expr::Sum{items}=>child(items),
        Expr::Contains{items,value}=>Ok(child(items)?&&child(value)?),
        Expr::Equal{left,right}|Expr::Less{left,right}|Expr::Add{left,right}|Expr::Subtract{left,right}|Expr::Concat{left,right}=>Ok(child(left)?&&child(right)?),
        Expr::TextContains{text,search}=>Ok(child(text)?&&child(search)?),
        Expr::Lower{value}|Expr::Not{value}=>child(value),
        Expr::And{values}|Expr::Or{values}=>{
            pay(work,3)?;if values.len()>MAX_ITEMS{return Ok(false)}
            for value in values{pay(work,4)?;if !expr_shape(value,depth+1,nodes,work)?{return Ok(false)}}
            pay(work,1)?;Ok(true)
        }
        Expr::If{condition,then_value,else_value}=>Ok(child(condition)?&&child(then_value)?&&child(else_value)?),
        Expr::Coalesce{value,fallback}=>Ok(child(value)?&&child(fallback)?),
        Expr::DateAdd{date,days}=>Ok(child(date)?&&child(days)?),
        Expr::DateDifference{later,earlier}=>Ok(child(later)?&&child(earlier)?),
    }
}

// length Type: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const LENGTH_TYP_FRAME:u64=64+4*(size_of::<&Type>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0,0,0,0,0+size_of::<&String>(),0+size_of::<&Box<Type>>(),0+size_of::<&Box<Type>>()])+size_of::<u64>()) as u64;
fn length_typ(a:&Type,work:&mut dyn ContractWork)->Result<u64>{
    pay(work,32)?;
    match a {
        Type::Boolean=>{
            let mut n=18u64;
            Ok(n)
        },
        Type::Integer=>{
            let mut n=18u64;
            Ok(n)
        },
        Type::Text=>{
            let mut n=15u64;
            Ok(n)
        },
        Type::Date=>{
            let mut n=15u64;
            Ok(n)
        },
        Type::Reference{entity}=>{
            let mut n=30u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(entity,work))?);
            Ok(n)
        },
        Type::List{item}=>{
            let mut n=23u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TYP_FRAME, length_typ(item,work))?);
            Ok(n)
        },
        Type::Optional{item}=>{
            let mut n=27u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TYP_FRAME, length_typ(item,work))?);
            Ok(n)
        },
    }
}
// copy Type: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COPY_TYP_FRAME:u64=64+4*(size_of::<&Type>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<Type>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0,0,0,0,0+size_of::<&String>()+size_of::<String>()+size_of::<Result<String>>(),0+size_of::<&Box<Type>>()+size_of::<Box<Type>>()+size_of::<Result<Box<Type>>>(),0+size_of::<&Box<Type>>()+size_of::<Box<Type>>()+size_of::<Result<Box<Type>>>()])) as u64;
fn copy_typ(a:&Type,work:&mut dyn ContractWork)->Result<Type>{
    pay(work,64)?;
    match a {
        Type::Boolean=>{
            Ok(Type::Boolean)
        },
        Type::Integer=>{
            Ok(Type::Integer)
        },
        Type::Text=>{
            Ok(Type::Text)
        },
        Type::Date=>{
            Ok(Type::Date)
        },
        Type::Reference{entity}=>{
            pay(work,32)?;let entity=invoke!(work, COPY_TEXT_FRAME, copy_text(entity,work))?;
            Ok(Type::Reference{entity})
        },
        Type::List{item}=>{
            pay(work,32)?;let item=invoke!(work, COPY_BOX_TYP_FRAME, copy_box_typ(item,work))?;
            Ok(Type::List{item})
        },
        Type::Optional{item}=>{
            pay(work,32)?;let item=invoke!(work, COPY_BOX_TYP_FRAME, copy_box_typ(item,work))?;
            Ok(Type::Optional{item})
        },
    }
}
// compare Type: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COMPARE_TYP_FRAME:u64=64+4*(size_of::<&Type>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&Type>()+size_of::<u8>()+size_of::<u8>()+max_arm(&[0,0,0,0,0+size_of::<&String>()+size_of::<&String>(),0+size_of::<&Box<Type>>()+size_of::<&Box<Type>>(),0+size_of::<&Box<Type>>()+size_of::<&Box<Type>>()])) as u64;
fn compare_typ(a:&Type,b:&Type,work:&mut dyn ContractWork)->Result<KeyOrder>{
    pay(work,32)?;
    match a {
        Type::Boolean=>{
            let Type::Boolean=b else{return Ok((0u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        Type::Integer=>{
            let Type::Integer=b else{return Ok((1u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        Type::Text=>{
            let Type::Text=b else{return Ok((2u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        Type::Date=>{
            let Type::Date=b else{return Ok((3u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        Type::Reference{entity}=>{
            let Type::Reference{entity:b_entity}=b else{return Ok((4u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(entity,b_entity,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Type::List{item}=>{
            let Type::List{item:b_item}=b else{return Ok((5u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TYP_FRAME, compare_typ(item,b_item,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Type::Optional{item}=>{
            let Type::Optional{item:b_item}=b else{return Ok((6u8).cmp(&invoke!(work,RANK_TYP_FRAME,rank_typ(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TYP_FRAME, compare_typ(item,b_item,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
    }
}
const RANK_TYP_FRAME:u64=64+4*(size_of::<&Type>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u8>>()+size_of::<AdapterError>()) as u64;
fn rank_typ(a:&Type,work:&mut dyn ContractWork)->Result<u8>{pay(work,32)?;Ok(match a{
    Type::Boolean=>0,
    Type::Integer=>1,
    Type::Text=>2,
    Type::Date=>3,
    Type::Reference{entity:_}=>4,
    Type::List{item:_}=>5,
    Type::Optional{item:_}=>6,
})}
// length DataValue: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const LENGTH_VALUE_FRAME:u64=64+4*(size_of::<&DataValue>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0,0+size_of::<&bool>(),0+size_of::<&i64>(),0+size_of::<&String>(),0+size_of::<&i32>(),0+size_of::<&String>()+size_of::<&String>(),0+size_of::<&Type>()+size_of::<&Vec<DataValue>>()])+size_of::<u64>()) as u64;
fn length_value(a:&DataValue,work:&mut dyn ContractWork)->Result<u64>{
    pay(work,32)?;
    match a {
        DataValue::Null=>{
            let mut n=15u64;
            Ok(n)
        },
        DataValue::Boolean{value}=>{
            let mut n=27u64;
            pay(work,32)?;n=wire_add!(work,n,if *value {4} else {5});
            Ok(n)
        },
        DataValue::Integer{value}=>{
            let mut n=27u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_SIGNED_FRAME, length_signed(value,work))?);
            Ok(n)
        },
        DataValue::Text{value}=>{
            let mut n=24u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(value,work))?);
            Ok(n)
        },
        DataValue::Date{days}=>{
            let mut n=23u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_DATE_FRAME, length_date(days,work))?);
            Ok(n)
        },
        DataValue::Reference{entity,record}=>{
            let mut n=40u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(entity,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(record,work))?);
            Ok(n)
        },
        DataValue::List{item_type,items}=>{
            let mut n=37u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TYP_FRAME, length_typ(item_type,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_VEC_VALUE_FRAME, length_vec_value(items,work))?);
            Ok(n)
        },
    }
}
// copy DataValue: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COPY_VALUE_FRAME:u64=64+4*(size_of::<&DataValue>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<DataValue>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0,0+size_of::<&bool>()+size_of::<bool>()+size_of::<Result<bool>>(),0+size_of::<&i64>()+size_of::<i64>()+size_of::<Result<i64>>(),0+size_of::<&String>()+size_of::<String>()+size_of::<Result<String>>(),0+size_of::<&i32>()+size_of::<i32>()+size_of::<Result<i32>>(),0+size_of::<&String>()+size_of::<&String>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<String>()+size_of::<Result<String>>(),0+size_of::<&Type>()+size_of::<&Vec<DataValue>>()+size_of::<Type>()+size_of::<Result<Type>>()+size_of::<Vec<DataValue>>()+size_of::<Result<Vec<DataValue>>>()])) as u64;
fn copy_value(a:&DataValue,work:&mut dyn ContractWork)->Result<DataValue>{
    pay(work,64)?;
    match a {
        DataValue::Null=>{
            Ok(DataValue::Null)
        },
        DataValue::Boolean{value}=>{
            pay(work,32)?;let value=*value;
            Ok(DataValue::Boolean{value})
        },
        DataValue::Integer{value}=>{
            pay(work,32)?;let value=*value;
            Ok(DataValue::Integer{value})
        },
        DataValue::Text{value}=>{
            pay(work,32)?;let value=invoke!(work, COPY_TEXT_FRAME, copy_text(value,work))?;
            Ok(DataValue::Text{value})
        },
        DataValue::Date{days}=>{
            pay(work,32)?;let days=*days;
            Ok(DataValue::Date{days})
        },
        DataValue::Reference{entity,record}=>{
            pay(work,32)?;let entity=invoke!(work, COPY_TEXT_FRAME, copy_text(entity,work))?;
            pay(work,32)?;let record=invoke!(work, COPY_TEXT_FRAME, copy_text(record,work))?;
            Ok(DataValue::Reference{entity,record})
        },
        DataValue::List{item_type,items}=>{
            pay(work,32)?;let item_type=invoke!(work, COPY_TYP_FRAME, copy_typ(item_type,work))?;
            pay(work,32)?;let items=invoke!(work, COPY_VEC_VALUE_FRAME, copy_vec_value(items,work))?;
            Ok(DataValue::List{item_type,items})
        },
    }
}
// compare DataValue: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COMPARE_VALUE_FRAME:u64=64+4*(size_of::<&DataValue>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&DataValue>()+size_of::<u8>()+size_of::<u8>()+max_arm(&[0,0+size_of::<&bool>()+size_of::<&bool>(),0+size_of::<&i64>()+size_of::<&i64>(),0+size_of::<&String>()+size_of::<&String>(),0+size_of::<&i32>()+size_of::<&i32>(),0+size_of::<&String>()+size_of::<&String>()+size_of::<&String>()+size_of::<&String>(),0+size_of::<&Type>()+size_of::<&Vec<DataValue>>()+size_of::<&Type>()+size_of::<&Vec<DataValue>>()])) as u64;
fn compare_value(a:&DataValue,b:&DataValue,work:&mut dyn ContractWork)->Result<KeyOrder>{
    pay(work,32)?;
    match a {
        DataValue::Null=>{
            let DataValue::Null=b else{return Ok((0u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        DataValue::Boolean{value}=>{
            let DataValue::Boolean{value:b_value}=b else{return Ok((1u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=value.cmp(b_value);if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        DataValue::Integer{value}=>{
            let DataValue::Integer{value:b_value}=b else{return Ok((2u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=value.cmp(b_value);if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        DataValue::Text{value}=>{
            let DataValue::Text{value:b_value}=b else{return Ok((3u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        DataValue::Date{days}=>{
            let DataValue::Date{days:b_days}=b else{return Ok((4u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=days.cmp(b_days);if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        DataValue::Reference{entity,record}=>{
            let DataValue::Reference{entity:b_entity,record:b_record}=b else{return Ok((5u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(entity,b_entity,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(record,b_record,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        DataValue::List{item_type,items}=>{
            let DataValue::List{item_type:b_item_type,items:b_items}=b else{return Ok((6u8).cmp(&invoke!(work,RANK_VALUE_FRAME,rank_value(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TYP_FRAME, compare_typ(item_type,b_item_type,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_VEC_VALUE_FRAME, compare_vec_value(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
    }
}
const RANK_VALUE_FRAME:u64=64+4*(size_of::<&DataValue>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u8>>()+size_of::<AdapterError>()) as u64;
fn rank_value(a:&DataValue,work:&mut dyn ContractWork)->Result<u8>{pay(work,32)?;Ok(match a{
    DataValue::Null=>0,
    DataValue::Boolean{value:_}=>1,
    DataValue::Integer{value:_}=>2,
    DataValue::Text{value:_}=>3,
    DataValue::Date{days:_}=>4,
    DataValue::Reference{entity:_,record:_}=>5,
    DataValue::List{item_type:_,items:_}=>6,
})}
// length Expr: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const LENGTH_EXPR_FRAME:u64=64+4*(size_of::<&Expr>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0+size_of::<&Type>()+size_of::<&DataValue>(),0+size_of::<&String>(),0+size_of::<&Box<Expr>>()+size_of::<&String>(),0+size_of::<&String>(),0,0+size_of::<&String>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Vec<SortKey>>()+size_of::<&usize>()+size_of::<&bool>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>(),0+size_of::<&Vec<Expr>>(),0+size_of::<&Vec<Expr>>(),0+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()])+size_of::<u64>()) as u64;
fn length_expr(a:&Expr,work:&mut dyn ContractWork)->Result<u64>{
    pay(work,32)?;
    match a {
        Expr::Literal{value_type,value}=>{
            let mut n=41u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TYP_FRAME, length_typ(value_type,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_VALUE_FRAME, length_value(value,work))?);
            Ok(n)
        },
        Expr::Variable{name}=>{
            let mut n=27u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(name,work))?);
            Ok(n)
        },
        Expr::Field{record,field}=>{
            let mut n=35u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(record,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(field,work))?);
            Ok(n)
        },
        Expr::State{state}=>{
            let mut n=25u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(state,work))?);
            Ok(n)
        },
        Expr::Today=>{
            let mut n=16u64;
            Ok(n)
        },
        Expr::Query{entity,binding,predicate,sort,limit,include_archived}=>{
            let mut n=87u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(entity,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(binding,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(predicate,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_VEC_SORT_FRAME, length_vec_sort(sort,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_USIZE_FRAME, length_usize(limit,work))?);
            pay(work,32)?;n=wire_add!(work,n,if *include_archived {4} else {5});
            Ok(n)
        },
        Expr::Filter{items,binding,predicate}=>{
            let mut n=50u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(binding,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(predicate,work))?);
            Ok(n)
        },
        Expr::Map{items,binding,value}=>{
            let mut n=43u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(binding,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
            Ok(n)
        },
        Expr::Count{items}=>{
            let mut n=25u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            Ok(n)
        },
        Expr::Any{items,binding,predicate}=>{
            let mut n=47u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(binding,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(predicate,work))?);
            Ok(n)
        },
        Expr::All{items,binding,predicate}=>{
            let mut n=47u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_TEXT_FRAME, length_text(binding,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(predicate,work))?);
            Ok(n)
        },
        Expr::Sum{items}=>{
            let mut n=23u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            Ok(n)
        },
        Expr::Contains{items,value}=>{
            let mut n=37u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(items,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
            Ok(n)
        },
        Expr::Equal{left,right}=>{
            let mut n=33u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(left,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(right,work))?);
            Ok(n)
        },
        Expr::Less{left,right}=>{
            let mut n=32u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(left,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(right,work))?);
            Ok(n)
        },
        Expr::Add{left,right}=>{
            let mut n=31u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(left,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(right,work))?);
            Ok(n)
        },
        Expr::Subtract{left,right}=>{
            let mut n=36u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(left,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(right,work))?);
            Ok(n)
        },
        Expr::Concat{left,right}=>{
            let mut n=34u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(left,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(right,work))?);
            Ok(n)
        },
        Expr::TextContains{text,search}=>{
            let mut n=42u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(text,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(search,work))?);
            Ok(n)
        },
        Expr::Lower{value}=>{
            let mut n=25u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
            Ok(n)
        },
        Expr::And{values}=>{
            let mut n=24u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_VEC_EXPR_FRAME, length_vec_expr(values,work))?);
            Ok(n)
        },
        Expr::Or{values}=>{
            let mut n=23u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_VEC_EXPR_FRAME, length_vec_expr(values,work))?);
            Ok(n)
        },
        Expr::Not{value}=>{
            let mut n=23u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
            Ok(n)
        },
        Expr::If{condition,then_value,else_value}=>{
            let mut n=54u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(condition,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(then_value,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(else_value,work))?);
            Ok(n)
        },
        Expr::Coalesce{value,fallback}=>{
            let mut n=40u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(fallback,work))?);
            Ok(n)
        },
        Expr::DateAdd{date,days}=>{
            let mut n=35u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(date,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(days,work))?);
            Ok(n)
        },
        Expr::DateDifference{later,earlier}=>{
            let mut n=46u64;
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(later,work))?);
            pay(work,32)?;n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(earlier,work))?);
            Ok(n)
        },
    }
}
// copy Expr: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COPY_EXPR_FRAME:u64=64+4*(size_of::<&Expr>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<Expr>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+max_arm(&[0+size_of::<&Type>()+size_of::<&DataValue>()+size_of::<Type>()+size_of::<Result<Type>>()+size_of::<DataValue>()+size_of::<Result<DataValue>>(),0+size_of::<&String>()+size_of::<String>()+size_of::<Result<String>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<String>()+size_of::<Result<String>>(),0+size_of::<&String>()+size_of::<String>()+size_of::<Result<String>>(),0,0+size_of::<&String>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Vec<SortKey>>()+size_of::<&usize>()+size_of::<&bool>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Vec<SortKey>>()+size_of::<Result<Vec<SortKey>>>()+size_of::<usize>()+size_of::<Result<usize>>()+size_of::<bool>()+size_of::<Result<bool>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<String>()+size_of::<Result<String>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Vec<Expr>>()+size_of::<Vec<Expr>>()+size_of::<Result<Vec<Expr>>>(),0+size_of::<&Vec<Expr>>()+size_of::<Vec<Expr>>()+size_of::<Result<Vec<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()])) as u64;
fn copy_expr(a:&Expr,work:&mut dyn ContractWork)->Result<Expr>{
    pay(work,64)?;
    match a {
        Expr::Literal{value_type,value}=>{
            pay(work,32)?;let value_type=invoke!(work, COPY_TYP_FRAME, copy_typ(value_type,work))?;
            pay(work,32)?;let value=invoke!(work, COPY_VALUE_FRAME, copy_value(value,work))?;
            Ok(Expr::Literal{value_type,value})
        },
        Expr::Variable{name}=>{
            pay(work,32)?;let name=invoke!(work, COPY_TEXT_FRAME, copy_text(name,work))?;
            Ok(Expr::Variable{name})
        },
        Expr::Field{record,field}=>{
            pay(work,32)?;let record=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(record,work))?;
            pay(work,32)?;let field=invoke!(work, COPY_TEXT_FRAME, copy_text(field,work))?;
            Ok(Expr::Field{record,field})
        },
        Expr::State{state}=>{
            pay(work,32)?;let state=invoke!(work, COPY_TEXT_FRAME, copy_text(state,work))?;
            Ok(Expr::State{state})
        },
        Expr::Today=>{
            Ok(Expr::Today)
        },
        Expr::Query{entity,binding,predicate,sort,limit,include_archived}=>{
            pay(work,32)?;let entity=invoke!(work, COPY_TEXT_FRAME, copy_text(entity,work))?;
            pay(work,32)?;let binding=invoke!(work, COPY_TEXT_FRAME, copy_text(binding,work))?;
            pay(work,32)?;let predicate=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(predicate,work))?;
            pay(work,32)?;let sort=invoke!(work, COPY_VEC_SORT_FRAME, copy_vec_sort(sort,work))?;
            pay(work,32)?;let limit=*limit;
            pay(work,32)?;let include_archived=*include_archived;
            Ok(Expr::Query{entity,binding,predicate,sort,limit,include_archived})
        },
        Expr::Filter{items,binding,predicate}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            pay(work,32)?;let binding=invoke!(work, COPY_TEXT_FRAME, copy_text(binding,work))?;
            pay(work,32)?;let predicate=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(predicate,work))?;
            Ok(Expr::Filter{items,binding,predicate})
        },
        Expr::Map{items,binding,value}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            pay(work,32)?;let binding=invoke!(work, COPY_TEXT_FRAME, copy_text(binding,work))?;
            pay(work,32)?;let value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(value,work))?;
            Ok(Expr::Map{items,binding,value})
        },
        Expr::Count{items}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            Ok(Expr::Count{items})
        },
        Expr::Any{items,binding,predicate}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            pay(work,32)?;let binding=invoke!(work, COPY_TEXT_FRAME, copy_text(binding,work))?;
            pay(work,32)?;let predicate=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(predicate,work))?;
            Ok(Expr::Any{items,binding,predicate})
        },
        Expr::All{items,binding,predicate}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            pay(work,32)?;let binding=invoke!(work, COPY_TEXT_FRAME, copy_text(binding,work))?;
            pay(work,32)?;let predicate=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(predicate,work))?;
            Ok(Expr::All{items,binding,predicate})
        },
        Expr::Sum{items}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            Ok(Expr::Sum{items})
        },
        Expr::Contains{items,value}=>{
            pay(work,32)?;let items=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(items,work))?;
            pay(work,32)?;let value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(value,work))?;
            Ok(Expr::Contains{items,value})
        },
        Expr::Equal{left,right}=>{
            pay(work,32)?;let left=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(left,work))?;
            pay(work,32)?;let right=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(right,work))?;
            Ok(Expr::Equal{left,right})
        },
        Expr::Less{left,right}=>{
            pay(work,32)?;let left=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(left,work))?;
            pay(work,32)?;let right=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(right,work))?;
            Ok(Expr::Less{left,right})
        },
        Expr::Add{left,right}=>{
            pay(work,32)?;let left=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(left,work))?;
            pay(work,32)?;let right=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(right,work))?;
            Ok(Expr::Add{left,right})
        },
        Expr::Subtract{left,right}=>{
            pay(work,32)?;let left=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(left,work))?;
            pay(work,32)?;let right=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(right,work))?;
            Ok(Expr::Subtract{left,right})
        },
        Expr::Concat{left,right}=>{
            pay(work,32)?;let left=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(left,work))?;
            pay(work,32)?;let right=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(right,work))?;
            Ok(Expr::Concat{left,right})
        },
        Expr::TextContains{text,search}=>{
            pay(work,32)?;let text=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(text,work))?;
            pay(work,32)?;let search=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(search,work))?;
            Ok(Expr::TextContains{text,search})
        },
        Expr::Lower{value}=>{
            pay(work,32)?;let value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(value,work))?;
            Ok(Expr::Lower{value})
        },
        Expr::And{values}=>{
            pay(work,32)?;let values=invoke!(work, COPY_VEC_EXPR_FRAME, copy_vec_expr(values,work))?;
            Ok(Expr::And{values})
        },
        Expr::Or{values}=>{
            pay(work,32)?;let values=invoke!(work, COPY_VEC_EXPR_FRAME, copy_vec_expr(values,work))?;
            Ok(Expr::Or{values})
        },
        Expr::Not{value}=>{
            pay(work,32)?;let value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(value,work))?;
            Ok(Expr::Not{value})
        },
        Expr::If{condition,then_value,else_value}=>{
            pay(work,32)?;let condition=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(condition,work))?;
            pay(work,32)?;let then_value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(then_value,work))?;
            pay(work,32)?;let else_value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(else_value,work))?;
            Ok(Expr::If{condition,then_value,else_value})
        },
        Expr::Coalesce{value,fallback}=>{
            pay(work,32)?;let value=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(value,work))?;
            pay(work,32)?;let fallback=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(fallback,work))?;
            Ok(Expr::Coalesce{value,fallback})
        },
        Expr::DateAdd{date,days}=>{
            pay(work,32)?;let date=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(date,work))?;
            pay(work,32)?;let days=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(days,work))?;
            Ok(Expr::DateAdd{date,days})
        },
        Expr::DateDifference{later,earlier}=>{
            pay(work,32)?;let later=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(later,work))?;
            pay(work,32)?;let earlier=invoke!(work, COPY_BOX_EXPR_FRAME, copy_box_expr(earlier,work))?;
            Ok(Expr::DateDifference{later,earlier})
        },
    }
}
// compare Expr: arguments, return/error, accumulator/order, and maximum complete variant-field arm.
const COMPARE_EXPR_FRAME:u64=64+4*(size_of::<&Expr>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&Expr>()+size_of::<u8>()+size_of::<u8>()+max_arm(&[0+size_of::<&Type>()+size_of::<&DataValue>()+size_of::<&Type>()+size_of::<&DataValue>(),0+size_of::<&String>()+size_of::<&String>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&String>(),0+size_of::<&String>()+size_of::<&String>(),0,0+size_of::<&String>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Vec<SortKey>>()+size_of::<&usize>()+size_of::<&bool>()+size_of::<&String>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Vec<SortKey>>()+size_of::<&usize>()+size_of::<&bool>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&String>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Vec<Expr>>()+size_of::<&Vec<Expr>>(),0+size_of::<&Vec<Expr>>()+size_of::<&Vec<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>(),0+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()+size_of::<&Box<Expr>>()])) as u64;
fn compare_expr(a:&Expr,b:&Expr,work:&mut dyn ContractWork)->Result<KeyOrder>{
    pay(work,32)?;
    match a {
        Expr::Literal{value_type,value}=>{
            let Expr::Literal{value_type:b_value_type,value:b_value}=b else{return Ok((0u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TYP_FRAME, compare_typ(value_type,b_value_type,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_VALUE_FRAME, compare_value(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Variable{name}=>{
            let Expr::Variable{name:b_name}=b else{return Ok((1u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(name,b_name,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Field{record,field}=>{
            let Expr::Field{record:b_record,field:b_field}=b else{return Ok((2u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(record,b_record,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(field,b_field,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::State{state}=>{
            let Expr::State{state:b_state}=b else{return Ok((3u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(state,b_state,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Today=>{
            let Expr::Today=b else{return Ok((4u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            Ok(KeyOrder::Equal)
        },
        Expr::Query{entity,binding,predicate,sort,limit,include_archived}=>{
            let Expr::Query{entity:b_entity,binding:b_binding,predicate:b_predicate,sort:b_sort,limit:b_limit,include_archived:b_include_archived}=b else{return Ok((5u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(entity,b_entity,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(binding,b_binding,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(predicate,b_predicate,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_VEC_SORT_FRAME, compare_vec_sort(sort,b_sort,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=limit.cmp(b_limit);if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=include_archived.cmp(b_include_archived);if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Filter{items,binding,predicate}=>{
            let Expr::Filter{items:b_items,binding:b_binding,predicate:b_predicate}=b else{return Ok((6u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(binding,b_binding,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(predicate,b_predicate,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Map{items,binding,value}=>{
            let Expr::Map{items:b_items,binding:b_binding,value:b_value}=b else{return Ok((7u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(binding,b_binding,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Count{items}=>{
            let Expr::Count{items:b_items}=b else{return Ok((8u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Any{items,binding,predicate}=>{
            let Expr::Any{items:b_items,binding:b_binding,predicate:b_predicate}=b else{return Ok((9u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(binding,b_binding,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(predicate,b_predicate,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::All{items,binding,predicate}=>{
            let Expr::All{items:b_items,binding:b_binding,predicate:b_predicate}=b else{return Ok((10u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_TEXT_FRAME, compare_text(binding,b_binding,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(predicate,b_predicate,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Sum{items}=>{
            let Expr::Sum{items:b_items}=b else{return Ok((11u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Contains{items,value}=>{
            let Expr::Contains{items:b_items,value:b_value}=b else{return Ok((12u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(items,b_items,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Equal{left,right}=>{
            let Expr::Equal{left:b_left,right:b_right}=b else{return Ok((13u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(left,b_left,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(right,b_right,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Less{left,right}=>{
            let Expr::Less{left:b_left,right:b_right}=b else{return Ok((14u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(left,b_left,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(right,b_right,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Add{left,right}=>{
            let Expr::Add{left:b_left,right:b_right}=b else{return Ok((15u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(left,b_left,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(right,b_right,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Subtract{left,right}=>{
            let Expr::Subtract{left:b_left,right:b_right}=b else{return Ok((16u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(left,b_left,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(right,b_right,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Concat{left,right}=>{
            let Expr::Concat{left:b_left,right:b_right}=b else{return Ok((17u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(left,b_left,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(right,b_right,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::TextContains{text,search}=>{
            let Expr::TextContains{text:b_text,search:b_search}=b else{return Ok((18u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(text,b_text,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(search,b_search,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Lower{value}=>{
            let Expr::Lower{value:b_value}=b else{return Ok((19u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::And{values}=>{
            let Expr::And{values:b_values}=b else{return Ok((20u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_VEC_EXPR_FRAME, compare_vec_expr(values,b_values,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Or{values}=>{
            let Expr::Or{values:b_values}=b else{return Ok((21u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_VEC_EXPR_FRAME, compare_vec_expr(values,b_values,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Not{value}=>{
            let Expr::Not{value:b_value}=b else{return Ok((22u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::If{condition,then_value,else_value}=>{
            let Expr::If{condition:b_condition,then_value:b_then_value,else_value:b_else_value}=b else{return Ok((23u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(condition,b_condition,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(then_value,b_then_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(else_value,b_else_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::Coalesce{value,fallback}=>{
            let Expr::Coalesce{value:b_value,fallback:b_fallback}=b else{return Ok((24u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(fallback,b_fallback,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::DateAdd{date,days}=>{
            let Expr::DateAdd{date:b_date,days:b_days}=b else{return Ok((25u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(date,b_date,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(days,b_days,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
        Expr::DateDifference{later,earlier}=>{
            let Expr::DateDifference{later:b_later,earlier:b_earlier}=b else{return Ok((26u8).cmp(&invoke!(work,RANK_EXPR_FRAME,rank_expr(b,work))?))};
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(later,b_later,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            pay(work,32)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(earlier,b_earlier,work))?;if order!=KeyOrder::Equal{return Ok(order)}
            Ok(KeyOrder::Equal)
        },
    }
}
const RANK_EXPR_FRAME:u64=64+4*(size_of::<&Expr>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u8>>()+size_of::<AdapterError>()) as u64;
fn rank_expr(a:&Expr,work:&mut dyn ContractWork)->Result<u8>{pay(work,32)?;Ok(match a{
    Expr::Literal{value_type:_,value:_}=>0,
    Expr::Variable{name:_}=>1,
    Expr::Field{record:_,field:_}=>2,
    Expr::State{state:_}=>3,
    Expr::Today=>4,
    Expr::Query{entity:_,binding:_,predicate:_,sort:_,limit:_,include_archived:_}=>5,
    Expr::Filter{items:_,binding:_,predicate:_}=>6,
    Expr::Map{items:_,binding:_,value:_}=>7,
    Expr::Count{items:_}=>8,
    Expr::Any{items:_,binding:_,predicate:_}=>9,
    Expr::All{items:_,binding:_,predicate:_}=>10,
    Expr::Sum{items:_}=>11,
    Expr::Contains{items:_,value:_}=>12,
    Expr::Equal{left:_,right:_}=>13,
    Expr::Less{left:_,right:_}=>14,
    Expr::Add{left:_,right:_}=>15,
    Expr::Subtract{left:_,right:_}=>16,
    Expr::Concat{left:_,right:_}=>17,
    Expr::TextContains{text:_,search:_}=>18,
    Expr::Lower{value:_}=>19,
    Expr::And{values:_}=>20,
    Expr::Or{values:_}=>21,
    Expr::Not{value:_}=>22,
    Expr::If{condition:_,then_value:_,else_value:_}=>23,
    Expr::Coalesce{value:_,fallback:_}=>24,
    Expr::DateAdd{date:_,days:_}=>25,
    Expr::DateDifference{later:_,earlier:_}=>26,
})}
const LENGTH_SORT_FRAME:u64=64+4*(size_of::<&SortKey>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&Expr>()+size_of::<&bool>()+size_of::<u64>()) as u64;
fn length_sort(a:&SortKey,work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;
let SortKey{value,descending}=a;
let mut n=24u64;
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(value,work))?);
pay(work,32)?;
n=wire_add!(work,n,if *descending {4} else {5});
Ok(n)}
const COPY_SORT_FRAME:u64=64+4*(size_of::<&SortKey>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<SortKey>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&Expr>()+size_of::<&bool>()+size_of::<Expr>()+size_of::<Result<Expr>>()+size_of::<bool>()+size_of::<Result<bool>>()) as u64;
fn copy_sort(a:&SortKey,work:&mut dyn ContractWork)->Result<SortKey>{pay(work,32)?;
let SortKey{value,descending}=a;
pay(work,32)?;
let value=invoke!(work, COPY_EXPR_FRAME, copy_expr(value,work))?;
pay(work,32)?;
let descending=*descending;
Ok(SortKey{value,descending})}
const COMPARE_SORT_FRAME:u64=64+4*(size_of::<&SortKey>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&SortKey>()+size_of::<&Expr>()+size_of::<&bool>()+size_of::<&Expr>()+size_of::<&bool>()) as u64;
fn compare_sort(a:&SortKey,b:&SortKey,work:&mut dyn ContractWork)->Result<KeyOrder>{pay(work,32)?;
let SortKey{value,descending}=a;
let SortKey{value:b_value,descending:b_descending}=b;
pay(work,32)?;
let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(value,b_value,work))?;if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=descending.cmp(b_descending);if order!=KeyOrder::Equal{return Ok(order)}
Ok(KeyOrder::Equal)}
const LENGTH_LIMITS_FRAME:u64=64+4*(size_of::<&RuntimeLimits>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&u64>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&u64>()+size_of::<u64>()) as u64;
fn length_limits(a:&RuntimeLimits,work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;
let RuntimeLimits{fuel,collection_items,action_steps,transaction_writes,output_bytes,elapsed_millis}=a;
let mut n=101u64;
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_UNSIGNED_FRAME, length_unsigned(fuel,work))?);
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_USIZE_FRAME, length_usize(collection_items,work))?);
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_USIZE_FRAME, length_usize(action_steps,work))?);
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_USIZE_FRAME, length_usize(transaction_writes,work))?);
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_USIZE_FRAME, length_usize(output_bytes,work))?);
pay(work,32)?;
n=wire_add!(work,n,invoke!(work, LENGTH_UNSIGNED_FRAME, length_unsigned(elapsed_millis,work))?);
Ok(n)}
const COPY_LIMITS_FRAME:u64=64+4*(size_of::<&RuntimeLimits>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<RuntimeLimits>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&u64>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&u64>()+size_of::<u64>()+size_of::<Result<u64>>()+size_of::<usize>()+size_of::<Result<usize>>()+size_of::<usize>()+size_of::<Result<usize>>()+size_of::<usize>()+size_of::<Result<usize>>()+size_of::<usize>()+size_of::<Result<usize>>()+size_of::<u64>()+size_of::<Result<u64>>()) as u64;
fn copy_limits(a:&RuntimeLimits,work:&mut dyn ContractWork)->Result<RuntimeLimits>{pay(work,32)?;
let RuntimeLimits{fuel,collection_items,action_steps,transaction_writes,output_bytes,elapsed_millis}=a;
pay(work,32)?;
let fuel=*fuel;
pay(work,32)?;
let collection_items=*collection_items;
pay(work,32)?;
let action_steps=*action_steps;
pay(work,32)?;
let transaction_writes=*transaction_writes;
pay(work,32)?;
let output_bytes=*output_bytes;
pay(work,32)?;
let elapsed_millis=*elapsed_millis;
Ok(RuntimeLimits{fuel,collection_items,action_steps,transaction_writes,output_bytes,elapsed_millis})}
const COMPARE_LIMITS_FRAME:u64=64+4*(size_of::<&RuntimeLimits>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<KeyOrder>()+size_of::<&RuntimeLimits>()+size_of::<&u64>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&u64>()+size_of::<&u64>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&usize>()+size_of::<&u64>()) as u64;
fn compare_limits(a:&RuntimeLimits,b:&RuntimeLimits,work:&mut dyn ContractWork)->Result<KeyOrder>{pay(work,32)?;
let RuntimeLimits{fuel,collection_items,action_steps,transaction_writes,output_bytes,elapsed_millis}=a;
let RuntimeLimits{fuel:b_fuel,collection_items:b_collection_items,action_steps:b_action_steps,transaction_writes:b_transaction_writes,output_bytes:b_output_bytes,elapsed_millis:b_elapsed_millis}=b;
pay(work,32)?;
let order=fuel.cmp(b_fuel);if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=collection_items.cmp(b_collection_items);if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=action_steps.cmp(b_action_steps);if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=transaction_writes.cmp(b_transaction_writes);if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=output_bytes.cmp(b_output_bytes);if order!=KeyOrder::Equal{return Ok(order)}
pay(work,32)?;
let order=elapsed_millis.cmp(b_elapsed_millis);if order!=KeyOrder::Equal{return Ok(order)}
Ok(KeyOrder::Equal)}
const COPY_BOX_TYP_FRAME:u64=64+4*(size_of::<&Type>()+size_of::<&mut dyn ContractWork>()+size_of::<Type>()+size_of::<Result<Type>>()+size_of::<Box<Type>>()+size_of::<Result<Box<Type>>>()+size_of::<AdapterError>()) as u64;
fn copy_box_typ(a:&Type,work:&mut dyn ContractWork)->Result<Box<Type>>{
 pay(work,64+4*size_of::<Type>() as u64)?;
 let value=invoke!(work, COPY_TYP_FRAME, copy_typ(a,work))?;Ok(Box::new(value))
}
const COPY_BOX_EXPR_FRAME:u64=64+4*(size_of::<&Expr>()+size_of::<&mut dyn ContractWork>()+size_of::<Expr>()+size_of::<Result<Expr>>()+size_of::<Box<Expr>>()+size_of::<Result<Box<Expr>>>()+size_of::<AdapterError>()) as u64;
fn copy_box_expr(a:&Expr,work:&mut dyn ContractWork)->Result<Box<Expr>>{
 pay(work,64+4*size_of::<Expr>() as u64)?;
 let value=invoke!(work, COPY_EXPR_FRAME, copy_expr(a,work))?;Ok(Box::new(value))
}
const LENGTH_VEC_EXPR_FRAME:u64=64+4*(size_of::<&[Expr]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::iter::Enumerate<std::slice::Iter<'_, Expr>>>()+size_of::<&Expr>()+size_of::<u64>()) as u64;
fn length_vec_expr(a:&[Expr],work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;
let mut n=2u64;for (i,item) in a.iter().enumerate(){pay(work,16)?;if i!=0{n=wire_add!(work,n,1);}
n=wire_add!(work,n,invoke!(work, LENGTH_EXPR_FRAME, length_expr(item,work))?);}pay(work,16)?;Ok(n)}
const COPY_VEC_EXPR_FRAME:u64=64+4*(size_of::<&[Expr]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<Vec<Expr>>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, Expr>>()+size_of::<&Expr>()+size_of::<Vec<Expr>>()+size_of::<Expr>()+size_of::<Result<Expr>>()+size_of::<u64>()+size_of::<u64>()) as u64;
fn copy_vec_expr(a:&[Expr],work:&mut dyn ContractWork)->Result<Vec<Expr>>{pay(work,32)?;
pay(work,32)?;
let count=u64::try_from(a.len()).map_err(|_|cost::overflow())?;
let bytes=count.checked_mul(size_of::<Expr>() as u64).ok_or_else(cost::overflow)?;
let debit=bytes.checked_mul(4).and_then(|n|n.checked_add(64)).ok_or_else(cost::overflow)?;
pay(work,debit)?;let mut values=Vec::with_capacity(a.len());
for item in a{pay(work,16)?;let value=invoke!(work, COPY_EXPR_FRAME, copy_expr(item,work))?;values.push(value);}
pay(work,16)?;Ok(values)}
const COMPARE_VEC_EXPR_FRAME:u64=64+4*(size_of::<&[Expr]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, Expr>>()+size_of::<&Expr>()+size_of::<&[Expr]>()+size_of::<&Expr>()+size_of::<KeyOrder>()+size_of::<std::ops::Range<usize>>()) as u64;
fn compare_vec_expr(a:&[Expr],b:&[Expr],work:&mut dyn ContractWork)->Result<KeyOrder>{pay(work,32)?;
for i in 0..a.len().min(b.len()){pay(work,16)?;let order=invoke!(work, COMPARE_EXPR_FRAME, compare_expr(&a[i],&b[i],work))?;if order!=KeyOrder::Equal{return Ok(order)}}pay(work,16)?;Ok(a.len().cmp(&b.len()))}
const LENGTH_VEC_VALUE_FRAME:u64=64+4*(size_of::<&[DataValue]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::iter::Enumerate<std::slice::Iter<'_, DataValue>>>()+size_of::<&DataValue>()+size_of::<u64>()) as u64;
fn length_vec_value(a:&[DataValue],work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;
let mut n=2u64;for (i,item) in a.iter().enumerate(){pay(work,16)?;if i!=0{n=wire_add!(work,n,1);}
n=wire_add!(work,n,invoke!(work, LENGTH_VALUE_FRAME, length_value(item,work))?);}pay(work,16)?;Ok(n)}
const COPY_VEC_VALUE_FRAME:u64=64+4*(size_of::<&[DataValue]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<Vec<DataValue>>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, DataValue>>()+size_of::<&DataValue>()+size_of::<Vec<DataValue>>()+size_of::<DataValue>()+size_of::<Result<DataValue>>()+size_of::<u64>()+size_of::<u64>()) as u64;
fn copy_vec_value(a:&[DataValue],work:&mut dyn ContractWork)->Result<Vec<DataValue>>{pay(work,32)?;
pay(work,32)?;
let count=u64::try_from(a.len()).map_err(|_|cost::overflow())?;
let bytes=count.checked_mul(size_of::<DataValue>() as u64).ok_or_else(cost::overflow)?;
let debit=bytes.checked_mul(4).and_then(|n|n.checked_add(64)).ok_or_else(cost::overflow)?;
pay(work,debit)?;let mut values=Vec::with_capacity(a.len());
for item in a{pay(work,16)?;let value=invoke!(work, COPY_VALUE_FRAME, copy_value(item,work))?;values.push(value);}
pay(work,16)?;Ok(values)}
const COMPARE_VEC_VALUE_FRAME:u64=64+4*(size_of::<&[DataValue]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, DataValue>>()+size_of::<&DataValue>()+size_of::<&[DataValue]>()+size_of::<&DataValue>()+size_of::<KeyOrder>()+size_of::<std::ops::Range<usize>>()) as u64;
fn compare_vec_value(a:&[DataValue],b:&[DataValue],work:&mut dyn ContractWork)->Result<KeyOrder>{pay(work,32)?;
for i in 0..a.len().min(b.len()){pay(work,16)?;let order=invoke!(work, COMPARE_VALUE_FRAME, compare_value(&a[i],&b[i],work))?;if order!=KeyOrder::Equal{return Ok(order)}}pay(work,16)?;Ok(a.len().cmp(&b.len()))}
const LENGTH_VEC_SORT_FRAME:u64=64+4*(size_of::<&[SortKey]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<u64>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::iter::Enumerate<std::slice::Iter<'_, SortKey>>>()+size_of::<&SortKey>()+size_of::<u64>()) as u64;
fn length_vec_sort(a:&[SortKey],work:&mut dyn ContractWork)->Result<u64>{pay(work,32)?;
let mut n=2u64;for (i,item) in a.iter().enumerate(){pay(work,16)?;if i!=0{n=wire_add!(work,n,1);}
n=wire_add!(work,n,invoke!(work, LENGTH_SORT_FRAME, length_sort(item,work))?);}pay(work,16)?;Ok(n)}
const COPY_VEC_SORT_FRAME:u64=64+4*(size_of::<&[SortKey]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<Vec<SortKey>>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, SortKey>>()+size_of::<&SortKey>()+size_of::<Vec<SortKey>>()+size_of::<SortKey>()+size_of::<Result<SortKey>>()+size_of::<u64>()+size_of::<u64>()) as u64;
fn copy_vec_sort(a:&[SortKey],work:&mut dyn ContractWork)->Result<Vec<SortKey>>{pay(work,32)?;
pay(work,32)?;
let count=u64::try_from(a.len()).map_err(|_|cost::overflow())?;
let bytes=count.checked_mul(size_of::<SortKey>() as u64).ok_or_else(cost::overflow)?;
let debit=bytes.checked_mul(4).and_then(|n|n.checked_add(64)).ok_or_else(cost::overflow)?;
pay(work,debit)?;let mut values=Vec::with_capacity(a.len());
for item in a{pay(work,16)?;let value=invoke!(work, COPY_SORT_FRAME, copy_sort(item,work))?;values.push(value);}
pay(work,16)?;Ok(values)}
const COMPARE_VEC_SORT_FRAME:u64=64+4*(size_of::<&[SortKey]>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<u64>()+size_of::<usize>()+size_of::<std::slice::Iter<'_, SortKey>>()+size_of::<&SortKey>()+size_of::<&[SortKey]>()+size_of::<&SortKey>()+size_of::<KeyOrder>()+size_of::<std::ops::Range<usize>>()) as u64;
fn compare_vec_sort(a:&[SortKey],b:&[SortKey],work:&mut dyn ContractWork)->Result<KeyOrder>{pay(work,32)?;
for i in 0..a.len().min(b.len()){pay(work,16)?;let order=invoke!(work, COMPARE_SORT_FRAME, compare_sort(&a[i],&b[i],work))?;if order!=KeyOrder::Equal{return Ok(order)}}pay(work,16)?;Ok(a.len().cmp(&b.len()))}

pub(super) const COMPARE_FRAME:u64=64+4*(size_of::<&OwnedCaptureKey>()*2+size_of::<&mut dyn ContractWork>()+size_of::<Result<KeyOrder>>()+size_of::<AdapterError>()+size_of::<KeyOrder>()) as u64;
pub(super) fn compare(a:&OwnedCaptureKey,b:&OwnedCaptureKey,work:&mut dyn ContractWork)->Result<KeyOrder>{
    pay(work,32)?;
    macro_rules! field{($frame:expr,$call:expr)=>{let order=invoke!(work,$frame,$call)?;if order!=KeyOrder::Equal{return Ok(order)}}}
    field!(COMPARE_TEXT_FRAME,compare_text(&a.entity,&b.entity,work));
    field!(COMPARE_TEXT_FRAME,compare_text(&a.binding,&b.binding,work));
    field!(COMPARE_EXPR_FRAME,compare_expr(&a.expression,&b.expression,work));
    field!(COMPARE_TYP_FRAME,compare_typ(&a.expected,&b.expected,work));
    field!(COMPARE_LIMITS_FRAME,compare_limits(&a.limits,&b.limits,work));
    pay(work,0)?;Ok(KeyOrder::Equal)
}
pub(super) const SEARCH_FRAME:u64=64+4*(size_of::<&[Box<OwnedCaptureKey>]>()+size_of::<&OwnedCaptureKey>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<std::result::Result<usize,usize>>>()+size_of::<AdapterError>()+size_of::<usize>()*3+size_of::<KeyOrder>()) as u64;
pub(super) fn search(cache:&[Box<OwnedCaptureKey>],key:&OwnedCaptureKey,work:&mut dyn ContractWork)->Result<std::result::Result<usize,usize>>{
    pay(work,32)?;let mut low=0usize;let mut high=cache.len();
    while low<high{pay(work,32)?;let mid=low+(high-low)/2;
        pay(work,16)?; // index dereference of an owned Box, never pointer identity
        match invoke!(work,COMPARE_FRAME,compare(&cache[mid],key,work))?{
            KeyOrder::Less=>low=mid+1,KeyOrder::Greater=>high=mid,
            KeyOrder::Equal=>{pay(work,0)?;return Ok(Ok(mid))},
        }
    }pay(work,0)?;Ok(Err(low))
}
pub(super) const PUBLISH_FRAME:u64=64+4*(size_of::<&mut Vec<Box<OwnedCaptureKey>>>()+size_of::<&mut usize>()+size_of::<Option<OwnedCaptureKey>>()+size_of::<OwnedCaptureKey>()+size_of::<&mut dyn ContractWork>()+size_of::<Result<()>>()+size_of::<AdapterError>()+size_of::<usize>()*6+size_of::<u64>()*4+size_of::<Box<OwnedCaptureKey>>()+size_of::<std::result::Result<usize,usize>>()) as u64;
pub(super) fn publish(cache:&mut Vec<Box<OwnedCaptureKey>>,cache_bytes:&mut usize,key:Option<OwnedCaptureKey>,work:&mut dyn ContractWork)->Result<()>{
    let Some(key)=key else{return Ok(())};
    pay(work,32)?;
    if cache.len()>=MAX_ITEMS{return Ok(())}
    let bytes=cache_bytes.checked_add(key.wire_len).ok_or_else(cost::overflow)?;
    if bytes>MAX_WIRE_BYTES{return Ok(())}
    let index=match invoke!(work,SEARCH_FRAME,search(cache,&key,work))?{Ok(_)=>return Ok(()),Err(index)=>index};
    pay(work,64)?;
    let capacity=cache.capacity();let length=cache.len();
    let target=if length==capacity{capacity.checked_mul(2).ok_or_else(cost::overflow)?.max(1).min(MAX_ITEMS)}else{capacity};
    let growth=if length==capacity{
        let slots=capacity.checked_add(target).ok_or_else(cost::overflow)?;
        u64::try_from(slots).map_err(|_|cost::overflow())?.checked_mul(size_of::<Box<OwnedCaptureKey>>() as u64).and_then(|n|n.checked_mul(4)).and_then(|n|n.checked_add(64)).ok_or_else(cost::overflow)?
    }else{0};
    let slots=length.checked_sub(index).and_then(|n|n.checked_add(1)).ok_or_else(cost::overflow)?;
    let shift=u64::try_from(slots).map_err(|_|cost::overflow())?.checked_mul(size_of::<Box<OwnedCaptureKey>>() as u64).and_then(|n|n.checked_mul(4)).and_then(|n|n.checked_add(64)).ok_or_else(cost::overflow)?;
    pay(work,growth)?;pay(work,shift)?;
    // Additional stable owner allocation/move/drop; the key's deep payload
    // copy/drop traversal was paid when it was constructed.
    pay(work,64+4*size_of::<OwnedCaptureKey>() as u64)?;pay(work,0)?;
    // Requested-capacity convention: allocator internals/over-allocation are not
    // claimed as exact work. No fallible check/debit after the first mutation.
    let key=Box::new(key);
    if length==capacity{cache.reserve_exact(target-length);}
    cache.insert(index,key);*cache_bytes=bytes;Ok(())
}
pub(super) fn key(entity:&str,binding:&str,expression:&Expr,expected:&Type,limits:&RuntimeLimits,work:&mut dyn ContractWork)->Result<Option<OwnedCaptureKey>>{
    pay(work,0)?;pay(work,4)?;let mut nodes=0;
    if !text_shape(entity,work)?||!text_shape(binding,work)?||!expr_shape(expression,0,&mut nodes,work)?||!type_shape(expected,0,work)?{return Ok(None)}
    pay(work,32)?;
    // Six-element array: seven punctuation bytes and the constant source 0.
    let mut length=8u64;
    length=wire_add!(work,length,invoke!(work,LENGTH_TEXT_FRAME,length_text(entity,work))?);
    length=wire_add!(work,length,invoke!(work,LENGTH_TEXT_FRAME,length_text(binding,work))?);
    length=wire_add!(work,length,invoke!(work,LENGTH_EXPR_FRAME,length_expr(expression,work))?);
    length=wire_add!(work,length,invoke!(work,LENGTH_TYP_FRAME,length_typ(expected,work))?);
    length=wire_add!(work,length,invoke!(work,LENGTH_LIMITS_FRAME,length_limits(limits,work))?);
    pay(work,16)?;let wire_len=usize::try_from(length).map_err(|_|cost::overflow())?;
    let entity=invoke!(work,COPY_TEXT_FRAME,copy_text(entity,work))?;
    let binding=invoke!(work,COPY_TEXT_FRAME,copy_text(binding,work))?;
    let expression=invoke!(work,COPY_EXPR_FRAME,copy_expr(expression,work))?;
    let expected=invoke!(work,COPY_TYP_FRAME,copy_typ(expected,work))?;
    let limits=invoke!(work,COPY_LIMITS_FRAME,copy_limits(limits,work))?;
    pay(work,0)?;Ok(Some(OwnedCaptureKey{entity,binding,expression,expected,limits,wire_len}))
}
#[cfg(test)]
pub(super) fn bytes_for_test(key:&OwnedCaptureKey)->Vec<u8>{canonical_bytes(&(0usize,&key.entity,&key.binding,&key.expression,&key.expected,&key.limits)).unwrap()}
#[cfg(test)]
#[path = "capture/typed_key_controls.rs"]
mod typed_key_controls;
