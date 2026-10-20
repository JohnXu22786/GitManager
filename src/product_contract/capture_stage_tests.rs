// A descendant of product_contract. No paid implementation or stub lives here.
use super::*;
#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;
#[path = "../../tests/support/product_contract_fixture.rs"]
mod rich;
#[path = "../../tests/support/capture_native_oracle/product_contract.rs"]
mod old;
use serde_json::{json, Value};

#[derive(Default)]
struct Work { calls: Vec<u64>, remaining: Option<u64>, fail_call: Option<usize>, fail_after_apps: Option<usize> }
impl ContractWork for Work {
    fn charge(&mut self, n:u64)->std::result::Result<(),WorkFailure> {
        self.calls.push(n);
        if self.fail_call == Some(self.calls.len()) || self.fail_after_apps.is_some_and(|n| validation_counts().apps >= n) { return Err(WorkFailure::Cancelled); }
        if let Some(left)=&mut self.remaining {
            if n > *left { return Err(WorkFailure::BudgetExhausted("source test fuel")); }
            *left-=n;
        }
        Ok(())
    }
}
fn capture(value:Value)->CapturedProgram {
    CapturedProgram::capture(&serde_json::to_vec(&value).unwrap(),"project",Producer::UserAuthored,None).unwrap()
}
fn old_capture(c:&CapturedProgram)->old::CapturedProgram {
    serde_json::from_slice(&serde_json::to_vec(c).unwrap()).unwrap()
}
fn old_data(d:&DataSnapshot)->old::DataSnapshot {
    serde_json::from_slice(&serde_json::to_vec(d).unwrap()).unwrap()
}
fn result_text(result:ContractWorkResult<()>)->std::result::Result<(),String> {
    result.map_err(|e| match e {
        ContractWorkError::Contract(e)=>e.0,
        ContractWorkError::Resource(e)=>panic!("unexpected resource error: {e:?}"),
    })
}
fn checked(c:&CapturedProgram,d:&DataSnapshot)->ContractWorkResult<()> {
    with_capture_validation(c,d,&mut Work::default(),|ctx|ctx.ensure_source_and_data())
}
fn static_old(c:&CapturedProgram,entity:&str,binding:&str,e:&Expr)->std::result::Result<usize,String> {
    let mut app=old_capture(c).program;
    let e:old::Expr=serde_json::from_slice(&serde_json::to_vec(e).unwrap()).unwrap();
    let mut id="gm_scope_projection".to_owned();
    while app.actions.iter().any(|a|a.id==id) { id.push('_'); }
    app.actions.push(old::ActionDefinition {
        id,label:"Host projection type check".into(),
        parameters:BTreeMap::from([(binding.into(),old::Type::reference(entity))]),
        guards:vec![],ensures:vec![],steps:vec![old::Statement::Assert {
            condition:old::Expr::Equal {left:Box::new(e.clone()),right:Box::new(e)},
            message:"Projection type check".into(),
        }],
    });
    app.validate().map_err(|e|e.0)?;
    Ok(old::canonical_bytes(&app).unwrap().len())
}
fn static_paid(c:&CapturedProgram,entity:&str,binding:&str,e:&Expr)->std::result::Result<(),String> {
    let d=DataSnapshot::empty("project",&c.program).unwrap();
    result_text(with_capture_validation(c,&d,&mut Work::default(),|ctx| {
        ctx.ensure_source_and_data()?;
        ctx.validate_extension(entity,binding,e)
    }))
}
fn expr(value:Value)->Expr { serde_json::from_value(value).unwrap() }
fn yes()->Value { json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}) }
fn integer(n:i64)->Value { json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}}) }
fn text_expr(s:&str)->Value { json!({"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":s}}) }
fn ints()->Value { json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"integer"}},"value":{"kind":"list","item_type":{"kind":"integer"},"items":[{"kind":"integer","value":1}]}}) }
fn query()->Value { json!({"kind":"query","entity":"row","binding":"item","predicate":yes(),"sort":[],"limit":2,"include_archived":false}) }

#[test]
fn closed_capture_keeps_one_paid_validation_and_four_literal_facts() {
    for value in [fixture::minimal(),rich::organizer(),rich::equipment(),rich::new_composition()] {
        for bytes in [serde_json::to_vec(&value).unwrap(),serde_json::to_vec_pretty(&value).unwrap()] {
            let c=CapturedProgram::capture(&bytes,"project",Producer::UserAuthored,None).unwrap();
            let d=DataSnapshot::empty("project",&c.program).unwrap();
            let oc=old_capture(&c);
            let native_len=old::canonical_bytes(&oc.program).unwrap().len();
            let expected_session=old::SessionState::initial(&oc.program).unwrap();
            let before=serde_json::to_vec(&c).unwrap();
            reset_validation_counts();
            c.validate().unwrap();
            assert_eq!(validation_counts(),ValidationCounts {captures:1,apps:3,observables:0,views:0});
            assert_eq!(serde_json::to_vec(&c).unwrap(),before);
            old::reset_validation_counts();
            oc.validate().unwrap();
            let ordinary=old::validation_counts();
            assert_eq!((ordinary.captures,ordinary.apps,ordinary.observables,ordinary.views),(1,3,0,0));
            assert_eq!(serde_json::to_vec(&c).unwrap(),before);
            reset_validation_counts();
            with_capture_validation(&c,&d,&mut Work::default(),|ctx|->ContractWorkResult<()> {
                ctx.ensure_source_and_data()?;
                assert!(std::ptr::eq(ctx.program,&c));
                assert!(std::ptr::eq(ctx.data,&d));
                let facts=ctx.facts.as_ref().unwrap();
                assert_eq!(facts.canonical_len,native_len);
                assert_eq!(facts.action_count,c.program.actions.len());
                if c.program.id=="source_fixture" {
                    assert_eq!((facts.total_nodes,facts.action_prefix_nodes),(7,2));
                }
                ctx.ensure_source_and_data()?;
                let actual=ctx.initial_session(&mut Work::default())?;
                assert_eq!(serde_json::to_value(actual).unwrap(),serde_json::to_value(&expected_session).unwrap());
                assert_eq!(validation_counts(),ValidationCounts {captures:1,apps:1,observables:0,views:0});
                Ok(())
            }).unwrap();
            assert_eq!(serde_json::to_vec(&c).unwrap(),before);
        }
    }
}

#[test]
fn complete_external_error_prefixes_match_the_frozen_native_capture() {
    let good=capture(fixture::minimal());
    let data=DataSnapshot::empty("project",&good.program).unwrap();
    let other=capture(rich::organizer());
    let mut cases:Vec<(&str,CapturedProgram,usize,usize)>=vec![];
    for (name,bytes) in fixture::malformed_sources(MAX_WIRE_BYTES) {
        let mut c=good.clone();c.source_bytes=bytes;cases.push((name,c,0,0));
    }
    for (name,value) in fixture::semantic_negatives() {
        let mut c=good.clone();c.source_bytes=serde_json::to_vec(&value).unwrap();
        let apps=if matches!(name,"unknown_typed_field"|"unknown_typed_variant") {0} else {1};
        cases.push((name,c,apps,apps));
    }
    for prefix in 0..9 {
        let mut c=good.clone();
        match prefix {
            0=>{c.binding.project_id="BAD".into();c.source_bytes=b"?".to_vec();},
            1=>{c.binding.program_path="../bad".into();c.artifact.version=2;},
            2=>c.artifact.version=2,
            3=>c.artifact.kind=ArtifactKind::ExternalWeb,
            4=>c.artifact.language.push('!'),
            5=>c.program.label.push('!'),
            6=>{c.artifact.raw_digest=other.artifact.raw_digest.clone();c.binding.source_digest=other.binding.source_digest.clone();},
            7=>c.artifact.program_digest=other.artifact.program_digest.clone(),
            _=>c.artifact.semantic_digest=other.artifact.semantic_digest.clone(),
        }
        cases.push(("suffix/preference",c,[0,0,0,0,0,1,1,2,3][prefix],[0,0,0,0,0,1,1,1,1][prefix]));
    }
    // Finite multiple-fault prefixes: the earlier real check must win even when
    // the retained App and all later identities are independently wrong.
    for prefix in 0..12 {
        let mut c=good.clone();
        c.program.label.push('!');
        c.artifact.raw_digest=other.artifact.raw_digest.clone();
        c.binding.source_digest=other.binding.source_digest.clone();
        c.artifact.program_digest=other.artifact.program_digest.clone();
        c.artifact.semantic_digest=other.artifact.semantic_digest.clone();
        let (name,ordinary_apps,paid_apps)=match prefix {
            0|1=>{
                if prefix==0 {c.binding.project_id="BAD".into();} else {c.binding.program_path="../bad".into();}
                c.artifact.version=2;c.artifact.kind=ArtifactKind::ExternalWeb;c.artifact.language.push('!');
                c.source_bytes=b"?".to_vec();
                (if prefix==0 {"binding project before artifact/parser"} else {"binding path before artifact/parser"},0,0)
            },
            2=>{
                c.artifact.version=2;c.artifact.kind=ArtifactKind::ExternalWeb;c.artifact.language.push('!');
                c.source_bytes=vec![b'!';MAX_WIRE_BYTES+1];
                ("version before kind/language/source cap",0,0)
            },
            3=>{
                c.artifact.kind=ArtifactKind::ExternalWeb;c.artifact.language.push('!');
                c.source_bytes=vec![b'!';MAX_WIRE_BYTES+1];
                ("kind/language before source cap",0,0)
            },
            4=>{c.source_bytes=vec![b'!';MAX_WIRE_BYTES+1];("source cap before parser/retained/digests",0,0)},
            5=>{c.source_bytes=b"?".to_vec();("parser before retained/digests",0,0)},
            6=>{c.source_bytes=b"{}".to_vec();("typed conversion before retained/digests",0,0)},
            7=>{
                let mut value=fixture::minimal();value["label"]=json!(" ");
                c.source_bytes=serde_json::to_vec(&value).unwrap();
                ("V1 semantic before retained/digests",1,1)
            },
            8=>("retained App before all digests",1,1),
            9=>{c.program=good.program.clone();("raw before Program/semantic",1,1)},
            10=>{
                c.program=good.program.clone();c.artifact.raw_digest=good.artifact.raw_digest.clone();
                c.binding.source_digest=good.binding.source_digest.clone();
                ("Program before semantic",2,1)
            },
            _=>{
                c.program=good.program.clone();c.artifact.raw_digest=good.artifact.raw_digest.clone();
                c.binding.source_digest=good.binding.source_digest.clone();c.artifact.program_digest=good.artifact.program_digest.clone();
                ("semantic after Program",3,1)
            },
        };
        cases.push((name,c,ordinary_apps,paid_apps));
    }
    for (name,c,ordinary_apps,paid_apps) in cases {
        let before=serde_json::to_vec(&c).unwrap();
        let oc=old_capture(&c);
        old::reset_validation_counts();
        let error=oc.validate().unwrap_err().0;
        let counts=old::validation_counts();
        assert_eq!((counts.captures,counts.apps),(1,ordinary_apps),"frozen: {name}");
        reset_validation_counts();
        assert_eq!(c.validate().unwrap_err().0,error,"ordinary: {name}");
        assert_eq!((validation_counts().captures,validation_counts().apps),(1,ordinary_apps),"ordinary: {name}");
        assert_eq!(serde_json::to_vec(&c).unwrap(),before,"ordinary input: {name}");
        reset_validation_counts();
        assert_eq!(result_text(checked(&c,&data)),Err(error),"paid: {name}");
        let actual=validation_counts();
        assert_eq!((actual.captures,actual.apps),(1,paid_apps),"paid: {name}");
        assert_eq!(serde_json::to_vec(&c).unwrap(),before,"paid input: {name}");
    }
}

fn data_fixture(c:&CapturedProgram)->DataSnapshot {
    let mut d=DataSnapshot::empty("project",&c.program).unwrap();
    d.schema[0].unique=vec![vec!["name".into()]];
    d.schema[0].fields.push(FieldDefinition {id:"friend".into(),label:"Friend".into(),value_type:Type::Optional {item:Box::new(Type::reference("row"))}});
    d.records.push(Record {entity:"row".into(),id:"r1".into(),revision:1,created_program:c.artifact.program_digest.clone(),archived:false,values:BTreeMap::from([("name".into(),DataValue::Text {value:"one".into()})])});
    d
}
fn event(c:&CapturedProgram)->BusinessEvent {
    BusinessEvent {id:"event".into(),sequence:1,operation_id:"operation".into(),action:"historical_action".into(),day:0,program:c.artifact.program_digest.clone(),changes:vec![RecordChange {entity:"row".into(),record:"r1".into(),before:Some(BTreeMap::new()),after:BTreeMap::new(),archived:false}],outputs:vec![]}
}
#[test]
fn borrowed_schema_and_full_data_cross_products_match_frozen_clone_validator() {
    let c=capture(fixture::minimal());
    let good=data_fixture(&c);
    let mut cases=vec![("valid wider schema",good.clone())];
    for case in 0..27 {
        let mut d=good.clone();
        match case {
            0=>d.version=2,
            1=>d.project_id="BAD".into(),
            2=>d.records=vec![d.records[0].clone();MAX_COLLECTION+1],
            3=>d.schema[0].fields[0].value_type=Type::reference("missing"),
            4=>{let f=d.schema[0].fields[0].clone();d.schema[0].fields.push(f);},
            5=>d.schema[0].unique=vec![vec![]],
            6=>d.schema[0].constraints=vec![Expr::State {state:"missing".into()}],
            7=>d.records[0].id="BAD".into(),
            8=>d.records[0].revision=0,
            9=>d.records.push(d.records[0].clone()),
            10=>d.records[0].entity="missing".into(),
            11=>{d.records[0].values.remove("name");},
            12=>{d.records[0].values.insert("name".into(),DataValue::Boolean {value:true});},
            13=>{d.records[0].values.insert("friend".into(),DataValue::Reference {entity:"row".into(),record:"missing".into()});},
            14=>{let mut r=d.records[0].clone();r.id="r2".into();d.records.push(r);},
            15=>{let mut r=d.records[0].clone();r.id="r2".into();r.archived=true;d.records.push(r);},
            16=>{d.generation=1;let mut e=event(&c);e.id="BAD".into();d.events.push(e);},
            17=>{d.generation=1;let mut e=event(&c);e.operation_id="BAD".into();d.events.push(e);},
            18=>{d.generation=1;let mut e=event(&c);e.day=i32::MAX;d.events.push(e);},
            19=>{d.generation=2;let e=event(&c);let mut f=e.clone();f.id="other".into();f.sequence=2;d.events=vec![e,f];},
            20=>{d.generation=0;d.events.push(event(&c));},
            21=>{d.generation=1;let mut e=event(&c);e.changes.push(e.changes[0].clone());d.events.push(e);},
            22=>{d.generation=1;let mut e=event(&c);e.changes[0].record="missing".into();d.events.push(e);},
            23=>{d.generation=1;let mut e=event(&c);e.changes[0].before.as_mut().unwrap().insert("later".into(),DataValue::Null);d.events.push(e);},
            24=>{d.generation=1;let mut e=event(&c);e.changes[0].after.insert("name".into(),DataValue::Integer {value:1});d.events.push(e);},
            25=>{d.generation=1;let mut e=event(&c);e.changes[0].after.insert("friend".into(),DataValue::Reference {entity:"row".into(),record:"missing".into()});d.events.push(e);},
            _=>{d.schema[0].unique.clear();let r=d.records[0].clone();d.records=(0..300).map(|i|{let mut r=r.clone();r.id=format!("r{i}");r.values.insert("name".into(),DataValue::Text {value:"x".repeat(4096)});r}).collect();},
        }
        cases.push(("data prefix",d));
    }
    for (name,d) in cases {
        let expected=old_data(&d).validate().map_err(|e|e.0);
        assert_eq!(result_text(checked(&c,&d)),expected,"{name}");
    }
    // Unvalidated deep schema is traversed by the shared validator, never cloned first.
    let mut d=good;
    let mut t=Type::Text;
    for _ in 0..32 {t=Type::list(t);}
    d.schema[0].fields[0].value_type=t;
    assert_eq!(result_text(checked(&c,&d)),old_data(&d).validate().map_err(|e|e.0));
}

#[test]
fn synthetic_all_expr_families_match_actual_old_clone_append_validate() {
    let mut value=fixture::minimal();
    value["state"]=json!([{"id":"flag","label":"Flag","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":true}}]);
    let c=capture(value);
    let families=vec![
        yes(), json!({"kind":"variable","name":"record"}),
        json!({"kind":"field","record":{"kind":"variable","name":"record"},"field":"name"}),
        json!({"kind":"state","state":"flag"}),json!({"kind":"today"}),query(),
        json!({"kind":"filter","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"map","items":ints(),"binding":"x","value":{"kind":"variable","name":"x"}}),
        json!({"kind":"count","items":ints()}),
        json!({"kind":"any","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"all","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"sum","items":ints()}),json!({"kind":"contains","items":ints(),"value":integer(1)}),
        json!({"kind":"equal","left":integer(1),"right":integer(2)}),
        json!({"kind":"less","left":integer(1),"right":integer(2)}),
        json!({"kind":"add","left":integer(1),"right":integer(2)}),
        json!({"kind":"subtract","left":integer(1),"right":integer(2)}),
        json!({"kind":"concat","left":text_expr("é\""),"right":text_expr("\n")}),
        json!({"kind":"text_contains","text":text_expr("x"),"search":text_expr("")}),
        json!({"kind":"lower","value":text_expr("ΟΣ")}),
        json!({"kind":"and","values":[yes(),yes()]}),json!({"kind":"or","values":[yes(),yes()]}),
        json!({"kind":"not","value":yes()}),
        json!({"kind":"if","condition":yes(),"then_value":integer(1),"else_value":integer(2)}),
        json!({"kind":"coalesce","value":{"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"null"}},"fallback":text_expr("x")}),
        json!({"kind":"date_add","date":{"kind":"today"},"days":integer(1)}),
        json!({"kind":"date_difference","later":{"kind":"today"},"earlier":{"kind":"today"}}),
        json!({"kind":"if","condition":yes(),"then_value":yes(),"else_value":{"kind":"variable","name":"missing"}}),
        json!({"kind":"query","entity":"row","binding":"record","predicate":yes(),"sort":[],"limit":1,"include_archived":false}),
        json!({"kind":"and","values":[]}),
    ];
    for e in families.into_iter().map(expr) {
        assert_eq!(static_paid(&c,"row","record",&e),static_old(&c,"row","record",&e).map(|_|()),"{e:?}");
    }
}

fn with_nodes(target:usize)->CapturedProgram {
    let mut v=fixture::minimal();
    let mut steps=vec![json!({"kind":"assert","condition":{"kind":"and","values":vec![yes();256]},"message":"x"});31];
    // 31*(Assert1+And1+256 literals)=7998; observable/view suffix=5.
    let residual=target-8003;
    assert!(residual>=3 && residual<=258);
    steps.push(json!({"kind":"assert","condition":{"kind":"and","values":vec![yes();residual-2]},"message":"x"}));
    v["actions"][0]["steps"]=Value::Array(steps);
    capture(v)
}
#[test]
fn synthetic_depth_both_occurrences_suffix_and_action_limits_match_native() {
    let c=capture(fixture::minimal());
    for depth in [14,15,16] {
        let mut e=yes();
        for _ in 0..depth { e=json!({"kind":"not","value":e}); }
        let e=expr(e);
        assert_eq!(static_paid(&c,"row","record",&e),static_old(&c,"row","record",&e).map(|_|()));
    }
    for t in [8188,8189,8190,8192] {
        let c=with_nodes(t);
        let d=DataSnapshot::empty("project",&c.program).unwrap();
        with_capture_validation(&c,&d,&mut Work::default(),|ctx|->ContractWorkResult<()> {
            ctx.ensure_source_and_data()?;
            assert_eq!(ctx.facts.as_ref().unwrap().total_nodes,t);
            assert_eq!(ctx.facts.as_ref().unwrap().action_prefix_nodes,t-5);
            Ok(())
        }).unwrap();
        for e in [expr(yes()),expr(json!({"kind":"not","value":yes()}))] {
            assert_eq!(static_paid(&c,"row","record",&e),static_old(&c,"row","record",&e).map(|_|()),"T={t}");
        }
    }
    for n in [255,256] {
        let mut v=fixture::minimal();let template=v["actions"][0].clone();
        v["actions"]=Value::Array((0..n).map(|i|{let mut a=template.clone();a["id"]=json!(format!("a{i}"));a}).collect());
        let c=capture(v);
        for (entity,binding) in [("row","record"),("missing","BAD")] {
            assert_eq!(static_paid(&c,entity,binding,&expr(yes())),static_old(&c,entity,binding,&expr(yes())).map(|_|()));
        }
    }
    let mut v=fixture::minimal();let template=v["actions"][0].clone();
    v["actions"]=Value::Array(("gm_scope_projection".len()..=128).map(|len|{let mut a=template.clone();a["id"]=json!(format!("gm_scope_projection{}","_".repeat(len-"gm_scope_projection".len())));a}).collect());
    let c=capture(v);
    let expected=static_old(&c,"missing","BAD",&expr(yes())).unwrap_err();
    assert!(expected.starts_with("invalid stable ID: gm_scope_projection"));
    assert_eq!(static_paid(&c,"missing","BAD",&expr(yes())),Err(expected));
}

#[test]
fn first_cause_is_typed_sticky_and_empty_capture_does_not_validate() {
    let c=capture(fixture::minimal());let d=DataSnapshot::empty("project",&c.program).unwrap();
    reset_validation_counts();
    with_capture_validation(&c,&d,&mut Work::default(),|_|->ContractWorkResult<()> {Ok(())}).unwrap();
    assert_eq!(validation_counts(),ValidationCounts::default());
    for refuse in 1..=4 {
        let mut work=Work {fail_call:Some(refuse),..Work::default()};
        let result=with_capture_validation(&c,&d,&mut work,|ctx|->ContractWorkResult<()> {
            let _=ctx.ensure_source_and_data();
            Ok(())
        });
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::Cancelled)));
    }
    // Refuse in the reached native-validation prefix, using the existing actual
    // validation reader, not a fabricated paid-operation counter or expected sum.
    for apps in [1] {
        reset_validation_counts();
        let mut work=Work {fail_after_apps:Some(apps),..Work::default()};
        let result=with_capture_validation(&c,&d,&mut work,|ctx|->ContractWorkResult<()> {
            let _=ctx.ensure_source_and_data();
            Ok(())
        });
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::Cancelled)));
        assert_eq!(validation_counts().apps,apps);
    }
    let mut bad=c.clone();bad.binding.project_id="BAD".into();
    let expected=old_capture(&bad).validate().unwrap_err().0;
    let mut work=Work::default();
    let result=with_capture_validation(&bad,&d,&mut work,|ctx|->ContractWorkResult<()> {
        let _=ctx.ensure_source_and_data();
        ctx.with_host(|w|w.fail_call=Some(w.calls.len()+1));
        Ok(())
    });
    assert_eq!(result_text(result),Err(expected));
    assert_eq!(work.fail_call,Some(work.calls.len()+1),"finalization must not make a new poll after native semantic error");
}

#[test]
fn paid_value_validation_preserves_optional_null_and_malformed_prefixes() {
    for (v,t) in [
        (DataValue::Null,Type::Optional {item:Box::new(Type::Boolean)}),
        (DataValue::Null,Type::Boolean),
        (DataValue::Reference {entity:"row".into(),record:"BAD".into()},Type::reference("row")),
        (DataValue::List {item_type:Type::Integer,items:vec![DataValue::Integer {value:1}]},Type::list(Type::Integer)),
        (DataValue::List {item_type:Type::Text,items:vec![DataValue::Integer {value:1}]},Type::list(Type::Integer)),
    ] {
        let ov:old::DataValue=serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        let ot:old::Type=serde_json::from_slice(&serde_json::to_vec(&t).unwrap()).unwrap();
        for depth in [0,4,5] {
            let expected=old::validate_value(&ov,&ot,depth).map_err(|e|e.0);
            assert_eq!(result_text(validate_value_with_work(&v,&t,depth,&mut Work::default())),expected);
        }
    }
}

#[test]
fn synthetic_augmented_wire_exactly_straddles_native_one_mib_limit() {
    let e=expr(yes());
    let mut v=fixture::minimal();let template=v["actions"][0].clone();
    v["actions"]=Value::Array((0..255).map(|i|{let mut a=template.clone();a["id"]=json!(format!("a{i}"));a["label"]=json!("x");a}).collect());
    let small=capture(v.clone());
    let base=old::canonical_bytes(&old_capture(&small).program).unwrap().len();
    let augmented=static_old(&small,"row","record",&e).unwrap();
    let addition=augmented-base;
    for delta in [-1isize,0,1] {
        let target=((MAX_WIRE_BYTES-addition) as isize+delta) as usize;
        let mut value=v.clone();let mut left=target-base;
        for a in value["actions"].as_array_mut().unwrap() {
            let n=left.min(MAX_TEXT_BYTES-1);a["label"]=json!("x".repeat(n+1));left-=n;
        }
        assert_eq!(left,0,"padding capacity");
        let c=capture(value);
        assert_eq!(old::canonical_bytes(&old_capture(&c).program).unwrap().len(),target);
        let expected=static_old(&c,"row","record",&e).map(|_|());
        assert_eq!(expected.is_ok(),delta<=0);
        assert_eq!(static_paid(&c,"row","record",&e),expected);
    }
}

#[cfg(target_pointer_width="64")]
#[test]
fn shared_native_formula_literals_have_independent_boundary_expectations() {
    use super::capture_payment as paid;
    for (m,h) in [(0,1),(1,1),(10,1),(11,2),(70,2),(71,3),(430,3),(431,4),(2590,4)] {
        assert_eq!(paid::btree_levels(m).unwrap(),h);
    }
    for (m,expected) in [(0,98),(10,98),(11,192),(70,192),(71,286),(430,286),(431,380)] {
        // Borrowed identifier "x": Ord 3 fixed + 2 operand bytes =5.
        assert_eq!(paid::btree_search(m,5).unwrap(),expected);
    }
    for (n,expected) in [(0,4),(1,6),(128,260),(4096,8196)] {
        assert_eq!(paid::string_copy_bytes(n).unwrap(),expected);
    }
    // Real source unique-ID set family: K=&str,V=(), node reserve340 bytes.
    for (m,expected) in [(0,395),(1,169),(10,313),(11,3488)] {
        assert_eq!(paid::btree_insert::<&str,()>(m,5).unwrap(),expected);
    }
    assert_eq!(paid::btree_clone::<&str,()>(11,0).unwrap(),4526);
    assert_eq!(paid::btree_bulk::<&str,()>(11,5).unwrap(),6249);
    assert!(matches!(paid::btree_insert::<&str,()>(usize::MAX,5),Err(ContractWorkError::Resource(_))));
    assert!(matches!(paid::string_copy_bytes(usize::MAX),Err(ContractWorkError::Resource(_))));
    // Independent refusal prefix at source context entry, not a fake work counter.
    let c=capture(fixture::minimal());let d=DataSnapshot::empty("project",&c.program).unwrap();
    let b=10+std::mem::size_of::<CaptureValidation<'_, '_, Work>>() as u64;
    for limit in [b-1,b,b+1] {
        let mut work=Work {remaining:Some(limit),..Work::default()};
        reset_validation_counts();
        let result=with_capture_validation(&c,&d,&mut work,|_|->ContractWorkResult<()> {Ok(())});
        assert_eq!(result.is_ok(),limit>=b);
        assert_eq!(validation_counts(),ValidationCounts::default());
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_paid_capture_uses_source_relation_before_v1() {
    let c = capture(fixture::minimal());
    let before = serde_json::to_vec(&c).unwrap();
    reset_validation_counts();
    let mut work = Work::default();
    let state = capture_work::WorkState::new(&mut work);
    let result = state.finish(c.validate_audited(Audit(Some(&state))));
    assert!(result.is_ok());
    assert_eq!(validation_counts().apps, 1);
    // Independent F_C=576 and R_C=212 on the authenticated 64-bit target.
    // This root reserve occurs after actual intake, before the first relation node.
    assert_eq!(work.calls.windows(4).filter(|w| *w == [96,149972,6,12]).count(),1);
    assert_eq!(serde_json::to_vec(&c).unwrap(),before);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn lazy_error_success_keeps_literal_conditions_and_removes_format_work() {
    let mut work = Work::default();
    {
        let state = capture_work::WorkState::new(&mut work);
        let a = Audit(Some(&state));
        a.check_id("x").unwrap();
    }
    assert_eq!(work.calls,[4,1,9,1,4,1]);
    let mut work = Work::default();
    {
        let state = capture_work::WorkState::new(&mut work);
        Audit(Some(&state)).unique(["x"]).unwrap();
    }
    assert_eq!(work.calls,[9,1,3,4,1,9,1,4,1,6,360,395,8,1,1,2,1]);
    let mut app:AppDefinition=serde_json::from_value(fixture::minimal()).unwrap();
    app.entities[0].fields=vec![FieldDefinition {id:"x".into(),label:"X".into(),value_type:Type::Optional {item:Box::new(Type::Date)}}];
    let env=Environment::new();
    let mut work=Work::default();
    {
        let state=capture_work::WorkState::new(&mut work);
        let v=Validator::new(&app,Audit(Some(&state)));
        let bound=v.bound(&env,"x",Type::Boolean).unwrap();
        assert_eq!(bound,BTreeMap::from([("x".to_owned(),Type::Boolean)]));
        assert!(env.is_empty());
    }
    assert_eq!(&work.calls[..12],&[8,4,1,9,1,4,1,276,6,98,8,1]);
    for (method,expected,total) in [(0,Type::Date,50),(1,Type::Date,50),(1,Type::Optional {item:Box::new(Type::Date)},54)] {
        let mut work=Work::default();
        {
            let state=capture_work::WorkState::new(&mut work);
            let mut v=Validator::new(&app,Audit(Some(&state)));
            if method==0 {v.expect(&Expr::Today,&env,&expected,0,true).unwrap();}
            else {v.assignment(&Expr::Today,&env,&expected,0).unwrap();}
            assert_eq!(v.nodes,1);
        }
        let expected=if total==50 {vec![8,8,8,1,12,4,8,1]} else {vec![8,8,8,1,12,4,4,8,1]};
        assert_eq!(work.calls,expected);
        assert_eq!(work.calls.iter().sum::<u64>(),total);
    }
    let mut work=Work::default();
    {
        let state=capture_work::WorkState::new(&mut work);
        let mut v=Validator::new(&app,Audit(Some(&state)));
        v.assignments("row",&BTreeMap::new(),&env,0,true).unwrap();
    }
    assert_eq!(work.calls,[8,2,1,8,8,1,3,3,5,6,5,5,8,276,6,98,8,1]);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn lazy_error_selection_and_failure_reserves_preserve_first_typed_cause() {
    for (id,expected) in [("x",vec![4,1,9,1,4,1]),("!",vec![4,1,9,1,4,2,320,325,8,44,1])] {
        let mut work=Work::default();
        let state=capture_work::WorkState::new(&mut work);
        let result=state.finish(Audit(Some(&state)).check_id(id));
        if id=="x" {assert_eq!(result,Ok(()));}
        else {assert_eq!(result,Err(ContractWorkError::Contract(ContractError("invalid stable ID: !".into()))));}
        assert_eq!(work.calls,expected);
        for index in 0..expected.len() {
            for cancel in [true,false] {
                let prefix=expected[..=index].iter().sum::<u64>();
                let mut work=Work {fail_call:cancel.then_some(index+1),remaining:(!cancel).then_some(prefix-1),..Work::default()};
                let state=capture_work::WorkState::new(&mut work);
                let a=Audit(Some(&state));
                let result=a.check_id(id);
                assert!(result.is_err());
                assert!(a.pay(0).is_err());
                let result=state.finish(result);
                let cause=if cancel {WorkFailure::Cancelled} else {WorkFailure::BudgetExhausted("source test fuel")};
                assert_eq!(result,Err(ContractWorkError::Resource(cause)));
                assert_eq!(work.calls,&expected[..=index]);
            }
        }
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn lazy_error_failures_match_original_bytes_and_check_order() {
    let mut app:AppDefinition=serde_json::from_value(fixture::minimal()).unwrap();
    app.entities[0].fields=vec![FieldDefinition {id:"x".into(),label:"X".into(),value_type:Type::Text}];
    let occupied=Environment::from([("x".to_owned(),Type::Boolean)]);
    let empty=Environment::new();
    for (method,expected,suffix) in [
        (0,"duplicate ID: x",vec![8,2,320,246,8,34,1]),
        (1,"binding shadows existing variable: x",vec![8,2,320,558,8,76,1]),
        (2,"expression type mismatch: expected Text, found Date",vec![8,3,4,4,320,1575,8,220,1]),
        (3,"assignment type mismatch: expected Text, found Date",vec![8,3,4,4,320,1575,8,220,1]),
        (4,"missing required field: x",vec![8,2,320,395,8,54,1]),
    ] {
        let run=|a:Audit<'_>|->Result<()> {
            let mut validator=Validator::new(&app,a);
            match method {
                0=>a.unique(["x","x","INVALID"]),
                1=>validator.bound(&occupied,"x",Type::Date).map(|_|()),
                2=>validator.expect(&Expr::Today,&empty,&Type::Text,0,true),
                3=>validator.assignment(&Expr::Today,&empty,&Type::Text,0),
                _=>validator.assignments("row",&BTreeMap::new(),&empty,0,true),
            }
        };
        assert_eq!(run(Audit::none()),Err(ContractError(expected.into())));
        let mut trace=Work::default();let state=capture_work::WorkState::new(&mut trace);
        assert_eq!(state.finish(run(Audit(Some(&state)))),Err(ContractWorkError::Contract(ContractError(expected.into()))));
        assert!(trace.calls.ends_with(&suffix));
        for index in trace.calls.len()-suffix.len()..trace.calls.len() {
            for cancel in [true,false] {
                let prefix=trace.calls[..=index].iter().sum::<u64>();
                let mut work=Work {fail_call:cancel.then_some(index+1),remaining:(!cancel).then_some(prefix-1),..Work::default()};
                let state=capture_work::WorkState::new(&mut work);let a=Audit(Some(&state));
                let result=run(a);assert!(result.is_err());assert!(a.pay(0).is_err());
                let cause=if cancel {WorkFailure::Cancelled}else{WorkFailure::BudgetExhausted("source test fuel")};
                assert_eq!(state.finish(result),Err(ContractWorkError::Resource(cause)));
                assert_eq!(work.calls,&trace.calls[..=index]);
            }
        }
        assert_eq!(occupied,Environment::from([("x".to_owned(),Type::Boolean)]));
    }
    for id in ["".to_owned(),"x".repeat(129),"!x".into(),"x!".into(),"\"\\\nβ".into()] {
        let expected=format!("invalid stable ID: {id}");
        assert_eq!(Audit::none().check_id(&id),Err(ContractError(expected.clone())));
        let mut work=Work::default();let state=capture_work::WorkState::new(&mut work);
        assert_eq!(state.finish(Audit(Some(&state)).check_id(&id)),Err(ContractWorkError::Contract(ContractError(expected))));
    }
    for id in ["a0_-".to_owned(),"x".repeat(128)] {
        let mut work=Work::default();let state=capture_work::WorkState::new(&mut work);
        assert_eq!(state.finish(Audit(Some(&state)).check_id(&id)),Ok(()));
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn lazy_error_all_success_selectors_and_outcomes_refuse_exactly() {
    let mut app:AppDefinition=serde_json::from_value(fixture::minimal()).unwrap();
    app.entities[0].fields=vec![FieldDefinition {id:"x".into(),label:"X".into(),value_type:Type::Optional {item:Box::new(Type::Date)}}];
    let empty=Environment::new();
    let before=serde_json::to_vec(&app).unwrap();
    for method in 0..6 {
        let run=|a:Audit<'_>|->Result<()> {
            let mut v=Validator::new(&app,a);
            match method {
                0=>a.unique(["x"]),
                1=>v.bound(&empty,"x",Type::Boolean).map(|bound|assert_eq!(bound,Environment::from([("x".into(),Type::Boolean)]))),
                2=>v.expect(&Expr::Today,&empty,&Type::Date,0,true),
                3=>v.assignment(&Expr::Today,&empty,&Type::Date,0),
                4=>v.assignments("row",&BTreeMap::new(),&empty,0,true),
                _=>v.assignment(&Expr::Today,&empty,&Type::Optional {item:Box::new(Type::Date)},0),
            }
        };
        assert_eq!(run(Audit::none()),Ok(()));
        let mut trace=Work::default();let state=capture_work::WorkState::new(&mut trace);
        assert_eq!(state.finish(run(Audit(Some(&state)))),Ok(()));
        let selector=match method {0=>12,1=>10,2|3=>6,4=>16,_=>7};
        assert_eq!(&trace.calls[selector..selector+2],&[8,1],"method{method}");
        for index in selector..=selector+1 {
            let funded=trace.calls[..=index].iter().try_fold(0u64,|n,&x|n.checked_add(x)).unwrap();
            let mut work=Work {remaining:Some(funded),..Work::default()};
            let state=capture_work::WorkState::new(&mut work);let a=Audit(Some(&state));
            let result=run(a);
            if index+1==trace.calls.len() {
                assert_eq!(state.finish(result),Ok(()));assert_eq!(work.calls,trace.calls);
            } else {
                assert!(a.pay(0).is_err());
                assert_eq!(state.finish(result),Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("source test fuel"))));
                assert_eq!(work.calls,&trace.calls[..=index+1]);
            }
            assert_eq!(work.remaining,Some(0));
        }
        for index in 0..trace.calls.len() {
            for cancel in [true,false] {
                let prefix=trace.calls[..=index].iter().try_fold(0u64,|n,&x|n.checked_add(x)).unwrap();
                let mut work=Work {fail_call:cancel.then_some(index+1),remaining:(!cancel).then_some(prefix-1),..Work::default()};
                let state=capture_work::WorkState::new(&mut work);let a=Audit(Some(&state));
                let result=run(a);assert!(result.is_err());assert!(a.pay(0).is_err());
                let cause=if cancel {WorkFailure::Cancelled}else{WorkFailure::BudgetExhausted("source test fuel")};
                assert_eq!(state.finish(result),Err(ContractWorkError::Resource(cause)));
                assert_eq!(work.calls,&trace.calls[..=index],"method{method}, callback{index}");
                assert!(empty.is_empty());assert_eq!(serde_json::to_vec(&app).unwrap(),before);
            }
        }
    }
}
