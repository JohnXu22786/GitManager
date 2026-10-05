#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "fixtures/tool_workspace_fixture.rs"]
mod fixture;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/ui/tool_workspace.rs"]
mod tool_workspace;
#[path = "../src/tool_workspace_protocol.rs"]
mod tool_workspace_protocol;

use egui::{Key, Modifiers, Vec2};
use egui_harness::EguiHarness;
use tool_project::*;
use tool_workspace::{ToolWorkspace, WorkspaceOutput};
use tool_workspace_protocol::*;

fn frame(
    h: &mut EguiHarness,
    state: &mut ToolWorkspace,
    view: &WorkspaceView<'_>,
) -> WorkspaceOutput {
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| state.show(ui, view))
            .inner
    })
}
fn click(
    h: &mut EguiHarness,
    state: &mut ToolWorkspace,
    view: &WorkspaceView<'_>,
    key: &str,
) -> WorkspaceOutput {
    let initial = frame(h, state, view);
    let rect = initial
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {:?}", initial.controls.keys()))
        .rect;
    h.press_at(rect.center());
    frame(h, state, view);
    h.release_at(rect.center());
    frame(h, state, view)
}
fn type_in(
    h: &mut EguiHarness,
    state: &mut ToolWorkspace,
    view: &WorkspaceView<'_>,
    key: &str,
    text: &str,
) {
    click(h, state, view, key);
    h.key(Key::A, true, Modifiers::COMMAND);
    frame(h, state, view);
    h.key(Key::A, false, Modifiers::NONE);
    h.text(text);
    frame(h, state, view);
}

#[test]
fn create_cancel_and_reopen_preserves_input_without_creating_example_records() {
    let mut h = EguiHarness::new(Vec2::new(800.0, 1000.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::entry_view();
    click(&mut h, &mut state, &view, "entry.create");
    type_in(&mut h, &mut state, &view, "project.name", "我的手作订单");
    type_in(
        &mut h,
        &mut state,
        &view,
        "project.location",
        "/home/me/orders",
    );
    assert!(click(&mut h, &mut state, &view, "back").request.is_none());
    click(&mut h, &mut state, &view, "entry.create");
    let result = click(&mut h, &mut state, &view, "project.submit");
    let req = result.request.unwrap();
    assert!(
        matches!(req.action, WorkspaceAction::CreateProject { ref name, ref location, .. } if name == "我的手作订单" && location == "/home/me/orders")
    );
    assert!(click(&mut h, &mut state, &view, "project.submit")
        .request
        .is_none());
    assert!(!frame(&mut h, &mut state, &view)
        .text
        .iter()
        .any(|s| s == "已保存"));
}

#[test]
fn record_validation_keyboard_input_and_failed_retry_keep_exact_command() {
    let model = fixture::model();
    let mut view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(800.0, 1300.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    assert!(click(&mut h, &mut state, &view, "record.save")
        .request
        .is_none());
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.order_number",
        "中文长标题 🌱",
    );
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.work_description",
        "制作木盒",
    );
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(&req.action, WorkspaceAction::Record(ToolCommand::CreateRecord { values, expected_generation: 4, .. }) if values.get("order_number") == Some(&FieldValue::Text("中文长标题 🌱".into())))
    );
    assert!(click(&mut h, &mut state, &view, "record.save")
        .request
        .is_none());
    let reply = OperationStatus {
        delivery_id: 1,
        context: req.context.clone(),
        outcome: OperationOutcome::Failed("磁盘已满".into()),
    };
    view.operation = Some(&reply);
    let retry = click(&mut h, &mut state, &view, "operation.retry")
        .request
        .unwrap();
    assert_eq!(req, retry);
}

#[test]
fn record_switch_rejects_stale_completion_and_preserves_dirty_draft() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(800.0, 1300.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.order_number",
        "尚未保存的标题",
    );
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "record.select.order-2");
    let page = frame(&mut h, &mut state, &view);
    assert!(page.text.iter().any(|s| s.contains("第二笔")));
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(&req.action, WorkspaceAction::Record(ToolCommand::EditRecord { changes, .. }) if changes.get("order_number") == Some(&Some(FieldValue::Text("尚未保存的标题".into()))))
    );
}

#[test]
fn read_only_and_generation_changes_block_unsaved_record_overwrite() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(800.0, 1300.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(&mut h, &mut state, &view, "field.order_number", "保留输入");
    model.generation += 1;
    let view = fixture::project_view(&model);
    let result = click(&mut h, &mut state, &view, "record.save");
    assert!(result.request.is_none());
    assert!(result.text.iter().any(|s| s.contains("项目已有变化")));
    model.access = ProjectAccess::ReadOnly;
    let view = fixture::project_view(&model);
    assert!(!frame(&mut h, &mut state, &view).controls["record.save"].enabled);
}

#[test]
fn narrow_window_long_chinese_and_escape_keep_back_reachable() {
    let mut model = fixture::model();
    model.project_name = "这是一个很长的中文工作室名称".repeat(25);
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(280.0, 360.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    let output = frame(&mut h, &mut state, &view);
    let back = output.controls["back"].rect;
    assert!(
        back.left() >= 0.0 && back.right() <= 280.0 && back.top() >= 0.0 && back.bottom() <= 360.0
    );
    h.key(Key::Escape, true, Modifiers::NONE);
    let result = frame(&mut h, &mut state, &view);
    assert!(result.request.is_none());
    h.key(Key::Escape, false, Modifiers::NONE);
    assert!(frame(&mut h, &mut state, &view)
        .controls
        .contains_key("record.new"));
}

#[test]
fn invalid_dates_and_optional_missing_values_are_not_silently_coerced() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(800.0, 1400.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    type_in(&mut h, &mut state, &view, "field.order_number", "测试");
    type_in(&mut h, &mut state, &view, "field.work_description", "制作");
    click(&mut h, &mut state, &view, "present.promised_on");
    type_in(&mut h, &mut state, &view, "field.promised_on", "2026-02-30");
    assert!(click(&mut h, &mut state, &view, "record.save")
        .request
        .is_none());
    type_in(&mut h, &mut state, &view, "field.promised_on", "2026-10-09");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(&req.action, WorkspaceAction::Record(ToolCommand::CreateRecord { values, .. }) if values.get("promised_on") == Some(&FieldValue::Date(fixture::date(9))) && !values.contains_key("notes"))
    );
}

#[test]
fn rule_choices_emit_typed_scope_and_unresolved_intent_without_activation() {
    for (scope_key, scope) in [
        ("scope.single", ScopeKind::SingleRecord),
        ("scope.future", ScopeKind::FutureRecords),
        ("scope.incomplete", ScopeKind::IncompleteAndFuture),
        ("scope.all", ScopeKind::AllExistingAndFuture),
    ] {
        for (choice_key, choice) in [
            ("decision.both", DecisionChoice::Both),
            ("decision.neither", DecisionChoice::Neither),
            ("decision.defer", DecisionChoice::Defer),
        ] {
            let model = fixture::model();
            let view = fixture::project_view(&model);
            let mut h = EguiHarness::new(Vec2::new(900.0, 1800.0));
            let mut state = ToolWorkspace::default();
            click(&mut h, &mut state, &view, "record.select.order-1");
            begin_change(&mut h, &mut state, &view);
            type_in(
                &mut h,
                &mut state,
                &view,
                "change.request",
                "等材料时怎么计算才合适？",
            );
            click(&mut h, &mut state, &view, scope_key);
            let req = click(&mut h, &mut state, &view, choice_key)
                .request
                .unwrap();
            assert!(
                matches!(req.action, WorkspaceAction::SaveDecision { choice: actual, ref input, preview_id: None, .. } if actual == choice && input.scope == scope && input.original_request == "等材料时怎么计算才合适？")
            );
        }
    }
}

#[test]
fn supplied_rehearsal_results_require_current_context_and_local_evidence() {
    let model = fixture::model();
    let mut view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    let rehearsal = fixture::rehearsal(req.context.clone());
    let reply = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::PreviewReady,
    };
    view.operation = Some(&reply);
    view.rehearsal = Some(&rehearsal);
    view.next_operation_id = "operation-2";
    let output = frame(&mut h, &mut state, &view);
    assert!(output.text.iter().any(|s| s.contains("2026-10-09")));
    assert!(output.controls["candidate.adopt.a"].enabled);
    let mut stale_rehearsal = rehearsal.clone();
    stale_rehearsal.fresh = false;
    view.rehearsal = Some(&stale_rehearsal);
    assert!(!frame(&mut h, &mut state, &view).controls["candidate.adopt.a"].enabled);
    let mut rehearsal = rehearsal.clone();
    rehearsal.candidates[0].evidence[0].source = EvidenceSource::ImportedUntrusted;
    view.rehearsal = Some(&rehearsal);
    assert!(!frame(&mut h, &mut state, &view).controls["candidate.adopt.a"].enabled);
    type_in(&mut h, &mut state, &view, "change.date", "2026-10-06");
    assert!(!frame(&mut h, &mut state, &view).controls["candidate.adopt.a"].enabled);
}

#[test]
fn project_switch_ignores_old_operation_success_and_result() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 1800.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    let result = fixture::rehearsal(req.context.clone());
    let status = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Committed,
    };
    let mut other = fixture::model();
    other.project_id = "project-2".into();
    let mut switched = fixture::project_view(&other);
    switched.session_id = "session-2";
    switched.operation = Some(&status);
    switched.rehearsal = Some(&result);
    let output = frame(&mut h, &mut state, &switched);
    assert!(!output.text.iter().any(|s| s == "已保存"));
    assert!(!output.controls.contains_key("candidate.adopt.a"));
}

#[test]
fn editing_optional_boolean_preserves_true_until_user_changes_it() {
    let mut model = fixture::model();
    model.active_spec.fields.push(FieldDefinition {
        id: "urgent".into(),
        display_name: "加急".into(),
        kind: FieldKind::Boolean,
        role: FieldRole::Custom,
        required: false,
        extensions: Default::default(),
    });
    model.records[0]
        .values
        .insert("urgent".into(), FieldValue::Boolean(true));
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(800.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(&mut h, &mut state, &view, "field.order_number", "修改标题");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Record(ToolCommand::EditRecord { changes,.. }) if !changes.contains_key("urgent"))
    );
}

#[test]
fn save_acknowledgement_for_previous_record_does_not_label_new_record_saved() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(800.0, 1500.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(&mut h, &mut state, &view, "field.order_number", "标题一");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "record.select.order-2");
    model.generation += 1;
    let ack = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Committed,
    };
    let mut view = fixture::project_view(&model);
    view.operation = Some(&ack);
    let output = frame(&mut h, &mut state, &view);
    assert!(output.text.iter().any(|s| s.contains("第二笔")));
    assert!(!output.text.iter().any(|s| s == "已保存"));
}

#[test]
fn withdrawal_preview_keeps_later_data_visible_and_blocks_conflicts() {
    let mut model = fixture::model();
    model.decisions.push(fixture::decision());
    let mut view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "decisions.open");
    let req = click(&mut h, &mut state, &view, "decision.undo.decision-1")
        .request
        .unwrap();
    let preview = WithdrawalView {
        context: req.context.clone(),
        preview_id: "undo-1".into(),
        decision_id: "decision-1".into(),
        fresh: true,
        ready_to_commit: true,
        comparisons: vec![],
        preserved_record_count: 34,
        preserved_event_count: 105,
        retained_bindings: vec![RetainedBindingView {
            record_id: None,
            rule_key: RuleKey::Reminder,
            behavior_revision_id: "later-reminder".into(),
        }],
        conflicts: vec![],
    };
    let ack = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::PreviewReady,
    };
    view.withdrawal = Some(&preview);
    view.operation = Some(&ack);
    view.next_operation_id = "operation-2";
    let output = frame(&mut h, &mut state, &view);
    assert!(output
        .text
        .iter()
        .any(|s| s.contains("34") && s.contains("105")));
    assert!(output.text.iter().any(|s| s.contains("later-reminder")));
    let mut conflict = preview.clone();
    conflict.conflicts.push(ConflictView {
        decision_id: "other".into(),
        message: "后续决定依赖这条规则".into(),
        expected: vec![],
        actual: vec![],
    });
    view.withdrawal = Some(&conflict);
    assert!(click(&mut h, &mut state, &view, "withdrawal.confirm")
        .request
        .is_none());
    view.withdrawal = Some(&preview);
    let req = click(&mut h, &mut state, &view, "withdrawal.confirm")
        .request
        .unwrap();
    assert_eq!(
        req.action,
        WorkspaceAction::ConfirmWithdrawal {
            preview_id: "undo-1".into()
        }
    );
    assert!(click(&mut h, &mut state, &view, "withdrawal.confirm")
        .request
        .is_none());
}

#[test]
fn filtering_and_status_actions_use_supplied_spec_and_current_revision() {
    let mut model = fixture::model();
    model.records[1].current_stage_id = "completed".into();
    model.records[1].derived.current_stage_id = "completed".into();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "filter.incomplete");
    assert!(!frame(&mut h, &mut state, &view)
        .controls
        .contains_key("record.select.order-2"));
    click(&mut h, &mut state, &view, "record.select.order-1");
    let req = click(&mut h, &mut state, &view, "stage.in_progress")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Record(ToolCommand::TransitionStage { record_id,expected_generation: 4,expected_record_revision: 3,to_stage_id,.. }) if record_id == "order-1" && to_stage_id == "in_progress")
    );
}

#[test]
fn discarded_layout_pass_does_not_lose_the_emitted_request() {
    let view = fixture::entry_view();
    let mut h = EguiHarness::new(Vec2::new(850.0, 1000.0));
    let mut state = ToolWorkspace::default();
    let position = frame(&mut h, &mut state, &view).controls["entry.example"]
        .rect
        .center();
    h.press_at(position);
    frame(&mut h, &mut state, &view);
    h.release_at(position);
    let mut first_pass = true;
    let output = h.frame(|ctx| {
        let output = egui::CentralPanel::default()
            .show(ctx, |ui| state.show(ui, &view))
            .inner;
        if first_pass {
            first_pass = false;
            ctx.request_discard("exercise real workspace output across layout passes");
        }
        output
    });
    assert!(matches!(
        output.request.map(|r| r.action),
        Some(WorkspaceAction::StartExample)
    ));
    assert!(frame(&mut h, &mut state, &view).request.is_none());
}

#[test]
fn returning_to_a_project_preserves_its_dirty_record_input() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.order_number",
        "切换前保留的输入",
    );
    let mut other = fixture::model();
    other.project_id = "other".into();
    let mut other_view = fixture::project_view(&other);
    other_view.session_id = "session-2";
    frame(&mut h, &mut state, &other_view);
    let mut returned = fixture::project_view(&model);
    returned.session_id = "session-3";
    click(&mut h, &mut state, &returned, "record.select.order-1");
    click(&mut h, &mut state, &returned, "record.edit");
    let req = click(&mut h, &mut state, &returned, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Record(ToolCommand::EditRecord { changes,.. }) if changes.get("order_number") == Some(&Some(FieldValue::Text("切换前保留的输入".into()))))
    );
}

#[test]
fn rejected_save_can_be_corrected_without_losing_input() {
    let model = fixture::model();
    let mut view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    type_in(&mut h, &mut state, &view, "field.order_number", "原输入");
    type_in(&mut h, &mut state, &view, "field.work_description", "内容");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    let status = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Rejected("字段不兼容".into()),
    };
    view.operation = Some(&status);
    view.next_operation_id = "operation-2";
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.work_description",
        "修正内容",
    );
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Record(ToolCommand::CreateRecord { values,.. }) if values.get("order_number") == Some(&FieldValue::Text("原输入".into())) && values.get("work_description") == Some(&FieldValue::Text("修正内容".into())))
    );
}

#[test]
fn configuration_edits_preserve_stable_ids_and_add_only_optional_fields() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "project.configure");
    type_in(&mut h, &mut state, &view, "project.name", "新的工具名");
    click(&mut h, &mut state, &view, "spec.expand");
    for _ in 0..20 {
        frame(&mut h, &mut state, &view);
    }
    type_in(
        &mut h,
        &mut state,
        &view,
        "spec.field.order_number",
        "工作编号",
    );
    type_in(&mut h, &mut state, &view, "spec.optional.name", "材质");
    click(&mut h, &mut state, &view, "spec.optional.add");
    let req = click(&mut h, &mut state, &view, "project.submit")
        .request
        .unwrap();
    let WorkspaceAction::Configure { name, spec } = req.action else {
        panic!("expected configuration request")
    };
    assert_eq!(name, "新的工具名");
    assert_eq!(spec.revision, 2);
    assert_eq!(spec.field("order_number").unwrap().display_name, "工作编号");
    assert_eq!(spec.field("order_number").unwrap().kind, FieldKind::Text);
    assert!(!spec.field("custom-1").unwrap().required);
    assert_eq!(spec.field("custom-1").unwrap().display_name, "材质");
    assert!(!frame(&mut h, &mut state, &view)
        .text
        .iter()
        .any(|s| s == "已保存"));
}

#[test]
fn stale_record_can_be_explicitly_discarded_and_reloaded() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(900.0, 1500.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(&mut h, &mut state, &view, "field.order_number", "旧草稿");
    model.generation = 5;
    model.records[0].record_revision = 4;
    model.records[0]
        .values
        .insert("order_number".into(), FieldValue::Text("别人改过".into()));
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.reload");
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.work_description",
        "核对后的新内容",
    );
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Record(ToolCommand::EditRecord { changes,expected_generation: 5,expected_record_revision: 4,.. }) if !changes.contains_key("order_number"))
    );
}

#[test]
fn settings_remain_editable_after_ack_and_survive_reopening() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "project.configure");
    type_in(&mut h, &mut state, &view, "project.name", "已提交名称");
    let req = click(&mut h, &mut state, &view, "project.submit")
        .request
        .unwrap();
    model.generation += 1;
    model.project_name = "已提交名称".into();
    model.active_spec.revision += 1;
    let status = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Committed,
    };
    let mut view = fixture::project_view(&model);
    view.operation = Some(&status);
    view.next_operation_id = "operation-2";
    type_in(&mut h, &mut state, &view, "project.name", "尚未提交名称");
    let output = frame(&mut h, &mut state, &view);
    assert!(output.controls["project.submit"].enabled);
    assert!(!output.text.iter().any(|s| s == "已保存"));
    let mut closed = fixture::entry_view();
    closed.session_id = "closed";
    frame(&mut h, &mut state, &closed);
    let mut reopened = fixture::project_view(&model);
    reopened.session_id = "reopened";
    reopened.next_operation_id = "operation-3";
    click(&mut h, &mut state, &reopened, "project.configure");
    let req = click(&mut h, &mut state, &reopened, "project.submit")
        .request
        .unwrap();
    assert!(matches!(req.action,WorkspaceAction::Configure { name,.. } if name == "尚未提交名称"));
}

#[test]
fn repeated_identical_failed_deliveries_allow_exact_retry_each_time() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(850.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    type_in(&mut h, &mut state, &view, "field.order_number", "测试");
    type_in(&mut h, &mut state, &view, "field.work_description", "内容");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    for delivery_id in 1..=3 {
        let status = OperationStatus {
            delivery_id,
            context: req.context.clone(),
            outcome: OperationOutcome::Failed("仍然没有可用空间".into()),
        };
        let mut retry_view = fixture::project_view(&model);
        retry_view.operation = Some(&status);
        let retry = click(&mut h, &mut state, &retry_view, "operation.retry")
            .request
            .unwrap();
        assert_eq!(retry, req);
    }
}

#[test]
fn rehearsal_carries_full_decision_metadata_and_effective_record_policy() {
    let mut model = fixture::model();
    model.decisions.push(fixture::decision());
    let policies = std::collections::BTreeMap::from([(
        "order-1".into(),
        BehaviorPolicy {
            reminder: ReminderPolicy::WaitingAfterDays { days_waiting: 7 },
            ..BehaviorPolicy::default()
        },
    )]);
    let mut view = fixture::project_view(&model);
    view.record_policies = &policies;
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.reason",
        "改变原来判断的理由",
    );
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.unresolved",
        "还需要看看其他订单",
    );
    click(&mut h, &mut state, &view, "change.supersedes.expand");
    for _ in 0..20 {
        frame(&mut h, &mut state, &view);
    }
    click(&mut h, &mut state, &view, "change.supersedes.decision-1");
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    let WorkspaceAction::Rehearse { input } = req.action else {
        panic!("expected rehearsal")
    };
    assert_eq!(input.rationale, "改变原来判断的理由");
    assert_eq!(input.unresolved_questions, vec!["还需要看看其他订单"]);
    assert_eq!(input.supersedes, vec!["decision-1"]);
    assert!(input
        .candidates
        .iter()
        .all(|p| p.reminder == ReminderPolicy::WaitingAfterDays { days_waiting: 7 }));
}

#[test]
fn a_rule_draft_cannot_reapply_outdated_policy_after_project_changes() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    type_in(&mut h, &mut state, &view, "change.request", "保留这句话");
    model.generation += 1;
    let view = fixture::project_view(&model);
    assert!(click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .is_none());
    click(&mut h, &mut state, &view, "change.reload");
    fill_change_text(&mut h, &mut state, &view);
    assert!(click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .is_some());
}

#[test]
fn stage_save_preserves_an_unsaved_field_draft() {
    let mut model = fixture::model();
    let mut h = EguiHarness::new(Vec2::new(900.0, 1500.0));
    let mut state = ToolWorkspace::default();
    let view = fixture::project_view(&model);
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.edit");
    type_in(
        &mut h,
        &mut state,
        &view,
        "field.order_number",
        "还没有保存的标题",
    );
    click(&mut h, &mut state, &view, "back");
    let req = click(&mut h, &mut state, &view, "stage.in_progress")
        .request
        .unwrap();
    model.generation += 1;
    model.records[0].record_revision += 1;
    model.records[0].current_stage_id = "in_progress".into();
    let status = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Committed,
    };
    let mut view = fixture::project_view(&model);
    view.operation = Some(&status);
    view.next_operation_id = "operation-2";
    click(&mut h, &mut state, &view, "record.edit");
    let output = frame(&mut h, &mut state, &view);
    assert!(output.text.iter().any(|s| s == "未保存"));
    assert!(output.text.iter().any(|s| s.contains("项目已有变化")));
}

#[test]
fn pending_history_shows_original_request_and_can_be_explicitly_resolved() {
    let mut model = fixture::model();
    let mut decision = fixture::decision();
    decision.choice = DecisionChoice::Defer;
    decision.status = DecisionStatus::Pending;
    decision.rationale.clear();
    model.decisions.push(decision);
    let requests = std::collections::BTreeMap::from([(
        "decision-1".into(),
        "等待多久才应该调整交付安排？".into(),
    )]);
    let mut view = fixture::project_view(&model);
    view.decision_requests = &requests;
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "decisions.open");
    assert!(frame(&mut h, &mut state, &view)
        .text
        .iter()
        .any(|s| s.contains("等待多久才应该调整交付安排？")));
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    click(&mut h, &mut state, &view, "change.supersedes.expand");
    for _ in 0..20 {
        frame(&mut h, &mut state, &view);
    }
    click(&mut h, &mut state, &view, "change.supersedes.decision-1");
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::Rehearse { input } if input.supersedes == vec!["decision-1"])
    );
}

#[test]
fn opening_another_path_does_not_replace_the_unsubmitted_creation_location() {
    let view = fixture::entry_view();
    let mut h = EguiHarness::new(Vec2::new(900.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "entry.create");
    type_in(
        &mut h,
        &mut state,
        &view,
        "project.location",
        "/home/me/new-tool",
    );
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "entry.open");
    type_in(
        &mut h,
        &mut state,
        &view,
        "project.location",
        "/home/me/existing-tool",
    );
    click(&mut h, &mut state, &view, "back");
    click(&mut h, &mut state, &view, "entry.create");
    let req = click(&mut h, &mut state, &view, "project.submit")
        .request
        .unwrap();
    assert!(
        matches!(req.action,WorkspaceAction::CreateProject { location,.. } if location == "/home/me/new-tool")
    );
}

#[test]
fn rule_requests_require_the_text_and_reason_expected_by_the_decision_layer() {
    let model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    click(&mut h, &mut state, &view, "record.change");
    assert!(click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .is_none());
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.request",
        "想比较等待时的交付安排",
    );
    assert!(click(&mut h, &mut state, &view, "decision.defer")
        .request
        .is_none());
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.reason",
        "还不能判断哪一种合适",
    );
    assert!(click(&mut h, &mut state, &view, "decision.defer")
        .request
        .is_some());
}

fn fill_change_text(h: &mut EguiHarness, state: &mut ToolWorkspace, view: &WorkspaceView<'_>) {
    type_in(
        h,
        state,
        view,
        "change.request",
        "想比较等待时的两种日期安排",
    );
    type_in(
        h,
        state,
        view,
        "change.reason",
        "需要选择符合实际做法的结果",
    );
}
fn begin_change(h: &mut EguiHarness, state: &mut ToolWorkspace, view: &WorkspaceView<'_>) {
    click(h, state, view, "record.change");
    fill_change_text(h, state, view);
}

#[test]
fn read_only_rehearsal_retry_is_allowed_but_record_write_retry_is_not() {
    let mut model = fixture::model();
    model.access = ProjectAccess::ReadOnly;
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    let failure = OperationStatus {
        delivery_id: 1,
        context: req.context.clone(),
        outcome: OperationOutcome::Failed("演练暂时未完成".into()),
    };
    let mut view = fixture::project_view(&model);
    view.operation = Some(&failure);
    assert_eq!(
        click(&mut h, &mut state, &view, "operation.retry").request,
        Some(req)
    );

    let mut model = fixture::model();
    let view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 1500.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.new");
    type_in(&mut h, &mut state, &view, "field.order_number", "真实记录");
    type_in(&mut h, &mut state, &view, "field.work_description", "内容");
    let req = click(&mut h, &mut state, &view, "record.save")
        .request
        .unwrap();
    model.access = ProjectAccess::ReadOnly;
    let failure = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Failed("保存结果尚未确认".into()),
    };
    let mut view = fixture::project_view(&model);
    view.operation = Some(&failure);
    assert!(click(&mut h, &mut state, &view, "operation.retry")
        .request
        .is_none());
}

#[test]
fn back_cancels_only_the_local_rehearsal_and_ignores_its_late_reply() {
    let model = fixture::model();
    let mut view = fixture::project_view(&model);
    let mut h = EguiHarness::new(Vec2::new(900.0, 2100.0));
    let mut state = ToolWorkspace::default();
    click(&mut h, &mut state, &view, "record.select.order-1");
    begin_change(&mut h, &mut state, &view);
    let req = click(&mut h, &mut state, &view, "change.rehearse")
        .request
        .unwrap();
    click(&mut h, &mut state, &view, "back");
    assert!(frame(&mut h, &mut state, &view).controls["project.close"].enabled);
    let late = OperationStatus {
        delivery_id: 1,
        context: req.context,
        outcome: OperationOutcome::Failed("旧演练的结果".into()),
    };
    view.operation = Some(&late);
    view.next_operation_id = "operation-2";
    assert!(!frame(&mut h, &mut state, &view)
        .text
        .iter()
        .any(|s| s.contains("旧演练的结果")));
}
