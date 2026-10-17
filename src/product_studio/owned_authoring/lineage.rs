//! Correlation of actual host-supplied CLI evidence, not invocation, consent UI,
//! external/native completion ingestion, or authentication against local tampering.
use super::*;
use crate::product_provider::{
    ConsentReceipt, DataDisclosure, InvocationProvenance, JobState, ProviderKind, ProviderResult,
};

/// An immutable local correlation result. Not deserializable, and never an
/// approval, proof of model quality, lifecycle capability or adoption handle.
#[derive(Clone, Debug)]
pub struct OwnedCliLineage {
    request: OwnedAuthorRequestV1,
    result: ParsedOwnedAuthorResponse,
    producer: Producer,
    /// Retain the exact outer/wire/raw associations separately from the domain
    /// producer contract. Receipt bytes are never supplied by author JSON.
    receipt: transport::JobReceipt,
    wire_digest: String,
}
impl OwnedCliLineage {
    /// Supply the disclosure/consent retained by the trusted host at prepare /
    /// authorize time, and a ProviderResult from the actual transport ingestion.
    /// Merely passing mutually consistent JSON does not establish execution.
    pub fn validate(
        request: &OwnedAuthorRequestV1,
        options: &ProviderOptions,
        disclosure: &DataDisclosure,
        consent: &ConsentReceipt,
        result: &ProviderResult,
    ) -> Result<Self> {
        let wire = request.encode(options)?;
        let wire_digest = wire.digest().map_err(error)?;
        let receipt = &result.receipt;
        require(
            result.final_bytes.len() <= options.limits.result_bytes,
            "result exceeds the authorized result-byte limit",
        )?;
        require(
            disclosure.request_id == wire.request_id
                && disclosure.request_digest == wire_digest
                && disclosure.provider == wire.provider
                && disclosure.profile == wire.profile
                && disclosure.recipient == wire.provider.recipient()
                && disclosure.source_digest == wire.source_digest
                && disclosure.prompt_digest == transport::digest(&wire.prompt)
                && disclosure.schema_digest == transport::digest(&wire.schema)
                && disclosure.purpose == wire.purpose
                && disclosure.data_categories == wire.data_categories,
            "exact prepared request or disclosure changed",
        )?;
        require(
            disclosure.nonce.len() == 32
                && disclosure
                    .nonce
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid prepared transport nonce",
        )?;
        for digest in [
            &disclosure.wire_schema_digest,
            &disclosure.stdin_digest,
            &disclosure.capability_digest,
            &disclosure.environment_digest,
            &disclosure.command_digest,
        ] {
            Digest::try_from(digest.clone())?;
        }
        require(
            consent.disclosure_digest == disclosure.digest()
                && !consent.approval_reference.trim().is_empty()
                && consent.approval_reference.len() <= 1024,
            "exact explicit consent is missing or changed",
        )?;
        require(
            receipt.version == transport::RECEIPT_VERSION
                && receipt.state == JobState::TransportValidated
                && receipt.exit_code == Some(0)
                && receipt.request_id == wire.request_id
                && receipt.request_digest == wire_digest
                && receipt.source_digest == wire.source_digest
                && receipt.provider == wire.provider
                && receipt.disclosure == *disclosure
                && receipt.consent.as_ref() == Some(consent)
                && receipt.result_digest.as_deref()
                    == Some(transport::digest(&result.final_bytes).as_str()),
            "terminal receipt does not bind the exact outer result, request or consent",
        )?;
        require(
            receipt.capabilities.provider == wire.provider
                && receipt.capabilities.provenance == receipt.provenance
                && receipt.capabilities.digest() == disclosure.capability_digest
                && receipt.capabilities.supports(wire.profile),
            "provider capability, executable, profile or provenance changed",
        )?;
        require(
            receipt
                .started_at_unix_ms
                .zip(receipt.finished_at_unix_ms)
                .is_some_and(|(start, end)| start <= end && start < consent.expires_at_unix_ms)
                && receipt
                    .session_id
                    .as_ref()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= MAX_TEXT_BYTES),
            "missing or invalid terminal session/timestamps",
        )?;
        let domain = request.domain_request()?;
        let producer = match receipt.provenance {
            InvocationProvenance::TransportFixture => Producer::Fixture {
                name: format!("recorded transport fixture {}", receipt.request_id),
            },
            InvocationProvenance::LiveCli => Producer::LiveAgent {
                provider: match receipt.provider {
                    ProviderKind::Codex => "codex",
                    ProviderKind::Claude => "claude",
                }
                .into(),
                invocation_id: receipt.request_id.clone(),
                session_id: receipt.session_id.clone(),
                // Frozen domain contract: never replace with outer/wire identity.
                request_digest: domain.identity()?,
            },
        };
        let parsed = ParsedOwnedAuthorResponse::parse_for(&result.final_bytes, request)?;
        DevelopmentResult {
            response: parsed.response().clone(),
            producer: producer.clone(),
        }
        .validate_for(&domain)?;
        Ok(Self {
            request: request.clone(),
            result: parsed,
            producer,
            receipt: receipt.clone(),
            wire_digest,
        })
    }
    pub fn producer(&self) -> &Producer {
        &self.producer
    }
    pub fn raw_bytes(&self) -> &[u8] {
        self.result.raw_bytes()
    }
    pub fn receipt(&self) -> &transport::JobReceipt {
        &self.receipt
    }
    pub fn wire_digest(&self) -> &str {
        &self.wire_digest
    }
    pub fn capture_candidate(&self, candidate: &str) -> Result<CapturedProgram> {
        let returned = self
            .result
            .response()
            .candidates
            .iter()
            .find(|c| c.id == candidate)
            .ok_or_else(|| error("candidate absent from this correlated result"))?;
        let domain = self.request.domain_request()?;
        let current = &domain.sources[self.request.envelope.current_source_index];
        CapturedProgram::capture(
            returned.source_json.as_bytes(),
            &domain.project_id,
            self.producer.clone(),
            current.binding.task.clone(),
        )?
        .at_path(&current.binding.program_path)
    }
    pub fn validate_capture(&self, candidate: &str, capture: &CapturedProgram) -> Result<()> {
        require(
            self.capture_candidate(candidate)? == *capture,
            "retained candidate source, producer, task or path changed",
        )
    }
    pub fn materialize(
        &self,
        snapshot: &ProjectSnapshot,
        candidate: &str,
    ) -> Result<Option<MaterializedOwnedProposal>> {
        self.result.materialize(
            &self.request,
            snapshot,
            candidate,
            &self.capture_candidate(candidate)?,
        )
    }
}
