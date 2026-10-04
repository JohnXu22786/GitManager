use crate::app::{App, Tab, TaskVerificationCommandDraft};
use crate::git_ops::TaskDiffReviewState;
use crate::harness::{ClaudeHarness, CodexHarness, ResumeCapability, TaskHarness};
use crate::tasks::TaskRecord;
use crate::task_verification::{VerificationCommand, VerificationState};
use eframe::egui;
use std::path::Path;

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.heading("Tasks");
        if crate::ui::ellipsis_button(ui, "＋ Link existing workspace").clicked() {
            app.task_form_open = true;
            app.task_title.clear();
            app.task_worktree_path = app.repo_path.clone();
        }
    });
    ui.label("Local task records linked to existing Git workspaces.");

    let codex = CodexHarness;
    let codex_capabilities = codex.capabilities();
    let codex_availability = codex.availability();
    let claude = ClaudeHarness;
    let claude_capabilities = claude.capabilities();
    let claude_availability = claude.availability();
    let availability_color = if codex_availability.available {
        App::adaptive_green(ui.style().visuals.dark_mode)
    } else {
        App::adaptive_yellow(ui.style().visuals.dark_mode)
    };
    ui.colored_label(availability_color, &codex_availability.message);
    if !codex_capabilities.tracks_sessions
        && codex_capabilities.resume == ResumeCapability::UserSelectsSession
    {
        ui.label(
            "New Codex session IDs are not captured; tasks without a saved ID use Codex's worktree-filtered picker.",
        );
    }
    let claude_availability_color = if claude_availability.available {
        App::adaptive_green(ui.style().visuals.dark_mode)
    } else {
        App::adaptive_yellow(ui.style().visuals.dark_mode)
    };
    ui.colored_label(claude_availability_color, &claude_availability.message);
    if claude_availability.available
        && claude_capabilities.continue_latest
        && !claude_capabilities.tracks_sessions
    {
        ui.label(
            "Claude Code can continue the latest conversation in this worktree, but cannot identify which task session it belongs to.",
        );
    }

    if let Some(error) = app.task_registry.load_error() {
        ui.colored_label(
            App::adaptive_red(ui.style().visuals.dark_mode),
            format!("Task registry could not be loaded; its file was left untouched: {error}"),
        );
    }

    let mut browse_for_worktree = false;
    let mut create_task = false;
    if app.task_form_open {
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("New task").strong());
            ui.label("Title or goal");
            ui.text_edit_singleline(&mut app.task_title);
            ui.add_space(4.0);
            ui.label("Existing worktree");
            ui.horizontal(|ui| {
                let path_width = (ui.available_width() - 90.0).max(160.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.task_worktree_path)
                        .desired_width(path_width),
                );
                if crate::ui::ellipsis_button(ui, "Browse...").clicked() {
                    browse_for_worktree = true;
                }
            });
            ui.horizontal(|ui| {
                if app.git.is_open() && ui.button("Use current workspace").clicked() {
                    app.task_worktree_path = app.repo_path.clone();
                }
                let can_create = !app.task_title.trim().is_empty()
                    && !app.task_worktree_path.trim().is_empty()
                    && !app.is_busy()
                    && app.task_registry.load_error().is_none();
                if ui
                    .add_enabled(can_create, egui::Button::new("Add task"))
                    .clicked()
                {
                    create_task = true;
                }
                if ui.button("Cancel").clicked() {
                    app.task_form_open = false;
                }
            });
        });
    }

    if browse_for_worktree {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select an existing Git worktree")
            .pick_folder()
        {
            app.task_worktree_path = path.to_string_lossy().into_owned();
        }
    }

    if create_task {
        match TaskRecord::from_worktree(
            &app.task_title,
            Path::new(&app.task_worktree_path),
        ) {
            Ok(record) => match app.task_registry.add(record) {
                Ok(()) => {
                    app.task_form_open = false;
                    app.task_title.clear();
                    app.show_success("Task linked to the existing workspace".into());
                }
                Err(error) => app.show_error(format!("Could not save task: {error}")),
            },
            Err(error) => app.show_error(error),
        }
    }

    ui.add_space(8.0);
    ui.separator();
    let entries = app.task_registry.entries().to_vec();
    if entries.is_empty() && app.task_registry.load_error().is_none() {
        ui.add_space(8.0);
        ui.label("No tasks yet. Link an existing workspace to keep it in this list.");
    } else if entries.is_empty() {
        ui.add_space(8.0);
        ui.label("Task records are unavailable because the registry could not be read.");
    }

    let mut open_workspace: Option<String> = None;
    let mut unlink_task: Option<String> = None;
    let mut start_codex: Option<TaskRecord> = None;
    let mut resume_codex: Option<TaskRecord> = None;
    let mut start_claude: Option<TaskRecord> = None;
    let mut resume_claude: Option<TaskRecord> = None;
    let mut review_task: Option<TaskRecord> = None;
    let mut configure_verification: Option<TaskRecord> = None;
    let mut run_verification: Option<TaskRecord> = None;
    let mut cancel_verification: Option<String> = None;
    for task in &entries {
        let available = worktree_directory_present(&task.worktree_path);
        let current_fingerprint = if task.verification_result.is_some() {
            app.current_task_fingerprint(ctx, task)
        } else {
            None
        };
        let verification_running = app.task_verification_is_running(&task.id);
        let any_verification_running = app.task_verification_any_running();
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&task.title).strong())
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::ui::add_enabled_ellipsis(
                        ui,
                        !verification_running,
                        "Unlink",
                    )
                    .clicked()
                    {
                        unlink_task = Some(task.id.clone());
                    }
                    if ui
                        .add_enabled(
                            available && !app.is_busy(),
                            egui::Button::new("Open workspace"),
                        )
                        .clicked()
                    {
                        open_workspace = Some(task.worktree_path.clone());
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!app.is_busy(), egui::Button::new("Review changes"))
                    .clicked()
                {
                    review_task = Some(task.clone());
                }
                if ui
                    .add_enabled(
                        available
                            && codex_availability.available
                            && codex_capabilities.can_start
                            && !app.is_busy(),
                        egui::Button::new("Start in Codex"),
                    )
                    .clicked()
                {
                    start_codex = Some(task.clone());
                }
                let has_codex_session = task.provider_ref.as_deref() == Some(codex.provider_ref())
                    && task.session_ref.is_some();
                let resume_label = if has_codex_session {
                    "Resume Codex session"
                } else {
                    "Choose Codex session"
                };
                if ui
                    .add_enabled(
                        available
                            && codex_availability.available
                            && !app.is_busy()
                            && codex_capabilities.resume != ResumeCapability::Unsupported,
                        egui::Button::new(resume_label),
                    )
                    .clicked()
                {
                    resume_codex = Some(task.clone());
                }
                if ui
                    .add_enabled(
                        available
                            && claude_availability.available
                            && claude_capabilities.can_start
                            && !app.is_busy(),
                        egui::Button::new("Start in Claude Code"),
                    )
                    .clicked()
                {
                    start_claude = Some(task.clone());
                }
                let has_claude_session =
                    task.provider_ref.as_deref() == Some(claude.provider_ref())
                        && task.session_ref.is_some();
                let can_resume_claude_exact = has_claude_session
                    && claude_capabilities.resume == ResumeCapability::SessionReference;
                let can_resume_claude =
                    can_resume_claude_exact || claude_capabilities.continue_latest;
                let claude_resume_label = if can_resume_claude_exact {
                    "Resume Claude Code session"
                } else if claude_capabilities.continue_latest {
                    "Continue recent Claude conversation"
                } else {
                    "Claude session resume unavailable"
                };
                if ui
                    .add_enabled(
                        available
                            && claude_availability.available
                            && !app.is_busy()
                            && can_resume_claude,
                        egui::Button::new(claude_resume_label),
                    )
                    .clicked()
                {
                    resume_claude = Some(task.clone());
                }
            });
            ui.label(format!("Repository: {}", task.repository_path));
            ui.label(format!("Worktree: {}", task.worktree_path));
            if task.provider_ref.as_deref() == Some(codex.provider_ref()) {
                ui.label(if task.session_ref.is_some() {
                    "Harness: Codex · session reference recorded"
                } else {
                    "Harness: Codex · session ID not tracked"
                });
            }
            if task.provider_ref.as_deref() == Some(claude.provider_ref()) {
                ui.label(if task.session_ref.is_some() {
                    "Harness: Claude Code · assigned ID saved for best-effort resume"
                } else {
                    "Harness: Claude Code · session ID not tracked"
                });
            }
            render_task_verification(
                app,
                ui,
                task,
                available,
                verification_running,
                any_verification_running,
                current_fingerprint.as_ref(),
                &mut configure_verification,
                &mut run_verification,
                &mut cancel_verification,
            );
            ui.horizontal_wrapped(|ui| {
                if let Some(branch) = &task.branch {
                    ui.label(format!("Branch: {branch}"));
                }
                if let Some(commit) = &task.base_commit {
                    ui.label(format!("Base commit: {}", &commit[..commit.len().min(12)]));
                }
                ui.label(format!("Created: {}", task.created_at));
                let state = if available {
                    "Directory present"
                } else {
                    "Unavailable"
                };
                let color = if available {
                    App::adaptive_green(ui.style().visuals.dark_mode)
                } else {
                    App::adaptive_yellow(ui.style().visuals.dark_mode)
                };
                ui.colored_label(color, state);
            });
        });
    }

    if let Some(task) = configure_verification {
        app.task_verification_editor = Some(TaskVerificationCommandDraft::for_task(&task));
    }
    render_verification_command_editor(app, ui, &entries);

    if let Some(task) = run_verification {
        app.start_task_verification(ctx, &task);
    }
    if let Some(task_id) = cancel_verification {
        app.cancel_task_verification(&task_id);
    }

    if let Some(id) = unlink_task {
        match app.task_registry.unlink(&id) {
            Ok(()) => {
                app.forget_task_fingerprint_probe(&id);
                app.forget_task_pull_request_probe(&id);
                if app
                    .task_diff_review
                    .as_ref()
                    .is_some_and(|view| view.task_id == id)
                {
                    app.task_diff_review = None;
                }
                app.show_success("Task record unlinked; workspace and branch were left unchanged".into());
            }
            Err(error) => app.show_error(format!("Could not unlink task: {error}")),
        }
    }

    if let Some(path) = open_workspace {
        app.open_repo(&path);
        if app.git.is_open() && app.repo_path == path {
            app.current_tab = Tab::Worktrees;
        }
    }

    if let Some(task) = review_task {
        app.start_task_diff_review(ctx, &task);
    }

    if let Some(task) = start_codex {
        match app.task_registry.check_provider_start(&task) {
            Ok(()) => match codex.start(&task) {
                Ok(session_ref) => match app.task_registry.record_provider_start(
                    &task.id,
                    task.provider_ref.as_deref(),
                    task.session_ref.as_deref(),
                    codex.provider_ref(),
                    session_ref.as_deref(),
                ) {
                    Ok(()) if session_ref.is_some() => app.show_success(
                        "Codex launch requested with this task's goal and its session reference was saved."
                            .into(),
                    ),
                    Ok(()) => app.show_success(
                        "Codex launch requested with this task's goal. Session ID and run state are not tracked."
                            .into(),
                    ),
                    Err(error) => app.show_error(format!(
                        "Codex was launched, but its provider/session reference could not be saved: {error}"
                    )),
                },
                Err(error) => app.show_error(error),
            },
            Err(error) => app.show_error(format!(
                "Could not confirm the Codex task before launch; the launch was not started: {error}"
            )),
        }
    }

    if let Some(task) = start_claude {
        match app.task_registry.check_provider_start(&task) {
            Ok(()) => match claude.start(&task) {
                Ok(session_ref) => match app.task_registry.record_provider_start(
                    &task.id,
                    task.provider_ref.as_deref(),
                    task.session_ref.as_deref(),
                    claude.provider_ref(),
                    session_ref.as_deref(),
                ) {
                    Ok(()) if session_ref.is_some() => app.show_success(
                        "Claude Code launch requested with this task's goal. Its assigned session ID was saved for best-effort resume; session creation and run state are not confirmed or tracked."
                            .into(),
                    ),
                    Ok(()) if claude_capabilities.continue_latest => app.show_success(
                        "Claude Code launch requested with this task's goal. No session ID was saved, so continuation opens only the latest conversation in this worktree; session creation and run state are not tracked."
                            .into(),
                    ),
                    Ok(()) => app.show_success(
                        "Claude Code launch requested with this task's goal, but no session ID was saved; this task session cannot be resumed from GitManager, and session creation/run state are not tracked."
                            .into(),
                    ),
                    Err(error) => app.show_error(format!(
                        "Claude Code was launched, but its provider/session reference could not be saved: {error}"
                    )),
                },
                Err(error) => app.show_error(error),
            },
            Err(error) => app.show_error(format!(
                "Could not confirm the Claude Code task before launch; the launch was not started: {error}"
            )),
        }
    }

    if let Some(task) = resume_codex {
        match codex.resume(&task) {
            Ok(()) if task.provider_ref.as_deref() == Some(codex.provider_ref())
                && task.session_ref.is_some() =>
            {
                app.show_success("Codex resume requested for the saved session reference.".into())
            }
            Ok(()) => app.show_success(
                "Codex worktree-filtered session picker launch requested. Choose this task's session."
                    .into(),
            ),
            Err(error) => app.show_error(error),
        }
    }

    if let Some(task) = resume_claude {
        let has_saved_session = task.provider_ref.as_deref() == Some(claude.provider_ref())
            && task.session_ref.is_some()
            && claude_capabilities.resume == ResumeCapability::SessionReference;
        match claude.resume(&task) {
            Ok(()) if has_saved_session => app.show_success(
                "Claude Code resume requested for the assigned session ID; session creation and run state are not confirmed or tracked."
                    .into(),
            ),
            Ok(()) => app.show_success(
                "Claude Code continuation requested for the most recent conversation in this worktree; it may differ from this task's assigned session, and session creation/run state are not tracked."
                    .into(),
            ),
            Err(error) => app.show_error(error),
        }
    }

    render_task_diff_review(app, ui);
}

#[allow(clippy::too_many_arguments)]
fn render_task_verification(
    app: &mut App,
    ui: &mut egui::Ui,
    task: &TaskRecord,
    available: bool,
    verification_running: bool,
    any_verification_running: bool,
    current_fingerprint: Option<&Result<String, String>>,
    configure_verification: &mut Option<TaskRecord>,
    run_verification: &mut Option<TaskRecord>,
    cancel_verification: &mut Option<String>,
) {
    let cancellation_requested = app.task_verification_cancel_requested(&task.id);
    ui.separator();
    ui.label(egui::RichText::new("Task verification").strong());
    if let Some(command) = &task.verification_command {
        ui.monospace(format!("{} {:?}", command.executable, command.args));
    } else {
        ui.label("No verification command configured.");
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Configure command").clicked() {
            *configure_verification = Some(task.clone());
        }
        if verification_running {
            let progress_label = if cancellation_requested { "Cancelling…" } else { "Running" };
            ui.label(egui::RichText::new(progress_label).color(App::adaptive_yellow(ui.style().visuals.dark_mode)));
            if !cancellation_requested && ui.button("Cancel verification").clicked() {
                *cancel_verification = Some(task.id.clone());
            }
        } else if ui
            .add_enabled(
                available
                    && task.verification_command.is_some()
                    && !any_verification_running
                    && !app.is_busy(),
                egui::Button::new("Run verification"),
            )
            .clicked()
        {
            *run_verification = Some(task.clone());
        }
    });

    if let Some(result) = &task.verification_result {
        let dark = ui.style().visuals.dark_mode;
        let source_is_current = match (&result.source_fingerprint, current_fingerprint) {
            (Some(saved), Some(Ok(current))) => Some(saved == current),
            _ => None,
        };
        let command_matches = task.verification_command.as_ref() == Some(&result.command);
        let state_prefix = if source_is_current == Some(false) {
            "Stale · "
        } else if source_is_current.is_none() && result.source_fingerprint.is_some() {
            if current_fingerprint.is_none() {
                "Checking freshness · "
            } else {
                "Freshness unknown · "
            }
        } else if !command_matches {
            "Previous command · "
        } else if result.source_fingerprint.is_none() {
            "Freshness unknown · "
        } else {
            ""
        };
        let state_label = format!("{state_prefix}{}", result.state.label());
        let state_color = if source_is_current != Some(true) || !command_matches {
            App::adaptive_yellow(dark)
        } else {
            match result.state {
                VerificationState::Passed => App::adaptive_green(dark),
                VerificationState::Failed | VerificationState::Error => App::adaptive_red(dark),
                VerificationState::Running
                | VerificationState::TimedOut
                | VerificationState::Cancelled => App::adaptive_yellow(dark),
            }
        };
        ui.colored_label(state_color, format!("Last result: {state_label}"));
        if result.state == VerificationState::Running && !verification_running {
            ui.colored_label(
                App::adaptive_yellow(dark),
                "Run was recorded as running; live process state is unavailable.",
            );
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(exit_code) = result.exit_code {
                ui.label(format!("Exit code: {exit_code}"));
            }
            ui.label(format!("Started: {}", result.started_at));
            if let Some(finished_at) = &result.finished_at {
                ui.label(format!("Finished: {finished_at}"));
            }
        });
        if !command_matches {
            ui.colored_label(App::adaptive_yellow(dark), "The saved result used a different command configuration.");
        }
        match source_is_current {
            Some(true) => {
                ui.colored_label(App::adaptive_green(dark), "Source state matches this result.");
            }
            Some(false) => {
                ui.colored_label(App::adaptive_yellow(dark), "Stale: task source changed after this run.");
            }
            None if result.source_fingerprint.is_some() && current_fingerprint.is_some() => {
                let error = current_fingerprint.and_then(|fingerprint| fingerprint.as_ref().err());
                ui.colored_label(
                    App::adaptive_yellow(dark),
                    format!("Freshness unavailable: {}", error.cloned().unwrap_or_else(|| "could not compare source fingerprints".into())),
                );
            }
            None if result.source_fingerprint.is_some() => {
                ui.label("Checking whether the task source has changed…");
            }
            _ => {
                ui.colored_label(App::adaptive_yellow(dark), "Source freshness could not be established.");
            }
        }
        egui::CollapsingHeader::new("Verification output")
            .id_salt(("task_verification_output", &task.id))
            .show(ui, |ui| {
                ui.label(format!("Executable: {}", result.command.executable));
                ui.label(format!("Arguments: {:?}", result.command.args));
                ui.label(format!(
                    "Timeout: {} minute(s)",
                    result.command.timeout_seconds.saturating_add(59) / 60
                ));
                if let Some(fingerprint) = &result.source_fingerprint {
                    ui.monospace(format!("Source fingerprint: {fingerprint}"));
                }
                if result.output_truncated {
                    ui.colored_label(
                        App::adaptive_yellow(ui.style().visuals.dark_mode),
                        "Output is incomplete or truncated; up to 16 KiB per stream is retained.",
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(180.0)
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
}

fn render_verification_command_editor(app: &mut App, ui: &mut egui::Ui, entries: &[TaskRecord]) {
    let Some(editor_task_id) = app
        .task_verification_editor
        .as_ref()
        .map(|editor| editor.task_id.clone())
    else {
        return;
    };
    if !entries.iter().any(|task| task.id == editor_task_id) {
        app.task_verification_editor = None;
        return;
    }
    if app.task_verification_is_running(&editor_task_id) {
        ui.label("Command settings cannot be changed while this task is running verification.");
        return;
    }
    let Some(editor) = app.task_verification_editor.as_mut() else {
        return;
    };

    let mut save = false;
    let mut remove = false;
    let mut cancel = false;
    ui.add_space(8.0);
    ui.group(|ui| {
        ui.label(egui::RichText::new(format!("Verification command · {}", editor.title)).strong());
        ui.label("The executable and arguments are launched directly without shell parsing.");
        ui.label("Executable");
        ui.text_edit_singleline(&mut editor.executable);
        ui.label("Arguments (JSON array of strings; use \"\" for an empty argument)");
        let arguments_width = ui.available_width();
        ui.add(
            egui::TextEdit::multiline(&mut editor.arguments)
                .desired_rows(3)
                .desired_width(arguments_width),
        );
        ui.horizontal(|ui| {
            ui.label("Timeout (minutes)");
            ui.add(egui::TextEdit::singleline(&mut editor.timeout_minutes).desired_width(80.0));
        });
        ui.horizontal(|ui| {
            if ui.button("Save command").clicked() {
                save = true;
            }
            if ui.button("Remove command").clicked() {
                remove = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });

    let task_id = editor.task_id.clone();
    let executable = editor.executable.clone();
    let arguments = editor.arguments.clone();
    let timeout_text = editor.timeout_minutes.clone();
    if save {
        let command = match timeout_text.trim().parse::<u64>() {
            Ok(minutes) if (1..=24 * 60).contains(&minutes) => {
                serde_json::from_str::<Vec<String>>(&arguments)
                    .map(|args| VerificationCommand {
                        executable: executable.trim().to_string(),
                        args,
                        timeout_seconds: minutes * 60,
                    })
                    .map_err(|error| {
                        format!("Arguments must be a JSON array of strings: {error}")
                    })
            }
            _ => Err("Set the timeout to a whole number from 1 to 1440 minutes".into()),
        };
        match command {
            Ok(command) => match command.validate() {
                Ok(()) => match app
                    .task_registry
                    .save_verification_command(&task_id, Some(command))
                {
                    Ok(()) => {
                        app.task_verification_editor = None;
                        app.show_success("Task verification command saved".into());
                    }
                    Err(error) => app.show_error(format!("Could not save command: {error}")),
                },
                Err(error) => app.show_error(error),
            },
            Err(error) => app.show_error(error),
        }
    } else if remove {
        match app
            .task_registry
            .save_verification_command(&task_id, None)
        {
            Ok(()) => {
                app.task_verification_editor = None;
                app.show_success("Task verification command removed".into());
            }
            Err(error) => app.show_error(format!("Could not remove command: {error}")),
        }
    } else if cancel {
        app.task_verification_editor = None;
    }
}

pub fn render_task_diff_review(app: &mut App, ui: &mut egui::Ui) {
    let Some(view) = app.task_diff_review.as_ref() else {
        return;
    };

    ui.separator();
    let mut close_review = false;
    let mut mark_reviewed: Option<(String, String)> = None;
    let mut mark_pending: Option<String> = None;
    ui.horizontal(|ui| {
        ui.heading(format!("Review changes: {}", view.title));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close_review = true;
            }
        });
    });

    match &view.state {
        TaskDiffReviewState::Loading => {
            ui.label("Reading the task worktree diff…");
        }
        TaskDiffReviewState::Unavailable(reason) => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!("Complete review diff unavailable: {reason}"),
            );
        }
        TaskDiffReviewState::Error(reason) => {
            ui.colored_label(
                App::adaptive_red(ui.style().visuals.dark_mode),
                format!("Could not read the complete task diff: {reason}"),
            );
        }
        TaskDiffReviewState::Ready(review) => {
            ui.label(
                "Diff from the saved base through current HEAD, staged and unstaged edits, and untracked files.",
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Base:");
                ui.monospace(&review.base_commit);
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Current HEAD:");
                ui.monospace(&review.head_commit);
            });

            ui.label(format!("Changed files ({})", review.files.len()));
            if review.files.is_empty() {
                ui.label("No changes since the saved base commit.");
            } else {
                let task = app
                    .task_registry
                    .entries()
                    .iter()
                    .find(|task| task.id == view.task_id);
                if let Some(task) = task {
                    if task.reviewed_source_fingerprint.as_deref()
                        == Some(review.source_fingerprint.as_str())
                    {
                        ui.label(format!(
                            "Reviewed for this source snapshot on {}. This does not record approval, merge, or verification.",
                            task.reviewed_at.as_deref().unwrap_or("an unknown date")
                        ));
                        if ui.button("Set review to pending").clicked() {
                            mark_pending = Some(task.id.clone());
                        }
                    } else if ui.button("Mark this snapshot as reviewed").clicked() {
                        mark_reviewed = Some((
                            task.id.clone(),
                            review.source_fingerprint.clone(),
                        ));
                    }
                } else {
                    ui.label("This task record is no longer linked; its review disposition cannot be changed.");
                }
                egui::ScrollArea::both()
                    .id_salt(("task_diff_files", &view.task_id))
                    .max_height(150.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for file in &review.files {
                            ui.monospace(format!("{}  {}", file.status, file.path));
                        }
                    });
            }

            if !review.patch.is_empty() {
                ui.label("Complete patch");
                egui::ScrollArea::both()
                    .id_salt(("task_diff_patch", &view.task_id))
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&review.patch).monospace())
                                .extend(),
                        );
                    });
            }
        }
    }

    if close_review {
        app.task_diff_review = None;
    }
    if let Some((task_id, source_fingerprint)) = mark_reviewed {
        match app
            .task_registry
            .mark_reviewed(&task_id, &source_fingerprint)
        {
            Ok(()) => app.show_success(
                "Recorded as reviewed for the displayed task snapshot. Verification remains separate."
                    .into(),
            ),
            Err(error) => app.show_error(format!("Could not record task review: {error}")),
        }
    }
    if let Some(task_id) = mark_pending {
        match app.task_registry.mark_review_pending(&task_id) {
            Ok(()) => app.show_success("Task review set to pending".into()),
            Err(error) => app.show_error(format!("Could not set task review to pending: {error}")),
        }
    }
}

fn worktree_directory_present(path: &str) -> bool {
    Path::new(path).is_dir()
}
