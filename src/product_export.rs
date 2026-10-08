//! Export one verified, committed output without rerunning its producing action.
//!
//! The host must capture the displayed snapshot's full Evidence digest and the
//! selected inventory occurrence, obtain the user's destination choice, then
//! publish once. Comparison/draft outputs cannot construct a prepared export.
//! Module registration, dialogs and host operation fencing are separate work.
use crate::product_contract::{canonical_digest, Digest, Id, IdentityDomain, OutputFormat};
use crate::product_locations::{IssueKind, OperationIssue, SelectedFile};
use crate::product_store::{ProductStore, ProjectSnapshot};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// A digest alone is ambiguous when identical results were emitted repeatedly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactSelection {
    pub inventory_index: usize,
    pub expected_digest: Digest,
}

/// Metadata for an original committed occurrence, not a regenerated result.
/// No historical session identity is claimed: BusinessEvent does not retain it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSummary {
    pub project_id: Id,
    pub snapshot_basis: Digest,
    pub snapshot_revision: u64,
    pub inventory_index: usize,
    pub event_id: Id,
    pub event_sequence: u64,
    pub operation_id: Id,
    pub event_output_ordinal: usize,
    pub event_receipt: Digest,
    pub produced_day: i32,
    pub producing_program: Digest,
    /// Retain the output ID rather than borrow a possibly changed current label.
    pub output_id: Id,
    pub format: OutputFormat,
    pub row_count: usize,
    pub byte_count: usize,
    pub bytes_digest: Digest,
    pub artifact_digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    Cancelled,
    StaleBasis,
    SelectionChanged,
    Issue(OperationIssue),
}
impl From<OperationIssue> for ExportError {
    fn from(issue: OperationIssue) -> Self {
        Self::Issue(issue)
    }
}
impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Export cancelled before writing."),
            Self::StaleBasis => {
                f.write_str("Saved work changed. Refresh and select the output again.")
            }
            Self::SelectionChanged => f.write_str(
                "The selected saved output could not be verified. Refresh and select it again.",
            ),
            Self::Issue(issue) => issue.fmt(f),
        }
    }
}
impl std::error::Error for ExportError {}

/// Bounded identification of the attempted destination and original output.
/// Only ExportOutcome::Verified attests a matching readback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReceipt {
    pub destination: PathBuf,
    pub output: ExportSummary,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportOutcome {
    /// No call to the destination's write operation was made.
    NotAttempted(ExportError),
    /// write_new can fail after publication. Preserve any file, tell the user
    /// it may have been saved, and never automatically retry or remove it.
    PublicationUncertain {
        receipt: ExportReceipt,
        issue: OperationIssue,
    },
    /// Publication returned success, but the saved bytes could not be verified.
    PublishedUnverified {
        receipt: ExportReceipt,
        issue: OperationIssue,
    },
    /// Readback matched exactly at verification time; later edits are possible.
    Verified(ExportReceipt),
}

/// No arbitrary-byte constructor, mutable accessor, deserialization or Clone.
/// Only a fresh verified ProductStore snapshot can supply these bytes.
pub struct PreparedExport {
    store: ProductStore,
    summary: ExportSummary,
    bytes: Vec<u8>,
}
pub struct SelectedExport {
    prepared: PreparedExport,
    file: SelectedFile,
    destination: PathBuf,
}

fn snapshot_basis(snapshot: &ProjectSnapshot) -> Result<Digest, ExportError> {
    canonical_digest(IdentityDomain::Evidence, snapshot)
        .map_err(OperationIssue::from)
        .map_err(ExportError::from)
}
impl PreparedExport {
    pub fn capture(
        store: &ProductStore,
        expected_snapshot: &Digest,
        selection: ArtifactSelection,
    ) -> Result<Self, ExportError> {
        let snapshot = store.load().map_err(OperationIssue::from)?;
        let basis = snapshot_basis(&snapshot)?;
        if &basis != expected_snapshot {
            return Err(ExportError::StaleBasis);
        }
        let artifact = snapshot
            .artifacts
            .get(selection.inventory_index)
            .filter(|artifact| artifact.digest == selection.expected_digest)
            .ok_or(ExportError::SelectionChanged)?;
        artifact.validate().map_err(OperationIssue::from)?;
        // Snapshot validation already binds this complete ordered inventory.
        // Resolve by occurrence, never find the first matching content digest.
        let (event, ordinal, receipt) = snapshot
            .data
            .events
            .iter()
            .flat_map(|event| {
                event
                    .outputs
                    .iter()
                    .enumerate()
                    .map(move |(ordinal, receipt)| (event, ordinal, receipt))
            })
            .nth(selection.inventory_index)
            .ok_or(ExportError::SelectionChanged)?;
        if receipt != &artifact.digest {
            return Err(ExportError::SelectionChanged);
        }
        let summary = ExportSummary {
            project_id: snapshot.data.project_id.clone(),
            snapshot_basis: basis,
            snapshot_revision: snapshot.revision,
            inventory_index: selection.inventory_index,
            event_id: event.id.clone(),
            event_sequence: event.sequence,
            operation_id: event.operation_id.clone(),
            event_output_ordinal: ordinal,
            event_receipt: canonical_digest(IdentityDomain::Evidence, event)
                .map_err(OperationIssue::from)?,
            produced_day: event.day,
            producing_program: event.program.clone(),
            output_id: artifact.output.clone(),
            format: artifact.format,
            row_count: artifact.rows.len(),
            byte_count: artifact.bytes.len(),
            bytes_digest: artifact.bytes_digest.clone(),
            artifact_digest: artifact.digest.clone(),
        };
        Ok(Self {
            store: store.clone(),
            summary,
            bytes: artifact.bytes.clone(),
        })
    }
    pub fn summary(&self) -> &ExportSummary {
        &self.summary
    }
    /// Call only for a deliberate user-selected path. Pin its parent now so a
    /// replaced destination cannot silently redirect the later publication.
    pub fn select_destination(self, path: &Path) -> Result<SelectedExport, ExportError> {
        let file = SelectedFile::new(path)?;
        Ok(SelectedExport {
            prepared: self,
            file,
            destination: path.to_path_buf(),
        })
    }
}
impl SelectedExport {
    /// One attempt only. Cancellation is honored immediately before writing.
    /// There is no transaction spanning store and destination: another process
    /// can advance the store after the final load. The receipt identifies the
    /// exact snapshot revision checked here, not a promise of continued currency.
    pub fn publish(self, cancelled: &AtomicBool) -> ExportOutcome {
        let check = || -> Result<(), ExportError> {
            if cancelled.load(Ordering::Acquire) {
                return Err(ExportError::Cancelled);
            }
            self.file.check_parent()?;
            let current = self.prepared.store.load().map_err(OperationIssue::from)?;
            if snapshot_basis(&current)? != self.prepared.summary.snapshot_basis {
                return Err(ExportError::StaleBasis);
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(ExportError::Cancelled);
            }
            Ok(())
        };
        if let Err(error) = check() {
            return ExportOutcome::NotAttempted(error);
        }
        let receipt = ExportReceipt {
            destination: self.destination,
            output: self.prepared.summary,
        };
        // The immutable bytes already contain the encoder's CSV/JSON policy.
        // Do not normalize, add a BOM, convert formats or serialize rows again.
        if let Err(issue) = self.file.write_new(&self.prepared.bytes) {
            return ExportOutcome::PublicationUncertain { receipt, issue };
        }
        // Once publication began, cancellation cannot truthfully undo the save.
        // Always finish verification and return what actually happened.
        match self.file.read(self.prepared.bytes.len()) {
            Ok(bytes) if bytes == self.prepared.bytes => ExportOutcome::Verified(receipt),
            Ok(_) => ExportOutcome::PublishedUnverified {
                receipt,
                issue: OperationIssue::new(
                    IssueKind::Corrupt,
                    "export readback differs from the original committed bytes",
                ),
            },
            Err(issue) => ExportOutcome::PublishedUnverified { receipt, issue },
        }
    }
}
