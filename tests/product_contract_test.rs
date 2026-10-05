#[path = "support/product_contract_fixture.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;

use product_contract::*;
use serde_json::{json, Value};

fn parse(value: Value) -> Result<AppDefinition, ContractError> {
    AppDefinition::parse(&serde_json::to_vec(&value).unwrap())
}
fn app() -> AppDefinition {
    parse(fixture::organizer()).unwrap()
}

#[test]
fn structurally_distinct_workflows_and_new_compositions_validate() {
    for value in [
        fixture::organizer(),
        fixture::equipment(),
        fixture::new_composition(),
    ] {
        let first = parse(value).unwrap();
        assert_eq!(
            first,
            AppDefinition::parse(&canonical_bytes(&first).unwrap()).unwrap()
        );
    }
}

#[test]
fn duplicate_unknown_and_executable_payload_members_are_rejected() {
    let source = serde_json::to_string(&fixture::organizer()).unwrap();
    let duplicate = source.replacen("\"version\":1", "\"version\":1,\"version\":1", 1);
    assert!(AppDefinition::parse(duplicate.as_bytes()).is_err());
    for (key, value) in [
        ("passed", json!(true)),
        ("shell", json!("rm -rf /")),
        ("network", json!({})),
        ("evidence", json!({"passed":true})),
    ] {
        let mut p = fixture::organizer();
        p[key] = value;
        assert!(parse(p).is_err(), "accepted {key}");
    }
    let mut p = fixture::organizer();
    p["actions"][0]["steps"][0] = json!({"kind":"eval","code":"1+1"});
    assert!(parse(p).is_err());
}

#[test]
fn versions_empty_required_sections_and_duplicate_ids_are_rejected() {
    let mut p = fixture::organizer();
    p["version"] = json!(2);
    assert!(parse(p).is_err());
    for section in ["entities", "views"] {
        let mut p = fixture::organizer();
        p[section] = json!([]);
        assert!(parse(p).is_err());
    }
    for section in [
        "entities",
        "state",
        "actions",
        "views",
        "outputs",
        "observables",
    ] {
        let mut p = fixture::organizer();
        let first = p[section][0].clone();
        p[section].as_array_mut().unwrap().push(first);
        assert!(parse(p).is_err(), "duplicate {section}");
    }
    let mut p = fixture::organizer();
    p["entities"][0]["id"] = json!("Display name");
    assert!(parse(p).is_err());
}

#[test]
fn invalid_references_and_action_types_cannot_reach_runtime() {
    for mutate in [
        ("/actions/0/steps/0/entity", json!("missing")),
        ("/actions/0/steps/0/values/name", fixture::yes()),
        ("/views/0/actions/0/action", json!("missing")),
        ("/views/0/keys/0/binding", json!("missing")),
        ("/state/1/value_type/item/entity", json!("missing")),
        ("/observables/0/value/items/state", json!("missing")),
    ]
    .into_iter()
    {
        let mut p = fixture::organizer();
        *p.pointer_mut(mutate.0).unwrap() = mutate.1;
        assert!(parse(p).is_err());
    }
    let mut p = fixture::equipment();
    p["actions"][0]["guards"] = json!([fixture::text("true")]);
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["actions"][0]["steps"][0]["values"]
        .as_object_mut()
        .unwrap()
        .remove("name");
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["actions"][0]["steps"][0]["values"]["unknown"] = fixture::text("x");
    assert!(parse(p).is_err());
}

#[test]
fn query_collection_and_view_type_errors_are_rejected() {
    let mut p = fixture::organizer();
    p["views"][0]["kind"]["rows"]["limit"] = json!(0);
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["views"][0]["kind"]["rows"]["predicate"] = fixture::text("yes");
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["actions"][1]["steps"][0]["target"]["state"] = json!("search");
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["views"][0]["kind"]["selection"]["state"] = json!("search");
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["views"][1]["kind"]["fields"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(parse(p).is_err());
}

#[test]
fn parser_and_typed_ast_have_hostile_input_budgets() {
    assert!(AppDefinition::parse(&vec![b' '; MAX_WIRE_BYTES + 1]).is_err());
    let mut expr = fixture::yes();
    for _ in 0..MAX_EXPR_DEPTH + 1 {
        expr = json!({"kind":"not","value":expr});
    }
    let mut p = fixture::organizer();
    p["actions"][0]["guards"] = json!([expr]);
    assert!(parse(p).is_err());
    let mut p = fixture::organizer();
    p["label"] = json!("x".repeat(MAX_TEXT_BYTES + 1));
    assert!(parse(p).is_err());
    let mut typed = app();
    for _ in 0..MAX_EXPR_DEPTH + 1 {
        typed.actions[0].guards = vec![Expr::Not {
            value: Box::new(typed.actions[0].guards.pop().unwrap_or(Expr::Literal {
                value_type: Type::Boolean,
                value: DataValue::Boolean { value: true },
            })),
        }];
    }
    assert!(typed.validate().is_err());
    let mut p = fixture::organizer();
    p["actions"][0]["guards"] = Value::Array(vec![fixture::yes(); MAX_ITEMS + 1]);
    assert!(parse(p).is_err());
}

#[test]
fn raw_canonical_and_semantic_program_identities_are_separate() {
    let first = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    let second = CapturedProgram::capture(
        &serde_json::to_vec_pretty(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    assert_ne!(first.artifact.raw_digest, second.artifact.raw_digest);
    assert_eq!(
        first.artifact.program_digest,
        second.artifact.program_digest
    );
    assert_eq!(
        first.artifact.semantic_digest,
        second.artifact.semantic_digest
    );
    let mut renamed = fixture::organizer();
    renamed["label"] = json!("Same logic with another label");
    let renamed = CapturedProgram::capture(
        &serde_json::to_vec(&renamed).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    assert_ne!(
        first.artifact.program_digest,
        renamed.artifact.program_digest
    );
    assert_eq!(
        first.artifact.semantic_digest,
        renamed.artifact.semantic_digest
    );
    let mut changed = fixture::organizer();
    changed["views"][0]["kind"]["rows"]["limit"] = json!(2);
    let changed = CapturedProgram::capture(
        &serde_json::to_vec(&changed).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    assert_ne!(
        first.artifact.semantic_digest,
        changed.artifact.semantic_digest
    );
    let mut tampered = first.clone();
    tampered.source_bytes.push(b' ');
    assert!(tampered.validate().is_err());
}

#[test]
fn canonical_identity_preserves_ordered_actions_and_domains() {
    let a = json!({"z":1,"a":{"y":2,"b":3}});
    let b = json!({"a":{"b":3,"y":2},"z":1});
    assert_eq!(
        canonical_digest(IdentityDomain::Request, &a).unwrap(),
        canonical_digest(IdentityDomain::Request, &b).unwrap()
    );
    assert_ne!(
        canonical_digest(IdentityDomain::Data, &a).unwrap(),
        canonical_digest(IdentityDomain::Request, &a).unwrap()
    );
    let mut p = fixture::organizer();
    let before = parse(p.clone()).unwrap().identity().unwrap();
    p["actions"][0]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"assert","condition":fixture::yes(),"message":"must hold"}));
    assert_ne!(before, parse(p).unwrap().identity().unwrap());
}

#[test]
fn source_loci_require_exact_source_and_safe_json_pointers() {
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    let mut locus = SourceLocus {
        relative_path: "program.json".into(),
        raw_digest: capture.artifact.raw_digest.clone(),
        pointer: "/actions/0/steps/0".into(),
    };
    locus.validate_against(&capture).unwrap();
    locus.pointer = "/actions/900".into();
    assert!(locus.validate_against(&capture).is_err());
    locus.pointer = "/actions/0".into();
    locus.relative_path = "../private".into();
    assert!(locus.validate_against(&capture).is_err());
    locus.relative_path = "program.json".into();
    locus.raw_digest = canonical_digest(IdentityDomain::Data, &json!(1)).unwrap();
    assert!(locus.validate_against(&capture).is_err());
}

#[test]
fn snapshot_identity_and_validation_preserve_later_facts() {
    let a = app();
    let mut data = DataSnapshot::empty("project", &a).unwrap();
    data.records.push(Record {
        entity: "person".into(),
        id: "one".into(),
        revision: 1,
        created_program: a.identity().unwrap(),
        archived: false,
        values: std::collections::BTreeMap::from([
            (
                "name".into(),
                DataValue::Text {
                    value: "Ada".into(),
                },
            ),
            (
                "area".into(),
                DataValue::Text {
                    value: "West".into(),
                },
            ),
        ]),
    });
    data.validate().unwrap();
    let before = data.identity().unwrap();
    data.records[0].values.insert(
        "area".into(),
        DataValue::Text {
            value: "East".into(),
        },
    );
    assert_ne!(before, data.identity().unwrap());
    data.records.push(data.records[0].clone());
    assert!(data.validate().is_err());
    data.records.pop();
    data.records[0]
        .values
        .insert("extra".into(), DataValue::Boolean { value: true });
    assert!(data.validate().is_err());
}

#[test]
fn imported_model_claims_never_become_execution_evidence() {
    let response = json!({"version":1,"request_digest":canonical_digest(IdentityDomain::Request,&json!(1)).unwrap(),"candidates":[{"id":"candidate","source_json":serde_json::to_string(&fixture::organizer()).unwrap()}],"hypotheses":[],"evolutions":[],"unsupported":[]});
    DevelopmentResponse::parse(&serde_json::to_vec(&response).unwrap()).unwrap();
    for key in ["passed", "evidence", "observations", "adopted"] {
        let mut p = response.clone();
        p[key] = json!(true);
        assert!(DevelopmentResponse::parse(&serde_json::to_vec(&p).unwrap()).is_err());
    }
    let mut p = response;
    p["candidates"][0]["source_json"] = json!("echo shell");
    assert!(DevelopmentResponse::parse(&serde_json::to_vec(&p).unwrap()).is_err());
}

#[test]
fn runtime_freshness_checks_every_source_data_driver_and_session_component() {
    let captured = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "structural".into(),
        },
        None,
    )
    .unwrap();
    let d = canonical_digest(IdentityDomain::Data, &json!(1)).unwrap();
    let context = RunBinding {
        source: captured.binding.clone(),
        artifact: captured.artifact.clone(),
        data_digest: d.clone(),
        input_digest: d.clone(),
        scenario_digest: d.clone(),
        decision_digest: d.clone(),
        session_digest: d.clone(),
        runtime_version: "runtime-1".into(),
        driver_version: "driver-1".into(),
    };
    let mut changed = context.clone();
    changed.driver_version = "driver-2".into();
    assert!(!context.matches(&changed));
    changed = context.clone();
    changed.session_digest = canonical_digest(IdentityDomain::Session, &json!(2)).unwrap();
    assert!(!context.matches(&changed));
    changed = context.clone();
    changed.artifact.raw_digest = canonical_digest(IdentityDomain::Source, &json!(2)).unwrap();
    assert!(!context.matches(&changed));
    assert!(context.matches(&context));
}

fn count_property() -> AcceptedProperty {
    AcceptedProperty {
        id: "count_matches".into(),
        description: "Preview count equals actual exported rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "result".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::OutputCount {
                point: "result".into(),
                output: "roster".into(),
            },
        },
    }
}
fn empty_view() -> ViewObservation {
    ViewObservation {
        view: "people".into(),
        rows: vec![],
        controls: Default::default(),
        selected: vec![],
        enabled_actions: Default::default(),
        form_values: Default::default(),
    }
}
#[test]
fn accepted_properties_require_actual_observation_and_artifact_content() {
    let property = count_property();
    property.validate().unwrap();
    assert_eq!(property.evaluate(&[]), None);
    let digest = canonical_digest(IdentityDomain::Data, &json!(1)).unwrap();
    let artifact = LocalArtifact::from_rows(
        "roster",
        OutputFormat::Csv,
        text_columns(&["name"]),
        vec![std::collections::BTreeMap::from([(
            "name".into(),
            DataValue::Text {
                value: "Ada, =friend".into(),
            },
        )])],
    )
    .unwrap();
    let mut observation = Observation {
        value_types: app().observable_types().unwrap(),
        point: "result".into(),
        data_digest: digest.clone(),
        session_digest: digest,
        values: std::collections::BTreeMap::from([(
            "selected_count".into(),
            DataValue::Integer { value: 1 },
        )]),
        view: empty_view(),
        view_schema: None,
        outputs: vec![artifact],
    };
    assert_eq!(property.evaluate(&[observation.clone()]), Some(true));
    observation
        .values
        .insert("selected_count".into(), DataValue::Integer { value: 2 });
    assert_eq!(property.evaluate(&[observation.clone()]), Some(false));
    observation.outputs[0].rows.clear();
    assert!(observation.outputs[0].validate().is_err());
    assert_eq!(property.evaluate(&[observation]), None);
}

#[test]
fn output_codec_quotes_text_and_never_accepts_unrelated_bytes() {
    let mut artifact = LocalArtifact::from_rows(
        "export",
        OutputFormat::Csv,
        text_columns(&["name"]),
        vec![std::collections::BTreeMap::from([(
            "name".into(),
            DataValue::Text {
                value: "=HYPERLINK(\"x\")".into(),
            },
        )])],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(artifact.bytes.clone()).unwrap(),
        "name\r\n\"'=HYPERLINK(\"\"x\"\")\"\r\n"
    );
    artifact.validate().unwrap();
    artifact.bytes = b"unrelated".to_vec();
    artifact.bytes_digest = LocalArtifact::bytes_identity(&artifact.bytes);
    assert!(artifact.validate().is_err());
}

#[test]
fn scope_does_not_generalize_missing_context_or_other_operations() {
    let scope = DecisionScope {
        operations: std::collections::BTreeSet::from(["export_people".into()]),
        population: Population::CreatedAfter { generation: 4 },
        conditions: std::collections::BTreeMap::from([(
            "purpose".into(),
            DataValue::Text {
                value: "roster".into(),
            },
        )]),
        excluded_records: vec![],
        unknowns: vec![],
    };
    scope.validate().unwrap();
    let mut context = ScopeContext {
        operation: "delete_people".into(),
        record: None,
        is_new: None,
        created_generation: Some(5),
        attributes: Default::default(),
        predicate_result: None,
    };
    assert_eq!(scope.matches(&context), ScopeMatch::Outside);
    context.operation = "export_people".into();
    assert_eq!(scope.matches(&context), ScopeMatch::Unknown);
    context.attributes.insert(
        "purpose".into(),
        DataValue::Text {
            value: "roster".into(),
        },
    );
    assert_eq!(scope.matches(&context), ScopeMatch::Applies);
    context.created_generation = Some(4);
    assert_eq!(scope.matches(&context), ScopeMatch::Outside);
}

#[test]
fn supersession_cycles_and_unresolved_active_decisions_are_rejected() {
    let digest = canonical_digest(IdentityDomain::Evidence, &json!(1)).unwrap();
    let decision = ScopedDecision {
        id: "one".into(),
        revision: 1,
        request: "Keep export count accurate".into(),
        rationale: None,
        scope: DecisionScope {
            operations: std::collections::BTreeSet::from(["export_people".into()]),
            population: Population::All,
            conditions: Default::default(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::EitherAcceptable,
        status: DecisionStatus::Active,
        obligations: vec![count_property()],
        scenarios: vec![digest.clone()],
        witness: digest,
        supersedes: vec![],
    };
    decision.validate().unwrap();
    let mut pending = decision.clone();
    pending.outcome = DecisionOutcome::BothNeeded;
    assert!(pending.validate().is_err());
    pending.status = DecisionStatus::Pending;
    pending.validate().unwrap();
    let mut one = decision.clone();
    let mut two = decision;
    two.id = "two".into();
    one.status = DecisionStatus::Superseded { by: "two".into() };
    two.supersedes = vec!["one".into()];
    let mut graph = DecisionGraph {
        version: 1,
        revision: 2,
        decisions: vec![one, two],
    };
    graph.validate().unwrap();
    graph.decisions[1].status = DecisionStatus::Superseded { by: "one".into() };
    graph.decisions[0].supersedes = vec!["two".into()];
    assert!(graph.validate().is_err());
}

#[test]
fn request_identity_and_response_matching_include_active_intentions() {
    let request = DevelopmentRequest {
        version: 1,
        id: "request".into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Generate,
        request: "A simple local roster".into(),
        sources: vec![],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "candidate".into(),
            source_json: serde_json::to_string(&fixture::organizer()).unwrap(),
        }],
        hypotheses: vec![],
        evolutions: vec![],
        unsupported: vec![],
    };
    response.validate_for(&request).unwrap();
    let mut changed = request;
    changed.unknowns.push(UnknownBoundary {
        id: "deletion".into(),
        operations: std::collections::BTreeSet::from(["delete".into()]),
        description: "Deletion behavior not chosen".into(),
    });
    assert!(response.validate_for(&changed).is_err());
}

#[test]
fn action_placement_requires_row_exactly_when_bound() {
    let organizer = app();
    validate_input(
        &SemanticInput::Activate {
            view: "people".into(),
            binding: "export_button".into(),
            row: None,
        },
        &organizer,
    )
    .unwrap();
    assert!(validate_input(
        &SemanticInput::Activate {
            view: "people".into(),
            binding: "export_button".into(),
            row: Some(RecordRef {
                entity: "person".into(),
                record: "one".into()
            })
        },
        &organizer
    )
    .is_err());
    let equipment = parse(fixture::equipment()).unwrap();
    assert!(validate_input(
        &SemanticInput::Activate {
            view: "assets".into(),
            binding: "borrow_asset".into(),
            row: None
        },
        &equipment
    )
    .is_err());
    validate_input(
        &SemanticInput::Activate {
            view: "assets".into(),
            binding: "borrow_asset".into(),
            row: Some(RecordRef {
                entity: "asset".into(),
                record: "one".into(),
            }),
        },
        &equipment,
    )
    .unwrap();
    let mut broken = fixture::equipment();
    broken["views"][0]["actions"][0]["placement"] = json!("toolbar");
    assert!(parse(broken).is_err());
}

fn scenario() -> ScenarioSpec {
    let a = app();
    ScenarioSpec {
        version: 1,
        id: "scene".into(),
        label: "Synthetic component scene".into(),
        seed: DataSnapshot::empty("project", &a).unwrap(),
        session: SessionState::initial(&a).unwrap(),
        clock_day: 0,
        random_seed: 0,
        inputs: vec![SemanticInput::Observe {
            point: "result".into(),
        }],
        validity: vec![],
    }
}
fn evidence() -> RunEvidence {
    let scene = scenario();
    let captured = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "contract test, not execution".into(),
        },
        None,
    )
    .unwrap();
    let data = scene.seed.identity().unwrap();
    let session = scene.session.identity().unwrap();
    let binding = RunBinding {
        source: captured.binding.clone(),
        artifact: captured.artifact,
        data_digest: data.clone(),
        input_digest: scene.input_identity().unwrap(),
        scenario_digest: scene.identity().unwrap(),
        decision_digest: canonical_digest(IdentityDomain::Decision, &json!([])).unwrap(),
        session_digest: session.clone(),
        runtime_version: "runtime-1".into(),
        driver_version: "driver-1".into(),
    };
    RunEvidence {
        version: 1,
        id: "fixture_run".into(),
        origin: ExecutionOrigin::TestFixture,
        source_after: binding.source.clone(),
        binding,
        state: EvidenceState::Observed,
        trace: vec![TraceStep {
            index: 0,
            input: scene.inputs[0].clone(),
            before_data: data.clone(),
            after_data: data.clone(),
            before_session: session.clone(),
            after_session: session.clone(),
            outputs: vec![],
            outcome: StepOutcome::Applied,
            diagnostic: None,
        }],
        observations: vec![Observation {
            value_types: app().observable_types().unwrap(),
            point: "result".into(),
            data_digest: data,
            session_digest: session,
            values: std::collections::BTreeMap::from([(
                "selected_count".into(),
                DataValue::Integer { value: 0 },
            )]),
            view: empty_view(),
            view_schema: None,
            outputs: vec![],
        }],
        limits: RuntimeLimits::default(),
        errors: vec![],
        uncovered: vec![],
    }
}
#[test]
fn scenario_identity_covers_seed_session_clock_and_input_sequence() {
    let first = scenario();
    first.validate(&app()).unwrap();
    let digest = first.input_identity().unwrap();
    let mut changed = first.clone();
    changed.clock_day = 1;
    assert_ne!(digest, changed.input_identity().unwrap());
    changed = first.clone();
    changed
        .inputs
        .insert(0, SemanticInput::AdvanceClock { days: 1 });
    assert_ne!(digest, changed.input_identity().unwrap());
    changed = first;
    changed.session.values.insert(
        "search".into(),
        DataValue::Text {
            value: "West".into(),
        },
    );
    assert_ne!(digest, changed.input_identity().unwrap());
    changed.inputs.push(SemanticInput::Observe {
        point: "result".into(),
    });
    assert!(changed.validate(&app()).is_err());
}
#[test]
fn observed_receipts_reject_stale_sources_unbound_points_and_broken_state_chains() {
    let run = evidence();
    run.validate().unwrap();
    let mut stale = run.clone();
    stale.source_after.producer = Producer::UserAuthored;
    assert!(stale.validate().is_err());
    let mut wrong = run.clone();
    wrong.observations[0].point = "not_in_trace".into();
    assert!(wrong.validate().is_err());
    wrong = run.clone();
    wrong.observations[0].data_digest = canonical_digest(IdentityDomain::Data, &json!(3)).unwrap();
    assert!(wrong.validate().is_err());
    wrong = run;
    wrong.trace[0].outcome = StepOutcome::Failed;
    assert!(wrong.validate().is_err());
}
#[test]
fn identical_observations_cannot_be_a_demonstrated_differential_witness() {
    let before = evidence();
    let property = AcceptedProperty {
        id: "zero".into(),
        description: "Selected count is zero".into(),
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
    let mut witness = DifferentialWitness {
        version: 1,
        id: "witness".into(),
        scenario: scenario(),
        before: before.clone(),
        after: before,
        state: EvidenceState::Observed,
        distinguishing_properties: vec![property],
        minimization: None,
    };
    assert!(witness.validate().is_err());
    witness.after.observations[0]
        .values
        .insert("selected_count".into(), DataValue::Integer { value: 1 });
    witness.validate().unwrap();
}

#[test]
fn stored_snapshot_bound_matches_the_bounded_reader() {
    let a = app();
    let mut snapshot = DataSnapshot::empty("project", &a).unwrap();
    for i in 0..260 {
        snapshot.records.push(Record {
            entity: "person".into(),
            id: format!("row{i}"),
            revision: 1,
            created_program: a.identity().unwrap(),
            archived: false,
            values: std::collections::BTreeMap::from([
                (
                    "name".into(),
                    DataValue::Text {
                        value: "x".repeat(MAX_TEXT_BYTES),
                    },
                ),
                (
                    "area".into(),
                    DataValue::Text {
                        value: "West".into(),
                    },
                ),
            ]),
        });
    }
    assert!(snapshot.validate().is_err());
}

#[test]
fn unknown_artifact_versions_cannot_enter_evolution_plans() {
    let captured = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "test".into(),
        },
        None,
    )
    .unwrap();
    let mut proposal = EvolutionProposal {
        version: 1,
        id: "evolution".into(),
        request_digest: canonical_digest(IdentityDomain::Request, &json!(1)).unwrap(),
        candidate: captured.artifact,
        needs: vec!["need".into()],
        proposed_retirement: vec![],
        preserved_obligations: vec![],
        mappings: vec![],
        scenarios: vec![],
    };
    proposal.validate().unwrap();
    proposal.candidate.version = 2;
    assert!(proposal.validate().is_err());
}

fn witnessed_difference() -> DifferentialWitness {
    let before = evidence();
    let mut after = before.clone();
    after.observations[0]
        .values
        .insert("selected_count".into(), DataValue::Integer { value: 1 });
    DifferentialWitness {
        version: 1,
        id: "witness".into(),
        scenario: scenario(),
        before,
        after,
        state: EvidenceState::Observed,
        distinguishing_properties: vec![AcceptedProperty {
            id: "zero".into(),
            description: "Selection is empty".into(),
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
        }],
        minimization: None,
    }
}
#[test]
fn false_or_unknown_validity_obligations_invalidate_witnesses() {
    let mut witness = witnessed_difference();
    witness.validate().unwrap();
    witness.scenario.validity = vec![count_property()]; // Missing output is unknown.
    for run in [&mut witness.before, &mut witness.after] {
        run.binding.scenario_digest = witness.scenario.identity().unwrap();
    }
    assert!(witness.validate().is_err());
    witness.scenario.validity[0].predicate = PropertyPredicate::Equal {
        left: PropertyTerm::Literal {
            value_type: Type::Integer,
            value: DataValue::Integer { value: 0 },
        },
        right: PropertyTerm::Literal {
            value_type: Type::Integer,
            value: DataValue::Integer { value: 1 },
        },
    };
    for run in [&mut witness.before, &mut witness.after] {
        run.binding.scenario_digest = witness.scenario.identity().unwrap();
    }
    assert!(witness.validate().is_err());
}
#[test]
fn missing_exclusion_identity_and_relevant_unknown_boundaries_stay_unknown() {
    let mut scope = DecisionScope {
        operations: std::collections::BTreeSet::from(["export_people".into()]),
        population: Population::All,
        conditions: Default::default(),
        excluded_records: vec![RecordRef {
            entity: "person".into(),
            record: "one".into(),
        }],
        unknowns: vec![],
    };
    let mut context = ScopeContext {
        operation: "export_people".into(),
        record: None,
        is_new: None,
        created_generation: None,
        attributes: Default::default(),
        predicate_result: None,
    };
    assert_eq!(scope.matches(&context), ScopeMatch::Unknown);
    scope.excluded_records.clear();
    scope.unknowns.push(UnknownBoundary {
        id: "undecided".into(),
        operations: scope.operations.clone(),
        description: "This operation's boundary remains undecided".into(),
    });
    assert_eq!(scope.matches(&context), ScopeMatch::Unknown);
    context.operation = "delete".into();
    assert_eq!(scope.matches(&context), ScopeMatch::Outside);
}
#[test]
fn artifact_receipts_bind_output_identity_not_only_bytes() {
    let mut output = LocalArtifact::from_rows(
        "roster",
        OutputFormat::Json,
        text_columns(&["name"]),
        vec![],
    )
    .unwrap();
    output.output = "different_business_output".into();
    assert!(output.validate().is_err());
}
#[test]
fn nested_inputs_are_bounded_in_receipts_and_development_context() {
    let mut run = evidence();
    run.state = EvidenceState::Failed;
    run.trace[0].input = SemanticInput::AdvanceClock { days: u32::MAX };
    run.observations.clear();
    assert!(run.validate().is_err());
    let request = DevelopmentRequest {
        version: 1,
        id: "request".into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Generate,
        request: "Tool".into(),
        sources: vec![],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![SemanticInput::AdvanceClock { days: u32::MAX }],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    assert!(request.validate().is_err());
}
#[test]
fn navigation_and_clock_receipts_cannot_mutate_durable_data_or_emit() {
    for input in [
        SemanticInput::Navigate {
            view: "people".into(),
        },
        SemanticInput::AdvanceClock { days: 1 },
    ] {
        let mut run = evidence();
        run.state = EvidenceState::Failed;
        run.observations.clear();
        run.trace[0].input = input;
        run.trace[0].after_data = canonical_digest(IdentityDomain::Data, &json!(999)).unwrap();
        assert!(run.validate().is_err());
    }
    let mut run = evidence();
    run.state = EvidenceState::Failed;
    run.observations.clear();
    run.trace[0].input = SemanticInput::AdvanceClock { days: 1 };
    run.trace[0].after_session = canonical_digest(IdentityDomain::Session, &json!(999)).unwrap();
    assert!(run.validate().is_err());
}
#[test]
fn evidence_checks_selected_runtime_collection_and_output_limits() {
    let mut run = evidence();
    run.limits.collection_items = 1;
    run.observations[0]
        .value_types
        .insert("items".into(), Type::list(Type::Integer));
    run.observations[0].values.insert(
        "items".into(),
        DataValue::List {
            item_type: Type::Integer,
            items: vec![
                DataValue::Integer { value: 1 },
                DataValue::Integer { value: 2 },
            ],
        },
    );
    assert!(run.validate().is_err());
    let mut run = evidence();
    let output =
        LocalArtifact::from_rows("roster", OutputFormat::Csv, text_columns(&["name"]), vec![])
            .unwrap();
    let mut emit = run.trace[0].clone();
    emit.input = SemanticInput::Invoke {
        action: "export_people".into(),
        arguments: Default::default(),
    };
    emit.outputs = vec![output.digest.clone()];
    run.trace[0].index = 1;
    run.trace.insert(0, emit);
    run.observations[0].outputs = vec![output];
    run.limits.output_bytes = 1;
    assert!(run.validate().is_err());
}
#[test]
fn evolution_mapping_cannot_embed_an_invalid_replacement_scenario() {
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "test".into(),
        },
        None,
    )
    .unwrap();
    let mut replacement = scenario();
    replacement.version = 2;
    let mut proposal = EvolutionProposal {
        version: 1,
        id: "new_design".into(),
        request_digest: canonical_digest(IdentityDomain::Request, &json!(1)).unwrap(),
        candidate: capture.artifact,
        needs: vec!["need".into()],
        proposed_retirement: vec![],
        preserved_obligations: vec![],
        mappings: vec![],
        scenarios: vec![ScenarioMapping {
            original: scenario().identity().unwrap(),
            source_program: None,
            replacement,
            explanation: "Retain the original need".into(),
        }],
    };
    assert!(proposal.validate().is_err());
    proposal.scenarios[0].replacement.version = 1;
    proposal.scenarios.push(proposal.scenarios[0].clone());
    assert!(proposal.validate().is_err());
}
#[test]
fn compatibility_metadata_is_strict_and_bounded() {
    let digest = canonical_digest(IdentityDomain::Data, &json!(1)).unwrap();
    let mut report = CompatibilityReport {
        state: CompatibilityState::Compatible,
        current_data: digest.clone(),
        target_program: digest,
        retained_records: vec![],
        retained_events: vec!["bad event ID".into()],
        retained_fields: vec![],
        issues: vec![],
    };
    assert!(report.validate().is_err());
    report.retained_events.clear();
    report.retained_fields.push(SemanticKey {
        kind: SemanticKind::Field,
        entity: None,
        id: "name".into(),
    });
    assert!(report.validate().is_err());
    report.retained_fields.clear();
    report.issues = vec!["x".repeat(MAX_TEXT_BYTES + 1)];
    assert!(report.validate().is_err());
    report.issues.clear();
    report.retained_fields = vec![SemanticKey {
        kind: SemanticKind::Action,
        entity: None,
        id: "action".into(),
    }];
    assert!(report.validate().is_err());
}
#[test]
fn optional_collections_cannot_masquerade_as_scalar_controls() {
    let mut app = fixture::organizer();
    app["state"][0]["value_type"] =
        json!({"kind":"optional","item":{"kind":"list","item":{"kind":"text"}}});
    app["state"][0]["initial"] = json!({"kind":"null"});
    assert!(parse(app).is_err());
}
#[test]
fn csv_headers_are_literal_text_even_when_ids_start_with_minus() {
    let output =
        LocalArtifact::from_rows("output", OutputFormat::Csv, text_columns(&["-1-1"]), vec![])
            .unwrap();
    assert_eq!(output.bytes, b"'-1-1\r\n");
}
#[test]
fn development_result_validates_producer_metadata() {
    let request = DevelopmentRequest {
        version: 1,
        id: "request".into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Generate,
        request: "Tool".into(),
        sources: vec![],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    let result = DevelopmentResult {
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec!["No supported capability".into()],
        },
        producer: Producer::LiveAgent {
            provider: String::new(),
            invocation_id: String::new(),
            session_id: None,
            request_digest: request.identity().unwrap(),
        },
    };
    assert!(result.validate_for(&request).is_err());
}

#[test]
fn incomplete_runs_cannot_claim_no_difference() {
    let mut run = evidence();
    run.state = EvidenceState::NoDifferenceFound;
    run.trace[0].outcome = StepOutcome::BudgetExhausted;
    run.observations.clear();
    assert!(run.validate().is_err());
    let mut witness = witnessed_difference();
    witness.state = EvidenceState::NoDifferenceFound;
    witness.before = run;
    witness.before.state = EvidenceState::Inconclusive;
    assert!(witness.validate().is_err());
}
#[test]
fn empty_lists_count_the_outer_level_in_type_depth() {
    let mut item = Type::Text;
    for _ in 0..MAX_TYPE_DEPTH {
        item = Type::list(item);
    }
    let input = SemanticInput::Invoke {
        action: "action".into(),
        arguments: std::collections::BTreeMap::from([(
            "value".into(),
            DataValue::List {
                item_type: item,
                items: vec![],
            },
        )]),
    };
    assert!(validate_input_shape(&input).is_err());
}
#[test]
fn reduction_operation_descriptions_are_nonempty_and_bounded() {
    for description in [String::new(), "x".repeat(MAX_TEXT_BYTES + 1)] {
        let mut witness = witnessed_difference();
        let digest = witness.scenario.identity().unwrap();
        witness.minimization = Some(MinimalityCertificate {
            initial: digest.clone(),
            final_scenario: digest,
            deletion_operations: vec![description],
            trials: vec![],
            final_single_deletions: vec![],
            complete: false,
        });
        assert!(witness.validate().is_err());
    }
}
#[test]
fn csv_output_accepts_its_exact_inclusive_byte_ceiling() {
    let mut rows = vec![
        std::collections::BTreeMap::from([(
            "a".into(),
            DataValue::Text {
                value: "x".repeat(4096)
            }
        )]);
        255
    ];
    rows.push(std::collections::BTreeMap::from([(
        "a".into(),
        DataValue::Text {
            value: "x".repeat(3581),
        },
    )]));
    let output =
        LocalArtifact::from_rows("output", OutputFormat::Csv, text_columns(&["a"]), rows).unwrap();
    assert_eq!(output.bytes.len(), MAX_OUTPUT_BYTES);
    output.validate().unwrap();
}
#[test]
fn provider_evolution_suggestions_carry_typed_design_changes_without_evidence_authority() {
    let source = serde_json::to_string(&fixture::new_composition()).unwrap();
    let value = json!({"version":1,"request_digest":"0".repeat(64),"candidates":[{"id":"candidate","source_json":source}],"hypotheses":[],"evolutions":[{"id":"third_design","candidate":"candidate","needs":["need"],"proposed_retirement":["old_rule"],"preserved_obligations":[],"mappings":[{"from":{"kind":"action","entity":null,"id":"collect"},"to":{"kind":"action","entity":null,"id":"save_collection"}}],"scenarios":[{"original":scenario().identity().unwrap(),"replacement_json":serde_json::to_string(&scenario()).unwrap(),"explanation":"A scene for the new explicit action"}]}],"unsupported":[]});
    DevelopmentResponse::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    let mut invalid = value.clone();
    invalid["evolutions"][0]["candidate"] = json!("missing");
    assert!(DevelopmentResponse::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    invalid = value;
    invalid["evolutions"][0]["passed"] = json!(true);
    assert!(DevelopmentResponse::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
}

#[test]
fn reconciliation_results_bind_known_needs_mappings_and_replacement_scenes() {
    let scene = scenario();
    let source = CapturedProgram::capture(
        &serde_json::to_vec(&fixture::organizer()).unwrap(),
        "project",
        Producer::Fixture {
            name: "reconciliation contract fixture".into(),
        },
        None,
    )
    .unwrap();
    let decision = ScopedDecision {
        id: "old_rule".into(),
        revision: 1,
        request: "Preserve useful selection behavior".into(),
        rationale: None,
        scope: DecisionScope {
            operations: std::collections::BTreeSet::from(["collect".into()]),
            population: Population::All,
            conditions: Default::default(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::Accept {
            artifact: source.artifact.program_digest.clone(),
        },
        status: DecisionStatus::Active,
        obligations: vec![count_property()],
        scenarios: vec![scene.identity().unwrap()],
        witness: canonical_digest(IdentityDomain::Evidence, &json!(1)).unwrap(),
        supersedes: vec![],
    };
    let request = DevelopmentRequest {
        version: 1,
        id: "reconcile".into(),
        project_id: "project".into(),
        operation: DevelopmentOperation::Reconcile,
        request: "Add an explicit collection action".into(),
        sources: vec![source],
        context: DevelopmentContext {
            view: None,
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![SelectedScenario {
            disclosure: Disclosure::Synthetic,
            scenario: scene.clone(),
        }],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 1,
            decisions: vec![decision],
        },
        unknowns: vec![],
        required_capabilities: Default::default(),
    };
    let value = json!({"version":1,"request_digest":request.identity().unwrap(),"candidates":[{"id":"candidate","source_json":serde_json::to_string(&fixture::new_composition()).unwrap()}],"hypotheses":[],"evolutions":[{"id":"third_design","candidate":"candidate","needs":["old_rule"],"proposed_retirement":["old_rule"],"preserved_obligations":[count_property().identity().unwrap()],"mappings":[{"from":{"kind":"action","entity":null,"id":"collect"},"to":{"kind":"action","entity":null,"id":"save_collection"}}],"scenarios":[{"original":scene.identity().unwrap(),"replacement_json":serde_json::to_string(&scene).unwrap(),"explanation":"Retain the accepted scene"}]}],"unsupported":[]});
    let response = DevelopmentResponse::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    response.validate_for(&request).unwrap();
    let mut invalid = response.clone();
    invalid.evolutions[0].needs = vec!["invented_need".into()];
    assert!(invalid.validate_for(&request).is_err());
    invalid = response.clone();
    invalid.evolutions[0].mappings[0].to.id = "missing_action".into();
    assert!(invalid.validate_for(&request).is_err());
    invalid = response.clone();
    invalid.evolutions[0].proposed_retirement = vec!["unrelated_rule".into()];
    assert!(invalid.validate_for(&request).is_err());
    invalid = response;
    invalid.evolutions[0].scenarios[0].original =
        canonical_digest(IdentityDomain::Scenario, &json!(999)).unwrap();
    assert!(invalid.validate_for(&request).is_err());
}

#[test]
fn scoped_entities_missing_from_replacement_are_unmapped_not_outside() {
    let scope = DecisionScope {
        operations: std::collections::BTreeSet::from(["collect".into()]),
        population: Population::Entity {
            entity: "missing".into(),
        },
        conditions: Default::default(),
        excluded_records: vec![],
        unknowns: vec![],
    };
    assert!(scope.validate_for(&app()).is_err());
}

#[test]
fn empty_output_columns_preserve_declared_types_for_portable_properties() {
    let output = LocalArtifact::from_rows(
        "roster",
        OutputFormat::Json,
        vec![FieldDefinition {
            id: "name".into(),
            label: "Name".into(),
            value_type: Type::Integer,
        }],
        vec![],
    )
    .unwrap();
    let mut observation = evidence().observations.remove(0);
    observation.outputs = vec![output];
    let mut property = AcceptedProperty {
        id: "empty_names".into(),
        description: "There are no exported names".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::OutputColumn {
                point: "result".into(),
                output: "roster".into(),
                column: "name".into(),
                value_type: Type::Text,
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::Text),
                value: DataValue::List {
                    item_type: Type::Text,
                    items: vec![],
                },
            },
        },
    };
    assert_eq!(property.evaluate(&[observation.clone()]), None);
    property.predicate = PropertyPredicate::Equal {
        left: PropertyTerm::OutputColumn {
            point: "result".into(),
            output: "roster".into(),
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
    assert_eq!(property.evaluate(&[observation]), Some(true));
}
#[test]
fn incomplete_witnesses_retain_an_exact_executed_prefix() {
    let mut witness = witnessed_difference();
    witness.state = EvidenceState::Inconclusive;
    witness.scenario.inputs.push(SemanticInput::Observe {
        point: "later".into(),
    });
    for run in [&mut witness.before, &mut witness.after] {
        run.binding.scenario_digest = witness.scenario.identity().unwrap();
        run.binding.input_digest = witness.scenario.input_identity().unwrap();
    }
    let mut last = witness.before.trace[0].clone();
    last.index = 1;
    last.input = witness.scenario.inputs[1].clone();
    witness.before.trace.push(last);
    let mut last = witness.before.observations[0].clone();
    last.point = "later".into();
    witness.before.observations.push(last);
    witness.after.state = EvidenceState::Inconclusive;
    witness.after.errors = vec!["Cancelled before second input".into()];
    witness.validate().unwrap();
    witness.after.trace[0].input = SemanticInput::Navigate {
        view: "people".into(),
    };
    witness.after.observations.clear();
    assert!(witness.validate().is_err());
}

fn text_columns(ids: &[&str]) -> Vec<FieldDefinition> {
    ids.iter()
        .map(|id| FieldDefinition {
            id: (*id).into(),
            label: (*id).into(),
            value_type: Type::Text,
        })
        .collect()
}

#[test]
fn null_observable_values_do_not_erase_declared_types() {
    let mut observation = evidence().observations.remove(0);
    observation
        .values
        .insert("selected_count".into(), DataValue::Null);
    let optional_text = Type::Optional {
        item: Box::new(Type::Text),
    };
    let property = AcceptedProperty {
        id: "missing_value".into(),
        description: "No value yet".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "result".into(),
                observable: "selected_count".into(),
                value_type: optional_text.clone(),
            },
            right: PropertyTerm::Literal {
                value_type: optional_text,
                value: DataValue::Null,
            },
        },
    };
    observation.value_types.insert(
        "selected_count".into(),
        Type::Optional {
            item: Box::new(Type::Integer),
        },
    );
    let integer_identity = observation.identity().unwrap();
    assert_eq!(property.evaluate(&[observation.clone()]), None);
    observation.value_types.insert(
        "selected_count".into(),
        Type::Optional {
            item: Box::new(Type::Text),
        },
    );
    assert_ne!(integer_identity, observation.identity().unwrap());
    assert_eq!(property.evaluate(&[observation]), Some(true));
}

#[test]
fn independently_evaluated_properties_reject_invalid_observation_receipts() {
    let mut observation = evidence().observations.remove(0);
    observation
        .values
        .insert("undeclared".into(), DataValue::Integer { value: 9 });
    assert!(observation.validate().is_err());
    let property = AcceptedProperty {
        id: "zero".into(),
        description: "Selection is empty".into(),
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
    assert_eq!(property.evaluate(&[observation]), None);
}
