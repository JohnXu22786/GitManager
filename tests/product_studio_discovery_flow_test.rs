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

#[test]
fn synthetic_record_example_is_playable_without_fabricating_a_current_copy() {
    let dir = tempdir();
    let store = ProductStore::create(
        &dir.path().join("tool"),
        &design(3, fixture_producer(), false),
        20000,
    )
    .unwrap();
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
        None,
        "discover-synthetic",
        "Show planning alternatives",
        cancel(),
    )
    .unwrap();
    let mut scenario = scene(&basis, &authored);
    scenario.seed.records[0].id = "synthetic-only".into();
    scenario.seed.events.clear();
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
    let result = DevelopmentResult {
        producer: fixture_producer(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: d.request().identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "current".into(),
                    source_json: String::from_utf8(current.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "primary".into(),
                    source_json: String::from_utf8(authored.source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "synthetic-choice".into(),
                statement: "Plan the waiting example".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "plan".into(),
                observable: "planned".into(),
                sources: vec![SourceLocus {
                    relative_path: authored.binding.program_path.clone(),
                    raw_digest: authored.artifact.raw_digest.clone(),
                    pointer: "/actions/7".into(),
                }],
                alternatives: vec!["current".into(), "primary".into()],
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
    let q = d
        .evaluate(&store, result, policy, vec![], cancel())
        .unwrap();
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
