//! Repository-independent novice workspace. Rendering emits one typed request;
//! the controller alone opens files, executes rules and confirms persistence.

use crate::tool_project::*;
use crate::tool_workspace_protocol::*;
use chrono::NaiveDate;
use egui::{Button, Color32, Response, ScrollArea, TextEdit, Ui};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct WorkspaceOutput {
    pub request: Option<WorkspaceRequest>,
    #[cfg(test)]
    pub controls: BTreeMap<String, ControlGeometry>,
    #[cfg(test)]
    pub text: Vec<String>,
}
#[cfg(test)]
pub struct ControlGeometry {
    pub rect: egui::Rect,
    pub enabled: bool,
}
impl WorkspaceOutput {
    fn control(&mut self, key: &str, response: Response) -> Response {
        #[cfg(test)]
        self.controls.insert(
            key.into(),
            ControlGeometry {
                rect: response.rect,
                enabled: response.enabled(),
            },
        );
        #[cfg(not(test))]
        let _ = key;
        response
    }
    fn label(&mut self, ui: &mut Ui, text: impl Into<String>) {
        let text = text.into();
        #[cfg(test)]
        self.text.push(text.clone());
        ui.add(egui::Label::new(text).wrap());
    }
    fn button(&mut self, ui: &mut Ui, key: &str, label: &str, enabled: bool) -> bool {
        self.control(key, ui.add_enabled(enabled, Button::new(label).wrap()))
            .clicked()
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Home,
    Create,
    Open,
    Records,
    Detail,
    Edit,
    Configure,
    Change,
    Decisions,
    Withdrawal,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RecordFilter {
    #[default]
    All,
    Incomplete,
    Waiting,
    Due,
}
#[derive(Clone)]
struct FieldInput {
    present: bool,
    text: String,
}
#[derive(Clone)]
struct RecordDraft {
    generation: u64,
    revision: Option<u64>,
    values: BTreeMap<String, FieldInput>,
    original: RecordValues,
    stage: String,
    dirty: bool,
}
impl RecordDraft {
    fn new(model: &ToolViewModel, record: Option<&ToolRecordView>) -> Self {
        let original = record.map(|r| r.values.clone()).unwrap_or_default();
        Self {
            generation: model.generation,
            revision: record.map(|r| r.record_revision),
            values: model
                .active_spec
                .fields
                .iter()
                .map(|f| {
                    (
                        f.id.clone(),
                        FieldInput {
                            present: f.required || original.contains_key(&f.id),
                            text: original
                                .get(&f.id)
                                .map(|v| match v {
                                    FieldValue::Boolean(b) => b.to_string(),
                                    _ => value_text(v),
                                })
                                .unwrap_or_default(),
                        },
                    )
                })
                .collect(),
            original,
            stage: record
                .map(|r| r.current_stage_id.clone())
                .unwrap_or_else(|| model.active_spec.default_stage_id.clone()),
            dirty: false,
        }
    }
    fn values(&self, spec: &ToolSpec) -> Result<RecordValues, String> {
        let mut values = BTreeMap::new();
        for field in &spec.fields {
            let input = self
                .values
                .get(&field.id)
                .ok_or("字段已变化，请重新打开表单")?;
            if !input.present {
                continue;
            }
            let text = &input.text;
            let invalid = || format!("请检查“{}”的填写内容", field.display_name);
            let value = match &field.kind {
                FieldKind::Text => FieldValue::Text(text.clone()),
                FieldKind::Date => FieldValue::Date(parse_date(text).map_err(|_| invalid())?),
                FieldKind::Integer => {
                    FieldValue::Integer(text.trim().parse().map_err(|_| invalid())?)
                }
                FieldKind::Boolean => FieldValue::Boolean(match text.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(invalid()),
                }),
                FieldKind::Enum { options } if options.contains(text) => {
                    FieldValue::Enum(text.clone())
                }
                FieldKind::Enum { .. } => return Err(invalid()),
            };
            if field.required && matches!(&value, FieldValue::Text(v) if v.trim().is_empty()) {
                return Err(format!("请填写{}", field.display_name));
            }
            values.insert(field.id.clone(), value);
        }
        ProjectSnapshot::validate_values(spec, &values)
            .map_err(|e| format!("内容不能保存：{}", e.path))?;
        Ok(values)
    }
}

struct ProjectDraft {
    name: String,
    location: String,
    spec: ToolSpec,
    generation: Option<u64>,
    optional_name: String,
    optional_kind: usize,
}
impl Default for ProjectDraft {
    fn default() -> Self {
        Self {
            name: "我的订单工具".into(),
            location: String::new(),
            spec: studio_order_template(),
            generation: None,
            optional_name: String::new(),
            optional_kind: 0,
        }
    }
}
#[derive(Clone)]
struct ChangeDraft {
    generation: u64,
    original_request: String,
    reason: String,
    unresolved: String,
    policy: BehaviorPolicy,
    scope: ScopeKind,
    date: String,
    due_override: bool,
    due_date: String,
    stage: String,
    supersedes: Vec<String>,
}
impl ChangeDraft {
    fn new(view: &WorkspaceView<'_>, policy: &BehaviorPolicy) -> Self {
        Self {
            generation: view.project.map(|m| m.generation).unwrap_or(0),
            original_request: String::new(),
            reason: String::new(),
            unresolved: String::new(),
            policy: policy.clone(),
            scope: ScopeKind::SingleRecord,
            date: view.as_of_date.to_string(),
            due_override: false,
            due_date: String::new(),
            stage: String::new(),
            supersedes: vec![],
        }
    }
    fn input(&self, spec: &ToolSpec) -> Result<RuleInput, String> {
        if self.original_request.trim().is_empty() || self.reason.trim().is_empty() {
            return Err("请填写想修改的做法和理由，再演练或保存决定".into());
        }
        let mut record_changes = BTreeMap::new();
        if self.due_override {
            let field = spec
                .field_for_role(FieldRole::PromisedDate)
                .ok_or("这个工具没有承诺日期字段")?;
            record_changes.insert(
                field.id.clone(),
                Some(FieldValue::Date(parse_date(&self.due_date)?)),
            );
        }
        let mut a = self.policy.clone();
        a.due_date = DateDuePolicy::KeepOriginal;
        let mut b = self.policy.clone();
        b.due_date = DateDuePolicy::ExtendByPausedDays;
        a.validate().map_err(|_| "提醒天数超出支持范围")?;
        Ok(RuleInput {
            original_request: self.original_request.clone(),
            rationale: self.reason.clone(),
            unresolved_questions: lines(&self.unresolved),
            supersedes: self.supersedes.clone(),
            scope: self.scope,
            candidates: [a, b],
            as_of_date: parse_date(&self.date)?,
            record_changes,
            stage_override: (!self.stage.is_empty()).then(|| self.stage.clone()),
        })
    }
}

#[derive(Default)]
pub struct ToolWorkspace {
    page: Page,
    session: String,
    project: Option<String>,
    selected: Option<String>,
    filter: RecordFilter,
    query: String,
    drafts: BTreeMap<(String, Option<String>), RecordDraft>,
    project_draft: ProjectDraft,
    open_location: String,
    configurations: BTreeMap<String, ProjectDraft>,
    changes: BTreeMap<(String, Option<String>), ChangeDraft>,
    pending: Option<WorkspaceRequest>,
    failed: bool,
    seen_status: Option<OperationStatus>,
    last_operation: String,
    preview_request: Option<RequestContext>,
    input_epoch: u64,
    notice: Option<String>,
    error: Option<String>,
    withdrawal_decision: Option<String>,
    frame_request: Option<(u64, WorkspaceRequest)>,
}

impl ToolWorkspace {
    pub fn show(&mut self, ui: &mut Ui, view: &WorkspaceView<'_>) -> WorkspaceOutput {
        let mut out = WorkspaceOutput::default();
        let frame_id =
            ui.ctx().cumulative_pass_nr() - ui.ctx().output(|o| o.num_completed_passes as u64);
        self.synchronize(view);
        // Navigation is outside the content scroll area, including on small windows.
        ui.horizontal_wrapped(|ui| {
            if !matches!(self.page, Page::Home | Page::Records)
                && out.button(ui, "back", "返回（保留输入）", true)
            {
                self.back();
            }
            if view.project.is_some()
                && out.button(
                    ui,
                    "project.close",
                    if view.is_example {
                        "退出虚构示例"
                    } else {
                        "关闭工具"
                    },
                    self.pending.is_none(),
                )
            {
                self.emit(view, WorkspaceAction::CloseProject, &mut out);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape))
            && !matches!(self.page, Page::Home | Page::Records)
        {
            self.back();
        }
        if view.is_example {
            out.label(
                ui,
                "虚构示例 · 不是真实订单。退出后创建的正式工具不会带入这些记录。",
            );
        }
        if let Some(model) = view.project {
            if model.access == ProjectAccess::ReadOnly {
                out.label(ui, "只读：可以查看，不能保存或采用规则");
            }
        }
        ScrollArea::vertical()
            .id_salt(("tool-workspace", &self.session, self.page as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                self.status(ui, view, &mut out);
                match (self.page, view.project) {
                    (Page::Home, _) | (_, None)
                        if !matches!(self.page, Page::Create | Page::Open) =>
                    {
                        self.home(ui, view, &mut out)
                    }
                    (Page::Create, _) => self.project_form(ui, view, false, &mut out),
                    (Page::Open, _) => self.open_form(ui, view, &mut out),
                    (Page::Configure, Some(_)) => self.project_form(ui, view, true, &mut out),
                    (Page::Records, Some(model)) => self.records(ui, view, model, &mut out),
                    (Page::Detail, Some(model)) => self.detail(ui, view, model, &mut out),
                    (Page::Edit, Some(model)) => self.record_form(ui, view, model, &mut out),
                    (Page::Change, Some(model)) => self.change(ui, view, model, &mut out),
                    (Page::Decisions, Some(model)) => self.decisions(ui, view, model, &mut out),
                    (Page::Withdrawal, Some(_)) => self.withdrawal(ui, view, &mut out),
                    _ => self.home(ui, view, &mut out),
                }
                if let Some(message) = &self.notice {
                    out.label(ui, message);
                }
            });
        if let Some(request) = &out.request {
            self.frame_request = Some((frame_id, request.clone()));
        } else if let Some((id, request)) = &self.frame_request {
            if *id == frame_id {
                out.request = Some(request.clone());
            }
        }
        out
    }

    fn synchronize(&mut self, view: &WorkspaceView<'_>) {
        let project = view.project.map(|m| m.project_id.clone());
        if self.session != view.session_id || self.project != project {
            self.session = view.session_id.into();
            self.project = project;
            self.selected = view
                .project
                .and_then(|m| m.selected_record.as_ref().map(|r| r.record_id.clone()));
            self.page = if view.project.is_some() {
                Page::Records
            } else {
                Page::Home
            };
            self.pending = None;
            self.failed = false;
            self.seen_status = None;
            self.preview_request = None;
            self.notice = None;
            self.error = None;
            self.frame_request = None;
            self.input_epoch += 1;
        }
        let Some(status) = view.operation else {
            return;
        };
        if self.seen_status.as_ref().is_some_and(|old| {
            old.context == status.context && old.delivery_id >= status.delivery_id
        }) {
            return;
        }
        let Some(pending) = &self.pending else {
            return;
        };
        if status.context != pending.context {
            return;
        }
        match &status.outcome {
            OperationOutcome::Running => {
                self.failed = false;
            }
            OperationOutcome::Failed(message) => {
                self.failed = true;
                self.error = Some(format!("未保存：{message}。输入已保留"));
            }
            OperationOutcome::Rejected(message) => {
                self.pending = None;
                self.failed = false;
                self.error = Some(format!("未保存：{message}。输入已保留，请修改后再试"));
            }
            OperationOutcome::PreviewReady => {
                if matches!(
                    pending.action,
                    WorkspaceAction::Rehearse { .. } | WorkspaceAction::PreviewWithdrawal { .. }
                ) {
                    self.pending = None;
                    self.failed = false;
                }
            }
            OperationOutcome::Committed => {
                let advanced = view.project.is_some_and(|m| {
                    Some(&m.project_id) == pending.context.project_id.as_ref()
                        && pending.context.generation.is_some_and(|g| m.generation > g)
                });
                if !advanced
                    || matches!(
                        pending.action,
                        WorkspaceAction::Rehearse { .. }
                            | WorkspaceAction::PreviewWithdrawal { .. }
                    )
                {
                    return;
                }
                if matches!(
                    pending.action,
                    WorkspaceAction::Record(
                        ToolCommand::CreateRecord { .. } | ToolCommand::EditRecord { .. }
                    )
                ) {
                    self.drafts.remove(&(
                        self.project.clone().unwrap_or_else(|| self.session.clone()),
                        pending.context.record_id.clone(),
                    ));
                }
                if matches!(pending.action, WorkspaceAction::Configure { .. }) {
                    self.configurations.remove(&self.project_key());
                }
                self.pending = None;
                self.failed = false;
                self.error = None;
                self.notice = (status.context.record_id == self.selected
                    && status.context.input_epoch == self.input_epoch)
                    .then(|| "已保存".into());
                if self.page == Page::Edit {
                    self.page = if self.selected.is_some() {
                        Page::Detail
                    } else {
                        Page::Records
                    };
                }
            }
        }
        self.seen_status = Some(status.clone());
    }
    fn context(&self, view: &WorkspaceView<'_>) -> RequestContext {
        RequestContext {
            request_id: view.next_operation_id.into(),
            session_id: view.session_id.into(),
            project_id: view.project.map(|m| m.project_id.clone()),
            generation: view.project.map(|m| m.generation),
            record_id: self.selected.clone(),
            record_revision: view
                .project
                .and_then(|m| self.record(m).map(|r| r.record_revision)),
            input_epoch: self.input_epoch,
        }
    }
    fn current_context(&self, view: &WorkspaceView<'_>, context: &RequestContext) -> bool {
        let mut current = self.context(view);
        current.request_id = context.request_id.clone();
        current == *context && self.preview_request.as_ref() == Some(context)
    }
    fn emit(
        &mut self,
        view: &WorkspaceView<'_>,
        action: WorkspaceAction,
        out: &mut WorkspaceOutput,
    ) {
        if self.pending.is_some()
            || out.request.is_some()
            || self.last_operation == view.next_operation_id
        {
            return;
        }
        let request = WorkspaceRequest {
            context: self.context(view),
            action,
        };
        if matches!(
            request.action,
            WorkspaceAction::Rehearse { .. } | WorkspaceAction::PreviewWithdrawal { .. }
        ) {
            self.preview_request = Some(request.context.clone());
        }
        self.last_operation = view.next_operation_id.into();
        self.pending = Some(request.clone());
        self.failed = false;
        self.error = None;
        self.notice = None;
        out.request = Some(request);
    }
    fn writable(&self, view: &WorkspaceView<'_>) -> bool {
        self.pending.is_none()
            && view
                .project
                .is_some_and(|m| m.access == ProjectAccess::Writable)
    }
    fn back(&mut self) {
        // A copied rehearsal has no committed side effect to undo. Discard its
        // local wait; late results remain fenced out by the input epoch below.
        // A submitted write is kept pending until the controller resolves it.
        if self.pending.as_ref().is_some_and(|request| {
            matches!(
                request.action,
                WorkspaceAction::Rehearse { .. } | WorkspaceAction::PreviewWithdrawal { .. }
            )
        }) {
            self.pending = None;
            self.failed = false;
        }
        self.page = match self.page {
            Page::Edit | Page::Change => {
                if self.selected.is_some() {
                    Page::Detail
                } else {
                    Page::Records
                }
            }
            Page::Detail | Page::Configure | Page::Decisions => Page::Records,
            Page::Withdrawal => Page::Decisions,
            _ => Page::Home,
        };
        self.input_epoch += 1;
        self.preview_request = None;
        self.error = None;
    }
    fn record<'a>(&self, model: &'a ToolViewModel) -> Option<&'a ToolRecordView> {
        self.selected
            .as_ref()
            .and_then(|id| model.records.iter().find(|r| &r.record_id == id))
    }
    fn project_key(&self) -> String {
        self.project.clone().unwrap_or_else(|| self.session.clone())
    }
    fn key(&self) -> (String, Option<String>) {
        (
            self.project.clone().unwrap_or_else(|| self.session.clone()),
            self.selected.clone(),
        )
    }
    fn status(&mut self, ui: &mut Ui, view: &WorkspaceView<'_>, out: &mut WorkspaceOutput) {
        if let Some(pending) = &self.pending {
            if self.failed {
                let same = pending.context.session_id == view.session_id
                    && pending.context.project_id == view.project.map(|m| m.project_id.clone())
                    && pending.context.generation == view.project.map(|m| m.generation);
                let writable = view
                    .project
                    .is_none_or(|m| m.access == ProjectAccess::Writable)
                    || matches!(
                        pending.action,
                        WorkspaceAction::Rehearse { .. }
                            | WorkspaceAction::PreviewWithdrawal { .. }
                    );
                if out.button(ui, "operation.retry", "重试同一次操作", same && writable) {
                    out.request = self.pending.clone();
                    self.failed = false;
                    self.error = None;
                }
                out.label(
                    ui,
                    "确认保存结果前不会重复提交其他操作。返回不会撤销已发出的保存请求。",
                );
            } else {
                out.label(
                    ui,
                    if matches!(
                        pending.action,
                        WorkspaceAction::Rehearse { .. }
                            | WorkspaceAction::PreviewWithdrawal { .. }
                    ) {
                        "演练中…"
                    } else {
                        "正在处理，请等待保存确认…"
                    },
                );
            }
        }
        if let Some(message) = &self.error {
            ui.visuals_mut().override_text_color = Some(Color32::from_rgb(200, 80, 70));
            out.label(ui, message);
            ui.visuals_mut().override_text_color = None;
        }
        if let Some(model) = view.project {
            if let Some(message) = &model.access_message {
                out.label(ui, message);
            }
        }
    }
    fn home(&mut self, ui: &mut Ui, view: &WorkspaceView<'_>, out: &mut WorkspaceOutput) {
        ui.heading("我的工具");
        out.label(
            ui,
            "从一笔熟悉的工作开始。不需要仓库、终端、账号或 API 密钥。",
        );
        if out.button(ui, "entry.create", "创建我的工具", self.pending.is_none()) {
            self.page = Page::Create;
        }
        if out.button(ui, "entry.open", "打开已有工具", self.pending.is_none()) {
            self.page = Page::Open;
        }
        if out.button(ui, "entry.example", "体验虚构示例", self.pending.is_none()) {
            self.emit(view, WorkspaceAction::StartExample, out);
        }
        out.label(ui, "示例与正式工具分开。真实记录只保存在你明确选择的位置。");
    }
    fn open_form(&mut self, ui: &mut Ui, view: &WorkspaceView<'_>, out: &mut WorkspaceOutput) {
        ui.heading("打开已有工具");
        out.label(ui, "填写已有工具的本地目录");
        out.control(
            "project.location",
            ui.add(TextEdit::singleline(&mut self.open_location).desired_width(f32::INFINITY)),
        );
        if out.button(
            ui,
            "project.open",
            "打开",
            self.pending.is_none() && !self.open_location.trim().is_empty(),
        ) {
            self.emit(
                view,
                WorkspaceAction::OpenProject {
                    location: self.open_location.trim().into(),
                },
                out,
            );
        }
    }
    fn project_form(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        configure: bool,
        out: &mut WorkspaceOutput,
    ) {
        ui.heading(if configure {
            "工具设置"
        } else {
            "创建我的工具"
        });
        let mut draft = if configure {
            self.configurations
                .remove(&self.project_key())
                .unwrap_or_else(|| {
                    let m = view.project.expect("configuration needs a project");
                    ProjectDraft {
                        name: m.project_name.clone(),
                        spec: m.active_spec.clone(),
                        generation: Some(m.generation),
                        ..ProjectDraft::default()
                    }
                })
        } else {
            std::mem::take(&mut self.project_draft)
        };
        let allowed = self.pending.is_none() && (!configure || self.writable(view));
        let mut edited = false;
        ui.add_enabled_ui(allowed, |ui| {
            out.label(ui, "工具名称");
            edited |= out
                .control(
                    "project.name",
                    ui.add(TextEdit::singleline(&mut draft.name).desired_width(f32::INFINITY)),
                )
                .changed();
            if !configure {
                out.label(ui, "本地保存位置（正式工具不包含虚构记录）");
                edited |= out
                    .control(
                        "project.location",
                        ui.add(
                            TextEdit::singleline(&mut draft.location).desired_width(f32::INFINITY),
                        ),
                    )
                    .changed();
            }
            let fields = ui.collapsing("字段与阶段名称", |ui| {
                out.label(ui, "可以改显示名称。字段身份、已有类型和原始事实保持不变。");
                for field in &mut draft.spec.fields {
                    out.label(
                        ui,
                        format!(
                            "{}{}",
                            field.id,
                            if field.required {
                                "（必填）"
                            } else {
                                "（可选）"
                            }
                        ),
                    );
                    edited |= out
                        .control(
                            &format!("spec.field.{}", field.id),
                            ui.add(
                                TextEdit::singleline(&mut field.display_name)
                                    .desired_width(f32::INFINITY),
                            ),
                        )
                        .changed();
                }
                for stage in &mut draft.spec.stages {
                    out.label(ui, &stage.id);
                    edited |= out
                        .control(
                            &format!("spec.stage.{}", stage.id),
                            ui.add(
                                TextEdit::singleline(&mut stage.display_name)
                                    .desired_width(f32::INFINITY),
                            ),
                        )
                        .changed();
                }
                out.label(ui, "新增可选字段的显示名称");
                edited |= out
                    .control(
                        "spec.optional.name",
                        ui.add(
                            TextEdit::singleline(&mut draft.optional_name)
                                .desired_width(f32::INFINITY),
                        ),
                    )
                    .changed();
                ui.horizontal_wrapped(|ui| {
                    for (i, label) in ["文字", "日期", "整数", "是或否"].iter().enumerate()
                    {
                        edited |= ui
                            .selectable_value(&mut draft.optional_kind, i, *label)
                            .changed();
                    }
                });
                if out.button(
                    ui,
                    "spec.optional.add",
                    "添加可选字段",
                    !draft.optional_name.trim().is_empty()
                        && draft.spec.fields.len() < MAX_FIELD_DEFINITIONS,
                ) {
                    edited = true;
                    let mut n = 1;
                    while draft.spec.field(&format!("custom-{n}")).is_some() {
                        n += 1;
                    }
                    let id = format!("custom-{n}");
                    draft.spec.fields.push(FieldDefinition {
                        id: id.clone(),
                        display_name: draft.optional_name.trim().into(),
                        kind: match draft.optional_kind {
                            1 => FieldKind::Date,
                            2 => FieldKind::Integer,
                            3 => FieldKind::Boolean,
                            _ => FieldKind::Text,
                        },
                        role: FieldRole::Custom,
                        required: false,
                        extensions: BTreeMap::new(),
                    });
                    draft.spec.detail_field_ids.push(id);
                    draft.optional_name.clear();
                }
            });
            edited |= out.control("spec.expand", fields.header_response).changed();
        });
        if edited {
            self.notice = None;
        }
        if configure
            && view
                .project
                .is_some_and(|m| draft.name != m.project_name || draft.spec != m.active_spec)
        {
            out.label(ui, "未保存");
        }
        let stale = configure && draft.generation != view.project.map(|m| m.generation);
        if stale {
            out.label(
                ui,
                "项目已有变化，设置草稿已保留。可核对后明确放弃草稿并重新读取",
            );
        }
        if stale
            && out.button(
                ui,
                "config.reload",
                "放弃设置草稿并重新读取",
                self.pending.is_none(),
            )
        {
            let model = view.project.expect("configuration has project");
            draft = ProjectDraft {
                name: model.project_name.clone(),
                spec: model.active_spec.clone(),
                generation: Some(model.generation),
                ..ProjectDraft::default()
            };
        }
        if out.button(
            ui,
            "project.submit",
            if configure {
                "保存设置"
            } else {
                "创建正式工具"
            },
            allowed && !stale,
        ) {
            if draft.name.trim().is_empty() || (!configure && draft.location.trim().is_empty()) {
                self.error = Some("请填写工具名称和本地保存位置".into());
            } else {
                let mut spec = draft.spec.clone();
                spec.display_name = draft.name.trim().into();
                if configure {
                    spec.revision = spec.revision.saturating_add(1);
                }
                match spec.validate() {
                    Err(error) => {
                        self.error = Some(format!("请检查字段或阶段名称：{}", error.path))
                    }
                    Ok(()) => self.emit(
                        view,
                        if configure {
                            WorkspaceAction::Configure {
                                name: draft.name.trim().into(),
                                spec,
                            }
                        } else {
                            WorkspaceAction::CreateProject {
                                name: draft.name.trim().into(),
                                location: draft.location.trim().into(),
                                spec,
                            }
                        },
                        out,
                    ),
                }
            }
        }
        if configure {
            self.configurations.insert(self.project_key(), draft);
        } else {
            self.project_draft = draft;
        }
    }
    fn records(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        model: &ToolViewModel,
        out: &mut WorkspaceOutput,
    ) {
        out.label(ui, &model.project_name);
        if let Some(location) = view.location {
            out.label(ui, format!("保存位置：{location}"));
        }
        ui.horizontal_wrapped(|ui| {
            if out.button(ui, "record.new", "新增记录", self.writable(view)) {
                self.selected = None;
                self.page = Page::Edit;
                self.notice = None;
                self.input_epoch += 1;
            }
            if out.button(ui, "project.configure", "工具设置", self.pending.is_none()) {
                self.page = Page::Configure;
            }
            if out.button(ui, "decisions.open", "决定历史与撤回", true) {
                self.page = Page::Decisions;
                self.input_epoch += 1;
            }
        });
        out.control(
            "record.search",
            ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("搜索标题或内容")
                    .desired_width(f32::INFINITY),
            ),
        );
        ui.horizontal_wrapped(|ui| {
            for (filter, key, label) in [
                (RecordFilter::All, "filter.all", "全部"),
                (RecordFilter::Incomplete, "filter.incomplete", "未完成"),
                (RecordFilter::Waiting, "filter.waiting", "等待中"),
                (RecordFilter::Due, "filter.due", "临期或已逾期"),
            ] {
                out.control(key, ui.selectable_value(&mut self.filter, filter, label));
            }
        });
        let mut count = 0;
        for record in &model.records {
            let stage = model.active_spec.stage(&record.current_stage_id);
            let matches_filter = match self.filter {
                RecordFilter::All => true,
                RecordFilter::Incomplete => stage.is_some_and(|s| !s.terminal),
                RecordFilter::Waiting => {
                    stage.is_some_and(|s| s.clock == StageClock::Paused && !s.terminal)
                }
                RecordFilter::Due => {
                    stage.is_some_and(|s| !s.terminal)
                        && record
                            .derived
                            .display_due_date
                            .is_some_and(|d| (d - view.as_of_date).num_days() <= 1)
                }
            };
            let matches_query = self.query.is_empty()
                || record.values.values().any(|v| {
                    value_text(v)
                        .to_lowercase()
                        .contains(&self.query.to_lowercase())
                });
            if !matches_filter || !matches_query {
                continue;
            }
            count += 1;
            let title = record_title(model, record);
            let label = format!(
                "{title}\n{}",
                stage.map(|s| s.display_name.as_str()).unwrap_or("未知状态")
            );
            if out.button(
                ui,
                &format!("record.select.{}", record.record_id),
                &label,
                true,
            ) {
                self.selected = Some(record.record_id.clone());
                self.page = Page::Detail;
                self.input_epoch += 1;
                self.preview_request = None;
                self.notice = None;
            }
        }
        if count == 0 {
            out.label(
                ui,
                if model.records.is_empty() {
                    "还没有记录。点击“新增记录”开始第一笔工作"
                } else {
                    "没有符合筛选条件的记录，可以改选“全部”或清空搜索"
                },
            );
        }
    }
    fn detail(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        model: &ToolViewModel,
        out: &mut WorkspaceOutput,
    ) {
        let Some(record) = self.record(model).cloned() else {
            out.label(ui, "这笔记录已不可用，请返回列表重新选择");
            return;
        };
        out.label(ui, record_title(model, &record));
        ui.horizontal_wrapped(|ui| {
            if out.button(ui, "record.edit", "编辑记录", self.writable(view)) {
                self.page = Page::Edit;
                self.notice = None;
                self.input_epoch += 1;
            }
            if out.button(ui, "record.change", "按我的做法修改", true) {
                self.page = Page::Change;
                self.input_epoch += 1;
            }
        });
        for field in &model.active_spec.fields {
            out.label(
                ui,
                format!(
                    "{}：{}",
                    field.display_name,
                    record
                        .values
                        .get(&field.id)
                        .map(value_text)
                        .unwrap_or_else(|| "未填写".into())
                ),
            );
        }
        show_observation(ui, out, &record.derived, &model.active_spec);
        out.label(
            ui,
            "按日历日计算，不推断工作日、节假日或营业时间。没有起始时间时显示无法确定。",
        );
        if let Some(stage) = model.active_spec.stage(&record.current_stage_id) {
            out.label(ui, "记录现在的进展");
            ui.horizontal_wrapped(|ui| {
                for next in &stage.allowed_next_stage_ids {
                    if let Some(target) = model.active_spec.stage(next) {
                        if out.button(
                            ui,
                            &format!("stage.{next}"),
                            &format!("改为{}", target.display_name),
                            self.writable(view),
                        ) {
                            self.emit(
                                view,
                                WorkspaceAction::Record(ToolCommand::TransitionStage {
                                    operation_id: view.next_operation_id.into(),
                                    expected_generation: model.generation,
                                    record_id: record.record_id.clone(),
                                    expected_record_revision: record.record_revision,
                                    to_stage_id: target.id.clone(),
                                    occurred_at: view.now,
                                }),
                                out,
                            );
                        }
                    }
                }
            });
        }
        ui.collapsing("发生过什么", |ui| {
            let events: Vec<_> = view
                .record_history
                .iter()
                .filter(|e| e.record_id == record.record_id)
                .collect();
            if events.is_empty() {
                out.label(ui, "暂无可显示的历史；不会根据当前状态补写过去事件");
            }
            for event in events {
                let text = match &event.kind {
                    RecordEventKind::Created { .. } => "创建记录".into(),
                    RecordEventKind::FieldsChanged { changes } => {
                        format!("修改了 {} 项业务信息", changes.len())
                    }
                    RecordEventKind::StageChanged {
                        from_stage_id,
                        to_stage_id,
                    } => format!(
                        "{} → {}",
                        stage_name(&model.active_spec, from_stage_id),
                        stage_name(&model.active_spec, to_stage_id)
                    ),
                };
                out.label(ui, format!("{}：{text}", event.occurred_at.to_rfc3339()));
            }
        });
    }
    fn record_form(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        model: &ToolViewModel,
        out: &mut WorkspaceOutput,
    ) {
        ui.heading(if self.selected.is_some() {
            "编辑记录"
        } else {
            "新增记录"
        });
        let key = self.key();
        let mut draft = self
            .drafts
            .remove(&key)
            .unwrap_or_else(|| RecordDraft::new(model, self.record(model)));
        let stale = draft.generation != model.generation
            || draft.revision != self.record(model).map(|r| r.record_revision);
        if stale {
            out.label(
                ui,
                "项目已有变化，未保存输入已保留。请先核对最新记录，不会覆盖他人的更新",
            );
        }
        if stale
            && out.button(
                ui,
                "record.reload",
                "放弃此草稿并重新读取记录",
                self.pending.is_none(),
            )
        {
            draft = RecordDraft::new(model, self.record(model));
        }
        if draft.dirty {
            self.notice = None;
            out.label(ui, "未保存");
        }
        ui.add_enabled_ui(self.writable(view) && !stale, |ui| {
            for field in &model.active_spec.fields {
                let Some(input) = draft.values.get_mut(&field.id) else {
                    continue;
                };
                ui.horizontal_wrapped(|ui| {
                    out.label(ui, &field.display_name);
                    if !field.required {
                        if out
                            .control(
                                &format!("present.{}", field.id),
                                ui.checkbox(&mut input.present, "填写此项"),
                            )
                            .changed()
                        {
                            draft.dirty = true;
                        }
                    } else {
                        out.label(ui, "必填");
                    }
                });
                ui.add_enabled_ui(input.present, |ui| {
                    let changed = match &field.kind {
                        FieldKind::Boolean => {
                            let mut value = input.text == "true";
                            let r = out.control(
                                &format!("field.{}", field.id),
                                ui.checkbox(&mut value, "是"),
                            );
                            if input.text.is_empty() || r.changed() {
                                input.text = value.to_string();
                            }
                            r.changed()
                        }
                        FieldKind::Enum { options } => {
                            let before = input.text.clone();
                            ui.horizontal_wrapped(|ui| {
                                for option in options {
                                    ui.selectable_value(&mut input.text, option.clone(), option);
                                }
                            });
                            before != input.text
                        }
                        _ => {
                            if field.kind == FieldKind::Date {
                                out.label(ui, "日期格式：2026-10-06");
                            }
                            let edit = if field.role == FieldRole::Notes
                                || field.role == FieldRole::WorkDescription
                            {
                                TextEdit::multiline(&mut input.text).desired_rows(3)
                            } else {
                                TextEdit::singleline(&mut input.text)
                            };
                            out.control(
                                &format!("field.{}", field.id),
                                ui.add(edit.desired_width(f32::INFINITY)),
                            )
                            .changed()
                        }
                    };
                    draft.dirty |= changed;
                });
            }
        });
        if out.button(ui, "record.save", "保存记录", self.writable(view) && !stale) {
            match draft.values(&model.active_spec) {
                Err(error) => self.error = Some(error),
                Ok(values) => {
                    let command = if let Some(record) = self.record(model) {
                        let changes: BTreeMap<_, _> = model
                            .active_spec
                            .fields
                            .iter()
                            .filter_map(|f| {
                                let value = values.get(&f.id);
                                (value != draft.original.get(&f.id))
                                    .then(|| (f.id.clone(), value.cloned()))
                            })
                            .collect();
                        if changes.is_empty() {
                            self.error = Some("没有需要保存的修改".into());
                            None
                        } else {
                            Some(ToolCommand::EditRecord {
                                operation_id: view.next_operation_id.into(),
                                expected_generation: draft.generation,
                                record_id: record.record_id.clone(),
                                expected_record_revision: draft
                                    .revision
                                    .unwrap_or(record.record_revision),
                                changes,
                                occurred_at: view.now,
                            })
                        }
                    } else {
                        Some(ToolCommand::CreateRecord {
                            operation_id: view.next_operation_id.into(),
                            expected_generation: draft.generation,
                            record_id: view.next_operation_id.into(),
                            initial_stage_id: draft.stage.clone(),
                            values,
                            occurred_at: view.now,
                        })
                    };
                    if let Some(command) = command {
                        self.emit(view, WorkspaceAction::Record(command), out);
                    }
                }
            }
        }
        if draft.dirty {
            self.notice = None;
        }
        self.drafts.insert(key, draft);
    }
    fn change(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        model: &ToolViewModel,
        out: &mut WorkspaceOutput,
    ) {
        ui.heading("支持的规则调整");
        out.label(ui,"目前支持制作计时、显示交付目标和跟进提醒。原话会保留；其他需求仍待解决，不会自动变成已经实现的功能。");
        let Some(policy) = self
            .selected
            .as_ref()
            .and_then(|id| view.record_policies.get(id))
        else {
            out.label(
                ui,
                "无法读取这笔记录当前生效的规则，请刷新工作区后再试。不会使用其他规则代替",
            );
            return;
        };
        let key = self.key();
        let mut draft = self
            .changes
            .remove(&key)
            .unwrap_or_else(|| ChangeDraft::new(view, policy));
        let stale = draft.generation != model.generation;
        if stale {
            out.label(
                ui,
                "项目已有变化，修改原话和草稿已保留。请核对当前规则后重新开始演练",
            );
            if out.button(
                ui,
                "change.reload",
                "放弃规则草稿并读取当前规则",
                self.pending.is_none(),
            ) {
                draft = ChangeDraft::new(view, policy);
                self.input_epoch += 1;
            }
        }
        let mut changed = false;
        ui.add_enabled_ui(self.pending.is_none(), |ui| {
            out.label(ui, "想按怎样的做法修改？（必填）");
            changed |= out
                .control(
                    "change.request",
                    ui.add(
                        TextEdit::multiline(&mut draft.original_request)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    ),
                )
                .changed();
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .radio_value(
                        &mut draft.policy.timer,
                        TimerPolicy::PauseStagesMarkedPaused,
                        "等待时暂停制作计时",
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut draft.policy.timer,
                        TimerPolicy::CountPausedStages,
                        "等待也计入制作时间",
                    )
                    .changed();
            });
            out.label(
                ui,
                "将实际比较：A 保留原承诺日期；B 显示目标按等待天数顺延。两者都会保留原始承诺。",
            );
            let mut reminder_kind = match draft.policy.reminder {
                ReminderPolicy::Never => 0,
                ReminderPolicy::WaitingBeforeDue { .. } => 1,
                ReminderPolicy::WaitingAfterDays { .. } => 2,
            };
            let old_kind = reminder_kind;
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut reminder_kind, 0, "不提醒");
                ui.radio_value(&mut reminder_kind, 1, "临近承诺日提醒");
                ui.radio_value(&mut reminder_kind, 2, "等待达到天数时提醒");
            });
            if reminder_kind != old_kind {
                changed = true;
                draft.policy.reminder = match reminder_kind {
                    1 => ReminderPolicy::WaitingBeforeDue { days_before_due: 1 },
                    2 => ReminderPolicy::WaitingAfterDays { days_waiting: 3 },
                    _ => ReminderPolicy::Never,
                };
            }
            match &mut draft.policy.reminder {
                ReminderPolicy::WaitingBeforeDue { days_before_due } => {
                    changed |= ui
                        .add(
                            egui::DragValue::new(days_before_due)
                                .range(0..=3650)
                                .suffix(" 天前"),
                        )
                        .changed();
                }
                ReminderPolicy::WaitingAfterDays { days_waiting } => {
                    changed |= ui
                        .add(
                            egui::DragValue::new(days_waiting)
                                .range(1..=3650)
                                .suffix(" 天后"),
                        )
                        .changed();
                }
                _ => {}
            }
            out.label(ui, "演练日期（只改变副本，用日期变化试验不同等待天数）");
            changed |= out
                .control(
                    "change.date",
                    ui.add(TextEdit::singleline(&mut draft.date).desired_width(f32::INFINITY)),
                )
                .changed();
            changed |= out
                .control(
                    "change.override_due",
                    ui.checkbox(&mut draft.due_override, "在副本里试一个新的承诺日期"),
                )
                .changed();
            if draft.due_override {
                changed |= out
                    .control(
                        "change.due_date",
                        ui.add(
                            TextEdit::singleline(&mut draft.due_date)
                                .desired_width(f32::INFINITY)
                                .hint_text("2026-10-06"),
                        ),
                    )
                    .changed();
            }
            ui.collapsing("在副本里改变状态", |ui| {
                changed |= ui
                    .radio_value(&mut draft.stage, String::new(), "保持当前状态")
                    .changed();
                for stage in &model.active_spec.stages {
                    changed |= ui
                        .radio_value(&mut draft.stage, stage.id.clone(), &stage.display_name)
                        .changed();
                }
                out.label(ui, "只有允许的状态转移才可演练，不会改写实际记录的过去事件");
            });
            out.label(ui, "适用范围");
            for (scope, key, label) in [
                (ScopeKind::SingleRecord, "scope.single", "只改这一笔"),
                (
                    ScopeKind::FutureRecords,
                    "scope.future",
                    "只用于以后新建的记录",
                ),
                (
                    ScopeKind::IncompleteAndFuture,
                    "scope.incomplete",
                    "改当前未完成记录和以后新建的记录",
                ),
                (
                    ScopeKind::AllExistingAndFuture,
                    "scope.all",
                    "包括历史记录并用于以后新建的记录",
                ),
            ] {
                changed |= out
                    .control(key, ui.radio_value(&mut draft.scope, scope, label))
                    .changed();
            }
            out.label(ui, "理由（必填，可以写“暂时还不能判断”）");
            changed |= out
                .control(
                    "change.reason",
                    ui.add(
                        TextEdit::multiline(&mut draft.reason)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    ),
                )
                .changed();
            out.label(ui, "尚未解决的问题（每行一项，可留空）");
            changed |= out
                .control(
                    "change.unresolved",
                    ui.add(
                        TextEdit::multiline(&mut draft.unresolved)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    ),
                )
                .changed();
            let supersedes = ui.collapsing("明确替代以前的决定", |ui| {
                out.label(
                    ui,
                    "只有确实改变了主意才选择。未选择的有效决定仍会全部检查。",
                );
                for decision in model.decisions.iter().filter(|d| {
                    matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                }) {
                    let mut selected = draft.supersedes.contains(&decision.decision_id);
                    if out
                        .control(
                            &format!("change.supersedes.{}", decision.decision_id),
                            ui.checkbox(
                                &mut selected,
                                format!(
                                    "{}：{}",
                                    decision.decision_id,
                                    view.decision_requests
                                        .get(&decision.decision_id)
                                        .map(String::as_str)
                                        .unwrap_or(&decision.rationale)
                                ),
                            ),
                        )
                        .changed()
                    {
                        changed = true;
                        draft.supersedes.retain(|id| id != &decision.decision_id);
                        if selected {
                            draft.supersedes.push(decision.decision_id.clone());
                        }
                    }
                }
            });
            out.control("change.supersedes.expand", supersedes.header_response);
        });
        if changed {
            self.input_epoch += 1;
            self.notice = None;
        }
        let input = draft.input(&model.active_spec);
        if let Err(error) = &input {
            out.label(ui, error);
        }
        if out.button(
            ui,
            "change.rehearse",
            "在副本上演练两种结果",
            self.pending.is_none() && input.is_ok() && !stale,
        ) {
            self.emit(
                view,
                WorkspaceAction::Rehearse {
                    input: input.as_ref().unwrap().clone(),
                },
                out,
            );
        }
        if let Some(preview) = view.rehearsal {
            let current = !stale
                && preview.fresh
                && self.current_context(view, &preview.context)
                && input.as_ref().is_ok_and(|i| i.scope == preview.scope.kind);
            out.label(
                ui,
                if current {
                    "演练结果：实际执行的副本"
                } else {
                    "证据过期：需要重新演练"
                },
            );
            out.label(
                ui,
                format!(
                    "冻结范围：{} 笔现有记录；{}以后新建记录的默认规则",
                    preview.scope.frozen_record_ids.len(),
                    if preview.scope.applies_to_future_records {
                        "会改变"
                    } else {
                        "不改变"
                    }
                ),
            );
            out.label(
                ui,
                "原始录入、状态变化、完成时间与承诺事实不会被规则预览改写",
            );
            for candidate in &preview.candidates {
                ui.separator();
                out.label(ui, &candidate.label);
                out.label(ui, "实际现有记录的影响");
                for comparison in &candidate.comparisons {
                    show_comparison(ui, out, comparison, &model.active_spec);
                }
                out.label(ui, "演练例子的实际结果（可能使用了你修改过的副本输入）");
                for evidence in &candidate.evidence {
                    out.label(
                        ui,
                        match evidence.source {
                            EvidenceSource::NativeExecution => "来源：内置规则实际执行",
                            EvidenceSource::ImportedUntrusted => "来源：外部提议，尚未本地验证",
                            EvidenceSource::Prototype => "来源：交互原型",
                            EvidenceSource::Unknown => "来源：未知",
                        },
                    );
                    for observation in &evidence.observations {
                        show_observation(ui, out, observation, &model.active_spec);
                    }
                    for error in &evidence.errors {
                        out.label(ui, format!("演练错误：{error}"));
                    }
                }
                show_conflicts(ui, out, &candidate.conflicts, &model.active_spec);
                let local = !candidate.evidence.is_empty()
                    && candidate.evidence.iter().all(|e| {
                        e.source == EvidenceSource::NativeExecution
                            && e.status != EvidenceStatus::Failed
                            && e.errors.is_empty()
                            && e.runtime_semantics_version == RUNTIME_SEMANTICS_VERSION
                    });
                let enabled = self.writable(view)
                    && current
                    && local
                    && candidate.ready_to_adopt
                    && candidate.conflicts.is_empty();
                if enabled {
                    out.label(ui, "可以采用，保存确认后才会生效");
                }
                if out.button(
                    ui,
                    &format!("candidate.adopt.{}", candidate.candidate_id),
                    "采用这个结果",
                    enabled,
                ) {
                    self.emit(
                        view,
                        WorkspaceAction::SaveDecision {
                            choice: DecisionChoice::Adopt,
                            input: input.as_ref().unwrap().clone(),
                            preview_id: Some(preview.preview_id.clone()),
                            candidate_id: Some(candidate.candidate_id.clone()),
                        },
                        out,
                    );
                }
            }
        } else {
            out.label(
                ui,
                "还没有演练结果。运行后会显示真实计算值，不会预填通过状态",
            );
        }
        ui.separator();
        out.label(
            ui,
            "还不能选择一个结果？下面的选择只保存待解决问题，不激活任何规则",
        );
        for (choice, key, label) in [
            (DecisionChoice::Both, "decision.both", "两种都需要"),
            (DecisionChoice::Neither, "decision.neither", "都不合适"),
            (
                DecisionChoice::Defer,
                "decision.defer",
                "不知道或以后再决定",
            ),
        ] {
            if out.button(
                ui,
                key,
                label,
                self.writable(view) && input.is_ok() && !stale,
            ) {
                self.emit(
                    view,
                    WorkspaceAction::SaveDecision {
                        choice,
                        input: {
                            let mut input = input.as_ref().unwrap().clone();
                            input.supersedes.clear();
                            input
                        },
                        preview_id: None,
                        candidate_id: None,
                    },
                    out,
                );
            }
        }
        self.changes.insert(key, draft);
    }
    fn decisions(
        &mut self,
        ui: &mut Ui,
        view: &WorkspaceView<'_>,
        model: &ToolViewModel,
        out: &mut WorkspaceOutput,
    ) {
        ui.heading("决定历史");
        out.label(ui,"要继续待决定的事项，可从记录进入“按我的做法修改”，在“明确替代以前的决定”里选择它，然后重新演练和采用");
        if model.decisions.is_empty() {
            out.label(ui, "还没有保存过的决定。可从一笔记录的“按我的做法修改”开始");
        }
        for decision in &model.decisions {
            ui.separator();
            out.label(
                ui,
                format!(
                    "{} · {} · {}",
                    decision.decision_id,
                    choice_name(decision.choice),
                    match decision.status {
                        DecisionStatus::Pending => "待决定",
                        DecisionStatus::Active => "生效",
                        DecisionStatus::Superseded => "已被替代",
                        DecisionStatus::Withdrawn => "已撤回",
                    }
                ),
            );
            out.label(
                ui,
                format!(
                    "原始要求：{}",
                    view.decision_requests
                        .get(&decision.decision_id)
                        .map(String::as_str)
                        .unwrap_or("原话暂时无法读取")
                ),
            );
            out.label(ui, &decision.rationale);
            for question in &decision.unresolved_questions {
                out.label(ui, format!("未解决：{question}"));
            }
            if let Some(scope) = &decision.scope {
                out.label(
                    ui,
                    format!(
                        "范围：{}；冻结 {} 笔记录",
                        scope_name(scope.kind),
                        scope.frozen_record_ids.len()
                    ),
                );
            }
            for observed in &decision.expected_outcomes {
                show_observation(ui, out, observed, &model.active_spec);
            }
            if decision.choice == DecisionChoice::Adopt
                && matches!(
                    decision.status,
                    DecisionStatus::Active | DecisionStatus::Superseded
                )
                && out.button(
                    ui,
                    &format!("decision.undo.{}", decision.decision_id),
                    "预览撤回这次规则改变",
                    self.writable(view),
                )
            {
                self.withdrawal_decision = Some(decision.decision_id.clone());
                self.page = Page::Withdrawal;
                self.input_epoch += 1;
                self.emit(
                    view,
                    WorkspaceAction::PreviewWithdrawal {
                        decision_id: decision.decision_id.clone(),
                    },
                    out,
                );
            }
        }
    }
    fn withdrawal(&mut self, ui: &mut Ui, view: &WorkspaceView<'_>, out: &mut WorkspaceOutput) {
        ui.heading("安全撤回预览");
        out.label(
            ui,
            "撤回的是这次规则改变，不是恢复整库旧快照。确认之前不会改变业务记录。",
        );
        let Some(preview) = view
            .withdrawal
            .filter(|p| Some(&p.decision_id) == self.withdrawal_decision.as_ref())
        else {
            out.label(ui, "等待当前决定的实际撤回预览");
            return;
        };
        let model = view.project.expect("withdrawal page requires project");
        let current = preview.fresh && self.current_context(view, &preview.context);
        if !current {
            out.label(ui, "证据过期：请返回决定历史重新预览");
        }
        out.label(
            ui,
            format!(
                "保留 {} 笔当前记录及 {} 条原始事件，包含后续新增记录、备注修改和完成事实",
                preview.preserved_record_count, preview.preserved_event_count
            ),
        );
        for binding in &preview.retained_bindings {
            out.label(
                ui,
                format!(
                    "保留后来的{}规则：{} · {}",
                    rule_name(binding.rule_key),
                    binding.record_id.as_deref().unwrap_or("以后新建记录"),
                    binding.behavior_revision_id
                ),
            );
        }
        for comparison in &preview.comparisons {
            show_comparison(ui, out, comparison, &model.active_spec);
        }
        show_conflicts(ui, out, &preview.conflicts, &model.active_spec);
        if out.button(
            ui,
            "withdrawal.confirm",
            "确认撤回这次规则改变",
            self.writable(view)
                && current
                && preview.ready_to_commit
                && preview.conflicts.is_empty(),
        ) {
            self.emit(
                view,
                WorkspaceAction::ConfirmWithdrawal {
                    preview_id: preview.preview_id.clone(),
                },
                out,
            );
        }
    }
}

fn parse_date(text: &str) -> Result<NaiveDate, String> {
    let text = text.trim();
    if text.len() != 10 {
        return Err("请填写有效日期，例如 2026-10-06".into());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map_err(|_| "请填写有效日期，例如 2026-10-06".into())
}
fn value_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Text(v) | FieldValue::Enum(v) => v.clone(),
        FieldValue::Date(v) => v.to_string(),
        FieldValue::Integer(v) => v.to_string(),
        FieldValue::Boolean(v) => if *v { "是" } else { "否" }.into(),
    }
}
fn record_title(model: &ToolViewModel, record: &ToolRecordView) -> String {
    model
        .active_spec
        .field_for_role(FieldRole::Title)
        .and_then(|f| record.values.get(&f.id))
        .map(value_text)
        .unwrap_or_else(|| "未命名记录".into())
}
fn stage_name<'a>(spec: &'a ToolSpec, id: &'a str) -> &'a str {
    spec.stage(id)
        .map(|s| s.display_name.as_str())
        .unwrap_or(id)
}
fn date_text(date: Option<NaiveDate>) -> String {
    date.map(|d| d.to_string())
        .unwrap_or_else(|| "无法确定".into())
}
fn show_observation(
    ui: &mut Ui,
    out: &mut WorkspaceOutput,
    value: &EvidenceObservation,
    spec: &ToolSpec,
) {
    out.label(
        ui,
        format!(
            "状态：{}；制作时间：{}；等待：{} 个日历日",
            stage_name(spec, &value.current_stage_id),
            value
                .elapsed_work_days
                .map(|n| format!("{n} 个日历日"))
                .unwrap_or_else(|| "无法确定".into()),
            value.paused_days
        ),
    );
    out.label(
        ui,
        format!(
            "原始承诺：{}；显示交付目标：{}",
            date_text(value.original_due_date),
            date_text(value.display_due_date)
        ),
    );
    if value.original_due_date != value.display_due_date {
        out.label(ui, "目标日期变化不代表已经通知客户或获得客户同意");
    }
    match &value.reminder {
        Some(ReminderObservation::FollowUpWhileWaiting {
            waiting_days,
            days_until_due,
        }) => {
            let when = match days_until_due {
                Some(1) => "明天到目标日".into(),
                Some(0) => "今天到目标日".into(),
                Some(n) if *n < 0 => format!("目标日已过 {} 天", n.unsigned_abs()),
                Some(n) => format!("距目标日还有 {n} 天"),
                None => "目标日无法确定".into(),
            };
            out.label(
                ui,
                format!("下一步：已等待 {waiting_days} 天，{when}，请跟进材料"),
            );
        }
        None => out.label(ui, "下一步：当前规则没有跟进提醒"),
    }
}
fn show_comparison(
    ui: &mut Ui,
    out: &mut WorkspaceOutput,
    c: &ObservationComparison,
    spec: &ToolSpec,
) {
    out.label(ui, format!("记录 {} · 修改前", c.before.record_id));
    show_observation(ui, out, &c.before, spec);
    out.label(ui, "修改后");
    show_observation(ui, out, &c.after, spec);
}
fn show_conflicts(
    ui: &mut Ui,
    out: &mut WorkspaceOutput,
    conflicts: &[ConflictView],
    spec: &ToolSpec,
) {
    for conflict in conflicts {
        out.label(
            ui,
            format!("与决定 {} 冲突：{}", conflict.decision_id, conflict.message),
        );
        out.label(ui, "原来确认的结果");
        for expected in &conflict.expected {
            show_observation(ui, out, expected, spec);
        }
        out.label(ui, "这次实际结果");
        for actual in &conflict.actual {
            show_observation(ui, out, actual, spec);
        }
    }
}
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn choice_name(choice: DecisionChoice) -> &'static str {
    match choice {
        DecisionChoice::Adopt => "采用结果",
        DecisionChoice::Both => "两种都需要",
        DecisionChoice::Neither => "都不合适",
        DecisionChoice::Defer => "以后再决定",
    }
}
fn scope_name(scope: ScopeKind) -> &'static str {
    match scope {
        ScopeKind::SingleRecord => "只改这一笔",
        ScopeKind::FutureRecords => "只用于以后新建的记录",
        ScopeKind::IncompleteAndFuture => "当前未完成和以后新建的记录",
        ScopeKind::AllExistingAndFuture => "包括历史和以后新建的记录",
    }
}
fn rule_name(rule: RuleKey) -> &'static str {
    match rule {
        RuleKey::Timer => "计时",
        RuleKey::DeliveryTarget => "交付目标",
        RuleKey::Reminder => "提醒",
    }
}
