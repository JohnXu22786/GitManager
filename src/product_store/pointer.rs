//! The actual persisted Pointer, shared by the store and the isolated codec test.
use crate::product_contract::{Digest, Id};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pointer {
    pub(super) magic: String,
    pub(super) version: u32,
    pub(super) project_id: Id,
    pub(super) revision: u64,
    pub(super) object: Digest,
}
