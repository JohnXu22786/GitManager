//! Fresh synthetic P5 All-population fixture. No restored/private acceptance claim.
//! Uses only the existing common-fact constructors and production entry points.
use crate::common::*;
use crate::product_contract::*;
use crate::product_decisions::*;
use crate::product_runtime::LocalRuntime;
use crate::product_store::{scope::*, ProductStore, ProjectSnapshot};
use std::sync::{atomic::AtomicBool, Arc};

pub const POLICY: &str = "policy_memory_used";
pub const COUNT: &str = "policy_memory_count";
pub fn cancel() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
pub fn engine(s: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(s.clone()))
}
pub fn invoke(action: &str, arguments: Values) -> SemanticInput {
    SemanticInput::Invoke { action: action.into(), arguments }
}
pub fn value(s: &str) -> DataValue { DataValue::Text { value: s.into() } }
pub fn reference(r: &RecordRef) -> DataValue {
    DataValue::Reference { entity: r.entity.clone(), record: r.record.clone() }
}
pub fn row(s: &DataSnapshot, title: &str) -> RecordRef {
    let r = s.records.iter().find(|r| r.entity == "job" && r.values.get("title") == Some(&value(title))).unwrap();
    RecordRef { entity: r.entity.clone(), record: r.id.clone() }
}
pub fn material(s: &DataSnapshot) -> RecordRef {
    let r = s.records.iter().find(|r| r.entity == "material").unwrap();
    RecordRef { entity: r.entity.clone(), record: r.id.clone() }
}
pub fn apply(s: &ProductStore, operation: &str, input: SemanticInput) {
    let before = s.load().unwrap();
    s.apply(before.revision, operation, &input, RuntimeLimits::default()).unwrap();
    let after = s.load().unwrap();
    assert_eq!(&after.data.events[..before.data.events.len()], before.data.events.as_slice());
    assert_eq!(after.artifacts, before.artifacts);
}
pub fn add(s: &ProductStore, operation: &str, title: &str) {
    apply(s, operation, invoke("add", [("title".into(), value(title))].into()));
}
pub fn job_action(s: &ProductStore, operation: &str, action: &str, title: &str) {
    let r = row(&s.load().unwrap().data, title);
    apply(s, operation, invoke(action, [("job".into(), reference(&r))].into()));
}
pub fn material_input(s: &ProductStore, action: &str, note: &str) -> SemanticInput {
    invoke(action, [("material".into(), reference(&material(&s.load().unwrap().data))), ("note".into(), value(note))].into())
}
pub fn scenario(s: &ProductStore, id: &str, mut inputs: Vec<SemanticInput>) -> ScenarioSpec {
    let before = s.load().unwrap();
    inputs.push(SemanticInput::Observe { point: "result".into() });
    ScenarioSpec { version: 1, id: id.into(), label: format!("Fresh synthetic P5 {id}"), seed: before.data, session: before.session, clock_day: before.clock_day, random_seed: 0, inputs, validity: vec![] }
}
pub fn observed_count(scene: &AcceptedScene) -> i64 {
    assert_eq!(scene.evidence().state, EvidenceState::Observed);
    let observation = scene.observations().iter().find(|o| o.point == "result").unwrap();
    assert_eq!(observation.value_types[COUNT], Type::Integer);
    let DataValue::Integer { value } = observation.values[COUNT] else { panic!("count must be an integer") };
    value
}

pub fn pair() -> (CapturedProgram, CapturedProgram) {
    let (a, b) = crate::common::pair();
    let (mut a, mut b) = (a.program, b.program);
    // Re-arm memory for this exact active owner on every later withdrawal.
    // Update existing rows before the unchanged create-if-absent gate so a
    // newly created row is written only once, with pending=true at creation.
    let Statement::ForEach { steps, .. } = &mut b.actions.iter_mut().find(|a| a.id == "withdraw").unwrap().steps[0] else { panic!() };
    steps.insert(0, each(
        query("pause", "p", eq(field("p", "job"), var("paused_job"))),
        "existing_memory",
        vec![update(var("existing_memory"), vec![("pending", boolean(true))])],
    ));
    for app in [&mut a, &mut b] {
        app.entities.iter_mut().find(|e| e.id == "job").unwrap().fields.push(FieldDefinition { id: POLICY.into(), label: "Policy memory used".into(), value_type: Type::Boolean });
        let Statement::Create { values, .. } = &mut app.actions.iter_mut().find(|a| a.id == "add").unwrap().steps[0] else { panic!() };
        values.insert(POLICY.into(), boolean(false));
        app.actions.push(action("begin", vec![("job", Type::reference("job"))], vec![eq(field("job", "status"), text("ready"))], vec![update(var("job"), vec![("status", text("active"))])]));
        app.observables.push(ObservableDefinition { id: COUNT.into(), label: "Policy memory count".into(), value: count(query("job", "j", and(vec![not(done("j")), field("j", POLICY)]))) });
    }
    for (app, owned) in [(&mut a, false), (&mut b, true)] {
        for action in &mut app.actions {
            for step in &mut action.steps {
                if let Statement::ForEach { binding, steps, .. } = step {
                    if binding == "job" {
                        // Preserve memory while unavailable, exactly as the existing
                        // operable fixture does. The flag reads it before the clear.
                        if owned {
                            let Statement::ForEach { items, .. } = &mut steps[1] else { panic!() };
                            let Expr::Query { predicate, .. } = items else { panic!() };
                            **predicate = and(vec![*predicate.clone(), not(eq(field("job", "status"), text("waiting")))]);
                        }
                        steps.insert(1, update(var("job"), vec![(POLICY, if owned { pending("job") } else { boolean(false) })]));
                    }
                }
            }
        }
    }
    let mut arrival_a = a.actions.iter().find(|a| a.id == "withdraw").unwrap().clone();
    arrival_a.id = "arrive".into();
    arrival_a.label = "Arrive".into();
    arrival_a.guards = vec![not(field("material", "received"))];
    arrival_a.steps[0] = update(var("material"), vec![("received", boolean(true))]);
    let mut arrival_b = arrival_a.clone();
    arrival_b.steps[2] = b.actions.iter().find(|a| a.id == "withdraw").unwrap().steps[3].clone();
    a.actions.push(arrival_a);
    b.actions.push(arrival_b);
    (capture(&a), capture(&b))
}
pub fn sources(a: &CapturedProgram, b: &CapturedProgram) -> OwnedSourceRequest {
    let mut edits = vec![OwnedSpanEdit { id: "capture".into(), before: endpoint(a, "withdraw", vec![], 0, 0, None), after: endpoint(b, "withdraw", vec![], 0, 1, None) }];
    for (id, old, new) in [("register", 2, 2), ("remove", 1, 1), ("withdraw", 2, 3), ("arrive", 2, 2)] {
        edits.push(OwnedSpanEdit { id: id.into(), before: endpoint(a, id, vec![old], 0, 2, Some("job")), after: endpoint(b, id, vec![new], 0, 3, Some("job")) });
    }
    OwnedSourceRequest { before: canonical_digest(IdentityDomain::Source, a).unwrap(), after: canonical_digest(IdentityDomain::Source, b).unwrap(), root_entity: "job".into(), edits }
}
pub fn request(s: &ProductStore, a: &CapturedProgram, b: &CapturedProgram) -> OwnedPreviewRequest {
    let mut r = crate::common::request(s, a, b);
    r.inventory.sources = sources(a, b);
    r.purpose = "Fresh synthetic PropertiesOnly P5 All journey".into();
    r
}
pub fn setup() -> (tempfile::TempDir, ProductStore, CapturedProgram, CapturedProgram) {
    let (a, b) = pair();
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let s = ProductStore::create(dir.path().join("store"), &a, 20000).unwrap();
    for title in ["live", "reserve", "complete-original", "finished"] { add(&s, &format!("add-{title}"), title); }
    job_action(&s, "finish-before-policy", "complete", "finished");
    for title in ["live", "reserve"] {
        let r = row(&s.load().unwrap().data, title);
        apply(&s, &format!("register-{title}"), invoke("register", [("owner".into(), reference(&r)), ("name".into(), value("shared-material"))].into()));
    }
    apply(&s, "material-arrived", material_input(&s, "arrive", "before policy"));
    let before = s.load().unwrap();
    let unfinished: Vec<_> = before.data.records.iter().filter(|r| r.entity == "job" && r.values["status"] != value("done")).collect();
    assert_eq!(unfinished.len(), 3);
    assert!(unfinished.iter().all(|r| r.values[POLICY] == DataValue::Boolean { value: false }));
    assert!(!before.data.records.iter().any(|r| r.entity == "pause"));
    // An independent ordinary obligation has its own real source/frame/witness.
    let scene = engine(&s).accept_current_scene(&before, &scenario(&s, "independent-add", vec![invoke("add", [("title".into(), value("independent-scene"))].into())]), Disclosure::Synthetic).unwrap();
    let choice = Choice {
        id: "independent-add".into(), request: "Adding a job in this original frame leaves four ready jobs".into(), rationale: None,
        scope: DecisionScope { operations: ["add".into()].into(), population: Population::All, conditions: Values::new(), excluded_records: vec![], unknowns: vec![] },
        outcome: DecisionOutcome::KeepCurrent, binding: IntentionBinding::PropertiesOnly,
        obligations: vec![AcceptedProperty { id: "four-ready".into(), description: "Independent common ready count".into(), predicate: PropertyPredicate::Equal { left: PropertyTerm::Observed { point: "result".into(), observable: "ready_count".into(), value_type: Type::Integer }, right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: 4 } } } }],
    };
    let change = engine(&s).prepare_choice(&s, &a, choice, vec![scene], "record-independent").unwrap();
    engine(&s).adopt(&s, &change).unwrap();
    (dir, s, a, b)
}
pub fn prepare(s: &ProductStore, a: &CapturedProgram, b: &CapturedProgram) -> PreparedOwnedScopedChangeV2 {
    let before = s.load().unwrap();
    let r = request(s, a, b);
    let witnesses: Vec<_> = [OwnedSourceSide::Before, OwnedSourceSide::After].into_iter().map(|side| LifecycleWitness { side, scenario: scenario(s, "lifecycle", vec![]), completed: row(&before.data, "finished"), unfinished: row(&before.data, "live") }).collect();
    let accepted = LifecycleAcceptanceHost::prepare(s, b, &r, &witnesses, ProofBudget::default(), cancel()).unwrap().synthetic_operator_accept_for_test(s).unwrap();
    let (p, report) = engine(s).prepare_owned_change(s, b, &r, &accepted, "initial-owned", ProofBudget::default(), cancel()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(s.load().unwrap(), before);
    p
}
pub fn policy_choice(p: &PreparedOwnedScopedChangeV2) -> Choice {
    Choice { id: "owned-policy".into(), request: "In the accepted action scenes fewer than two unfinished jobs use remembered policy state".into(), rationale: None, scope: p.scope(), outcome: DecisionOutcome::Accept { artifact: p.target().artifact.program_digest.clone() }, binding: IntentionBinding::PropertiesOnly, obligations: vec![AcceptedProperty { id: "policy-count".into(), description: "A meaningful policy-only bound with at least two eligible roots".into(), predicate: PropertyPredicate::Less { left: PropertyTerm::Observed { point: "result".into(), observable: COUNT.into(), value_type: Type::Integer }, right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: 2 } } } }] }
}
pub fn action_scenes(s: &ProductStore, p: &PreparedOwnedScopedChangeV2) -> Vec<AcceptedScene> {
    let before = s.load().unwrap();
    let live = row(&before.data, "live");
    let complete = row(&before.data, "complete-original");
    let link = before.data.records.iter().find(|r| r.entity == "requirement").unwrap();
    let material = reference(&material(&before.data));
    let withdrawal = invoke("withdraw", [("material".into(), material.clone()), ("note".into(), value("scene"))].into());
    let scenes = vec![
        ("add", vec![invoke("add", [("title".into(), value("scene-new"))].into())]),
        ("complete", vec![invoke("complete", [("job".into(), reference(&complete))].into())]),
        ("register", vec![invoke("register", [("owner".into(), reference(&live)), ("name".into(), value("scene-material"))].into())]),
        ("remove", vec![invoke("remove", [("link".into(), DataValue::Reference { entity: "requirement".into(), record: link.id.clone() })].into())]),
        ("withdraw", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal.clone()]),
        ("begin", vec![invoke("begin", [("job".into(), reference(&live))].into())]),
        ("arrive", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal, invoke("arrive", [("material".into(), material), ("note".into(), value("scene"))].into())]),
    ];
    scenes.into_iter().map(|(id, inputs)| {
        let scene = engine(s).accept_owned_scene(s, p, &scenario(s, id, inputs), Disclosure::Synthetic).unwrap();
        assert!(observed_count(&scene) < 2);
        scene
    }).collect()
}
pub fn assert_nonvacuity(s: &ProductStore, p: &PreparedOwnedScopedChangeV2) {
    let before = s.load().unwrap();
    let mut inputs: Vec<_> = ["live", "reserve"].into_iter().map(|title| invoke("begin", [("job".into(), reference(&row(&before.data, title)))].into())).collect();
    inputs.push(material_input(s, "withdraw", "two actual policy contributions"));
    let spec = scenario(s, "policy-counterexample", inputs);
    let baseline = engine(s).accept_owned_current_scene(s, p, &spec, Disclosure::Synthetic).unwrap();
    let candidate = engine(s).accept_owned_scene(s, p, &spec, Disclosure::Synthetic).unwrap();
    assert_eq!(observed_count(&baseline), 0);
    assert_eq!(observed_count(&candidate), 2); // Fails the chosen predicate; never a chosen scene.
    assert_eq!(s.load().unwrap(), before);
}
pub fn adopt(s: &ProductStore, a: &CapturedProgram, b: &CapturedProgram) -> ProjectSnapshot {
    let before = s.load().unwrap();
    let p = prepare(s, a, b);
    assert_nonvacuity(s, &p);
    let choice = policy_choice(&p);
    let change = engine(s).prepare_owned_choice(s, p.clone(), choice, action_scenes(s, &p), p.operation_id()).unwrap();
    assert_eq!(s.load().unwrap(), before);
    let after = engine(s).adopt(s, &change).unwrap();
    assert_eq!(after.data.events, before.data.events);
    assert_eq!(after.data.generation, before.data.generation);
    assert_eq!(after.session, before.session);
    assert_eq!(after.clock_day, before.clock_day);
    assert_eq!(after.artifacts, before.artifacts);
    for old in &before.data.records {
        let now = after.data.records.iter().find(|r| r.id == old.id).unwrap();
        assert_eq!(now.revision, old.revision);
        for (key, value) in &old.values { assert_eq!(now.values.get(key), Some(value)); }
    }
    let state = after.owned_scope().unwrap();
    assert_eq!((state.layers.len(), state.initializations.len(), state.compositions.len(), state.adoptions.len()), (1, 1, 1, 1));
    assert_eq!(state.adoptions[0].decisions, vec!["owned-policy"]);
    assert_eq!(engine(s).check_current(&after).unwrap().disposition, CheckDisposition::Ready);
    after
}
// Actual changed executables: the variant no longer marks the reserve root;
// the pair also adds the same disjoint state write after ordinary creation.
// The future continuation verifier must prove this, rather than trust the comment.
pub fn evolved(a: &CapturedProgram, b: &CapturedProgram, marker: &str) -> (CapturedProgram, CapturedProgram) {
    let (mut a, mut b) = (a.program.clone(), b.program.clone());
    for app in [&mut a, &mut b] {
        let add = app.actions.iter_mut().find(|a| a.id == "add").unwrap();
        add.steps.truncate(1);
        add.steps.push(Statement::SetState { state: "name_input".into(), value: text(marker) });
    }
    for action in &mut b.actions {
        for statement in &mut action.steps {
            if let Statement::ForEach { binding, steps, .. } = statement {
                if binding == "job" {
                    let Statement::Update { values, .. } = &mut steps[1] else { panic!() };
                    values.insert(POLICY.into(), and(vec![pending("job"), not(eq(field("job", "title"), text("reserve")))]));
                }
            }
        }
    }
    (capture(&a), capture(&b))
}
pub fn flag(s: &ProjectSnapshot, title: &str) -> bool {
    let id = row(&s.data, title).record;
    s.data.records.iter().find(|r| r.id == id).unwrap().values[POLICY] == DataValue::Boolean { value: true }
}
pub fn member_key(s: &ProjectSnapshot) -> String {
    let state = s.owned_scope().unwrap();
    let init = state.initializations.first().unwrap();
    let keys: Vec<_> = init.additions[0].values.keys().filter(|key| key.ends_with("_member")).collect();
    assert_eq!(keys.len(), 1);
    keys[0].clone()
}
pub fn assert_member(s: &ProjectSnapshot, title: &str, key: &str, expected: bool) {
    let id = row(&s.data, title).record;
    assert_eq!(s.data.records.iter().find(|r| r.id == id).unwrap().values[key], DataValue::Boolean { value: expected });
}
