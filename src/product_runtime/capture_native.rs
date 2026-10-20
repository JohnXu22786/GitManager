//! Finite paid native interfaces, pinned to the reviewed Rust1.99 envelopes.
use super::*;
use super::capture_cost::{add,mul,number,overflow,pay};
use std::mem::size_of;

pub(super) fn lower_paid(value:&str,work:&mut dyn ContractWork)->Result<String> {
    pay(work,16+12*size_of::<usize>() as u64)?;
    let b=number(value.len())?;
    let(mut m,mut p,mut k,mut d,mut e)=(0u64,0u64,0u64,0u64,0u64);
    let(mut at,mut first_b,mut last_b,mut first_m,mut last_m)=(0u64,0u64,0u64,0u64,0u64);
    let mut prefix=true;
    let mut chars=value.chars();
    loop {
        pay(work,23)?;
        let Some(c)=chars.next() else{break};
        if prefix && c.is_ascii(){p=add(p,1)?}else{prefix=false;}
        if c=='Σ' {if k==0{first_b=at;first_m=m;}last_b=at;last_m=m;k=add(k,1)?;}
        if c=='İ'{d=add(d,1)?;}
        if matches!(c,'İ'|'Ⱥ'|'Ⱦ'){e=add(e,1)?;}
        at=add(at,c.len_utf8() as u64)?;m=add(m,1)?;
    }
    pay(work,192)?;
    let(cb,cc)=if k==0{(0,0)}else{
        (mul(2,b)?.checked_sub(first_b).and_then(|x|x.checked_sub(b-last_b-2)).and_then(|x|x.checked_sub(4)).ok_or_else(overflow)?,
         mul(2,m)?.checked_sub(first_m).and_then(|x|x.checked_sub(m-last_m-1)).and_then(|x|x.checked_sub(2)).ok_or_else(overflow)?)
    };
    let q=p/16;let r=p%16;let f=u64::from(b-16*q>=16&&p<b);let z=u64::from(p<b);
    let g=u64::from(e>0);let c=if g==0{b}else{8.max(mul(2,b)?)};
    let prefix_cost=add(add(add(add(add(add(20,mul(16,q)?)?,mul(16,f)?)?,r)?,z)?,mul(2,p)?)?,add(mul(32,add(q,f)?)?,16)?)?;
    let decode=add(b-p,mul(4,m-p)?)?;
    let mapping=mul(58,m-p-k)?;
    let sigma=add(add(mul(8,k)?,cb)?,add(mul(cc,1051)?,mul(mul(2,k)?.min(cc),83)?)?)?;
    let output=add(mul(8,add(m-p,d)?)?,add(b,e)?-p)?;
    let storage=add(add(add(b,mul(g,c)?)?,mul(g,b)?)?,add(34,mul(4,g)?)?)?;
    let native=add(add(add(prefix_cost,decode)?,mapping)?,add(add(sigma,output)?,storage)?)?;
    pay(work,native)?;
    let result=value.to_lowercase();
    pay(work,0)?;
    Ok(result)
}

pub(super) fn text_contains_paid(text:&str,search:&str,work:&mut dyn ContractWork)->Result<bool> {
    pay(work,6)?;
    if search.is_empty(){return Ok(true)}
    if search.len()>text.len(){return Ok(false)}
    let a=text.as_bytes();let b=search.as_bytes();
    for start in 0..=a.len()-b.len() {
        pay(work,4)?;
        let mut matched=true;
        for offset in 0..b.len() {
            pay(work,6)?;
            if a[start+offset]!=b[offset]{matched=false;break;}
        }
        if matched {pay(work,1)?;return Ok(true)}
    }
    pay(work,1)?;Ok(false)
}

fn sort_reserve(n:usize,size:usize,payload:u64,work:&mut dyn ContractWork)->Result<()> {
    let(h,l)=if n<2{(0,0)}else{
        let l=u64::from(usize::BITS-1-n.leading_zeros());(l+u64::from(!n.is_power_of_two()),l)
    };
    let n=number(n)?;let size=number(size)?;
    let(cn,mn)=if n<2{(0,0)}else if n<=20{
        let c=mul(n,n-1)?/2;(c,add(c,mul(2,n-1)?)?)
    }else{(mul(n,add(add(mul(2,h)?,mul(8,l)?)?,40)?)?,mul(n,add(add(mul(4,h)?,mul(12,l)?)?,48)?)?)};
    let c=cn.max(mul(n,h)?);let moves=mn.max(mul(3,mul(n,h)?)?);
    let control=mul(32,add(add(c,moves)?,add(n,1)?)?)?;
    let storage=if n<2{0}else{
        let a=(add(n,1)?/2).max(n.min(8_000_000/size)).max(48).max(n/2);
        let requested=mul(a,size)?;let heap=if requested>4096{requested}else{0};
        let(mut x,mut pivot)=(n,1u64);while x>1{x=add(x,7)?/8;pivot=add(pivot,1)?;}
        let pointer=size_of::<usize>() as u64;
        add(add(add(add(add(4096,heap)?,mul(2,add(mul(66,pointer)?,66)?)?)?,
            mul(add(mul(2,l)?,2)?,add(mul(16,pointer)?,size)?)?)?,mul(pivot,mul(8,pointer)?)?)?,
            add(mul(4,size)?,mul(32,pointer)?)?)?
    };
    pay(work,add(add(add(mul(c,payload)?,mul(moves,size)?)?,control)?,add(storage,12)?)?)
}
pub(super) fn sort_candidates_paid(rows:&mut [&Record],work:&mut dyn ContractWork)->Result<()> {
    pay(work,4)?;let mut width=0usize;
    for row in rows.iter(){pay(work,6)?;width=width.max(row.id.len());}
    pay(work,128)?;
    sort_reserve(rows.len(),size_of::<&Record>(),add(3,mul(2,number(width)?)?)?,work)?;
    rows.sort_by(|a,b|a.id.cmp(&b.id));
    pay(work,0)
}
pub(super) fn sort_rows_paid(rows:&mut [(Id,Vec<DataValue>,DataValue)],sort:&[SortKey],work:&mut dyn ContractWork)->Result<()> {
    pay(work,4)?;let(mut id_width,mut width)=(0usize,0usize);
    for row in rows.iter(){
        pay(work,6)?;id_width=id_width.max(row.0.len());
        for value in &row.1 {
            pay(work,10)?;
            let w=match value {
                DataValue::Text{value}=>value.len(),
                DataValue::Reference{entity,record}=>entity.len().checked_add(record.len()).ok_or_else(overflow)?,
                _=>0,
            };width=width.max(w);
        }
    }
    pay(work,128)?;
    let payload=add(add(7,mul(number(sort.len())?,add(7,mul(2,number(width)?)?)?)?)?,mul(2,number(id_width)?)?)?;
    sort_reserve(rows.len(),size_of::<(Id,Vec<DataValue>,DataValue)>(),payload,work)?;
    rows.sort_by(|a,b| {
        for(i,key)in sort.iter().enumerate(){
            let order=eval::compare(&a.1[i],&b.1[i]);
            if order!=std::cmp::Ordering::Equal{return if key.descending{order.reverse()}else{order}}
        }
        a.0.cmp(&b.0)
    });
    pay(work,0)
}
fn count_add(a:usize,b:usize)->Result<usize>{a.checked_add(b).ok_or_else(||exhausted("cumulative output limit"))}
fn quoted(s:&str,work:&mut dyn ContractWork)->Result<usize>{
    pay(work,4)?;let mut n=2usize;
    for byte in s.bytes(){
        pay(work,10)?;
        let width=match byte{b'"'|b'\\'|8|9|10|12|13=>2,0..=31=>6,_=>1};
        n=count_add(n,width)?;
    }
    Ok(n)
}
fn decimal(value:i64,work:&mut dyn ContractWork)->Result<usize>{
    pay(work,6)?;let mut n=usize::from(value<0);let mut u=value.unsigned_abs();
    loop{pay(work,4)?;n=count_add(n,1)?;u/=10;if u==0{break}}
    Ok(n)
}
fn type_bytes(t:&Type,work:&mut dyn ContractWork)->Result<usize>{
    pay(work,6)?;
    match t {
        Type::Boolean|Type::Integer=>Ok(18),Type::Text|Type::Date=>Ok(15),
        Type::Reference{entity}=>count_add(30,quoted(entity,work)?),
        Type::List{item}=>{pay(work,2)?;count_add(23,type_bytes(item,work)?)},
        Type::Optional{item}=>{pay(work,2)?;count_add(27,type_bytes(item,work)?)},
    }
}
pub(super) fn count_output_paid(value:&DataValue,work:&mut dyn ContractWork)->Result<usize>{
    pay(work,6)?;
    match value {
        DataValue::Null=>Ok(15),DataValue::Boolean{value}=>Ok(if *value{31}else{32}),
        DataValue::Integer{value}=>count_add(27,decimal(*value,work)?),
        DataValue::Date{days}=>count_add(23,decimal(i64::from(*days),work)?),
        DataValue::Text{value}=>count_add(24,quoted(value,work)?),
        DataValue::Reference{entity,record}=>count_add(count_add(40,quoted(entity,work)?)?,quoted(record,work)?),
        DataValue::List{item_type,items}=>{
            pay(work,2)?;let mut n=count_add(39,type_bytes(item_type,work)?)?;
            pay(work,1)?;
            for(i,item)in items.iter().enumerate(){
                pay(work,7)?;n=count_add(n,count_output_paid(item,work)?)?;
                if i>0{n=count_add(n,1)?;}
            }
            pay(work,1)?;Ok(n)
        }
    }
}
