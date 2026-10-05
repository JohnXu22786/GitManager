#[path = "fixtures/tool_project_fixture.rs"]
mod fixture;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_runtime.rs"]
mod tool_runtime;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use std::collections::BTreeMap;
use tool_project::{
    studio_order_template, BehaviorPolicy, DateDuePolicy, FieldKind, FieldRole, FieldValue,
    ReminderPolicy, TimerPolicy, ToolCommand,
};
use tool_runtime::{apply_command, evaluate_record, run_scenario, RuntimeError};

fn scenario_for_project(
    snapshot: &tool_project::ProjectSnapshot,
    scenario_id: &str,
) -> tool_project::Scenario {
    tool_project::Scenario {
        scenario_id: scenario_id.to_owned(),
        name: "场景基础数据校验".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings.clone(),
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 5),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: Vec::new(),
        expected: Vec::new(),
        extensions: BTreeMap::new(),
    }
}

#[test]
fn studio_and_repair_specs_validate_and_share_one_runtime() {
    let order_spec = studio_order_template();
    let repair_spec = fixture::maintenance_task_spec();
    assert!(order_spec.validate().is_ok());
    assert!(repair_spec.validate().is_ok());
    assert_ne!(order_spec.entity_type, repair_spec.entity_type);
    assert_ne!(order_spec.fields[0].id, repair_spec.fields[0].id);

    let order = fixture::studio_order_project("studio-project");
    let repair = fixture::active_waiting_project(repair_spec, "repair-project");
    let policy = fixture::standard_behavior();
    let as_of = fixture::date(2026, 10, 5);

    let order_result = evaluate_record(&order, "record-1", &policy, as_of).unwrap();
    let repair_result = evaluate_record(&repair, "record-1", &policy, as_of).unwrap();
    assert_eq!(order_result.elapsed_work_days, Some(1));
    assert_eq!(repair_result.elapsed_work_days, Some(1));
    assert_eq!(order_result.paused_days, 3);
    assert_eq!(repair_result.paused_days, 3);
    assert_eq!(
        order_result.display_due_date,
        Some(fixture::date(2026, 10, 6))
    );
    assert_eq!(
        repair_result.display_due_date,
        Some(fixture::date(2026, 10, 6))
    );
}

#[test]
fn invalid_typed_fields_and_dangling_stage_references_are_rejected() {
    let mut wrong_date_type = studio_order_template();
    wrong_date_type
        .fields
        .iter_mut()
        .find(|field| field.role == FieldRole::PromisedDate)
        .unwrap()
        .kind = FieldKind::Integer;
    assert!(wrong_date_type.validate().is_err());

    let mut dangling_transition = studio_order_template();
    dangling_transition.stages[0]
        .allowed_next_stage_ids
        .push("missing-stage".to_owned());
    assert!(dangling_transition.validate().is_err());

    let mut active_timer = studio_order_template();
    active_timer.stages[3].clock = tool_project::StageClock::Active;
    assert!(active_timer.validate().is_err());

    let mut compatible_revision =
        fixture::empty_project(studio_order_template(), "revision-project");
    let mut next_spec = compatible_revision.active_spec().unwrap().clone();
    next_spec.revision = 2;
    next_spec.display_name = "工作室订单（更新）".to_owned();
    next_spec.fields.push(tool_project::FieldDefinition {
        id: "priority".to_owned(),
        display_name: "优先级".to_owned(),
        kind: FieldKind::Integer,
        role: FieldRole::Custom,
        required: false,
        extensions: BTreeMap::new(),
    });
    compatible_revision.spec_revisions.push(next_spec.clone());
    compatible_revision.active_spec = (&next_spec).into();
    assert!(compatible_revision.validate().is_ok());

    let mut changed_type = compatible_revision.clone();
    changed_type.spec_revisions[1].fields[0].kind = FieldKind::Integer;
    assert!(changed_type.validate().is_err());
    let mut removed_field = compatible_revision.clone();
    removed_field.spec_revisions[1]
        .fields
        .retain(|field| field.id != "notes");
    assert!(removed_field.validate().is_err());
    let mut required_addition = compatible_revision.clone();
    required_addition.spec_revisions[1]
        .fields
        .iter_mut()
        .find(|field| field.id == "priority")
        .unwrap()
        .required = true;
    assert!(required_addition.validate().is_err());

    let mut snapshot = fixture::studio_order_project("typed-project");
    let before = serde_json::to_vec(&snapshot).unwrap();
    let error = apply_command(
        &snapshot,
        &ToolCommand::EditRecord {
            operation_id: "bad-edit".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            changes: BTreeMap::from([(
                "promised_on".to_owned(),
                Some(FieldValue::Text("tomorrow".to_owned())),
            )]),
            occurred_at: fixture::at(2026, 10, 3, 9),
        },
        fixture::at(2026, 10, 3, 9),
    )
    .unwrap_err();
    assert!(matches!(error, RuntimeError::ProjectValidation(_)));
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);

    snapshot.records[0].current_stage_id = "not-a-stage".to_owned();
    assert!(snapshot.validate().is_err());
}

#[test]
fn required_text_fields_reject_blank_values_on_create_and_edit() {
    let template = studio_order_template();
    let valid_values = fixture::studio_order_project("required-text-source").records[0]
        .typed_values
        .clone();
    let required_text_field_ids: Vec<_> = template
        .fields
        .iter()
        .filter(|field| {
            field.required
                && field.kind == FieldKind::Text
                && matches!(field.role, FieldRole::Title | FieldRole::WorkDescription)
        })
        .map(|field| field.id.clone())
        .collect();
    assert_eq!(required_text_field_ids.len(), 2);

    for (field_index, field_id) in required_text_field_ids.iter().enumerate() {
        for (blank_index, blank) in ["", " \t\n"].iter().enumerate() {
            let mut values = valid_values.clone();
            values.insert(field_id.clone(), FieldValue::Text((*blank).to_owned()));
            let empty = fixture::empty_project(template.clone(), "required-text-create-project");
            let create_error = apply_command(
                &empty,
                &ToolCommand::CreateRecord {
                    operation_id: format!("blank-create-{field_index}-{blank_index}"),
                    expected_generation: empty.generation,
                    record_id: "blank-create-record".to_owned(),
                    initial_stage_id: template.default_stage_id.clone(),
                    values,
                    occurred_at: fixture::at(2026, 10, 1, 9),
                },
                fixture::at(2026, 10, 1, 9),
            )
            .unwrap_err();
            assert!(matches!(
                create_error,
                RuntimeError::ProjectValidation(ref error)
                    if error.code == tool_project::ValidationCode::MissingRequiredField
            ));

            let existing = fixture::studio_order_project("required-text-edit-project");
            let record = &existing.records[0];
            let edit_error = apply_command(
                &existing,
                &ToolCommand::EditRecord {
                    operation_id: format!("blank-edit-{field_index}-{blank_index}"),
                    expected_generation: existing.generation,
                    record_id: record.record_id.clone(),
                    expected_record_revision: record.record_revision,
                    changes: BTreeMap::from([(
                        field_id.clone(),
                        Some(FieldValue::Text((*blank).to_owned())),
                    )]),
                    occurred_at: fixture::at(2026, 10, 3, 9),
                },
                fixture::at(2026, 10, 3, 9),
            )
            .unwrap_err();
            assert!(matches!(
                edit_error,
                RuntimeError::ProjectValidation(ref error)
                    if error.code == tool_project::ValidationCode::MissingRequiredField
            ));
        }
    }
}

#[test]
fn behavior_revision_cannot_be_its_own_parent() {
    let mut snapshot = fixture::empty_project(studio_order_template(), "self-parent-project");
    snapshot
        .behavior_revisions
        .push(tool_project::BehaviorRevision {
            revision_id: "behavior-2".to_owned(),
            operation_id: None,
            parent_revision_id: Some("behavior-2".to_owned()),
            policy: fixture::standard_behavior(),
            reason: "A self-parenting revision is invalid".to_owned(),
            created_at: fixture::at(2026, 10, 1, 8),
        });
    snapshot.active_behavior_revision_id = "behavior-2".to_owned();

    assert!(matches!(
        snapshot.validate().unwrap_err().code,
        tool_project::ValidationCode::DanglingReference
    ));
}

#[test]
fn set_behavior_operation_id_cannot_create_a_second_revision() {
    let snapshot = fixture::empty_project(studio_order_template(), "behavior-operation-project");
    let mut first_policy = fixture::standard_behavior();
    first_policy.due_date = DateDuePolicy::ExtendByPausedDays;
    let first = apply_command(
        &snapshot,
        &ToolCommand::SetBehavior {
            operation_id: "set-behavior-once".to_owned(),
            expected_generation: snapshot.generation,
            revision_id: "behavior-2".to_owned(),
            parent_revision_id: Some("behavior-1".to_owned()),
            policy: first_policy,
            reason: "Extend the target by paused days".to_owned(),
            occurred_at: fixture::at(2026, 10, 2, 9),
        },
        fixture::at(2026, 10, 2, 9),
    )
    .unwrap();
    let first: tool_project::ProjectSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();

    let duplicate = apply_command(
        &first,
        &ToolCommand::SetBehavior {
            operation_id: "set-behavior-once".to_owned(),
            expected_generation: first.generation,
            revision_id: "behavior-3".to_owned(),
            parent_revision_id: Some("behavior-2".to_owned()),
            policy: fixture::standard_behavior(),
            reason: "Change the target back".to_owned(),
            occurred_at: fixture::at(2026, 10, 3, 9),
        },
        fixture::at(2026, 10, 3, 9),
    )
    .unwrap_err();

    assert!(matches!(duplicate, RuntimeError::InvalidOperation(_)));
    assert_eq!(first.behavior_revisions.len(), 2);
}

#[test]
fn illegal_and_future_transitions_are_rejected_without_changing_history() {
    let snapshot = fixture::studio_order_project("transition-project");
    let before = serde_json::to_vec(&snapshot).unwrap();
    let illegal = apply_command(
        &snapshot,
        &ToolCommand::TransitionStage {
            operation_id: "illegal-transition".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            to_stage_id: "queued".to_owned(),
            occurred_at: fixture::at(2026, 10, 3, 9),
        },
        fixture::at(2026, 10, 3, 9),
    )
    .unwrap_err();
    assert!(matches!(illegal, RuntimeError::InvalidTransition { .. }));

    let future = apply_command(
        &snapshot,
        &ToolCommand::TransitionStage {
            operation_id: "future-transition".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            to_stage_id: "in_progress".to_owned(),
            occurred_at: fixture::at(2026, 10, 6, 9),
        },
        fixture::at(2026, 10, 5, 9),
    )
    .unwrap_err();
    assert!(matches!(future, RuntimeError::FutureEvent { .. }));
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
}

#[test]
fn calendar_runtime_computes_waiting_days_and_keeps_the_original_deadline() {
    let snapshot = fixture::studio_order_project("calendar-project");
    let original_policy = fixture::standard_behavior();
    let result = evaluate_record(
        &snapshot,
        "record-1",
        &original_policy,
        fixture::date(2026, 10, 5),
    )
    .unwrap();
    assert_eq!(result.elapsed_work_days, Some(1));
    assert_eq!(result.paused_days, 3);
    assert_eq!(result.original_due_date, Some(fixture::date(2026, 10, 6)));
    assert_eq!(result.display_due_date, Some(fixture::date(2026, 10, 6)));
    assert_eq!(
        result.reminder,
        Some(tool_runtime::NextStepReminder::FollowUpWhileWaiting {
            waiting_days: 3,
            days_until_due: Some(1),
        })
    );

    let extended = BehaviorPolicy {
        timer: TimerPolicy::PauseStagesMarkedPaused,
        due_date: DateDuePolicy::ExtendByPausedDays,
        reminder: ReminderPolicy::WaitingBeforeDue { days_before_due: 1 },
    };
    let changed =
        evaluate_record(&snapshot, "record-1", &extended, fixture::date(2026, 10, 5)).unwrap();
    assert_eq!(changed.display_due_date, Some(fixture::date(2026, 10, 9)));
    assert_eq!(
        snapshot.records[0].typed_values.get("promised_on"),
        Some(&FieldValue::Date(fixture::date(2026, 10, 6)))
    );

    let mut repeated_waits = snapshot.clone();
    repeated_waits = apply_command(
        &repeated_waits,
        &ToolCommand::TransitionStage {
            operation_id: "resume-1".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            to_stage_id: "in_progress".to_owned(),
            occurred_at: fixture::at(2026, 10, 3, 9),
        },
        fixture::at(2026, 10, 3, 9),
    )
    .unwrap();
    repeated_waits = apply_command(
        &repeated_waits,
        &ToolCommand::TransitionStage {
            operation_id: "wait-2".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 3,
            to_stage_id: "waiting_materials".to_owned(),
            occurred_at: fixture::at(2026, 10, 4, 9),
        },
        fixture::at(2026, 10, 4, 9),
    )
    .unwrap();
    let repeated = evaluate_record(
        &repeated_waits,
        "record-1",
        &extended,
        fixture::date(2026, 10, 5),
    )
    .unwrap();
    assert_eq!(repeated.elapsed_work_days, Some(2));
    assert_eq!(repeated.paused_days, 2);
    assert_eq!(repeated.display_due_date, Some(fixture::date(2026, 10, 8)));
}

#[test]
fn pause_accounting_is_independent_of_the_work_start_date() {
    let mut late_work_start = fixture::studio_order_project("late-work-start-project");
    late_work_start.records[0].typed_values.insert(
        "started_on".to_owned(),
        FieldValue::Date(fixture::date(2026, 10, 4)),
    );
    if let tool_project::RecordEventKind::Created { initial_values, .. } =
        &mut late_work_start.event_history[0].kind
    {
        initial_values.insert(
            "started_on".to_owned(),
            FieldValue::Date(fixture::date(2026, 10, 4)),
        );
    } else {
        panic!("first record event must be creation");
    }
    late_work_start.validate().unwrap();

    let policy = BehaviorPolicy {
        timer: TimerPolicy::PauseStagesMarkedPaused,
        due_date: DateDuePolicy::ExtendByPausedDays,
        reminder: ReminderPolicy::WaitingAfterDays { days_waiting: 3 },
    };
    let late_start_result = evaluate_record(
        &late_work_start,
        "record-1",
        &policy,
        fixture::date(2026, 10, 5),
    )
    .unwrap();
    assert_eq!(late_start_result.elapsed_work_days, Some(0));
    assert_eq!(late_start_result.paused_days, 3);
    assert_eq!(
        late_start_result.display_due_date,
        Some(fixture::date(2026, 10, 9))
    );
    assert_eq!(
        late_start_result.reminder,
        Some(tool_runtime::NextStepReminder::FollowUpWhileWaiting {
            waiting_days: 3,
            days_until_due: Some(4),
        })
    );

    let source = fixture::studio_order_project("initial-paused-source");
    let mut initial = fixture::empty_project(studio_order_template(), "initial-paused-project");
    let mut values = source.records[0].typed_values.clone();
    values.insert(
        "started_on".to_owned(),
        FieldValue::Date(fixture::date(2026, 10, 4)),
    );
    initial = apply_command(
        &initial,
        &ToolCommand::CreateRecord {
            operation_id: "create-paused-record".to_owned(),
            expected_generation: 0,
            record_id: "paused-record".to_owned(),
            initial_stage_id: "waiting_materials".to_owned(),
            values,
            occurred_at: fixture::at(2026, 10, 1, 9),
        },
        fixture::at(2026, 10, 1, 9),
    )
    .unwrap();
    let initially_paused_result = evaluate_record(
        &initial,
        "paused-record",
        &policy,
        fixture::date(2026, 10, 5),
    )
    .unwrap();
    assert_eq!(initially_paused_result.paused_days, 4);
    assert_eq!(
        initially_paused_result.display_due_date,
        Some(fixture::date(2026, 10, 10))
    );
    assert_eq!(
        initially_paused_result.reminder,
        Some(tool_runtime::NextStepReminder::FollowUpWhileWaiting {
            waiting_days: 4,
            days_until_due: Some(5),
        })
    );
}

#[test]
fn project_collections_scenario_expectations_and_extensions_are_bounded() {
    let mut revisions = fixture::empty_project(studio_order_template(), "revision-limit-project");
    let mut revision = revisions.active_spec().unwrap().clone();
    for next_revision in 2..=(tool_project::MAX_SPEC_REVISIONS as u32 + 1) {
        revision.revision = next_revision;
        revisions.spec_revisions.push(revision.clone());
    }
    revisions.active_spec = tool_project::SpecReference::from(&revision);
    assert!(matches!(
        revisions.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let mut decisions = fixture::empty_project(studio_order_template(), "decision-limit-project");
    decisions.decisions.resize(
        tool_project::MAX_DECISIONS + 1,
        tool_project::DecisionRecord {
            decision_id: "decision-over-limit".to_owned(),
            intent_revision: 1,
            choice: tool_project::DecisionChoice::Defer,
            expected_outcomes: Vec::new(),
            scope: None,
            rationale: "Keep this decision bounded".to_owned(),
            unresolved_questions: Vec::new(),
            scenario_ids: Vec::new(),
            evidence_ids: Vec::new(),
            supersedes: Vec::new(),
            status: tool_project::DecisionStatus::Pending,
            created_at: fixture::at(2026, 10, 1, 8),
            extensions: BTreeMap::new(),
        },
    );
    assert!(matches!(
        decisions.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let mut project_extensions =
        fixture::empty_project(studio_order_template(), "extension-limit-project");
    for index in 0..=tool_project::MAX_EXTENSION_FIELDS {
        project_extensions.extensions.insert(
            format!("future_{index}"),
            serde_json::Value::String("kept".to_owned()),
        );
    }
    assert!(matches!(
        project_extensions.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let snapshot = fixture::studio_order_project("expectation-limit-project");
    let mut scenario = tool_project::Scenario {
        scenario_id: "expectation-limit-scenario".to_owned(),
        name: "Bounded expectations".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings.clone(),
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 5),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: vec![tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        }],
        expected: Vec::new(),
        extensions: BTreeMap::new(),
    };
    let observation = run_scenario(&scenario).unwrap().observations[0].clone();
    scenario.expected = vec![observation; tool_project::MAX_SCENARIO_EXPECTATIONS + 1];
    assert!(matches!(
        scenario.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let mut nested_extension =
        fixture::empty_project(studio_order_template(), "nested-extension-project");
    let mut value = serde_json::Value::Null;
    for _ in 0..=tool_project::MAX_EXTENSION_JSON_DEPTH {
        value = serde_json::Value::Array(vec![value]);
    }
    nested_extension
        .extensions
        .insert("future_nested".to_owned(), value);
    assert!(matches!(
        nested_extension.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let mut escaped_extension =
        fixture::empty_project(studio_order_template(), "escaped-extension-project");
    for index in 0..20 {
        escaped_extension.extensions.insert(
            format!("control_{index}"),
            serde_json::Value::String("\0".repeat(50_000)),
        );
    }
    assert!(matches!(
        escaped_extension.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));
}

#[test]
fn scenario_validation_checks_its_embedded_records_and_event_history() {
    let snapshot = fixture::studio_order_project("scenario-base-validation-project");
    let mut invalid_values = scenario_for_project(&snapshot, "invalid-values-scenario");
    invalid_values.records[0].typed_values.insert(
        "promised_on".to_owned(),
        FieldValue::Text("not a date".to_owned()),
    );
    assert!(matches!(
        invalid_values.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidFieldValue
    ));

    let mut invalid_history = scenario_for_project(&snapshot, "invalid-history-scenario");
    invalid_history.event_history[1].record_revision += 1;
    assert!(matches!(
        invalid_history.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidEvent
    ));
}

#[test]
fn project_snapshots_reject_scenarios_with_changed_project_creation_time() {
    let mut project = fixture::studio_order_project("scenario-project-created-time");
    let mut scenario = scenario_for_project(&project, "scenario-with-false-project-age");
    scenario.project_created_at = fixture::at(2026, 10, 1, 6);
    scenario.steps.extend([
        tool_project::ScenarioStep::CreateRecord {
            operation_id: "create-before-project-existed".to_owned(),
            record_id: "pre-project-record".to_owned(),
            initial_stage_id: "queued".to_owned(),
            values: project.records[0].typed_values.clone(),
            occurred_at: fixture::at(2026, 10, 1, 7),
        },
        tool_project::ScenarioStep::Observe {
            record_id: "pre-project-record".to_owned(),
        },
    ]);

    let evidence = run_scenario(&scenario).unwrap();
    assert_eq!(evidence.observations[0].record_id, "pre-project-record");
    project.scenarios.push(scenario);
    project.evidence.push(evidence);

    let error = project.validate().unwrap_err();
    assert_eq!(error.code, tool_project::ValidationCode::InvalidTimestamp);
    assert_eq!(error.path, "scenarios.project_created_at");
}

#[test]
fn scenario_steps_validate_field_values_before_execution() {
    let empty = fixture::empty_project(studio_order_template(), "scenario-step-project");
    let valid_values = fixture::studio_order_project("scenario-step-values").records[0]
        .typed_values
        .clone();

    let mut missing_required = scenario_for_project(&empty, "missing-required-step-scenario");
    missing_required
        .steps
        .push(tool_project::ScenarioStep::CreateRecord {
            operation_id: "create-missing-fields".to_owned(),
            record_id: "new-record".to_owned(),
            initial_stage_id: "queued".to_owned(),
            values: BTreeMap::new(),
            occurred_at: fixture::at(2026, 10, 2, 9),
        });
    assert!(matches!(
        missing_required.validate().unwrap_err().code,
        tool_project::ValidationCode::MissingRequiredField
    ));

    let mut unknown_field = scenario_for_project(&empty, "unknown-field-step-scenario");
    let mut values = valid_values.clone();
    values.insert(
        "future_field".to_owned(),
        FieldValue::Text("not declared by the spec".to_owned()),
    );
    unknown_field
        .steps
        .push(tool_project::ScenarioStep::CreateRecord {
            operation_id: "create-unknown-field".to_owned(),
            record_id: "new-record".to_owned(),
            initial_stage_id: "queued".to_owned(),
            values,
            occurred_at: fixture::at(2026, 10, 2, 9),
        });
    assert!(matches!(
        unknown_field.validate().unwrap_err().code,
        tool_project::ValidationCode::UnknownField
    ));

    let mut wrong_type = scenario_for_project(&empty, "wrong-type-step-scenario");
    let mut values = valid_values.clone();
    values.insert(
        "promised_on".to_owned(),
        FieldValue::Text("not a date".to_owned()),
    );
    wrong_type
        .steps
        .push(tool_project::ScenarioStep::CreateRecord {
            operation_id: "create-wrong-type".to_owned(),
            record_id: "new-record".to_owned(),
            initial_stage_id: "queued".to_owned(),
            values,
            occurred_at: fixture::at(2026, 10, 2, 9),
        });
    assert!(matches!(
        wrong_type.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidFieldValue
    ));

    let base = fixture::studio_order_project("scenario-edit-values");
    let mut oversized_text = scenario_for_project(&base, "oversized-edit-step-scenario");
    oversized_text
        .steps
        .push(tool_project::ScenarioStep::EditRecord {
            operation_id: "edit-oversized-notes".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            changes: BTreeMap::from([(
                "notes".to_owned(),
                Some(FieldValue::Text("x".repeat(16_385))),
            )]),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });
    assert!(matches!(
        oversized_text.validate().unwrap_err().code,
        tool_project::ValidationCode::LimitExceeded
    ));

    let mut unknown_edit = scenario_for_project(&base, "unknown-edit-field-scenario");
    unknown_edit
        .steps
        .push(tool_project::ScenarioStep::EditRecord {
            operation_id: "remove-unknown-field".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: 2,
            changes: BTreeMap::from([("missing_field".to_owned(), None)]),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });
    assert!(matches!(
        unknown_edit.validate().unwrap_err().code,
        tool_project::ValidationCode::UnknownField
    ));
}

#[test]
fn scenario_steps_validate_references_transitions_revisions_and_operation_ids() {
    use tool_project::{ScenarioStep, ValidationCode};

    let snapshot = fixture::studio_order_project("scenario-reference-project");
    let mut missing_transition = scenario_for_project(&snapshot, "missing-transition-record");
    missing_transition
        .steps
        .push(ScenarioStep::TransitionStage {
            operation_id: "missing-transition-op".to_owned(),
            record_id: "missing-record".to_owned(),
            expected_record_revision: 1,
            to_stage_id: "making".to_owned(),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });

    let current_record = &snapshot.records[0];
    let spec = snapshot
        .spec_revisions
        .iter()
        .find(|spec| {
            spec.spec_id == current_record.spec.spec_id
                && spec.revision == current_record.spec.revision
        })
        .unwrap();
    let current_stage = spec.stage(&current_record.current_stage_id).unwrap();
    let disallowed_stage = spec
        .stages
        .iter()
        .find(|stage| {
            stage.id != current_stage.id
                && !current_stage
                    .allowed_next_stage_ids
                    .iter()
                    .any(|allowed| allowed == &stage.id)
        })
        .unwrap();

    let mut unknown_transition_stage = scenario_for_project(&snapshot, "unknown-transition-stage");
    unknown_transition_stage
        .steps
        .push(ScenarioStep::TransitionStage {
            operation_id: "unknown-transition-op".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: current_record.record_revision,
            to_stage_id: "missing-stage".to_owned(),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });

    let mut disallowed_transition = scenario_for_project(&snapshot, "disallowed-transition");
    disallowed_transition
        .steps
        .push(ScenarioStep::TransitionStage {
            operation_id: "disallowed-transition-op".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: current_record.record_revision,
            to_stage_id: disallowed_stage.id.clone(),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });

    let mut stale_revision = scenario_for_project(&snapshot, "stale-step-revision");
    stale_revision.steps.push(ScenarioStep::TransitionStage {
        operation_id: "stale-revision-op".to_owned(),
        record_id: "record-1".to_owned(),
        expected_record_revision: current_record.record_revision - 1,
        to_stage_id: current_stage.allowed_next_stage_ids[0].clone(),
        occurred_at: fixture::at(2026, 10, 5, 9),
    });

    let mut missing_observation = scenario_for_project(&snapshot, "missing-observation-record");
    missing_observation.steps.push(ScenarioStep::Observe {
        record_id: "missing-record".to_owned(),
    });

    let mut duplicate_operation = scenario_for_project(&snapshot, "duplicate-step-operation");
    duplicate_operation.steps.extend([
        ScenarioStep::EditRecord {
            operation_id: "duplicate-step-op".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: current_record.record_revision,
            changes: BTreeMap::from([(
                "notes".to_owned(),
                Some(FieldValue::Text("first edit".to_owned())),
            )]),
            occurred_at: fixture::at(2026, 10, 5, 9),
        },
        ScenarioStep::EditRecord {
            operation_id: "duplicate-step-op".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: current_record.record_revision + 1,
            changes: BTreeMap::from([(
                "notes".to_owned(),
                Some(FieldValue::Text("second edit".to_owned())),
            )]),
            occurred_at: fixture::at(2026, 10, 5, 9),
        },
    ]);

    let mut reused_history_operation = scenario_for_project(&snapshot, "reused-history-operation");
    reused_history_operation
        .steps
        .push(ScenarioStep::EditRecord {
            operation_id: "wait-1".to_owned(),
            record_id: "record-1".to_owned(),
            expected_record_revision: current_record.record_revision,
            changes: BTreeMap::from([(
                "notes".to_owned(),
                Some(FieldValue::Text("edit after existing event".to_owned())),
            )]),
            occurred_at: fixture::at(2026, 10, 5, 9),
        });

    let mut reversed_step_time = scenario_for_project(&snapshot, "reversed-step-time");
    reversed_step_time.steps.push(ScenarioStep::EditRecord {
        operation_id: "reversed-step-time-op".to_owned(),
        record_id: "record-1".to_owned(),
        expected_record_revision: current_record.record_revision,
        changes: BTreeMap::from([(
            "notes".to_owned(),
            Some(FieldValue::Text("out-of-order edit".to_owned())),
        )]),
        occurred_at: fixture::at(2026, 10, 2, 8),
    });

    let mut future_step_time = scenario_for_project(&snapshot, "future-step-time");
    future_step_time.steps.push(ScenarioStep::CreateRecord {
        operation_id: "future-step-time-op".to_owned(),
        record_id: "future-record".to_owned(),
        initial_stage_id: "queued".to_owned(),
        values: current_record.typed_values.clone(),
        occurred_at: fixture::at(2026, 10, 6, 9),
    });

    let mut past_observation_date = scenario_for_project(&snapshot, "past-observation-date");
    past_observation_date.as_of_date = fixture::date(2026, 10, 1);

    let mut future_advance_date = scenario_for_project(&snapshot, "future-advance-date");
    future_advance_date.steps.push(ScenarioStep::AdvanceDate {
        as_of_date: fixture::date(2026, 10, 6),
    });

    let mut missing_expected_record = scenario_for_project(&snapshot, "missing-expected-record");
    missing_expected_record
        .expected
        .push(tool_project::EvidenceObservation {
            record_id: "missing-record".to_owned(),
            current_stage_id: current_record.current_stage_id.clone(),
            elapsed_work_days: None,
            paused_days: 0,
            original_due_date: None,
            display_due_date: None,
            reminder: None,
        });

    let mut unknown_expected_stage = scenario_for_project(&snapshot, "unknown-expected-stage");
    unknown_expected_stage
        .expected
        .push(tool_project::EvidenceObservation {
            record_id: "record-1".to_owned(),
            current_stage_id: "missing-stage".to_owned(),
            elapsed_work_days: None,
            paused_days: 0,
            original_due_date: None,
            display_due_date: None,
            reminder: None,
        });

    let cases = [
        (
            "transition references an unknown record",
            missing_transition,
            ValidationCode::DanglingReference,
        ),
        (
            "transition references an unknown stage",
            unknown_transition_stage,
            ValidationCode::DanglingReference,
        ),
        (
            "transition violates the spec",
            disallowed_transition,
            ValidationCode::InvalidTransition,
        ),
        (
            "step uses a stale record revision",
            stale_revision,
            ValidationCode::InvalidEvent,
        ),
        (
            "observation references an unknown record",
            missing_observation,
            ValidationCode::DanglingReference,
        ),
        (
            "scenario reuses a step operation ID",
            duplicate_operation,
            ValidationCode::DuplicateId,
        ),
        (
            "scenario reuses an event ID from its base history",
            reused_history_operation,
            ValidationCode::DuplicateId,
        ),
        (
            "scenario event timestamps cannot move backwards",
            reversed_step_time,
            ValidationCode::InvalidTimestamp,
        ),
        (
            "scenario events cannot occur after the fixed clock",
            future_step_time,
            ValidationCode::InvalidTimestamp,
        ),
        (
            "scenario cannot observe history from a later date",
            past_observation_date,
            ValidationCode::InvalidTimestamp,
        ),
        (
            "scenario cannot advance past its fixed clock",
            future_advance_date,
            ValidationCode::InvalidTimestamp,
        ),
        (
            "expectation references an unknown record",
            missing_expected_record,
            ValidationCode::DanglingReference,
        ),
        (
            "expectation references an unknown stage",
            unknown_expected_stage,
            ValidationCode::DanglingReference,
        ),
    ];
    for (case, scenario, expected_code) in cases {
        assert_eq!(
            scenario.validate().unwrap_err().code,
            expected_code,
            "{case}"
        );
    }
}

#[test]
fn scenario_decision_and_imported_evidence_validate_observation_contents() {
    let snapshot = fixture::studio_order_project("observation-project");
    let mut scenario = scenario_for_project(&snapshot, "observation-scenario");
    scenario.expected.push(tool_project::EvidenceObservation {
        record_id: "r".repeat(tool_project::MAX_ID_LENGTH + 1),
        current_stage_id: "waiting".to_owned(),
        elapsed_work_days: Some(1),
        paused_days: 0,
        original_due_date: None,
        display_due_date: None,
        reminder: None,
    });
    assert!(matches!(
        scenario.expected[0].validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidObservation
    ));
    assert!(matches!(
        scenario.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidObservation
    ));

    let mut invalid_decision = snapshot.clone();
    invalid_decision
        .decisions
        .push(tool_project::DecisionRecord {
            decision_id: "negative-observation-decision".to_owned(),
            intent_revision: 1,
            choice: tool_project::DecisionChoice::Defer,
            expected_outcomes: vec![tool_project::EvidenceObservation {
                record_id: "record-1".to_owned(),
                current_stage_id: "waiting_for_parts".to_owned(),
                elapsed_work_days: None,
                paused_days: -1,
                original_due_date: None,
                display_due_date: None,
                reminder: None,
            }],
            scope: None,
            rationale: "preserve invalid observations for validation test".to_owned(),
            unresolved_questions: Vec::new(),
            scenario_ids: Vec::new(),
            evidence_ids: Vec::new(),
            supersedes: Vec::new(),
            status: tool_project::DecisionStatus::Pending,
            created_at: fixture::at(2026, 10, 2, 9),
            extensions: BTreeMap::new(),
        });
    assert!(matches!(
        invalid_decision.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidObservation
    ));

    let mut valid_scenario = scenario_for_project(&snapshot, "imported-observation-scenario");
    valid_scenario
        .steps
        .push(tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        });
    valid_scenario.as_of_date = fixture::date(2026, 10, 8);
    valid_scenario.fixed_now = fixture::at(2026, 10, 8, 9);
    valid_scenario.candidate_policy = BehaviorPolicy {
        timer: TimerPolicy::PauseStagesMarkedPaused,
        due_date: DateDuePolicy::KeepOriginal,
        reminder: ReminderPolicy::WaitingAfterDays { days_waiting: 1 },
    };
    let mut imported = run_scenario(&valid_scenario).unwrap();
    imported.source = tool_project::EvidenceSource::ImportedUntrusted;
    imported.run_id = "imported-overdue-observation".to_owned();
    assert!(matches!(
        imported.observations[0].reminder,
        Some(tool_project::ReminderObservation::FollowUpWhileWaiting {
            days_until_due: Some(-2),
            ..
        })
    ));
    let mut overdue_snapshot = snapshot.clone();
    overdue_snapshot.scenarios.push(valid_scenario.clone());
    overdue_snapshot.evidence.push(imported.clone());
    assert!(overdue_snapshot.validate().is_ok());

    let mut negative_elapsed = imported.clone();
    negative_elapsed.run_id = "imported-negative-elapsed".to_owned();
    negative_elapsed.observations[0].elapsed_work_days = Some(-1);
    let mut elapsed_snapshot = snapshot.clone();
    elapsed_snapshot.scenarios.push(valid_scenario.clone());
    elapsed_snapshot.evidence.push(negative_elapsed);
    assert!(matches!(
        elapsed_snapshot.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidObservation
    ));

    let mut negative_waiting = imported;
    negative_waiting.run_id = "imported-negative-waiting".to_owned();
    negative_waiting.observations[0].reminder =
        Some(tool_project::ReminderObservation::FollowUpWhileWaiting {
            waiting_days: -1,
            days_until_due: Some(-2),
        });
    let mut waiting_snapshot = snapshot;
    waiting_snapshot.scenarios.push(valid_scenario);
    waiting_snapshot.evidence.push(negative_waiting);
    assert!(matches!(
        waiting_snapshot.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidObservation
    ));
}

#[test]
fn evidence_and_decision_observations_resolve_records_and_stages() {
    let base = fixture::studio_order_project("observation-reference-project");
    let scenario = scenario_for_project(&base, "observation-reference-scenario");
    let evidence = run_scenario(&scenario).unwrap();
    let observation = evidence.observations[0].clone();

    let mut valid = base.clone();
    valid.scenarios.push(scenario.clone());
    valid.evidence.push(evidence.clone());
    assert!(valid.validate().is_ok());

    #[derive(Clone, Copy)]
    enum Collection {
        EvidenceObservations,
        EvidenceExpected,
        DecisionExpected,
    }

    let collections = [
        (Collection::EvidenceObservations, "evidence observations"),
        (Collection::EvidenceExpected, "evidence expectations"),
        (Collection::DecisionExpected, "decision expectations"),
    ];
    for (collection, collection_name) in collections {
        for (reference_name, missing_record) in [("record", true), ("stage", false)] {
            let mut invalid_observation = observation.clone();
            if missing_record {
                invalid_observation.record_id = "missing-record".to_owned();
            } else {
                invalid_observation.current_stage_id = "missing-stage".to_owned();
            }

            let mut candidate = base.clone();
            candidate.scenarios.push(scenario.clone());
            match collection {
                Collection::EvidenceObservations => {
                    let mut invalid_evidence = evidence.clone();
                    invalid_evidence.observations = vec![invalid_observation];
                    candidate.evidence.push(invalid_evidence);
                }
                Collection::EvidenceExpected => {
                    let mut invalid_evidence = evidence.clone();
                    invalid_evidence.expected = vec![invalid_observation];
                    candidate.evidence.push(invalid_evidence);
                }
                Collection::DecisionExpected => {
                    candidate.decisions.push(tool_project::DecisionRecord {
                        decision_id: "invalid-reference-decision".to_owned(),
                        intent_revision: 1,
                        choice: tool_project::DecisionChoice::Defer,
                        expected_outcomes: vec![invalid_observation],
                        scope: None,
                        rationale: "Check linked scenario observation references".to_owned(),
                        unresolved_questions: Vec::new(),
                        scenario_ids: vec![scenario.scenario_id.clone()],
                        evidence_ids: Vec::new(),
                        supersedes: Vec::new(),
                        status: tool_project::DecisionStatus::Pending,
                        created_at: fixture::at(2026, 10, 2, 9),
                        extensions: BTreeMap::new(),
                    });
                }
            }
            assert_eq!(
                candidate.validate().unwrap_err().code,
                tool_project::ValidationCode::DanglingReference,
                "{collection_name} must resolve an unknown {reference_name}"
            );
        }
    }
}

#[test]
fn missing_start_date_stays_unknown_instead_of_becoming_zero() {
    let mut snapshot = fixture::studio_order_project("missing-start-project");
    snapshot.records[0].typed_values.remove("started_on");
    if let tool_project::RecordEventKind::Created { initial_values, .. } =
        &mut snapshot.event_history[0].kind
    {
        initial_values.remove("started_on");
    } else {
        panic!("first record event must be creation");
    }
    snapshot.validate().unwrap();
    let result = evaluate_record(
        &snapshot,
        "record-1",
        &fixture::standard_behavior(),
        fixture::date(2026, 10, 5),
    )
    .unwrap();
    assert_eq!(result.elapsed_work_days, None);
}

#[test]
fn scenario_runner_is_deterministic_and_does_not_mutate_its_input_copy() {
    let snapshot = fixture::studio_order_project("scenario-project");
    let before = serde_json::to_vec(&snapshot).unwrap();
    let scenario = tool_project::Scenario {
        scenario_id: "scenario-a".to_owned(),
        name: "固定日期演练".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings.clone(),
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 4),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: vec![
            tool_project::ScenarioStep::AdvanceDate {
                as_of_date: fixture::date(2026, 10, 5),
            },
            tool_project::ScenarioStep::Observe {
                record_id: "record-1".to_owned(),
            },
        ],
        expected: vec![],
        extensions: BTreeMap::new(),
    };

    let first = run_scenario(&scenario).unwrap();
    let second = run_scenario(&scenario).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.observations.len(), 1);
    assert_eq!(first.observations[0].elapsed_work_days, Some(1));
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);

    let mut mismatched_calendar = scenario;
    mismatched_calendar
        .date_settings
        .calendar_utc_offset_seconds = -12 * 60 * 60;
    mismatched_calendar.fixed_now = chrono::FixedOffset::east_opt(14 * 60 * 60)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 5, 9, 0, 0)
        .single()
        .unwrap();
    mismatched_calendar.steps = vec![tool_project::ScenarioStep::AdvanceDate {
        as_of_date: fixture::date(2026, 10, 5),
    }];
    assert!(matches!(
        run_scenario(&mismatched_calendar),
        Err(RuntimeError::ProjectValidation(error))
            if error.code == tool_project::ValidationCode::InvalidTimestamp
    ));
}

#[test]
fn explicit_calendar_date_values_round_trip_as_dates() {
    let value = FieldValue::Date(NaiveDate::from_ymd_opt(2026, 10, 6).unwrap());
    let json = serde_json::to_string(&value).unwrap();
    let decoded: FieldValue = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, value);
}

#[test]
fn scenario_replays_records_across_spec_revisions_and_distinguishes_candidate_runs() {
    let snapshot = fixture::studio_order_project("scenario-revision-project");
    let mut scenario = tool_project::Scenario {
        scenario_id: "scenario-revisions".to_owned(),
        name: "跨版本订单演练".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings,
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 4),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: vec![tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        }],
        expected: vec![],
        extensions: BTreeMap::new(),
    };
    let first_spec = scenario.spec_revisions.last().unwrap().clone();
    let mut second_spec = first_spec.clone();
    second_spec.revision = 2;
    second_spec.display_name = "工作室订单（字段调整后）".to_owned();
    scenario.spec_revisions.push(second_spec.clone());
    scenario.active_spec = tool_project::SpecReference::from(&second_spec);

    let scenario_value = serde_json::to_value(&scenario).unwrap();
    let scenario: tool_project::Scenario = serde_json::from_value(scenario_value).unwrap();

    let original_run = run_scenario(&scenario).unwrap();
    let mut other_candidate = scenario.clone();
    other_candidate.candidate_policy.due_date = DateDuePolicy::ExtendByPausedDays;
    let alternative_run = run_scenario(&other_candidate).unwrap();
    assert_ne!(original_run.run_id, alternative_run.run_id);
    assert_eq!(original_run.observations[0].elapsed_work_days, Some(1));
    assert_eq!(alternative_run.observations[0].elapsed_work_days, Some(1));

    let mut evidence_snapshot = snapshot;
    evidence_snapshot.scenarios.push(scenario);
    evidence_snapshot.evidence = vec![original_run, alternative_run];
    assert!(evidence_snapshot.validate().is_ok());
}

#[test]
fn frozen_binding_scope_must_resolve_every_record_id() {
    let mut snapshot = fixture::studio_order_project("scope-project");
    snapshot.rule_bindings.push(tool_project::RuleBinding {
        binding_id: "binding-scope".to_owned(),
        rule_key: tool_project::RuleKey::Timer,
        record_id: None,
        behavior_revision_id: "behavior-1".to_owned(),
        previous_binding_id: None,
        scope: tool_project::ScopeSelection {
            kind: tool_project::ScopeKind::SingleRecord,
            confirmed_generation: snapshot.generation,
            frozen_record_ids: vec!["missing-record".to_owned()],
            applies_to_future_records: false,
            effective_sequence: 1,
        },
        effective_sequence: 1,
    });
    assert!(snapshot.validate().is_err());
}

#[test]
fn rule_binding_history_must_point_to_an_earlier_same_dimension_binding() {
    use tool_project::{RuleBinding, RuleKey, ScopeKind, ScopeSelection, ValidationCode};

    fn binding(id: &str, rule_key: RuleKey, previous: Option<&str>) -> RuleBinding {
        RuleBinding {
            binding_id: id.to_owned(),
            rule_key,
            record_id: None,
            behavior_revision_id: "behavior-1".to_owned(),
            previous_binding_id: previous.map(str::to_owned),
            scope: ScopeSelection {
                kind: ScopeKind::FutureRecords,
                confirmed_generation: 0,
                frozen_record_ids: Vec::new(),
                applies_to_future_records: true,
                effective_sequence: 1,
            },
            effective_sequence: 1,
        }
    }

    let mut self_link = fixture::empty_project(studio_order_template(), "binding-self-link");
    self_link.rule_bindings.push(binding(
        "binding-self",
        RuleKey::Timer,
        Some("binding-self"),
    ));

    let mut forward_link = fixture::empty_project(studio_order_template(), "binding-forward-link");
    forward_link.rule_bindings.push(binding(
        "binding-first",
        RuleKey::Timer,
        Some("binding-later"),
    ));
    forward_link
        .rule_bindings
        .push(binding("binding-later", RuleKey::Timer, None));

    let mut cross_dimension_link =
        fixture::empty_project(studio_order_template(), "binding-cross-dimension");
    cross_dimension_link
        .rule_bindings
        .push(binding("timer-binding", RuleKey::Timer, None));
    cross_dimension_link.rule_bindings.push(binding(
        "reminder-binding",
        RuleKey::Reminder,
        Some("timer-binding"),
    ));

    for (case, project) in [
        ("binding cannot point to itself", self_link),
        ("binding history cannot point forward", forward_link),
        (
            "binding history cannot cross rule dimensions",
            cross_dimension_link,
        ),
    ] {
        assert_eq!(
            project.validate().unwrap_err().code,
            ValidationCode::InvalidScope,
            "{case}"
        );
    }
}

#[test]
fn nested_tool_spec_extensions_survive_a_roundtrip() {
    let field_extension = serde_json::json!({"unit": "millimeter"});
    let stage_extension = serde_json::json!({"queue": "materials"});
    let mut encoded = serde_json::to_value(studio_order_template()).unwrap();
    encoded["fields"][0]["future_field_metadata"] = field_extension.clone();
    encoded["stages"][0]["future_stage_metadata"] = stage_extension.clone();

    let decoded: tool_project::ToolSpec = serde_json::from_value(encoded).unwrap();
    let roundtrip = serde_json::to_value(decoded).unwrap();

    assert_eq!(
        roundtrip["fields"][0]["future_field_metadata"],
        field_extension
    );
    assert_eq!(
        roundtrip["stages"][0]["future_stage_metadata"],
        stage_extension
    );
}

#[test]
fn flattened_extensions_cannot_shadow_serialized_fields() {
    let snapshot = fixture::studio_order_project("extension-collision-project");
    let mut shadowed_spec = studio_order_template();
    shadowed_spec
        .extensions
        .insert("spec_id".to_owned(), serde_json::json!("shadow"));
    assert!(shadowed_spec.validate().is_err());

    let mut shadowed_field = studio_order_template();
    shadowed_field.fields[0]
        .extensions
        .insert("id".to_owned(), serde_json::json!("shadow"));
    assert!(shadowed_field.validate().is_err());

    let mut shadowed_stage = studio_order_template();
    shadowed_stage.stages[0]
        .extensions
        .insert("clock".to_owned(), serde_json::json!("shadow"));
    assert!(shadowed_stage.validate().is_err());

    let mut with_record = snapshot.clone();
    with_record.records[0]
        .extensions
        .insert("record_id".to_owned(), serde_json::json!("shadow"));
    assert!(with_record.validate().is_err());

    let mut with_event = snapshot.clone();
    with_event.event_history[0]
        .extensions
        .insert("event_id".to_owned(), serde_json::json!("shadow"));
    assert!(with_event.validate().is_err());

    let mut with_project = snapshot.clone();
    with_project
        .extensions
        .insert("format_version".to_owned(), serde_json::json!(99));
    assert!(with_project.validate().is_err());

    let scenario = tool_project::Scenario {
        scenario_id: "extension-scenario".to_owned(),
        name: "扩展字段检查".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings,
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 5),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: vec![tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        }],
        expected: vec![],
        extensions: BTreeMap::new(),
    };
    let evidence = run_scenario(&scenario).unwrap();

    let mut with_scenario = snapshot.clone();
    with_scenario.scenarios.push(scenario.clone());
    with_scenario.scenarios[0]
        .extensions
        .insert("active_spec".to_owned(), serde_json::json!("shadow"));
    assert!(with_scenario.validate().is_err());

    let mut with_decision = snapshot.clone();
    with_decision.decisions.push(tool_project::DecisionRecord {
        decision_id: "extension-decision".to_owned(),
        intent_revision: 1,
        choice: tool_project::DecisionChoice::Defer,
        expected_outcomes: vec![],
        scope: None,
        rationale: "保留待定".to_owned(),
        unresolved_questions: vec![],
        scenario_ids: vec![],
        evidence_ids: vec![],
        supersedes: vec![],
        status: tool_project::DecisionStatus::Pending,
        created_at: snapshot.created_at,
        extensions: BTreeMap::from([("decision_id".to_owned(), serde_json::json!("shadow"))]),
    });
    assert!(with_decision.validate().is_err());

    let mut with_evidence = snapshot.clone();
    with_evidence.scenarios.push(scenario.clone());
    with_evidence.evidence.push(evidence.clone());
    with_evidence.evidence[0]
        .extensions
        .insert("run_id".to_owned(), serde_json::json!("shadow"));
    assert!(with_evidence.validate().is_err());

    let mut with_proposal = snapshot;
    with_proposal
        .proposals
        .push(tool_project::ProposalEnvelope {
            proposal_id: "extension-proposal".to_owned(),
            project_id: with_proposal.project_id.clone(),
            baseline_fingerprint: "a".repeat(64),
            source: tool_project::ProposalSource::LocalBuiltIn,
            candidate_policies: vec![fixture::standard_behavior()],
            rationale: "内置规则候选".to_owned(),
            unknowns: vec![],
            imported_claims: BTreeMap::new(),
            extensions: BTreeMap::from([("proposal_id".to_owned(), serde_json::json!("shadow"))]),
        });
    assert!(with_proposal.validate().is_err());
}

#[test]
fn decision_scope_must_resolve_frozen_record_ids() {
    let mut snapshot = fixture::studio_order_project("decision-scope-project");
    snapshot.decisions.push(tool_project::DecisionRecord {
        decision_id: "decision-scope".to_owned(),
        intent_revision: 1,
        choice: tool_project::DecisionChoice::Adopt,
        expected_outcomes: vec![],
        scope: Some(tool_project::ScopeSelection {
            kind: tool_project::ScopeKind::SingleRecord,
            confirmed_generation: snapshot.generation,
            frozen_record_ids: vec!["missing-record".to_owned()],
            applies_to_future_records: false,
            effective_sequence: 1,
        }),
        rationale: "仅用于所选记录".to_owned(),
        unresolved_questions: vec![],
        scenario_ids: vec![],
        evidence_ids: vec![],
        supersedes: vec![],
        status: tool_project::DecisionStatus::Active,
        created_at: snapshot.updated_at,
        extensions: BTreeMap::new(),
    });
    assert!(snapshot.validate().is_err());
}

#[test]
fn decision_history_references_only_prior_decisions_and_project_timestamps() {
    let snapshot = fixture::studio_order_project("decision-history-project");
    let make_decision =
        |decision_id: &str, supersedes: Vec<&str>, created_at: DateTime<FixedOffset>| {
            tool_project::DecisionRecord {
                decision_id: decision_id.to_owned(),
                intent_revision: 1,
                choice: tool_project::DecisionChoice::Adopt,
                expected_outcomes: vec![],
                scope: None,
                rationale: "Keep decision history ordered".to_owned(),
                unresolved_questions: vec![],
                scenario_ids: vec![],
                evidence_ids: vec![],
                supersedes: supersedes.into_iter().map(str::to_owned).collect(),
                status: tool_project::DecisionStatus::Active,
                created_at,
                extensions: BTreeMap::new(),
            }
        };

    let mut forward_reference = snapshot.clone();
    forward_reference.decisions = vec![
        make_decision(
            "decision-first",
            vec!["decision-later"],
            snapshot.updated_at,
        ),
        make_decision("decision-later", vec![], snapshot.updated_at),
    ];
    assert_eq!(
        forward_reference.validate().unwrap_err().code,
        tool_project::ValidationCode::DanglingReference
    );

    let mut cycle = snapshot.clone();
    cycle.decisions = vec![
        make_decision(
            "decision-first",
            vec!["decision-second"],
            snapshot.updated_at,
        ),
        make_decision(
            "decision-second",
            vec!["decision-first"],
            snapshot.updated_at,
        ),
    ];
    assert_eq!(
        cycle.validate().unwrap_err().code,
        tool_project::ValidationCode::DanglingReference
    );

    let mut earlier_timestamp = snapshot.clone();
    earlier_timestamp.decisions = vec![make_decision(
        "decision-before-project",
        vec![],
        fixture::at(2026, 10, 1, 7),
    )];
    assert_eq!(
        earlier_timestamp.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidTimestamp
    );

    let mut later_timestamp = snapshot.clone();
    later_timestamp.decisions = vec![make_decision(
        "decision-after-project-update",
        vec![],
        fixture::at(2026, 10, 2, 10),
    )];
    assert_eq!(
        later_timestamp.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidTimestamp
    );

    let mut reversed_history_time = snapshot.clone();
    reversed_history_time.decisions = vec![
        make_decision("decision-first", vec![], snapshot.updated_at),
        make_decision(
            "decision-second",
            vec!["decision-first"],
            snapshot.created_at,
        ),
    ];
    assert_eq!(
        reversed_history_time.validate().unwrap_err().code,
        tool_project::ValidationCode::InvalidTimestamp
    );
}

#[test]
fn native_evidence_must_match_the_local_scenario_execution() {
    let snapshot = fixture::studio_order_project("native-evidence-project");
    let scenario = tool_project::Scenario {
        scenario_id: "native-evidence-scenario".to_owned(),
        name: "本地执行证据".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings,
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 5),
        fixed_now: fixture::at(2026, 10, 5, 9),
        steps: vec![tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        }],
        expected: vec![],
        extensions: BTreeMap::new(),
    };
    let evidence = run_scenario(&scenario).unwrap();
    let mut valid = snapshot;
    valid.scenarios.push(scenario);
    valid.evidence.push(evidence);
    assert!(valid.validate().is_ok());
    assert_eq!(
        tool_runtime::validate_native_evidence(&valid).unwrap(),
        tool_runtime::NativeEvidenceCheck::Current
    );

    let mut stale_runtime = valid.clone();
    stale_runtime.evidence[0].runtime_semantics_version =
        tool_project::RUNTIME_SEMANTICS_VERSION - 1;
    assert_eq!(
        tool_runtime::validate_native_evidence(&stale_runtime).unwrap(),
        tool_runtime::NativeEvidenceCheck::StaleRuntime
    );

    let mut forged_id = valid.clone();
    forged_id.evidence[0].run_id = "run-forged".to_owned();
    assert!(tool_runtime::validate_native_evidence(&forged_id).is_err());

    let mut forged_fingerprint = valid.clone();
    forged_fingerprint.evidence[0].candidate_fingerprint = "0".repeat(64);
    assert!(tool_runtime::validate_native_evidence(&forged_fingerprint).is_err());

    let mut forged_status = valid.clone();
    forged_status.evidence[0].status = tool_project::EvidenceStatus::Passed;
    assert!(tool_runtime::validate_native_evidence(&forged_status).is_err());

    let mut forged_observation = valid;
    forged_observation.evidence[0].observations[0].paused_days += 1;
    assert!(tool_runtime::validate_native_evidence(&forged_observation).is_err());
}
