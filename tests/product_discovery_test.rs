//! Recorded/fake provider responses exercise components, never live AI acceptance.
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
use product_decisions::IntentionBinding;
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
    saved_decision_for_target(
        r,
        out,
        outcome,
        obligations,
        product_scenarios::ObservationTarget::Observable {
            point: "done".into(),
            observable: "selected_count".into(),
        },
    )
}
fn saved_decision_for_target(
    r: &mut DevelopmentRequest,
    out: &DevelopmentResult,
    outcome: DecisionOutcome,
    obligations: Vec<AcceptedProperty>,
    target: product_scenarios::ObservationTarget,
) -> DifferentialWitness {
    let scene = out.response.hypotheses[0].scenario().unwrap();
    let observed = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .compare(
            &r.sources[0],
            &r.sources[1],
            &scene,
            &r.decisions,
            target,
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

fn independent_properties(prior: DifferentialWitness) -> DiscoveryPolicy {
    DiscoveryPolicy {
        chosen_artifacts: [(
            "saved".into(),
            prior.before.binding.artifact.program_digest.clone(),
        )]
        .into(),
        retained_witnesses: vec![prior],
        witness_bindings: [("saved".into(), IntentionBinding::PropertiesOnly)].into(),
        ..DiscoveryPolicy::default()
    }
}

#[test]
fn explicit_requirement_violations_are_defects_never_preference_questions() {
    let (mut r, mut out) = input();
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    refresh(&r, &mut out);
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::Accept { artifact },
        vec![count_is(1)],
    );
    refresh(&r, &mut out);
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    r.examples.clear();
    refresh(&r, &mut out);
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    refresh(&r, &mut out);
    out.response.hypotheses[0].kind = HypothesisKind::RequestedChange;
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let prior = saved_decision(
        &mut r,
        &out,
        DecisionOutcome::KeepCurrent,
        vec![count_is(2)],
    );
    r.context.recent_inputs.clear();
    out.response.hypotheses.clear();
    refresh(&r, &mut out);
    let report = run(&r, &out, independent_properties(prior.clone()));
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
    let rejected = capture(alt);
    let prior = product_scenarios::ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .compare(
            &r.sources[0],
            &rejected,
            &scene,
            &r.decisions,
            product_scenarios::ObservationTarget::ViewColumn {
                point: "done".into(),
                column: "name".into(),
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
        witness: prior.identity().unwrap(),
        supersedes: vec![],
    });
    refresh(&r, &mut out);
    let report = run(
        &r,
        &out,
        DiscoveryPolicy {
            retained_witnesses: vec![prior],
            witness_bindings: [("name-intent".into(), IntentionBinding::PropertiesOnly)].into(),
            chosen_artifacts: [(
                "name-intent".into(),
                r.sources[0].artifact.program_digest.clone(),
            )]
            .into(),
            ..DiscoveryPolicy::default()
        },
    );
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
    assert_omitted_retained_observation_is_discovered(0);
}

#[test]
fn equal_provider_observations_do_not_skip_changed_retained_scenes() {
    assert_omitted_retained_observation_is_discovered(1);
}

#[test]
fn consumer_labels_do_not_exclude_revealing_producer_only_history() {
    assert_omitted_retained_observation_with_consumer(1, true);
}

fn assert_omitted_retained_observation_is_discovered(final_value: i64) {
    assert_omitted_retained_observation_with_consumer(final_value, false);
}

fn assert_omitted_retained_observation_with_consumer(final_value: i64, consumer: bool) {
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
    after["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"equal","left":{"kind":"state","state":"calls"},"right":integer(0)},"then_value":integer(2),"else_value":integer(final_value)});
    request.sources[1] = capture(after);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    scene.inputs.remove(1);
    if consumer {
        result.response.hypotheses[0].action = "export_people".into();
        scene.inputs.insert(
            scene.inputs.len() - 1,
            invoke("export_people", Values::new()),
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

#[test]
fn missing_or_incompatible_observed_channels_remain_explicitly_unverified() {
    for case in ["omitted_export", "changed_type", "removed_secondary"] {
        let (mut request, mut result) = input();
        let mut before = filtered();
        if case == "removed_secondary" {
            before["observables"].as_array_mut().unwrap().push(json!({
                "id":"secondary", "label":"Secondary", "value":{
                    "kind":"literal", "value_type":{"kind":"integer"},
                    "value":{"kind":"integer","value":10}
                }
            }));
        }
        let mut after = before.clone();
        let (action, pointer) = match case {
            "omitted_export" => {
                after["actions"][2]["steps"] = json!([{"kind":"set_state","state":"selected","value":{"kind":"state","state":"selected"}}]);
                ("export_people", "/actions/2/steps")
            }
            "changed_type" => {
                after["observables"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"one"}});
                ("collect", "/observables/0/value")
            }
            "removed_secondary" => {
                after["observables"].as_array_mut().unwrap().pop();
                ("collect", "/observables/1")
            }
            _ => unreachable!(),
        };
        request.sources = vec![capture(before.clone()), capture(after.clone())];
        result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
        result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
        let hypothesis = &mut result.response.hypotheses[0];
        hypothesis.action = action.into();
        // A removed node's locus must reference the exact baseline bytes.
        hypothesis.sources[0].raw_digest = request.sources
            [usize::from(case != "removed_secondary")]
        .artifact
        .raw_digest
        .clone();
        hypothesis.sources[0].pointer = pointer.into();
        hypothesis.scenario_json = serde_json::to_string(&scenario(
            &request.sources[0],
            vec![
                add("Ada"),
                invoke("collect", Values::new()),
                invoke("export_people", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        ))
        .unwrap();
        refresh(&request, &mut result);
        let report = run(&request, &result, DiscoveryPolicy::default());
        assert_eq!(
            report
                .runs
                .iter()
                .filter(|r| r.id.starts_with("captured-"))
                .count(),
            2,
            "{case}"
        );
        assert!(report
            .runs
            .iter()
            .filter(|r| r.id.starts_with("captured-"))
            .all(|r| r.state == EvidenceState::Observed));
        assert!(report.questions.is_empty(), "{case}");
        assert!(
            !report.unverified.is_empty(),
            "missing uncertainty for {case}"
        );
        assert_eq!(report.log[0].disposition, Disposition::Unverified, "{case}");
    }
}

#[test]
fn retained_view_navigation_participates_in_changed_source_relevance() {
    let (mut request, mut result) = input();
    let mut before = filtered();
    let mut secondary = before["views"][0].clone();
    secondary["id"] = json!("review_people");
    let secondary_index = before["views"].as_array().unwrap().len();
    before["views"].as_array_mut().unwrap().push(secondary);
    let mut after = before.clone();
    after["views"][secondary_index]["kind"]["rows"]["sort"][0]["descending"] = json!(true);
    request.sources = vec![capture(before.clone()), capture(after.clone())];
    result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
    result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
    let mut scene = scenario(
        &request.sources[0],
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            SemanticInput::Navigate {
                view: "review_people".into(),
            },
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    result.response.hypotheses[0].sources[0].pointer =
        format!("/views/{secondary_index}/kind/rows");
    refresh(&request, &mut result);
    let prior = saved_decision_for_target(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
        product_scenarios::ObservationTarget::ViewRows {
            point: "done".into(),
        },
    );
    let prior_scene = prior.scenario.identity().unwrap();
    after["views"][secondary_index]["kind"]["rows"]["predicate"] = json!({"kind":"equal","left":field(var("item"),"name"),"right":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}});
    request.sources[1] = capture(after);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    scene.inputs.remove(3);
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
    assert_eq!(report.questions.len(), 1);
    assert!(report.questions[0]
        .witnesses
        .iter()
        .any(|w| w.initial_scenario().identity().unwrap() == prior_scene
            && w.witness()
                .after
                .observations
                .iter()
                .any(|o| o.view.view == "review_people" && o.view.rows.len() == 1)));
}

#[test]
fn cumulative_export_sampling_limits_remain_unverified_not_absent() {
    let (mut request, mut result) = input();
    let mut before = filtered();
    before["state"].as_array_mut().unwrap().push(json!({
        "id":"export_values", "label":"Export values", "value_type":{"kind":"list","item":{"kind":"integer"}},
        "initial":{"kind":"list","item_type":{"kind":"integer"},"items":vec![json!({"kind":"integer","value":0});6000]}
    }));
    before["actions"][2]["steps"][0]["items"] = json!({"kind":"state","state":"export_values"});
    before["actions"][2]["steps"][0]["columns"]["name"] = text("before");
    let mut after = before.clone();
    after["actions"][2]["steps"][0]["columns"]["name"] = text("after");
    request.sources = vec![capture(before.clone()), capture(after.clone())];
    result.response.candidates[0].source_json = serde_json::to_string(&before).unwrap();
    result.response.candidates[1].source_json = serde_json::to_string(&after).unwrap();
    let hypothesis = &mut result.response.hypotheses[0];
    hypothesis.action = "export_people".into();
    hypothesis.sources[0].raw_digest = request.sources[1].artifact.raw_digest.clone();
    hypothesis.sources[0].pointer = "/actions/2/steps/0/columns/name".into();
    hypothesis.scenario_json = serde_json::to_string(&scenario(
        &request.sources[0],
        vec![
            invoke("export_people", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    ))
    .unwrap();
    refresh(&request, &mut result);
    let report = run(
        &request,
        &result,
        DiscoveryPolicy {
            search: product_scenarios::SearchBudget {
                runtime: RuntimeLimits {
                    fuel: 10_000_000,
                    elapsed_millis: 60_000,
                    ..RuntimeLimits::default()
                },
                ..product_scenarios::SearchBudget::default()
            },
            ..DiscoveryPolicy::default()
        },
    );
    let captured: Vec<_> = report
        .runs
        .iter()
        .filter(|r| r.id.starts_with("captured-"))
        .collect();
    assert_eq!(captured.len(), 2);
    for run in &captured {
        assert_eq!(run.state, EvidenceState::Observed, "{:?}", run.errors);
        let outputs = &run.observations[0].outputs;
        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().all(|output| output.rows.len() == 6000));
    }
    assert_ne!(
        captured[0].observations[0].outputs[0].rows[0],
        captured[1].observations[0].outputs[0].rows[0]
    );
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert_eq!(report.log[0].disposition, Disposition::Unverified);
}

#[test]
fn downstream_consumer_hypotheses_include_executed_producer_actions() {
    let (request, mut result) = input();
    result.response.hypotheses[0].action = "export_people".into();
    let report = run(&request, &result, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert_eq!(report.questions[0].action, "export_people");
    assert!(report.questions[0].witnesses.iter().any(|w| {
        let before = &w.witness().before.observations;
        let after = &w.witness().after.observations;
        before.iter().zip(after).any(|(a, b)| {
            !a.outputs.is_empty()
                && !b.outputs.is_empty()
                && a.outputs.iter().map(|o| o.rows.len()).sum::<usize>()
                    != b.outputs.iter().map(|o| o.rows.len()).sum::<usize>()
        })
    }));
}

#[test]
fn executed_view_changes_outside_sample_targets_remain_explicitly_unverified() {
    let (mut request, mut result) = input();
    let mut after = filtered();
    after["views"][0]["actions"][0]["enabled"] = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}});
    request.sources[1] = capture(after);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    result.response.hypotheses[0].sources[0].pointer = "/views/0/actions/0/enabled".into();
    refresh(&request, &mut result);
    let report = run(&request, &result, DiscoveryPolicy::default());
    let captured: Vec<_> = report
        .runs
        .iter()
        .filter(|r| r.id.starts_with("captured-"))
        .collect();
    assert_eq!(captured.len(), 2);
    assert!(captured.iter().all(|r| r.state == EvidenceState::Observed));
    assert_ne!(
        captured[0].observations[0].view.enabled_actions,
        captured[1].observations[0].view.enabled_actions
    );
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert_eq!(report.log[0].disposition, Disposition::Unverified);
}

#[test]
fn additional_chosen_scenes_use_verified_selected_context() {
    for history in ["verified", "forged"] {
        let (mut request, mut result) = multiple_observable_input();
        let selected = request.sources[1].artifact.program_digest.clone();
        let prior = saved_decision(
            &mut request,
            &result,
            DecisionOutcome::Accept { artifact: selected },
            vec![],
        );
        let mut additional = request.examples[0].scenario.clone();
        additional.id = "chosen-additional".into();
        additional.clock_day += 1;
        let digest = additional.identity().unwrap();
        let actual = product_runtime::LocalRuntime::default()
            .replay(
                &request.sources[1],
                &additional,
                &request.decisions,
                RuntimeLimits::default(),
                "accepted-selected",
            )
            .unwrap();
        assert_eq!(actual.state, EvidenceState::Observed);
        let mut observations = actual.observations;
        if history == "forged" {
            observations[0]
                .values
                .insert("secondary".into(), DataValue::Integer { value: 99 });
        }
        request.decisions.decisions[0]
            .scenarios
            .push(digest.clone());
        request.examples.push(SelectedScenario {
            disclosure: Disclosure::Synthetic,
            scenario: additional,
        });
        request.accepted_scenes.push(AcceptedSceneContext {
            decision: "saved".into(),
            source: request.sources[1].artifact.clone(),
            scenario: digest,
            observations,
            disclosure: Disclosure::Synthetic,
        });
        refresh(&request, &mut result);
        let report = run(
            &request,
            &result,
            DiscoveryPolicy {
                retained_witnesses: vec![prior],
                ..DiscoveryPolicy::default()
            },
        );
        assert!(report.defects.is_empty());
        assert!(report.questions.is_empty());
        assert!(report
            .runs
            .iter()
            .any(|run| run.id == "accepted-history-selected"
                && run.origin == ExecutionOrigin::ProductionRuntime
                && run.state == EvidenceState::Observed));
        if history == "verified" {
            assert_eq!(report.checks.len(), 2);
            assert!(report
                .checks
                .iter()
                .all(|check| check.state == CheckState::Satisfied));
            assert!(report.unverified.is_empty(), "{:?}", report.unverified);
        } else {
            assert!(!report.unverified.is_empty());
        }
    }
}

#[test]
fn chosen_outcome_channel_loss_is_unknown_not_an_observed_violation() {
    for side in ["current", "alternative"] {
        for change in ["removed", "retyped"] {
            let (mut request, mut result) = input();
            for index in 0..2 {
                let mut program: Value =
                    serde_json::from_slice(&request.sources[index].source_bytes).unwrap();
                program["observables"].as_array_mut().unwrap().push(json!({"id":"secondary","label":"Secondary","value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":7}}}));
                request.sources[index] = capture(program);
                result.response.candidates[index].source_json =
                    String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
            }
            result.response.hypotheses[0].sources[0].raw_digest =
                request.sources[1].artifact.raw_digest.clone();
            refresh(&request, &mut result);
            let selected = request.sources[1].artifact.program_digest.clone();
            let prior = saved_decision(
                &mut request,
                &result,
                DecisionOutcome::Accept { artifact: selected },
                vec![],
            );
            let mut modified: Value =
                serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
            if change == "removed" {
                modified["observables"].as_array_mut().unwrap().pop();
            } else {
                modified["observables"][1]["value"] = json!({"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"seven"}});
            }
            let modified = capture(modified);
            if side == "current" {
                request.sources[1] = modified.clone();
            }
            result.response.candidates[usize::from(side == "current")].source_json =
                String::from_utf8(modified.source_bytes.clone()).unwrap();
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
            assert!(
                report.defects.is_empty(),
                "{side}/{change}: {:?}",
                report.defects
            );
            assert!(!report.unverified.is_empty(), "{side}/{change}");
            assert!(report.questions.is_empty());
            if side == "current" {
                assert!(report
                    .checks
                    .iter()
                    .any(|check| check.state == CheckState::Unknown));
            }
            assert!(report
                .log
                .iter()
                .all(|entry| entry.disposition != Disposition::Settled));
        }
    }
}

#[test]
fn every_accepted_scene_is_searched_when_provider_omits_the_new_outcome() {
    assert_additional_scene_history(true, "verified", "original");
}

#[test]
fn additional_accepted_scenes_use_their_own_verified_history() {
    for history in ["verified", "missing", "forged"] {
        assert_additional_scene_history(false, history, "original");
    }
}

#[test]
fn additional_scene_instrumentation_does_not_reopen_a_settled_choice() {
    for instrumentation in ["renamed", "removed_observe", "inert"] {
        assert_additional_scene_history(false, "verified", instrumentation);
    }
}

fn assert_additional_scene_history(changed: bool, history: &str, instrumentation: &str) {
    let (mut request, mut result) = input();
    let integer = |value| json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":value}});
    let mut before = filtered();
    before["state"].as_array_mut().unwrap().extend([
        json!({"id":"result","label":"Result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}),
        json!({"id":"special","label":"Special","value_type":{"kind":"boolean"},"initial":{"kind":"boolean","value":false}}),
    ]);
    before["observables"][0]["value"] = json!({"kind":"state","state":"result"});
    before["actions"][1]["steps"] = json!([{"kind":"set_state","state":"result","value":{"kind":"if","condition":{"kind":"state","state":"special"},"then_value":integer(3),"else_value":integer(1)}}]);
    let mut after = before.clone();
    after["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"state","state":"special"},"then_value":integer(2),"else_value":integer(0)});
    request.sources = vec![capture(before), capture(after.clone())];
    for index in 0..2 {
        result.response.candidates[index].source_json =
            String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
    }
    let scene = scenario(
        &request.sources[0],
        vec![
            invoke("collect", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    refresh(&request, &mut result);
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    request.decisions.decisions[0].status = DecisionStatus::Pending;
    let mut additional = scene;
    additional.id = "additional-accepted-scene".into();
    additional
        .session
        .values
        .insert("special".into(), DataValue::Boolean { value: true });
    if instrumentation == "removed_observe" {
        additional.inputs.push(SemanticInput::Observe {
            point: "duplicate".into(),
        });
    }
    if instrumentation != "original" {
        let mut proposed = additional.clone();
        match instrumentation {
            "renamed" => {
                proposed.inputs[1] = SemanticInput::Observe {
                    point: "renamed".into(),
                };
            }
            "removed_observe" => {
                proposed.inputs.pop();
            }
            _ => {
                proposed.inputs.insert(
                    0,
                    SemanticInput::Control {
                        view: "people".into(),
                        control: "search_input".into(),
                        value: string(""),
                    },
                );
            }
        }
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&proposed).unwrap();
    }
    let additional_digest = additional.identity().unwrap();
    request.decisions.decisions[0]
        .scenarios
        .push(additional_digest.clone());
    request.examples.push(SelectedScenario {
        disclosure: Disclosure::Synthetic,
        scenario: additional.clone(),
    });
    if history != "missing" {
        for source in &request.sources {
            let observed = product_runtime::LocalRuntime::default()
                .replay(
                    source,
                    &additional,
                    &request.decisions,
                    RuntimeLimits::default(),
                    "accepted-additional",
                )
                .unwrap();
            assert_eq!(observed.state, EvidenceState::Observed);
            let mut observations = observed.observations;
            if history == "forged" {
                observations[0]
                    .values
                    .insert("selected_count".into(), DataValue::Integer { value: 99 });
            }
            request.accepted_scenes.push(AcceptedSceneContext {
                decision: "saved".into(),
                source: source.artifact.clone(),
                scenario: additional_digest.clone(),
                observations,
                disclosure: Disclosure::Synthetic,
            });
        }
    }
    if changed {
        let old = request.sources[1].clone();
        after["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"state","state":"special"},"then_value":integer(4),"else_value":integer(0)});
        request.sources[1] = capture(after);
        request.sources.push(old);
    }
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
    assert!(report
        .runs
        .iter()
        .any(|r| r.id == "captured-retained-candidate"
            && r.binding.scenario_digest == additional_digest
            && r.state == EvidenceState::Observed
            && r.observations[0].values["selected_count"]
                == DataValue::Integer {
                    value: if changed { 4 } else { 2 }
                }));
    if !changed {
        assert!(report.questions.is_empty(), "{history}: {:?}", report.log);
        if history == "verified" && instrumentation != "inert" {
            assert_eq!(report.log[0].disposition, Disposition::Settled);
            for id in ["accepted-history-before", "accepted-history-after"] {
                assert!(report.runs.iter().any(|run| run.id == id
                    && run.origin == ExecutionOrigin::ProductionRuntime
                    && run.state == EvidenceState::Observed
                    && run.binding.input_digest
                        == request.examples[1].scenario.input_identity().unwrap()));
            }
        } else {
            assert!(!report.unverified.is_empty(), "{history}");
            assert_eq!(report.log[0].disposition, Disposition::Unverified);
        }
        return;
    }
    assert_eq!(report.questions.len(), 1, "{:?}", report.unverified);
    assert!(report.questions[0]
        .witnesses
        .iter()
        .any(
            |w| w.initial_scenario().identity().unwrap() == additional_digest
                && w.witness()
                    .after
                    .observations
                    .iter()
                    .any(|o| o.values["selected_count"]
                        == DataValue::Integer {
                            value: if changed { 4 } else { 2 }
                        })
        ));
}

#[test]
fn unchanged_captured_channels_still_validate_alternative_types_and_presence() {
    for change in ["removed", "retyped", "added"] {
        let (mut request, mut result) = input();
        for index in 0..2 {
            let mut program: Value =
                serde_json::from_slice(&request.sources[index].source_bytes).unwrap();
            program["views"][0]["kind"]["selection"] = Value::Null;
            program["observables"].as_array_mut().unwrap().push(json!({"id":"secondary","label":"Secondary","value":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":7}}}));
            request.sources[index] = capture(program);
            result.response.candidates[index].source_json =
                String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
        }
        let mut alternative: Value =
            serde_json::from_slice(&request.sources[0].source_bytes).unwrap();
        match change {
            "removed" => {
                alternative["observables"].as_array_mut().unwrap().pop();
            }
            "retyped" => {
                alternative["observables"][1]["value"] = json!({"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"seven"}});
            }
            _ => {
                alternative["observables"][1]["id"] = json!("alternative_only");
            }
        }
        result.response.candidates[0].source_json = serde_json::to_string(&alternative).unwrap();
        result.response.hypotheses[0].sources[0].raw_digest =
            request.sources[1].artifact.raw_digest.clone();
        refresh(&request, &mut result);
        let report = run(&request, &result, DiscoveryPolicy::default());
        assert_eq!(report.questions.len(), 1, "{change}");
        assert!(
            report
                .unverified
                .iter()
                .any(|message| message.contains("secondary")),
            "{change}: {:?}",
            report.unverified
        );
        if change == "added" {
            assert!(report
                .unverified
                .iter()
                .any(|message| message.contains("alternative_only")));
        }
        assert_eq!(report.log[0].state, EvidenceState::Inconclusive, "{change}");
        assert!(report.questions[0]
            .witnesses
            .iter()
            .all(|w| w.initial_runs().0.state == EvidenceState::Observed
                && w.initial_runs().1.state == EvidenceState::Observed));
    }
}

#[test]
fn supported_comparisons_do_not_hide_unrepresented_material_without_history() {
    let (mut request, mut result) = input();
    // Isolate the supported count/export contrast from selection presentation.
    for index in 0..2 {
        let mut program: Value =
            serde_json::from_slice(&request.sources[index].source_bytes).unwrap();
        program["views"][0]["kind"]["selection"] = Value::Null;
        request.sources[index] = capture(program);
        result.response.candidates[index].source_json =
            String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
    }
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    refresh(&request, &mut result);
    assert!(run(&request, &result, DiscoveryPolicy::default())
        .unverified
        .is_empty());
    for change in ["toolbar_action", "row_action", "output_format"] {
        for side in ["candidate", "alternative", "alternative_only"] {
            let (mut request, mut result) = input();
            if change == "row_action" {
                for index in 0..2 {
                    let mut program: Value =
                        serde_json::from_slice(&request.sources[index].source_bytes).unwrap();
                    let mut binding = program["views"][0]["actions"][0].clone();
                    binding["id"] = json!("row_export");
                    binding["placement"] = json!("row");
                    program["views"][0]["actions"]
                        .as_array_mut()
                        .unwrap()
                        .push(binding);
                    request.sources[index] = capture(program);
                    result.response.candidates[index].source_json =
                        String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
                }
            }
            let source = usize::from(side != "alternative");
            let mut program: Value =
                serde_json::from_slice(&request.sources[source].source_bytes).unwrap();
            if change == "output_format" {
                program["outputs"][0]["format"] = json!("json");
            } else {
                let index = usize::from(change == "row_action");
                program["views"][0]["actions"][index]["enabled"] = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}});
            }
            let modified = capture(program);
            if side == "candidate" {
                request.sources[1] = modified.clone();
            }
            result.response.candidates[usize::from(side == "candidate")].source_json =
                String::from_utf8(modified.source_bytes.clone()).unwrap();
            result.response.hypotheses[0].sources[0].raw_digest =
                request.sources[1].artifact.raw_digest.clone();
            refresh(&request, &mut result);
            let report = run(&request, &result, DiscoveryPolicy::default());
            let channel = match change {
                "toolbar_action" => "done/actions",
                "row_action" => "done/row_actions",
                _ => "done/output/0/format",
            };
            assert!(
                report
                    .unverified
                    .iter()
                    .any(|message| message.contains(channel)),
                "{change}/{side}: {:?}",
                report.unverified
            );
            assert_eq!(
                report.log[0].state,
                EvidenceState::Inconclusive,
                "{change}/{side}"
            );
            if side == "alternative_only" {
                assert!(report.questions.is_empty(), "{change}/{side}");
                assert_eq!(
                    report.log[0].disposition,
                    Disposition::Unverified,
                    "{change}/{side}"
                );
            } else {
                assert_eq!(report.questions.len(), 1, "{change}/{side}");
                assert!(report.questions[0]
                    .witnesses
                    .iter()
                    .all(|w| w.witness().before.state == EvidenceState::Observed
                        && w.witness().after.state == EvidenceState::Observed));
            }
        }
    }
}

#[test]
fn familiar_contrasts_do_not_settle_new_unrepresented_material_changes() {
    for change in ["toolbar_action", "row_action", "output_format"] {
        let (mut request, mut result) = input();
        if change == "row_action" {
            for index in 0..2 {
                let mut program: Value =
                    serde_json::from_slice(&request.sources[index].source_bytes).unwrap();
                let mut binding = program["views"][0]["actions"][0].clone();
                binding["id"] = json!("row_export");
                binding["placement"] = json!("row");
                program["views"][0]["actions"]
                    .as_array_mut()
                    .unwrap()
                    .push(binding);
                request.sources[index] = capture(program);
                result.response.candidates[index].source_json =
                    String::from_utf8(request.sources[index].source_bytes.clone()).unwrap();
            }
            result.response.hypotheses[0].sources[0].raw_digest =
                request.sources[1].artifact.raw_digest.clone();
            refresh(&request, &mut result);
        }
        let prior = saved_decision(
            &mut request,
            &result,
            DecisionOutcome::EitherAcceptable,
            vec![],
        );
        let expected = prior.after.observations.clone();
        let mut after: Value = serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
        if change == "output_format" {
            after["outputs"][0]["format"] = json!("json");
        } else {
            let index = usize::from(change == "row_action");
            after["views"][0]["actions"][index]["enabled"] = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}});
        }
        request.sources[1] = capture(after);
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
        assert!(
            report.runs.iter().any(|r| r.id == "retained-after"
                && r.state == EvidenceState::Observed
                && r.observations
                    .iter()
                    .zip(&expected)
                    .any(|(a, b)| a.view != b.view || a.outputs != b.outputs)),
            "{change}"
        );
        assert!(
            report.questions.is_empty(),
            "unrepresented change cannot become an invented choice: {change}"
        );
        assert!(!report.unverified.is_empty(), "{change}");
        assert_eq!(
            report.log[0].disposition,
            Disposition::Unverified,
            "{change}"
        );
    }
}

#[test]
fn retained_search_preserves_history_and_obeys_current_workflow_validity() {
    for action in ["collect", "export_people"] {
        assert_current_retained_workflow_validity(action);
    }
}
fn assert_current_retained_workflow_validity(hypothesis_action: &str) {
    let (mut request, mut result) = input();
    let observed_count = PropertyTerm::Observed {
        point: "done".into(),
        observable: "selected_count".into(),
        value_type: Type::Integer,
    };
    let mut scene = result.response.hypotheses[0].scenario().unwrap();
    scene.validity.push(AcceptedProperty {
        id: "historical-upper-bound".into(),
        description: "The approved scene has fewer than three collected rows".into(),
        predicate: PropertyPredicate::Less {
            left: observed_count.clone(),
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 3 },
            },
        },
    });
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    let prior_digest = prior.identity().unwrap();
    result.response.hypotheses[0].action = hypothesis_action.into();
    let mut after: Value = serde_json::from_slice(&request.sources[1].source_bytes).unwrap();
    let unfiltered = after["actions"][1]["steps"][0]["value"].clone();
    after["actions"][1]["steps"][0]["value"] = json!({"kind":"if","condition":{"kind":"equal","left":{"kind":"state","state":"search"},"right":text("")},"then_value":unfiltered,"else_value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}});
    request.sources[1] = capture(after);
    result.response.candidates[1].source_json =
        String::from_utf8(request.sources[1].source_bytes.clone()).unwrap();
    result.response.hypotheses[0].sources[0].raw_digest =
        request.sources[1].artifact.raw_digest.clone();
    scene
        .inputs
        .retain(|input| !matches!(input, SemanticInput::Control { .. }));
    result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
    refresh(&request, &mut result);
    let positive = AcceptedProperty {
        id: "current-positive-count".into(),
        description: "Legitimate comparison work must collect at least one row".into(),
        predicate: PropertyPredicate::Less {
            left: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 0 },
            },
            right: observed_count,
        },
    };
    let mut checked_scene = prior.scenario.clone();
    checked_scene.validity.push(positive.clone());
    let report = run(
        &request,
        &result,
        DiscoveryPolicy {
            retained_witnesses: vec![prior.clone()],
            workflow_validity: [("collect".into(), vec![positive])].into_iter().collect(),
            ..DiscoveryPolicy::default()
        },
    );
    assert!(
        report.questions.is_empty(),
        "an invalid retained workflow cannot create a choice"
    );
    assert!(!report.unverified.is_empty());
    let invalid = report
        .runs
        .iter()
        .find(|run| run.id == "captured-retained-candidate")
        .unwrap();
    assert_ne!(invalid.state, EvidenceState::Observed);
    assert_eq!(
        invalid.binding.scenario_digest,
        checked_scene.identity().unwrap()
    );
    assert!(invalid
        .observations
        .iter()
        .any(|o| o.values["selected_count"] == DataValue::Integer { value: 0 }));
    assert_eq!(prior.identity().unwrap(), prior_digest);
}

#[test]
fn relabeling_a_consumer_does_not_reask_a_settled_executed_producer() {
    let (mut request, mut result) = input();
    let prior = saved_decision(
        &mut request,
        &result,
        DecisionOutcome::EitherAcceptable,
        vec![],
    );
    result.response.hypotheses[0].action = "export_people".into();
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
    assert_eq!(report.log[0].disposition, Disposition::Settled);
}

#[test]
fn different_action_labels_group_the_same_verified_contrast() {
    let (request, mut result) = input();
    let mut consumer = result.response.hypotheses[0].clone();
    consumer.id = "export-perspective".into();
    consumer.action = "export_people".into();
    consumer
        .unknowns
        .push("Export scope remains undecided".into());
    result.response.hypotheses.push(consumer);
    let report = run(&request, &result, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    assert_eq!(
        report.questions[0].hypotheses,
        vec!["choice", "export-perspective"]
    );
    assert!(report.questions[0]
        .unknowns
        .contains(&"Export scope remains undecided".into()));
    assert_eq!(report.log[1].disposition, Disposition::Grouped);
    assert!(!report.questions[0].witnesses.is_empty());
}

#[test]
fn a_bridge_hypothesis_merges_every_connected_evidence_group() {
    let (request, mut result) = input();
    let mut clear = filtered();
    clear["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}});
    result.response.candidates.push(GeneratedCandidate {
        id: "clear".into(),
        source_json: serde_json::to_string(&clear).unwrap(),
    });
    result.response.hypotheses[0].unknowns = vec!["first boundary".into()];
    let mut other = result.response.hypotheses[0].clone();
    other.id = "clear-export".into();
    other.action = "export_people".into();
    other.alternatives = vec!["clear".into(), "replace".into()];
    other.unknowns = vec!["second boundary".into()];
    let mut bridge = result.response.hypotheses[0].clone();
    bridge.id = "bridge".into();
    bridge.alternatives = vec!["retain".into(), "clear".into(), "replace".into()];
    bridge.unknowns = vec!["bridge boundary".into()];
    result.response.hypotheses.extend([other, bridge]);
    let report = run(&request, &result, DiscoveryPolicy::default());
    assert_eq!(report.questions.len(), 1);
    let question = &report.questions[0];
    assert_eq!(
        question.hypotheses,
        vec!["choice", "clear-export", "bridge"]
    );
    assert_eq!(
        question.unknowns,
        vec!["first boundary", "second boundary", "bridge boundary"]
    );
    assert_eq!(
        question
            .witnesses
            .iter()
            .map(|w| w.before_program().artifact.program_digest.clone())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        report
            .log
            .iter()
            .filter(|l| l.disposition == Disposition::Question)
            .count(),
        1
    );
}

#[test]
fn current_workflow_constraints_apply_to_requirement_and_intention_prechecks() {
    for check in ["requirement", "obligation", "chosen"] {
        let (mut request, mut result) = input();
        let mut policy = DiscoveryPolicy::default();
        let mut bad = filtered();
        let observed_count = PropertyTerm::Observed {
            point: "done".into(),
            observable: "selected_count".into(),
            value_type: Type::Integer,
        };
        if check == "chosen" {
            let selected = request.sources[1].artifact.program_digest.clone();
            let prior = saved_decision(
                &mut request,
                &result,
                DecisionOutcome::Accept { artifact: selected },
                vec![],
            );
            request.decisions.decisions[0].scope.operations =
                ["export_people".into()].into_iter().collect();
            policy.retained_witnesses.push(prior);
            let filtered_rows = bad["actions"][1]["steps"][0]["items"].clone();
            bad["actions"][2]["steps"].as_array_mut().unwrap().insert(
                0,
                json!({"kind":"set_state","state":"selected","value":filtered_rows}),
            );
            policy.workflow_validity.insert(
                "export_people".into(),
                vec![AcceptedProperty {
                    id: "current-export-minimum".into(),
                    description: "The current legitimate workflow requires two collected rows"
                        .into(),
                    predicate: PropertyPredicate::Not {
                        value: Box::new(PropertyPredicate::Less {
                            left: observed_count.clone(),
                            right: PropertyTerm::Literal {
                                value_type: Type::Integer,
                                value: DataValue::Integer { value: 2 },
                            },
                        }),
                    },
                }],
            );
        } else {
            bad["actions"][2]["steps"] = json!([{"kind":"set_state","state":"search","value":{"kind":"state","state":"search"}}]);
            policy.workflow_validity.insert(
                "export_people".into(),
                vec![AcceptedProperty {
                    id: "current-export-positive".into(),
                    description: "A legitimate export must emit at least one row".into(),
                    predicate: PropertyPredicate::Less {
                        left: PropertyTerm::Literal {
                            value_type: Type::Integer,
                            value: DataValue::Integer { value: 0 },
                        },
                        right: PropertyTerm::OutputCount {
                            point: "done".into(),
                            output: "roster".into(),
                        },
                    },
                }],
            );
            if check == "requirement" {
                policy.requirements.push(RequirementCase {
                    id: "checked-export".into(),
                    scenario: scenario(
                        &request.sources[0],
                        vec![
                            add("Ada"),
                            invoke("collect", Values::new()),
                            invoke("export_people", Values::new()),
                            SemanticInput::Observe {
                                point: "done".into(),
                            },
                        ],
                    ),
                    properties: vec![count_is(1)],
                });
            } else {
                let property = AcceptedProperty {
                    id: "positive-selection".into(),
                    description: "Preserve a positive selected count".into(),
                    predicate: PropertyPredicate::Less {
                        left: PropertyTerm::Literal {
                            value_type: Type::Integer,
                            value: DataValue::Integer { value: 0 },
                        },
                        right: observed_count,
                    },
                };
                let prior = saved_decision(
                    &mut request,
                    &result,
                    DecisionOutcome::EitherAcceptable,
                    vec![property],
                );
                request.decisions.decisions[0].scope.operations =
                    ["export_people".into()].into_iter().collect();
                policy.retained_witnesses.push(prior);
            }
        }
        result.response.candidates.push(GeneratedCandidate {
            id: "bad-precheck".into(),
            source_json: serde_json::to_string(&bad).unwrap(),
        });
        result.response.hypotheses[0].alternatives = vec!["bad-precheck".into(), "replace".into()];
        let mut scene = result.response.hypotheses[0].scenario().unwrap();
        scene.inputs.retain(|input| !matches!(input, SemanticInput::Invoke { action, .. } if action == "export_people"));
        result.response.hypotheses[0].scenario_json = serde_json::to_string(&scene).unwrap();
        refresh(&request, &mut result);
        let report = run(&request, &result, policy);
        assert!(
            report.questions.is_empty(),
            "invalid {check} precheck allowed a preference"
        );
        assert!(!report.unverified.is_empty(), "{check}");
        assert!(
            report
                .runs
                .iter()
                .any(|r| r.state != EvidenceState::Observed),
            "{check}"
        );
    }
}
