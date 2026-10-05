#[path = "support/egui_harness.rs"]
mod egui_harness;
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
#[path = "../src/tool_studio.rs"]
mod tool_studio;
#[path = "../src/ui/tool_workspace.rs"]
mod tool_workspace;
#[path = "../src/tool_workspace_protocol.rs"]
mod tool_workspace_protocol;
mod ui {
    pub mod tool_workspace {
        pub use crate::tool_workspace::*;
    }
}
use std::collections::BTreeMap;
use tool_project::*;
use tool_studio::ToolStudio;
use tool_workspace_protocol::*;

fn temp_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("tool-studio-test-")
        .tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap()
}
fn studio() -> ToolStudio {
    ToolStudio::with_clock(fixture::at(2026, 10, 5, 12))
}
fn request(
    studio: &ToolStudio,
    action: WorkspaceAction,
    record: Option<&str>,
    epoch: u64,
) -> WorkspaceRequest {
    let view = studio.workspace_view();
    WorkspaceRequest {
        context: RequestContext {
            request_id: view.next_operation_id.into(),
            session_id: view.session_id.into(),
            project_id: view.project.map(|m| m.project_id.clone()),
            generation: view.project.map(|m| m.generation),
            record_id: record.map(str::to_owned),
            record_revision: studio.snapshot().and_then(|s| {
                s.records
                    .iter()
                    .find(|r| Some(r.record_id.as_str()) == record)
                    .map(|r| r.record_revision)
            }),
            input_epoch: epoch,
        },
        action,
    }
}
fn send(
    studio: &mut ToolStudio,
    action: WorkspaceAction,
    record: Option<&str>,
    epoch: u64,
) -> OperationOutcome {
    let request = request(studio, action, record, epoch);
    studio.handle(request)
}
fn open_fixture() -> (tempfile::TempDir, ToolStudio) {
    let dir = temp_dir();
    tool_store::ProjectStore::create(
        dir.path().join("project"),
        &fixture::studio_order_project("integration"),
        "initial",
    )
    .unwrap();
    let mut s = studio();
    send(
        &mut s,
        WorkspaceAction::OpenProject {
            location: dir.path().join("project").display().to_string(),
        },
        None,
        1,
    );
    assert!(s.snapshot().is_some());
    (dir, s)
}
fn input(s: &ToolStudio, scope: ScopeKind) -> RuleInput {
    let a = tool_decisions::policy_for_record(s.snapshot().unwrap(), "record-1").unwrap();
    let mut b = a.clone();
    b.due_date = DateDuePolicy::ExtendByPausedDays;
    RuleInput {
        rule_keys: vec![RuleKey::DeliveryTarget],
        original_request: "等待时如何处理？".into(),
        rationale: "保持原始承诺，比较真实后果".into(),
        unresolved_questions: vec![],
        supersedes: vec![],
        scope,
        candidates: [a, b],
        as_of_date: fixture::date(2026, 10, 5),
        record_changes: BTreeMap::new(),
        stage_override: None,
    }
}
fn adopt(s: &mut ToolStudio, input: RuleInput, candidate: usize, epoch: u64) -> OperationOutcome {
    assert_eq!(
        send(
            s,
            WorkspaceAction::Rehearse {
                input: input.clone()
            },
            Some("record-1"),
            epoch
        ),
        OperationOutcome::PreviewReady
    );
    let p = s.workspace_view().rehearsal.unwrap().clone();
    send(
        s,
        WorkspaceAction::SaveDecision {
            choice: DecisionChoice::Adopt,
            input,
            preview_id: Some(p.preview_id),
            candidate_id: Some(p.candidates[candidate].candidate_id.clone()),
        },
        Some("record-1"),
        epoch,
    )
}
#[test]
fn no_repository_create_record_reopen_and_example_isolation() {
    let dir = temp_dir();
    let path = dir.path().join("real");
    let mut s = studio();
    send(&mut s, WorkspaceAction::StartExample, None, 1);
    assert_eq!(s.snapshot().unwrap().records.len(), 1);
    send(&mut s, WorkspaceAction::CloseProject, None, 2);
    let created = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "真实工具".into(),
            location: path.display().to_string(),
            spec: studio_order_template(),
        },
        None,
        3,
    );
    assert!(s.snapshot().is_some(), "{created:?}");
    assert!(s.snapshot().unwrap().records.is_empty());
    let op = s.workspace_view().next_operation_id.to_owned();
    let snap = s.snapshot().unwrap();
    let cmd = ToolCommand::CreateRecord {
        operation_id: op,
        expected_generation: snap.generation,
        record_id: "real-record".into(),
        initial_stage_id: "queued".into(),
        values: BTreeMap::from([
            ("order_number".into(), FieldValue::Text("真实的一笔".into())),
            (
                "work_description".into(),
                FieldValue::Text("手工制作".into()),
            ),
        ]),
        occurred_at: fixture::at(2026, 10, 5, 12),
    };
    assert_eq!(
        send(&mut s, WorkspaceAction::Record(cmd), None, 4),
        OperationOutcome::Committed
    );
    send(&mut s, WorkspaceAction::CloseProject, None, 5);
    send(
        &mut s,
        WorkspaceAction::OpenProject {
            location: path.display().to_string(),
        },
        None,
        6,
    );
    assert_eq!(s.snapshot().unwrap().records.len(), 1);
    assert_eq!(s.snapshot().unwrap().records[0].record_id, "real-record");
}
#[test]
fn real_ab_rehearsal_and_copy_edits_never_mutate_business_data() {
    let (_dir, mut s) = open_fixture();
    let before = s.snapshot().unwrap().clone();
    let mut i = input(&s, ScopeKind::SingleRecord);
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i.clone() },
        Some("record-1"),
        2,
    );
    let p = s.workspace_view().rehearsal.unwrap();
    let a = &p.candidates[0].evidence[0].observations[0];
    let b = &p.candidates[1].evidence[0].observations[0];
    assert_eq!((a.elapsed_work_days, a.paused_days), (Some(1), 3));
    assert_eq!(a.display_due_date, Some(fixture::date(2026, 10, 6)));
    assert_eq!(b.display_due_date, Some(fixture::date(2026, 10, 9)));
    i.record_changes.insert(
        "promised_on".into(),
        Some(FieldValue::Date(fixture::date(2026, 10, 8))),
    );
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i },
        Some("record-1"),
        3,
    );
    assert_eq!(
        s.workspace_view().rehearsal.unwrap().candidates[1].evidence[0].observations[0]
            .display_due_date,
        Some(fixture::date(2026, 10, 11))
    );
    assert_eq!(s.snapshot().unwrap(), &before);
}
#[test]
fn all_four_scopes_use_bound_daily_policy_and_frozen_sets() {
    for scope in [
        ScopeKind::SingleRecord,
        ScopeKind::FutureRecords,
        ScopeKind::IncompleteAndFuture,
        ScopeKind::AllExistingAndFuture,
    ] {
        let (_dir, mut s) = open_fixture();
        let i = input(&s, scope);
        assert_eq!(adopt(&mut s, i, 1, 2), OperationOutcome::Committed);
        let snap = s.snapshot().unwrap();
        let current = tool_decisions::policy_for_record(snap, "record-1").unwrap();
        let future = tool_decisions::policy_for_future(snap).unwrap();
        assert_eq!(
            current.due_date,
            if scope == ScopeKind::FutureRecords {
                DateDuePolicy::KeepOriginal
            } else {
                DateDuePolicy::ExtendByPausedDays
            }
        );
        assert_eq!(
            future.due_date,
            if scope == ScopeKind::SingleRecord {
                DateDuePolicy::KeepOriginal
            } else {
                DateDuePolicy::ExtendByPausedDays
            }
        );
        assert_eq!(s.workspace_view().record_policies["record-1"], current);
    }
}
#[test]
fn unresolved_choices_persist_original_words_without_activating_rules() {
    for choice in [
        DecisionChoice::Both,
        DecisionChoice::Neither,
        DecisionChoice::Defer,
    ] {
        let (_dir, mut s) = open_fixture();
        let before = s.snapshot().unwrap().behavior_revisions.clone();
        let i = input(&s, ScopeKind::SingleRecord);
        assert_eq!(
            send(
                &mut s,
                WorkspaceAction::SaveDecision {
                    choice,
                    input: i.clone(),
                    preview_id: None,
                    candidate_id: None
                },
                Some("record-1"),
                2
            ),
            OperationOutcome::Committed
        );
        let snap = s.snapshot().unwrap();
        assert_eq!(snap.behavior_revisions, before);
        assert_eq!(snap.decisions[0].status, DecisionStatus::Pending);
        assert_eq!(
            s.workspace_view().decision_requests[&snap.decisions[0].decision_id],
            i.original_request
        );
    }
}
#[test]
fn lost_commit_ack_retries_exact_candidate_and_never_doubles_effects() {
    let (_dir, mut s) = open_fixture();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i.clone() },
        Some("record-1"),
        2,
    );
    let p = s.workspace_view().rehearsal.unwrap().clone();
    let req = request(
        &s,
        WorkspaceAction::SaveDecision {
            choice: DecisionChoice::Adopt,
            input: i,
            preview_id: Some(p.preview_id),
            candidate_id: Some(p.candidates[1].candidate_id.clone()),
        },
        Some("record-1"),
        2,
    );
    s.fail_next_commit(tool_store::StoreFaultPoint::AfterPointerSwitch);
    assert!(matches!(s.handle(req.clone()), OperationOutcome::Failed(_)));
    let failed_delivery = s.workspace_view().operation.unwrap().delivery_id;
    s.set_clock(fixture::at(2026, 10, 6, 12));
    assert_eq!(s.handle(req.clone()), OperationOutcome::Committed);
    assert_eq!(s.snapshot().unwrap().decisions.len(), 1);
    assert!(s.workspace_view().operation.unwrap().delivery_id > failed_delivery);
    let generation = s.snapshot().unwrap().generation;
    assert_eq!(s.handle(req), OperationOutcome::Committed);
    assert_eq!(s.snapshot().unwrap().generation, generation);
}
#[test]
fn stale_identity_and_raw_behavior_bypass_are_rejected() {
    let (_dir, mut s) = open_fixture();
    let before = s.snapshot().unwrap().clone();
    let i = input(&s, ScopeKind::SingleRecord);
    let req = request(
        &s,
        WorkspaceAction::Rehearse { input: i },
        Some("record-1"),
        2,
    );
    for field in 0..5 {
        let mut stale = req.clone();
        match field {
            0 => stale.context.session_id.push('x'),
            1 => stale.context.project_id = Some("wrong".into()),
            2 => stale.context.generation = Some(42),
            3 => stale.context.record_revision = Some(42),
            _ => stale.context.record_id = Some("missing".into()),
        };
        assert!(matches!(s.handle(stale), OperationOutcome::Rejected(_)));
    }
    let cmd = ToolCommand::SetBehavior {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: before.generation,
        revision_id: "bypass".into(),
        parent_revision_id: None,
        policy: BehaviorPolicy::default(),
        reason: "bypass".into(),
        occurred_at: fixture::at(2026, 10, 5, 12),
    };
    assert!(matches!(
        send(&mut s, WorkspaceAction::Record(cmd), Some("record-1"), 3),
        OperationOutcome::Rejected(_)
    ));
    assert_eq!(s.snapshot().unwrap(), &before);
}
#[test]
fn concurrent_save_rejects_stale_preview_without_overwriting_other_writer() {
    let (dir, mut s) = open_fixture();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i.clone() },
        Some("record-1"),
        2,
    );
    let p = s.workspace_view().rehearsal.unwrap().clone();
    let other = tool_store::ProjectStore::open(dir.path().join("project")).unwrap();
    let mut changed = s.snapshot().unwrap().clone();
    changed.project_name = "另一窗口的修改".into();
    other
        .commit(changed.generation, "other-save", &changed)
        .unwrap();
    assert!(matches!(
        send(
            &mut s,
            WorkspaceAction::SaveDecision {
                choice: DecisionChoice::Adopt,
                input: i,
                preview_id: Some(p.preview_id),
                candidate_id: Some(p.candidates[1].candidate_id.clone())
            },
            Some("record-1"),
            2
        ),
        OperationOutcome::Rejected(_)
    ));
    assert_eq!(s.snapshot().unwrap().project_name, "另一窗口的修改");
    assert!(s.snapshot().unwrap().decisions.is_empty());
}
#[test]
fn corrupt_project_is_diagnostic_and_never_becomes_empty_writable_success() {
    let dir = temp_dir();
    let path = dir.path().join("broken");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("CURRENT"), b"broken bytes").unwrap();
    let mut s = studio();
    send(
        &mut s,
        WorkspaceAction::OpenProject {
            location: path.display().to_string(),
        },
        None,
        1,
    );
    assert!(s.snapshot().is_none());
    assert!(s.diagnostic().is_some());
    assert_eq!(
        std::fs::read(path.join("CURRENT")).unwrap(),
        b"broken bytes"
    );
}
#[test]
fn actual_egui_example_action_reaches_controller_and_renders_real_records() {
    let mut h = egui_harness::EguiHarness::new(egui::vec2(800.0, 900.0));
    let mut ui = tool_workspace::ToolWorkspace::default();
    let mut s = studio();
    let frame = |h: &mut egui_harness::EguiHarness,
                 ui: &mut tool_workspace::ToolWorkspace,
                 s: &ToolStudio| {
        h.frame(|ctx| {
            egui::CentralPanel::default()
                .show(ctx, |egui| ui.show(egui, &s.workspace_view()))
                .inner
        })
    };
    let rect = frame(&mut h, &mut ui, &s).controls["entry.example"].rect;
    h.press_at(rect.center());
    frame(&mut h, &mut ui, &s);
    h.release_at(rect.center());
    let output = frame(&mut h, &mut ui, &s);
    s.handle(output.request.unwrap());
    assert_eq!(s.snapshot().unwrap().records.len(), 1);
    let output = frame(&mut h, &mut ui, &s);
    assert!(output.controls.contains_key("record.new"));
    assert!(output.text.iter().any(|t| t.contains("虚构示例")));
}
fn ui_frame(
    h: &mut egui_harness::EguiHarness,
    ui: &mut tool_workspace::ToolWorkspace,
    s: &ToolStudio,
) -> tool_workspace::WorkspaceOutput {
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |egui| ui.show(egui, &s.workspace_view()))
            .inner
    })
}
fn ui_click(
    h: &mut egui_harness::EguiHarness,
    ui: &mut tool_workspace::ToolWorkspace,
    s: &mut ToolStudio,
    key: &str,
) {
    let first = ui_frame(h, ui, s);
    let control = first
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}"));
    assert!(control.enabled, "disabled {key}: {:?}", first.text);
    let center = control.rect.center();
    h.press_at(center);
    ui_frame(h, ui, s);
    h.release_at(center);
    let out = ui_frame(h, ui, s);
    if let Some(request) = out.request {
        let result = s.handle(request);
        assert!(
            !matches!(
                result,
                OperationOutcome::Rejected(_) | OperationOutcome::Failed(_)
            ),
            "{result:?}"
        );
    }
    ui_frame(h, ui, s);
}
fn ui_type(
    h: &mut egui_harness::EguiHarness,
    ui: &mut tool_workspace::ToolWorkspace,
    s: &mut ToolStudio,
    key: &str,
    text: &str,
) {
    ui_click(h, ui, s, key);
    h.key(egui::Key::A, true, egui::Modifiers::COMMAND);
    ui_frame(h, ui, s);
    h.key(egui::Key::A, false, egui::Modifiers::NONE);
    h.text(text);
    ui_frame(h, ui, s);
}
#[test]
fn configured_optional_field_uses_pinned_old_record_spec_and_active_new_spec() {
    let (_dir, mut s) = open_fixture();
    let mut spec = s.snapshot().unwrap().active_spec().unwrap().clone();
    spec.revision += 1;
    spec.fields.push(FieldDefinition {
        id: "custom-1".into(),
        display_name: "包装要求".into(),
        kind: FieldKind::Text,
        role: FieldRole::Custom,
        required: false,
        extensions: BTreeMap::new(),
    });
    spec.detail_field_ids.push("custom-1".into());
    assert_eq!(
        send(
            &mut s,
            WorkspaceAction::Configure {
                name: "已配置".into(),
                spec
            },
            None,
            2
        ),
        OperationOutcome::Committed
    );
    let location = s.workspace_view().location.unwrap().to_owned();
    send(&mut s, WorkspaceAction::CloseProject, None, 3);
    send(&mut s, WorkspaceAction::OpenProject { location }, None, 4);
    let mut h = egui_harness::EguiHarness::new(egui::vec2(900.0, 1600.0));
    let mut ui = tool_workspace::ToolWorkspace::default();
    ui_click(&mut h, &mut ui, &mut s, "record.select.record-1");
    ui_click(&mut h, &mut ui, &mut s, "record.edit");
    let out = ui_frame(&mut h, &mut ui, &s);
    assert!(!out.controls.contains_key("field.custom-1"));
    assert!(out.text.iter().any(|t| t.contains("版本")));
    ui_type(
        &mut h,
        &mut ui,
        &mut s,
        "field.work_description",
        "保留旧字段编辑能力",
    );
    ui_click(&mut h, &mut ui, &mut s, "record.save");
    assert_eq!(
        s.snapshot().unwrap().records[0].typed_values["work_description"],
        FieldValue::Text("保留旧字段编辑能力".into())
    );
    ui_click(&mut h, &mut ui, &mut s, "back");
    ui_click(&mut h, &mut ui, &mut s, "record.new");
    ui_type(&mut h, &mut ui, &mut s, "field.order_number", "新记录");
    ui_type(&mut h, &mut ui, &mut s, "field.work_description", "新规范");
    ui_click(&mut h, &mut ui, &mut s, "present.custom-1");
    ui_type(&mut h, &mut ui, &mut s, "field.custom-1", "礼盒");
    ui_click(&mut h, &mut ui, &mut s, "record.save");
    assert_eq!(s.snapshot().unwrap().records.len(), 2);
    assert_eq!(
        s.snapshot().unwrap().records[1].typed_values["custom-1"],
        FieldValue::Text("礼盒".into())
    );
}
fn add_live(s: &mut ToolStudio, id: &str, stage: &str, epoch: u64) {
    let command = ToolCommand::CreateRecord {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: s.snapshot().unwrap().generation,
        record_id: id.into(),
        initial_stage_id: stage.into(),
        values: s.snapshot().unwrap().records[0].typed_values.clone(),
        occurred_at: s.workspace_view().now,
    };
    assert_eq!(
        send(s, WorkspaceAction::Record(command), None, epoch),
        OperationOutcome::Committed
    );
}
fn edit_live(s: &mut ToolStudio, id: &str, notes: &str, epoch: u64) {
    let revision = s
        .snapshot()
        .unwrap()
        .records
        .iter()
        .find(|r| r.record_id == id)
        .unwrap()
        .record_revision;
    let command = ToolCommand::EditRecord {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: s.snapshot().unwrap().generation,
        record_id: id.into(),
        expected_record_revision: revision,
        changes: BTreeMap::from([("notes".into(), Some(FieldValue::Text(notes.into())))]),
        occurred_at: s.workspace_view().now,
    };
    assert_eq!(
        send(s, WorkspaceAction::Record(command), Some(id), epoch),
        OperationOutcome::Committed
    );
}
#[test]
fn day_thirty_withdrawal_preserves_new_records_notes_completion_and_retry() {
    let (_dir, mut s) = open_fixture();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    assert_eq!(adopt(&mut s, i, 1, 2), OperationOutcome::Committed);
    let decision = s.snapshot().unwrap().decisions[0].decision_id.clone();
    s.set_clock(fixture::at(2026, 10, 30, 12));
    for (index, stage) in ["in_progress", "queued", "cancelled"]
        .into_iter()
        .enumerate()
    {
        add_live(&mut s, &format!("day30-{index}"), stage, 3 + index as u64);
    }
    edit_live(&mut s, "record-1", "后来补充的真实备注", 6);
    edit_live(&mut s, "day30-0", "完成之前的备注", 7);
    let command = ToolCommand::TransitionStage {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: s.snapshot().unwrap().generation,
        record_id: "day30-0".into(),
        expected_record_revision: 2,
        to_stage_id: "completed".into(),
        occurred_at: s.workspace_view().now,
    };
    assert_eq!(
        send(&mut s, WorkspaceAction::Record(command), Some("day30-0"), 8),
        OperationOutcome::Committed
    );
    let facts = s.snapshot().unwrap().records.clone();
    let events = s.snapshot().unwrap().event_history.clone();
    assert_eq!(
        send(
            &mut s,
            WorkspaceAction::PreviewWithdrawal {
                decision_id: decision
            },
            None,
            9
        ),
        OperationOutcome::PreviewReady
    );
    let p = s.workspace_view().withdrawal.unwrap().clone();
    assert_eq!(p.preserved_record_count, 4);
    let req = request(
        &s,
        WorkspaceAction::ConfirmWithdrawal {
            preview_id: p.preview_id,
        },
        None,
        9,
    );
    s.fail_next_commit(tool_store::StoreFaultPoint::AfterPointerSwitch);
    assert!(matches!(s.handle(req.clone()), OperationOutcome::Failed(_)));
    assert_eq!(s.handle(req.clone()), OperationOutcome::Committed);
    assert_eq!(s.handle(req), OperationOutcome::Committed);
    assert_eq!(s.snapshot().unwrap().records, facts);
    assert_eq!(s.snapshot().unwrap().event_history, events);
    assert_eq!(
        tool_decisions::policy_for_future(s.snapshot().unwrap())
            .unwrap()
            .due_date,
        DateDuePolicy::KeepOriginal
    );
}
#[test]
fn regression_conflict_requires_explicit_supersession_and_preserves_unrelated_intent() {
    let (_dir, mut s) = open_fixture();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    assert_eq!(adopt(&mut s, i, 0, 2), OperationOutcome::Committed);
    let first = s.snapshot().unwrap().decisions[0].decision_id.clone();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i.clone() },
        Some("record-1"),
        3,
    );
    let p = s.workspace_view().rehearsal.unwrap().clone();
    assert!(!p.candidates[1].ready_to_adopt);
    assert_eq!(p.candidates[1].conflicts[0].decision_id, first);
    assert!(matches!(
        send(
            &mut s,
            WorkspaceAction::SaveDecision {
                choice: DecisionChoice::Adopt,
                input: i.clone(),
                preview_id: Some(p.preview_id),
                candidate_id: Some(p.candidates[1].candidate_id.clone())
            },
            Some("record-1"),
            3
        ),
        OperationOutcome::Rejected(_)
    ));
    let mut replacement = i;
    replacement.supersedes = vec![first];
    assert_eq!(
        adopt(&mut s, replacement, 1, 4),
        OperationOutcome::Committed
    );
    assert_eq!(
        s.snapshot().unwrap().decisions[0].status,
        DecisionStatus::Superseded
    );
    assert_eq!(
        s.snapshot().unwrap().decisions[1].status,
        DecisionStatus::Active
    );
}
#[test]
fn repeated_retry_failures_have_distinct_deliveries_and_preserve_exact_record_command() {
    let (_dir, mut s) = open_fixture();
    let command = ToolCommand::EditRecord {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: 0,
        record_id: "record-1".into(),
        expected_record_revision: 2,
        changes: BTreeMap::from([("notes".into(), Some(FieldValue::Text("一次写入".into())))]),
        occurred_at: s.workspace_view().now,
    };
    let req = request(&s, WorkspaceAction::Record(command), Some("record-1"), 2);
    let mut deliveries = vec![];
    for _ in 0..2 {
        s.fail_next_commit(tool_store::StoreFaultPoint::AfterSnapshotSync);
        assert!(matches!(s.handle(req.clone()), OperationOutcome::Failed(_)));
        deliveries.push(s.workspace_view().operation.unwrap().delivery_id);
    }
    assert!(deliveries[1] > deliveries[0]);
    s.set_clock(fixture::at(2026, 10, 6, 12));
    assert_eq!(s.handle(req), OperationOutcome::Committed);
    assert_eq!(s.snapshot().unwrap().records[0].record_revision, 3);
    assert_eq!(
        s.snapshot()
            .unwrap()
            .event_history
            .last()
            .unwrap()
            .occurred_at,
        fixture::at(2026, 10, 5, 12)
    );
}
#[test]
fn changed_preview_candidate_epoch_and_input_cannot_commit() {
    for defect in 0..4 {
        let (_dir, mut s) = open_fixture();
        let mut i = input(&s, ScopeKind::SingleRecord);
        send(
            &mut s,
            WorkspaceAction::Rehearse { input: i.clone() },
            Some("record-1"),
            2,
        );
        let p = s.workspace_view().rehearsal.unwrap().clone();
        let mut preview_id = p.preview_id;
        let mut candidate_id = p.candidates[1].candidate_id.clone();
        let mut epoch = 2;
        match defect {
            0 => preview_id.push('x'),
            1 => candidate_id.push('x'),
            2 => epoch = 3,
            _ => i.rationale.push('x'),
        }
        assert!(matches!(
            send(
                &mut s,
                WorkspaceAction::SaveDecision {
                    choice: DecisionChoice::Adopt,
                    input: i,
                    preview_id: Some(preview_id),
                    candidate_id: Some(candidate_id)
                },
                Some("record-1"),
                epoch
            ),
            OperationOutcome::Rejected(_)
        ));
        assert!(s.snapshot().unwrap().decisions.is_empty());
    }
}
fn export_package(
    s: &mut ToolStudio,
    dir: &std::path::Path,
    task: Option<tool_proposals::TaskIdentity>,
) -> serde_json::Value {
    let snap = s.snapshot().unwrap();
    let case = tool_decisions::RehearsalCase {
        scenario: tool_decisions::scenario_from_snapshot(
            snap,
            "selected-example",
            "已检查的虚构夹具",
            fixture::date(2026, 10, 5),
            fixture::at(2026, 10, 5, 12),
        )
        .unwrap(),
        target: tool_decisions::RehearsalTarget::Record("record-1".into()),
    };
    let selection = tool_proposals::ExportSelection {
        original_request: "只改变显示目标".into(),
        confirmed_intents: snap
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| tool_proposals::SelectedIntent {
                decision_id: d.decision_id.clone(),
                sanitized_intent: "保留已确认结果".into(),
            })
            .collect(),
        examples: vec![tool_proposals::SelectedExample {
            disclosure: tool_proposals::ExampleDisclosure::Synthetic,
            case,
        }],
        task,
    };
    let bytes = s.prepare_export(selection).unwrap();
    s.write_export(&dir.join("requirements.json")).unwrap();
    let export: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut proposal = export["proposal_template"].clone();
    proposal["envelope"]["candidate_policies"][0]["due_date"] =
        serde_json::json!("extend_by_paused_days");
    proposal["envelope"]["rationale"] = serde_json::json!("我已检查本地演练后果");
    proposal
}
#[test]
fn imported_proposal_uses_wrapper_persists_provenance_and_requires_fresh_export_after_reopen() {
    let (dir, mut s) = open_fixture();
    let package = export_package(&mut s, dir.path(), None);
    let bytes = serde_json::to_vec(&package).unwrap();
    s.import_proposal(bytes.as_slice(), None).unwrap();
    s.replay_imported(
        "candidate-1",
        "record-1",
        ScopeKind::AllExistingAndFuture,
        vec![],
        None,
    )
    .unwrap();
    s.fail_next_commit(tool_store::StoreFaultPoint::AfterPointerSwitch);
    assert!(matches!(
        s.save_imported(DecisionChoice::Adopt, None).unwrap(),
        OperationOutcome::Failed(_)
    ));
    assert_eq!(
        s.save_imported(DecisionChoice::Adopt, None).unwrap(),
        OperationOutcome::Committed
    );
    let decision = s.snapshot().unwrap().decisions[0].decision_id.clone();
    assert!(s.snapshot().unwrap().decisions[0]
        .extensions
        .contains_key(tool_proposals::PROVENANCE_KEY));
    assert_eq!(
        s.proposal_associations(&|_| None)[0]
            .1
            .as_ref()
            .unwrap()
            .task_status,
        tool_proposals::TaskAssociationStatus::Unlinked
    );
    let location = s.workspace_view().location.unwrap().into();
    send(&mut s, WorkspaceAction::CloseProject, None, 3);
    send(&mut s, WorkspaceAction::OpenProject { location }, None, 4);
    assert_eq!(s.snapshot().unwrap().decisions[0].decision_id, decision);
    assert!(s
        .import_proposal(bytes.as_slice(), None)
        .unwrap_err()
        .contains("重新导出"));
}
#[test]
fn imported_claims_never_bypass_native_conflict_and_explicit_replacement() {
    let (dir, mut s) = open_fixture();
    let i = input(&s, ScopeKind::AllExistingAndFuture);
    assert_eq!(adopt(&mut s, i, 0, 2), OperationOutcome::Committed);
    let first = s.snapshot().unwrap().decisions[0].decision_id.clone();
    let mut package = export_package(&mut s, dir.path(), None);
    package["envelope"]["imported_claims"] =
        serde_json::json!({"passed":true,"summary":"claimed success"});
    let bytes = serde_json::to_vec(&package).unwrap();
    s.import_proposal(bytes.as_slice(), None).unwrap();
    s.replay_imported(
        "candidate-1",
        "record-1",
        ScopeKind::AllExistingAndFuture,
        vec![],
        None,
    )
    .unwrap();
    assert!(s.save_imported(DecisionChoice::Adopt, None).is_err());
    assert_eq!(s.snapshot().unwrap().decisions.len(), 1);
    s.replay_imported(
        "candidate-1",
        "record-1",
        ScopeKind::AllExistingAndFuture,
        vec![first],
        None,
    )
    .unwrap();
    assert_eq!(
        s.save_imported(DecisionChoice::Adopt, None).unwrap(),
        OperationOutcome::Committed
    );
    assert_eq!(
        s.snapshot().unwrap().decisions[0].status,
        DecisionStatus::Superseded
    );
}
#[test]
fn proposal_retry_checks_current_source_and_stale_association_does_not_disable_local_work() {
    let (dir, mut s) = open_fixture();
    let task = tool_proposals::TaskIdentity {
        task_id: "task-one".into(),
        candidate_fingerprint: "a".repeat(64),
    };
    let package = export_package(&mut s, dir.path(), Some(task.clone()));
    let bytes = serde_json::to_vec(&package).unwrap();
    s.import_proposal(bytes.as_slice(), Some(&task)).unwrap();
    s.replay_imported(
        "candidate-1",
        "record-1",
        ScopeKind::AllExistingAndFuture,
        vec![],
        Some(&task),
    )
    .unwrap();
    s.fail_next_commit(tool_store::StoreFaultPoint::AfterPointerSwitch);
    assert!(matches!(
        s.save_imported(DecisionChoice::Adopt, Some(&task)).unwrap(),
        OperationOutcome::Failed(_)
    ));
    assert!(matches!(
        s.save_imported(DecisionChoice::Adopt, None).unwrap(),
        OperationOutcome::Failed(_)
    ));
    s.reopen_saved().unwrap();
    assert_eq!(s.snapshot().unwrap().decisions.len(), 1);
    assert_eq!(
        s.proposal_associations(&|_| None)[0]
            .1
            .as_ref()
            .unwrap()
            .task_status,
        tool_proposals::TaskAssociationStatus::Missing
    );
    edit_live(&mut s, "record-1", "关联任务缺失仍能工作", 1);
}
#[test]
fn cancel_back_and_project_switch_hide_old_evidence_without_writes() {
    let (_dir, mut s) = open_fixture();
    let before = s.snapshot().unwrap().clone();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(900.0, 1700.0));
    let mut ui = tool_workspace::ToolWorkspace::default();
    ui_click(&mut h, &mut ui, &mut s, "record.select.record-1");
    ui_click(&mut h, &mut ui, &mut s, "record.change");
    ui_type(&mut h, &mut ui, &mut s, "change.request", "保持这份要求");
    ui_type(&mut h, &mut ui, &mut s, "change.reason", "比较本地结果");
    ui_click(&mut h, &mut ui, &mut s, "change.rehearse");
    ui_click(&mut h, &mut ui, &mut s, "back");
    assert_eq!(s.snapshot().unwrap(), &before);
    let out = ui_frame(&mut h, &mut ui, &s);
    assert!(!out
        .controls
        .keys()
        .any(|k| k.starts_with("candidate.adopt")));
    ui_click(&mut h, &mut ui, &mut s, "project.close");
    assert!(s.snapshot().is_none());
    let out = ui_frame(&mut h, &mut ui, &s);
    assert!(out.controls.contains_key("entry.create"));
}
#[test]
fn independent_old_mode_files_remain_byte_identical() {
    let (dir, mut s) = open_fixture();
    let tasks = dir.path().join("tasks.json");
    let recent = dir.path().join("recent_repos.json");
    std::fs::write(&tasks, b"old task sentinel").unwrap();
    std::fs::write(&recent, b"old recent sentinel").unwrap();
    let i = input(&s, ScopeKind::SingleRecord);
    assert_eq!(adopt(&mut s, i, 1, 2), OperationOutcome::Committed);
    edit_live(&mut s, "record-1", "独立业务", 3);
    assert_eq!(std::fs::read(&tasks).unwrap(), b"old task sentinel");
    assert_eq!(std::fs::read(&recent).unwrap(), b"old recent sentinel");
}

#[test]
fn proposal_review_exposes_real_impacts_separately_from_synthetic_results() {
    let (dir, mut s) = open_fixture();
    let bytes = s
        .prepare_synthetic_export_for_test("record-1", ScopeKind::AllExistingAndFuture)
        .unwrap();
    s.write_export(&dir.path().join("synthetic.json")).unwrap();
    let mut package: serde_json::Value =
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["proposal_template"].clone();
    package["envelope"]["candidate_policies"][0]["due_date"] =
        serde_json::json!("extend_by_paused_days");
    let bytes = serde_json::to_vec(&package).unwrap();
    s.import_proposal(bytes.as_slice(), None).unwrap();
    s.replay_imported(
        "candidate-1",
        "record-1",
        ScopeKind::AllExistingAndFuture,
        vec![],
        None,
    )
    .unwrap();
    let lines = s.imported_review_lines().join("\n");
    assert!(lines.contains("实际记录"));
    assert!(lines.contains("2026-10-06") && lines.contains("2026-10-09"));
    assert!(lines.contains("未来默认"));
    assert!(lines.contains("副本"));
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("完成一项虚构工作"),
        "production synthetic export must not copy live description"
    );
}
#[test]
fn terminal_record_export_uses_future_example_for_incomplete_and_future_scope() {
    let (dir, mut s) = open_fixture();
    add_live(&mut s, "done", "completed", 2);
    let bytes = s
        .prepare_synthetic_export_for_test("done", ScopeKind::IncompleteAndFuture)
        .unwrap();
    s.write_export(&dir.path().join("terminal.json")).unwrap();
    let package =
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["proposal_template"].clone();
    let bytes = serde_json::to_vec(&package).unwrap();
    s.import_proposal(bytes.as_slice(), None).unwrap();
    s.replay_imported(
        "candidate-1",
        "done",
        ScopeKind::IncompleteAndFuture,
        vec![],
        None,
    )
    .unwrap();
    assert_eq!(
        s.save_imported(DecisionChoice::Adopt, None).unwrap(),
        OperationOutcome::Committed
    );
}
#[cfg(unix)]
#[test]
fn permission_denied_creation_can_recover_and_choose_another_location() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
    }
    let _restore = Restore(locked.clone());
    let mut s = studio();
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "保留输入".into(),
            location: locked.join("project").display().to_string(),
            spec: studio_order_template(),
        },
        None,
        1,
    );
    assert!(
        matches!(outcome, OperationOutcome::Failed(_)),
        "{outcome:?}"
    );
    s.recover_creation().unwrap();
    assert!(s.snapshot().is_none());
    assert!(!locked.join("project").exists());
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "保留输入".into(),
            location: dir.path().join("corrected").display().to_string(),
            spec: studio_order_template(),
        },
        None,
        2,
    );
    assert_eq!(outcome, OperationOutcome::PreviewReady);
    assert_eq!(s.snapshot().unwrap().project_name, "保留输入");
}
#[test]
fn create_recovery_opens_an_already_persisted_lost_ack_without_duplicate_project() {
    let dir = temp_dir();
    let mut s = studio();
    s.lose_next_create_ack();
    let path = dir.path().join("lost-create");
    assert!(matches!(
        send(
            &mut s,
            WorkspaceAction::CreateProject {
                name: "已落盘".into(),
                location: path.display().to_string(),
                spec: studio_order_template()
            },
            None,
            1
        ),
        OperationOutcome::Failed(_)
    ));
    s.recover_creation().unwrap();
    assert_eq!(s.snapshot().unwrap().operation_receipts.len(), 1);
    assert_eq!(s.snapshot().unwrap().project_name, "已落盘");
}
#[test]
fn injected_disk_full_never_marks_saved_and_exact_retry_preserves_data() {
    let (_dir, mut s) = open_fixture();
    let before = s.snapshot().unwrap().clone();
    let command = ToolCommand::EditRecord {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: 0,
        record_id: "record-1".into(),
        expected_record_revision: 2,
        changes: BTreeMap::from([(
            "notes".into(),
            Some(FieldValue::Text("磁盘恢复后保存".into())),
        )]),
        occurred_at: s.workspace_view().now,
    };
    let req = request(&s, WorkspaceAction::Record(command), Some("record-1"), 2);
    s.fail_next_commit_io(std::io::ErrorKind::StorageFull);
    assert!(matches!(s.handle(req.clone()), OperationOutcome::Failed(_)));
    assert_eq!(s.snapshot().unwrap(), &before);
    assert_eq!(s.handle(req), OperationOutcome::Committed);
    assert_eq!(s.snapshot().unwrap().records[0].record_revision, 3);
}

#[cfg(target_os = "linux")]
#[test]
fn actual_os_lock_contention_reports_failure_then_exact_retry_succeeds() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let (dir, mut s) = open_fixture();
    let lock = dir.path().join("project/.write.lock");
    let mut child = Command::new("flock")
        .arg("-x")
        .arg(&lock)
        .args(["sh", "-c", "printf 'locked\\n'; sleep 7"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("Linux CI provides util-linux flock");
    struct Reap(Option<std::process::Child>);
    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    reader.read_line(&mut ready).unwrap();
    assert_eq!(ready, "locked\n");
    let mut guard = Reap(Some(child));
    let before = s.snapshot().unwrap().clone();
    let command = ToolCommand::EditRecord {
        operation_id: s.workspace_view().next_operation_id.into(),
        expected_generation: 0,
        record_id: "record-1".into(),
        expected_record_revision: 2,
        changes: BTreeMap::from([(
            "notes".into(),
            Some(FieldValue::Text("锁释放后保存".into())),
        )]),
        occurred_at: s.workspace_view().now,
    };
    let req = request(&s, WorkspaceAction::Record(command), Some("record-1"), 2);
    assert!(
        matches!(s.handle(req.clone()),OperationOutcome::Failed(message) if message.contains("writer"))
    );
    assert_eq!(s.snapshot().unwrap(), &before);
    assert!(guard.0.as_mut().unwrap().wait().unwrap().success());
    guard.0.take();
    assert_eq!(s.handle(req), OperationOutcome::Committed);
}
#[test]
fn real_project_rejects_temporary_repository_and_unstable_locations() {
    let dir = temp_dir();
    let mut s = ToolStudio::new();
    let location = dir.path().join("temporary");
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "真实项目".into(),
            location: location.display().to_string(),
            spec: studio_order_template(),
        },
        None,
        1,
    );
    assert!(matches!(outcome,OperationOutcome::Rejected(message) if message.contains("临时")));
    assert!(!location.exists());
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git2::Repository::init(&repo).unwrap();
    let location = repo.join("business");
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "真实项目".into(),
            location: location.display().to_string(),
            spec: studio_order_template(),
        },
        None,
        2,
    );
    assert!(matches!(outcome,OperationOutcome::Rejected(message) if message.contains("Git")));
    assert!(!location.exists());
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "真实项目".into(),
            location: "relative-project".into(),
            spec: studio_order_template(),
        },
        None,
        3,
    );
    assert!(matches!(outcome, OperationOutcome::Rejected(_)));
    assert!(s.snapshot().is_none());
}

#[test]
fn future_what_if_can_be_deferred_without_saving_future_facts() {
    for choice in [
        DecisionChoice::Both,
        DecisionChoice::Neither,
        DecisionChoice::Defer,
    ] {
        let (_dir, mut s) = open_fixture();
        let before = s.snapshot().unwrap().records.clone();
        let mut i = input(&s, ScopeKind::SingleRecord);
        i.as_of_date = fixture::date(2026, 10, 8);
        assert_eq!(
            send(
                &mut s,
                WorkspaceAction::Rehearse { input: i.clone() },
                Some("record-1"),
                2
            ),
            OperationOutcome::PreviewReady
        );
        assert_eq!(
            send(
                &mut s,
                WorkspaceAction::SaveDecision {
                    choice,
                    input: i,
                    preview_id: None,
                    candidate_id: None
                },
                Some("record-1"),
                2
            ),
            OperationOutcome::Committed
        );
        let snapshot = s.snapshot().unwrap();
        assert_eq!(snapshot.decisions[0].status, DecisionStatus::Pending);
        assert_eq!(
            snapshot.decisions[0].created_at,
            fixture::at(2026, 10, 5, 12)
        );
        assert_eq!(snapshot.records, before);
        assert!(snapshot.scenarios.is_empty() && snapshot.evidence.is_empty());
    }
}
#[cfg(unix)]
#[test]
fn symbolic_parent_cannot_redirect_real_project_creation() {
    let dir = temp_dir();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let alias = dir.path().join("alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    let mut s = studio();
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "不跟随链接".into(),
            location: alias.join("project").display().to_string(),
            spec: studio_order_template(),
        },
        None,
        1,
    );
    assert!(
        matches!(outcome, OperationOutcome::Rejected(_)),
        "{outcome:?}"
    );
    assert!(!target.join("project").exists());
}

#[test]
fn configured_enum_field_uses_actual_widgets_and_round_trips_selected_value() {
    let (_dir, mut s) = open_fixture();
    let mut h = egui_harness::EguiHarness::new(egui::vec2(900.0, 2000.0));
    let mut ui = tool_workspace::ToolWorkspace::default();
    ui_click(&mut h, &mut ui, &mut s, "project.configure");
    ui_click(&mut h, &mut ui, &mut s, "spec.expand");
    ui_type(&mut h, &mut ui, &mut s, "spec.optional.name", "包装选项");
    ui_click(&mut h, &mut ui, &mut s, "spec.optional.kind.4");
    ui_type(
        &mut h,
        &mut ui,
        &mut s,
        "spec.optional.choices",
        "small\nlarge",
    );
    ui_click(&mut h, &mut ui, &mut s, "spec.optional.add");
    ui_click(&mut h, &mut ui, &mut s, "project.submit");
    assert_eq!(
        s.snapshot()
            .unwrap()
            .active_spec()
            .unwrap()
            .field("custom-1")
            .unwrap()
            .kind,
        FieldKind::Enum {
            options: vec!["small".into(), "large".into()]
        }
    );
    ui_click(&mut h, &mut ui, &mut s, "back");
    ui_click(&mut h, &mut ui, &mut s, "record.new");
    ui_type(&mut h, &mut ui, &mut s, "field.order_number", "enum-record");
    ui_type(
        &mut h,
        &mut ui,
        &mut s,
        "field.work_description",
        "new schema",
    );
    ui_click(&mut h, &mut ui, &mut s, "present.custom-1");
    ui_click(&mut h, &mut ui, &mut s, "enum.custom-1.1");
    ui_click(&mut h, &mut ui, &mut s, "record.save");
    let location = s.workspace_view().location.unwrap().to_owned();
    ui_click(&mut h, &mut ui, &mut s, "project.close");
    send(&mut s, WorkspaceAction::OpenProject { location }, None, 1);
    assert_eq!(
        s.snapshot().unwrap().records[1].typed_values["custom-1"],
        FieldValue::Enum("large".into())
    );
    assert_eq!(s.snapshot().unwrap().records[0].spec.revision, 1);
    assert_eq!(s.snapshot().unwrap().records[1].spec.revision, 2);
}

#[test]
fn extending_single_record_timer_to_future_records_changes_future_default() {
    let (_dir, mut s) = open_fixture();
    let mut first = input(&s, ScopeKind::SingleRecord);
    first.rule_keys.push(RuleKey::Timer);
    for candidate in &mut first.candidates {
        candidate.timer = TimerPolicy::CountPausedStages;
    }
    assert_eq!(adopt(&mut s, first, 0, 2), OperationOutcome::Committed);
    assert_eq!(
        tool_decisions::policy_for_record(s.snapshot().unwrap(), "record-1")
            .unwrap()
            .timer,
        TimerPolicy::CountPausedStages
    );
    assert_eq!(
        tool_decisions::policy_for_future(s.snapshot().unwrap())
            .unwrap()
            .timer,
        TimerPolicy::PauseStagesMarkedPaused
    );
    let mut next = input(&s, ScopeKind::FutureRecords);
    next.rule_keys.push(RuleKey::Timer);
    assert_eq!(adopt(&mut s, next, 0, 3), OperationOutcome::Committed);
    assert_eq!(
        tool_decisions::policy_for_future(s.snapshot().unwrap())
            .unwrap()
            .timer,
        TimerPolicy::CountPausedStages
    );
}

#[test]
fn explicit_scope_expansion_preserves_unselected_dimensions() {
    for rule in [RuleKey::Timer, RuleKey::Reminder] {
        let (_dir, mut s) = open_fixture();
        let initial = tool_decisions::policy_for_future(s.snapshot().unwrap()).unwrap();
        let mut first = input(&s, ScopeKind::SingleRecord);
        first.rule_keys = vec![RuleKey::DeliveryTarget, RuleKey::Timer, RuleKey::Reminder];
        for candidate in &mut first.candidates {
            candidate.timer = TimerPolicy::CountPausedStages;
            candidate.reminder = ReminderPolicy::WaitingAfterDays { days_waiting: 2 };
        }
        assert_eq!(adopt(&mut s, first, 0, 2), OperationOutcome::Committed);
        let mut next = input(&s, ScopeKind::FutureRecords);
        next.rule_keys = vec![RuleKey::DeliveryTarget, rule];
        assert_eq!(adopt(&mut s, next, 0, 3), OperationOutcome::Committed);
        let future = tool_decisions::policy_for_future(s.snapshot().unwrap()).unwrap();
        assert_eq!(
            future.timer,
            if rule == RuleKey::Timer {
                TimerPolicy::CountPausedStages
            } else {
                initial.timer
            }
        );
        assert_eq!(
            future.reminder,
            if rule == RuleKey::Reminder {
                ReminderPolicy::WaitingAfterDays { days_waiting: 2 }
            } else {
                initial.reminder
            }
        );
    }
}
#[test]
fn dimension_toggle_cannot_reuse_earlier_preview_even_with_same_policy_values() {
    let (_dir, mut s) = open_fixture();
    let mut i = input(&s, ScopeKind::FutureRecords);
    send(
        &mut s,
        WorkspaceAction::Rehearse { input: i.clone() },
        Some("record-1"),
        2,
    );
    let p = s.workspace_view().rehearsal.unwrap().clone();
    i.rule_keys.push(RuleKey::Timer);
    assert!(matches!(
        send(
            &mut s,
            WorkspaceAction::SaveDecision {
                choice: DecisionChoice::Adopt,
                input: i,
                preview_id: Some(p.preview_id),
                candidate_id: Some(p.candidates[0].candidate_id.clone())
            },
            Some("record-1"),
            2
        ),
        OperationOutcome::Rejected(_)
    ));
    assert!(s.snapshot().unwrap().decisions.is_empty());
}

#[cfg(unix)]
#[test]
fn unreadable_git_metadata_is_not_proof_that_location_is_outside_a_repository() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    drop(git2::Repository::init(&repo).unwrap());
    let metadata = repo.join(".git");
    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o000)).unwrap();
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
    }
    let _restore = Restore(metadata);
    assert!(git2::Repository::discover(&repo).is_err());
    let mut s = studio();
    let location = repo.join("business");
    let outcome = send(
        &mut s,
        WorkspaceAction::CreateProject {
            name: "不得进入仓库".into(),
            location: location.display().to_string(),
            spec: studio_order_template(),
        },
        None,
        1,
    );
    assert!(
        matches!(outcome, OperationOutcome::Rejected(_)),
        "{outcome:?}"
    );
    assert!(!location.exists());
}
