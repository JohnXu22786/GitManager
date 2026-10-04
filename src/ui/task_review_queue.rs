use crate::app::App;
use crate::git_ops::TaskReviewQueueState;
use crate::task_delivery::{
    CheckState, PullRequestAction, PullRequestCheck, PullRequestRemoteView,
    PullRequestSnapshot, PullRequestStatusView, TaskSourceFreshness,
};
use crate::task_verification::VerificationState;
use crate::tasks::{resolve_dependency, DependencyReference, TaskRecord};
use eframe::egui;
use std::collections::{HashMap, HashSet};

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
        .iter()
        .cloned()
        .map(|task| {
            let state = app.current_task_review_queue_state(ctx, &task);
            (task, state)
        })
        .collect::<Vec<_>>();
    let pull_request_statuses = task_states
        .iter()
        .map(|(task, _)| {
            (
                task.id.clone(),
                app.current_task_pull_request_status(ctx, task),
            )
        })
        .collect::<HashMap<_, _>>();
    render_dependency_order(ui, &entries, &pull_request_statuses);

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
            ..
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
            render_shared_file_overlap(ui, task, state.as_ref(), &task_states);
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
                task,
                &pull_request_statuses[&task.id],
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
            render_shared_file_overlap(ui, task, state.as_ref(), &task_states);
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
                task,
                &pull_request_statuses[&task.id],
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

#[derive(Clone)]
enum DependencyReadiness {
    Ready,
    Delivered,
    Blocked(String),
}

fn render_dependency_order(
    ui: &mut egui::Ui,
    entries: &[TaskRecord],
    pull_request_statuses: &HashMap<String, PullRequestStatusView>,
) {
    ui.add_space(10.0);
    ui.separator();
    ui.heading("Dependency order");
    ui.weak(
        "Order comes only from explicit task dependencies. Shared file paths do not imply a dependency. A prerequisite is complete when its current task source has a merged PR.",
    );

    let mut repositories = Vec::new();
    for task in entries {
        if !repositories.contains(&task.repository_path) {
            repositories.push(task.repository_path.clone());
        }
    }

    for repository_path in repositories {
        let tasks = entries
            .iter()
            .filter(|task| task.repository_path == repository_path)
            .cloned()
            .collect::<Vec<_>>();
        let (ordered_ids, unavailable) = dependency_order(&tasks);
        ui.group(|ui| {
            ui.label(format!("Repository: {repository_path}"));
            let mut readiness = HashMap::new();
            for (position, task_id) in ordered_ids.iter().enumerate() {
                let Some(task) = tasks.iter().find(|task| task.id == *task_id) else {
                    continue;
                };
                let state = task_readiness(
                    task,
                    &tasks,
                    pull_request_statuses,
                    &mut HashSet::new(),
                    &mut readiness,
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{}. {}", position + 1, task.title));
                    match state {
                        DependencyReadiness::Delivered => ui.colored_label(
                            App::adaptive_green(ui.style().visuals.dark_mode),
                            "Delivered",
                        ),
                        DependencyReadiness::Ready => ui.colored_label(
                            App::adaptive_green(ui.style().visuals.dark_mode),
                            "Ready",
                        ),
                        DependencyReadiness::Blocked(reason) => ui.colored_label(
                            App::adaptive_yellow(ui.style().visuals.dark_mode),
                            format!("Blocked · {reason}"),
                        ),
                    };
                });
            }
            for task in &tasks {
                if let Some(reason) = unavailable.get(&task.id) {
                    ui.colored_label(
                        App::adaptive_yellow(ui.style().visuals.dark_mode),
                        format!("{} · order unavailable: {reason}", task.title),
                    );
                }
            }
        });
    }
}

fn dependency_order(tasks: &[TaskRecord]) -> (Vec<String>, HashMap<String, String>) {
    let mut unavailable = HashMap::new();
    for task in tasks {
        for dependency in &task.dependencies {
            let issue = match resolve_dependency(task, dependency, tasks) {
                DependencyReference::Resolved(_) => None,
                DependencyReference::Missing => Some(format!(
                    "dependency task {} is missing or unlinked",
                    dependency.task_id
                )),
                DependencyReference::RepositoryChanged => Some(format!(
                    "dependency task {} no longer matches its saved repository or worktree",
                    dependency.task_id
                )),
            };
            if let Some(issue) = issue {
                unavailable.entry(task.id.clone()).or_insert(issue);
                break;
            }
        }
    }

    loop {
        let blocked_by_unavailable = tasks.iter().find_map(|task| {
            if unavailable.contains_key(&task.id) {
                return None;
            }
            task.dependencies.iter().find_map(|dependency| {
                let DependencyReference::Resolved(target) =
                    resolve_dependency(task, dependency, tasks)
                else {
                    return None;
                };
                unavailable.get(&target.id).map(|_| {
                    (
                        task.id.clone(),
                        format!("depends on {} whose order is unavailable", target.title),
                    )
                })
            })
        });
        let Some((task_id, reason)) = blocked_by_unavailable else {
            break;
        };
        unavailable.insert(task_id, reason);
    }

    let mut ordered = Vec::new();
    let mut completed = HashSet::new();
    loop {
        let next = tasks.iter().find(|task| {
            !unavailable.contains_key(&task.id)
                && !completed.contains(&task.id)
                && task.dependencies.iter().all(|dependency| {
                    matches!(resolve_dependency(task, dependency, tasks),
                        DependencyReference::Resolved(target) if completed.contains(&target.id))
                })
        });
        let Some(task) = next else {
            break;
        };
        completed.insert(task.id.clone());
        ordered.push(task.id.clone());
    }

    for task in tasks {
        if !unavailable.contains_key(&task.id) && !completed.contains(&task.id) {
            unavailable.insert(
                task.id.clone(),
                "a dependency cycle prevents a valid order".into(),
            );
        }
    }
    (ordered, unavailable)
}

fn task_readiness(
    task: &TaskRecord,
    entries: &[TaskRecord],
    pull_request_statuses: &HashMap<String, PullRequestStatusView>,
    visiting: &mut HashSet<String>,
    memo: &mut HashMap<String, DependencyReadiness>,
) -> DependencyReadiness {
    if let Some(readiness) = memo.get(&task.id) {
        return readiness.clone();
    }
    if !visiting.insert(task.id.clone()) {
        return DependencyReadiness::Blocked("dependency cycle".into());
    }

    for dependency in &task.dependencies {
        let target = match resolve_dependency(task, dependency, entries) {
            DependencyReference::Resolved(target) => target,
            DependencyReference::Missing => {
                let result = DependencyReadiness::Blocked(format!(
                    "dependency task {} is missing or unlinked",
                    dependency.task_id
                ));
                visiting.remove(&task.id);
                memo.insert(task.id.clone(), result.clone());
                return result;
            }
            DependencyReference::RepositoryChanged => {
                let result = DependencyReadiness::Blocked(format!(
                    "dependency task {} no longer matches its saved repository or worktree",
                    dependency.task_id
                ));
                visiting.remove(&task.id);
                memo.insert(task.id.clone(), result.clone());
                return result;
            }
        };
        match task_readiness(target, entries, pull_request_statuses, visiting, memo) {
            DependencyReadiness::Delivered => {}
            DependencyReadiness::Ready => {
                let result = DependencyReadiness::Blocked(format!(
                    "waiting for {} to be merged first",
                    target.title
                ));
                visiting.remove(&task.id);
                memo.insert(task.id.clone(), result.clone());
                return result;
            }
            DependencyReadiness::Blocked(reason) => {
                let result = DependencyReadiness::Blocked(format!(
                    "{} is blocked: {reason}",
                    target.title
                ));
                visiting.remove(&task.id);
                memo.insert(task.id.clone(), result.clone());
                return result;
            }
        }
    }

    let result = if task_pull_request_is_currently_merged(&task.id, pull_request_statuses) {
        DependencyReadiness::Delivered
    } else {
        DependencyReadiness::Ready
    };
    visiting.remove(&task.id);
    memo.insert(task.id.clone(), result.clone());
    result
}

fn task_pull_request_is_currently_merged(
    task_id: &str,
    pull_request_statuses: &HashMap<String, PullRequestStatusView>,
) -> bool {
    pull_request_statuses
        .get(task_id)
        .is_some_and(|status| match &status.remote {
            PullRequestRemoteView::Ready { snapshot } => {
                snapshot.is_merged()
                    && matches!(&snapshot.freshness, TaskSourceFreshness::Current { .. })
            }
            _ => false,
        })
}

fn render_shared_file_overlap(
    ui: &mut egui::Ui,
    task: &TaskRecord,
    state: Option<&TaskReviewQueueState>,
    task_states: &[(TaskRecord, Option<TaskReviewQueueState>)],
) {
    let same_repository = task_states
        .iter()
        .filter(|(other, _)| {
            other.id != task.id && other.repository_path == task.repository_path
        })
        .collect::<Vec<_>>();
    if same_repository.is_empty() {
        return;
    }

    ui.label(egui::RichText::new("Shared-file overlap").strong());
    ui.weak("Matching paths indicate shared files, not a merge conflict.");

    let Some(state) = state else {
        ui.label("Checking overlap · this task's diff state is still pending.");
        return;
    };
    let Some(paths) = task_changed_paths(state) else {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            "Overlap unavailable · this task's complete current diff or source state is unavailable.",
        );
        return;
    };

    let mut found_overlap = false;
    let mut unavailable_tasks = Vec::new();
    let mut checking_tasks = Vec::new();
    for (other, other_state) in same_repository {
        let Some(other_state) = other_state.as_ref() else {
            checking_tasks.push(other.title.as_str());
            continue;
        };
        let Some(other_paths) = task_changed_paths(other_state) else {
            unavailable_tasks.push(other.title.as_str());
            continue;
        };
        let overlapping_paths = paths
            .iter()
            .filter(|path| other_paths.contains(path))
            .collect::<Vec<_>>();
        if !overlapping_paths.is_empty() {
            found_overlap = true;
            ui.label(format!("Shared with {}:", other.title));
            ui.monospace(
                overlapping_paths
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
    }

    if !found_overlap && unavailable_tasks.is_empty() && checking_tasks.is_empty() {
        ui.label("No shared changed files in the current task diffs.");
    }
    if !unavailable_tasks.is_empty() {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            format!(
                "Overlap unavailable with {} · complete diff or source state is unavailable.",
                unavailable_tasks.join(", "),
            ),
        );
    }
    if !checking_tasks.is_empty() {
        ui.label(format!(
            "Checking overlap with {} · diff states are still pending.",
            checking_tasks.join(", "),
        ));
    }
}

fn task_changed_paths(state: &TaskReviewQueueState) -> Option<&[String]> {
    match state {
        TaskReviewQueueState::Reviewable { changed_paths, .. } => Some(changed_paths),
        TaskReviewQueueState::NoReviewableChanges { .. } => Some(&[]),
        TaskReviewQueueState::Unavailable(_) | TaskReviewQueueState::Error(_) => None,
    }
}

fn render_task_delivery(
    app: &mut App,
    ui: &mut egui::Ui,
    task: &TaskRecord,
    status: &PullRequestStatusView,
    current_source_fingerprint: Option<&str>,
    next_action: &mut Option<(
        TaskRecord,
        PullRequestAction,
        Option<String>,
        Option<String>,
    )>,
) {
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

    if let Some(message) = &status.action_message {
        let color = if message.succeeded {
            App::adaptive_green(ui.style().visuals.dark_mode)
        } else {
            App::adaptive_red(ui.style().visuals.dark_mode)
        };
        ui.colored_label(color, &message.text);
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
    let lifecycle = if snapshot.is_merged() {
        "Merged"
    } else {
        match snapshot.state.to_ascii_uppercase().as_str() {
            "CLOSED" => "Closed",
            "OPEN" if snapshot.is_draft => "Draft",
            "OPEN" => "Open",
            _ => "Unavailable",
        }
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
