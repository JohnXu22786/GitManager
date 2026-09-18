use crate::app::App;
use crate::git_ops::{GitOperation, RemoteInfo};
use eframe::egui;

fn initialize_push_branch(
    push_branch: &mut String,
    current_branch: &str,
    head_is_detached: bool,
    user_edited: bool,
) {
    if !user_edited {
        if head_is_detached {
            push_branch.clear();
        } else {
            *push_branch = current_branch.to_owned();
        }
    }
}

fn initialize_remote_name(
    remote_name: &mut String,
    default_remote: &str,
    remote_list: &[RemoteInfo],
    user_edited: bool,
) {
    if !user_edited && !remote_list.iter().any(|remote| remote.name == remote_name.as_str()) {
        *remote_name = default_remote.to_owned();
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
    let head_is_detached = app.git.head_is_detached().unwrap_or(false);
    let default_remote = app.remote_list.first().map(|r| r.name.clone()).unwrap_or_default();

    initialize_push_branch(
        &mut app.push_branch,
        &current_branch,
        head_is_detached,
        app.push_branch_user_edited,
    );
    initialize_remote_name(
        &mut app.remote_name,
        &default_remote,
        &app.remote_list,
        app.remote_name_user_edited,
    );

    let busy = app.is_busy();

    ui.heading("Push");
    ui.horizontal(|ui| {
        ui.label("Remote:");
        if ui.text_edit_singleline(&mut app.remote_name).changed() {
            app.remote_name_user_edited = true;
        }
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
    use crate::git_ops::RemoteInfo;
    use crate::recent::RecentRepos;
    use eframe::egui;
    use git2::Repository;
    use std::path::Path;
    use super::initialize_push_branch;

    fn create_repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).expect("init repo");
        let signature = git2::Signature::now(
            "Git Manager Test",
            "git-manager-test@example.com",
        )
        .expect("signature");
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
        show_panel_with_events(app, ctx, Vec::new());
    }

    fn show_panel_with_events(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) {
        let _ = ctx.run(
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
                    egui::ScrollArea::horizontal()
                        .id_salt("tab_bar_scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for label in [
                                    "📊 Status",
                                    "🔀 Branches",
                                    "📂 Worktrees",
                                    "📋 Log",
                                    "📦 Stash",
                                    "🌐 Remotes",
                                ] {
                                    ui.add(egui::Button::new(label));
                                }
                            });
                        });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("main_content_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            show(app, ui, ctx);
                            ui.allocate_space(ui.available_size());
                        });
                });
            },
        );
    }

    #[test]
    fn follows_current_branch_until_push_branch_is_edited() {
        let mut push_branch = String::new();

        initialize_push_branch(&mut push_branch, "main", false, false);
        assert_eq!(push_branch, "main");

        initialize_push_branch(&mut push_branch, "feature", false, false);
        assert_eq!(push_branch, "feature");

        push_branch = String::from("release/v1");
        initialize_push_branch(&mut push_branch, "hotfix", false, true);

        assert_eq!(push_branch, "release/v1");
    }

    #[test]
    fn preserves_attached_branch_named_like_detached_display() {
        let mut push_branch = String::new();

        initialize_push_branch(&mut push_branch, "detached at abc1234", false, false);

        assert_eq!(push_branch, "detached at abc1234");
    }

    #[test]
    fn panel_clears_default_after_head_is_detached() {
        let repo_dir = tempfile::tempdir_in(".").expect("temp dir");
        let repo = create_repo_with_commit(repo_dir.path());
        let initial_branch = repo
            .head()
            .expect("HEAD")
            .shorthand()
            .expect("initial branch")
            .to_owned();
        let initial_oid = repo.head().expect("HEAD").target().expect("initial commit");
        drop(repo);

        let mut app = App::new();
        app.git.open(repo_dir.path()).expect("open repo");
        let ctx = egui::Context::default();

        show_panel(&mut app, &ctx);
        assert_eq!(app.push_branch, initial_branch);

        let repo = Repository::open(repo_dir.path()).expect("reopen repo");
        repo.set_head_detached(initial_oid).expect("detach HEAD");
        drop(repo);

        show_panel(&mut app, &ctx);

        assert!(app.push_branch.is_empty());

        app.push_branch = "release/v1".to_owned();
        app.push_branch_user_edited = true;
        show_panel(&mut app, &ctx);

        assert_eq!(app.push_branch, "release/v1");
    }

    #[test]
    fn preserves_partially_typed_remote_name_when_user_edited() {
        let mut app = App::new();
        app.remote_list = vec![
            RemoteInfo {
                name: "origin".to_string(),
                url: "https://example.com/origin.git".to_string(),
            },
            RemoteInfo {
                name: "upstream".to_string(),
                url: "https://example.com/upstream.git".to_string(),
            },
        ];
        app.remote_name = String::from("origin");
        let recent_repos_dir = tempfile::tempdir_in(".").expect("recent repos temp dir");
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        let ctx = egui::Context::default();

        for _ in 0..12 {
            show_panel_with_events(
                &mut app,
                &ctx,
                vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            show_panel_with_events(
                &mut app,
                &ctx,
                vec![egui::Event::Text("u".to_string())],
            );
            if app.remote_name_user_edited {
                break;
            }
        }

        assert!(app.remote_name_user_edited);
        assert_eq!(app.remote_name, "originu");

        show_panel(&mut app, &ctx);

        assert_eq!(app.remote_name, "originu");
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

    fn create_repo_with_remote(dir: &Path, name: &str, url: &str) -> Repository {
        let repo = create_repo_with_commit(dir);
        repo.remote(name, url).expect("create remote");
        repo
    }

    #[test]
    fn panel_resets_remote_selection_after_repository_switch() {
        let first_repo_dir = tempfile::tempdir_in(".").expect("first temp dir");
        let first_repo = create_repo_with_remote(
            first_repo_dir.path(),
            "upstream",
            "https://example.com/upstream.git",
        );
        drop(first_repo);

        let second_repo_dir = tempfile::tempdir_in(".").expect("second temp dir");
        let second_repo = create_repo_with_remote(
            second_repo_dir.path(),
            "origin",
            "https://example.com/origin.git",
        );
        drop(second_repo);

        let mut app = App::new();
        let recent_repos_dir = tempfile::tempdir_in(".").expect("recent repos temp dir");
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        let ctx = egui::Context::default();

        app.open_repo(first_repo_dir.path().to_str().expect("first repo path"));
        show_panel(&mut app, &ctx);
        assert_eq!(app.remote_name, "upstream");
        app.remote_name_user_edited = true;

        app.open_repo(second_repo_dir.path().to_str().expect("second repo path"));
        assert!(!app.remote_name_user_edited);
        show_panel(&mut app, &ctx);

        assert_eq!(app.remote_name, "origin");
    }

    #[test]
    fn panel_preserves_edited_remote_name_after_refresh() {
        let repo_dir = tempfile::tempdir_in(".").expect("temp dir");
        let repo = create_repo_with_remote(
            repo_dir.path(),
            "origin",
            "https://example.com/origin.git",
        );
        drop(repo);

        let mut app = App::new();
        let recent_repos_dir = tempfile::tempdir_in(".").expect("recent repos temp dir");
        app.recent_repos = RecentRepos::load_from(recent_repos_dir.path().join("recent.json"));
        app.open_repo(repo_dir.path().to_str().expect("repo path"));
        let ctx = egui::Context::default();
        show_panel(&mut app, &ctx);

        app.remote_name = String::from("ori");
        app.remote_name_user_edited = true;
        app.refresh_all();
        show_panel(&mut app, &ctx);

        assert_eq!(app.remote_name, "ori");
    }
}
