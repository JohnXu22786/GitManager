//! Offline copied-work flow regressions. Transport fixtures are not live AI evidence.
#[path = "../src/product_studio/change_adapter.rs"]
mod change_adapter;
#[path = "../src/product_studio/discovery_flow.rs"]
mod discovery_flow;
#[path = "fixtures/product_prepared_pairs/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use discovery_flow::*;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_discovery::*;
use product_runtime::LocalRuntime;
use product_store::ProductStore;
use std::sync::{atomic::AtomicBool, Arc};

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
fn modify(
    store: &ProductStore,
    authored: &CapturedProgram,
) -> (DevelopmentRequest, DevelopmentResult) {
    let s = store.load().unwrap();
    let r = engine(store)
        .development_request(
            &s,
            "modify-plan",
            DevelopmentOperation::Modify,
            "Add planning",
            DevelopmentContext {
                view: Some("work".into()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(s.data.identity().unwrap()),
                session_digest: Some(s.session.identity().unwrap()),
            },
        )
        .unwrap();
    let out = DevelopmentResult {
        producer: authored.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: r.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: String::from_utf8(authored.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    (r, out)
}
fn draft(store: &ProductStore) -> DiscoveryDraft {
    let authored = design(1, fixture_producer(), false);
    let (request, result) = modify(store, &authored);
    let prepared = store
        .prepare_managed_evolution(
            &authored,
            &change_adapter::slot_mappings(&store.load().unwrap(), &authored).unwrap(),
            "primary",
        )
        .unwrap();
    DiscoveryDraft::after_modify(
        store,
        &store.load().unwrap(),
        request,
        result,
        "changed",
        Some(prepared),
        "discover-plan",
        "Add planning",
        cancel(),
    )
    .unwrap()
}
fn response(
    store: &ProductStore,
    draft: &DiscoveryDraft,
    value: i64,
) -> (DevelopmentResult, DiscoveryPolicy) {
    let primary = draft.primary_preparation().unwrap();
    let alternate = design(value, fixture_producer(), false);
    let (_, mut result, policy) = discovery(store, primary, &alternate);
    result.response.request_digest = draft.request().identity().unwrap();
    (result, policy)
}
fn queue(store: &ProductStore, value: i64) -> DiscoveryQueue {
    let d = draft(store);
    let (r, p) = response(store, &d, value);
    let prepared = d
        .prepare_alternatives(store, &r, "alternatives", cancel())
        .unwrap();
    d.evaluate(store, r, p, prepared, cancel()).unwrap()
}
fn open(store: &ProductStore, q: &DiscoveryQueue) -> PairExperience {
    q.open_pair(store, &q.report().questions[0].id, 0, cancel())
        .unwrap()
}

#[test]
fn new_scope_layers_route_back_to_their_original_rule_comparison() {
    for managed in [false, true] {
        let dir = tempdir();
        let store = if managed {
            managed_store(&dir.path().join("tool"))
        } else {
            ProductStore::create(&dir.path().join("tool"), &program(false), 20000).unwrap()
        };
        let basis = store.load().unwrap();
        let authored = program(true);
        let (m, r) = modify(&store, &authored);
        let layer = store
            .prepare_scoped_change(
                &authored,
                &request(&basis, product_store::scope::ScopePopulation::FutureWork),
                "prospective-layer",
            )
            .unwrap();
        assert!(matches!(
            DiscoveryDraft::after_modify(
                &store,
                &basis,
                m.clone(),
                r.clone(),
                "changed",
                Some(layer.clone()),
                "discover-layer",
                "Pause future work",
                cancel()
            ),
            Err(AdmissionError::RuleComparisonRequired)
        ));
        if managed {
            let primary = prepare(&store, &authored, "whole-design");
            let d = DiscoveryDraft::after_modify(
                &store,
                &basis,
                m,
                r,
                "changed",
                Some(primary),
                "discover-whole",
                "Explore the whole design",
                cancel(),
            )
            .unwrap();
            let result = DevelopmentResult {
                producer: authored.binding.producer.clone(),
                response: DevelopmentResponse {
                    version: 1,
                    request_digest: d.request().identity().unwrap(),
                    candidates: vec![
                        GeneratedCandidate {
                            id: "primary".into(),
                            source_json: String::from_utf8(
                                d.request().sources[1].source_bytes.clone(),
                            )
                            .unwrap(),
                        },
                        GeneratedCandidate {
                            id: "layer".into(),
                            source_json: String::from_utf8(authored.source_bytes.clone()).unwrap(),
                        },
                    ],
                    hypotheses: vec![],
                    evolutions: vec![],
                    unsupported: vec![],
                },
            };
            assert!(matches!(
                d.evaluate(
                    &store,
                    result,
                    DiscoveryPolicy::default(),
                    vec![PreparedAlternative {
                        id: "layer".into(),
                        prepared: layer,
                        mappings: vec![]
                    }],
                    cancel()
                ),
                Err(AdmissionError::RuleComparisonRequired)
            ));
        }
        assert_eq!(store.load().unwrap(), basis);
    }
}

#[test]
fn exact_modify_then_discover_keeps_authored_and_compiled_results_distinct() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let d = draft(&store);
    assert_eq!(d.request().operation, DevelopmentOperation::Discover);
    assert_ne!(d.request().id, d.modify_request().id);
    assert_eq!(
        d.request().sources[0],
        *store.load().unwrap().program().unwrap()
    );
    assert_eq!(
        d.request().sources[1],
        *d.primary_preparation().unwrap().target()
    );
    assert_ne!(
        d.request().sources[1],
        *d.primary_preparation().unwrap().candidate()
    );
    assert!(d
        .prepare_alternatives(&store, d.modify_result(), "wrong", cancel())
        .is_err());
    let (result, policy) = response(&store, &d, 2);
    let original = result.clone();
    let prepared = d
        .prepare_alternatives(&store, &result, "alt", cancel())
        .unwrap();
    let q = d
        .evaluate(&store, result, policy, prepared, cancel())
        .unwrap();
    assert_eq!(q.report().questions.len(), 1);
    assert_eq!(q.report().lowerings[0].result(), &original);
    assert_eq!(
        q.report().lowerings[0].authored().binding.producer,
        original.producer
    );
    assert_ne!(
        q.report().lowerings[0].authored(),
        q.report().lowerings[0].target()
    );
    assert!(q
        .report()
        .coverage
        .iter()
        .any(|s| s.contains("No current-side experience")));
    assert!(!q.report().runs.is_empty());
    assert!(!q.report().log.is_empty());
    let view = q.view();
    assert_eq!(
        view.questions[0].witnesses[0].labels,
        ["Option A", "Option B"]
    );
    assert!(matches!(view.state, QueueState::Questions));
}

#[test]
fn known_violations_uncertainty_and_no_questions_remain_visible() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let q = queue(&store, 0);
    assert!(q.report().questions.is_empty());
    assert!(!q.report().unverified.is_empty());
    assert!(matches!(
        q.view().state,
        QueueState::NeedsRepair | QueueState::Unverified
    ));
    assert_eq!(q.view().unverified, q.report().unverified);
    let d = draft(&store);
    let (mut r, p) = response(&store, &d, 2);
    r.response.hypotheses.clear();
    let a = d
        .prepare_alternatives(&store, &r, "empty-alt", cancel())
        .unwrap();
    let empty = d.evaluate(&store, r, p, a, cancel()).unwrap();
    assert!(empty.report().questions.is_empty());
    assert_eq!(empty.view().coverage, empty.report().coverage);
    assert!(!matches!(empty.view().state, QueueState::Questions));
}

#[test]
fn copied_pair_inputs_share_allocation_and_expire_the_old_choice_ticket() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let before = store.load().unwrap();
    let q = queue(&store, 2);
    let mut pair = open(&store, &q);
    let old = pair.view().ticket.clone();
    assert_eq!(pair.view().labels, ["Option A", "Option B"]);
    assert_eq!(pair.scenes().unwrap()[0].scenario().seed, before.data);
    assert!(pair
        .scenes()
        .unwrap()
        .iter()
        .all(|s| s.evidence().state == EvidenceState::Observed));
    pair.trial(
        &store,
        invoke(
            "add",
            &[
                ("name", text("Copied next job")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        ),
        cancel(),
    )
    .unwrap();
    assert_ne!(pair.view().ticket, old);
    for index in 0..2 {
        assert_eq!(
            pair.scenes().unwrap()[index]
                .observations()
                .last()
                .unwrap()
                .view,
            pair.view().runs[index].observation
        );
    }
    assert!(!pair.view().minimal);
    let scenes = pair.scenes().unwrap();
    assert_eq!(scenes[0].scenario(), scenes[1].scenario());
    assert_eq!(
        pair.view().runs[0]
            .retained_records
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        pair.view().runs[1]
            .retained_records
            .iter()
            .map(|r| &r.id)
            .collect::<Vec<_>>()
    );
    assert!(pair
        .prepare_choice(
            &store,
            &old,
            DecisionOutcome::Deferred,
            "old-choice",
            "old-recording",
            &[],
            cancel()
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let fresh = pair.view().ticket.clone();
    assert!(pair
        .trial(&store, invoke("missing", &[]), cancel())
        .is_err());
    assert!(pair.scenes().is_none());
    assert!(pair
        .prepare_choice(
            &store,
            &fresh,
            DecisionOutcome::Deferred,
            "failed-choice",
            "failed-recording",
            &[],
            cancel()
        )
        .is_err());
}

#[test]
fn all_four_pending_outcomes_reopen_without_installing_either_design() {
    for (i, outcome) in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ]
    .into_iter()
    .enumerate()
    {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = managed_store(&path);
        let before = store.load().unwrap();
        let q = queue(&store, 2);
        let pair = open(&store, &q);
        let change = pair
            .prepare_choice(
                &store,
                &pair.view().ticket,
                outcome.clone(),
                "pending-designs",
                &format!("record-{i}"),
                &[],
                cancel(),
            )
            .unwrap();
        let recorded = engine(&store).adopt(&store, &change).unwrap();
        assert_eq!(recorded.active_revision, before.active_revision);
        assert_eq!(recorded.data, before.data);
        assert_eq!(recorded.session, before.session);
        assert_eq!(recorded.decisions.decisions[0].outcome, outcome);
        assert_eq!(
            recorded.decisions.decisions[0].status,
            DecisionStatus::Pending
        );
        let reopened = ProductStore::open(&path).unwrap();
        assert_eq!(reopened.load().unwrap(), recorded);
        assert_eq!(
            engine(&reopened).discovery_scenes(&recorded).unwrap().len(),
            2
        );
        assert!(pair
            .prepare_choice(
                &reopened,
                &pair.view().ticket,
                DecisionOutcome::Deferred,
                "stale",
                "stale-record",
                &[],
                cancel()
            )
            .is_err());
    }
}

#[test]
fn pending_pair_reopens_after_daily_work_and_selected_resolution_uses_fresh_preparation() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = managed_store(&path);
    let basis = store.load().unwrap();
    let a = prepare(&store, &design(1, fixture_producer(), false), "saved-a");
    let b = prepare(&store, &design(2, fixture_producer(), false), "saved-b");
    let scenes = engine(&store)
        .accept_paired_scoped_scenes(
            &store,
            &a,
            &b,
            &full_scene(&basis, a.target()),
            Disclosure::ExplicitlySelected,
        )
        .unwrap();
    let remembered = engine(&store)
        .prepare_paired_rehearsed_choice(
            &store,
            a,
            b,
            choice(DecisionOutcome::BothNeeded),
            scenes.to_vec(),
            &[],
            "remember",
        )
        .unwrap();
    engine(&store).adopt(&store, &remembered).unwrap();
    let late = add(&store, "later-daily", "Later real work");
    let reopened = ProductStore::open(&path).unwrap();
    let fresh = PairExperience::reopen(&reopened, "pending-designs", "reopen", cancel()).unwrap();
    let basis = reopened.load().unwrap();
    assert_eq!(fresh.scenes().unwrap()[0].scenario().seed, basis.data);
    assert!(!fresh.view().minimal);
    let view = fresh
        .checked_view(
            &reopened,
            "check-resolution",
            &["pending-designs".into()],
            cancel(),
        )
        .unwrap();
    assert!(view.available[0], "{:?}", view.readiness_notes);
    let selected = DecisionOutcome::Accept {
        artifact: fresh.scenes().unwrap()[0]
            .program()
            .artifact
            .program_digest
            .clone(),
    };
    let change = fresh
        .prepare_choice(
            &reopened,
            &view.ticket,
            selected,
            "selected-design",
            "resolve-pair",
            &["pending-designs".into()],
            cancel(),
        )
        .unwrap();
    let adopted = engine(&reopened).adopt(&reopened, &change).unwrap();
    assert!(matches!(
        adopted.decisions.decisions[0].status,
        DecisionStatus::Superseded { .. }
    ));
    assert!(adopted.data.records.iter().any(|r| r.id == late.id));
    assert_eq!(adopted.data.records, basis.data.records);
    assert_eq!(adopted.data.events, basis.data.events);
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), adopted);
}

#[test]
fn retained_choice_suppresses_a_repeated_question_and_discloses_real_history() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = managed_store(&path);
    let q = queue(&store, 2);
    let pair = open(&store, &q);
    let change = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Deferred,
            "pending-designs",
            "remember",
            &[],
            cancel(),
        )
        .unwrap();
    engine(&store).adopt(&store, &change).unwrap();
    let reopened = ProductStore::open(&path).unwrap();
    let d = draft(&reopened);
    assert_eq!(d.request().accepted_scenes.len(), 2);
    assert!(d
        .request()
        .examples
        .iter()
        .all(|s| s.disclosure == Disclosure::ExplicitlySelected));
    let wire = encode_request(d.request(), &ProviderOptions::default()).unwrap();
    assert!(wire
        .data_categories
        .iter()
        .any(|s| s.contains("real business copies, not sanitized")));
    let (r, p) = response(&reopened, &d, 2);
    let a = d
        .prepare_alternatives(&reopened, &r, "new-alt", cancel())
        .unwrap();
    let q = d.evaluate(&reopened, r, p, a, cancel()).unwrap();
    assert!(q.report().questions.is_empty(), "{:?}", q.report().log);
    assert!(q
        .report()
        .log
        .iter()
        .any(|l| l.disposition == Disposition::Settled));
}

#[test]
fn exact_current_echo_is_reused_and_each_example_labels_its_own_sources() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let existing = prepare(
        &store,
        &design(3, fixture_producer(), false),
        "existing-plan",
    );
    store
        .adopt_scoped(store.load().unwrap().revision, &existing)
        .unwrap();
    let d = draft(&store);
    let (mut r, p) = response(&store, &d, 2);
    r.response.candidates.push(GeneratedCandidate {
        id: "current".into(),
        source_json: String::from_utf8(d.request().sources[0].source_bytes.clone()).unwrap(),
    });
    r.response.hypotheses[0].alternatives.push("current".into());
    let a = d
        .prepare_alternatives(&store, &r, "echo-alt", cancel())
        .unwrap();
    assert_eq!(
        a.len(),
        1,
        "Request-source echoes are reused, never reauthored"
    );
    let q = d.evaluate(&store, r, p, a, cancel()).unwrap();
    let view = q.view();
    let labels: Vec<_> = view
        .questions
        .iter()
        .flat_map(|q| q.witnesses.iter().map(|w| w.labels.clone()))
        .collect();
    assert!(
        labels
            .iter()
            .any(|pair| pair.iter().any(|s| s == "Current")),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|pair| pair == &["Option A", "Option B"]),
        "{labels:?}"
    );
    for question in &q.report().questions {
        for (index, witness) in question.witnesses.iter().enumerate() {
            let pair = q.open_pair(&store, &question.id, index, cancel()).unwrap();
            assert_eq!(
                pair.scenes().unwrap()[0].program(),
                witness.before_program()
            );
            assert_eq!(pair.scenes().unwrap()[1].program(), witness.after_program());
        }
    }
}

#[test]
fn reduced_example_records_its_experienced_operation_not_the_question_group() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let d = draft(&store);
    let (mut r, p) = response(&store, &d, 2);
    r.response.hypotheses[0].action = "export".into();
    let mut scenario: ScenarioSpec =
        serde_json::from_str(&r.response.hypotheses[0].scenario_json).unwrap();
    scenario.inputs.insert(1, invoke("export", &[]));
    r.response.hypotheses[0].scenario_json = serde_json::to_string(&scenario).unwrap();
    let a = d
        .prepare_alternatives(&store, &r, "group-alt", cancel())
        .unwrap();
    let q = d.evaluate(&store, r, p, a, cancel()).unwrap();
    assert_eq!(q.report().questions[0].action, "export");
    let pair = open(&store, &q);
    assert!(!pair.scenes().unwrap()[0]
        .scenario()
        .inputs
        .iter()
        .any(|i| matches!(i, SemanticInput::Invoke {action,..} if action == "export")));
    let change = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Deferred,
            "actual-plan",
            "record-actual-plan",
            &[],
            cancel(),
        )
        .unwrap();
    let recorded = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(
        recorded.decisions.decisions[0].scope.operations,
        ["plan".into()].into_iter().collect()
    );
    assert_eq!(engine(&store).discovery_scenes(&recorded).unwrap().len(), 2);
    PairExperience::reopen(&store, "actual-plan", "reopen-actual", cancel()).unwrap();
}

#[test]
fn known_primary_requirement_violation_routes_to_repair_without_preference_cards() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let authored = design(0, fixture_producer(), false);
    let (m, r) = modify(&store, &authored);
    let prepared = prepare(&store, &authored, "invalid-primary");
    let d = DiscoveryDraft::after_modify(
        &store,
        &store.load().unwrap(),
        m,
        r,
        "changed",
        Some(prepared),
        "discover-defect",
        "Add positive planning",
        cancel(),
    )
    .unwrap();
    let (r, p) = response(&store, &d, 2);
    let a = d
        .prepare_alternatives(&store, &r, "valid-alt", cancel())
        .unwrap();
    let q = d.evaluate(&store, r, p, a, cancel()).unwrap();
    assert!(q.report().questions.is_empty());
    assert!(!q.report().defects.is_empty());
    assert_eq!(q.view().state, QueueState::NeedsRepair);
}

#[test]
fn observation_only_minimum_keeps_original_example_and_replays_actions_for_recording() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let constant = |value| {
        let mut raw =
            serde_json::to_value(design(value, fixture_producer(), false).program).unwrap();
        raw["observables"][1]["value"] = serde_json::to_value(int(value)).unwrap();
        CapturedProgram::capture(
            &serde_json::to_vec(&raw).unwrap(),
            "scope-project",
            fixture_producer(),
            None,
        )
        .unwrap()
    };
    let authored = constant(1);
    let (m, r) = modify(&store, &authored);
    let primary = prepare(&store, &authored, "constant-primary");
    let d = DiscoveryDraft::after_modify(
        &store,
        &store.load().unwrap(),
        m,
        r,
        "changed",
        Some(primary),
        "discover-constant",
        "Plan positive work",
        cancel(),
    )
    .unwrap();
    let (_, mut r, p) = discovery(&store, d.primary_preparation().unwrap(), &constant(2));
    r.response.request_digest = d.request().identity().unwrap();
    let a = d
        .prepare_alternatives(&store, &r, "constant-alt", cancel())
        .unwrap();
    let q = d.evaluate(&store, r, p, a, cancel()).unwrap();
    let question = &q.report().questions[0];
    let witness = &question.witnesses[0];
    assert!(witness
        .witness()
        .scenario
        .inputs
        .iter()
        .all(|i| !matches!(i, SemanticInput::Invoke { .. })));
    let exact = q.open_example(&store, &question.id, 0, cancel()).unwrap();
    assert_eq!(exact.original().witness(), witness.witness());
    let pair = open(&store, &q);
    assert!(!pair.view().minimal);
    assert!(pair.scenes().unwrap()[0]
        .scenario()
        .inputs
        .iter()
        .any(|i| matches!(i, SemanticInput::Invoke {action,..} if action == "plan")));
    let change = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Deferred,
            "constant-choice",
            "constant-record",
            &[],
            cancel(),
        )
        .unwrap();
    let saved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(engine(&store).discovery_scenes(&saved).unwrap().len(), 2);
}

fn ordinary_or_managed_queue(
    managed: bool,
    synthetic: bool,
    two_prospective: bool,
) -> (
    tempfile::TempDir,
    ProductStore,
    product_store::ProjectSnapshot,
    DiscoveryQueue,
    DevelopmentResult,
    DiscoveryPolicy,
) {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = if managed {
        let store = managed_store(&path);
        let existing = prepare(
            &store,
            &design(3, fixture_producer(), false),
            "existing-plan",
        );
        store
            .adopt_scoped(store.load().unwrap().revision, &existing)
            .unwrap();
        store
    } else {
        let current = if two_prospective {
            program(true)
        } else {
            design(3, fixture_producer(), false)
        };
        ProductStore::create(&path, &current, 20000).unwrap()
    };
    add(&store, "real-row", "Real saved work");
    let basis = store.load().unwrap();
    let authored = design(1, fixture_producer(), false);
    let (m, r) = modify(&store, &authored);
    let d = DiscoveryDraft::after_modify(
        &store,
        &basis,
        m,
        r,
        "changed",
        managed.then(|| prepare(&store, &authored, "prospective-plan")),
        "discover-synthetic",
        "Show planning alternatives",
        cancel(),
    )
    .unwrap();
    let target = &d.request().sources[1];
    let mut scenario = scene(&basis, target);
    if synthetic {
        scenario.seed.records[0].id = "synthetic-only".into();
        scenario.seed.events.clear();
    }
    scenario.inputs.insert(
        0,
        invoke("wait", &[("row", reference(&scenario.seed.records[0]))]),
    );
    let requirement = AcceptedProperty {
        id: "waiting".into(),
        description: "Keep the waiting example meaningful".into(),
        predicate: PropertyPredicate::Less {
            left: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 0 },
            },
            right: PropertyTerm::Observed {
                point: "result".into(),
                observable: "waiting_jobs".into(),
                value_type: Type::Integer,
            },
        },
    };
    let current = basis.program().unwrap();
    let first = if two_prospective { target } else { current };
    let second = if two_prospective {
        design(2, fixture_producer(), false)
    } else {
        target.clone()
    };
    let result = DevelopmentResult {
        producer: fixture_producer(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: d.request().identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "first".into(),
                    source_json: String::from_utf8(first.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "second".into(),
                    source_json: String::from_utf8(second.source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "synthetic-choice".into(),
                statement: "Plan the waiting example".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "plan".into(),
                observable: "planned".into(),
                sources: vec![SourceLocus {
                    relative_path: target.binding.program_path.clone(),
                    raw_digest: target.artifact.raw_digest.clone(),
                    pointer: "/actions/7".into(),
                }],
                alternatives: vec!["first".into(), "second".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scenario).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut policy = DiscoveryPolicy::default();
    policy
        .workflow_validity
        .insert("plan".into(), vec![requirement]);
    if two_prospective {
        // This is an independent host requirement, not provider-authored
        // permission to treat an arbitrary missing Current feature as a choice.
        policy.required_actions.insert("plan".into());
        policy.requirements.push(RequirementCase {
            id: "positive-new-plan".into(),
            scenario: scenario.clone(),
            properties: vec![AcceptedProperty {
                id: "positive".into(),
                description: "The requested action produces a positive plan".into(),
                predicate: PropertyPredicate::Less {
                    left: PropertyTerm::Literal {
                        value_type: Type::Integer,
                        value: DataValue::Integer { value: 0 },
                    },
                    right: PropertyTerm::Observed {
                        point: "result".into(),
                        observable: "planned".into(),
                        value_type: Type::Integer,
                    },
                },
            }],
        });
    }
    let q = d
        .evaluate(&store, result.clone(), policy.clone(), vec![], cancel())
        .unwrap();
    if two_prospective {
        assert!(
            !q.report().questions.is_empty(),
            "No checked ordinary question: logs={:?}, defects={:?}, unverified={:?}, coverage={:?}",
            q.report().log,
            q.report().defects,
            q.report().unverified,
            q.report().coverage
        );
    }
    (dir, store, basis, q, result, policy)
}

#[test]
fn ordinary_unprepared_design_can_be_experienced_and_selected_on_saved_work() {
    let (_dir, store, basis, q, _, _) = ordinary_or_managed_queue(false, false, false);
    let pair = open(&store, &q);
    let artifact = pair
        .scenes()
        .unwrap()
        .iter()
        .find(|scene| scene.program() != basis.program().unwrap())
        .unwrap()
        .program()
        .artifact
        .program_digest
        .clone();
    let selected = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Accept {
                artifact: artifact.clone(),
            },
            "ordinary-choice",
            "ordinary-adoption",
            &[],
            cancel(),
        )
        .unwrap();
    let adopted = engine(&store).adopt(&store, &selected).unwrap();
    assert_eq!(adopted.program().unwrap().artifact.program_digest, artifact);
    assert_eq!(adopted.data.records, basis.data.records);
    assert_eq!(adopted.data.events, basis.data.events);
    assert_eq!(store.load().unwrap(), adopted);
}

fn assert_current_alternative_reopens(managed: bool) {
    let (dir, store, basis, q, _, _) = ordinary_or_managed_queue(managed, false, false);
    let pair = open(&store, &q);
    let alternative = pair
        .scenes()
        .unwrap()
        .iter()
        .find(|scene| scene.program() != basis.program().unwrap())
        .unwrap()
        .program()
        .clone();
    let change = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Deferred,
            "pending-current-alternative",
            "record-current-alternative",
            &[],
            cancel(),
        )
        .unwrap();
    let recorded = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(recorded.program().unwrap(), basis.program().unwrap());
    assert_eq!(recorded.data, basis.data);
    assert_eq!(recorded.session, basis.session);
    if !managed {
        assert!(!recorded.programs.contains(&alternative),
            "This regression needs the ordinary alternative retained only in its checked scene object");
    }
    let late = add(&store, "later-current-alternative", "Later real work");
    let reopened = ProductStore::open(&dir.path().join("tool")).unwrap();
    let fresh_basis = reopened.load().unwrap();
    let fresh = PairExperience::reopen(
        &reopened,
        "pending-current-alternative",
        "reopen-current-alternative",
        cancel(),
    )
    .unwrap();
    assert!(fresh.view().labels.iter().any(|label| label == "Current"));
    assert!(!fresh.view().minimal);
    for scene in fresh.scenes().unwrap() {
        assert_eq!(scene.scenario().seed, fresh_basis.data);
        assert!(scene
            .scenario()
            .seed
            .records
            .iter()
            .any(|r| r.id == late.id));
    }
    if !managed {
        assert!(fresh
            .scenes()
            .unwrap()
            .iter()
            .any(|scene| scene.program() == &alternative));
    }
    let resolved = fresh
        .prepare_choice(
            &reopened,
            &fresh.view().ticket,
            DecisionOutcome::KeepCurrent,
            "selected-current",
            "resolve-current-alternative",
            &["pending-current-alternative".into()],
            cancel(),
        )
        .unwrap();
    let adopted = engine(&reopened).adopt(&reopened, &resolved).unwrap();
    assert_eq!(adopted.program().unwrap(), fresh_basis.program().unwrap());
    assert_eq!(adopted.data.records, fresh_basis.data.records);
    assert_eq!(adopted.data.events, fresh_basis.data.events);
    assert!(matches!(
        adopted.decisions.decisions[0].status,
        DecisionStatus::Superseded { .. }
    ));
}

#[test]
fn ordinary_current_alternative_pending_choice_reopens_from_archived_capture() {
    assert_current_alternative_reopens(false);
}

#[test]
fn managed_current_alternative_pending_choice_reopens_without_current_rehearsal() {
    assert_current_alternative_reopens(true);
}

#[test]
fn pending_new_layer_reopens_through_its_rule_comparison() {
    let dir = tempdir();
    let store = ProductStore::create(&dir.path().join("tool"), &program(false), 20000).unwrap();
    let row = add(&store, "row", "Waiting work");
    action(&store, "wait", "wait", &row);
    tick(&store, "days", 20003);
    let basis = store.load().unwrap();
    let proposal = store
        .prepare_scoped_change(
            &program(true),
            &request(&basis, product_store::scope::ScopePopulation::All),
            "new-layer",
        )
        .unwrap();
    let scenario = ScenarioSpec {
        version: 1,
        id: "pending-rule".into(),
        label: "Compare a waiting rule".into(),
        seed: proposal.seed().clone(),
        session: SessionState::initial(&proposal.target().program).unwrap(),
        clock_day: basis.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&row))]),
            invoke("complete", &[("row", reference(&row))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let checker = engine(&store);
    let current = checker
        .accept_prepared_current_scene(&store, &proposal, &scenario, Disclosure::ExplicitlySelected)
        .unwrap();
    let alternative = checker
        .accept_scoped_scene(&store, &proposal, &scenario, Disclosure::ExplicitlySelected)
        .unwrap();
    let pending = Choice {
        id: "pending-rule".into(),
        request: "Keep this rule choice".into(),
        rationale: None,
        scope: proposal.scope().clone(),
        outcome: DecisionOutcome::Deferred,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = checker
        .prepare_rehearsed_choice(
            &store,
            proposal,
            pending,
            vec![current, alternative],
            &[],
            "record-rule",
        )
        .unwrap();
    let saved = checker.adopt(&store, &change).unwrap();
    assert!(matches!(
        PairExperience::reopen(&store, "pending-rule", "reopen-rule", cancel()),
        Err(AdmissionError::RuleComparisonRequired)
    ));
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn reopened_choice_observes_actions_after_the_original_last_point() {
    let dir = tempdir();
    let current = design(1, fixture_producer(), false);
    let store = ProductStore::create(&dir.path().join("tool"), &current, 20000).unwrap();
    let row = add(&store, "row", "Copied work");
    let basis = store.load().unwrap();
    let alternative = design(2, fixture_producer(), false);
    let mut scenario = scene(&basis, &current);
    scenario
        .inputs
        .push(invoke("wait", &[("row", reference(&row))]));
    let checker = engine(&store);
    let a = checker
        .accept_current_scene(&basis, &scenario, Disclosure::ExplicitlySelected)
        .unwrap();
    let b = accept_scene(
        &LocalRuntime::default(),
        &alternative,
        &scenario,
        Disclosure::ExplicitlySelected,
        RuntimeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        a.observations().last().unwrap().values["waiting_jobs"],
        DataValue::Integer { value: 0 }
    );
    let mut pending = choice(DecisionOutcome::Deferred);
    pending.scope.operations.insert("wait".into());
    let saved = checker
        .prepare_choice(&store, &current, pending, vec![a, b], "save-tail")
        .unwrap();
    checker.adopt(&store, &saved).unwrap();
    let mut fresh =
        PairExperience::reopen(&store, "pending-designs", "reopen-tail", cancel()).unwrap();
    assert!(!fresh.view().minimal);
    for (index, accepted) in fresh.scenes().unwrap().iter().enumerate() {
        assert_eq!(
            accepted.observations().last().unwrap().view,
            fresh.view().runs[index].observation
        );
        assert_eq!(
            accepted.observations().last().unwrap().values["waiting_jobs"],
            DataValue::Integer { value: 1 }
        );
        assert_eq!(accepted.observations()[0].point, "result");
    }
    fresh
        .trial(
            &store,
            invoke("resume", &[("row", reference(&row))]),
            cancel(),
        )
        .unwrap();
    for (index, accepted) in fresh.scenes().unwrap().iter().enumerate() {
        assert_eq!(
            accepted.observations().len(),
            2,
            "Only the host-owned trailing point is replaced"
        );
        assert_eq!(
            accepted.observations().last().unwrap().view,
            fresh.view().runs[index].observation
        );
        assert_eq!(
            accepted.observations().last().unwrap().values["waiting_jobs"],
            DataValue::Integer { value: 0 }
        );
    }
    let resolved = fresh
        .prepare_choice(
            &store,
            &fresh.view().ticket,
            DecisionOutcome::KeepCurrent,
            "tail-current",
            "resolve-tail",
            &["pending-designs".into()],
            cancel(),
        )
        .unwrap();
    let adopted = checker.adopt(&store, &resolved).unwrap();
    assert_eq!(adopted.data, basis.data);
    assert_eq!(adopted.session, basis.session);
}

#[test]
fn ordinary_prospective_pair_reopens_when_current_lacks_the_new_feature() {
    let (dir, store, basis, q, mut next_result, policy) =
        ordinary_or_managed_queue(false, false, true);
    assert!(!basis
        .program()
        .unwrap()
        .program
        .actions
        .iter()
        .any(|action| action.id == "plan"));
    let pair = open(&store, &q);
    assert_eq!(pair.view().labels, ["Option A", "Option B"]);
    let pending = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::BothNeeded,
            "new-ordinary-feature",
            "record-new-feature",
            &[],
            cancel(),
        )
        .unwrap();
    let recorded = engine(&store).adopt(&store, &pending).unwrap();
    assert_eq!(recorded.program().unwrap(), basis.program().unwrap());
    assert_eq!(recorded.data, basis.data);
    let late = add(&store, "later-ordinary-feature", "Later saved work");
    let reopened = ProductStore::open(&dir.path().join("tool")).unwrap();
    let fresh = PairExperience::reopen(
        &reopened,
        "new-ordinary-feature",
        "reopen-new-feature",
        cancel(),
    )
    .unwrap();
    assert_eq!(fresh.view().labels, ["Option A", "Option B"]);
    for scene in fresh.scenes().unwrap() {
        assert!(scene
            .scenario()
            .seed
            .records
            .iter()
            .any(|r| r.id == late.id));
        assert!(scene
            .program()
            .program
            .actions
            .iter()
            .any(|a| a.id == "plan"));
    }
    // A saved pending need must survive the next real Modify -> Discover
    // envelope and retained-history evaluation before the person resolves it.
    let authored = design(1, fixture_producer(), false);
    let (modify_request, modify_result) = modify(&reopened, &authored);
    let next = DiscoveryDraft::after_modify(
        &reopened,
        &reopened.load().unwrap(),
        modify_request,
        modify_result,
        "changed",
        None,
        "discover-after-pending",
        "Keep the saved planning need",
        cancel(),
    )
    .unwrap();
    assert_eq!(next.request().accepted_scenes.len(), 2);
    assert!(next
        .request()
        .accepted_scenes
        .iter()
        .all(|scene| scene.decision == "new-ordinary-feature"));
    next_result.response.request_digest = next.request().identity().unwrap();
    let repeated = next
        .evaluate(&reopened, next_result, policy, vec![], cancel())
        .unwrap();
    assert!(
        repeated.report().questions.is_empty(),
        "{:?}",
        repeated.report().log
    );
    assert!(
        repeated
            .report()
            .log
            .iter()
            .any(|entry| entry.disposition == Disposition::Settled),
        "{:?}",
        repeated.report().log
    );
    let selected = DecisionOutcome::Accept {
        artifact: fresh.scenes().unwrap()[0]
            .program()
            .artifact
            .program_digest
            .clone(),
    };
    let change = fresh
        .prepare_choice(
            &reopened,
            &fresh.view().ticket,
            selected,
            "ordinary-feature-selected",
            "resolve-new-feature",
            &["new-ordinary-feature".into()],
            cancel(),
        )
        .unwrap();
    let adopted = engine(&reopened).adopt(&reopened, &change).unwrap();
    assert!(adopted
        .program()
        .unwrap()
        .program
        .actions
        .iter()
        .any(|action| action.id == "plan"));
    assert!(adopted.data.records.iter().any(|r| r.id == late.id));
    assert!(matches!(
        adopted.decisions.decisions[0].status,
        DecisionStatus::Superseded { .. }
    ));
}

#[test]
fn pending_original_replay_requires_exact_fresh_pending_decision() {
    let (_dir, store, _, q, _, _) = ordinary_or_managed_queue(false, false, false);
    let pair = open(&store, &q);
    let prepared = pair
        .prepare_choice(
            &store,
            &pair.view().ticket,
            DecisionOutcome::Deferred,
            "history-check",
            "record-history-check",
            &[],
            cancel(),
        )
        .unwrap();
    let checker = engine(&store);
    let recorded = checker.adopt(&store, &prepared).unwrap();
    let decision = recorded.decisions.decisions[0].clone();
    let originals = checker
        .pending_original_scenes(&recorded, &decision)
        .unwrap();
    assert_eq!(originals, pair.scenes().unwrap().to_vec());

    let mut wrong = decision.clone();
    wrong.id = "absent-decision".into();
    assert!(checker.pending_original_scenes(&recorded, &wrong).is_err());
    let mut tampered = decision.clone();
    tampered.request = "A different claimed need".into();
    assert!(checker
        .pending_original_scenes(&recorded, &tampered)
        .is_err());
    tampered = decision.clone();
    tampered.witness = canonical_digest(IdentityDomain::Evidence, &"different-scenes").unwrap();
    assert!(checker
        .pending_original_scenes(&recorded, &tampered)
        .is_err());

    add(&store, "later-history-check", "Later saved work");
    let current = store.load().unwrap();
    assert!(checker
        .pending_original_scenes(&recorded, &decision)
        .is_err());
    assert_eq!(
        checker
            .pending_original_scenes(&current, &decision)
            .unwrap(),
        originals
    );
    let reopened =
        PairExperience::reopen(&store, "history-check", "reopen-history-check", cancel()).unwrap();
    let selected = reopened
        .prepare_choice(
            &store,
            &reopened.view().ticket,
            DecisionOutcome::KeepCurrent,
            "history-selected",
            "resolve-history-check",
            &["history-check".into()],
            cancel(),
        )
        .unwrap();
    let adopted = checker.adopt(&store, &selected).unwrap();
    let past = adopted
        .decisions
        .decisions
        .iter()
        .find(|d| d.id == "history-check")
        .unwrap();
    assert!(matches!(past.status, DecisionStatus::Superseded { .. }));
    assert!(checker
        .pending_original_scenes(&adopted, &decision)
        .is_err());
    assert!(checker.pending_original_scenes(&adopted, past).is_err());
    let active = adopted
        .decisions
        .decisions
        .iter()
        .find(|d| d.status == DecisionStatus::Active)
        .unwrap();
    assert!(checker.pending_original_scenes(&adopted, active).is_err());
    assert!(checker.discovery_scenes(&adopted).unwrap().is_empty());
    let mut verifier = engine(&store);
    let losing_feature =
        verifier.check_discovery_candidate(&adopted, &program(true), &[], RuntimeLimits::default());
    assert!(losing_feature.map(|report| report.disposition != CheckDisposition::Ready).unwrap_or(true),
        "An active promise cannot become unrelated historical evidence when the proposed tool loses its feature");
    assert_eq!(store.load().unwrap(), adopted);
}

#[test]
fn pending_history_keeps_current_replay_and_rejects_mismatched_pairs() {
    for current_has_feature in [true, false] {
        let dir = tempdir();
        let current = if current_has_feature {
            design(3, fixture_producer(), false)
        } else {
            program(true)
        };
        let store = ProductStore::create(&dir.path().join("tool"), &current, 20000).unwrap();
        add(&store, "row", "Saved work");
        let basis = store.load().unwrap();
        let a = design(1, fixture_producer(), false);
        let b = design(2, fixture_producer(), false);
        let first = scene(&basis, &a);
        let mut second = first.clone();
        if !current_has_feature {
            second.id = "a-different-saved-scenario".into();
        }
        let original_a = accept_scene(
            &LocalRuntime::default(),
            &a,
            &first,
            Disclosure::ExplicitlySelected,
            RuntimeLimits::default(),
        )
        .unwrap();
        let original_b = accept_scene(
            &LocalRuntime::default(),
            &b,
            &second,
            Disclosure::ExplicitlySelected,
            RuntimeLimits::default(),
        )
        .unwrap();
        let checker = engine(&store);
        let change = checker
            .prepare_choice(
                &store,
                basis.program().unwrap(),
                choice(DecisionOutcome::BothNeeded),
                vec![original_a, original_b],
                "record-boundary",
            )
            .unwrap();
        let saved = checker.adopt(&store, &change).unwrap();
        if current_has_feature {
            let mapped = checker.discovery_scenes(&saved).unwrap();
            assert_eq!(mapped.len(), 2);
            for scene in mapped {
                assert_eq!(scene.replay().binding.source, current.binding);
                assert_ne!(scene.original().binding.source, current.binding);
            }
            let mut tampered = saved.clone();
            tampered.decisions.decisions[0].request = "Altered claimed history".into();
            assert!(checker.discovery_scenes(&tampered).is_err());
        } else {
            assert!(
                checker.discovery_scenes(&saved).is_err(),
                "Different original scenarios cannot enter the same-input prospective fallback"
            );
        }
        assert_eq!(store.load().unwrap(), saved);
    }
}

#[test]
fn synthetic_record_example_is_playable_without_fabricating_a_current_copy() {
    let (_dir, store, basis, q, _, _) = ordinary_or_managed_queue(false, true, false);
    let question = &q.report().questions[0];
    let view = q.checked_view(&store, cancel()).unwrap();
    assert!(view.questions[0].witnesses[0].playable);
    assert!(!view.questions[0].witnesses[0].copyable);
    assert!(view.questions[0].witnesses[0].copy_issue.is_some());
    let mut example = q.open_example(&store, &question.id, 0, cancel()).unwrap();
    assert_eq!(
        example.original().witness(),
        question.witnesses[0].witness()
    );
    assert!(example.view().runs[0]
        .retained_records
        .iter()
        .any(|r| r.id == "synthetic-only"));
    example
        .trial(&store, invoke("plan", &[]), cancel())
        .unwrap();
    assert!(!example.view().minimal);
    assert_eq!(store.load().unwrap(), basis);
    assert!(q.open_pair(&store, &question.id, 0, cancel()).is_err());
}

#[test]
fn changed_business_basis_cannot_accept_a_late_discovery_result() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let d = draft(&store);
    let (r, p) = response(&store, &d, 2);
    let a = d
        .prepare_alternatives(&store, &r, "late-alt", cancel())
        .unwrap();
    add(&store, "daily-work", "Real work while discovery ran");
    assert!(d.evaluate(&store, r, p, a, cancel()).is_err());
}

#[cfg(unix)]
#[test]
fn actual_fixture_transport_runs_modify_then_discover_with_separate_receipts() {
    use product_provider::{unix_ms, ConsentReceipt, JobState, ProviderKind, ProviderTransport};
    use std::{fs, os::unix::fs::PermissionsExt};
    fn run(
        root: &std::path::Path,
        request: &DevelopmentRequest,
        result: &DevelopmentResult,
    ) -> (DevelopmentResult, product_provider::JobReceipt) {
        fs::create_dir_all(root).unwrap();
        let bin = root.join("fixture.py");
        let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            "cfg['response']",
        );
        {
            let _guard = product_provider::fixture_executable_write_guard();
            fs::write(&bin, script).unwrap();
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(
                bin.with_extension("json"),
                serde_json::to_vec(&serde_json::json!({"response":result.response})).unwrap(),
            )
            .unwrap();
        }
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        let t = ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home)
            .unwrap();
        let p = prepare_development(t, request, ProviderOptions::default()).unwrap();
        let c = ConsentReceipt {
            disclosure_digest: p.disclosure().digest(),
            approval_reference: "synthetic fixture only".into(),
            expires_at_unix_ms: unix_ms() + 60_000,
        };
        let bridge = p.authorize(c);
        let out = bridge.develop(request, &|| false).unwrap();
        (out, bridge.receipt().unwrap())
    }
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let (m, fixture_result) = modify(&store, &design(1, fixture_producer(), false));
    let (m_result, m_receipt) = run(&dir.path().join("modify"), &m, &fixture_result);
    let authored = CapturedProgram::capture(
        m_result.response.candidates[0].source_json.as_bytes(),
        &m.project_id,
        m_result.producer.clone(),
        None,
    )
    .unwrap();
    let prepared = store
        .prepare_managed_evolution(
            &authored,
            &change_adapter::slot_mappings(&store.load().unwrap(), &authored).unwrap(),
            "primary",
        )
        .unwrap();
    let d = DiscoveryDraft::after_modify(
        &store,
        &store.load().unwrap(),
        m,
        m_result,
        "changed",
        Some(prepared),
        "discover-plan",
        "Add planning",
        cancel(),
    )
    .unwrap();
    let (fixture_result, p) = response(&store, &d, 2);
    let (r, receipt) = run(&dir.path().join("discover"), d.request(), &fixture_result);
    assert_eq!(receipt.state, JobState::TransportValidated);
    assert_eq!(m_receipt.state, JobState::TransportValidated);
    assert_ne!(receipt.request_id, m_receipt.request_id);
    assert!(matches!(r.producer, Producer::Fixture { .. }));
    let a = d
        .prepare_alternatives(&store, &r, "transport-alts", cancel())
        .unwrap();
    let q = d.evaluate(&store, r, p, a, cancel()).unwrap();
    assert_eq!(q.report().questions.len(), 1);
}
