//! Task source capture, handoff and checked review on Studio's single worker.
use super::*;
use crate::harness::{ClaudeHarness, CodexHarness, ProductTaskBundle, TaskHarness};
use journal::{CompletedTask, ExternalAssociation, TaskAssociation};
use task_source_flow::{ExternalStatus, SourceAnalysis, SourceStatus, TaskSourceFlow};

#[derive(Default)]
pub(super) struct State {
    pub flow: Option<TaskSourceFlow>,
    pub view: task_source_host::View,
    ready: Option<(task_source_host::Disclosure, Basis, u64)>,
    queued: Option<SourceAnalysis<ChangeDraft>>,
    completed: Option<Completed>,
    attempted: Option<(SourceBinding, Basis)>,
    unavailable: Option<String>,
    terminal_write_blocked: bool,
    focused: Option<RecordRef>,
    pub evidence: Option<(Id, SourceAnalysis<()>)>,
}
#[derive(Clone)]
struct Completed {
    capture: CapturedProgram,
    request: DevelopmentRequest,
    basis: Basis,
    verified: bool,
}
impl Worker {
    #[cfg(test)]
    fn pause_task_review_publication(&self, after: bool) {
        let pause = if after {
            &self.config.hooks.after_task_review_publish
        } else {
            &self.config.hooks.before_task_review_publish
        };
        if let Some(pause) = pause {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    fn task_root(&self) -> Result<PathBuf, String> {
        Ok(self
            .locations
            .as_ref()
            .ok_or("Tool location unavailable")?
            .path()
            .join("external-task-jobs"))
    }
    fn task_link(&self) -> Result<&TaskAssociation, String> {
        self.journal
            .as_ref()
            .and_then(|j| j.value.task.as_ref())
            .ok_or_else(|| "No task is linked".to_string())
    }
    fn task_matches_open(&self) -> bool {
        self.opened
            .as_ref()
            .is_some_and(|o| self.task_link().is_ok_and(|l| l.tool == o.association))
    }
    pub(super) fn task_view(&self) -> task_source_host::View {
        let mut view = self.task.view.clone();
        if self.task_matches_open() {
            view.linked = true;
            view.link = self.task_link().ok().map(task_source_host::Link::from);
            view.pending = self
                .task_link()
                .ok()
                .and_then(|l| l.external.as_ref())
                .map(|e| e.pending.request().id.clone());
            view.source = self.task.flow.as_ref().map(TaskSourceFlow::view);
            if let (Some(source), Some(reason)) = (&mut view.source, &self.task.unavailable) {
                source.source_status = SourceStatus::Unavailable(reason.clone());
                source.last_capture_stale = source.last_capture.is_some();
            }
            if let Some(completed) = &self.task.completed {
                let source = view.source.get_or_insert_with(|| task_source_flow::View {
                    task_id: self.task_link().unwrap().task_id.clone(),
                    source_status: SourceStatus::Unavailable(
                        "Saved completion could not be revalidated".into(),
                    ),
                    last_capture: None,
                    last_capture_stale: true,
                    external: ExternalStatus::Captured {
                        request_id: completed.request.id.clone(),
                    },
                    can_reopen: false,
                });
                if source.last_capture.is_none() {
                    source.last_capture = Some(task_source_flow::SourceSummary {
                        label: completed.capture.program.label.clone(),
                        binding: completed.capture.binding.clone(),
                        artifact: completed.capture.artifact.clone(),
                    });
                    source.last_capture_stale = true;
                }
                source.can_reopen = false;
            }
            if let (Some(source), Some(request_id)) = (&mut view.source, &view.pending) {
                if !matches!(
                    source.external,
                    ExternalStatus::AwaitingCompletion { .. } | ExternalStatus::Interrupted { .. }
                ) {
                    source.external = ExternalStatus::Interrupted { request_id: request_id.clone(), reason: "The exact request is retained after an interrupted preparation or uncertain launch. Reopen only for inspection, or abandon local intake. No automatic relaunch occurs".into() };
                    source.can_reopen = true;
                }
            }
        }
        // Current source availability permits a fresh explicit request even
        // when a historical completion is stale. Its review authority remains
        // separate and can only return after exact receipt/context validation.
        view.can_review = view.can_review
            && self.task.completed.as_ref().is_none_or(|c| c.verified)
            && view
                .source
                .as_ref()
                .is_some_and(|s| s.source_status == SourceStatus::Current)
            && self.task.queued.as_ref().is_some_and(|r| {
                self.task
                    .flow
                    .as_ref()
                    .and_then(|f| f.last_capture())
                    .is_some_and(|c| c == r.source())
            });
        view
    }
    pub(super) fn restore_task(&mut self, gate: &Gate) -> Result<(), String> {
        gate.check()?;
        self.task = State::default();
        if !self.task_matches_open() {
            return Ok(());
        }
        let link = self.task_link()?.clone();
        self.task.view.linked = true;
        if let Some(completed) = &link.completed {
            // Install the binding before any fallible reads: a damaged receipt
            // cannot fall back to an ordinary source review on a fresh Basis.
            self.task.completed = Some(Completed {
                capture: completed.capture.clone(),
                request: completed.external.pending.request().clone(),
                basis: completed.external.basis.clone(),
                verified: false,
            });
        }
        let result = (|| {
            super::super::task_source_machine::registered_task(&link)?;
            let saved = link
                .external
                .as_ref()
                .or_else(|| link.completed.as_ref().map(|c| &c.external));
            match saved {
                Some(external) => TaskSourceFlow::interrupted(
                    &link.tool.identity.project_id,
                    &link.task_id,
                    external.pending.clone(),
                )
                .map_err(error),
                None => TaskSourceFlow::link(&link.tool.identity.project_id, &link.task_id)
                    .map_err(error),
            }
        })();
        match result {
            Ok(mut flow) => {
                let reopened = if flow.pending_external().is_some() && flow.view().can_reopen {
                    flow.reopen_external(
                        &self.task_root()?,
                        &gate.cancelled,
                        &AtomicBool::new(false),
                    )
                    .map_err(error)
                } else {
                    Ok(())
                };
                if let Err(e) = reopened {
                    self.task.view.message = format!("Saved intake could not be reopened: {e}");
                } else if let Some(completed) = &link.completed {
                    match flow.intake_external(&completed.external.pending.request().id, &AtomicBool::new(false)) {
                        Ok(Some(edit)) if edit.capture() == &completed.capture && edit.request() == completed.external.pending.request() => {
                            self.task.completed.as_mut().unwrap().verified = true;
                        }
                        _ => self.task.view.message = "Saved completion no longer matches its exact receipt/source. It was kept for inspection; no evidence was rebased".into(),
                    }
                }
                self.task.flow = Some(flow);
                // Opening saved daily work restores exact receipt authority,
                // not copied analysis under an unrelated startup/open gate.
                // Explicit Tasks entry/reselection retries the original context
                // through retry_completed_review with its actual cancellable gate.
            }
            Err(e) => {
                self.task.unavailable = Some(e.clone());
                self.task.view.message =
                    format!("Linked source unavailable. Saved daily work remains usable. {e}");
            }
        }
        Ok(())
    }
    pub(super) fn tasks(
        &mut self,
        focused: Option<RecordRef>,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.current(key)?;
        gate.check()?;
        self.task.focused = focused;
        if self.task.queued.is_none() && self.task.completed.is_none() {
            // Explicit re-entry also permits a failed/transient read to retry.
            self.task.attempted = None;
        }
        match task_source_flow::task_choices() {
            Ok(choices) => self.task.view.choices = choices,
            Err(e) => {
                self.task.view.choices.clear();
                self.task.view.message = error(e);
            }
        }
        self.page = Page::Tasks {
            basis: key.basis.clone().ok_or("Open saved work first")?,
        };
        self.retry_task_source(gate)
    }
    fn retry_task_source(&mut self, gate: &Gate) -> Result<(), String> {
        gate.check()?;
        if self.task_matches_open() && self.task.flow.is_none() {
            // Reconstruct only the existing journal-bound task. A temporary
            // outage never authorizes replacing its identity or handoff.
            let focused = self.task.focused.clone();
            let choices = self.task.view.choices.clone();
            let restored = self.restore_task(gate);
            self.task.focused = focused;
            self.task.view.choices = choices;
            restored?;
            if let Err(e) = gate.check() {
                self.task.queued = None;
                self.task.view.can_review = false;
                return Err(e);
            }
        }
        self.poll_task(gate)?;
        self.retry_completed_review(gate)
    }
    pub(super) fn link_task(
        &mut self,
        task_id: &str,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        let opened = self.current(key)?;
        if self
            .journal
            .as_ref()
            .and_then(|j| j.value.task.as_ref())
            .is_some_and(|t| t.external.is_some())
        {
            return Err(
                "Finish or explicitly abandon the registered external intake before relinking"
                    .into(),
            );
        }
        // A reused registry ID is not the identity the user selected earlier.
        // Validate its original pins before considering any replacement flow.
        if self.task_matches_open() && self.task_link()?.task_id == task_id {
            super::super::task_source_machine::registered_task(self.task_link()?)?;
            return self.retry_task_source(gate);
        }
        let registry = crate::tasks::TaskRegistry::load();
        if let Some(e) = registry.load_error() {
            return Err(e.into());
        }
        let task = registry
            .entries()
            .iter()
            .find(|t| t.id == task_id && t.worktree_cleanup_completed_at.is_none())
            .ok_or("That task is no longer registered")?;
        let flow = TaskSourceFlow::link(&opened.association.identity.project_id, task_id)
            .map_err(error)?;
        let link = TaskAssociation {
            tool: opened.association.clone(),
            task_id: task.id.clone(),
            created_at: task.created_at.clone(),
            repository: PathBuf::from(&task.repository_path),
            worktree: PathBuf::from(&task.worktree_path),
            external: None,
            completed: None,
        };
        gate.check()?;
        self.journal(|j| j.task = Some(link))?;
        self.task = State {
            focused: self.task.focused.clone(),
            flow: Some(flow),
            ..Default::default()
        };
        self.task.view.linked = true;
        self.task.view.choices = task_source_flow::task_choices().map_err(error)?;
        self.poll_task(gate)
    }
    /// An explicit task entry/selection can retry interrupted copied analysis.
    /// Background polls never repeatedly retry unsupported completed input.
    fn retry_completed_review(&mut self, gate: &Gate) -> Result<(), String> {
        let Some(completed) = self.task.completed.clone() else {
            return Ok(());
        };
        if self.task.queued.is_some() {
            return Ok(());
        }
        gate.check()?;
        let link = self.task_link()?.clone();
        let disk = JournalFile::read_only(self.locations.as_ref().unwrap().path())?;
        if canonical_bytes(&disk.task).map_err(error)?
            != canonical_bytes(&Some(link.clone())).map_err(error)?
        {
            return Err(
                "The saved completed association changed; reopen it before retrying".into(),
            );
        }
        let saved = link
            .completed
            .as_ref()
            .ok_or("No exact completed request is retained")?;
        if saved.capture != completed.capture
            || saved.external.pending.request() != &completed.request
            || saved.external.basis != completed.basis
        {
            return Err("The completed source or its original request context changed".into());
        }
        super::super::task_source_machine::registered_task(&link)?;
        // Reuse the existing read-only recovery interface, locally on this
        // worker. This temporary validation never launches or publishes a job.
        let mut validation = TaskSourceFlow::interrupted(
            &link.tool.identity.project_id,
            &link.task_id,
            saved.external.pending.clone(),
        )
        .map_err(error)?;
        validation
            .reopen_external(&self.task_root()?, &gate.cancelled, &AtomicBool::new(false))
            .map_err(error)?;
        let exact = validation
            .intake_external(&completed.request.id, &AtomicBool::new(false))
            .map_err(error)?;
        gate.check()?;
        if !exact.is_some_and(|edit| {
            edit.capture() == &completed.capture && edit.request() == &completed.request
        }) {
            return Err("The completion no longer matches its original receipt and source".into());
        }
        self.queue_task_review(
            completed.capture,
            Some(completed.request),
            Some(&completed.basis),
            gate,
        )?;
        // A restart may have seen temporarily changed source. Only the exact
        // receipt/source/request and original-Basis checks above restore trust.
        self.task.completed.as_mut().unwrap().verified = true;
        self.task.view.can_review = true;
        self.task.view.message = "The exact saved completion was checked again on its original context; review is ready when the source is settled".into();
        Ok(())
    }
    fn queue_task_review(
        &mut self,
        capture: CapturedProgram,
        request: Option<DevelopmentRequest>,
        expected: Option<&Basis>,
        gate: &Gate,
    ) -> Result<(), String> {
        self.task.queued = None;
        let current = self.opened.as_ref().ok_or("Open the saved tool first")?;
        let fresh = current.store.load().map_err(error)?;
        let basis = Basis::capture(&fresh)?;
        if fresh != current.snapshot
            || self.today() != basis.day
            || expected.is_some_and(|b| b != &basis)
        {
            self.task.queued = None;
            return Err("Saved data or intentions changed while the external task was working. Its earlier evidence is stale; return to saved work before a fresh review".into());
        }
        let need = request
            .as_ref()
            .map(|r| r.request.clone())
            .unwrap_or_else(|| "Review the actual complete edits in the linked task".into());
        let flow = self.task.flow.as_ref().ok_or("Linked source unavailable")?;
        let checked = flow
            .analyze(&capture, &gate.cancelled, |capture| {
                #[cfg(test)]
                if let Some(pause) = &self.config.hooks.before_task_analysis {
                    pause.reached.store(true, Ordering::Release);
                    while !pause.release.load(Ordering::Acquire)
                        && !gate.cancelled.load(Ordering::Acquire)
                    {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    gate.check().map_err(AdapterError::Failed)?;
                }
                let mut change = ChangeDraft::new(fresh.clone(), capture.clone(), need)
                    .map_err(AdapterError::Failed)?;
                change.request = request;
                change
                    .prepare(&current.store, gate)
                    .map_err(AdapterError::Failed)?;
                change
                    .checked_view(&current.store, gate)
                    .map_err(AdapterError::Failed)?;
                Ok(change)
            })
            .map_err(error)?;
        gate.check()?;
        #[cfg(test)]
        self.pause_task_review_publication(false);
        if !gate.finish() {
            return Err("Task analysis was cancelled; its copied review was discarded".into());
        }
        self.task.queued = Some(checked);
        #[cfg(test)]
        self.pause_task_review_publication(true);
        Ok(())
    }
    pub(super) fn poll_task(&mut self, gate: &Gate) -> Result<(), String> {
        if !self.task_matches_open() {
            return Ok(());
        }
        if let Err(e) = super::super::task_source_machine::registered_task(self.task_link()?) {
            self.task.view.message =
                format!("Linked source unavailable: {e}. Saved daily work remains usable");
            self.task.unavailable = Some(e);
            self.task.view.can_review = false;
            self.task.queued = None;
            self.task.attempted = None;
            return Ok(());
        }
        self.task.unavailable = None;
        let Some(flow) = self.task.flow.as_mut() else {
            return Ok(());
        };
        flow.poll();
        gate.check()?;
        let pending = self.task_link()?.external.clone();
        if let Some(external) = pending {
            let request_id = external.pending.request().id.clone();
            let status = self.task.flow.as_ref().unwrap().view().external;
            if matches!(status, ExternalStatus::AwaitingCompletion { .. })
                && !self.task.terminal_write_blocked
            {
                // This signal means explicit handoff abandonment, never read
                // cancellation. A cancelled poll leaves the exact job pending.
                let handoff_cancelled = AtomicBool::new(false);
                let result = self
                    .task
                    .flow
                    .as_mut()
                    .unwrap()
                    .intake_external(&request_id, &handoff_cancelled);
                match result {
                    Ok(Some(edit)) => {
                        let completed = CompletedTask {
                            external: external.clone(),
                            capture: edit.capture().clone(),
                        };
                        let retained = (|| {
                            #[cfg(test)]
                            if let Some(pause) = &self.config.hooks.before_task_completion_commit {
                                pause.reached.store(true, Ordering::Release);
                                while !pause.release.load(Ordering::Acquire)
                                    && !gate.cancelled.load(Ordering::Acquire)
                                {
                                    std::thread::sleep(Duration::from_millis(1));
                                }
                            }
                            gate.check()?;
                            let disk =
                                JournalFile::read_only(self.locations.as_ref().unwrap().path())?;
                            if canonical_bytes(&disk.task).map_err(error)?
                                != canonical_bytes(&self.journal.as_ref().unwrap().value.task)
                                    .map_err(error)?
                            {
                                return Err("The task association changed before completion could be retained".into());
                            }
                            self.journal(|j| {
                                let task = j.task.as_mut().unwrap();
                                task.completed = Some(completed.clone());
                                task.external = None;
                            })
                        })();
                        if let Err(e) = retained {
                            // A read cancellation or failed bounded publication
                            // does not revoke or clear the original pending ticket.
                            let link = self.task_link()?;
                            self.task.flow = Some(
                                TaskSourceFlow::interrupted(
                                    &link.tool.identity.project_id,
                                    &link.task_id,
                                    external.pending.clone(),
                                )
                                .map_err(error)?,
                            );
                            self.task.terminal_write_blocked = true;
                            self.task.queued = None;
                            return Err(format!("Completed source was not admitted; original pending request was kept. {e}"));
                        }
                        self.task.completed = Some(Completed {
                            capture: edit.capture().clone(),
                            request: edit.request().clone(),
                            basis: external.basis.clone(),
                            verified: true,
                        });
                        if let Err(e) = self.queue_task_review(
                            edit.capture().clone(),
                            Some(edit.request().clone()),
                            Some(&external.basis),
                            gate,
                        ) {
                            self.task.view.message = format!("The completed request was saved, but copied review is unavailable. Select this task again to retry its original context. {e}");
                            return Err(e);
                        }
                        self.task.view.message = "Matching external completion captured; locally checked review is ready. No live author certification is implied".into();
                    }
                    Ok(None) => (),
                    Err(e) => {
                        self.task.view.message = format!(
                            "External completion rejected; saved work is unchanged. {}",
                            error(e)
                        )
                    }
                }
            }
        } else if let Some(completed) = &self.task.completed {
            // Intake's original request and saved Basis are immutable. A later
            // matching watcher event must never take the ordinary review path.
            let source = self.task.flow.as_ref().unwrap().last_capture();
            let original_queue = completed.verified
                && self.task.queued.as_ref().is_some_and(|q| {
                    q.source() == &completed.capture
                        && q.value().request.as_ref() == Some(&completed.request)
                        && Basis::capture(&q.value().snapshot).as_ref() == Ok(&completed.basis)
                });
            if (self.task.flow.as_ref().unwrap().view().source_status == SourceStatus::Current
                && source.is_some_and(|source| source != &completed.capture))
                || Basis::capture(&self.opened.as_ref().unwrap().snapshot)? != completed.basis
            {
                self.task.queued = None;
                self.task.view.message = "Source or saved work changed after the registered request. Its original evidence is stale; authorize a fresh request before using later edits".into();
            } else if !original_queue {
                self.task.queued = None;
            }
        } else if !matches!(
            self.task.flow.as_ref().unwrap().view().external,
            ExternalStatus::Cancelled { .. } | ExternalStatus::Rejected { .. }
        ) && self.task.flow.as_ref().unwrap().view().source_status
            == SourceStatus::Current
        {
            if let Some(capture) = self.task.flow.as_ref().unwrap().last_capture().cloned() {
                let basis = Basis::capture(&self.opened.as_ref().unwrap().snapshot)?;
                let attempt = (capture.binding.clone(), basis);
                // Reopening a review keeps its queue. A daily data/session
                // change rebuilds ordinary review, even without a source event.
                // Unsupported unchanged input is attempted only once per basis.
                if self.task.attempted.as_ref() != Some(&attempt) {
                    self.task.attempted = Some(attempt.clone());
                    if let Err(e) = self.queue_task_review(capture, None, None, gate) {
                        if gate.cancelled.load(Ordering::Acquire)
                            && self.task.attempted.as_ref() == Some(&attempt)
                        {
                            self.task.attempted = None;
                        }
                        self.task.view.message = e;
                    }
                }
            }
        }
        self.task.view.source = self.task.flow.as_ref().map(TaskSourceFlow::view);
        let current = self.opened.as_ref().unwrap();
        self.task.view.can_review = self
            .task
            .view
            .source
            .as_ref()
            .is_some_and(|v| v.source_status == SourceStatus::Current)
            && self
                .task
                .queued
                .as_ref()
                .is_some_and(|r| r.value().snapshot == current.snapshot);
        Ok(())
    }
    pub(super) fn review_task(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        self.current(key)?;
        self.check_task_key(key)?;
        let queued = self
            .task
            .queued
            .as_ref()
            .ok_or("Wait for a complete checked source review")?;
        let flow = self.task.flow.as_ref().ok_or("Linked source unavailable")?;
        flow.check_before_adoption(queued, &gate.cancelled)
            .map_err(error)?;
        let source = queued.source().clone();
        let current = self.opened.as_ref().unwrap();
        if queued.value().snapshot != current.store.load().map_err(error)? {
            return Err("This source review uses older saved data; reopen current work".into());
        }
        let mut page = queued.value().checked_view(&current.store, gate)?;
        page.origin = "Actual captured external task edits. Independent copied execution is local; no live author or service receipt is invented".into();
        let evidence = flow
            .analyze(&source, &gate.cancelled, |_| Ok(()))
            .map_err(error)?;
        let draft = queued.value().clone();
        #[cfg(test)]
        self.pause_task_review_publication(false);
        if !gate.finish() {
            return Err("Task review was cancelled; the previous page was kept".into());
        }
        self.task.evidence = Some((draft.operation.clone(), evidence));
        self.change = Some(draft);
        self.page = Page::Change(page);
        self.notice = "These are actual captured task edits with ExternalAuthor provenance. Try them on copied work; nothing has been adopted".into();
        #[cfg(test)]
        self.pause_task_review_publication(true);
        Ok(())
    }
    pub(super) fn task_request(
        &mut self,
        need: &str,
        provider: ProviderKind,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_pending()?;
        self.check_task_key(key)?;
        let current = self.current(key)?;
        if self.task_link()?.external.is_some() || self.ready.is_some() || self.generation_blocked {
            return Err("Resolve the existing generation or task intake first".into());
        }
        let capture = self
            .task
            .flow
            .as_ref()
            .and_then(|f| f.last_capture())
            .ok_or("Wait for complete task source")?
            .clone();
        let mut selected = current
            .store
            .runtime_view()
            .map_err(error)?
            .observation
            .selected;
        for focused in self
            .task
            .focused
            .iter()
            .chain(current.snapshot.session.focused_record.iter())
        {
            if !current
                .snapshot
                .data
                .records
                .iter()
                .any(|r| r.entity == focused.entity && r.id == focused.record)
            {
                return Err("The focused work no longer exists; return to saved work before requesting a change".into());
            }
            if !selected.contains(focused) {
                selected.push(focused.clone());
            }
        }
        let context = DevelopmentContext {
            view: Some(current.snapshot.session.view.clone()),
            selected,
            recent_inputs: self.recent_inputs.clone(),
            data_digest: Some(current.snapshot.data.identity().map_err(error)?),
            session_digest: Some(current.snapshot.session.identity().map_err(error)?),
        };
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(gate.cancelled.clone()),
            IntentArchive::new(current.store.clone()),
        );
        let mut request = engine
            .development_request(
                &current.snapshot,
                &id("task-request"),
                DevelopmentOperation::Modify,
                need,
                context,
            )
            .map_err(error)?;
        // First is the editable task baseline. Preserve all distinct historical
        // captures verbatim. Identical editable/saved-current source occurs once;
        // otherwise saved current remains last, as supplied by inheritance.
        request.sources.retain(|s| s.binding != capture.binding);
        request.sources.insert(0, capture.clone());
        self.task
            .flow
            .as_ref()
            .unwrap()
            .analyze(&capture, &gate.cancelled, |_| {
                request.validate()?;
                Ok(())
            })
            .map_err(error)?;
        let basis = Basis::capture(&current.snapshot)?;
        if current.store.load().map_err(error)? != current.snapshot || self.today() != basis.day {
            return Err("Saved work changed while preparing the task request".into());
        }
        let disclosure = task_source_host::Disclosure::new(request, provider)?;
        #[cfg(test)]
        self.pause_task_review_publication(false);
        if !gate.finish() {
            return Err("Task request was cancelled; the previous exact consent was kept".into());
        }
        self.task.ready = Some((disclosure.clone(), basis, key.epoch));
        self.task.view.consent = Some(disclosure);
        self.page = Page::Tasks {
            basis: key.basis.clone().unwrap(),
        };
        #[cfg(test)]
        self.pause_task_review_publication(true);
        Ok(())
    }
    pub(super) fn authorize_task(
        &mut self,
        digest: &Digest,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        // Consent binds the original request, not permission to bypass a
        // provider or write that became unresolved after disclosure.
        self.no_pending()?;
        if self.generation_blocked
            || self.ready.is_some()
            || self
                .journal
                .as_ref()
                .is_some_and(|j| j.value.provider.is_some())
        {
            return Err("Resolve the existing generation or unfinished operation before authorizing this task request".into());
        }
        self.check_task_key(key)?;
        let current = self.current(key)?;
        let (disclosure, basis, epoch) = self
            .task
            .ready
            .as_ref()
            .ok_or("Review a fresh exact task request first")?;
        if digest != &disclosure.digest
            || key.epoch != *epoch
            || key.basis.as_ref() != Some(basis)
            || Basis::capture(&current.store.load().map_err(error)?)? != *basis
            || self.today() != basis.day
        {
            return Err(
                "The source, data or prepared authorization changed; review a fresh request".into(),
            );
        }
        let link = self.task_link()?.clone();
        if link.external.is_some() {
            return Err("This task already has registered intake".into());
        }
        let task = super::super::task_source_machine::registered_task(&link)?;
        let flow = self.task.flow.as_ref().ok_or("Linked source unavailable")?;
        flow.analyze(&disclosure.request.sources[0], &gate.cancelled, |_| Ok(()))
            .map_err(error)?;
        let harness: &dyn TaskHarness = match disclosure.provider {
            ProviderKind::Codex => &CodexHarness,
            ProviderKind::Claude => &ClaudeHarness,
        };
        let availability = harness.availability();
        if !availability.available {
            return Err(availability.message);
        }
        let disclosure = disclosure.clone();
        let external = ExternalAssociation {
            pending: task_source_flow::PendingExternal::unissued(disclosure.request.clone())
                .map_err(error)?,
            basis: basis.clone(),
            provider: disclosure.provider,
            disclosure: disclosure.digest.clone(),
            approval_operation: key.operation.clone(),
            launch_attempted: false,
        };
        // A completed job's recovery flow may still be Interrupted because its
        // old receipt cannot reopen. Fresh explicit consent authorizes this new
        // request, never that historical flow or receipt. Validate the pinned
        // task and exact new baseline before replacing any durable association.
        let fresh_flow = if link.completed.is_some() {
            let flow = TaskSourceFlow::link(&link.tool.identity.project_id, &link.task_id)
                .map_err(error)?;
            flow.analyze(&disclosure.request.sources[0], &gate.cancelled, |_| Ok(()))
                .map_err(error)?;
            super::super::task_source_machine::registered_task(&link)?;
            Some(flow)
        } else {
            None
        };
        gate.check()?;
        self.journal(|j| {
            j.need = disclosure.request.request.clone();
            let task = j.task.as_mut().unwrap();
            task.external = Some(external);
            task.completed = None;
        })?;
        if let Some(flow) = fresh_flow {
            self.task.flow = Some(flow);
        }
        self.task.ready = None;
        self.task.view.consent = None;
        let root = self.task_root()?;
        crate::product_locations::Folder::ensure(&root).map_err(error)?;
        let invocation = self
            .task
            .flow
            .as_mut()
            .unwrap()
            .prepare_external(&root, &disclosure.request, &AtomicBool::new(false))
            .map_err(error)?;
        let pending = self
            .task
            .flow
            .as_ref()
            .unwrap()
            .pending_external()
            .ok_or("Prepared task lost its pending ticket")?
            .clone();
        self.journal(|j| j.task.as_mut().unwrap().external.as_mut().unwrap().pending = pending)?;
        #[cfg(test)]
        let product_executable = self
            .config
            .hooks
            .task_product_executable
            .clone()
            .unwrap_or(std::env::current_exe().map_err(error)?);
        #[cfg(not(test))]
        let product_executable = std::env::current_exe().map_err(error)?;
        let bundle = ProductTaskBundle {
            task_id: link.task_id.clone(),
            request_id: invocation.request().id.clone(),
            request_digest: invocation
                .request()
                .identity()
                .map_err(error)?
                .as_str()
                .into(),
            request_path: invocation.request_path().into(),
            product_executable,
            worktree: invocation.worktree().into(),
        };
        bundle.prompt(&task)?;
        self.task
            .flow
            .as_ref()
            .unwrap()
            .analyze(invocation.baseline(), &gate.cancelled, |_| Ok(()))
            .map_err(error)?;
        let mut registry = crate::tasks::TaskRegistry::load();
        registry.check_provider_start(&task).map_err(error)?;
        let fresh_task = super::super::task_source_machine::registered_task(self.task_link()?)?;
        if fresh_task.provider_ref != task.provider_ref
            || fresh_task.session_ref != task.session_ref
        {
            return Err(
                "The registered task provider changed before launch; nothing was launched".into(),
            );
        }
        let disk = JournalFile::read_only(self.locations.as_ref().unwrap().path())?;
        if canonical_bytes(&disk.task).map_err(error)?
            != canonical_bytes(&self.journal.as_ref().unwrap().value.task).map_err(error)?
        {
            return Err(
                "The registered request changed before launch; nothing was launched".into(),
            );
        }
        let mut checked = TaskSourceFlow::interrupted(
            &link.tool.identity.project_id,
            &link.task_id,
            self.task_link()?.external.as_ref().unwrap().pending.clone(),
        )
        .map_err(error)?;
        checked
            .reopen_external(&root, &gate.cancelled, &AtomicBool::new(false))
            .map_err(error)?;
        self.task.flow = Some(checked);
        self.journal(|j| {
            j.task
                .as_mut()
                .unwrap()
                .external
                .as_mut()
                .unwrap()
                .launch_attempted = true
        })?;
        gate.commit()?;
        let session = harness.start_product_task(&task, &bundle).map_err(|e| format!("Harness launch failed or its outcome is uncertain. The exact request is retained; no automatic relaunch will occur. {e}"))?;
        registry.record_provider_start(&task.id, task.provider_ref.as_deref(), task.session_ref.as_deref(), harness.provider_ref(), session.as_deref()).map_err(|e| format!("Harness launch was requested, but recording its session failed. Do not relaunch blindly: {e}"))?;
        self.task.view.source = self.task.flow.as_ref().map(TaskSourceFlow::view);
        self.task.view.message = "Harness launch requested with the exact bundle. Waiting for machine submission; terminal launch or exit is not completion".into();
        self.task.queued = None;
        self.task.completed = None;
        self.task.attempted = None;
        self.task.terminal_write_blocked = false;
        Ok(())
    }
    pub(super) fn reopen_task(&mut self, request: &str, gate: &Gate) -> Result<(), String> {
        let link = self.task_link()?.clone();
        if link
            .external
            .as_ref()
            .is_none_or(|e| e.pending.request().id != request)
        {
            return Err("That request is not registered for intake".into());
        }
        super::super::task_source_machine::registered_task(&link)?;
        let pending = link.external.as_ref().unwrap().pending.clone();
        self.task.flow = Some(
            TaskSourceFlow::interrupted(&link.tool.identity.project_id, &link.task_id, pending)
                .map_err(error)?,
        );
        let root = self.task_root()?;
        self.task
            .flow
            .as_mut()
            .ok_or("Source unavailable")?
            .reopen_external(&root, &gate.cancelled, &AtomicBool::new(false))
            .map_err(error)?;
        self.task.view.source = self.task.flow.as_ref().map(TaskSourceFlow::view);
        self.task.terminal_write_blocked = false;
        self.poll_task(gate)
    }
    pub(super) fn abandon_task(&mut self, request: &str, gate: &Gate) -> Result<(), String> {
        if self
            .task_link()?
            .external
            .as_ref()
            .is_none_or(|e| e.pending.request().id != request)
        {
            return Err("That request is no longer pending".into());
        }
        #[cfg(test)]
        self.before_abandon(gate);
        gate.commit()?;
        // Durable revocation comes first, even if the task disappeared and the
        // in-memory flow cannot be recreated. Late submission then has no authority.
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_task_abandon_write {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.journal(|j| j.task.as_mut().unwrap().external = None)?;
        if let Some(flow) = &mut self.task.flow {
            let _ = flow.cancel_external(request);
        }
        self.task.ready = None;
        self.task.queued = None;
        self.task.evidence = None;
        self.task.view.consent = None;
        self.task.view.source = self.task.flow.as_ref().map(TaskSourceFlow::view);
        self.task.view.message = "Local intake abandoned. This did not stop an independent author or undo its source edits".into();
        Ok(())
    }
    fn check_task_key(&self, key: &Key) -> Result<(), String> {
        let link = self.task_link()?;
        super::super::task_source_machine::registered_task(link)?;
        if key.task.as_ref() != Some(&task_source_host::Link::from(link))
            || key.task_source.as_ref()
                != self
                    .task
                    .flow
                    .as_ref()
                    .and_then(|f| f.last_capture())
                    .map(|c| &c.binding)
        {
            return Err("The linked task source changed after that command was chosen. Review the current source and choose again".into());
        }
        Ok(())
    }
    pub(super) fn retained_task_evidence(
        &self,
        source: &CapturedProgram,
        gate: &Gate,
    ) -> Result<Option<SourceAnalysis<()>>, String> {
        let Some(task) = &source.binding.task else {
            return Ok(None);
        };
        if !self.task_matches_open() || self.task_link()?.task_id != task.task_id {
            return Err("The retained choice's original task is not linked to this tool".into());
        }
        super::super::task_source_machine::registered_task(self.task_link()?)?;
        // The archived capture is the authority being checked. A newer watcher
        // capture must never replace its original bytes or full fingerprint.
        self.task
            .flow
            .as_ref()
            .ok_or("The retained choice's linked task is unavailable")?
            .analyze(source, &gate.cancelled, |_| Ok(()))
            .map(Some)
            .map_err(error)
    }
    pub(super) fn has_task_evidence(&self, operation: &str) -> bool {
        self.task
            .evidence
            .as_ref()
            .is_some_and(|(id, _)| id == operation)
    }
    pub(super) fn check_task_evidence(&self, operation: &str, gate: &Gate) -> Result<(), String> {
        if let Some((_, evidence)) = self
            .task
            .evidence
            .as_ref()
            .filter(|(id, _)| id == operation)
        {
            if !self.task_matches_open() {
                return Err("This source review belongs to another tool".into());
            }
            super::super::task_source_machine::registered_task(self.task_link()?)?;
            self.task
                .flow
                .as_ref()
                .ok_or("Linked task unavailable")?
                .check_before_adoption(evidence, &gate.cancelled)
                .map_err(error)?;
        }
        Ok(())
    }
    pub(super) fn inspect_task(&mut self, request: &str) -> Result<(), String> {
        let graph = self
            .task
            .flow
            .as_ref()
            .ok_or("Linked source unavailable")?
            .inspect_decisions(request)
            .map_err(error)?;
        self.task.view.inspection = Some(serde_json::to_string_pretty(graph).map_err(error)?);
        Ok(())
    }
    pub(super) fn decline_task(&mut self) {
        self.task.ready = None;
        self.task.view.consent = None;
    }
}
