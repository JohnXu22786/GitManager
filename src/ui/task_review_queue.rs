use crate::app::App;
use crate::git_ops::TaskReviewQueueState;
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
        });
    }

    if let Some(task) = open_task {
        app.start_task_diff_review(ctx, &task);
    }
    crate::ui::task_panel::render_task_diff_review(app, ui);
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
