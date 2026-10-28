use serde::{Deserialize, Serialize};

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
