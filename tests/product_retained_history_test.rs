//! Real store/decision/discovery integration over synthetic executable scenes.
//! Fixture responses are not live AI or native desktop acceptance evidence.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
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
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_discovery::*;
use product_runtime::LocalRuntime;
use product_store::ProductStore;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{atomic::AtomicBool, Arc},
};

fn replacement() -> Value {
    let mut p = filtered();
    p["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":p["actions"][1]["steps"][0]["items"].clone()});
    p
}
fn workflow(p: &CapturedProgram) -> ScenarioSpec {
    scenario(
        p,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("Ada"),
            },
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    )
}
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
fn accepted(p: &CapturedProgram, s: &ScenarioSpec) -> AcceptedScene {
    accept_scene(
        &LocalRuntime::default(),
        p,
        s,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap()
}
fn save(
    store: &ProductStore,
    outcome: DecisionOutcome,
    binding: IntentionBinding,
    properties: Vec<AcceptedProperty>,
    scenes: Vec<AcceptedScene>,
) {
    let current = store.load().unwrap();
    let choice = Choice {
        id: "selected".into(),
        request: "Keep these collection outcomes".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["collect".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome,
        obligations: properties,
        binding,
    };
    let e = engine(store);
    let plan = e
        .prepare_choice(
            store,
            current.program().unwrap(),
            choice,
            scenes,
            "save-choice",
        )
        .unwrap();
    e.adopt(store, &plan).unwrap();
}
fn positive_count() -> AcceptedProperty {
    AcceptedProperty {
        id: "positive".into(),
        description: "At least one selected person".into(),
        predicate: PropertyPredicate::Less {
            left: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 0 },
            },
            right: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
        },
    }
}
fn request(store: &ProductStore, candidate: &CapturedProgram) -> DevelopmentRequest {
    let current = store.load().unwrap();
    engine(store)
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "discover".into(),
                project_id: "runtime-project".into(),
                operation: DevelopmentOperation::Discover,
                request: "Improve collection".into(),
                sources: vec![current.program().unwrap().clone(), candidate.clone()],
                context: DevelopmentContext {
                    view: Some("people".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: None,
                    session_digest: None,
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: BTreeSet::new(),
            },
        )
        .unwrap()
}
fn response(
    r: &DevelopmentRequest,
    other: &CapturedProgram,
    s: &ScenarioSpec,
) -> DevelopmentResult {
    DevelopmentResult {
        producer: Producer::Fixture {
            name: "retained-history response".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: r.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "other".into(),
                    source_json: String::from_utf8(other.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "candidate".into(),
                    source_json: String::from_utf8(r.sources[1].source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "collect-choice".into(),
                statement: "Collection may differ".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "collect".into(),
                observable: "selected_count".into(),
                sources: vec![SourceLocus {
                    relative_path: "program.json".into(),
                    raw_digest: r.sources[1].artifact.raw_digest.clone(),
                    pointer: "/actions/1/steps/0".into(),
                }],
                alternatives: vec!["other".into(), "candidate".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(s).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    }
}
fn policy(store: &ProductStore) -> DiscoveryPolicy {
    DiscoveryPolicy {
        retained_history: Some(VerifiedRetainedHistory::load(store).unwrap()),
        ..DiscoveryPolicy::default()
    }
}
fn discover_with(
    store: &ProductStore,
    candidate: &CapturedProgram,
    other: &CapturedProgram,
    s: &ScenarioSpec,
) -> DiscoveryReport {
    let r = request(store, candidate);
    discover(
        &r,
        &response(&r, other, s),
        &policy(store),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}

#[test]
fn concrete_accept_and_keep_current_survive_reopen_without_inventing_a_rejected_side() {
    for keep in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let p = capture(filtered());
        let s = workflow(&p);
        let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
        let outcome = if keep {
            DecisionOutcome::KeepCurrent
        } else {
            DecisionOutcome::Accept {
                artifact: p.artifact.program_digest.clone(),
            }
        };
        save(
            &store,
            outcome,
            IntentionBinding::ObservedOutcome,
            vec![],
            vec![accepted(&p, &s)],
        );
        drop(store);
        let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
        let mut next = filtered();
        next["label"] = json!("Reimplemented label");
        let candidate = capture(next);
        let report = discover_with(&reopened, &candidate, &capture(replacement()), &s);
        assert!(report.questions.is_empty());
        assert!(report.defects.is_empty(), "{:?}", report.defects);
        assert!(report.unverified.is_empty(), "{:?}", report.unverified);
        assert!(report
            .checks
            .iter()
            .any(|c| c.state == CheckState::Satisfied));
    }
}
#[test]
fn concrete_outcome_with_passing_property_blocks_changed_candidate_before_questions() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(filtered());
    let s = workflow(&p);
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::ObservedOutcome,
        vec![positive_count()],
        vec![accepted(&p, &s)],
    );
    let report = discover_with(&store, &capture(replacement()), &p, &s);
    assert!(report.questions.is_empty());
    assert!(!report.defects.is_empty());
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Violated));
}
#[test]
fn concrete_outcome_gate_checks_offered_alternatives_too() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(filtered());
    let s = workflow(&p);
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::ObservedOutcome,
        vec![positive_count()],
        vec![accepted(&p, &s)],
    );
    // A new observable makes the captured candidate a real edit without changing
    // the approved concrete result; the offered replacement violates that result.
    let mut next = filtered();
    next["observables"].as_array_mut().unwrap().push(json!({"id":"extra","label":"Extra","value":json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":9}})}));
    let candidate = capture(next);
    let report = discover_with(&store, &candidate, &capture(replacement()), &s);
    assert!(report.questions.is_empty());
    assert!(report.checks.iter().any(|c| c.state == CheckState::Violated
        && c.binding.artifact == capture(replacement()).artifact));
    assert!(report
        .log
        .iter()
        .all(|l| l.disposition != Disposition::Question));
}
#[test]
fn explicit_properties_only_does_not_accidentally_bind_concrete_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(filtered());
    let s = workflow(&p);
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::PropertiesOnly,
        vec![positive_count()],
        vec![accepted(&p, &s)],
    );
    let report = discover_with(&store, &capture(replacement()), &p, &s);
    assert!(report.defects.is_empty(), "{:?}", report.defects);
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
    assert_eq!(report.questions.len(), 1);
}
#[test]
fn both_real_nonbinary_scenes_survive_restart_and_only_new_outcomes_reopen() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let a = capture(filtered());
        let b = capture(replacement());
        let s = workflow(&a);
        let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
        save(
            &store,
            outcome,
            IntentionBinding::ObservedOutcome,
            vec![],
            vec![accepted(&a, &s), accepted(&b, &s)],
        );
        drop(store);
        let store = ProductStore::open(dir.path().join("tool")).unwrap();
        let r = request(&store, &b);
        assert_eq!(r.accepted_scenes.len(), 2);
        let unchanged = discover_with(&store, &b, &a, &s);
        assert!(unchanged.questions.is_empty());
        assert!(
            unchanged.unverified.is_empty(),
            "{:?}",
            unchanged.unverified
        );
        assert!(unchanged
            .log
            .iter()
            .any(|l| l.disposition == Disposition::Settled));
        let mut novel = replacement();
        novel["actions"][1]["steps"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")});
        let changed = discover_with(&store, &capture(novel), &a, &s);
        assert_eq!(changed.questions.len(), 1, "{:?}", changed.unverified);
    }
}
#[test]
fn missing_tampered_and_ambiguous_history_never_become_settled_or_new_questions() {
    let dir = tempfile::tempdir().unwrap();
    let a = capture(filtered());
    let b = capture(replacement());
    let s = workflow(&a);
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::Deferred,
        IntentionBinding::ObservedOutcome,
        vec![],
        vec![accepted(&a, &s), accepted(&b, &s)],
    );
    for corruption in 0..3 {
        let mut r = request(&store, &b);
        let p = policy(&store);
        match corruption {
            0 => {
                r.accepted_scenes.pop();
            }
            1 => {
                r.accepted_scenes[0].observations[0]
                    .values
                    .insert("selected_count".into(), DataValue::Integer { value: 99 });
            }
            _ => {
                r.decisions.decisions[0].witness = r.sources[0].artifact.program_digest.clone();
            }
        }
        let result = response(&r, &a, &s);
        let report = discover(&r, &result, &p, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(report.questions.is_empty());
        assert!(!report.unverified.is_empty());
        assert!(report
            .log
            .iter()
            .all(|l| l.disposition != Disposition::Settled));
    }
}

#[test]
fn multiple_scenes_and_source_qualified_mappings_remain_checked_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let original = capture(filtered());
    let first = workflow(&original);
    let mut second = first.clone();
    second.id = "other-scene".into();
    second.inputs[0] = add("Amy");
    second.inputs[1] = add("Zara");
    let store = ProductStore::create(dir.path().join("tool"), &original, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::ObservedOutcome,
        vec![],
        vec![accepted(&original, &first), accepted(&original, &second)],
    );
    let mut renamed = filtered();
    renamed["actions"][1]["id"] = json!("gather");
    renamed["observables"][0]["id"] = json!("picked_count");
    let target = capture(renamed.clone());
    let mappings = vec![
        SemanticMapping {
            from: SemanticKey {
                kind: SemanticKind::Action,
                entity: None,
                id: "collect".into(),
            },
            to: SemanticKey {
                kind: SemanticKind::Action,
                entity: None,
                id: "gather".into(),
            },
        },
        SemanticMapping {
            from: SemanticKey {
                kind: SemanticKind::Observable,
                entity: None,
                id: "selected_count".into(),
            },
            to: SemanticKey {
                kind: SemanticKind::Observable,
                entity: None,
                id: "picked_count".into(),
            },
        },
    ];
    let e = engine(&store);
    let plan = e
        .prepare_change(&store, &target, &mappings, "rename")
        .unwrap();
    e.adopt(&store, &plan).unwrap();
    drop(store);
    let store = ProductStore::open(dir.path().join("tool")).unwrap();
    renamed["label"] = json!("Refactored collector");
    let next = capture(renamed);
    let r = request(&store, &next);
    assert_eq!(r.accepted_scenes.len(), 2);
    let mut scene = first.clone();
    for input in &mut scene.inputs {
        if let SemanticInput::Invoke { action, .. } = input {
            if action == "collect" {
                *action = "gather".into();
            }
        }
    }
    let mut out = response(&r, &target, &scene);
    out.response.hypotheses[0].action = "gather".into();
    out.response.hypotheses[0].observable = "picked_count".into();
    let report = discover(&r, &out, &policy(&store), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    assert!(report.defects.is_empty());
    assert!(report.unverified.is_empty(), "{:?}", report.unverified);
    assert_eq!(
        report
            .checks
            .iter()
            .filter(|c| c.state == CheckState::Satisfied)
            .count(),
        2
    );
    assert!(report
        .checks
        .iter()
        .all(|c| c.binding.artifact == next.artifact));
}
#[test]
fn retained_package_corruption_or_deletion_after_capture_is_unverified() {
    for remove in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let a = capture(filtered());
        let b = capture(replacement());
        let s = workflow(&a);
        let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
        save(
            &store,
            DecisionOutcome::Deferred,
            IntentionBinding::ObservedOutcome,
            vec![],
            vec![accepted(&a, &s), accepted(&b, &s)],
        );
        let r = request(&store, &b);
        let p = policy(&store);
        let before = store.load().unwrap();
        let path = dir.path().join("tool").join(format!(
            "extension-{}.json",
            before.decisions.decisions[0].witness.as_str()
        ));
        if remove {
            std::fs::remove_file(path).unwrap();
        } else {
            std::fs::write(path, b"[]").unwrap();
        }
        let report = discover(
            &r,
            &response(&r, &a, &s),
            &p,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(report.questions.is_empty());
        assert!(!report.unverified.is_empty());
        assert!(VerifiedRetainedHistory::load(&store).is_err());
        assert_eq!(store.load().unwrap(), before);
    }
}
#[test]
fn portable_context_without_host_history_cannot_weaken_concrete_intention() {
    let dir = tempfile::tempdir().unwrap();
    let a = capture(filtered());
    let b = capture(replacement());
    let s = workflow(&a);
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::ObservedOutcome,
        vec![positive_count()],
        vec![accepted(&a, &s)],
    );
    let r = request(&store, &b);
    let report = discover(
        &r,
        &response(&r, &a, &s),
        &DiscoveryPolicy::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert!(report
        .checks
        .iter()
        .all(|c| c.state != CheckState::Satisfied));
}
#[test]
fn a_missing_nonbinary_side_and_ambiguous_correspondence_remain_unknown() {
    for ambiguous in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let a = capture(filtered());
        let b = capture(replacement());
        let s = workflow(&a);
        let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
        let mut scenes = vec![accepted(&a, &s)];
        if ambiguous {
            scenes.push(accepted(&b, &s));
            // A distinct accepted input with the same effective operations but
            // different observation instrumentation cannot be guessed as the
            // correspondence for a third observation trace.
            let mut other = s.clone();
            other.inputs.insert(
                0,
                SemanticInput::Observe {
                    point: "initial".into(),
                },
            );
            scenes.push(accepted(&a, &other));
            scenes.push(accepted(&b, &other));
        }
        save(
            &store,
            DecisionOutcome::Deferred,
            IntentionBinding::ObservedOutcome,
            vec![],
            scenes,
        );
        let mut proposed = s.clone();
        if ambiguous {
            proposed.inputs.insert(
                1,
                SemanticInput::Observe {
                    point: "after-add".into(),
                },
            );
        }
        let report = discover_with(&store, &b, &a, &proposed);
        assert!(report.questions.is_empty());
        assert!(!report.unverified.is_empty());
    }
}
#[test]
fn unrelated_operations_and_consumer_labels_do_not_broaden_saved_collection_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = filtered();
    for id in ["delete", "move"] {
        source["actions"].as_array_mut().unwrap().push(json!({"id":id,"label":id,"parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"search","value":text("")}],"ensures":[]}));
    }
    let a = capture(source.clone());
    let s = workflow(&a);
    let mut changed = source.clone();
    changed["actions"][1]["steps"] = replacement()["actions"][1]["steps"].clone();
    let b = capture(changed);
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::Deferred,
        IntentionBinding::ObservedOutcome,
        vec![],
        vec![accepted(&a, &s), accepted(&b, &s)],
    );
    let r = request(&store, &b);
    let mut out = response(&r, &a, &s);
    // The provider labels the consumer, but actual producer operations must
    // still select the retained history and suppress the unchanged question.
    out.response.hypotheses[0].action = "export_people".into();
    let report = discover(&r, &out, &policy(&store), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    assert!(report.unverified.is_empty(), "{:?}", report.unverified);
    assert_eq!(
        store.load().unwrap().decisions.decisions[0]
            .scope
            .operations,
        ["collect".into()].into()
    );
    assert!(!report
        .checks
        .iter()
        .any(|c| c.explanation.contains("delete") || c.explanation.contains("move")));
}

#[test]
fn removed_observable_is_unknown_and_exact_host_rename_mapping_is_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let a = capture(filtered());
    let s = workflow(&a);
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    save(
        &store,
        DecisionOutcome::KeepCurrent,
        IntentionBinding::ObservedOutcome,
        vec![positive_count()],
        vec![accepted(&a, &s)],
    );
    let mut renamed = filtered();
    renamed["observables"][0]["id"] = json!("picked_count");
    let b = capture(renamed);
    let r = request(&store, &b);
    let mut out = response(&r, &a, &s);
    out.response.hypotheses.clear();
    let absent = discover(&r, &out, &policy(&store), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(absent.questions.is_empty());
    assert!(!absent.unverified.is_empty());
    let mut mapped = policy(&store);
    mapped
        .retained_history
        .as_mut()
        .unwrap()
        .map_target(
            &b,
            vec![SemanticMapping {
                from: SemanticKey {
                    kind: SemanticKind::Observable,
                    entity: None,
                    id: "selected_count".into(),
                },
                to: SemanticKey {
                    kind: SemanticKind::Observable,
                    entity: None,
                    id: "picked_count".into(),
                },
            }],
        )
        .unwrap();
    let checked = discover(&r, &out, &mapped, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(checked.questions.is_empty());
    assert!(checked.defects.is_empty());
    assert!(checked
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied && c.binding.artifact == b.artifact));
    let mut removed = filtered();
    removed["observables"] = json!([]);
    let removed = capture(removed);
    let r = request(&store, &removed);
    let mut out = response(&r, &a, &s);
    out.response.hypotheses.clear();
    let unknown = discover(&r, &out, &policy(&store), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(unknown.questions.is_empty());
    assert!(!unknown.unverified.is_empty());
}
