//! Direct shared-contract/runtime regressions. No discovery/minimizer dependency.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
use fixture::*;
use product_contract::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
fn input() -> (DevelopmentRequest, DevelopmentResult) {
    let a = capture(filtered());
    let mut v = filtered();
    v["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":v["actions"][1]["steps"][0]["items"].clone()});
    let b = capture(v);
    let scene = scenario(
        &a,
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
    );
    let request = DevelopmentRequest {
        version: 1,
        id: "discover-test".into(),
        project_id: "runtime-project".into(),
        operation: DevelopmentOperation::Discover,
        request: "Make collecting the current search results work smoothly".into(),
        sources: vec![a.clone(), b.clone()],
        context: DevelopmentContext {
            view: Some("people".into()),
            selected: vec![],
            recent_inputs: vec![invoke("collect", Values::new())],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: decisions(),
        unknowns: vec![],
        required_capabilities: BTreeSet::new(),
    };
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![
            GeneratedCandidate {
                id: "retain".into(),
                source_json: String::from_utf8(a.source_bytes.clone()).unwrap(),
            },
            GeneratedCandidate {
                id: "replace".into(),
                source_json: String::from_utf8(b.source_bytes.clone()).unwrap(),
            },
        ],
        hypotheses: vec![ChoiceHypothesis {
            id: "choice".into(),
            statement: "Repeated collection may keep or replace previous results".into(),
            kind: HypothesisKind::UnresolvedChoice,
            action: "collect".into(),
            observable: "selected_count".into(),
            sources: vec![SourceLocus {
                relative_path: "program.json".into(),
                raw_digest: b.artifact.raw_digest.clone(),
                pointer: "/actions/1/steps/0".into(),
            }],
            alternatives: vec!["retain".into(), "replace".into()],
            related_decisions: vec![],
            scenario_json: serde_json::to_string(&scene).unwrap(),
            unknowns: vec![],
        }],
        evolutions: vec![],
        unsupported: vec![],
    };
    (
        request,
        DevelopmentResult {
            response,
            producer: Producer::Fixture {
                name: "recorded untrusted hypotheses".into(),
            },
        },
    )
}
fn refresh(r: &DevelopmentRequest, out: &mut DevelopmentResult) {
    out.response.request_digest = r.identity().unwrap();
}
fn saved_decision(
    r: &mut DevelopmentRequest,
    out: &DevelopmentResult,
    outcome: DecisionOutcome,
    obligations: Vec<AcceptedProperty>,
) -> DifferentialWitness {
    let scene = out.response.hypotheses[0].scenario().unwrap();
    let runtime = product_runtime::LocalRuntime::default();
    let before = runtime
        .replay(
            &r.sources[0],
            &scene,
            &r.decisions,
            RuntimeLimits::default(),
            "before-run",
        )
        .unwrap();
    let after = runtime
        .replay(
            &r.sources[1],
            &scene,
            &r.decisions,
            RuntimeLimits::default(),
            "after-run",
        )
        .unwrap();
    assert_eq!(before.state, EvidenceState::Observed);
    assert_eq!(after.state, EvidenceState::Observed);
    let property = AcceptedProperty {
        id: "actual-count".into(),
        description: "Actual first result".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: before.observations[0].values["selected_count"].clone(),
            },
        },
    };
    let observed = DifferentialWitness {
        version: 1,
        id: "accepted-witness".into(),
        scenario: scene.clone(),
        before,
        after,
        state: EvidenceState::Observed,
        distinguishing_properties: vec![property],
        minimization: None,
    };
    observed.validate().unwrap();
    r.examples.push(SelectedScenario {
        disclosure: Disclosure::Synthetic,
        scenario: scene.clone(),
    });
    r.decisions.decisions.push(ScopedDecision {
        id: "saved".into(),
        revision: 1,
        request: "Keep the approved collection behavior".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["collect".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        status: if matches!(outcome, DecisionOutcome::Deferred) {
            DecisionStatus::Pending
        } else {
            DecisionStatus::Active
        },
        outcome,
        obligations,
        scenarios: vec![scene.identity().unwrap()],
        witness: observed.identity().unwrap(),
        supersedes: vec![],
    });
    observed
}
fn accepted_scene_request() -> (DevelopmentRequest, DevelopmentResult) {
    let (mut request, mut result) = input();
    let prior = saved_decision(&mut request, &result, DecisionOutcome::Deferred, vec![]);
    request.decisions.decisions[0].outcome = DecisionOutcome::BothNeeded;
    request.operation = DevelopmentOperation::Reconcile;
    let mut wire = serde_json::to_value(&request).unwrap();
    wire["accepted_scenes"] = json!([
        {"decision":"saved","source":request.sources[0].artifact,"scenario":prior.scenario.identity().unwrap(),"observations":prior.before.observations,"disclosure":"synthetic"},
        {"decision":"saved","source":request.sources[1].artifact,"scenario":prior.scenario.identity().unwrap(),"observations":prior.after.observations,"disclosure":"synthetic"}
    ]);
    request = serde_json::from_value(wire).unwrap();
    result.response.hypotheses.clear();
    refresh(&request, &mut result);
    (request, result)
}

#[test]
fn retirement_proposals_only_name_current_listed_needs() {
    let (mut request, mut result) = input();
    saved_decision(&mut request, &result, DecisionOutcome::Deferred, vec![]);
    request.decisions.decisions[0].outcome = DecisionOutcome::BothNeeded;
    request.operation = DevelopmentOperation::Reconcile;
    result.response.hypotheses.clear();
    result.response.evolutions.push(EvolutionSuggestion {
        id: "combine".into(),
        candidate: "replace".into(),
        needs: vec!["saved".into()],
        proposed_retirement: vec!["saved".into()],
        preserved_obligations: vec![],
        mappings: vec![],
        scenarios: vec![],
    });
    refresh(&request, &mut result);
    assert!(result.validate_for(&request).is_ok());
    assert_eq!(
        request.decisions.decisions[0].status,
        DecisionStatus::Pending
    );
    let mut other = request.decisions.decisions[0].clone();
    other.id = "other".into();
    other.status = DecisionStatus::Active;
    other.outcome = DecisionOutcome::EitherAcceptable;
    request.decisions.decisions.push(other);
    result.response.evolutions[0].proposed_retirement = vec!["other".into()];
    refresh(&request, &mut result);
    assert!(result.validate_for(&request).is_err());
    result.response.evolutions[0].needs.push("other".into());
    assert!(result.validate_for(&request).is_ok());
    request.decisions.decisions[0].status = DecisionStatus::Superseded { by: "other".into() };
    request.decisions.decisions[1]
        .supersedes
        .push("saved".into());
    result.response.evolutions[0].proposed_retirement = vec!["saved".into()];
    refresh(&request, &mut result);
    assert!(result.validate_for(&request).is_err());
    result.response.evolutions[0].proposed_retirement = vec!["unknown".into()];
    assert!(result.validate_for(&request).is_err());
}

#[test]
fn accepted_scene_context_binds_both_real_sides_and_disclosure() {
    let (request, _) = accepted_scene_request();
    request.validate().unwrap();
    let value = serde_json::to_value(&request).unwrap();
    let scenes = value["accepted_scenes"].as_array().unwrap();
    assert_eq!(scenes[0]["scenario"], scenes[1]["scenario"]);
    assert_ne!(scenes[0]["source"], scenes[1]["source"]);
    assert_ne!(scenes[0]["observations"], scenes[1]["observations"]);
    for mutate in [
        "duplicate",
        "unknown_decision",
        "unknown_source",
        "unknown_scene",
        "disclosure",
        "point",
        "missing_observation",
        "too_many",
    ] {
        let mut wire = value.clone();
        match mutate {
            "duplicate" => {
                let item = wire["accepted_scenes"][0].clone();
                wire["accepted_scenes"].as_array_mut().unwrap().push(item);
            }
            "unknown_decision" => wire["accepted_scenes"][0]["decision"] = json!("missing"),
            "unknown_source" => {
                wire["accepted_scenes"][0]["source"]["program_digest"] = json!("a".repeat(64))
            }
            "unknown_scene" => wire["accepted_scenes"][0]["scenario"] = json!("b".repeat(64)),
            "disclosure" => {
                wire["accepted_scenes"][0]["disclosure"] = json!("explicitly_selected_sanitized")
            }
            "point" => wire["accepted_scenes"][0]["observations"][0]["point"] = json!("forged"),
            "missing_observation" => wire["accepted_scenes"][0]["observations"] = json!([]),
            "too_many" => {
                let item = wire["accepted_scenes"][0].clone();
                wire["accepted_scenes"] = json!(vec![item; MAX_ITEMS + 1]);
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<DevelopmentRequest>(wire)
                .map_or(true, |r| r.validate().is_err()),
            "accepted {mutate}"
        );
    }
    let mut oversized = request.clone();
    let mut observation = oversized.accepted_scenes[0].observations[0].clone();
    observation.values.insert(
        "payload".into(),
        DataValue::Text {
            value: "x".repeat(MAX_TEXT_BYTES),
        },
    );
    oversized.accepted_scenes[0].observations = vec![observation; MAX_ITEMS];
    assert!(oversized.validate().unwrap_err().0.contains("byte limit"));
    let (empty, _) = input();
    let mut old = serde_json::to_value(&empty).unwrap();
    assert!(old.get("accepted_scenes").is_none());
    let before = empty.identity().unwrap();
    old["accepted_scenes"] = json!([]);
    assert_eq!(
        serde_json::from_value::<DevelopmentRequest>(old)
            .unwrap()
            .identity()
            .unwrap(),
        before
    );
}

#[test]
fn same_scene_evolution_mappings_require_unambiguous_source_programs() {
    let (request, mut result) = accepted_scene_request();
    let original = request.examples[0].scenario.identity().unwrap();
    let mut combined: Value = serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
    let mut keep: Value = serde_json::to_value(&request.sources[0].program.actions[1]).unwrap();
    keep["id"] = json!("keep_collecting");
    combined["actions"].as_array_mut().unwrap().push(keep);
    result.response.candidates[1].source_json = serde_json::to_string(&combined).unwrap();
    let mut replacements = vec![
        request.examples[0].scenario.clone(),
        request.examples[0].scenario.clone(),
    ];
    for input in &mut replacements[0].inputs {
        if let SemanticInput::Invoke { action, .. } = input {
            if action == "collect" {
                *action = "keep_collecting".into();
            }
        }
    }
    let mapping = |index: usize| json!({"original":original,"source_program":request.sources[index].artifact.program_digest,"replacement_json":serde_json::to_string(&replacements[index]).unwrap(),"explanation":"Preserve this accepted side"});
    let mut response = serde_json::to_value(&result.response).unwrap();
    response["evolutions"] = json!([{"id":"combine","candidate":"replace","needs":["saved"],"proposed_retirement":["saved"],"preserved_obligations":[],"mappings":[],"scenarios":[mapping(0),mapping(1)]}]);
    let program = capture(combined);
    for (index, replacement) in replacements.iter().enumerate() {
        let run = product_runtime::LocalRuntime::default()
            .replay(
                &program,
                replacement,
                &request.decisions,
                RuntimeLimits::default(),
                "mapped",
            )
            .unwrap();
        assert_eq!(run.state, EvidenceState::Observed);
        assert_eq!(
            run.observations[0].values,
            request.accepted_scenes[index].observations[0].values
        );
        assert_eq!(
            run.observations[0].outputs,
            request.accepted_scenes[index].observations[0].outputs
        );
    }
    result.response = serde_json::from_value(response.clone()).unwrap();
    result.validate_for(&request).unwrap();
    let decoded = result.response.evolutions[0].scenarios[0].decode().unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["source_program"],
        response["evolutions"][0]["scenarios"][0]["source_program"]
    );
    let mut one_side = request.clone();
    one_side.accepted_scenes.remove(0);
    let mut mismatched = result.response.clone();
    mismatched.request_digest = one_side.identity().unwrap();
    assert!(mismatched.validate_for(&one_side).is_err());
    let mut legacy = mismatched.clone();
    legacy.evolutions[0].scenarios.remove(0);
    legacy.evolutions[0].scenarios[0].source_program = None;
    legacy.validate_for(&one_side).unwrap();
    assert!(serde_json::to_value(&legacy.evolutions[0].scenarios[0])
        .unwrap()
        .get("source_program")
        .is_none());
    for kind in ["ambiguous", "duplicate", "unknown"] {
        let mut wire = response.clone();
        match kind {
            "ambiguous" => {
                wire["evolutions"][0]["scenarios"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
                wire["evolutions"][0]["scenarios"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("source_program");
            }
            "duplicate" => {
                wire["evolutions"][0]["scenarios"][1] =
                    wire["evolutions"][0]["scenarios"][0].clone()
            }
            "unknown" => {
                wire["evolutions"][0]["scenarios"][0]["source_program"] = json!("d".repeat(64))
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<DevelopmentResponse>(wire)
                .map_or(true, |r| r.validate_for(&request).is_err()),
            "accepted {kind}"
        );
    }
}

#[test]
fn omitted_need_cannot_hide_known_accepted_scene_source_facts() {
    let (mut request, mut result) = accepted_scene_request();
    let mut other = request.decisions.decisions[0].clone();
    other.id = "other-need".into();
    request.decisions.decisions.push(other);
    let mut third = filtered();
    third["label"] = json!("Another captured implementation");
    let third = capture(third);
    assert!(request
        .sources
        .iter()
        .all(|source| { source.artifact.program_digest != third.artifact.program_digest }));
    request.sources.push(third.clone());
    request.validate().unwrap();
    result.response.evolutions = vec![EvolutionSuggestion {
        id: "unrelated-need".into(),
        candidate: "replace".into(),
        needs: vec!["other-need".into()],
        proposed_retirement: vec![],
        preserved_obligations: vec![],
        mappings: vec![],
        scenarios: vec![SuggestedScenarioMapping {
            original: request.examples[0].scenario.identity().unwrap(),
            source_program: Some(request.sources[1].artifact.program_digest.clone()),
            replacement_json: serde_json::to_string(&request.examples[0].scenario).unwrap(),
            explanation: "Preserve an accepted result".into(),
        }],
    }];
    refresh(&request, &mut result);
    result.validate_for(&request).unwrap();
    let mut incorrectly_accepted = vec![];
    for (kind, source) in [
        ("ambiguous missing source", None),
        (
            "unaccepted actual source",
            Some(third.artifact.program_digest),
        ),
    ] {
        result.response.evolutions[0].scenarios[0].source_program = source;
        if result.validate_for(&request).is_ok() {
            incorrectly_accepted.push(kind);
        }
    }
    assert!(
        incorrectly_accepted.is_empty(),
        "accepted {incorrectly_accepted:?}"
    );
}

#[test]
fn observation_instrumentation_cannot_change_created_records_or_ordered_exports() {
    let source = capture(filtered());
    let initial = scenario(
        &source,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = product_runtime::LocalRuntime::default();
    let original = runtime
        .replay(
            &source,
            &initial,
            &decisions(),
            RuntimeLimits::default(),
            "original",
        )
        .unwrap();
    assert_eq!(original.state, EvidenceState::Observed);
    assert_eq!(original.observations[0].view.rows.len(), 2);
    assert_ne!(
        original.observations[0].view.rows[0].record,
        original.observations[0].view.rows[1].record
    );
    let mut instrumented = initial.clone();
    instrumented.inputs.insert(
        1,
        SemanticInput::Observe {
            point: "midway".into(),
        },
    );
    instrumented.inputs.push(SemanticInput::Observe {
        point: "again".into(),
    });
    for renamed in [false, true] {
        let mut scene = instrumented.clone();
        if renamed {
            for input in &mut scene.inputs {
                if let SemanticInput::Observe { point } = input {
                    *point = format!("renamed-{point}");
                }
            }
        }
        let actual = runtime
            .replay(
                &source,
                &scene,
                &decisions(),
                RuntimeLimits::default(),
                "instrumented",
            )
            .unwrap();
        assert_eq!(actual.state, EvidenceState::Observed);
        let last = actual.observations.last().unwrap();
        let expected = &original.observations[0];
        assert_eq!(last.data_digest, expected.data_digest);
        assert_eq!(last.session_digest, expected.session_digest);
        assert_eq!(last.view, expected.view);
        assert_eq!(last.outputs, expected.outputs);
        assert_ne!(actual.binding.input_digest, original.binding.input_digest);
    }
}

#[test]
fn mapped_action_names_and_additive_schema_preserve_replay_record_identity() {
    let source = capture(filtered());
    let initial = scenario(
        &source,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = product_runtime::LocalRuntime::default();
    let original = runtime
        .replay(
            &source,
            &initial,
            &decisions(),
            RuntimeLimits::default(),
            "original",
        )
        .unwrap();
    let mut value = filtered();
    let old = value["actions"][0]["id"].as_str().unwrap().to_string();
    value["actions"][0]["id"] = json!("renamed_creation");
    value["views"][1]["kind"]["action"] = json!("renamed_creation");
    let mut extra = value["entities"][0].clone();
    extra["id"] = json!("extra");
    value["entities"].as_array_mut().unwrap().push(extra);
    let mapped = capture(value);
    let mut changed = initial.clone();
    changed.id = "another-scene-name".into();
    changed.label = "Another display label".into();
    changed.seed.schema = mapped.program.entities.clone();
    for input in &mut changed.inputs {
        if let SemanticInput::Invoke { action, .. } = input {
            if action == &old {
                *action = "renamed_creation".into();
            }
        }
    }
    let actual = runtime
        .replay(
            &mapped,
            &changed,
            &decisions(),
            RuntimeLimits::default(),
            "mapped",
        )
        .unwrap();
    assert_eq!(actual.state, EvidenceState::Observed);
    assert_eq!(actual.observations[0].view, original.observations[0].view);
    assert_eq!(
        actual.observations[0].outputs,
        original.observations[0].outputs
    );
    assert_ne!(actual.binding.input_digest, original.binding.input_digest);
    assert_ne!(actual.binding.source, original.binding.source);
}

#[test]
fn replay_ids_are_unique_for_repeated_mutations_and_interleaved_steps() {
    let source = capture(filtered());
    let scene = scenario(
        &source,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "first".into(),
            },
            SemanticInput::Navigate {
                view: "people".into(),
            },
            SemanticInput::AdvanceClock { days: 1 },
            add("Ada"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let ids = scene.replay_operation_ids().unwrap();
    assert_eq!(ids.len(), scene.inputs.len());
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len()
    );
    assert!(ids.iter().all(|id| id.len() <= 128));
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &source,
            &scene,
            &decisions(),
            RuntimeLimits::default(),
            "repeated",
        )
        .unwrap();
    assert_eq!(run.state, EvidenceState::Observed);
    assert_eq!(run.observations[1].view.rows.len(), 2);
    assert_ne!(
        run.observations[1].view.rows[0].record,
        run.observations[1].view.rows[1].record
    );
    assert_eq!(run.observations[1].outputs[0].rows.len(), 2);
    let mut changed = scene.clone();
    changed.random_seed += 1;
    assert_ne!(
        changed.operation_namespace().unwrap(),
        scene.operation_namespace().unwrap()
    );
}

#[test]
fn real_view_rows_and_columns_have_typed_empty_and_null_provenance() {
    let a = capture(filtered());
    let mut v = filtered();
    v["views"][0]["kind"]["rows"]["sort"][0]["descending"] = json!(true);
    let b = capture(v);
    let s = scenario(
        &a,
        vec![
            add("Ada"),
            add("Zoe"),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = product_runtime::LocalRuntime::default();
    let before = runtime
        .replay(&a, &s, &decisions(), RuntimeLimits::default(), "before")
        .unwrap();
    let after = runtime
        .replay(&b, &s, &decisions(), RuntimeLimits::default(), "after")
        .unwrap();
    assert_eq!(before.state, EvidenceState::Observed);
    assert_eq!(after.state, EvidenceState::Observed);
    assert_ne!(
        before.observations[0].view.rows,
        after.observations[0].view.rows
    );
    assert_eq!(before.binding.input_digest, after.binding.input_digest);
    let names = AcceptedProperty {
        id: "ordered-names".into(),
        description: "Actual column order".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewColumn {
                point: "done".into(),
                column: "name".into(),
                value_type: Type::Text,
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::Text),
                value: DataValue::List {
                    item_type: Type::Text,
                    items: vec![string("Ada"), string("Zoe")],
                },
            },
        },
    };
    assert_eq!(names.evaluate(&before.observations), Some(true));
    assert_eq!(names.evaluate(&after.observations), Some(false));
    let p = AcceptedProperty {
        id: "empty-rows".into(),
        description: "Empty typed rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewRows {
                point: "empty".into(),
                entity: "person".into(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::reference("person")),
                value: DataValue::List {
                    item_type: Type::reference("person"),
                    items: vec![],
                },
            },
        },
    };
    let empty = scenario(
        &a,
        vec![SemanticInput::Observe {
            point: "empty".into(),
        }],
    );
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &a,
            &empty,
            &decisions(),
            RuntimeLimits::default(),
            "empty-run",
        )
        .unwrap();
    assert_eq!(p.evaluate(&run.observations), Some(true));
    let wrong_empty = AcceptedProperty {
        id: "wrong-empty-column".into(),
        description: "A mismatched empty type is unknown".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewColumn {
                point: "empty".into(),
                column: "name".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::Integer),
                value: DataValue::List {
                    item_type: Type::Integer,
                    items: vec![],
                },
            },
        },
    };
    assert_eq!(wrong_empty.evaluate(&run.observations), None);

    let mut old = run.observations.clone();
    old[0].view_schema = None;
    assert_eq!(p.evaluate(&old), None);
    let mut wrong = p.clone();
    if let PropertyPredicate::Equal { left, .. } = &mut wrong.predicate {
        *left = PropertyTerm::ViewRows {
            point: "empty".into(),
            entity: "loan".into(),
        };
    }
    assert_eq!(wrong.evaluate(&run.observations), None);
    let mut optional = filtered();
    optional["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    optional["views"][0]["kind"]["columns"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"note","label":"Note","value":field(var("row"),"note")}));
    let optional = capture(optional);
    let s = scenario(
        &optional,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &optional,
            &s,
            &decisions(),
            RuntimeLimits::default(),
            "null-run",
        )
        .unwrap();
    let typ = Type::Optional {
        item: Box::new(Type::Text),
    };
    let p = AcceptedProperty {
        id: "null-cell".into(),
        description: "Nullable view column".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewColumn {
                point: "done".into(),
                column: "note".into(),
                value_type: typ.clone(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(typ.clone()),
                value: DataValue::List {
                    item_type: typ,
                    items: vec![DataValue::Null],
                },
            },
        },
    };
    assert_eq!(p.evaluate(&run.observations), Some(true));
    let mut corrupt = run.observations.clone();
    corrupt[0]
        .view_schema
        .as_mut()
        .unwrap()
        .columns
        .insert("note".into(), Type::Integer);
    assert!(corrupt[0].validate().is_err());
    assert_eq!(p.evaluate(&corrupt), None);
}

#[test]
fn legacy_observation_shape_is_preserved_and_old_execution_versions_are_stale() {
    let source = capture(filtered());
    let scene = scenario(
        &source,
        vec![SemanticInput::Observe {
            point: "done".into(),
        }],
    );
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &source,
            &scene,
            &decisions(),
            RuntimeLimits::default(),
            "current",
        )
        .unwrap();
    assert_eq!(run.binding.runtime_version, "local-interpreter/2");
    assert_eq!(run.binding.driver_version, "semantic-input/2");
    let mut old = run.clone();
    old.binding.runtime_version = "local-interpreter/1".into();
    assert!(!old.is_current(&run.binding));
    old = run.clone();
    old.binding.driver_version = "semantic-input/1".into();
    assert!(!old.is_current(&run.binding));
    let mut observation = run.observations[0].clone();
    observation.view_schema = None;
    let legacy = json!({"point":observation.point,"data_digest":observation.data_digest,"session_digest":observation.session_digest,"values":observation.values,"value_types":observation.value_types,"view":observation.view,"outputs":observation.outputs});
    assert_eq!(serde_json::to_value(&observation).unwrap(), legacy);
    assert_eq!(
        canonical_digest(IdentityDomain::Evidence, &observation).unwrap(),
        canonical_digest(IdentityDomain::Evidence, &legacy).unwrap()
    );
    assert!(serde_json::from_value::<Observation>(legacy)
        .unwrap()
        .view_schema
        .is_none());
}
#[test]
fn discovery_history_sources_cannot_displace_or_ambiguate_the_primary_pair() {
    let (mut request, _) = accepted_scene_request();
    request.operation = DevelopmentOperation::Discover;
    let mut value = filtered();
    value["label"] = json!("Selected historical source");
    let history = capture(value);
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &history,
            &request.examples[0].scenario,
            &request.decisions,
            RuntimeLimits::default(),
            "history",
        )
        .unwrap();
    let mut context = request.accepted_scenes[0].clone();
    context.source = history.artifact.clone();
    context.observations = run.observations;
    request.sources.push(history);
    request.accepted_scenes.push(context);
    request.validate().unwrap();
    let mut unrelated = request.clone();
    unrelated.accepted_scenes.pop();
    assert!(unrelated.validate().is_err());
    let mut duplicate = request.clone();
    duplicate.sources.push(duplicate.sources[2].clone());
    assert!(duplicate.validate().is_err());
    let mut missing = request.clone();
    missing.sources.remove(1);
    assert!(missing.validate().is_err());
    let mut wrong = request.clone();
    let old = wrong.sources[1].artifact.clone();
    let mut value = serde_json::to_value(&wrong.sources[1].program).unwrap();
    value["id"] = json!("another_app");
    wrong.sources[1] = capture(value);
    wrong.accepted_scenes.retain(|s| s.source != old);
    assert!(wrong.validate().is_err());
}
#[test]
fn withdrawn_history_is_terminal_receipt_bound_and_not_a_retirement_proposal() {
    let (mut request, mut result) = input();
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    let original = request.decisions.decisions[0].clone();
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["status"] = json!({"kind":"withdrawn","adoption":"recovery-1"});
    request.decisions.decisions[0] = serde_json::from_value(wire.clone()).unwrap();
    let mut older = original.clone();
    older.id = "older".into();
    older.status = DecisionStatus::Superseded { by: "saved".into() };
    request.decisions.decisions[0]
        .supersedes
        .push("older".into());
    request.decisions.decisions.push(older);
    request.decisions.validate().unwrap();
    assert_eq!(
        request.decisions.decisions[0].witness,
        prior.identity().unwrap()
    );
    assert_eq!(request.decisions.decisions[0].scenarios, original.scenarios);
    assert!(request
        .decisions
        .decisions
        .iter()
        .all(|d| d.status != DecisionStatus::Active));
    let mut revival = request.decisions.clone();
    let mut successor = original.clone();
    successor.id = "revival".into();
    successor.supersedes = vec!["saved".into()];
    revival.decisions.push(successor);
    assert!(
        revival.validate().is_err(),
        "a withdrawn node cannot silently become a supersession predecessor"
    );

    wire["status"]["adoption"] = json!("invalid receipt id");
    assert!(serde_json::from_value::<ScopedDecision>(wire)
        .unwrap()
        .validate()
        .is_err());
    request.operation = DevelopmentOperation::Reconcile;
    result.response.hypotheses.clear();
    result.response.evolutions.push(EvolutionSuggestion {
        id: "invalid-retirement".into(),
        candidate: "replace".into(),
        needs: vec!["saved".into(), "older".into()],
        proposed_retirement: vec!["saved".into()],
        preserved_obligations: vec![],
        mappings: vec![],
        scenarios: vec![],
    });
    refresh(&request, &mut result);
    assert!(result.validate_for(&request).is_err());
    result.response.evolutions[0].proposed_retirement = vec!["older".into()];
    assert!(result.validate_for(&request).is_err());
}
