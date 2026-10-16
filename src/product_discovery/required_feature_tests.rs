//! Typed absence classification only. These synthetic requirements do not
//! substitute for the host's separately tested explicit UI acceptance.
use super::*;
#[path = "../../tests/fixtures/product_runtime/mod.rs"]
mod fixture;
use fixture::*;
use serde_json::json;

fn case(
    kind: &str,
) -> (
    CapturedProgram,
    CapturedProgram,
    ScenarioSpec,
    DiscoveryPolicy,
) {
    let mut current = filtered();
    if kind == "selection" {
        current["views"][0]["kind"]["selection"] = serde_json::Value::Null;
    }
    let before = capture(current);
    let mut raw = filtered();
    let query = raw["actions"][1]["steps"][0]["items"].clone();
    raw["actions"].as_array_mut().unwrap().push(json!({"id":"feature","label":"New feature","parameters":match kind { "control" => json!({"value":{"kind":"text"}}), "selection" => json!({"value":{"kind":"list","item":{"kind":"reference","entity":"person"}}}), _ => json!({}) },"guards":[],"steps":[{"kind":"set_state","state":"selected","value":query}],"ensures":[]}));
    if kind == "activate" {
        raw["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"feature_button","label":"Feature","action":"feature","placement":"toolbar","arguments":{},"enabled":yes()}));
    }
    if kind == "control" {
        raw["views"][0]["kind"]["controls"].as_array_mut().unwrap().push(json!({"id":"feature_control","label":"Feature control","state":"search","on_change":"feature"}));
    }
    if kind == "selection" {
        raw["views"][0]["kind"]["selection"]["on_change"] = json!("feature");
    }
    if kind == "submit" {
        raw["views"].as_array_mut().unwrap().push(json!({"id":"feature_form","label":"Feature form","kind":{"kind":"form","action":"feature","fields":[],"defaults":{}},"actions":[],"keys":[]}));
    }
    let candidate = capture(raw);
    let input = match kind {
        "invoke" => SemanticInput::Invoke {
            action: "feature".into(),
            arguments: Values::new(),
        },
        "activate" => SemanticInput::Activate {
            view: "people".into(),
            binding: "feature_button".into(),
            row: None,
        },
        "submit" => SemanticInput::Submit {
            view: "feature_form".into(),
            arguments: Values::new(),
        },
        "control" => SemanticInput::Control {
            view: "people".into(),
            control: "feature_control".into(),
            value: string("Ada"),
        },
        "selection" => SemanticInput::Control {
            view: "people".into(),
            control: "pick".into(),
            value: DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        },
        _ => unreachable!(),
    };
    let scene = scenario(
        &candidate,
        vec![
            input,
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
    );
    scene.validate(&candidate.program).unwrap();
    assert!(scene.validate(&before.program).is_err());
    let property = AcceptedProperty {
        id: "selected-result".into(),
        description: "Synthetic independently supplied exact result".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "result".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 0 },
            },
        },
    };
    let mut requirement_scene = scene.clone();
    if kind == "submit" {
        requirement_scene.inputs.insert(
            0,
            SemanticInput::Navigate {
                view: "feature_form".into(),
            },
        );
    }
    let policy = DiscoveryPolicy {
        required_actions: ["feature".into()].into(),
        requirements: vec![RequirementCase {
            id: "explicit-case".into(),
            scenario: requirement_scene,
            properties: vec![property],
        }],
        ..Default::default()
    };
    (before, candidate, scene, policy)
}
#[test]
fn only_exact_required_invoke_activate_submit_and_control_absence_is_classified() {
    for kind in ["invoke", "activate", "submit", "control", "selection"] {
        let (before, candidate, scene, policy) = case(kind);
        assert!(
            classify(&before, &candidate, &scene, &policy).is_some(),
            "{kind}"
        );
        let mut no_case = policy.clone();
        no_case.requirements.clear();
        assert!(
            classify(&before, &candidate, &scene, &no_case).is_none(),
            "No independent property: {kind}"
        );
        let mut unrequired = policy.clone();
        unrequired.required_actions.clear();
        assert!(
            classify(&before, &candidate, &scene, &unrequired).is_none(),
            "A changed name is not independent acceptance: {kind}"
        );
        let mut unrelated = scene.clone();
        unrelated.inputs.insert(
            0,
            SemanticInput::Invoke {
                action: "unrelated_missing".into(),
                arguments: Values::new(),
            },
        );
        assert!(
            classify(&before, &candidate, &unrelated, &policy).is_none(),
            "Unrelated invalid input: {kind}"
        );
        let mut wrong_session = scene.clone();
        wrong_session.session.view = "missing_view".into();
        assert!(
            classify(&before, &candidate, &wrong_session, &policy).is_none(),
            "Unrelated view: {kind}"
        );
    }
}
#[test]
fn wrong_ui_target_and_arguments_are_not_explained_as_a_missing_feature() {
    let (before, candidate, mut scene, policy) = case("activate");
    scene.inputs[0] = SemanticInput::Activate {
        view: "new_person".into(),
        binding: "feature_button".into(),
        row: None,
    };
    assert!(classify(&before, &candidate, &scene, &policy).is_none());
    for kind in ["invoke", "submit"] {
        let (before, candidate, mut scene, policy) = case(kind);
        match &mut scene.inputs[0] {
            SemanticInput::Invoke { arguments, .. } | SemanticInput::Submit { arguments, .. } => {
                arguments.insert("wrong_argument".into(), string("not declared"));
            }
            _ => unreachable!(),
        }
        assert!(
            classify(&before, &candidate, &scene, &policy).is_none(),
            "{kind}"
        );
    }
    let (before, candidate, mut scene, policy) = case("control");
    if let SemanticInput::Control { value, .. } = &mut scene.inputs[0] {
        *value = DataValue::Integer { value: 7 };
    }
    assert!(classify(&before, &candidate, &scene, &policy).is_none());
}

#[test]
fn selection_callback_absence_preserves_exact_target_and_list_reference_types() {
    let (before, candidate, scene, policy) = case("selection");
    for (view, control, value) in [
        (
            "new_person",
            "pick",
            DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        ),
        (
            "people",
            "missing_pick",
            DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        ),
        ("people", "pick", string("not a reference list")),
        (
            "people",
            "pick",
            DataValue::List {
                item_type: Type::Text,
                items: vec![string("not a reference")],
            },
        ),
        (
            "people",
            "pick",
            DataValue::List {
                item_type: Type::reference("asset"),
                items: vec![reference("asset", "drill")],
            },
        ),
    ] {
        let mut wrong = scene.clone();
        wrong.inputs[0] = SemanticInput::Control {
            view: view.into(),
            control: control.into(),
            value,
        };
        assert!(wrong.validate(&candidate.program).is_err());
        assert!(classify(&before, &candidate, &wrong, &policy).is_none());
    }
    // A valid but unrelated Current-incompatible control must survive the
    // projection and make classification fail even beside the required callback.
    let mut raw: serde_json::Value = serde_json::from_slice(&candidate.source_bytes).unwrap();
    raw["state"].as_array_mut().unwrap().push(json!({"id":"extra_text","label":"Extra text","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}}));
    raw["views"][0]["kind"]["controls"].as_array_mut().unwrap().push(json!({"id":"unrelated_control","label":"Unrelated control","state":"extra_text","on_change":null}));
    let candidate = capture(raw);
    let mut unrelated = scene.clone();
    unrelated.session = SessionState::initial(&candidate.program).unwrap();
    unrelated.inputs.insert(
        0,
        SemanticInput::Control {
            view: "people".into(),
            control: "unrelated_control".into(),
            value: string("unrelated"),
        },
    );
    unrelated.validate(&candidate.program).unwrap();
    assert!(classify(&before, &candidate, &unrelated, &policy).is_none());
}

fn navigation_case() -> (
    CapturedProgram,
    CapturedProgram,
    ScenarioSpec,
    DiscoveryPolicy,
) {
    let (before, candidate, _, _) = case("invoke");
    let mut raw: serde_json::Value = serde_json::from_slice(&candidate.source_bytes).unwrap();
    let mut view = raw["views"][0].clone();
    view["id"] = json!("feature_results");
    view["kind"]["selection"] = serde_json::Value::Null;
    raw["views"].as_array_mut().unwrap().push(view);
    raw["state"].as_array_mut().unwrap().push(json!({"id":"feature_text","label":"Feature state","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}}));
    let candidate = capture(raw);
    let scene = scenario(
        &candidate,
        vec![
            add("Ada"),
            invoke("feature", Values::new()),
            SemanticInput::Navigate {
                view: "feature_results".into(),
            },
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
    );
    let policy = DiscoveryPolicy {
        required_actions: ["feature".into()].into(),
        requirements: vec![RequirementCase {
            id: "feature-results".into(),
            scenario: scene.clone(),
            properties: vec![AcceptedProperty {
                id: "visible-name".into(),
                description: "The required feature presents its result".into(),
                predicate: PropertyPredicate::Equal {
                    left: PropertyTerm::ViewColumn {
                        point: "result".into(),
                        column: "name".into(),
                        value_type: Type::Text,
                    },
                    right: PropertyTerm::Literal {
                        value_type: Type::list(Type::Text),
                        value: DataValue::List {
                            item_type: Type::Text,
                            items: vec![string("Ada")],
                        },
                    },
                },
            }],
        }],
        ..Default::default()
    };
    (before, candidate, scene, policy)
}

#[test]
fn independently_required_new_view_navigation_is_only_classified_not_executed() {
    let (before, candidate, scene, policy) = navigation_case();
    let run = LocalRuntime::default()
        .replay(
            &candidate,
            &scene,
            &decisions(),
            RuntimeLimits::default(),
            "checked",
        )
        .unwrap();
    assert_eq!(run.state, EvidenceState::Observed);
    assert_eq!(run.observations[0].view.view, "feature_results");
    assert_eq!(
        policy.requirements[0].properties[0].evaluate(&run.observations),
        Some(true)
    );
    let expected = LocalRuntime::default()
        .replay(
            &before,
            &scene,
            &decisions(),
            RuntimeLimits::default(),
            "baseline",
        )
        .unwrap_err();
    assert_eq!(
        classify(&before, &candidate, &scene, &policy),
        Some(expected),
        "The independently checked feature's presented new view belongs to its type-level absence"
    );
}

fn request(before: &CapturedProgram, candidate: &CapturedProgram) -> DevelopmentRequest {
    DevelopmentRequest {
        version: 1,
        id: "required-navigation".into(),
        project_id: "runtime-project".into(),
        operation: DevelopmentOperation::Discover,
        request: "Add the independently checked feature and its results view".into(),
        sources: vec![before.clone(), candidate.clone()],
        context: DevelopmentContext {
            view: Some("people".into()),
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: decisions(),
        unknowns: vec![],
        required_capabilities: BTreeSet::new(),
    }
}

#[test]
fn new_view_discovery_keeps_two_real_prospective_runs_and_no_current_experience() {
    let (before, candidate, scene, policy) = navigation_case();
    let request = request(&before, &candidate);
    let mut alternative: serde_json::Value =
        serde_json::from_slice(&candidate.source_bytes).unwrap();
    alternative["actions"][3]["steps"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")});
    let result = DevelopmentResult {
        producer: Producer::Fixture {
            name: "synthetic navigation choices".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "exact".into(),
                    source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "other".into(),
                    source_json: serde_json::to_string(&alternative).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "feature-choice".into(),
                statement: "The required feature may select or leave its visible results".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "feature".into(),
                observable: "selected_count".into(),
                sources: vec![SourceLocus {
                    relative_path: "program.json".into(),
                    raw_digest: candidate.artifact.raw_digest.clone(),
                    pointer: "/actions/3".into(),
                }],
                alternatives: vec!["exact".into(), "other".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scene).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.defects.is_empty(), "{:?}", report.defects);
    assert!(report.unverified.is_empty(), "{:?}", report.unverified);
    assert_eq!(report.questions.len(), 1);
    assert!(report
        .runs
        .iter()
        .all(|run| run.binding.artifact != before.artifact));
    assert!(report.runs.iter().any(|run| run.id == "captured-candidate"
        && run.binding.scenario_digest == scene.identity().unwrap()));
    assert!(report.questions[0].witnesses.iter().all(|witness| {
        witness
            .before_program()
            .program
            .views
            .iter()
            .any(|view| view.id == "feature_results")
            && witness
                .after_program()
                .program
                .views
                .iter()
                .any(|view| view.id == "feature_results")
    }));
}

#[test]
fn new_view_absence_needs_a_successful_typed_requirement_for_the_same_action() {
    let (before, candidate, scene, policy) = navigation_case();
    for kind in [
        "failed",
        "unverified",
        "wrong_type",
        "untyped",
        "unpresented",
        "conditional",
        "runtime",
        "budget",
        "different_action",
    ] {
        let mut policy = policy.clone();
        let mut candidate = candidate.clone();
        let case = &mut policy.requirements[0];
        match kind {
            "failed" => {
                let PropertyPredicate::Equal { right, .. } = &mut case.properties[0].predicate
                else {
                    unreachable!()
                };
                *right = PropertyTerm::Literal {
                    value_type: Type::list(Type::Text),
                    value: DataValue::List {
                        item_type: Type::Text,
                        items: vec![string("Wrong")],
                    },
                };
            }
            "unverified" => {
                let PropertyPredicate::Equal {
                    left: PropertyTerm::ViewColumn { column, .. },
                    ..
                } = &mut case.properties[0].predicate
                else {
                    unreachable!()
                };
                *column = "unknown_column".into();
            }
            "wrong_type" => {
                case.properties[0].predicate = PropertyPredicate::Equal {
                    left: PropertyTerm::ViewColumn {
                        point: "result".into(),
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
                };
            }
            "untyped" => {
                case.properties[0].predicate = PropertyPredicate::Equal {
                    left: PropertyTerm::Observed {
                        point: "result".into(),
                        observable: "selected_count".into(),
                        value_type: Type::Integer,
                    },
                    right: PropertyTerm::Literal {
                        value_type: Type::Integer,
                        value: DataValue::Integer { value: 1 },
                    },
                };
            }
            "unpresented" => {
                case.scenario.inputs.insert(
                    3,
                    SemanticInput::Navigate {
                        view: "people".into(),
                    },
                );
            }
            "conditional" => {
                case.properties[0].predicate = PropertyPredicate::Or {
                    values: vec![
                        PropertyPredicate::Equal {
                            left: PropertyTerm::Literal {
                                value_type: Type::Integer,
                                value: DataValue::Integer { value: 1 },
                            },
                            right: PropertyTerm::Literal {
                                value_type: Type::Integer,
                                value: DataValue::Integer { value: 1 },
                            },
                        },
                        case.properties[0].predicate.clone(),
                    ],
                };
            }
            "runtime" => policy.search.runtime.fuel = 1,
            "budget" => policy.max_precheck_replays = 0,
            "different_action" => {
                let mut raw: serde_json::Value =
                    serde_json::from_slice(&candidate.source_bytes).unwrap();
                let mut action = raw["actions"][3].clone();
                action["id"] = json!("other_feature");
                raw["actions"].as_array_mut().unwrap().push(action);
                candidate = capture(raw);
                case.scenario.inputs[1] = invoke("other_feature", Values::new());
                policy.required_actions.insert("other_feature".into());
                let mut feature_case = case.clone();
                feature_case.id = "feature-without-new-view".into();
                feature_case.scenario.inputs[1] = invoke("feature", Values::new());
                feature_case.scenario.inputs[2] = SemanticInput::Navigate {
                    view: "people".into(),
                };
                policy.requirements.push(feature_case);
            }
            _ => unreachable!(),
        }
        assert!(
            classify(&before, &candidate, &scene, &policy).is_none(),
            "{kind}"
        );
    }
    let mut rows = policy.clone();
    rows.requirements[0].properties[0].predicate = PropertyPredicate::And {
        values: vec![PropertyPredicate::Equal {
            left: PropertyTerm::Count {
                value: Box::new(PropertyTerm::ViewRows {
                    point: "result".into(),
                    entity: "person".into(),
                }),
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 1 },
            },
        }],
    };
    assert!(classify(&before, &candidate, &scene, &rows).is_some());
}

#[test]
fn new_view_projection_preserves_other_invalid_inputs_and_source_identity() {
    let (before, candidate, scene, policy) = navigation_case();
    for kind in [
        "unrelated_view",
        "unrelated_action",
        "unsupported_control",
        "wrong_argument",
        "session_type",
        "session_view",
    ] {
        let mut candidate = candidate.clone();
        let mut scene = scene.clone();
        match kind {
            "unrelated_view" => {
                let mut raw: serde_json::Value =
                    serde_json::from_slice(&candidate.source_bytes).unwrap();
                let mut view = raw["views"][2].clone();
                view["id"] = json!("unrelated_results");
                raw["views"].as_array_mut().unwrap().push(view);
                candidate = capture(raw);
                scene.inputs.insert(
                    2,
                    SemanticInput::Navigate {
                        view: "unrelated_results".into(),
                    },
                );
            }
            "unrelated_action" => {
                let mut raw: serde_json::Value =
                    serde_json::from_slice(&candidate.source_bytes).unwrap();
                let mut action = raw["actions"][3].clone();
                action["id"] = json!("unrelated_feature");
                raw["actions"].as_array_mut().unwrap().push(action);
                candidate = capture(raw);
                scene
                    .inputs
                    .insert(2, invoke("unrelated_feature", Values::new()));
            }
            "unsupported_control" => {
                scene.inputs.insert(
                    3,
                    SemanticInput::Control {
                        view: "feature_results".into(),
                        control: "search_input".into(),
                        value: string("Ada"),
                    },
                );
            }
            "wrong_argument" => {
                scene.inputs[1] = invoke("feature", args(&[("extra", string("wrong"))]))
            }
            "session_type" => {
                scene
                    .session
                    .values
                    .insert("search".into(), DataValue::Integer { value: 3 });
            }
            "session_view" => scene.session.view = "feature_results".into(),
            _ => unreachable!(),
        }
        assert!(
            classify(&before, &candidate, &scene, &policy).is_none(),
            "{kind}"
        );
    }
    let cancelled = ReplayBudget {
        remaining: Cell::new(policy.max_precheck_replays),
        cancelled: Arc::new(AtomicBool::new(true)),
    };
    let (_, unknown, runs, evidence) = check_requirements(
        &candidate,
        &request(&before, &candidate),
        &policy,
        &LocalRuntime::default(),
        &cancelled,
    )
    .unwrap();
    assert!(!unknown.is_empty());
    assert!(runs.is_empty());
    assert!(required_feature_absence(&before, &candidate, &scene, &policy, &evidence).is_none());
    let evidence = feature_evidence(&candidate, &policy);
    let rebound = candidate.clone().at_path("moved/program.json").unwrap();
    assert!(required_feature_absence(&before, &rebound, &scene, &policy, &evidence).is_none());
    let mut changed: serde_json::Value = serde_json::from_slice(&candidate.source_bytes).unwrap();
    changed["label"] = json!("Changed source identity");
    assert!(
        required_feature_absence(&before, &capture(changed), &scene, &policy, &evidence).is_none()
    );
}

fn feature_evidence(
    candidate: &CapturedProgram,
    policy: &DiscoveryPolicy,
) -> RequiredFeatureEvidence {
    let budget = ReplayBudget {
        remaining: Cell::new(policy.max_precheck_replays),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    check_requirements(
        candidate,
        &request(candidate, candidate),
        policy,
        &LocalRuntime::default(),
        &budget,
    )
    .unwrap()
    .3
}

fn classify(
    before: &CapturedProgram,
    candidate: &CapturedProgram,
    scene: &ScenarioSpec,
    policy: &DiscoveryPolicy,
) -> Option<AdapterError> {
    required_feature_absence(
        before,
        candidate,
        scene,
        policy,
        &feature_evidence(candidate, policy),
    )
}

#[test]
fn required_navigation_permutations_require_the_view_in_every_success_path() {
    let (before, candidate, mut scene, mut policy) = navigation_case();
    let good = policy.requirements[0].properties[0].predicate.clone();
    let mut wrong = good.clone();
    let PropertyPredicate::Equal { right, .. } = &mut wrong else {
        unreachable!()
    };
    *right = PropertyTerm::Literal {
        value_type: Type::list(Type::Text),
        value: DataValue::List {
            item_type: Type::Text,
            items: vec![string("Other ordering")],
        },
    };
    policy.requirements[0].properties[0].predicate = PropertyPredicate::Or {
        values: vec![wrong, PropertyPredicate::And { values: vec![good] }],
    };
    scene.inputs.insert(
        3,
        SemanticInput::Navigate {
            view: "people".into(),
        },
    );
    scene.inputs.insert(
        4,
        SemanticInput::Navigate {
            view: "feature_results".into(),
        },
    );
    assert!(classify(&before, &candidate, &scene, &policy).is_some());
    scene.inputs.remove(1);
    assert!(
        classify(&before, &candidate, &scene, &policy).is_none(),
        "A proven view alone is not a required missing action"
    );
}
