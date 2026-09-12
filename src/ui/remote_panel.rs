use crate::app::App;
use crate::git_ops::GitOperation;
use eframe::egui;

fn initialize_push_branch(push_branch: &mut String, current_branch: &str, user_edited: bool) {
    if !user_edited {
        *push_branch = current_branch.to_owned();
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Remotes").heading()).truncate()).on_hover_text("Remotes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::add_enabled_ellipsis(ui, !app.is_busy(), "🔄 Refresh").clicked() {
                app.refresh_all();
            }
        });
    });

    ui.separator();

    if !app.remote_list.is_empty() {
        ui.label(egui::RichText::new("Configured Remotes").strong());
        for remote in &app.remote_list {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    let name_clone = remote.name.clone();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&remote.name).color(egui::Color32::from_rgb(100, 200, 255)).strong(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(name_clone);
                    let url_clone = remote.url.clone();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&remote.url)
                                .color(egui::Color32::GRAY)
                                .text_style(egui::TextStyle::Small),
                        )
                        .truncate(),
                    )
                    .on_hover_text(url_clone);

                });
            });
        }
    } else {
        ui.label("No remotes configured");
    }

    ui.add_space(10.0);
    ui.separator();

    let current_branch = app.git.current_branch().unwrap_or_default();
    let default_remote = app.remote_list.first().map(|r| r.name.clone()).unwrap_or_default();

    initialize_push_branch(
        &mut app.push_branch,
        &current_branch,
        app.push_branch_user_edited,
    );
    if app.remote_name.is_empty() {
        app.remote_name = default_remote.clone();
    }

    let busy = app.is_busy();

    ui.heading("Push");
    ui.horizontal(|ui| {
        ui.label("Remote:");
        ui.text_edit_singleline(&mut app.remote_name);
    });
    ui.horizontal(|ui| {
        ui.label("Branch:");
        if ui.text_edit_singleline(&mut app.push_branch).changed() {
            app.push_branch_user_edited = true;
        }
    });
    ui.checkbox(&mut app.push_force, "Force Push");
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Push").clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            app.start_operation(ctx, &format!("Push to {}/{}", remote, branch), GitOperation::Push {
                remote,
                branch,
                force: app.push_force,
            });
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Pull");
    ui.checkbox(&mut app.pull_rebase, "Rebase instead of merge");
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Pull").clicked() {
        let remote = app.remote_name.trim().to_string();
        let branch = app.push_branch.trim().to_string();
        if remote.is_empty() || branch.is_empty() {
            app.show_error("Remote and branch required".into());
        } else {
            app.start_operation(ctx, &format!("Pull from {}/{}", remote, branch), GitOperation::Pull {
                remote,
                branch,
                rebase: app.pull_rebase,
            });
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.heading("Fetch");
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Fetch").clicked() {
        let remote = app.remote_name.trim().to_string();
        if remote.is_empty() {
            app.show_error("Remote required".into());
        } else {
            app.start_operation(ctx, &format!("Fetch from {}", remote), GitOperation::Fetch(remote));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::show;
    use crate::app::App;
    use eframe::egui;
    use git2::Repository;
    use std::path::Path;
    use super::initialize_push_branch;

    fn create_repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).expect("init repo");
        let signature = repo.signature().expect("signature");
        let tree_oid = {
            let mut index = repo.index().expect("index");
            index.write_tree().expect("write tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("initial commit");
        drop(tree);
        repo
    }

    fn show_panel(app: &mut App, ctx: &egui::Context) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(app, ui, ctx);
                });
            },
        );
    }

    #[test]
    fn follows_current_branch_until_push_branch_is_edited() {
        let mut push_branch = String::new();

        initialize_push_branch(&mut push_branch, "main", false);
        assert_eq!(push_branch, "main");

        initialize_push_branch(&mut push_branch, "feature", false);
        assert_eq!(push_branch, "feature");

        push_branch = String::from("release/v1");
        initialize_push_branch(&mut push_branch, "hotfix", true);

        assert_eq!(push_branch, "release/v1");
    }

    #[test]
    fn panel_updates_untouched_default_after_branch_change() {
        let repo_dir = tempfile::tempdir_in(".").expect("temp dir");
        let repo = create_repo_with_commit(repo_dir.path());
        let initial_branch = repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("initial branch")
            .to_owned();
        let initial_commit = repo.head().expect("HEAD").peel_to_commit().expect("commit");
        repo.branch("feature", &initial_commit, false)
            .expect("create feature branch");
        drop(initial_commit);
        drop(repo);

        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repo");
        let ctx = egui::Context::default();

        show_panel(&mut app, &ctx);
        assert_eq!(app.push_branch, initial_branch);

        app.git.checkout_branch("feature").expect("checkout feature");
        show_panel(&mut app, &ctx);

        assert_eq!(app.push_branch, "feature");
    }
}
