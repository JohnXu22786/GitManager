//! Linked-task source workflow for the existing studio worker and controller.
//!
//! Poll and all filesystem/analysis methods belong on the existing worker. The
//! host owns operation/basis fencing, consent, the journal, genuine invocation,
//! independent runtime verification and final commit. Nothing here executes an
//! external program, creates another registry or turns completion into approval.
use crate::product_contract::{
    AdapterError, ArtifactRef, CapturedProgram, ContractError, DecisionGraph, DevelopmentRequest,
    Id, SourceBinding, MAX_WIRE_BYTES,
};
use crate::product_provider::{ProviderRequest, ProviderResult, ProviderTransport};
use crate::product_sources::{
    ExternalHandoff, ExternalHandoffTicket, SourceUpdate, SourceWatcher, TaskSourceAdapter,
};
use crate::tasks::TaskRegistry;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
type Result<T> = std::result::Result<T, AdapterError>;
fn invalid(message: &str) -> AdapterError {
    AdapterError::Invalid(ContractError(message.into()))
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Acquire) {
        Err(AdapterError::Cancelled)
    } else {
        Ok(())
    }
}
fn message(error: &AdapterError) -> String {
    match error {
        AdapterError::Invalid(e) => e.to_string(),
        AdapterError::Unsupported(s)
        | AdapterError::Stale(s)
        | AdapterError::BudgetExhausted(s)
        | AdapterError::Failed(s) => s.clone(),
        AdapterError::Cancelled => "Cancelled; saved work is unchanged".into(),
    }
}

#[derive(Clone, Debug)]
pub struct TaskChoice {
    pub task_id: Id,
    pub title: String,
    pub location: String,
}
/// Selection is an existing registry ID, never a new or label-based task entry.
pub fn task_choices() -> Result<Vec<TaskChoice>> {
    let registry = TaskRegistry::load();
    if let Some(error) = registry.load_error() {
        return Err(AdapterError::Failed(format!(
            "Task registry is unavailable: {error}"
        )));
    }
    Ok(registry
        .entries()
        .iter()
        .filter(|task| task.worktree_cleanup_completed_at.is_none())
        .map(|task| TaskChoice {
            task_id: task.id.clone(),
            title: task.title.clone(),
            location: task.worktree_path.clone(),
        })
        .collect())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceStatus {
    Current,
    Pending(String),
    Unsupported(String),
    Unavailable(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalStatus {
    None,
    AwaitingCompletion { request_id: Id },
    Captured { request_id: Id },
    Interrupted { request_id: Id, reason: String },
    Cancelled { request_id: Id },
    Rejected { request_id: Id, reason: String },
}
#[derive(Clone, Debug)]
pub struct SourceSummary {
    pub label: String,
    pub binding: SourceBinding,
    pub artifact: ArtifactRef,
}
#[derive(Clone, Debug)]
pub struct View {
    pub task_id: Id,
    pub source_status: SourceStatus,
    pub last_capture: Option<SourceSummary>,
    pub last_capture_stale: bool,
    pub external: ExternalStatus,
    pub can_reopen: bool,
}
#[derive(Clone, Debug)]
pub enum Event {
    LinkTask { task_id: Id },
    RequestExternalChange { task_id: Id, source: SourceBinding },
    InspectDecisions { request_id: Id },
    CancelExternal { request_id: Id },
    ReopenExternal { request_id: Id },
}
#[derive(Debug)]
pub enum FlowUpdate {
    Unchanged,
    StatusChanged,
    Captured(Box<CapturedProgram>),
}
/// Save only in the existing host journal, bound to its authorized request/job.
/// A missing ticket is deliberately recoverable only as an interrupted state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingExternal {
    request: DevelopmentRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ticket: Option<ExternalHandoffTicket>,
}
impl PendingExternal {
    /// Initial journal association, before any local bundle write. It cannot
    /// reopen a job until the host durably adds the ticket from preparation.
    pub fn unissued(request: DevelopmentRequest) -> Result<Self> {
        request.validate()?;
        if serde_json::to_vec(&request)
            .map_err(|e| AdapterError::Failed(e.to_string()))?
            .len()
            > MAX_WIRE_BYTES
        {
            return Err(invalid("Job bundle exceeds byte limit"));
        }
        Ok(Self {
            request,
            ticket: None,
        })
    }
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
}
/// Input to the host's genuinely authorized invocation route. This descriptor
/// has no provider receipt and does not assert that an author was invoked.
#[derive(Clone, Debug)]
pub struct ExternalInvocation {
    request: DevelopmentRequest,
    baseline: CapturedProgram,
    worktree: PathBuf,
    request_path: PathBuf,
}
impl ExternalInvocation {
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn baseline(&self) -> &CapturedProgram {
        &self.baseline
    }
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
    pub fn request_path(&self) -> &Path {
        &self.request_path
    }
}
#[derive(Debug)]
pub struct ExternalEdit {
    request: DevelopmentRequest,
    baseline: CapturedProgram,
    capture: CapturedProgram,
}
impl ExternalEdit {
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn baseline(&self) -> &CapturedProgram {
        &self.baseline
    }
    pub fn capture(&self) -> &CapturedProgram {
        &self.capture
    }
}
/// Only a source freshness envelope. Its value is not automatically runtime
/// evidence, user approval or authority to adopt anything.
#[derive(Debug)]
pub struct SourceAnalysis<T> {
    source: CapturedProgram,
    value: T,
}
impl<T> SourceAnalysis<T> {
    pub fn source(&self) -> &CapturedProgram {
        &self.source
    }
    pub fn value(&self) -> &T {
        &self.value
    }
    pub fn into_parts(self) -> (CapturedProgram, T) {
        (self.source, self.value)
    }
}

pub struct TaskSourceFlow {
    adapter: TaskSourceAdapter,
    watcher: SourceWatcher,
    last: Option<CapturedProgram>,
    status: SourceStatus,
    pending: Option<PendingExternal>,
    handoff: Option<ExternalHandoff>,
    external: ExternalStatus,
}
impl TaskSourceFlow {
    pub fn link(project_id: &str, task_id: &str) -> Result<Self> {
        let adapter = TaskSourceAdapter::link(project_id, task_id)?;
        let watcher = SourceWatcher::new(adapter.clone())?;
        Ok(Self {
            adapter,
            watcher,
            last: None,
            status: SourceStatus::Pending("Waiting for a stable complete program".into()),
            pending: None,
            handoff: None,
            external: ExternalStatus::None,
        })
    }
    pub fn view(&self) -> View {
        View {
            task_id: self.adapter.task_id().into(),
            source_status: self.status.clone(),
            last_capture: self.last.as_ref().map(|c| SourceSummary {
                label: c.program.label.clone(),
                binding: c.binding.clone(),
                artifact: c.artifact.clone(),
            }),
            last_capture_stale: self.last.is_some() && self.status != SourceStatus::Current,
            external: self.external.clone(),
            can_reopen: matches!(self.external, ExternalStatus::Interrupted { .. })
                && self.pending.as_ref().is_some_and(|p| p.ticket.is_some()),
        }
    }
    pub fn last_capture(&self) -> Option<&CapturedProgram> {
        self.last.as_ref()
    }
    fn status(&mut self, status: SourceStatus) -> FlowUpdate {
        if self.status == status {
            FlowUpdate::Unchanged
        } else {
            self.status = status;
            FlowUpdate::StatusChanged
        }
    }
    /// Native events only trigger V05's bounded coalescing and periodic checks.
    /// Keep polling from the worker even when no filesystem event was delivered.
    pub fn poll(&mut self) -> FlowUpdate {
        match self.watcher.poll() {
            SourceUpdate::Captured(capture) => {
                self.last = Some(*capture.clone());
                self.status = SourceStatus::Current;
                FlowUpdate::Captured(capture)
            }
            SourceUpdate::Pending(reason) => self.status(SourceStatus::Pending(reason)),
            SourceUpdate::Unavailable(AdapterError::Unsupported(reason)) => {
                self.status(SourceStatus::Unsupported(reason))
            }
            SourceUpdate::Unavailable(error) => {
                self.status(SourceStatus::Unavailable(message(&error)))
            }
            SourceUpdate::Unchanged => {
                // A partial write can be repaired back to exactly the last
                // complete capture, for which the watcher emits Unchanged.
                if self.status != SourceStatus::Current
                    && self
                        .last
                        .as_ref()
                        .is_some_and(|last| self.adapter.ensure_fresh(last).is_ok())
                {
                    self.status(SourceStatus::Current)
                } else {
                    FlowUpdate::Unchanged
                }
            }
        }
    }
    pub fn analyze<T>(
        &self,
        source: &CapturedProgram,
        cancelled: &AtomicBool,
        work: impl FnOnce(&CapturedProgram) -> Result<T>,
    ) -> Result<SourceAnalysis<T>> {
        check_cancelled(cancelled)?;
        let value = self.adapter.with_fresh_source(source, || {
            check_cancelled(cancelled)?;
            let value = work(source)?;
            check_cancelled(cancelled)?;
            Ok(value)
        })?;
        check_cancelled(cancelled)?;
        Ok(SourceAnalysis {
            source: source.clone(),
            value,
        })
    }
    /// Call immediately before the existing host's final commit gate. The host
    /// must also check its operation, data, decisions, preview and user choice.
    pub fn check_before_adoption<T>(
        &self,
        result: &SourceAnalysis<T>,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        check_cancelled(cancelled)?;
        self.adapter.ensure_fresh(&result.source)?;
        check_cancelled(cancelled)
    }
    /// Preserve V02's original opaque receipt and provenance unchanged. A
    /// transport fixture, successful file read or result text cannot become a
    /// live invocation or independent runtime evidence here.
    pub fn ingest_transport(
        &self,
        transport: &ProviderTransport,
        request: &ProviderRequest,
        baseline: &CapturedProgram,
        cancelled: &AtomicBool,
    ) -> Result<Option<SourceAnalysis<ProviderResult>>> {
        let checked = self.analyze(baseline, cancelled, |_| {
            self.adapter.ingest_transport(transport, request, baseline)
        })?;
        Ok(checked.value.map(|value| SourceAnalysis {
            source: checked.source,
            value,
        }))
    }
    /// Only unfinished intake belongs in the host's pending journal slot.
    /// Terminal requests remain inspectable, but cannot be persisted as pending.
    pub fn pending_external(&self) -> Option<&PendingExternal> {
        if matches!(
            self.external,
            ExternalStatus::AwaitingCompletion { .. } | ExternalStatus::Interrupted { .. }
        ) {
            self.pending.as_ref()
        } else {
            None
        }
    }
    /// The host must journal the exact already-authorized request before this
    /// side effect, then retain the returned ticket before launching an author.
    /// A crash before ticket retention stays interrupted; never prepare that ID again.
    pub fn prepare_external(
        &mut self,
        root: &Path,
        request: &DevelopmentRequest,
        cancelled: &AtomicBool,
    ) -> Result<ExternalInvocation> {
        check_cancelled(cancelled)?;
        if matches!(
            self.external,
            ExternalStatus::AwaitingCompletion { .. } | ExternalStatus::Interrupted { .. }
        ) {
            return Err(invalid(
                "Finish or explicitly abandon the existing external job first",
            ));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request.id == request.id)
        {
            return Err(invalid(
                "A used request ID cannot be prepared again; reopen its saved job or use a new ID",
            ));
        }
        self.pending = Some(PendingExternal::unissued(request.clone())?);
        self.handoff = None;
        let prepared = match ExternalHandoff::prepare(&self.adapter, root, request)
            .and_then(|prepared| prepared.ticket().map(|ticket| (prepared, ticket)))
        {
            Ok((prepared, ticket)) => {
                self.pending.as_mut().expect("unissued association").ticket = Some(ticket);
                prepared
            }
            Err(error) => {
                self.external = ExternalStatus::Rejected {
                    request_id: request.id.clone(),
                    reason: message(&error),
                };
                return Err(error);
            }
        };
        self.handoff = Some(prepared);
        self.external = ExternalStatus::AwaitingCompletion {
            request_id: request.id.clone(),
        };
        if let Err(error) = check_cancelled(cancelled) {
            self.cancel_external(&request.id)?;
            return Err(error);
        }
        let prepared = self.handoff.as_ref().expect("just prepared");
        Ok(ExternalInvocation {
            request: prepared.request().clone(),
            baseline: prepared.baseline().clone(),
            worktree: self.adapter.worktree().into(),
            request_path: prepared.request_path(),
        })
    }
    fn pending_for(&self, id: &str) -> Result<&PendingExternal> {
        self.pending
            .as_ref()
            .filter(|pending| pending.request.id == id)
            .ok_or_else(|| invalid("This operation belongs to another external request"))
    }
    fn running(&self, id: &str) -> Result<&ExternalHandoff> {
        self.pending_for(id)?;
        if !matches!(self.external, ExternalStatus::AwaitingCompletion { .. }) {
            return Err(invalid(
                "This external job is no longer awaiting completion",
            ));
        }
        self.handoff
            .as_ref()
            .ok_or_else(|| invalid("The external job must be safely reopened first"))
    }
    /// Machine operation: only the registered request ID, never a caller path.
    /// Duplicate submission is refused without overwriting the first completion.
    pub fn submit_external(&mut self, id: &str, cancelled: &AtomicBool) -> Result<()> {
        self.running(id)?;
        if let Err(error) = check_cancelled(cancelled) {
            self.cancel_external(id)?;
            return Err(error);
        }
        let result = self.running(id)?.submit_current_program();
        if let Err(error) = check_cancelled(cancelled) {
            self.cancel_external(id)?;
            return Err(error);
        }
        result
    }
    pub fn inspect_decisions(&self, id: &str) -> Result<&DecisionGraph> {
        Ok(&self.pending_for(id)?.request.decisions)
    }
    pub fn intake_external(
        &mut self,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<Option<ExternalEdit>> {
        self.running(id)?;
        if let Err(error) = check_cancelled(cancelled) {
            self.cancel_external(id)?;
            return Err(error);
        }
        let handoff = self.running(id)?;
        let result = handoff.ingest();
        if let Err(error) = check_cancelled(cancelled) {
            self.cancel_external(id)?;
            return Err(error);
        }
        match result {
            Ok(None) => Ok(None),
            Ok(Some(capture)) => {
                if self.last.as_ref().is_some_and(|last| last != &capture) {
                    self.status = SourceStatus::Pending(
                        "External edits captured; refreshing the watched source".into(),
                    );
                }
                let handoff = self.handoff.take().expect("running job");
                self.external = ExternalStatus::Captured {
                    request_id: id.into(),
                };
                Ok(Some(ExternalEdit {
                    request: handoff.request().clone(),
                    baseline: handoff.baseline().clone(),
                    capture,
                }))
            }
            Err(error) => {
                self.handoff = None;
                self.external = ExternalStatus::Rejected {
                    request_id: id.into(),
                    reason: message(&error),
                };
                Err(error)
            }
        }
    }
    /// Abandon this controller's intake. This does not claim to terminate a
    /// separately running author or undo source edits. Late files are not ingested.
    pub fn cancel_external(&mut self, id: &str) -> Result<()> {
        self.pending_for(id)?;
        self.handoff = None;
        self.external = ExternalStatus::Cancelled {
            request_id: id.into(),
        };
        Ok(())
    }
    /// Restore only the controller association. Reopening needs the host's
    /// registered job root and a saved ticket; it never resumes an invocation.
    pub fn interrupted(project_id: &str, task_id: &str, pending: PendingExternal) -> Result<Self> {
        pending.request.validate()?;
        if pending.request.project_id != project_id
            || pending
                .request
                .sources
                .iter()
                .filter(|source| {
                    source
                        .binding
                        .task
                        .as_ref()
                        .is_some_and(|task| task.task_id == task_id)
                })
                .count()
                != 1
        {
            return Err(invalid(
                "The interrupted request belongs to another project or task",
            ));
        }
        let mut flow = Self::link(project_id, task_id)?;
        let reason = if pending.ticket.is_some() {
            "External intake was interrupted. Reopen the saved job, or abandon it and request a new change. Saved daily work remains available."
        } else {
            "The original job association is unavailable. Abandon local intake and request a new change with a new ID. Saved daily work remains available."
        };
        flow.external = ExternalStatus::Interrupted {
            request_id: pending.request.id.clone(),
            reason: reason.into(),
        };
        flow.pending = Some(pending);
        Ok(flow)
    }
    fn check_reopen_cancellation(
        &mut self,
        request_id: &str,
        operation_cancelled: &AtomicBool,
        handoff_cancelled: &AtomicBool,
    ) -> Result<()> {
        // Explicit abandonment wins when both signals arrive together.
        if handoff_cancelled.load(Ordering::Acquire) {
            self.cancel_external(request_id)?;
            return Err(AdapterError::Cancelled);
        }
        check_cancelled(operation_cancelled)
    }
    /// Cancellation of this read (for example, a superseded page) preserves the
    /// unfinished job and its ticket. Only explicit abandonment of this exact
    /// handoff makes local intake terminal, as in cancel_external/submit/intake.
    /// The host supplies distinct signals and durably records the pending slot.
    /// Neither signal claims to stop an independently running external author.
    pub fn reopen_external(
        &mut self,
        root: &Path,
        operation_cancelled: &AtomicBool,
        handoff_cancelled: &AtomicBool,
    ) -> Result<()> {
        if !matches!(self.external, ExternalStatus::Interrupted { .. }) {
            return Err(invalid("Only an interrupted job can be reopened"));
        }
        let request_id = self
            .pending
            .as_ref()
            .ok_or_else(|| invalid("No saved external request"))?
            .request
            .id
            .clone();
        self.check_reopen_cancellation(&request_id, operation_cancelled, handoff_cancelled)?;
        let pending = self.pending_for(&request_id)?;
        let handoff = pending.ticket.as_ref().ok_or_else(|| AdapterError::Unsupported(
            "The original job association is unavailable. Keep this job interrupted, or abandon local intake and request a new change with a new ID.".into()))
            .and_then(|ticket| ExternalHandoff::reopen(&self.adapter, root, &pending.request, ticket));
        // Check both signals even if the read failed. A superseded read does not
        // discard recovery; explicit abandonment must still clear its projection.
        self.check_reopen_cancellation(&request_id, operation_cancelled, handoff_cancelled)?;
        let handoff = handoff?;
        self.external = ExternalStatus::AwaitingCompletion { request_id };
        self.handoff = Some(handoff);
        Ok(())
    }
}

/// Presentation only. Selection and all follow-up actions return exact IDs to
/// the existing host, which supplies operation/basis fencing and authorization.
pub fn show(ui: &mut egui::Ui, choices: &[TaskChoice], view: Option<&View>) -> Option<Event> {
    let mut event = None;
    for task in choices {
        ui.push_id(&task.task_id, |ui| {
            if ui
                .button(&task.title)
                .on_hover_text(&task.location)
                .clicked()
            {
                event = Some(Event::LinkTask {
                    task_id: task.task_id.clone(),
                });
            }
        });
    }
    let Some(view) = view else {
        return event;
    };
    match &view.source_status {
        SourceStatus::Current => {
            ui.label("Complete source captured; independent analysis is still required");
        }
        SourceStatus::Pending(reason)
        | SourceStatus::Unsupported(reason)
        | SourceStatus::Unavailable(reason) => {
            ui.label(reason);
        }
    }
    if let Some(last) = &view.last_capture {
        ui.label(&last.label);
        if view.last_capture_stale {
            ui.label("This last complete version is stale and cannot be adopted");
        }
        if view.source_status == SourceStatus::Current
            && !matches!(
                view.external,
                ExternalStatus::AwaitingCompletion { .. } | ExternalStatus::Interrupted { .. }
            )
            && ui.button("Request an external change").clicked()
        {
            event = Some(Event::RequestExternalChange {
                task_id: view.task_id.clone(),
                source: last.binding.clone(),
            });
        }
    }
    let id = match &view.external {
        ExternalStatus::None => return event,
        ExternalStatus::AwaitingCompletion { request_id } => {
            ui.label(
                "Waiting for matching external completion. No live invocation is certified here.",
            );
            request_id
        }
        ExternalStatus::Captured { request_id } => {
            ui.label("External edits captured; review and independent checks are still required");
            request_id
        }
        ExternalStatus::Interrupted { request_id, reason }
        | ExternalStatus::Rejected { request_id, reason } => {
            ui.label(reason);
            request_id
        }
        ExternalStatus::Cancelled { request_id } => {
            ui.label("Local intake abandoned. This does not stop a separately running author.");
            request_id
        }
    };
    if view.can_reopen && ui.button("Retry saved intake").clicked() {
        event = Some(Event::ReopenExternal {
            request_id: id.clone(),
        });
    }
    if ui.button("Inspect handed-off decisions").clicked() {
        event = Some(Event::InspectDecisions {
            request_id: id.clone(),
        });
    }
    if matches!(
        view.external,
        ExternalStatus::AwaitingCompletion { .. } | ExternalStatus::Interrupted { .. }
    ) && ui.button("Abandon local intake").clicked()
    {
        event = Some(Event::CancelExternal {
            request_id: id.clone(),
        });
    }
    event
}
