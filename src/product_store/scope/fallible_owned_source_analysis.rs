//! Allocation-aware rederivation of the exact Owned source structural report.
//! The analyzer borrows decoded captures and stores only bounded report output.
use super::fallible_owned_source_decode::{
    DecodedOwnedCommonStatement, DecodedOwnedRootBinding, DecodedOwnedSourceRequest,
    DecodedOwnedSpanEdit, DecodedOwnedSpanEndpoint, DecodedOwnedStructuralChange,
    DecodedOwnedStructuralReport, DecodedOwnedWriter,
};
use super::owned_contract::{OwnedSourceSide, OwnedStatementKind, OwnedStructuralChangeKind};
use super::owned_source_shared::{self, LexicalEnvironment, SourceFailure};
use crate::product_alloc::{self, ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationFailure, AllocationSite};
use crate::product_contract::{
    self, AppRef, AppValidationError, CollectionTarget, ContractError, Digest, Expr, FallibleDecodedCapturedProgram,
    IdentityDomain, StatementSlice, StatementView, StringValueMap, Type, MAX_AST_NODES, MAX_COLLECTION, MAX_EXPR_DEPTH,
    MAX_ITEMS,
};
use crate::product_json::Value;
use std::fmt;

#[derive(Debug)]
pub(crate) enum SourceAnalysisFailure {
    Contract(ContractError),
    Allocation(AllocationFailure),
}
impl fmt::Display for SourceAnalysisFailure {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {
        match self { Self::Contract(e) => fmt::Display::fmt(e,f), Self::Allocation(e)=>fmt::Display::fmt(e,f) }
    }
}
impl std::error::Error for SourceAnalysisFailure {}
fn error(cx: &mut AllocationContext, message: impl fmt::Display) -> SourceAnalysisFailure {
    let mut text = String::new();
    if let Err(failure) = product_alloc::try_append_fmt(&mut text, format_args!("Owned source structure: "), cx, AllocationSite::Formatting) {
        return match failure { product_alloc::FormatFailure::Allocation(failure) => SourceAnalysisFailure::Allocation(failure), product_alloc::FormatFailure::Formatter => SourceAnalysisFailure::Contract(ContractError(String::new())) };
    }
    if let Err(failure) = product_alloc::try_append_fmt(&mut text, format_args!("{message}"), cx, AllocationSite::Formatting) {
        return match failure { product_alloc::FormatFailure::Allocation(failure) => SourceAnalysisFailure::Allocation(failure), product_alloc::FormatFailure::Formatter => SourceAnalysisFailure::Contract(ContractError(String::new())) };
    }
    SourceAnalysisFailure::Contract(ContractError(text))
}
fn app_error(error: AppValidationError)->SourceAnalysisFailure {
    match error { AppValidationError::Allocation(e)=>SourceAnalysisFailure::Allocation(e), AppValidationError::Contract(e)=>SourceAnalysisFailure::Contract(e) }
}
fn codec_error(value: crate::product_backup::fallible_codec::CodecError, cx: &mut AllocationContext)->SourceAnalysisFailure {
    match value { crate::product_backup::fallible_codec::CodecError::Allocation(e)=>SourceAnalysisFailure::Allocation(e), _=>error(cx, format_args!("canonical encoding invariant")) }
}
fn require(condition:bool, cx:&mut AllocationContext, message:impl fmt::Display)->Result<(),SourceAnalysisFailure>{ if condition {Ok(())} else {Err(error(cx, message))} }

fn copy_string(value:&str,cx:&mut AllocationContext)->Result<String,SourceAnalysisFailure>{
    product_alloc::try_copy_str(value,cx,AllocationSite::String).map_err(SourceAnalysisFailure::Allocation)
}
fn copy_digest(value:&Digest,cx:&mut AllocationContext)->Result<Digest,SourceAnalysisFailure>{
    let text=copy_string(value.as_str(),cx)?;
    Digest::from_owned_lower_hex(text).ok_or_else(|| error(cx, format_args!("digest invariant")))
}
fn push<T>(values:&mut Vec<T>, value:T,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    product_alloc::try_reserve(values,1,cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    values.push(value);Ok(())
}
fn copy_path(path:&[usize],cx:&mut AllocationContext)->Result<Vec<usize>,SourceAnalysisFailure>{
    let mut out=Vec::new();product_alloc::try_reserve(&mut out,path.len(),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;out.extend_from_slice(path);Ok(out)
}
fn copy_type(value:&Type,cx:&mut AllocationContext)->Result<Type,SourceAnalysisFailure>{
    product_contract::try_clone_type_with(value,cx).map_err(SourceAnalysisFailure::Allocation)
}
fn codec_canonical(value:&Value,cx:&mut AllocationContext)->Result<Vec<u8>,SourceAnalysisFailure>{
    crate::product_json::canonical::try_canonical_bytes_bounded(value,crate::product_backup::MAX_BACKUP_BYTES,cx)
        .map_err(|error|match error {crate::product_json::canonical::EncodeError::Allocation(e)=>SourceAnalysisFailure::Allocation(e),_=>self::error(cx, format_args!("canonical encoding invariant"))})
}
fn digest_value(value:&Value,cx:&mut AllocationContext)->Result<Digest,SourceAnalysisFailure>{
    let bytes=codec_canonical(value,cx)?;
    crate::product_backup::fallible_codec::bytes_digest(IdentityDomain::Evidence,&bytes,cx).map_err(|error|codec_error(error,cx))
}

#[derive(Clone,Copy)]
struct Endpoint<'a>{source:&'a Digest,action:&'a str,list_path:&'a[usize],start:usize,len:usize,kinds:&'a[OwnedStatementKind],digest:&'a Digest,previous:Option<&'a Digest>,next:Option<&'a Digest>,root:Option<Root<'a>>}
#[derive(Clone,Copy,PartialEq,Eq)]
struct Root<'a>{name:&'a str,value_type:&'a Type}
impl<'a> From<&'a DecodedOwnedSpanEndpoint> for Endpoint<'a>{
    fn from(v:&'a DecodedOwnedSpanEndpoint)->Self{Self{source:&v.source,action:&v.action,list_path:&v.list_path,start:v.start,len:v.len,kinds:&v.kinds,digest:&v.digest,previous:v.previous.as_ref(),next:v.next.as_ref(),root:v.root.as_ref().map(|r|Root{name:&r.name,value_type:&r.value_type})}}
}
fn overlaps(a:Endpoint<'_>,b:Endpoint<'_>)->bool{
    if a.action!=b.action{return false}
    if a.list_path==b.list_path{return a.start==b.start||(a.start<b.start&&b.start<a.start+a.len)||(b.start<a.start&&a.start<b.start+b.len)}
    if b.list_path.starts_with(a.list_path){let child=b.list_path[a.list_path.len()];return a.start<=child&&child<a.start+a.len}
    if a.list_path.starts_with(b.list_path){return overlaps(b,a)} false
}
struct Resolved<'a>{steps:StatementSlice<'a>,env:LexicalEnvironment}
fn extend_creates(steps:StatementSlice<'_>,env:&mut LexicalEnvironment,cx:&mut Option<&mut AllocationContext>)->Result<(),SourceFailure>{
    for i in 0..steps.len(){if let StatementView::Create{entity,bind,..}=steps.get(i).unwrap(){env.reference(bind,entity,cx)?;}}
    Ok(())
}
fn source_failure(value:SourceFailure)->SourceAnalysisFailure{match value{SourceFailure::Allocation(e)=>SourceAnalysisFailure::Allocation(e),SourceFailure::Contract(e)=>SourceAnalysisFailure::Contract(e)}}
fn resolve<'a>(app:AppRef<'a>,identity:&Digest,root_entity:&str,endpoint:Endpoint<'_>,cx:&mut AllocationContext)->Result<Resolved<'a>,SourceAnalysisFailure>{
    require(endpoint.source==identity,cx,format_args!("stale source at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    require(endpoint.list_path.len()<=MAX_EXPR_DEPTH&&endpoint.len<=MAX_ITEMS,cx,format_args!("span bounds exceeded at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    let action=app.action(endpoint.action).ok_or_else(||error(cx, format_args!("unknown action at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start)))?;
    let mut steps=action.steps();
    let mut mode=Some(&mut *cx);
    let mut env=LexicalEnvironment::parameters(action.parameters(),app.is_fallible(),&mut mode).map_err(source_failure)?;
    for index in endpoint.list_path {
        let step=match steps.get(*index) { Some(step)=>step, None=>return Err(error(mode.as_deref_mut().expect("resolver allocation context"), format_args!("missing list path at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))) };
        extend_creates(StatementSlice::slice_prefix(steps,*index),&mut env,&mut mode).map_err(source_failure)?;
        let StatementView::ForEach{items,binding,steps:nested}=step else{return Err(error(cx, format_args!("list path is not a ForEach at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start)))};
        let item=owned_source_shared::list_item(app,items,&env,&mut mode).map_err(source_failure)?;
        env.insert(binding,&item,&mut mode).map_err(source_failure)?;
        steps=nested;
    }
    let end=match endpoint.start.checked_add(endpoint.len).filter(|end|*end<=steps.len()) { Some(end)=>end, None=>return Err(error(mode.as_deref_mut().expect("resolver allocation context"), format_args!("span outside statement list at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))) };
    extend_creates(StatementSlice::slice_range(steps,0,endpoint.start),&mut env,&mut mode).map_err(source_failure)?;
    require(endpoint.kinds.len()==endpoint.len&& (endpoint.start..end).zip(endpoint.kinds.iter()).all(|(index,kind)|statement_kind(steps.get(index).expect("checked span index"))==*kind),cx,format_args!("statement kind mismatch at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    let digest=steps.span_digest_with(endpoint.start,endpoint.len,cx).map_err(|error|codec_error(error,cx))?;
    require(&digest==endpoint.digest,cx,format_args!("span digest mismatch at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    let previous=endpoint.start.checked_sub(1).map(|i|steps.statement_digest_with(i,cx)).transpose().map_err(|error|codec_error(error,cx))?;
    let next=steps.get(end).map(|_|steps.statement_digest_with(end,cx)).transpose().map_err(|error|codec_error(error,cx))?;
    require(previous.as_ref()==endpoint.previous&&next.as_ref()==endpoint.next,cx,format_args!("neighbor anchor mismatch at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    if let Some(binding)=endpoint.root{
        let expected=Type::Reference{entity:copy_string(root_entity,cx)?};
        require(binding.value_type==&expected&&env.get(binding.name)==Some(binding.value_type),cx,format_args!("root binder or type mismatch at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
    }
    let tail=StatementSlice::slice_range(steps,end,steps.len()-end);
    let ensures=if endpoint.list_path.is_empty(){action.ensures()}else{&[]};
    for index in endpoint.start..end {
        if let StatementView::Create{bind,..}=steps.get(index).unwrap(){
            require(!owned_source_shared::statements_contain_variable(tail,bind)&&!ensures.iter().any(|e|owned_source_shared::expression_contains_variable(e,bind)),cx,format_args!("Create binder {bind} escapes span at {} {:?} at {}",endpoint.action,endpoint.list_path,endpoint.start))?;
        }
    }
    Ok(Resolved{steps:StatementSlice::slice_range(steps,endpoint.start,endpoint.len),env})
}
fn statement_kind(view:StatementView<'_>)->OwnedStatementKind{match view{
    StatementView::Create{..}=>OwnedStatementKind::Create,StatementView::Update{..}=>OwnedStatementKind::Update,StatementView::Archive{..}=>OwnedStatementKind::Archive,StatementView::SetState{..}=>OwnedStatementKind::SetState,StatementView::Collection{..}=>OwnedStatementKind::Collection,StatementView::ForEach{..}=>OwnedStatementKind::ForEach,StatementView::Assert{..}=>OwnedStatementKind::Assert,StatementView::Emit{..}=>OwnedStatementKind::Emit,
}}
fn endpoints_for<'a>(edits:&'a [DecodedOwnedSpanEdit],side:OwnedSourceSide,action:&str,cx:&mut AllocationContext)->Result<Vec<&'a DecodedOwnedSpanEdit>,SourceAnalysisFailure>{
    let mut out=Vec::new();
    product_alloc::try_reserve(&mut out,edits.len(),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    for edit in edits {
        let endpoint=match side {OwnedSourceSide::Before=>&edit.before,OwnedSourceSide::After=>&edit.after};
        if endpoint.action==action {push(&mut out,edit,cx)?;}
    }
    Ok(out)
}

fn clone_endpoint(src:&DecodedOwnedSpanEndpoint,cx:&mut AllocationContext)->Result<DecodedOwnedSpanEndpoint,SourceAnalysisFailure>{
    let mut path=copy_path(&src.list_path,cx)?; let mut kinds=Vec::new();product_alloc::try_reserve(&mut kinds,src.kinds.len(),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;kinds.extend_from_slice(&src.kinds);
    Ok(DecodedOwnedSpanEndpoint{source:copy_digest(&src.source,cx)?,action:copy_string(&src.action,cx)?,list_path:path,start:src.start,len:src.len,kinds,digest:copy_digest(&src.digest,cx)?,previous:src.previous.as_ref().map(|v|copy_digest(v,cx)).transpose()?,next:src.next.as_ref().map(|v|copy_digest(v,cx)).transpose()?,root:src.root.as_ref().map(|v|Ok(DecodedOwnedRootBinding{name:copy_string(&v.name,cx)?,value_type:copy_type(&v.value_type,cx)?})).transpose()?})
}

pub(crate) fn analyze_owned_sources_fallible(
    before:&FallibleDecodedCapturedProgram,after:&FallibleDecodedCapturedProgram,request:&DecodedOwnedSourceRequest,cx:&mut AllocationContext,
)->Result<DecodedOwnedStructuralReport,SourceAnalysisFailure>{
    before.validate_with(cx).map_err(app_error)?;
    after.validate_with(cx).map_err(app_error)?;
    let before_id=before.revision_with(cx).map_err(app_error)?;let after_id=after.revision_with(cx).map_err(app_error)?;
    require(&request.before==&before_id&&&request.after==&after_id,cx,"stale exact source identity")?;
    require(before.project_id()==after.project_id(),cx,"source projects differ")?;
    let old_app=AppRef::Decoded(before.app());let new_app=AppRef::Decoded(after.app());
    require(old_app.entities().iter().any(|e|e.id==request.root_entity)&&new_app.entities().iter().any(|e|e.id==request.root_entity),cx,"root entity is missing")?;
    require(request.edits.len()<=MAX_ITEMS,cx,"too many edit spans")?;
    let mut ids=OrderedSet::new();
    for (index,edit) in request.edits.iter().enumerate(){
        let key=copy_string(&edit.id,cx)?;
        require(product_contract::valid_id(&edit.id)&&ids.try_insert(key,cx).map_err(SourceAnalysisFailure::Allocation)?,cx,"invalid or duplicate edit ID")?;
        let old=Endpoint::from(&edit.before);let new=Endpoint::from(&edit.after);
        require(old.action==new.action&&(old.len>0||new.len>0),cx,format_args!("cross-action or empty correspondence {}",edit.id))?;
        let old_resolved=resolve(old_app,&request.before,&request.root_entity,old,cx)?;
        let new_resolved=resolve(new_app,&request.after,&request.root_entity,new,cx)?;
        require(old_resolved.env.equivalent(&new_resolved.env)&&old.root==new.root,cx,format_args!("unsupported author binder correspondence in {}",edit.id))?;
        if !old_resolved.steps.is_empty()&&!new_resolved.steps.is_empty(){
            let mut mode=Some(&mut *cx);
            require(owned_source_shared::correspond(old_app,old_resolved.steps,new_app,new_resolved.steps,&old_resolved.env,&mut mode).map_err(source_failure)?,cx,format_args!("unsupported typed lexical binder correspondence in {}",edit.id))?;
        }
        for prior in &request.edits[..index]{require(!overlaps(Endpoint::from(&prior.before),old)&&!overlaps(Endpoint::from(&prior.after),new),cx,format_args!("overlapping edit spans {} and {}",prior.id,edit.id))?;}
    }
    // Build the ordered common-statement map using the same typed loop-header
    // comparison as the legacy resolver, with every path and output reserved.
    let mut common_statements=Vec::new();
    product_alloc::try_reserve(&mut common_statements,0,cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    for action in old_app.actions().iter(){
        let candidate=new_app.action(action.id()).ok_or_else(||error(cx, format_args!("removed action {}",action.id())))?;
        let old_endpoints=endpoints_for(&request.edits,OwnedSourceSide::Before,action.id(),cx)?;
        let new_endpoints=endpoints_for(&request.edits,OwnedSourceSide::After,action.id(),cx)?;
        compare_statement_lists(action.id(),action.steps(),candidate.steps(),&[],&old_endpoints,&new_endpoints,&mut common_statements,cx)?;
    }
    let changes=passive_changes(old_app,new_app,cx)?;
    let mut writers=Vec::new();
    for (app,source,side) in [(old_app,&request.before,OwnedSourceSide::Before),(new_app,&request.after,OwnedSourceSide::After)]{
        for action in app.actions().iter(){
            let mut mode=if app.is_fallible(){Some(&mut *cx)}else{None};
            let mut env=LexicalEnvironment::parameters(action.parameters(),app.is_fallible(),&mut mode).map_err(source_failure)?;
            writers_for(app,source,side,action.id(),action.steps(),&[],&mut env,&mut writers,cx)?;
        }
    }
    let mut edits=Vec::new();product_alloc::try_reserve(&mut edits,request.edits.len(),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    for edit in &request.edits{push(&mut edits,DecodedOwnedSpanEdit{id:copy_string(&edit.id,cx)?,before:clone_endpoint(&edit.before,cx)?,after:clone_endpoint(&edit.after,cx)?},cx)?;}
    Ok(DecodedOwnedStructuralReport{before:copy_digest(&request.before,cx)?,after:copy_digest(&request.after,cx)?,root_entity:copy_string(&request.root_entity,cx)?,edits,common_statements,writers,changes})
}

struct Tree<'a,'e>{path:Vec<usize>,kind:OwnedStatementKind,header:super::owned_resolve::StatementHeader<'a>,body:Vec<Node<'a,'e>>}
enum Node<'a,'e>{Slot(&'e str),Common(Tree<'a,'e>)}
fn skeleton<'a,'e>(steps:StatementSlice<'a>,path:&[usize],side:OwnedSourceSide,edits:&'e [&'e DecodedOwnedSpanEdit],cx:&mut AllocationContext)->Result<Vec<Node<'a,'e>>,SourceAnalysisFailure>{
    let mut out=Vec::new();
    let cap=steps.len().checked_add(edits.len()).ok_or_else(||error(cx, "statement skeleton size overflow"))?;
    require(cap<=MAX_AST_NODES,cx,"statement skeleton exceeds bound")?;
    product_alloc::try_reserve(&mut out,cap,cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    let mut index=0;
    while index<=steps.len(){
        if let Some(edit)=edits.iter().find(|e|{let endpoint=match side{OwnedSourceSide::Before=>&e.before,OwnedSourceSide::After=>&e.after};endpoint.list_path==path&&endpoint.start==index}){
            push(&mut out,Node::Slot(&edit.id),cx)?;
            let endpoint=match side{OwnedSourceSide::Before=>&edit.before,OwnedSourceSide::After=>&edit.after};
            if endpoint.len>0{index+=endpoint.len;continue}
        }
        if index==steps.len(){break}
        let view=steps.get(index).expect("bounded statement index");
        let mut current=copy_path(path,cx)?;push(&mut current,index,cx)?;
        let (kind,body)=match view{
            StatementView::Create{..}=>(OwnedStatementKind::Create,Vec::new()),StatementView::Update{..}=>(OwnedStatementKind::Update,Vec::new()),StatementView::Archive{..}=>(OwnedStatementKind::Archive,Vec::new()),StatementView::SetState{..}=>(OwnedStatementKind::SetState,Vec::new()),StatementView::Collection{..}=>(OwnedStatementKind::Collection,Vec::new()),StatementView::ForEach{steps:nested,..}=>(OwnedStatementKind::ForEach,skeleton(nested,&current,side,edits,cx)?),StatementView::Assert{..}=>(OwnedStatementKind::Assert,Vec::new()),StatementView::Emit{..}=>(OwnedStatementKind::Emit,Vec::new()),
        };
        push(&mut out,Node::Common(Tree{path:current,kind,header:super::owned_resolve::statement_header(view),body}),cx)?;
        index+=1;
    }
    Ok(out)
}
fn match_skeleton(action:&str,before:&[Node<'_,'_>],after:&[Node<'_,'_>],out:&mut Vec<DecodedOwnedCommonStatement>,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    require(before.len()==after.len(),cx,format_args!("uncovered or reordered statements in action {action}"))?;
    for (a,b) in before.iter().zip(after){match(a,b){
        (Node::Slot(a),Node::Slot(b)) if a==b=>{},
        (Node::Common(a),Node::Common(b)) if a.header==b.header=>{
            push(out,DecodedOwnedCommonStatement{action:copy_string(action,cx)?,before:copy_path(&a.path,cx)?,after:copy_path(&b.path,cx)?,kind:a.kind},cx)?;
            match_skeleton(action,&a.body,&b.body,out,cx)?;
        }
        _=>return Err(error(cx, format_args!("uncovered or reordered statement/binder in action {action}"))),
    }} Ok(())
}
fn compare_statement_lists(action:&str,before:StatementSlice<'_>,after:StatementSlice<'_>,path:&[usize],old:&[&DecodedOwnedSpanEdit],new:&[&DecodedOwnedSpanEdit],out:&mut Vec<DecodedOwnedCommonStatement>,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    let old=skeleton(before,path,OwnedSourceSide::Before,old,cx)?;let new=skeleton(after,path,OwnedSourceSide::After,new,cx)?;match_skeleton(action,&old,&new,out,cx)
}

fn get<'a>(value:&'a Value,key:&str)->Option<&'a Value>{match value{Value::Object(fields)=>fields.get(key),_=>None}}
fn get_mut<'a>(value:&'a mut Value,key:&str)->Option<&'a mut Value>{match value{Value::Object(fields)=>fields.get_mut(key),_=>None}}
fn string_value(value:&Value)->Option<&str>{match value{Value::String(s)=>Some(s),_=>None}}
fn array(value:&Value)->Option<&[Value]>{match value{Value::Array(v)=>Some(v),_=>None}}
fn array_mut(value:&mut Value)->Option<&mut Vec<Value>>{match value{Value::Array(v)=>Some(v),_=>None}}
fn id_field(value:&Value)->Option<&str>{get(value,"id").and_then(string_value)}
fn try_clone_json(value:&Value,cx:&mut AllocationContext)->Result<Value,SourceAnalysisFailure>{Ok(match value{
    Value::Null=>Value::Null,Value::Bool(v)=>Value::Bool(*v),Value::Number(v)=>Value::Number(match v{crate::product_json::Number::U64(v)=>crate::product_json::Number::U64(*v),crate::product_json::Number::I64(v)=>crate::product_json::Number::I64(*v),crate::product_json::Number::F64(v)=>crate::product_json::Number::F64(*v)}),
    Value::String(v)=>Value::String(copy_string(v,cx)?),
    Value::Array(values)=>{require(values.len()<=MAX_AST_NODES,cx,"serialized source tree exceeds bound")?;let mut out=Vec::new();product_alloc::try_reserve(&mut out,values.len(),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;for value in values{push(&mut out,try_clone_json(value,cx)?,cx)?;}Value::Array(out)},
    Value::Object(values)=>{require(values.len()<=MAX_COLLECTION,cx,"serialized source map exceeds bound")?;let mut out=OrderedMap::new();out.try_reserve(values.len(),cx).map_err(SourceAnalysisFailure::Allocation)?;for(key,value)in values.iter(){out.try_insert(copy_string(key,cx)?,try_clone_json(value,cx)?,cx).map_err(SourceAnalysisFailure::Allocation)?;}Value::Object(out)},
})}
fn find_array_item<'a>(values:&'a[Value],id:&str)->Option<&'a Value>{values.iter().find(|value|id_field(value)==Some(id))}
fn view_kind_columns_mut(value:&mut Value)->Option<&mut Vec<Value>>{let kind=get_mut(value,"kind")?;match get(kind,"kind").and_then(string_value)?{"list"|"detail"=>array_mut(get_mut(kind,"columns")?),_=>None}}
fn view_kind_columns(value:&Value)->Option<&[Value]>{let kind=get(value,"kind")?;match get(kind,"kind").and_then(string_value)?{"list"|"detail"=>array(get(kind,"columns")?),_=>None}}
fn canonical_equal(a:&Value,b:&Value,cx:&mut AllocationContext)->Result<bool,SourceAnalysisFailure>{Ok(codec_canonical(a,cx)?==codec_canonical(b,cx)?)}
fn normalize_actions(value:&mut Value){if let Some(actions)=get_mut(value,"actions").and_then(array_mut){for action in actions{if let Value::Object(fields)=action{fields.remove("steps");}}}}
fn normalize_columns(shell_view:&mut Value,before_view:&Value,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    let Some(after_columns)=view_kind_columns(shell_view) else{return Ok(())};
    let Some(old_columns)=view_kind_columns(before_view) else{return Ok(())};
    let mut retained=Vec::new();product_alloc::try_reserve(&mut retained,after_columns.len().min(old_columns.len()),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    for after in after_columns {if let Some(id)=id_field(after){if let Some(old)=find_array_item(old_columns,id){push(&mut retained,try_clone_json(old,cx)?,cx)?;}}}
    if let Some(columns)=view_kind_columns_mut(shell_view){*columns=retained;}
    Ok(())
}
fn normalize_observables(shell:&mut Value,before:&Value,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    let Some(after_values)=get(shell,"observables").and_then(array) else{return Ok(())};
    let Some(old_values)=get(before,"observables").and_then(array) else{return Ok(())};
    let mut retained=Vec::new();product_alloc::try_reserve(&mut retained,after_values.len().min(old_values.len()),cx,AllocationSite::Vector).map_err(SourceAnalysisFailure::Allocation)?;
    for after in after_values {if let Some(id)=id_field(after){if let Some(old)=find_array_item(old_values,id){push(&mut retained,try_clone_json(old,cx)?,cx)?;}}}
    if let Some(values)=get_mut(shell,"observables").and_then(array_mut){*values=retained;}
    Ok(())
}
fn normalize_entities(shell:&mut Value,before:&Value){
    let old_entities=get(before,"entities").and_then(array).unwrap_or(&[]);
    if let Some(entities)=get_mut(shell,"entities").and_then(array_mut){entities.retain(|entity|id_field(entity).is_some_and(|id|find_array_item(old_entities,id).is_some()));}
}
fn normalize_views(shell:&mut Value,before:&Value,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    let count=get(shell,"views").and_then(array).map_or(0,|v|v.len());
    let old_views=get(before,"views").and_then(array).unwrap_or(&[]);
    for index in 0..count {
        let id=get(shell,"views").and_then(array).and_then(|v|v.get(index)).and_then(id_field).ok_or_else(||error(cx, "view identity missing"))?;
        let Some(old)=find_array_item(old_views,id) else{continue};
        if let Some(view)=get_mut(shell,"views").and_then(array_mut).and_then(|v|v.get_mut(index)){normalize_columns(view,old,cx)?;}
    }
    Ok(())
}
pub(super) fn passive_changes(before:AppRef<'_>,after:AppRef<'_>,cx:&mut AllocationContext)->Result<Vec<DecodedOwnedStructuralChange>,SourceAnalysisFailure>{
    let before_id=before.id().ok_or_else(||error(cx, "missing app identity"))?;
    let mut before_tree=match before {AppRef::Legacy(app)=>crate::product_backup::fallible_codec::try_serialize_value(app,cx),AppRef::Decoded(app)=>crate::product_backup::fallible_codec::try_serialize_value(app,cx),AppRef::Snapshot=>return Err(error(cx, "missing app"))}.map_err(|error|codec_error(error,cx))?;
    let mut after_tree=match after {AppRef::Legacy(app)=>crate::product_backup::fallible_codec::try_serialize_value(app,cx),AppRef::Decoded(app)=>crate::product_backup::fallible_codec::try_serialize_value(app,cx),AppRef::Snapshot=>return Err(error(cx, "missing app"))}.map_err(|error|codec_error(error,cx))?;
    let mut changes=Vec::new();
    let old_label=get(&before_tree,"label").ok_or_else(||error(cx, "missing app label"))?;
    let new_label=get(&after_tree,"label").ok_or_else(||error(cx, "missing app label"))?;
    if !canonical_equal(old_label,new_label,cx)?{
        push(&mut changes,DecodedOwnedStructuralChange{kind:OwnedStructuralChangeKind::ApplicationLabelChanged,container:None,id:copy_string(before_id,cx)?,before:Some(digest_value(old_label,cx)?),after:digest_value(new_label,cx)?,before_type:None,after_type:None},cx)?;
    }
    let old_entities=get(&before_tree,"entities").and_then(array).unwrap_or(&[]);
    let new_entities=get(&after_tree,"entities").and_then(array).unwrap_or(&[]);
    for entity in new_entities {let id=id_field(entity).ok_or_else(||error(cx, "entity identity missing"))?;if find_array_item(old_entities,id).is_none(){push(&mut changes,DecodedOwnedStructuralChange{kind:OwnedStructuralChangeKind::EntityAdded,container:None,id:copy_string(id,cx)?,before:None,after:digest_value(entity,cx)?,before_type:None,after_type:None},cx)?;}}
    let old_views=get(&before_tree,"views").and_then(array).unwrap_or(&[]);
    let new_views=get(&after_tree,"views").and_then(array).unwrap_or(&[]);
    for new_view in new_views {let view_id=id_field(new_view).ok_or_else(||error(cx, "view identity missing"))?;let Some(old_view)=find_array_item(old_views,view_id) else{continue};
        let (Some(old_columns),Some(new_columns))=(view_kind_columns(old_view),view_kind_columns(new_view)) else{continue};
        for new_column in new_columns {let column_id=id_field(new_column).ok_or_else(||error(cx, "column identity missing"))?;let old_column=find_array_item(old_columns,column_id);
            if let Some(old)=old_column {if canonical_equal(old,new_column,cx)?{continue}}
            let before_type: Option<Type>=if let Some((entity,expr))=before.column_expression(view_id,column_id){let mut mode=if before.is_fallible(){Some(&mut *cx)}else{None};let mut env=LexicalEnvironment::empty(before.is_fallible());env.reference("row",entity,&mut mode).map_err(source_failure)?;let typ=owned_source_shared::expression_type(before,expr,&env,&mut mode).map_err(source_failure)?;Some(typ)}else{None};
            let after_type=if let Some((entity,expr))=after.column_expression(view_id,column_id){let mut mode=if after.is_fallible(){Some(&mut *cx)}else{None};let mut env=LexicalEnvironment::empty(after.is_fallible());env.reference("row",entity,&mut mode).map_err(source_failure)?;Some(owned_source_shared::expression_type(after,expr,&env,&mut mode).map_err(source_failure)?)}else{None};
            push(&mut changes,DecodedOwnedStructuralChange{kind:if old_column.is_some(){OwnedStructuralChangeKind::ColumnChanged}else{OwnedStructuralChangeKind::ColumnAdded},container:Some(copy_string(view_id,cx)?),id:copy_string(column_id,cx)?,before:old_column.map(|v|digest_value(v,cx)).transpose()?,after:digest_value(new_column,cx)?,before_type:before_type.map(|v|copy_type(&v,cx)).transpose()?,after_type:after_type.map(|v|copy_type(&v,cx)).transpose()?},cx)?;
        }
    }
    let old_observables=get(&before_tree,"observables").and_then(array).unwrap_or(&[]);let new_observables=get(&after_tree,"observables").and_then(array).unwrap_or(&[]);
    for new_observable in new_observables {let id=id_field(new_observable).ok_or_else(||error(cx, "observable identity missing"))?;let old=find_array_item(old_observables,id);
        if let Some(value)=old {if canonical_equal(value,new_observable,cx)?{continue}}
        let old_type=(0..before.observable_count()).find_map(|index|before.observable(index).filter(|(observable,_)|*observable==id).map(|(_,expr)|expr)).map(|expr|{let mut mode=if before.is_fallible(){Some(&mut *cx)}else{None};let env=LexicalEnvironment::empty(before.is_fallible());owned_source_shared::expression_type(before,expr,&env,&mut mode).map_err(source_failure)}).transpose()?;
        let new_type=(0..after.observable_count()).find_map(|index|after.observable(index).filter(|(observable,_)|*observable==id).map(|(_,expr)|expr)).map(|expr|{let mut mode=if after.is_fallible(){Some(&mut *cx)}else{None};let env=LexicalEnvironment::empty(after.is_fallible());owned_source_shared::expression_type(after,expr,&env,&mut mode).map_err(source_failure)}).transpose()?;
        push(&mut changes,DecodedOwnedStructuralChange{kind:if old.is_some(){OwnedStructuralChangeKind::ObservableChanged}else{OwnedStructuralChangeKind::ObservableAdded},container:None,id:copy_string(id,cx)?,before:old.map(|v|digest_value(v,cx)).transpose()?,after:digest_value(new_observable,cx)?,before_type:old_type.map(|v|copy_type(&v,cx)).transpose()?,after_type:new_type.map(|v|copy_type(&v,cx)).transpose()?},cx)?;
    }
    if let Value::Object(fields)=&mut before_tree{fields.remove("label");} if let Value::Object(fields)=&mut after_tree{fields.remove("label");}
    normalize_entities(&mut after_tree,&before_tree);
    normalize_views(&mut after_tree,&before_tree,cx)?;
    normalize_observables(&mut after_tree,&before_tree,cx)?;
    normalize_actions(&mut before_tree);normalize_actions(&mut after_tree);
    require(canonical_equal(&before_tree,&after_tree,cx)?,cx,"unclassified schema, action header, view, output, state, or ordering change")?;
    Ok(changes)
}

fn push_writer(source:&Digest,side:OwnedSourceSide,action:&str,path:&[usize],kind:OwnedStatementKind,entity:&str,field:Option<&str>,definition:&crate::product_contract::EntityDefinition,out:&mut Vec<DecodedOwnedWriter>,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    let value_type=field.map(|id|definition.fields.iter().find(|f|f.id==id).map(|f|&f.value_type).ok_or_else(||error(cx, "writer field missing"))).transpose()?;
    push(out,DecodedOwnedWriter{side,source:copy_digest(source,cx)?,action:copy_string(action,cx)?,path:copy_path(path,cx)?,kind,entity:copy_string(entity,cx)?,field:field.map(|v|copy_string(v,cx)).transpose()?,value_type:value_type.map(|v|copy_type(v,cx)).transpose()?},cx)
}
fn entity_from_expression(app:AppRef<'_>,record:&Expr,env:&LexicalEnvironment,cx:&mut AllocationContext)->Result<String,SourceAnalysisFailure>{
    match owned_source_shared::expression_type(app,record,env,&mut Some(cx)).map_err(source_failure)?{
        Type::Reference{entity}=>Ok(entity),_=>Err(error(cx, "writer target is not a typed record")),
    }
}
pub(super) fn writers_for(app:AppRef<'_>,source:&Digest,side:OwnedSourceSide,action:&str,steps:StatementSlice<'_>,path:&[usize],env:&mut LexicalEnvironment,out:&mut Vec<DecodedOwnedWriter>,cx:&mut AllocationContext)->Result<(),SourceAnalysisFailure>{
    for index in 0..steps.len(){let statement=steps.get(index).expect("bounded statement index");let mut current=copy_path(path,cx)?;push(&mut current,index,cx)?;let kind=statement_kind(statement);
        match statement{
            StatementView::Create{entity,..}=>{let definition=app.entities().iter().find(|e|e.id==entity).ok_or_else(||error(cx, "created entity missing"))?;for field in &definition.fields{push_writer(source,side,action,&current,kind,entity,Some(&field.id),definition,out,cx)?;}}
            StatementView::Update{record,values}=>{let entity=entity_from_expression(app,record,env,cx)?;let definition=app.entities().iter().find(|e|e.id==entity).ok_or_else(||error(cx, "writer entity missing"))?;for(field,_)in values.iter(){push_writer(source,side,action,&current,kind,&entity,Some(field),definition,out,cx)?;}}
            StatementView::Archive{record}=>{let entity=entity_from_expression(app,record,env,cx)?;let definition=app.entities().iter().find(|e|e.id==entity).ok_or_else(||error(cx, "writer entity missing"))?;push_writer(source,side,action,&current,kind,&entity,None,definition,out,cx)?;}
            StatementView::Collection{target:CollectionTarget::Field{record,field},..}=>{let entity=entity_from_expression(app,record,env,cx)?;let definition=app.entities().iter().find(|e|e.id==entity).ok_or_else(||error(cx, "writer entity missing"))?;push_writer(source,side,action,&current,kind,&entity,Some(field),definition,out,cx)?;}
            _=>{}
        }
        match statement{
            StatementView::Create{entity,bind,..}=>env.reference(bind,entity,&mut Some(&mut *cx)).map_err(source_failure)?,
            StatementView::ForEach{items,binding,steps:nested}=>{let item=owned_source_shared::list_item(app,items,env,&mut Some(&mut *cx)).map_err(source_failure)?;let mut local=env.clone_with(&mut Some(&mut *cx)).map_err(source_failure)?;local.insert(binding,&item,&mut Some(&mut *cx)).map_err(source_failure)?;writers_for(app,source,side,action,nested,&current,&mut local,out,cx)?;},
            _=>{}
        }
    }
    Ok(())
}
