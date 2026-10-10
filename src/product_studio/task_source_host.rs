//! UI data for the existing worker's linked task. No filesystem work on egui.
use super::*;
use task_source_flow::{ExternalStatus, SourceStatus, TaskChoice};

// Private in-memory pins for a queued command. Existing TaskRegistry and the
// journal remain the authorities; this is not another registry or ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Link {
    tool: Association,
    task_id: Id,
    created_at: String,
    repository: PathBuf,
    worktree: PathBuf,
}
impl From<&journal::TaskAssociation> for Link {
    fn from(link: &journal::TaskAssociation) -> Self {
        Self {
            tool: link.tool.clone(),
            task_id: link.task_id.clone(),
            created_at: link.created_at.clone(),
            repository: link.repository.clone(),
            worktree: link.worktree.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct View {
    pub link: Option<Link>,
    pub choices: Vec<TaskChoice>,
    pub source: Option<task_source_flow::View>,
    pub message: String,
    pub linked: bool,
    pub can_review: bool,
    pub consent: Option<Disclosure>,
    pub inspection: Option<String>,
    pub pending: Option<Id>,
}
#[derive(Clone)]
pub(super) struct Disclosure {
    pub request: DevelopmentRequest,
    pub provider: ProviderKind,
    pub digest: Digest,
    pub review: String,
}
impl Disclosure {
    pub fn new(request: DevelopmentRequest, provider: ProviderKind) -> Result<Self, String> {
        request.validate().map_err(error)?;
        let review = serde_json::to_string_pretty(&request).map_err(error)?;
        if serde_json::to_vec(&request).map_err(error)?.len() > MAX_WIRE_BYTES {
            return Err("The complete request and retained intentions exceed the handoff limit. Nothing was sent or truncated".into());
        }
        let digest = canonical_digest(
            IdentityDomain::Evidence,
            &("task-handoff-consent/1", provider, &request),
        )
        .map_err(error)?;
        Ok(Self {
            request,
            provider,
            digest,
            review,
        })
    }
}
impl View {
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        trace: &mut WidgetTrace,
        interactive: bool,
        need: &mut String,
        provider: &mut ProviderKind,
    ) -> Option<Action> {
        let mut action = None;
        trace.label(ui, "Existing development tasks");
        trace.label(ui, "Link a task already registered in GitManager. Saved daily work stays separate from external edits and remains usable.");
        if !self.message.is_empty() {
            trace.label(ui, self.message.clone());
        }
        if let Some(disclosure) = &self.consent {
            trace.label(
                ui,
                format!("Review the exact request for {:?}", disclosure.provider),
            );
            trace.label(ui, "This sends the request below, captured task source, selected work context, and retained intention examples to your existing configured Harness service. Examples can include saved business records; review them before sharing.");
            trace.label(ui, "Effective host context and tool permissions are not independently attested. This is not a DataOnly invocation. The Harness may access additional worktree or host context under its existing configuration. No permissions are added here.");
            trace.label(ui, "Existing account usage charges may apply; GitManager cannot quote them. Do not authorize if the current service or its permissions are unsuitable. No new subscription or purchase is approved by this button.");
            ui.collapsing("Exact source, context and intentions to share", |ui| {
                let mut review = disclosure.review.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut review)
                        .interactive(false)
                        .desired_rows(18),
                );
            });
            if trace.button(
                ui,
                "studio.external-authorize",
                "Authorize this request and open the configured Harness",
                interactive,
            ) {
                action = Some(Action::TaskAuthorize {
                    disclosure: disclosure.digest.clone(),
                });
            }
            if trace.button(
                ui,
                "studio.external-decline",
                "Keep working without sending",
                interactive,
            ) {
                action = Some(Action::TaskDecline);
            }
        } else {
            if self.choices.is_empty() {
                trace.label(ui, "No registered development tasks are available. Existing Git task controls can register a worktree.");
            }
            let pending = self.pending.is_some();
            for task in &self.choices {
                if trace.button(
                    ui,
                    &format!("studio.task.{}", task.task_id),
                    &task.title,
                    interactive && !pending,
                ) {
                    action = Some(Action::TaskLink {
                        task_id: task.task_id.clone(),
                    });
                }
            }
            if let Some(source) = &self.source {
                match &source.source_status {
                    SourceStatus::Current => trace.label(
                        ui,
                        "Complete source captured; independent review is still required",
                    ),
                    SourceStatus::Pending(reason)
                    | SourceStatus::Unsupported(reason)
                    | SourceStatus::Unavailable(reason) => {
                        trace.label(ui, format!("Source unavailable or changing: {reason}"))
                    }
                }
                if let Some(last) = &source.last_capture {
                    trace.label(ui, format!("Last complete source: {}", last.label));
                    if source.last_capture_stale {
                        trace.label(
                            ui,
                            "This last complete version is stale and cannot be adopted",
                        );
                    }
                }
                if trace.button(
                    ui,
                    "studio.task-review",
                    "Review complete task changes on copied work",
                    interactive && self.can_review && !pending,
                ) {
                    action = Some(Action::TaskReview);
                }
                trace.label(ui, "What should work differently in this task?");
                trace.control(
                    "studio.task-need",
                    ui.add_enabled(
                        interactive && !pending,
                        egui::TextEdit::multiline(need).desired_rows(3),
                    ),
                );
                ui.horizontal(|ui| {
                    ui.selectable_value(provider, ProviderKind::Codex, "Codex");
                    ui.selectable_value(provider, ProviderKind::Claude, "Claude Code");
                });
                if trace.button(
                    ui,
                    "studio.external-request",
                    "Review a request for the external Harness",
                    interactive
                        && !pending
                        && source.source_status == SourceStatus::Current
                        && !need.trim().is_empty(),
                ) {
                    action = Some(Action::TaskRequest {
                        need: need.clone(),
                        provider: *provider,
                    });
                }
                let request = match &source.external {
                    ExternalStatus::None => None,
                    ExternalStatus::AwaitingCompletion { request_id } => {
                        trace.label(ui, "Waiting for registered completion. Opening a terminal is not proof of generation or success.");
                        Some(request_id)
                    }
                    ExternalStatus::Interrupted { request_id, reason }
                    | ExternalStatus::Rejected { request_id, reason } => {
                        trace.label(ui, reason.clone());
                        Some(request_id)
                    }
                    ExternalStatus::Captured { request_id } => {
                        trace.label(ui, "External edits captured. Review and local execution are required before adoption.");
                        Some(request_id)
                    }
                    ExternalStatus::Cancelled { request_id } => {
                        trace.label(ui, "Local intake abandoned. The independently running author was not stopped and may still edit files.");
                        Some(request_id)
                    }
                };
                if let Some(request_id) = request {
                    if trace.button(
                        ui,
                        "studio.task-inspect",
                        "Inspect the exact inherited intentions",
                        interactive,
                    ) {
                        action = Some(Action::TaskInspect {
                            request_id: request_id.clone(),
                        });
                    }
                    if source.can_reopen
                        && trace.button(
                            ui,
                            "studio.task-reopen",
                            "Reopen this saved intake without relaunching",
                            interactive,
                        )
                    {
                        action = Some(Action::TaskReopen {
                            request_id: request_id.clone(),
                        });
                    }
                    if pending
                        && trace.button(
                            ui,
                            "studio.task-abandon",
                            "Abandon this local intake; do not stop the author",
                            interactive,
                        )
                    {
                        action = Some(Action::TaskAbandon {
                            request_id: request_id.clone(),
                        });
                    }
                }
            }
        }
        if self.source.is_none() {
            if let Some(request_id) = &self.pending {
                trace.label(
                    ui,
                    "An interrupted local intake is still registered; saved daily work is usable",
                );
                if trace.button(
                    ui,
                    "studio.task-reopen",
                    "Reopen this saved intake without relaunching",
                    interactive,
                ) {
                    action = Some(Action::TaskReopen {
                        request_id: request_id.clone(),
                    });
                }
                if trace.button(
                    ui,
                    "studio.task-abandon",
                    "Abandon this local intake; do not stop the author",
                    interactive,
                ) {
                    action = Some(Action::TaskAbandon {
                        request_id: request_id.clone(),
                    });
                }
            }
        }
        if let Some(inspection) = &self.inspection {
            ui.collapsing("Inherited intentions", |ui| {
                ui.label(inspection);
            });
        }
        if trace.button(
            ui,
            "studio.tasks-back",
            "Return to saved daily work",
            interactive,
        ) {
            action = Some(Action::ReturnDaily);
        }
        action
    }
}
