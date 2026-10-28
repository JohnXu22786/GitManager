//! Same lexical Eval, paid-row-only operations. Ordinary Eval bodies stay intact.
use super::*;
use super::capture_cost as cost;
use super::eval::{boolean,day,integer,list,text};
use std::cmp::Ordering as Compare;

impl Eval<'_> {
    fn references_paid(&mut self,value:&DataValue)->Result<()> {
        self.meter.tick(1)?;cost::pay(self.meter,2)?;
        match value {
            DataValue::Reference{..}=>{self.meter.tick(self.data.records.len() as u64)?;cost::record(self.data,value,self.meter)?;}
            DataValue::List{items,..}=>{
                cost::pay(self.meter,1)?;
                for value in items{cost::pay(self.meter,3)?;self.references_paid(value)?;}
                cost::pay(self.meter,1)?;
            }
            _=>{}
        }Ok(())
    }
    pub(super) fn typ_paid(&mut self,expr:&Expr,env:&Env)->Result<Type> {
        self.meter.tick(1)?;cost::pay(self.meter,3)?;
        Ok(match expr {
            Expr::Literal{value_type,..}=>{cost::typ(value_type,self.meter)?;value_type.clone()}
            Expr::Variable{name}=>{
                cost::search(env.len(),name.len(),self.meter)?;
                let t=&env.get(name).ok_or_else(||failed("missing variable type"))?.0;
                cost::typ(t,self.meter)?;t.clone()
            }
            Expr::State{state}=>{
                cost::pay(self.meter,1)?;let mut found=None;
                for s in &self.app.state {cost::pay(self.meter,3)?;if cost::eq_string(&s.id,state,self.meter)?{found=Some(&s.value_type);break;}}
                if found.is_none(){cost::pay(self.meter,1)?;}
                let t=found.ok_or_else(||failed("missing state type"))?;
                cost::typ(t,self.meter)?;t.clone()
            }
            Expr::Field{record,field}=>{
                let Type::Reference{entity}=self.typ(record,env)? else{return Err(failed("invalid field type"))};
                cost::pay(self.meter,1)?;let mut found=None;
                for e in &self.app.entities {
                    cost::pay(self.meter,3)?;
                    if cost::eq_string(&e.id,&entity,self.meter)? {
                        cost::pay(self.meter,1)?;
                        for f in &e.fields {cost::pay(self.meter,3)?;if cost::eq_string(&f.id,field,self.meter)?{found=Some(&f.value_type);break;}}
                        break;
                    }
                }
                if found.is_none(){cost::pay(self.meter,2)?;}
                let t=found.ok_or_else(||failed("missing field type"))?;cost::typ(t,self.meter)?;t.clone()
            }
            Expr::Query{entity,..}=>{let t=cost::reference_type(entity,self.meter)?;cost::list_type(t,self.meter)?}
            Expr::Filter{items,..}=>self.typ(items,env)?,
            Expr::Map{items,binding,value}=>{
                let Type::List{item}=self.typ(items,env)? else{return Err(failed("invalid map type"))};
                cost::env(env,self.meter)?;let mut local=env.clone();cost::pay(self.meter,2)?;
                cost::bind(&mut local,binding,*item,DataValue::Null,self.meter)?;
                let t=self.typ(value,&local)?;cost::list_type(t,self.meter)?
            }
            Expr::If{then_value,..}=>self.typ(then_value,env)?,
            Expr::Coalesce{fallback,..}=>self.typ(fallback,env)?,
            Expr::Today|Expr::DateAdd{..}=>Type::Date,
            Expr::Count{..}|Expr::Sum{..}|Expr::Add{..}|Expr::Subtract{..}|Expr::DateDifference{..}=>Type::Integer,
            Expr::Concat{..}|Expr::Lower{..}=>Type::Text,
            _=>Type::Boolean,
        })
    }
    pub(super) fn eval_paid(&mut self,expr:&Expr,env:&Env)->Result<DataValue> {
        self.meter.tick(1)?;cost::pay(self.meter,4)?;
        let value=match expr {
            Expr::Literal{value,..}=>{
                self.meter.value(value)?;self.references_paid(value)?;
                cost::value(value,self.meter)?;value.clone()
            }
            Expr::Variable{name}=>{
                cost::search(env.len(),name.len(),self.meter)?;
                let value=&env.get(name).ok_or_else(||failed("unbound variable"))?.1;
                self.meter.value(value)?;cost::value(value,self.meter)?;value.clone()
            }
            Expr::Field{record:expr,field}=>{
                let key=self.eval(expr,env)?;self.meter.tick(self.data.records.len() as u64)?;
                let record=cost::record(self.data,&key,self.meter)?;
                cost::search(record.values.len(),field.len(),self.meter)?;
                let found=record.values.get(field);
                // Full selected owner is paid BEFORE the native .cloned().
                if let Some(value)=found{cost::value(value,self.meter)?}else{cost::pay(self.meter,2)?;}
                let value=found.cloned().unwrap_or(DataValue::Null);
                self.meter.value(&value)?;value
            }
            Expr::State{state}=>{
                cost::search(self.session.values.len(),state.len(),self.meter)?;
                let value=self.session.values.get(state).ok_or_else(||failed("missing session value"))?;
                self.meter.value(value)?;cost::value(value,self.meter)?;value.clone()
            }
            Expr::Today=>{cost::pay(self.meter,1)?;DataValue::Date{days:self.day}}
            Expr::Query{entity,binding,predicate,sort,limit,include_archived}=>{
                cost::pay(self.meter,3)?;let mut candidates=Vec::new();
                for record in &self.data.records {
                    self.meter.tick(1)?;cost::pay(self.meter,3)?;
                    if cost::eq_string(&record.entity,entity,self.meter)? {
                        cost::pay(self.meter,3)?;
                        if !record.archived||*include_archived{cost::push(&mut candidates,record,self.meter)?;}
                    }
                }
                cost::pay(self.meter,1)?;self.meter.collection(candidates.len())?;
                cost::pay(self.meter,12)?;
                let comparisons=candidates.len().saturating_mul((usize::BITS-candidates.len().max(1).leading_zeros()) as usize);
                self.meter.tick(comparisons as u64)?;
                capture_native::sort_candidates_paid(&mut candidates,self.meter)?;
                cost::pay(self.meter,3)?;let mut rows=Vec::new();
                for record in candidates {
                    self.meter.tick(1)?;cost::pay(self.meter,3)?;
                    let value=cost::reference(&record.entity,&record.id,self.meter)?;
                    cost::env(env,self.meter)?;let mut local=env.clone();
                    let typ=cost::reference_type(entity,self.meter)?;
                    cost::value(&value,self.meter)?;
                    cost::bind(&mut local,binding,typ,value.clone(),self.meter)?;
                    cost::pay(self.meter,2)?;
                    if boolean(self.eval(predicate,&local)?)? {
                        cost::pay(self.meter,3)?;let mut keys=Vec::new();
                        for key in sort {cost::pay(self.meter,3)?;let value=self.eval(&key.value,&local)?;cost::push(&mut keys,value,self.meter)?;}
                        cost::pay(self.meter,1)?;cost::string(&record.id,self.meter)?;
                        cost::push(&mut rows,(record.id.clone(),keys,value),self.meter)?;
                        self.meter.collection(rows.len())?;
                    }
                }
                cost::pay(self.meter,1)?;
                cost::pay(self.meter,16)?;
                let comparisons=rows.len().saturating_mul((usize::BITS-rows.len().max(1).leading_zeros()) as usize).saturating_mul(sort.len().max(1));
                self.meter.tick(comparisons as u64)?;
                capture_native::sort_rows_paid(&mut rows,sort,self.meter)?;
                self.meter.tick(1)?;cost::pay(self.meter,5)?;
                let item_type=cost::reference_type(entity,self.meter)?;let mut items=Vec::new();
                for(_,_,value)in rows.into_iter().take(*limit){cost::pay(self.meter,3)?;cost::push(&mut items,value,self.meter)?;}
                cost::pay(self.meter,1)?;DataValue::List{item_type,items}
            }
            Expr::Filter{items,binding,predicate}|Expr::Any{items,binding,predicate}|Expr::All{items,binding,predicate}=>{
                cost::pay(self.meter,8)?;let(typ,items)=list(self.eval(items,env)?)?;
                let mut kept=Vec::new();let all=matches!(expr,Expr::All{..});let any=matches!(expr,Expr::Any{..});let mut result=all;
                for item in items {
                    self.meter.tick(1)?;cost::pay(self.meter,3)?;
                    cost::env(env,self.meter)?;let mut local=env.clone();cost::typ(&typ,self.meter)?;cost::value(&item,self.meter)?;
                    cost::bind(&mut local,binding,typ.clone(),item.clone(),self.meter)?;
                    cost::pay(self.meter,5)?;let yes=boolean(self.eval(predicate,&local)?)?;
                    if any&&yes{result=true;break}if all&&!yes{result=false;break}
                    if yes{cost::push(&mut kept,item,self.meter)?;}
                }
                cost::pay(self.meter,2)?;
                if all||any{DataValue::Boolean{value:result}}else{DataValue::List{item_type:typ,items:kept}}
            }
            Expr::Map{items,binding,value}=>{
                cost::pay(self.meter,5)?;let(typ,items)=list(self.eval(items,env)?)?;
                cost::env(env,self.meter)?;let mut local=env.clone();cost::typ(&typ,self.meter)?;cost::pay(self.meter,2)?;
                cost::bind(&mut local,binding,typ.clone(),DataValue::Null,self.meter)?;
                let result_type=self.typ(value,&local)?;cost::pay(self.meter,3)?;let mut mapped=Vec::new();
                for item in items {
                    self.meter.tick(1)?;cost::pay(self.meter,3)?;cost::typ(&typ,self.meter)?;
                    cost::bind(&mut local,binding,typ.clone(),item,self.meter)?;
                    let result=self.eval(value,&local)?;cost::push(&mut mapped,result,self.meter)?;
                }
                cost::pay(self.meter,1)?;DataValue::List{item_type:result_type,items:mapped}
            }
            Expr::Count{items}=>{cost::pay(self.meter,3)?;DataValue::Integer{value:list(self.eval(items,env)?)?.1.len() as i64}}
            Expr::Sum{items}=>{
                cost::pay(self.meter,4)?;let mut sum=0i64;
                for value in list(self.eval(items,env)?)?.1 {
                    self.meter.tick(1)?;cost::pay(self.meter,7)?;
                    sum=sum.checked_add(integer(value)?).ok_or_else(||failed("integer overflow"))?;
                }
                cost::pay(self.meter,1)?;DataValue::Integer{value:sum}
            }
            Expr::Contains{items,value}=>{
                cost::pay(self.meter,3)?;let(_,items)=list(self.eval(items,env)?)?;let value=self.eval(value,env)?;
                self.meter.tick(items.len() as u64)?;let mut found=false;
                for item in &items{cost::pay(self.meter,3)?;if cost::equal(item,&value,self.meter)?{found=true;break}}
                cost::pay(self.meter,1)?;DataValue::Boolean{value:found}
            }
            Expr::Equal{left,right}=>{
                let left=self.eval(left,env)?;let right=self.eval(right,env)?;
                self.meter.value(&left)?;self.meter.value(&right)?;
                DataValue::Boolean{value:cost::equal(&left,&right,self.meter)?}
            }
            Expr::Less{left,right}=>{
                let left=self.eval(left,env)?;let right=self.eval(right,env)?;
                DataValue::Boolean{value:cost::compare(&left,&right,self.meter)?==Compare::Less}
            }
            Expr::Add{left,right}|Expr::Subtract{left,right}=>{
                cost::pay(self.meter,7)?;let a=integer(self.eval(left,env)?)?;let b=integer(self.eval(right,env)?)?;
                DataValue::Integer{value:if matches!(expr,Expr::Add{..}){a.checked_add(b)}else{a.checked_sub(b)}.ok_or_else(||failed("integer overflow"))?}
            }
            Expr::Concat{left,right}=>{
                cost::pay(self.meter,7)?;let mut a=text(self.eval(left,env)?)?;let b=text(self.eval(right,env)?)?;
                if a.len().saturating_add(b.len())>MAX_TEXT_BYTES{return Err(exhausted("text byte limit"))}
                self.meter.tick((a.len()+b.len()) as u64)?;
                cost::pay(self.meter,20)?;
                let n=a.len().checked_add(b.len()).ok_or_else(cost::overflow)?;let grows=n>a.capacity();
                let bytes=cost::add(8,cost::number(b.len())?)?;
                let bytes=if grows{cost::add(bytes,cost::add(cost::number(a.capacity())?,cost::add(cost::number(n)?,4)?)?)?}else{bytes};
                cost::pay(self.meter,bytes)?;if grows{a.reserve_exact(b.len());}a.push_str(&b);cost::pay(self.meter,0)?;
                DataValue::Text{value:a}
            }
            Expr::TextContains{text:source,search}=>{
                cost::pay(self.meter,4)?;let a=text(self.eval(source,env)?)?;let b=text(self.eval(search,env)?)?;
                self.meter.tick((a.len()+b.len()) as u64)?;DataValue::Boolean{value:capture_native::text_contains_paid(&a,&b,self.meter)?}
            }
            Expr::Lower{value}=>{
                cost::pay(self.meter,2)?;let value=text(self.eval(value,env)?)?;self.meter.tick(value.len() as u64)?;
                let value=capture_native::lower_paid(&value,self.meter)?;
                cost::pay(self.meter,2)?;if value.len()>MAX_TEXT_BYTES{return Err(exhausted("text byte limit"))}DataValue::Text{value}
            }
            Expr::And{values}|Expr::Or{values}=>{
                cost::pay(self.meter,3)?;let is_and=matches!(expr,Expr::And{..});let mut result=is_and;
                for value in values{cost::pay(self.meter,5)?;let value=boolean(self.eval(value,env)?)?;if value!=is_and{result=value;break;}}
                cost::pay(self.meter,1)?;DataValue::Boolean{value:result}
            }
            Expr::Not{value}=>{cost::pay(self.meter,2)?;DataValue::Boolean{value:!boolean(self.eval(value,env)?)?}}
            Expr::If{condition,then_value,else_value}=>{
                cost::pay(self.meter,2)?;if boolean(self.eval(condition,env)?)?{self.eval(then_value,env)?}else{self.eval(else_value,env)?}
            }
            Expr::Coalesce{value,fallback}=>{
                let value=self.eval(value,env)?;cost::pay(self.meter,2)?;if value==DataValue::Null{self.eval(fallback,env)?}else{value}
            }
            Expr::DateAdd{date,days}=>{
                cost::pay(self.meter,12)?;let date=day(self.eval(date,env)?)?;let days=integer(self.eval(days,env)?)?;
                let value=i64::from(date).checked_add(days).and_then(|v|i32::try_from(v).ok()).ok_or_else(||failed("date overflow"))?;
                validate_day(value)?;DataValue::Date{days:value}
            }
            Expr::DateDifference{later,earlier}=>{
                cost::pay(self.meter,5)?;DataValue::Integer{value:i64::from(day(self.eval(later,env)?)?)-i64::from(day(self.eval(earlier,env)?)?)}
            }
        };
        self.meter.tick(1)?;Ok(value)
    }
}
