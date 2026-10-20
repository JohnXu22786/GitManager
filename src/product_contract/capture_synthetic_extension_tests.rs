//! Concrete synthetic-action count tests. Frozen native ActionDefinition is the oracle.
use super::*;
#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;
#[path = "../../tests/support/capture_native_oracle/product_contract.rs"]
mod old;
use serde_json::{json, Value};

#[derive(Default)]
struct Work { calls: Vec<u64>, left: Option<u64>, refuse: Option<usize>, cause: Option<WorkFailure> }
impl ContractWork for Work {
    fn charge(&mut self, n:u64)->std::result::Result<(),WorkFailure> {
        self.calls.push(n);
        if self.refuse==Some(self.calls.len()) {return Err(self.cause.clone().unwrap_or(WorkFailure::Cancelled));}
        if let Some(left)=&mut self.left {*left=left.checked_sub(n).ok_or(WorkFailure::BudgetExhausted("synthetic test fuel"))?;}
        Ok(())
    }
}
fn synthetic<'a>(id:&'a str,binding:&'a str,entity:&'a str,expr:&'a Expr)->SyntheticAction<'a> {
    SyntheticAction { id,label:"Host projection type check",parameters:Parameters{binding,entity},guards:[],ensures:[],steps:[SyntheticAssert{kind:"assert",condition:SyntheticEqual{kind:"equal",left:expr,right:expr},message:"Projection type check"}] }
}
fn original(id:&str,binding:&str,entity:&str,expr:&Expr)->old::ActionDefinition {
    let expr:old::Expr=serde_json::from_slice(&serde_json::to_vec(expr).unwrap()).unwrap();
    old::ActionDefinition{id:id.into(),label:"Host projection type check".into(),parameters:BTreeMap::from([(binding.into(),old::Type::reference(entity))]),guards:vec![],ensures:vec![],steps:vec![old::Statement::Assert{condition:old::Expr::Equal{left:Box::new(expr.clone()),right:Box::new(expr)},message:"Projection type check".into()}]}
}
fn app()->AppDefinition {serde_json::from_value(fixture::minimal()).unwrap()}
fn paid_extension(app:&AppDefinition,expr:&Expr,w:&mut Work)->ContractWorkResult<()> {
    let f=app.validate_audited(Audit::none()).unwrap();
    let state=WorkState::new(w);
    state.finish(extension(app,(f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count),"row","record",expr,Audit(Some(&state))))
}
#[test]
fn synthetic_extension_removes_dedicated_canonical_output_work() {
    let app=app();let expr=Expr::Today;
    let mut old_work=Work::default();
    let state=WorkState::new(&mut old_work);
    let bytes=state.finish(capture_canonical::canonical_bytes_audited(&synthetic("gm_scope_projection","record","row",&expr),Audit(Some(&state)))).unwrap();
    assert_eq!(bytes,old::canonical_bytes(&original("gm_scope_projection","record","row",&expr)).unwrap());
    let mut work=Work::default();paid_extension(&app,&expr,&mut work).unwrap();
    assert!(!work.calls.windows(old_work.calls.len()).any(|w|w==old_work.calls),"audited extension still executes the complete old Value/Vec descriptor and output reservation");
    for (name,expr) in [("today",Expr::Today),("text4096",Expr::Literal{value_type:Type::Text,value:DataValue::Text{value:"x".repeat(MAX_TEXT_BYTES)}}),("escaped4096",Expr::Literal{value_type:Type::Text,value:DataValue::Text{value:"\u{1}".repeat(MAX_TEXT_BYTES)}})] {
        let action=synthetic("gm_scope_projection","record","row",&expr);let mut old_work=Work::default();let state=WorkState::new(&mut old_work);
        let bytes=state.finish(capture_canonical::canonical_bytes_audited(&action,Audit(Some(&state)))).unwrap();let mut count_work=Work::default();assert_eq!(counted(&action,&mut count_work).unwrap(),bytes.len());
        eprintln!("synthetic-savings {}",json!({"case":name,"bytes":bytes.len(),"old_units":old_work.calls.iter().sum::<u64>(),"count_units":count_work.calls.iter().sum::<u64>(),"new_selection_units":96}));
    }
}
fn counted(action:&SyntheticAction<'_>,w:&mut Work)->ContractWorkResult<usize> {
    let state=WorkState::new(w);
    state.finish(capture_canonical::synthetic_canonical_len_audited(action,Audit(Some(&state))))
}
fn boolean()->Value {json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}})}
fn variants()->Vec<Expr> {
    let b=boolean();let mut out=vec![b.clone(),json!({"kind":"variable","name":"x"}),json!({"kind":"field","record":b,"field":"f"}),json!({"kind":"state","state":"s"}),json!({"kind":"today"}),json!({"kind":"query","entity":"e","binding":"b","predicate":b,"sort":[{"value":b,"descending":true}],"limit":17,"include_archived":true})];
    for kind in ["filter","any","all"] {out.push(json!({"kind":kind,"items":b,"binding":"b","predicate":b}));}
    out.push(json!({"kind":"map","items":b,"binding":"b","value":b}));
    for kind in ["count","sum"] {out.push(json!({"kind":kind,"items":b}));}
    out.push(json!({"kind":"contains","items":b,"value":b}));
    for kind in ["equal","less","add","subtract","concat"] {out.push(json!({"kind":kind,"left":b,"right":b}));}
    out.push(json!({"kind":"text_contains","text":b,"search":b}));
    for kind in ["lower","not"] {out.push(json!({"kind":kind,"value":b}));}
    for kind in ["and","or"] {out.push(json!({"kind":kind,"values":[b,b]}));}
    out.push(json!({"kind":"if","condition":b,"then_value":b,"else_value":b}));
    out.push(json!({"kind":"coalesce","value":b,"fallback":b}));
    out.push(json!({"kind":"date_add","date":b,"days":b}));
    out.push(json!({"kind":"date_difference","later":b,"earlier":b}));
    assert_eq!(out.len(),27);out.into_iter().map(|x|serde_json::from_value(x).unwrap()).collect()
}
fn json_height(value:&Value)->usize {
    match value {Value::Object(v)=>1+v.values().map(json_height).max().unwrap_or(0),Value::Array(v)=>1+v.iter().map(json_height).max().unwrap_or(0),_=>0}
}
fn parity(id:&str,binding:&str,entity:&str,e:&Expr) {
    let action=synthetic(id,binding,entity,e);
    let native=old::canonical_bytes(&original(id,binding,entity,e)).unwrap();
    let mut work=Work::default();let n=counted(&action,&mut work).unwrap();
    assert_eq!(n,native.len());
    assert_eq!(action.serialization_height(Audit::none()).unwrap(),json_height(&serde_json::from_slice(&native).unwrap()));
    assert_eq!(canonical_bytes(&action).unwrap(),native);
    let old_expr:old::Expr=serde_json::from_slice(&serde_json::to_vec(e).unwrap()).unwrap();
    // Independent literal fixed syntax; never used as the production count.
    assert_eq!(n,224+serde_json::to_vec(id).unwrap().len()+serde_json::to_vec(binding).unwrap().len()+serde_json::to_vec(entity).unwrap().len()+2*old::canonical_bytes(&old_expr).unwrap().len());
    assert_eq!(work.calls.first(),Some(&0));assert_eq!(work.calls.last(),Some(&0));
    assert_eq!(work.calls.iter().filter(|&&n|n==0).count(),2);
}
#[test]
fn synthetic_count_all_closed_variants_match_frozen_native() {
    for e in variants() {parity("id","binding","entity",&e);}
    let types=vec![Type::Boolean,Type::Integer,Type::Text,Type::Date,Type::reference("e"),Type::list(Type::Text),Type::Optional{item:Box::new(Type::reference("z"))}];
    let values=vec![DataValue::Null,DataValue::Boolean{value:false},DataValue::Integer{value:i64::MIN},DataValue::Text{value:"text".into()},DataValue::Date{days:i32::MIN},DataValue::Reference{entity:"e".into(),record:"r".into()},DataValue::List{item_type:Type::Text,items:vec![DataValue::Text{value:"nested".into()}]}];
    // Shallow serialization coverage, not a grant that every type/value pair is valid.
    for typ in types {for value in &values {parity("id","b","e",&Expr::Literal{value_type:typ.clone(),value:value.clone()});}}
}
#[test]
fn synthetic_count_escapes_widths_vectors_and_deepest_valid_graph() {
    let mut texts=vec![String::new(),"\"\\/\u{7f}é😀\u{2028}\u{2029}e\u{301}".into(),"é".repeat(MAX_TEXT_BYTES/2)];
    texts.push((0u8..32).map(char::from).collect());
    for text in texts {parity(&text,&text,&text,&Expr::Literal{value_type:Type::Text,value:DataValue::Text{value:text.clone()}});}
    for n in [i64::MIN,-100,-10,-1,0,1,9,10,99,100,i64::MAX] {parity("id","b","e",&Expr::Literal{value_type:Type::Integer,value:DataValue::Integer{value:n}});}
    for days in [i32::MIN,-719162,-10,0,9,10,2932896,i32::MAX] {parity("id","b","e",&Expr::Literal{value_type:Type::Date,value:DataValue::Date{days}});}
    for limit in [0,1,9,10,99,100,MAX_COLLECTION,usize::MAX] {for count in [0,1,3] {for descending in [false,true] {
        let e=Expr::Query{entity:"row".into(),binding:"item".into(),predicate:Box::new(serde_json::from_value(boolean()).unwrap()),sort:(0..count).map(|_|SortKey{value:Expr::Today,descending}).collect(),limit,include_archived:descending};parity("id","b","e",&e);
    }}}
    for count in [0,1,3] {let values=(0..count).map(|_|serde_json::from_value(boolean()).unwrap()).collect::<Vec<_>>();parity("id","b","e",&Expr::And{values:values.clone()});parity("id","b","e",&Expr::Or{values});}
    let app=app();let mut e:Expr=serde_json::from_value(boolean()).unwrap();
    for _ in 0..MAX_EXPR_DEPTH-2 {e=Expr::Not{value:Box::new(e)};}
    paid_extension(&app,&e,&mut Work::default()).unwrap();parity("id","b","e",&e);
    let mut typ=Type::Integer;let mut value=DataValue::Integer{value:0};
    for _ in 0..MAX_TYPE_DEPTH {value=DataValue::List{item_type:typ.clone(),items:vec![value]};typ=Type::list(typ);}
    let e=Expr::Literal{value_type:typ,value};paid_extension(&app,&e,&mut Work::default()).unwrap();parity("id","b","e",&e);
}
#[test]
fn synthetic_count_every_callback_refuses_and_budget_boundaries() {
    let e=Expr::Today;let action=synthetic("id","b","e",&e);let mut trace=Work::default();let length=counted(&action,&mut trace).unwrap();
    let total=trace.calls.iter().sum::<u64>();
    for (limit,success) in [(total-1,false),(total,true),(total+1,true)] {let mut w=Work{left:Some(limit),..Work::default()};assert_eq!(counted(&action,&mut w).is_ok(),success);}
    for index in 0..trace.calls.len() {for cause in [WorkFailure::Cancelled,WorkFailure::BudgetExhausted("synthetic refusal")] {
        let mut w=Work{refuse:Some(index+1),cause:Some(cause.clone()),..Work::default()};
        assert_eq!(counted(&action,&mut w),Err(ContractWorkError::Resource(cause)));
        assert_eq!(w.calls,trace.calls[..=index]);
    }}
    let stack=trace.calls.iter().position(|&n|n==4*5*(64*size_of::<usize>() as u64+64)).unwrap();
    let positive=[1,2,3,stack,trace.calls.len()-2];
    for index in positive {let prefix=trace.calls[..index].iter().sum::<u64>();let packet=trace.calls[index];assert!(packet>0);
        for allowance in [packet-1,packet,packet+1] {let mut w=Work{left:Some(prefix+allowance),..Work::default()};let result=counted(&action,&mut w);
            assert_eq!(&w.calls[..=index],&trace.calls[..=index]);if allowance<packet {assert_eq!(w.calls.len(),index+1);assert!(result.is_err());}else{assert!(w.calls.len()>index+1);}
        }
    }
    eprintln!("synthetic-component {}",json!({"length":length,"units":total,"calls":trace.calls}));
}
#[test]
fn synthetic_extension_prefix_and_ordinary_tail_stay_exact() {
    let good=Expr::Today;
    let mut marker=Work::default();counted(&synthetic("id","b","e",&good),&mut marker).unwrap();
    for count in [0,1,3] {
        let mut app=app();let template=app.actions[0].clone();app.actions=(0..count).map(|n|{let mut a=template.clone();a.id=format!("gm_scope_projection{}","_".repeat(n));a}).collect();
        // No fixture action-binding points to check; the original minimal has none.
        let f=app.validate_audited(Audit::none()).unwrap();let id=format!("gm_scope_projection{}","_".repeat(count));
        let mut old_app:old::AppDefinition=serde_json::from_slice(&serde_json::to_vec(&app).unwrap()).unwrap();old_app.actions.push(original(&id,"record","row",&good));old_app.validate().unwrap();
        let added=old::canonical_bytes(&original(&id,"record","row",&good)).unwrap().len();assert_eq!(old::canonical_bytes(&old_app).unwrap().len(),f.canonical_len+added+usize::from(count>0));
        let before=serde_json::to_vec(&app).unwrap();
        extension(&app,(f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count),"row","record",&good,Audit::none()).unwrap();
        paid_extension(&app,&good,&mut Work::default()).unwrap();assert_eq!(serde_json::to_vec(&app).unwrap(),before);
        for e in [Expr::Variable{name:"missing".into()},Expr::Literal{value_type:Type::Optional{item:Box::new(Type::Optional{item:Box::new(Type::Text)})},value:DataValue::Null}] {
            let mut old_bad:old::AppDefinition=serde_json::from_slice(&before).unwrap();old_bad.actions.push(original(&id,"record","row",&e));let expected=old_bad.validate().unwrap_err().0;
            let mut w=Work::default();assert_eq!(paid_extension(&app,&e,&mut w),Err(ContractWorkError::Contract(ContractError(expected))));
            assert!(!w.calls.windows(6).any(|w|w==&marker.calls[..6]),"no count after semantic error");
        }
    }
}
#[test]
fn synthetic_count_schema_and_custom_serialization_are_pinned() {
    let source=include_str!("../product_contract.rs").replace("\r\n","\n");let local=include_str!("capture_validation.rs").replace("\r\n","\n");let mut parts=Vec::new();
    for (name,kind) in [("Type","enum"),("DataValue","enum"),("Expr","enum"),("SortKey","struct"),("ActionDefinition","struct"),("Statement","enum")] {
        let at=source.find(&format!("pub {kind} {name} {{")).unwrap();let start=source[..at].rfind("#[derive").unwrap();let end=start+source[start..].find("\n}").unwrap()+2;parts.push(source[start..end].to_owned());
    }
    let start=local.find("#[derive(Serialize)]\npub(super) struct SyntheticAction").unwrap();let end=local.find("fn extension(\n").unwrap();parts.push(local[start..end].to_owned());
    let start=source.find("pub fn canonical_bytes<").unwrap();let end=start+source[start..].find("\n}").unwrap()+2;parts.push(source[start..end].to_owned());
    assert_eq!(parts.join("\n\n")+"\n",include_str!("capture_synthetic_schema.txt").replace("\r\n","\n"));
}

#[test]
fn synthetic_count_actual_small_fixture_components_do_not_regress() {
    // Exact before-side expressions copied from the unchanged owned_recovery
    // constructors, independently characterized before the height-bound repair.
    for (name,binding,raw) in [
        ("lifecycle","record",r####"{"kind":"equal","left":{"field":"status","kind":"field","record":{"kind":"variable","name":"record"}},"right":{"kind":"literal","value":{"kind":"text","value":"done"},"value_type":{"kind":"text"}}}"####),
        ("view/jobs/title","row",r####"{"field":"title","kind":"field","record":{"kind":"variable","name":"row"}}"####),
        ("count/policy_memory_count","j",r####"{"kind":"and","values":[{"kind":"not","value":{"kind":"equal","left":{"field":"status","kind":"field","record":{"kind":"variable","name":"j"}},"right":{"kind":"literal","value":{"kind":"text","value":"done"},"value_type":{"kind":"text"}}}},{"field":"policy_memory_used","kind":"field","record":{"kind":"variable","name":"j"}}]}"####),
    ] {
        let expr:Expr=serde_json::from_str(raw).unwrap();let action=synthetic("gm_scope_projection",binding,"job",&expr);
        let mut old_work=Work::default();let state=WorkState::new(&mut old_work);let native=state.finish(capture_canonical::canonical_bytes_audited(&action,Audit(Some(&state)))).unwrap();
        let mut new_work=Work::default();assert_eq!(counted(&action,&mut new_work).unwrap(),native.len());
        let before=old_work.calls.iter().sum::<u64>();let after=new_work.calls.iter().sum::<u64>()+96;
        assert!(after<before,"{name}: actual small component regressed {before} -> {after}");
    }
}

#[test]
fn synthetic_height_paid_walk_refuses_every_entry_and_loop() {
    #[cfg(target_pointer_width="64")]
    assert_eq!(SYNTHETIC_HEIGHT_FRAME,544);
    let mut exprs=variants();
    exprs.push(Expr::Literal{value_type:Type::Optional{item:Box::new(Type::list(Type::Text))},value:DataValue::List{item_type:Type::list(Type::Text),items:vec![DataValue::List{item_type:Type::Text,items:vec![]},DataValue::Text{value:"x".into()}]}});
    for expr in exprs {
        let other=Expr::Not{value:Box::new(Expr::Today)};
        let mut action=synthetic("id","b","e",&expr);action.steps[0].condition.right=&other;
        let native=canonical_bytes(&action).unwrap();let expected=json_height(&serde_json::from_slice(&native).unwrap());
        let mut work=Work::default();let height={let state=WorkState::new(&mut work);state.finish(action.serialization_height(Audit(Some(&state)))).unwrap()};assert_eq!(height,expected);
        assert_eq!(counted(&action,&mut Work::default()).unwrap(),native.len());
        assert!(work.calls.iter().all(|&n|n==SYNTHETIC_HEIGHT_FRAME as u64+96 || n==32));
        let total=work.calls.iter().sum::<u64>();
        for (limit,success) in [(total-1,false),(total,true),(total+1,true)] {let mut w=Work{left:Some(limit),..Work::default()};let state=WorkState::new(&mut w);assert_eq!(state.finish(action.serialization_height(Audit(Some(&state)))).is_ok(),success);}
        for index in 0..work.calls.len() {for cause in [WorkFailure::Cancelled,WorkFailure::BudgetExhausted("height refusal")] {
            let mut w=Work{refuse:Some(index+1),cause:Some(cause.clone()),..Work::default()};let state=WorkState::new(&mut w);
            assert_eq!(state.finish(action.serialization_height(Audit(Some(&state)))),Err(ContractWorkError::Resource(cause)));assert_eq!(w.calls,work.calls[..=index]);
        }}
    }
    // Max container contribution per Expr edge is Query/sort/SortKey. This
    // serialization-only graph is not used to bypass semantic validation.
    let mut deep=Expr::Today;
    for _ in 0..MAX_EXPR_DEPTH-2 {deep=Expr::Query{entity:"e".into(),binding:"b".into(),predicate:Box::new(Expr::Today),sort:vec![SortKey{value:deep,descending:false}],limit:1,include_archived:false};}
    parity("id","b","e",&deep);
}
#[test]
fn synthetic_extension_selector_refusal_is_retained() {
    let c=CapturedProgram::capture(&serde_json::to_vec(&fixture::minimal()).unwrap(),"project",Producer::UserAuthored,None).unwrap();
    let data=DataSnapshot::empty("project",&c.program).unwrap();let expr=Expr::Today;
    let mut marker=Work::default();counted(&synthetic("gm_scope_projection","record","row",&expr),&mut marker).unwrap();
    let mut trace=Work::default();with_capture_validation(&c,&data,&mut trace,|ctx|->ContractWorkResult<()>{ctx.validate_extension("row","record",&expr)}).unwrap();
    let matches=trace.calls.windows(7).enumerate().filter(|(_,w)|*w==&marker.calls[..7]).map(|(i,_)|i).collect::<Vec<_>>();assert_eq!(matches.len(),1);
    let selector=matches[0]-1;assert_eq!(trace.calls[selector],32+2*size_of::<Result<usize>>() as u64+2*size_of::<usize>() as u64);
    for cause in [WorkFailure::Cancelled,WorkFailure::BudgetExhausted("selector refusal")] {
        let mut w=Work{refuse:Some(selector+1),cause:Some(cause.clone()),..Work::default()};
        let result=with_capture_validation(&c,&data,&mut w,|ctx|->ContractWorkResult<()>{let _=ctx.validate_extension("row","record",&expr);Ok(())});
        assert_eq!(result,Err(ContractWorkError::Resource(cause)));assert_eq!(w.calls,trace.calls[..=selector]);
    }
}
