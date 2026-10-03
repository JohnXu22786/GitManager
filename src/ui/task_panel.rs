use crate::app::{App, Tab};
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
            ui.label(format!("Repository: {}", task.repository_path));
            ui.label(format!("Worktree: {}", task.worktree_path));
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
}

fn worktree_directory_present(path: &str) -> bool {
    Path::new(path).is_dir()
}
