//! Fresh source-proof negatives for the first P5 implementation slice.
#[path = "fixtures/common_fact/mod.rs"] mod common;
#[path = "fixtures/owned_recovery/mod.rs"] mod fixture;
#[path = "../src/product_backup.rs"] mod product_backup;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "../src/product_decisions/mod.rs"] mod product_decisions;
#[path = "../src/product_locations.rs"] mod product_locations;
#[path = "../src/product_protocol.rs"] mod product_protocol;
#[path = "../src/product_runtime/mod.rs"] mod product_runtime;
#[path = "../src/product_store/mod.rs"] mod product_store;
use fixture::*;
use product_contract::*;
use product_store::scope::*;

fn refusal(a: &CapturedProgram, b: &CapturedProgram, sources: OwnedSourceRequest) {
    let (_dir, store, a0, b0) = setup();
    adopt(&store, &a0, &b0);
    let before = store.load().unwrap();
    let layer = before.owned_scope().unwrap().layers.keys().next().unwrap();
    assert!(engine(&store).prepare_owned_evolution(&store, a, b, &sources, layer,
        "bad-continuation", ProofBudget::default(), cancel()).is_err());
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn continuation_refuses_wrong_logical_slot_and_source() {
    let (a, b) = pair();
    let (a, b) = evolved(&a, &b, "new");
    let mut wrong = sources(&a, &b);
    wrong.edits[1].id = "foreign-logical-slot".into();
    refusal(&a, &b, wrong);
    let mut wrong = sources(&a, &b);
    wrong.before = wrong.after.clone();
    refusal(&a, &b, wrong);
}

#[test]
fn continuation_refuses_changed_entering_control() {
    let (a, b) = pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        app.actions.iter_mut().find(|a| a.id == "withdraw").unwrap().guards.push(
            common::eq(common::var("note"), common::text("only this note")));
    }
    let (a, b) = (common::capture(&a), common::capture(&b));
    refusal(&a, &b, sources(&a, &b));
}

#[test]
fn continuation_refuses_common_controlled_writer() {
    let (a, b) = pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        app.actions.iter_mut().find(|a| a.id == "begin").unwrap().steps.push(
            common::update(common::var("job"), vec![(POLICY, common::boolean(true))]));
    }
    let (a, b) = (common::capture(&a), common::capture(&b));
    refusal(&a, &b, sources(&a, &b));
}

#[test]
fn continuation_refuses_changed_upstream_dependency() {
    let (a, b) = pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        let action = app.actions.iter_mut().find(|a| a.id == "arrive").unwrap();
        let Statement::Update { values, .. } = &mut action.steps[0] else { panic!() };
        values.insert("received".into(), common::boolean(false));
    }
    let (a, b) = (common::capture(&a), common::capture(&b));
    refusal(&a, &b, sources(&a, &b));
}

#[test]
fn continuation_refuses_missing_protected_destination() {
    let (a, b) = pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] { app.observables.retain(|o| o.id != COUNT); }
    let (a, b) = (common::capture(&a), common::capture(&b));
    refusal(&a, &b, sources(&a, &b));
}

#[test]
fn continuation_refuses_changed_receiver_origin() {
    let (a, b) = pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        let action = app.actions.iter_mut().find(|a| a.id == "arrive").unwrap();
        let Statement::ForEach { items, .. } = &mut action.steps[2] else { panic!() };
        let Expr::Query { predicate, binding, .. } = items else { panic!() };
        **predicate = common::and(vec![*predicate.clone(), common::eq(common::field(binding, "title"), common::text("live"))]);
    }
    let (a, b) = (common::capture(&a), common::capture(&b));
    refusal(&a, &b, sources(&a, &b));
}

#[test]
fn continuation_refuses_relocated_dependency_across_capture_boundary() {
    let (a0,b0)=pair();
    let mut request=sources(&a0,&b0);
    let (mut a,mut b)=(a0.program,b0.program);
    for app in [&mut a,&mut b] {
        let action=app.actions.iter_mut().find(|a|a.id=="arrive").unwrap();
        let writer=action.steps.remove(0);
        action.steps.push(writer);
    }
    let (a,b)=(common::capture(&a),common::capture(&b));
    request.before=canonical_digest(IdentityDomain::Source,&a).unwrap();
    request.after=canonical_digest(IdentityDomain::Source,&b).unwrap();
    for edit in &mut request.edits {
        for(source,endpoint)in [(&a,&mut edit.before),(&b,&mut edit.after)] {
            let path=if endpoint.action=="arrive" {vec![1]} else {endpoint.list_path.clone()};
            *endpoint=common::endpoint(source,&endpoint.action,path,endpoint.start,endpoint.len,
                endpoint.root.as_ref().map(|r|r.name.as_str()));
        }
    }
    refusal(&a,&b,request);
}
