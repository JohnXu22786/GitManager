//! Allocation-aware rederivation of the diagnostic common-fact inventory.
//! This DTO is bound to a runtime-authenticated lifecycle capability and is
//! never assembled from the persisted inventory fields.
use super::fallible_owned_source_analysis::{analyze_owned_sources_fallible, SourceAnalysisFailure};
use super::fallible_owned_source_decode::{DecodedOwnedStructuralReport, DecodedOwnedWriter};
use super::fallible_owned_lifecycle_acceptance::FallibleAcceptedPreviewLifecycle;
use super::owned_contract::{OwnedSourceSide, OwnedStatementKind};
use super::owned_source_shared::{self, LexicalEnvironment, SourceFailure};
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{self, AppRef, AppValidationError, CollectionOperation, CollectionTarget, ContractError, Digest, Expr, FallibleDecodedCapturedProgram, IdentityDomain, RuntimeLimits, StringMapRef, StringValueMap, Type, ViewKindRef, MAX_AST_NODES, MAX_COLLECTION, MAX_TEXT_BYTES};
use crate::product_json::Value;
use crate::product_store::{fallible_project_snapshot_decode::DecodedProjectSnapshot, StoreError};
use serde::{Serialize, Serializer};
use serde::ser::{SerializeMap, SerializeSeq, SerializeTuple};
use std::fmt;

#[derive(Debug)]
pub(crate) enum CommonFactInventoryFailure { Store(StoreError), Allocation(AllocationFailure) }
impl From<AllocationFailure> for CommonFactInventoryFailure { fn from(value: AllocationFailure) -> Self { Self::Allocation(value) } }
impl From<StoreError> for CommonFactInventoryFailure { fn from(value: StoreError) -> Self { Self::Store(value) } }
impl From<SourceAnalysisFailure> for CommonFactInventoryFailure { fn from(value: SourceAnalysisFailure) -> Self { match value { SourceAnalysisFailure::Allocation(v) => Self::Allocation(v), SourceAnalysisFailure::Contract(v) => Self::Store(v.into()) } } }
fn app_error(value: AppValidationError) -> CommonFactInventoryFailure { match value { AppValidationError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), AppValidationError::Contract(v) => CommonFactInventoryFailure::Store(v.into()) } }
impl From<ContractError> for CommonFactInventoryFailure { fn from(value: ContractError) -> Self { Self::Store(value.into()) } }
fn codec_error(value: crate::product_backup::fallible_codec::CodecError) -> CommonFactInventoryFailure { match value { crate::product_backup::fallible_codec::CodecError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), _ => CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: identity encoding invariant".into())) } }
fn error(cx: &mut AllocationContext, args: fmt::Arguments<'_>) -> CommonFactInventoryFailure {
    let mut text = String::new();
    for part in [format_args!("Common fact inventory: "), args] {
        if let Err(failure) = product_alloc::try_append_fmt(&mut text, part, cx, AllocationSite::Formatting) {
            return match failure { product_alloc::FormatFailure::Allocation(v) => CommonFactInventoryFailure::Allocation(v), product_alloc::FormatFailure::Formatter => CommonFactInventoryFailure::Store(StoreError::Invalid(String::new())) };
        }
    }
    CommonFactInventoryFailure::Store(StoreError::Invalid(text))
}
fn copy_string(value: &str, cx: &mut AllocationContext) -> Result<String, CommonFactInventoryFailure> { product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into) }
fn copy_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, CommonFactInventoryFailure> { Digest::from_owned_lower_hex(copy_string(value.as_str(), cx)?).ok_or_else(|| CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: digest copy invariant".into()))) }
fn copy_indices(value:&[usize],cx:&mut AllocationContext)->Result<Vec<usize>,CommonFactInventoryFailure>{let mut out=Vec::new();product_alloc::try_reserve(&mut out,value.len(),cx,AllocationSite::Vector)?;out.extend_from_slice(value);Ok(out)}
fn copy_expr(value: &Expr, cx: &mut AllocationContext) -> Result<Expr, CommonFactInventoryFailure> { product_contract::try_clone_expr_with(value, cx).map_err(app_error) }
fn copy_type(value: &Type, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> { product_contract::try_clone_type_with(value, cx).map_err(CommonFactInventoryFailure::Allocation) }
fn copy_entity(value:&product_contract::EntityDefinition,cx:&mut AllocationContext)->Result<product_contract::EntityDefinition,CommonFactInventoryFailure>{product_contract::try_clone_entity_definition_with(value,cx).map_err(app_error)}
fn push<T>(values: &mut Vec<T>, value: T, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
    if values.len() >= MAX_AST_NODES { return Err(error(cx, format_args!("inventory node limit exceeded"))); }
    product_alloc::try_push(values, value, cx, AllocationSite::Vector).map_err(Into::into)
}
fn format_owned(cx: &mut AllocationContext, args: fmt::Arguments<'_>) -> Result<String, CommonFactInventoryFailure> { let mut value=String::new(); product_alloc::try_append_fmt(&mut value,args,cx,AllocationSite::Formatting).map_err(|error|match error { product_alloc::FormatFailure::Allocation(v)=>CommonFactInventoryFailure::Allocation(v), product_alloc::FormatFailure::Formatter=>CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: formatting invariant".into())) })?; Ok(value) }
fn relation(typ: &Type) -> Option<&str> { match typ { Type::Reference { entity } => Some(entity), Type::List { item } | Type::Optional { item } => relation(item), _ => None } }
fn item(app: AppRef<'_>, expr: &Expr, env: &LexicalEnvironment, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> {
    match owned_source_shared::expression_type(app, expr, env, &mut Some(&mut *cx)) {
        Ok(Type::List { item }) => Ok(*item), Ok(_) => Err(error(cx, format_args!("expected validated collection"))),
        Err(SourceFailure::Allocation(failure)) => Err(CommonFactInventoryFailure::Allocation(failure)),
        Err(SourceFailure::Contract(value)) => Err(CommonFactInventoryFailure::Store(value.into())),
    }
}
fn copy_env(env: &LexicalEnvironment, cx: &mut AllocationContext) -> Result<super::fallible_scalar_decode::FallibleScopeMap<String, Type>, CommonFactInventoryFailure> {
    let mut out=super::fallible_scalar_decode::FallibleScopeMap::new();
    for (key,value) in env.iter() { out.insert(copy_string(key,cx)?,copy_type(value,cx)?,cx).map_err(|value|match value { crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(v)=>CommonFactInventoryFailure::Allocation(v), _=>CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: environment copy invariant".into())) })?; }
    Ok(out)
}

pub(crate) struct DecodedCommonFactUse {
    pub(crate) side: OwnedSourceSide,
    pub(crate) source: Digest,
    pub(crate) location: String,
    pub(crate) kind: String,
    pub(crate) value_type: Option<Type>,
    pub(crate) expression: Option<Expr>,
    pub(crate) environment: super::fallible_scalar_decode::FallibleScopeMap<String, Type>,
    pub(crate) entity: Option<String>,
    pub(crate) field: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) details: Value,
}
struct ProductJsonValueRef<'a>(&'a Value);
impl Serialize for ProductJsonValueRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Number(crate::product_json::Number::U64(value)) => serializer.serialize_u64(*value),
            Value::Number(crate::product_json::Number::I64(value)) => serializer.serialize_i64(*value),
            Value::Number(crate::product_json::Number::F64(value)) => serializer.serialize_f64(*value),
            Value::String(value) => serializer.serialize_str(value),
            Value::Array(values) => { let mut seq=serializer.serialize_seq(Some(values.len()))?;for value in values{seq.serialize_element(&ProductJsonValueRef(value))?;}seq.end() },
            Value::Object(values) => { let mut map=serializer.serialize_map(Some(values.len()))?;for (key,value) in values.iter(){map.serialize_entry(key,&ProductJsonValueRef(value))?;}map.end() },
        }
    }
}
impl Serialize for DecodedCommonFactUse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map=serializer.serialize_map(Some(12))?;
        map.serialize_entry("side",&self.side)?;map.serialize_entry("source",&self.source)?;
        map.serialize_entry("location",&self.location)?;map.serialize_entry("kind",&self.kind)?;
        map.serialize_entry("value_type",&self.value_type)?;map.serialize_entry("expression",&self.expression)?;
        map.serialize_entry("environment",&self.environment)?;map.serialize_entry("entity",&self.entity)?;
        map.serialize_entry("field",&self.field)?;map.serialize_entry("state",&self.state)?;
        map.serialize_entry("details",&ProductJsonValueRef(&self.details))?;map.end()
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactObligation { pub(crate) side: Option<OwnedSourceSide>, pub(crate) location: String, pub(crate) code: String, pub(crate) reason: String }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactPhase { pub(crate) kind: DecodedCommonFactPhaseKind, pub(crate) edit: Option<String>, pub(crate) before: DecodedCommonFactSpan, pub(crate) after: DecodedCommonFactSpan }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all="snake_case")]
pub(crate) enum DecodedCommonFactPhaseKind { CommonSubtree, CommonHeader, Variant }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactSpan { pub(crate) path: Vec<usize>, pub(crate) len: usize, pub(crate) digest: Digest, pub(crate) environment: super::fallible_scalar_decode::FallibleScopeMap<String, Type>, pub(crate) controls: Vec<Expr>, pub(crate) previous: Option<Digest>, pub(crate) next: Option<Digest> }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactAction { pub(crate) action: String, pub(crate) parameters: super::fallible_scalar_decode::FallibleScopeMap<String, Type>, pub(crate) guards: Vec<Expr>, pub(crate) ensures: Vec<Expr>, pub(crate) before_identity: Digest, pub(crate) after_identity: Digest, pub(crate) phases: Vec<DecodedCommonFactPhase> }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactReference { pub(crate) side: OwnedSourceSide, pub(crate) entity: String, pub(crate) field: String, pub(crate) target_entity: String, pub(crate) required: bool, pub(crate) matches_retained_schema: bool, pub(crate) unique: Vec<Vec<String>>, pub(crate) producers: Vec<DecodedOwnedWriter>, pub(crate) mutations: Vec<DecodedOwnedWriter> }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactIdentityHazard { pub(crate) side: OwnedSourceSide, pub(crate) action: String, pub(crate) variant_create: Vec<usize>, pub(crate) common_create: Vec<usize>, pub(crate) relation: String, pub(crate) reason: String }
#[derive(Debug, Serialize)]
pub(crate) struct DecodedCommonFactResources { pub(crate) total_record_cap: usize, pub(crate) stored_records: usize, pub(crate) includes_archived_and_auxiliary: bool, pub(crate) ordinary_defaults: RuntimeLimits, pub(crate) max_query_limit_proves_order_independence: bool, pub(crate) failures_commit_fuel_and_trace: bool }
#[derive(Serialize)]
pub(crate) struct DecodedCommonFactInventory {
    pub(crate) snapshot: Digest, pub(crate) retained_data: Digest, pub(crate) retained_schema: Vec<crate::product_contract::EntityDefinition>,
    pub(crate) before_schema: Vec<crate::product_contract::EntityDefinition>, pub(crate) after_schema: Vec<crate::product_contract::EntityDefinition>,
    pub(crate) structure: DecodedOwnedStructuralReport, pub(crate) lifecycle: super::fallible_owned_request_decode::DecodedCommonFactLifecycleSuggestion,
    pub(crate) lifecycle_writers: Vec<DecodedOwnedWriter>, pub(crate) actions: Vec<DecodedCommonFactAction>, pub(crate) uses: Vec<DecodedCommonFactUse>,
    pub(crate) references: Vec<DecodedCommonFactReference>, pub(crate) identity_hazards: Vec<DecodedCommonFactIdentityHazard>, pub(crate) obligations: Vec<DecodedCommonFactObligation>, pub(crate) resources: DecodedCommonFactResources,
}
impl crate::product_backup::fallible_codec::BackupWire for DecodedCommonFactInventory {}

#[derive(Clone, Copy)]
enum Detail<'a> {
    Null,
    Create { entity: &'a str, bind: &'a str },
    State { state: &'a str },
    Collection { target: &'a CollectionTarget, operation: CollectionOperation },
    Binding { binding: &'a str },
    Emit { output: &'a str, binding: &'a str },
    Schema(&'a crate::product_contract::FieldDefinition),
    Unique(&'a [String]),
    StateDefinition(&'a crate::product_contract::StateDefinition),
    Parameter { action: &'a str, parameter: &'a str },
    Output(&'a crate::product_contract::OutputDefinition),
    Control(crate::product_contract::ControlRef<'a>),
    Selection(crate::product_contract::SelectionRef<'a>),
    Form { action: &'a str },
    FormInput(crate::product_contract::FormInputRef<'a>),
    ActionBinding { action: &'a str, placement: crate::product_contract::ActionPlacement },
    Key(crate::product_contract::KeyRef<'a>),
}
impl Serialize for Detail<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        match self {
            Self::Null => return serializer.serialize_unit(),
            Self::Schema(value) => return value.serialize(serializer),
            Self::Unique(value) => return value.serialize(serializer),
            Self::StateDefinition(value) => return value.serialize(serializer),
            Self::Output(value) => return value.serialize(serializer),
            _ => {}
        }
        let mut map=serializer.serialize_map(None)?;
        match self {
            Self::Null|Self::Schema(_)|Self::Unique(_)|Self::StateDefinition(_)|Self::Output(_)=>unreachable!(),
            Self::Create{entity,bind}=>{map.serialize_entry("entity",entity)?;map.serialize_entry("bind",bind)?;},
            Self::State{state}=>map.serialize_entry("state",state)?,
            Self::Collection{target,operation}=>{map.serialize_entry("target",target)?;map.serialize_entry("operation",operation)?;},
            Self::Binding{binding}=>map.serialize_entry("binding",binding)?,
            Self::Emit{output,binding}=>{map.serialize_entry("output",output)?;map.serialize_entry("binding",binding)?;},
            Self::Parameter{action,parameter}=>{map.serialize_entry("action",action)?;map.serialize_entry("parameter",parameter)?;},
            Self::Control(value)=>{map.serialize_entry("id",value.id())?;map.serialize_entry("label",value.label())?;map.serialize_entry("state",value.state())?;map.serialize_entry("on_change",&value.on_change())?;},
            Self::Selection(value)=>{map.serialize_entry("id",value.id())?;map.serialize_entry("state",value.state())?;map.serialize_entry("on_change",&value.on_change())?;},
            Self::Form{action}=>map.serialize_entry("action",action)?,
            Self::FormInput(value)=>{map.serialize_entry("parameter",value.parameter())?;map.serialize_entry("label",value.label())?;},
            Self::ActionBinding{action,placement}=>{map.serialize_entry("action",action)?;map.serialize_entry("placement",placement)?;},
            Self::Key(value)=>{map.serialize_entry("key",value.key())?;map.serialize_entry("modifiers",&KeyModifiersRef(*value))?;map.serialize_entry("binding",value.binding())?;},
        }
        map.end()
    }
}
impl crate::product_backup::fallible_codec::BackupWire for Detail<'_> {}

struct KeyModifiersRef<'a>(crate::product_contract::KeyRef<'a>);
impl Serialize for KeyModifiersRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.modifier_count()))?;
        for index in 0..self.0.modifier_count() {
            seq.serialize_element(&self.0.modifier(index).expect("bounded key modifier index"))?;
        }
        seq.end()
    }
}

struct UseVisitor<'a> {
    app: AppRef<'a>, source: &'a Digest, side: OwnedSourceSide, root: &'a str,
    uses: Vec<DecodedCommonFactUse>, obligations: Vec<DecodedCommonFactObligation>,
    cx: &'a mut AllocationContext,
}
impl UseVisitor<'_> {
    fn need(&mut self, at: &str, code: &str, reason: &str) -> Result<(), CommonFactInventoryFailure> {
        let value=DecodedCommonFactObligation { side:Some(self.side), location:copy_string(at,self.cx)?, code:copy_string(code,self.cx)?, reason:copy_string(reason,self.cx)? };
        push(&mut self.obligations,value,self.cx)
    }
    fn at(&mut self, args: fmt::Arguments<'_>) -> Result<String, CommonFactInventoryFailure> { format_owned(self.cx,args) }
    fn expression_type(&mut self, expr:&Expr,env:&LexicalEnvironment)->Result<Type,CommonFactInventoryFailure>{
        match owned_source_shared::expression_type(self.app,expr,env,&mut Some(&mut *self.cx)){
            Ok(value)=>Ok(value),Err(SourceFailure::Allocation(value))=>Err(value.into()),Err(SourceFailure::Contract(value))=>Err(value.into()),
        }
    }
    fn push_use(&mut self, at:&str, kind:&str, ty:Option<&Type>, expr:Option<&Expr>, env:&LexicalEnvironment, entity:Option<&str>, field:Option<&str>, state:Option<&str>, details:Detail<'_>)->Result<(),CommonFactInventoryFailure>{
        let value_type=ty.map(|v|copy_type(v,self.cx)).transpose()?;
        let expression=expr.map(|v|copy_expr(v,self.cx)).transpose()?;
        let detail=crate::product_backup::fallible_codec::try_serialize_value(&details,self.cx).map_err(codec_error)?;
        let use_value=DecodedCommonFactUse{
            side:self.side, source:copy_digest(self.source,self.cx)?, location:copy_string(at,self.cx)?, kind:copy_string(kind,self.cx)?,
            value_type,expression,environment:copy_env(env,self.cx)?,entity:entity.map(|v|copy_string(v,self.cx)).transpose()?,
            field:field.map(|v|copy_string(v,self.cx)).transpose()?,state:state.map(|v|copy_string(v,self.cx)).transpose()?,details:detail,
        };
        push(&mut self.uses,use_value,self.cx)
    }
    fn set_last_state(&mut self,state:&str)->Result<(),CommonFactInventoryFailure>{
        let copied=copy_string(state,self.cx)?;self.uses.last_mut().ok_or_else(||CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: missing just-emitted use".into())))?.state=Some(copied);Ok(())
    }
    fn descriptor(&mut self, at:&str, kind:&str, ty:Option<&Type>, env:&LexicalEnvironment, details:Detail<'_>)->Result<(),CommonFactInventoryFailure>{
        self.push_use(at,kind,ty,None,env,ty.and_then(relation),None,None,details)
    }
    fn write(&mut self, entity:&str, at:&str)->Result<(),CommonFactInventoryFailure>{
        if entity != self.root {
            self.need(at,"shared_effect","Create/update/archive/collection effects on an existing or added non-root relation need exact producer/control/target closure; lifecycle or structural equality grants no shared-write authority")
        } else {
            self.need(at,"cross_root_coupling","Root effects require exact correlation of the recorded target, values, guards, and enclosing selections; same-entity parameters, aliases, or common source do not prove they identify the same root, even without a query")
        }
    }
    fn statements(&mut self, action:&str, steps:crate::product_contract::StatementSlice<'_>, path:&[usize], env:&mut LexicalEnvironment)->Result<(),CommonFactInventoryFailure>{
        for i in 0..steps.len(){
            let mut current=copy_indices(path,self.cx)?;product_alloc::try_push(&mut current,i,self.cx,AllocationSite::Vector)?;
            let at=self.at(format_args!("{action} {current:?}"))?;
            match steps.get(i).expect("bounded statement index") {
                crate::product_contract::StatementView::Create{entity,values,bind}=>{
                    let ty=Type::Reference{entity:copy_string(entity,self.cx)?};self.descriptor(&at,"create",Some(&ty),env,Detail::Create{entity,bind})?;self.write(entity,&at)?;
                    if entity==self.root{self.need(&at,"root_membership","Root Create changes membership and initial legacy/lifecycle values; all hidden producers, scalar input provenance, and retained identity validation remain dependencies")?;}
                    for (field,value) in values.iter(){let loc=self.at(format_args!("{at}.value.{field}"))?;self.expr(value,env,&loc)?;}
                    let typ=Type::Reference{entity:copy_string(entity,self.cx)?};env.insert(bind,&typ,&mut Some(&mut *self.cx)).map_err(source_failure)?;
                }
                crate::product_contract::StatementView::Update{record,values}=>{
                    let ty=self.expression_type(record,env)?;let entity=relation(&ty).ok_or_else(||error(self.cx,format_args!("validated target lacks entity")))?;self.write(entity,&at)?;
                    let loc=self.at(format_args!("{at}.target"))?;self.expr(record,env,&loc)?;
                    for (field,value) in values.iter(){let loc=self.at(format_args!("{at}.value.{field}"))?;self.expr(value,env,&loc)?;}
                }
                crate::product_contract::StatementView::Archive{record}=>{
                    let ty=self.expression_type(record,env)?;let entity=relation(&ty).ok_or_else(||error(self.cx,format_args!("validated target lacks entity")))?;self.write(entity,&at)?;self.need(&at,"membership_mutation","Archival changes active relation membership and invalidates an immutable owner/lifecycle assumption")?;
                    let loc=self.at(format_args!("{at}.target"))?;self.expr(record,env,&loc)?;
                }
                crate::product_contract::StatementView::SetState{state,value}=>{
                    let ty=self.expression_type(value,env)?;self.descriptor(&at,"state_write",Some(&ty),env,Detail::State{state})?;self.set_last_state(state)?;
                    self.need(&at,"reset_closure","A state reset/write needs fresh whole-pair input, ingress, producer, consumer, and failure-order closure; an earlier common-operation certificate does not authorize it")?;
                    let loc=self.at(format_args!("{at}.value"))?;self.expr(value,env,&loc)?;
                }
                crate::product_contract::StatementView::Collection{target,operation,items}=>{
                    self.descriptor(&at,"collection",None,env,Detail::Collection{target,operation})?;
                    match target{
                        CollectionTarget::State{state}=>{self.set_last_state(state)?;self.need(&at,"reset_closure","Collection mutation is a state writer and requires complete input/reference closure")?;}
                        CollectionTarget::Field{record,field}=>{let ty=self.expression_type(record,env)?;let entity=relation(&ty).ok_or_else(||error(self.cx,format_args!("validated collection target lacks entity")))?;self.write(entity,&at)?;let expr=Expr::Field{record:Box::new(copy_expr(record,self.cx)?),field:copy_string(field,self.cx)?};let loc=self.at(format_args!("{at}.target"))?;self.expr(&expr,env,&loc)?;}
                    }
                    let loc=self.at(format_args!("{at}.items"))?;self.expr(items,env,&loc)?;
                }
                crate::product_contract::StatementView::ForEach{items,binding,steps:nested}=>{
                    self.descriptor(&at,"for_each",None,env,Detail::Binding{binding})?;self.need(&at,"phase_reachability","Selection is evaluated once and nested queries see current transactional state in authored iteration order; variant work cannot change common selection/reachability or justify scope pushdown")?;
                    let loc=self.at(format_args!("{at}.items"))?;self.expr(items,env,&loc)?;let typ=item(self.app,items,env,self.cx)?;let mut inner=env.clone_with(&mut Some(&mut *self.cx)).map_err(source_failure)?;inner.insert(binding,&typ,&mut Some(&mut *self.cx)).map_err(source_failure)?;self.statements(action,nested,&current,&mut inner)?;
                }
                crate::product_contract::StatementView::Assert{condition,..}=>{self.need(&at,"global_admission","Ordinary assertions can reject the entire transaction; policy-dependent business rejection is not a resource exception")?;let loc=self.at(format_args!("{at}.condition"))?;self.expr(condition,env,&loc)?;}
                crate::product_contract::StatementView::Emit{output,items,binding,columns}=>{
                    self.descriptor(&at,"emit",None,env,Detail::Emit{output,binding})?;self.need(&at,"output_closure","Stored/completed outputs and ordered emitted rows require separate preservation and identity-use proofs")?;
                    let loc=self.at(format_args!("{at}.items"))?;self.expr(items,env,&loc)?;let typ=item(self.app,items,env,self.cx)?;let mut inner=env.clone_with(&mut Some(&mut *self.cx)).map_err(source_failure)?;inner.insert(binding,&typ,&mut Some(&mut *self.cx)).map_err(source_failure)?;
                    for (column,value) in columns.iter(){let loc=self.at(format_args!("{at}.column.{column}"))?;self.expr(value,&inner,&loc)?;}
                }
            }
        }
        Ok(())
    }
    fn program(&mut self)->Result<(),CommonFactInventoryFailure>{
        let empty=LexicalEnvironment::empty(true);
        for entity in self.app.entities(){
            for field in &entity.fields{let at=self.at(format_args!("entity {} field {}",entity.id,field.id))?;self.descriptor(&at,"schema_field",Some(&field.value_type),&empty,Detail::Schema(field))?;}
            let mut env=LexicalEnvironment::empty(true);let rt=Type::Reference{entity:copy_string(&entity.id,self.cx)?};env.insert("record",&rt,&mut Some(&mut *self.cx)).map_err(source_failure)?;
            for (i,unique) in entity.unique.iter().enumerate(){let at=self.at(format_args!("entity {} unique[{i}]",entity.id))?;self.descriptor(&at,"unique",None,&env,Detail::Unique(unique))?;self.need(&at,"global_uniqueness","Exact uniqueness tuple controls transaction admission and may couple roots; global common keys are not owner partitions")?;}
            for (i,constraint) in entity.constraints.iter().enumerate(){let at=self.at(format_args!("entity {} constraint[{i}]",entity.id))?;self.need(&at,"global_admission","Even unchanged unrelated-entity constraints determine transaction admission; policy-dependent constraints remain unsupported")?;self.expr(constraint,&env,&at)?;}
        }
        for state in self.app.states(){let at=self.at(format_args!("state {} initial",state.id))?;self.descriptor(&at,"state_initial",Some(&state.value_type),&empty,Detail::StateDefinition(state))?;self.set_last_state(&state.id)?;self.reference_input(&at,&state.value_type)?;let expr=Expr::Literal{value_type:copy_type(&state.value_type,self.cx)?,value:product_contract::try_clone_data_value_with(&state.initial,self.cx).map_err(|failure|CommonFactInventoryFailure::Allocation(failure))?};self.expr(&expr,&empty,&at)?;}
        for action in self.app.actions().iter(){
            let mut env=LexicalEnvironment::parameters(action.parameters(),true,&mut Some(&mut *self.cx)).map_err(source_failure)?;
            for (name,ty) in action.parameters().iter(){let at=self.at(format_args!("{} parameter {name}",action.id()))?;self.descriptor(&at,"parameter",Some(ty),&env,Detail::Parameter{action:action.id(),parameter:name})?;self.reference_input(&at,ty)?;}
            for (i,guard) in action.guards().iter().enumerate(){let at=self.at(format_args!("{} guard[{i}]",action.id()))?;self.need(&at,"admission_reachability","Ordinary admission and non-error common-phase reachability must be independent of varying policy state at equal common facts/lifecycle; active-status guards are not lifecycle authority")?;self.expr(guard,&env,&at)?;}
            self.statements(action.id(),action.steps(),&[],&mut env)?;
            for (i,ensure) in action.ensures().iter().enumerate(){let at=self.at(format_args!("{} ensure[{i}]",action.id()))?;self.need(&at,"global_admission","Action ensures can reject the whole transaction; policy-dependent ensures remain unsupported even when unchanged or owner-local")?;self.expr(ensure,&env,&at)?;}
        }
        for output in self.app.outputs(){let at=self.at(format_args!("output {}",output.id))?;self.descriptor(&at,"output_definition",None,&empty,Detail::Output(output))?;}
        for i in 0..self.app.observable_count(){let (id,expr)=self.app.observable(i).expect("bounded observable index");let at=self.at(format_args!("observable {id}"))?;self.expr(expr,&empty,&at)?;}
        for vi in 0..self.app.view_count(){let view=self.app.view(vi).expect("bounded view index");let view_id=view.id();let mut env=LexicalEnvironment::empty(true);
            match view.kind(){
                ViewKindRef::List{entity,rows,columns,controls,selection}=>{let at=self.at(format_args!("view {view_id} rows"))?;self.expr(rows,&env,&at)?;let row=Type::Reference{entity:copy_string(entity,self.cx)?};env.insert("row",&row,&mut Some(&mut *self.cx)).map_err(source_failure)?;
                    for col in columns.iter(){let at=self.at(format_args!("view {view_id} column {}",col.id()))?;self.expr(col.value(),&env,&at)?;}
                    for control in controls.iter(){let at=self.at(format_args!("view {view_id} control {}",control.id()))?;let ty=self.app.state_type(control.state()).ok_or_else(||error(self.cx,format_args!("validated control missing state")))?;self.descriptor(&at,"control",Some(ty),&env,Detail::Control(control))?;self.set_last_state(control.state())?;self.reference_input(&at,ty)?;self.need(&at,"input_argument_closure","Control edits and on-change callbacks are ingress; state validation and automatic argument/target derivation need closure")?;}
                    if let Some(selection)=selection{let at=self.at(format_args!("view {view_id} selection {}",selection.id()))?;let ty=self.app.state_type(selection.state()).ok_or_else(||error(self.cx,format_args!("validated selection missing state")))?;self.descriptor(&at,"selection",Some(ty),&env,Detail::Selection(selection))?;self.set_last_state(selection.state())?;self.reference_input(&at,ty)?;self.need(&at,"input_argument_closure","Explicit user selection is actual input, not equivalent future choices; automatic selection callbacks and state/reference validation require closure")?;}
                }
                ViewKindRef::Detail{entity,record,columns}=>{let at=self.at(format_args!("view {view_id} record"))?;self.expr(record,&env,&at)?;let row=Type::Reference{entity:copy_string(entity,self.cx)?};env.insert("row",&row,&mut Some(&mut *self.cx)).map_err(source_failure)?;for col in columns.iter(){let at=self.at(format_args!("view {view_id} column {}",col.id()))?;self.expr(col.value(),&env,&at)?;}}
                ViewKindRef::Form{action,fields,defaults}=>{let action_def=self.app.action(action).ok_or_else(||error(self.cx,format_args!("validated form action missing")))?;let at=self.at(format_args!("view {view_id} form"))?;self.descriptor(&at,"form",None,&env,Detail::Form{action})?;let params=action_def.parameters();
                    for field in fields.iter(){let at=self.at(format_args!("view {view_id} input {}",field.parameter()))?;let ty=params.get(field.parameter()).ok_or_else(||error(self.cx,format_args!("validated form parameter missing")))?;self.descriptor(&at,"form_input",Some(ty),&env,Detail::FormInput(field))?;self.reference_input(&at,ty)?;}
                    for (name,value) in defaults.iter(){let at=self.at(format_args!("view {view_id} default {name}"))?;let ty=params.get(name).ok_or_else(||error(self.cx,format_args!("validated form default parameter missing")))?;let literal=Expr::Literal{value_type:copy_type(ty,self.cx)?,value:product_contract::try_clone_data_value_with(value,self.cx).map_err(|failure|CommonFactInventoryFailure::Allocation(failure))?};self.expr(&literal,&env,&at)?;self.reference_input(&at,ty)?;}
                }
            }
            for binding in view.actions().iter(){let binding_env=if binding.placement()==product_contract::ActionPlacement::Row{&env}else{&empty};let at=self.at(format_args!("view {view_id} binding {}",binding.id()))?;self.descriptor(&at,"action_binding",None,binding_env,Detail::ActionBinding{action:binding.action(),placement:binding.placement()})?;let enabled_at=self.at(format_args!("{at} enabled"))?;self.need(&enabled_at,"input_argument_closure","Action enabling is an executable ingress condition; exact state, policy, callback, and reference dependencies require closure even for zero-argument actions")?;self.expr(binding.enabled(),binding_env,&enabled_at)?;
                for (name,expr) in binding.arguments().iter(){let at=self.at(format_args!("{at} argument {name}"))?;self.need(&at,"input_argument_closure","Scalar parameter typing does not prove independent user input: policy/state/alias-derived arguments and targets need complete producer/consumer closure")?;self.expr(expr,binding_env,&at)?;}
            }
            for (i,key) in view.keys().iter().enumerate(){let at=self.at(format_args!("view {view_id} key[{i}]"))?;self.descriptor(&at,"key",None,&env,Detail::Key(key))?;}
        }
        Ok(())
    }
    fn reference_input(&mut self,at:&str,ty:&Type)->Result<(),CommonFactInventoryFailure>{if relation(ty).is_some(){self.need(at,"reference_admission","Runtime validates input references, including optional/list wrappers and unused parameters; exact identity and retained reference closure are required")?;}Ok(())}
    fn expr(&mut self, expr:&Expr, env:&LexicalEnvironment, at:&str)->Result<(),CommonFactInventoryFailure>{
        let ty=self.expression_type(expr,env)?;
        let mut retained_type=None;let mut entity=relation(&ty);let mut field=None;let mut state=None;
        let kind=match expr{
            Expr::Literal{..}=>"literal",Expr::Variable{..}=>"variable",
            Expr::Field{record,field:id}=>{retained_type=Some(self.expression_type(record,env)?);entity=relation(retained_type.as_ref().expect("field type retained"));field=Some(id.as_str());"field"},
            Expr::State{state:id}=>{state=Some(id.as_str());"state"},Expr::Today=>"today",
            Expr::Query{entity:id,..}=>{entity=Some(id);"query"},Expr::Filter{..}=>"filter",Expr::Map{..}=>"map",Expr::Count{..}=>"count",Expr::Any{..}=>"any",Expr::All{..}=>"all",Expr::Sum{..}=>"sum",Expr::Contains{..}=>"contains",
            Expr::Equal{left,..}=>if relation(&self.expression_type(left,env)?).is_some(){"reference_equality"}else{"equal"},
            Expr::Less{..}=>"less",Expr::Add{..}=>"add",Expr::Subtract{..}=>"subtract",Expr::Concat{..}=>"concat",Expr::TextContains{..}=>"text_contains",Expr::Lower{..}=>"lower",Expr::And{..}=>"and",Expr::Or{..}=>"or",Expr::Not{..}=>"not",Expr::If{..}=>"if",Expr::Coalesce{..}=>"coalesce",Expr::DateAdd{..}=>"date_add",Expr::DateDifference{..}=>"date_difference",
        };
        self.push_use(at,kind,Some(&ty),Some(expr),env,entity,field,state,Detail::Null)?;
        match expr{
            Expr::Literal{..}|Expr::Variable{..}|Expr::State{..}|Expr::Today=>{},
            Expr::Field{record,..}=>{let at=self.at(format_args!("{at}.record"))?;self.expr(record,env,&at)?;},
            Expr::Query{entity,binding,predicate,sort,..}=>{
                self.need(at,"identity_order","Relation membership, archive inclusion, predicate/error order, ID ordering/final tie-break, and limit are dependencies; even an untruncated loop can have order-sensitive shared writes")?;
                self.need(at,"cross_root_coupling","Query membership and consumers need exact root-correlation and immutable-owner closure; an entity type or a shared common subtree does not confine effects to the named input root")?;
                let mut inner=env.clone_with(&mut Some(&mut *self.cx)).map_err(source_failure)?;let typ=Type::Reference{entity:copy_string(entity,self.cx)?};inner.insert(binding,&typ,&mut Some(&mut *self.cx)).map_err(source_failure)?;
                let at=self.at(format_args!("{at}.predicate"))?;self.expr(predicate,&inner,&at)?;
                for (i,key) in sort.iter().enumerate(){let at=self.at(format_args!("{at}.sort[{i}]"))?;self.expr(&key.value,&inner,&at)?;}
            }
            Expr::Filter{items,binding,predicate}|Expr::Any{items,binding,predicate}|Expr::All{items,binding,predicate}=>{
                let at_items=self.at(format_args!("{at}.items"))?;self.expr(items,env,&at_items)?;let kind=match expr{Expr::Filter{..}=>"filter",Expr::Any{..}=>"any",_=>"all"};let item=item(self.app,items,env,self.cx)?;let mut inner=env.clone_with(&mut Some(&mut *self.cx)).map_err(source_failure)?;inner.insert(binding,&item,&mut Some(&mut *self.cx)).map_err(source_failure)?;let at=self.at(format_args!("{at}.predicate"))?;self.expr(predicate,&inner,&at)?;let _=kind;
            }
            Expr::Map{items,binding,value}=>{let at_items=self.at(format_args!("{at}.items"))?;self.expr(items,env,&at_items)?;let item=item(self.app,items,env,self.cx)?;let mut inner=env.clone_with(&mut Some(&mut *self.cx)).map_err(source_failure)?;inner.insert(binding,&item,&mut Some(&mut *self.cx)).map_err(source_failure)?;let at=self.at(format_args!("{at}.value"))?;self.expr(value,&inner,&at)?;}
            Expr::Count{items}|Expr::Sum{items}=>{let at=self.at(format_args!("{at}.items"))?;self.expr(items,env,&at)?;}
            Expr::Contains{items,value}=>{let a=self.at(format_args!("{at}.items"))?;self.expr(items,env,&a)?;let a=self.at(format_args!("{at}.value"))?;self.expr(value,env,&a)?;}
            Expr::Equal{left,right}|Expr::Less{left,right}|Expr::Add{left,right}|Expr::Subtract{left,right}|Expr::Concat{left,right}=>{let a=self.at(format_args!("{at}.left"))?;self.expr(left,env,&a)?;let a=self.at(format_args!("{at}.right"))?;self.expr(right,env,&a)?;}
            Expr::TextContains{text,search}=>{let a=self.at(format_args!("{at}.text"))?;self.expr(text,env,&a)?;let a=self.at(format_args!("{at}.search"))?;self.expr(search,env,&a)?;}
            Expr::Lower{value}|Expr::Not{value}=>{let a=self.at(format_args!("{at}.value"))?;self.expr(value,env,&a)?;}
            Expr::And{values}|Expr::Or{values}=>for (i,value) in values.iter().enumerate(){let a=self.at(format_args!("{at}.operand[{i}]"))?;self.expr(value,env,&a)?;},
            Expr::If{condition,then_value,else_value}=>{let a=self.at(format_args!("{at}.condition"))?;self.expr(condition,env,&a)?;let a=self.at(format_args!("{at}.then"))?;self.expr(then_value,env,&a)?;let a=self.at(format_args!("{at}.else"))?;self.expr(else_value,env,&a)?;}
            Expr::Coalesce{value,fallback}=>{let a=self.at(format_args!("{at}.value"))?;self.expr(value,env,&a)?;let a=self.at(format_args!("{at}.fallback"))?;self.expr(fallback,env,&a)?;}
            Expr::DateAdd{date,days}=>{let a=self.at(format_args!("{at}.date"))?;self.expr(date,env,&a)?;let a=self.at(format_args!("{at}.days"))?;self.expr(days,env,&a)?;}
            Expr::DateDifference{later,earlier}=>{let a=self.at(format_args!("{at}.later"))?;self.expr(later,env,&a)?;let a=self.at(format_args!("{at}.earlier"))?;self.expr(earlier,env,&a)?;}
        }
        Ok(())
    }
}
fn source_failure(value:SourceFailure)->CommonFactInventoryFailure{match value{SourceFailure::Allocation(v)=>CommonFactInventoryFailure::Allocation(v),SourceFailure::Contract(v)=>CommonFactInventoryFailure::Store(v.into())}}
fn derive_side_uses(app:AppRef<'_>,source:&Digest,side:OwnedSourceSide,root:&str,predicate:&Expr,cx:&mut AllocationContext)->Result<(Vec<DecodedCommonFactUse>,Vec<DecodedCommonFactObligation>,std::ops::Range<usize>),CommonFactInventoryFailure>{
    let mut visitor=UseVisitor{app,source,side,root,uses:Vec::new(),obligations:Vec::new(),cx};
    visitor.program()?;let start=visitor.uses.len();let mut env=LexicalEnvironment::empty(true);
    let ty=Type::Reference{entity:copy_string(root,visitor.cx)?};env.insert("record",&ty,&mut Some(&mut *visitor.cx)).map_err(source_failure)?;
    visitor.expr(predicate,&env,"lifecycle predicate")?;let end=visitor.uses.len();Ok((visitor.uses,visitor.obligations,start..end))
}

fn ordered_identity_hazards(
    structure: &DecodedOwnedStructuralReport,
    cx: &mut AllocationContext,
) -> Result<Vec<DecodedCommonFactIdentityHazard>, CommonFactInventoryFailure> {
    let mut output = Vec::new();
    for side in [OwnedSourceSide::Before, OwnedSourceSide::After] {
        let mut variants = OrderedSet::new();
        for writer in &structure.writers {
            if writer.side == side && writer.kind == OwnedStatementKind::Create && writer_edited(writer, structure) {
                variants.try_insert((copy_string(&writer.action, cx)?, copy_indices(&writer.path, cx)?), cx)
                    .map_err(CommonFactInventoryFailure::Allocation)?;
            }
        }
        for common in structure.common_statements.iter().filter(|s| s.kind == OwnedStatementKind::Create) {
            let path = match side { OwnedSourceSide::Before => &common.before, OwnedSourceSide::After => &common.after };
            for (action, prior) in variants.iter() {
                let repeated = (1..path.len()).any(|n| prior.starts_with(&path[..n]));
                if action == &common.action && (prior < path || repeated) {
                    let writer = structure.writers.iter().find(|w| w.side == side && w.action == common.action && w.path == *path && w.kind == OwnedStatementKind::Create).expect("common create writer");
                    push(&mut output, DecodedCommonFactIdentityHazard {
                        side, action: copy_string(&common.action, cx)?, variant_create: copy_indices(prior, cx)?,
                        common_create: copy_indices(path, cx)?, relation: copy_string(&writer.entity, cx)?,
                        reason: copy_string("Action-wide creation ordinal may differ, including across loop iterations; actual IDs/provenance and every identity/order consumer require closure", cx)?,
                    }, cx)?;
                }
            }
        }
    }
    Ok(output)
}

fn clone_lifecycle(
    value: &super::fallible_owned_request_decode::DecodedCommonFactLifecycleSuggestion,
    cx: &mut AllocationContext,
) -> Result<super::fallible_owned_request_decode::DecodedCommonFactLifecycleSuggestion, CommonFactInventoryFailure> {
    Ok(super::fallible_owned_request_decode::DecodedCommonFactLifecycleSuggestion {
        entity: copy_string(&value.entity, cx)?, before: copy_expr(&value.before, cx)?,
        after: copy_expr(&value.after, cx)?, meaning: copy_string(&value.meaning, cx)?,
    })
}

struct ActionWire<'a>(crate::product_contract::ActionRef<'a>);
impl Serialize for ActionWire<'_>{fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{match self.0{crate::product_contract::ActionRef::Legacy(v)=>v.serialize(s),crate::product_contract::ActionRef::Decoded(v)=>v.serialize(s)}}}
impl crate::product_backup::fallible_codec::BackupWire for ActionWire<'_>{}
fn action_identity(action: crate::product_contract::ActionRef<'_>, cx: &mut AllocationContext) -> Result<Digest, CommonFactInventoryFailure> {
    crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence, &ActionWire(action), cx).map_err(codec_error)
}

/// Recompute the complete diagnostic inventory over canonical fallible owners.
/// The lifecycle token binds this derivation to the exact accepted request and
/// frame, but the output remains diagnostic and grants no admission authority.
pub(crate) fn derive_pair_fallible(
    snapshot: &DecodedProjectSnapshot,
    before: &FallibleDecodedCapturedProgram,
    after: &FallibleDecodedCapturedProgram,
    request: &super::fallible_owned_request_decode::DecodedOwnedPreviewRequest,
    lifecycle: &FallibleAcceptedPreviewLifecycle,
    cx: &mut AllocationContext,
) -> Result<DecodedCommonFactInventory, CommonFactInventoryFailure> {
    let inventory_request = request.inventory();
    let basis = crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Data, snapshot, cx).map_err(codec_error)?;
    if basis != inventory_request.snapshot {
        return Err(error(cx, format_args!("stale authenticated current frame")));
    }
    let before_revision = before.revision_with(cx).map_err(app_error)?;
    let after_revision = after.revision_with(cx).map_err(app_error)?;
    if before_revision != *request.source_before() || after_revision != *request.source_after() {
        return Err(error(cx, format_args!("stale authenticated snapshot/source basis")));
    }
    let request_identity = request.identity_with(cx).map_err(codec_error)?;
    if !lifecycle.matches_inventory_inputs(&basis, &request_identity, &before_revision, &after_revision) {
        return Err(error(cx, format_args!("stale authenticated lifecycle capability")));
    }
    if before.project_id() != after.project_id() || before.project_id() != snapshot.data().project_id() {
        return Err(error(cx, format_args!("source projects differ")));
    }

    let before_app = AppRef::Decoded(before.app());
    let after_app = AppRef::Decoded(after.app());
    let structure = analyze_owned_sources_fallible(before, after, &inventory_request.sources, cx)?;
    if inventory_request.lifecycle.entity != structure.root_entity
        || inventory_request.lifecycle.meaning.trim().is_empty()
        || inventory_request.lifecycle.meaning.len() > MAX_TEXT_BYTES
    {
        return Err(error(cx, format_args!("lifecycle suggestion must name the exact root and a bounded explicit meaning")));
    }
    let mut root_env = LexicalEnvironment::empty(true);
    let root_type = Type::Reference { entity: copy_string(&structure.root_entity, cx)? };
    root_env.insert("record", &root_type, &mut Some(&mut *cx)).map_err(source_failure)?;
    for (app, predicate) in [(before_app, &inventory_request.lifecycle.before), (after_app, &inventory_request.lifecycle.after)] {
        match owned_source_shared::expression_type(app, predicate, &root_env, &mut Some(&mut *cx)) {
            Ok(Type::Boolean) => (),
            Ok(_) => return Err(error(cx, format_args!("lifecycle suggestion is not Boolean"))),
            Err(value) => return Err(source_failure(value)),
        }
        super::compiler::own_row(predicate, "record", &structure.root_entity, app, &LexicalEnvironment::empty(true), false)
            .map_err(CommonFactInventoryFailure::Store)?;
    }

    let (mut uses, mut obligations, origin_before) = derive_side_uses(before_app, &structure.before, OwnedSourceSide::Before, &structure.root_entity, &inventory_request.lifecycle.before, cx)?;
    let (uses_after, obligations_after, origin_after_local) = derive_side_uses(after_app, &structure.after, OwnedSourceSide::After, &structure.root_entity, &inventory_request.lifecycle.after, cx)?;
    let after_offset = uses.len();
    for value in obligations_after { push(&mut obligations, value, cx)?; }
    for value in uses_after { push(&mut uses, value, cx)?; }
    let origins = [origin_before, origin_after_local.start + after_offset..origin_after_local.end + after_offset];

    push(&mut obligations, DecodedCommonFactObligation {
        side: None, location: copy_string("lifecycle", cx)?, code: copy_string("lifecycle_meaning", cx)?,
        reason: copy_string("The source/basis-bound own-row suggestion is untrusted: business meaning, exact admission occurrences, preservation, and completed-history sealing remain unproved", cx)?,
    }, cx)?;
    let mut lifecycle_writers = Vec::new();
    for writer in &structure.writers {
        let lifecycle_field = origins.iter().flat_map(|range| &uses[range.clone()]).any(|use_site| {
            use_site.side == writer.side && use_site.kind == "field"
                && use_site.entity.as_deref() == Some(writer.entity.as_str())
                && use_site.field.as_deref() == writer.field.as_deref()
        });
        if writer.entity == structure.root_entity && (lifecycle_field || writer.kind == OwnedStatementKind::Archive) {
            push(&mut lifecycle_writers, copy_writer(writer, cx)?, cx)?;
            let code = if writer_edited(writer, &structure) { "mixed_lifecycle_transition" } else { "lifecycle_writer" };
            push(&mut obligations, DecodedCommonFactObligation {
                side: Some(writer.side), location: format_owned(cx, format_args!("{} {:?}", writer.action, writer.path))?,
                code: copy_string(code, cx)?, reason: copy_string("Every writer, including unchanged create/begin/complete paths, needs lifecycle classification; an owned span in a mixed action must preserve lifecycle truth, and equal values do not prove sealing", cx)?,
            }, cx)?;
        }
    }

    let mut references = Vec::new();
    for (app, side) in [(before_app, OwnedSourceSide::Before), (after_app, OwnedSourceSide::After)] {
        for entity in app.entities() {
            for field in &entity.fields {
                let Some(target_entity) = relation(&field.value_type) else { continue; };
                let mut producers = Vec::new();
                let mut mutations = Vec::new();
                for writer in &structure.writers {
                    if writer.side == side && writer.entity == entity.id
                        && (writer.field.as_deref() == Some(field.id.as_str()) || writer.kind == OwnedStatementKind::Archive)
                    {
                        if writer.kind == OwnedStatementKind::Create {
                            push(&mut producers, copy_writer(writer, cx)?, cx)?;
                        } else {
                            push(&mut mutations, copy_writer(writer, cx)?, cx)?;
                            push(&mut obligations, DecodedCommonFactObligation {
                                side: Some(side), location: format_owned(cx, format_args!("{} {:?}", writer.action, writer.path))?,
                                code: copy_string("reference_mutation", cx)?, reason: copy_string("Reference reassignment, collection mutation, or archival prevents an immutable-owner proof, including unchanged and Invoke-only actions", cx)?,
                            }, cx)?;
                        }
                    }
                }
                push(&mut obligations, DecodedCommonFactObligation {
                    side: Some(side), location: format_owned(cx, format_args!("entity {} field {}", entity.id, field.id))?,
                    code: copy_string("reference_ownership", cx)?, reason: copy_string("Typed reference is only an immutable-owner candidate: exact Create provenance, aliases, retained rows, uniqueness, and all mutation routes require proof; shared references confer no write authority", cx)?,
                }, cx)?;
                let mut unique = Vec::new();
                for keys in &entity.unique {
                    let mut row = Vec::new();
                    product_alloc::try_reserve(&mut row, keys.len(), cx, AllocationSite::Vector)?;
                    for key in keys { push(&mut row, copy_string(key, cx)?, cx)?; }
                    push(&mut unique, row, cx)?;
                }
                let matches_retained_schema = snapshot.data().schema().iter().find(|value| value.id == entity.id) == Some(entity);
                push(&mut references, DecodedCommonFactReference {
                    side, entity: copy_string(&entity.id, cx)?, field: copy_string(&field.id, cx)?,
                    target_entity: copy_string(target_entity, cx)?, required: matches!(field.value_type, Type::Reference { .. }),
                    matches_retained_schema, unique, producers, mutations,
                }, cx)?;
            }
        }
    }
    for entity in snapshot.data().schema() {
        if before_app.entities().iter().find(|value| value.id == entity.id) != Some(entity)
            || after_app.entities().iter().find(|value| value.id == entity.id) != Some(entity)
        {
            push(&mut obligations, DecodedCommonFactObligation {
                side: None, location: format_owned(cx, format_args!("retained entity {}", entity.id))?,
                code: copy_string("retained_schema", cx)?, reason: copy_string("Cumulative retained schema differs from a source definition; source equality cannot establish runtime compatibility or retained uniqueness", cx)?,
            }, cx)?;
        }
        for (i, _) in entity.unique.iter().enumerate() {
            push(&mut obligations, DecodedCommonFactObligation {
                side: None, location: format_owned(cx, format_args!("retained entity {} unique[{i}]", entity.id))?,
                code: copy_string("retained_uniqueness", cx)?, reason: copy_string("Retained uniqueness is a global transaction admission dependency; owner partitioning must be proved from the exact tuple and retained references", cx)?,
            }, cx)?;
        }
    }

    let mut actions = Vec::new();
    for action in before_app.actions().iter() {
        let candidate = after_app.action(action.id()).ok_or_else(|| error(cx, format_args!("removed action {}", action.id())))?;
        let mut action_phases = Vec::new();
        phases(before_app, after_app, action, candidate, &structure, action.steps(), &[], &mut action_phases, cx)?;
        let mut parameters = super::fallible_scalar_decode::FallibleScopeMap::new();
        for (name, typ) in action.parameters().iter() {
            parameters.insert(copy_string(name, cx)?, copy_type(typ, cx)?, cx).map_err(|value| match value {
                crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(v) => CommonFactInventoryFailure::Allocation(v),
                _ => CommonFactInventoryFailure::Store(StoreError::Invalid("Common fact inventory: parameter copy invariant".into())),
            })?;
        }
        let mut guards = Vec::new();
        for value in action.guards() { push(&mut guards, copy_expr(value, cx)?, cx)?; }
        let mut ensures = Vec::new();
        for value in action.ensures() { push(&mut ensures, copy_expr(value, cx)?, cx)?; }
        push(&mut actions, DecodedCommonFactAction {
            action: copy_string(action.id(), cx)?, parameters, guards, ensures,
            before_identity: action_identity(action, cx)?, after_identity: action_identity(candidate, cx)?, phases: action_phases,
        }, cx)?;
        push(&mut obligations, DecodedCommonFactObligation {
            side: None, location: copy_string(action.id(), cx)?, code: copy_string("resource_atomicity", cx)?,
            reason: copy_string("Common phases must run once in authored order; ordinary fuel/collection/statement/write/output/elapsed/cancellation failures must not commit business/session/output prefixes, while fuel and trace/outcome may change", cx)?,
        }, cx)?;
    }
    let identity_hazards = ordered_identity_hazards(&structure, cx)?;
    for hazard in &identity_hazards {
        push(&mut obligations, DecodedCommonFactObligation {
            side: Some(hazard.side), location: format_owned(cx, format_args!("{} {:?}", hazard.action, hazard.common_create))?,
            code: copy_string("fresh_identity", cx)?, reason: copy_string(&hazard.reason, cx)?,
        }, cx)?;
    }
    let mut retained_schema = Vec::new();
    for value in snapshot.data().schema() { push(&mut retained_schema, copy_entity(value, cx)?, cx)?; }
    let mut before_schema = Vec::new();
    for value in before_app.entities() { push(&mut before_schema, copy_entity(value, cx)?, cx)?; }
    let mut after_schema = Vec::new();
    for value in after_app.entities() { push(&mut after_schema, copy_entity(value, cx)?, cx)?; }
    let retained_data = snapshot.data().identity_with(cx).map_err(app_error)?;
    Ok(DecodedCommonFactInventory {
        snapshot: basis, retained_data, retained_schema, before_schema, after_schema, structure,
        lifecycle: clone_lifecycle(&inventory_request.lifecycle, cx)?, lifecycle_writers, actions, uses, references, identity_hazards, obligations,
        resources: DecodedCommonFactResources {
            total_record_cap: MAX_COLLECTION, stored_records: snapshot.data().record_count(),
            includes_archived_and_auxiliary: true, ordinary_defaults: RuntimeLimits::default(),
            max_query_limit_proves_order_independence: false, failures_commit_fuel_and_trace: true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_contract::*;
    use crate::product_store::{AdoptionReceipt, OperationReceipt, ProjectSnapshot};
    use super::super::common_fact_inventory::{CommonFactInventory, CommonFactInventoryRequest};
    use std::collections::BTreeMap;

    fn app() -> AppDefinition {
        AppDefinition {
            version: CONTRACT_VERSION,
            id: "inventory-app".into(),
            label: "Inventory app".into(),
            entities: vec![EntityDefinition {
                id: "job".into(), label: "Job".into(),
                fields: vec![
                    FieldDefinition { id: "done".into(), label: "Done".into(), value_type: Type::Boolean },
                    FieldDefinition { id: "team".into(), label: "Team".into(), value_type: Type::Optional { item: Box::new(Type::Reference { entity: "team".into() }) } },
                ],
                unique: vec![], constraints: vec![],
            }, EntityDefinition {
                id: "team".into(), label: "Team".into(),
                fields: vec![FieldDefinition { id: "label".into(), label: "Label".into(), value_type: Type::Text }],
                unique: vec![], constraints: vec![],
            }],
            state: vec![],
            actions: vec![ActionDefinition { id: "finish".into(), label: "Finish".into(), parameters: BTreeMap::from([("record".into(), Type::Reference { entity: "job".into() })]), guards: vec![], steps: vec![
                Statement::Assert { condition: Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }, message: "ok".into() },
                Statement::Update { record: Expr::Variable { name: "record".into() }, values: BTreeMap::from([("done".into(), Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } })]) },
                Statement::ForEach { items: Expr::Query { entity: "team".into(), binding: "team_row".into(), predicate: Box::new(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }), sort: vec![], limit: MAX_COLLECTION, include_archived: false }, binding: "team_row".into(), steps: vec![Statement::Assert { condition: Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }, message: "nested".into() }] },
            ], ensures: vec![] }],
            outputs: vec![], observables: vec![],
            views: vec![ViewDefinition { id: "jobs".into(), label: "Jobs".into(), kind: ViewKind::List {
                entity: "job".into(), rows: Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }), sort: vec![], limit: MAX_COLLECTION, include_archived: false },
                columns: vec![], controls: vec![], selection: None,
            }, actions: vec![], keys: vec![] }],
            initial_view: "jobs".into(),
        }
    }
    fn syntax<T: Serialize>(value: &T, cx: &mut AllocationContext) -> Value {
        crate::product_json::parse_json_bytes(&serde_json::to_vec(value).unwrap(), cx).unwrap()
    }
    fn decoded_fixture() -> (ProjectSnapshot, FallibleDecodedCapturedProgram, FallibleDecodedCapturedProgram, super::super::fallible_owned_request_decode::DecodedOwnedPreviewRequest, FallibleAcceptedPreviewLifecycle, CommonFactInventory) {
        let app = app();
        let app_bytes = canonical_bytes(&app).unwrap();
        let before = CapturedProgram::capture(&app_bytes, "inventory-project", Producer::Fixture { name: "inventory".into() }, None).unwrap();
        let after = before.clone();
        let before_revision = crate::product_store::revision(&before).unwrap();
        let snapshot = ProjectSnapshot {
            magic: crate::product_store::MAGIC.into(), version: crate::product_store::FORMAT, revision: 0,
            programs: vec![before.clone()], active_revision: before_revision.clone(),
            data: DataSnapshot::empty("inventory-project", &app).unwrap(),
            session: SessionState::initial(&app).unwrap(), clock_day: 0,
            decisions: DecisionGraph { version: CONTRACT_VERSION, revision: 0, decisions: vec![] },
            adoptions: Vec::<AdoptionReceipt>::new(), operations: BTreeMap::<String, OperationReceipt>::new(), artifacts: vec![],
            scope: super::super::ScopeEnvelope::ScalarV1(super::super::ScopeState::default()),
        };
        let request = CommonFactInventoryRequest {
            snapshot: canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            sources: super::super::OwnedSourceRequest { before: before_revision.clone(), after: crate::product_store::revision(&after).unwrap(), root_entity: "job".into(), edits: vec![] },
            lifecycle: super::super::CommonFactLifecycleSuggestion {
                entity: "job".into(), before: Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "done".into() },
                after: Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "done".into() }, meaning: "lifecycle meaning".into(),
            },
        };
        let legacy = super::super::common_fact_inventory::derive_pair(&snapshot, &before, &after, &request).unwrap();
        let mut cx = AllocationContext::new();
        let decoded_snapshot = crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot, &mut cx), &mut cx).unwrap();
        let decoded_before = decode_captured_program_fallible(syntax(&before, &mut cx), &mut cx).unwrap();
        let decoded_after = decode_captured_program_fallible(syntax(&after, &mut cx), &mut cx).unwrap();
        let preview = serde_json::json!({ "inventory":request,"auxiliary":[],"population":{"kind":"all"},"excluded_records":[],"operations":[],"availability":"nullable_added_boolean","purpose":"test" });
        let decoded_request = super::super::fallible_owned_request_decode::decode_owned_preview_request(syntax(&preview,&mut cx),&mut cx).unwrap();
        let accepted = FallibleAcceptedPreviewLifecycle::test_inventory_binding(&decoded_snapshot,&decoded_before,&decoded_after,&decoded_request,&mut cx).unwrap();
        (snapshot,decoded_before,decoded_after,decoded_request,accepted,legacy)
    }

    #[test]
    fn inventory_bytes_match_legacy_and_each_growth_refusal_is_typed() {
        let (snapshot,before,after,request,accepted,legacy)=decoded_fixture();
        let mut setup=AllocationContext::new();
        let decoded_snapshot=crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot,&mut setup),&mut setup).unwrap();
        let mut cx=AllocationContext::new();
        let decoded=derive_pair_fallible(&decoded_snapshot,&before,&after,&request,&accepted,&mut cx).unwrap();
        let actual=crate::product_backup::fallible_codec::canonical_bytes(&decoded,&mut AllocationContext::new()).unwrap();
        let expected=canonical_bytes(&legacy).unwrap();
        let actual_value=serde_json::from_slice::<serde_json::Value>(&actual).unwrap();
        let expected_value=serde_json::from_slice::<serde_json::Value>(&expected).unwrap();
        if actual_value!=expected_value {for key in ["actions","after_schema","before_schema","identity_hazards","lifecycle","lifecycle_writers","obligations","references","resources","retained_data","retained_schema","snapshot","structure","uses"] {if actual_value.get(key)!=expected_value.get(key){if key=="actions"{eprintln!("INVENTORY_ACTION_PHASE_DIFF actual={} expected={}",actual_value["actions"][0]["phases"],expected_value["actions"][0]["phases"]);}else{eprintln!("INVENTORY_FIELD_DIFF {key}: actual={} expected={}",actual_value.get(key).unwrap(),expected_value.get(key).unwrap());}}}}
        assert_eq!(actual_value,expected_value);
        assert_eq!(actual,expected);
        let before_bytes=crate::product_backup::fallible_codec::canonical_bytes(&decoded_snapshot,&mut AllocationContext::new()).unwrap();
        let growths=cx.growth_requests();assert!(growths>0);
        for n in 1..=growths {
            let mut refusal=AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(n).unwrap(),true);
            assert!(matches!(derive_pair_fallible(&decoded_snapshot,&before,&after,&request,&accepted,&mut refusal),Err(CommonFactInventoryFailure::Allocation(_))),"allocation refusal {n} must remain typed");
        }
        let after_bytes=crate::product_backup::fallible_codec::canonical_bytes(&decoded_snapshot,&mut AllocationContext::new()).unwrap();assert_eq!(before_bytes,after_bytes);
    }

    #[test]
    fn stale_frame_source_and_non_boolean_lifecycle_keep_error_order() {
        let (snapshot,before,after,request,accepted,_)=decoded_fixture();
        let mut changed=snapshot.clone();
        changed.clock_day=changed.clock_day.saturating_add(1);
        let mut cx=AllocationContext::new();
        let changed=crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&changed,&mut cx),&mut cx).unwrap();
        let error=match derive_pair_fallible(&changed,&before,&after,&request,&accepted,&mut cx){Err(error)=>error,Ok(_)=>panic!("stale frame was accepted")};
        assert!(matches!(error,CommonFactInventoryFailure::Store(StoreError::Invalid(message)) if message=="Common fact inventory: stale authenticated current frame"));

        let mut request_json=serde_json::to_value(&request).unwrap();
        request_json["inventory"]["sources"]["before"]=serde_json::json!("0000000000000000000000000000000000000000000000000000000000000000");
        let mut cx=AllocationContext::new();
        let stale_source=super::super::fallible_owned_request_decode::decode_owned_preview_request(syntax(&request_json,&mut cx),&mut cx).unwrap();
        let stale_cap=FallibleAcceptedPreviewLifecycle::test_inventory_binding(&crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot,&mut cx),&mut cx).unwrap(),&before,&after,&stale_source,&mut cx).unwrap();
        let mut cx=AllocationContext::new();
        let snapshot_decoded=crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot,&mut cx),&mut cx).unwrap();
        let error=match derive_pair_fallible(&snapshot_decoded,&before,&after,&stale_source,&stale_cap,&mut cx){Err(error)=>error,Ok(_)=>panic!("stale source was accepted")};
        assert!(matches!(error,CommonFactInventoryFailure::Store(StoreError::Invalid(message)) if message=="Common fact inventory: stale authenticated snapshot/source basis"));

        let mut request_json=serde_json::to_value(&request).unwrap();
        request_json["inventory"]["lifecycle"]["after"]=serde_json::to_value(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value:"not-bool".into() } }).unwrap();
        let mut cx=AllocationContext::new();
        let invalid=super::super::fallible_owned_request_decode::decode_owned_preview_request(syntax(&request_json,&mut cx),&mut cx).unwrap();
        let snapshot_decoded=crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot,&mut cx),&mut cx).unwrap();
        let invalid_cap=FallibleAcceptedPreviewLifecycle::test_inventory_binding(&snapshot_decoded,&before,&after,&invalid,&mut cx).unwrap();
        let mut cx=AllocationContext::new();
        let snapshot_decoded=crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(syntax(&snapshot,&mut cx),&mut cx).unwrap();
        let error=match derive_pair_fallible(&snapshot_decoded,&before,&after,&invalid,&invalid_cap,&mut cx){Err(error)=>error,Ok(_)=>panic!("non-Boolean lifecycle was accepted")};
        assert!(matches!(error,CommonFactInventoryFailure::Store(StoreError::Invalid(message)) if message=="Common fact inventory: lifecycle suggestion is not Boolean"));
    }
}

fn copy_writer(value:&DecodedOwnedWriter,cx:&mut AllocationContext)->Result<DecodedOwnedWriter,CommonFactInventoryFailure>{Ok(DecodedOwnedWriter{side:value.side,source:copy_digest(&value.source,cx)?,action:copy_string(&value.action,cx)?,path:copy_indices(&value.path,cx)?,kind:value.kind,entity:copy_string(&value.entity,cx)?,field:value.field.as_ref().map(|v|copy_string(v,cx)).transpose()?,value_type:value.value_type.as_ref().map(|v|copy_type(v,cx)).transpose()?})}
fn writer_inside(writer:&DecodedOwnedWriter,endpoint:&super::fallible_owned_source_decode::DecodedOwnedSpanEndpoint)->bool{endpoint.action==writer.action&&writer.path.starts_with(&endpoint.list_path)&&writer.path.get(endpoint.list_path.len()).is_some_and(|i|(endpoint.start..endpoint.start+endpoint.len).contains(i))}
fn writer_edited(writer:&DecodedOwnedWriter,structure:&DecodedOwnedStructuralReport)->bool{structure.edits.iter().any(|e|writer_inside(writer,match writer.side{OwnedSourceSide::Before=>&e.before,OwnedSourceSide::After=>&e.after}))}
fn span(app:AppRef<'_>,action:crate::product_contract::ActionRef<'_>,path:&[usize],len:usize,cx:&mut AllocationContext)->Result<DecodedCommonFactSpan,CommonFactInventoryFailure>{
    let (start,parents)=path.split_last().ok_or_else(||error(cx,format_args!("empty phase path")))?;
    let mut steps=action.steps();let mut env=LexicalEnvironment::parameters(action.parameters(),true,&mut Some(&mut *cx)).map_err(source_failure)?;
    let mut controls=Vec::new();product_alloc::try_reserve(&mut controls,action.guards().len().saturating_add(parents.len()),cx,AllocationSite::Vector)?;
    for guard in action.guards(){push(&mut controls,copy_expr(guard,cx)?,cx)?;}
    for index in parents{
        for prior in 0..*index{if let Some(crate::product_contract::StatementView::Create{entity,bind,..})=steps.get(prior){env.reference(bind,entity,&mut Some(&mut *cx)).map_err(source_failure)?;}}
        let Some(crate::product_contract::StatementView::ForEach{items,binding,steps:nested})=steps.get(*index) else{return Err(error(cx,format_args!("phase path is not a validated loop")))};
        let ty=item(app,items,&env,cx)?;env.insert(binding,&ty,&mut Some(&mut *cx)).map_err(source_failure)?;push(&mut controls,copy_expr(items,cx)?,cx)?;steps=nested;
    }
    for prior in 0..*start{if let Some(crate::product_contract::StatementView::Create{entity,bind,..})=steps.get(prior){env.reference(bind,entity,&mut Some(&mut *cx)).map_err(source_failure)?;}}
    let digest=crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence,&CommonSpanIdentityWire("common-fact-span/1",steps.slice_range(*start,len)),cx).map_err(codec_error)?;
    let previous=start.checked_sub(1).map(|i|steps.statement_digest_with(i,cx)).transpose().map_err(codec_error)?;
    let next=steps.get(start.checked_add(len).ok_or_else(||error(cx,format_args!("phase endpoint overflow")))?).map(|_|steps.statement_digest_with(start+len,cx)).transpose().map_err(codec_error)?;
    Ok(DecodedCommonFactSpan{path:copy_indices(path,cx)?,len,digest,environment:copy_env(&env,cx)?,controls,previous,next})
}
fn statement_bytes(steps:crate::product_contract::StatementSlice<'_>,index:usize,cx:&mut AllocationContext)->Result<Vec<u8>,CommonFactInventoryFailure>{let value=steps.get(index).ok_or_else(||error(cx,format_args!("missing validated statement")))?;crate::product_backup::fallible_codec::canonical_bytes(&StatementViewWire(value),cx).map_err(codec_error)}
fn statement_path_bytes(mut steps:crate::product_contract::StatementSlice<'_>,path:&[usize],cx:&mut AllocationContext)->Result<Vec<u8>,CommonFactInventoryFailure>{let (last,parents)=path.split_last().ok_or_else(||error(cx,format_args!("empty statement path")))?;for index in parents{let Some(crate::product_contract::StatementView::ForEach{steps:nested,..})=steps.get(*index)else{return Err(error(cx,format_args!("statement path is not a loop")))};steps=nested;}statement_bytes(steps,*last,cx)}
#[derive(Clone,Copy)]struct StatementViewWire<'a>(crate::product_contract::StatementView<'a>);
impl Serialize for StatementViewWire<'_>{fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{serialize_statement_view(self.0,s)}}
fn serialize_statement_view<S:Serializer>(v:crate::product_contract::StatementView<'_>,s:S)->Result<S::Ok,S::Error>{
    use crate::product_contract::StatementView as V;let mut m=s.serialize_map(None)?;
    match v{
        V::Create{entity,values,bind}=>{m.serialize_entry("kind","create")?;m.serialize_entry("entity",entity)?;m.serialize_entry("values",&StringExprMapWire(values))?;m.serialize_entry("bind",bind)?;}
        V::Update{record,values}=>{m.serialize_entry("kind","update")?;m.serialize_entry("record",record)?;m.serialize_entry("values",&StringExprMapWire(values))?;}
        V::Archive{record}=>{m.serialize_entry("kind","archive")?;m.serialize_entry("record",record)?;}
        V::SetState{state,value}=>{m.serialize_entry("kind","set_state")?;m.serialize_entry("state",state)?;m.serialize_entry("value",value)?;}
        V::Collection{target,operation,items}=>{m.serialize_entry("kind","collection")?;m.serialize_entry("target",target)?;m.serialize_entry("operation",&operation)?;m.serialize_entry("items",items)?;}
        V::ForEach{items,binding,steps}=>{m.serialize_entry("kind","for_each")?;m.serialize_entry("items",items)?;m.serialize_entry("binding",binding)?;m.serialize_entry("steps",&StatementSliceWire(steps))?;}
        V::Assert{condition,message}=>{m.serialize_entry("kind","assert")?;m.serialize_entry("condition",condition)?;m.serialize_entry("message",message)?;}
        V::Emit{output,items,binding,columns}=>{m.serialize_entry("kind","emit")?;m.serialize_entry("output",output)?;m.serialize_entry("items",items)?;m.serialize_entry("binding",binding)?;m.serialize_entry("columns",&StringExprMapWire(columns))?;}
    }m.end()
}
struct StringExprMapWire<'a>(StringMapRef<'a,Expr>);
impl Serialize for StringExprMapWire<'_>{fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{let mut m=s.serialize_map(Some(self.0.len()))?;for(k,v)in self.0.iter(){m.serialize_entry(k,v)?;}m.end()}}
struct StatementSliceWire<'a>(crate::product_contract::StatementSlice<'a>);
impl Serialize for StatementSliceWire<'_>{fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{let mut seq=s.serialize_seq(Some(self.0.len()))?;for i in 0..self.0.len(){seq.serialize_element(&StatementViewWire(self.0.get(i).expect("bounded statement index")))?;}seq.end()}}
struct CommonSpanIdentityWire<'a>(&'static str,crate::product_contract::StatementSlice<'a>);
impl Serialize for CommonSpanIdentityWire<'_>{fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{let mut tuple=s.serialize_tuple(2)?;tuple.serialize_element(self.0)?;tuple.serialize_element(&StatementSliceWire(self.1))?;tuple.end()}}
impl crate::product_backup::fallible_codec::BackupWire for StatementViewWire<'_>{}
impl crate::product_backup::fallible_codec::BackupWire for StatementSliceWire<'_>{}
impl crate::product_backup::fallible_codec::BackupWire for StringExprMapWire<'_>{}
impl crate::product_backup::fallible_codec::BackupWire for CommonSpanIdentityWire<'_>{}

fn phases(before:AppRef<'_>,after:AppRef<'_>,a:crate::product_contract::ActionRef<'_>,b:crate::product_contract::ActionRef<'_>,structure:&DecodedOwnedStructuralReport,steps:crate::product_contract::StatementSlice<'_>,path:&[usize],out:&mut Vec<DecodedCommonFactPhase>,cx:&mut AllocationContext)->Result<(),CommonFactInventoryFailure>{
    let mut index=0;while index<=steps.len(){
        if let Some(edit)=structure.edits.iter().find(|e|e.before.action==a.id()&&e.before.list_path==path&&e.before.start==index){let mut old=copy_indices(&edit.before.list_path,cx)?;product_alloc::try_push(&mut old,edit.before.start,cx,AllocationSite::Vector)?;let mut new=copy_indices(&edit.after.list_path,cx)?;product_alloc::try_push(&mut new,edit.after.start,cx,AllocationSite::Vector)?;push(out,DecodedCommonFactPhase{kind:DecodedCommonFactPhaseKind::Variant,edit:Some(copy_string(&edit.id,cx)?),before:span(before,a,&old,edit.before.len,cx)?,after:span(after,b,&new,edit.after.len,cx)?},cx)?;if edit.before.len>0{index+=edit.before.len;continue}}
        if index==steps.len(){break}let mut current=copy_indices(path,cx)?;product_alloc::try_push(&mut current,index,cx,AllocationSite::Vector)?;
        let common=structure.common_statements.iter().find(|s|s.action==a.id()&&s.before==current).ok_or_else(||error(cx,format_args!("unresolved common phase")))?;
        let nested_variant=structure.edits.iter().any(|e|e.before.action==a.id()&&e.before.list_path.starts_with(&current));
        let complete=statement_bytes(steps,index,cx)?==statement_path_bytes(b.steps(),&common.after,cx)?&&!nested_variant;
        push(out,DecodedCommonFactPhase{kind:if complete{DecodedCommonFactPhaseKind::CommonSubtree}else{DecodedCommonFactPhaseKind::CommonHeader},edit:None,before:span(before,a,&current,1,cx)?,after:span(after,b,&common.after,1,cx)?},cx)?;
        if !complete{let Some(crate::product_contract::StatementView::ForEach{steps:nested,..})=steps.get(index)else{return Err(error(cx,format_args!("different common leaf")))};phases(before,after,a,b,structure,nested,&current,out,cx)?;}index+=1;
    }Ok(())
}
