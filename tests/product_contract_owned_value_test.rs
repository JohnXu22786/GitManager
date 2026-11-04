#![allow(unexpected_cfgs)] // Explicit bounded standalone mode; ordinary Cargo imports the real source.
#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
include!("support/contract_owned_value_cases.rs");
