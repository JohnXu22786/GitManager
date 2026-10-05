//! Capability- and consent-bound CLI transport. Returned JSON is untrusted data.
//!
//! See README.md for live capability limits, threat model and integration API.
//! This module never creates approval, installs a provider, or executes a result.
mod adapter;
#[path = "../tool_proposal_input.rs"]
mod input;
mod probe;
mod process;
pub mod protocol;
mod storage;
pub use protocol::*;

use probe::Endpoint;
use ring::rand::{SecureRandom, SystemRandom};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

/// Test-only fixture publication guard. This cannot enable a real profile.
#[cfg(test)]
pub fn fixture_executable_write_guard() -> std::sync::MutexGuard<'static, ()> {
    process::FIXTURE_SPAWN_GATE
        .lock()
        .expect("fixture spawn coordination")
}

#[derive(Clone)]
pub struct ProviderTransport {
    root: PathBuf,
    endpoint: Endpoint,
}
#[derive(Debug, Clone)]
pub struct PreparedJob {
    pub disclosure: DataDisclosure,
    pub capabilities: ProviderCapabilities,
}
struct Gate {
    cancelled: bool,
    terminal: bool,
}
pub struct ProviderJob {
    directory: PathBuf,
    cancel: Arc<AtomicBool>,
    gate: Arc<Mutex<Gate>>,
    receive: mpsc::Receiver<Result<JobReceipt, String>>,
    worker: Option<thread::JoinHandle<()>>,
    result: Option<JobReceipt>,
}
impl ProviderJob {
    /// Nonblocking; reports a durable terminal receipt, never intermediate text.
    pub fn poll(&mut self) -> Result<Option<JobReceipt>, String> {
        if self.result.is_some() {
            return Ok(self.result.clone());
        }
        match self.receive.try_recv() {
            Ok(result) => {
                let receipt = result?;
                if let Some(worker) = self.worker.take() {
                    worker.join().map_err(|_| "provider worker panicked")?;
                }
                self.result = Some(receipt.clone());
                Ok(Some(receipt))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(
                "provider worker stopped without a durable terminal receipt; reconcile the job"
                    .into(),
            ),
        }
    }
    /// True means cancellation was accepted before the terminal commit. The
    /// shared gate linearizes cancellation against publication of completion.
    pub fn cancel(&self) -> Result<bool, String> {
        let mut gate = self.gate.lock().map_err(|_| "job state lock poisoned")?;
        if gate.terminal {
            return Ok(false);
        }
        gate.cancelled = true;
        self.cancel.store(true, Ordering::Release);
        storage::write(&self.directory.join("cancelled"), b"cancel requested\n")?;
        Ok(true)
    }
}
impl Drop for ProviderJob {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.cancel();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}
impl ProviderTransport {
    /// Use the user's normal credential HOME; credentials are never read/copied.
    /// Only a trusted, explicitly selected installed executable belongs here.
    pub fn new(
        root: PathBuf,
        provider: ProviderKind,
        executable: PathBuf,
        credential_home: PathBuf,
    ) -> Result<Self, String> {
        if root.to_str().is_none()
            || executable.to_str().is_none()
            || credential_home.to_str().is_none()
        {
            return Err("transport paths must be valid UTF-8".into());
        }
        storage::safe_directory(&root, true)?;
        storage::safe_directory(&credential_home, false)?;
        if !executable.is_absolute() {
            return Err("CLI selection must be an absolute path".into());
        }
        // Resolve installed executable symlinks once and bind its content hash.
        let executable = fs::canonicalize(&executable).unwrap_or(executable);
        if executable.to_str().is_none() {
            return Err("canonical CLI path must be valid UTF-8".into());
        }
        Ok(Self {
            root,
            endpoint: Endpoint {
                kind: provider,
                executable,
                home: credential_home,
                fixture: false,
            },
        })
    }
    #[cfg(test)]
    pub fn new_fixture(
        root: PathBuf,
        provider: ProviderKind,
        executable: PathBuf,
        home: PathBuf,
    ) -> Result<Self, String> {
        let mut transport = Self::new(root, provider, executable, home)?;
        transport.endpoint.fixture = true;
        Ok(transport)
    }
    pub fn provider(&self) -> ProviderKind {
        self.endpoint.kind
    }
    pub fn executable(&self) -> &Path {
        &self.endpoint.executable
    }
    /// Runs only bounded version/help/auth status commands, with cleared env.
    /// No cached successful probe can survive an executable or config change.
    pub fn probe(&self) -> ProviderCapabilities {
        self.endpoint.probe(&self.root)
    }
    fn directory(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("invalid job ID".into());
        }
        let path = self.root.join(id);
        storage::safe_directory(&path, false)?;
        Ok(path)
    }
    pub fn prepare(&self, request: ProviderRequest) -> Result<PreparedJob, String> {
        request.validate()?;
        if request.provider != self.endpoint.kind {
            return Err("provider selection changed".into());
        }
        let capabilities = self.probe();
        if !capabilities.supports(request.profile) {
            return Err(format!(
                "required provider capability is unavailable: {}",
                capabilities.detail
            ));
        }
        let mut random = [0u8; 16];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| "could not create job nonce")?;
        let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let (stdin, schema) = adapter::wire(&request, &nonce)?;
        let capability_notice = match request.profile {
            CapabilityProfile::DataOnly => "Requires an established no-tools, no-MCP, no-hooks, no-plugins and no-host-context profile; not an OS sandbox for the installed CLI itself.",
            CapabilityProfile::TrustedHarness => "Trusted-Harness is NOT tool-free or confined data disclosure. The installed CLI and administrator-managed configuration can apply hooks, plugins/MCP and host context, potentially reading or transmitting files outside this job and making network calls. We request supported restrictions and disable known integrations, but effective configuration and sandbox enforcement are unverified. The CLI itself retains ordinary OS-user access and existing authentication. Model-service communication is separate from tool subprocess network restrictions. Approve only if you trust this installed Harness and its configuration.",
        };
        let command_digest =
            self.endpoint
                .command_digest(&self.root.join(&request.request_id), &schema, &nonce);
        let disclosure = DataDisclosure {
            request_id: request.request_id.clone(), nonce, provider: request.provider,
            recipient: request.provider.recipient().into(), request_digest: request.digest()?,
            source_digest: request.source_digest.clone(), prompt_digest: digest(&request.prompt),
            schema_digest: digest(&request.schema), wire_schema_digest: digest(&schema),
            stdin_digest: digest(&stdin), capability_digest: capabilities.digest(),
            profile: request.profile, purpose: request.purpose.clone(), data_categories: request.data_categories.clone(),
            capability_notice: capability_notice.into(),
            billing_notice: "Uses existing subscription authentication and may consume its quota. No API key, new login or paid-API fallback is permitted.".into(),
            environment_digest: self.endpoint.environment_digest(), command_digest,
        };
        storage::safe_directory(&self.root, false)?;
        let directory = self.root.join(&request.request_id);
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .map_err(|e| format!("request ID already used or job directory unavailable: {e}"))?;
        storage::sync_directory(&self.root)?;
        drop(storage::file(&directory.join("lock"), true, true)?);
        let lock = storage::lock(&directory)?;
        storage::write_json(&directory.join("request.json"), &request)?;
        storage::write(&directory.join("stdin.json"), &stdin)?;
        storage::write(&directory.join("schema.json"), &schema)?;
        let receipt = JobReceipt {
            version: RECEIPT_VERSION,
            request_id: request.request_id,
            request_digest: disclosure.request_digest.clone(),
            source_digest: disclosure.source_digest.clone(),
            provider: request.provider,
            provenance: capabilities.provenance,
            capabilities: capabilities.clone(),
            disclosure: disclosure.clone(),
            consent: None,
            state: JobState::Prepared,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
            exit_code: None,
            session_id: None,
            result_digest: None,
            stdout_digest: None,
            stderr_digest: None,
            usage: None,
            detail: "prepared locally; no provider request sent".into(),
        };
        storage::write_json(&directory.join("receipt.json"), &receipt)?;
        drop(lock);
        Ok(PreparedJob {
            disclosure,
            capabilities,
        })
    }
    pub fn submit(
        &self,
        prepared: PreparedJob,
        consent: &ConsentReceipt,
    ) -> Result<ProviderJob, String> {
        let directory = self.directory(&prepared.disclosure.request_id)?;
        let lock = storage::lock(&directory)?;
        let mut receipt = storage::receipt(&directory)?;
        if receipt.state != JobState::Prepared
            || receipt.disclosure != prepared.disclosure
            || receipt.capabilities != prepared.capabilities
        {
            return Err("prepared job changed or was already started".into());
        }
        if consent.approval_reference.trim().is_empty()
            || consent.approval_reference.len() > 1024
            || consent.expires_at_unix_ms <= unix_ms()
            || consent.disclosure_digest != receipt.disclosure.digest()
        {
            return Err("exact current disclosure has not been authorized".into());
        }
        let request: ProviderRequest = storage::json(&directory.join("request.json"))?;
        if request.digest()? != receipt.request_digest
            || request.provider != self.endpoint.kind
            || receipt.provenance
                != if self.endpoint.fixture {
                    InvocationProvenance::TransportFixture
                } else {
                    InvocationProvenance::LiveCli
                }
        {
            return Err("request/provider/provenance changed".into());
        }
        if self.endpoint.environment_digest() != receipt.disclosure.environment_digest {
            return Err("provider environment changed".into());
        }
        let (expected_stdin, expected_schema) = adapter::wire(&request, &receipt.disclosure.nonce)?;
        let stdin = storage::read(&directory.join("stdin.json"), MAX_RESULT_BYTES)?;
        let schema = storage::read(&directory.join("schema.json"), MAX_RESULT_BYTES)?;
        if stdin != expected_stdin
            || schema != expected_schema
            || digest(&stdin) != receipt.disclosure.stdin_digest
            || digest(&schema) != receipt.disclosure.wire_schema_digest
        {
            return Err("prepared input/schema route changed".into());
        }
        if self
            .endpoint
            .command_digest(&directory, &schema, &receipt.disclosure.nonce)
            != receipt.disclosure.command_digest
        {
            return Err("provider invocation arguments or executable route changed".into());
        }
        // Probe again immediately before sending anything. No hidden provider or
        // profile fallback, even when auth was valid during preparation.
        let fresh = self.probe();
        if fresh != receipt.capabilities || !fresh.supports(request.profile) {
            return Err(
                "provider capability or executable changed; prepare and authorize again".into(),
            );
        }
        if fs::symlink_metadata(directory.join("final.json")).is_ok()
            || fs::symlink_metadata(directory.join("result.json")).is_ok()
        {
            return Err("pre-existing output cannot belong to this invocation".into());
        }
        if consent.expires_at_unix_ms <= unix_ms() {
            return Err("authorization expired during capability probes".into());
        }
        receipt.consent = Some(consent.clone());
        receipt.state = JobState::Running;
        receipt.started_at_unix_ms = Some(unix_ms());
        receipt.detail = "provider job started; no validated result".into();
        storage::write_json(&directory.join("receipt.json"), &receipt)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(Mutex::new(Gate {
            cancelled: false,
            terminal: false,
        }));
        let (send, receive) = mpsc::channel();
        let transport = self.clone();
        let work_dir = directory.clone();
        let worker_cancel = cancel.clone();
        let worker_gate = gate.clone();
        let worker = thread::spawn(move || {
            let result = transport.execute(
                work_dir,
                lock,
                request,
                receipt,
                stdin,
                schema,
                worker_cancel,
                worker_gate,
            );
            let _ = send.send(result);
        });
        Ok(ProviderJob {
            directory,
            cancel,
            gate,
            receive,
            worker: Some(worker),
            result: None,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        directory: PathBuf,
        _lock: storage::JobLock,
        request: ProviderRequest,
        mut receipt: JobReceipt,
        stdin: Vec<u8>,
        schema: Vec<u8>,
        cancel: Arc<AtomicBool>,
        gate: Arc<Mutex<Gate>>,
    ) -> Result<JobReceipt, String> {
        let mut command = self.endpoint.command(&directory);
        command.args(adapter::arguments(
            request.provider,
            &directory,
            &schema,
            &receipt.disclosure.nonce,
        ));
        let output_path = directory.join("final.json");
        let output = process::run(
            command,
            &stdin,
            &request.limits,
            &cancel,
            if request.provider == ProviderKind::Codex {
                Some(output_path.as_path())
            } else {
                None
            },
            receipt.consent.as_ref().map(|c| c.expires_at_unix_ms),
        );
        receipt.exit_code = output.exit_code;
        receipt.stdout_digest = Some(digest(&output.stdout));
        receipt.stderr_digest = Some(digest(&output.stderr));
        let mut payload = None;
        let state = if let Some(failure) = output.failure {
            receipt.detail = output.detail;
            failure
        } else if let Some(failure) = adapter::provider_failure(&output.stdout) {
            receipt.detail = "provider reported an error; no result accepted".into();
            failure
        } else if output.exit_code != Some(0) {
            receipt.detail = "provider exited unsuccessfully".into();
            JobState::ProviderError
        } else {
            let final_file = if request.provider == ProviderKind::Codex {
                storage::read(&output_path, request.limits.result_bytes).map(Some)
            } else {
                Ok(None)
            };
            match final_file.and_then(|bytes| {
                adapter::parse(
                    request.provider,
                    &output.stdout,
                    bytes.as_deref(),
                    &receipt.disclosure,
                )
            }) {
                Ok(parsed) if parsed.final_bytes.len() <= request.limits.result_bytes => {
                    receipt.session_id = Some(parsed.session);
                    receipt.usage = parsed.usage;
                    payload = Some(parsed.final_bytes);
                    receipt.detail="transport syntax and correlation validated; domain validation and execution still required".into();
                    JobState::TransportValidated
                }
                Ok(_) => {
                    receipt.detail = "payload exceeds byte limit".into();
                    JobState::OutputLimit
                }
                Err(error) => {
                    receipt.detail = error;
                    JobState::InvalidOutput
                }
            }
        };
        let mut gate = gate.lock().map_err(|_| "job state lock poisoned")?;
        receipt.state = if gate.cancelled || cancel.load(Ordering::Acquire) {
            receipt.detail = "cancellation accepted before terminal publication".into();
            JobState::Cancelled
        } else {
            state
        };
        if receipt.state == JobState::TransportValidated {
            let bytes = payload.ok_or("missing validated payload")?;
            storage::write(&directory.join("result.json"), &bytes)?;
            receipt.result_digest = Some(digest(&bytes));
        }
        receipt.finished_at_unix_ms = Some(unix_ms());
        storage::write_json(&directory.join("receipt.json"), &receipt)?;
        gate.terminal = true;
        Ok(receipt)
    }
    pub fn inspect(&self, id: &str) -> Result<JobReceipt, String> {
        storage::receipt(&self.directory(id)?)
    }
    /// A released OS lock and a Running receipt mean the former owner stopped,
    /// not that its provider succeeded. Do not kill saved PIDs (PID reuse),
    /// resume a session, import a late file, or resend automatically.
    pub fn reconcile(&self, id: &str) -> Result<JobReceipt, String> {
        let directory = self.directory(id)?;
        let _lock = storage::lock(&directory)?;
        let mut receipt = storage::receipt(&directory)?;
        if receipt.state == JobState::Running {
            receipt.state = JobState::Interrupted;
            receipt.finished_at_unix_ms = Some(unix_ms());
            receipt.detail="previous owner interrupted; process cleanup is unconfirmed and late output is quarantined. Start a newly authorized job only after resolving the old provider process.".into();
            storage::write_json(&directory.join("receipt.json"), &receipt)?;
        }
        Ok(receipt)
    }
    /// Automatic ingestion: no editable result manifest and no model-supplied
    /// path or success assertion is accepted. The caller supplies fresh source.
    pub fn ingest(
        &self,
        id: &str,
        expected_request: &str,
        current_source: &str,
    ) -> Result<Option<ProviderResult>, String> {
        let directory = self.directory(id)?;
        let _lock = storage::lock(&directory)?;
        let receipt = storage::receipt(&directory)?;
        if receipt.provenance
            != if self.endpoint.fixture {
                InvocationProvenance::TransportFixture
            } else {
                InvocationProvenance::LiveCli
            }
            || receipt.provider != self.endpoint.kind
            || receipt.request_digest != expected_request
            || receipt.source_digest != current_source
        {
            return Err("result is stale or belongs to another provider/request/source".into());
        }
        if receipt.state != JobState::TransportValidated {
            return Ok(None);
        }
        if receipt.exit_code != Some(0)
            || receipt.session_id.is_none()
            || receipt
                .consent
                .as_ref()
                .is_none_or(|c| c.disclosure_digest != receipt.disclosure.digest())
        {
            return Err("completion lacks matching terminal provenance".into());
        }
        let request: ProviderRequest = storage::json(&directory.join("request.json"))?;
        if request.digest()? != expected_request {
            return Err("saved request was modified".into());
        }
        let bytes = storage::read(&directory.join("result.json"), request.limits.result_bytes)?;
        if receipt.result_digest != Some(digest(&bytes)) {
            return Err("result publication incomplete or changed".into());
        }
        input::parse_json_bytes(&bytes).map_err(|e| e.to_string())?;
        Ok(Some(ProviderResult {
            final_bytes: bytes,
            receipt,
        }))
    }
}
