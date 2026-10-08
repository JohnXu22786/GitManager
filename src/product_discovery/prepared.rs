//! Exact authored-result to host-compiled execution links. These are local
//! consistency proofs, not provider flags or an additional signing authority.
use super::*;
use crate::product_store::{
    scope::{PreparedScopedChange, ScopedExecutionContext},
    ProjectSnapshot,
};

/// A host-only lowering receipt retaining the actual response, its authored
/// capture and the separately authored executable. Provider source locations
/// still name their original request bytes; this never relocates a JSON pointer.
#[derive(Clone, Debug)]
pub struct PreparedDiscoveryCandidate {
    request: DevelopmentRequest,
    result: DevelopmentResult,
    candidate_id: Id,
    authored: CapturedProgram,
    prepared: PreparedScopedChange,
    mappings: Vec<SemanticMapping>,
    basis: Digest,
}
impl PreparedDiscoveryCandidate {
    pub fn from_result(
        current: &ProjectSnapshot,
        request: &DevelopmentRequest,
        result: &DevelopmentResult,
        candidate_id: &str,
        prepared: PreparedScopedChange,
        mappings: Vec<SemanticMapping>,
    ) -> Result<Self, AdapterError> {
        request.validate()?;
        result.validate_for(request)?;
        if request.operation != DevelopmentOperation::Discover
            || request.sources.first() != Some(current.program().map_err(unavailable)?)
            || request.decisions != current.decisions
            || request.project_id != current.data.project_id
            || mappings.len() > MAX_ITEMS
        {
            return Err(invalid(
                "prepared discovery result does not name the current request basis",
            ));
        }
        let candidate = result
            .response
            .candidates
            .iter()
            .find(|c| c.id == candidate_id)
            .ok_or_else(|| {
                invalid("prepared discovery candidate is absent from the actual result")
            })?;
        let authored = CapturedProgram::capture(
            candidate.source_json.as_bytes(),
            &request.project_id,
            result.producer.clone(),
            None,
        )?;
        if &authored != prepared.candidate() {
            return Err(invalid(
                "prepared discovery authored capture differs from the actual result",
            ));
        }
        ScopedExecutionContext::prepared(current, &prepared)
            .map_err(unavailable)?
            .admit_target(prepared.target())
            .map_err(unavailable)?;
        Ok(Self {
            request: request.clone(),
            result: result.clone(),
            candidate_id: candidate_id.into(),
            authored,
            prepared,
            mappings,
            basis: canonical_digest(IdentityDomain::Data, current)?,
        })
    }
    pub fn authored(&self) -> &CapturedProgram {
        &self.authored
    }
    pub fn target(&self) -> &CapturedProgram {
        self.prepared.target()
    }
    pub fn result(&self) -> &DevelopmentResult {
        &self.result
    }
    pub fn request(&self) -> &DevelopmentRequest {
        &self.request
    }
    pub fn candidate_id(&self) -> &str {
        &self.candidate_id
    }
    pub(super) fn preparation(&self) -> &PreparedScopedChange {
        &self.prepared
    }
    pub(super) fn mappings(&self) -> &[SemanticMapping] {
        &self.mappings
    }
    pub(super) fn verify(
        &self,
        current: &ProjectSnapshot,
        request: &DevelopmentRequest,
        result: &DevelopmentResult,
    ) -> Result<(), AdapterError> {
        if request != &self.request
            || result != &self.result
            || canonical_digest(IdentityDomain::Data, current)? != self.basis
        {
            return Err(AdapterError::Stale(
                "prepared discovery request, actual result or frozen basis changed".into(),
            ));
        }
        Self::from_result(
            current,
            request,
            result,
            &self.candidate_id,
            self.prepared.clone(),
            self.mappings.clone(),
        )?;
        Ok(())
    }
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::Unsupported(format!(
        "Prepared discovery lowering is unverified: {error}"
    ))
}
