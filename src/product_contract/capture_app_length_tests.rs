//! App-only complete native count controls. This child can inspect the actual
//! private counter and reserve seam; it contains no production stubs or hooks.
use super::*;
use serde_json::{json, Value};
use std::io::Write;

#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;
#[path = "../../tests/support/product_contract_fixture.rs"]
mod rich;
#[path = "../../tests/support/capture_native_oracle/product_contract.rs"]
mod old;

#[derive(Default)]
struct Work {
    calls: Vec<u64>,
    left: Option<u64>,
    refuse: Option<usize>,
    cause: Option<WorkFailure>,
}
impl ContractWork for Work {
    fn charge(&mut self, n: u64) -> std::result::Result<(), WorkFailure> {
        self.calls.push(n);
        if self.refuse == Some(self.calls.len()) {
            return Err(self.cause.clone().unwrap_or(WorkFailure::Cancelled));
        }
        if let Some(left) = &mut self.left {
            *left = left.checked_sub(n)
                .ok_or(WorkFailure::BudgetExhausted("app length test fuel"))?;
        }
        Ok(())
    }
}
fn counted(app: &AppDefinition, work: &mut Work) -> ContractWorkResult<usize> {
    let state = WorkState::new(work);
    state.finish(app_canonical_len_audited(app, Audit(Some(&state))))
}
fn validated(app: &AppDefinition, work: &mut Work) -> ContractWorkResult<usize> {
    let state = WorkState::new(work);
    state.finish(app.validate_audited(Audit(Some(&state))).map(|facts| facts.canonical_len))
}
fn old_app(app: &AppDefinition) -> old::AppDefinition {
    serde_json::from_slice(&serde_json::to_vec(app).unwrap()).unwrap()
}
fn yes() -> Value { rich::yes() }
fn integer(n: i64) -> Value {
    json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}})
}
fn ints() -> Value {
    json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"integer"}},
        "value":{"kind":"list","item_type":{"kind":"integer"},"items":[{"kind":"integer","value":1}]}})
}
fn query() -> Value {
    json!({"kind":"query","entity":"row","binding":"item","predicate":yes(),
        "sort":[{"value":rich::field(rich::var("item"),"name"),"descending":true}],
        "limit":MAX_COLLECTION,"include_archived":true})
}
fn literal(typ: Value, value: Value) -> Value {
    json!({"kind":"literal","value_type":typ,"value":value})
}
fn expression_cases() -> Vec<Value> {
    vec![
        yes(), integer(0), integer(i64::MIN), integer(i64::MAX),
        rich::text(""), rich::text("ASCII quote\" slash\\\n\r\t\u{8}\u{c} βé🦀"),
        rich::text(&(0u8..=31).map(char::from).collect::<String>()),
        literal(json!({"kind":"boolean"}),json!({"kind":"boolean","value":false})),
        literal(json!({"kind":"date"}),json!({"kind":"date","days":-719162})),
        literal(json!({"kind":"date"}),json!({"kind":"date","days":2932896})),
        literal(json!({"kind":"reference","entity":"row"}),json!({"kind":"reference","entity":"row","record":"r1"})),
        literal(json!({"kind":"optional","item":{"kind":"text"}}),json!({"kind":"null"})),
        literal(json!({"kind":"optional","item":{"kind":"text"}}),json!({"kind":"text","value":"some"})),
        literal(json!({"kind":"list","item":{"kind":"integer"}}),json!({"kind":"list","item_type":{"kind":"integer"},"items":[]})),
        ints(), json!({"kind":"state","state":"flag"}), json!({"kind":"today"}), query(),
        json!({"kind":"filter","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"map","items":ints(),"binding":"x","value":rich::var("x")}),
        json!({"kind":"count","items":ints()}),
        json!({"kind":"any","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"all","items":ints(),"binding":"x","predicate":yes()}),
        json!({"kind":"sum","items":ints()}),
        json!({"kind":"contains","items":ints(),"value":integer(1)}),
        json!({"kind":"equal","left":integer(1),"right":integer(2)}),
        json!({"kind":"less","left":integer(1),"right":integer(2)}),
        json!({"kind":"add","left":integer(1),"right":integer(2)}),
        json!({"kind":"subtract","left":integer(1),"right":integer(2)}),
        json!({"kind":"concat","left":rich::text("a"),"right":rich::text("b")}),
        json!({"kind":"text_contains","text":rich::text("abc"),"search":rich::text("")}),
        json!({"kind":"lower","value":rich::text("ΟΣ")}),
        json!({"kind":"and","values":[yes(),yes()]}),
        json!({"kind":"or","values":[yes(),yes()]}),
        json!({"kind":"not","value":yes()}),
        json!({"kind":"if","condition":yes(),"then_value":integer(1),"else_value":integer(2)}),
        json!({"kind":"coalesce","value":literal(json!({"kind":"optional","item":{"kind":"text"}}),json!({"kind":"null"})),"fallback":rich::text("x")}),
        json!({"kind":"date_add","date":{"kind":"today"},"days":integer(1)}),
        json!({"kind":"date_difference","later":{"kind":"today"},"earlier":{"kind":"today"}}),
    ]
}
fn complete_fixture() -> Value {
    let mut app = fixture::minimal();
    app["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"numbers","label":"Numbers","value_type":{"kind":"list","item":{"kind":"integer"}}}));
    app["entities"][0]["unique"] = json!([["name"]]);
    app["entities"][0]["constraints"] = json!([yes()]);
    app["state"] = json!([
        {"id":"flag","label":"Flag","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":true}},
        {"id":"search","label":"Search","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}},
        {"id":"selected","label":"Selected","value_type":{"kind":"list","item":{"kind":"reference","entity":"row"}},"initial":{"kind":"list","item_type":{"kind":"reference","entity":"row"},"items":[]}},
        {"id":"numbers","label":"Numbers","value_type":{"kind":"list","item":{"kind":"integer"}},"initial":{"kind":"list","item_type":{"kind":"integer"},"items":[{"kind":"integer","value":i64::MIN},{"kind":"integer","value":0},{"kind":"integer","value":i64::MAX}]}}
    ]);
    app["outputs"] = json!([
        {"id":"csv","label":"CSV","format":"csv","columns":[{"id":"number","label":"Number","value_type":{"kind":"integer"}}]},
        {"id":"json","label":"JSON","format":"json","columns":[{"id":"number","label":"Number","value_type":{"kind":"integer"}}]}
    ]);
    let actions = app["actions"].as_array_mut().unwrap();
    actions.push(json!({"id":"change_search","label":"Search changed","parameters":{"value":{"kind":"text"}},"guards":[],"steps":[{"kind":"set_state","state":"search","value":rich::var("value")}],"ensures":[]}));
    actions.push(json!({"id":"change_selection","label":"Selection changed","parameters":{"value":{"kind":"list","item":{"kind":"reference","entity":"row"}}},"guards":[],"steps":[{"kind":"set_state","state":"selected","value":rich::var("value")}],"ensures":[]}));
    actions.push(json!({"id":"mutate","label":"Mutate","parameters":{"record":{"kind":"reference","entity":"row"}},"guards":[yes()],"steps":[
        {"kind":"create","entity":"row","values":{"numbers":ints(),"name":rich::text("created")},"bind":"created"},
        {"kind":"update","record":rich::var("record"),"values":{"name":rich::text("updated")}},
        {"kind":"archive","record":rich::var("created")},
        {"kind":"collection","target":{"kind":"state","state":"numbers"},"operation":"insert","items":ints()},
        {"kind":"collection","target":{"kind":"state","state":"numbers"},"operation":"remove","items":ints()},
        {"kind":"collection","target":{"kind":"field","record":rich::var("record"),"field":"numbers"},"operation":"insert","items":ints()},
        {"kind":"collection","target":{"kind":"field","record":rich::var("record"),"field":"numbers"},"operation":"remove","items":ints()},
        {"kind":"for_each","items":ints(),"binding":"number","steps":[{"kind":"assert","condition":{"kind":"equal","left":rich::var("number"),"right":integer(1)},"message":"number"}]},
        {"kind":"emit","output":"csv","items":ints(),"binding":"number","columns":{"number":rich::var("number")}},
        {"kind":"emit","output":"json","items":ints(),"binding":"number","columns":{"number":rich::var("number")}}
    ],"ensures":[yes()]}));
    app["observables"] = Value::Array(expression_cases().into_iter().enumerate().map(|(i,value)| {
        json!({"id":format!("case_{i}"),"label":format!("Case {i}"),"value":value})
    }).collect());
    app["views"][0]["kind"]["controls"] = json!([
        {"id":"search_input","label":"Search","state":"search","on_change":"change_search"},
        {"id":"flag_input","label":"Flag","state":"flag","on_change":null}
    ]);
    app["views"][0]["kind"]["selection"] = json!({"id":"selection","state":"selected","on_change":"change_selection"});
    app["views"][0]["actions"] = json!([
        {"id":"toolbar","label":"Toolbar","placement":"toolbar","action":"check","arguments":{},"enabled":yes()},
        {"id":"row","label":"Row","placement":"row","action":"mutate","arguments":{"record":rich::var("row")},"enabled":yes()}
    ]);
    app["views"][0]["keys"] = json!([
        {"key":"a","modifiers":["control","alt","shift","command"],"binding":"toolbar"},
        {"key":"b","modifiers":[],"binding":"row"}
    ]);
    app["views"].as_array_mut().unwrap().push(json!({"id":"detail","label":"Detail","kind":{"kind":"detail","entity":"row","record":literal(json!({"kind":"reference","entity":"row"}),json!({"kind":"reference","entity":"row","record":"r1"})),"columns":[{"id":"name","label":"Name","value":rich::field(rich::var("row"),"name")}]},"actions":[],"keys":[]}));
    app["views"].as_array_mut().unwrap().push(json!({"id":"form","label":"Form","kind":{"kind":"form","action":"change_search","fields":[{"parameter":"value","label":"Value"}],"defaults":{"value":{"kind":"text","value":"default"}}},"actions":[],"keys":[]}));
    app
}
fn deepest_valid_fixture() -> Value {
    let mut app = fixture::minimal();
    let mut typ = json!({"kind":"integer"});
    let mut value = json!({"kind":"integer","value":0});
    for _ in 0..MAX_TYPE_DEPTH {
        value = json!({"kind":"list","item_type":typ,"items":[value]});
        typ = json!({"kind":"list","item":typ});
    }
    app["state"] = json!([{"id":"nested","label":"Nested","value_type":typ,"initial":value}]);
    let mut expr = yes();
    for _ in 0..MAX_EXPR_DEPTH { expr = json!({"kind":"not","value":expr}); }
    app["observables"][0]["value"] = expr;
    let mut steps = json!([{"kind":"assert","condition":yes(),"message":"deep"}]);
    // One shared Statement/Expr depth: deepest Assert is15, its literal is16.
    for i in 0..MAX_EXPR_DEPTH-1 {
        steps = json!([{"kind":"for_each","items":ints(),"binding":format!("nested_{i}"),"steps":steps}]);
    }
    app["actions"][0]["steps"] = steps;
    app
}
fn reordered_source(value: &Value) -> Vec<u8> {
    let pairs = value.as_object().unwrap().iter().rev().map(|(key,value)| {
        format!("{}:{}",serde_json::to_string(key).unwrap(),serde_json::to_string(value).unwrap())
    }).collect::<Vec<_>>();
    format!("{{{}}}",pairs.join(",")).into_bytes()
}
fn json_depth(value: &Value) -> usize {
    match value {
        Value::Array(items) => 1 + items.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(items) => 1 + items.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[test]
fn app_length_closed_schema_and_prerequisites_match_frozen_native() {
    for value in [fixture::minimal(),rich::organizer(),rich::equipment(),rich::new_composition(),complete_fixture(),deepest_valid_fixture()] {
        let mut originals = Vec::new();
        for bytes in [serde_json::to_vec(&value).unwrap(),serde_json::to_vec_pretty(&value).unwrap(),reordered_source(&value)] {
            let app = AppDefinition::parse(&bytes).unwrap();
            let before = app.clone();
            let old = old_app(&app);
            old.validate().unwrap(); // Real full semantic prerequisite before count.
            let expected = old::canonical_bytes(&old).unwrap();
            let mut work = Work::default();
            assert_eq!(counted(&app,&mut work),Ok(expected.len()));
            assert_eq!(app,before);
            assert_eq!(serde_json::to_vec(&app).unwrap(),serde_json::to_vec(&before).unwrap());
            assert_eq!(canonical_bytes(&app).unwrap(),expected);
            assert_eq!(app.identity().unwrap().as_str(),old.identity().unwrap().as_str());
            assert_eq!(app.semantic_identity().unwrap().as_str(),old.semantic_identity().unwrap().as_str());
            assert_eq!(work.calls.first(),Some(&0));
            assert_eq!(work.calls.last(),Some(&0));
            assert_eq!(work.calls.iter().filter(|&&n|n==0).count(),2);
            originals.push((app,expected.len()));
        }
        assert_eq!(originals[0],originals[1]);
        assert_eq!(originals[1],originals[2]);
    }
    // Deliberately invalid scalar bounds are shallow direct-serialization cases,
    // never full-validator or arbitrary unchecked-App coverage. Their fixed full
    // JSON container depth is explicitly below the reviewed H74 envelope.
    for days in [i32::MIN,i32::MAX] {
        let mut value = fixture::minimal();
        value["version"] = json!(u32::MAX);
        value["state"] = json!([{"id":"date","label":"Date","value_type":{"kind":"date"},"initial":{"kind":"date","days":days}}]);
        value["views"][0]["kind"]["rows"]["limit"] = json!(usize::MAX);
        assert!(json_depth(&value)<74);
        let app: AppDefinition = serde_json::from_value(value).unwrap();
        assert_eq!(old_app(&app).validate().unwrap_err().0,"unsupported contract version");
        assert_eq!(counted(&app,&mut Work::default()).unwrap(),old::canonical_bytes(&old_app(&app)).unwrap().len());
    }
}

#[test]
fn app_length_identity_targets_and_ordinary_paths_stay_distinct() {
    let app: AppDefinition = serde_json::from_value(complete_fixture()).unwrap();
    let mut relabeled = app.clone();
    relabeled.label.push('!');
    for candidate in [&app,&relabeled] {
        let old = old_app(candidate);
        let ordinary = candidate.validate_audited(Audit::none()).unwrap();
        assert_eq!(ordinary.canonical_len,old::canonical_bytes(&old).unwrap().len());
        let mut cleared = candidate.clone();
        cleared.clear_display_labels_audited(Audit::none()).unwrap();
        let semantic_bytes = old::canonical_bytes(&old_app(&cleared)).unwrap();
        assert_ne!(semantic_bytes,old::canonical_bytes(&old).unwrap());
        assert_eq!(canonical_bytes(&cleared).unwrap(),semantic_bytes);
        assert_eq!(candidate.semantic_identity().unwrap().as_str(),old.semantic_identity().unwrap().as_str());
        let bytes = serde_json::to_vec(candidate).unwrap();
        let capture = CapturedProgram::capture(&bytes,"project",Producer::UserAuthored,None).unwrap();
        let old_capture: old::CapturedProgram = serde_json::from_slice(&serde_json::to_vec(&capture).unwrap()).unwrap();
        assert_eq!(capture.validate().map_err(|e|e.0),old_capture.validate().map_err(|e|e.0));
    }
    assert_ne!(app.identity().unwrap(),relabeled.identity().unwrap());
    assert_eq!(app.semantic_identity().unwrap(),relabeled.semantic_identity().unwrap());
    // Ordinary-path branch entry/Vec lifetime is source-assisted; no observer is added.
}

#[test]
#[cfg(target_pointer_width = "64")]
fn app_length_counter_keeps_full_chunks_and_native_err_before_poll() {
    let mut work = Work::default();
    {
        let state = WorkState::new(&mut work);
        let mut counter = AppLengthCounter {audit:Audit(Some(&state)),size:0,overflowed:false};
        assert_eq!(counter.write(&[]).unwrap(),0);
        counter.write_all(&[]).unwrap();
        assert_eq!(counter.write(b"abc").unwrap(),3);
        counter.write_all(&[]).unwrap();
        counter.write_all(b"12345").unwrap();
        let beyond = vec![b'x';MAX_WIRE_BYTES+1];
        assert_eq!(counter.write(&beyond).unwrap(),beyond.len());
        counter.write_all(b"tail").unwrap();
        let before = counter.size;
        counter.flush().unwrap();
        assert_eq!(counter.size,before);
        assert_eq!(counter.size,MAX_WIRE_BYTES+13);
        assert!(!counter.overflowed);
        assert_eq!(state.finish(counter.finish(Ok(()))),Ok(MAX_WIRE_BYTES+13));
    }
    assert_eq!(work.calls,vec![0]); // Only finish's success poll; no per-chunk callback.
    let app: AppDefinition = serde_json::from_value(fixture::minimal()).unwrap();
    app.validate().unwrap();
    for native in [false,true] {
        let mut work = Work::default();
        let outcome = {
            let state = WorkState::new(&mut work);
            let a = Audit(Some(&state));
            let shape = Shape {audit:a,bytes:1076,nodes:158,pairs:75,native:7303,canonical:false,digest_copies:0};
            reserve_app_count(&shape).unwrap(); // Actual ErrorImpl/FMT ownership reserve.
            let mut counter = AppLengthCounter {audit:a,size:usize::MAX-1,overflowed:false};
            assert_eq!(counter.write(b"x").unwrap(),1);
            let result = if native {
                let result = serde_json::to_writer(&mut counter,&app);
                assert!(result.as_ref().unwrap_err().is_io());
                result
            } else {
                assert!(counter.write(b"x").is_err());
                Ok(()) // A forged later success cannot erase an actual sink failure.
            };
            assert_eq!(counter.size,usize::MAX);
            assert!(counter.overflowed);
            assert!(a.pay(0).is_err()); // Typed sticky slot already set before finish.
            assert!(counter.write_all(&[]).is_err());
            assert!(counter.write(b"later").is_err());
            assert_eq!(counter.size,usize::MAX);
            state.finish(counter.finish(result))
        };
        assert_eq!(outcome,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));
        assert_eq!(work.calls,vec![96,320,1855,8,260,5,101,50671]);
    }
    // Defensive display branch only: a finite independently produced native Err,
    // not evidence that a well-formed App spontaneously fails serialization.
    let error = serde_json::from_str::<Value>("{").unwrap_err();
    let expected = error.to_string();
    let mut work = Work::default();
    let outcome = {
        let state = WorkState::new(&mut work);
        let a = Audit(Some(&state));
        reserve_app_count(&Shape {audit:a,bytes:1076,nodes:158,pairs:75,native:7303,canonical:false,digest_copies:0}).unwrap();
        let counter = AppLengthCounter {audit:a,size:17,overflowed:false};
        state.finish(counter.finish(Err(error)))
    };
    assert_eq!(outcome,Err(ContractWorkError::Contract(ContractError(expected))));
    assert_eq!(work.calls,vec![96,320,1855,8,260,5,101,50671]);
}

// Independent literal transcribed in the reviewed LITERAL-CONTROLS.json.
const MINIMAL_LITERAL: &str = r#"{"version":1,"id":"source_fixture","label":"Source fixture","entities":[{"id":"row","label":"Rows","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}],"state":[],"actions":[{"id":"check","label":"Check","parameters":{},"guards":[],"steps":[{"kind":"assert","condition":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"message":"yes"}],"ensures":[]}],"outputs":[],"observables":[{"id":"yes","label":"Yes","value":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}}],"views":[{"id":"rows","label":"Rows","kind":{"kind":"list","entity":"row","rows":{"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":1,"include_archived":false},"columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],"controls":[],"selection":null},"actions":[],"keys":[]}],"initial_view":"rows"}"#;

#[test]
#[cfg(target_pointer_width = "64")]
fn app_length_literal_payments_and_each_new_boundary_refuse_exactly() {
    let literal: Value = serde_json::from_str(MINIMAL_LITERAL).unwrap();
    assert_eq!(literal,fixture::minimal());
    let app: AppDefinition = serde_json::from_value(literal).unwrap();
    app.validate().unwrap();
    assert_eq!(old::canonical_bytes(&old_app(&app)).unwrap().len(),1076);
    assert_eq!(size_of::<usize>(),8);
    assert_eq!(size_of::<Shape<'_>>(),56);
    assert_eq!(size_of::<AppLengthCounter<'_>>(),24);
    assert_eq!(size_of::<serde_json::Serializer<&mut AppLengthCounter<'_>>>(),8);
    assert_eq!(size_of::<serde_json::Result<()>>(),8);
    // H/R/STACK and ROOT are independently reviewed literals, not measured frames.
    assert_eq!(8+3*(MAX_EXPR_DEPTH+1)+2*(MAX_TYPE_DEPTH+1)+(MAX_TYPE_DEPTH+1),74);
    assert_eq!(64*size_of::<usize>()+64,576);
    assert_eq!(4*74*576,170496);
    assert_eq!(12+2*size_of::<AppLengthCounter<'_>>()+2*size_of::<serde_json::Serializer<&mut AppLengthCounter<'_>>>()+2*size_of::<serde_json::Result<()>>(),92);
    let mut descriptor_work = Work::default();
    {
        let state = WorkState::new(&mut descriptor_work);
        let a = Audit(Some(&state));
        let mut shape = Shape::new(a).unwrap();
        a.pay(1).unwrap();
        shape.canonical = false;
        app.serialize(&mut shape).unwrap();
        assert_eq!((shape.bytes,shape.nodes,shape.pairs,shape.native,shape.digest_copies),(1076,158,75,7303,0));
    }
    assert_eq!(descriptor_work.calls.iter().sum::<u64>(),68+1+13395);
    let mut exact = Work {left:Some(237372),..Work::default()};
    assert_eq!(counted(&app,&mut exact),Ok(1076));
    assert_eq!(exact.left,Some(0));
    assert_eq!(exact.calls.iter().sum::<u64>(),237372);
    assert_eq!(&exact.calls[..5],&[0,96,170496,68,1]);
    assert!(exact.calls.ends_with(&[96,320,1855,8,260,5,101,50671,0]));
    assert_eq!(&exact.calls[3..exact.calls.len()-9],descriptor_work.calls.as_slice());
    assert_eq!(exact.calls[..exact.calls.len()-2].iter().sum::<u64>(),186701);
    let mut short = Work {left:Some(237371),..Work::default()};
    assert_eq!(counted(&app,&mut short),Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("app length test fuel"))));
    assert_eq!(short.left,Some(50670));
    assert_eq!(short.calls.as_slice(),&exact.calls[..exact.calls.len()-1]);
    assert_eq!(short.calls.last(),Some(&50671));
    // Initial poll, stack arithmetic/storage, Shape/direct flag, first object,
    // first key/string byte, and actual first sequence/container/map callbacks.
    let mut boundaries = vec![0,1,2,3,4,5,6,7,8,9,10,11,12];
    for amount in [18,20,34] {
        boundaries.push(exact.calls.iter().position(|&n|n==amount).unwrap());
    }
    // Reserve arithmetic, display descriptor/FMT/copy, Box descriptor/owner,
    // final native aggregate and success-only poll: every new tail payment.
    boundaries.extend(exact.calls.len()-9..exact.calls.len());
    boundaries.sort_unstable();
    boundaries.dedup();
    for index in boundaries {
        for cause in [WorkFailure::Cancelled,WorkFailure::BudgetExhausted("boundary refusal")] {
            let mut work = Work {refuse:Some(index+1),cause:Some(cause.clone()),..Work::default()};
            let result = {
                let state = WorkState::new(&mut work);
                let a = Audit(Some(&state));
                let result = app_canonical_len_audited(&app,a);
                assert!(result.is_err());
                assert!(a.pay(0).is_err()); // Even a now-cleared carrier cannot replace first cause.
                state.finish(result)
            };
            assert_eq!(result,Err(ContractWorkError::Resource(cause)),"callback {index}");
            assert_eq!(work.calls.as_slice(),&exact.calls[..=index],"callback {index}");
        }
    }
    // Native non-entry on a refused prepayment is source-assisted from immediate
    // propagation before to_writer; no event counter or native hook is invented.
}

#[test]
#[cfg(target_pointer_width = "64")]
fn app_length_checked_reserve_and_descriptor_overflow_stay_typed() {
    // Independent input witnesses for L*2, N*4, Q*4, W sums/+2, W*14,
    // and A+native/root/overflow reserve. None calls the serializer.
    let cases = [
        (u64::MAX,0,0,0), (0,u64::MAX,0,0), (0,0,u64::MAX,0),
        (u64::MAX/2,1,0,0), (u64::MAX/2-2,0,2,0),
        (u64::MAX/2,0,0,0), (u64::MAX/28+1,0,0,0),
        (0,0,0,u64::MAX), (0,0,0,u64::MAX-28), (0,0,0,u64::MAX-120),
    ];
    for (bytes,nodes,pairs,native) in cases {
        let mut work = Work::default();
        let result = {
            let state = WorkState::new(&mut work);
            let shape = Shape {audit:Audit(Some(&state)),bytes,nodes,pairs,native,canonical:false,digest_copies:0};
            state.finish(reserve_app_count(&shape))
        };
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));
        assert_eq!(work.calls,vec![96]); // Arithmetic prepaid; no error/native reserve or poll.
    }
    for which in 0..4 {
        let mut work = Work::default();
        let result = {
            let state = WorkState::new(&mut work);
            let a = Audit(Some(&state));
            let mut shape = Shape {audit:a,bytes:0,nodes:0,pairs:0,native:0,canonical:false,digest_copies:0};
            let result = match which {
                0 => {shape.bytes=u64::MAX;shape.add(1,0)},
                1 => {shape.native=u64::MAX;shape.add(0,1)},
                2 => {shape.nodes=u64::MAX;shape.node()},
                _ => {
                    shape.pairs=u64::MAX;
                    let mut object=shape.object().unwrap();
                    object.field("x",&true)
                },
            };
            assert!(a.pay(0).is_err());
            state.finish(result)
        };
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));
        assert!(!work.calls.contains(&0));
    }
    // STACK/ROOT have no runtime injection seam; their constant checked products
    // are source-reviewed, with literal ABI assertions in the preceding control.
}

#[test]
#[cfg(target_pointer_width = "32")]
fn app_length_reserve_usize_conversion_is_checked_on_32_bit_targets() {
    let mut work = Work::default();
    let result = {
        let state = WorkState::new(&mut work);
        let shape = Shape {audit:Audit(Some(&state)),bytes:u64::from(u32::MAX)+1,nodes:0,pairs:0,native:0,canonical:false,digest_copies:0};
        state.finish(reserve_app_count(&shape))
    };
    assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));
    assert_eq!(work.calls,vec![96]);
}

fn wire_app(target: usize) -> AppDefinition {
    let mut value = fixture::minimal();
    value["observables"] = Value::Array((0..MAX_ITEMS).map(|i| {
        json!({"id":format!("wire_{i}"),"label":"x","value":yes()})
    }).collect());
    let mut app: AppDefinition = serde_json::from_value(value).unwrap();
    // Frozen old bytes, not Shape/the new helper, size the legal label padding.
    let initial = old::canonical_bytes(&old_app(&app)).unwrap().len();
    let mut extra = target.checked_sub(initial).unwrap();
    for observable in &mut app.observables {
        let add = extra.min(MAX_TEXT_BYTES-1);
        observable.label.push_str(&"x".repeat(add));
        extra -= add;
    }
    assert_eq!(extra,0);
    assert_eq!(old::canonical_bytes(&old_app(&app)).unwrap().len(),target);
    app
}
#[test]
fn app_length_real_validator_wire_boundary_keeps_semantic_precedence() {
    for target in [MAX_WIRE_BYTES-1,MAX_WIRE_BYTES,MAX_WIRE_BYTES+1] {
        let app = wire_app(target);
        let expected = old_app(&app).validate().map_err(|e|e.0);
        assert_eq!(expected,if target<=MAX_WIRE_BYTES {Ok(())} else {Err("program exceeds byte budget".into())});
        // For +1 the frozen full validator reached only its final wire error,
        // establishing all earlier semantic prerequisites without a fake gate.
        assert_eq!(counted(&app,&mut Work::default()),Ok(target));
        let mut work = Work::default();
        let actual = validated(&app,&mut work);
        match expected {
            Ok(()) => {assert_eq!(actual,Ok(target));assert!(work.calls.ends_with(&[0,3,1,5]));},
            Err(error) => {
                assert_eq!(actual,Err(ContractWorkError::Contract(ContractError(error))));
                let poll = work.calls.iter().rposition(|&n|n==0).unwrap();
                assert_eq!(&work.calls[poll..poll+4],&[0,3,1,2]);
            },
        }
        assert_eq!(app,wire_app(target));
    }
    let oversized = wire_app(MAX_WIRE_BYTES+1);
    for latent in [false,true] {
        let mut app = oversized.clone();
        if latent {
            app.observables.last_mut().unwrap().value = serde_json::from_value(json!({"kind":"if","condition":yes(),"then_value":yes(),"else_value":{"kind":"variable","name":"missing"}})).unwrap();
        } else if let ViewKind::List {controls,..} = &mut app.views[0].kind {
            controls.push(StateControl {id:"bad_control".into(),label:"Bad".into(),state:"missing".into(),on_change:None});
        }
        let expected = old_app(&app).validate().unwrap_err().0;
        assert_ne!(expected,"program exceeds byte budget");
        let mut work = Work::default();
        assert_eq!(validated(&app,&mut work),Err(ContractWorkError::Contract(ContractError(expected))));
        assert!(!work.calls.windows(5).any(|w|w==[0,96,170496,68,1]));
    }
    // This is component semantic parity on a recording work carrier, NOT a10M
    // host-fit claim. Source capture's earlier raw-size gate is tested separately.
}

fn source_result(capture: &CapturedProgram, work: &mut Work) -> ContractWorkResult<()> {
    let state = WorkState::new(work);
    state.finish(capture.validate_audited(Audit(Some(&state))).map(|_|()))
}
fn capture_minimal() -> CapturedProgram {
    CapturedProgram::capture(&serde_json::to_vec(&fixture::minimal()).unwrap(),"project",Producer::UserAuthored,None).unwrap()
}
#[test]
#[cfg(target_pointer_width = "64")]
fn app_length_three_source_counts_and_publication_fail_closed() {
    let capture = capture_minimal();
    let data = DataSnapshot::empty("project",&capture.program).unwrap();
    let mut direct = Work::default();
    reset_validation_counts();
    source_result(&capture,&mut direct).unwrap();
    assert_eq!(validation_counts().captures,1);
    assert_eq!(validation_counts().apps,3);
    let starts: Vec<usize> = direct.calls.windows(5).enumerate().filter_map(|(i,w)| {
        (w==[0,96,170496,68,1]).then_some(i)
    }).collect();
    assert_eq!(starts.len(),3);
    let count_ends: Vec<usize> = starts.iter().map(|&start| {
        let offset = direct.calls[start..].windows(8).position(|w|w==[320,1855,8,260,5,101,50671,0]).unwrap();
        start+offset+6 // Actual last pre-native payment, reached through existing carrier.
    }).collect();
    let mut all = Work::default();
    with_capture_validation(&capture,&data,&mut all,|ctx|->ContractWorkResult<()> {
        ctx.ensure_source_and_data()?;
        assert_eq!(ctx.facts.as_ref().unwrap().canonical_len,1076);
        Ok(())
    }).unwrap();
    // Four existing context/run-host callbacks dominate the exact direct prefix.
    assert_eq!(&all.calls[4..4+direct.calls.len()],direct.calls.as_slice());
    let publication = 4+direct.calls.len();
    assert_eq!(all.calls[publication],5);
    for index in count_ends.into_iter().flat_map(|i|[i+4,i+5]).chain([publication]) {
        let mut work = Work {refuse:Some(index+1),..Work::default()};
        let result = with_capture_validation(&capture,&data,&mut work,|ctx|->ContractWorkResult<()> {
            assert_eq!(ctx.ensure_source_and_data(),Err(ContractWorkError::Resource(WorkFailure::Cancelled)));
            assert!(ctx.facts.is_none());
            Ok(()) // Swallow only to test the existing final first-cause barrier.
        });
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::Cancelled)));
        assert_eq!(work.calls.as_slice(),&all.calls[..=index]);
    }
    // Source suffix failures keep exact native errors and counts; Program mismatch
    // skips V3 and the semantic clone/encoding. Counts/native entry are source-
    // assisted via these unchanged counters/call sites, not a new native observer.
    let other = CapturedProgram::capture(&serde_json::to_vec(&rich::organizer()).unwrap(),"project",Producer::UserAuthored,None).unwrap();
    for fault in 0..5 {
        let mut bad = capture.clone();
        match fault {
            0 => bad.program.label.push('!'),
            1 => {bad.artifact.raw_digest=other.artifact.raw_digest.clone();bad.binding.source_digest=other.binding.source_digest.clone();},
            2 => bad.artifact.program_digest=other.artifact.program_digest.clone(),
            3 => bad.artifact.semantic_digest=other.artifact.semantic_digest.clone(),
            _ => {bad.source_bytes=vec![b'!';MAX_WIRE_BYTES+1];bad.artifact.program_digest=other.artifact.program_digest.clone();},
        }
        let old: old::CapturedProgram = serde_json::from_slice(&serde_json::to_vec(&bad).unwrap()).unwrap();
        old::reset_validation_counts();
        let expected = old.validate().unwrap_err().0;
        let old_counts = old::validation_counts();
        reset_validation_counts();
        let mut work = Work::default();
        let result = with_capture_validation(&bad,&data,&mut work,|ctx|->ContractWorkResult<()> {
            let error = ctx.ensure_source_and_data().unwrap_err();
            assert!(ctx.facts.is_none());
            Err(error)
        });
        assert_eq!(result,Err(ContractWorkError::Contract(ContractError(expected))));
        assert_eq!((validation_counts().captures,validation_counts().apps),(old_counts.captures,old_counts.apps));
        assert_eq!(validation_counts().apps,[1,1,2,3,0][fault]);
        assert_eq!(work.calls.windows(5).filter(|w|*w==[0,96,170496,68,1]).count(),[1,1,2,3,0][fault]);
    }
    // Complete source counts do not grant frame success. The existing source-facts
    // gate may remain published when the later independent Data validator fails.
    let mut invalid_data = data.clone();
    invalid_data.version = 2;
    let result = with_capture_validation(&capture,&invalid_data,&mut Work::default(),|ctx|->ContractWorkResult<()> {
        let error = ctx.ensure_source_and_data().unwrap_err();
        assert!(ctx.facts.is_some());
        Err(error)
    });
    assert_eq!(result,Err(ContractWorkError::Contract(ContractError("unsupported contract version".into()))));
    // A separately copied capture re-enters all existing validations, even after a
    // prior context succeeded. No successful-count/source capability is reused.
    for next in [capture.clone(),capture_minimal()] {
        reset_validation_counts();
        with_capture_validation(&next,&data,&mut Work::default(),|ctx|->ContractWorkResult<()> {ctx.ensure_source_and_data()}).unwrap();
        assert_eq!(validation_counts().apps,3);
        assert_eq!(validation_counts().captures,1);
    }
}
