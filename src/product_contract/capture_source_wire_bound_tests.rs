//! Same-call source bound controls. Native serde remains the only JSON oracle.
use super::app_length_tests::{capture_minimal, complete_fixture, deepest_valid_fixture, fixture, old, old_app, source_result, Work};
use super::*;
use serde_json::{json, Value};

fn rebind(value: Value, raw: Option<Vec<u8>>) -> CapturedProgram {
    let mut c = capture_minimal();
    c.program = serde_json::from_value(value).unwrap();
    c.source_bytes = raw.unwrap_or_else(|| serde_json::to_vec(&c.program).unwrap());
    c.artifact.raw_digest = bytes_digest(IdentityDomain::Source, &c.source_bytes);
    c.binding.source_digest = c.artifact.raw_digest.clone();
    c.artifact.program_digest = canonical_digest(IdentityDomain::Program, &c.program).unwrap();
    let mut semantic = c.program.clone();
    semantic.clear_display_labels_audited(Audit::none()).unwrap();
    c.artifact.semantic_digest = canonical_digest(IdentityDomain::SemanticProgram, &semantic).unwrap();
    c
}
fn facts(c: &CapturedProgram, w: &mut Work) -> ContractWorkResult<[usize;4]> {
    let state = WorkState::new(w);
    state.finish(c.validate_audited(Audit(Some(&state))).map(|f|
        [f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count]))
}
fn count_starts(calls: &[u64]) -> usize {
    calls.windows(6).filter(|w| *w == [0,96,170496,358,1,1]).count()
}
fn expected(c: &CapturedProgram) -> std::result::Result<(),String> {
    let old: old::CapturedProgram = serde_json::from_slice(&serde_json::to_vec(c).unwrap()).unwrap();
    old.validate().map_err(|e| e.0)
}
fn parity(c: &CapturedProgram, hit: bool) {
    let want = expected(c);
    assert_eq!(c.validate().map_err(|e|e.0),want);
    let mut work = Work::default();
    closed_value::native_outputs::begin();
    let got = facts(c,&mut work).map(|_|()).map_err(|e|match e {
        ContractWorkError::Contract(e)=>e.0, other=>panic!("unexpected {other:?}")
    });
    let outputs = closed_value::native_outputs::take();
    assert_eq!(got,want);
    // Admission marker is a paid closed-branch root, not a proof API. It occurs
    // before V1 even for a semantic error; native-conversion errors stay misses.
    assert_eq!(work.calls.windows(3).any(|w|w==[128,2240,12]),hit);
    if got.is_ok() {
        assert_eq!(count_starts(&work.calls),usize::from(!hit));
        assert_eq!(outputs.len(),if hit {2} else {0});
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_complete_hit_removes_only_count_and_keeps_exact_facts() {
    for value in [fixture::minimal(),complete_fixture(),deepest_valid_fixture()] {
        let c = rebind(value,None);
        let mut work = Work::default();
        closed_value::native_outputs::begin();
        let actual = facts(&c,&mut work).unwrap();
        let outputs = closed_value::native_outputs::take();
        let original = c.program.validate_audited(Audit::none()).unwrap();
        assert_eq!(actual,[original.total_nodes,original.action_prefix_nodes,original.canonical_len,original.action_count]);
        assert_eq!(outputs.len(),2);
        assert_eq!(actual[2],outputs[0].0.len());
        assert_eq!(outputs[0].0,old::canonical_bytes(&old_app(&c.program)).unwrap());
        assert_ne!(outputs[0].1,outputs[1].1,"Program Vec stays live through Semantic output");
        assert!(outputs[0].0.len()>outputs[1].0.len());
        eprintln!("source-wire-hit {}",json!({"facts":actual,"calls":work.calls}));
        assert_eq!(count_starts(&work.calls),0,"complete hit must remove the duplicate native count");
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_ordinary_and_miss_keep_original_traces() {
    for value in [fixture::minimal(),complete_fixture()] {
        let app: AppDefinition = serde_json::from_value(value.clone()).unwrap();
        let mut work = Work::default();
        let state = WorkState::new(&mut work);
        let f = state.finish(app.validate_audited(Audit(Some(&state)))).unwrap();
        assert_eq!(count_starts(&work.calls),1);
        eprintln!("source-wire-ordinary {}",json!({"facts":[f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count],"calls":work.calls}));
        let mut source = serde_json::to_value(&app).unwrap();
        if source["views"][0]["kind"]["selection"].is_null() {
            source["views"][0]["kind"].as_object_mut().unwrap().remove("selection");
        } else {
            source["views"][0]["kind"]["controls"][1].as_object_mut().unwrap().remove("on_change");
        }
        let c = rebind(value,Some(serde_json::to_vec(&source).unwrap()));
        let mut work = Work::default();
        let f = facts(&c,&mut work).unwrap();
        assert_eq!(count_starts(&work.calls),1);
        eprintln!("source-wire-miss {}",json!({"facts":f,"calls":work.calls}));
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_integer_categories_and_semantic_limits() {
    for n in [i64::MIN,-1,0,1,i64::MAX] {
        let mut v=fixture::minimal();
        v["observables"][0]["value"]=json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}});
        parity(&rebind(v,None),true);
    }
    for n in [i32::MIN,-719162,0,2932896,i32::MAX] {
        let mut v=fixture::minimal();
        v["observables"][0]["value"]=json!({"kind":"literal","value_type":{"kind":"date"},"value":{"kind":"date","days":n}});
        parity(&rebind(v,None),true);
    }
    for n in [0,1,u32::MAX] {
        let mut v=fixture::minimal();v["version"]=json!(n);parity(&rebind(v,None),true);
    }
    for n in [0,1,MAX_COLLECTION,MAX_COLLECTION+1,usize::MAX] {
        let mut v=fixture::minimal();v["views"][0]["kind"]["rows"]["limit"]=json!(n);parity(&rebind(v,None),true);
    }
    // Actual enclosing integer field, not isolated Number coercion.
    for raw in ["-0","-0.0","0.0","1e0","1e20","1e-9999","-1e-9999","18446744073709551616","-9223372036854775809","1e9999","01","+1"] {
        let mut v=fixture::minimal();
        let zero = ["-0","-0.0","0.0","1e-9999","-1e-9999"].contains(&raw);
        v["views"][0]["kind"]["rows"]["limit"]=json!(if zero {0} else {1});
        let needle=format!("\"limit\":{}",if zero {0} else {1});
        let bytes=serde_json::to_string(&v).unwrap().replace(&needle,&format!("\"limit\":{raw}")).into_bytes();
        parity(&rebind(v,Some(bytes)),false);
    }
    for (kind, field, typ) in [("integer","value","integer"),("date","days","date")] {
        for token in ["-0","-0.0","0.0","0e0","1e-9999"] {
            let mut v=fixture::minimal();
            let mut number=json!({"kind":kind});number[field]=json!(0);
            v["observables"][0]["value"]=json!({"kind":"literal","value_type":{"kind":typ},"value":number});
            let needle=format!("\"{field}\":0");
            let raw=serde_json::to_string(&v).unwrap().replace(&needle,&format!("\"{field}\":{token}")).into_bytes();
            parity(&rebind(v,Some(raw)),false);
        }
    }
    for (n,token) in [(1000usize,"1e3"),(9223372036854775808usize,"9.223372036854776e18")] {
        let mut v=fixture::minimal();v["views"][0]["kind"]["rows"]["limit"]=json!(n);
        let raw=serde_json::to_string(&v).unwrap().replace(&format!("\"limit\":{n}"),&format!("\"limit\":{token}")).into_bytes();
        parity(&rebind(v,Some(raw)),false);
    }
    for token in ["1.0","1e0"] {
        let v=fixture::minimal();let raw=serde_json::to_string(&v).unwrap().replace("\"version\":1",&format!("\"version\":{token}")).into_bytes();
        parity(&rebind(v,Some(raw)),false);
    }
    let mut v=fixture::minimal();
    v["views"][0]["kind"]["rows"]["limit"]=json!(i64::MAX as u64+1);
    parity(&rebind(v,None),true);
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_strings_escaped_keys_and_malformed_intake() {
    let mut label=String::from("X");
    label.extend((0u8..=31).map(char::from));
    label.push_str("\"\\/ASCII\u{7f}\u{85}éβ🦀\u{2028}\u{2029}e\u{301}");
    let mut v=fixture::minimal();v["label"]=json!(label);
    let compact=serde_json::to_string(&v).unwrap();
    let escaped=compact.replace('é',"\\u00e9").replace('🦀',"\\ud83e\\udd80")
        .replace("\"version\"","\"\\u0076ersion\"").replace('/',"\\/")
        .replace("\\n","\\u000a").replace("\\r","\\u000d").replace("\\t","\\u0009")
        .replace("\\b","\\u0008").replace("\\f","\\u000c");
    let reversed=format!("{{{}}}",v.as_object().unwrap().iter().rev().map(|(k,v)|
        format!("{} : {}",serde_json::to_string(k).unwrap(),serde_json::to_string(v).unwrap())).collect::<Vec<_>>().join(", "));
    for raw in [compact.into_bytes(),escaped.into_bytes(),serde_json::to_vec_pretty(&v).unwrap(),reversed.into_bytes()] {
        let c=rebind(v.clone(),Some(raw));parity(&c,true);
        let f=facts(&c,&mut Work::default()).unwrap();
        assert_eq!(f[2],serde_json::to_vec(&c.program).unwrap().len());
        assert!(f[2]<=c.source_bytes.len());
    }
    for raw in [br#"{"version":1,"\u0076ersion":1}"#.to_vec(),br#"{"label":"\uD800"}"#.to_vec(),vec![0xff],br#"{"label":"\uDC00"}"#.to_vec()] {
        let mut c=rebind(fixture::minimal(),Some(raw));c.program.version=99;parity(&c,false);
    }
    for (name,v) in fixture::semantic_negatives() {
        if let Ok(app)=serde_json::from_value::<AppDefinition>(v.clone()) {
            let c=rebind(serde_json::to_value(app).unwrap(),None);
            assert!(expected(&c).is_err(),"{name}");parity(&c,true);
        }
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_raw_boundaries_and_exact_extension_length() {
    let c=rebind(fixture::minimal(),None);
    for n in [MAX_WIRE_BYTES-1,MAX_WIRE_BYTES,MAX_WIRE_BYTES+1] {
        let mut raw=c.source_bytes.clone();raw.resize(n,b' ');
        let c=rebind(fixture::minimal(),Some(raw));
        let mut work=Work::default();
        let f=facts(&c,&mut work);
        if n>MAX_WIRE_BYTES { assert_eq!(f,Err(ContractWorkError::Contract(ContractError("source exceeds byte limit".into())))); }
        else {
            assert_eq!(f.unwrap()[2],serde_json::to_vec(&c.program).unwrap().len());
            let data=DataSnapshot::empty("project",&c.program).unwrap();
            let expr=c.program.observables[0].value.clone();
            with_capture_validation(&c,&data,&mut Work::default(),|ctx|->ContractWorkResult<()> {
                ctx.validate_extension("row","row",&expr)
            }).unwrap();
            // The original whole-App augmentation must agree, while raw size
            // at the cap cannot be used as an exact canonical length.
            let mut augmented=old_app(&c.program);
            let old_expr: old::Expr=serde_json::from_slice(&serde_json::to_vec(&expr).unwrap()).unwrap();
            augmented.actions.push(old::ActionDefinition {
                id:"gm_scope_projection".into(),label:"Host projection type check".into(),
                parameters:BTreeMap::from([("row".into(),old::Type::reference("row"))]),
                guards:vec![],ensures:vec![],steps:vec![old::Statement::Assert {
                    condition:old::Expr::Equal {left:Box::new(old_expr.clone()),right:Box::new(old_expr)},
                    message:"Projection type check".into(),
                }],
            });
            augmented.validate().unwrap();
            let compact_len=old::canonical_bytes(&old_app(&c.program)).unwrap().len();
            let augmented_len=old::canonical_bytes(&augmented).unwrap().len();
            assert!(augmented_len<=MAX_WIRE_BYTES);
            assert!(n+augmented_len-compact_len>MAX_WIRE_BYTES,"raw upper bound is not an exact fact");
        }
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_identity_error_order_and_failure_publication() {
    let c=rebind(fixture::minimal(),None);
    let other=rebind(complete_fixture(),None);
    let data=DataSnapshot::empty("project",&c.program).unwrap();
    for (fault,output_count) in [(0,0),(1,0),(2,1),(3,2)] {
        let mut bad=c.clone();
        match fault {
            0=>{bad.program.version=99;let mut v=fixture::minimal();v["version"]=json!(99);bad.source_bytes=serde_json::to_vec(&v).unwrap();},
            1=>bad.artifact.raw_digest=other.artifact.raw_digest.clone(),
            2=>{bad.artifact.program_digest=other.artifact.program_digest.clone();bad.artifact.semantic_digest=other.artifact.semantic_digest.clone();},
            _=>bad.artifact.semantic_digest=other.artifact.semantic_digest.clone(),
        }
        let want=expected(&bad).unwrap_err();
        closed_value::native_outputs::begin();
        let error=with_capture_validation(&bad,&data,&mut Work::default(),|ctx|->ContractWorkResult<()> {
            let e=ctx.ensure_source_and_data().unwrap_err();assert!(ctx.facts.is_none());
            assert_eq!(e,ContractWorkError::Contract(ContractError(want.clone())));Ok(())
        }).unwrap_err();
        assert_eq!(error,ContractWorkError::Contract(ContractError(want)));
        assert_eq!(closed_value::native_outputs::take().len(),output_count);
    }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_every_callback_refuses_without_early_facts() {
    let c=rebind(fixture::minimal(),None);
    let data=DataSnapshot::empty("project",&c.program).unwrap();
    let mut trace=Work::default();source_result(&c,&mut trace).unwrap();
    // Exhaustive small-source callbacks include the owner, both polls, V1,
    // finalizer, all surviving outputs/hashes, and existing publication.
    for index in 0..=trace.calls.len() {
        for cause in [WorkFailure::Cancelled,WorkFailure::BudgetExhausted("wire-bound refusal")] {
            let mut work=Work{refuse:Some(index+5),cause:Some(cause.clone()),..Work::default()};
            let result=with_capture_validation(&c,&data,&mut work,|ctx|->ContractWorkResult<()> {
                assert!(ctx.ensure_source_and_data().is_err());assert!(ctx.facts.is_none());Ok(())
            });
            assert_eq!(result,Err(ContractWorkError::Resource(cause)));
            assert_eq!(&work.calls[4..4+index.min(trace.calls.len())],&trace.calls[..index.min(trace.calls.len())]);
        }
    }
    for next in [c.clone(),rebind(fixture::minimal(),None)] { parity(&next,true); }
}

#[test]
#[cfg(target_pointer_width="64")]
fn source_wire_owner_and_finalizer_exact_one_below_packets() {
    let c=rebind(fixture::minimal(),None);
    let data=DataSnapshot::empty("project",&c.program).unwrap();
    let mut trace=Work::default();source_result(&c,&mut trace).unwrap();
    let owner=trace.calls.windows(3).position(|w|w==[128,2240,12]).unwrap();
    let bound=trace.calls.windows(5).position(|w|w==[0,128,0,3,1]).unwrap()+1;
    let finalizer=trace.calls.windows(5).position(|w|w==[0,128,5,0,96]).unwrap()+1;
    for index in [owner,owner+1,bound,finalizer,finalizer+1] {
        let mut context=Work::default();
        with_capture_validation(&c,&data,&mut context,|ctx|->ContractWorkResult<()> {ctx.ensure_source_and_data()}).unwrap();
        let position=index+4;
        let prefix=context.calls[..position].iter().sum::<u64>();
        let amount=context.calls[position];
        assert!(amount>0);
        for allowance in [amount-1,amount] {
            let mut work=Work{left:Some(prefix+allowance),..Work::default()};
            closed_value::native_outputs::begin();
            let result=with_capture_validation(&c,&data,&mut work,|ctx|->ContractWorkResult<()> {
                assert!(ctx.ensure_source_and_data().is_err());assert!(ctx.facts.is_none());Ok(())
            });
            assert!(matches!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted(_)))));
            assert_eq!(&work.calls[..=position],&context.calls[..=position]);
            if allowance<amount { assert_eq!(work.calls.len(),position+1); }
            else { assert!(work.calls.len()>position+1); }
            assert_eq!(closed_value::native_outputs::take().len(),usize::from(index>=finalizer));
        }
    }
}
