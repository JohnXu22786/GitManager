#![allow(unexpected_cfgs)] // Explicit bounded standalone mode; ordinary Cargo imports the real source.
#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
include!("support/contract_owned_value_cases.rs");
