// TEST-ONLY frozen original lines108-167; allocating oracle, never a fallback.
// Imports/error/require below and digest_bytes wrapper are validation scaffolding.
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
#[derive(Debug)]
pub struct ContractError(pub String);
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
type Result<T> = std::result::Result<T, ContractError>;
fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition { Ok(()) } else { Err(ContractError(message.into())) }
}
// BEGIN VERBATIM ORIGINAL
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest(String);
impl Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Digest {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self> {
        require(
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid SHA-256 digest",
        )?;
        Ok(Self(value))
    }
}
impl From<Digest> for String {
    fn from(value: Digest) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityDomain {
    Source,
    Program,
    SemanticProgram,
    Data,
    Schema,
    Input,
    Session,
    Observation,
    Output,
    OutputBytes,
    Request,
    Scenario,
    Decision,
    Evolution,
    Adoption,
    Evidence,
}
fn bytes_digest(domain: IdentityDomain, bytes: &[u8]) -> Digest {
    let mut input = b"gitmanager.product.v1\0".to_vec();
    input.extend(serde_json::to_vec(&domain).expect("finite enum"));
    input.push(0);
    input.extend(bytes);
    Digest(
        digest(&SHA256, &input)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
// END VERBATIM ORIGINAL
pub fn digest_bytes(domain: IdentityDomain, bytes: &[u8]) -> Digest {
    bytes_digest(domain, bytes)
}
