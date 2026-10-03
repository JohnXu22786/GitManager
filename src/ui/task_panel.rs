use crate::app::{App, Tab};
use crate::harness::{ClaudeHarness, CodexHarness, ResumeCapability, TaskHarness};
use crate::tasks::TaskRecord;
use eframe::egui;
use std::path::Path;

pub fn show(app: &mut App, ui: &mut egui::Ui, _ctx: &egui::Context) {
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
    for task in &entries {
        let available = worktree_directory_present(&task.worktree_path);
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&task.title).strong())
                        .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::ui::ellipsis_button(ui, "Unlink").clicked() {
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

    if let Some(id) = unlink_task {
        match app.task_registry.unlink(&id) {
            Ok(()) => app.show_success("Task record unlinked; workspace and branch were left unchanged".into()),
            Err(error) => app.show_error(format!("Could not unlink task: {error}")),
        }
    }

    if let Some(path) = open_workspace {
        app.open_repo(&path);
        if app.git.is_open() && app.repo_path == path {
            app.current_tab = Tab::Worktrees;
        }
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
}

fn worktree_directory_present(path: &str) -> bool {
    Path::new(path).is_dir()
}
