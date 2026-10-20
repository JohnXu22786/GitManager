//! Shared capture-only formula namespace; never grants validation authority.
use super::bounded_input::capture_payment_core as core;
use super::{ContractWorkError, ContractWorkResult, WorkFailure};

pub(crate) fn overflow() -> ContractWorkError {
    ContractWorkError::Resource(WorkFailure::BudgetExhausted(
        "capture work arithmetic overflow",
    ))
}
pub(crate) fn string_copy_bytes(n: usize) -> ContractWorkResult<u64> {
    core::string_copy_bytes(n).ok_or_else(overflow)
}
pub(crate) fn btree_levels(n: usize) -> ContractWorkResult<usize> {
    core::btree_levels(n).ok_or_else(overflow)
}
pub(crate) fn btree_search(n: usize, c: u64) -> ContractWorkResult<u64> {
    core::btree_search(n, c).ok_or_else(overflow)
}
pub(crate) fn btree_insert<K, V>(n: usize, c: u64) -> ContractWorkResult<u64> {
    core::btree_insert::<K, V>(n, c).ok_or_else(overflow)
}
pub(crate) fn btree_clone<K, V>(n: usize, payload: u64) -> ContractWorkResult<u64> {
    core::btree_clone::<K, V>(n, payload).ok_or_else(overflow)
}
pub(crate) fn btree_bulk<K, V>(n: usize, c: u64) -> ContractWorkResult<u64> {
    core::btree_bulk::<K, V>(n, c).ok_or_else(overflow)
}

/// Descriptor fuel is paid by the capture caller before this checked formula.
pub(crate) fn format_bytes(bytes: usize) -> ContractWorkResult<u64> {
    core::format_bytes(bytes).ok_or_else(overflow)
}
