//! Opaque development transport contracts; no application schema or proof type.
use ring::digest::{digest as hash, SHA256};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PROMPT_BYTES: usize = 128 * 1024;
pub const MAX_SCHEMA_BYTES: usize = 32 * 1024;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub const RECEIPT_VERSION: u32 = 1;

pub fn digest(bytes: &[u8]) -> String {
    hash(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub(crate) fn encoded_digest<T: Serialize>(value: &T) -> String {
    digest(&serde_json::to_vec(value).expect("transport structs serialize"))
}
pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
}
impl ProviderKind {
    pub fn recipient(self) -> &'static str {
        match self {
            Self::Codex => "OpenAI via the existing Codex subscription",
            Self::Claude => "Anthropic via the existing Claude subscription",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityProfile {
    /// No model tools, MCP, hooks, plugins or host context discovery.
    DataOnly,
    /// Separately disclosed provider-managed restrictions; NOT a data sandbox.
    TrustedHarness,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationProvenance {
    LiveCli,
    TransportFixture,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Missing,
    UnsupportedVersion,
    UnsupportedCapability,
    UnsupportedPlatform,
    AuthRequired,
    ProbeFailed,
    ReadyUntested,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapabilities {
    pub provider: ProviderKind,
    pub readiness: Readiness,
    pub executable_digest: String,
    pub version: String,
    pub probe_digest: String,
    pub data_only: bool,
    pub trusted_harness: bool,
    pub detail: String,
    pub provenance: InvocationProvenance,
}
impl ProviderCapabilities {
    pub fn digest(&self) -> String {
        encoded_digest(self)
    }
    pub fn supports(&self, profile: CapabilityProfile) -> bool {
        self.readiness == Readiness::ReadyUntested
            && match profile {
                CapabilityProfile::DataOnly => self.data_only,
                CapabilityProfile::TrustedHarness => self.trusted_harness,
            }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobLimits {
    pub timeout_ms: u64,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub result_bytes: usize,
}
impl Default for JobLimits {
    fn default() -> Self {
        Self {
            timeout_ms: 120_000,
            stdout_bytes: 2 * 1024 * 1024,
            stderr_bytes: 64 * 1024,
            result_bytes: MAX_RESULT_BYTES,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequest {
    pub request_id: String,
    pub provider: ProviderKind,
    /// Caller-owned identity of the source/context, never a local path.
    pub source_digest: String,
    pub prompt: Vec<u8>,
    pub schema: Vec<u8>,
    pub purpose: String,
    pub data_categories: Vec<String>,
    pub profile: CapabilityProfile,
    pub limits: JobLimits,
}
impl ProviderRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.request_id) || !valid_digest(&self.source_digest) {
            return Err("invalid request or source identity".into());
        }
        if self.prompt.is_empty()
            || self.prompt.len() > MAX_PROMPT_BYTES
            || std::str::from_utf8(&self.prompt).is_err()
        {
            return Err("prompt must be bounded nonempty UTF-8".into());
        }
        if self.schema.len() > MAX_SCHEMA_BYTES {
            return Err("schema exceeds transport limit".into());
        }
        relocated_schema(&self.schema)?;
        if self.purpose.is_empty()
            || self.purpose.len() > 1024
            || self.data_categories.is_empty()
            || self.data_categories.len() > 32
            || self
                .data_categories
                .iter()
                .any(|s| s.is_empty() || s.len() > 256)
        {
            return Err("bounded data disclosure is required".into());
        }
        let limit = &self.limits;
        if limit.timeout_ms == 0
            || limit.timeout_ms > 900_000
            || limit.stdout_bytes == 0
            || limit.stdout_bytes > 2 * 1024 * 1024
            || limit.stderr_bytes == 0
            || limit.stderr_bytes > 64 * 1024
            || limit.result_bytes == 0
            || limit.result_bytes > MAX_RESULT_BYTES
        {
            return Err("invalid job limits".into());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        Ok(encoded_digest(self))
    }
}
/// Only schema-valued locations are traversed. Literal instance values under
/// const/enum/default/examples, and property names, must remain byte-semantic.
pub(crate) fn relocated_schema(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let mut schema = super::input::parse_json_bytes(bytes).map_err(|e| e.to_string())?;
    if !schema.is_object() {
        return Err("schema root must be an object".into());
    }
    fn visit(schema: &mut serde_json::Value) -> Result<(), String> {
        let Some(map) = schema.as_object_mut() else {
            return Ok(());
        };
        for (key, value) in map {
            match key.as_str() {
                "$ref" | "$dynamicRef" => {
                    let reference = value.as_str().ok_or("schema reference must be a string")?;
                    if reference != "#" && !reference.starts_with("#/") {
                        return Err("only root-local JSON-pointer references are supported".into());
                    }
                    *value = serde_json::Value::String(format!(
                        "#/properties/payload{}",
                        &reference[1..]
                    ));
                }
                "$id" | "$anchor" | "$dynamicAnchor" | "$recursiveRef" | "$recursiveAnchor" => {
                    return Err("schema resource IDs, anchors and legacy recursive references are unsupported".into())
                }
                "$defs" | "definitions" | "properties" | "patternProperties"
                | "dependentSchemas" | "dependencies" => {
                    if let Some(children) = value.as_object_mut() {
                        for child in children.values_mut() {
                            visit(child)?;
                        }
                    }
                }
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    if let Some(children) = value.as_array_mut() {
                        for child in children {
                            visit(child)?;
                        }
                    }
                }
                "items" => {
                    if let Some(children) = value.as_array_mut() {
                        for child in children {
                            visit(child)?;
                        }
                    } else {
                        visit(value)?;
                    }
                }
                "additionalItems"
                | "additionalProperties"
                | "unevaluatedProperties"
                | "unevaluatedItems"
                | "propertyNames"
                | "contains"
                | "not"
                | "if"
                | "then"
                | "else"
                | "contentSchema" => visit(value)?,
                _ => (),
            }
        }
        Ok(())
    }
    visit(&mut schema)?;
    Ok(schema)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataDisclosure {
    pub request_id: String,
    pub nonce: String,
    pub provider: ProviderKind,
    pub recipient: String,
    pub request_digest: String,
    pub source_digest: String,
    pub prompt_digest: String,
    pub schema_digest: String,
    pub wire_schema_digest: String,
    pub stdin_digest: String,
    pub capability_digest: String,
    pub profile: CapabilityProfile,
    pub purpose: String,
    pub data_categories: Vec<String>,
    pub capability_notice: String,
    pub billing_notice: String,
    pub environment_digest: String,
    pub command_digest: String,
}
impl DataDisclosure {
    pub fn digest(&self) -> String {
        encoded_digest(self)
    }
}
/// A trusted controller supplies this only after actual user authorization.
/// A hash match binds scope, not identity: this library is NOT an approval UI.
/// Never populate this from model output, saved login state or a fixture receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentReceipt {
    pub disclosure_digest: String,
    pub approval_reference: String,
    pub expires_at_unix_ms: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Prepared,
    Running,
    TransportValidated,
    Cancelled,
    TimedOut,
    Interrupted,
    Quota,
    NetworkError,
    AuthRequired,
    AuthorizationExpired,
    ProviderError,
    InvalidOutput,
    OutputLimit,
}
impl JobState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Prepared | Self::Running)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobReceipt {
    pub version: u32,
    pub request_id: String,
    pub request_digest: String,
    pub source_digest: String,
    pub provider: ProviderKind,
    pub provenance: InvocationProvenance,
    pub capabilities: ProviderCapabilities,
    pub disclosure: DataDisclosure,
    pub consent: Option<ConsentReceipt>,
    pub state: JobState,
    pub started_at_unix_ms: Option<u64>,
    pub finished_at_unix_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub session_id: Option<String>,
    pub result_digest: Option<String>,
    pub stdout_digest: Option<String>,
    pub stderr_digest: Option<String>,
    pub usage: Option<serde_json::Value>,
    pub detail: String,
}
#[derive(Debug, Clone)]
pub struct ProviderResult {
    /// Syntax and correlation checked only. Domain validation and independent
    /// execution MUST precede adoption or any claim of behavior correctness.
    pub final_bytes: Vec<u8>,
    pub receipt: JobReceipt,
}
