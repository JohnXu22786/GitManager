//! Presentation and dialog results for the single Studio worker.
use super::*;
use local_files_flow::{DiagnosisView, ImportPreview, Inventory};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Origin {
    pub tool: Association,
    pub basis: Basis,
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct RecoveredSelection {
    pub tool: Association,
    pub snapshot: Digest,
    pub pending: Option<Digest>,
}
#[derive(Clone)]
pub(super) enum View {
    Inventory {
        origin: Origin,
        inventory: Inventory,
    },
    Import {
        origin: Option<Origin>,
        preview: ImportPreview,
        destination: PathBuf,
    },
    Diagnosis {
        origin: Option<Origin>,
        view: DiagnosisView,
    },
    Report {
        origin: Option<Origin>,
        message: String,
        destination: Option<PathBuf>,
        recovered: Option<RecoveredSelection>,
    },
}
impl View {
    pub fn origin(&self) -> Option<&Origin> {
        match self {
            Self::Inventory { origin, .. } => Some(origin),
            Self::Import { origin, .. }
            | Self::Diagnosis { origin, .. }
            | Self::Report { origin, .. } => origin.as_ref(),
        }
    }
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        trace: &mut WidgetTrace,
        interactive: bool,
        can_write: bool,
    ) -> Option<Action> {
        let mut action = None;
        match self {
            Self::Inventory { inventory, .. } => {
                trace.label(ui, "Saved files and backups");
                trace.label(ui, "These outputs came from saved work. Saving a file does not run its action again.");
                if trace.button(
                    ui,
                    "studio.backup",
                    "Save a checked backup copy…",
                    interactive && can_write,
                ) {
                    action = Some(Action::SaveBackup {
                        basis: inventory.basis.clone(),
                    });
                }
                if trace.button(ui, "studio.import", "Open a backup file…", interactive) {
                    action = Some(Action::ChooseImport);
                }
                if trace.button(
                    ui,
                    "studio.diagnose",
                    "Check saved work and automatic backups",
                    interactive,
                ) {
                    action = Some(Action::Diagnose { recent: None });
                }
                if inventory.rows.is_empty() {
                    trace.label(
                        ui,
                        "No saved outputs yet. Use an output action in your tool first.",
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(500.0)
                    .show(ui, |ui| {
                        for row in &inventory.rows {
                            ui.separator();
                            trace.label(
                                ui,
                                format!(
                                    "{} · {} rows · {} bytes · {:?}",
                                    row.output_label, row.rows, row.bytes, row.format
                                ),
                            );
                            trace.label(
                                ui,
                                format!(
                                    "{} · {} · event {} · output {} · day {}",
                                    row.program_label,
                                    row.action_label,
                                    row.event_sequence,
                                    row.output_ordinal + 1,
                                    row.produced_day
                                ),
                            );
                            if trace.button(
                                ui,
                                &format!("studio.export.{}", row.selection.inventory_index),
                                "Save this exact output…",
                                interactive && can_write,
                            ) {
                                action = Some(Action::Export {
                                    basis: inventory.basis.clone(),
                                    selection: row.selection.clone(),
                                });
                            }
                        }
                    });
            }
            Self::Import {
                preview,
                destination,
                ..
            } => {
                trace.label(ui, "Recover a separate copy");
                trace.label(ui, preview.source.clone());
                if let Some(summary) = &preview.summary {
                    trace.label(
                        ui,
                        format!(
                            "{} · {} records · {} saved actions · {} outputs · {} decisions",
                            summary.label,
                            summary.records,
                            summary.events,
                            summary.outputs,
                            summary.decisions
                        ),
                    );
                }
                trace.label(ui, "Your current tool and the original backup stay intact. Later work outside this backup is not included. This does not undo later actions or restore old behavior on current data.");
                if preview.requires_upgrade {
                    trace.label(ui, "This older backup needs a verified local upgrade. The separate copy will be reopened before use; the original backup stays unchanged.");
                }
                trace.label(ui, format!("New separate tool: {}", destination.display()));
                if trace.button(
                    ui,
                    "studio.recovery-location",
                    "Choose a different parent folder…",
                    interactive,
                ) {
                    action = Some(Action::RecoveryLocation {
                        operation: preview.operation.clone(),
                    });
                }
                if trace.button(
                    ui,
                    "studio.recover",
                    "Recover and verify this separate copy",
                    interactive && can_write,
                ) {
                    action = Some(Action::RecoverFile {
                        operation: preview.operation.clone(),
                        backup: preview.backup.clone(),
                        destination: destination.clone(),
                    });
                }
            }
            Self::Diagnosis { view, .. } => {
                trace.label(ui, "Check saved work");
                if let Some(summary) = &view.current {
                    trace.label(
                        ui,
                        format!(
                            "Saved work verified: {} · {} records · {} saved actions",
                            summary.label, summary.records, summary.events
                        ),
                    );
                }
                if let Some(issue) = &view.current_issue {
                    trace.label(ui, &issue.message);
                    trace.label(ui, &issue.next_step);
                }
                for issue in &view.checkpoint_issues {
                    trace.label(ui, format!("Backup check: {}", issue.message));
                }
                if view.recovery.is_some() {
                    trace.label(ui, "A verified automatic backup is available. Your original files will be kept.");
                    if trace.button(
                        ui,
                        "studio.review-recovery",
                        "Review a separate recovery copy",
                        interactive,
                    ) {
                        action = Some(Action::ReviewRecovery);
                    }
                } else if view.current_issue.is_some() {
                    trace.label(ui, "No verified automatic backup was found. Keep the original files and choose a backup file to inspect.");
                }
                if trace.button(ui, "studio.import", "Open a backup file…", interactive) {
                    action = Some(Action::ChooseImport);
                }
            }
            Self::Report {
                message,
                destination,
                recovered,
                origin,
            } => {
                trace.label(ui, message);
                if let Some(path) = destination {
                    trace.label(ui, path.display().to_string());
                }
                if let Some(tool) = recovered {
                    trace.label(ui, if origin.is_some() { "The original tool remains selected. Open this separate copy only if you want to work there." } else { "The original files were kept. Open this separate copy only if you want to work there." });
                    if tool.pending.is_some() {
                        trace.label(ui, "Opening this copy keeps the complete original interrupted attempt for review and starts separate work. It does not confirm or repeat the original operation; later work outside the backup is not included.");
                    }
                    if trace.button(
                        ui,
                        "studio.open-recovered",
                        if tool.pending.is_some() {
                            "Keep the original attempt and open this verified separate copy"
                        } else {
                            "Open the verified separate copy"
                        },
                        interactive,
                    ) {
                        action = Some(Action::OpenRecovered {
                            selection: tool.clone(),
                        });
                    }
                }
            }
        }
        if trace.button(ui, "studio.files-back", "Return to work", interactive) {
            action = Some(Action::FilesBack);
        }
        if trace.button(
            ui,
            "studio.files-home",
            "Close this page and return home",
            interactive,
        ) {
            action = Some(Action::Close);
        }
        action
    }
}

/// rfd's native Option does not distinguish a closed dialog from backend failure.
/// Only a backend with an explicit cancellation result may produce Cancelled.
#[derive(Clone, Debug)]
pub enum DialogResult {
    Selected(PathBuf),
    Cancelled,
    Unavailable,
    Error(String),
    Indeterminate,
}
impl DialogResult {
    pub(super) fn selected(self) -> Result<PathBuf, String> {
        match self {
            Self::Selected(path) => Ok(path),
            Self::Cancelled => Err("File selection cancelled. No file action was started.".into()),
            Self::Unavailable => Err("The file dialog is unavailable. Your saved work and previous destination were kept.".into()),
            Self::Error(message) => Err(format!("The file dialog failed: {message}. Your saved work and previous destination were kept.")),
            Self::Indeterminate => Err("No file was selected. The dialog could not confirm whether it was closed or failed. No file action was started.".into()),
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum DialogKind {
    Folder,
    OpenBackup,
    SaveOutput(OutputFormat),
    SaveBackup,
}

#[cfg(test)]
impl ProductStudio {
    pub fn test_files(&mut self) {
        self.issue(Action::Files);
    }
    pub fn test_export(&mut self, index: usize) {
        if let Page::LocalFiles(View::Inventory { inventory, .. }) = &self.page {
            if let Some(row) = inventory.rows.get(index) {
                self.issue(Action::Export {
                    basis: inventory.basis.clone(),
                    selection: row.selection.clone(),
                });
            }
        }
    }
    pub fn test_recovery_target(&self) -> Option<&Path> {
        if let Page::LocalFiles(View::Import { destination, .. }) = &self.page {
            Some(destination)
        } else {
            None
        }
    }
    pub fn test_active_tool(&self) -> Option<&Path> {
        match &self.page {
            Page::LocalFiles(view) => view.origin().map(|o| o.tool.path.as_path()),
            Page::Daily { tool, .. } => Some(&tool.path),
            _ => None,
        }
    }
    pub fn test_pending_operation(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.key.operation.as_str())
    }
    pub fn test_recent_ids(&self) -> Vec<String> {
        self.recent
            .iter()
            .map(|r| r.tool.id.as_str().to_string())
            .collect()
    }
}

/// Synthetic host-journal fixtures; never constructs persisted project stores.
#[cfg(test)]
pub fn test_seed_recovery_history(root: &Path, count: usize, near_limit: bool) {
    let mut journal = JournalFile::open(root).unwrap();
    let mut value = journal.value.clone();
    let original = value.pending.clone().unwrap();
    let tool = match &original {
        Interrupted::Create { tool }
        | Interrupted::Daily { tool, .. }
        | Interrupted::Change { tool, .. } => tool.clone(),
    };
    value.recovery_handoffs = (0..count)
        .map(|i| journal::RecoveryHandoff {
            operation: format!("previous-recovery-{i}"),
            original: original.clone(),
            recovered: tool.clone(),
        })
        .collect();
    if near_limit {
        let Interrupted::Daily { input, .. } = value.pending.as_mut().unwrap() else {
            panic!("daily fixture required");
        };
        *input = SemanticInput::Control {
            view: "people".into(),
            control: "search_input".into(),
            value: DataValue::List {
                item_type: Type::Text,
                items: vec![],
            },
        };
        let target = MAX_WIRE_BYTES - 32;
        loop {
            let size = canonical_bytes(&value).unwrap().len();
            if size >= target {
                break;
            }
            let Interrupted::Daily {
                input:
                    SemanticInput::Control {
                        value: DataValue::List { items, .. },
                        ..
                    },
                ..
            } = value.pending.as_mut().unwrap()
            else {
                unreachable!()
            };
            items.push(DataValue::Text {
                value: String::new(),
            });
            let empty_size = canonical_bytes(&value).unwrap().len();
            let Interrupted::Daily {
                input:
                    SemanticInput::Control {
                        value: DataValue::List { items, .. },
                        ..
                    },
                ..
            } = value.pending.as_mut().unwrap()
            else {
                unreachable!()
            };
            if empty_size > target {
                items.pop();
                break;
            }
            let DataValue::Text { value: text } = items.last_mut().unwrap() else {
                unreachable!()
            };
            *text = "x".repeat((target - empty_size).min(MAX_TEXT_BYTES));
        }
    }
    journal.write(value).unwrap();
}
