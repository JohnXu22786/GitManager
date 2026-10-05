//! Actual task source capture. Watch notifications trigger rechecks; they never
//! establish provenance, execution success, or approval to adopt a program.
mod handoff;
#[path = "../tool_proposal_input.rs"]
mod input;
mod safe_path;
mod watcher;
pub use handoff::{ExternalCompletion, ExternalHandoff};
pub use watcher::{SourceUpdate, SourceWatcher};

use crate::{
    product_contract::{
        valid_id, validate_relative_path, AdapterError, ArtifactKind, CapturedProgram,
        ContractError, Producer, SourceAdapter, SourceBinding, TaskSource, MAX_WIRE_BYTES,
    },
    product_provider::{ProviderRequest, ProviderResult, ProviderTransport},
    task_verification::source_fingerprint,
    tasks::{TaskRecord, TaskRegistry},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const DECLARATION_PATH: &str = ".gitmanager/product.json";
pub const MAX_DECLARATION_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDeclaration {
    pub version: u32,
    pub project_id: String,
    pub artifact_kind: ArtifactKind,
    pub program_path: String,
}

/// Pins an existing registry entry, never creates another task registry. Each
/// capture reloads the actual registry and the complete original fingerprint.
#[derive(Debug, Clone)]
pub struct TaskSourceAdapter {
    project: String,
    task_id: String,
    created_at: String,
    repository: PathBuf,
    worktree: PathBuf,
}
fn invalid(message: impl Into<String>) -> AdapterError {
    AdapterError::Invalid(ContractError(message.into()))
}
fn fresh_task(id: &str) -> Result<TaskRecord, AdapterError> {
    let registry = TaskRegistry::load();
    if let Some(error) = registry.load_error() {
        return Err(AdapterError::Failed(format!(
            "Task registry is unavailable: {error}"
        )));
    }
    let task = registry
        .entries()
        .iter()
        .find(|task| task.id == id)
        .ok_or_else(|| AdapterError::Stale("The linked task no longer exists".into()))?;
    if task.worktree_cleanup_completed_at.is_some() {
        return Err(AdapterError::Stale(
            "The linked worktree was removed".into(),
        ));
    }
    Ok(task.clone())
}
fn resolve(path: &str) -> Result<PathBuf, AdapterError> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(invalid("Task paths must be absolute"));
    }
    safe_path::directory(path)?;
    fs::canonicalize(path).map_err(|e| AdapterError::Stale(e.to_string()))
}
impl TaskSourceAdapter {
    pub fn link(project_id: &str, task_id: &str) -> Result<Self, AdapterError> {
        if !valid_id(project_id) || !valid_id(task_id) {
            return Err(invalid("Invalid project or task ID"));
        }
        let task = fresh_task(task_id)?;
        let adapter = Self {
            project: project_id.into(),
            task_id: task_id.into(),
            created_at: task.created_at,
            repository: resolve(&task.repository_path)?,
            worktree: resolve(&task.worktree_path)?,
        };
        adapter.fingerprint()?;
        Ok(adapter)
    }
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn project_id(&self) -> &str {
        &self.project
    }
    fn task(&self) -> Result<TaskRecord, AdapterError> {
        let task = fresh_task(&self.task_id)?;
        if task.created_at != self.created_at
            || resolve(&task.repository_path)? != self.repository
            || resolve(&task.worktree_path)? != self.worktree
        {
            return Err(AdapterError::Stale(
                "The linked task was moved or replaced".into(),
            ));
        }
        Ok(task)
    }
    fn fingerprint(&self) -> Result<String, AdapterError> {
        self.task()?;
        source_fingerprint(&self.worktree, &self.repository).map_err(AdapterError::Stale)
    }
    fn check_target(&self, project: &str, task: Option<&str>) -> Result<(), AdapterError> {
        if project != self.project || task != Some(self.task_id.as_str()) {
            return Err(invalid("Source belongs to another project or task"));
        }
        Ok(())
    }
    fn read_declaration(&self) -> Result<ProgramDeclaration, AdapterError> {
        if !self
            .worktree
            .join(DECLARATION_PATH)
            .try_exists()
            .map_err(|e| AdapterError::Failed(e.to_string()))?
        {
            return Err(AdapterError::Unsupported("This repository has no generated-program declaration. Its source has not been executed by a runtime adapter.".into()));
        }
        let bytes = safe_path::read(&self.worktree, DECLARATION_PATH, MAX_DECLARATION_BYTES)?;
        let value = input::parse_json_bytes(&bytes).map_err(|e| invalid(e.to_string()))?;
        let declaration: ProgramDeclaration =
            serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        if declaration.version != 1 || declaration.project_id != self.project {
            return Err(invalid("Declaration version or project does not match"));
        }
        safe_path::relative(&declaration.program_path)?;
        if declaration.program_path == DECLARATION_PATH {
            return Err(invalid("Declaration cannot be executable source"));
        }
        if declaration.artifact_kind != ArtifactKind::GeneratedApp {
            return Err(AdapterError::Unsupported("This repository requires an unavailable executable runtime adapter. No source execution or behavior validation has occurred.".into()));
        }
        Ok(declaration)
    }
    pub fn capture_current(&self) -> Result<CapturedProgram, AdapterError> {
        self.capture(&self.project, Some(&self.task_id))
    }
    /// Check before execution and immediately before adoption. Artifact identity
    /// alone is insufficient: unrelated/ignored task changes also stale proof.
    pub fn ensure_fresh(&self, captured: &CapturedProgram) -> Result<(), AdapterError> {
        captured.validate()?;
        self.check_target(
            &captured.binding.project_id,
            captured.binding.task.as_ref().map(|t| t.task_id.as_str()),
        )?;
        let current = self.capture_current()?;
        if current.binding != captured.binding || current.artifact != captured.artifact {
            return Err(AdapterError::Stale(
                "Task source changed; run a fresh analysis before using this result".into(),
            ));
        }
        Ok(())
    }
    /// A worker must discard its return value if source changes during its run.
    pub fn with_fresh_source<T>(
        &self,
        captured: &CapturedProgram,
        work: impl FnOnce() -> Result<T, AdapterError>,
    ) -> Result<T, AdapterError> {
        self.ensure_fresh(captured)?;
        let result = work();
        self.ensure_fresh(captured)?;
        result
    }
    /// V02 owns transport validation and origin. This boundary returns the
    /// opaque result unchanged, with no domain parse or evidence promotion.
    /// source_digest is the captured SourceBinding identity, not a program hash.
    pub fn ingest_transport(
        &self,
        transport: &ProviderTransport,
        request: &ProviderRequest,
        baseline: &CapturedProgram,
    ) -> Result<Option<ProviderResult>, AdapterError> {
        if request.source_digest != baseline.binding.identity()?.as_str() {
            return Err(invalid("Provider request is bound to another source"));
        }
        let digest = request.digest().map_err(invalid)?;
        self.with_fresh_source(baseline, || {
            transport
                .ingest(&request.request_id, &digest, &request.source_digest)
                .map_err(AdapterError::Failed)
        })
    }
}
impl SourceAdapter for TaskSourceAdapter {
    fn capture(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> Result<CapturedProgram, AdapterError> {
        self.check_target(project_id, task_id)?;
        let task = self.task()?;
        let before = self.fingerprint()?;
        let declaration = self.read_declaration()?;
        let bytes = safe_path::read(&self.worktree, &declaration.program_path, MAX_WIRE_BYTES)?;
        let producer=Producer::ExternalAuthor { description:format!("External edits in linked task {}; recorded provider {:?}, session {:?}. No verified live invocation is claimed.",task.id,task.provider_ref,task.session_ref) };
        let captured = CapturedProgram::capture(
            &bytes,
            &self.project,
            producer,
            Some(TaskSource {
                task_id: self.task_id.clone(),
                source_fingerprint: before.clone(),
            }),
        )?
        .at_path(&declaration.program_path)?;
        let after = self.fingerprint()?;
        let final_task = self.task()?;
        if before != after
            || task.provider_ref != final_task.provider_ref
            || task.session_ref != final_task.session_ref
        {
            return Err(AdapterError::Stale(
                "Task changed during source capture; retry after writing finishes".into(),
            ));
        }
        Ok(captured)
    }
    fn current_binding(
        &self,
        project_id: &str,
        task_id: Option<&str>,
    ) -> Result<SourceBinding, AdapterError> {
        Ok(self.capture(project_id, task_id)?.binding)
    }
}
