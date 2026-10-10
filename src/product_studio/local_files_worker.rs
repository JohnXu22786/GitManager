//! Checked local-file actions share the host's command queue and final Gate.
use super::*;
use crate::product_export::{ArtifactSelection, PreparedExport};
use crate::product_locations::SelectedFile;
use crate::product_store::RecoveryDestination;
use journal::{FileAttempt, FileTarget};
use local_files_flow::*;
use local_files_host::{DialogKind, DialogResult, Origin, RecoveredSelection, View};

pub(super) struct Recovery {
    origin: Option<Origin>,
    draft: RecoveryDraft,
    destination: PathBuf,
    selected: RecoveryDestination,
    session: String,
    epoch: u64,
}
impl Worker {
    fn local_origin(&self) -> Result<Option<Origin>, String> {
        self.opened
            .as_ref()
            .map(|o| {
                Ok(Origin {
                    tool: o.association.clone(),
                    basis: Basis::capture(&o.snapshot)?,
                })
            })
            .transpose()
    }
    fn check_local_origin(&self, origin: Option<&Origin>, key: &Key) -> Result<(), String> {
        if self.local_origin()?.as_ref() != origin || key.basis.as_ref() != origin.map(|o| &o.basis)
        {
            return Err(
                "The selected tool or file page changed. Start a fresh file action.".into(),
            );
        }
        Ok(())
    }
    fn no_file_attempt(&self) -> Result<(), String> {
        if self
            .journal
            .as_ref()
            .ok_or("Restart information is unavailable")?
            .value
            .local_file
            .is_some()
        {
            return Err("Review the earlier file attempt and keep its files before starting another. Saved tool work is still available.".into());
        }
        Ok(())
    }
    pub(super) fn files(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        if !matches!(self.page, Page::Daily { .. }) {
            return Err("Return to saved work before opening its files".into());
        }
        let opened = self.current(key)?;
        let view = inventory(&opened.store, &opened.snapshot)?;
        let origin = self.local_origin()?.ok_or("No saved tool is open")?;
        if !gate.finish() {
            return Err("Opening saved files was cancelled".into());
        }
        self.recovery = None;
        self.recovered_location = None;
        self.page = Page::LocalFiles(View::Inventory {
            origin,
            inventory: view,
        });
        Ok(())
    }
    fn inventory_origin(&self, basis: &Digest, key: &Key) -> Result<Origin, String> {
        let Page::LocalFiles(View::Inventory { origin, inventory }) = &self.page else {
            return Err("Refresh the saved file inventory first".into());
        };
        if &inventory.basis != basis || &origin.basis.snapshot != basis {
            return Err("The saved output inventory changed".into());
        }
        self.check_local_origin(Some(origin), key)?;
        self.current(key)?;
        Ok(origin.clone())
    }
    fn stage_file(&mut self, attempt: FileAttempt, gate: &Gate) -> Result<(), String> {
        self.no_file_attempt()?;
        gate.check()?;
        self.journal(|j| j.local_file = Some(attempt))?;
        // Even a lost acknowledgement retains the exact target for inspection.
        #[cfg(test)]
        if self.config.hooks.interrupt_file == Some(false) {
            return Err("Synthetic interruption before external file effect; inspect the retained attempt. It was not repeated.".into());
        }
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.after_file_staged {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if let Err(e) = gate.commit() {
            self.journal(|j| j.local_file = None)?;
            return Err(e);
        }
        Ok(())
    }
    fn file_effect_finished(&mut self, definitive: bool) -> Result<(), String> {
        #[cfg(test)]
        if self.config.hooks.interrupt_file == Some(true) {
            return Err("Synthetic interruption after external file effect; acknowledgement was lost. Inspect the retained attempt; it was not repeated.".into());
        }
        if definitive {
            self.journal(|j| j.local_file = None)?;
        }
        Ok(())
    }
    fn before_file(&self, gate: &Gate) -> Result<(), String> {
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_file_effect {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        gate.check()
    }
    pub(super) fn export_file(
        &mut self,
        basis: &Digest,
        selection: &ArtifactSelection,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_file_attempt()?;
        let origin = self.inventory_origin(basis, key)?;
        let opened = self.opened.as_ref().unwrap();
        // Selection carries the occurrence index and digest, never a name lookup.
        let draft =
            PreparedExport::capture(&opened.store, basis, selection.clone()).map_err(error)?;
        let summary = draft.summary().clone();
        let destination = self
            .file_dialog(
                DialogKind::SaveOutput(summary.format),
                "Save this exact original output",
            )
            .selected()?;
        let selected = draft.select_destination(&destination).map_err(error)?;
        self.before_file(gate)?;
        self.inventory_origin(basis, key)?;
        self.stage_file(
            FileAttempt {
                operation: key.operation.clone(),
                tool: Some(origin.tool.clone()),
                basis: Some(origin.basis.clone()),
                destination: destination.clone(),
                target: FileTarget::Output {
                    inventory_index: selection.inventory_index,
                    artifact: selection.expected_digest.clone(),
                    bytes_digest: summary.bytes_digest.clone(),
                    byte_count: summary.byte_count,
                },
            },
            gate,
        )?;
        let result = selected.publish(&gate.cancelled);
        self.after_commit();
        let mut report = export_report(&result);
        if report.status == PublicationStatus::Verified {
            report.message.push_str(&format!(
                " Saved action {} · output {}.",
                summary.event_sequence,
                summary.event_output_ordinal + 1
            ));
        }
        let definitive = matches!(
            report.status,
            PublicationStatus::NotAttempted | PublicationStatus::Verified
        );
        self.file_effect_finished(definitive)?;
        self.notice = report.message.clone();
        self.page = Page::LocalFiles(View::Report {
            origin: Some(origin),
            message: report.message,
            destination: report.destination,
            recovered: None,
        });
        Ok(())
    }
    pub(super) fn backup_file(
        &mut self,
        basis: &Digest,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_file_attempt()?;
        let origin = self.inventory_origin(basis, key)?;
        let opened = self.opened.as_ref().unwrap();
        let draft = BackupDraft::prepare(&opened.store, &opened.snapshot, &key.operation)?;
        let digest = draft.summary().digest.clone();
        let destination = self
            .file_dialog(DialogKind::SaveBackup, "Save a checked backup copy")
            .selected()?;
        let selected = draft.select_destination(
            DestinationChoice {
                operation: key.operation.clone(),
                destination: Some(destination.clone()),
            },
            &key.operation,
            &gate.cancelled,
        )?;
        self.before_file(gate)?;
        self.inventory_origin(basis, key)?;
        self.stage_file(
            FileAttempt {
                operation: key.operation.clone(),
                tool: Some(origin.tool.clone()),
                basis: Some(origin.basis.clone()),
                destination: destination.clone(),
                target: FileTarget::Backup { digest },
            },
            gate,
        )?;
        let result = selected.publish(&gate.cancelled);
        self.after_commit();
        let report = backup_report(&result);
        self.file_effect_finished(matches!(
            report.status,
            PublicationStatus::Verified | PublicationStatus::NotAttempted
        ))?;
        self.notice = report.message.clone();
        self.page = Page::LocalFiles(View::Report {
            origin: Some(origin),
            message: report.message,
            destination: report.destination,
            recovered: None,
        });
        Ok(())
    }
    pub(super) fn choose_import(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        if !matches!(self.page, Page::Home | Page::LocalFiles(_)) {
            return Err("Return to saved files before importing a backup".into());
        }
        let origin = self.local_origin()?;
        self.check_local_origin(origin.as_ref(), key)?;
        let path = self
            .file_dialog(DialogKind::OpenBackup, "Open a generated-tool backup")
            .selected()?;
        gate.check()?;
        let draft = RecoveryDraft::read(&path, &key.operation).map_err(error)?;
        let destination = self
            .locations
            .as_ref()
            .ok_or("Default tool location unavailable")?
            .new_tool_path()
            .map_err(error)?;
        let selected = RecoveryDestination::select(&destination).map_err(error)?;
        self.check_local_origin(origin.as_ref(), key)?;
        if !gate.finish() {
            return Err("Backup inspection cancelled; no recovery was started".into());
        }
        self.recovered_location = None;
        self.page = Page::LocalFiles(View::Import {
            origin: origin.clone(),
            preview: draft.preview().clone(),
            destination: destination.clone(),
        });
        self.recovery = Some(Recovery {
            origin,
            draft,
            destination,
            selected,
            session: key.session.clone(),
            epoch: key.epoch,
        });
        Ok(())
    }
    pub(super) fn diagnose_file(
        &mut self,
        selected: Option<&Digest>,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        let origin = self.local_origin()?;
        self.check_local_origin(origin.as_ref(), key)?;
        let locations = self
            .locations
            .as_ref()
            .ok_or("Default tool location unavailable")?;
        let entries = RecentTools::open(locations.path())
            .and_then(|r| r.list())
            .map_err(error)?;
        let entry = entries.into_iter().find(|entry| match selected { Some(id) => &entry.tool.id == id, None => origin.as_ref().is_some_and(|o| o.tool.path == entry.tool.path && o.tool.identity == entry.tool.identity) }).ok_or("This tool has no verified recent instance. Open its original folder or inspect a backup file.")?;
        if selected.is_some() && !matches!(self.page, Page::Home) {
            return Err("Choose the recent tool from the home page".into());
        }
        let shelf = CheckpointShelf::for_tool(locations, &entry.tool.identity, &entry.tool.id)
            .map_err(error)?;
        let diagnosis = diagnose(
            &entry.tool.path,
            &entry.tool.identity,
            &shelf,
            &key.operation,
        );
        let destination = locations.new_tool_path().map_err(error)?;
        let selected = RecoveryDestination::select(&destination).map_err(error)?;
        if !gate.finish() {
            return Err("Saved-work check cancelled".into());
        }
        self.recovered_location = None;
        self.recovery = diagnosis.recovery.map(|draft| Recovery {
            origin: origin.clone(),
            draft,
            destination,
            selected,
            session: key.session.clone(),
            epoch: key.epoch,
        });
        self.page = Page::LocalFiles(View::Diagnosis {
            origin,
            view: diagnosis.view,
        });
        Ok(())
    }
    fn checked_recovery(&self, key: &Key) -> Result<&Recovery, String> {
        let recovery = self
            .recovery
            .as_ref()
            .ok_or("Inspect the backup again before recovering")?;
        if recovery.session != key.session || recovery.epoch != key.epoch {
            return Err("This recovery belongs to an earlier page".into());
        }
        self.check_local_origin(recovery.origin.as_ref(), key)?;
        Ok(recovery)
    }
    pub(super) fn review_recovery(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        if !matches!(self.page, Page::LocalFiles(View::Diagnosis { .. })) {
            return Err("Check saved work before reviewing its backup".into());
        }
        let recovery = self.checked_recovery(key)?;
        let page = Page::LocalFiles(View::Import {
            origin: recovery.origin.clone(),
            preview: recovery.draft.preview().clone(),
            destination: recovery.destination.clone(),
        });
        if !gate.finish() {
            return Err("Recovery review cancelled".into());
        }
        self.page = page;
        Ok(())
    }
    pub(super) fn recovery_location(
        &mut self,
        operation: &str,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.recovery_selection(operation, None, None, key)?;
        let parent = self
            .file_dialog(
                DialogKind::Folder,
                "Choose a parent folder for the separate recovered tool",
            )
            .selected()?;
        let destination = ToolLocations::chosen(parent)
            .and_then(|l| l.new_tool_path())
            .map_err(error)?;
        let selected = RecoveryDestination::select(&destination).map_err(error)?;
        self.recovery_selection(operation, None, None, key)?;
        if !gate.finish() {
            return Err("Recovery location selection cancelled; previous target was kept".into());
        }
        let recovery = self.recovery.as_mut().unwrap();
        recovery.destination = destination.clone();
        recovery.selected = selected;
        self.page = Page::LocalFiles(View::Import {
            origin: recovery.origin.clone(),
            preview: recovery.draft.preview().clone(),
            destination,
        });
        Ok(())
    }
    fn recovery_selection(
        &self,
        operation: &str,
        backup: Option<&Digest>,
        destination: Option<&Path>,
        key: &Key,
    ) -> Result<&Recovery, String> {
        let recovery = self.checked_recovery(key)?;
        let Page::LocalFiles(View::Import {
            preview,
            destination: shown,
            ..
        }) = &self.page
        else {
            return Err("Review this separate copy before recovering it".into());
        };
        if preview.operation != operation
            || recovery.draft.preview().operation != operation
            || recovery.draft.preview().backup != preview.backup
            || backup.is_some_and(|b| b != &preview.backup)
            || shown != &recovery.destination
            || recovery.selected.path() != shown
            || destination.is_some_and(|d| d != shown)
        {
            return Err("The recovery selection changed; inspect the backup again".into());
        }
        Ok(recovery)
    }
    pub(super) fn recover_file(
        &mut self,
        operation: &str,
        backup: &Digest,
        destination: &Path,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        self.no_file_attempt()?;
        self.recovery_selection(operation, Some(backup), Some(destination), key)?;
        self.before_file(gate)?;
        let recovery = self.recovery_selection(operation, Some(backup), Some(destination), key)?;
        let origin = recovery.origin.clone();
        let identity = recovery
            .draft
            .preview()
            .summary
            .as_ref()
            .ok_or("Missing checked backup identity")?
            .identity
            .clone();
        let recent = RecentTools::open(
            self.locations
                .as_ref()
                .ok_or("Default location unavailable")?
                .path(),
        )
        .map_err(error)?;
        self.stage_file(
            FileAttempt {
                operation: key.operation.clone(),
                tool: origin.as_ref().map(|o| o.tool.clone()),
                basis: origin.as_ref().map(|o| o.basis.clone()),
                destination: destination.into(),
                target: FileTarget::Recovery {
                    backup: backup.clone(),
                    identity: identity.clone(),
                },
            },
            gate,
        )?;
        let recovery = self.recovery.take().unwrap();
        let outcome = recovery.draft.recover_selected(
            &recovery.selected,
            DestinationChoice {
                operation: operation.into(),
                destination: Some(destination.into()),
            },
            operation,
            &gate.cancelled,
            &recent,
            self.locations.as_ref().unwrap(),
            unix_ms(),
            |stage| {
                gate.upgrade_stage.store(
                    match stage {
                        UpgradeProgress::Validating => 1,
                        UpgradeProgress::Staged => 2,
                        UpgradeProgress::Verified => 3,
                        UpgradeProgress::Activating => 4,
                        UpgradeProgress::RestartRequired => 5,
                    },
                    Ordering::Release,
                );
            },
        );
        self.after_commit();
        let report = recovery_outcome_report(&outcome);
        let recovered = match &outcome {
            RecoveryOutcome::Recovered(result) => Some(RecoveredSelection {
                tool: Association {
                    path: destination.into(),
                    identity,
                },
                snapshot: canonical_digest(IdentityDomain::Evidence, &result.opened.snapshot)
                    .map_err(error)?,
                pending: self
                    .journal
                    .as_ref()
                    .and_then(|j| j.value.pending.as_ref())
                    .map(|p| canonical_digest(IdentityDomain::Evidence, p).map_err(error))
                    .transpose()?,
            }),
            _ => None,
        };
        self.file_effect_finished(matches!(
            report.status,
            RecoveryStatus::Ready | RecoveryStatus::NotAttempted
        ))?;
        self.notice = report.message.clone();
        self.recovered_location = recovered.as_ref().map(|_| recovery.selected.clone());
        self.page = Page::LocalFiles(View::Report {
            origin,
            message: report.message,
            destination: report.destination,
            recovered,
        });
        Ok(())
    }
    pub(super) fn open_recovered(
        &mut self,
        selection: &RecoveredSelection,
        key: &Key,
        gate: &Gate,
    ) -> Result<(), String> {
        let Page::LocalFiles(View::Report {
            origin,
            recovered: Some(shown),
            ..
        }) = &self.page
        else {
            return Err("No verified separate copy is selected".into());
        };
        if selection != shown {
            return Err("The selected recovered tool changed".into());
        }
        self.check_local_origin(origin.as_ref(), key)?;
        let tool = &selection.tool;
        let selected_location = self
            .recovered_location
            .as_ref()
            .ok_or("The recovered location selection is unavailable; inspect the copy again")?;
        if selected_location.path() != tool.path {
            return Err("The recovered location selection changed".into());
        }
        selected_location.check().map_err(error)?;
        let pending = self
            .journal
            .as_ref()
            .ok_or("Restart information unavailable")?
            .value
            .pending
            .clone();
        if pending
            .as_ref()
            .map(|p| canonical_digest(IdentityDomain::Evidence, p).map_err(error))
            .transpose()?
            != selection.pending
        {
            return Err(
                "The original interrupted attempt changed; inspect this recovery again".into(),
            );
        }
        let opened = open_verified(&tool.path, Some(&tool.identity)).map_err(error)?;
        if canonical_digest(IdentityDomain::Evidence, &opened.snapshot).map_err(error)?
            != selection.snapshot
        {
            return Err("The recovered copy changed since verification. Its work was kept; inspect it again before switching.".into());
        }
        let _page = daily_page(tool, &opened.store, &opened.snapshot)?;
        #[cfg(test)]
        if let Some(pause) = &self.config.hooks.before_recovery_handoff {
            pause.reached.store(true, Ordering::Release);
            while !pause.release.load(Ordering::Acquire) && !gate.cancelled.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        gate.check()?;
        // A pinned store can still read a moved original parent. Recheck the
        // user's selected path identity too before retaining that path in the
        // atomic handoff or clearing the original unfinished attempt.
        selected_location.check().map_err(error)?;
        if opened.store.load().map_err(error)? != opened.snapshot {
            return Err("The recovered copy changed before opening".into());
        }
        gate.commit()?;
        self.journal(|j| {
            if let Some(original) = pending {
                j.recovery_handoffs.push(journal::RecoveryHandoff {
                    operation: key.operation.clone(),
                    original,
                    recovered: tool.clone(),
                });
            }
            j.pending = None;
            j.last = Some(tool.clone());
        })?;
        self.install_opened(tool.clone(), opened)
    }
    pub(super) fn files_back(&mut self, key: &Key, gate: &Gate) -> Result<(), String> {
        if !matches!(self.page, Page::LocalFiles(_)) {
            return Err("No file page is open".into());
        }
        self.check_local_origin(self.local_origin()?.as_ref(), key)?;
        if self.opened.is_some() {
            self.return_daily(gate)?;
        } else {
            if !gate.finish() {
                return Err("Returning was cancelled".into());
            }
            self.page = Page::Home;
        }
        self.recovery = None;
        self.recovered_location = None;
        Ok(())
    }
    pub(super) fn file_attempt_notice(&self) -> Option<(Id, String)> {
        let attempt = self.journal.as_ref()?.value.local_file.as_ref()?;
        let inspection = match &attempt.target {
            FileTarget::Output {
                bytes_digest,
                byte_count,
                ..
            } => SelectedFile::new(&attempt.destination)
                .and_then(|f| f.read(*byte_count))
                .ok()
                .filter(|bytes| LocalArtifact::bytes_identity(bytes) == *bytes_digest)
                .map(|_| "The selected file currently matches the expected output bytes."),
            FileTarget::Backup { digest } => {
                RecoveryDraft::read(&attempt.destination, &attempt.operation)
                    .ok()
                    .filter(|d| d.preview().backup == *digest)
                    .map(|_| "The selected backup currently passes its integrity checks.")
            }
            FileTarget::Recovery { identity, .. } => {
                open_verified(&attempt.destination, Some(identity))
                    .ok()
                    .map(|_| {
                        "A separate tool at the selected location currently passes verification."
                    })
            }
        }
        .unwrap_or("The selected destination could not be verified; keep any files there.");
        Some((attempt.operation.clone(), format!("An interrupted file attempt was not repeated. {} {} This inspection is not a receipt for the interrupted action. Keep the original work and inspect this location before starting another file write: {}", match &attempt.target { FileTarget::Output { .. } => "Output save.", FileTarget::Backup { .. } => "Backup save.", FileTarget::Recovery { .. } => "Separate recovery." }, inspection, attempt.destination.display())))
    }
    pub(super) fn acknowledge_file(&mut self, operation: &str, gate: &Gate) -> Result<(), String> {
        if !self
            .journal
            .as_ref()
            .and_then(|j| j.value.local_file.as_ref())
            .is_some_and(|f| f.operation == operation)
        {
            return Err("This file attempt changed; review it again".into());
        }
        gate.commit()?;
        self.journal(|j| j.local_file = None)?;
        self.notice = "File-attempt review finished. Existing files were kept; no external operation was repeated.".into();
        Ok(())
    }
    pub(super) fn file_dialog(&self, kind: DialogKind, title: &str) -> DialogResult {
        #[cfg(test)]
        {
            let _ = (kind, title);
            if let Some(reply) = self
                .config
                .hooks
                .file_dialog_queue
                .lock()
                .unwrap()
                .pop_front()
            {
                return reply;
            }
            self.config.hooks.file_dialog.clone().unwrap_or_else(|| {
                self.config
                    .hooks
                    .folder_choice
                    .clone()
                    .map(DialogResult::Selected)
                    .unwrap_or(DialogResult::Cancelled)
            })
        }
        #[cfg(not(test))]
        {
            #[cfg(target_os = "linux")]
            if std::env::var_os("DISPLAY").is_none()
                && std::env::var_os("WAYLAND_DISPLAY").is_none()
            {
                return DialogResult::Unavailable;
            }
            let run = || {
                let mut dialog = rfd::FileDialog::new().set_title(title);
                if let Some(locations) = &self.locations {
                    dialog = dialog.set_directory(locations.path());
                }
                match kind {
                    DialogKind::Folder => dialog.pick_folder(),
                    DialogKind::OpenBackup => dialog
                        .add_filter("Generated-tool backup", &["gmbak"])
                        .pick_file(),
                    DialogKind::SaveBackup => dialog
                        .set_file_name("saved-work.gmbak")
                        .add_filter("Generated-tool backup", &["gmbak"])
                        .save_file(),
                    DialogKind::SaveOutput(format) => match format {
                        OutputFormat::Csv => dialog
                            .set_file_name("output.csv")
                            .add_filter("CSV output", &["csv"])
                            .save_file(),
                        OutputFormat::Json => dialog
                            .set_file_name("output.json")
                            .add_filter("JSON output", &["json"])
                            .save_file(),
                    },
                }
            };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
                Ok(Some(path)) => DialogResult::Selected(path),
                Ok(None) => DialogResult::Indeterminate,
                Err(_) => DialogResult::Error("native dialog backend stopped unexpectedly".into()),
            }
        }
    }
}
