//! Lossless boundary representation; domain identity and validation are unchanged.
#[path = "../tool_proposal_input.rs"]
mod input;
use crate::product_contract::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const ENCODING: &str = "gitmanager.development-request.exact-utf8";
const VERSION: u32 = 1;

/// Only exact raw UTF-8 sources use a different representation. This is not
/// provenance, consent or execution authority. Hosts embedding this DTO must
/// strictly parse their whole bounded envelope before deserializing it, then
/// call `to_request` before using the reconstructed domain request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactUtf8DevelopmentRequest {
    encoding: String,
    version: u32,
    request_digest: Digest,
    request: RequestV1,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestV1 {
    version: u32,
    id: Id,
    project_id: Id,
    operation: DevelopmentOperation,
    request: String,
    sources: Vec<SourceV1>,
    context: DevelopmentContext,
    examples: Vec<SelectedScenario>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    accepted_scenes: Vec<AcceptedSceneContext>,
    decisions: DecisionGraph,
    unknowns: Vec<UnknownBoundary>,
    required_capabilities: BTreeSet<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceV1 {
    artifact: ArtifactRef,
    binding: SourceBinding,
    source_utf8: String,
    program: AppDefinition,
}
impl ExactUtf8DevelopmentRequest {
    /// The caller still checks its complete prompt or storage envelope's cap.
    pub fn from_request(request: &DevelopmentRequest) -> Result<Self, ContractError> {
        let request_digest = request.identity()?;
        let sources = request
            .sources
            .iter()
            .map(|source| {
                Ok(SourceV1 {
                    artifact: source.artifact.clone(),
                    binding: source.binding.clone(),
                    source_utf8: String::from_utf8(source.source_bytes.clone())
                        .map_err(|error| ContractError(error.to_string()))?,
                    program: source.program.clone(),
                })
            })
            .collect::<Result<_, ContractError>>()?;
        Ok(Self {
            encoding: ENCODING.into(),
            version: VERSION,
            request_digest,
            request: RequestV1 {
                version: request.version,
                id: request.id.clone(),
                project_id: request.project_id.clone(),
                operation: request.operation,
                request: request.request.clone(),
                sources,
                context: request.context.clone(),
                examples: request.examples.clone(),
                accepted_scenes: request.accepted_scenes.clone(),
                decisions: request.decisions.clone(),
                unknowns: request.unknowns.clone(),
                required_capabilities: request.required_capabilities.clone(),
            },
        })
    }

    pub fn to_request(&self) -> Result<DevelopmentRequest, ContractError> {
        if self.encoding != ENCODING || self.version != VERSION {
            return Err(ContractError(
                "unsupported development request encoding or version".into(),
            ));
        }
        let request = DevelopmentRequest {
            version: self.request.version,
            id: self.request.id.clone(),
            project_id: self.request.project_id.clone(),
            operation: self.request.operation,
            request: self.request.request.clone(),
            sources: self
                .request
                .sources
                .iter()
                .map(|source| CapturedProgram {
                    artifact: source.artifact.clone(),
                    binding: source.binding.clone(),
                    source_bytes: source.source_utf8.as_bytes().to_vec(),
                    program: source.program.clone(),
                })
                .collect(),
            context: self.request.context.clone(),
            examples: self.request.examples.clone(),
            accepted_scenes: self.request.accepted_scenes.clone(),
            decisions: self.request.decisions.clone(),
            unknowns: self.request.unknowns.clone(),
            required_capabilities: self.request.required_capabilities.clone(),
        };
        // identity() validates all source bytes/ASTs, bindings, counts, text,
        // decisions and selected scenes before binding the original domain.
        if request.identity()? != self.request_digest {
            return Err(ContractError(
                "development request identity mismatch".into(),
            ));
        }
        Ok(request)
    }

    /// Standalone intake uses the existing byte, depth and duplicate-key gates.
    pub fn parse(bytes: &[u8]) -> Result<Self, ContractError> {
        let value =
            input::parse_json_bytes(bytes).map_err(|error| ContractError(error.to_string()))?;
        let encoded: Self =
            serde_json::from_value(value).map_err(|error| ContractError(error.to_string()))?;
        encoded.to_request()?;
        Ok(encoded)
    }
}
