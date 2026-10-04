use crate::app::App;
use crate::git_ops::TaskReviewQueueState;
use crate::task_delivery::{
    CheckState, PullRequestAction, PullRequestCheck, PullRequestRemoteView,
    PullRequestSnapshot, PullRequestStatusView, TaskSourceFreshness,
};
use crate::task_verification::VerificationState;
use crate::tasks::{dependency_order, resolve_dependency, DependencyReference, TaskRecord};
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
        if app.task_integrated_verification.result().is_some()
            || app.task_integrated_verification.is_running()
        {
            app.task_integrated_verification.refresh_current_state(
                &app.task_merge_preview.repository_path,
                &app.task_merge_preview.base_ref,
                None,
                None,
                Some(false),
                &[],
                &HashMap::new(),
            );
            render_integrated_verification(
                app,
                ui,
                ctx,
                &[],
                Some(false),
                &None,
                &None,
            );
        }
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
    render_merge_preview(app, ui, ctx, &entries, &task_states);

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

fn render_merge_preview(
    app: &mut App,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    entries: &[TaskRecord],
    task_states: &[(TaskRecord, Option<TaskReviewQueueState>)],
) {
    use crate::task_merge_preview::TaskMergePreviewState;

    app.task_merge_preview.poll();
    let running = app.task_merge_preview.is_running();
    let integrated_running = app.task_integrated_verification.is_running();
    let controls_enabled = !running && !integrated_running;
    ui.add_space(10.0);
    ui.separator();
    ui.heading("Non-destructive merge preview");
    ui.weak(
        "Select task sources, a repository, and a local base branch. The preview follows saved task dependencies and uses only changes captured from clean task worktrees.",
    );

    let mut repository_paths = entries
        .iter()
        .map(|task| task.repository_path.clone())
        .collect::<Vec<_>>();
    repository_paths.sort();
    repository_paths.dedup();
    if repository_paths.is_empty() {
        ui.label("No linked task repositories are available to preview.");
        return;
    }
    if !running && !repository_paths
        .iter()
        .any(|path| path == &app.task_merge_preview.repository_path)
    {
        app.task_merge_preview
            .set_repository(repository_paths[0].clone());
    }

    let mut selected_repository = app.task_merge_preview.repository_path.clone();
    ui.add_enabled_ui(controls_enabled, |ui| {
        egui::ComboBox::from_id_salt("task_merge_preview_repository")
            .selected_text(&selected_repository)
            .show_ui(ui, |ui| {
                for path in &repository_paths {
                    ui.selectable_value(&mut selected_repository, path.clone(), path);
                }
            });
    });
    if selected_repository != app.task_merge_preview.repository_path {
        app.task_merge_preview.set_repository(selected_repository);
    }
    app.task_merge_preview.refresh_base_refs_if_due();
    ctx.request_repaint_after(std::time::Duration::from_secs(5));

    if let Some(error) = app.task_merge_preview.base_ref_error.as_deref() {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            format!("Base branches unavailable: {error}"),
        );
    } else {
        let mut selected_base = app.task_merge_preview.base_ref.clone();
        ui.add_enabled_ui(controls_enabled, |ui| {
            egui::ComboBox::from_id_salt("task_merge_preview_base")
                .selected_text(if selected_base.is_empty() {
                    "Choose a local base branch"
                } else {
                    selected_base.as_str()
                })
                .show_ui(ui, |ui| {
                    for branch in &app.task_merge_preview.base_refs {
                        ui.selectable_value(&mut selected_base, branch.clone(), branch);
                    }
                });
        });
        if selected_base != app.task_merge_preview.base_ref {
            app.task_merge_preview.base_ref = selected_base;
            app.task_merge_preview.invalidate();
        }
    }

    let repository_tasks = entries
        .iter()
        .filter(|task| task.repository_path == app.task_merge_preview.repository_path)
        .cloned()
        .collect::<Vec<_>>();
    let (ordered_ids, order_unavailable) = dependency_order(&repository_tasks);
    let mut selection_changed = false;
    ui.label("Choose tasks to integrate in dependency order:");
    ui.add_enabled_ui(controls_enabled, |ui| {
        for task in &repository_tasks {
            if let Some(reason) = order_unavailable.get(&task.id) {
                ui.colored_label(
                    App::adaptive_yellow(ui.style().visuals.dark_mode),
                    format!("{} · order unavailable: {reason}", task.title),
                );
                continue;
            }
            let mut selected = app
                .task_merge_preview
                .selected_task_ids
                .contains(&task.id);
            if ui.checkbox(&mut selected, &task.title).changed() {
                if selected {
                    app.task_merge_preview
                        .selected_task_ids
                        .insert(task.id.clone());
                } else {
                    app.task_merge_preview
                        .selected_task_ids
                        .remove(&task.id);
                }
                selection_changed = true;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Select all available").clicked() {
                for task_id in &ordered_ids {
                    app.task_merge_preview
                        .selected_task_ids
                        .insert(task_id.clone());
                }
                selection_changed = true;
            }
            if ui.button("Clear selection").clicked()
                && !app.task_merge_preview.selected_task_ids.is_empty()
            {
                app.task_merge_preview.selected_task_ids.clear();
                selection_changed = true;
            }
        });
    });
    if selection_changed {
        app.task_merge_preview.invalidate();
    }

    let selected_ids = app.task_merge_preview.selected_task_ids.clone();
    let missing_selected = selected_ids
        .iter()
        .filter(|id| {
            !repository_tasks
                .iter()
                .any(|task| task.id.as_str() == id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    let blocked_selected = selected_ids
        .iter()
        .filter_map(|id| order_unavailable.get(id).map(|reason| (id, reason)))
        .collect::<Vec<_>>();
    let omitted_prerequisites = repository_tasks
        .iter()
        .filter(|task| selected_ids.contains(&task.id))
        .flat_map(|task| {
            task.dependencies
                .iter()
                .filter(|dependency| !selected_ids.contains(&dependency.task_id))
                .map(|dependency| {
                    let prerequisite = repository_tasks
                        .iter()
                        .find(|candidate| candidate.id == dependency.task_id)
                        .map(|candidate| candidate.title.as_str())
                        .unwrap_or(dependency.task_id.as_str());
                    (task.title.clone(), prerequisite.to_owned())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let ordered_tasks = ordered_ids
        .iter()
        .filter(|id| selected_ids.contains(*id))
        .filter_map(|id| repository_tasks.iter().find(|task| task.id == *id).cloned())
        .collect::<Vec<_>>();
    let result_needs_identity_check = matches!(
        app.task_merge_preview.state(),
        TaskMergePreviewState::ConflictFree { .. } | TaskMergePreviewState::Conflicted { .. }
    );
    if result_needs_identity_check
        && !app.task_merge_preview.selection_matches(
            &app.task_merge_preview.repository_path,
            &app.task_merge_preview.base_ref,
            &ordered_tasks,
        )
    {
        app.task_merge_preview.show_unavailable(
            "The selected task set or dependency order changed after this preview. Run it again.",
        );
    }
    let stale_source_reason = match app.task_merge_preview.state() {
        TaskMergePreviewState::ConflictFree {
            source_fingerprints,
            ..
        }
        | TaskMergePreviewState::Conflicted {
            source_fingerprints,
            ..
        } => source_fingerprints.iter().find_map(|(task_id, expected)| {
            let (task, current_state) = task_states.iter().find(|(task, _)| &task.id == task_id)?;
            match current_state {
                Some(TaskReviewQueueState::Reviewable { source_fingerprint, .. })
                | Some(TaskReviewQueueState::NoReviewableChanges { source_fingerprint })
                    if source_fingerprint != expected => Some(format!(
                        "{} changed after the preview snapshot. Run the preview again.",
                        task.title
                    )),
                Some(TaskReviewQueueState::Unavailable(reason))
                | Some(TaskReviewQueueState::Error(reason)) => Some(format!(
                    "{} is no longer available for preview: {reason}",
                    task.title
                )),
                _ => None,
            }
        }),
        _ => None,
    };
    if let Some(reason) = stale_source_reason {
        app.task_merge_preview.show_unavailable(reason);
    }
    let base_freshness_reason = app.task_merge_preview.check_base_freshness();
    let base_state_current = base_freshness_reason.is_none();
    if let Some(reason) = base_freshness_reason {
        app.task_merge_preview.show_unavailable(reason);
    }
    let current_source_fingerprints = task_states
        .iter()
        .filter_map(|(task, state)| match state {
            Some(TaskReviewQueueState::Reviewable { source_fingerprint, .. })
            | Some(TaskReviewQueueState::NoReviewableChanges { source_fingerprint }) => {
                Some((task.id.clone(), source_fingerprint.clone()))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let (current_base_oid, current_tree_oid, source_state_current) = match app.task_merge_preview.state() {
        TaskMergePreviewState::ConflictFree {
            base_oid,
            preview_tree_oid,
            source_fingerprints,
            ..
        } => {
            let mut all_current = true;
            let mut unknown = false;
            for task in &ordered_tasks {
                match (
                    current_source_fingerprints.get(&task.id),
                    source_fingerprints.get(&task.id),
                ) {
                    (Some(current), Some(expected)) if current == expected => {}
                    (Some(_), Some(_)) => all_current = false,
                    _ => {
                        let is_unavailable = task_states
                            .iter()
                            .find(|(candidate, _)| candidate.id == task.id)
                            .is_some_and(|(_, state)| {
                                matches!(
                                    state,
                                    Some(TaskReviewQueueState::Unavailable(_)
                                        | TaskReviewQueueState::Error(_))
                                )
                            });
                        if is_unavailable {
                            all_current = false;
                        } else {
                            unknown = true;
                        }
                    }
                }
            }
            let source_state_current = if !all_current {
                Some(false)
            } else if unknown {
                None
            } else {
                Some(true)
            };
            (
                Some(base_oid.clone()),
                Some(preview_tree_oid.clone()),
                source_state_current,
            )
        }
        TaskMergePreviewState::Running => (None, None, None),
        _ => (None, None, Some(false)),
    };
    let selection_is_current = app.task_merge_preview.selection_matches(
        &app.task_merge_preview.repository_path,
        &app.task_merge_preview.base_ref,
        &ordered_tasks,
    );
    let preview_is_current = if !selection_is_current || !base_state_current {
        Some(false)
    } else {
        source_state_current
    };
    app.task_integrated_verification.refresh_current_state(
        &app.task_merge_preview.repository_path,
        &app.task_merge_preview.base_ref,
        current_base_oid.as_deref(),
        current_tree_oid.as_deref(),
        preview_is_current,
        &ordered_tasks,
        &current_source_fingerprints,
    );
    if matches!(
        app.task_merge_preview.state(),
        TaskMergePreviewState::ConflictFree { .. } | TaskMergePreviewState::Conflicted { .. }
    ) {
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }
    let can_preview = controls_enabled
        && !ordered_tasks.is_empty()
        && missing_selected.is_empty()
        && blocked_selected.is_empty()
        && omitted_prerequisites.is_empty()
        && app.task_merge_preview.base_ref_error.is_none()
        && app
            .task_merge_preview
            .base_refs
            .contains(&app.task_merge_preview.base_ref);
    if ui
        .add_enabled(can_preview, egui::Button::new("Preview selected tasks"))
        .clicked()
    {
        for task in &ordered_tasks {
            app.invalidate_task_review_queue_state(&task.id);
        }
        app.task_merge_preview.start(ctx, ordered_tasks);
    }
    if !blocked_selected.is_empty() {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            "One or more selected tasks have an unavailable dependency order.",
        );
    }
    if !missing_selected.is_empty() {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            "One or more selected tasks are no longer available in this repository. Clear the selection and choose current tasks.",
        );
    }
    for (task_title, prerequisite) in &omitted_prerequisites {
        ui.colored_label(
            App::adaptive_yellow(ui.style().visuals.dark_mode),
            format!("{task_title} depends on {prerequisite}; select that prerequisite too."),
        );
    }

    match app.task_merge_preview.state() {
        TaskMergePreviewState::Idle => {}
        TaskMergePreviewState::Running => {
            ui.label("Checking captured task sources in a temporary integration area…");
        }
        TaskMergePreviewState::ConflictFree {
            changed_files,
            task_titles,
            ..
        } => {
            ui.colored_label(
                App::adaptive_green(ui.style().visuals.dark_mode),
                format!("No file conflicts found across {} task(s).", task_titles.len()),
            );
            render_preview_changed_files(ui, changed_files);
            ui.weak("A clean preview does not prove that tests pass or that the changes are safe to merge.");
        }
        TaskMergePreviewState::Conflicted {
            changed_files,
            task_titles,
            task_title,
            task_position,
            conflicts,
            ..
        } => {
            ui.colored_label(
                App::adaptive_red(ui.style().visuals.dark_mode),
                format!(
                    "Conflicts while integrating {task_title} at task {task_position} of {}.",
                    task_titles.len()
                ),
            );
            render_preview_changed_files(ui, changed_files);
            ui.label("Conflicted paths and available merge stages:");
            egui::ScrollArea::vertical()
                .max_height(110.0)
                .show(ui, |ui| {
                    for conflict in conflicts {
                        ui.monospace(format!(
                            "{} · {}",
                            conflict.path,
                            conflict.stages.join(" + ")
                        ));
                    }
                });
        }
        TaskMergePreviewState::Unavailable {
            reason,
            changed_files,
            task_titles,
        } => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!("Preview unavailable: {reason}"),
            );
            if !task_titles.is_empty() {
                ui.label(format!("Selected task sources: {}", task_titles.join(", ")));
            }
            if !changed_files.is_empty() {
                render_preview_changed_files(ui, changed_files);
            }
        }
    }
    render_integrated_verification(
        app,
        ui,
        ctx,
        &ordered_tasks,
        preview_is_current,
        &current_base_oid,
        &current_tree_oid,
    );
    ui.weak("The preview does not merge, commit, or push task changes.");
}

fn render_integrated_verification(
    app: &mut App,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    tasks: &[TaskRecord],
    preview_is_current: Option<bool>,
    base_oid: &Option<String>,
    tree_oid: &Option<String>,
) {
    use crate::task_integrated_verification::{
        resolve_command_coverage, IntegratedVerificationStatus,
    };

    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new("Integrated verification").strong());
    let coverage = resolve_command_coverage(tasks);
    match &coverage {
        Ok(coverage) if coverage.commands.is_empty() => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                "Unavailable · none of the selected tasks has a verification command configured.",
            );
        }
        Ok(coverage) => {
            ui.label("Configured commands that apply to this ordered task selection:");
            for (index, entry) in coverage.commands.iter().enumerate() {
                let covered = entry
                    .task_ids
                    .iter()
                    .filter_map(|id| tasks.iter().find(|task| &task.id == id))
                    .map(|task| format!("{} ({})", task.title, task.id))
                    .collect::<Vec<_>>();
                ui.monospace(format!(
                    "#{} · {} {:?} · timeout {}s",
                    index + 1,
                    entry.command.executable,
                    entry.command.args,
                    entry.command.timeout_seconds
                ));
                ui.label(format!("Configured for: {}", covered.join(", ")));
            }
            if !coverage.unconfigured_task_ids.is_empty() {
                let missing = coverage
                    .unconfigured_task_ids
                    .iter()
                    .filter_map(|id| tasks.iter().find(|task| &task.id == id))
                    .map(|task| format!("{} ({})", task.title, task.id))
                    .collect::<Vec<_>>();
                ui.colored_label(
                    App::adaptive_yellow(ui.style().visuals.dark_mode),
                    format!("No command configured for: {}", missing.join(", ")),
                );
            }
            if coverage.commands.len() > 1 {
                ui.weak("Selected tasks have different verification configurations. Each listed command runs once, in dependency order, for the tasks shown.");
            }
            if !coverage.unconfigured_task_ids.is_empty() {
                ui.weak("A passing aggregate will remain partial because these selected tasks have no configured command.");
            }
        }
        Err(error) => {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                format!("Integrated verification unavailable: {error}"),
            );
        }
    }

    let integrated_running = app.task_integrated_verification.is_running();
    if integrated_running {
        ui.horizontal(|ui| {
            ui.colored_label(
                App::adaptive_yellow(ui.style().visuals.dark_mode),
                "Running in the isolated combined preview tree…",
            );
            if ui.button("Cancel integrated verification").clicked() {
                app.cancel_task_integrated_verification();
            }
        });
    } else {
        let has_commands = coverage
            .as_ref()
            .is_ok_and(|coverage| !coverage.commands.is_empty());
        let can_run = preview_is_current == Some(true) && has_commands && !app.is_busy();
        if ui
            .add_enabled(can_run, egui::Button::new("Run integrated verification"))
            .clicked()
        {
            app.start_task_integrated_verification(ctx, tasks);
        }
        if preview_is_current != Some(true) {
            ui.label(if preview_is_current.is_none() {
                "Checking the current combined preview inputs…"
            } else {
                "Run verification only after a current, conflict-free preview is available."
            });
        }
    }

    if let Some(result) = app.task_integrated_verification.result() {
        let dark = ui.style().visuals.dark_mode;
        let color = match result.status {
            IntegratedVerificationStatus::Passed => App::adaptive_green(dark),
            IntegratedVerificationStatus::Failed
            | IntegratedVerificationStatus::Error => App::adaptive_red(dark),
            IntegratedVerificationStatus::Running
            | IntegratedVerificationStatus::Partial
            | IntegratedVerificationStatus::Cancelled
            | IntegratedVerificationStatus::TimedOut
            | IntegratedVerificationStatus::Stale
            | IntegratedVerificationStatus::Unavailable => App::adaptive_yellow(dark),
        };
        ui.colored_label(color, format!("Integrated result: {}", result.status.label()));
        if let Some(reason) = &result.stale_reason {
            ui.label(reason);
        }
        ui.label(format!(
            "Tasks in dependency order: {}",
            result.ordered_task_ids.join(" → ")
        ));
        ui.label(format!("Base commit: {}", result.base_oid));
        ui.label(format!("Combined preview tree: {}", result.preview_tree_oid));
        ui.label(format!("Started: {}", result.started_at));
        if let Some(finished_at) = &result.finished_at {
            ui.label(format!("Finished: {finished_at}"));
        }
        egui::CollapsingHeader::new("Bound input fingerprints")
            .id_salt(("integrated_verification_inputs", &result.run_id))
            .show(ui, |ui| {
                ui.label(format!("Repository: {}", result.repository_path));
                ui.label(format!("Base branch: {}", result.base_ref));
                for task in &result.tasks {
                    ui.monospace(format!(
                        "{} · {}",
                        task.task_id, task.source_fingerprint
                    ));
                }
            });
        for (index, outcome) in result.commands.iter().enumerate() {
            let covered = outcome
                .task_ids
                .iter()
                .filter_map(|id| result.tasks.iter().find(|task| &task.task_id == id))
                .map(|task| format!("{} ({})", task.title, task.task_id))
                .collect::<Vec<_>>();
            egui::CollapsingHeader::new(format!(
                "#{} · {} {:?} · {}",
                index + 1,
                outcome.command.executable,
                outcome.command.args,
                covered.join(", ")
            ))
            .id_salt(("integrated_verification", &result.run_id, &outcome.command.executable, &outcome.task_ids))
            .show(ui, |ui| {
                match &outcome.result {
                    Some(command_result) => {
                        ui.label(format!("Outcome: {}", command_result.state.label()));
                        if let Some(exit_code) = command_result.exit_code {
                            ui.label(format!("Exit code: {exit_code}"));
                        }
                        if command_result.output_truncated {
                            ui.label("Captured output was truncated.");
                        }
                        if !command_result.stdout.is_empty() {
                            ui.label("stdout");
                            ui.monospace(&command_result.stdout);
                        }
                        if !command_result.stderr.is_empty() {
                            ui.label("stderr");
                            ui.monospace(&command_result.stderr);
                        }
                        if command_result.stdout.is_empty() && command_result.stderr.is_empty() {
                            ui.label("No output captured.");
                        }
                    }
                    None => {
                        ui.label(outcome.not_run_reason.as_deref().unwrap_or("Not run"));
                    }
                };
            });
        }
        if !result.unconfigured_task_ids.is_empty() {
            ui.label(format!(
                "Unconfigured task IDs: {}",
                result.unconfigured_task_ids.join(", ")
            ));
        }
    } else if base_oid.is_some() && tree_oid.is_some() {
        ui.label("No integrated result has been saved for this preview.");
    }
}

fn render_preview_changed_files(ui: &mut egui::Ui, changed_files: &[String]) {
    ui.label(format!("Combined changed files ({}):", changed_files.len()));
    if changed_files.is_empty() {
        ui.label("No changed files in the selected task sources.");
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(110.0)
        .show(ui, |ui| {
            for path in changed_files {
                ui.monospace(path);
            }
        });
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
