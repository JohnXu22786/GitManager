//! Request-correlated external-author completion. An author edits the declared
//! program, then publishes completion last. No terminal-close or file-existence
//! event is treated as a successful development run or runtime proof.
use super::{input, invalid, safe_path, TaskSourceAdapter};
use crate::product_contract::{
    canonical_digest, AdapterError, CapturedProgram, DevelopmentRequest, Digest, IdentityDomain,
    MAX_WIRE_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalCompletion {
    pub version: u32,
    pub request_id: String,
    pub request_digest: Digest,
    pub project_id: String,
    pub task_id: String,
    pub baseline_source_fingerprint: String,
    pub source_fingerprint: String,
    pub program_path: String,
    pub raw_digest: Digest,
    pub program_digest: Digest,
}
/// Correlation retained in the existing controller journal alongside its exact
/// authorized request. This is not cryptographic authority, renewed consent or
/// proof that an external author ran. Never recover it from untrusted job files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalHandoffTicket {
    version: u32,
    project_id: String,
    task_id: String,
    created_at: String,
    repository: PathBuf,
    worktree: PathBuf,
    directory: PathBuf,
    request_digest: Digest,
    request_bytes_digest: Digest,
}
/// Controller-owned bundle. It remains outside the complete task fingerprint
/// so a completion marker does not introduce a self-referential source digest.
/// Merely preparing this local bundle does not launch or authorize a Harness.
#[derive(Debug)]
pub struct ExternalHandoff {
    adapter: TaskSourceAdapter,
    directory: PathBuf,
    request: DevelopmentRequest,
    baseline: CapturedProgram,
    request_bytes: Vec<u8>,
}
impl ExternalHandoff {
    pub fn prepare(
        adapter: &TaskSourceAdapter,
        root: &Path,
        request: &DevelopmentRequest,
    ) -> Result<Self, AdapterError> {
        let baseline = Self::request_baseline(adapter, request)?;
        adapter.ensure_fresh(&baseline)?;
        safe_path::directory(root)?;
        let root = fs::canonicalize(root).map_err(|e| AdapterError::Failed(e.to_string()))?;
        if root.starts_with(adapter.worktree()) {
            return Err(invalid("Job bundles must be outside the task source tree"));
        }
        let bytes = Self::request_bytes(request)?;
        // Creation rejects reused IDs, including interrupted/partial old jobs.
        let directory = safe_path::create_job(&root, &request.id)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("request.json"))
            .map_err(|e| AdapterError::Failed(e.to_string()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| AdapterError::Failed(e.to_string()))?;
        adapter.ensure_fresh(&baseline)?;
        Ok(Self {
            adapter: adapter.clone(),
            directory,
            request: request.clone(),
            baseline,
            request_bytes: bytes,
        })
    }
    fn request_baseline(
        adapter: &TaskSourceAdapter,
        request: &DevelopmentRequest,
    ) -> Result<CapturedProgram, AdapterError> {
        request.validate()?;
        if request.project_id != adapter.project_id() {
            return Err(invalid("Request belongs to another project"));
        }
        // External handoffs designate the first source as the editable task
        // baseline. Later captures may be historical intentions from this same
        // task; they must remain unchanged and are never fallback baselines.
        let baseline = request
            .sources
            .first()
            .filter(|source| {
                source
                    .binding
                    .task
                    .as_ref()
                    .is_some_and(|task| task.task_id == adapter.task_id())
            })
            .ok_or_else(|| {
                invalid("The first request source must be the linked editable task baseline")
            })?;
        if request
            .sources
            .iter()
            .skip(1)
            .any(|source| source.binding == baseline.binding)
        {
            return Err(invalid("Duplicate editable task baseline is ambiguous"));
        }
        let baseline = baseline.clone();
        Ok(baseline)
    }
    fn request_bytes(request: &DevelopmentRequest) -> Result<Vec<u8>, AdapterError> {
        let bytes = serde_json::to_vec(request).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_WIRE_BYTES {
            return Err(invalid("Job bundle exceeds byte limit"));
        }
        Ok(bytes)
    }
    pub fn ticket(&self) -> Result<ExternalHandoffTicket, AdapterError> {
        self.adapter.task()?;
        self.check_request()?;
        Ok(ExternalHandoffTicket {
            version: 1,
            project_id: self.adapter.project.clone(),
            task_id: self.adapter.task_id.clone(),
            created_at: self.adapter.created_at.clone(),
            repository: self.adapter.repository.clone(),
            worktree: self.adapter.worktree.clone(),
            directory: self.directory.clone(),
            request_digest: self.request.identity()?,
            // Pin original bytes separately from the canonical domain request.
            request_bytes_digest: canonical_digest(IdentityDomain::Request, &self.request_bytes)?,
        })
    }
    /// Read-only restoration of an existing controller-associated job. The host
    /// must supply its saved ticket and exact authorized request, plus its own
    /// registered root. No preparation, write, invocation or consent occurs.
    /// The author may still be editing; only ingest can accept a completion.
    pub fn reopen(
        adapter: &TaskSourceAdapter,
        root: &Path,
        request: &DevelopmentRequest,
        ticket: &ExternalHandoffTicket,
    ) -> Result<Self, AdapterError> {
        let baseline = Self::request_baseline(adapter, request)?;
        let request_bytes = Self::request_bytes(request)?;
        safe_path::directory(root)?;
        let root = fs::canonicalize(root).map_err(|e| AdapterError::Failed(e.to_string()))?;
        let directory = root.join(&request.id);
        if root.starts_with(adapter.worktree()) || directory.starts_with(adapter.worktree()) {
            return Err(invalid("Job bundles must be outside the task source tree"));
        }
        if directory != ticket.directory {
            return Err(invalid(
                "The saved job belongs to another job root or request",
            ));
        }
        safe_path::directory(&directory)?;
        let resumed = Self {
            adapter: adapter.clone(),
            directory,
            request: request.clone(),
            baseline,
            request_bytes,
        };
        if resumed.ticket()? != *ticket {
            return Err(invalid(
                "The saved job request or linked task identity changed",
            ));
        }
        // Do not compare current bytes to the original baseline: legitimate
        // external edits are expected. The declared artifact path must stay put.
        if adapter.read_declaration()?.program_path != resumed.baseline.binding.program_path {
            return Err(invalid("Declared program moved during this request"));
        }
        // Reload the registry and exact bundle after inspecting the source.
        if resumed.ticket()? != *ticket {
            return Err(invalid("The saved job changed while reopening"));
        }
        Ok(resumed)
    }
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn baseline(&self) -> &CapturedProgram {
        &self.baseline
    }
    pub fn request_path(&self) -> PathBuf {
        self.directory.join("request.json")
    }
    pub fn completion_path(&self) -> PathBuf {
        self.directory.join("complete.json")
    }
    fn check_request(&self) -> Result<(), AdapterError> {
        if safe_path::read(&self.directory, "request.json", MAX_WIRE_BYTES)? != self.request_bytes {
            return Err(invalid("The prepared request bundle was changed"));
        }
        Ok(())
    }
    /// Machine-facing submission helper: computes identities from actual source
    /// instead of requiring a person or Harness to hand-calculate/copy JSON.
    /// This publishes a completion assertion only; ingestion rechecks it.
    pub fn submit_current_program(&self) -> Result<(), AdapterError> {
        self.check_request()?;
        let capture = self.adapter.capture_current()?;
        if capture.binding.program_path != self.baseline.binding.program_path {
            return Err(invalid("Declared program moved during this request"));
        }
        let complete = ExternalCompletion {
            version: 1,
            request_id: self.request.id.clone(),
            request_digest: self.request.identity()?,
            project_id: self.request.project_id.clone(),
            task_id: self.adapter.task_id().into(),
            baseline_source_fingerprint: self
                .baseline
                .binding
                .task
                .as_ref()
                .expect("validated linked baseline")
                .source_fingerprint
                .clone(),
            source_fingerprint: capture
                .binding
                .task
                .as_ref()
                .expect("linked capture")
                .source_fingerprint
                .clone(),
            program_path: capture.binding.program_path.clone(),
            raw_digest: capture.artifact.raw_digest.clone(),
            program_digest: capture.artifact.program_digest.clone(),
        };
        let bytes = serde_json::to_vec(&complete).map_err(|e| invalid(e.to_string()))?;
        safe_path::directory(&self.directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)
            .map_err(|e| AdapterError::Failed(e.to_string()))?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|e| AdapterError::Failed(e.to_string()))?;
        self.adapter.ensure_fresh(&capture)?;
        // Do not overwrite another completion, even for the same request.
        temporary
            .persist_noclobber(self.completion_path())
            .map_err(|e| AdapterError::Failed(e.error.to_string()))?;
        Ok(())
    }
    /// Poll from the controller's worker; incomplete/missing completion stays
    /// pending. A valid result is immutable source, never adopted behavior.
    pub fn ingest(&self) -> Result<Option<CapturedProgram>, AdapterError> {
        self.check_request()?;
        match fs::symlink_metadata(self.completion_path()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(AdapterError::Failed(e.to_string())),
            Ok(_) => (),
        }
        let bytes = safe_path::read(
            &self.directory,
            "complete.json",
            super::MAX_DECLARATION_BYTES,
        )?;
        let value = match input::parse_json_bytes(&bytes) {
            Err(input::InputError::InvalidJson(e)) if e.is_eof() => return Ok(None),
            Err(e) => return Err(invalid(e.to_string())),
            Ok(value) => value,
        };
        let complete: ExternalCompletion =
            serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        if complete.version != 1
            || complete.request_id != self.request.id
            || complete.request_digest != self.request.identity()?
            || complete.project_id != self.request.project_id
            || complete.task_id != self.adapter.task_id()
            || complete.baseline_source_fingerprint
                != self
                    .baseline
                    .binding
                    .task
                    .as_ref()
                    .expect("validated linked baseline")
                    .source_fingerprint
            || complete.program_path != self.baseline.binding.program_path
        {
            return Err(invalid(
                "Completion belongs to another request, task, project, or baseline",
            ));
        }
        let capture = self.adapter.capture_current()?;
        if capture
            .binding
            .task
            .as_ref()
            .expect("linked capture")
            .source_fingerprint
            != complete.source_fingerprint
            || capture.binding.program_path != complete.program_path
            || capture.artifact.raw_digest != complete.raw_digest
            || capture.artifact.program_digest != complete.program_digest
        {
            return Err(AdapterError::Stale(
                "Completed program changed or was only partially published".into(),
            ));
        }
        self.check_request()?;
        if safe_path::read(
            &self.directory,
            "complete.json",
            super::MAX_DECLARATION_BYTES,
        )? != bytes
        {
            return Err(AdapterError::Stale(
                "Completion changed during ingestion".into(),
            ));
        }
        self.adapter.ensure_fresh(&capture)?;
        Ok(Some(capture))
    }
}
