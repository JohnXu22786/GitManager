//! Synthetic local host-module tests; no live provider or reachable UI claims.
#[path = "../src/product_studio/change_adapter.rs"]
mod change_adapter;
#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_studio/intention_flow.rs"]
mod intention_flow;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use intention_flow::*;
use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::{scope::*, ProductStore, ProjectSnapshot};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn cancellation() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
fn scenario(snapshot: &ProjectSnapshot, id: &str) -> ScenarioSpec {
    ScenarioSpec {
        version: 1,
        id: id.into(),
        label: "Saved work example".into(),
        seed: snapshot.data.clone(),
        session: snapshot.session.clone(),
        clock_day: snapshot.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    }
}
fn scope() -> DecisionScope {
    DecisionScope {
        operations: ["export".into()].into(),
        population: Population::All,
        conditions: Values::new(),
        excluded_records: vec![],
        unknowns: vec![],
    }
}
fn choice(id: &str, outcome: DecisionOutcome) -> Choice {
    Choice {
        id: id.into(),
        request: "Keep this way of working".into(),
        rationale: None,
        scope: scope(),
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    }
}
fn record_need(store: &ProductStore, id: &str, outcome: DecisionOutcome) {
    let e = engine(store);
    let s = store.load().unwrap();
    let scene = e
        .accept_current_scene(&s, &scenario(&s, id), Disclosure::Synthetic)
        .unwrap();
    let ready = e
        .prepare_choice(
            store,
            s.program().unwrap(),
            choice(id, outcome),
            vec![scene],
            &format!("record-{id}"),
        )
        .unwrap();
    e.adopt(store, &ready).unwrap();
}
fn extended(current: &CapturedProgram) -> CapturedProgram {
    let mut value = serde_json::to_value(&current.program).unwrap();
    value["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"note","label":"Work note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    value["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"set_note","label":"Edit work note","parameters":{"row":{"kind":"reference","entity":"job"},"note":{"kind":"optional","item":{"kind":"text"}}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"note":var("note")}}],"ensures":[]}));
    value["entities"].as_array_mut().unwrap().push(serde_json::json!({"id":"material","label":"Materials","fields":[{"id":"name","label":"Material","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    value["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"log_material","label":"Log material","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"material","bind":"material","values":{"name":var("name")}}],"ensures":[]}));
    capture(value)
}
fn response(
    request: &DevelopmentRequest,
    candidate: &CapturedProgram,
    needs: &[&str],
    retire: &[&str],
) -> DevelopmentResult {
    DevelopmentResult {
        producer: candidate.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "design".into(),
                source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            unsupported: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "new-design".into(),
                candidate: "design".into(),
                needs: needs.iter().map(|v| v.to_string()).collect(),
                proposed_retirement: retire.iter().map(|v| v.to_string()).collect(),
                preserved_obligations: request
                    .decisions
                    .decisions
                    .iter()
                    .filter(|d| d.status == DecisionStatus::Active)
                    .flat_map(|d| &d.obligations)
                    .map(|p| p.identity().unwrap())
                    .collect(),
                mappings: vec![],
                scenarios: vec![],
            }],
        },
    }
}
struct OnceProvider {
    result: DevelopmentResult,
    calls: std::cell::Cell<usize>,
}
impl DevelopmentProvider for OnceProvider {
    fn develop(
        &self,
        request: &DevelopmentRequest,
        _: &dyn Fn() -> bool,
    ) -> Result<DevelopmentResult, AdapterError> {
        assert_eq!(request.operation, DevelopmentOperation::Reconcile);
        assert!(request
            .request
            .contains("evolution suggestion ID: new-design"));
        assert_eq!(
            self.calls.replace(self.calls.get() + 1),
            0,
            "must not double-call provider"
        );
        self.result.response.validate_for(request)?;
        Ok(self.result.clone())
    }
}

#[test]
fn reconciliation_refuses_stale_results_then_resolves_from_fresh_daily_work() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    for id in ["first", "second"] {
        record_need(&store, id, DecisionOutcome::BothNeeded);
    }
    let e = engine(&store);
    let old = store.load().unwrap();
    let pending = Reconciliation::new(
        &store,
        &old,
        &e,
        "old-request",
        "Keep both",
        &["first".into(), "second".into()],
        "new-design",
        "adopt-design",
        cancellation(),
    )
    .unwrap();
    let candidate = extended(old.program().unwrap());
    let provider = OnceProvider {
        result: response(
            pending.request(),
            &candidate,
            &["first", "second"],
            &["first", "second"],
        ),
        calls: std::cell::Cell::new(0),
    };
    let late = add(&store, "late-work", "Legitimate later work");
    assert!(pending
        .develop(&store, &e, &provider, cancellation())
        .is_err());
    assert_eq!(provider.calls.get(), 0);
    let current = store.load().unwrap();
    let fresh = Reconciliation::new(
        &store,
        &current,
        &e,
        "fresh-request",
        "Keep both",
        &["first".into(), "second".into()],
        "new-design",
        "adopt-design",
        cancellation(),
    )
    .unwrap();
    let provider = OnceProvider {
        result: response(
            fresh.request(),
            &candidate,
            &["first", "second"],
            &["first", "second"],
        ),
        calls: std::cell::Cell::new(0),
    };
    let design = fresh
        .develop(&store, &e, &provider, cancellation())
        .unwrap();
    let saved = e
        .adopt(
            &store,
            &design.decision(&store, &e, cancellation()).unwrap(),
        )
        .unwrap();
    assert_eq!(row(&saved, &late), row(&current, &late));
    assert_eq!(saved.data.events, current.data.events);
    assert_eq!(provider.calls.get(), 1);
}

#[test]
fn managed_design_reuses_slots_and_refuses_moved_destinations_stale_basis_and_cancel() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let s = store.load().unwrap();
    let layer = store
        .prepare_scoped_change(
            &program(true),
            &request(&s, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let s = store.adopt_scoped(s.revision, &layer).unwrap();
    let candidate = extended(&change_adapter::baseline(&s).unwrap());
    let prepared =
        prepare_managed_design(&store, &s, &candidate, "new-design", cancellation()).unwrap();
    assert_eq!(prepared.candidate(), &candidate);
    assert_eq!(prepared.operation_id(), "new-design");
    ScopedExecutionContext::prepared(&s, &prepared).unwrap();
    let mut moved = serde_json::to_value(&candidate.program).unwrap();
    moved["actions"][3]["steps"].as_array_mut().unwrap().insert(0, serde_json::json!({"kind":"update","record":var("row"),"values":{"waiting":boolean(false)}}));
    assert!(prepare_managed_design(&store, &s, &capture(moved), "moved", cancellation()).is_err());
    let cancel = cancellation();
    cancel.store(true, Ordering::Release);
    assert!(prepare_managed_design(&store, &s, &candidate, "cancelled", cancel).is_err());
    add(&store, "late", "Later work");
    assert!(prepare_managed_design(&store, &s, &candidate, "stale", cancellation()).is_err());
}

fn scoped_rule(store: &ProductStore) -> Digest {
    let existing = add(store, "existing", "Existing work");
    action(store, "wait-existing", "wait", &existing);
    tick(store, "advance", 20003);
    let s = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&s, ScopePopulation::All),
            "bad-rule",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let mut example = scenario(&s, "bad-rule-example");
    example.seed = prepared.seed().clone();
    example.session = SessionState::initial(&prepared.target().program).unwrap();
    example.inputs.splice(
        0..0,
        [
            invoke("calculate", &[("row", reference(&existing))]),
            invoke("complete", &[("row", reference(&existing))]),
        ],
    );
    let e = engine(store);
    let accepted = e
        .accept_scoped_scene(store, &prepared, &example, Disclosure::Synthetic)
        .unwrap();
    let mut c = choice(
        "bad-rule-intent",
        DecisionOutcome::Accept {
            artifact: prepared.target().artifact.program_digest.clone(),
        },
    );
    c.scope = prepared.scope().clone();
    let ready = e
        .prepare_scoped_choice(store, prepared, c, vec![accepted], "bad-rule")
        .unwrap();
    e.adopt(store, &ready).unwrap();
    layer
}

#[test]
fn withdrawal_previews_checked_current_data_and_preserves_later_work_after_restart() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let layer = scoped_rule(&store);
    let s = store.load().unwrap();
    let candidate = extended(&change_adapter::baseline(&s).unwrap());
    let prepared =
        prepare_managed_design(&store, &s, &candidate, "add-fields", cancellation()).unwrap();
    let e = engine(&store);
    let ready = e
        .prepare_managed_change(&store, prepared, &[], "add-fields")
        .unwrap();
    e.adopt(&store, &ready).unwrap();
    let later = add(&store, "later", "Later legitimate work");
    action(&store, "edit-later", "wait", &later);
    apply(
        &store,
        "note-later",
        invoke(
            "set_note",
            &[
                ("row", reference(&later)),
                ("note", text("Keep this later note")),
            ],
        ),
    );
    tick(&store, "later-days", 20006);
    action(&store, "complete-later", "complete", &later);
    apply(
        &store,
        "material",
        invoke("log_material", &[("name", text("Oak"))]),
    );
    let facts = apply(&store, "output", invoke("export", &[]));
    let mut withdrawal = Withdrawal::prepare(
        &store,
        &facts,
        &e,
        &[layer.clone()],
        "withdraw",
        cancellation(),
    )
    .unwrap();
    assert_eq!(withdrawal.view().retire, vec!["bad-rule-intent"]);
    assert_eq!(withdrawal.view().layers, vec![layer.clone()]);
    let trial = withdrawal
        .trial(
            &store,
            invoke("log_material", &[("name", text("Copied only"))]),
            cancellation(),
        )
        .unwrap();
    assert!(trial
        .retained_records
        .iter()
        .any(|r| r.entity == "material" && r.values["name"] == text("Copied only")));
    assert_eq!(store.load().unwrap(), facts);
    let ready = withdrawal.decision(&store, &e, cancellation()).unwrap();
    assert_eq!(ready.plan().id, "withdraw");
    assert_eq!(ready.candidate(), &withdrawal.view().candidate);
    let saved = e.adopt(&store, &ready).unwrap();
    assert_eq!(saved.data, facts.data);
    assert_eq!(saved.artifacts, facts.artifacts);
    let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
    assert_eq!(reopened.load().unwrap(), saved);
    let view = HistoryView::load(&reopened, &saved, cancellation()).unwrap();
    let rule = view
        .decisions
        .iter()
        .find(|d| d.decision.id == "bad-rule-intent")
        .unwrap();
    assert_eq!(
        rule.decision.status,
        DecisionStatus::Withdrawn {
            adoption: "withdraw".into()
        }
    );
    assert!(rule.receipts.iter().any(|r| r.plan.id == "withdraw"));
    assert!(!view.layers.iter().find(|l| l.id == layer).unwrap().active);
    assert!(!rule.scenes.is_empty());
    let continued = add(&reopened, "continued", "After restart");
    action(&reopened, "continued-wait", "wait", &continued);
    tick(&reopened, "continued-days", 20008);
    let result = action(&reopened, "continued-calc", "calculate", &continued);
    assert_eq!(
        row(&result, &continued).values["production"],
        DataValue::Integer { value: 2 }
    );
    assert_eq!(row(&result, &later), row(&saved, &later));
}

#[test]
fn withdrawal_refuses_stale_or_cancelled_preview_and_independent_conflict() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let layer = scoped_rule(&store);
    record_need(&store, "independent", DecisionOutcome::KeepCurrent);
    let s = store.load().unwrap();
    let e = engine(&store);
    // The independent export promise still needs the changed production rule.
    assert!(
        Withdrawal::prepare(&store, &s, &e, &[layer.clone()], "withdraw", cancellation()).is_err()
    );
    let later = add(&store, "still-usable", "Continue after refusal");
    let now = store.load().unwrap();
    let forward = extended(&change_adapter::baseline(&now).unwrap());
    let prepared =
        prepare_managed_design(&store, &now, &forward, "forward", cancellation()).unwrap();
    let ready = e
        .prepare_managed_change(&store, prepared, &[], "forward")
        .unwrap();
    let repaired = e.adopt(&store, &ready).unwrap();
    assert_eq!(row(&repaired, &later), row(&now, &later));
    assert!(repaired
        .program()
        .unwrap()
        .program
        .actions
        .iter()
        .any(|a| a.id == "log_material"));
    assert!(Withdrawal::prepare(&store, &s, &e, &[layer], "stale", cancellation()).is_err());
}

#[test]
fn archive_history_rejects_changed_decisions_foreign_scenes_and_corrupt_objects() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    record_need(&store, "active", DecisionOutcome::KeepCurrent);
    record_need(&store, "pending", DecisionOutcome::BothNeeded);
    let s = store.load().unwrap();
    let archive = IntentArchive::new(store.clone());
    let view = HistoryView::load(&store, &s, cancellation()).unwrap();
    assert_eq!(view.decisions.len(), 2);
    assert_eq!(view.decisions[1].decision.status, DecisionStatus::Pending);
    let mut changed = s.decisions.decisions[0].clone();
    changed.request.push_str(" forged");
    assert!(archive.accepted_scenes(&changed).is_err());
    changed = s.decisions.decisions[0].clone();
    changed.scenarios.reverse();
    changed.scenarios.push(changed.scenarios[0].clone());
    assert!(archive.accepted_scenes(&changed).is_err());
    changed = s.decisions.decisions[0].clone();
    changed.witness = s.decisions.decisions[1].witness.clone();
    assert!(archive.accepted_scenes(&changed).is_err());
    let witness = &s.decisions.decisions[0].witness;
    let path = dir
        .path()
        .join("tool")
        .join(format!("extension-{}.json", witness.as_str()));
    std::fs::write(path, b"corrupt").unwrap();
    assert!(archive.accepted_scenes(&s.decisions.decisions[0]).is_err());
    assert!(HistoryView::load(&store, &s, cancellation()).is_err());
}

#[test]
fn reconciliation_rejects_cosmetic_design_and_unrequested_retirement() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    for id in ["first", "second"] {
        record_need(&store, id, DecisionOutcome::BothNeeded);
    }
    record_need(&store, "independent", DecisionOutcome::KeepCurrent);
    let e = engine(&store);
    let s = store.load().unwrap();
    for (candidate, retire) in [
        (s.program().unwrap().clone(), vec!["first", "second"]),
        (
            extended(s.program().unwrap()),
            vec!["first", "second", "independent"],
        ),
    ] {
        let pending = Reconciliation::new(
            &store,
            &s,
            &e,
            "request",
            "Keep both",
            &["first".into(), "second".into()],
            "new-design",
            "adopt-design",
            cancellation(),
        )
        .unwrap();
        let provider = OnceProvider {
            result: response(pending.request(), &candidate, &["first", "second"], &retire),
            calls: std::cell::Cell::new(0),
        };
        assert!(pending
            .develop(&store, &e, &provider, cancellation())
            .is_err());
        assert_eq!(provider.calls.get(), 1);
        assert_eq!(store.load().unwrap(), s);
    }
}

#[test]
fn managed_reconciliation_binds_actual_result_target_and_final_operation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let s = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&s, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    store.adopt_scoped(s.revision, &prepared).unwrap();
    for id in ["first", "second"] {
        record_need(&store, id, DecisionOutcome::BothNeeded);
    }
    let s = store.load().unwrap();
    let e = engine(&store);
    let request = Reconciliation::new(
        &store,
        &s,
        &e,
        "request",
        "Keep both",
        &["first".into(), "second".into()],
        "new-design",
        "exact-operation",
        cancellation(),
    )
    .unwrap();
    let candidate = extended(&change_adapter::baseline(&s).unwrap());
    assert!(request
        .request()
        .request
        .contains("evolution suggestion ID: new-design"));
    let actual = response(
        request.request(),
        &candidate,
        &["first", "second"],
        &["first", "second"],
    );
    let design = request
        .develop_prepared(&store, &e, actual, cancellation())
        .unwrap();
    assert_eq!(design.view().authored, candidate);
    assert_ne!(design.view().candidate, candidate);
    assert_eq!(store.load().unwrap(), s);
    let ready = design.decision(&store, &e, cancellation()).unwrap();
    assert_eq!(ready.plan().id, "exact-operation");
    let saved = e.adopt(&store, &ready).unwrap();
    assert_eq!(saved.scope.layers, s.scope.layers);
    assert_eq!(
        ProductStore::open(dir.path().join("tool"))
            .unwrap()
            .load()
            .unwrap(),
        saved
    );
}

#[test]
fn checked_withdrawal_cannot_commit_after_daily_work_or_cancellation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let s = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&s, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let s = store.adopt_scoped(s.revision, &prepared).unwrap();
    let e = engine(&store);
    let withdrawal =
        Withdrawal::prepare(&store, &s, &e, &[layer.clone()], "withdraw", cancellation()).unwrap();
    let cancel = cancellation();
    cancel.store(true, Ordering::Release);
    assert!(withdrawal.decision(&store, &e, cancel).is_err());
    let later = add(&store, "later", "Work after preview");
    assert!(withdrawal.decision(&store, &e, cancellation()).is_err());
    let facts = store.load().unwrap();
    let fresh = Withdrawal::prepare(
        &store,
        &facts,
        &e,
        &[layer],
        "fresh-withdraw",
        cancellation(),
    )
    .unwrap();
    let saved = e
        .adopt(&store, &fresh.decision(&store, &e, cancellation()).unwrap())
        .unwrap();
    assert_eq!(row(&saved, &later), row(&facts, &later));
}

#[path = "../src/ui/product_runtime_view.rs"]
pub(crate) mod product_runtime_view;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "support/egui_harness.rs"]
mod egui_harness;

#[test]
fn history_controls_select_exact_ids_despite_equal_business_labels() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    record_need(&store, "first", DecisionOutcome::BothNeeded);
    record_need(&store, "second", DecisionOutcome::BothNeeded);
    let view = HistoryView::load(&store, &store.load().unwrap(), cancellation()).unwrap();
    let mut state = HistoryState::default();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1000.0, 1400.0));
    let frame = |h: &mut egui_harness::EguiHarness, state: &mut HistoryState| {
        h.frame(|ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| state.show(ui, &view, true))
                .inner
        })
    };
    for key in ["intention-need-first", "intention-need-second"] {
        let (_, trace) = frame(&mut h, &mut state);
        let point = trace.controls[key].rect.center();
        h.press_at(point);
        frame(&mut h, &mut state);
        h.release_at(point);
        frame(&mut h, &mut state);
    }
    let (_, trace) = frame(&mut h, &mut state);
    let point = trace.controls["intention-reconcile"].rect.center();
    assert!(trace.controls["intention-reconcile"].enabled);
    h.press_at(point);
    frame(&mut h, &mut state);
    h.release_at(point);
    let (event, _) = frame(&mut h, &mut state);
    let Event::Reconcile { needs, .. } = event.unwrap() else {
        panic!("wrong event");
    };
    assert_eq!(needs, vec!["first", "second"]);
    assert_eq!(store.load().unwrap().decisions.decisions.len(), 2);
}

#[path = "fixtures/product_runtime/mod.rs"]
mod evolution_fixture;
mod distinct_needs {
    use super::evolution_fixture::*;
    use super::{
        cancellation, intention_flow::*, product_contract::*, product_decisions::*,
        product_runtime::LocalRuntime, product_store::ProductStore,
    };
    use serde_json::json;
    fn scope() -> DecisionScope {
        DecisionScope {
            operations: ["export_people".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        }
    }
    fn accepted(p: &CapturedProgram, id: &str) -> AcceptedScene {
        accepted_for(p, id, "export_people")
    }
    fn accepted_for(p: &CapturedProgram, id: &str, action: &str) -> AcceptedScene {
        let mut s = scenario(
            p,
            vec![
                add("Ada"),
                invoke("collect", Values::new()),
                invoke(action, Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        );
        s.id = id.into();
        accept_scene(
            &LocalRuntime::default(),
            p,
            &s,
            Disclosure::Synthetic,
            RuntimeLimits::default(),
        )
        .unwrap()
    }
    fn choice(id: &str, outcome: DecisionOutcome) -> Choice {
        Choice {
            id: id.into(),
            request: "Preserve this accepted work example".into(),
            rationale: None,
            scope: scope(),
            outcome,
            obligations: vec![],
            binding: IntentionBinding::ObservedOutcome,
        }
    }
    fn invariant() -> AcceptedProperty {
        AcceptedProperty {
            id: "count-match".into(),
            description: "Preview count equals actual output rows".into(),
            predicate: PropertyPredicate::Equal {
                left: PropertyTerm::Observed {
                    point: "done".into(),
                    observable: "selected_count".into(),
                    value_type: Type::Integer,
                },
                right: PropertyTerm::OutputCount {
                    point: "done".into(),
                    output: "roster".into(),
                },
            },
        }
    }
    struct FixtureProvider {
        response: DevelopmentResponse,
        calls: std::cell::Cell<usize>,
    }
    impl DevelopmentProvider for FixtureProvider {
        fn develop(
            &self,
            request: &DevelopmentRequest,
            _: &dyn Fn() -> bool,
        ) -> Result<DevelopmentResult, AdapterError> {
            assert_eq!(self.calls.replace(self.calls.get() + 1), 0);
            assert_eq!(request.operation, DevelopmentOperation::Reconcile);
            assert!(request
                .request
                .contains("evolution suggestion ID: split-design"));
            assert!(request.sources.iter().any(|p| p
                .program
                .actions
                .iter()
                .any(|a| a.id == "export_people")));
            assert!(request.accepted_scenes.len() >= 2);
            let mut response = self.response.clone();
            response.request_digest = request.identity()?;
            Ok(DevelopmentResult {
                response,
                producer: Producer::Fixture {
                    name: "Explicit synthetic evolution response".into(),
                },
            })
        }
    }
    #[test]
    fn new_design_reconciles_distinct_workflows_and_preserves_independent_history() {
        let dir = super::fixture::tempdir();
        let p = capture(organizer());
        let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let a = accepted(&p, "gathered");
        let prepared = engine
            .prepare_choice(
                &store,
                &p,
                choice("old-default", DecisionOutcome::KeepCurrent),
                vec![a.clone()],
                "save-default",
            )
            .unwrap();
        engine.adopt(&store, &prepared).unwrap();
        let concrete = engine
            .prepare_choice(
                &store,
                &p,
                choice("independent-concrete", DecisionOutcome::KeepCurrent),
                vec![a.clone()],
                "save-concrete",
            )
            .unwrap();
        engine.adopt(&store, &concrete).unwrap();
        let mut invariant_choice = choice("independent", DecisionOutcome::KeepCurrent);
        invariant_choice.obligations = vec![invariant()];
        invariant_choice.binding = IntentionBinding::PropertiesOnly;
        let prepared = engine
            .prepare_choice(
                &store,
                &p,
                invariant_choice,
                vec![a.clone()],
                "save-invariant",
            )
            .unwrap();
        engine.adopt(&store, &prepared).unwrap();
        let mut empty_program = organizer();
        empty_program["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
        empty_program["actions"][2]["id"] = json!("one_off_original");
        empty_program["views"][0]["actions"][0]["action"] = json!("one_off_original");
        let b = accepted_for(&capture(empty_program), "one-off", "one_off_original");
        let prepared = engine
            .prepare_choice(
                &store,
                &p,
                {
                    let mut c = choice("second-need", DecisionOutcome::BothNeeded);
                    c.scope.operations = ["one_off_original".into()].into();
                    c
                },
                vec![b.clone()],
                "save-second",
            )
            .unwrap();
        let before = engine.adopt(&store, &prepared).unwrap();
        assert_ne!(a.observations(), b.observations());
        assert_eq!(a.observations()[0].outputs[0].rows.len(), 1);
        assert_eq!(b.observations()[0].outputs[0].rows.len(), 0);
        let pending = Reconciliation::new(
            &store,
            &before,
            &engine,
            "synthesize",
            "Support both accepted ways of working",
            &["old-default".into(), "second-need".into()],
            "split-design",
            "adopt-design",
            cancellation(),
        )
        .unwrap();
        let request = pending.request();
        // This clearly fixture-origin provider response adds a new durable audit entity
        // and transaction, rather than renaming a demonstration's third button.
        let mut design = organizer();
        design["entities"].as_array_mut().unwrap().push(json!({"id":"dispatch","label":"Dispatch log","fields":[{"id":"purpose","label":"Purpose","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
        let mut special = design["actions"][2].clone();
        special["id"] = json!("dispatch_one_off");
        special["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
        special["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"create","entity":"dispatch","values":{"purpose":text("One-off work completed")},"bind":"logged"}));
        design["actions"].as_array_mut().unwrap().push(special);
        design["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"one_off_button","label":"One-off dispatch","placement":"toolbar","action":"dispatch_one_off","arguments":{},"enabled":yes()}));
        let mut mapped = b.scenario().clone();
        mapped.inputs[2] = invoke("dispatch_one_off", Values::new());
        let response = DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new-design".into(),
                source_json: serde_json::to_string(&design).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "split-design".into(),
                candidate: "new-design".into(),
                needs: vec!["old-default".into(), "second-need".into()],
                proposed_retirement: vec!["old-default".into()],
                preserved_obligations: vec![invariant().identity().unwrap()],
                mappings: vec![SemanticMapping {
                    from: SemanticKey {
                        kind: SemanticKind::Action,
                        entity: None,
                        id: "one_off_original".into(),
                    },
                    to: SemanticKey {
                        kind: SemanticKind::Action,
                        entity: None,
                        id: "dispatch_one_off".into(),
                    },
                }],
                scenarios: vec![SuggestedScenarioMapping {
                    source_program: None,
                    original: b.scenario().identity().unwrap(),
                    replacement_json: serde_json::to_string(&mapped).unwrap(),
                    explanation: "Use the new explicit one-off transaction".into(),
                }],
            }],
            unsupported: vec![],
        };
        let provider = FixtureProvider {
            response: response.clone(),
            calls: std::cell::Cell::new(0),
        };
        let mut design = pending
            .develop(&store, &engine, &provider, cancellation())
            .unwrap();
        assert_eq!(provider.calls.get(), 1);
        assert_eq!(design.view().retire, vec!["old-default"]);
        assert!(design
            .view()
            .preserve
            .contains(&"independent-concrete".into()));
        assert!(design.view().preserve.contains(&"independent".into()));
        let preview = design
            .trial(
                &store,
                invoke("dispatch_one_off", Values::new()),
                cancellation(),
            )
            .unwrap();
        assert!(preview
            .retained_records
            .iter()
            .any(|r| r.entity == "dispatch"));
        assert_eq!(store.load().unwrap(), before); // Keep, reject, and defer are all non-committing.
        assert!(matches!(
            design.view().candidate.binding.producer,
            Producer::Fixture { .. }
        ));
        let ready = design.decision(&store, &engine, cancellation()).unwrap();
        assert_eq!(store.load().unwrap(), before);
        assert!(ready.report().runs.len() >= 4);
        let adopted = engine.adopt(&store, &ready).unwrap();
        assert!(
            matches!(&adopted.decisions.decisions.iter().find(|d|d.id=="old-default").unwrap().status,DecisionStatus::Superseded{by} if by=="split-design")
        );
        assert_eq!(
            adopted
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == "independent")
                .unwrap()
                .status,
            DecisionStatus::Active
        );
        assert_eq!(
            adopted
                .decisions
                .decisions
                .iter()
                .find(|d| d.id == "old-default")
                .unwrap()
                .witness,
            before.decisions.decisions[0].witness
        );
        assert_eq!(
            ProductStore::open(dir.path().join("tool"))
                .unwrap()
                .load()
                .unwrap(),
            adopted
        );
        let fresh = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        assert_eq!(
            fresh
                .check_all(&adopted.decisions, adopted.program().unwrap(), &[])
                .unwrap()
                .disposition,
            CheckDisposition::Ready
        );
        let history = HistoryView::load(&store, &adopted, cancellation()).unwrap();
        assert!(history
            .decisions
            .iter()
            .find(|d| d.decision.id == "old-default")
            .unwrap()
            .status_text
            .contains("Replaced"));
        assert_eq!(
            history
                .decisions
                .iter()
                .find(|d| d.decision.id == "second-need")
                .unwrap()
                .decision
                .status,
            DecisionStatus::Pending
        );
        for id in ["independent", "independent-concrete"] {
            assert_eq!(
                history
                    .decisions
                    .iter()
                    .find(|d| d.decision.id == id)
                    .unwrap()
                    .decision
                    .status,
                DecisionStatus::Active
            );
        }
        let successor = history
            .decisions
            .iter()
            .find(|d| d.decision.id == "split-design")
            .unwrap();
        let second = history
            .decisions
            .iter()
            .find(|d| d.decision.id == "split-design-second-need")
            .unwrap();
        assert_eq!(
            successor.scenes[0].accepted.observations()[0].outputs[0]
                .rows
                .len(),
            1
        );
        assert_eq!(
            second.scenes[0].accepted.observations()[0].outputs[0]
                .rows
                .len(),
            0
        );
    }
}

#[test]
fn history_displays_original_results_sources_and_unassociated_receipts() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "work", "Oak frame");
    record_need(&store, "first", DecisionOutcome::KeepCurrent);
    let before = store.load().unwrap();
    let mut original = before.program().unwrap().clone();
    original.binding.producer = Producer::Fixture {
        name: "Different original capture".into(),
    };
    original.validate().unwrap();
    let scene = accept_scene(
        &LocalRuntime::default(),
        &original,
        &scenario(&before, "second"),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let e = engine(&store);
    let pending = e
        .prepare_choice(
            &store,
            before.program().unwrap(),
            choice("second", DecisionOutcome::BothNeeded),
            vec![scene],
            "save-pending",
        )
        .unwrap();
    let saved = e.adopt(&store, &pending).unwrap();
    let view = HistoryView::load(&store, &saved, cancellation()).unwrap();
    assert_eq!(
        view.decisions[0].scenes[0]
            .accepted
            .program()
            .artifact
            .program_digest,
        view.decisions[1].scenes[0]
            .accepted
            .program()
            .artifact
            .program_digest
    );
    assert_ne!(
        view.decisions[0].scenes[0].source,
        view.decisions[1].scenes[0].source
    );
    assert!(view.decisions[1].receipts.is_empty()); // Never invent a birth association.
    let mut state = HistoryState::default();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(1200.0, 1800.0));
    let frame = |h: &mut egui_harness::EguiHarness, state: &mut HistoryState| {
        h.frame(|ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| state.show(ui, &view, true))
                .inner
        })
    };
    let (_, trace) = frame(&mut h, &mut state);
    let point = trace.controls["intention-details-first"].rect.center();
    h.press_at(point);
    frame(&mut h, &mut state);
    h.release_at(point);
    frame(&mut h, &mut state);
    let (_, trace) = frame(&mut h, &mut state);
    let text = trace.text.join("\n");
    assert!(text.contains("Oak frame"), "{text}");
    assert!(text.contains("Work results: 1 output rows"), "{text}");
    assert!(
        text.contains(view.decisions[0].scenes[0].source.as_str()),
        "{text}"
    );
    assert!(text.contains("save-pending"), "{text}");
    assert!(text.contains("No birth association is inferred"), "{text}");
}

#[test]
fn history_keeps_nonbinary_outcomes_distinct_and_does_not_accept_rejected_examples() {
    for (id, outcome, label, selectable) in [
        (
            "either",
            DecisionOutcome::EitherAcceptable,
            "Either option is acceptable",
            true,
        ),
        (
            "both",
            DecisionOutcome::BothNeeded,
            "Both ways of working are needed",
            true,
        ),
        (
            "neither",
            DecisionOutcome::NeitherFits,
            "Neither example fits",
            false,
        ),
        (
            "later",
            DecisionOutcome::Deferred,
            "Decision deferred",
            false,
        ),
    ] {
        let dir = tempdir();
        let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
        record_need(&store, id, outcome);
        let basis = store.load().unwrap();
        let view = HistoryView::load(&store, &basis, cancellation()).unwrap();
        let mut state = HistoryState::default();
        let mut h = egui_harness::EguiHarness::new(egui::vec2(1000.0, 1400.0));
        let (_, trace) = h.frame(|ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| state.show(ui, &view, true))
                .inner
        });
        let text = trace.text.join("\n");
        assert!(text.contains(label), "{text}");
        assert!(!text.contains("Keep the experienced result"), "{text}");
        assert!(!text.contains("accepted examples"), "{text}");
        assert_eq!(
            trace.controls[&format!("intention-need-{id}")].enabled,
            selectable
        );
        let point = trace.controls[&format!("intention-details-{id}")]
            .rect
            .center();
        let frame = |h: &mut egui_harness::EguiHarness, state: &mut HistoryState| {
            h.frame(|ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| state.show(ui, &view, true))
                    .inner
            })
        };
        h.press_at(point);
        frame(&mut h, &mut state);
        h.release_at(point);
        frame(&mut h, &mut state);
        let (_, expanded) = frame(&mut h, &mut state);
        let expanded = expanded.text.join("\n");
        assert!(expanded.contains("Recorded result at"), "{expanded}");
        assert!(!expanded.contains("Accepted result"), "{expanded}");
        if !selectable {
            let result = Reconciliation::new(
                &store,
                &basis,
                &engine(&store),
                "request",
                "Keep these needs",
                &[id.into()],
                "design",
                "save",
                cancellation(),
            );
            assert!(result.err().unwrap().contains("no accepted result"));
        }
        assert_eq!(store.load().unwrap(), basis);
    }
}
