//! Offline controller regressions. No live AI, native GUI, or user acceptance proof.
#[path = "../src/harness.rs"]
mod harness;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_export.rs"]
mod product_export;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/ui/product_runtime_view.rs"]
pub(crate) mod product_runtime_view;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_sources/mod.rs"]
mod product_sources;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/task_delivery.rs"]
mod task_delivery;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "../src/product_studio.rs"]
mod product_studio;
use product_studio::rule_trace_testing;
#[test]
fn imported_trace_keeps_observations_and_created_ids() {
    rule_trace_testing::imported_trace_keeps_observations_and_created_ids();
}
#[test]
fn trial_keeps_an_existing_result_observation() {
    rule_trace_testing::trial_keeps_an_existing_result_observation();
}
#[test]
fn failing_or_cancelled_trial_clears_old_authority() {
    rule_trace_testing::failing_or_cancelled_trial_clears_old_authority();
}

#[test]
fn import_rejects_changed_selection_and_preparation() {
    rule_trace_testing::import_rejects_changed_selection_and_preparation();
}

#[test]
fn import_rejects_corrupted_prepared_frames() {
    rule_trace_testing::import_rejects_corrupted_prepared_frames();
}

#[test]
fn imported_either_acceptable_reopens_after_later_work() {
    rule_trace_testing::imported_either_acceptable_reopens_after_later_work();
}

#[test]
fn imported_both_needed_reopens_after_later_work() {
    rule_trace_testing::imported_both_needed_reopens_after_later_work();
}

#[test]
fn imported_neither_fits_reopens_after_later_work() {
    rule_trace_testing::imported_neither_fits_reopens_after_later_work();
}

#[test]
fn imported_deferred_reopens_after_later_work() {
    rule_trace_testing::imported_deferred_reopens_after_later_work();
}

#[test]
fn imported_keep_current_retains_committed_current() {
    rule_trace_testing::imported_keep_current_retains_committed_current();
}

#[test]
fn imported_accept_adopts_freshly_prepared_rule() {
    rule_trace_testing::imported_accept_adopts_freshly_prepared_rule();
}

#[test]
fn actionless_minimum_imports_only_checked_original_without_broadening_scope() {
    rule_trace_testing::actionless_minimum_imports_only_checked_original_without_broadening_scope();
}

#[test]
fn unverified_synthetic_frame_cannot_create_copy_authority() {
    rule_trace_testing::unverified_synthetic_frame_cannot_create_copy_authority();
}
