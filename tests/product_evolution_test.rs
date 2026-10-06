#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
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
use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::ProductStore;
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
}
impl DevelopmentProvider for FixtureProvider {
    fn develop(
        &self,
        request: &DevelopmentRequest,
        _: &dyn Fn() -> bool,
    ) -> Result<DevelopmentResult, AdapterError> {
        assert_eq!(request.operation, DevelopmentOperation::Reconcile);
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
fn provider_produced_new_design_replays_both_needs_and_retires_only_named_default() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
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
    let request = engine
        .reconciliation_request(
            &before,
            "synthesize",
            "Support both accepted ways of working",
            &["old-default".into(), "second-need".into()],
        )
        .unwrap();
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
    };
    let draft = engine
        .develop_evolution(&provider, &request, "split-design", &|| false)
        .unwrap();
    assert_eq!(store.load().unwrap(), before); // Keep, reject, and defer are all non-committing.
    assert!(matches!(
        draft.candidate().binding.producer,
        Producer::Fixture { .. }
    ));
    let ready = engine
        .prepare_evolution(&store, &draft, "adopt-design")
        .unwrap();
    assert_eq!(store.load().unwrap(), before);
    assert!(ready.report().runs.len() >= 3);
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
    // The same external response cannot retire an unrelated independent promise.
    let mut bad = response;
    bad.evolutions[0]
        .proposed_retirement
        .push("independent".into());
    let provider = FixtureProvider { response: bad };
    assert!(engine
        .develop_evolution(&provider, &request, "split-design", &|| false)
        .is_err());
}
#[test]
fn evolution_cannot_weaken_scenes_or_claim_checks_passed() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for id in ["first", "second"] {
        let prepared = engine
            .prepare_choice(
                &store,
                &p,
                choice(id, DecisionOutcome::BothNeeded),
                vec![accepted(&p, id)],
                &format!("save-{id}"),
            )
            .unwrap();
        engine.adopt(&store, &prepared).unwrap();
    }
    let before = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &before,
            "synthesize",
            "Keep both examples",
            &["first".into(), "second".into()],
        )
        .unwrap();
    let mut s = request.examples[0].scenario.clone();
    s.seed.records.clear();
    s.clock_day += 1;
    let mut fake_design = organizer();
    fake_design["observables"].as_array_mut().unwrap().push(json!({"id":"new_design_detail","label":"New design detail","value":text("Generated fixture extension")}));
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "fake".into(),
            source_json: serde_json::to_string(&fake_design).unwrap(),
        }],
        hypotheses: vec![],
        evolutions: vec![EvolutionSuggestion {
            id: "proposal".into(),
            candidate: "fake".into(),
            needs: vec!["first".into(), "second".into()],
            proposed_retirement: vec![],
            preserved_obligations: vec![],
            mappings: vec![],
            scenarios: vec![SuggestedScenarioMapping {
                source_program: None,
                original: request.examples[0].scenario.identity().unwrap(),
                replacement_json: serde_json::to_string(&s).unwrap(),
                explanation: "Pretend the old requirements no longer matter".into(),
            }],
        }],
        unsupported: vec![],
    };
    let provider = FixtureProvider { response };
    assert!(engine
        .develop_evolution(&provider, &request, "proposal", &|| false)
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn recovery_reuses_current_records_fields_events_outputs_and_refuses_incompatibility() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut c = choice("independent", DecisionOutcome::KeepCurrent);
    c.obligations = vec![invariant()];
    c.binding = IntentionBinding::PropertiesOnly;
    let prepared = engine
        .prepare_choice(&store, &p, c, vec![accepted(&p, "export")], "save")
        .unwrap();
    let saved = engine.adopt(&store, &prepared).unwrap();
    let first = store
        .apply(
            saved.revision,
            "first",
            &add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut source = organizer();
    source["label"] = json!("Mistaken newer behavior");
    source["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    source["actions"][0]["steps"][0]["values"]["note"] = json!({"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"text","value":"Later fact"}});
    source["actions"].as_array_mut().unwrap().push(json!({"id":"complete","label":"Complete work","parameters":{"person":{"kind":"reference","entity":"person"}},"guards":[],"steps":[{"kind":"update","record":var("person"),"values":{"note":{"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"text","value":"Completed historical work"}}}}],"ensures":[]}));
    let newer = capture(source);
    let switch = engine
        .prepare_change(&store, &newer, &[], "switch")
        .unwrap();
    let switched = engine.adopt(&store, &switch).unwrap();
    assert_eq!(switched.data.records, first.data.records);
    let later = store
        .apply(
            switched.revision,
            "later",
            &add("Zoe"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let completed = store
        .apply(
            later.revision,
            "complete",
            &invoke(
                "complete",
                args(&[("person", reference("person", &first.data.records[0].id))]),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let collected = store
        .apply(
            completed.revision,
            "collect",
            &invoke("collect", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let exported = store
        .apply(
            collected.revision,
            "export",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let recovery = engine.prepare_recovery(&store, &p, "restore").unwrap();
    let restored = engine.adopt(&store, &recovery).unwrap();
    assert_eq!(restored.data.records, exported.data.records);
    assert_eq!(restored.data.events, exported.data.events);
    assert_eq!(restored.artifacts, exported.artifacts);
    assert_eq!(
        restored.data.records[0].values["note"],
        string("Completed historical work")
    );
    assert!(restored
        .data
        .events
        .iter()
        .any(|e| e.action == "complete" && e.program == newer.artifact.program_digest));
    assert_eq!(
        restored.data.records[1].values["note"],
        string("Later fact")
    );
    assert_eq!(
        ProductStore::open(dir.path().join("tool"))
            .unwrap()
            .load()
            .unwrap(),
        restored
    );
    let continued = store
        .apply(
            restored.revision,
            "continued",
            &add("Ian"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(continued.data.records.len(), 3);
    let runtime = LocalRuntime::default();
    let run = runtime
        .resume(
            continued.program().unwrap(),
            &continued.data,
            &continued.session,
            continued.clock_day,
            0,
            RuntimeLimits::default(),
            &continued.artifacts,
        )
        .unwrap();
    assert_eq!(
        runtime.view_model(&run).unwrap().retained_records,
        continued.data.records
    );
    let mut incompatible = organizer();
    incompatible["entities"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"required","label":"Required","value_type":{"kind":"text"}}));
    incompatible["actions"][0]["steps"][0]["values"]["required"] = text("new");
    assert!(engine
        .prepare_recovery(&store, &capture(incompatible), "unsafe")
        .is_err());
    assert_eq!(store.load().unwrap(), continued);
    let other = ProductStore::create(dir.path().join("other"), &p, 20000).unwrap();
    let other_engine =
        DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(other.clone()));
    let mut required = organizer();
    required["entities"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"required","label":"Required","value_type":{"kind":"text"}}));
    required["actions"][0]["steps"][0]["values"]["required"] = text("Later required fact");
    let required = capture(required);
    let ready = other_engine
        .prepare_change(&other, &required, &[], "extend")
        .unwrap();
    let adopted = other_engine.adopt(&other, &ready).unwrap();
    let later = other
        .apply(
            adopted.revision,
            "new-required-record",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(matches!(
        other_engine.prepare_recovery(&other, &p, "incompatible-old"),
        Err(DecisionError::Store(
            product_store::StoreError::Incompatible(_)
        ))
    ));
    assert_eq!(other.load().unwrap(), later);
}

#[test]
fn explicitly_withdrawing_a_bad_choice_keeps_independent_promises_and_later_work() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut c = choice("invariant", DecisionOutcome::KeepCurrent);
    c.obligations = vec![invariant()];
    c.binding = IntentionBinding::PropertiesOnly;
    let ready = engine
        .prepare_choice(&store, &p, c, vec![accepted(&p, "good")], "save-invariant")
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    // Emptying selection before output is a different accepted behavior, while
    // preview/output equality remains an independent invariant.
    let mut bad = organizer();
    bad["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    let bad = capture(bad);
    let ready = engine
        .prepare_choice(
            &store,
            &bad,
            {
                let mut c = choice(
                    "mistake",
                    DecisionOutcome::Accept {
                        artifact: bad.artifact.program_digest.clone(),
                    },
                );
                c.obligations = vec![AcceptedProperty {
                    id: "obsolete-empty-count".into(),
                    description: "The mistaken accepted export had no rows".into(),
                    predicate: PropertyPredicate::Equal {
                        left: PropertyTerm::OutputCount {
                            point: "done".into(),
                            output: "roster".into(),
                        },
                        right: PropertyTerm::Literal {
                            value_type: Type::Integer,
                            value: DataValue::Integer { value: 0 },
                        },
                    },
                }];
                c
            },
            vec![accepted(&bad, "mistake-scene")],
            "adopt-mistake",
        )
        .unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    let later = store
        .apply(
            adopted.revision,
            "later",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(engine
        .prepare_recovery(&store, &p, "unapproved-withdrawal")
        .is_err());
    let ready = engine
        .prepare_withdrawal(&store, &p, &["mistake".into()], "restore")
        .unwrap();
    let restored = engine.adopt(&store, &ready).unwrap();
    assert_eq!(restored.data.records, later.data.records);
    assert_eq!(restored.data.events, later.data.events);
    assert_eq!(
        restored
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "invariant")
            .unwrap()
            .status,
        DecisionStatus::Active
    );
    assert!(matches!(
        restored
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "mistake")
            .unwrap()
            .status,
        DecisionStatus::Withdrawn { .. }
    ));
    assert_eq!(
        engine.check_current(&restored).unwrap().disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn one_both_needed_choice_retains_two_same_input_outcomes_for_synthesis() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let a = accepted(&p, "same-scene");
    let mut alternate = organizer();
    alternate["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    let b = accepted(&capture(alternate), "same-scene");
    assert_eq!(
        a.scenario().identity().unwrap(),
        b.scenario().identity().unwrap()
    );
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("both", DecisionOutcome::BothNeeded),
            vec![a.clone(), b.clone()],
            "save-both",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let request = engine
        .reconciliation_request(
            &saved,
            "synthesize",
            "Support both actual outcomes",
            &["both".into()],
        )
        .unwrap();
    assert_eq!(request.decisions.decisions[0].scenarios.len(), 2);
    assert_eq!(request.sources.len(), 2);
    assert_eq!(store.load().unwrap(), saved);
    assert_eq!(request.accepted_scenes.len(), 2);
    assert_eq!(request.examples.len(), 1);
    assert_eq!(request.sources.last(), Some(&p));
    assert_ne!(
        request.accepted_scenes[0].observations,
        request.accepted_scenes[1].observations
    );
    let mut design = organizer();
    design["entities"].as_array_mut().unwrap().push(json!({"id":"dispatch","label":"Dispatch log","fields":[{"id":"purpose","label":"Purpose","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    let mut empty_action = serde_json::to_value(
        &b.program()
            .program
            .actions
            .iter()
            .find(|a| a.id == "export_people")
            .unwrap(),
    )
    .unwrap();
    empty_action["id"] = json!("export_empty");
    empty_action["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"create","entity":"dispatch","values":{"purpose":text("Explicit empty dispatch")},"bind":"logged"}));
    design["actions"].as_array_mut().unwrap().push(empty_action);
    design["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"empty_button","label":"Empty dispatch","placement":"toolbar","action":"export_empty","arguments":{},"enabled":yes()}));
    let mut mapped = b.scenario().clone();
    mapped.inputs[2] = invoke("export_empty", Values::new());
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "split".into(),
            source_json: serde_json::to_string(&design).unwrap(),
        }],
        hypotheses: vec![],
        evolutions: vec![EvolutionSuggestion {
            id: "split-design".into(),
            candidate: "split".into(),
            needs: vec!["both".into()],
            proposed_retirement: vec!["both".into()],
            preserved_obligations: vec![],
            mappings: vec![],
            scenarios: vec![SuggestedScenarioMapping {
                original: b.scenario().identity().unwrap(),
                source_program: Some(b.program().artifact.program_digest.clone()),
                replacement_json: serde_json::to_string(&mapped).unwrap(),
                explanation: "The separate dispatch preserves this exact accepted side".into(),
            }],
        }],
        unsupported: vec![],
    };
    for source in [
        None,
        Some(p.artifact.program_digest.clone()),
        Some(canonical_digest(IdentityDomain::Program, &"unknown source").unwrap()),
    ] {
        let mut wrong = response.clone();
        wrong.evolutions[0].scenarios[0].source_program = source;
        assert!(engine
            .develop_evolution(
                &FixtureProvider { response: wrong },
                &request,
                "split-design",
                &|| false
            )
            .is_err());
        assert_eq!(store.load().unwrap(), saved);
    }
    let mut duplicate = response.clone();
    let repeated = duplicate.evolutions[0].scenarios[0].clone();
    duplicate.evolutions[0].scenarios.push(repeated);
    assert!(engine
        .develop_evolution(
            &FixtureProvider {
                response: duplicate
            },
            &request,
            "split-design",
            &|| false
        )
        .is_err());
    let draft = engine
        .develop_evolution(
            &FixtureProvider { response },
            &request,
            "split-design",
            &|| false,
        )
        .unwrap();
    let ready = engine
        .prepare_evolution(&store, &draft, "adopt-split")
        .unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    assert!(matches!(
        adopted
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == "both")
            .unwrap()
            .status,
        DecisionStatus::Superseded { .. }
    ));
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    let next = engine
        .development_request(
            &adopted,
            "next",
            DevelopmentOperation::Modify,
            "Improve the layout",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(adopted.data.identity().unwrap()),
                session_digest: None,
            },
        )
        .unwrap();
    assert!(!next.unknowns.iter().any(|u| u.id == "pending-both"));
}

#[test]
fn explicit_replacement_can_supply_new_arguments_without_weakening_the_scene() {
    for retire_originals in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let p = capture(organizer());
        let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let mut independent = choice("unrelated", DecisionOutcome::KeepCurrent);
        independent.obligations = vec![invariant()];
        independent.binding = IntentionBinding::PropertiesOnly;
        let ready = engine
            .prepare_choice(
                &store,
                &p,
                independent,
                vec![accepted(&p, "unrelated")],
                "save-unrelated",
            )
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
        for id in ["first", "second"] {
            let ready = engine
                .prepare_choice(
                    &store,
                    &p,
                    choice(
                        id,
                        DecisionOutcome::Accept {
                            artifact: p.artifact.program_digest.clone(),
                        },
                    ),
                    vec![accepted(&p, id)],
                    &format!("save-{id}"),
                )
                .unwrap();
            engine.adopt(&store, &ready).unwrap();
        }
        let current = store.load().unwrap();
        let request = engine
            .reconciliation_request(
                &current,
                "synthesize",
                "Make the new operation explicit",
                &["first".into(), "second".into()],
            )
            .unwrap();
        let mut candidate = organizer();
        candidate["actions"][2]["parameters"] = json!({"mode":{"kind":"text"}});
        candidate["views"][0]["actions"][0]["arguments"] = json!({"mode":text("normal")});
        let mappings = request
            .examples
            .iter()
            .map(|example| {
                let mut replacement = example.scenario.clone();
                replacement.inputs[2] =
                    invoke("export_people", args(&[("mode", string("normal"))]));
                SuggestedScenarioMapping {
                    source_program: None,
                    original: example.scenario.identity().unwrap(),
                    replacement_json: serde_json::to_string(&replacement).unwrap(),
                    explanation: "The replacement command now has an explicit mode".into(),
                }
            })
            .collect();
        let provider = FixtureProvider {
            response: DevelopmentResponse {
                version: 1,
                request_digest: request.identity().unwrap(),
                candidates: vec![GeneratedCandidate {
                    id: "new".into(),
                    source_json: serde_json::to_string(&candidate).unwrap(),
                }],
                hypotheses: vec![],
                evolutions: vec![EvolutionSuggestion {
                    id: "with-mode".into(),
                    candidate: "new".into(),
                    needs: vec!["first".into(), "second".into()],
                    proposed_retirement: if retire_originals {
                        vec!["first".into(), "second".into()]
                    } else {
                        vec![]
                    },
                    preserved_obligations: vec![invariant().identity().unwrap()],
                    mappings: vec![],
                    scenarios: mappings,
                }],
                unsupported: vec![],
            },
        };
        let draft = engine
            .develop_evolution(&provider, &request, "with-mode", &|| false)
            .unwrap();
        let ready = engine
            .prepare_evolution(&store, &draft, "adopt-mode")
            .unwrap();
        let adopted = engine.adopt(&store, &ready).unwrap();
        assert_eq!(
            engine.check_current(&adopted).unwrap().disposition,
            CheckDisposition::Ready
        );
        for original in ["first", "second"] {
            assert!(
                adopted
                    .decisions
                    .decisions
                    .iter()
                    .find(|d| d.id == original)
                    .unwrap()
                    .status
                    == if retire_originals {
                        DecisionStatus::Superseded {
                            by: if original == "first" {
                                "with-mode".into()
                            } else {
                                "with-mode-second".into()
                            },
                        }
                    } else {
                        DecisionStatus::Active
                    }
            );
        }
        let fresh = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let reopened = ProductStore::open(dir.path().join("tool"))
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(
            fresh.check_current(&reopened).unwrap().disposition,
            CheckDisposition::Ready
        );
        let archive = IntentArchive::new(store.clone());
        validate_bundle(&reopened, &archive.export_for(&reopened).unwrap()).unwrap();
        candidate["label"] = json!("A later compatible display name");
        let later = capture(candidate.clone());
        let ready = fresh
            .prepare_change(&store, &later, &[], "later-label")
            .unwrap();
        let saved = fresh.adopt(&store, &ready).unwrap();
        assert_eq!(
            fresh.check_current(&saved).unwrap().disposition,
            CheckDisposition::Ready
        );
        let needs: Vec<_> = saved
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.id.clone())
            .collect();
        let second_request = fresh
            .reconciliation_request(
                &saved,
                "second-evolution",
                "Add the next explicit option",
                &needs,
            )
            .unwrap();
        candidate["actions"][2]["parameters"]["locale"] = json!({"kind":"text"});
        candidate["views"][0]["actions"][0]["arguments"]["locale"] = text("en");
        let replacements = second_request
            .examples
            .iter()
            .filter(|example| {
                second_request.decisions.decisions.iter().any(|d| {
                    needs.contains(&d.id)
                        && d.scenarios.contains(&example.scenario.identity().unwrap())
                })
            })
            .map(|example| {
                let mut replacement = example.scenario.clone();
                replacement.inputs[2] = invoke(
                    "export_people",
                    args(&[("mode", string("normal")), ("locale", string("en"))]),
                );
                SuggestedScenarioMapping {
                    source_program: None,
                    original: example.scenario.identity().unwrap(),
                    replacement_json: serde_json::to_string(&replacement).unwrap(),
                    explanation: "Keep the same outcome with both explicit parameters".into(),
                }
            })
            .collect();
        let second_provider = FixtureProvider {
            response: DevelopmentResponse {
                version: 1,
                request_digest: second_request.identity().unwrap(),
                candidates: vec![GeneratedCandidate {
                    id: "next".into(),
                    source_json: serde_json::to_string(&candidate).unwrap(),
                }],
                hypotheses: vec![],
                evolutions: vec![EvolutionSuggestion {
                    id: "second-design".into(),
                    candidate: "next".into(),
                    needs,
                    proposed_retirement: vec![],
                    preserved_obligations: vec![invariant().identity().unwrap()],
                    mappings: vec![],
                    scenarios: replacements,
                }],
                unsupported: vec![],
            },
        };
        let draft = fresh
            .develop_evolution(&second_provider, &second_request, "second-design", &|| {
                false
            })
            .unwrap();
        let ready = fresh
            .prepare_evolution(&store, &draft, "adopt-second")
            .unwrap();
        let second = fresh.adopt(&store, &ready).unwrap();
        assert_eq!(
            fresh.check_current(&second).unwrap().disposition,
            CheckDisposition::Ready
        );
        validate_bundle(&second, &archive.export_for(&second).unwrap()).unwrap();
    }
}
#[test]
fn inserted_inputs_cannot_retarget_an_export_intention_to_collection() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for id in ["first", "second"] {
        let ready = engine
            .prepare_choice(
                &store,
                &p,
                choice(id, DecisionOutcome::BothNeeded),
                vec![accepted(&p, id)],
                &format!("save-{id}"),
            )
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
    }
    let current = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &current,
            "synthesize",
            "Keep export as the business operation",
            &["first".into(), "second".into()],
        )
        .unwrap();
    let mut candidate = organizer();
    candidate["observables"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"design_note","label":"Design note","value":text("New structure")}));
    let mappings = request
        .examples
        .iter()
        .map(|example| {
            let mut replacement = example.scenario.clone();
            replacement
                .inputs
                .insert(2, invoke("collect", Values::new()));
            SuggestedScenarioMapping {
                source_program: None,
                original: example.scenario.identity().unwrap(),
                replacement_json: serde_json::to_string(&replacement).unwrap(),
                explanation: "The new flow has an extra idempotent collection step".into(),
            }
        })
        .collect();
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&candidate).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "design".into(),
                candidate: "new".into(),
                needs: vec!["first".into(), "second".into()],
                proposed_retirement: vec![],
                preserved_obligations: vec![],
                mappings: vec![],
                scenarios: mappings,
            }],
            unsupported: vec![],
        },
    };
    let draft = engine
        .develop_evolution(&provider, &request, "design", &|| false)
        .unwrap();
    let ready = engine
        .prepare_evolution(&store, &draft, "adopt-design")
        .unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    for d in adopted
        .decisions
        .decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Active)
    {
        assert_eq!(d.scope.operations, ["export_people".into()].into());
    }
}

#[test]
fn separate_scenes_collectively_cover_a_multi_action_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = organizer();
    let mut alternate = source["actions"][2].clone();
    alternate["id"] = json!("alternate_export");
    source["actions"].as_array_mut().unwrap().push(alternate);
    let p = capture(source.clone());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut both = choice("both", DecisionOutcome::BothNeeded);
    both.scope.operations.insert("alternate_export".into());
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            both,
            vec![
                accepted(&p, "first"),
                accepted_for(&p, "second", "alternate_export"),
                accept_scene(
                    &LocalRuntime::default(),
                    &p,
                    &scenario(
                        &p,
                        vec![
                            add("Boundary"),
                            SemanticInput::Observe {
                                point: "done".into(),
                            },
                        ],
                    ),
                    Disclosure::Synthetic,
                    RuntimeLimits::default(),
                )
                .unwrap(),
            ],
            "save",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let request = engine
        .reconciliation_request(
            &saved,
            "synthesize",
            "Support both operations",
            &["both".into()],
        )
        .unwrap();
    source["observables"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"design_note","label":"Design note","value":text("New structure")}));
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&source).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "design".into(),
                candidate: "new".into(),
                needs: vec!["both".into()],
                proposed_retirement: vec![],
                preserved_obligations: vec![],
                mappings: vec![],
                scenarios: vec![],
            }],
            unsupported: vec![],
        },
    };
    let draft = engine
        .develop_evolution(&provider, &request, "design", &|| false)
        .unwrap();
    let ready = engine.prepare_evolution(&store, &draft, "adopt").unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    let active = adopted
        .decisions
        .decisions
        .iter()
        .find(|d| d.status == DecisionStatus::Active)
        .unwrap();
    assert_eq!(
        active.scope.operations,
        ["export_people".into(), "alternate_export".into()].into()
    );
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn recovery_preserves_independent_mappings_for_the_retained_target() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice(
                "independent",
                DecisionOutcome::Accept {
                    artifact: p.artifact.program_digest.clone(),
                },
            ),
            vec![accepted(&p, "original")],
            "accept",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let mut renamed = organizer();
    renamed["actions"][2]["id"] = json!("download");
    renamed["views"][0]["actions"][0]["action"] = json!("download");
    let b = capture(renamed.clone());
    let mapping = SemanticMapping {
        from: SemanticKey {
            kind: SemanticKind::Action,
            id: "export_people".into(),
            entity: None,
        },
        to: SemanticKey {
            kind: SemanticKind::Action,
            id: "download".into(),
            entity: None,
        },
    };
    let ready = engine
        .prepare_change(&store, &b, &[mapping], "rename")
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    renamed["label"] = json!("Later label");
    let c = capture(renamed);
    let ready = engine.prepare_change(&store, &c, &[], "later").unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let ready = engine
        .prepare_recovery(&store, &b, "restore-label")
        .unwrap();
    let restored = engine.adopt(&store, &ready).unwrap();
    assert_eq!(saved.data, restored.data);
    assert_eq!(
        engine.check_current(&restored).unwrap().disposition,
        CheckDisposition::Ready
    );
    let reopened = ProductStore::open(dir.path().join("tool"))
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(
        engine.check_current(&reopened).unwrap().disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn withdrawing_a_new_action_restores_a_program_without_that_action() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut extra = organizer();
    let mut action = extra["actions"][2].clone();
    action["id"] = json!("new_action");
    extra["actions"].as_array_mut().unwrap().push(action);
    let extra = capture(extra);
    let mut chosen = choice(
        "mistake",
        DecisionOutcome::Accept {
            artifact: extra.artifact.program_digest.clone(),
        },
    );
    chosen.scope.operations = ["new_action".into()].into();
    let ready = engine
        .prepare_choice(
            &store,
            &extra,
            chosen,
            vec![accepted_for(&extra, "new-scene", "new_action")],
            "add-feature",
        )
        .unwrap();
    let added = engine.adopt(&store, &ready).unwrap();
    let later = store
        .apply(
            added.revision,
            "later-record",
            &add("Keep me"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let ready = engine
        .prepare_withdrawal(&store, &p, &["mistake".into()], "undo-feature")
        .unwrap();
    let restored = engine.adopt(&store, &ready).unwrap();
    assert_eq!(restored.data, later.data);
    assert_eq!(restored.decisions.decisions.len(), 1);
    assert_eq!(
        restored.decisions.decisions[0].status,
        DecisionStatus::Withdrawn {
            adoption: "undo-feature".into()
        }
    );
    let archive = IntentArchive::new(store.clone());
    validate_bundle(&restored, &archive.export_for(&restored).unwrap()).unwrap();
    let mut forged = restored.clone();
    forged.decisions.decisions[0].status = DecisionStatus::Withdrawn {
        adoption: "add-feature".into(),
    };
    assert!(engine.check_current(&forged).is_err());
    assert!(archive.export_for(&forged).is_err());

    assert_eq!(
        restored.decisions.decisions[0].witness,
        later.decisions.decisions[0].witness
    );
    let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
    assert_eq!(
        engine
            .check_current(&reopened.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    reopened
        .apply(
            restored.revision,
            "still-usable",
            &add("After restoration"),
            RuntimeLimits::default(),
        )
        .unwrap();
}

#[test]
fn an_explicit_replacement_cannot_omit_an_unrelated_scoped_operation() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let property = AcceptedProperty {
        id: "selected-one".into(),
        description: "One item remains selected in this workflow".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 1 },
            },
        },
    };
    for id in ["first", "second", "unrelated"] {
        let mut c = choice(id, DecisionOutcome::KeepCurrent);
        if id == "unrelated" {
            c.obligations = vec![property.clone()];
            c.binding = IntentionBinding::PropertiesOnly;
        }
        let ready = engine
            .prepare_choice(&store, &p, c, vec![accepted(&p, id)], &format!("save-{id}"))
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
    }
    let saved = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &saved,
            "synthesize",
            "Make a required parameter explicit",
            &["first".into(), "second".into()],
        )
        .unwrap();
    let mut design = organizer();
    design["actions"][2]["parameters"] = json!({"mode":{"kind":"text"}});
    design["views"][0]["actions"][0]["arguments"] = json!({"mode":text("normal")});
    let scenarios = request
        .examples
        .iter()
        .map(|e| {
            let mut mapped = e.scenario.clone();
            if mapped.id == "unrelated" {
                mapped.inputs.remove(2);
            } else {
                mapped.inputs[2] = invoke("export_people", args(&[("mode", string("normal"))]));
            }
            SuggestedScenarioMapping {
                original: e.scenario.identity().unwrap(),
                source_program: Some(p.artifact.program_digest.clone()),
                replacement_json: serde_json::to_string(&mapped).unwrap(),
                explanation: "Rehearse the same scoped operation".into(),
            }
        })
        .collect();
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&design).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "design".into(),
                candidate: "new".into(),
                needs: vec!["first".into(), "second".into()],
                proposed_retirement: vec![],
                preserved_obligations: vec![property.identity().unwrap()],
                mappings: vec![],
                scenarios,
            }],
            unsupported: vec![],
        },
    };
    assert!(engine
        .develop_evolution(&provider, &request, "design", &|| false)
        .is_err());
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn explicitly_property_only_needs_remain_property_only_after_evolution() {
    let dir = tempfile::tempdir().unwrap();
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for id in ["first", "second"] {
        let mut c = choice(id, DecisionOutcome::KeepCurrent);
        c.obligations = vec![invariant()];
        c.binding = IntentionBinding::PropertiesOnly;
        let ready = engine
            .prepare_choice(&store, &p, c, vec![accepted(&p, id)], &format!("save-{id}"))
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
    }
    let saved = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &saved,
            "synthesize",
            "Preserve the two stated invariants",
            &["first".into(), "second".into()],
        )
        .unwrap();
    let mut design = organizer();
    design["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&design).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "property-design".into(),
                candidate: "new".into(),
                needs: vec!["first".into(), "second".into()],
                proposed_retirement: vec!["first".into(), "second".into()],
                preserved_obligations: vec![invariant().identity().unwrap()],
                mappings: vec![],
                scenarios: vec![],
            }],
            unsupported: vec![],
        },
    };
    let draft = engine
        .develop_evolution(&provider, &request, "property-design", &|| false)
        .unwrap();
    let ready = engine.prepare_evolution(&store, &draft, "adopt").unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    for decision in adopted
        .decisions
        .decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Active)
    {
        assert_eq!(
            engine.intention_binding(decision).unwrap(),
            IntentionBinding::PropertiesOnly
        );
    }
    let archive = IntentArchive::new(store.clone());
    let bundle = archive.export_for(&adopted).unwrap();
    let recovered = ProductStore::create_recovered_with(
        dir.path().join("recovered-properties"),
        &adopted,
        |target| {
            IntentArchive::new(target.clone())
                .restore_for(&adopted, &bundle)
                .map_err(|e| product_store::StoreError::Invalid(e.to_string()))
        },
    )
    .unwrap();
    let recovered_engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(recovered.clone()),
    );
    let envelope = recovered_engine
        .development_request(
            &recovered.load().unwrap(),
            "fresh-properties",
            DevelopmentOperation::Modify,
            "Preserve the independent promises",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(adopted.data.identity().unwrap()),
                session_digest: None,
            },
        )
        .unwrap();
    assert!(envelope.request.contains("properties_only"));
    assert_eq!(envelope.accepted_scenes.len(), 2);
    let ready = recovered_engine
        .prepare_change(&recovered, &p, &[], "another-preserving-implementation")
        .unwrap();
    let restored = recovered_engine.adopt(&recovered, &ready).unwrap();
    assert_eq!(
        recovered_engine
            .check_current(&restored)
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn mixed_scoped_scenes_evolve_with_collective_applicable_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = organizer();
    let mut second_action = source["actions"][0].clone();
    second_action["id"] = json!("add_guest");
    source["actions"]
        .as_array_mut()
        .unwrap()
        .push(second_action);
    let p = capture(source.clone());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenes = [("north", "south"), ("south", "north")]
        .into_iter()
        .map(|(first, second)| {
            accept_scene(
                &LocalRuntime::default(),
                &p,
                &scenario(
                    &p,
                    vec![
                        invoke(
                            "add_person",
                            args(&[("name", string("Person")), ("area", string(first))]),
                        ),
                        invoke(
                            "add_guest",
                            args(&[("name", string("Guest")), ("area", string(second))]),
                        ),
                        SemanticInput::Observe {
                            point: "done".into(),
                        },
                    ],
                ),
                Disclosure::Synthetic,
                RuntimeLimits::default(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut need = choice("both", DecisionOutcome::BothNeeded);
    need.scope.operations = ["add_person".into(), "add_guest".into()].into();
    need.scope.conditions = args(&[("area", string("north"))]);
    let ready = engine
        .prepare_choice(&store, &p, need, scenes, "remember")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let request = engine
        .reconciliation_request(
            &saved,
            "reconcile",
            "Keep both north workflows",
            &["both".into()],
        )
        .unwrap();
    source["observables"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"design_note","label":"Design note","value":text("New structure")}));
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&source).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "design".into(),
                candidate: "new".into(),
                needs: vec!["both".into()],
                proposed_retirement: vec!["both".into()],
                preserved_obligations: vec![],
                mappings: vec![],
                scenarios: vec![],
            }],
            unsupported: vec![],
        },
    };
    let draft = engine
        .develop_evolution(&provider, &request, "design", &|| false)
        .unwrap();
    let ready = engine.prepare_evolution(&store, &draft, "adopt").unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let successor = saved
        .decisions
        .decisions
        .iter()
        .find(|d| d.status == DecisionStatus::Active)
        .unwrap();
    assert_eq!(
        successor.scope.operations,
        ["add_person".into(), "add_guest".into()].into()
    );
    assert_eq!(
        successor.scope.conditions,
        args(&[("area", string("north"))])
    );
    assert_eq!(
        engine.check_current(&saved).unwrap().disposition,
        CheckDisposition::Ready
    );
    validate_bundle(
        &saved,
        &IntentArchive::new(store).export_for(&saved).unwrap(),
    )
    .unwrap();
}

#[test]
fn withdrawn_replay_recipes_do_not_block_removing_obsolete_session_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = organizer();
    source["state"].as_array_mut().unwrap().push(json!({"id":"obsolete_note","label":"Old note","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}}));
    let p = capture(source.clone());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for id in ["first", "second"] {
        let ready = engine
            .prepare_choice(
                &store,
                &p,
                choice(id, DecisionOutcome::KeepCurrent),
                vec![accepted(&p, id)],
                &format!("save-{id}"),
            )
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
    }
    let before = store.load().unwrap();
    let request = engine
        .reconciliation_request(
            &before,
            "reconcile",
            "Keep the accepted export work",
            &["first".into(), "second".into()],
        )
        .unwrap();
    source["actions"][2]["parameters"] = json!({"mode":{"kind":"text"}});
    source["views"][0]["actions"][0]["arguments"] = json!({"mode":text("normal")});
    let replacements = request
        .examples
        .iter()
        .map(|example| {
            let mut replacement = example.scenario.clone();
            replacement.inputs[2] = invoke("export_people", args(&[("mode", string("normal"))]));
            SuggestedScenarioMapping {
                source_program: Some(p.artifact.program_digest.clone()),
                original: example.scenario.identity().unwrap(),
                replacement_json: serde_json::to_string(&replacement).unwrap(),
                explanation: "Use the new explicit parameter".into(),
            }
        })
        .collect();
    let provider = FixtureProvider {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "new".into(),
                source_json: serde_json::to_string(&source).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![EvolutionSuggestion {
                id: "design".into(),
                candidate: "new".into(),
                needs: vec!["first".into(), "second".into()],
                proposed_retirement: vec!["first".into(), "second".into()],
                preserved_obligations: vec![],
                mappings: vec![],
                scenarios: replacements,
            }],
            unsupported: vec![],
        },
    };
    let draft = engine
        .develop_evolution(&provider, &request, "design", &|| false)
        .unwrap();
    let ready = engine
        .prepare_evolution(&store, &draft, "adopt-design")
        .unwrap();
    let evolved = engine.adopt(&store, &ready).unwrap();
    assert!(engine
        .current_mappings(&evolved)
        .unwrap()
        .iter()
        .any(|mapping| !mapping.scenarios.is_empty()));
    let mut cleaned = source.clone();
    cleaned["state"]
        .as_array_mut()
        .unwrap()
        .retain(|state| state["id"] != "obsolete_note");
    let cleaned = capture(cleaned);
    assert!(engine
        .prepare_change(&store, &cleaned, &[], "still-protected")
        .is_err());
    assert_eq!(store.load().unwrap(), evolved);
    let exact = evolved
        .decisions
        .decisions
        .iter()
        .filter(|d| d.status == DecisionStatus::Active)
        .map(|d| d.id.clone())
        .collect::<Vec<_>>();
    let ready = engine
        .prepare_withdrawal(&store, evolved.program().unwrap(), &exact, "withdraw")
        .unwrap();
    let withdrawn = engine.adopt(&store, &ready).unwrap();
    let live = store
        .apply(
            withdrawn.revision,
            "later-record",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let ready = engine
        .prepare_change(&store, &cleaned, &[], "remove-obsolete-state")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    assert_eq!(saved.data, live.data);
    assert_eq!(saved.decisions, withdrawn.decisions);
    assert!(saved
        .adoptions
        .iter()
        .any(|adoption| adoption.plan.id == "adopt-design"));
    assert!(!saved.session.values.contains_key("obsolete_note"));
    assert!(engine
        .current_mappings(&saved)
        .unwrap()
        .iter()
        .all(|mapping| mapping.scenarios.is_empty()));
    let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
    let fresh = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(reopened.clone()),
    );
    assert_eq!(
        fresh
            .check_current(&reopened.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    validate_bundle(
        &saved,
        &IntentArchive::new(reopened).export_for(&saved).unwrap(),
    )
    .unwrap();
}
