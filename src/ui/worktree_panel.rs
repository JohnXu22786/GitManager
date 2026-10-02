use crate::app::{App, FormSubmission};
use crate::git_ops::GitOperation;
use crate::git_ops::WorktreeInfo;
use crate::recent::RecentEntry;
use crate::ui::{column_cell, column_header, column_header_static};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use eframe::egui;
use std::path::Path;

const WORKTREE_ACTIONS_WIDTH: f32 = 150.0;
const WORKTREE_STATUS_MIN_WIDTH: f32 = 88.0;
const WORKTREE_STATUS_WIDTH: f32 = 170.0;
const MAX_WORKTREE_NAME_BYTES: usize = 240;

fn worktree_status_column_width(available_width: f32) -> f32 {
    let width = available_width - WORKTREE_ACTIONS_WIDTH - 220.0;
    if width < WORKTREE_STATUS_MIN_WIDTH {
        0.0
    } else {
        width.min(WORKTREE_STATUS_WIDTH)
    }
}

fn same_recent_path(entry_path: &str, worktree_path: &Path) -> bool {
    let entry_path = Path::new(entry_path);
    if entry_path == worktree_path {
        return true;
    }

    let entry_path = std::fs::canonicalize(entry_path).unwrap_or_else(|_| entry_path.to_path_buf());
    let worktree_path =
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    #[cfg(windows)]
    {
        let entry_path = entry_path.to_string_lossy();
        let worktree_path = worktree_path.to_string_lossy();
        entry_path
            .as_ref()
            .eq_ignore_ascii_case(worktree_path.as_ref())
    }
    #[cfg(not(windows))]
    {
        entry_path == worktree_path
    }
}

fn recent_open_activity(
    entries: &[RecentEntry],
    path: &Path,
    now: DateTime<Local>,
) -> (String, String) {
    let Some(entry) = entries
        .iter()
        .find(|entry| same_recent_path(&entry.path, path))
    else {
        return (
            "No app history record".into(),
            "No matching Git Manager open is in recent history; use from terminals or editors is not tracked."
                .into(),
        );
    };
    let Ok(local_time) = NaiveDateTime::parse_from_str(&entry.last_opened, "%Y-%m-%d %H:%M:%S")
    else {
        return (
            "Open time unknown".into(),
            format!(
                "Git Manager history contains an invalid open time: {}",
                entry.last_opened
            ),
        );
    };
    let Some(opened_at) = Local.from_local_datetime(&local_time).earliest() else {
        return (
            "Open time unknown".into(),
            format!(
                "Git Manager history contains an ambiguous open time: {}",
                entry.last_opened
            ),
        );
    };

    let seconds = now.signed_duration_since(opened_at).num_seconds();
    let label = if seconds < 0 {
        "Opened here just now".to_string()
    } else if seconds < 60 {
        "Opened here <1m ago".to_string()
    } else if seconds < 3_600 {
        format!("Opened here {}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("Opened here {}h ago", seconds / 3_600)
    } else if seconds < 2_592_000 {
        format!("Opened here {}d ago", seconds / 86_400)
    } else if seconds < 31_536_000 {
        format!("Opened here {}mo ago", seconds / 2_592_000)
    } else {
        format!("Opened here {}y ago", seconds / 31_536_000)
    };
    let detail = if seconds < 0 {
        format!(
            "Last opened in Git Manager at {}; the saved time is ahead of the system clock. Use outside Git Manager is not tracked.",
            entry.last_opened
        )
    } else {
        format!(
            "Last opened in Git Manager at {}. Use from terminals, editors, or other apps is not tracked.",
            entry.last_opened
        )
    };
    (label, detail)
}

fn worktree_status_label(worktree: &WorktreeInfo, dark: bool) -> (String, egui::Color32) {
    let status = &worktree.status;
    if let Some(error) = status.inspection_error.as_deref() {
        return (
            format!("Status unavailable: {}", error),
            App::adaptive_red(dark),
        );
    }
    if status.directory_missing {
        return ("Directory missing".into(), App::adaptive_yellow(dark));
    }

    let mut parts = vec![if status.change_path_count == 0 {
        "Clean".to_string()
    } else {
        format!("{} affected", status.change_path_count)
    }];
    if status.staged_changes > 0 {
        parts.push(format!("{} staged", status.staged_changes));
    }
    if status.unstaged_changes > 0 {
        parts.push(format!("{} modified", status.unstaged_changes));
    }
    if status.untracked_paths > 0 {
        parts.push(format!("{} untracked", status.untracked_paths));
    }
    if status.ignored_paths > 0 {
        parts.push(format!("{} ignored", status.ignored_paths));
    }
    if status.conflicted_paths > 0 {
        parts.push(format!("{} conflicts", status.conflicted_paths));
    }
    match (status.upstream.as_deref(), status.ahead, status.behind) {
        (Some(upstream), Some(ahead), Some(behind)) => {
            if ahead > 0 {
                parts.push(format!("{} ahead of {}", ahead, upstream));
            }
            if behind > 0 {
                parts.push(format!("{} behind {}", behind, upstream));
            }
            if ahead == 0 && behind == 0 {
                parts.push(format!("Up to date with {}", upstream));
            }
        }
        (Some(_), _, _) => parts.push("Tracking unknown".into()),
        (None, _, _) if worktree.branch.is_some() => parts.push("No upstream".into()),
        _ => {}
    }
    if !worktree.is_main {
        match status.merged_into_main {
            Some(true) => parts.push("Merged to main".into()),
            Some(false) => parts.push("Not merged".into()),
            None => parts.push("Merge status unknown".into()),
        }
    }
    if status.locked {
        parts.push("Locked".into());
    }

    let needs_attention = status.has_removal_blockers()
        || status.ahead.is_some_and(|ahead| ahead > 0)
        || status.behind.is_some_and(|behind| behind > 0)
        || status.merged_into_main == Some(false)
        || (!worktree.is_main && status.merged_into_main.is_none())
        || (status.upstream.is_none() && worktree.branch.is_some())
        || (status.upstream.is_some() && (status.ahead.is_none() || status.behind.is_none()));
    let color = if needs_attention {
        App::adaptive_yellow(dark)
    } else {
        App::adaptive_green(dark)
    };
    (parts.join(" · "), color)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new("Worktrees").heading()).truncate()).on_hover_text("Worktrees");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.is_busy();
            if crate::ui::add_enabled_ellipsis(ui, !busy, "🔄 Refresh").clicked() {
                app.refresh_all(ctx);
            }
            if crate::ui::add_enabled_ellipsis(ui, !busy, "Prune").clicked() {
                app.start_operation(ctx, "Pruning stale worktrees", GitOperation::PruneWorktrees);
            }
        });
    });

    ui.separator();

    let worktrees = app.worktrees.clone();
    let main_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| w.is_main).cloned().collect();
    let linked_wts: Vec<WorktreeInfo> = worktrees.iter().filter(|w| !w.is_main).cloned().collect();

    // ── Column header row (left-to-right: Path | Branch/SHA | Status | Actions) ─
    ui.horizontal(|ui| {
        let cw = &mut app.column_widths;
        let avail = ui.available_width();
        let status_w = worktree_status_column_width(avail);
        let reserved = WORKTREE_ACTIONS_WIDTH + status_w;
        let max_cols = (avail - reserved).max(120.0);

        // Only Path column is draggable; Branch/SHA fills the remaining width.
        let mut path_w = cw.get("worktree_path", 280.0);
        path_w = path_w.clamp(60.0, max_cols - 60.0);
        let bs_w = max_cols - path_w;

        column_header(ui, "Path", &mut path_w, 60.0, max_cols - 60.0, "wt_path_hdr");
        cw.set("worktree_path", path_w);
        column_header_static(ui, "Branch / opened", bs_w);
        if status_w > 0.0 {
            column_header_static(ui, "Status", status_w);
        }

        // Actions (rightmost)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new("Actions").strong()));
        });
    });

    ui.separator();

    // ── Content ──────────────────────────────────────────────────────
    let max_list_height = (ui.available_height() * 0.67).max(150.0);

    // Main Worktree (scrollable)
    if !main_wts.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("wt_main_list")
            .max_height(max_list_height)
            .show(ui, |ui| {
            ui.label(egui::RichText::new("Main Worktree").strong());
            for wt in &main_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        });
    }

    // Separator between Main and Linked (outside ScrollArea)
    if !linked_wts.is_empty() {
        if !main_wts.is_empty() {
            ui.add_space(10.0);
        }
        ui.separator();
    }

    // Linked Worktrees (scrollable)
    if !linked_wts.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("wt_linked_list")
            .max_height(max_list_height)
            .show(ui, |ui| {
            ui.label(egui::RichText::new("Linked Worktrees").strong());
            for wt in &linked_wts {
                show_worktree_row(app, ui, ctx, wt);
            }
        });
    }

    // Separator after Linked / before Add Worktree (outside ScrollArea)
    if !linked_wts.is_empty() || !main_wts.is_empty() {
        ui.add_space(10.0);
        ui.separator();
    }
    ui.heading("Add Worktree");

    ui.horizontal(|ui| {
        ui.label("Branch:");
        ui.text_edit_singleline(&mut app.new_worktree_branch);
        ui.checkbox(&mut app.new_worktree_create_branch, "Create new branch");
    });
    egui::CollapsingHeader::new("Advanced")
        .id_salt("worktree_creation_advanced")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Worktree name:");
                ui.text_edit_singleline(&mut app.new_worktree_name);
                ui.label("(defaults from branch)");
            });
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut app.new_worktree_path);
                ui.label("(leave empty for default)");
            });
        });

    let busy = app.is_busy();
    if crate::ui::add_enabled_ellipsis(ui, !busy, "Add Worktree").clicked() {
        let branch = app.new_worktree_branch.trim().to_string();
        if branch.is_empty() {
            app.show_error("Branch required".into());
        } else {
            let name = match worktree_name_for_creation(&branch, &app.new_worktree_name) {
                Ok(name) => name,
                Err(error) => {
                    app.show_error(error.into());
                    return;
                }
            };
            let path = if app.new_worktree_path.trim().is_empty() {
                let Some(repo_path) = app.git.path() else {
                    app.show_error(
                        "Cannot determine the repository path for a default worktree".into(),
                    );
                    return;
                };
                let Some(path) = default_worktree_path(repo_path, &name) else {
                    app.show_error(
                        "Cannot choose a default worktree path because the repository has no parent directory"
                            .into(),
                    );
                    return;
                };
                path
            } else {
                std::path::PathBuf::from(app.new_worktree_path.trim())
            };

            let form_submission = FormSubmission::CreateWorktree {
                name: app.new_worktree_name.clone(),
                path: app.new_worktree_path.clone(),
                branch: app.new_worktree_branch.clone(),
                create_branch: app.new_worktree_create_branch,
            };
            app.start_operation_with_form_submission(
                ctx,
                &format!("Create worktree '{}'", name),
                GitOperation::CreateWorktree {
                    name,
                    path,
                    branch: Some(branch),
                    new_branch: app.new_worktree_create_branch,
                },
                form_submission,
            );
        }
    }
}

fn default_worktree_name(value: &str) -> String {
    const SHA256_SUFFIX_LENGTH: usize = 1 + 64;
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut name = String::new();
    for byte in value.bytes() {
        push_encoded_branch_byte(&mut name, byte);
    }
    if name.len() > MAX_WORKTREE_NAME_BYTES {
        let prefix_limit = MAX_WORKTREE_NAME_BYTES - SHA256_SUFFIX_LENGTH;
        let mut prefix = String::new();
        for byte in value.bytes() {
            let previous_len = prefix.len();
            push_encoded_branch_byte(&mut prefix, byte);
            if prefix.len() > prefix_limit {
                prefix.truncate(previous_len);
                break;
            }
        }

        let digest = ring::digest::digest(&ring::digest::SHA256, value.as_bytes());
        name = prefix;
        name.push('~');
        for byte in digest.as_ref() {
            name.push(HEX[(byte >> 4) as usize] as char);
            name.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    if name.is_empty() {
        return "worktree".into();
    }
    if is_windows_reserved_device_name(&name) {
        let escaped_first = format!("~{:02X}", value.as_bytes()[0]);
        name.replace_range(0..1, &escaped_first);
    }
    name
}

fn push_encoded_branch_byte(name: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    if byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_') {
        name.push(char::from(byte));
    } else {
        name.push('~');
        name.push(HEX[(byte >> 4) as usize] as char);
        name.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let device_name = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(|character| matches!(character, ' ' | '.'))
        .to_ascii_uppercase();
    matches!(
        device_name.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        device_name.strip_prefix(prefix).is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

fn default_worktree_path(
    repo_path: &std::path::Path,
    name: &str,
) -> Option<std::path::PathBuf> {
    repo_path.parent().map(|parent| parent.join(name))
}

fn worktree_name_for_creation(branch: &str, name_input: &str) -> Result<String, &'static str> {
    if name_input.trim().is_empty() {
        return Ok(default_worktree_name(branch));
    }

    let has_invalid_character = name_input.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
    });
    if name_input.len() > MAX_WORKTREE_NAME_BYTES
        || matches!(name_input, "." | "..")
        || name_input.ends_with(' ')
        || name_input.ends_with('.')
        || has_invalid_character
        || is_windows_reserved_device_name(name_input)
    {
        return Err("Worktree name must be a single valid directory name up to 240 bytes");
    }

    Ok(name_input.into())
}

fn show_worktree_row(app: &mut App, ui: &mut egui::Ui, ctx: &egui::Context, wt: &WorktreeInfo) {
    let wt_path = wt.path.clone();
    let busy = app.is_busy();
    let icon = if wt.is_main { "★ " } else { "○ " };

    let branch_display = wt.branch.as_deref().unwrap_or("detached");
    let sha_short = wt.sha.get(..7).unwrap_or(&wt.sha);
    let path_display = wt.path.to_string_lossy().to_string();
    let avail = ui.available_width();
    let status_w = worktree_status_column_width(avail);
    let reserved = WORKTREE_ACTIONS_WIDTH + status_w;
    let is_current = app
        .git
        .path()
        .is_some_and(|current_path| current_path == wt_path.as_path());
    let max_cols = (avail - reserved).max(120.0);
    let mut path_w = app.column_widths.get("worktree_path", 280.0);
    path_w = path_w.clamp(60.0, max_cols - 60.0);
    let bs_w = max_cols - path_w;
    let (status_text, status_color) = worktree_status_label(wt, ui.style().visuals.dark_mode);
    let (recent_label, recent_detail) =
        recent_open_activity(app.recent_repos.entries(), &wt.path, Local::now());
    let mut branch_sha_text = format!(
        "{}{} [{}] · {} ({})",
        icon, branch_display, sha_short, recent_label, recent_detail
    );
    if status_w == 0.0 {
        branch_sha_text.push_str(&format!(" · {}", status_text));
    }

    // Left-to-right flow: Path, Branch/open history, Status, Actions.
    ui.horizontal(|ui| {
        column_cell(ui, path_w, &path_display, egui::Color32::GRAY);

        column_cell(ui, bs_w, &branch_sha_text, ui.style().visuals.text_color());
        if status_w > 0.0 {
            column_cell(ui, status_w, &status_text, status_color);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            if !wt.is_main {
                ui.menu_button("…", |ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Remove…"))
                        .clicked()
                    {
                        app.preview_worktree_cleanup(ctx, wt, false);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Force Remove…"))
                        .clicked()
                    {
                        app.preview_worktree_cleanup(ctx, wt, true);
                        ui.close_menu();
                    }
                });
            }
            let folder_button = ui.add_enabled(!busy, egui::Button::new("Folder"));
            if folder_button.clicked() {
                if let Err(error) = open::that(&wt_path) {
                    app.show_error(format!(
                        "Could not open worktree in file manager: {}",
                        error
                    ));
                }
            }
            folder_button.on_hover_text("Open this worktree in the file manager");

            let open_button = ui.add_enabled(!busy && !is_current, egui::Button::new("Open"));
            if open_button.clicked() {
                open_worktree(app, &wt_path);
            }
            open_button.on_hover_text("Open this worktree in Git Manager");
        });
    });
}

fn open_worktree(app: &mut App, path: &std::path::Path) {
    app.open_repo_path(path);
}

#[cfg(test)]
mod tests {
    use super::{
        default_worktree_name, default_worktree_path, open_worktree, recent_open_activity,
        worktree_name_for_creation, worktree_status_column_width, worktree_status_label,
    };
    use crate::app::App;
    use crate::git_ops::{WorktreeInfo, WorktreeStatusSummary};
    use crate::recent::{RecentEntry, RecentRepos};
    use chrono::{Local, TimeZone};
    use std::path::{Path, PathBuf};

    #[test]
    fn status_column_hides_when_the_window_is_too_narrow() {
        assert_eq!(worktree_status_column_width(440.0), 0.0);
        assert_eq!(worktree_status_column_width(458.0), 88.0);
        assert_eq!(worktree_status_column_width(800.0), 170.0);
    }

    #[test]
    fn recent_open_activity_shows_last_git_manager_open() {
        let path = Path::new("/repo/feature");
        let entries = [RecentEntry {
            path: path.to_string_lossy().into_owned(),
            name: "feature".into(),
            last_opened: "2025-01-31 12:00:00".into(),
        }];
        let now = Local
            .with_ymd_and_hms(2025, 2, 1, 12, 0, 0)
            .single()
            .expect("local time");

        let (label, detail) = recent_open_activity(&entries, path, now);

        assert_eq!(label, "Opened here 1d ago");
        assert!(detail.contains("Last opened in Git Manager"));
        assert!(detail.contains("not tracked"));
    }

    #[test]
    fn recent_open_activity_does_not_claim_missing_history_means_unused() {
        let now = Local
            .with_ymd_and_hms(2025, 2, 1, 12, 0, 0)
            .single()
            .expect("local time");

        let (label, detail) = recent_open_activity(&[], Path::new("/repo/feature"), now);

        assert_eq!(label, "No app history record");
        assert!(detail.contains("not tracked"));
    }

    #[test]
    fn worktree_status_label_surfaces_changes_and_tracking_state() {
        let mut worktree = WorktreeInfo {
            path: PathBuf::from("/repo/feature"),
            branch: Some("feature".into()),
            sha: "1234567890".into(),
            head_snapshot: None,
            is_main: false,
            git_link_identity: None,
            status: WorktreeStatusSummary {
                staged_changes: 1,
                untracked_paths: 1,
                change_path_count: 2,
                upstream: Some("origin/feature".into()),
                ahead: Some(2),
                behind: Some(1),
                merged_into_main: Some(false),
                ..WorktreeStatusSummary::default()
            },
        };

        let (label, color) = worktree_status_label(&worktree, false);
        assert!(label.contains("2 affected"));
        assert!(label.contains("1 staged"));
        assert!(label.contains("1 untracked"));
        assert!(label.contains("2 ahead"));
        assert!(label.contains("1 behind"));
        assert!(label.contains("Not merged"));
        assert_eq!(color, App::adaptive_yellow(false));

        worktree.status = WorktreeStatusSummary {
            inspection_error: Some("status unavailable".into()),
            ..WorktreeStatusSummary::default()
        };
        let (label, color) = worktree_status_label(&worktree, false);
        assert!(label.contains("Status unavailable"));
        assert_eq!(color, App::adaptive_red(false));
    }

    #[test]
    fn root_repository_has_no_default_worktree_path() {
        #[cfg(windows)]
        let root = Path::new(r"C:\");
        #[cfg(not(windows))]
        let root = Path::new("/");

        assert_eq!(default_worktree_path(root, "feature"), None);
    }

    #[test]
    fn open_action_switches_to_worktree_repository() {
        let worktree_dir = tempfile::tempdir().expect("worktree directory");
        drop(git2::Repository::init(worktree_dir.path()).expect("initialize worktree repository"));
        let recent_dir = tempfile::tempdir().expect("recent repositories directory");
        let mut app = App::new();
        app.recent_repos = RecentRepos::load_from(recent_dir.path().join("recent.json"));

        open_worktree(&mut app, worktree_dir.path());

        assert_eq!(app.repo_path, worktree_dir.path().to_str().expect("UTF-8 path"));
        assert_eq!(app.git.path(), Some(worktree_dir.path()));
        let ctx = eframe::egui::Context::default();
        for _ in 0..200 {
            app.process_pending_ops(&ctx);
            if !app.is_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.is_busy(), "repository refresh should finish");
    }

    #[test]
    fn default_worktree_path_is_sibling_of_repository() {
        let repo_path = PathBuf::from("parent").join("repository");

        assert_eq!(
            default_worktree_path(&repo_path, "feature"),
            Some(PathBuf::from("parent").join("feature"))
        );
    }

    #[test]
    fn explicit_worktree_names_are_preserved_or_rejected() {
        assert_eq!(
            worktree_name_for_creation("feature/new", "Release-v1.0"),
            Ok("Release-v1.0".into())
        );
        assert_eq!(
            worktree_name_for_creation("feature/new", " Release"),
            Ok(" Release".into())
        );
        assert!(worktree_name_for_creation("feature/new", "Release/v1.0").is_err());
        assert!(worktree_name_for_creation("feature/new", "../../outside").is_err());
        assert!(worktree_name_for_creation("feature/new", "CON.txt").is_err());
        assert_eq!(
            worktree_name_for_creation("feature/new", ""),
            Ok("feature~2Fnew".into())
        );
    }

    #[test]
    fn windows_superscript_device_names_are_rejected() {
        for name in ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"] {
            assert!(
                worktree_name_for_creation("feature/new", name).is_err(),
                "reserved name was accepted: {name}"
            );
        }
    }

    #[test]
    fn branch_default_uses_one_distinct_portable_name_for_worktree_and_directory() {
        let repo_path = PathBuf::from("parent").join("repository");
        let worktree_name = default_worktree_name("feature/add-login");

        assert_eq!(worktree_name, "feature~2Fadd-login");
        assert_eq!(
            default_worktree_name(r"feature\add-login"),
            "feature~5Cadd-login"
        );
        assert_eq!(
            default_worktree_name("feature/<user>"),
            "feature~2F~3Cuser~3E"
        );
        let slash_branch_name = default_worktree_name("feature/login");
        let hyphen_branch_name = default_worktree_name("feature-login");
        assert_ne!(slash_branch_name, hyphen_branch_name);
        assert_ne!(
            default_worktree_path(&repo_path, &slash_branch_name),
            default_worktree_path(&repo_path, &hyphen_branch_name)
        );
        assert_ne!(
            default_worktree_name("Feature/login"),
            default_worktree_name("feature/login")
        );
        assert_eq!(
            default_worktree_name("CON.txt"),
            "~43~4F~4E~2Etxt"
        );
        assert_eq!(default_worktree_name("con"), "~63on");
        assert_eq!(default_worktree_name("lpt9.log"), "lpt9~2Elog");
        assert_eq!(default_worktree_name("feature."), "feature~2E");
        assert_eq!(default_worktree_name(".."), "~2E~2E");
        assert_eq!(default_worktree_name(""), "worktree");
        let long_branch_a = format!("{}A", "A".repeat(85));
        let long_branch_b = format!("{}B", "A".repeat(85));
        let long_name_a = default_worktree_name(&long_branch_a);
        let long_name_b = default_worktree_name(&long_branch_b);
        assert!(long_name_a.len() <= 240);
        assert!(long_name_b.len() <= 240);
        assert_ne!(long_name_a, long_name_b);
        assert_eq!(
            default_worktree_path(&repo_path, &worktree_name),
            Some(PathBuf::from("parent").join("feature~2Fadd-login"))
        );
    }
}
