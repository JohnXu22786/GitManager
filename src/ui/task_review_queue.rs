use crate::app::App;
use crate::git_ops::TaskReviewQueueState;
use crate::task_delivery::{
    CheckState, PullRequestAction, PullRequestCheck, PullRequestRemoteView,
    PullRequestSnapshot, TaskSourceFreshness,
};
use crate::task_verification::VerificationState;
use crate::tasks::TaskRecord;
use eframe::egui;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading("Task review queue");
    ui.label(
        "Review disposition applies to one exact task source snapshot. Verification status is separate; reviewed does not mean approved, merged, or verified.",
    );

    if let Some(error) = app.task_registry.load_error() {
        ui.colored_label(
            App::adaptive_red(ui.style().visuals.dark_mode),
            format!("Task registry could not be loaded; its file was left untouched: {error}"),
        );
    }

    let entries = app.task_registry.entries().to_vec();
    if entries.is_empty() {
        ui.add_space(8.0);
        ui.label(if app.task_registry.load_error().is_some() {
            "Task records are unavailable because the registry could not be read."
        } else {
            "No task records are linked yet."
        });
        crate::ui::task_panel::render_task_diff_review(app, ui);
        return;
    }

    let task_states = entries
        .into_iter()
        .map(|task| {
            let state = app.current_task_review_queue_state(ctx, &task);
            (task, state)
        })
        .collect::<Vec<_>>();
    let mut open_task: Option<TaskRecord> = None;
    let mut delivery_action: Option<(
        TaskRecord,
        PullRequestAction,
        Option<String>,
        Option<String>,
    )> = None;

    let reviewable = task_states
        .iter()
        .filter(|(_, state)| matches!(state, Some(TaskReviewQueueState::Reviewable { .. })))
        .collect::<Vec<_>>();
    ui.add_space(10.0);
    ui.heading(format!("Reviewable changes ({})", reviewable.len()));
    if reviewable.is_empty() {
        ui.label("No task has a confirmed reviewable change set yet.");
    }
    for (task, state) in reviewable {
        let Some(TaskReviewQueueState::Reviewable {
            source_fingerprint,
            changed_file_count,
        }) = state.as_ref()
        else {
            continue;
        };
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&task.title).strong());
                ui.label(format!("{changed_file_count} changed file(s)"));
                if ui
                    .add_enabled(!app.is_busy(), egui::Button::new("Open complete diff"))
                    .clicked()
                {
                    open_task = Some(task.clone());
                }
            });
            ui.label(format!("Worktree: {}", task.worktree_path));
            ui.label(review_disposition(task, source_fingerprint));
            ui.label(format!(
                "Verification: {}",
                verification_status(
                    task,
                    Some(source_fingerprint),
                    app.task_verification_is_running(&task.id),
                )
            ));
            render_verification_details(ui, task);
            render_task_delivery(
                app,
                ui,
                ctx,
                task,
                Some(source_fingerprint.as_str()),
                &mut delivery_action,
            );
        });
    }

    let not_ready = task_states
        .iter()
        .filter(|(_, state)| !matches!(state, Some(TaskReviewQueueState::Reviewable { .. })))
        .collect::<Vec<_>>();
    ui.add_space(10.0);
    ui.heading(format!("Not ready to review ({})", not_ready.len()));
    for (task, state) in not_ready {
        let verification_fingerprint = match state.as_ref() {
            Some(TaskReviewQueueState::NoReviewableChanges { source_fingerprint }) => {
                Some(source_fingerprint.clone())
            }
            Some(TaskReviewQueueState::Reviewable { source_fingerprint, .. }) => {
                Some(source_fingerprint.clone())
            }
            _ => app
                .current_task_fingerprint(ctx, task)
                .and_then(Result::ok),
        };
        let delivery_fingerprint = match state.as_ref() {
            Some(TaskReviewQueueState::NoReviewableChanges { source_fingerprint })
            | Some(TaskReviewQueueState::Reviewable { source_fingerprint, .. }) => {
                Some(source_fingerprint.as_str())
            }
            _ => verification_fingerprint.as_deref(),
        };
        ui.group(|ui| {
            ui.label(egui::RichText::new(&task.title).strong());
            match state {
                Some(TaskReviewQueueState::NoReviewableChanges { source_fingerprint }) => {
                    ui.label("No reviewable changes since this task's saved base.");
                    ui.label(format!(
                        "Verification: {}",
                        verification_status(
                            task,
                            Some(source_fingerprint),
                            app.task_verification_is_running(&task.id),
                        )
                    ));
                    render_verification_details(ui, task);
                }
                Some(TaskReviewQueueState::Unavailable(reason)) => {
                    ui.colored_label(
                        App::adaptive_yellow(ui.style().visuals.dark_mode),
                        format!("Complete task diff unavailable: {reason}"),
                    );
                    ui.label(format!(
                        "Verification: {}",
                        verification_status(
                            task,
                            verification_fingerprint.as_deref(),
                            app.task_verification_is_running(&task.id),
                        )
                    ));
                    render_verification_details(ui, task);
                }
                Some(TaskReviewQueueState::Error(reason)) => {
                    ui.colored_label(
                        App::adaptive_red(ui.style().visuals.dark_mode),
                        format!("Could not check task changes: {reason}"),
                    );
                    ui.label(format!(
                        "Verification: {}",
                        verification_status(
                            task,
                            verification_fingerprint.as_deref(),
                            app.task_verification_is_running(&task.id),
                        )
                    ));
                    render_verification_details(ui, task);
                }
                Some(TaskReviewQueueState::Reviewable { .. }) => {}
                None => {
                    ui.label("Checking for a complete, reviewable task diff…");
                    ui.label(format!(
                        "Verification: {}",
                        verification_status(
                            task,
                            verification_fingerprint.as_deref(),
                            app.task_verification_is_running(&task.id),
                        )
                    ));
                }
            }
            render_task_delivery(
                app,
                ui,
                ctx,
                task,
                delivery_fingerprint,
                &mut delivery_action,
            );
        });
    }

    if let Some(task) = open_task {
        app.start_task_diff_review(ctx, &task);
    }
    if let Some((task, action, identifier, source_fingerprint)) = delivery_action {
        app.start_task_pull_request_action(ctx, &task, action, identifier, source_fingerprint);
    }
    crate::ui::task_panel::render_task_diff_review(app, ui);
}

fn render_task_delivery(
    app: &mut App,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    task: &TaskRecord,
    current_source_fingerprint: Option<&str>,
    next_action: &mut Option<(
        TaskRecord,
        PullRequestAction,
        Option<String>,
        Option<String>,
    )>,
) {
    let status = app.current_task_pull_request_status(ctx, task);
    let action_running = app.task_pull_request_action_running(&task.id);
    ui.separator();
    ui.label(egui::RichText::new("PR and remote CI").strong());
    match &status.remote {
        PullRequestRemoteView::Unlinked => {
            ui.label("Unlinked · create a PR or associate an existing one.");
            ui.label("GitHub CLI (`gh`) uses its existing sign-in; Git Manager stores no credentials.");
        }
        PullRequestRemoteView::Checking { action } => {
            ui.label(format!("Checking · {action}…"));
            ui.label("Source: GitHub via `gh`.");
        }
        PullRequestRemoteView::Refreshing {
            previous,
            previous_at,
        } => {
            ui.label("Refreshing remote status · previous results are not current.");
            if let Some(previous) = previous {
                render_pull_request_snapshot(
                    ui,
                    &task.id,
                    previous,
                    previous_at.as_deref(),
                    true,
                );
            } else if let Some(previous_at) = previous_at {
                ui.label(format!("Last response: {previous_at}"));
            } else {
                ui.label("Source: GitHub via `gh` · waiting for a remote response.");
            }
        }
        PullRequestRemoteView::Unavailable {
            message,
            previous,
            previous_at,
        } => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!("Unavailable · {message}"),
            );
            ui.label("Source: GitHub via `gh` · current remote status could not be confirmed.");
            if let Some(previous) = previous {
                ui.label("Last known result; its current status could not be confirmed:");
                render_pull_request_snapshot(
                    ui,
                    &task.id,
                    previous,
                    previous_at.as_deref(),
                    true,
                );
            }
        }
        PullRequestRemoteView::Ready { snapshot } => {
            render_pull_request_snapshot(
                ui,
                &task.id,
                snapshot,
                Some(&snapshot.fetched_at),
                false,
            );
        }
    }

    if let Some(message) = status.action_message {
        let color = if message.succeeded {
            App::adaptive_green(ui.style().visuals.dark_mode)
        } else {
            App::adaptive_red(ui.style().visuals.dark_mode)
        };
        ui.colored_label(color, message.text);
    }

    let linked_url = app
        .task_registry
        .entries()
        .iter()
        .find(|entry| entry.id == task.id)
        .map_or(task.pull_request_url.as_deref(), |entry| {
            entry.pull_request_url.as_deref()
        });
    let pr_is_linked = linked_url.is_some();
    ui.horizontal_wrapped(|ui| {
        if let Some(url) = linked_url
            .filter(|url| url.starts_with("https://") && !url.chars().any(char::is_whitespace))
        {
            ui.hyperlink_to("Open PR", url);
        }
        if pr_is_linked && !action_running
            && ui.button("Refresh PR status").clicked()
        {
            *next_action = Some((
                task.clone(),
                PullRequestAction::Refresh,
                None,
                current_source_fingerprint.map(str::to_owned),
            ));
        }
    });

    ui.horizontal(|ui| {
        let input = app.task_pull_request_input(&task.id);
        ui.add(
            egui::TextEdit::singleline(input)
                .hint_text("PR number or HTTPS URL")
                .desired_width(230.0),
        );
        let can_associate = !input.trim().is_empty() && !action_running;
        if ui
            .add_enabled(
                can_associate,
                egui::Button::new(if pr_is_linked {
                    "Change PR association"
                } else {
                    "Associate PR"
                }),
            )
            .clicked()
        {
            *next_action = Some((
                task.clone(),
                PullRequestAction::Associate,
                Some(input.trim().to_string()),
                current_source_fingerprint.map(str::to_owned),
            ));
        }
        if !pr_is_linked {
            let can_create = task.branch.as_deref().is_some_and(|branch| !branch.is_empty())
                && !action_running;
            if ui
                .add_enabled(can_create, egui::Button::new("Create PR"))
                .clicked()
            {
                *next_action = Some((
                    task.clone(),
                    PullRequestAction::Create,
                    None,
                    current_source_fingerprint.map(str::to_owned),
                ));
            }
            if task.branch.as_deref().map_or(true, |branch| branch.is_empty()) {
                ui.label("Create PR requires a task linked to a branch; you can still associate an existing PR.");
            }
        }
    });
}

fn render_pull_request_snapshot(
    ui: &mut egui::Ui,
    task_id: &str,
    snapshot: &PullRequestSnapshot,
    fetched_at: Option<&str>,
    previous: bool,
) {
    let lifecycle = match snapshot.state.to_ascii_uppercase().as_str() {
        "MERGED" => "Merged",
        "CLOSED" => "Closed",
        "OPEN" if snapshot.is_draft => "Draft",
        "OPEN" => "Open",
        _ => "Unavailable",
    };
    ui.label(format!(
        "PR #{} · {} · {} → {} · {}",
        snapshot.number, lifecycle, snapshot.head_branch, snapshot.base_branch, snapshot.title
    ));
    let sha = short_sha(&snapshot.head_sha);
    let stale_prefix = if previous { "Last known · " } else { "" };
    match &snapshot.freshness {
        TaskSourceFreshness::Current { local_head_sha } => {
            if previous {
                ui.label(format!(
                    "{stale_prefix}remote checks {} on PR head {}; this result is not current.",
                    snapshot.check_state.label(),
                    short_sha(local_head_sha)
                ));
            } else {
                match snapshot.check_state {
                    CheckState::NoChecks => {
                        ui.label(format!("No checks reported by GitHub for PR head {sha}."));
                    }
                    _ => {
                        ui.label(format!(
                            "Remote checks: {} on PR head {sha}.",
                            snapshot.check_state.label()
                        ));
                    }
                };
                ui.label(format!(
                    "Freshness: task HEAD {} matches the PR head and the worktree is clean.",
                    short_sha(local_head_sha)
                ));
            }
        }
        TaskSourceFreshness::Stale {
            local_head_sha,
            reason,
        } => {
            let local = local_head_sha
                .as_deref()
                .map(short_sha)
                .unwrap_or_else(|| "unavailable".into());
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!(
                    "Stale · GitHub checks {} on PR head {sha}; local task HEAD {local}: {reason}.",
                    snapshot.check_state.label()
                ),
            );
        }
        TaskSourceFreshness::Unavailable { reason } => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!(
                    "Freshness unavailable · GitHub checks {} on PR head {sha}; {reason}.",
                    snapshot.check_state.label()
                ),
            );
        }
    }
    if let Some(fetched_at) = fetched_at {
        ui.label(format!("Source: GitHub via `gh` · fetched {fetched_at}"));
    }
    render_check_details(ui, task_id, &snapshot.checks);
}

fn render_check_details(ui: &mut egui::Ui, task_id: &str, checks: &[PullRequestCheck]) {
    if checks.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("GitHub checks ({})", checks.len()))
        .id_salt((task_id, "task_pull_request_checks", checks.len()))
        .show(ui, |ui| {
            for check in checks {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} · {}", check.name, check.state.label()));
                    if let Some(url) = &check.details_url {
                        ui.hyperlink_to("Details", url);
                    }
                });
            }
        });
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn review_disposition(task: &TaskRecord, current_fingerprint: &str) -> String {
    match task.reviewed_source_fingerprint.as_deref() {
        Some(reviewed) if reviewed == current_fingerprint => format!(
            "Reviewed for this source snapshot on {}",
            task.reviewed_at.as_deref().unwrap_or("an unknown date")
        ),
        Some(_) => "Stale review · task source changed since it was reviewed".into(),
        None => "Pending review".into(),
    }
}

fn verification_status(
    task: &TaskRecord,
    current_fingerprint: Option<&str>,
    running_locally: bool,
) -> String {
    let Some(result) = &task.verification_result else {
        return "Missing".into();
    };
    if result.state == VerificationState::Running {
        return if running_locally {
            "Running".into()
        } else {
            "Stale · saved run did not finish".into()
        };
    }
    if task.verification_command.as_ref() != Some(&result.command) {
        return "Stale · command changed".into();
    }
    let Some(saved_fingerprint) = result.source_fingerprint.as_deref() else {
        return "Stale · result has no source fingerprint".into();
    };
    let Some(current_fingerprint) = current_fingerprint else {
        return "Freshness unavailable".into();
    };
    if saved_fingerprint != current_fingerprint {
        return "Stale".into();
    }
    match result.state {
        VerificationState::Passed => "Passed".into(),
        VerificationState::Failed => "Failed".into(),
        VerificationState::Cancelled => "Failed · cancelled".into(),
        VerificationState::TimedOut => "Failed · timed out".into(),
        VerificationState::Error => "Failed · could not run".into(),
        VerificationState::Running => "Running".into(),
    }
}

fn render_verification_details(ui: &mut egui::Ui, task: &TaskRecord) {
    egui::CollapsingHeader::new("Verification result details")
        .id_salt(("review_queue_verification", &task.id))
        .show(ui, |ui| {
            let Some(result) = &task.verification_result else {
                ui.label("No verification result has been saved for this task.");
                return;
            };
            ui.label(format!("Command: {} {:?}", result.command.executable, result.command.args));
            ui.label(format!("Result: {}", result.state.label()));
            ui.label(format!("Started: {}", result.started_at));
            if let Some(finished_at) = &result.finished_at {
                ui.label(format!("Finished: {finished_at}"));
            }
            if let Some(exit_code) = result.exit_code {
                ui.label(format!("Exit code: {exit_code}"));
            }
            if result.output_truncated {
                ui.label("Captured output was truncated.");
            }
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    if !result.stdout.is_empty() {
                        ui.label("stdout");
                        ui.monospace(&result.stdout);
                    }
                    if !result.stderr.is_empty() {
                        ui.label("stderr");
                        ui.monospace(&result.stderr);
                    }
                    if result.stdout.is_empty() && result.stderr.is_empty() {
                        ui.label("No output captured.");
                    }
                });
        });
}
