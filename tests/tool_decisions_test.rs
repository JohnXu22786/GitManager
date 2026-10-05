#[path = "fixtures/tool_project_fixture.rs"]
mod fixture;
#[path = "../src/tool_decisions.rs"]
mod tool_decisions;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_runtime.rs"]
mod tool_runtime;
#[path = "../src/tool_store.rs"]
mod tool_store;

use std::collections::BTreeMap;
use tool_decisions::*;
use tool_project::*;
use tool_runtime::apply_command;
use tool_store::{CommitDisposition, ProjectStore, StoreError, StoreFaultPoint, StoreLoad};

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    fixture::at(2026, 10, 5, 12)
}
fn day() -> chrono::NaiveDate {
    fixture::date(2026, 10, 5)
}
fn project() -> ProjectSnapshot {
    fixture::studio_order_project("decisions-project")
}
fn load(store: &ProjectStore) -> ProjectSnapshot {
    match store.load().unwrap() {
        StoreLoad::Writable(s) => s,
        other => panic!("{other:?}"),
    }
}
fn store_for(snapshot: &ProjectSnapshot) -> (tempfile::TempDir, ProjectStore, ProjectSnapshot) {
    let dir = tempfile::tempdir().unwrap();
    let store =
        ProjectStore::create(dir.path().join("project"), snapshot, "initial-project").unwrap();
    let loaded = load(&store);
    (dir, store, loaded)
}
fn commit(store: &ProjectStore, change: &PreparedChange) -> ProjectSnapshot {
    store
        .commit(
            change.expected_generation(),
            change.operation_id(),
            change.snapshot(),
        )
        .unwrap()
        .snapshot
}
fn request(snapshot: &ProjectSnapshot, id: &str, kind: ScopeKind, rule: RuleKey) -> ChangeRequest {
    let scope = freeze_scope(snapshot, kind, Some("record-1")).unwrap();
    let mut policy = policy_for_record(snapshot, "record-1").unwrap();
    match rule {
        RuleKey::Timer => policy.timer = TimerPolicy::CountPausedStages,
        RuleKey::DeliveryTarget => policy.due_date = DateDuePolicy::ExtendByPausedDays,
        RuleKey::Reminder => policy.reminder = ReminderPolicy::WaitingAfterDays { days_waiting: 1 },
    }
    let target = if kind == ScopeKind::FutureRecords {
        RehearsalTarget::FutureRecord("record-1".into())
    } else {
        RehearsalTarget::Record("record-1".into())
    };
    ChangeRequest {
        request_id: format!("request-{id}"),
        operation_id: format!("operation-{id}"),
        decision_id: format!("decision-{id}"),
        revision_id: format!("revision-{id}"),
        rule_keys: vec![rule],
        candidate_policy: policy,
        scope,
        original_request: "等材料时按我的做法处理，保留原话".into(),
        rationale: "我确认了演练中的结果".into(),
        unresolved_questions: vec![],
        supersedes: vec![],
        cases: vec![RehearsalCase {
            scenario: scenario_from_snapshot(
                snapshot,
                &format!("scenario-{id}"),
                "真实演练",
                day(),
                now(),
            )
            .unwrap(),
            target,
        }],
    }
}
fn adopt(
    store: &ProjectStore,
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
) -> ProjectSnapshot {
    let preview = rehearse_change(snapshot, request, day(), now()).unwrap();
    let prepared = prepare_adoption(
        snapshot,
        request,
        &preview,
        &request.request_id,
        day(),
        now(),
    )
    .unwrap();
    commit(store, &prepared)
}
fn add_record(
    snapshot: &ProjectSnapshot,
    id: &str,
    stage: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> ProjectSnapshot {
    apply_command(
        snapshot,
        &ToolCommand::CreateRecord {
            operation_id: format!("create-{id}"),
            expected_generation: snapshot.generation,
            record_id: id.into(),
            initial_stage_id: stage.into(),
            values: snapshot.records[0].typed_values.clone(),
            occurred_at: at,
        },
        at,
    )
    .unwrap()
}

#[test]
fn real_rehearsal_uses_copies_and_changes_with_inputs() {
    let snapshot = project();
    let before = serde_json::to_vec(&snapshot).unwrap();
    let mut req = request(
        &snapshot,
        "a",
        ScopeKind::SingleRecord,
        RuleKey::DeliveryTarget,
    );
    req.candidate_policy.due_date = DateDuePolicy::KeepOriginal;
    let a = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert_eq!(a.evidence[0].observations[0].elapsed_work_days, Some(1));
    assert_eq!(a.evidence[0].observations[0].paused_days, 3);
    assert_eq!(
        a.evidence[0].observations[0].display_due_date,
        Some(fixture::date(2026, 10, 6))
    );
    req.candidate_policy.due_date = DateDuePolicy::ExtendByPausedDays;
    let b = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert_eq!(
        b.evidence[0].observations[0].display_due_date,
        Some(fixture::date(2026, 10, 9))
    );
    assert_eq!(
        b.evidence[0].observations[0].original_due_date,
        Some(fixture::date(2026, 10, 6))
    );
    req.cases[0].scenario.steps.insert(
        0,
        ScenarioStep::EditRecord {
            operation_id: "scenario-edit".into(),
            record_id: "record-1".into(),
            expected_record_revision: 2,
            changes: BTreeMap::from([(
                "promised_on".into(),
                Some(FieldValue::Date(fixture::date(2026, 10, 10))),
            )]),
            occurred_at: now(),
        },
    );
    let changed = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert_eq!(
        changed.evidence[0].observations[0].display_due_date,
        Some(fixture::date(2026, 10, 13))
    );
    assert_eq!(before, serde_json::to_vec(&snapshot).unwrap());
}

#[test]
fn four_scopes_freeze_ids_and_preserve_unselected_defaults() {
    for kind in [
        ScopeKind::SingleRecord,
        ScopeKind::FutureRecords,
        ScopeKind::IncompleteAndFuture,
        ScopeKind::AllExistingAndFuture,
    ] {
        let mut snapshot = add_record(&project(), "historical", "completed", now());
        snapshot = add_record(&snapshot, "cancelled", "cancelled", now());
        snapshot = add_record(&snapshot, "other-open", "in_progress", now());
        let (_dir, store, snapshot) = store_for(&snapshot);
        let req = request(&snapshot, "scoped", kind, RuleKey::DeliveryTarget);
        let expected_count = match kind {
            ScopeKind::SingleRecord => 1,
            ScopeKind::FutureRecords => 0,
            ScopeKind::IncompleteAndFuture => 2,
            ScopeKind::AllExistingAndFuture => 4,
        };
        assert_eq!(req.scope.frozen_record_ids.len(), expected_count);
        let adopted = adopt(&store, &snapshot, &req);
        assert_eq!(adopted.records, snapshot.records);
        assert_eq!(adopted.event_history, snapshot.event_history);
        for record in &snapshot.records {
            let changed = req.scope.frozen_record_ids.contains(&record.record_id);
            assert_eq!(
                policy_for_record(&adopted, &record.record_id)
                    .unwrap()
                    .due_date,
                if changed {
                    DateDuePolicy::ExtendByPausedDays
                } else {
                    DateDuePolicy::KeepOriginal
                }
            );
        }
        let future = add_record(&adopted, "later", "in_progress", now());
        assert_eq!(
            policy_for_record(&future, "later").unwrap().due_date,
            if kind == ScopeKind::SingleRecord {
                DateDuePolicy::KeepOriginal
            } else {
                DateDuePolicy::ExtendByPausedDays
            }
        );
    }
}

#[test]
fn changed_record_set_or_completion_requires_new_preview() {
    let (_dir, store, snapshot) = store_for(&project());
    let req = request(
        &snapshot,
        "race",
        ScopeKind::IncompleteAndFuture,
        RuleKey::Timer,
    );
    let preview = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    let created = add_record(&snapshot, "racing", "completed", now());
    let newer = store
        .commit(snapshot.generation, "create-racing", &created)
        .unwrap()
        .snapshot;
    assert!(prepare_adoption(&newer, &req, &preview, &req.request_id, day(), now()).is_err());
    let prepared =
        prepare_adoption(&snapshot, &req, &preview, &req.request_id, day(), now()).unwrap();
    assert!(matches!(
        store.commit(
            prepared.expected_generation(),
            prepared.operation_id(),
            prepared.snapshot()
        ),
        Err(StoreError::GenerationConflict { .. })
    ));
    let active = add_record(&project(), "finishing", "in_progress", now());
    let (_dir2, store2, active) = store_for(&active);
    let req2 = request(
        &active,
        "completion-race",
        ScopeKind::IncompleteAndFuture,
        RuleKey::Timer,
    );
    let preview2 = rehearse_change(&active, &req2, day(), now()).unwrap();
    let completed = apply_command(
        &active,
        &ToolCommand::TransitionStage {
            operation_id: "complete-finishing".into(),
            expected_generation: active.generation,
            record_id: "finishing".into(),
            expected_record_revision: 1,
            to_stage_id: "completed".into(),
            occurred_at: now(),
        },
        now(),
    )
    .unwrap();
    let completed = store2
        .commit(active.generation, "complete-finishing", &completed)
        .unwrap()
        .snapshot;
    assert!(
        prepare_adoption(&completed, &req2, &preview2, &req2.request_id, day(), now()).is_err()
    );
}

#[test]
fn unresolved_choices_round_trip_without_activating_or_losing_words() {
    for choice in [
        DecisionChoice::Both,
        DecisionChoice::Neither,
        DecisionChoice::Defer,
    ] {
        let (_dir, store, snapshot) = store_for(&project());
        let mut req = request(
            &snapshot,
            "unknown",
            ScopeKind::SingleRecord,
            RuleKey::Timer,
        );
        req.unresolved_questions = vec!["不知道什么条件下选哪种".into()];
        let prepared = prepare_unresolved(&snapshot, &req, choice, now()).unwrap();
        let next = commit(&store, &prepared);
        let reopened = load(&store);
        assert_eq!(next, reopened);
        assert_eq!(reopened.rule_bindings, snapshot.rule_bindings);
        assert_eq!(reopened.behavior_revisions, snapshot.behavior_revisions);
        assert_eq!(reopened.decisions[0].status, DecisionStatus::Pending);
        assert_eq!(reopened.decisions[0].choice, choice);
        assert_eq!(
            decision_behavior_revision_id(&reopened.decisions[0]).unwrap(),
            None
        );
        assert_eq!(
            decision_original_request(&reopened.decisions[0]).unwrap(),
            req.original_request
        );
        assert_eq!(
            reopened.decisions[0].unresolved_questions,
            req.unresolved_questions
        );
    }
}

#[test]
fn confirmed_intent_conflicts_and_explicit_supersession_keeps_other_decisions() {
    let (_dir, store, snapshot) = store_for(&project());
    let mut a = request(
        &snapshot,
        "keep",
        ScopeKind::AllExistingAndFuture,
        RuleKey::DeliveryTarget,
    );
    a.candidate_policy.due_date = DateDuePolicy::KeepOriginal;
    let first = adopt(&store, &snapshot, &a);
    let reminder = request(
        &first,
        "reminder",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Reminder,
    );
    let second = adopt(&store, &first, &reminder);
    let mut b = request(
        &second,
        "extend",
        ScopeKind::AllExistingAndFuture,
        RuleKey::DeliveryTarget,
    );
    let blocked = rehearse_change(&second, &b, day(), now()).unwrap();
    assert!(blocked
        .conflicts
        .iter()
        .any(|c| c.decision_id == a.decision_id));
    assert!(prepare_adoption(&second, &b, &blocked, &b.request_id, day(), now()).is_err());
    // The reminder is intentionally a separate constraint; a changed due date also
    // changes its displayed countdown, so its concrete regression must be explicit.
    b.supersedes = vec![a.decision_id.clone(), reminder.decision_id.clone()];
    let next = adopt(&store, &second, &b);
    assert_eq!(next.decisions[0].status, DecisionStatus::Superseded);
    assert_eq!(next.decisions[1].status, DecisionStatus::Superseded);
    assert_eq!(next.decisions[2].status, DecisionStatus::Active);
    assert_eq!(next.decisions[2].intent_revision, 2);
    assert_eq!(
        decision_behavior_revision_id(&next.decisions[2]).unwrap(),
        Some(b.revision_id.clone())
    );
    assert_eq!(next.decisions[2].supersedes, b.supersedes);
}

#[test]
fn disjoint_scope_and_rule_decisions_are_replayed_without_false_conflicts() {
    let snapshot = add_record(&project(), "other", "in_progress", now());
    let (_dir, store, snapshot) = store_for(&snapshot);
    let timer = request(&snapshot, "timer", ScopeKind::SingleRecord, RuleKey::Timer);
    let first = adopt(&store, &snapshot, &timer);
    let due = request(
        &first,
        "due",
        ScopeKind::SingleRecord,
        RuleKey::DeliveryTarget,
    );
    let preview = rehearse_change(&first, &due, day(), now()).unwrap();
    assert!(preview.conflicts.is_empty());
    assert_eq!(
        preview.checked_decision_ids,
        vec![timer.decision_id.clone()]
    );
    let second = adopt(&store, &first, &due);
    assert_eq!(second.decisions[0].status, DecisionStatus::Active);
    let mut other = request(&second, "other", ScopeKind::SingleRecord, RuleKey::Timer);
    other.scope = freeze_scope(&second, ScopeKind::SingleRecord, Some("other")).unwrap();
    other.cases[0].target = RehearsalTarget::Record("other".into());
    other.candidate_policy.timer = TimerPolicy::PauseStagesMarkedPaused;
    assert!(rehearse_change(&second, &other, day(), now())
        .unwrap()
        .conflicts
        .is_empty());
}

#[test]
fn all_freshness_inputs_and_stale_callbacks_block_adoption() {
    let snapshot = project();
    let req = request(
        &snapshot,
        "fresh",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let preview = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert!(prepare_adoption(&snapshot, &req, &preview, "new-request", day(), now()).is_err());
    assert!(prepare_adoption(
        &snapshot,
        &req,
        &preview,
        &req.request_id,
        fixture::date(2026, 10, 6),
        fixture::at(2026, 10, 6, 12)
    )
    .is_err());
    let mut changed = req.clone();
    changed.candidate_policy.timer = TimerPolicy::PauseStagesMarkedPaused;
    assert!(
        prepare_adoption(&snapshot, &changed, &preview, &req.request_id, day(), now()).is_err()
    );
    changed = req.clone();
    changed.scope = freeze_scope(&snapshot, ScopeKind::SingleRecord, Some("record-1")).unwrap();
    assert!(
        prepare_adoption(&snapshot, &changed, &preview, &req.request_id, day(), now()).is_err()
    );
    changed = req.clone();
    changed.cases[0].scenario.name = "不同场景".into();
    assert!(
        prepare_adoption(&snapshot, &changed, &preview, &req.request_id, day(), now()).is_err()
    );
    let mut newer = snapshot.clone();
    newer.spec_revisions[0].display_name = "changed".into();
    assert!(prepare_adoption(&newer, &req, &preview, &req.request_id, day(), now()).is_err());
    let mut forged = preview.clone();
    forged.evidence[0].runtime_semantics_version += 1;
    assert!(prepare_adoption(&snapshot, &req, &forged, &req.request_id, day(), now()).is_err());
    forged = preview.clone();
    forged.evidence[0].source = EvidenceSource::ImportedUntrusted;
    assert!(prepare_adoption(&snapshot, &req, &forged, &req.request_id, day(), now()).is_err());
    let pending = request(
        &snapshot,
        "pending-freshness",
        ScopeKind::SingleRecord,
        RuleKey::Timer,
    );
    let with_decision =
        prepare_unresolved(&snapshot, &pending, DecisionChoice::Defer, now()).unwrap();
    assert!(prepare_adoption(
        with_decision.snapshot(),
        &req,
        &preview,
        &req.request_id,
        day(),
        now()
    )
    .is_err());
    let edited = apply_command(
        &snapshot,
        &ToolCommand::EditRecord {
            operation_id: "freshness-edit".into(),
            expected_generation: snapshot.generation,
            record_id: "record-1".into(),
            expected_record_revision: 2,
            changes: BTreeMap::from([(
                "notes".into(),
                Some(FieldValue::Text("changed data".into())),
            )]),
            occurred_at: now(),
        },
        now(),
    )
    .unwrap();
    assert!(prepare_adoption(&edited, &req, &preview, &req.request_id, day(), now()).is_err());
    newer = snapshot.clone();
    newer.generation += 1;
    assert!(prepare_adoption(&newer, &req, &preview, &req.request_id, day(), now()).is_err());
}

#[test]
fn failure_and_forged_outcomes_never_become_adoptable() {
    let snapshot = project();
    let mut req = request(
        &snapshot,
        "failure",
        ScopeKind::SingleRecord,
        RuleKey::Timer,
    );
    req.cases[0].scenario.expected = vec![EvidenceObservation::from(
        &tool_runtime::evaluate_record(&snapshot, "record-1", &fixture::standard_behavior(), day())
            .unwrap(),
    )];
    let failed = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert_eq!(failed.evidence[0].status, EvidenceStatus::Failed);
    assert!(prepare_adoption(&snapshot, &req, &failed, &req.request_id, day(), now()).is_err());
    req.cases[0].scenario.expected.clear();
    let mut forged = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    forged.evidence[0].observations[0].elapsed_work_days = Some(99);
    assert!(prepare_adoption(&snapshot, &req, &forged, &req.request_id, day(), now()).is_err());
}

#[test]
fn withdrawal_retains_day_thirty_business_data_and_independent_rule() {
    let mut initial = add_record(&project(), "done-old", "completed", now());
    initial = add_record(&initial, "cancelled-old", "cancelled", now());
    let (_dir, store, base) = store_for(&initial);
    let r1 = request(&base, "r1", ScopeKind::AllExistingAndFuture, RuleKey::Timer);
    let first = adopt(&store, &base, &r1);
    let r2 = request(
        &first,
        "r2",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Reminder,
    );
    let mut current = adopt(&store, &first, &r2);
    let later = fixture::at(2026, 10, 30, 12);
    for id in ["new-one", "new-two", "new-three"] {
        let candidate = add_record(&current, id, "in_progress", later);
        current = store
            .commit(current.generation, &format!("create-{id}"), &candidate)
            .unwrap()
            .snapshot;
    }
    for id in ["new-one", "new-two"] {
        let revision = current
            .records
            .iter()
            .find(|r| r.record_id == id)
            .unwrap()
            .record_revision;
        let candidate = apply_command(
            &current,
            &ToolCommand::EditRecord {
                operation_id: format!("note-{id}"),
                expected_generation: current.generation,
                record_id: id.into(),
                expected_record_revision: revision,
                changes: BTreeMap::from([(
                    "notes".into(),
                    Some(FieldValue::Text(format!("保留新备注 {id}"))),
                )]),
                occurred_at: later,
            },
            later,
        )
        .unwrap();
        current = store
            .commit(current.generation, &format!("note-{id}"), &candidate)
            .unwrap()
            .snapshot;
    }
    let candidate = apply_command(
        &current,
        &ToolCommand::TransitionStage {
            operation_id: "complete-new".into(),
            expected_generation: current.generation,
            record_id: "new-three".into(),
            expected_record_revision: 1,
            to_stage_id: "completed".into(),
            occurred_at: later,
        },
        later,
    )
    .unwrap();
    current = store
        .commit(current.generation, "complete-new", &candidate)
        .unwrap()
        .snapshot;
    let before_records = current.records.clone();
    let before_events = current.event_history.clone();
    let preview = rehearse_withdrawal(
        &current,
        &r1.revision_id,
        "undo-r1",
        "undo-request",
        fixture::date(2026, 10, 30),
        later,
    )
    .unwrap();
    assert!(preview.conflicts.is_empty());
    assert_eq!(preview.preserved_record_count, 6);
    let prepared = prepare_withdrawal(
        &current,
        &preview,
        "undo-request",
        fixture::date(2026, 10, 30),
        later,
    )
    .unwrap();
    let undone = commit(&store, &prepared);
    assert_eq!(undone.records, before_records);
    assert_eq!(undone.event_history, before_events);
    assert_eq!(undone.decisions[0].status, DecisionStatus::Withdrawn);
    assert_eq!(undone.decisions[1].status, DecisionStatus::Active);
    for record in &undone.records {
        let policy = policy_for_record(&undone, &record.record_id).unwrap();
        assert_eq!(policy.timer, TimerPolicy::PauseStagesMarkedPaused);
        assert_eq!(policy.reminder, r2.candidate_policy.reminder);
    }
    assert_eq!(
        policy_for_future(&undone).unwrap().timer,
        TimerPolicy::PauseStagesMarkedPaused
    );
    assert_eq!(
        store
            .commit(
                prepared.expected_generation(),
                prepared.operation_id(),
                prepared.snapshot()
            )
            .unwrap()
            .disposition,
        CommitDisposition::AlreadyApplied
    );
    assert!(rehearse_withdrawal(
        &undone,
        &r1.revision_id,
        "undo-again",
        "request-again",
        fixture::date(2026, 10, 30),
        later
    )
    .is_err());
}

#[test]
fn withdrawal_preserves_later_same_dimension_and_explains_partial_supersession() {
    let snapshot = add_record(&project(), "other", "in_progress", now());
    let (_dir, store, snapshot) = store_for(&snapshot);
    let r1 = request(
        &snapshot,
        "r1",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let first = adopt(&store, &snapshot, &r1);
    let mut r3 = request(&first, "r3", ScopeKind::SingleRecord, RuleKey::Timer);
    r3.candidate_policy.timer = TimerPolicy::PauseStagesMarkedPaused;
    r3.supersedes = vec![r1.decision_id.clone()];
    let current = adopt(&store, &first, &r3);
    let preview = rehearse_withdrawal(
        &current,
        &r1.revision_id,
        "undo-r1",
        "undo-request",
        day(),
        now(),
    )
    .unwrap();
    assert!(preview
        .preserved_later_bindings
        .iter()
        .any(|p| p.record_id.as_deref() == Some("record-1") && p.rule_key == RuleKey::Timer));
    let prepared = prepare_withdrawal(&current, &preview, "undo-request", day(), now()).unwrap();
    let undone = commit(&store, &prepared);
    assert_eq!(
        policy_for_record(&undone, "record-1").unwrap().timer,
        r3.candidate_policy.timer
    );
    assert_eq!(undone.decisions[1].status, DecisionStatus::Active);
    assert_eq!(
        policy_for_record(&undone, "other").unwrap().timer,
        TimerPolicy::PauseStagesMarkedPaused
    );
}

#[test]
fn withdrawal_cannot_silently_break_later_active_intent() {
    let (_dir, store, snapshot) = store_for(&project());
    let r1 = request(
        &snapshot,
        "r1",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let first = adopt(&store, &snapshot, &r1);
    let r2 = request(&first, "r2", ScopeKind::SingleRecord, RuleKey::Timer);
    let current = adopt(&store, &first, &r2);
    let preview = rehearse_withdrawal(
        &current,
        &r2.revision_id,
        "undo-r2",
        "undo-request",
        day(),
        now(),
    )
    .unwrap();
    // Same rule restores R1, whose decision must still be checked.
    assert!(preview.conflicts.is_empty());
    assert!(preview.checked_decision_ids.contains(&r1.decision_id));
}

#[test]
fn commit_retry_after_lost_reply_is_idempotent_and_two_writers_conflict() {
    let (_dir, store, snapshot) = store_for(&project());
    let req = request(
        &snapshot,
        "commit",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let preview = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    let prepared =
        prepare_adoption(&snapshot, &req, &preview, &req.request_id, day(), now()).unwrap();
    assert!(matches!(
        store.commit_with_fault(
            prepared.expected_generation(),
            prepared.operation_id(),
            prepared.snapshot(),
            StoreFaultPoint::AfterPointerSwitch
        ),
        Err(StoreError::InjectedInterruption(_))
    ));
    let retry = store
        .commit(
            prepared.expected_generation(),
            prepared.operation_id(),
            prepared.snapshot(),
        )
        .unwrap();
    assert_eq!(retry.disposition, CommitDisposition::AlreadyApplied);
    assert_eq!(retry.snapshot.decisions.len(), 1);
    let other = request(
        &snapshot,
        "other",
        ScopeKind::SingleRecord,
        RuleKey::Reminder,
    );
    let other_preview = rehearse_change(&snapshot, &other, day(), now()).unwrap();
    let other_prepared = prepare_adoption(
        &snapshot,
        &other,
        &other_preview,
        &other.request_id,
        day(),
        now(),
    )
    .unwrap();
    assert!(matches!(
        store.commit(
            other_prepared.expected_generation(),
            other_prepared.operation_id(),
            other_prepared.snapshot()
        ),
        Err(StoreError::GenerationConflict { .. })
    ));
    let mut altered = prepared.snapshot().clone();
    altered.project_name = "different request".into();
    assert!(matches!(
        store.commit(
            prepared.expected_generation(),
            prepared.operation_id(),
            &altered
        ),
        Err(StoreError::OperationIdentityConflict { .. })
    ));
}

#[test]
fn non_order_domain_rehearses_adopts_and_withdraws_with_same_engine() {
    let snapshot =
        fixture::active_waiting_project(fixture::maintenance_task_spec(), "repair-project");
    let (_dir, store, snapshot) = store_for(&snapshot);
    let req = request(
        &snapshot,
        "repair",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let current = adopt(&store, &snapshot, &req);
    assert_eq!(
        evaluate_bound_record(&current, "record-1", day())
            .unwrap()
            .elapsed_work_days,
        Some(4)
    );
    let preview = rehearse_withdrawal(
        &current,
        &req.revision_id,
        "repair-undo",
        "repair-request",
        day(),
        now(),
    )
    .unwrap();
    let result = commit(
        &store,
        &prepare_withdrawal(&current, &preview, "repair-request", day(), now()).unwrap(),
    );
    assert_eq!(
        evaluate_bound_record(&result, "record-1", day())
            .unwrap()
            .elapsed_work_days,
        Some(1)
    );
    assert_eq!(result.records, snapshot.records);
}

#[test]
fn invalid_scope_duplicate_ids_and_missing_regression_evidence_fail_closed() {
    let snapshot = project();
    let mut req = request(
        &snapshot,
        "invalid",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    req.scope.frozen_record_ids.clear();
    assert!(rehearse_change(&snapshot, &req, day(), now()).is_err());
    req = request(
        &snapshot,
        "invalid",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    req.rule_keys.push(RuleKey::Timer);
    assert!(rehearse_change(&snapshot, &req, day(), now()).is_err());
    req = request(
        &snapshot,
        "invalid",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    req.cases.clear();
    assert!(rehearse_change(&snapshot, &req, day(), now()).is_err());
    let (_dir, store, snapshot) = store_for(&snapshot);
    req = request(
        &snapshot,
        "valid",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let mut current = adopt(&store, &snapshot, &req);
    current.decisions[0].extensions.clear();
    let next = request(
        &current,
        "next",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Reminder,
    );
    assert!(!rehearse_change(&current, &next, day(), now())
        .unwrap()
        .conflicts
        .is_empty());
}

#[test]
fn confirmed_scope_constrains_other_records_and_future_defaults() {
    let snapshot = add_record(&project(), "other", "in_progress", now());
    let (_dir, store, snapshot) = store_for(&snapshot);
    let r1 = request(
        &snapshot,
        "whole-scope",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let current = adopt(&store, &snapshot, &r1);
    for kind in [ScopeKind::SingleRecord, ScopeKind::FutureRecords] {
        let mut next = request(&current, "opposite", kind, RuleKey::Timer);
        next.candidate_policy.timer = TimerPolicy::PauseStagesMarkedPaused;
        if kind == ScopeKind::SingleRecord {
            next.scope = freeze_scope(&current, kind, Some("other")).unwrap();
            next.cases[0].target = RehearsalTarget::Record("other".into());
        }
        let preview = rehearse_change(&current, &next, day(), now()).unwrap();
        assert!(preview
            .conflicts
            .iter()
            .any(|conflict| conflict.decision_id == r1.decision_id));
    }
}

#[test]
fn withdrawal_blocks_a_real_later_reminder_dependency() {
    let (_dir, store, snapshot) = store_for(&project());
    let r1 = request(
        &snapshot,
        "extend",
        ScopeKind::AllExistingAndFuture,
        RuleKey::DeliveryTarget,
    );
    let first = adopt(&store, &snapshot, &r1);
    let r2 = request(
        &first,
        "remind",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Reminder,
    );
    let current = adopt(&store, &first, &r2);
    let preview = rehearse_withdrawal(
        &current,
        &r1.revision_id,
        "withdraw-extend",
        "withdraw-request",
        day(),
        now(),
    )
    .unwrap();
    assert!(preview
        .conflicts
        .iter()
        .any(|conflict| conflict.decision_id == r2.decision_id));
    assert!(matches!(
        prepare_withdrawal(&current, &preview, "withdraw-request", day(), now()),
        Err(DecisionError::Conflicts(_))
    ));
    assert_eq!(current, load(&store));
}

#[test]
fn pending_resolution_is_explicit_and_keeps_original_intent_history() {
    let (_dir, store, snapshot) = store_for(&project());
    let unknown = request(
        &snapshot,
        "unknown-first",
        ScopeKind::SingleRecord,
        RuleKey::Timer,
    );
    let pending = commit(
        &store,
        &prepare_unresolved(&snapshot, &unknown, DecisionChoice::Defer, now()).unwrap(),
    );
    let mut resolved = request(
        &pending,
        "resolved",
        ScopeKind::SingleRecord,
        RuleKey::Timer,
    );
    resolved.supersedes = vec![unknown.decision_id.clone()];
    let current = adopt(&store, &pending, &resolved);
    assert_eq!(current.decisions[0].status, DecisionStatus::Superseded);
    assert_eq!(current.decisions[0].choice, DecisionChoice::Defer);
    assert_eq!(current.decisions[1].intent_revision, 2);
    assert_eq!(
        decision_original_request(&current.decisions[0]).unwrap(),
        unknown.original_request
    );
}

#[test]
fn repeated_withdrawals_follow_attribution_without_resurrecting_removed_rules() {
    let (_dir, store, snapshot) = store_for(&project());
    let r0 = request(
        &snapshot,
        "r0",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let first = adopt(&store, &snapshot, &r0);
    let mut r1 = request(
        &first,
        "r1",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    r1.candidate_policy.timer = TimerPolicy::PauseStagesMarkedPaused;
    r1.supersedes = vec![r0.decision_id.clone()];
    let second = adopt(&store, &first, &r1);
    let preview0 = rehearse_withdrawal(
        &second,
        &r0.revision_id,
        "withdraw-r0",
        "request-0",
        day(),
        now(),
    )
    .unwrap();
    let third = commit(
        &store,
        &prepare_withdrawal(&second, &preview0, "request-0", day(), now()).unwrap(),
    );
    let preview1 = rehearse_withdrawal(
        &third,
        &r1.revision_id,
        "withdraw-r1",
        "request-1",
        day(),
        now(),
    )
    .unwrap();
    let last = commit(
        &store,
        &prepare_withdrawal(&third, &preview1, "request-1", day(), now()).unwrap(),
    );
    assert_eq!(
        policy_for_record(&last, "record-1").unwrap().timer,
        TimerPolicy::PauseStagesMarkedPaused
    );
    assert_eq!(
        policy_for_future(&last).unwrap().timer,
        TimerPolicy::PauseStagesMarkedPaused
    );
}

#[test]
fn withdrawal_preview_becomes_stale_after_new_business_data() {
    let (_dir, store, snapshot) = store_for(&project());
    let req = request(
        &snapshot,
        "rule",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Timer,
    );
    let current = adopt(&store, &snapshot, &req);
    let preview = rehearse_withdrawal(
        &current,
        &req.revision_id,
        "undo",
        "undo-request",
        day(),
        now(),
    )
    .unwrap();
    assert!(prepare_withdrawal(&current, &preview, "new-request", day(), now()).is_err());
    let changed = add_record(&current, "added-after-preview", "in_progress", now());
    assert!(prepare_withdrawal(&changed, &preview, "undo-request", day(), now()).is_err());
    assert!(prepare_withdrawal(
        &current,
        &preview,
        "undo-request",
        fixture::date(2026, 10, 6),
        fixture::at(2026, 10, 6, 12)
    )
    .is_err());
}

#[test]
fn copied_wait_dates_and_desensitized_history_rehearse_without_touching_live_data() {
    let snapshot = project();
    let mut req = request(
        &snapshot,
        "copy",
        ScopeKind::SingleRecord,
        RuleKey::DeliveryTarget,
    );
    let copy = &mut req.cases[0].scenario;
    copy.event_history[1].occurred_at = fixture::at(2026, 10, 3, 9);
    copy.records[0]
        .typed_values
        .insert("order_number".into(), FieldValue::Text("虚构示例".into()));
    if let RecordEventKind::Created { initial_values, .. } = &mut copy.event_history[0].kind {
        initial_values.insert("order_number".into(), FieldValue::Text("虚构示例".into()));
    }
    let before = snapshot.clone();
    let preview = rehearse_change(&snapshot, &req, day(), now()).unwrap();
    assert_eq!(preview.evidence[0].observations[0].paused_days, 2);
    assert_eq!(
        preview.evidence[0].observations[0].elapsed_work_days,
        Some(2)
    );
    assert_eq!(
        preview.evidence[0].observations[0].display_due_date,
        Some(fixture::date(2026, 10, 8))
    );
    assert_eq!(snapshot, before);
    assert_eq!(
        preview.impacts[0].after.display_due_date,
        Some(fixture::date(2026, 10, 9))
    );
}

#[test]
fn heterogeneous_existing_dependencies_do_not_create_false_new_conflicts() {
    let snapshot = add_record(&project(), "other", "in_progress", now());
    let (_dir, store, snapshot) = store_for(&snapshot);
    let mut extension = request(
        &snapshot,
        "other-extension",
        ScopeKind::SingleRecord,
        RuleKey::DeliveryTarget,
    );
    extension.scope = freeze_scope(&snapshot, ScopeKind::SingleRecord, Some("other")).unwrap();
    extension.cases[0].target = RehearsalTarget::Record("other".into());
    let first = adopt(&store, &snapshot, &extension);
    let reminder = request(
        &first,
        "global-reminder",
        ScopeKind::AllExistingAndFuture,
        RuleKey::Reminder,
    );
    let second = adopt(&store, &first, &reminder);
    let unrelated = request(
        &second,
        "unrelated-timer",
        ScopeKind::SingleRecord,
        RuleKey::Timer,
    );
    let preview = rehearse_change(&second, &unrelated, day(), now()).unwrap();
    assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
    let third = adopt(&store, &second, &unrelated);
    assert!(third
        .decisions
        .iter()
        .all(|decision| decision.status == DecisionStatus::Active));
    // Removing an actual dependency must still conflict after preserving the
    // heterogeneous baseline: this changes the other record's delivery policy.
    let mut opposite = request(
        &third,
        "opposite-extension",
        ScopeKind::SingleRecord,
        RuleKey::DeliveryTarget,
    );
    opposite.scope = freeze_scope(&third, ScopeKind::SingleRecord, Some("other")).unwrap();
    opposite.cases[0].target = RehearsalTarget::Record("other".into());
    opposite.candidate_policy.due_date = DateDuePolicy::KeepOriginal;
    let changed = rehearse_change(&third, &opposite, day(), now()).unwrap();
    assert!(changed
        .conflicts
        .iter()
        .any(|conflict| conflict.decision_id == reminder.decision_id));
}
