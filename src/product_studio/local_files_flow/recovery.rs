//! Explicit verified import and fresh-location recovery, using existing helpers.
use super::{fresh, DestinationChoice, Event};
use crate::product_backup::{
    doctor, inspect_backup, open_verified, BackupIntake, BackupSummary, CheckpointReceipt,
    CheckpointShelf, OpenedTool, VerifiedBackup, MAX_BACKUP_BYTES,
};
use crate::product_contract::{canonical_bytes, Digest};
use crate::product_decisions::IntentionBundle;
use crate::product_locations::{
    CreatedTool, IssueKind, OperationIssue, RecentTools, SelectedFile, ToolIdentity, ToolLocations,
};
use crate::product_store::{
    LegacyUpgrade, ProductStore, ProjectSnapshot, RecoveryDestination, UpgradeProgress,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

type Result<T> = std::result::Result<T, OperationIssue>;

#[derive(Clone, Debug)]
pub struct ImportPreview {
    pub operation: String,
    pub source: String,
    pub backup: Digest,
    pub requires_upgrade: bool,
    pub summary: Option<BackupSummary>,
}
pub struct RecoveryDraft {
    intake: BackupIntake,
    preview: ImportPreview,
    expected: ProjectSnapshot,
}
impl RecoveryDraft {
    pub fn read(path: &Path, operation: &str) -> Result<Self> {
        let bytes = SelectedFile::new(path)?.read(MAX_BACKUP_BYTES)?;
        let intake = inspect_backup(&bytes)?;
        let (summary, expected, requires_upgrade) = match &intake {
            BackupIntake::Current(backup) => (backup.summary()?, backup.snapshot().clone(), false),
            BackupIntake::UpgradeRequired(legacy) => {
                // Intake already verified the entire canonical envelope and
                // intention reachability. Reuse the existing read-only upgrade
                // decoder for preview; do not introduce a second migration.
                let value: serde_json::Value = serde_json::from_slice(legacy.original_bytes())?;
                let upgraded =
                    LegacyUpgrade::decode(&canonical_bytes(&value["payload"]["snapshot"])?)?;
                let snapshot = upgraded.snapshot;
                let intentions: IntentionBundle =
                    serde_json::from_value(value["payload"]["intentions"].clone())?;
                let summary = BackupSummary {
                    digest: serde_json::from_value(value["digest"].clone())?,
                    identity: ToolIdentity::from_snapshot(&snapshot)?,
                    label: snapshot.program()?.program.label.clone(),
                    revision: snapshot.revision,
                    records: snapshot.data.records.len(),
                    events: snapshot.data.events.len(),
                    outputs: snapshot.artifacts.len(),
                    decisions: snapshot.decisions.decisions.len(),
                    intention_objects: intentions.object_count(),
                };
                (summary, snapshot, true)
            }
        };
        let preview = ImportPreview {
            operation: operation.into(),
            source: path.display().to_string(),
            backup: summary.digest.clone(),
            requires_upgrade,
            summary: Some(summary),
        };
        Ok(Self {
            intake,
            preview,
            expected,
        })
    }
    fn checkpoint(backup: VerifiedBackup, operation: &str) -> Result<Self> {
        let summary = backup.summary()?;
        let expected = backup.snapshot().clone();
        Ok(Self {
            preview: ImportPreview {
                operation: operation.into(),
                source: "Verified automatic backup".into(),
                backup: summary.digest.clone(),
                requires_upgrade: false,
                summary: Some(summary),
            },
            intake: BackupIntake::Current(backup),
            expected,
        })
    }
    pub fn preview(&self) -> &ImportPreview {
        &self.preview
    }
    /// Call only after explicit fresh-copy confirmation and the shared host's
    /// final Gate. Cancellation before creation refuses; once creation starts,
    /// report its actual result even if the page is subsequently closed.
    #[allow(clippy::too_many_arguments)]
    pub fn recover(
        self,
        choice: DestinationChoice,
        active_operation: &str,
        cancelled: &AtomicBool,
        recent: &RecentTools,
        locations: &ToolLocations,
        opened_unix_ms: u64,
        progress: impl FnMut(UpgradeProgress),
    ) -> RecoveryOutcome {
        self.recover_at(
            choice,
            active_operation,
            cancelled,
            recent,
            locations,
            opened_unix_ms,
            progress,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recover_selected(
        self,
        selected: &RecoveryDestination,
        choice: DestinationChoice,
        active_operation: &str,
        cancelled: &AtomicBool,
        recent: &RecentTools,
        locations: &ToolLocations,
        opened_unix_ms: u64,
        progress: impl FnMut(UpgradeProgress),
    ) -> RecoveryOutcome {
        self.recover_at(
            choice,
            active_operation,
            cancelled,
            recent,
            locations,
            opened_unix_ms,
            progress,
            Some(selected),
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn recover_at(
        self,
        choice: DestinationChoice,
        active_operation: &str,
        cancelled: &AtomicBool,
        recent: &RecentTools,
        locations: &ToolLocations,
        opened_unix_ms: u64,
        mut progress: impl FnMut(UpgradeProgress),
        selected: Option<&RecoveryDestination>,
    ) -> RecoveryOutcome {
        let path = match choice.resolve(&self.preview.operation, active_operation, cancelled) {
            Ok(path) => path,
            Err(error) => return RecoveryOutcome::NotAttempted(error),
        };
        if cancelled.load(Ordering::Acquire) {
            return RecoveryOutcome::NotAttempted("Recovery cancelled before writing.".into());
        }
        if let Some(selected) = selected {
            if selected.path() != path {
                return RecoveryOutcome::NotAttempted(
                    "The selected recovery destination changed.".into(),
                );
            }
            if let Err(error) = selected.check() {
                return RecoveryOutcome::NotAttempted(error.to_string());
            }
        }
        let created = match (&self.intake, selected) {
            (BackupIntake::Current(backup), None) => {
                backup.recover_tool(&path, recent, opened_unix_ms)
            }
            (BackupIntake::Current(backup), Some(selected)) => {
                backup.recover_tool_selected(selected, recent, opened_unix_ms)
            }
            (BackupIntake::UpgradeRequired(legacy), None) => {
                legacy.upgrade_recover_tool(&path, recent, opened_unix_ms, &mut progress)
            }
            (BackupIntake::UpgradeRequired(legacy), Some(selected)) => legacy
                .upgrade_recover_tool_selected(selected, recent, opened_unix_ms, &mut progress),
        };
        match created {
            Err(issue) => RecoveryOutcome::AttemptFailed {
                destination: path,
                issue,
            },
            Ok(created) => finish_recovery(path, created, &self.expected, locations),
        }
    }
}

pub enum CheckpointOutcome {
    Verified(CheckpointReceipt),
    Warning(OperationIssue),
    /// No fresh instance ID was confirmed, so do not borrow another shelf.
    RegistrationRequired,
}
pub struct RecoveredTool {
    pub destination: PathBuf,
    pub opened: OpenedTool,
    pub registration: Result<Digest>,
    pub checkpoint: CheckpointOutcome,
}
pub enum RecoveryOutcome {
    NotAttempted(String),
    /// The helper may have staged or activated bytes before returning an error.
    /// Keep the target; no automatic retry, cleanup or assertion of no file.
    AttemptFailed {
        destination: PathBuf,
        issue: OperationIssue,
    },
    SavedButUnavailable {
        destination: PathBuf,
        registration: Result<Digest>,
        issue: OperationIssue,
    },
    Recovered(Box<RecoveredTool>),
}
fn finish_recovery(
    path: PathBuf,
    created: CreatedTool,
    expected: &ProjectSnapshot,
    locations: &ToolLocations,
) -> RecoveryOutcome {
    let identity = ToolIdentity::from_snapshot(expected);
    let verified = identity.and_then(|identity| {
        let opened = open_verified(&path, Some(&identity))?;
        if opened.snapshot != *expected || created.store.load()? != *expected {
            return Err(OperationIssue::new(
                IssueKind::Collision,
                "recovered work changed before verification",
            ));
        }
        Ok(opened)
    });
    match verified {
        Err(issue) => RecoveryOutcome::SavedButUnavailable {
            destination: path,
            registration: created.registration,
            issue,
        },
        Ok(mut opened) => {
            let checkpoint = checkpoint_after(&mut opened, locations, &created.registration);
            RecoveryOutcome::Recovered(Box::new(RecoveredTool {
                destination: path,
                opened,
                registration: created.registration,
                checkpoint,
            }))
        }
    }
}

// Deterministic synthetic race injection after the real creation helper has
// returned and released its lock, before the shared reopen/install boundary.
#[cfg(test)]
pub fn test_finish_created_recovery(
    path: PathBuf,
    created: CreatedTool,
    expected: &ProjectSnapshot,
    locations: &ToolLocations,
) -> RecoveryOutcome {
    finish_recovery(path, created, expected, locations)
}

/// Before a risky host transition, a failure blocks that transition. This does
/// not commit the transition or replace the host's final freshness/Gate checks.
pub fn checkpoint_before(
    store: &ProductStore,
    snapshot: &ProjectSnapshot,
    shelf: &CheckpointShelf,
) -> Result<CheckpointReceipt> {
    fresh(store, snapshot).map_err(|e| OperationIssue::new(IssueKind::Collision, e))?;
    let receipt = shelf.capture(store)?;
    fresh(store, snapshot).map_err(|e| OperationIssue::new(IssueKind::Collision, e))?;
    Ok(receipt)
}
/// After a committed save, a failure is a warning. Reuse the one-use verified
/// open handoff and the actual independent recent-instance checkpoint shelf.
pub fn checkpoint_after(
    opened: &mut OpenedTool,
    locations: &ToolLocations,
    registration: &Result<Digest>,
) -> CheckpointOutcome {
    let Ok(instance) = registration else {
        return CheckpointOutcome::RegistrationRequired;
    };
    match CheckpointShelf::for_tool(locations, &opened.summary.identity, instance)
        .and_then(|shelf| opened.checkpoint(&shelf))
    {
        Ok(receipt) => CheckpointOutcome::Verified(receipt),
        Err(issue) => CheckpointOutcome::Warning(issue),
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosisView {
    pub current: Option<BackupSummary>,
    pub current_issue: Option<OperationIssue>,
    pub recovery: Option<ImportPreview>,
    pub checkpoint_issues: Vec<OperationIssue>,
}
pub struct Diagnosis {
    pub view: DiagnosisView,
    pub recovery: Option<RecoveryDraft>,
}
pub fn diagnose(
    path: &Path,
    expected: &ToolIdentity,
    shelf: &CheckpointShelf,
    operation: &str,
) -> Diagnosis {
    let report = doctor(path, expected, shelf);
    let mut issues = report.checkpoint_issues;
    let recovery =
        report.recovery.and_then(
            |backup| match RecoveryDraft::checkpoint(backup, operation) {
                Ok(draft) => Some(draft),
                Err(issue) => {
                    issues.push(issue);
                    None
                }
            },
        );
    Diagnosis {
        view: DiagnosisView {
            current: report.current,
            current_issue: report.issue,
            recovery: recovery.as_ref().map(|r| r.preview().clone()),
            checkpoint_issues: issues,
        },
        recovery,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStatus {
    NotAttempted,
    Unconfirmed,
    SavedUnavailable,
    Ready,
}
#[derive(Clone, Debug)]
pub struct RecoveryReport {
    pub status: RecoveryStatus,
    pub message: String,
    pub destination: Option<PathBuf>,
    pub issues: Vec<OperationIssue>,
}
pub fn recovery_outcome_report(outcome: &RecoveryOutcome) -> RecoveryReport {
    match outcome {
        RecoveryOutcome::NotAttempted(message) => RecoveryReport {
            status: RecoveryStatus::NotAttempted, message: message.clone(), destination: None, issues: vec![],
        },
        RecoveryOutcome::AttemptFailed { destination, issue } => RecoveryReport {
            status: RecoveryStatus::Unconfirmed,
            message: "Recovery could not be confirmed. The selected folder may contain saved or staged work. Keep it and the original backup. Do not repeat recovery into the same folder.".into(),
            destination: Some(destination.clone()), issues: vec![issue.clone()],
        },
        RecoveryOutcome::SavedButUnavailable { destination, registration, issue } => {
            let mut issues = vec![issue.clone()];
            if let Err(issue) = registration { issues.push(issue.clone()); }
            RecoveryReport {
                status: RecoveryStatus::SavedUnavailable,
                message: "A separate copy was saved, but it could not be reopened and verified. Keep the selected folder and the original backup. Do not repeat recovery into the same folder.".into(),
                destination: Some(destination.clone()), issues,
            }
        },
        RecoveryOutcome::Recovered(result) => {
            let mut issues = vec![];
            if let Err(issue) = &result.registration { issues.push(issue.clone()); }
            if let CheckpointOutcome::Warning(issue) = &result.checkpoint { issues.push(issue.clone()); }
            RecoveryReport { status: RecoveryStatus::Ready, message: recovery_report(result), destination: Some(result.destination.clone()), issues }
        },
    }
}
pub fn show_recovery_report(ui: &mut egui::Ui, report: &RecoveryReport) -> Option<Event> {
    ui.label(&report.message);
    if let Some(path) = &report.destination {
        ui.label(path.display().to_string());
    }
    // Generic issue retry advice is intentionally not used after a write.
    ui.button("Return to work").clicked().then_some(Event::Back)
}

pub fn recovery_report(result: &RecoveredTool) -> String {
    let mut message = "The separate recovered tool was reopened and its saved work verified. The original files were kept.".to_string();
    if result.registration.is_err() {
        message.push_str(" Its recent-tool entry could not be confirmed; reopen the selected folder to find it. Do not repeat recovery.");
    }
    if matches!(result.checkpoint, CheckpointOutcome::Warning(_)) {
        message.push_str(" Its automatic backup needs attention. Your recovered work is saved; do not repeat recovery.");
    }
    message
}
fn show_summary(ui: &mut egui::Ui, summary: &BackupSummary) {
    ui.strong(&summary.label);
    ui.label(format!(
        "{} records · {} saved actions · {} outputs · {} decisions",
        summary.records, summary.events, summary.outputs, summary.decisions
    ));
}
pub fn show_import(ui: &mut egui::Ui, preview: &ImportPreview, interactive: bool) -> Option<Event> {
    ui.heading("Recover a separate copy");
    ui.label(&preview.source);
    if let Some(summary) = &preview.summary {
        show_summary(ui, summary);
    }
    ui.label("Choose a new folder for this backup's saved work. Your current tool and the original backup stay intact. Later work outside this backup is not included.");
    if preview.requires_upgrade {
        ui.label("This older backup needs a verified local upgrade. The separate copy will be reopened before use; the original backup stays unchanged.");
    }
    let mut event = None;
    ui.add_enabled_ui(interactive, |ui| {
        if ui.button("Choose a new folder and recover…").clicked() {
            event = Some(Event::Recover {
                operation: preview.operation.clone(),
                backup: preview.backup.clone(),
            });
        }
        if ui.button("Cancel").clicked() {
            event = Some(Event::Back);
        }
    });
    event
}
pub fn show_diagnosis(ui: &mut egui::Ui, view: &DiagnosisView, interactive: bool) -> Option<Event> {
    ui.heading("Check saved work");
    if let Some(summary) = &view.current {
        ui.label("Saved work verified and ready to open.");
        show_summary(ui, summary);
    }
    if let Some(issue) = &view.current_issue {
        ui.label(&issue.message);
        ui.label("Keep the original files. Recovery creates a separate tool; it does not rewind this one.");
    }
    for issue in &view.checkpoint_issues {
        ui.label(format!("Backup check: {}", issue.message));
    }
    if let Some(preview) = &view.recovery {
        return show_import(ui, preview, interactive);
    }
    if view.current_issue.is_some() {
        ui.label("No verified automatic backup was found. You can choose another backup file to inspect.");
    }
    let mut event = None;
    ui.add_enabled_ui(interactive, |ui| {
        if ui.button("Open a backup file").clicked() {
            event = Some(Event::ChooseImport);
        }
        if ui.button("Return to work").clicked() {
            event = Some(Event::Back);
        }
    });
    event
}
