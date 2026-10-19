//! Paid shape guard for the unchanged complete native canonical tuple.
use super::*;
use super::capture_cost::{pay,contract};

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
pub(super) fn key(entity:&str,binding:&str,expression:&Expr,expected:&Type,limits:&RuntimeLimits,work:&mut dyn ContractWork)->Result<Option<Vec<u8>>>{
    pay(work,4)?;let mut nodes=0;
    if !text_shape(entity,work)?||!text_shape(binding,work)?||!expr_shape(expression,0,&mut nodes,work)?||!type_shape(expected,0,work)?{return Ok(None)}
    pay(work,7)?;
    let key=canonical_bytes_with_work(&(0usize,entity,binding,expression,expected,limits),work).map_err(contract)?;
    Ok(Some(key))
}
