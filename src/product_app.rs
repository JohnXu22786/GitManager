//! Parallel product-owned schema; UNROUTED (R05a-d, R06c).
//! All 25 types, 64 variants, 161 fields. App byte decoding is structural and
//! UNVALIDATED. Managed fallible deep copy/prepared equality are separate.
//! Borrowed fallible canonical encoding preserves all fields and labels.
//! No validation, semantic projection, Shared owner, paid-work
//! accounting or capture/runtime/store routing guarantee here.
//! Decode byte entrypoints enforce product_json's 1 MiB / 64-container bounds.
//! Encoding has no parser-depth/byte cap and borrows raw types or managed owners.
//! Decoder descendants share one explicit AllocationContext. No legacy/Serde
//! conversion or allocation-failure fallback. The sealed Value decoder is
//! private; only tests can inject arbitrary manually built Value trees.
//! Diagnostic float spelling alone uses the existing serde_json Number display
//! under pinned default/std WITHOUT arbitrary_precision; see error::JsonFloat.
#[path = "product_app/error.rs"] mod error;
#[path = "product_app/decode.rs"] mod decode;
#[path = "product_app/schema_core.rs"] mod schema_core;
#[path = "product_app/schema_expr.rs"] mod schema_expr;
#[path = "product_app/schema_app.rs"] mod schema_app;
pub use error::{AppDecodeError, Expected, SchemaFailure, UnexpectedOwned};
pub use schema_core::*;
pub use schema_expr::*;
pub use schema_app::*;
#[cfg(test)]
pub(crate) use decode::{decode_value, DecodeOwned, InputMode};

#[path = "product_app/owned_graph.rs"] mod owned_graph;
#[path = "product_app/copy.rs"] mod copy;
#[path = "product_app/equality.rs"] mod equality;
pub use owned_graph::OwnedAppGraph;
pub(crate) use owned_graph::ManagedListBuilder;
pub use equality::PreparedAppEq;
#[cfg(test)]
pub(crate) use owned_graph::{BORROWED_RECORD_BYTES,OWNED_NODE_BYTES};

#[path = "product_app/encode.rs"] mod encode;
#[cfg(test)]
pub(crate) use encode::FRAME_BYTES as ENCODE_FRAME_BYTES;
