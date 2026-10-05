//! Recorded/fake provider responses exercise components, never live AI acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
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
use fixture::*;
use product_contract::*;
use product_discovery::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{atomic::AtomicBool, Arc},
};

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
fn run(r: &DevelopmentRequest, out: &DevelopmentResult, p: DiscoveryPolicy) -> DiscoveryReport {
    discover(r, out, &p, Arc::new(AtomicBool::new(false))).unwrap()
}
fn refresh(r: &DevelopmentRequest, out: &mut DevelopmentResult) {
    out.response.request_digest = r.identity().unwrap();
}

#[test]
fn source_edits_drive_executed_grouped_questions_without_answer_input() {
    let (r, mut output) = input();
    let mut duplicate = output.response.hypotheses[0].clone();
    duplicate.id = "same-choice".into();
    duplicate.statement = "A second model wording".into();
    output.response.hypotheses.push(duplicate);
    let report = run(&r, &output, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert_eq!(report.log.len(), 2);
    assert_eq!(report.questions[0].hypotheses.len(), 2);
    let w = report.questions[0].witnesses[0].witness();
    assert_eq!(w.before.origin, ExecutionOrigin::ProductionRuntime);
    assert_eq!(w.after.binding.artifact, r.sources[1].artifact);
    assert_ne!(
        w.before.observations[0].values,
        w.after.observations[0].values
    );
    assert!(w.minimization.as_ref().unwrap().complete);
    assert!(matches!(
        report.questions[0].producer,
        Producer::Fixture { .. }
    ));
    assert_eq!(
        r.examples.len(),
        0,
        "no answer scene was given to discovery request"
    );
}

#[test]
fn equivalent_source_and_irrelevant_changed_actions_remain_quiet() {
    let (mut r, mut output) = input();
    let mut same = r.sources[0].program.clone();
    same.label = "New title".into();
    r.sources[1] = CapturedProgram::capture(
        &serde_json::to_vec_pretty(&same).unwrap(),
        "runtime-project",
        Producer::ExternalAuthor {
            description: "format and label edit".into(),
        },
        None,
    )
    .unwrap();
    refresh(&r, &mut output);
    output.response.hypotheses.clear();
    output.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    let report = run(&r, &output, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(report.delta.behavioral_loci.is_empty());
    let (mut r, mut output) = input();
    let mut v = filtered();
    v["actions"][0]["steps"][0]["values"]["area"] = text("south");
    r.sources[1] = capture(v);
    refresh(&r, &mut output);
    output.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    output.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    output.response.hypotheses[0].sources[0].pointer = "/actions/0".into();
    let report = run(&r, &output, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
}

#[test]
fn forged_or_stale_loci_and_unbound_candidate_are_rejected() {
    let (r, out) = input();
    for edit in 0..3 {
        let mut out = out.clone();
        match edit {
            0 => out.response.hypotheses[0].sources[0].pointer = "/does-not-exist".into(),
            1 => {
                out.response.hypotheses[0].sources[0].raw_digest =
                    canonical_digest(IdentityDomain::Source, &"uncaptured stale source").unwrap()
            }
            _ => {
                out.response.candidates[1].source_json =
                    out.response.candidates[0].source_json.clone()
            }
        };
        let result = discover(
            &r,
            &out,
            &DiscoveryPolicy::default(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(result.is_err() || result.unwrap().questions.is_empty());
    }
}

fn saved_decision(
    r: &mut DevelopmentRequest,
    out: &DevelopmentResult,
    outcome: DecisionOutcome,
    obligations: Vec<AcceptedProperty>,
) -> DifferentialWitness {
    let scene = out.response.hypotheses[0].scenario().unwrap();
    let observed = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .compare(
            &r.sources[0],
            &r.sources[1],
            &scene,
            &r.decisions,
            product_scenarios::ObservationTarget::Observable {
                point: "done".into(),
                observable: "selected_count".into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap()
        .witness
        .unwrap()
        .witness()
        .clone();
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
fn count_is(n: i64) -> AcceptedProperty {
    AcceptedProperty {
        id: "approved-count".into(),
        description: "Keep the observed collection".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: n },
            },
        },
    }
}

#[test]
fn explicit_requirement_violations_are_defects_never_preference_questions() {
    let (mut r, mut out) = input();
    saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.defects.is_empty());
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Violated));
    assert_eq!(report.checks[0].binding.artifact, r.sources[1].artifact);
}

#[test]
fn satisfied_settled_and_deferred_scenes_do_not_reask() {
    for outcome in [DecisionOutcome::EitherAcceptable, DecisionOutcome::Deferred] {
        let (mut r, mut out) = input();
        let prior = saved_decision(&mut r, &out, outcome, vec![]);
        refresh(&r, &mut out);
        let report = run(
            &r,
            &out,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(report.questions.is_empty());
        assert!(report
            .log
            .iter()
            .any(|e| e.disposition == Disposition::Settled));
    }
    let (mut r, mut out) = input();
    let artifact = r.sources[1].artifact.program_digest.clone();
    saved_decision(
        &mut r,
        &out,
        DecisionOutcome::Accept { artifact },
        vec![count_is(1)],
    );
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(report.defects.is_empty());
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
}

#[test]
fn missing_decision_scene_is_unknown_and_unrelated_scope_does_not_block() {
    let (mut r, mut out) = input();
    saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    r.examples.clear();
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    r.examples.push(SelectedScenario {
        disclosure: Disclosure::Synthetic,
        scenario: out.response.hypotheses[0].scenario().unwrap(),
    });
    r.decisions.decisions[0].scope.operations = ["add_person".into()].into_iter().collect();
    r.decisions.decisions[0].obligations = vec![AcceptedProperty {
        id: "same-view".into(),
        description: "The entered visible name is retained".into(),
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
                    items: vec![string("Ada")],
                },
            },
        },
    }];
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
}

#[test]
fn missing_new_feature_is_not_a_competing_implementation() {
    let (r, out) = input();
    let mut policy = DiscoveryPolicy::default();
    policy
        .required_actions
        .insert("new_requested_action".into());
    let report = run(&r, &out, policy);
    assert!(report.questions.is_empty());
    assert!(!report.defects.is_empty());
}

#[test]
fn provider_labels_do_not_override_host_requirement_checks() {
    let (mut r, mut out) = input();
    saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    refresh(&r, &mut out);
    out.response.hypotheses[0].kind = HypothesisKind::RequestedChange;
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.defects.is_empty());
}

#[test]
fn domain_transport_projection_binds_exact_source_and_cancellation() {
    let (r, _) = input();
    let options = ProviderOptions::default();
    let wire = encode_request(&r, &options).unwrap();
    assert_eq!(
        wire.source_digest,
        r.sources[1].binding.identity().unwrap().as_str()
    );
    let prompt: Value = serde_json::from_slice(&wire.prompt).unwrap();
    assert_eq!(prompt["request"], serde_json::to_value(&r).unwrap());
    assert!(!prompt["instructions"].as_str().unwrap().contains("contact"));
    assert_eq!(wire.schema, RESPONSE_SCHEMA.as_bytes());
    let mut changed = r.clone();
    changed.request.push('!');
    assert_ne!(
        wire.digest().unwrap(),
        encode_request(&changed, &options)
            .unwrap()
            .digest()
            .unwrap()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn consented_fake_cli_bridge_preserves_origin_and_rejects_mismatches() {
    use product_provider::{unix_ms, ConsentReceipt, ProviderKind, ProviderTransport};
    use std::{fs, os::unix::fs::PermissionsExt};
    let (r, out) = input();
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let bin = root.join("fixture.py");
    {
        let _guard = product_provider::fixture_executable_write_guard();
        let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
            "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
            "cfg['response']",
        );
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            bin.with_extension("json"),
            serde_json::to_vec(&json!({"response":out.response})).unwrap(),
        )
        .unwrap();
    }
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    let transport =
        ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home).unwrap();
    let prepared = prepare_development(transport, &r, ProviderOptions::default()).unwrap();
    let consent = ConsentReceipt {
        disclosure_digest: prepared.disclosure().digest(),
        approval_reference: "synthetic fixture only".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let bridge = prepared.authorize(consent);
    let mut wrong = r.clone();
    wrong.request.push('!');
    assert!(bridge.develop(&wrong, &|| false).is_err());
    assert!(matches!(
        bridge.develop(&r, &|| true),
        Err(AdapterError::Cancelled)
    ));
    let result = bridge.develop(&r, &|| false).unwrap();
    assert!(matches!(result.producer, Producer::Fixture { .. }));
    result.validate_for(&r).unwrap();
    assert_eq!(
        run(&r, &result, DiscoveryPolicy::default()).questions.len(),
        1
    );
}

#[test]
fn equivalent_refactor_and_failed_explicit_checks_are_quiet() {
    let (mut r, mut out) = input();
    let mut v = filtered();
    let predicate = v["actions"][1]["steps"][0]["items"]["predicate"].clone();
    v["actions"][1]["steps"][0]["items"]["predicate"] =
        json!({"kind":"and","values":[yes(),predicate]});
    r.sources[1] = capture(v);
    out.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.runs.is_empty());
    assert!(report
        .log
        .iter()
        .all(|e| e.disposition == Disposition::NoWitness));
    let (r, out) = input();
    let mut scene = out.response.hypotheses[0].scenario().unwrap();
    scene.inputs.clear();
    let policy = DiscoveryPolicy {
        requirements: vec![RequirementCase {
            id: "required-work".into(),
            scenario: scene,
            properties: vec![count_is(2)],
        }],
        ..DiscoveryPolicy::default()
    };
    let report = run(&r, &out, policy);
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
}

#[test]
fn actual_view_only_change_is_discovered_without_changed_named_observable() {
    let (mut r, mut out) = input();
    let mut v = filtered();
    v["views"][0]["kind"]["rows"]["sort"][0]["descending"] = json!(true);
    r.sources[1] = capture(v);
    out.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    let h = &mut out.response.hypotheses[0];
    h.sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    h.sources[0].pointer = "/views/0/kind/rows/sort".into();
    let scene = scenario(
        &r.sources[0],
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    h.scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    let w = report.questions[0].witnesses[0].witness();
    assert_ne!(
        w.before.observations[0].view.rows,
        w.after.observations[0].view.rows
    );
}

#[test]
fn changed_pending_outcomes_reopen_but_unavailable_saved_proof_stays_unknown() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::Deferred, vec![]);
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    let mut v = filtered();
    v["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}});
    r.sources[1] = capture(v);
    out.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
}

#[test]
fn all_saved_obligations_are_rechecked_even_if_provider_mentions_none() {
    let (mut r, mut out) = input();
    saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    r.context.recent_inputs.clear();
    out.response.hypotheses.clear();
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(!report.defects.is_empty());
    assert!(report.questions.is_empty());
    assert!(!report.checks.is_empty());
}

#[test]
fn unsupported_capabilities_budget_and_provider_oracles_cannot_create_proof() {
    let (mut r, mut out) = input();
    r.required_capabilities.insert("network-access".into());
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    let (r, mut out) = input();
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            max_comparisons: 0,
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(report
        .log
        .iter()
        .any(|e| e.state == EvidenceState::Inconclusive));
    let mut scene = out.response.hypotheses[0].scenario().unwrap();
    scene.validity = vec![count_is(100)];
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert!(report.questions[0].witnesses[0]
        .witness()
        .scenario
        .validity
        .is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn bridge_rejects_correlated_and_domain_mismatches_and_cancels_running_fixture() {
    use product_provider::{unix_ms, ConsentReceipt, ProviderKind, ProviderTransport};
    use std::{
        cell::Cell,
        fs,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    for mode in ["wrong_digest", "wrong_source", "domain_wrong", "slow"] {
        let (request, mut result) = input();
        if mode == "domain_wrong" {
            result.response.request_digest =
                canonical_digest(IdentityDomain::Request, &"other request").unwrap();
        }
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let bin = root.join("fake.py");
        {
            let _guard = product_provider::fixture_executable_write_guard();
            let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
                "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
                "cfg['response']",
            );
            fs::write(&bin, script).unwrap();
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(
                bin.with_extension("json"),
                serde_json::to_vec(&json!({"mode":mode,"response":result.response})).unwrap(),
            )
            .unwrap();
        }
        let home = root.join("home");
        fs::create_dir(&home).unwrap();
        let transport =
            ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home)
                .unwrap();
        let prepared =
            prepare_development(transport, &request, ProviderOptions::default()).unwrap();
        let consent = ConsentReceipt {
            disclosure_digest: prepared.disclosure().digest(),
            approval_reference: "fixture-only".into(),
            expires_at_unix_ms: unix_ms() + 60_000,
        };
        let bridge = prepared.authorize(consent);
        let checks = Cell::new(0);
        let started = Instant::now();
        let actual = bridge.develop(&request, &|| {
            checks.set(checks.get() + 1);
            mode == "slow" && checks.get() > 5
        });
        assert!(actual.is_err(), "{mode}");
        if mode == "slow" {
            assert!(matches!(actual, Err(AdapterError::Cancelled)));
            assert!(started.elapsed() < Duration::from_secs(5));
        }
        assert!(
            bridge.develop(&request, &|| false).is_err(),
            "consumed jobs are never silently resent"
        );
    }
}

#[test]
fn unrelated_approved_obligation_rejects_whole_program_alternative() {
    let (mut r, mut out) = input();
    let mut alt = filtered();
    alt["actions"][0]["steps"][0]["values"]["name"] = text("Wrong");
    out.response.candidates[0].source_json = serde_json::to_string(&alt).unwrap();
    let scene = scenario(
        &r.sources[0],
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let property = AcceptedProperty {
        id: "keep-name".into(),
        description: "Retain the entered name".into(),
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
                    items: vec![string("Ada")],
                },
            },
        },
    };
    r.examples.push(SelectedScenario {
        disclosure: Disclosure::Synthetic,
        scenario: scene.clone(),
    });
    r.decisions.decisions.push(ScopedDecision {
        id: "name-intent".into(),
        revision: 1,
        request: "Use the name I enter".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["add_person".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        status: DecisionStatus::Active,
        obligations: vec![property],
        scenarios: vec![scene.identity().unwrap()],
        witness: canonical_digest(IdentityDomain::Evidence, &"approved name").unwrap(),
        supersedes: vec![],
    });
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.defects.is_empty());
    assert!(report.questions.is_empty());
}

#[test]
fn equivalent_edit_cannot_be_used_to_invent_an_alternative_choice() {
    let (mut r, mut out) = input();
    let mut equivalent = filtered();
    let predicate = equivalent["actions"][1]["steps"][0]["items"]["predicate"].clone();
    equivalent["actions"][1]["steps"][0]["items"]["predicate"] =
        json!({"kind":"and","values":[yes(),predicate]});
    r.sources[1] = capture(equivalent);
    out.response.candidates[0].source_json = out.response.candidates[1].source_json.clone();
    out.response.candidates[1].source_json =
        String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    refresh(&r, &mut out);
    let report = run(&r, &out, DiscoveryPolicy::default());
    assert!(report.questions.is_empty());
    assert!(report
        .log
        .iter()
        .all(|l| l.disposition == Disposition::NoWitness));
}

#[test]
fn renamed_scenes_stay_settled_but_a_third_outcome_is_not_hidden() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::EitherAcceptable, vec![]);
    let mut scene = out.response.hypotheses[0].scenario().unwrap();
    scene.id = "renamed".into();
    scene.label = "Different caption".into();
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let policy = DiscoveryPolicy {
        retained_witnesses: vec![prior],
        ..DiscoveryPolicy::default()
    };
    let report = run(&r, &out, policy.clone());
    assert!(report.questions.is_empty());
    let mut third = filtered();
    third["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}});
    out.response.candidates.push(GeneratedCandidate {
        id: "third".into(),
        source_json: serde_json::to_string(&third).unwrap(),
    });
    out.response.hypotheses[0].alternatives.push("third".into());
    let report = run(&r, &out, policy);
    assert_eq!(report.questions.len(), 1);
    let third = AppDefinition::parse(
        out.response
            .candidates
            .iter()
            .find(|c| c.id == "third")
            .unwrap()
            .source_json
            .as_bytes(),
    )
    .unwrap()
    .identity()
    .unwrap();
    assert!(report.questions[0].witnesses.iter().all(|w| w
        .before_program()
        .artifact
        .program_digest
        == third));
}

#[test]
fn unavailable_retained_scene_is_not_a_new_verified_choice() {
    let (mut r, mut out) = input();
    saved_decision(&mut r, &out, DecisionOutcome::Deferred, vec![]);
    let mut prior_scene = out.response.hypotheses[0].scenario().unwrap();
    prior_scene.inputs.insert(0, add("Blocked"));
    let prior = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .compare(
            &r.sources[0],
            &r.sources[1],
            &prior_scene,
            &r.decisions,
            product_scenarios::ObservationTarget::Observable {
                point: "done".into(),
                observable: "selected_count".into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap()
        .witness
        .unwrap()
        .witness()
        .clone();
    r.decisions.decisions[0].witness = prior.identity().unwrap();
    let mut alt = filtered();
    alt["actions"][0]["guards"] =
        json!([{"kind":"not","value":{"kind":"equal","left":var("name"),"right":text("Blocked")}}]);
    out.response.candidates[0].source_json = serde_json::to_string(&alt).unwrap();
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert!(report
        .runs
        .iter()
        .any(|r| r.id == "retained-before" && r.state == EvidenceState::Failed));
}

#[test]
fn genuinely_new_feature_compares_two_complete_implementations() {
    let (mut r, mut out) = input();
    let mut old = filtered();
    old["actions"].as_array_mut().unwrap().remove(2);
    old["views"][0]["actions"] = json!([]);
    old["views"][0]["keys"] = json!([]);
    r.sources[0] = capture(old);
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            required_actions: ["export_people".into()].into_iter().collect(),
            requirements: vec![export_requirement(&out)],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    let witness = &report.questions[0].witnesses[0];
    for p in [witness.before_program(), witness.after_program()] {
        assert!(p.program.actions.iter().any(|a| a.id == "export_people"));
    }
    assert!(!r.sources[0]
        .program
        .actions
        .iter()
        .any(|a| a.id == "export_people"));
}

#[test]
fn precheck_budget_and_cancellation_stop_before_replaying_unbounded_obligations() {
    let (r, out) = input();
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            max_precheck_replays: 0,
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert!(report.runs.is_empty());
    let mut policy = DiscoveryPolicy::default();
    for i in 0..256 {
        policy.requirements.push(RequirementCase {
            id: format!("case-{i}"),
            scenario: out.response.hypotheses[0].scenario().unwrap(),
            properties: vec![count_is(1)],
        });
    }
    let started = std::time::Instant::now();
    let report = discover(&r, &out, &policy, Arc::new(AtomicBool::new(true))).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert!(report.runs.is_empty());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
}

#[test]
fn concrete_accepted_outcomes_are_enforced_without_predicate_obligations() {
    let (mut r, mut out) = input();
    let accepted = r.sources[0].artifact.program_digest.clone();
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::Accept { artifact: accepted },
        vec![],
    );
    refresh(&r, &mut out);
    let policy = DiscoveryPolicy {
        retained_witnesses: vec![prior],
        ..DiscoveryPolicy::default()
    };
    for omit in [false, true] {
        let mut out = out.clone();
        if omit {
            out.response.hypotheses.clear();
        }
        let report = run(&r, &out, policy.clone());
        assert!(report.questions.is_empty());
        assert!(!report.defects.is_empty());
        assert!(report
            .checks
            .iter()
            .any(|c| c.state == CheckState::Violated));
    }
    let (mut r, mut out) = input();
    let accepted = r.sources[1].artifact.program_digest.clone();
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::Accept { artifact: accepted },
        vec![],
    );
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(report.defects.is_empty());
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
}

#[test]
fn unresolved_keep_current_side_is_unknown_until_controller_supplies_mapping() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::KeepCurrent, vec![]);
    refresh(&r, &mut out);
    let mut policy = DiscoveryPolicy {
        retained_witnesses: vec![prior],
        ..DiscoveryPolicy::default()
    };
    let report = run(&r, &out, policy.clone());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    policy
        .chosen_artifacts
        .insert("saved".into(), r.sources[0].artifact.program_digest.clone());
    let report = run(&r, &out, policy);
    assert!(report.questions.is_empty());
    assert!(!report.defects.is_empty());
}

#[test]
fn pending_predicates_are_not_active_and_either_acceptable_can_keep_invariants() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::Deferred, vec![count_is(2)]);
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.defects.is_empty());
    assert!(report.questions.is_empty());
    assert!(report
        .log
        .iter()
        .any(|l| l.disposition == Disposition::Settled));
    let (mut r, mut out) = input();
    let invariant = AcceptedProperty {
        id: "nonnegative".into(),
        description: "Selection counts are nonnegative".into(),
        predicate: PropertyPredicate::Not {
            value: Box::new(PropertyPredicate::Less {
                left: PropertyTerm::Observed {
                    point: "done".into(),
                    observable: "selected_count".into(),
                    value_type: Type::Integer,
                },
                right: PropertyTerm::Literal {
                    value_type: Type::Integer,
                    value: DataValue::Integer { value: 0 },
                },
            }),
        },
    };
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::EitherAcceptable,
        vec![invariant],
    );
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.defects.is_empty());
    assert!(report.questions.is_empty());
    assert!(report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
}

#[test]
fn inert_extra_steps_are_suppressed_by_reduction_evidence() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::Deferred, vec![]);
    let mut scene = out.response.hypotheses[0].scenario().unwrap();
    scene.inputs.insert(
        0,
        SemanticInput::Navigate {
            view: scene.session.view.clone(),
        },
    );
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(report
        .log
        .iter()
        .any(|l| l.disposition == Disposition::Settled));
}

#[test]
fn a_declared_noop_cannot_count_as_a_required_new_feature() {
    let (mut r, mut out) = input();
    let mut old = filtered();
    old["actions"].as_array_mut().unwrap().remove(2);
    old["views"][0]["actions"] = json!([]);
    old["views"][0]["keys"] = json!([]);
    r.sources[0] = capture(old);
    let mut noop = filtered();
    noop["actions"][2]["steps"] =
        json!([{"kind":"set_state","state":"search","value":{"kind":"state","state":"search"}}]);
    out.response.candidates[0].source_json = serde_json::to_string(&noop).unwrap();
    refresh(&r, &mut out);
    let mut policy = DiscoveryPolicy {
        required_actions: ["export_people".into()].into_iter().collect(),
        ..DiscoveryPolicy::default()
    };
    let report = run(&r, &out, policy.clone());
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    policy.requirements.push(export_requirement(&out));
    let report = run(&r, &out, policy);
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
}
fn export_requirement(out: &DevelopmentResult) -> RequirementCase {
    RequirementCase {
        id: "export-selected".into(),
        scenario: out.response.hypotheses[0].scenario().unwrap(),
        properties: vec![AcceptedProperty {
            id: "all-selected-rows".into(),
            description: "The export contains the selected rows".into(),
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
        }],
    }
}

#[test]
fn a_valid_witness_does_not_erase_unchecked_alternative_coverage() {
    let (r, mut out) = input();
    let mut third = filtered();
    third["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}});
    out.response.candidates.push(GeneratedCandidate {
        id: "third".into(),
        source_json: serde_json::to_string(&third).unwrap(),
    });
    out.response.hypotheses[0].alternatives.push("third".into());
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            max_comparisons: 1,
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    assert!(report
        .unverified
        .iter()
        .any(|s| s.contains("comparison budget")));
}

#[test]
fn reducing_a_condition_cannot_hide_an_original_third_outcome() {
    let (mut r, mut out) = input();
    let integer = |n| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}});
    let mut a = filtered();
    a["state"].as_array_mut().unwrap().extend([json!({"id":"mode","label":"Mode","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":false}}),json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}})]);
    a["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":integer(1)}]);
    a["observables"][0]["value"] = json!({"kind":"state","state":"result"});
    let mut b = a.clone();
    b["actions"][1]["steps"][0]["value"] = integer(0);
    r.sources = vec![capture(a.clone()), capture(b.clone())];
    out.response.candidates[0].source_json = serde_json::to_string(&a).unwrap();
    out.response.candidates[1].source_json = serde_json::to_string(&b).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    let scene = scenario(
        &r.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let prior = saved_decision(&mut r, &out, DecisionOutcome::EitherAcceptable, vec![]);
    let mut current = scene;
    current
        .session
        .values
        .insert("mode".into(), DataValue::Boolean { value: true });
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&current).unwrap();
    let mut c = a;
    c["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"state","state":"mode"},"then_value":integer(2),"else_value":integer(1)});
    out.response.candidates.push(GeneratedCandidate {
        id: "third".into(),
        source_json: serde_json::to_string(&c).unwrap(),
    });
    out.response.hypotheses[0].alternatives.push("third".into());
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    let w = &report.questions[0].witnesses[0];
    assert_eq!(
        w.witness().before.observations[0].values["selected_count"],
        DataValue::Integer { value: 2 }
    );
    assert_eq!(
        w.witness().scenario.session.values["mode"],
        DataValue::Boolean { value: true }
    );
}

#[test]
fn a_settled_early_observation_cannot_hide_a_later_new_outcome() {
    let (mut r, mut out) = input();
    let integer = |n| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}});
    let mut a = filtered();
    a["state"].as_array_mut().unwrap().extend([json!({"id":"mode","label":"Mode","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":false}}),json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}})]);
    a["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":integer(1)}]);
    a["observables"][0]["value"] = json!({"kind":"state","state":"result"});
    let mut b = a.clone();
    b["actions"][1]["steps"][0]["value"] = integer(0);
    r.sources = vec![capture(a.clone()), capture(b.clone())];
    out.response.candidates[0].source_json = serde_json::to_string(&a).unwrap();
    out.response.candidates[1].source_json = serde_json::to_string(&b).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    let scene = scenario(
        &r.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "first".into(),
            },
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let prior = saved_decision(&mut r, &out, DecisionOutcome::EitherAcceptable, vec![]);
    let mut current = scene;
    current
        .session
        .values
        .insert("mode".into(), DataValue::Boolean { value: true });
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&current).unwrap();
    let mut c = a;
    c["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"and","values":[{"kind":"state","state":"mode"},{"kind":"equal","left":{"kind":"state","state":"result"},"right":integer(1)}]},"then_value":integer(2),"else_value":integer(1)});
    out.response.candidates.push(GeneratedCandidate {
        id: "third".into(),
        source_json: serde_json::to_string(&c).unwrap(),
    });
    out.response.hypotheses[0].alternatives.push("third".into());
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    assert!(report.questions[0].witnesses.iter().any(|w| w
        .witness()
        .before
        .observations
        .iter()
        .any(|o| o.values["selected_count"] == DataValue::Integer { value: 2 })));
}

#[test]
fn a_new_combination_of_familiar_values_is_preserved() {
    let (mut r, mut out) = input();
    let integer = |n| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":n}});
    let mut a = filtered();
    a["state"].as_array_mut().unwrap().extend([json!({"id":"mode","label":"Mode","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":false}}),json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}})]);
    a["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":integer(1)}]);
    a["observables"][0]["value"] = json!({"kind":"state","state":"result"});
    let mut b = a.clone();
    b["actions"][1]["steps"][0]["value"] = integer(0);
    r.sources = vec![capture(a.clone()), capture(b.clone())];
    out.response.candidates[0].source_json = serde_json::to_string(&a).unwrap();
    out.response.candidates[1].source_json = serde_json::to_string(&b).unwrap();
    out.response.hypotheses[0].sources[0].raw_digest = r.sources[1].artifact.raw_digest.clone();
    let scene = scenario(
        &r.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "first".into(),
            },
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let prior = saved_decision(&mut r, &out, DecisionOutcome::EitherAcceptable, vec![]);
    let mut current = scene;
    current
        .session
        .values
        .insert("mode".into(), DataValue::Boolean { value: true });
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&current).unwrap();
    let mut c = a;
    c["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"and","values":[{"kind":"state","state":"mode"},{"kind":"equal","left":{"kind":"state","state":"result"},"right":integer(1)}]},"then_value":integer(0),"else_value":integer(1)});
    out.response.candidates.push(GeneratedCandidate {
        id: "third".into(),
        source_json: serde_json::to_string(&c).unwrap(),
    });
    out.response.hypotheses[0].alternatives.push("third".into());
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    for witness in &report.questions[0].witnesses {
        let certificate = witness.witness().minimization.as_ref().unwrap();
        assert!(certificate.complete);
        assert!(certificate
            .final_single_deletions
            .iter()
            .all(
                |trial| trial.outcome != ReductionOutcome::DifferencePreserved
                    && trial.outcome != ReductionOutcome::Inconclusive
            ));
    }
    assert!(report.questions[0].witnesses.iter().any(|w| w
        .witness()
        .distinguishing_properties
        .iter()
        .any(|p| matches!(p.predicate, PropertyPredicate::And { .. }))));
    assert!(report.questions[0].witnesses.iter().any(|w| w
        .witness()
        .before
        .observations
        .first()
        .unwrap()
        .values["selected_count"]
        == DataValue::Integer { value: 1 }
        && w.witness().before.observations.last().unwrap().values["selected_count"]
            == DataValue::Integer { value: 0 }));
}

#[test]
fn an_adjacent_duplicate_observation_does_not_reask_a_settled_choice() {
    let (mut r, mut out) = input();
    let prior = saved_decision(&mut r, &out, DecisionOutcome::EitherAcceptable, vec![]);
    let mut scene = out.response.hypotheses[0].scenario().unwrap();
    scene.inputs.push(SemanticInput::Observe {
        point: "again".into(),
    });
    out.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(report
        .log
        .iter()
        .any(|l| l.disposition == Disposition::Settled));
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
fn accepted_scene_context_binds_both_real_sides_and_disclosure() {
    let (request, _) = accepted_scene_request();
    request.validate().unwrap();
    let value = serde_json::to_value(&request).unwrap();
    let scenes = value["accepted_scenes"].as_array().unwrap();
    assert_eq!(scenes[0]["scenario"], scenes[1]["scenario"]);
    assert_ne!(scenes[0]["source"], scenes[1]["source"]);
    assert_ne!(scenes[0]["observations"], scenes[1]["observations"]);
    let projection = encode_request(&request, &ProviderOptions::default()).unwrap();
    let projected: Value = serde_json::from_slice(&projection.prompt).unwrap();
    assert_eq!(
        projected["request"]["accepted_scenes"],
        value["accepted_scenes"]
    );
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
fn discovery_primary_pair_stays_bound_when_selected_history_is_appended() {
    let (mut request, mut result) = accepted_scene_request();
    request.operation = DevelopmentOperation::Discover;
    let mut historic = filtered();
    historic["label"] = json!("Earlier approved display name");
    let historic = capture(historic);
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &historic,
            &request.examples[0].scenario,
            &request.decisions,
            RuntimeLimits::default(),
            "history",
        )
        .unwrap();
    let mut context = request.accepted_scenes[0].clone();
    context.source = historic.artifact.clone();
    context.observations = run.observations;
    request.sources.push(historic);
    request.accepted_scenes.push(context);
    refresh(&request, &mut result);
    let wire = encode_request(&request, &ProviderOptions::default()).unwrap();
    assert_eq!(
        wire.source_digest,
        request.sources[1].binding.identity().unwrap().as_str()
    );
    assert!(discover(
        &request,
        &result,
        &DiscoveryPolicy::default(),
        Arc::new(AtomicBool::new(false))
    )
    .is_ok());
    let mut unrelated = request.clone();
    unrelated.accepted_scenes.pop();
    assert!(unrelated.validate().is_err());
    let mut duplicated = request.clone();
    duplicated.sources.push(duplicated.sources[2].clone());
    assert!(duplicated.validate().is_err());
    let mut missing = request.clone();
    missing.sources.remove(1);
    assert!(missing.validate().is_err());
    let mut reordered = request.clone();
    reordered.sources.swap(1, 2);
    let changed = encode_request(&reordered, &ProviderOptions::default()).unwrap();
    assert_ne!(wire.source_digest, changed.source_digest);
}

#[test]
fn a_common_initial_observation_does_not_reopen_a_saved_choice() {
    for rename_final in [false, true] {
        let (mut request, mut result) = input();
        let prior = saved_decision(&mut request, &result, DecisionOutcome::Deferred, vec![]);
        let mut scene = result.response.hypotheses[0].scenario().unwrap();
        scene.inputs.insert(
            0,
            SemanticInput::Observe {
                point: "initial".into(),
            },
        );
        if rename_final {
            for input in &mut scene.inputs {
                if let SemanticInput::Observe { point } = input {
                    if point == "done" {
                        *point = "renamed-final".into();
                    }
                }
            }
        }
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let report = run(
            &request,
            &result,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(report.questions.is_empty());
        assert!(report
            .log
            .iter()
            .any(|entry| entry.disposition == Disposition::Settled));
        assert!(!report.normalizations.is_empty());
    }
}

#[test]
fn grouped_hypotheses_preserve_all_unresolved_provider_boundaries() {
    let (request, mut result) = input();
    result.response.hypotheses[0].unknowns =
        vec!["The maximum real-world batch size is unknown".into()];
    let mut duplicate = result.response.hypotheses[0].clone();
    duplicate.id = "same-choice-other-boundary".into();
    duplicate
        .unknowns
        .push("The locale-specific export policy is unknown".into());
    result.response.hypotheses.push(duplicate);
    let report = run(&request, &result, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert_eq!(
        report.questions[0].unknowns,
        result.response.hypotheses[1].unknowns
    );
}

#[test]
fn ambiguous_retained_points_are_not_presented_as_a_verified_new_choice() {
    for different_trace in [false, true] {
        let (mut request, mut result) = input();
        let mut original = result.response.hypotheses[0].scenario().unwrap();
        original.inputs.extend([
            SemanticInput::Navigate {
                view: "people".into(),
            },
            SemanticInput::Observe {
                point: "again".into(),
            },
        ]);
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&original).unwrap();
        let prior = saved_decision(
            &mut request,
            &result,
            DecisionOutcome::EitherAcceptable,
            vec![],
        );
        let mut renamed = original;
        for input in &mut renamed.inputs {
            if let SemanticInput::Observe { point } = input {
                *point = format!("renamed-{point}");
            } else if different_trace && matches!(input, SemanticInput::Navigate { .. }) {
                *input = SemanticInput::AdvanceClock { days: 1 };
            }
        }
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&renamed).unwrap();
        refresh(&request, &mut result);
        let report = run(
            &request,
            &result,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(report.questions.is_empty());
        if different_trace {
            assert!(report
                .log
                .iter()
                .any(|entry| entry.disposition == Disposition::Unverified));
            assert!(report
                .unverified
                .iter()
                .any(|entry| entry.contains("correspondence")));
        } else {
            assert!(report
                .log
                .iter()
                .any(|entry| entry.disposition == Disposition::Settled));
        }
    }
}

#[test]
fn permuting_existing_point_names_cannot_invent_a_new_outcome() {
    for extra_initial in [false, true] {
        let (mut request, mut result) = input();
        let mut before = filtered();
        before["state"].as_array_mut().unwrap().push(json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
        before["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":{"kind":"add","left":{"kind":"state","state":"result"},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}}]);
        before["observables"][0]["value"] = json!({"kind":"state","state":"result"});
        let mut after = before.clone();
        after["actions"][1]["steps"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":0}});
        request.sources = vec![capture(before.clone()), capture(after.clone())];
        result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
        result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
        result.response.hypotheses[0].sources[0].raw_digest =
            request.sources[1].artifact.raw_digest.clone();
        let mut scene = scenario(
            &request.sources[0],
            vec![
                invoke("collect", Values::new()),
                SemanticInput::Observe {
                    point: "first".into(),
                },
                invoke("collect", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        );
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let prior = saved_decision(
            &mut request,
            &result,
            DecisionOutcome::EitherAcceptable,
            vec![],
        );
        assert_eq!(
            prior
                .before
                .observations
                .iter()
                .map(|o| o.values["selected_count"].clone())
                .collect::<Vec<_>>(),
            vec![
                DataValue::Integer { value: 1 },
                DataValue::Integer { value: 2 }
            ]
        );
        for input in &mut scene.inputs {
            if let SemanticInput::Observe { point } = input {
                *point = if point == "first" { "done" } else { "first" }.into();
            }
        }
        if extra_initial {
            scene.inputs.insert(
                0,
                SemanticInput::Observe {
                    point: "initial".into(),
                },
            );
        }
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let report = run(
            &request,
            &result,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(report.questions.is_empty());
        assert!(report
            .log
            .iter()
            .any(|entry| entry.disposition == Disposition::Settled));
    }
}

#[test]
fn changed_action_sequences_need_verified_correspondence_before_new_questions() {
    for change in ["delete", "insert", "reorder"] {
        let (mut request, mut result) = input();
        let mut before = filtered();
        before["state"].as_array_mut().unwrap().push(json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
        before["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":{"kind":"add","left":{"kind":"state","state":"result"},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}}}]);
        before["actions"].as_array_mut().unwrap().push(json!({"id":"touch","label":"Leave search unchanged","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"search","value":{"kind":"state","state":"search"}}],"ensures":[]}));
        before["observables"][0]["value"] = json!({"kind":"state","state":"result"});
        let mut after = before.clone();
        after["actions"][1]["steps"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":0}});
        request.sources = vec![capture(before.clone()), capture(after.clone())];
        result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
        result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
        result.response.hypotheses[0].sources[0].raw_digest =
            request.sources[1].artifact.raw_digest.clone();
        let mut scene = scenario(
            &request.sources[0],
            vec![
                invoke("touch", Values::new()),
                SemanticInput::Observe {
                    point: "initial".into(),
                },
                invoke("collect", Values::new()),
                SemanticInput::Observe {
                    point: "first".into(),
                },
                invoke("collect", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        );
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let prior = saved_decision(
            &mut request,
            &result,
            DecisionOutcome::EitherAcceptable,
            vec![],
        );
        match change {
            "delete" => {
                scene.inputs.remove(0);
            }
            "insert" => scene.inputs.insert(2, invoke("touch", Values::new())),
            "reorder" => {
                let input = scene.inputs.remove(0);
                scene.inputs.insert(3, input);
            }
            _ => unreachable!(),
        }
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let report = run(
            &request,
            &result,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(
            report.questions.is_empty(),
            "invented a choice after {change}"
        );
        assert!(report
            .log
            .iter()
            .any(|entry| entry.disposition == Disposition::Unverified));
        assert!(report
            .unverified
            .iter()
            .any(|entry| entry.contains("correspondence")));
    }
}

#[test]
fn incomplete_alternative_intention_checks_remain_explicit_with_or_without_a_witness() {
    for (fault, chosen) in [("fuel", false), ("fuel", true), ("missing", false)] {
        for valid_alternative in [false, true] {
            let (mut request, mut result) = input();
            let integer = |value| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":value}});
            let mut sources = vec![];
            for source in &request.sources {
                let mut app: Value = serde_json::from_slice(&source.source_bytes).unwrap();
                app["state"].as_array_mut().unwrap().push(json!({"id":"health","label":"Health","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}));
                app["observables"].as_array_mut().unwrap().push(json!({"id":"health","label":"Health","value":{"kind":"state","state":"health"}}));
                app["actions"].as_array_mut().unwrap().push(json!({"id":"check","label":"Check","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"health","value":integer(0)}],"ensures":[]}));
                sources.push(capture(app));
            }
            request.sources = sources;
            for (index, source) in request.sources.iter().enumerate() {
                result.response.candidates[index].source_json =
                    String::from_utf8(source.source_bytes.clone()).unwrap();
            }
            result.response.hypotheses[0].sources[0].raw_digest =
                request.sources[1].artifact.raw_digest.clone();
            let mut discovery_scene = result.response.hypotheses[0].scenario().unwrap();
            discovery_scene.session = SessionState::initial(&request.sources[1].program).unwrap();
            result.response.hypotheses[0].scenario_json =
                serde_json::to_string(&discovery_scene).unwrap();
            let checked = scenario(
                &request.sources[1],
                vec![
                    invoke("check", Values::new()),
                    SemanticInput::Observe {
                        point: "done".into(),
                    },
                ],
            );
            let mut old: Value = serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
            old["actions"][3]["steps"][0]["value"] = integer(1);
            let old = capture(old);
            let prior = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
                .compare(
                    &old,
                    &request.sources[1],
                    &checked,
                    &request.decisions,
                    product_scenarios::ObservationTarget::Observable {
                        point: "done".into(),
                        observable: "health".into(),
                    },
                    RuntimeLimits::default(),
                )
                .unwrap()
                .witness
                .unwrap()
                .witness()
                .clone();
            let mut property = count_is(0);
            if let PropertyPredicate::Equal {
                left: PropertyTerm::Observed { observable, .. },
                ..
            } = &mut property.predicate
            {
                *observable = "health".into();
            }
            request.examples.push(SelectedScenario {
                disclosure: Disclosure::Synthetic,
                scenario: checked.clone(),
            });
            request.decisions.decisions.push(ScopedDecision {
                id: "saved-health".into(),
                revision: 1,
                request: "Keep the checked behavior".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["check".into()].into_iter().collect(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::Accept {
                    artifact: request.sources[1].artifact.program_digest.clone(),
                },
                status: DecisionStatus::Active,
                obligations: if chosen { vec![] } else { vec![property] },
                scenarios: vec![checked.identity().unwrap()],
                witness: prior.identity().unwrap(),
                supersedes: vec![],
            });
            let mut bad: Value = serde_json::from_slice(&request.sources[0].source_bytes).unwrap();
            if fault == "fuel" {
                let mut value = integer(0);
                for _ in 0..7 {
                    value = json!({"kind":"add","left":value,"right":integer(0)});
                }
                bad["actions"][3]["steps"] = json!(vec![
                    json!({"kind":"set_state","state":"health","value":value});
                    256
                ]);
            } else {
                bad["observables"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|o| o["id"] != "health");
            }
            let bad = capture(bad);
            result.response.candidates.push(GeneratedCandidate {
                id: "unverified-alternative".into(),
                source_json: String::from_utf8(bad.source_bytes.clone()).unwrap(),
            });
            result.response.hypotheses[0].alternatives = if valid_alternative {
                vec![
                    "retain".into(),
                    "unverified-alternative".into(),
                    "replace".into(),
                ]
            } else {
                vec!["unverified-alternative".into(), "replace".into()]
            };
            refresh(&request, &mut result);
            let mut policy = DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            };
            policy.search.runtime.fuel = 2000;
            let valid = product_runtime::LocalRuntime::default()
                .replay(
                    &request.sources[1],
                    &checked,
                    &request.decisions,
                    policy.search.runtime.clone(),
                    "current-check",
                )
                .unwrap();
            assert_eq!(valid.state, EvidenceState::Observed);
            let failed = product_runtime::LocalRuntime::default()
                .replay(
                    &bad,
                    &checked,
                    &request.decisions,
                    policy.search.runtime.clone(),
                    "bad-check",
                )
                .unwrap();
            if fault == "fuel" {
                assert_eq!(failed.state, EvidenceState::Inconclusive);
            } else {
                assert_eq!(failed.state, EvidenceState::Observed);
                assert!(!failed.observations[0].values.contains_key("health"));
            }
            for source in &request.sources {
                let anchored = product_runtime::LocalRuntime::default()
                    .replay(
                        source,
                        &discovery_scene,
                        &request.decisions,
                        policy.search.runtime.clone(),
                        "anchor-check",
                    )
                    .unwrap();
                assert_eq!(anchored.state, EvidenceState::Observed);
            }
            let report = run(&request, &result, policy.clone());
            assert!(!report.unverified.is_empty(),"lost {fault} uncertainty with chosen={chosen}, valid_alternative={valid_alternative}");
            assert_eq!(report.questions.len(), usize::from(valid_alternative));
            assert!(report
                .log
                .iter()
                .all(|entry| entry.disposition != Disposition::Settled));
            assert!(report
                .log
                .iter()
                .all(|entry| entry.state == EvidenceState::Inconclusive));
            if fault == "fuel" && !chosen && valid_alternative {
                let mut unverified_current: Value =
                    serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
                unverified_current["actions"][3]["steps"] =
                    serde_json::to_value(&bad.program.actions[3].steps).unwrap();
                request.sources[1] = capture(unverified_current);
                result.response.candidates[1].source_json =
                    String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
                result.response.hypotheses[0].sources[0].raw_digest =
                    request.sources[1].artifact.raw_digest.clone();
                result.response.hypotheses[0].alternatives =
                    vec!["retain".into(), "replace".into()];
                refresh(&request, &mut result);
                let report = run(&request, &result, policy);
                assert!(!report.unverified.is_empty());
                assert!(
                    report.questions.is_empty(),
                    "unverified current intentions were offered on another action"
                );
                assert!(report
                    .log
                    .iter()
                    .all(|entry| entry.disposition == Disposition::Unverified));
            }
            if !valid_alternative {
                assert!(report
                    .log
                    .iter()
                    .all(|entry| entry.disposition == Disposition::Unverified));
            }
        }
    }
}

fn multiple_observable_input() -> (DevelopmentRequest, DevelopmentResult) {
    let (mut request, mut result) = input();
    let mut before = filtered();
    before["state"].as_array_mut().unwrap().extend([json!({"id":"x","label":"Primary result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}),json!({"id":"y","label":"Secondary result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}})]);
    let set = |state: &str, value| json!({"kind":"set_state","state":state,"value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":value}}});
    before["actions"][1]["steps"] = json!([set("x", 1), set("y", 10)]);
    before["observables"][0]["value"] = json!({"kind":"state","state":"x"});
    before["observables"].as_array_mut().unwrap().push(
        json!({"id":"secondary","label":"Secondary result","value":{"kind":"state","state":"y"}}),
    );
    let mut after = before.clone();
    after["actions"][1]["steps"] = json!([set("x", 0), set("y", 20)]);
    request.sources = vec![capture(before.clone()), capture(after.clone())];
    result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
    result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scenario(
        &request.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    ))
    .unwrap();
    refresh(&request, &mut result);
    (request, result)
}

#[test]
fn a_changed_secondary_observable_is_not_hidden_by_a_familiar_primary() {
    let (mut request, mut result) = multiple_observable_input();
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    let mut candidate: Value = serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
    candidate["actions"][1]["steps"][1]["value"]["value"]["value"] = json!(30);
    request.sources[1] = capture(candidate);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    refresh(&request, &mut result);
    let report = run(
        &request,
        &result,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert_eq!(report.questions.len(), 1);
    assert!(report.questions[0].witnesses.iter().any(|w| w
        .witness()
        .after
        .observations
        .iter()
        .any(|o| o.values["secondary"] == DataValue::Integer { value: 30 })));
}

#[test]
fn different_primary_labels_group_the_same_executed_multi_observable_contrast() {
    for renamed in [false, true] {
        let (request, mut result) = multiple_observable_input();
        let mut other = result.response.hypotheses[0].clone();
        other.id = "secondary-perspective".into();
        other.observable = "secondary".into();
        if renamed {
            let mut scene = other.scenario().unwrap();
            for input in &mut scene.inputs {
                if let SemanticInput::Observe { point } = input {
                    *point = "renamed".into();
                }
            }
            other.scenario_json = serde_json::to_string(&scene).unwrap();
        }
        result.response.hypotheses.push(other);
        let report = run(&request, &result, DiscoveryPolicy::default());
        assert_eq!(report.questions.len(), 1);
        assert_eq!(report.questions[0].hypotheses.len(), 2);
        assert!(report
            .log
            .iter()
            .any(|entry| entry.disposition == Disposition::Grouped));
    }
}

#[test]
fn changed_retained_points_cannot_be_hidden_by_omitting_proposed_observations() {
    let (mut request, mut result) = input();
    let mut before = filtered();
    let integer = |value| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":value}});
    before["state"].as_array_mut().unwrap().extend([json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}),json!({"id":"calls","label":"Calls","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}})]);
    before["observables"][0]["value"] = json!({"kind":"state","state":"result"});
    before["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":integer(1)},{"kind":"set_state","state":"calls","value":{"kind":"add","left":{"kind":"state","state":"calls"},"right":integer(1)}}]);
    let mut after = before.clone();
    after["actions"][1]["steps"][0]["value"] = integer(0);
    request.sources = vec![capture(before.clone()), capture(after.clone())];
    result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
    result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    let mut scene = scenario(
        &request.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "first".into(),
            },
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&request, &mut result);
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    let prior_scene = prior.scenario.identity().unwrap();
    after["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"equal","left":{"kind":"state","state":"calls"},"right":integer(0)},"then_value":integer(2),"else_value":integer(0)});
    request.sources[1] = capture(after);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    scene.inputs.remove(1);
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&request, &mut result);
    let report = run(
        &request,
        &result,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.runs.iter().any(|run| run.id == "retained-after"
        && run
            .observations
            .iter()
            .any(|o| o.values["selected_count"] == DataValue::Integer { value: 2 })));
    assert_eq!(report.questions.len(), 1);
    let retained = report.questions[0]
        .witnesses
        .iter()
        .find(|w| {
            w.initial_scenario().identity().unwrap() == prior_scene
                && w.witness()
                    .after
                    .observations
                    .iter()
                    .any(|o| o.values["selected_count"] == DataValue::Integer { value: 2 })
        })
        .expect("the new retained observation must become its own executable witness");
    let witness = retained.witness();
    assert_eq!(witness.scenario.inputs.len(), 2);
    assert_eq!(
        witness.before.binding.input_digest,
        witness.after.binding.input_digest
    );
    let certificate = witness.minimization.as_ref().unwrap();
    assert!(certificate.complete);
    assert!(!certificate.final_single_deletions.is_empty());
    assert!(certificate
        .final_single_deletions
        .iter()
        .all(|trial| matches!(
            trial.outcome,
            ReductionOutcome::DifferenceLost | ReductionOutcome::InvalidScenario
        )));
}

#[test]
fn withdrawn_decisions_keep_terminal_history_without_becoming_live_requirements() {
    let (mut request, mut result) = input();
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    let original = request.decisions.decisions[0].clone();
    let mut terminal = serde_json::to_value(&original).unwrap();
    terminal["status"] = json!({"kind":"withdrawn","adoption":"recovery-1"});
    request.decisions.decisions[0] = serde_json::from_value(terminal.clone()).unwrap();
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
    terminal["status"]["adoption"] = json!("invalid receipt id");
    assert!(serde_json::from_value::<ScopedDecision>(terminal)
        .unwrap()
        .validate()
        .is_err());
    request.examples.clear();
    refresh(&request, &mut result);
    let report = run(&request, &result, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert!(report.checks.is_empty());
    assert!(report.defects.is_empty());
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
