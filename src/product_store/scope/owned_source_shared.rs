//! Shared lexical substrate for owned source analysis. It borrows either
//! canonical app graph and keeps only explicitly charged lexical copies.
use crate::product_alloc::{self, ordered::OrderedMap, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    self, AppRef, AppValidationError, Expr, StringMapRef, StringValueMap, StatementSlice,
    StatementView, Type,
};
use std::collections::BTreeMap;

pub(super) enum SourceFailure {
    Contract(product_contract::ContractError),
    Allocation(AllocationFailure),
}
impl From<product_contract::ContractError> for SourceFailure {
    fn from(value: product_contract::ContractError) -> Self { Self::Contract(value) }
}
impl From<AllocationFailure> for SourceFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}
impl From<AppValidationError> for SourceFailure {
    fn from(value: AppValidationError) -> Self {
        match value { AppValidationError::Contract(value) => Self::Contract(value), AppValidationError::Allocation(value) => Self::Allocation(value) }
    }
}

/// Legacy storage is retained only for its unchanged legacy caller. Every
/// decoded route uses the sorted, explicitly fallible variant.
pub(super) enum LexicalEnvironment {
    Legacy(BTreeMap<String, Type>),
    Fallible(OrderedMap<String, Type>),
}
pub(super) enum LexicalIter<'a> {
    Legacy(std::collections::btree_map::Iter<'a, String, Type>),
    Fallible(std::slice::Iter<'a, (String, Type)>),
}
impl<'a> Iterator for LexicalIter<'a> {
    type Item = (&'a String, &'a Type);
    fn next(&mut self) -> Option<Self::Item> {
        match self { Self::Legacy(iter) => iter.next(), Self::Fallible(iter) => iter.next().map(|(k,v)|(k,v)) }
    }
}
impl ExactSizeIterator for LexicalIter<'_> {}
impl StringValueMap<Type> for LexicalEnvironment {
    type Iter<'a> = LexicalIter<'a> where Self: 'a;
    fn len(&self) -> usize { match self { Self::Legacy(v) => v.len(), Self::Fallible(v) => v.len() } }
    fn get(&self, key: &str) -> Option<&Type> { match self { Self::Legacy(v) => v.get(key), Self::Fallible(v) => v.get(key) } }
    fn contains_key(&self, key: &str) -> bool { match self { Self::Legacy(v) => v.contains_key(key), Self::Fallible(v) => v.contains_key(key) } }
    fn iter(&self) -> Self::Iter<'_> { match self { Self::Legacy(v) => LexicalIter::Legacy(v.iter()), Self::Fallible(v) => LexicalIter::Fallible(v.iter_entries()) } }
}
impl LexicalEnvironment {
    pub(super) fn empty(fallible: bool) -> Self {
        if fallible { Self::Fallible(OrderedMap::new()) } else { Self::Legacy(BTreeMap::new()) }
    }
    pub(super) fn parameters(
        params: StringMapRef<'_, Type>,
        fallible: bool,
        cx: &mut Option<&mut AllocationContext>,
    ) -> Result<Self, SourceFailure> {
        let mut out = Self::empty(fallible);
        if let Self::Fallible(map) = &mut out {
            let cx = cx.as_deref_mut().expect("decoded lexical environment has context");
            map.try_reserve(params.len(), cx)?;
        }
        for (key, value) in params.iter() { out.insert(key, value, cx)?; }
        Ok(out)
    }
    pub(super) fn insert(&mut self, key: &str, value: &Type, cx: &mut Option<&mut AllocationContext>) -> Result<(), SourceFailure> {
        match self {
            Self::Legacy(map) => { map.insert(key.to_owned(), value.clone()); }
            Self::Fallible(map) => {
                let cx = cx.as_deref_mut().expect("decoded lexical environment has context");
                let key = product_alloc::try_copy_str(key, cx, AllocationSite::String)?;
                let value = product_contract::try_clone_type_with(value, cx)?;
                map.try_insert(key, value, cx)?;
            }
        }
        Ok(())
    }
    pub(super) fn reference(&mut self, name: &str, entity: &str, cx: &mut Option<&mut AllocationContext>) -> Result<(), SourceFailure> {
        match self {
            Self::Legacy(map) => { map.insert(name.to_owned(), Type::reference(entity)); }
            Self::Fallible(map) => {
                let cx = cx.as_deref_mut().expect("decoded lexical environment has context");
                let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                let entity = product_alloc::try_copy_str(entity, cx, AllocationSite::String)?;
                map.try_insert(name, Type::Reference { entity }, cx)?;
            }
        }
        Ok(())
    }
    pub(super) fn clone_with(&self, cx: &mut Option<&mut AllocationContext>) -> Result<Self, SourceFailure> {
        match self {
            Self::Legacy(map) => Ok(Self::Legacy(map.clone())),
            Self::Fallible(map) => {
                let cx = cx.as_deref_mut().expect("decoded lexical environment has context");
                Ok(Self::Fallible(map.try_clone_with(cx,
                    |key,cx| product_alloc::try_copy_str(key,cx,AllocationSite::String),
                    |value,cx| product_contract::try_clone_type_with(value,cx),
                )?))
            }
        }
    }
    pub(super) fn equivalent(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|((ak,av),(bk,bv))| ak == bk && av == bv)
    }
}

pub(super) fn expression_type(
    app: AppRef<'_>, expression: &Expr, env: &LexicalEnvironment,
    cx: &mut Option<&mut AllocationContext>,
) -> Result<Type, SourceFailure> {
    match app {
        AppRef::Legacy(app) => Ok(app.expression_type_in(expression, match env { LexicalEnvironment::Legacy(v) => v, _ => unreachable!("legacy app uses legacy lexical owner") })?),
        AppRef::Decoded(app) => Ok(product_contract::decoded_expression_type_in_with(
            app, expression, env, cx.as_deref_mut().expect("decoded expression has context"),
        )?),
        AppRef::Snapshot => Err(SourceFailure::Contract(product_contract::ContractError("Owned source structure: missing app".into()))),
    }
}
pub(super) fn list_item(
    app: AppRef<'_>, items: &Expr, env: &LexicalEnvironment,
    cx: &mut Option<&mut AllocationContext>,
) -> Result<Type, SourceFailure> {
    match expression_type(app, items, env, cx)? {
        Type::List { item } => Ok(*item),
        _ => Err(SourceFailure::Contract(product_contract::ContractError("Owned source structure: loop does not have a typed collection".into()))),
    }
}
fn reference_type(entity: &str, cx: &mut Option<&mut AllocationContext>) -> Result<Type, SourceFailure> {
    match cx.as_deref_mut() {
        Some(cx) => Ok(Type::Reference { entity: product_alloc::try_copy_str(entity, cx, AllocationSite::String)? }),
        None => Ok(Type::reference(entity)),
    }
}

pub(super) fn expression_contains_variable(expression: &Expr, name: &str) -> bool {
    match expression {
        Expr::Variable { name: found } => found == name,
        Expr::Field { record, .. } => expression_contains_variable(record, name),
        Expr::Lower { value } | Expr::Not { value } => expression_contains_variable(value, name),
        Expr::Count { items } | Expr::Sum { items } => expression_contains_variable(items, name),
        Expr::Query { predicate, sort, .. } => expression_contains_variable(predicate, name) || sort.iter().any(|key| expression_contains_variable(&key.value, name)),
        Expr::Filter { items, predicate, .. } | Expr::Any { items, predicate, .. } | Expr::All { items, predicate, .. } =>
            expression_contains_variable(items, name) || expression_contains_variable(predicate, name),
        Expr::Map { items, value, .. } => expression_contains_variable(items, name) || expression_contains_variable(value, name),
        Expr::Equal { left, right } | Expr::Less { left, right } | Expr::Add { left, right } | Expr::Subtract { left, right } | Expr::Concat { left, right } =>
            expression_contains_variable(left, name) || expression_contains_variable(right, name),
        Expr::TextContains { text, search } => expression_contains_variable(text, name) || expression_contains_variable(search, name),
        Expr::And { values } | Expr::Or { values } => values.iter().any(|value| expression_contains_variable(value, name)),
        Expr::If { condition, then_value, else_value } => expression_contains_variable(condition, name) || expression_contains_variable(then_value, name) || expression_contains_variable(else_value, name),
        Expr::Coalesce { value, fallback } => expression_contains_variable(value, name) || expression_contains_variable(fallback, name),
        Expr::Contains { items, value } => expression_contains_variable(items, name) || expression_contains_variable(value, name),
        Expr::DateAdd { date, days } => expression_contains_variable(date, name) || expression_contains_variable(days, name),
        Expr::DateDifference { later, earlier } => expression_contains_variable(later, name) || expression_contains_variable(earlier, name),
        Expr::Literal { .. } | Expr::State { .. } | Expr::Today => false,
    }
}
pub(super) fn statements_contain_variable(steps: StatementSlice<'_>, name: &str) -> bool {
    for index in 0..steps.len() {
        match steps.get(index).expect("bounded statement index") {
            StatementView::Create { values, .. } => if values.iter().any(|(_, value)| expression_contains_variable(value, name)) { return true; },
            StatementView::Update { record, values } => if expression_contains_variable(record, name) || values.iter().any(|(_, value)| expression_contains_variable(value, name)) { return true; },
            StatementView::Archive { record } => if expression_contains_variable(record, name) { return true; },
            StatementView::SetState { value, .. } => if expression_contains_variable(value, name) { return true; },
            StatementView::Collection { target, items, .. } => if matches!(target, product_contract::CollectionTarget::Field { record, .. } if expression_contains_variable(record, name)) || expression_contains_variable(items, name) { return true; },
            StatementView::ForEach { items, steps, .. } => if expression_contains_variable(items, name) || statements_contain_variable(steps, name) { return true; },
            StatementView::Assert { condition, .. } => if expression_contains_variable(condition, name) { return true; },
            StatementView::Emit { items, columns, .. } => if expression_contains_variable(items, name) || columns.iter().any(|(_, value)| expression_contains_variable(value, name)) { return true; },
        }
    }
    false
}

struct Occurrence { kind: &'static str, name: String, value_type: Type, environment: LexicalEnvironment }
impl PartialEq for Occurrence {
    fn eq(&self, other: &Self) -> bool { self.kind == other.kind && self.name == other.name && self.value_type == other.value_type && self.environment.equivalent(&other.environment) }
}
impl Eq for Occurrence {}
struct Trace<'a> { app: AppRef<'a>, occurrences: Vec<Occurrence> }
impl Trace<'_> {
    fn bind(&mut self, kind: &'static str, name: &str, typ: Type, env: &LexicalEnvironment,
        cx: &mut Option<&mut AllocationContext>) -> Result<LexicalEnvironment, SourceFailure> {
        let mut entering = env.clone_with(cx)?;
        let copied_name = match cx.as_deref_mut() {
            Some(cx) => product_alloc::try_copy_str(name, cx, AllocationSite::String)?,
            None => name.to_owned(),
        };
        let copied_type = match cx.as_deref_mut() {
            Some(cx) => product_contract::try_clone_type_with(&typ, cx)?,
            None => typ.clone(),
        };
        reserve_push(&mut self.occurrences, Occurrence { kind, name: copied_name, value_type: copied_type, environment: entering.clone_with(cx)? }, cx)?;
        entering.insert(name, &typ, cx)?;
        Ok(entering)
    }
    fn expression(&mut self, expression: &Expr, env: &LexicalEnvironment,
        cx: &mut Option<&mut AllocationContext>) -> Result<(), SourceFailure> {
        match expression {
            Expr::Query { entity,binding,predicate,sort,.. } => {
                let typ = reference_type(entity, cx)?;
                let local = self.bind("query",binding,typ,env,cx)?;
                self.expression(predicate,&local,cx)?;
                for key in sort { self.expression(&key.value,&local,cx)?; }
            }
            Expr::Filter {items,binding,predicate} | Expr::Any {items,binding,predicate} | Expr::All {items,binding,predicate} => {
                self.expression(items,env,cx)?;
                let kind=match expression { Expr::Filter{..}=>"filter",Expr::Any{..}=>"any",_=>"all" };
                let typ=list_item(self.app,items,env,cx)?;
                let local=self.bind(kind,binding,typ,env,cx)?;
                self.expression(predicate,&local,cx)?;
            }
            Expr::Map {items,binding,value} => {
                self.expression(items,env,cx)?;
                let typ=list_item(self.app,items,env,cx)?;
                let local=self.bind("map",binding,typ,env,cx)?;
                self.expression(value,&local,cx)?;
            }
            Expr::Field{record,..} => self.expression(record,env,cx)?,
            Expr::Count{items}|Expr::Sum{items} => self.expression(items,env,cx)?,
            Expr::Contains{items,value} => { self.expression(items,env,cx)?; self.expression(value,env,cx)?; }
            Expr::Equal{left,right}|Expr::Less{left,right}|Expr::Add{left,right}|Expr::Subtract{left,right}|Expr::Concat{left,right} => { self.expression(left,env,cx)?; self.expression(right,env,cx)?; }
            Expr::TextContains{text,search} => { self.expression(text,env,cx)?; self.expression(search,env,cx)?; }
            Expr::Lower{value}|Expr::Not{value} => self.expression(value,env,cx)?,
            Expr::And{values}|Expr::Or{values} => for value in values { self.expression(value,env,cx)?; },
            Expr::If{condition,then_value,else_value} => { self.expression(condition,env,cx)?; self.expression(then_value,env,cx)?; self.expression(else_value,env,cx)?; }
            Expr::Coalesce{value,fallback} => { self.expression(value,env,cx)?; self.expression(fallback,env,cx)?; }
            Expr::DateAdd{date,days} => { self.expression(date,env,cx)?; self.expression(days,env,cx)?; }
            Expr::DateDifference{later,earlier} => { self.expression(later,env,cx)?; self.expression(earlier,env,cx)?; }
            Expr::Literal{..}|Expr::Variable{..}|Expr::State{..}|Expr::Today => {}
        }
        Ok(())
    }
    fn statements(&mut self, steps: StatementSlice<'_>, env: &mut LexicalEnvironment,
        cx: &mut Option<&mut AllocationContext>) -> Result<(), SourceFailure> {
        for i in 0..steps.len() {
            match steps.get(i).expect("bounded statement index") {
                StatementView::Create{entity,values,bind} => {
                    for (_,value) in values.iter() { self.expression(value,env,cx)?; }
                    let typ = reference_type(entity,cx)?;
                    *env=self.bind("create",bind,typ,env,cx)?;
                }
                StatementView::Update{record,values} => { self.expression(record,env,cx)?; for (_,value) in values.iter(){ self.expression(value,env,cx)?; } }
                StatementView::Archive{record} => self.expression(record,env,cx)?,
                StatementView::SetState{value,..} => self.expression(value,env,cx)?,
                StatementView::Collection{target,items,..} => { if let product_contract::CollectionTarget::Field{record,..}=target { self.expression(record,env,cx)?; } self.expression(items,env,cx)?; }
                StatementView::ForEach{items,binding,steps} => { self.expression(items,env,cx)?; let typ=list_item(self.app,items,env,cx)?; let mut local=self.bind("for_each",binding,typ,env,cx)?; self.statements(steps,&mut local,cx)?; }
                StatementView::Assert{condition,..} => self.expression(condition,env,cx)?,
                StatementView::Emit{items,binding,columns,..} => { self.expression(items,env,cx)?; let typ=list_item(self.app,items,env,cx)?; let local=self.bind("emit",binding,typ,env,cx)?; for (_,value) in columns.iter(){self.expression(value,&local,cx)?;} }
            }
        }
        Ok(())
    }
}
fn reserve_push<T>(items:&mut Vec<T>, item:T, cx:&mut Option<&mut AllocationContext>) -> Result<(),SourceFailure> {
    match cx.as_deref_mut() { Some(cx)=>product_alloc::try_reserve(items,1,cx,AllocationSite::Vector)?, None=>items.reserve(1) }
    items.push(item); Ok(())
}

pub(super) fn correspond(
    before: AppRef<'_>, old: StatementSlice<'_>, after: AppRef<'_>, new: StatementSlice<'_>,
    env: &LexicalEnvironment, cx: &mut Option<&mut AllocationContext>,
) -> Result<bool, SourceFailure> {
    let mut old_env=env.clone_with(cx)?;
    let mut old_trace=Trace{app:before,occurrences:Vec::new()};
    old_trace.statements(old,&mut old_env,cx)?;
    let mut new_env=env.clone_with(cx)?;
    let mut new_trace=Trace{app:after,occurrences:Vec::new()};
    new_trace.statements(new,&mut new_env,cx)?;
    Ok(new_trace.occurrences.starts_with(&old_trace.occurrences))
}
