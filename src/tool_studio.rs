//! Repository-independent application controller. All business writes pass through
//! the project store; rule writes additionally require genuine decision previews.
//! Bounded local work is synchronous, so no detached task can publish into a newer
//! screen. Request identities still fence retries, duplicate frames and sessions.
use crate::tool_decisions::{
    self as decisions, ChangePreview, ChangeRequest, PreparedChange, RehearsalCase,
    RehearsalTarget, WithdrawalPreview,
};
use crate::tool_project::*;
use crate::tool_proposals::{self as proposals, PreparedProposalChange, TaskIdentity};
use crate::tool_runtime;
use crate::tool_store::{ProjectStore, StoreError, StoreLoad};
use crate::tool_workspace_protocol::*;
use crate::ui::tool_workspace::{ToolWorkspace, WorkspaceOutput};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
fn identity(kind: &str) -> String {
    // Process identity, nanosecond timestamp and sequence also distinguish two
    // instances creating a project in the same wall-clock tick.
    format!(
        "{kind}-{:x}-{:x}-{:x}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}
fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn initial_policy() -> BehaviorPolicy {
    BehaviorPolicy {
        reminder: ReminderPolicy::WaitingBeforeDue { days_before_due: 1 },
        ..BehaviorPolicy::default()
    }
}

#[derive(Clone)]
enum Prepared {
    Record {
        operation_id: String,
        snapshot: ProjectSnapshot,
    },
    Native(PreparedChange),
    Proposal(PreparedProposalChange),
}
impl Prepared {
    fn snapshot(&self) -> &ProjectSnapshot {
        match self {
            Self::Record { snapshot, .. } => snapshot,
            Self::Native(p) => p.snapshot(),
            Self::Proposal(p) => p.snapshot(),
        }
    }
    fn expected_generation(&self) -> u64 {
        match self {
            Self::Record { snapshot, .. } => snapshot.generation,
            Self::Native(p) => p.expected_generation(),
            Self::Proposal(p) => p.expected_generation(),
        }
    }
    fn operation_id(&self) -> &str {
        match self {
            Self::Record { operation_id, .. } => operation_id,
            Self::Native(p) => p.operation_id(),
            Self::Proposal(p) => p.operation_id(),
        }
    }
}
struct Rehearsal {
    view: RehearsalView,
    input: RuleInput,
    now: DateTime<FixedOffset>,
    clock_date: NaiveDate,
    comparison_only: bool,
    candidates: Vec<(ChangeRequest, ChangePreview)>,
}
struct Withdrawal {
    view: WithdrawalView,
    preview: WithdrawalPreview,
    now: DateTime<FixedOffset>,
    date: NaiveDate,
}
struct Pending {
    request: WorkspaceRequest,
    prepared: Prepared,
}
struct PendingCreate {
    request: WorkspaceRequest,
    initial: ProjectSnapshot,
    location: String,
}

pub struct ToolStudio {
    workspace: ToolWorkspace,
    session: String,
    next_operation: String,
    now: DateTime<FixedOffset>,
    fixed_clock: bool,
    store: Option<ProjectStore>,
    snapshot: Option<ProjectSnapshot>,
    model: Option<ToolViewModel>,
    policies: BTreeMap<String, BehaviorPolicy>,
    record_specs: BTreeMap<String, ToolSpec>,
    decision_requests: BTreeMap<String, String>,
    location: Option<String>,
    diagnostic: Option<String>,
    creation_notice: Option<String>,
    example: Option<tempfile::TempDir>,
    operation: Option<OperationStatus>,
    delivery: u64,
    input_epoch: u64,
    completed: BTreeMap<String, (WorkspaceRequest, OperationOutcome)>,
    pending: Option<Pending>,
    pending_create: Option<PendingCreate>,
    rehearsal: Option<Rehearsal>,
    withdrawal: Option<Withdrawal>,
    exchange: Exchange,
    association_check: Option<(String, u64)>,
    association_report: Vec<String>,
    #[cfg(test)]
    fault: Option<crate::tool_store::StoreFaultPoint>,
    #[cfg(test)]
    io_fault: Option<std::io::ErrorKind>,
    #[cfg(test)]
    lose_create_ack: bool,
}
impl Default for ToolStudio {
    fn default() -> Self {
        Self::new()
    }
}
impl ToolStudio {
    pub fn new() -> Self {
        Self::construct(Local::now().fixed_offset(), false)
    }
    fn construct(now: DateTime<FixedOffset>, fixed_clock: bool) -> Self {
        Self {
            workspace: ToolWorkspace::default(),
            session: identity("session"),
            next_operation: identity("operation"),
            now,
            fixed_clock,
            store: None,
            snapshot: None,
            model: None,
            policies: BTreeMap::new(),
            record_specs: BTreeMap::new(),
            decision_requests: BTreeMap::new(),
            location: None,
            diagnostic: None,
            creation_notice: None,
            example: None,
            operation: None,
            delivery: 0,
            input_epoch: 0,
            completed: BTreeMap::new(),
            pending: None,
            pending_create: None,
            rehearsal: None,
            withdrawal: None,
            exchange: Exchange::default(),
            association_check: None,
            association_report: vec![],
            #[cfg(test)]
            fault: None,
            #[cfg(test)]
            io_fault: None,
            #[cfg(test)]
            lose_create_ack: false,
        }
    }
    #[cfg(test)]
    pub fn with_clock(now: DateTime<FixedOffset>) -> Self {
        Self::construct(now, true)
    }
    #[cfg(test)]
    pub fn set_clock(&mut self, now: DateTime<FixedOffset>) {
        self.now = now;
        let _ = self.rebuild();
    }
    #[cfg(test)]
    pub fn fail_next_commit(&mut self, fault: crate::tool_store::StoreFaultPoint) {
        self.fault = Some(fault);
    }
    #[cfg(test)]
    pub fn fail_next_commit_io(&mut self, kind: std::io::ErrorKind) {
        self.io_fault = Some(kind);
    }
    #[cfg(test)]
    pub fn lose_next_create_ack(&mut self) {
        self.lose_create_ack = true;
    }
    #[cfg(test)]
    pub fn prepare_synthetic_export_for_test(
        &mut self,
        record: &str,
        scope: ScopeKind,
    ) -> Result<Vec<u8>, String> {
        self.exchange.record = record.into();
        self.exchange.scope = scope;
        self.exchange.request = "已检查的虚构需求".into();
        self.synthetic_export(&|_| None)
    }
    pub fn snapshot(&self) -> Option<&ProjectSnapshot> {
        self.snapshot.as_ref()
    }
    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }
    fn date(&self) -> NaiveDate {
        self.snapshot
            .as_ref()
            .and_then(|s| FixedOffset::east_opt(s.date_settings.calendar_utc_offset_seconds))
            .map(|o| self.now.with_timezone(&o).date_naive())
            .unwrap_or(self.now.date_naive())
    }
    pub fn workspace_view(&self) -> WorkspaceView<'_> {
        WorkspaceView {
            session_id: &self.session,
            next_operation_id: &self.next_operation,
            project: self.model.as_ref(),
            now: self.now,
            as_of_date: self.date(),
            location: self.location.as_deref(),
            is_example: self.example.is_some(),
            decision_requests: &self.decision_requests,
            record_policies: &self.policies,
            record_specs: &self.record_specs,
            record_history: self
                .snapshot
                .as_ref()
                .map(|s| s.event_history.as_slice())
                .unwrap_or(&[]),
            operation: self.operation.as_ref(),
            rehearsal: self.rehearsal.as_ref().map(|r| &r.view),
            withdrawal: self.withdrawal.as_ref().map(|w| &w.view),
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        tasks: &[(String, String)],
        resolve_task: &dyn Fn(&str) -> Option<TaskIdentity>,
    ) {
        if !self.fixed_clock && self.example.is_none() {
            let previous = self.date();
            self.now = Local::now().fixed_offset();
            if previous != self.date() {
                self.invalidate_previews();
                if let Err(error) = self.rebuild() {
                    self.diagnostic = Some(error);
                }
            }
        }
        let identity = self
            .snapshot
            .as_ref()
            .map(|s| (self.session.clone(), s.generation));
        if identity != self.association_check {
            self.association_report = self.proposal_associations(resolve_task).into_iter().filter_map(|(id,result)|match result {
                Ok(a) if matches!(a.task_status, proposals::TaskAssociationStatus::Missing|proposals::TaskAssociationStatus::Changed) => Some(format!("提案 {id} 的任务关联已失效：{:?}。既有规则仍生效，本地工作可以继续；不能把旧来源关联当成当前验证。",a.task_status)),
                Err(e)=>Some(format!("提案 {id} 的来源记录无法验证：{e}。本地工作可以继续。")), _=>None,
            }).collect();
            self.association_check = identity;
        }
        for report in &self.association_report {
            ui.colored_label(egui::Color32::YELLOW, report);
        }
        if self.pending_create.is_some() {
            ui.label(
                "创建结果尚未确认。可重试同一次创建，或结束重试并核对原目录，再修改保存位置。",
            );
            if ui.button("结束创建重试并核对原目录（保留输入）").clicked() {
                if let Err(error) = self.recover_creation() {
                    self.creation_notice = Some(error);
                }
            }
        }
        if let Some(notice) = &self.creation_notice {
            ui.label(notice);
        }
        if self.pending.is_some() {
            ui.label("上一次保存尚待确认。重试会使用同一份候选；重新读取不会撤销已提交的内容。");
            if ui.button("结束重试并重新读取已保存项目").clicked() {
                if let Err(error) = self.reopen_saved() {
                    self.diagnostic = Some(error);
                }
            }
        }
        if let Some(diagnostic) = self.diagnostic() {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("只读诊断 / 未保存：{diagnostic}"),
            );
            ui.label(
                "原始文件保留。请复制整个项目目录作为备份后检查，不能把读取失败当成空项目保存。",
            );
        }
        if let Some(location) = &self.location {
            ui.label(format!("本地位置：{location}"));
        }
        if let Some(snapshot) = &self.snapshot {
            let offset = FixedOffset::east_opt(snapshot.date_settings.calendar_utc_offset_seconds)
                .map(|offset| offset.to_string())
                .unwrap_or_else(|| "无法确定".into());
            ui.label(format!(
                "项目日历：UTC{offset} · 当前日期 {} · 按日历日计算",
                self.date()
            ));
            self.show_exchange(ui, tasks, resolve_task);
            ui.separator();
        }
        let mut workspace = std::mem::take(&mut self.workspace);
        let WorkspaceOutput { request, .. } = workspace.show(ui, &self.workspace_view());
        self.workspace = workspace;
        if let Some(request) = request {
            self.handle(request);
            ui.ctx().request_repaint();
        }
    }
    fn publish(&mut self, context: &RequestContext, outcome: OperationOutcome) -> OperationOutcome {
        self.delivery += 1;
        self.operation = Some(OperationStatus {
            delivery_id: self.delivery,
            context: context.clone(),
            outcome: outcome.clone(),
        });
        outcome
    }
    fn validate_context(&self, request: &WorkspaceRequest) -> Result<(), String> {
        let c = &request.context;
        if c.session_id != self.session
            || c.project_id != self.snapshot.as_ref().map(|s| s.project_id.clone())
            || c.generation != self.snapshot.as_ref().map(|s| s.generation)
            || c.input_epoch < self.input_epoch
        {
            return Err("页面或项目已变化，请重新打开当前内容".into());
        }
        let revision = c.record_id.as_ref().and_then(|id| {
            self.snapshot
                .as_ref()?
                .records
                .iter()
                .find(|r| &r.record_id == id)
                .map(|r| r.record_revision)
        });
        if revision != c.record_revision || (c.record_id.is_some() && revision.is_none()) {
            return Err("记录已变化，请重新读取并演练".into());
        }
        if c.request_id != self.next_operation {
            return Err("操作身份已过期，请重新发起".into());
        }
        Ok(())
    }
    pub fn handle(&mut self, request: WorkspaceRequest) -> OperationOutcome {
        // Failed writes retain the original request AND prepared bytes. A retry
        // is never rebuilt using a later clock, model or operation identity.
        if let Some(pending) = &self.pending {
            if pending.request == request {
                return self.retry_local();
            }
            return self.publish(
                &request.context,
                OperationOutcome::Rejected("请先确认上一次保存结果，不能提交另一笔操作".into()),
            );
        }
        if let Some(pending) = &self.pending_create {
            if pending.request == request {
                return self.retry_create();
            }
            return self.publish(
                &request.context,
                OperationOutcome::Rejected("请先重试上一次创建操作".into()),
            );
        }
        if let Some((old, outcome)) = self.completed.get(&request.context.request_id) {
            let outcome = if old == &request {
                outcome.clone()
            } else {
                OperationOutcome::Rejected("相同操作身份不能用于不同内容".into())
            };
            return self.publish(&request.context, outcome);
        }
        if let Err(error) = self.validate_context(&request) {
            return self.publish(&request.context, OperationOutcome::Rejected(error));
        }
        self.input_epoch = request.context.input_epoch;
        self.next_operation = identity("operation");
        self.publish(&request.context, OperationOutcome::Running);
        let outcome = match self.execute(&request) {
            Ok(outcome) => outcome,
            Err(error) => OperationOutcome::Rejected(error),
        };
        if !matches!(outcome, OperationOutcome::Failed(_)) {
            self.completed.insert(
                request.context.request_id.clone(),
                (request.clone(), outcome.clone()),
            );
        }
        self.publish(&request.context, outcome)
    }
    fn execute(&mut self, request: &WorkspaceRequest) -> Result<OperationOutcome, String> {
        match &request.action {
            WorkspaceAction::CreateProject {
                name,
                location,
                spec,
            } => {
                let initial = ProjectSnapshot::new(
                    identity("project"),
                    name.clone(),
                    self.now,
                    ProjectDateSettings {
                        calendar_utc_offset_seconds: self.now.offset().local_minus_utc(),
                    },
                    spec.clone(),
                    initial_policy(),
                )
                .map_err(message)?;
                let checked_location = self.safe_location(location)?;
                self.ensure_default_parent(location, &checked_location)?;
                let location = checked_location;
                self.pending_create = Some(PendingCreate {
                    request: request.clone(),
                    initial,
                    location: location.display().to_string(),
                });
                Ok(self.retry_create())
            }
            WorkspaceAction::OpenProject { location } => {
                self.open(location)?;
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::CloseProject => {
                self.clear_project();
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::StartExample => {
                self.start_example()?;
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::Configure { name, spec } => {
                let source = self.writable()?;
                let old = source.active_spec().ok_or("缺少工具规范")?;
                validate_configuration(old, spec)?;
                let mut candidate = source.clone();
                candidate.project_name = name.clone();
                candidate.updated_at = self.now;
                candidate.active_spec = SpecReference::from(spec);
                candidate.spec_revisions.push(spec.clone());
                candidate.validate().map_err(message)?;
                self.pending = Some(Pending {
                    request: request.clone(),
                    prepared: Prepared::Record {
                        operation_id: request.context.request_id.clone(),
                        snapshot: candidate,
                    },
                });
                Ok(self.retry_local())
            }
            WorkspaceAction::Record(command) => {
                let source = self.writable()?;
                let (operation, generation) = match command {
                    ToolCommand::CreateRecord {
                        operation_id,
                        expected_generation,
                        ..
                    }
                    | ToolCommand::EditRecord {
                        operation_id,
                        expected_generation,
                        ..
                    }
                    | ToolCommand::TransitionStage {
                        operation_id,
                        expected_generation,
                        ..
                    } => (operation_id, *expected_generation),
                    ToolCommand::SetBehavior { .. } => {
                        return Err("规则变更必须先演练并保存决定".into())
                    }
                };
                if operation != &request.context.request_id
                    || Some(generation) != request.context.generation
                {
                    return Err("记录操作身份不匹配".into());
                }
                let candidate =
                    tool_runtime::apply_command(source, command, self.now).map_err(message)?;
                self.pending = Some(Pending {
                    request: request.clone(),
                    prepared: Prepared::Record {
                        operation_id: operation.clone(),
                        snapshot: candidate,
                    },
                });
                Ok(self.retry_local())
            }
            WorkspaceAction::Compare { input } => {
                if input.scope != ScopeKind::SingleRecord || !input.supersedes.is_empty() {
                    return Err(
                        "首次比较只演练这一笔记录，不替代任何决定；选定结果后再确认范围".into(),
                    );
                }
                self.rehearse(request, input, true)?;
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::Rehearse { input } => {
                self.rehearse(request, input, false)?;
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::SaveDecision {
                choice,
                input,
                preview_id,
                candidate_id,
            } => {
                let source = self.writable()?;
                let prepared = if *choice == DecisionChoice::Adopt {
                    let rehearsal = self.rehearsal.as_ref().ok_or("请先演练")?;
                    if rehearsal.comparison_only
                        || preview_id.as_ref() != Some(&rehearsal.view.preview_id)
                        || &rehearsal.input != input
                        || !same_input_context(&rehearsal.view.context, &request.context)
                        || self.date() != rehearsal.clock_date
                        || rehearsal.now > self.now
                    {
                        return Err("演练证据已过期，请重新演练".into());
                    }
                    let index = rehearsal
                        .view
                        .candidates
                        .iter()
                        .position(|c| Some(&c.candidate_id) == candidate_id.as_ref())
                        .ok_or("候选身份不匹配")?;
                    let (native_request, preview) = &rehearsal.candidates[index];
                    decisions::prepare_adoption(
                        source,
                        native_request,
                        preview,
                        &native_request.request_id,
                        input.as_of_date,
                        rehearsal.now,
                    )
                    .map_err(message)?
                } else {
                    let mut input = input.clone();
                    input.supersedes.clear();
                    if input.rationale.trim().is_empty() {
                        input.rationale = match choice {
                            DecisionChoice::Both => {
                                "System audit: no reason supplied; user selected both needed"
                            }
                            DecisionChoice::Neither => {
                                "System audit: no reason supplied; user selected neither fits"
                            }
                            DecisionChoice::Defer => {
                                "System audit: no reason supplied; user deferred this decision"
                            }
                            DecisionChoice::Adopt => unreachable!(),
                        }
                        .into();
                    }
                    let native = self.change_request(request, &input, 0, self.now, false)?;
                    decisions::prepare_unresolved(source, &native, *choice, self.now)
                        .map_err(message)?
                };
                self.pending = Some(Pending {
                    request: request.clone(),
                    prepared: Prepared::Native(prepared),
                });
                Ok(self.retry_local())
            }
            WorkspaceAction::PreviewWithdrawal { decision_id } => {
                let source = self.writable()?;
                let decision = source
                    .decisions
                    .iter()
                    .find(|d| &d.decision_id == decision_id)
                    .ok_or("决定不存在")?;
                let revision = decisions::decision_behavior_revision_id(decision)
                    .map_err(message)?
                    .ok_or("这个决定没有已采用的规则")?;
                let preview = decisions::rehearse_withdrawal(
                    source,
                    &revision,
                    &identity("withdraw"),
                    &request.context.request_id,
                    self.date(),
                    self.now,
                )
                .map_err(message)?;
                let view = WithdrawalView {
                    context: request.context.clone(),
                    preview_id: identity("preview"),
                    decision_id: decision_id.clone(),
                    fresh: true,
                    ready_to_commit: preview.conflicts.is_empty(),
                    comparisons: comparisons(&preview.impacts),
                    preserved_record_count: preview.preserved_record_count,
                    preserved_event_count: preview.preserved_event_count,
                    retained_bindings: preview
                        .preserved_later_bindings
                        .iter()
                        .map(|b| RetainedBindingView {
                            record_id: b.record_id.clone(),
                            rule_key: b.rule_key,
                            behavior_revision_id: b.behavior_revision_id.clone(),
                        })
                        .collect(),
                    conflicts: conflicts(&preview.conflicts),
                };
                self.withdrawal = Some(Withdrawal {
                    view,
                    preview,
                    now: self.now,
                    date: self.date(),
                });
                Ok(OperationOutcome::PreviewReady)
            }
            WorkspaceAction::ConfirmWithdrawal { preview_id } => {
                let source = self.writable()?;
                let w = self.withdrawal.as_ref().ok_or("请先预览撤回影响")?;
                if &w.view.preview_id != preview_id
                    || !same_input_context(&w.view.context, &request.context)
                    || self.date() != w.date
                {
                    return Err("撤回预览已过期，请重新预览".into());
                }
                let prepared = decisions::prepare_withdrawal(
                    source,
                    &w.preview,
                    &w.preview.request_id,
                    w.date,
                    w.now,
                )
                .map_err(message)?;
                self.pending = Some(Pending {
                    request: request.clone(),
                    prepared: Prepared::Native(prepared),
                });
                Ok(self.retry_local())
            }
        }
    }
    fn writable(&self) -> Result<&ProjectSnapshot, String> {
        if self.diagnostic.is_some() {
            return Err("项目处于只读诊断状态".into());
        }
        self.snapshot.as_ref().ok_or_else(|| "请先打开工具".into())
    }
    fn safe_location(&self, location: &str) -> Result<PathBuf, String> {
        let path = Path::new(location);
        if !path.is_absolute()
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err("请填写不含目录跳转的绝对路径".into());
        }
        let requested_parent = path.parent().ok_or("请选择新的项目目录名称")?;
        let allow_missing = default_tool_data_directory().as_deref() == Some(requested_parent);
        let mut parent = requested_parent;
        loop {
            match std::fs::symlink_metadata(parent) {
                Ok(metadata) if metadata.is_dir() || metadata.file_type().is_symlink() => break,
                Ok(_) => return Err("保存位置的父路径不是文件夹".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
                    parent = parent
                        .parent()
                        .ok_or("找不到可用的本地数据目录，请选择其他文件夹")?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err("请选择已经存在的父目录".into())
                }
                Err(error) => return Err(format!("无法检查保存位置，请改选文件夹：{error}")),
            }
        }
        for ancestor in parent.ancestors() {
            let metadata = std::fs::symlink_metadata(ancestor).map_err(message)?;
            if metadata.file_type().is_symlink() {
                return Err("保存位置不能包含符号链接".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                    != 0
                {
                    return Err("保存位置不能包含重解析点或目录链接".into());
                }
            }
        }
        let canonical_parent = std::fs::canonicalize(parent).map_err(message)?;
        // libgit2 reports NotFound even when an existing .git directory is
        // unreadable. That is not proof this location is outside a repository.
        // Inspect only the marker and its first directory entry, never its data.
        // An empty marker directory alone is not an initialized repository;
        // every nonempty or unreadable marker fails closed before any write.
        for ancestor in canonical_parent.ancestors() {
            let marker = ancestor.join(".git");
            match std::fs::symlink_metadata(&marker) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let first = std::fs::read_dir(&marker)
                        .map_err(|e| format!("无法确认 Git 仓库边界，请改选位置：{e}"))?
                        .next()
                        .transpose()
                        .map_err(message)?;
                    if first.is_some() {
                        return Err("请把工具保存在 Git 仓库以外的独立本地目录".into());
                    }
                }
                Ok(_) => return Err("保存位置位于 Git 元数据标记之内，请改选仓库外的目录".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法确认 Git 仓库边界，请改选位置：{error}")),
            }
        }
        match git2::Repository::discover(&canonical_parent) {
            Ok(_) => return Err("请把工具保存在 Git 仓库以外的独立本地目录".into()),
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(format!("无法确认 Git 仓库边界，请改选位置：{error}")),
        }
        path.file_name().ok_or("请选择新的项目目录名称")?;
        let destination = canonical_parent.join(path.strip_prefix(parent).map_err(message)?);
        let temporary = std::fs::canonicalize(std::env::temp_dir()).map_err(message)?;
        if !self.fixed_clock && destination.starts_with(temporary) {
            return Err("正式工具不能放在系统临时目录，请选择稳定的本地位置".into());
        }
        Ok(destination)
    }
    fn ensure_default_parent(&self, original: &str, location: &Path) -> Result<(), String> {
        let parent = location.parent().ok_or("保存位置没有父目录")?;
        if parent.is_dir() {
            return Ok(());
        }
        // Only the suggested application data hierarchy may be created for the
        // user. Arbitrary missing parents still need an explicit folder choice.
        let root = default_tool_data_directory().ok_or("没有可用的默认位置，请选择文件夹")?;
        if Path::new(original).parent() != Some(root.as_path()) {
            return Err("请选择已经存在的父目录".into());
        }
        create_private_data_directory(
            parent,
            #[cfg(test)]
            |_| {},
        )
        .map_err(|error| format!("无法创建本地保存文件夹，请改选位置：{error}"))?;
        self.safe_location(original)?;
        Ok(())
    }
    fn retry_create(&mut self) -> OperationOutcome {
        let pending = self.pending_create.as_ref().expect("retained create");
        let context = pending.request.context.clone();
        let result = ProjectStore::create(&pending.location, &pending.initial, &context.request_id);
        #[cfg(test)]
        if result.is_ok() && std::mem::take(&mut self.lose_create_ack) {
            return self.publish(
                &context,
                OperationOutcome::Failed("创建已发布，但确认回应丢失".into()),
            );
        }
        let outcome = match result {
            Ok(store) => match store.load() {
                Ok(StoreLoad::Writable(snapshot)) => {
                    let location = pending.location.clone();
                    self.pending_create = None;
                    self.install(store, snapshot, location);
                    OperationOutcome::PreviewReady
                }
                Ok(StoreLoad::ReadOnly(project)) => {
                    OperationOutcome::Failed(format!("创建结果需要检查：{:?}", project.reason))
                }
                Err(error) => OperationOutcome::Failed(error.to_string()),
            },
            Err(error) => {
                let outcome = store_outcome(&error);
                if matches!(outcome, OperationOutcome::Rejected(_)) {
                    self.pending_create = None;
                }
                outcome
            }
        };
        self.publish(&context, outcome)
    }
    fn retry_local(&mut self) -> OperationOutcome {
        self.commit_pending(None)
    }
    fn commit_pending(&mut self, task: Option<&TaskIdentity>) -> OperationOutcome {
        let pending = self.pending.as_ref().expect("exact pending candidate");
        let context = pending.request.context.clone();
        let original_request = pending.request.clone();
        if let Prepared::Proposal(p) = &pending.prepared {
            if let Err(error) = p.validate_for_commit(task) {
                return self.publish(
                    &context,
                    OperationOutcome::Failed(format!(
                        "提案来源关联已变化，保留原操作等待核对：{error}"
                    )),
                );
            }
        }
        let store = self.store.as_ref().expect("pending save has a store");
        let p = &pending.prepared;
        #[cfg(test)]
        let result = if let Some(kind) = self.io_fault.take() {
            Err(StoreError::Io(std::io::Error::from(kind)))
        } else if let Some(fault) = self.fault.take() {
            store.commit_with_fault(
                p.expected_generation(),
                p.operation_id(),
                p.snapshot(),
                fault,
            )
        } else {
            store.commit(p.expected_generation(), p.operation_id(), p.snapshot())
        };
        #[cfg(not(test))]
        let result = store.commit(p.expected_generation(), p.operation_id(), p.snapshot());
        let outcome = match result {
            Ok(result) => {
                self.snapshot = Some(result.snapshot);
                self.pending = None;
                self.invalidate_previews();
                if let Err(error) = self.rebuild() {
                    self.diagnostic = Some(error);
                }
                OperationOutcome::Committed
            }
            Err(error) => {
                let outcome = store_outcome(&error);
                if matches!(error, StoreError::GenerationConflict { .. }) {
                    self.pending = None;
                    let _ = self.reload();
                } else if matches!(outcome, OperationOutcome::Rejected(_)) {
                    self.pending = None;
                }
                outcome
            }
        };
        if matches!(outcome, OperationOutcome::Committed) {
            // Preserve duplicate-frame identity even after the original call returns.
            self.completed.insert(
                context.request_id.clone(),
                (original_request, outcome.clone()),
            );
        }
        self.publish(&context, outcome)
    }
    /// The user explicitly ends a creation retry. Probe the original destination
    /// without writing it. A matching persisted creation is opened; otherwise
    /// preserve the directory and all form inputs, without claiming cancellation
    /// or a definitely-uncommitted outcome when it could not be read.
    pub fn recover_creation(&mut self) -> Result<(), String> {
        let pending = self.pending_create.as_ref().ok_or("没有待确认的创建操作")?;
        let location = pending.location.clone();
        let context = pending.request.context.clone();
        let original = (
            pending.initial.project_id.clone(),
            pending.request.context.request_id.clone(),
        );
        let opened = ProjectStore::open(&location)
            .and_then(|store| store.load().map(|loaded| (store, loaded)));
        if let Ok((store, StoreLoad::Writable(snapshot))) = &opened {
            if snapshot.project_id == original.0
                && snapshot.operation_receipts.contains_key(&original.1)
            {
                self.install(store.clone(), snapshot.clone(), location);
                self.publish(&context, OperationOutcome::PreviewReady);
                self.creation_notice = Some(
                    "发现同一次创建的已保存项目，已从磁盘打开；请核对内容。没有重复创建。".into(),
                );
                return Ok(());
            }
        }
        let notice=match opened {
            Err(StoreError::Io(error)) if error.kind()==std::io::ErrorKind::NotFound => "原位置未找到已提交项目。已结束重试，创建输入仍保留；可重新选择保存位置。".into(),
            Err(error)=>format!("原目录暂时无法确认：{error}。已结束重试，不会再次写入原目录；输入保留，可改选位置。上一次可能已保存，请之后核对原目录。"),
            Ok((_,StoreLoad::ReadOnly(project)))=>format!("原目录需要只读检查：{:?}。已结束重试且保留原文件和输入；可选择其他位置，不能把原目录当作空白覆盖。",project.reason),
            Ok(_)=>"原目录已有其他项目。已结束重试，保留原文件和输入；请选择其他位置。".into(),
        };
        self.pending_create = None;
        self.session = identity("session");
        self.next_operation = identity("operation");
        self.input_epoch = 0;
        self.operation = None;
        self.completed.clear();
        self.creation_notice = Some(notice);
        Ok(())
    }
    /// Explicit recovery action: only abandon an uncertain retry after a full
    /// store read succeeds. The loaded history is authoritative, not a promise
    /// that the previous write was canceled or committed.
    pub fn reopen_saved(&mut self) -> Result<(), String> {
        let store = self.store.as_ref().ok_or("工具未打开")?.clone();
        let location = self.location.clone().ok_or("保存位置缺失")?;
        let loaded = store.load().map_err(message)?;
        let example = self.example.take();
        match loaded {
            StoreLoad::Writable(snapshot) => self.install(store, snapshot, location),
            StoreLoad::ReadOnly(project) => {
                self.clear_project();
                self.store = Some(store);
                self.location = Some(location);
                self.diagnostic = Some(format!("{:?}", project.reason));
            }
        }
        self.example = example;
        Ok(())
    }
    fn open(&mut self, location: &str) -> Result<(), String> {
        let store = ProjectStore::open(location).map_err(message)?;
        match store.load().map_err(message)? {
            StoreLoad::Writable(snapshot) => self.install(store, snapshot, location.into()),
            StoreLoad::ReadOnly(project) => {
                self.clear_project();
                self.store = Some(store);
                self.location = Some(location.into());
                self.diagnostic = Some(format!("{:?}", project.reason));
            }
        }
        Ok(())
    }
    fn install(&mut self, store: ProjectStore, snapshot: ProjectSnapshot, location: String) {
        self.clear_project();
        self.store = Some(store);
        self.snapshot = Some(snapshot);
        self.location = Some(location);
        if let Err(error) = self.rebuild() {
            self.diagnostic = Some(error);
        }
    }
    fn clear_project(&mut self) {
        self.store = None;
        self.snapshot = None;
        self.model = None;
        self.location = None;
        self.diagnostic = None;
        self.creation_notice = None;
        self.example = None;
        self.policies.clear();
        self.record_specs.clear();
        self.decision_requests.clear();
        self.pending = None;
        self.pending_create = None;
        self.rehearsal = None;
        self.withdrawal = None;
        self.operation = None;
        self.exchange = Exchange::default();
        self.association_check = None;
        self.association_report.clear();
        self.session = identity("session");
        self.next_operation = identity("operation");
        self.input_epoch = 0;
        self.completed.clear();
    }
    fn reload(&mut self) -> Result<(), String> {
        match self
            .store
            .as_ref()
            .ok_or("工具未打开")?
            .load()
            .map_err(message)?
        {
            StoreLoad::Writable(snapshot) => {
                self.snapshot = Some(snapshot);
                self.invalidate_previews();
                self.rebuild()
            }
            StoreLoad::ReadOnly(project) => {
                self.diagnostic = Some(format!("{:?}", project.reason));
                if let Some(model) = &mut self.model {
                    model.access = ProjectAccess::ReadOnly;
                }
                Err("项目文件需要检查，已停止写入".into())
            }
        }
    }
    fn rebuild(&mut self) -> Result<(), String> {
        let Some(snapshot) = &self.snapshot else {
            return Ok(());
        };
        let mut records = Vec::new();
        let mut policies = BTreeMap::new();
        for record in &snapshot.records {
            let derived =
                decisions::evaluate_bound_record(snapshot, &record.record_id, self.date())
                    .map_err(message)?;
            policies.insert(
                record.record_id.clone(),
                decisions::policy_for_record(snapshot, &record.record_id).map_err(message)?,
            );
            records.push(ToolRecordView {
                record_id: record.record_id.clone(),
                record_revision: record.record_revision,
                values: record.typed_values.clone(),
                current_stage_id: derived.current_stage_id.clone(),
                derived: EvidenceObservation::from(&derived),
            });
        }
        self.decision_requests = snapshot
            .decisions
            .iter()
            .filter_map(|d| {
                decisions::decision_original_request(d)
                    .ok()
                    .map(|words| (d.decision_id.clone(), words))
            })
            .collect();
        self.policies = policies;
        self.record_specs = snapshot
            .records
            .iter()
            .filter_map(|r| {
                snapshot
                    .spec_revisions
                    .iter()
                    .find(|s| s.spec_id == r.spec.spec_id && s.revision == r.spec.revision)
                    .cloned()
                    .map(|spec| (r.record_id.clone(), spec))
            })
            .collect();
        self.model = Some(ToolViewModel {
            project_id: snapshot.project_id.clone(),
            project_name: snapshot.project_name.clone(),
            generation: snapshot.generation,
            access: if self.diagnostic.is_some() {
                ProjectAccess::ReadOnly
            } else {
                ProjectAccess::Writable
            },
            active_spec: snapshot.active_spec().ok_or("缺少当前规范")?.clone(),
            active_behavior: decisions::policy_for_future(snapshot).map_err(message)?,
            records,
            selected_record: None,
            decisions: snapshot.decisions.clone(),
            scenarios: snapshot.scenarios.clone(),
            evidence: snapshot.evidence.clone(),
            proposals: snapshot.proposals.clone(),
            access_message: self.diagnostic.clone(),
            save_message: None,
        });
        Ok(())
    }
    fn invalidate_previews(&mut self) {
        if let Some(r) = &mut self.rehearsal {
            r.view.fresh = false;
        }
        if let Some(w) = &mut self.withdrawal {
            w.view.fresh = false;
        }
        self.exchange.replay = None;
    }
    fn change_request(
        &self,
        request: &WorkspaceRequest,
        input: &RuleInput,
        index: usize,
        now: DateTime<FixedOffset>,
        include_case: bool,
    ) -> Result<ChangeRequest, String> {
        let snapshot = self.writable()?;
        let record_id = request
            .context
            .record_id
            .as_deref()
            .ok_or("请选择用于演练的一笔记录")?;
        let candidate = input.candidates[index].clone();
        if !input.rule_keys.contains(&RuleKey::DeliveryTarget) {
            return Err("请选择明确的交付目标比较".into());
        }
        let scope =
            decisions::freeze_scope(snapshot, input.scope, Some(record_id)).map_err(message)?;
        let cases = if include_case {
            let mut scenario = decisions::scenario_from_snapshot(
                snapshot,
                &identity("scenario"),
                "用户选择的记录副本",
                input.as_of_date,
                now,
            )
            .map_err(message)?;
            let mut revision = request.context.record_revision.ok_or("记录修订缺失")?;
            if !input.record_changes.is_empty() {
                scenario.steps.push(ScenarioStep::EditRecord {
                    operation_id: identity("copy-edit"),
                    record_id: record_id.into(),
                    expected_record_revision: revision,
                    changes: input.record_changes.clone(),
                    occurred_at: now,
                });
                revision += 1;
            }
            if let Some(stage) = &input.stage_override {
                scenario.steps.push(ScenarioStep::TransitionStage {
                    operation_id: identity("copy-stage"),
                    record_id: record_id.into(),
                    expected_record_revision: revision,
                    to_stage_id: stage.clone(),
                    occurred_at: now,
                });
            }
            scenario.steps.push(ScenarioStep::Observe {
                record_id: record_id.into(),
            });
            let target = if input.scope == ScopeKind::FutureRecords
                || !scope.frozen_record_ids.iter().any(|id| id == record_id)
            {
                RehearsalTarget::FutureRecord(record_id.into())
            } else {
                RehearsalTarget::Record(record_id.into())
            };
            vec![RehearsalCase { scenario, target }]
        } else {
            vec![]
        };
        Ok(ChangeRequest {
            request_id: request.context.request_id.clone(),
            operation_id: identity("adopt"),
            decision_id: identity("decision"),
            revision_id: identity("behavior"),
            rule_keys: input.rule_keys.clone(),
            candidate_policy: candidate,
            scope,
            original_request: input.original_request.clone(),
            rationale: if input.rationale.trim().is_empty() {
                "System audit: no reason supplied; user selected this outcome".into()
            } else {
                input.rationale.clone()
            },
            unresolved_questions: input.unresolved_questions.clone(),
            supersedes: input.supersedes.clone(),
            cases,
        })
    }
    fn rehearse(
        &mut self,
        request: &WorkspaceRequest,
        input: &RuleInput,
        comparison_only: bool,
    ) -> Result<(), String> {
        let mut candidates = Vec::new();
        let mut displays = Vec::new();
        // Future what-if dates use an explicitly advanced copied clock; real
        // records and the controller's daily clock are unchanged.
        let offset =
            FixedOffset::east_opt(self.writable()?.date_settings.calendar_utc_offset_seconds)
                .ok_or("无效日期设置")?;
        let now = self.now.max(
            offset
                .from_local_datetime(
                    &input
                        .as_of_date
                        .and_time(self.now.with_timezone(&offset).time()),
                )
                .single()
                .ok_or("无效日期")?,
        );
        for index in 0..2 {
            let mut native_request = self.change_request(request, input, index, now, true)?;
            if comparison_only {
                native_request.rationale = "System-authored preview: copied record comparison; no decision or reason supplied".into();
            }
            let preview = decisions::rehearse_change(
                self.writable()?,
                &native_request,
                input.as_of_date,
                now,
            )
            .map_err(message)?;
            let mut display = candidate_view(
                &identity("candidate"),
                if index == 0 {
                    "方案 A · 保留原承诺日期"
                } else {
                    "方案 B · 显示目标按等待天数顺延"
                },
                &native_request,
                &preview,
            );
            if comparison_only {
                display.ready_to_adopt = false;
            }
            if now > self.now {
                display.ready_to_adopt = false;
                display
                    .label
                    .push_str("（未来日期假设，仅供比较；请用当前日期重新演练后采用）");
            }
            displays.push(display);
            candidates.push((native_request, preview));
        }
        self.rehearsal = Some(Rehearsal {
            view: RehearsalView {
                context: request.context.clone(),
                preview_id: identity("preview"),
                fresh: true,
                scope: candidates[0].0.scope.clone(),
                candidates: displays,
            },
            input: input.clone(),
            now,
            clock_date: self.date(),
            comparison_only,
            candidates,
        });
        Ok(())
    }
    fn start_example(&mut self) -> Result<(), String> {
        let dir = tempfile::Builder::new()
            .prefix("gitmanager-fictional-example-")
            .tempdir()
            .map_err(message)?;
        let fixed = FixedOffset::east_opt(8 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 5, 12, 0, 0)
            .single()
            .unwrap();
        let snapshot = example_snapshot(fixed)?;
        let location = dir.path().join("fictional");
        let store = ProjectStore::create(&location, &snapshot, &identity("example-create"))
            .map_err(message)?;
        let StoreLoad::Writable(snapshot) = store.load().map_err(message)? else {
            return Err("无法打开隔离示例".into());
        };
        self.now = fixed;
        self.install(store, snapshot, location.display().to_string());
        self.example = Some(dir);
        Ok(())
    }
}
fn same_input_context(preview: &RequestContext, current: &RequestContext) -> bool {
    preview.session_id == current.session_id
        && preview.project_id == current.project_id
        && preview.generation == current.generation
        && preview.record_id == current.record_id
        && preview.record_revision == current.record_revision
        && preview.input_epoch == current.input_epoch
}
fn store_outcome(error: &StoreError) -> OperationOutcome {
    match error {
        StoreError::Io(_) | StoreError::LockBusy => OperationOutcome::Failed(error.to_string()),
        #[cfg(test)]
        StoreError::InjectedInterruption(_) => OperationOutcome::Failed(error.to_string()),
        _ => OperationOutcome::Rejected(error.to_string()),
    }
}
fn comparisons(impacts: &[decisions::RecordImpact]) -> Vec<ObservationComparison> {
    impacts
        .iter()
        .map(|i| ObservationComparison {
            before: i.before.clone(),
            after: i.after.clone(),
        })
        .collect()
}
fn conflicts(items: &[decisions::DecisionConflict]) -> Vec<ConflictView> {
    items
        .iter()
        .map(|c| ConflictView {
            decision_id: c.decision_id.clone(),
            message: c.message.clone(),
            expected: c.expected.clone(),
            actual: c.actual.clone(),
        })
        .collect()
}
fn candidate_view(
    id: &str,
    label: &str,
    request: &ChangeRequest,
    preview: &ChangePreview,
) -> CandidateView {
    CandidateView {
        candidate_id: id.into(),
        label: label.into(),
        policy: request.candidate_policy.clone(),
        comparisons: comparisons(&preview.impacts),
        evidence: preview.evidence.clone(),
        conflicts: conflicts(&preview.conflicts),
        ready_to_adopt: preview.conflicts.is_empty()
            && !preview.evidence.is_empty()
            && preview.evidence.iter().all(|e| {
                e.source == EvidenceSource::NativeExecution
                    && e.status != EvidenceStatus::Failed
                    && e.errors.is_empty()
            }),
    }
}
fn validate_configuration(old: &ToolSpec, new: &ToolSpec) -> Result<(), String> {
    new.validate().map_err(message)?;
    if new.spec_id != old.spec_id
        || new.revision != old.revision.checked_add(1).ok_or("规范版本超出范围")?
        || new.entity_type != old.entity_type
        || new.template_id != old.template_id
        || new.default_stage_id != old.default_stage_id
        || new.extensions != old.extensions
    {
        return Err("首版只允许改显示名称和新增可选字段".into());
    }
    for field in &old.fields {
        let next = new.field(&field.id).ok_or("不能删除已有字段")?;
        let mut renamed = field.clone();
        renamed.display_name = next.display_name.clone();
        if &renamed != next {
            return Err("不能改变已有字段类型、角色或必填性".into());
        }
    }
    for field in new.fields.iter().filter(|f| old.field(&f.id).is_none()) {
        if field.required || field.role != FieldRole::Custom {
            return Err("新增字段必须是可选自定义字段".into());
        }
    }
    if old.stages.len() != new.stages.len() {
        return Err("首版不能增删工作阶段".into());
    }
    for stage in &old.stages {
        let next = new.stage(&stage.id).ok_or("不能删除阶段")?;
        let mut renamed = stage.clone();
        renamed.display_name = next.display_name.clone();
        if &renamed != next {
            return Err("计时规则必须通过演练修改".into());
        }
    }
    Ok(())
}
fn example_snapshot(now: DateTime<FixedOffset>) -> Result<ProjectSnapshot, String> {
    let at = |day| {
        now.offset()
            .with_ymd_and_hms(2026, 10, day, 9, 0, 0)
            .single()
            .unwrap()
    };
    let mut snapshot = ProjectSnapshot::new(
        identity("example"),
        "虚构工作室 · 体验".into(),
        at(1),
        ProjectDateSettings {
            calendar_utc_offset_seconds: 8 * 3600,
        },
        studio_order_template(),
        initial_policy(),
    )
    .map_err(message)?;
    snapshot = tool_runtime::apply_command(
        &snapshot,
        &ToolCommand::CreateRecord {
            operation_id: identity("example-record"),
            expected_generation: 0,
            record_id: "example-order".into(),
            initial_stage_id: "in_progress".into(),
            values: BTreeMap::from([
                (
                    "order_number".into(),
                    FieldValue::Text("虚构订单：手作台灯".into()),
                ),
                (
                    "work_description".into(),
                    FieldValue::Text("体验用例，没有真实客户资料".into()),
                ),
                ("started_on".into(), FieldValue::Date(at(1).date_naive())),
                ("promised_on".into(), FieldValue::Date(at(6).date_naive())),
            ]),
            occurred_at: at(1),
        },
        at(1),
    )
    .map_err(message)?;
    tool_runtime::apply_command(
        &snapshot,
        &ToolCommand::TransitionStage {
            operation_id: identity("example-wait"),
            expected_generation: 0,
            record_id: "example-order".into(),
            expected_record_revision: 1,
            to_stage_id: "waiting_materials".into(),
            occurred_at: at(2),
        },
        at(2),
    )
    .map_err(message)
}

struct ImportedReplay {
    session: String,
    selection: proposals::ReplaySelection,
    replay: proposals::ProposalReplay,
    now: DateTime<FixedOffset>,
    date: NaiveDate,
    input: (String, ScopeKind, Vec<String>),
}
struct Exchange {
    request: String,
    summaries: BTreeMap<String, String>,
    record: String,
    task: String,
    scope: ScopeKind,
    supersedes: Vec<String>,
    candidate: String,
    prepared_export: Option<proposals::RequirementExport>,
    requirements: Option<proposals::RequirementExport>,
    reviewed: bool,
    export_path: String,
    import_path: String,
    accepted: Option<proposals::AcceptedProposal>,
    replay: Option<ImportedReplay>,
    status: String,
    export_json: String,
}
impl Default for Exchange {
    fn default() -> Self {
        Self {
            request: String::new(),
            summaries: BTreeMap::new(),
            record: String::new(),
            task: String::new(),
            scope: ScopeKind::SingleRecord,
            supersedes: vec![],
            candidate: String::new(),
            prepared_export: None,
            requirements: None,
            reviewed: false,
            export_path: String::new(),
            import_path: String::new(),
            accepted: None,
            replay: None,
            status: String::new(),
            export_json: String::new(),
        }
    }
}
impl ToolStudio {
    /// Export construction accepts only the explicitly reviewed selection. No
    /// filesystem path, credential or unselected record is added by this method.
    pub fn prepare_export(
        &mut self,
        selection: proposals::ExportSelection,
    ) -> Result<Vec<u8>, String> {
        let export =
            proposals::export_requirements(self.writable()?, selection).map_err(message)?;
        let bytes = export.json().to_vec();
        self.exchange.prepared_export = Some(export);
        Ok(bytes)
    }
    pub fn write_export(&mut self, path: &Path) -> Result<(), String> {
        let export = self
            .exchange
            .prepared_export
            .as_ref()
            .ok_or("请先预览要导出的内容")?;
        // Never silently overwrite an existing file, follow a symlink or treat
        // mere preparation as a saved export. Existing files need another name.
        write_new_file(path, export.json())?;
        self.exchange.requirements = Some(export.clone());
        self.exchange.accepted = None;
        self.exchange.replay = None;
        Ok(())
    }
    pub fn import_proposal(
        &mut self,
        reader: impl std::io::Read,
        current_task: Option<&TaskIdentity>,
    ) -> Result<(), String> {
        let requirements = self.exchange.requirements.as_ref().ok_or(
            "本次打开工具后尚未导出需求包。请先重新导出，再导入匹配的提案；重启后不能直接导入旧包",
        )?;
        let accepted =
            proposals::import_proposal(reader, self.writable()?, requirements, current_task)
                .map_err(message)?;
        self.exchange.candidate = accepted
            .candidates()
            .first()
            .map(|c| c.candidate_id.clone())
            .unwrap_or_default();
        self.exchange.accepted = Some(accepted);
        self.exchange.replay = None;
        Ok(())
    }
    pub fn replay_imported(
        &mut self,
        candidate_id: &str,
        record: &str,
        scope: ScopeKind,
        supersedes: Vec<String>,
        current_task: Option<&TaskIdentity>,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("请先确认上一次保存结果".into());
        }
        let snapshot = self.writable()?;
        let accepted = self.exchange.accepted.as_ref().ok_or("请先导入提案")?;
        let selection = proposals::ReplaySelection {
            candidate_id: candidate_id.into(),
            request_id: identity("proposal-request"),
            operation_id: identity("proposal-adopt"),
            decision_id: identity("decision"),
            revision_id: identity("behavior"),
            scope: decisions::freeze_scope(snapshot, scope, Some(record)).map_err(message)?,
            supersedes: supersedes.clone(),
        };
        // An imported case has a fixed clock. Preserve it, but never allow its
        // future clock to timestamp a real commit in the future.
        let case = accepted
            .candidates()
            .iter()
            .find(|c| c.candidate_id == candidate_id)
            .and_then(|c| c.cases.first())
            .ok_or("提案候选没有演练例子")?;
        let now = case.scenario.fixed_now;
        let date = case.scenario.as_of_date;
        if now > self.now
            || self.date()
                != now
                    .with_timezone(
                        &FixedOffset::east_opt(snapshot.date_settings.calendar_utc_offset_seconds)
                            .ok_or("无效项目日期设置")?,
                    )
                    .date_naive()
        {
            return Err("提案时钟已过期或在未来，请重新导出当前需求".into());
        }
        let replay =
            proposals::rehearse_proposal(snapshot, accepted, &selection, current_task, date, now)
                .map_err(message)?;
        self.exchange.replay = Some(ImportedReplay {
            session: self.session.clone(),
            selection,
            replay,
            now,
            date: self.date(),
            input: (record.into(), scope, supersedes),
        });
        Ok(())
    }
    pub fn save_imported(
        &mut self,
        choice: DecisionChoice,
        current_task: Option<&TaskIdentity>,
    ) -> Result<OperationOutcome, String> {
        if self.pending.is_some() {
            return Ok(self.commit_pending(current_task));
        }
        let snapshot = self.writable()?;
        let accepted = self.exchange.accepted.as_ref().ok_or("提案已关闭")?;
        let replay = self.exchange.replay.as_ref().ok_or("请先本地演练")?;
        if replay.session != self.session || replay.date != self.date() {
            return Err("演练上下文已过期，请重新演练".into());
        }
        let prepared = if choice == DecisionChoice::Adopt {
            proposals::prepare_proposal_adoption(
                snapshot,
                accepted,
                &replay.replay,
                &replay.selection,
                current_task,
                replay.replay.preview().scenarios[0].as_of_date,
                replay.now,
            )
            .map_err(message)?
        } else {
            if !replay.selection.supersedes.is_empty() {
                return Err(
                    "暂缓、两种都需要或都不合适不能替代旧决定。请清除替代选择后重新演练".into(),
                );
            }
            proposals::prepare_proposal_unresolved(
                snapshot,
                accepted,
                &replay.replay,
                &replay.selection,
                current_task,
                choice,
                replay.now,
            )
            .map_err(message)?
        };
        let context = RequestContext {
            request_id: replay.selection.request_id.clone(),
            session_id: self.session.clone(),
            project_id: Some(snapshot.project_id.clone()),
            generation: Some(snapshot.generation),
            record_id: None,
            record_revision: None,
            input_epoch: self.input_epoch,
        };
        // This request is internal to the exchange UI; its exact identity remains
        // retained with the provenance wrapper until store acknowledgement.
        self.pending = Some(Pending {
            request: WorkspaceRequest {
                context,
                action: WorkspaceAction::CloseProject,
            },
            prepared: Prepared::Proposal(prepared),
        });
        Ok(self.commit_pending(current_task))
    }
    pub fn proposal_associations(
        &self,
        resolve_task: &dyn Fn(&str) -> Option<TaskIdentity>,
    ) -> Vec<(String, Result<proposals::SavedProposalAssociation, String>)> {
        let Some(snapshot) = self.snapshot() else {
            return vec![];
        };
        snapshot
            .decisions
            .iter()
            .filter(|d| d.extensions.contains_key(proposals::PROVENANCE_KEY))
            .map(|d| {
                let first = proposals::saved_proposal_association(snapshot, &d.decision_id, None);
                let result = match first {
                    Ok(Some(association)) => {
                        let current = association
                            .provenance
                            .task
                            .as_ref()
                            .and_then(|t| resolve_task(&t.task_id));
                        proposals::saved_proposal_association(
                            snapshot,
                            &d.decision_id,
                            current.as_ref(),
                        )
                        .map_err(message)
                        .and_then(|a| a.ok_or_else(|| "提案来源元数据缺失".into()))
                    }
                    Ok(None) => Err("提案来源元数据缺失".into()),
                    Err(error) => Err(error.to_string()),
                };
                (d.decision_id.clone(), result)
            })
            .collect()
    }
    /// These lines are shared by the review UI and integration assertions. Real
    /// impacts must not be confused with the deliberately synthetic copied case.
    pub fn imported_review_lines(&self) -> Vec<String> {
        let Some(replay) = &self.exchange.replay else {
            return vec![];
        };
        let preview = replay.replay.preview();
        let mut lines = vec![
            format!(
                "实际冻结范围：{} 笔现有记录；{}未来默认规则",
                preview.scope.frozen_record_ids.len(),
                if preview.scope.applies_to_future_records {
                    "会改变"
                } else {
                    "不改变"
                }
            ),
            "实际记录的前后影响（保留原始事实）".into(),
        ];
        for impact in &preview.impacts {
            lines.push(format!("采用前：{}", observation_line(&impact.before)));
            lines.push(format!("采用后：{}", observation_line(&impact.after)));
        }
        if preview.scope.applies_to_future_records {
            if let Some(before) = self
                .snapshot
                .as_ref()
                .and_then(|s| decisions::policy_for_future(s).ok())
            {
                lines.push(format!("未来默认，采用前：{}", policy_line(&before)));
            }
            lines.push(format!(
                "未来默认，采用后：{}",
                policy_line(&preview.future_policy)
            ));
        }
        lines.push("虚构或经选择的副本结果（与上述真实记录影响分开）".into());
        for evidence in &preview.evidence {
            for observation in &evidence.observations {
                lines.push(observation_line(observation));
            }
        }
        lines.push("目标变化不会改写原始承诺，也不代表已通知客户或取得同意".into());
        lines
    }
    fn show_exchange(
        &mut self,
        ui: &mut egui::Ui,
        tasks: &[(String, String)],
        resolve_task: &dyn Fn(&str) -> Option<TaskIdentity>,
    ) {
        ui.collapsing("可选：导出需求 / 导入结构化提案", |ui| {
            egui::ScrollArea::vertical().id_salt("proposal-exchange").max_height(420.0).show(ui, |ui| {
                ui.label("文件交换由你主动操作，不自动发送、不执行提案中的命令。支持有限计时、日期和提醒规则；这不是任意自然语言生成软件。");
                ui.label("默认生成虚构记录副本，不复制真实记录的字段或历史。请逐项核对规范名称、原话和有效意图摘要，文件未加密。");
                ui.label("重启、关闭或切换工具后，须重新导出当前需求包；不接受失去本次导出上下文的旧提案。");
                let writable = self.diagnostic.is_none() && self.pending.is_none();
                ui.add_enabled_ui(writable, |ui| {
                    ui.label("给提案作者的修改请求（请脱敏）");
                    let mut changed = ui.text_edit_multiline(&mut self.exchange.request).changed();
                    if let Some(snapshot) = &self.snapshot {
                        for d in snapshot.decisions.iter().filter(|d|d.status == DecisionStatus::Active) {
                            ui.label(format!("有效决定 {}：请填写已核对的脱敏摘要", d.decision_id));
                            changed |= ui.text_edit_singleline(self.exchange.summaries.entry(d.decision_id.clone()).or_default()).changed();
                        }
                        egui::ComboBox::from_id_salt("exchange-record").selected_text(if self.exchange.record.is_empty() { "选择演练对象" } else { &self.exchange.record }).show_ui(ui, |ui| {
                            for record in &snapshot.records { changed |= ui.selectable_value(&mut self.exchange.record, record.record_id.clone(), &record.record_id).changed(); }
                        });
                    }
                    egui::ComboBox::from_id_salt("exchange-task").selected_text(if self.exchange.task.is_empty(){"不关联开发任务"}else{&self.exchange.task}).show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut self.exchange.task, String::new(), "不关联开发任务").changed();
                        for (id,title) in tasks { changed |= ui.selectable_value(&mut self.exchange.task, id.clone(), title).changed(); }
                    });
                    for (scope,label) in [(ScopeKind::SingleRecord,"只改这一笔"),(ScopeKind::FutureRecords,"只用于以后新建记录"),(ScopeKind::IncompleteAndFuture,"当前未完成和未来记录"),(ScopeKind::AllExistingAndFuture,"全部历史和未来记录")] { changed |= ui.radio_value(&mut self.exchange.scope, scope, label).changed(); }
                    if changed { self.exchange.prepared_export = None; self.exchange.reviewed = false; self.exchange.replay = None; }
                    if ui.button("生成虚构需求包，先检查内容").clicked() {
                        let result = self.synthetic_export(resolve_task);
                        match result { Ok(bytes) => { self.exchange.export_json = String::from_utf8_lossy(&bytes).into_owned(); self.exchange.reviewed = false; self.exchange.status = "仅已准备，尚未写入文件".into(); } Err(e) => self.exchange.status = e }
                    }
                    if self.exchange.prepared_export.is_some() {
                        ui.add(egui::TextEdit::multiline(&mut self.exchange.export_json).desired_rows(8).desired_width(f32::INFINITY).interactive(false));
                        ui.checkbox(&mut self.exchange.reviewed,"我已检查以上完整导出内容，允许保存到下面的本地文件");
                        ui.label("新文件绝对路径（不会覆盖已有文件）"); ui.text_edit_singleline(&mut self.exchange.export_path);
                        if ui.add_enabled(self.exchange.reviewed, egui::Button::new("保存需求包")).clicked() {
                            let path = PathBuf::from(&self.exchange.export_path);
                            self.exchange.status = match self.write_export(&path) { Ok(()) => "需求包已保存到所选本地文件；尚未发送给任何服务".into(), Err(e) => format!("未保存：{e}") };
                        }
                    }
                    ui.separator(); ui.label("提案文件绝对路径（只读，最多 1 MiB）"); ui.text_edit_singleline(&mut self.exchange.import_path);
                    if ui.button("读取并检查提案").clicked() {
                        let path = PathBuf::from(&self.exchange.import_path);
                        let current = if self.exchange.task.is_empty(){None}else{resolve_task(&self.exchange.task)};
                        let result = open_regular_file(&path).and_then(|file|self.import_proposal(file,current.as_ref()));
                        self.exchange.status = match result { Ok(()) => "结构检查通过，仍是不可信提议。请本地演练后选择结果".into(), Err(e)=>format!("未导入：{e}") };
                    }
                    if let Some(accepted) = &self.exchange.accepted {
                        ui.label(format!("提案 {}；来源声明 {:?}，没有验证作者身份",accepted.envelope().proposal_id,accepted.envelope().source));
                        ui.label(&accepted.envelope().rationale);
                        for unknown in &accepted.envelope().unknowns { ui.label(format!("未知：{unknown}")); }
                        for candidate in accepted.candidates() {
                            if ui.radio_value(&mut self.exchange.candidate,candidate.candidate_id.clone(),&candidate.candidate_id).changed() {self.exchange.replay=None;}
                            ui.label(format!("候选涉及的做法：{}",candidate.rule_keys.iter().map(|key|match key {RuleKey::Timer=>"制作计时",RuleKey::DeliveryTarget=>"显示交付目标",RuleKey::Reminder=>"跟进提醒"}).collect::<Vec<_>>().join("、")));
                        }
                        if let Some(snapshot) = &self.snapshot { ui.collapsing("明确改变主意：选择要替代的决定", |ui| { for d in snapshot.decisions.iter().filter(|d|matches!(d.status,DecisionStatus::Active|DecisionStatus::Pending)) { let mut selected = self.exchange.supersedes.contains(&d.decision_id); if ui.checkbox(&mut selected,&d.decision_id).changed() {self.exchange.supersedes.retain(|id|id!=&d.decision_id); if selected {self.exchange.supersedes.push(d.decision_id.clone());} self.exchange.replay=None;} } }); }
                        if ui.button("在本地副本上执行提案").clicked() {
                            let candidate = self.exchange.candidate.clone(); let record = self.exchange.record.clone(); let supersedes = self.exchange.supersedes.clone();
                            let current = self.exchange.accepted.as_ref().and_then(|a|a.task()).and_then(|t|resolve_task(&t.task_id));
                            self.exchange.status = match self.replay_imported(&candidate,&record,self.exchange.scope,supersedes,current.as_ref()){Ok(())=>"已执行本地演练，尚未采用".into(),Err(e)=>format!("演练未通过：{e}")};
                        }
                    }
                });
                let mut choice = None;
                if let Some(replay) = &self.exchange.replay {
                    let preview = replay.replay.preview();
                    let current_input = (self.exchange.record.clone(),self.exchange.scope,self.exchange.supersedes.clone());
                    let fresh = replay.session == self.session && replay.date == self.date() && self.snapshot.as_ref().is_some_and(|s|s.generation==preview.generation) && replay.input==current_input;
                    for line in self.imported_review_lines() { ui.label(line); }
                    for conflict in &preview.conflicts { ui.colored_label(egui::Color32::LIGHT_RED,format!("与决定 {} 冲突：{}",conflict.decision_id,conflict.message)); for value in &conflict.expected {ui.label(format!("已确认：{value:?}"));} for value in &conflict.actual {ui.label(format!("当前候选：{value:?}"));} }
                    if !fresh { ui.label("证据过期，请重新演练"); }
                    if ui.add_enabled(writable && fresh && preview.conflicts.is_empty(),egui::Button::new("采用本地执行的提案结果")).clicked(){choice=Some(DecisionChoice::Adopt);}
                    ui.horizontal_wrapped(|ui| { for (value,label) in [(DecisionChoice::Both,"两种都需要，保存待解决"),(DecisionChoice::Neither,"都不合适"),(DecisionChoice::Defer,"以后再决定")] {if ui.add_enabled(writable && fresh,egui::Button::new(label)).clicked(){choice=Some(value);}} });
                }
                if let Some(value)=choice {
                    let current=self.exchange.accepted.as_ref().and_then(|a|a.task()).and_then(|t|resolve_task(&t.task_id));
                    self.exchange.status=match self.save_imported(value,current.as_ref()) {Ok(OperationOutcome::Committed)=>"决定已保存，来源关联已记录；外部作者身份仍未验证".into(),Ok(other)=>format!("未确认保存：{other:?}"),Err(e)=>format!("未保存：{e}")};
                }
                if self.pending.as_ref().is_some_and(|p|matches!(p.prepared,Prepared::Proposal(_))) && ui.button("重试同一次提案保存（重新检查任务来源）").clicked() {
                    let current=self.exchange.accepted.as_ref().and_then(|a|a.task()).and_then(|t|resolve_task(&t.task_id));
                    self.exchange.status=format!("{:?}",self.commit_pending(current.as_ref()));
                }
                ui.label(&self.exchange.status);
                if !self.snapshot.as_ref().is_none_or(|s|s.decisions.is_empty()) && ui.button("检查已保存提案的任务关联").clicked() {
                    self.exchange.status=self.proposal_associations(resolve_task).into_iter().map(|(id,result)|match result{Ok(a)=>format!("{id}：{:?}（仅表示关联新鲜度，不代表作者身份）",a.task_status),Err(e)=>format!("{id}：关联无效，{e}")}).collect::<Vec<_>>().join("\n");
                }
            });
        });
    }
    fn synthetic_export(
        &mut self,
        resolve_task: &dyn Fn(&str) -> Option<TaskIdentity>,
    ) -> Result<Vec<u8>, String> {
        let snapshot = self.writable()?;
        let id = &self.exchange.record;
        if !snapshot.records.iter().any(|r| &r.record_id == id) {
            return Err("请明确选择一笔本地记录作为规则作用对象。导出内容使用新的虚构例子".into());
        }
        let mut scenario = decisions::scenario_from_snapshot(
            snapshot,
            &identity("export-example"),
            "已选择的虚构例子",
            self.date(),
            self.now,
        )
        .map_err(message)?;
        scenario.records.clear();
        scenario.event_history.clear();
        scenario.event_sequence = 0;
        let spec = snapshot.active_spec().ok_or("工具规范缺失")?;
        let mut values = BTreeMap::new();
        for field in &spec.fields {
            let value = match field.role {
                FieldRole::Title => Some(FieldValue::Text("虚构工作".into())),
                FieldRole::WorkDescription => {
                    Some(FieldValue::Text("只用于比较规则的虚构例子".into()))
                }
                FieldRole::WorkStartedOn => Some(FieldValue::Date(self.date())),
                FieldRole::PromisedDate => Some(FieldValue::Date(
                    self.date().succ_opt().ok_or("日期超出范围")?,
                )),
                FieldRole::Notes => None,
                FieldRole::Custom => {
                    if field.required {
                        Some(match &field.kind {
                            FieldKind::Text => FieldValue::Text("虚构".into()),
                            FieldKind::Date => FieldValue::Date(self.date()),
                            FieldKind::Integer => FieldValue::Integer(1),
                            FieldKind::Boolean => FieldValue::Boolean(false),
                            FieldKind::Enum { options } => {
                                FieldValue::Enum(options.first().ok_or("枚举值缺失")?.clone())
                            }
                        })
                    } else {
                        None
                    }
                }
            };
            if let Some(value) = value {
                values.insert(field.id.clone(), value);
            }
        }
        let stage = spec
            .stages
            .iter()
            .find(|s| s.clock == StageClock::Paused)
            .unwrap_or_else(|| {
                spec.stage(&spec.default_stage_id)
                    .expect("validated default stage")
            });
        scenario.steps = vec![
            ScenarioStep::CreateRecord {
                operation_id: identity("synthetic-create"),
                record_id: id.clone(),
                initial_stage_id: stage.id.clone(),
                values,
                occurred_at: self.now,
            },
            ScenarioStep::Observe {
                record_id: id.clone(),
            },
        ];
        let frozen =
            decisions::freeze_scope(snapshot, self.exchange.scope, Some(id)).map_err(message)?;
        let target = if !frozen.frozen_record_ids.contains(id) && frozen.applies_to_future_records {
            RehearsalTarget::FutureRecord(id.clone())
        } else {
            RehearsalTarget::Record(id.clone())
        };
        let task = if self.exchange.task.is_empty() {
            None
        } else {
            Some(
                resolve_task(&self.exchange.task)
                    .ok_or("选中的任务缺失、移动或无法读取当前源码；可取消关联继续本地工作")?,
            )
        };
        let selection = proposals::ExportSelection {
            original_request: self.exchange.request.clone(),
            confirmed_intents: snapshot
                .decisions
                .iter()
                .filter(|d| d.status == DecisionStatus::Active)
                .map(|d| proposals::SelectedIntent {
                    decision_id: d.decision_id.clone(),
                    sanitized_intent: self
                        .exchange
                        .summaries
                        .get(&d.decision_id)
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
            examples: vec![proposals::SelectedExample {
                disclosure: proposals::ExampleDisclosure::Synthetic,
                case: RehearsalCase { scenario, target },
            }],
            task,
        };
        self.prepare_export(selection)
    }
}
fn observation_line(observation: &EvidenceObservation) -> String {
    format!(
        "{}：制作 {} 天，等待 {} 天；原承诺 {}，显示目标 {}；提醒 {:?}",
        observation.record_id,
        observation
            .elapsed_work_days
            .map(|d| d.to_string())
            .unwrap_or_else(|| "无法确定".into()),
        observation.paused_days,
        observation
            .original_due_date
            .map(|d| d.to_string())
            .unwrap_or_else(|| "未填写".into()),
        observation
            .display_due_date
            .map(|d| d.to_string())
            .unwrap_or_else(|| "未填写".into()),
        observation.reminder
    )
}
fn policy_line(policy: &BehaviorPolicy) -> String {
    let timer = match policy.timer {
        TimerPolicy::PauseStagesMarkedPaused => "等待时暂停制作计时",
        TimerPolicy::CountPausedStages => "等待也计入制作时间",
    };
    let due = match policy.due_date {
        DateDuePolicy::KeepOriginal => "保留原承诺日期",
        DateDuePolicy::ExtendByPausedDays => "显示目标按等待天数顺延",
    };
    let reminder = match policy.reminder {
        ReminderPolicy::Never => "不提醒".into(),
        ReminderPolicy::WaitingBeforeDue { days_before_due } => {
            format!("承诺日前 {days_before_due} 天提醒")
        }
        ReminderPolicy::WaitingAfterDays { days_waiting } => {
            format!("等待 {days_waiting} 天后提醒")
        }
    };
    format!("{timer}；{due}；{reminder}")
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if !path.is_absolute() {
        return Err("请选择绝对路径".into());
    }
    let parent = path.parent().ok_or("文件缺少父目录")?;
    for part in parent.ancestors() {
        if std::fs::symlink_metadata(part)
            .map_err(message)?
            .file_type()
            .is_symlink()
        {
            return Err("不能通过符号链接写出文件".into());
        }
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(message)?;
    temporary.write_all(bytes).map_err(message)?;
    temporary.as_file().sync_all().map_err(message)?;
    #[cfg(unix)]
    {
        temporary.persist_noclobber(path).map_err(message)?;
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| format!("文件已发布，但目录同步失败，保存结果尚未确认：{e}"))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, SetFileAttributesW, FILE_ATTRIBUTE_NORMAL, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = temporary
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Do not set REPLACE_EXISTING: a reviewed export never overwrites a file.
        let success = unsafe {
            SetFileAttributesW(source.as_ptr(), FILE_ATTRIBUTE_NORMAL) != 0
                && MoveFileExW(
                    source.as_ptr(),
                    destination.as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                ) != 0
        };
        if !success {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(any(unix, windows)))]
    return Err("这个平台不能确认导出文件的持久化".into());
    Ok(())
}
fn open_regular_file(path: &Path) -> Result<std::fs::File, String> {
    if !path.is_absolute() {
        return Err("请选择绝对路径".into());
    }
    for part in path.ancestors() {
        if std::fs::symlink_metadata(part)
            .map_err(message)?
            .file_type()
            .is_symlink()
        {
            return Err("不能读取符号链接提案".into());
        }
    }
    let metadata = std::fs::metadata(path).map_err(message)?;
    if !metadata.is_file() {
        return Err("提案必须是普通文件".into());
    }
    if metadata.len() > crate::tool_proposal_input::MAX_INPUT_BYTES as u64 {
        return Err("提案超过 1 MiB 限制".into());
    }
    std::fs::File::open(path).map_err(message)
}

/// Create the prevalidated default hierarchy without following a swapped
/// ancestor. Unix walks/mkdirs relative to pinned descriptors; Windows retains
/// no-delete-sharing directory handles for every ancestor until creation ends.
pub(crate) fn create_private_data_directory(
    path: &Path,
    #[cfg(test)] mut before_create: impl FnMut(&Path),
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::path::Component;
    if !path.is_absolute() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "data directory must be absolute",
        ));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::CString;
        use std::fs::File;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::raw::{c_char, c_int};
        use std::os::unix::ffi::OsStrExt;
        // These platform flags match the project's existing no-follow store.
        #[cfg(target_os = "linux")]
        const FLAGS: c_int = 0o200000 | 0o400000 | 0o2000000;
        #[cfg(target_os = "macos")]
        const FLAGS: c_int = 0x0010_0000 | 0x0000_0100 | 0x0100_0000;
        #[cfg(target_os = "linux")]
        type DirectoryMode = u32;
        #[cfg(target_os = "macos")]
        type DirectoryMode = u16;
        extern "C" {
            fn openat(directory: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
            fn mkdirat(directory: c_int, path: *const c_char, mode: DirectoryMode) -> c_int;
        }
        let mut parent = File::open("/")?;
        #[cfg(test)]
        let mut current = PathBuf::from("/");
        for component in path.components() {
            let name = match component {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "invalid directory component",
                    ))
                }
            };
            #[cfg(test)]
            current.push(name);
            let name = CString::new(name.as_bytes())
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "directory contains NUL"))?;
            let mut fd = unsafe { openat(parent.as_raw_fd(), name.as_ptr(), FLAGS) };
            if fd < 0 {
                let error = Error::last_os_error();
                if error.kind() != ErrorKind::NotFound {
                    return Err(error);
                }
                #[cfg(test)]
                before_create(&current);
                if unsafe { mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
                    let error = Error::last_os_error();
                    if error.kind() != ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                parent.sync_all()?;
                fd = unsafe { openat(parent.as_raw_fd(), name.as_ptr(), FLAGS) };
            }
            if fd < 0 {
                return Err(Error::last_os_error());
            }
            parent = unsafe { File::from_raw_fd(fd) };
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::fs::{File, OpenOptions};
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        fn pin(path: &Path) -> std::io::Result<File> {
            let handle = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let metadata = handle.metadata()?;
            if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "data directory is not a real directory",
                ));
            }
            Ok(handle)
        }
        let mut current = PathBuf::new();
        let mut guards = Vec::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => {
                    current.push(prefix.as_os_str());
                    continue;
                }
                Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
                _ => {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "invalid directory component",
                    ))
                }
            }
            match pin(&current) {
                Ok(handle) => guards.push(handle),
                Err(error) if error.kind() == ErrorKind::NotFound && !guards.is_empty() => {
                    #[cfg(test)]
                    before_create(&current);
                    match std::fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                    guards.push(pin(&current)?);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(Error::new(
            ErrorKind::Unsupported,
            "safe default directory creation is unavailable on this platform",
        ))
    }
}
