//! Saved-output, backup and recovery presentation for the single studio host.
//!
//! The host owns dialogs, session/epoch/day/path admission, the final Gate,
//! journal and worker queue. These one-use drafts consume a deliberate dialog
//! result; they are not another scheduler or durable authority. Register and
//! route this module in the host before claiming a reachable user workflow.
use crate::product_backup::{BackupSummary, VerifiedBackup};
use crate::product_contract::{canonical_digest, Digest, Id, IdentityDomain, OutputFormat};
use crate::product_export::{ArtifactSelection, ExportError, ExportOutcome, PreparedExport};
use crate::product_locations::OperationIssue;
use crate::product_store::{ProductStore, ProjectSnapshot};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

#[path = "local_files_flow/recovery.rs"]
mod recovery;
pub use recovery::*;

type Result<T> = std::result::Result<T, String>;

/// A reply to one host-owned dialog, not authorization for a different action.
#[derive(Clone, Debug)]
pub struct DestinationChoice {
    pub operation: Id,
    pub destination: Option<PathBuf>,
}
impl DestinationChoice {
    fn resolve(self, prepared: &str, active: &str, cancelled: &AtomicBool) -> Result<PathBuf> {
        if cancelled.load(Ordering::Acquire) || self.operation != prepared || active != prepared {
            return Err("This file action was cancelled or its page changed. Nothing was written by this action.".into());
        }
        self.destination
            .ok_or_else(|| "File selection cancelled. Nothing was written by this action.".into())
    }
}

#[derive(Clone, Debug)]
pub struct OutputRow {
    pub selection: ArtifactSelection,
    pub event_id: Id,
    pub event_sequence: u64,
    pub operation_id: Id,
    pub action_label: String,
    pub output_ordinal: usize,
    pub produced_day: i32,
    pub producing_program: Digest,
    pub program_label: String,
    pub output_label: String,
    pub format: OutputFormat,
    pub rows: usize,
    pub bytes: usize,
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub basis: Digest,
    pub rows: Vec<OutputRow>,
}

fn fresh(store: &ProductStore, snapshot: &ProjectSnapshot) -> Result<()> {
    if store.load().map_err(|e| e.to_string())? != *snapshot {
        return Err("Saved work changed. Refresh this page before choosing a file action.".into());
    }
    Ok(())
}

/// Only a complete freshly loaded committed snapshot can supply this inventory.
/// Labels come from the original producing revision, not today's definitions.
pub fn inventory(store: &ProductStore, snapshot: &ProjectSnapshot) -> Result<Inventory> {
    fresh(store, snapshot)?;
    let sources = snapshot
        .programs
        .iter()
        .map(|source| (source.artifact.program_digest.clone(), &source.program))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::with_capacity(snapshot.artifacts.len());
    for event in &snapshot.data.events {
        for (ordinal, digest) in event.outputs.iter().enumerate() {
            let index = rows.len();
            let artifact = snapshot
                .artifacts
                .get(index)
                .filter(|a| &a.digest == digest)
                .ok_or("The saved output occurrence could not be verified.")?;
            let source = sources
                .get(&event.program)
                .ok_or("The producing tool revision is unavailable.")?;
            let output = source
                .outputs
                .iter()
                .find(|o| o.id == artifact.output)
                .ok_or("The original output definition is unavailable.")?;
            let action = source
                .actions
                .iter()
                .find(|action| action.id == event.action)
                .ok_or("The original action definition is unavailable.")?;
            rows.push(OutputRow {
                selection: ArtifactSelection {
                    inventory_index: index,
                    expected_digest: digest.clone(),
                },
                event_id: event.id.clone(),
                event_sequence: event.sequence,
                operation_id: event.operation_id.clone(),
                action_label: action.label.clone(),
                output_ordinal: ordinal,
                produced_day: event.day,
                producing_program: event.program.clone(),
                program_label: source.label.clone(),
                output_label: output.label.clone(),
                format: artifact.format,
                rows: artifact.rows.len(),
                bytes: artifact.bytes.len(),
            });
        }
    }
    if rows.len() != snapshot.artifacts.len() {
        return Err("Some saved outputs have no verified producing event.".into());
    }
    Ok(Inventory {
        basis: canonical_digest(IdentityDomain::Evidence, snapshot).map_err(|e| e.to_string())?,
        rows,
    })
}

pub struct ExportDraft {
    operation: Id,
    prepared: PreparedExport,
}
impl ExportDraft {
    pub fn prepare(
        store: &ProductStore,
        snapshot: &ProjectSnapshot,
        selection: ArtifactSelection,
        operation: &str,
    ) -> Result<Self> {
        let basis =
            canonical_digest(IdentityDomain::Evidence, snapshot).map_err(|e| e.to_string())?;
        let prepared =
            PreparedExport::capture(store, &basis, selection).map_err(|e| e.to_string())?;
        Ok(Self {
            operation: operation.into(),
            prepared,
        })
    }
    pub fn summary(&self) -> &crate::product_export::ExportSummary {
        self.prepared.summary()
    }
    /// Consumes the only preparation even on cancellation or uncertain publication.
    /// The shared host must admit this operation through its final Gate first.
    pub fn publish(
        self,
        choice: DestinationChoice,
        active_operation: &str,
        cancelled: &AtomicBool,
    ) -> ExportOutcome {
        let path = match choice.resolve(&self.operation, active_operation, cancelled) {
            Ok(path) => path,
            Err(_) => return ExportOutcome::NotAttempted(ExportError::Cancelled),
        };
        match self.prepared.select_destination(&path) {
            Ok(selected) => selected.publish(cancelled),
            Err(error) => ExportOutcome::NotAttempted(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationStatus {
    NotAttempted,
    Uncertain,
    PublishedUnverified,
    Verified,
}
#[derive(Clone, Debug)]
pub struct FileReport {
    pub status: PublicationStatus,
    pub message: String,
    pub destination: Option<PathBuf>,
    /// Diagnostic evidence; its generic retry advice must not override the
    /// publication-specific message when a file may already exist.
    pub issue: Option<OperationIssue>,
}
pub fn export_report(outcome: &ExportOutcome) -> FileReport {
    let (status, message, destination, issue) = match outcome {
        ExportOutcome::NotAttempted(error) => (PublicationStatus::NotAttempted, error.to_string(), None, None),
        ExportOutcome::PublicationUncertain { receipt, issue } => (
            PublicationStatus::Uncertain,
            "A file may have been saved. Keep any file at the chosen location and inspect it. Do not repeat this export automatically.".into(),
            Some(receipt.destination.clone()), Some(issue.clone())),
        ExportOutcome::PublishedUnverified { receipt, issue } => (
            PublicationStatus::PublishedUnverified,
            "The file was saved, but its contents could not be verified. Keep it for inspection. Do not repeat this export automatically.".into(),
            Some(receipt.destination.clone()), Some(issue.clone())),
        ExportOutcome::Verified(receipt) => (
            PublicationStatus::Verified, "Saved and checked the exact original output.".into(),
            Some(receipt.destination.clone()), None),
    };
    FileReport {
        status,
        message,
        destination,
        issue,
    }
}

pub struct BackupDraft {
    operation: Id,
    store: ProductStore,
    backup: VerifiedBackup,
    summary: BackupSummary,
}
pub enum BackupOutcome {
    NotAttempted(String),
    Uncertain {
        destination: PathBuf,
        summary: BackupSummary,
        issue: OperationIssue,
    },
    Verified {
        destination: PathBuf,
        summary: BackupSummary,
    },
}
impl BackupDraft {
    pub fn prepare(
        store: &ProductStore,
        snapshot: &ProjectSnapshot,
        operation: &str,
    ) -> Result<Self> {
        let backup = VerifiedBackup::capture(store).map_err(|e| e.to_string())?;
        if backup.snapshot() != snapshot {
            return Err("Saved work changed. Prepare a fresh backup.".into());
        }
        let summary = backup.summary().map_err(|e| e.to_string())?;
        Ok(Self {
            operation: operation.into(),
            store: store.clone(),
            backup,
            summary,
        })
    }
    pub fn summary(&self) -> &BackupSummary {
        &self.summary
    }
    pub fn publish(
        self,
        choice: DestinationChoice,
        active_operation: &str,
        cancelled: &AtomicBool,
    ) -> BackupOutcome {
        let path = match choice.resolve(&self.operation, active_operation, cancelled) {
            Ok(path) => path,
            Err(error) => return BackupOutcome::NotAttempted(error),
        };
        if let Err(error) = fresh(&self.store, self.backup.snapshot()) {
            return BackupOutcome::NotAttempted(error);
        }
        if cancelled.load(Ordering::Acquire) {
            return BackupOutcome::NotAttempted("Backup cancelled before writing.".into());
        }
        // export_new can fail after writing. It cannot prove absence on error.
        match self.backup.export_new(&path) {
            Ok(summary) => BackupOutcome::Verified {
                destination: path,
                summary,
            },
            Err(issue) => BackupOutcome::Uncertain {
                destination: path,
                summary: self.summary,
                issue,
            },
        }
    }
}
pub fn backup_report(outcome: &BackupOutcome) -> FileReport {
    match outcome {
        BackupOutcome::NotAttempted(message) => FileReport { status: PublicationStatus::NotAttempted, message: message.clone(), destination: None, issue: None },
        BackupOutcome::Verified { destination, .. } => FileReport { status: PublicationStatus::Verified, message: "Backup saved and checked. It includes the saved work and its decisions.".into(), destination: Some(destination.clone()), issue: None },
        BackupOutcome::Uncertain { destination, issue, .. } => FileReport { status: PublicationStatus::Uncertain, message: "A backup may have been saved, but this attempt could not be confirmed. Keep any file at the chosen location. Do not repeat this backup automatically.".into(), destination: Some(destination.clone()), issue: Some(issue.clone()) },
    }
}

/// Local presentation events are wrapped by the host's existing Action/Key.
/// The runtime renderer's raw-digest event is deliberately not accepted here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Export {
        basis: Digest,
        selection: ArtifactSelection,
    },
    SaveBackup {
        basis: Digest,
    },
    ChooseImport,
    Diagnose,
    Recover {
        operation: Id,
        backup: Digest,
    },
    Back,
}

pub fn show_inventory(ui: &mut egui::Ui, view: &Inventory, interactive: bool) -> Option<Event> {
    let mut event = None;
    ui.heading("Saved files and backups");
    ui.label("These outputs came from saved work. Saving a file does not run its action again.");
    ui.add_enabled_ui(interactive, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Save a backup copy").clicked() {
                event = Some(Event::SaveBackup {
                    basis: view.basis.clone(),
                });
            }
            if ui.button("Open a backup file").clicked() {
                event = Some(Event::ChooseImport);
            }
            if ui.button("Check saved work").clicked() {
                event = Some(Event::Diagnose);
            }
        });
        if view.rows.is_empty() {
            ui.label("No saved outputs yet. Use an output action in your tool first.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in &view.rows {
                ui.push_id(row.selection.inventory_index, |ui| {
                    ui.separator();
                    ui.strong(&row.output_label);
                    let day =
                        chrono::DateTime::from_timestamp(i64::from(row.produced_day) * 86_400, 0)
                            .map(|date| date.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| format!("day {}", row.produced_day));
                    ui.label(format!(
                        "{} rows · {} bytes · {:?}",
                        row.rows, row.bytes, row.format
                    ));
                    ui.label(format!(
                        "Saved {day} by {} · action {} · event {} · output {}",
                        row.program_label,
                        row.action_label,
                        row.event_sequence,
                        row.output_ordinal + 1
                    ));
                    if ui.button("Save this output…").clicked() {
                        event = Some(Event::Export {
                            basis: view.basis.clone(),
                            selection: row.selection.clone(),
                        });
                    }
                });
            }
        });
        if ui.button("Return to work").clicked() {
            event = Some(Event::Back);
        }
    });
    event
}

pub fn show_file_report(ui: &mut egui::Ui, report: &FileReport) -> Option<Event> {
    ui.label(&report.message);
    if let Some(path) = &report.destination {
        ui.label(path.display().to_string());
    }
    // Never render generic OperationIssue.next_step here: it can advise retry
    // while this particular publication is already uncertain or complete.
    ui.button("Return to work").clicked().then_some(Event::Back)
}
