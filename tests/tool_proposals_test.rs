#[path = "fixtures/tool_project_fixture.rs"]
mod fixture;
#[path = "../src/tool_decisions.rs"]
mod tool_decisions;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
#[path = "../src/tool_proposals.rs"]
mod tool_proposals;
#[path = "../src/tool_runtime.rs"]
mod tool_runtime;
#[path = "../src/tool_store.rs"]
mod tool_store;

use serde_json::{json, Value};
use tool_decisions::*;
use tool_project::*;
use tool_proposals::*;

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    fixture::at(2026, 10, 5, 12)
}
fn day() -> chrono::NaiveDate {
    fixture::date(2026, 10, 5)
}
fn project() -> ProjectSnapshot {
    fixture::studio_order_project("proposal-project")
}
fn example(snapshot: &ProjectSnapshot) -> SelectedExample {
    SelectedExample {
        disclosure: ExampleDisclosure::Synthetic,
        case: RehearsalCase {
            scenario: scenario_from_snapshot(snapshot, "example-1", "虚构例子", day(), now())
                .unwrap(),
            target: RehearsalTarget::Record("record-1".into()),
        },
    }
}
fn selection(snapshot: &ProjectSnapshot) -> ExportSelection {
    ExportSelection {
        original_request: "等材料时顺延显示目标，保留原始承诺".into(),
        confirmed_intents: vec![],
        examples: vec![example(snapshot)],
        task: None,
    }
}
fn export(snapshot: &ProjectSnapshot) -> RequirementExport {
    export_requirements(snapshot, selection(snapshot)).unwrap()
}
fn bytes(package: &ProposalPackage) -> Vec<u8> {
    serde_json::to_vec(package).unwrap()
}
fn changed_package(export: &RequirementExport) -> ProposalPackage {
    let mut package = export.proposal_template();
    package.envelope.candidate_policies[0].due_date = DateDuePolicy::ExtendByPausedDays;
    package.candidates[0].rule_keys = vec![RuleKey::DeliveryTarget];
    package
}
fn replay_selection(snapshot: &ProjectSnapshot) -> ReplaySelection {
    ReplaySelection {
        candidate_id: "candidate-1".into(),
        request_id: "request-import".into(),
        operation_id: "operation-import".into(),
        decision_id: "decision-import".into(),
        revision_id: "revision-import".into(),
        scope: freeze_scope(snapshot, ScopeKind::SingleRecord, Some("record-1")).unwrap(),
        supersedes: vec![],
    }
}
fn import_value(
    snapshot: &ProjectSnapshot,
    export: &RequirementExport,
    value: Value,
) -> Result<AcceptedProposal, ProposalError> {
    import_proposal(
        serde_json::to_vec(&value).unwrap().as_slice(),
        snapshot,
        export,
        None,
    )
}

#[test]
fn legal_import_requires_native_replay_and_explicit_preparation() {
    let snapshot = project();
    let before = serde_json::to_vec(&snapshot).unwrap();
    let export = export(&snapshot);
    let imported = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    assert_eq!(imported.envelope().source, ProposalSource::ExternalHarness);
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &imported, &selected, None, day(), now()).unwrap();
    let evidence = &replay.preview().evidence[0];
    assert_eq!(evidence.source, EvidenceSource::NativeExecution);
    assert_eq!(evidence.observations[0].elapsed_work_days, Some(1));
    assert_eq!(
        evidence.observations[0].display_due_date,
        Some(fixture::date(2026, 10, 9))
    );
    let prepared =
        prepare_proposal_adoption(&snapshot, &imported, &replay, &selected, None, day(), now())
            .unwrap();
    assert_eq!(prepared.snapshot().decisions.len(), 1);
    assert_eq!(prepared.snapshot().records, snapshot.records);
    assert_eq!(before, serde_json::to_vec(&snapshot).unwrap());
}

#[test]
fn wrong_project_version_runtime_and_requirement_identity_are_rejected() {
    let snapshot = project();
    let export = export(&snapshot);
    for (pointer, value) in [
        ("/format", json!("shell-script")),
        ("/format_version", json!(2)),
        ("/runtime_semantics_version", json!(999)),
        ("/envelope/project_id", json!("other-project")),
        ("/requirements_fingerprint", json!("f".repeat(64))),
        ("/envelope/baseline_fingerprint", json!("f".repeat(64))),
    ] {
        let mut package = serde_json::to_value(export.proposal_template()).unwrap();
        *package.pointer_mut(pointer).unwrap() = value;
        assert!(
            import_value(&snapshot, &export, package).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn changed_project_invalidates_import_replay_and_adoption() {
    let snapshot = project();
    let export = export(&snapshot);
    let package = changed_package(&export);
    let imported = import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).unwrap();
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &imported, &selected, None, day(), now()).unwrap();
    let mut changed = snapshot.clone();
    changed.project_name = "changed, same generation".into();
    assert!(matches!(
        import_proposal(bytes(&package).as_slice(), &changed, &export, None),
        Err(ProposalError::StaleBase)
    ));
    assert!(matches!(
        rehearse_proposal(&changed, &imported, &selected, None, day(), now()),
        Err(ProposalError::StaleBase)
    ));
    assert!(
        prepare_proposal_adoption(&changed, &imported, &replay, &selected, None, day(), now())
            .is_err()
    );
}

#[test]
fn task_adapter_reports_missing_or_changed_without_affecting_daily_work() {
    let snapshot = project();
    let task = TaskIdentity {
        task_id: "task-123".into(),
        candidate_fingerprint: "a".repeat(64),
    };
    let changed = TaskIdentity {
        candidate_fingerprint: "b".repeat(64),
        ..task.clone()
    };
    assert_eq!(
        task_association_status(Some(&task), None),
        TaskAssociationStatus::Missing
    );
    assert_eq!(
        task_association_status(Some(&task), Some(&changed)),
        TaskAssociationStatus::Changed
    );
    assert_eq!(
        task_association_status(None, Some(&task)),
        TaskAssociationStatus::Unlinked
    );
    let mut selected_export = selection(&snapshot);
    selected_export.task = Some(task.clone());
    let export = export_requirements(&snapshot, selected_export).unwrap();
    let package = changed_package(&export);
    for current in [None, Some(&changed)] {
        assert!(matches!(
            import_proposal(bytes(&package).as_slice(), &snapshot, &export, current),
            Err(ProposalError::TaskAssociation(_))
        ));
    }
    let accepted =
        import_proposal(bytes(&package).as_slice(), &snapshot, &export, Some(&task)).unwrap();
    let selected = replay_selection(&snapshot);
    let replay =
        rehearse_proposal(&snapshot, &accepted, &selected, Some(&task), day(), now()).unwrap();
    assert!(prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        Some(&changed),
        day(),
        now()
    )
    .is_err());
    assert!(evaluate_bound_record(&snapshot, "record-1", day()).is_ok());
    let unlinked_export = self::export(&snapshot);
    assert!(import_proposal(
        bytes(&unlinked_export.proposal_template()).as_slice(),
        &snapshot,
        &unlinked_export,
        Some(&changed)
    )
    .is_ok());
}

#[test]
fn imported_task_cannot_be_substituted_or_silently_dropped() {
    let snapshot = project();
    let mut selected = selection(&snapshot);
    let task = TaskIdentity {
        task_id: "task-123".into(),
        candidate_fingerprint: "a".repeat(64),
    };
    selected.task = Some(task.clone());
    let export = export_requirements(&snapshot, selected).unwrap();
    let mut package = export.proposal_template();
    package.task = None;
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, Some(&task)).is_err());
    package.task = Some(TaskIdentity {
        task_id: "task-other".into(),
        ..task.clone()
    });
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, Some(&task)).is_err());
}

#[test]
fn unsupported_fields_scripts_paths_roles_and_operations_are_rejected() {
    let snapshot = project();
    let export = export(&snapshot);
    let template = serde_json::to_value(export.proposal_template()).unwrap();
    for pointer in [
        "",
        "/envelope",
        "/candidates/0",
        "/candidates/0/cases/0/scenario",
        "/candidates/0/cases/0/scenario/spec_revisions/0",
        "/candidates/0/cases/0/scenario/records/0",
        "/candidates/0/cases/0/scenario/event_history/0",
        "/envelope/candidate_policies/0/reminder",
    ] {
        let mut value = template.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "script".into(),
                json!("../../secret; curl https://evil.invalid"),
            );
        assert!(
            import_value(&snapshot, &export, value).is_err(),
            "{pointer}"
        );
    }
    for (pointer, value) in [
        (
            "/candidates/0/cases/0/scenario/spec_revisions/0/fields/0/role",
            json!("execute"),
        ),
        (
            "/candidates/0/cases/0/scenario/records/0/record_id",
            json!("../../secret"),
        ),
        ("/envelope/source", json!("local_built_in")),
        ("/envelope/candidate_policies/0/due_date", json!("shell")),
    ] {
        let mut package = template.clone();
        *package.pointer_mut(pointer).unwrap() = value;
        assert!(
            import_value(&snapshot, &export, package).is_err(),
            "{pointer}"
        );
    }
    let mut value = template;
    value["candidates"][0]["cases"][0]["scenario"]["steps"] =
        json!([{"step":"read_file","path":"../../secret"}]);
    assert!(import_value(&snapshot, &export, value).is_err());
}

#[test]
fn invalid_rule_parameters_and_field_references_are_rejected() {
    let snapshot = project();
    let export = export(&snapshot);
    for reminder in [
        json!({"rule":"waiting_after_days","days_waiting":0}),
        json!({"rule":"waiting_before_due","days_before_due":3651}),
        json!({"rule":"never","command":"whoami"}),
    ] {
        let mut value = serde_json::to_value(export.proposal_template()).unwrap();
        value["envelope"]["candidate_policies"][0]["reminder"] = reminder;
        assert!(import_value(&snapshot, &export, value).is_err());
    }
    let mut package = export.proposal_template();
    package.candidates[0].cases[0]
        .scenario
        .steps
        .push(ScenarioStep::EditRecord {
            operation_id: "edit-bad-field".into(),
            record_id: "record-1".into(),
            expected_record_revision: 2,
            changes: [("not-a-field".into(), Some(FieldValue::Boolean(true)))].into(),
            occurred_at: now(),
        });
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
}

#[test]
fn bounded_reader_is_used_for_size_depth_duplicates_and_io_failures() {
    let snapshot = project();
    let export = export(&snapshot);
    for bytes in [
        vec![b' '; tool_proposal_input::MAX_INPUT_BYTES + 1],
        format!("{}0{}", "[".repeat(65), "]".repeat(65)).into_bytes(),
        br#"{"format":1,"format":2}"#.to_vec(),
    ] {
        assert!(matches!(
            import_proposal(bytes.as_slice(), &snapshot, &export, None),
            Err(ProposalError::Input(_))
        ));
    }
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken input"))
        }
    }
    assert!(matches!(
        import_proposal(Broken, &snapshot, &export, None),
        Err(ProposalError::Input(_))
    ));
}

#[test]
fn candidate_case_operation_and_member_budgets_are_enforced() {
    let snapshot = project();
    let export = export(&snapshot);
    let template = export.proposal_template();
    let mut package = template.clone();
    package.candidates = vec![package.candidates[0].clone(); MAX_CANDIDATES + 1];
    package.envelope.candidate_policies = vec![BehaviorPolicy::default(); MAX_CANDIDATES + 1];
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package = template.clone();
    package.candidates[0].cases =
        vec![package.candidates[0].cases[0].clone(); MAX_CASES_PER_CANDIDATE + 1];
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package = template;
    package.candidates[0].cases[0].scenario.steps = vec![
        ScenarioStep::Observe {
            record_id: "record-1".into()
        };
        MAX_STEPS_PER_CASE + 1
    ];
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    let value = Value::Object(
        (0..=MAX_OBJECT_MEMBERS)
            .map(|i| (format!("field{i}"), Value::Null))
            .collect(),
    );
    assert!(matches!(
        import_value(&snapshot, &export, value),
        Err(ProposalError::Limit(_))
    ));
}

#[test]
fn candidate_ids_and_policies_must_be_one_to_one() {
    let snapshot = project();
    let export = export(&snapshot);
    let mut package = export.proposal_template();
    package.candidates.push(package.candidates[0].clone());
    package
        .envelope
        .candidate_policies
        .push(BehaviorPolicy::default());
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package.candidates[1].candidate_id = "candidate-2".into();
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_ok());
    package.envelope.candidate_policies.pop();
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
}

#[test]
fn forged_success_stays_untrusted_and_failing_expectations_block_adoption() {
    let snapshot = project();
    let export = export(&snapshot);
    let mut package = changed_package(&export);
    package
        .envelope
        .imported_claims
        .insert("passed".into(), json!(true));
    package
        .envelope
        .imported_claims
        .insert("summary".into(), json!("Guaranteed success"));
    package.candidates[0].cases[0].scenario.expected = vec![EvidenceObservation::from(
        &evaluate_bound_record(&snapshot, "record-1", day()).unwrap(),
    )];
    let accepted = import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).unwrap();
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    assert_eq!(replay.preview().evidence[0].status, EvidenceStatus::Failed);
    assert!(prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        None,
        day(),
        now()
    )
    .is_err());
    let mut value = serde_json::to_value(&package).unwrap();
    value["evidence"] = json!({"source":"native_execution","status":"passed"});
    assert!(import_value(&snapshot, &export, value).is_err());
    package
        .envelope
        .imported_claims
        .insert("execution".into(), json!({"script":"rm -rf /"}));
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
}

#[test]
fn candidate_switch_or_package_edit_cannot_reuse_native_preview() {
    let snapshot = project();
    let export = export(&snapshot);
    let mut package = changed_package(&export);
    let accepted = import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).unwrap();
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    package.envelope.candidate_policies[0].due_date = DateDuePolicy::KeepOriginal;
    let changed = import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).unwrap();
    assert!(matches!(
        prepare_proposal_adoption(&snapshot, &changed, &replay, &selected, None, day(), now()),
        Err(ProposalError::StaleCandidate)
    ));
    let mut switched = selected;
    switched.candidate_id = "candidate-2".into();
    assert!(prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &switched,
        None,
        day(),
        now()
    )
    .is_err());
}

#[test]
fn export_uses_only_selected_sanitized_inputs_and_strips_extensions() {
    let mut snapshot = project();
    let mut selected = selection(&snapshot);
    selected.examples[0].disclosure = ExampleDisclosure::ExplicitlySelectedSanitized;
    snapshot.project_name = "private-customer-project".into();
    snapshot
        .extensions
        .insert("private_blob".into(), json!({"token":"private-token"}));
    snapshot.spec_revisions[0]
        .extensions
        .insert("private_spec".into(), json!("private-spec-blob"));
    selected.examples[0].case.scenario.spec_revisions = snapshot.spec_revisions.clone();
    selected.examples[0].case.scenario.records[0]
        .extensions
        .insert("private_path".into(), json!("/private/customer"));
    selected.examples[0].case.scenario.event_history[0]
        .extensions
        .insert("private_history".into(), json!("history-secret"));
    selected.examples[0]
        .case
        .scenario
        .extensions
        .insert("private_scenario".into(), json!("scenario-secret"));
    let export = export_requirements(&snapshot, selected).unwrap();
    let text = std::str::from_utf8(export.json()).unwrap();
    for private in [
        "private-customer-project",
        "private-token",
        "private-spec-blob",
        "/private/customer",
        "history-secret",
        "scenario-secret",
    ] {
        assert!(!text.contains(private), "{private}");
    }
    let accepted = import_proposal(
        bytes(&export.proposal_template()).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    assert!(rehearse_proposal(
        &snapshot,
        &accepted,
        &replay_selection(&snapshot),
        None,
        day(),
        now()
    )
    .is_ok());
}

#[test]
fn export_does_not_select_live_records_or_raw_notes_implicitly() {
    let mut snapshot = project();
    let selected = selection(&snapshot);
    snapshot = tool_runtime::apply_command(
        &snapshot,
        &ToolCommand::EditRecord {
            operation_id: "private-edit".into(),
            expected_generation: snapshot.generation,
            record_id: "record-1".into(),
            expected_record_revision: 2,
            changes: [(
                "notes".into(),
                Some(FieldValue::Text("PRIVATE-CUSTOMER-NOTES".into())),
            )]
            .into(),
            occurred_at: now(),
        },
        now(),
    )
    .unwrap();
    let export = export_requirements(&snapshot, selected).unwrap();
    assert!(!std::str::from_utf8(export.json())
        .unwrap()
        .contains("PRIVATE-CUSTOMER-NOTES"));
    let mut empty = selection(&snapshot);
    empty.examples.clear();
    assert!(export_requirements(&snapshot, empty).is_err());
}

#[test]
fn active_intent_projection_is_explicit_and_native_conflicts_are_preserved() {
    let snapshot = project();
    let export = export(&snapshot);
    let accepted = import_proposal(
        bytes(&export.proposal_template()).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    let prepared =
        prepare_proposal_adoption(&snapshot, &accepted, &replay, &selected, None, day(), now())
            .unwrap();
    let snapshot = prepared.snapshot().clone();
    assert!(export_requirements(&snapshot, selection(&snapshot)).is_err());
    let mut selected_export = selection(&snapshot);
    selected_export.examples[0].case.scenario.scenario_id = "example-2".into();
    selected_export.confirmed_intents.push(SelectedIntent {
        decision_id: "decision-import".into(),
        sanitized_intent: "保持已确认的交付目标".into(),
    });
    let export = export_requirements(&snapshot, selected_export).unwrap();
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    let mut selected = replay_selection(&snapshot);
    selected.request_id = "request-second".into();
    selected.operation_id = "operation-second".into();
    selected.decision_id = "decision-second".into();
    selected.revision_id = "revision-second".into();
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    assert_eq!(
        replay.preview().checked_decision_ids,
        vec!["decision-import"]
    );
    assert!(!replay.preview().conflicts.is_empty());
    assert!(prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        None,
        day(),
        now()
    )
    .is_err());
    selected.supersedes = vec!["decision-import".into()];
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    assert!(prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        None,
        day(),
        now()
    )
    .is_ok());
}

#[test]
fn stale_scope_clock_and_scenario_identity_use_native_validation() {
    let snapshot = project();
    let export = export(&snapshot);
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    let mut selected = replay_selection(&snapshot);
    selected.scope.effective_sequence += 1;
    assert!(matches!(
        rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()),
        Err(ProposalError::Decision(DecisionError::StaleEvidence))
    ));
    selected = replay_selection(&snapshot);
    assert!(rehearse_proposal(
        &snapshot,
        &accepted,
        &selected,
        None,
        day(),
        fixture::at(2026, 10, 5, 13)
    )
    .is_err());
    let mut package = export.proposal_template();
    package.candidates[0].cases[0].scenario.base_generation += 1;
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
}

#[test]
fn supported_scenario_operations_replay_without_changing_live_facts() {
    let snapshot = project();
    let export = export(&snapshot);
    let mut package = changed_package(&export);
    package.candidates[0].cases[0].scenario.steps = vec![
        ScenarioStep::EditRecord {
            operation_id: "edit-example".into(),
            record_id: "record-1".into(),
            expected_record_revision: 2,
            changes: [(
                "promised_on".into(),
                Some(FieldValue::Date(fixture::date(2026, 10, 10))),
            )]
            .into(),
            occurred_at: now(),
        },
        ScenarioStep::TransitionStage {
            operation_id: "resume-example".into(),
            record_id: "record-1".into(),
            expected_record_revision: 3,
            to_stage_id: "in_progress".into(),
            occurred_at: now(),
        },
        ScenarioStep::CreateRecord {
            operation_id: "create-example-2".into(),
            record_id: "example-record-2".into(),
            initial_stage_id: "queued".into(),
            values: snapshot.records[0].typed_values.clone(),
            occurred_at: now(),
        },
        ScenarioStep::AdvanceDate { as_of_date: day() },
        ScenarioStep::Observe {
            record_id: "record-1".into(),
        },
    ];
    let before = snapshot.clone();
    let accepted = import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).unwrap();
    let replay = rehearse_proposal(
        &snapshot,
        &accepted,
        &replay_selection(&snapshot),
        None,
        day(),
        now(),
    )
    .unwrap();
    let observation = &replay.preview().evidence[0].observations[0];
    assert_eq!(observation.current_stage_id, "in_progress");
    assert_eq!(
        observation.display_due_date,
        Some(fixture::date(2026, 10, 13))
    );
    assert_eq!(snapshot, before);
}

#[test]
fn non_order_domain_and_future_examples_use_shared_contracts() {
    let snapshot =
        fixture::active_waiting_project(fixture::maintenance_task_spec(), "repair-project");
    let mut selected_export = selection(&snapshot);
    selected_export.examples[0].case.target = RehearsalTarget::FutureRecord("record-1".into());
    let export = export_requirements(&snapshot, selected_export).unwrap();
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    let mut selected = replay_selection(&snapshot);
    selected.scope = freeze_scope(&snapshot, ScopeKind::FutureRecords, None).unwrap();
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    assert!(replay.preview().impacts.is_empty());
    assert_eq!(
        replay.preview().evidence[0].observations[0].display_due_date,
        Some(fixture::date(2026, 10, 9))
    );
    let prepared =
        prepare_proposal_adoption(&snapshot, &accepted, &replay, &selected, None, day(), now())
            .unwrap();
    assert_eq!(
        evaluate_bound_record(prepared.snapshot(), "record-1", day()).unwrap(),
        evaluate_bound_record(&snapshot, "record-1", day()).unwrap()
    );
}

#[test]
fn total_work_limits_cannot_be_bypassed_by_many_small_candidates() {
    let snapshot = project();
    let export = export(&snapshot);
    for (candidate_count, case_count, step_count) in [(5, 7, 0), (2, 5, 64)] {
        let mut package = export.proposal_template();
        let first = package.candidates[0].clone();
        package.candidates = (0..candidate_count)
            .map(|i| {
                let mut candidate = first.clone();
                candidate.candidate_id = format!("candidate-{i}");
                candidate.cases = (0..case_count)
                    .map(|j| {
                        let mut case = first.cases[0].clone();
                        case.scenario.scenario_id = format!("case-{i}-{j}");
                        case.scenario.steps = vec![
                            ScenarioStep::Observe {
                                record_id: "record-1".into()
                            };
                            step_count
                        ];
                        case
                    })
                    .collect();
                candidate
            })
            .collect();
        package.envelope.candidate_policies = vec![BehaviorPolicy::default(); candidate_count];
        assert!(matches!(
            import_proposal(bytes(&package).as_slice(), &snapshot, &export, None),
            Err(ProposalError::Limit(_))
        ));
    }
}

#[test]
fn empty_payloads_unknown_claims_and_missing_example_targets_are_rejected() {
    let snapshot = project();
    let export = export(&snapshot);
    let first = export.proposal_template();
    let mut package = first.clone();
    package.candidates.clear();
    package.envelope.candidate_policies.clear();
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package = first.clone();
    package.candidates[0].cases.clear();
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package = first.clone();
    package.candidates[0].cases[0].target = RehearsalTarget::Record("missing-record".into());
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
    package = first;
    package
        .envelope
        .imported_claims
        .insert("passed".into(), json!("true"));
    assert!(import_proposal(bytes(&package).as_slice(), &snapshot, &export, None).is_err());
}

#[test]
fn durable_provenance_survives_store_reopen_and_lost_reply_retry() {
    let snapshot = project();
    let task = TaskIdentity {
        task_id: "task-123".into(),
        candidate_fingerprint: "a".repeat(64),
    };
    let mut selected_export = selection(&snapshot);
    selected_export.task = Some(task.clone());
    let dir = tempfile::tempdir().unwrap();
    let store =
        tool_store::ProjectStore::create(dir.path().join("project"), &snapshot, "create-project")
            .unwrap();
    let snapshot = match store.load().unwrap() {
        tool_store::StoreLoad::Writable(snapshot) => snapshot,
        other => panic!("{other:?}"),
    };
    let export = export_requirements(&snapshot, selected_export).unwrap();
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        Some(&task),
    )
    .unwrap();
    let selected = replay_selection(&snapshot);
    let replay =
        rehearse_proposal(&snapshot, &accepted, &selected, Some(&task), day(), now()).unwrap();
    let prepared = prepare_proposal_adoption(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        Some(&task),
        day(),
        now(),
    )
    .unwrap();
    prepared.validate_for_commit(Some(&task)).unwrap();
    let first = store
        .commit(
            prepared.expected_generation(),
            prepared.operation_id(),
            prepared.snapshot(),
        )
        .unwrap();
    let retried = store
        .commit(
            prepared.expected_generation(),
            prepared.operation_id(),
            prepared.snapshot(),
        )
        .unwrap();
    assert_eq!(first.snapshot, retried.snapshot);
    let reopened = match store.load().unwrap() {
        tool_store::StoreLoad::Writable(snapshot) => snapshot,
        other => panic!("{other:?}"),
    };
    let saved = saved_proposal_association(&reopened, "decision-import", Some(&task))
        .unwrap()
        .unwrap();
    assert_eq!(saved.task_status, TaskAssociationStatus::Current);
    assert_eq!(saved.provenance.task, Some(task.clone()));
    assert_eq!(saved.provenance.source, ProposalSource::ExternalHarness);
    assert_eq!(saved.provenance.candidate_id, "candidate-1");
    assert_eq!(
        saved_proposal_association(&reopened, "decision-import", None)
            .unwrap()
            .unwrap()
            .task_status,
        TaskAssociationStatus::Missing
    );
    let changed = TaskIdentity {
        candidate_fingerprint: "b".repeat(64),
        ..task
    };
    assert_eq!(
        saved_proposal_association(&reopened, "decision-import", Some(&changed))
            .unwrap()
            .unwrap()
            .task_status,
        TaskAssociationStatus::Changed
    );
    assert!(prepared.validate_for_commit(Some(&changed)).is_err());
    assert!(evaluate_bound_record(&reopened, "record-1", day()).is_ok());
    assert_eq!(reopened.decisions.len(), 1);
    assert_eq!(reopened.records, snapshot.records);
    assert_eq!(
        reopened.evidence[0].input_fingerprint,
        replay.preview().evidence[0].input_fingerprint
    );
    assert!(tool_runtime::validate_native_evidence(&reopened).is_ok());
}

#[test]
fn provenance_tampering_is_rejected_without_invalidating_daily_records() {
    let snapshot = project();
    let export = export(&snapshot);
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        None,
    )
    .unwrap();
    let selected = replay_selection(&snapshot);
    let replay = rehearse_proposal(&snapshot, &accepted, &selected, None, day(), now()).unwrap();
    let prepared =
        prepare_proposal_adoption(&snapshot, &accepted, &replay, &selected, None, day(), now())
            .unwrap();
    let snapshot = prepared.snapshot();
    assert_eq!(
        saved_proposal_association(snapshot, "decision-import", None)
            .unwrap()
            .unwrap()
            .task_status,
        TaskAssociationStatus::Unlinked
    );
    for mutation in 0..5 {
        let mut changed = snapshot.clone();
        match mutation {
            0 => {
                changed.evidence[0].extensions.clear();
            }
            1 => {
                changed.decisions[0]
                    .extensions
                    .get_mut(PROVENANCE_KEY)
                    .unwrap()["version"] = json!(999);
            }
            2 => {
                changed.decisions[0]
                    .extensions
                    .get_mut(PROVENANCE_KEY)
                    .unwrap()["path"] = json!("/secret");
            }
            3 => {
                changed.evidence[0].candidate_fingerprint = "f".repeat(64);
            }
            _ => {
                changed.decisions[0].extensions.remove(PROVENANCE_KEY);
            }
        }
        assert!(saved_proposal_association(&changed, "decision-import", None).is_err());
        assert!(evaluate_bound_record(&changed, "record-1", day()).is_ok());
    }
}

#[test]
fn unresolved_import_choices_preserve_provenance_without_activating_rules() {
    let snapshot = project();
    let task = TaskIdentity {
        task_id: "task-123".into(),
        candidate_fingerprint: "a".repeat(64),
    };
    let mut selected_export = selection(&snapshot);
    selected_export.task = Some(task.clone());
    let export = export_requirements(&snapshot, selected_export).unwrap();
    let accepted = import_proposal(
        bytes(&changed_package(&export)).as_slice(),
        &snapshot,
        &export,
        Some(&task),
    )
    .unwrap();
    let selected = replay_selection(&snapshot);
    let replay =
        rehearse_proposal(&snapshot, &accepted, &selected, Some(&task), day(), now()).unwrap();
    for choice in [
        DecisionChoice::Both,
        DecisionChoice::Neither,
        DecisionChoice::Defer,
    ] {
        let prepared = prepare_proposal_unresolved(
            &snapshot,
            &accepted,
            &replay,
            &selected,
            Some(&task),
            choice,
            now(),
        )
        .unwrap();
        let decoded: ProjectSnapshot =
            serde_json::from_slice(&serde_json::to_vec(prepared.snapshot()).unwrap()).unwrap();
        assert_eq!(decoded.decisions[0].status, DecisionStatus::Pending);
        assert_eq!(decoded.decisions[0].choice, choice);
        assert!(decoded.evidence.is_empty());
        assert_eq!(decoded.rule_bindings, snapshot.rule_bindings);
        assert_eq!(decoded.records, snapshot.records);
        assert_eq!(
            saved_proposal_association(&decoded, "decision-import", None)
                .unwrap()
                .unwrap()
                .task_status,
            TaskAssociationStatus::Missing
        );
    }
    assert!(prepare_proposal_unresolved(
        &snapshot,
        &accepted,
        &replay,
        &selected,
        Some(&task),
        DecisionChoice::Adopt,
        now()
    )
    .is_err());
}
