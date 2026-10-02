use crate::app::{App, FormSubmission};
use crate::git_ops::GitOperation;
use eframe::egui;

fn should_show_stage_action(status: char) -> bool {
    status != '!'
}

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Heading row: heading text on the left, buttons anchored to right edge
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Changes").heading()).truncate()).on_hover_text("Changes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Stage All").clicked() {
                app.start_operation(ctx, "Staging all", GitOperation::StageAll);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Unstage All").clicked() {
                app.start_operation(ctx, "Unstaging all", GitOperation::UnstageAll);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Discard All").clicked() {
                app.request_confirmation(
                    ctx,
                    "Confirm discard all changes",
                    "Restore tracked working-tree files from HEAD? This discards unstaged changes. Staged changes remain in the index, and untracked files that conflict with tracked paths may be overwritten.",
                    "Discard all",
                    "Discard all unstaged changes",
                    GitOperation::RestoreAll,
                );
            }
        });
    });

    ui.separator();

    let staged: Vec<_> = app.status_entries.iter().filter(|e| e.staged).cloned().collect();
    let unstaged: Vec<_> = app.status_entries.iter().filter(|e| !e.staged).cloned().collect();
    let dark = ctx.style().visuals.dark_mode;

    egui::ScrollArea::vertical()
        .id_salt("status_files")
        .show(ui, |ui| {
            if !staged.is_empty() {
                ui.label(egui::RichText::new("Staged").color(App::adaptive_green(dark)).strong());
                for entry in &staged {
                    let path = entry.path.clone();
                    let path_display = path.to_string_lossy().into_owned();
                    ui.horizontal(|ui| {
                        let busy = app.is_busy();
                        let color = crate::app::App::status_color_by_type(entry.status, dark);
                        ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(!busy, egui::Button::new("Unstage")).clicked() {
                                app.start_operation(ctx, &format!("Unstage {}", path_display), GitOperation::UnstageFile(path.clone()));
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Diff")).clicked() {
                                app.start_operation(ctx, &format!("Diff {}", path_display), GitOperation::GetDiff { path: path.clone(), staged: true });
                            }
                            let path_clone = path_display.clone();
                            ui.add(egui::Label::new(&path_display).truncate()).on_hover_text(path_clone);
                        });
                    });
                }
                ui.separator();
            }

            if !unstaged.is_empty() {
                ui.label(egui::RichText::new("Unstaged").color(App::adaptive_yellow(dark)).strong());
                for entry in &unstaged {
                    let path = entry.path.clone();
                    let path_display = path.to_string_lossy().into_owned();
                    ui.horizontal(|ui| {
                        let busy = app.is_busy();
                        let color = crate::app::App::status_color_by_type(entry.status, dark);
                        ui.label(egui::RichText::new(format!("[{}]", entry.status)).color(color).monospace());

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if should_show_stage_action(entry.status) {
                                if ui.add_enabled(!busy, egui::Button::new("Stage")).clicked() {
                                    app.start_operation(ctx, &format!("Stage {}", path_display), GitOperation::StageFile(path.clone()));
                                }
                            }
                            if entry.status != '?' && entry.status != '!' {
                                if ui.add_enabled(!busy, egui::Button::new("Discard")).clicked() {
                                    app.request_confirmation(
                                        ctx,
                                        "Confirm discard file changes",
                                        format!(
                                            "Restore {:?} from the index? This discards its unstaged working-tree changes; staged changes remain.",
                                            path
                                        ),
                                        "Discard file changes",
                                        format!("Discard unstaged changes to {:?}", path),
                                        GitOperation::RestoreFile(path.clone()),
                                    );
                                }
                            }
                            if ui.add_enabled(!busy, egui::Button::new("Diff")).clicked() {
                                app.start_operation(ctx, &format!("Diff {}", path_display), GitOperation::GetDiff { path: path.clone(), staged: false });
                            }
                            let path_clone = path_display.clone();
                            ui.add(egui::Label::new(&path_display).truncate()).on_hover_text(path_clone);
                        });
                    });
                }
            }

            if staged.is_empty() && unstaged.is_empty() {
                ui.label("No changes - working tree clean");
            }
        });

    ui.separator();
    ui.add_space(10.0);

    ui.heading("Commit");
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.commit_amend, "Amend");
    });

    let commit_msg = &mut app.commit_msg;
    egui::ScrollArea::vertical()
        .id_salt("commit_scroll")
        .show(ui, |ui| {
            ui.add_sized(
                egui::vec2(ui.available_width(), 80.0),
                egui::TextEdit::multiline(commit_msg).hint_text("Commit message"),
            );
        });

    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Commit").clicked() {
        if app.commit_msg.trim().is_empty() {
            app.show_error("Commit message cannot be empty".into());
        } else {
            let msg = app.commit_msg.trim().to_string();
            let amend = app.commit_amend;
            let form_submission = FormSubmission::Commit {
                message: app.commit_msg.clone(),
                amend: app.commit_amend,
            };
            app.start_operation_with_form_submission(
                ctx,
                "Committing",
                GitOperation::Commit { message: msg, amend },
                form_submission,
            );
        }
    }
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Uncommit").clicked() {
        app.start_operation(ctx, "Uncommitting", GitOperation::Uncommit);
    }

    if app.show_diff && !app.diff_content.is_empty() {
        ui.separator();
        ui.heading(format!("Diff: {}", app.diff_path));
        let diff_content = app.diff_content.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in &diff_content {
                let color = match line.origin {
                    '+' => if dark {
                        egui::Color32::from_rgb(60, 200, 60)
                    } else {
                        egui::Color32::from_rgb(0, 130, 0)
                    },
                    '-' => if dark {
                        egui::Color32::from_rgb(220, 60, 60)
                    } else {
                        egui::Color32::from_rgb(170, 30, 30)
                    },
                    _ => egui::Color32::GRAY,
                };
                let prefix = match line.origin {
                    '+' => "+",
                    '-' => "-",
                    ' ' => " ",
                    _ => " ",
                };
                ui.label(
                    egui::RichText::new(format!("{}{}", prefix, line.content.trim_end()))
                        .color(color)
                        .monospace(),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::git_ops::StatusEntry;

    fn status_panel_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(app, ui, ctx);
                });
            },
        )
    }

    fn click_status_button(app: &mut App, label: &str) {
        let ctx = egui::Context::default();
        let output = status_panel_frame(app, &ctx, Vec::new());
        let position = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(clipped.shape.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("button label '{}' should be rendered", label));

        for pressed in [true, false] {
            status_panel_frame(
                app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
            );
        }
    }

    /// Helper: run the status panel `show` function in a test context.
    fn run_status_panel(app: &mut App) {
        egui::__run_test_ctx(|ctx| {
                egui::Area::new("test_status_panel".into())
                .show(ctx, |ui| {
                    show(app, ui, ctx);
                });
        });
    }

    #[test]
    fn test_show_empty_entries() {
        let mut app = App::new();
        // No entries added - should show "No changes" without panic
        run_status_panel(&mut app);
        // No assertion needed; the test passes if no panic occurs
    }

    #[test]
    fn test_show_with_staged_entries() {
        let mut app = App::new();
        app.status_entries.push(StatusEntry {
            path: "src/main.rs".into(),
            status: 'M',
            staged: true,
        });
        app.status_entries.push(StatusEntry {
            path: "Cargo.toml".into(),
            status: 'A',
            staged: true,
        });
        run_status_panel(&mut app);
    }

    #[test]
    fn test_show_with_unstaged_entries() {
        let mut app = App::new();
        app.status_entries.push(StatusEntry {
            path: "src/lib.rs".into(),
            status: 'M',
            staged: false,
        });
        app.status_entries.push(StatusEntry {
            path: "README.md".into(),
            status: '?',
            staged: false,
        });
        app.status_entries.push(StatusEntry {
            path: "old_file.txt".into(),
            status: 'D',
            staged: false,
        });
        run_status_panel(&mut app);
    }

    #[test]
    fn test_show_with_both_staged_and_unstaged() {
        let mut app = App::new();
        app.status_entries.push(StatusEntry {
            path: "src/main.rs".into(),
            status: 'M',
            staged: true,
        });
        app.status_entries.push(StatusEntry {
            path: "src/main.rs".into(),
            status: 'M',
            staged: false,
        });
        app.status_entries.push(StatusEntry {
            path: "new_file.py".into(),
            status: '?',
            staged: false,
        });
        run_status_panel(&mut app);
    }

    #[test]
    fn test_show_with_large_number_of_entries() {
        let mut app = App::new();
        // Add 100 entries - this reproduces the scrolling issue
        // (many entries cause overflow without ScrollArea)
        for i in 0..100 {
            app.status_entries.push(StatusEntry {
                path: format!("src/file_{:03}.rs", i).into(),
                status: 'M',
                staged: i % 2 == 0,
            });
        }
        // Should not panic even with many entries
        run_status_panel(&mut app);
    }

    #[test]
    fn test_show_with_conflict_status() {
        let mut app = App::new();
        app.status_entries.push(StatusEntry {
            path: "conflict.txt".into(),
            status: 'U',
            staged: false,
        });
        run_status_panel(&mut app);
    }

    #[test]
    fn test_show_with_all_status_types() {
        let mut app = App::new();
        // Test all possible status characters
        for (i, status) in ['M', 'A', 'D', '?', '!', 'U', 'R'].iter().enumerate() {
            app.status_entries.push(StatusEntry {
                path: format!("file_{}.txt", i).into(),
                status: *status,
                staged: i % 2 == 0,
            });
        }
        run_status_panel(&mut app);
    }

    #[test]
    fn test_stage_action_is_available_for_deleted_entries() {
        assert!(should_show_stage_action('?'));
        assert!(should_show_stage_action('M'));
        assert!(should_show_stage_action('D'));
        assert!(!should_show_stage_action('!'));
    }

    #[test]
    fn discard_all_requires_confirmation_before_dispatch() {
        let repo_dir = tempfile::tempdir().expect("repository directory");
        drop(git2::Repository::init(repo_dir.path()).expect("initialize repository"));
        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repository");

        click_status_button(&mut app, "Discard All");

        assert_eq!(
            app.current_operation(),
            "Awaiting confirmation: Discard all unstaged changes"
        );
    }

    #[test]
    #[cfg(unix)]
    fn discard_file_requires_confirmation_and_escapes_filename_before_dispatch() {
        use std::os::unix::ffi::OsStringExt;

        let repo_dir = tempfile::tempdir().expect("repository directory");
        drop(git2::Repository::init(repo_dir.path()).expect("initialize repository"));
        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repository");
        let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(
            b"tracked\n\xfffile.txt".to_vec(),
        ));
        app.status_entries.push(StatusEntry {
            path: path.clone(),
            status: 'M',
            staged: false,
        });

        click_status_button(&mut app, "Discard");

        assert_eq!(
            app.current_operation(),
            format!("Awaiting confirmation: Discard unstaged changes to {:?}", path)
        );
        assert!(
            !app.current_operation().contains('\u{fffd}'),
            "the confirmation target must preserve invalid filename bytes"
        );
    }
}
