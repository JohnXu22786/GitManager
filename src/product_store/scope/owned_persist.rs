//! Whole-family reconstruction. All disk recipes are untrusted until the full
//! ordered source/decision/receipt chain and every retained recipe regenerate.
use super::common_fact_admission::{refuse, ProofMeter};
use super::*;
use std::sync::{atomic::AtomicBool, Arc};

pub(crate) use super::owned_epoch::VerifiedOwnedTransitionExecution;
pub(super) use super::owned_epoch::{OwnedExecutionRecipe, OwnedProducerEpoch};
pub(super) struct ReconstructedOwned {
    pub snapshot: Digest,
    pub recipes: BTreeMap<Digest, OwnedExecutionRecipe>,
    pub epochs: Vec<OwnedProducerEpoch>,
    pub proofs: BTreeMap<Digest, Arc<PreparedOwnedPreview>>,
    pub manifests: BTreeMap<Digest, OwnedCompositionV2>,
    pub initializations: BTreeMap<Digest, OwnedInitializationV2>,
}

// V11 producing fixed packets. Each is checked over the complete lowered
// owner/Result/argument/cleanup layouts; all prior payment families remain.
// Complete producing fixed slots: 54; direct activation/control sites: 18.
pub(super) const fn historical_initial_callback_payment<C>() -> u64 {
    comparison_payment(1152, [
        (size_of::<&mut C>(), 16),
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 24),
        (size_of::<&mut crate::product_store::scope::OwnedPolicyCheck<'_>>(), 8),
        (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 24),
        (size_of::<&crate::product_store::scope::HistoricalInitialLoan<'_>>(), 16),
        (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 8),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 8),
        (size_of::<(&mut crate::product_store::scope::OwnedPolicyCheck<'_>, &crate::product_store::scope::HistoricalInitialLoan<'_>)>(), 4),
        (size_of::<()>(), 24),
        (size_of::<isize>(), 12),
        (size_of::<crate::product_store::scope::HistoricalInitialLoan<'_>>(), 4),
        (size_of::<crate::product_store::scope::OwnedPolicyCheck<'_>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 12),
        (size_of::<std::option::Option<&crate::product_store::scope::owned_persist::ReconstructedOwned>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 16),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 24),
        (size_of::<u64>(), 8),
    ])
}
// Complete producing fixed slots: 105; direct activation/control sites: 47.
// Includes the new exact error String boundary and checked 81-byte literal copy.
pub(crate) const HISTORICAL_INITIAL_SURFACE_PAYMENT: u64 = comparison_payment(3008, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 32),
        (size_of::<&mut crate::product_store::scope::OwnedPolicyCheck<'_>>(), 12),
        (size_of::<&crate::product_contract::CapturedProgram>(), 24),
        (size_of::<&crate::product_contract::Digest>(), 8),
        (size_of::<&crate::product_store::ProjectSnapshot>(), 8),
        (size_of::<&crate::product_store::scope::owned_continuation::VerifiedOwnedContinuation>(), 8),
        (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 24),
        (size_of::<&crate::product_store::scope::HistoricalInitialLoan<'_>>(), 20),
        (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 8),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedLayerV2>(), 12),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedScopeStateV2>(), 8),
        (size_of::<&std::collections::BTreeMap<crate::product_contract::Digest, crate::product_store::scope::owned_wire::OwnedLayerV2>>(), 4),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_continuation::VerifiedOwnedContinuation>>(), 4),
        (size_of::<&str>(), 8),
        (size_of::<()>(), 32),
        (size_of::<*const crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 8),
        (size_of::<bool>(), 8),
        (size_of::<isize>(), 28),
        (size_of::<crate::product_store::StoreError>(), 4),
        (size_of::<crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface>(), 4),
        (size_of::<crate::product_store::scope::owned_persist::ReconstructedOwned>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, &crate::product_store::scope::owned_wire::OwnedLayerV2>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, &crate::product_store::scope::owned_wire::OwnedScopeStateV2>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_persist::ReconstructedOwned>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 12),
        (size_of::<std::option::Option<&crate::product_store::scope::owned_wire::OwnedLayerV2>>(), 4),
        (size_of::<std::result::Result<&crate::product_store::scope::owned_wire::OwnedLayerV2, crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<&crate::product_store::scope::owned_wire::OwnedScopeStateV2, crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 12),
        (size_of::<std::result::Result<crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface, crate::product_store::StoreError>>(), 12),
        (size_of::<std::result::Result<crate::product_store::scope::owned_persist::ReconstructedOwned, crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 56),
        (size_of::<std::string::String>(), 8),
        (size_of::<u64>(), 12),
        ("Common factual admission: owned policy: historical Initial loan execution differs".len(), 1),
    ]);
// Complete producing fixed slots: 19; direct activation/control sites: 7.
const HISTORICAL_INITIAL_WORKER_PAYMENT: u64 = comparison_payment(448, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 8),
        (size_of::<&crate::product_store::ProjectSnapshot>(), 4),
        (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 8),
        (size_of::<&crate::product_store::scope::HistoricalInitialLoan<'_>>(), 4),
        (size_of::<()>(), 16),
        (size_of::<isize>(), 4),
        (size_of::<crate::product_store::scope::owned_persist::RegistryOnly>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 8),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<crate::product_store::scope::owned_persist::ReconstructedOwned, crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 8),
    ]);
// Complete producing fixed slots: 160; direct activation/control sites: 64.
const HISTORICAL_INITIAL_SLOT_PAYMENT: u64 = comparison_payment(4096, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 48),
        (size_of::<&crate::product_contract::Digest>(), 20),
        (size_of::<&crate::product_store::ProjectSnapshot>(), 12),
        (size_of::<&crate::product_store::scope::owned_persist::GraphHistory>(), 16),
        (size_of::<&crate::product_store::scope::HistoricalInitialLoan<'_>>(), 12),
        (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialPrepared>(), 4),
        (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4),
        (size_of::<&crate::product_store::scope::owned_preview::PreparedOwnedPreview>(), 12),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedCompositionV2>(), 12),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedInitializationV2>(), 8),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedLayerV2>(), 8),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 24),
        (size_of::<&std::time::Duration>(), 4),
        (size_of::<&std::time::Instant>(), 4),
        (size_of::<()>(), 48),
        (size_of::<bool>(), 40),
        (size_of::<isize>(), 68),
        (size_of::<crate::product_contract::RuntimeLimits>(), 4),
        (size_of::<crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>(), 4),
        (size_of::<crate::product_store::scope::owned_persist::HistoricalInitialPrepared>(), 8),
        (size_of::<crate::product_store::scope::owned_preview::PreparedOwnedPreview>(), 16),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, bool>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 20),
        (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>(), 20),
        (size_of::<std::option::Option<crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>>(), 4),
        (size_of::<std::option::Option<u128>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 20),
        (size_of::<std::result::Result<bool, crate::product_store::StoreError>>(), 8),
        (size_of::<std::result::Result<crate::product_store::scope::owned_preview::PreparedOwnedPreview, crate::product_store::StoreError>>(), 8),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 80),
        (size_of::<std::result::Result<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>, crate::product_store::StoreError>>(), 12),
        (size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 16),
        (size_of::<std::time::Duration>(), 4),
        (size_of::<u128>(), 20),
        (size_of::<u64>(), 28),
    ]);
// Complete producing fixed slots: 11; direct activation/control sites: 6.
const HISTORICAL_INITIAL_SCAN_PAYMENT: u64 = comparison_payment(384, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4),
        (size_of::<&std::collections::BTreeMap<crate::product_contract::Digest, crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 4),
        (size_of::<()>(), 8),
        (size_of::<isize>(), 4),
        (size_of::<std::collections::btree_map::Iter<'_, crate::product_contract::Digest, crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 8),
        (size_of::<u64>(), 4),
    ]);
// Complete producing fixed slots: 15; direct activation/control sites: 7.
const HISTORICAL_INITIAL_NEXT_PAYMENT: u64 = comparison_payment(448, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4),
        (size_of::<&mut std::collections::btree_map::Iter<'_, crate::product_contract::Digest, crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 4),
        (size_of::<&crate::product_contract::Digest>(), 4),
        (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 4),
        (size_of::<()>(), 8),
        (size_of::<isize>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>(), 4),
        (size_of::<std::option::Option<(&crate::product_contract::Digest, &crate::product_store::scope::owned_epoch::OwnedExecutionRecipe)>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 8),
        (size_of::<u64>(), 4),
    ]);
// Complete producing fixed slots: 41; direct activation/control sites: 24.
const HISTORICAL_INITIAL_COMPARE_PAYMENT: u64 = comparison_payment(1536, [
        (size_of::<&&crate::product_contract::Digest>(), 8),
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 8),
        (size_of::<&crate::product_contract::Digest>(), 8),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 8),
        (size_of::<&str>(), 16),
        (size_of::<()>(), 16),
        (size_of::<bool>(), 4),
        (size_of::<isize>(), 12),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 8),
        (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>(), 4),
        (size_of::<std::option::Option<u64>>(), 8),
        (size_of::<std::option::Option<usize>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 8),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 16),
        (size_of::<std::result::Result<u64, std::num::TryFromIntError>>(), 4),
        (size_of::<u64>(), 8),
        (size_of::<usize>(), 16),
    ]);
// Complete producing fixed slots: 24; direct activation/control sites: 10.
const HISTORICAL_INITIAL_MINT_PAYMENT: u64 = comparison_payment(640, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 8),
        (size_of::<&crate::product_store::ProjectSnapshot>(), 4),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 8),
        (size_of::<()>(), 16),
        (size_of::<isize>(), 8),
        (size_of::<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 8),
        (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 8),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 16),
        (size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4),
        (size_of::<u64>(), 8),
    ]);
// Complete producing fixed slots: 74; direct activation/control sites: 23.
const HISTORICAL_INITIAL_TRANSITION_PAYMENT: u64 = comparison_payment(1472, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 16),
        (size_of::<&mut std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4),
        (size_of::<&mut std::option::Option<crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>>(), 4),
        (size_of::<&crate::product_store::ProjectSnapshot>(), 40),
        (size_of::<&crate::product_store::scope::contract::ScopeTransition>(), 12),
        (size_of::<&crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>(), 12),
        (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 20),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedCompositionV2>(), 12),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedInitializationV2>(), 12),
        (size_of::<&crate::product_store::scope::owned_wire::OwnedLayerV2>(), 12),
        (size_of::<&std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4),
        (size_of::<&std::option::Option<crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>>(), 4),
        (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4),
        (size_of::<()>(), 16),
        (size_of::<bool>(), 4),
        (size_of::<isize>(), 16),
        (size_of::<crate::product_store::scope::contract::ScopeTransition>(), 4),
        (size_of::<crate::product_store::scope::owned_continuation::VerifiedOwnedContinuation>(), 4),
        (size_of::<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 12),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::option::Option<&crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4),
        (size_of::<std::option::Option<&crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>>(), 4),
        (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 8),
        (size_of::<std::option::Option<crate::product_store::scope::owned_persist::FreshInitialOrigin<'_>>>(), 8),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution, crate::product_store::StoreError>>(), 12),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 24),
        (size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4),
        (size_of::<u64>(), 4),
    ]);
// Preserve every old transition component; prepay the additional view before
// take, selected activation, tuple construction or any new borrowed owner.
const HISTORICAL_INITIAL_TRANSITION_WITH_VIEW_PAYMENT: u64 =
    HISTORICAL_INITIAL_TRANSITION_PAYMENT
        .checked_add(owned_continuation::COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT)
        .expect("completed transition view payment overflow");
// Complete producing fixed slots: 14; direct activation/control sites: 7.
const HISTORICAL_INITIAL_ARC_PAYMENT: u64 = comparison_payment(448, [
        (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4),
        (size_of::<()>(), 8),
        (size_of::<isize>(), 8),
        (size_of::<crate::product_store::scope::owned_preview::PreparedOwnedPreview>(), 8),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 8),
        (size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 8),
        (size_of::<u64>(), 4),
    ]);

// Only the completed outer checker below mints this pair. No parts factory,
// registry getter, Clone/Copy, or source-selected historical endpoint exists.
pub(crate) struct HistoricalInitialLoan<'gate> {
    registry: &'gate ReconstructedOwned,
    execution: &'gate VerifiedOwnedTransitionExecution,
}
// V12 complete fixed transport packets. All old floors remain unchanged;
// the whole duplicate native interval is removed only by its one-use proof.
// I0: 53 actual full typed slots; 60 operations, including both worker-entry cleanup flags.
const HISTORICAL_INITIAL_INTERVAL_ENTRY_PAYMENT: u64 = comparison_payment(64_u64.checked_mul(60).expect("interval operation count overflow"), [
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(6).expect("interval slot count overflow")),
    (size_of::<&crate::product_contract::CapturedProgram>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 4_u64.checked_mul(3).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 4_u64.checked_mul(3).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_wire::OwnedLayerV2>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<bool>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<()>(), 4_u64.checked_mul(7).expect("interval slot count overflow")),
    (size_of::<isize>(), 4_u64.checked_mul(3).expect("interval slot count overflow")),
    (size_of::<crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<crate::product_store::scope::owned_persist::RegistryOnly>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_persist::ReconstructedOwned>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::result::Result<crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface, crate::product_store::StoreError>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::result::Result<crate::product_store::scope::owned_persist::ReconstructedOwned, crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 4_u64.checked_mul(6).expect("interval slot count overflow")),
    (size_of::<u64>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
]);
// I1: 15 actual full typed slots; 18 paid fixed activation/control operations.
const HISTORICAL_INITIAL_INTERVAL_MINT_PAYMENT: u64 = comparison_payment(64_u64.checked_mul(18).expect("interval operation count overflow"), [
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<()>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<isize>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<u64>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
]);
// I2: 66 full typed slots; 94 operations, including cause materialization, Err construction and later flag control.
const HISTORICAL_INITIAL_INTERVAL_SELECT_PAYMENT: u64 = comparison_payment(64_u64.checked_mul(94).expect("interval operation count overflow"), [
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(5).expect("interval slot count overflow")),
    (size_of::<&mut std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&crate::product_contract::DataSnapshot>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&std::time::Duration>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<&std::time::Instant>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<()>(), 4_u64.checked_mul(5).expect("interval slot count overflow")),
    (size_of::<bool>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<i32>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<isize>(), 4_u64.checked_mul(7).expect("interval slot count overflow")),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>(), 4_u64.checked_mul(2).expect("interval slot count overflow")),
    (size_of::<std::option::Option<u128>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<crate::product_store::StoreError>(), 4_u64.checked_mul(6).expect("interval slot count overflow")),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 4_u64.checked_mul(8).expect("interval slot count overflow")),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4_u64.checked_mul(4).expect("interval slot count overflow")),
    (size_of::<std::time::Duration>(), 4_u64.checked_mul(1).expect("interval slot count overflow")),
    (size_of::<u128>(), 4_u64.checked_mul(5).expect("interval slot count overflow")),
    (size_of::<u64>(), 4_u64.checked_mul(8).expect("interval slot count overflow")),
]);

// Only the selected committed Initial clone below mints this distinct one-use
// interval authority. It cannot escape, expose parts, or represent a row cache.
struct CompletedInitialInterval<'loan, 'outer> {
    loan: &'loan HistoricalInitialLoan<'outer>,
    original: &'loan Arc<PreparedOwnedPreview>,
}

#[cfg(test)]
pub(super) const HISTORICAL_INITIAL_INTERVAL_CENSUS: ([usize; 10], [u64; 3]) = {
    #[cfg(target_pointer_width = "64")]
    {
    assert!(size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>() == 8);
    assert!(size_of::<&mut std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>() == 8);
    assert!(size_of::<&crate::product_contract::CapturedProgram>() == 8);
    assert!(size_of::<&crate::product_contract::DataSnapshot>() == 8);
    assert!(size_of::<&crate::product_store::ProjectSnapshot>() == 8);
    assert!(size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>() == 8);
    assert!(size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>() == 8);
    assert!(size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>() == 8);
    assert!(size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>() == 8);
    assert!(size_of::<&crate::product_store::scope::owned_wire::OwnedLayerV2>() == 8);
    assert!(size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>() == 8);
    assert!(size_of::<&std::time::Duration>() == 8);
    assert!(size_of::<&std::time::Instant>() == 8);
    assert!(size_of::<()>() == 0);
    assert!(size_of::<*const crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>() == 8);
    assert!(size_of::<bool>() == 1);
    assert!(size_of::<i32>() == 4);
    assert!(size_of::<isize>() == 8);
    assert!(size_of::<crate::product_contract::RuntimeLimits>() == 48);
    assert!(size_of::<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>() == 16);
    assert!(size_of::<crate::product_store::scope::owned_persist::ReconstructedOwned>() == 144);
    assert!(size_of::<crate::product_store::scope::owned_persist::RegistryOnly>() == 0);
    assert!(size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, bool>>() == 152);
    assert!(size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, crate::product_store::scope::owned_persist::ReconstructedOwned>>() == 152);
    assert!(size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>() == 152);
    assert!(size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialInterval<'_, '_>>>() == 16);
    assert!(size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedInitialOrigin<'_, '_>>>() == 16);
    assert!(size_of::<std::option::Option<u128>>() == 32);
    assert!(size_of::<std::result::Result<(), crate::product_store::StoreError>>() == 152);
    assert!(size_of::<std::result::Result<bool, crate::product_store::StoreError>>() == 152);
    assert!(size_of::<std::result::Result<crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface, crate::product_store::StoreError>>() == 240);
    assert!(size_of::<std::result::Result<crate::product_store::scope::owned_persist::ReconstructedOwned, crate::product_store::StoreError>>() == 152);
    assert!(size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>() == 152);
    assert!(size_of::<std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>() == 8);
    assert!(size_of::<std::time::Duration>() == 16);
    assert!(size_of::<u128>() == 16);
    assert!(size_of::<u64>() == 8);
    assert!(size_of::<usize>() == 8);
    }
    ([
    size_of::<CompletedInitialInterval<'_, '_>>(),
    size_of::<Option<CompletedInitialInterval<'_, '_>>>(),
    size_of::<Result<ReconstructedOwned>>(),
    size_of::<Result<owned_continuation::VerifiedOwnedPolicySurface>>(),
    size_of::<Result<bool>>(),
    size_of::<Result<()>>(),
    size_of::<Result<std::convert::Infallible>>(),
    size_of::<Option<u128>>(),
    size_of::<RuntimeLimits>(),
    size_of::<owned_transition::OwnedPolicyCheck<'_>>(),
], [HISTORICAL_INITIAL_INTERVAL_ENTRY_PAYMENT,
    HISTORICAL_INITIAL_INTERVAL_MINT_PAYMENT,
    HISTORICAL_INITIAL_INTERVAL_SELECT_PAYMENT])
};

pub(super) struct CompletedInitialOrigin<'h, 'o> {
    retained: &'h ProjectSnapshot,
    original: &'o Arc<PreparedOwnedPreview>,
}
impl CompletedInitialOrigin<'_, '_> {
    // Both returned borrows are tied to this unforgeable completed owner.
    pub(super) fn references(&self) -> (&ProjectSnapshot, &Arc<PreparedOwnedPreview>) {
        (self.retained, self.original)
    }
}
// Preserve the native prospective Arc-allocation position after bind/seed checks.
// A hit borrows until that position; a miss still owns the native prepared value.
enum HistoricalInitialPrepared {
    Completed(Arc<PreparedOwnedPreview>),
    Native(PreparedOwnedPreview),
}

// Sole mint: the consumed first committed Initial origin below. Neither raw
// registry callers nor an equal-looking deserialized value can construct this.
pub(super) struct CompletedStructuralLoan<'loan, 'retained, 'original> {
    origin: &'loan CompletedInitialOrigin<'retained, 'original>,
    donor: &'loan Arc<VerifiedOwnedTransitionExecution>,
}
impl CompletedStructuralLoan<'_, '_, '_> {
    pub(super) fn references(&self) -> (&ProjectSnapshot, &Arc<VerifiedOwnedTransitionExecution>) {
        (self.origin.retained, self.donor)
    }
}

pub(crate) fn check_retained_transition_policies_with_initials<C>(
    snapshot: &ProjectSnapshot, mut check: C,
) -> Result<()>
where C: for<'gate, 'loan> FnMut(&mut owned_transition::OwnedPolicyCheck<'gate>, &HistoricalInitialLoan<'loan>) -> Result<()>,
{
    if !snapshot.owned_scope().is_ok_and(|state| {
        state.compositions.values().any(|m| {
            m.lowering == OWNED_TRANSITION_LOWERING_VERSION
                && m.transition == ScopeTransition::Withdrawal
        })
    }) { return Ok(()); }
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::ArchiveReconstruct);
    let registry = reconstruct(snapshot, &mut meter)?;
    #[cfg(test)]
    adoption_diagnostic::phase(adoption_diagnostic::Phase::ArchivePolicy);
    for execution in registry.checked_withdrawals() {
        meter.charge(1)?;
        meter.charge(6)?;
        meter.charge(const { historical_initial_callback_payment::<C>() })?;
        let loan = HistoricalInitialLoan { registry: &registry, execution };
        #[cfg(test)]
        combined_transition_reuse_tests::accounting("before-policy", &meter);
        owned_transition::invoke_historical_initial_policy(execution, &loan, &mut meter, &mut check)?;
        #[cfg(test)]
        combined_transition_reuse_tests::accounting("after-policy", &meter);
    }
    Ok(())
}
macro_rules! policy_frames_body {
    ($self:ident, $snapshot:expr, $surface:ident, $meter:expr,
     [$($select:tt)*], [$($visitor:tt)*], [$($replay:tt)*],
     $index:ident, $recipe:ident, $endpoint:ident, $day:ident, $visit:ident, [$($finish:tt)*]) => {{
        $self.check_snapshot($snapshot)?;
        let state = $snapshot.owned_scope()?;
        if !state.layers.contains_key($surface.original_layer()) || $self.epochs.is_empty() {
            return Err(crate::product_store::scope::owned_persist::err(
                "policy surface is not bound to this committed original layer",
            ));
        }
        // The type cannot be caller-constructed; the source still has to belong
        // to this exact reconstructed prefix, including provenance.
        let source = crate::product_store::scope::program($snapshot, $surface.source())?;
        $self.resolve(source)?;
        $($select)*
        for ($index, epoch) in $self.epochs.iter().enumerate() {
            let $recipe = $self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| crate::product_store::scope::owned_persist::err("policy producer recipe missing"))?;
            let ($endpoint, $day) = if let Some(next) = $self.epochs.get($index + 1) {
                let next_recipe = $self
                    .recipes
                    .get(&next.output)
                    .ok_or_else(|| crate::product_store::scope::owned_persist::err("policy successor recipe missing"))?;
                (next_recipe.seed(), next_recipe.basis().clock_day)
            } else {
                (&$snapshot.data, $snapshot.clock_day)
            };
            $($visitor)*
            $($replay)*
        }
        $($finish)*
    }};
}
pub(super) use policy_frames_body;

// Standalone administrative failure keeps its original full-error boundary.
// Shared joins below cross only the reviewed equal heap-cleanup regions.
macro_rules! completed_charge {
    ($meter:expr, $units:expr) => {
        match $meter.charge_cause($units) {
            Ok(()) => (),
            Err(cause) => return Err(cause.historical_initial_interval_error()),
        }
    };
}
macro_rules! completed_charge_to {
    ($failure:lifetime, $meter:expr, $units:expr) => {
        match $meter.charge_cause($units) {
            Ok(()) => (),
            Err(cause) => break $failure Some(cause),
        }
    };
}
macro_rules! completed_window_to {
    ($failure:lifetime, $meter:expr) => {{
        completed_charge_to!($failure, $meter, 0);
        $meter.reconstruction_default_window_after_poll()
    }};
}
macro_rules! completed_failure {
    ($failure:expr) => {
        if let Some(cause) = $failure {
            return Err(cause.historical_initial_interval_error());
        }
    };
}
macro_rules! completed_shape {
    ($pair:ident, $inner:expr) => {{
        'completed_shape: {
            let [a,p] = $inner.epochs.as_slice() else { break 'completed_shape false; };
            let [outer_a,outer_p,_,..] = $pair.loan.registry.epochs.as_slice() else { break 'completed_shape false; };
            a.output == outer_a.output && p.output == outer_p.output
                && a.adoption_revision == outer_a.adoption_revision
                && p.adoption_revision == outer_p.adoption_revision
        }
    }};
}
macro_rules! completed_recipe {
    ($pair:ident, $recipe:expr, $initial:expr) => {{
        match ($recipe, if $initial {$pair.initial} else {$pair.transition}) {
            (OwnedExecutionRecipe::Initial(a),OwnedExecutionRecipe::Initial(b)) => Arc::ptr_eq(a,b),
            (OwnedExecutionRecipe::Transition(a),OwnedExecutionRecipe::Transition(b)) => Arc::ptr_eq(a,b),
            _=>false,
        }
    }};
}
macro_rules! completed_tail {
    ($pair:ident, $inner:expr, $recipe:expr, $data:expr, $day:expr, $revision:expr) => {{
        completed_shape!($pair,$inner) && completed_recipe!($pair,$recipe,false)
            && std::ptr::eq($data,&$pair.loan.execution.current().data)
            && $day == $pair.loan.execution.current().clock_day
            && $revision == $pair.loan.execution.current().revision
    }};
}
macro_rules! completed_epoch {
    ($pair:ident, $inner:expr, $index:expr, $recipe:expr, $endpoint:expr, $day:expr) => {{
        if !completed_shape!($pair,$inner) { false } else {
            match $index {
                0 => completed_recipe!($pair,$recipe,true)
                    && std::ptr::eq($endpoint,$pair.transition.seed())
                    && $day == $pair.transition.basis().clock_day,
                1 => completed_recipe!($pair,$recipe,false)
                    && std::ptr::eq($endpoint,&$pair.loan.execution.current().data)
                    && $day == $pair.loan.execution.current().clock_day,
                _=>false,
            }
        }
    }};
}
macro_rules! completed_surface_body {
    ($pair:ident, $registry:ident, $source:ident, $layer:ident, $meter:ident) => {{
        #[cfg(test)] adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceDerive);
        #[cfg(test)] completed_interval_tests::derive_boundary("before",$meter);
        let surface = match $registry.resolve($source)? {
            OwnedExecutionRecipe::Initial(original) => owned_continuation::derive_policy_surface(original, $layer, None, $meter)?,
            OwnedExecutionRecipe::Transition(prior) => owned_continuation::derive_policy_surface(
                &prior.original, $layer, Some((&prior.continuation, &prior.target)), $meter)?,
        };
        #[cfg(test)] completed_interval_tests::derive_boundary("after",$meter);
        #[cfg(test)] adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceFrames);
        {
            let surface_ref = &surface;
            policy_frames_body!($registry, $pair.loan.execution.current(), surface_ref, $meter, [], [
                let mut visit = |frame: &DataSnapshot, meter: &mut ProofMeter| {
                    meter.charge(canonical_bytes(frame)?.len() as u64)?;
                    #[cfg(test)] completed_interval_tests::visit(index,frame);
                    surface_ref.validate_frame(frame)
                };
            ], [
                let failure = 'completed_epoch_failure: {
                completed_charge_to!('completed_epoch_failure, $meter, COMPLETED_INTERVAL_DISPATCH_PAYMENT);
                if completed_epoch!($pair,$registry,index,recipe,endpoint,day)
                    && completed_window_to!('completed_epoch_failure, $meter) {
                    #[cfg(test)] completed_interval_tests::stage("replay-entry",$meter);
                    // Pay all closed replay aliases before evaluating them.
                    completed_charge_to!('completed_epoch_failure, $meter, COMPLETED_INTERVAL_REPLAY_PAYMENT);
                    {
                        // Closed lexical aliases, not an arbitrary-input replay API.
                        let (recipe, endpoint, day) = if index == 0 {
                            #[cfg(test)] completed_interval_tests::stage("initial",$meter);
                            ($pair.initial,$pair.transition.seed(),$pair.transition.basis().clock_day)
                        } else {
                            #[cfg(test)] completed_interval_tests::stage("transition",$meter);
                            ($pair.transition,&$pair.loan.execution.current().data,$pair.loan.execution.current().clock_day)
                        };
                        completed_interval_replay_body!($pair,recipe,endpoint,day,surface_ref,$meter,index);
                    }
                    // Frame/prior/revision aliases have completed their native drops.
                    completed_charge_to!('completed_epoch_failure, $meter, 0);
                } else {
                    owned_replay::validate_recipe_history_with(recipe,recipe.seed(),recipe.basis().clock_day,
                        endpoint,day,$meter,&mut visit)?;
                }
                None
                };
                // Row errors never reach here: they map with the replay owners live.
                completed_failure!(failure);
            ], index,recipe,endpoint,day,visit,[()]);
        }
        #[cfg(test)] adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceComplete);
        Ok(surface)
    }};
}

macro_rules! completed_interval_replay_body {
    ($self:ident, $recipe:ident, $endpoint:expr, $day:expr, $surface:ident, $meter:ident, $index:expr) => {{
        let seed = $recipe.seed();
        let seed_day = $recipe.basis().clock_day;
        owned_replay::recipe_history_body!($recipe, seed, seed_day,
            $endpoint, $day, $meter, [
                $meter.charge(0)?;
                #[cfg(test)] completed_interval_tests::visit($index, seed);
                $surface.validate_frame(seed)?;
            ], [
                'completed_row: {
                    owned_replay::protected_row_body!($recipe, &frame, row, old, event.day, $meter, $meter.runtime()?,
                        [break 'completed_row;], [()], metadata, [
                            // NEW administration exists only at the native new-seal slot.
                            // All original row checks and early success precede it.
                            let failure = 'completed_row_failure: {
                                completed_charge_to!('completed_row_failure, $meter, COMPLETED_INTERVAL_ROW_PAYMENT);
                                #[cfg(test)] completed_interval_tests::stage("row-admin", $meter);
                                completed_charge_to!('completed_row_failure, $meter, COMPLETED_INTERVAL_PROJECTION_PAYMENT);
                                if completed_window_to!('completed_row_failure, $meter) {
                                    #[cfg(test)] completed_interval_tests::stage("projection-hit", $meter);
                                    completed_charge_to!('completed_row_failure, $meter, 0);
                                } else {
                                    #[cfg(test)] completed_interval_tests::stage("projection-native", $meter);
                                    owned_replay::native_completed_projection!($recipe, &frame, row, event.day, $meter, $meter.runtime()?, metadata);
                                }
                                None
                            };
                            // Convert with the native frame/prior/revisions still live.
                            completed_failure!(failure);
                        ]);
                }
            ], [
                $meter.charge(0)?;
                #[cfg(test)] completed_interval_tests::visit($index, &frame);
                $surface.validate_frame(&frame)?;
            ], [
                $meter.charge(0)?;
                #[cfg(test)] completed_interval_tests::visit($index, $endpoint);
                $surface.validate_frame($endpoint)?;
            ], frame, row, old, event, [$meter.charge(0)?;])
    }};
}
impl HistoricalInitialLoan<'_> {
    pub(super) fn surface(
        &self, execution: &VerifiedOwnedTransitionExecution, source: &CapturedProgram,
        meter: &mut ProofMeter,
    ) -> Result<owned_continuation::VerifiedOwnedPolicySurface> {
        // The caller retained the original five-unit entry charge. Preserve the
        // original layer error before the new paid exact-callback binding.
        let layer_id = execution.continuation.original_layer();
        let layer = execution.current().owned_scope()?.layers.get(layer_id)
            .ok_or_else(|| refuse("owned policy", "original layer missing"))?;
        if !std::ptr::eq(execution, self.execution) {
            return Err(StoreError::Invalid(String::from("Common factual admission: owned policy: historical Initial loan execution differs")));
        }
        #[cfg(test)]
        adoption_diagnostic::phase(adoption_diagnostic::Phase::SurfaceReconstruct);
        meter.charge(HISTORICAL_INITIAL_WORKER_PAYMENT)?;
        meter.charge(HISTORICAL_INITIAL_INTERVAL_ENTRY_PAYMENT)?;
        // New owner/lookup/enclosing transports are paid before any mint attempt.
        let mut completed = None;
        let failure = 'completed_mint_failure: {
        completed_charge_to!('completed_mint_failure, meter, COMPLETED_INTERVAL_MINT_PAYMENT);
        completed = 'completed_mint: {
            if !completed_window_to!('completed_mint_failure, meter) { break 'completed_mint None; }
            #[cfg(test)]
            if completed_interval_tests::force_native() { break 'completed_mint None; }
            let [first, prior, selected, ..] = self.registry.epochs.as_slice() else { break 'completed_mint None; };
            if self.execution.manifest.transition != ScopeTransition::Withdrawal
                || self.execution.manifest.output != selected.output
                || self.execution.manifest.previous.as_ref() != Some(&prior.output)
                || self.execution.current().revision.checked_add(1) != Some(selected.adoption_revision)
            { break 'completed_mint None; }
            let Some(initial) = self.registry.recipes.get(&first.output) else { break 'completed_mint None; };
            let Some(transition) = self.registry.recipes.get(&prior.output) else { break 'completed_mint None; };
            let (OwnedExecutionRecipe::Initial(a), OwnedExecutionRecipe::Transition(p)) = (initial, transition)
                else { break 'completed_mint None; };
            if p.manifest.transition != ScopeTransition::Evolution
                || p.manifest.output != prior.output
                || p.manifest.previous.as_ref() != Some(&first.output)
                || p.current().revision.checked_add(1) != Some(prior.adoption_revision)
                || !Arc::ptr_eq(a, &p.original)
                || !Arc::ptr_eq(a, &self.execution.original)
            { break 'completed_mint None; }
            completed_charge_to!('completed_mint_failure, meter, 0);
            #[cfg(test)] completed_interval_tests::stage("mint",meter);
            Some(CompletedHistoricalIntervals { loan: self, initial, transition })
        };
        None
        };
        completed_failure!(failure);
        let registry = self.reconstruct(completed.as_ref(), meter)?;
        completed_charge!(meter, COMPLETED_INTERVAL_SURFACE_PAYMENT);
        match completed.as_ref() {
            Some(pair) => {
                let registry = &registry;
                #[cfg(test)] completed_interval_tests::guard_cases(pair, registry);
                completed_surface_body!(pair,registry,source,layer,meter)
            },
            None => owned_transition::policy_surface_from_registry(execution, &registry, source, layer, meter),
        }
    }
    // Only this private R/E loan reaches the selected H reconstruction. Its
    // manifest comes from H's real committed receipt after complete historical
    // basis, prefix and graph validation. The two exact Arc associations extend
    // the already-verified outer prefix; equal payloads never create authority.
    fn transition_donor<'loan>(&'loan self, inner: &ReconstructedOwned,
        manifest: &OwnedCompositionV2, completed: Option<&CompletedInitialOrigin<'_, '_>>,
        meter: &mut ProofMeter,
    ) -> Result<Option<&'loan Arc<VerifiedOwnedTransitionExecution>>> {
        if !meter.reconstruction_default_window()? { return Ok(None); }
        let Some(completed) = completed else { return Ok(None); };
        let [first] = inner.epochs.as_slice() else { return Ok(None); };
        let Some(previous) = manifest.previous.as_ref() else { return Ok(None); };
        let [outer_initial, outer_transition, ..] = self.registry.epochs.as_slice() else {
            return Ok(None);
        };
        // The ordinal candidates are not authority. Fund every complete Digest
        // comparison and the bounded native get before inspecting those keys.
        #[cfg(test)]
        combined_transition_reuse_tests::boundary_next("match", HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT, meter);
        meter.charge(HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT)?;
        if &first.output != previous || &outer_initial.output != previous
            || outer_transition.output != manifest.output {
            return Ok(None);
        }
        let Some(OwnedExecutionRecipe::Transition(donor)) = self.registry.recipes.get(&manifest.output)
            else { return Ok(None); };
        if donor.manifest.transition != manifest.transition || !Arc::ptr_eq(&donor.original, completed.original) {
            return Ok(None);
        }
        Ok(Some(donor))
    }
    // Full live query at each original Initial slot. Missing/other-variant donor
    // is a native-constructor miss; resource failures never become misses.
    fn initial(&self, output: &Digest, meter: &mut ProofMeter)
        -> Result<Option<&Arc<PreparedOwnedPreview>>>
    {
        if !meter.reconstruction_default_window()? { return Ok(None); }
        meter.charge(HISTORICAL_INITIAL_SCAN_PAYMENT)?;
        let mut entries = self.registry.recipes.iter();
        loop {
            meter.charge(HISTORICAL_INITIAL_NEXT_PAYMENT)?;
            let Some((key, recipe)) = entries.next() else { return Ok(None); };
            meter.charge(HISTORICAL_INITIAL_COMPARE_PAYMENT)?;
            meter.charge(key.as_str().len().checked_add(output.as_str().len())
                .and_then(|n| u64::try_from(n).ok()).unwrap_or(u64::MAX))?;
            if key == output {
                return Ok(match recipe {
                    OwnedExecutionRecipe::Initial(initial) => Some(initial),
                    OwnedExecutionRecipe::Transition(_) => None,
                });
            }
        }
    }
}

// This owner never leaves the private reconstruction worker/finish operation.
// Only the freshly regenerated committed Initial can mint its extra Arc.
pub(super) struct FreshInitialOrigin<'s> {
    retained: &'s ProjectSnapshot,
    original: Arc<PreparedOwnedPreview>,
}
impl FreshInitialOrigin<'_> {
    pub(super) fn retained(&self) -> &ProjectSnapshot { self.retained }
    pub(super) fn comparison<'a>(&'a self, predecessor_before: &'a CapturedProgram,
        predecessor_after: &'a CapturedProgram) -> InitialComparison<'a> {
        InitialComparison { original: &self.original, predecessor_before, predecessor_after }
    }
}
pub(super) struct InitialComparison<'a> {
    original: &'a PreparedOwnedPreview,
    predecessor_before: &'a CapturedProgram,
    predecessor_after: &'a CapturedProgram,
}
impl<'a> InitialComparison<'a> {
    pub(super) fn references(&self) -> (&'a PreparedOwnedPreview, &'a CapturedProgram, &'a CapturedProgram) {
        (self.original, self.predecessor_before, self.predecessor_after)
    }
}
use std::mem::size_of;
use super::owned_continuation::comparison_payment;
trait ReconstructionFinish<'s>: Sized {
    type Output;
    // R1 is paid at the immediate audited wrapper caller, before construction.
    const FRAME_PAYMENT: u64 = comparison_payment(64, [
        (size_of::<Self>(), 2), (size_of::<Option<FreshInitialOrigin<'s>>>(), 2),
        (size_of::<ReconstructedOwned>(), 2), (size_of::<Result<ReconstructedOwned>>(), 2),
        (size_of::<Self::Output>(), 2), (size_of::<Result<Self::Output>>(), 2),
        (size_of::<&mut ProofMeter>(), 4),
    ]);
    const FINISH_PAYMENT: u64 = comparison_payment(32, [
        (size_of::<Self>(), 4), (size_of::<Self::Output>(), 4),
        (size_of::<Result<Self::Output>>(), 4), (size_of::<Option<FreshInitialOrigin<'s>>>(), 4),
        (size_of::<Option<&FreshInitialOrigin<'s>>>(), 4),
        (size_of::<&ProjectSnapshot>(), 4), (size_of::<&mut ProofMeter>(), 4),
    ]);
    // Full producing lexical scope and initial iterator-step transports. The
    // interval packet also prepays the next step, including terminal None.
    const PREFIX_SCOPE_PAYMENT: u64 = comparison_payment(64, [
        (size_of::<&ReconstructedOwned>(), 4),
        (size_of::<&ProjectSnapshot>(), 4),
        (size_of::<&DataSnapshot>(), 4),
        (size_of::<i32>(), 4),
        (size_of::<u64>(), 4),
        (size_of::<&mut ProofMeter>(), 8),
        (size_of::<std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>(), 12),
        (size_of::<std::slice::Iter<'_, OwnedProducerEpoch>>(), 4),
        (size_of::<&[OwnedProducerEpoch]>(), 4),
        (size_of::<&Vec<OwnedProducerEpoch>>(), 4),
        (size_of::<&mut std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>(), 4),
        (size_of::<Option<(usize, &OwnedProducerEpoch)>>(), 4),
        (size_of::<(usize, &OwnedProducerEpoch)>(), 4),
        (size_of::<usize>(), 4),
        (size_of::<&OwnedProducerEpoch>(), 4),
        (size_of::<isize>(), 12),
        (size_of::<Result<()>>(), 8),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>(), 8),
        (size_of::<Result<std::convert::Infallible>>(), 8),
        (size_of::<Result<Self::Output>>(), 4),
    ]);
    // Paid before every added selector/query. Both branches and all full
    // Results/residuals are funded, including outer finish-specific errors.
    const PREFIX_INTERVAL_PAYMENT: u64 = comparison_payment(320, [
        (size_of::<usize>(), 8),
        (size_of::<&OwnedProducerEpoch>(), 8),
        (size_of::<(usize, &OwnedProducerEpoch)>(), 4),
        (size_of::<Option<(usize, &OwnedProducerEpoch)>>(), 4),
        (size_of::<&mut std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>(), 4),
        (size_of::<isize>(), 48),
        (size_of::<(usize, bool)>(), 4),
        (size_of::<Option<&OwnedProducerEpoch>>(), 4),
        (size_of::<&[OwnedProducerEpoch]>(), 4),
        (size_of::<&Vec<OwnedProducerEpoch>>(), 4),
        (size_of::<&BTreeMap<Digest, OwnedExecutionRecipe>>(), 8),
        (size_of::<&Digest>(), 8),
        (size_of::<Option<&OwnedExecutionRecipe>>(), 8),
        (size_of::<&OwnedExecutionRecipe>(), 8),
        (size_of::<Result<&OwnedExecutionRecipe>>(), 8),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, &OwnedExecutionRecipe>>(), 8),
        (size_of::<(&DataSnapshot, i32, u64)>(), 4),
        (size_of::<&DataSnapshot>(), 4),
        (size_of::<i32>(), 4),
        (size_of::<u64>(), 24),
        (size_of::<&ReconstructedOwned>(), 8),
        (size_of::<&ProjectSnapshot>(), 16),
        (size_of::<&mut ProofMeter>(), 28),
        (size_of::<RuntimeLimits>(), 4),
        (size_of::<&std::time::Instant>(), 4),
        (size_of::<std::time::Duration>(), 4),
        (size_of::<&std::time::Duration>(), 4),
        (size_of::<u128>(), 16),
        (size_of::<Option<u128>>(), 4),
        (size_of::<bool>(), 12),
        (size_of::<Result<()>>(), 24),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>(), 24),
        (size_of::<Result<bool>>(), 8),
        (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>(), 4),
        (size_of::<Result<std::convert::Infallible>>(), 36),
        (size_of::<StoreError>(), 4),
        (size_of::<Result<Self::Output>>(), 4),
    ]);
    fn finish(self, snapshot: &'s ProjectSnapshot, registry: ReconstructedOwned,
        origin: Option<FreshInitialOrigin<'s>>, meter: &mut ProofMeter) -> Result<Self::Output>;
}
struct RegistryOnly;
struct TransitionFinish<'a> {
    before: &'a CapturedProgram,
    after: &'a CapturedProgram,
    sources: &'a OwnedSourceRequest,
    layer: &'a Digest,
    operation: &'a str,
    withdrawal: bool,
}
impl<'s> ReconstructionFinish<'s> for RegistryOnly {
    type Output = ReconstructedOwned;
    fn finish(self, _snapshot: &'s ProjectSnapshot, registry: ReconstructedOwned,
        origin: Option<FreshInitialOrigin<'s>>, _meter: &mut ProofMeter) -> Result<Self::Output> {
        drop(origin);
        Ok(registry)
    }
}
impl<'s> ReconstructionFinish<'s> for TransitionFinish<'_> {
    type Output = (ReconstructedOwned, Arc<VerifiedOwnedTransitionExecution>);
    fn finish(self, snapshot: &'s ProjectSnapshot, registry: ReconstructedOwned,
        origin: Option<FreshInitialOrigin<'s>>, meter: &mut ProofMeter) -> Result<Self::Output> {
        let execution = transition_with_origin(snapshot, &registry, self.before, self.after,
            self.sources, self.layer, self.operation, self.withdrawal, origin.as_ref(), meter)?;
        drop(origin);
        Ok((registry, execution))
    }
}
pub(super) const TRANSITION_RECONSTRUCT_PAYMENT: u64 = TransitionFinish::FRAME_PAYMENT;
const INITIAL_ORIGIN_PAYMENT: u64 = comparison_payment(32, [
    (size_of::<FreshInitialOrigin<'_>>(), 4), (size_of::<Option<FreshInitialOrigin<'_>>>(), 4),
    (size_of::<Arc<PreparedOwnedPreview>>(), 4), (size_of::<&ProjectSnapshot>(), 2),
]);
const HISTORICAL_ORIGIN_PAYMENT: u64 = comparison_payment(32, [
    (size_of::<Option<FreshInitialOrigin<'_>>>(), 4),
    (size_of::<Option<&FreshInitialOrigin<'_>>>(), 4),
    (size_of::<Result<VerifiedOwnedTransitionExecution>>(), 2), (size_of::<&mut ProofMeter>(), 2),
]);
// Original historical-origin work plus the two opaque accessors used only by
// lexical reconstructed verification. No constructor or raw field is exported.
const RECONSTRUCTED_ORIGIN_PAYMENT: u64 = comparison_payment(HISTORICAL_ORIGIN_PAYMENT, [
    (64, 4), // retained/comparison calls, return and branch activation
    (size_of::<&FreshInitialOrigin<'_>>(), 16),
    (size_of::<&ProjectSnapshot>(), 8),
    (size_of::<&CapturedProgram>(), 24),
    (size_of::<&Arc<PreparedOwnedPreview>>(), 8),
    (size_of::<&PreparedOwnedPreview>(), 8),
    (size_of::<InitialComparison<'_>>(), 16),
]);
// Exact completed Transition selection is separately funded, including the
// full helper/caller error path. Native misses retain every previous packet.
const HISTORICAL_TRANSITION_SELECT_PAYMENT: u64 = comparison_payment(64_u64.checked_mul(30).expect("transition activation overflow"), [
    (size_of::<&HistoricalInitialLoan<'_>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&[crate::product_store::scope::owned_epoch::OwnedProducerEpoch]>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<&crate::product_contract::Digest>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedProducerEpoch>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4_u64.checked_mul(4).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_wire::OwnedCompositionV2>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(6).expect("transition slot overflow")),
    (size_of::<&std::option::Option<crate::product_contract::Digest>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&std::time::Duration>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&std::time::Instant>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&std::vec::Vec<crate::product_store::scope::owned_epoch::OwnedProducerEpoch>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<()>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<OwnedExecutionRecipe>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<Result<ReconstructedOwned>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<bool>(), 4_u64.checked_mul(3).expect("transition slot overflow")),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<isize>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, bool>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>, std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::option::Option<&crate::product_contract::Digest>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_epoch::OwnedProducerEpoch>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>>(), 4_u64.checked_mul(6).expect("transition slot overflow")),
    (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::option::Option<u128>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::result::Result<bool, crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<std::result::Result<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>, crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::time::Duration>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<u128>(), 4_u64.checked_mul(5).expect("transition slot overflow")),
    (size_of::<u64>(), 4_u64.checked_mul(4).expect("transition slot overflow")),
]);
const HISTORICAL_TRANSITION_MATCH_PAYMENT: u64 = comparison_payment(64_u64.checked_mul(20).expect("transition activation overflow"), [
    (size_of::<&crate::product_store::scope::contract::ScopeTransition>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 4_u64.checked_mul(11).expect("transition slot overflow")),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<(&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe, &crate::product_store::scope::owned_epoch::OwnedExecutionRecipe)>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<()>(), 4_u64.checked_mul(3).expect("transition slot overflow")),
    (size_of::<(std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>, std::option::Option<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>, std::option::Option<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>, std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>)>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<Result<Option<&Arc<VerifiedOwnedTransitionExecution>>>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<bool>(), 4_u64.checked_mul(4).expect("transition slot overflow")),
    (size_of::<isize>(), 4_u64.checked_mul(8).expect("transition slot overflow")),
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>>(), 4_u64.checked_mul(4).expect("transition slot overflow")),
    (size_of::<std::option::Option<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
    (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 4_u64.checked_mul(2).expect("transition slot overflow")),
    (size_of::<u64>(), 4_u64.checked_mul(1).expect("transition slot overflow")),
]);
// Pinned Rust b940084d7eb6a299eb4bfeb8e34901bc051e7ac4: BTree nodes
// have capacity 11 and nonroot minimum 5. The completed R contains at most
// MAX_ITEMS committed plus MAX_ITEMS prospective recipes, all strict Digest64.
// BT_search = 4 + h * (6 + 11 * (3 + C)); C = String Ord(3 + 2*64)
// plus both Digest projections. This native logical envelope includes borrowed
// node/edge/return handling, not allocation or copied key payloads. The caller's
// full Rust Option/Result/guard transports are funded separately below.
const HISTORICAL_TRANSITION_LOOKUP_NATIVE_PAYMENT: u64 = {
    let max = MAX_ITEMS.checked_mul(2).expect("lookup inventory overflow");
    let mut quotient = max.checked_add(1).expect("lookup inventory overflow") / 2;
    let mut levels = 1u64;
    while quotient >= 6 {
        quotient /= 6;
        levels = levels.checked_add(1).expect("lookup height overflow");
    }
    let comparator = 64u64.checked_mul(2).expect("lookup key overflow")
        .checked_add(5).expect("lookup comparator overflow");
    let node = comparator.checked_add(3).expect("lookup dispatch overflow")
        .checked_mul(11).expect("lookup node overflow")
        .checked_add(6).expect("lookup node overflow");
    levels.checked_mul(node).expect("lookup height cost overflow")
        .checked_add(4).expect("lookup outer cost overflow")
};
const HISTORICAL_TRANSITION_GUARD_BYTES_PAYMENT: u64 =
    133u64.checked_mul(3).expect("transition guard comparison overflow");
// Positive additional opaque argument, as_ref/formal/return/reborrow slots;
// the retained SELECT floor still covers its full window/helper/caller errors.
const HISTORICAL_TRANSITION_FIRST_SELECT_PAYMENT: u64 = comparison_payment(
    HISTORICAL_TRANSITION_SELECT_PAYMENT.checked_add(64 * 3).expect("first select overflow"), [
    (size_of::<Option<&CompletedInitialOrigin<'_, '_>>>(), 16),
    (size_of::<&Option<CompletedInitialOrigin<'_, '_>>>(), 8),
    (size_of::<&CompletedInitialOrigin<'_, '_>>(), 8),
    (size_of::<bool>(), 8),
    (size_of::<Option<&Arc<VerifiedOwnedTransitionExecution>>>(), 4),
    (size_of::<&Vec<OwnedProducerEpoch>>(), 4),
    (size_of::<usize>(), 24),
    (size_of::<&OwnedProducerEpoch>(), 8),
    (size_of::<&ReconstructedOwned>(), 4),
    (size_of::<&Arc<PreparedOwnedPreview>>(), 4),
]);
const HISTORICAL_TRANSITION_FIRST_MATCH_PAYMENT: u64 = comparison_payment(
    HISTORICAL_TRANSITION_MATCH_PAYMENT
        .checked_add(HISTORICAL_TRANSITION_LOOKUP_NATIVE_PAYMENT).expect("first lookup overflow")
        .checked_add(HISTORICAL_TRANSITION_GUARD_BYTES_PAYMENT).expect("first guard overflow"), [
    (size_of::<Option<&OwnedExecutionRecipe>>(), 8),
    (size_of::<&BTreeMap<Digest, OwnedExecutionRecipe>>(), 8),
    (size_of::<&[OwnedProducerEpoch]>(), 16),
    (size_of::<&Vec<OwnedProducerEpoch>>(), 8),
    (size_of::<()>(), 4),
    (size_of::<bool>(), 4),
    (size_of::<&&Digest>(), 16),
    (size_of::<&Digest>(), 24),
]);
const HISTORICAL_TRANSITION_HIT_PAYMENT: u64 = comparison_payment(64 * 18, [
    (size_of::<&mut Option<FreshInitialOrigin<'_>>>(), 4),
    (size_of::<Option<FreshInitialOrigin<'_>>>(), 8),
    (size_of::<&mut Option<CompletedInitialOrigin<'_, '_>>>(), 4),
    (size_of::<Option<CompletedInitialOrigin<'_, '_>>>(), 8),
    (size_of::<&Arc<VerifiedOwnedTransitionExecution>>(), 16),
    (size_of::<Arc<VerifiedOwnedTransitionExecution>>(), 16),
    (size_of::<OwnedExecutionRecipe>(), 12),
    (size_of::<&std::sync::atomic::AtomicUsize>(), 4),
    (size_of::<std::sync::atomic::Ordering>(), 4), (size_of::<usize>(), 8),
    (size_of::<bool>(), 16), (size_of::<isize>(), 12),
    (size_of::<&mut ProofMeter>(), 12), (size_of::<u64>(), 12),
    (size_of::<Result<()>>(), 12),
    (size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>>>(), 12),
    (size_of::<Result<std::convert::Infallible>>(), 24),
    (size_of::<Result<ReconstructedOwned>>(), 12),
]);

// Additional direct-general route coverage: unlike the closed finish, this
// entry has not prepaid R4. Pay its complete new call/return headers locally.
const GENERAL_TRANSITION_PAYMENT: u64 = comparison_payment(64, [
    (size_of::<&ProjectSnapshot>(), 4), (size_of::<&ReconstructedOwned>(), 4),
    (size_of::<&CapturedProgram>(), 8), (size_of::<&OwnedSourceRequest>(), 4),
    (size_of::<&Digest>(), 4), (size_of::<&str>(), 4), (size_of::<bool>(), 4),
    (size_of::<Option<&FreshInitialOrigin<'_>>>(), 4), (size_of::<&mut ProofMeter>(), 4),
    (size_of::<Result<Arc<VerifiedOwnedTransitionExecution>>>(), 4),
    (size_of::<Arc<VerifiedOwnedTransitionExecution>>(), 4),
]);
struct GraphHistory {
    prior: BTreeMap<u64, DecisionGraph>,
    after: BTreeMap<u64, DecisionGraph>,
    births: BTreeMap<u64, BTreeSet<Id>>,
}
pub(super) fn err(reason: &str) -> StoreError {
    refuse("owned persisted proof", reason)
}
fn graph_history(snapshot: &ProjectSnapshot) -> Result<GraphHistory> {
    let mut graph = snapshot.decisions.clone();
    let mut result = GraphHistory {
        prior: BTreeMap::new(),
        after: BTreeMap::new(),
        births: BTreeMap::new(),
    };
    for receipt in snapshot.adoptions.iter().rev() {
        result.after.insert(receipt.revision, graph.clone());
        let required: BTreeSet<_> = graph
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.id.clone())
            .collect();
        if required != receipt.plan.required_decisions.iter().cloned().collect() {
            return Err(err("required decisions differ from the committed graph"));
        }
        let mut born = BTreeSet::new();
        if graph.identity()? != receipt.plan.expected_decisions {
            graph.revision = graph
                .revision
                .checked_sub(1)
                .ok_or_else(|| err("decision graph predecessor missing"))?;
            for id in &receipt.plan.retire_decisions {
                let decision = graph
                    .decisions
                    .iter_mut()
                    .find(|d| &d.id == id)
                    .ok_or_else(|| err("retired decision missing"))?;
                match &decision.status {
                    DecisionStatus::Withdrawn { adoption } if adoption == &receipt.plan.id => {}
                    DecisionStatus::Superseded { .. } => {}
                    _ => return Err(err("invalid decision retirement transition")),
                }
                decision.status = if matches!(
                    decision.outcome,
                    DecisionOutcome::Accept { .. } | DecisionOutcome::KeepCurrent
                ) {
                    DecisionStatus::Active
                } else {
                    DecisionStatus::Pending
                };
                decision.revision = decision
                    .revision
                    .checked_sub(1)
                    .ok_or_else(|| err("decision revision predecessor missing"))?;
            }
            while canonical_digest(IdentityDomain::Decision, &graph)?
                != receipt.plan.expected_decisions
            {
                let d = graph
                    .decisions
                    .pop()
                    .ok_or_else(|| err("prior decision graph does not reconstruct"))?;
                if d.revision != 1
                    || receipt.plan.retire_decisions.contains(&d.id)
                    || !matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending)
                {
                    return Err(err("decision birth altered terminal history"));
                }
                born.insert(d.id);
            }
            graph.validate()?;
        } else if !receipt.plan.retire_decisions.is_empty() {
            return Err(err("unchanged graph claims retirement"));
        }
        result.prior.insert(receipt.revision, graph.clone());
        result.births.insert(receipt.revision, born);
    }
    if graph.revision != 0 || !graph.decisions.is_empty() {
        return Err(err("decision history lacks its empty original checkpoint"));
    }
    Ok(result)
}
fn retained_state(state: &OwnedScopeStateV2, revision: u64) -> OwnedScopeStateV2 {
    let adoptions: Vec<_> = state
        .adoptions
        .iter()
        .filter(|a| a.revision <= revision)
        .cloned()
        .collect();
    let compositions: BTreeMap<_, _> = state
        .compositions
        .iter()
        .filter(|(id, _)| adoptions.iter().any(|a| &a.composition == *id))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let layers: BTreeMap<_, _> = state
        .layers
        .iter()
        .filter(|(id, _)| compositions.values().any(|m| m.layers.contains(id)))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    OwnedScopeStateV2 {
        version: OWNED_SCOPE_VERSION,
        initializations: state
            .initializations
            .iter()
            .filter(|i| layers.contains_key(&i.layer))
            .cloned()
            .collect(),
        layers,
        compositions,
        adoptions,
        rehearsals: state
            .rehearsals
            .iter()
            .filter(|(_, p)| p.recorded_revision <= revision)
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect(),
        correspondences: state
            .correspondences
            .iter()
            .filter(|(_, p)| p.revision <= revision)
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect(),
    }
}
#[derive(Clone, Copy)]
enum BasisPurpose {
    FirstEntry,
    Continuation,
}
fn historical_basis(
    snapshot: &ProjectSnapshot,
    basis: &OwnedBasisV2,
    graphs: &GraphHistory,
    purpose: BasisPurpose,
    meter: &mut ProofMeter,
) -> Result<ProjectSnapshot> {
    let frozen = &basis.frozen;
    if basis.container_format != super::super::FORMAT
        || frozen.revision >= snapshot.revision
        || basis.sources.is_empty()
        || basis.sources.len() > MAX_ITEMS
        || basis.artifacts.len() > MAX_ITEMS
    {
        return Err(err("unsupported, future or over-budget owned basis"));
    }
    frozen.data.validate()?;
    basis.graph.validate()?;
    if frozen.data.project_id != snapshot.data.project_id
        || frozen.decisions != basis.graph.identity()?
    {
        return Err(err("foreign basis or graph mismatch"));
    }
    let expected_graph = snapshot
        .adoptions
        .iter()
        .filter(|a| a.revision <= frozen.revision)
        .last()
        .and_then(|a| graphs.after.get(&a.revision));
    let empty = DecisionGraph {
        version: CONTRACT_VERSION,
        revision: 0,
        decisions: vec![],
    };
    if expected_graph.unwrap_or(&empty) != &basis.graph {
        return Err(err("basis decision checkpoint differs from ordered births"));
    }
    if basis.sources.len() > snapshot.programs.len()
        || basis
            .sources
            .iter()
            .zip(&snapshot.programs)
            .any(|(id, source)| revision(source).ok().as_ref() != Some(id))
    {
        return Err(err("historical source order is not a retained prefix"));
    }
    let mut seen = BTreeSet::new();
    let programs = basis
        .sources
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(err("duplicate basis source"));
            }
            Ok(program(snapshot, id)?.clone())
        })
        .collect::<Result<Vec<_>>>()?;
    let history = retained_state(snapshot.owned_scope()?, frozen.revision);
    let scope = match basis.family {
        OwnedBasisFamilyV2::ScalarEmpty => {
            if history != OwnedScopeStateV2::default() {
                return Err(err("owned lineage cannot reset to scalar"));
            }
            ScopeEnvelope::ScalarV1(ScopeState::default())
        }
        OwnedBasisFamilyV2::OwnedV2 => {
            if history.rehearsals.is_empty() && history.layers.is_empty() {
                return Err(err("naked historical owned basis"));
            }
            ScopeEnvelope::OwnedV2(history)
        }
    };
    let result = ProjectSnapshot {
        magic: snapshot.magic.clone(),
        version: basis.container_format,
        revision: frozen.revision,
        programs,
        active_revision: frozen.active.clone(),
        data: frozen.data.clone(),
        session: frozen.session.clone(),
        clock_day: frozen.day,
        decisions: basis.graph.clone(),
        adoptions: snapshot
            .adoptions
            .iter()
            .filter(|a| a.revision <= frozen.revision)
            .cloned()
            .collect(),
        operations: snapshot
            .operations
            .iter()
            .filter(|(_, r)| r.revision <= frozen.revision)
            .map(|(id, r)| (id.clone(), r.clone()))
            .collect(),
        artifacts: basis.artifacts.clone(),
        scope,
    };
    if canonical_digest(IdentityDomain::Data, &result)? != frozen.snapshot {
        return Err(err("complete historical snapshot does not reconstruct"));
    }
    result.validate_container()?;
    if let ScopeEnvelope::OwnedV2(state) = &result.scope {
        let retained: BTreeSet<_> = basis.sources.iter().collect();
        for manifest in state
            .compositions
            .values()
            .chain(state.rehearsals.values().map(|r| &r.manifest))
        {
            for id in std::iter::once(&manifest.output)
                .chain(std::iter::once(&manifest.business))
                .chain(manifest.basis.sources.iter())
            {
                meter.charge(1)?;
                if !retained.contains(id) {
                    return Err(err("historical container omitted a retained proof source"));
                }
            }
        }
        validate_recording_continuation(&result, meter)?;
    }
    if matches!(purpose, BasisPurpose::Continuation) {
        if !matches!(&result.scope, ScopeEnvelope::OwnedV2(s) if s.layers.len() == 1 && s.initializations.len() == 1 && !s.adoptions.is_empty())
        {
            return Err(err(
                "continuation basis lacks committed original layer and initialization",
            ));
        }
    } else {
        compiler::reject_reserved(&result.program()?.program)?;
        if matches!(basis.family, OwnedBasisFamilyV2::ScalarEmpty) {
            for source in &result.programs {
                compiler::reject_reserved(&source.program)?;
            }
        }
        if result
            .data
            .schema
            .iter()
            .any(|e| e.fields.iter().any(|f| f.id.starts_with(RESERVED_PREFIX)))
        {
            return Err(err("first-layer basis contains protected live storage"));
        }
    }
    result.session.validate(&result.program()?.program)?;
    validate_day(result.clock_day)?;
    // No recursively validating the same disk family. Ordered closure above and
    // the outer full-family pass authenticate all earlier retained recipes.
    Ok(result)
}
fn inventory(proof: &VerifiedOwnedPreviewPlan) -> Result<OwnedInventoryV2> {
    Ok(OwnedInventoryV2 {
        structure: analyze_owned_sources(
            proof.snapshot.program()?,
            &proof.candidate,
            &proof.request.inventory.sources,
        )?,
        certificates: proof.report.certificates.clone(),
        projections: proof.report.projections.clone(),
        phases: proof.report.phases.clone(),
        captured: proof.captured.clone(),
        outcome_comparison: proof.report.outcome_comparison.clone(),
    })
}
pub(super) fn initialization(
    layer: &OwnedLayerV2,
    prepared: &PreparedOwnedPreview,
) -> Result<OwnedInitializationV2> {
    let before = &layer.basis.frozen.data;
    let data = &prepared.initialized;
    let additions = before
        .records
        .iter()
        .filter(|r| r.entity == prepared.metadata.root)
        .map(|row| {
            let after = data
                .records
                .iter()
                .find(|r| r.entity == row.entity && r.id == row.id)
                .ok_or_else(|| err("initialization removed business row"))?;
            let values = after
                .values
                .iter()
                .filter(|(id, _)| !row.values.contains_key(*id))
                .map(|(id, v)| (id.clone(), v.clone()))
                .collect();
            Ok(MetadataAddition {
                record: history::row_ref(row),
                values,
            })
        })
        .collect::<Result<_>>()?;
    let auxiliary = data
        .schema
        .iter()
        .filter(|e| !before.schema.iter().any(|b| b.id == e.id))
        .cloned()
        .collect();
    Ok(OwnedInitializationV2 {
        version: OWNED_SCOPE_VERSION,
        operation: layer.operation.clone(),
        layer: layer.identity()?,
        before_data: before.identity()?,
        after_data: data.identity()?,
        before_schema: before.schema_identity()?,
        after_schema: data.schema_identity()?,
        additions,
        auxiliary,
        business_projection: canonical_digest(IdentityDomain::Data, &before.records)?,
        original_events: canonical_digest(IdentityDomain::Data, &before.events)?,
        original_records: before.records.len(),
        original_event_count: before.events.len(),
        capture_day: layer.basis.frozen.day,
        capture_program: layer.basis.frozen.active.clone(),
        prior_snapshot: layer.basis.frozen.snapshot.clone(),
        adoption_revision: layer
            .basis
            .frozen
            .revision
            .checked_add(1)
            .ok_or_else(|| err("basis revision overflow"))?,
    })
}
fn reconstruct_recipe(
    snapshot: &ProjectSnapshot,
    layer: &OwnedLayerV2,
    manifest: &OwnedCompositionV2,
    init: &OwnedInitializationV2,
    graphs: &GraphHistory,
    meter: &mut ProofMeter,
) -> Result<PreparedOwnedPreview> {
    meter.charge(canonical_bytes(&(layer, manifest, init))?.len() as u64)?;
    let id = layer.identity()?;
    if layer.version != OWNED_SCOPE_VERSION
        || layer.lowering != OWNED_LOWERING_VERSION
        || !valid_id(&layer.operation)
        || manifest.version != OWNED_SCOPE_VERSION
        || manifest.lowering != OWNED_LOWERING_VERSION
        || manifest.runtime != RUNTIME_VERSION
        || manifest.driver != DRIVER_VERSION
        || init.version != OWNED_SCOPE_VERSION
    {
        return Err(err(
            "unknown owned semantics, lowering, initialization or runtime",
        ));
    }
    if manifest.transition != ScopeTransition::Adoption
        || manifest.previous.is_some()
        || !manifest.correspondences.is_empty()
    {
        return Err(err(
            "owned evolution/withdrawal reconstruction is not yet supported",
        ));
    }
    if manifest.basis != layer.basis
        || manifest.operation != layer.operation
        || manifest.business != layer.candidate
        || manifest.layers != [id.clone()]
        || manifest.active != BTreeSet::from([id.clone()])
    {
        return Err(err(
            "composition changed exact layer/basis/active inventory",
        ));
    }
    let basis = historical_basis(
        snapshot,
        &layer.basis,
        graphs,
        BasisPurpose::FirstEntry,
        meter,
    )?;
    if matches!(&basis.scope, ScopeEnvelope::OwnedV2(s) if !s.layers.is_empty()) {
        return Err(err(
            "second committed owned layer is unsupported, including after withdrawal",
        ));
    }
    let candidate = program(snapshot, &layer.candidate)?;
    // Freshness covers the entire authenticated retained schema and every row.
    for auxiliary in &layer.request.auxiliary {
        if basis.data.schema.iter().any(|e| e.id == auxiliary.entity)
            || basis
                .data
                .records
                .iter()
                .any(|r| r.entity == auxiliary.entity)
        {
            return Err(err(
                "auxiliary identity already exists in retained schema/records",
            ));
        }
    }
    let (_accepted, proof) = common_fact_admission::reconstruct_and_derive_retained(
        &basis,
        candidate,
        &layer.request,
        &layer.lifecycle,
        meter,
    )?;
    if inventory(&proof)? != layer.inventory {
        return Err(err(
            "complete ownership, structure, capture or read-surface inventory differs",
        ));
    }
    let (target, metadata) = owned_lowering::compile_owned(&proof, &id, meter)?;
    if revision(&target)? != manifest.output || program(snapshot, &manifest.output)? != &target {
        return Err(err("owned compiled source/provenance does not regenerate"));
    }
    let data = owned_preview::initialize(&proof, &target, &metadata, meter)?;
    let prepared = PreparedOwnedPreview::from_retained(proof, target, metadata, data)?;
    if initialization(layer, &prepared)? != *init {
        return Err(err(
            "complete owned initialization receipt does not regenerate",
        ));
    }
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    runtime.resume(
        &prepared.target,
        &prepared.initialized,
        &layer.basis.frozen.session,
        layer.basis.frozen.day,
        0,
        meter.runtime()?,
        &basis.artifacts,
    )?;
    Ok(prepared)
}
fn linked<'a>(
    snapshot: &'a ProjectSnapshot,
    operation: &str,
    revision: u64,
) -> Result<&'a AdoptionReceipt> {
    let matches: Vec<_> = snapshot
        .adoptions
        .iter()
        .filter(|a| a.plan.id == operation && a.revision == revision)
        .collect();
    if matches.len() != 1
        || snapshot.operations.get(operation).map(|r| r.revision) != Some(revision)
    {
        return Err(err(
            "owned receipt lacks unique ordinary adoption/operation link",
        ));
    }
    Ok(matches[0])
}
fn bind_plan(
    snapshot: &ProjectSnapshot,
    linked: &AdoptionReceipt,
    manifest: &OwnedCompositionV2,
    layer: &OwnedLayerV2,
    initialized: &DataSnapshot,
    prospective: bool,
) -> Result<()> {
    let b = &manifest.basis.frozen;
    let target = if prospective {
        program(snapshot, &b.active)?
    } else {
        program(snapshot, &manifest.output)?
    };
    if linked.previous != b.active
        || linked.active != revision(target)?
        || linked.plan.current_source != program(snapshot, &b.active)?.binding
        || linked.plan.target != target.artifact
        || linked.plan.expected_data != b.data.identity()?
        || linked.plan.expected_generation != b.data.generation
        || linked.plan.expected_session != b.session.identity()?
        || linked.plan.expected_decisions != b.decisions
        || linked.plan.scope != layer.scope()
        || b.revision.checked_add(1) != Some(linked.revision)
    {
        return Err(err("owned plan differs from exact historical frame/scope"));
    }
    let runtime = LocalRuntime::default();
    if runtime.compatibility_at(target, &b.data, b.day)? != linked.plan.compatibility {
        return Err(err("historical compatibility report differs"));
    }
    if !prospective
        && runtime.compatibility_at(target, initialized, b.day)?.state
            != CompatibilityState::Compatible
    {
        return Err(err("initialized owned runtime is incompatible"));
    }
    Ok(())
}
fn check_correspondence(
    proof: &OwnedCorrespondenceV2,
    manifest: &OwnedCompositionV2,
    init: &OwnedInitializationV2,
    prepared: &PreparedOwnedPreview,
) -> Result<()> {
    let b = &manifest.basis.frozen;
    if proof.version != OWNED_SCOPE_VERSION
        || proof.source != *prepared.proof.snapshot.program()?
        || proof.original != b.data
        || proof.target != manifest.output
        || proof.day != b.day
        || proof.projected != init.after_data
        || proof.operation_seed != b.data.identity()?
        || proof.layer != init.layer
        || proof.composition != manifest.identity()?
        || proof.initialization != init.identity()?
        || proof.basis != b.snapshot
        || proof.session != b.session
        || proof.decisions != b.decisions
        || proof.random_seed != 0
    {
        return Err(err("owned correspondence frame/recipe differs"));
    }
    if let Some(pair) = &proof.scenario {
        let mut expected = pair.original.clone();
        expected.seed = prepared.initialized.clone();
        if pair.original.seed != b.data
            || pair.original.session != b.session
            || pair.original.clock_day != b.day
            || pair.original.random_seed != 0
            || pair.projected != expected
        {
            return Err(err(
                "owned correspondence changed original full input frame",
            ));
        }
        pair.original.validate(&proof.source.program)?;
        pair.projected.validate(&prepared.target.program)?;
    }
    Ok(())
}
// A call-local, fixed pair of actual native successes. Only the closed R/E loan
// below can mint it: before each immutable successor was constructed, ordinary
// reconstruct_with completed its immediate predecessor's native interval. R then
// completed in full. H=E.current restricts the same receipts/adoptions by revision;
// exact recipe/endpoint borrows never turn an ordinal or digest into authority.
struct CompletedHistoricalIntervals<'a, 'r> {
    loan: &'a HistoricalInitialLoan<'r>,
    initial: &'r OwnedExecutionRecipe,
    transition: &'r OwnedExecutionRecipe,
}
// Lexical selected replay: inputs originate only in the fixed owner's two methods.
// Frame construction, native validation and all seed/event/endpoint policy visits
// remain; the last event frame is not substituted for the endpoint.

// Provisional conservative complete-type packets; final producing census must
// cover every introduced place/operation before any fit or acceptance claim.
// Producing local joins: complete actual types, four lifecycle movements and64
// per direct call/control. Native map/Digest ceilings are separate. The1828
// entry surplus retains the approved43486 inventory decomposition; it is not
// an extra claimed operation or a fit-tuned discount. Every new seal pays anew.
const COMPLETED_INTERVAL_MINT_PAYMENT: u64 = comparison_payment(64 * 55 + 2 * HISTORICAL_TRANSITION_LOOKUP_NATIVE_PAYMENT + 4 * 133 + 1828, [
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 28),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 24),
    (size_of::<&std::collections::BTreeMap<crate::product_contract::Digest, crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 8),
    (size_of::<&crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>(), 4),
    (size_of::<&crate::product_contract::Digest>(), 48),
    (size_of::<&std::time::Duration>(), 4),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 4),
    (size_of::<&std::time::Instant>(), 4),
    (size_of::<&std::option::Option<&crate::product_contract::Digest>>(), 16),
    (size_of::<&std::option::Option<crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>>(), 4),
    (size_of::<&std::option::Option<crate::product_contract::Digest>>(), 8),
    (size_of::<&std::option::Option<u64>>(), 16),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 40),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedProducerEpoch>(), 12),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 8),
    (size_of::<&ProofMeter>(), 8),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 12),
    (size_of::<&crate::product_store::scope::contract::ScopeTransition>(), 24),
    (size_of::<&std::vec::Vec<crate::product_store::scope::owned_epoch::OwnedProducerEpoch>>(), 4),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 48),
    (size_of::<&[crate::product_store::scope::owned_epoch::OwnedProducerEpoch]>(), 4),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 12),
    (size_of::<(&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe, &crate::product_store::scope::owned_epoch::OwnedExecutionRecipe)>(), 4),
    (size_of::<crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>(), 4),
    (size_of::<std::time::Duration>(), 4),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>>(), 4),
    (size_of::<std::option::Option<&crate::product_contract::Digest>>(), 16),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>>(), 8),
    (size_of::<std::option::Option<crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>>(), 8),
    (size_of::<std::option::Option<crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<std::option::Option<u128>>(), 4),
    (size_of::<std::option::Option<u64>>(), 16),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 32),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 12),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4),
    (size_of::<crate::product_store::scope::contract::ScopeTransition>(), 8),
    (size_of::<crate::product_store::StoreError>(), 4),
    (size_of::<bool>(), 52),
    (size_of::<isize>(), 36),
    (size_of::<u128>(), 20),
    (size_of::<u64>(), 24),
    (size_of::<usize>(), 12),
]);
const COMPLETED_INTERVAL_TAIL_PAYMENT: u64 = comparison_payment(64 * 41 + 2 * 133, [
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 16),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 16),
    (size_of::<&crate::product_contract::DataSnapshot>(), 4),
    (size_of::<&crate::product_contract::Digest>(), 16),
    (size_of::<&std::time::Duration>(), 4),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 16),
    (size_of::<&std::time::Instant>(), 4),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 36),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedProducerEpoch>(), 16),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 12),
    (size_of::<&ProofMeter>(), 8),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4),
    (size_of::<&std::vec::Vec<crate::product_store::scope::owned_epoch::OwnedProducerEpoch>>(), 8),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 24),
    (size_of::<&[crate::product_store::scope::owned_epoch::OwnedProducerEpoch]>(), 8),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 12),
    (size_of::<(&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe, &crate::product_store::scope::owned_epoch::OwnedExecutionRecipe)>(), 4),
    (size_of::<*const crate::product_contract::DataSnapshot>(), 4),
    (size_of::<std::time::Duration>(), 4),
    (size_of::<std::option::Option<crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<std::option::Option<u128>>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 32),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 12),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4),
    (size_of::<crate::product_store::StoreError>(), 4),
    (size_of::<bool>(), 52),
    (size_of::<i32>(), 4),
    (size_of::<isize>(), 36),
    (size_of::<u128>(), 20),
    (size_of::<u64>(), 28),
    (size_of::<usize>(), 24),
]);
const COMPLETED_INTERVAL_SURFACE_PAYMENT: u64 = comparison_payment(64 * 9, [
    (size_of::<&crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>(), 4),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 20),
    (size_of::<&std::option::Option<crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>>(), 4),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 16),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4),
    (size_of::<&crate::product_store::scope::owned_continuation::VerifiedOwnedPolicySurface>(), 4),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 36),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4),
    (size_of::<std::option::Option<&crate::product_store::scope::owned_persist::CompletedHistoricalIntervals<'_, '_>>>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 8),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<crate::product_store::StoreError>(), 4),
    (size_of::<isize>(), 8),
]);
const COMPLETED_INTERVAL_DISPATCH_PAYMENT: u64 = comparison_payment(64 * 48 + 2 * 133, [
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_preview::PreparedOwnedPreview>>(), 32),
    (size_of::<&std::sync::Arc<crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>>(), 32),
    (size_of::<&crate::product_contract::DataSnapshot>(), 8),
    (size_of::<&crate::product_contract::Digest>(), 16),
    (size_of::<&std::time::Duration>(), 4),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 12),
    (size_of::<&std::time::Instant>(), 4),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 96),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedProducerEpoch>(), 16),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 12),
    (size_of::<&ProofMeter>(), 8),
    (size_of::<&crate::product_store::scope::owned_persist::ReconstructedOwned>(), 4),
    (size_of::<&std::vec::Vec<crate::product_store::scope::owned_epoch::OwnedProducerEpoch>>(), 8),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 16),
    (size_of::<&[crate::product_store::scope::owned_epoch::OwnedProducerEpoch]>(), 8),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 8),
    (size_of::<(&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe, &crate::product_store::scope::owned_epoch::OwnedExecutionRecipe)>(), 8),
    (size_of::<*const crate::product_contract::DataSnapshot>(), 16),
    (size_of::<std::time::Duration>(), 4),
    (size_of::<std::option::Option<crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<std::option::Option<u128>>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 24),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 8),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4),
    (size_of::<crate::product_store::StoreError>(), 4),
    (size_of::<bool>(), 60),
    (size_of::<i32>(), 16),
    (size_of::<isize>(), 40),
    (size_of::<u128>(), 20),
    (size_of::<u64>(), 24),
    (size_of::<usize>(), 24),
]);
const COMPLETED_INTERVAL_REPLAY_PAYMENT: u64 = comparison_payment(64 * 11, [
    (size_of::<&crate::product_contract::DataSnapshot>(), 16),
    (size_of::<&crate::product_store::scope::owned_persist::HistoricalInitialLoan<'_>>(), 8),
    (size_of::<&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe>(), 36),
    (size_of::<&crate::product_store::ProjectSnapshot>(), 16),
    (size_of::<&crate::product_store::scope::owned_epoch::VerifiedOwnedTransitionExecution>(), 16),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 8),
    (size_of::<(&crate::product_store::scope::owned_epoch::OwnedExecutionRecipe, &crate::product_contract::DataSnapshot, i32)>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 16),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 8),
    (size_of::<bool>(), 4),
    (size_of::<i32>(), 16),
    (size_of::<isize>(), 8),
    (size_of::<usize>(), 4),
]);
const COMPLETED_INTERVAL_ROW_PAYMENT: u64 = comparison_payment(64 * 4, [
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 4),
    (size_of::<std::option::Option<crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 16),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 4),
    (size_of::<crate::product_store::StoreError>(), 4),
    (size_of::<isize>(), 8),
]);
const COMPLETED_INTERVAL_PROJECTION_PAYMENT: u64 = comparison_payment(64 * 14, [
    (size_of::<&std::time::Duration>(), 4),
    (size_of::<&std::time::Instant>(), 4),
    (size_of::<&ProofMeter>(), 8),
    (size_of::<&mut crate::product_store::scope::common_fact_admission::ProofMeter>(), 12),
    (size_of::<std::time::Duration>(), 4),
    (size_of::<std::option::Option<u128>>(), 4),
    (size_of::<crate::product_store::scope::common_fact_admission::ProofChargeFailure>(), 24),
    (size_of::<std::result::Result<(), crate::product_store::scope::common_fact_admission::ProofChargeFailure>>(), 12),
    (size_of::<crate::product_contract::RuntimeLimits>(), 4),
    (size_of::<bool>(), 8),
    (size_of::<isize>(), 16),
    (size_of::<u128>(), 20),
    (size_of::<u64>(), 8),
]);

pub(super) fn reconstruct(
    snapshot: &ProjectSnapshot,
    meter: &mut ProofMeter,
) -> Result<ReconstructedOwned> {
    meter.charge(RegistryOnly::FRAME_PAYMENT)?;
    reconstruct_with(snapshot, meter, RegistryOnly)
}
macro_rules! reconstruct_body {
    ($snapshot:ident, $meter:ident, $finish:ident, $F:ident, $origin:ident, $layer:ident, $manifest:ident, $init:ident, $graphs:ident, $result:ident, $historical:ident, $proof:ident, $prepared:ident, $transition:ident, $initial:ident, $tail_index:ident, $tail_epoch:ident, $tail_recipe:ident, $tail_revision:ident, $tail_data:ident, $tail_day:ident, $state:ident, [$($state_serialization:tt)*], [$($completed_scope:tt)*], [$($initial_slot:tt)*], [$($transition_slot:tt)*], [$($prospective_slot:tt)*], [$($prospective_arc:tt)*], [$($completed_finish:tt)*], [$($tail_slot:tt)*], [$($final_tail_slot:tt)*]) => {{
    let mut $origin = None;
    $($completed_scope)*
    let $state = $snapshot.owned_scope()?;
    if $state.layers.len() > 1
        || $state.compositions.len() > MAX_ITEMS
        || $state.rehearsals.len() > MAX_ITEMS
        || $state.correspondences.len() > MAX_ITEMS
        || $state.initializations.len() != $state.layers.len()
        || $state.adoptions.len() != $state.compositions.len()
        || ($state.layers.is_empty() && $state.rehearsals.is_empty())
        || ($state.layers.is_empty() != $state.compositions.is_empty())
    {
        return Err(err("invalid owned family inventory or naked entry"));
    }
    $($state_serialization)*
    for (id, $layer) in &$state.layers {
        if $layer.identity()? != *id {
            return Err(err("owned layer map key differs"));
        }
    }
    for (id, composition) in &$state.compositions {
        if composition.identity()? != *id {
            return Err(err("owned composition map key differs"));
        }
    }
    for (id, $proof) in &$state.rehearsals {
        if $proof.version != OWNED_SCOPE_VERSION || $proof.identity()? != *id {
            return Err(err("owned rehearsal version/key differs"));
        }
    }
    for (id, $proof) in &$state.correspondences {
        if $proof.identity()? != *id {
            return Err(err("owned correspondence receipt key differs"));
        }
    }
    validate_revision_ownership($snapshot)?;
    let $graphs = graph_history($snapshot)?;
    let mut active = revision(
        $snapshot
            .programs
            .first()
            .ok_or_else(|| err("original source missing"))?,
    )?;
    let mut seen_operations = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    for a in &$snapshot.adoptions {
        if a.previous != active
            || a.plan.current_source != program($snapshot, &active)?.binding
            || a.plan.target != program($snapshot, &a.active)?.artifact
            || !seen_operations.insert(&a.plan.id)
            || $snapshot.operations.get(&a.plan.id).map(|o| o.revision) != Some(a.revision)
        {
            return Err(err("ordered ordinary source/operation chain differs"));
        }
        active = a.active.clone();
    }
    if active != $snapshot.active_revision {
        return Err(err("active source differs from ordered adoption tail"));
    }
    let mut $result = ReconstructedOwned {
        $snapshot: canonical_digest(IdentityDomain::Data, $snapshot)?,
        recipes: BTreeMap::new(),
        epochs: vec![],
        proofs: BTreeMap::new(),
        manifests: BTreeMap::new(),
        initializations: BTreeMap::new(),
    };
    let mut previous_revision = None;
    for receipt in &$state.adoptions {
        if receipt.version != OWNED_SCOPE_VERSION
            || !revisions.insert(receipt.revision)
            || previous_revision.is_some_and(|r| receipt.revision <= r)
        {
            return Err(err(
                "unknown, reordered or duplicate owned adoption receipt",
            ));
        }
        previous_revision = Some(receipt.revision);
        let $manifest = $state
            .compositions
            .get(&receipt.composition)
            .ok_or_else(|| err("adoption composition missing"))?;
        let [id] = $manifest.layers.as_slice() else {
            return Err(err("committed composition needs one original layer"));
        };
        let $layer = $state
            .layers
            .get(id)
            .ok_or_else(|| err("committed layer missing"))?;
        let $init = $state
            .initializations
            .iter()
            .find(|r| &r.$layer == id)
            .ok_or_else(|| err("committed initialization missing"))?;
        let adoption = linked($snapshot, &$manifest.operation, receipt.revision)?;
        let after = $graphs
            .after
            .get(&adoption.revision)
            .ok_or_else(|| err("adoption graph checkpoint missing"))?;
        let born = $graphs
            .births
            .get(&adoption.revision)
            .ok_or_else(|| err("adoption graph birth checkpoint missing"))?;
        let operation = $snapshot
            .operations
            .get(&adoption.plan.id)
            .ok_or_else(|| err("adoption operation missing"))?;
        if receipt.plan != adoption.plan.identity()? {
            return Err(err("owned plan identity differs"));
        }
        let recipe = if $result.epochs.is_empty() {
            if receipt.initialization != Some($init.identity()?)
                || operation.request
                    != owned_wire::adoption_request(&adoption.plan, after, $manifest, $init)?
            {
                return Err(err("initial adoption request/initializer differs"));
            }
            let actual: BTreeSet<_> = receipt.decisions.iter().cloned().collect();
            let expected: BTreeSet<_> = after
                .decisions
                .iter()
                .filter(|d| {
                    born.contains(&d.id)
                        && d.status == DecisionStatus::Active
                        && d.scope == $layer.scope()
                })
                .map(|d| d.id.clone())
                .collect();
            if actual.len() != receipt.decisions.len() || actual != expected
                || actual.iter().any(|id| !after.decisions.iter().any(|d| &d.id == id && matches!(&d.outcome, DecisionOutcome::Accept { artifact } if artifact == &adoption.plan.target.program_digest)))
            { return Err(err("owned accepted decisions differ from exact graph births")); }
$($initial_slot)*
            $result
                .proofs
                .insert($manifest.output.clone(), $initial.clone());
            OwnedExecutionRecipe::Initial($initial)
        } else {
            if receipt.initialization.is_some()
                || !receipt.decisions.is_empty()
                || !born.is_empty()
                || operation.request
                    != owned_wire::transition_request(&adoption.plan, after, $manifest)?
            {
                return Err(err(
                    "transition has initialization, decision birth or wrong request",
                ));
            }
            let $historical = historical_basis(
                $snapshot,
                &$manifest.basis,
                &$graphs,
                BasisPurpose::Continuation,
                $meter,
            )?;
            $meter.charge($F::PREFIX_SCOPE_PAYMENT)?;
            {
                let registry = &$result;
                let $tail_data = &$historical.data;
                let $tail_day = $historical.clock_day;
                let $tail_revision = $historical.revision;
                registry.check_snapshot($snapshot)?;
                for ($tail_index, $tail_epoch) in registry.epochs.iter().enumerate() {
                    $meter.charge($F::PREFIX_INTERVAL_PAYMENT)?;
                    if $tail_epoch.adoption_revision > $tail_revision {
                        return Err(err("history endpoint predates registered producer"));
                    }
                    let $tail_recipe = registry.recipes.get(&$tail_epoch.output)
                        .ok_or_else(|| err("registered producer recipe missing"))?;
                    if let Some(next) = registry.epochs.get($tail_index + 1) {
                        let next_recipe = registry.recipes.get(&next.output)
                            .ok_or_else(|| err("next producer missing"))?;
                        let (endpoint, endpoint_day, endpoint_revision) = (
                            next_recipe.seed(), next_recipe.basis().clock_day,
                            next_recipe.basis().revision,
                        );
                        // Only this reconstruction activation established the complete
                        // interval before its immutable successor was constructed. No
                        // caller-selected index/flag or exported skip capability exists.
                        if $meter.reconstruction_default_window()? {
                            $meter.charge(0)?;
                            $meter.charge(0)?;
                        } else {
                            owned_epoch::validate_interval($snapshot, $tail_recipe, $tail_epoch.adoption_revision,
                                endpoint_revision, endpoint, endpoint_day, $meter)?;
                        }
                    } else {
                        // Ordinary tails stay native; the selected slot can discharge
                        // only its exact completed first Initial interval.
$($tail_slot)*
                    }
                }
            }
            if $graphs.prior.get(&adoption.revision) != Some(&$historical.decisions) {
                return Err(err(
                    "transition prior graph differs from authenticated basis",
                ));
            }
            owned_epoch::validate_transition_graph(
                &$historical,
                $manifest,
                after,
                Some(&adoption.plan.retire_decisions),
            )?;
$($transition_slot)*
        };
        bind_plan($snapshot, adoption, $manifest, $layer, recipe.seed(), false)?;
        if LocalRuntime::default().compatibility_at(
            recipe.target(),
            recipe.seed(),
            $manifest.basis.frozen.day,
        )? != receipt.initialized_compatibility
        {
            return Err(err("owned compatibility receipt differs"));
        }
        if $result
            .recipes
            .insert($manifest.output.clone(), recipe)
            .is_some()
        {
            return Err(err("ambiguous owned executable"));
        }
        $result
            .manifests
            .insert($manifest.output.clone(), $manifest.clone());
        $result
            .initializations
            .insert($manifest.output.clone(), $init.clone());
        $result.epochs.push(OwnedProducerEpoch {
            output: $manifest.output.clone(),
            adoption_revision: receipt.revision,
        });
    }
    let mut recordings = BTreeSet::new();
    for $proof in $state.rehearsals.values() {
        // Initial alternatives are separate recordings, never a synthetic pair
        // of layers. Evolution pairs require the later checked evolution route.
        if !recordings.insert(&$proof.recorded_by)
            || $proof.witnesses.is_empty()
            || $proof.witnesses.len() > MAX_ITEMS
        {
            return Err(err(
                "ambiguous owned rehearsal recording or empty witnesses",
            ));
        }
        let $layer = $proof
            .$layer
            .as_ref()
            .ok_or_else(|| err("prospective owned layer recipe missing"))?;
        let $init = $proof
            .initialization
            .as_ref()
            .ok_or_else(|| err("prospective owned initialization missing"))?;
        let a = linked($snapshot, &$proof.recorded_by, $proof.recorded_revision)?;
        let born = $graphs
            .births
            .get(&a.revision)
            .ok_or_else(|| err("recording birth checkpoint missing"))?;
        let after_graph = $graphs
            .after
            .get(&a.revision)
            .ok_or_else(|| err("recording graph checkpoint missing"))?;
        let expected: BTreeSet<_> = after_graph
            .decisions
            .iter()
            .filter(|d| born.contains(&d.id) && d.status == DecisionStatus::Pending)
            .map(|d| d.id.clone())
            .collect();
        if $proof.witnesses.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(err(
                "prospective witnesses differ from pending decision births",
            ));
        }
        for (id, witness) in &$proof.witnesses {
            let d = after_graph
                .decisions
                .iter()
                .find(|d| &d.id == id)
                .ok_or_else(|| err("prospective decision missing"))?;
            if d.witness != *witness
                || d.scope != $layer.scope()
                || !matches!(
                    d.outcome,
                    DecisionOutcome::EitherAcceptable
                        | DecisionOutcome::BothNeeded
                        | DecisionOutcome::NeitherFits
                        | DecisionOutcome::Deferred
                )
            {
                return Err(err("prospective decision witness/outcome/scope differs"));
            }
        }
        let correspondence: Vec<_> = $state
            .correspondences
            .values()
            .filter(|r| r.operation == $proof.recorded_by)
            .map(|r| r.$proof.clone())
            .collect();
        let request = owned_wire::identity(
            "owned-decision-recording/2",
            &(
                &a.plan,
                &after_graph,
                owned_wire::recording_request($proof, &correspondence)?,
            ),
        )?;
        if $snapshot
            .operations
            .get(&a.plan.id)
            .ok_or_else(|| err("recording operation missing"))?
            .request
            != request
        {
            return Err(err("owned recording request inventory differs"));
        }
$($prospective_slot)*
        bind_plan(
            $snapshot,
            a,
            &$proof.$manifest,
            $layer,
            &$prepared.initialized,
            true,
        )?;
        if $prepared.initialized.identity()? != $proof.seed
            || LocalRuntime::default().compatibility_at(
                &$prepared.target,
                &$prepared.initialized,
                $proof.$manifest.basis.frozen.day,
            )? != $proof.compatibility
        {
            return Err(err("prospective owned seed/compatibility differs"));
        }
$($prospective_arc)*
        if $result
            .recipes
            .insert(
                $proof.$manifest.output.clone(),
                OwnedExecutionRecipe::Initial($prepared.clone()),
            )
            .is_some()
        {
            return Err(err("prospective recipe aliases committed executable"));
        }
        if $result
            .proofs
            .insert($proof.$manifest.output.clone(), $prepared)
            .is_some()
        {
            return Err(err("prospective recipe aliases another executable"));
        }
        $result
            .manifests
            .insert($proof.$manifest.output.clone(), $proof.$manifest.clone());
        $result
            .initializations
            .insert($proof.$manifest.output.clone(), $init.clone());
    }
    for receipt in $state.correspondences.values() {
        let $prepared = $result
            .proofs
            .get(&receipt.$proof.target)
            .ok_or_else(|| err("correspondence target recipe missing"))?;
        let $manifest = $result
            .manifests
            .get(&receipt.$proof.target)
            .ok_or_else(|| err("correspondence manifest missing"))?;
        let $init = $result
            .initializations
            .get(&receipt.$proof.target)
            .ok_or_else(|| err("correspondence initialization missing"))?;
        check_correspondence(&receipt.$proof, $manifest, $init, $prepared)?;
        linked($snapshot, &receipt.operation, receipt.revision)?;
        if !$state.rehearsals.values().any(|r| {
            r.recorded_by == receipt.operation
                && r.recorded_revision == receipt.revision
                && r.$manifest.output == receipt.$proof.target
        }) {
            return Err(err("correspondence lacks exact owned recording linkage"));
        }
    }
    // Every protected source and live storage cell must belong to reconstructed
    // committed proofs. Prospective namespaces never enter the live schema.
    let mut declared = BTreeMap::new();
    for source in &$snapshot.programs {
        let id = revision(source)?;
        if !$result.recipes.contains_key(&id) {
            compiler::reject_reserved(&source.program)?;
        }
        if $state.compositions.values().any(|m| m.output == id) {
            for e in &source.program.entities {
                for f in &e.fields {
                    if f.id.starts_with(RESERVED_PREFIX) {
                        declared.insert((e.id.clone(), f.id.clone()), f.value_type.clone());
                    }
                }
            }
        }
    }
    for e in &$snapshot.data.schema {
        for f in &e.fields {
            if f.id.starts_with(RESERVED_PREFIX)
                && declared.get(&(e.id.clone(), f.id.clone())) != Some(&f.value_type)
            {
                return Err(err("unregistered or prospective protected storage"));
            }
        }
    }
    if $state.layers.is_empty() {
        compiler::reject_reserved(&$snapshot.program()?.program)?;
        for p in $result.proofs.values() {
            for auxiliary in &p.$proof.request.auxiliary {
                if $snapshot
                    .data
                    .schema
                    .iter()
                    .any(|e| e.id == auxiliary.entity)
                    || $snapshot
                        .data
                        .records
                        .iter()
                        .any(|r| r.entity == auxiliary.entity)
                {
                    return Err(err("prospective auxiliary state leaked into live data"));
                }
            }
        }
    } else if !$state
        .compositions
        .values()
        .any(|m| m.output == $snapshot.active_revision)
    {
        return Err(err("committed owned lineage lost active history envelope"));
    }
    for a in &$snapshot.adoptions {
        if a.active != a.previous
            && ($result.recipes.contains_key(&a.active) || $result.recipes.contains_key(&a.previous))
            && !$state.adoptions.iter().any(|r| r.revision == a.revision)
        {
            return Err(err("managed source changed without owned adoption receipt"));
        }
    }
    validate_recording_continuation($snapshot, $meter)?;
    if !$result.epochs.is_empty() {
        #[cfg(test)]
        combined_transition_reuse_tests::boundary_next("tail-scope", $F::PREFIX_SCOPE_PAYMENT, $meter);
        $meter.charge($F::PREFIX_SCOPE_PAYMENT)?;
        {
            let registry = &$result;
            let $tail_data = &$snapshot.data;
            let $tail_day = $snapshot.clock_day;
            let $tail_revision = $snapshot.revision;
            registry.check_snapshot($snapshot)?;
            for ($tail_index, $tail_epoch) in registry.epochs.iter().enumerate() {
                $meter.charge($F::PREFIX_INTERVAL_PAYMENT)?;
                if $tail_epoch.adoption_revision > $tail_revision {
                    return Err(err("history endpoint predates registered producer"));
                }
                let $tail_recipe = registry.recipes.get(&$tail_epoch.output)
                    .ok_or_else(|| err("registered producer recipe missing"))?;
                if let Some(next) = registry.epochs.get($tail_index + 1) {
                    let next_recipe = registry.recipes.get(&next.output)
                        .ok_or_else(|| err("next producer missing"))?;
                    let (endpoint, endpoint_day, endpoint_revision) = (
                        next_recipe.seed(), next_recipe.basis().clock_day,
                        next_recipe.basis().revision,
                    );
                    // Only this reconstruction activation established the complete
                    // interval before its immutable successor was constructed. No
                    // caller-selected index/flag or exported skip capability exists.
                    if $meter.reconstruction_default_window()? {
                        $meter.charge(0)?;
                        $meter.charge(0)?;
                    } else {
                        owned_epoch::validate_interval($snapshot, $tail_recipe, $tail_epoch.adoption_revision,
                            endpoint_revision, endpoint, endpoint_day, $meter)?;
                    }
                } else {
                    // Ordinary tails stay native; the selected slot can discharge
                    // only its exact completed first Initial interval.
$($final_tail_slot)*
                }
            }
        }
        let epoch = $result
            .epochs
            .last()
            .ok_or_else(|| err("committed epoch missing"))?;
        if epoch.output != $snapshot.active_revision {
            return Err(err("active source is not committed epoch tail"));
        }
        let recipe = $result
            .recipes
            .get(&epoch.output)
            .ok_or_else(|| err("active recipe missing"))?;
        if $snapshot.revision == epoch.adoption_revision
            && ($snapshot.session != recipe.basis().session
                || $snapshot.clock_day != recipe.basis().clock_day
                || $snapshot.artifacts != recipe.basis().artifacts)
        {
            return Err(err("adoption changed exact session, day or artifacts"));
        }
    }
    $meter.charge($F::FINISH_PAYMENT)?;
    $($completed_finish)*
    $finish.$finish($snapshot, $result, $origin, $meter)

    }};
}
// Complete producing built/ElaborateDrops union: 50 administrative/positive
// local slots (including zero-sized units), 14 direct call/control sites.
// Native receipt/history/visitor transports and fixed floors remain paid;
// even the unchanged zero-poll and argument aliases are retained here.
const COLD_HISTORICAL_SERIALIZATION_PAYMENT: u64 = comparison_payment(64 * 14, [
    (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, StoreError>>>(), 12),
    (size_of::<Result<()>>(), 12),
    (size_of::<&mut ProofMeter>(), 24),
    (size_of::<isize>(), 12),
    (size_of::<std::result::Result<std::convert::Infallible, StoreError>>(), 24),
    (size_of::<u64>(), 4),
    (size_of::<&OwnedExecutionRecipe>(), 8),
    (size_of::<&ProjectSnapshot>(), 4),
    // Two thin references to the original noncapturing, zero-sized visitor.
    (size_of::<&mut ()>(), 8),
    (size_of::<&DataSnapshot>(), 20),
    (size_of::<i32>(), 20),
    (size_of::<&mut dyn FnMut(&DataSnapshot, &mut ProofMeter) -> Result<()>>(), 4),
    (size_of::<bool>(), 4),
]);

fn reconstruct_with<'s, F: ReconstructionFinish<'s>>(
    snapshot: &'s ProjectSnapshot, meter: &mut ProofMeter, finish: F,
) -> Result<F::Output> {    reconstruct_body!(snapshot, meter, finish, F, origin, layer, manifest, init, graphs, result, historical, proof, prepared, transition, initial, index, epoch, recipe, revision, data, day, state, [
    meter.charge(canonical_bytes(state)?.len() as u64)?;
    ], [], [
            let initial = Arc::new(reconstruct_recipe(
                snapshot, layer, manifest, init, &graphs, meter,
            )?);
            meter.charge(INITIAL_ORIGIN_PAYMENT)?;
            origin = Some(FreshInitialOrigin { retained: snapshot, original: initial.clone() });
    ], [
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_before("origin", RECONSTRUCTED_ORIGIN_PAYMENT, meter);
            meter.charge(RECONSTRUCTED_ORIGIN_PAYMENT)?;
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_after("origin", meter);
            let fresh_origin = origin.take();
            let transition = owned_continuation::reconstruct_transition(
                snapshot,
                &historical,
                &result,
                layer,
                init,
                manifest,
                fresh_origin.as_ref(),
                meter,
            )?;
            drop(fresh_origin);
            OwnedExecutionRecipe::Transition(Arc::new(transition))
    ], [
        let prepared = reconstruct_recipe(snapshot, layer, &proof.manifest, init, &graphs, meter)?;
    ], [
        let prepared = Arc::new(prepared);
    ], [], [
        #[cfg(test)] cold_serialization_tests::before(true, recipe, epoch.adoption_revision, revision, data, day, meter, COLD_HISTORICAL_SERIALIZATION_PAYMENT);
        meter.charge(COLD_HISTORICAL_SERIALIZATION_PAYMENT)?;
        // Only this lexical first historical tail borrows historical.data after
        // historical_basis authenticated its complete canonical container and
        // native bounded DataSnapshot writer. Retain the receipt/history proof,
        // every native projection and every original no-op visitor invocation.
        owned_epoch::validate_interval_body!(snapshot, recipe, epoch.adoption_revision,
            revision, data, meter, [{
                let seed = recipe.seed();
                let seed_day = recipe.basis().clock_day;
                let visit: &mut dyn FnMut(&DataSnapshot, &mut ProofMeter) -> Result<()> = &mut |_, _| {
                    Ok(())
                };
                owned_replay::recipe_history_body!(recipe, seed, seed_day, data, day, meter,
                    [visit(seed, meter)?;],
                    [owned_replay::protected_row(recipe, &frame, row, old, event.day, meter)?;],
                    [visit(&frame, meter)?;], [visit(data, meter)], frame, row, old, event, [
                        #[cfg(test)] cold_serialization_tests::poll(meter);
                        meter.charge(0)?;
                    ])
            }])?;
        #[cfg(test)] cold_serialization_tests::after(meter);
   ], [
        #[cfg(test)] cold_serialization_tests::before(false, recipe, epoch.adoption_revision, revision, data, day, meter, 0);
        owned_epoch::validate_interval(snapshot, recipe, epoch.adoption_revision,
            revision, data, day, meter)?;
        #[cfg(test)] cold_serialization_tests::after(meter);
    ])
}
impl HistoricalInitialLoan<'_> {
    fn reconstruct(&self, completed: Option<&CompletedHistoricalIntervals<'_, '_>>, meter: &mut ProofMeter) -> Result<ReconstructedOwned> {
        let snapshot = self.execution.current();
        meter.charge(RegistryOnly::FRAME_PAYMENT)?;
        let finish = RegistryOnly;
        reconstruct_body!(snapshot, meter, finish, RegistryOnly, origin, layer, manifest, init,
            graphs, result, historical, proof, prepared, transition, initial,
            index, epoch, recipe, revision, data, day, state, [
            meter.charge(0)?;
        ], [
            let mut completed_origin = None;
            let mut completed_interval = None;
        ], [
            meter.charge(HISTORICAL_INITIAL_SLOT_PAYMENT)?;
            let initial = match self.initial(&manifest.output, meter)? {
                Some(donor_initial) => {
                    meter.charge(HISTORICAL_INITIAL_MINT_PAYMENT)?;
                    completed_origin = Some(CompletedInitialOrigin { retained: snapshot, original: donor_initial });
                    let initial = donor_initial.clone();
                    meter.charge(0)?;
                    meter.charge(HISTORICAL_INITIAL_INTERVAL_MINT_PAYMENT)?;
                    completed_interval = Some(CompletedInitialInterval { loan: self, original: donor_initial });
                    initial
                }
                None => {
                    let initial = Arc::new(reconstruct_recipe(snapshot, layer, manifest, init, &graphs, meter)?);
                    meter.charge(INITIAL_ORIGIN_PAYMENT)?;
                    origin = Some(FreshInitialOrigin { retained: snapshot, original: initial.clone() });
                    initial
                }
            };
        ], [
            #[cfg(test)]
            combined_transition_reuse_tests::start_selection(meter);
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_before("select", HISTORICAL_TRANSITION_FIRST_SELECT_PAYMENT, meter);
            meter.charge(HISTORICAL_TRANSITION_FIRST_SELECT_PAYMENT)?;
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_after("select", meter);
            let donor = self.transition_donor(&result, manifest, completed_origin.as_ref(), meter)?;
            #[cfg(test)]
            let donor = combined_transition_reuse_tests::force_native(donor);
            #[cfg(test)]
            combined_transition_reuse_tests::selected(self, &result, manifest, completed_origin.as_ref(), donor, meter);
            if let Some(donor) = donor {
                #[cfg(test)]
            combined_transition_reuse_tests::boundary_before("hit", HISTORICAL_TRANSITION_HIT_PAYMENT, meter);
            meter.charge(HISTORICAL_TRANSITION_HIT_PAYMENT)?;
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_after("hit", meter);
                let fresh_origin = origin.take();
                let completed_origin = completed_origin.take();
                #[cfg(test)]
                combined_transition_reuse_tests::poll("pre-hit", meter);
                meter.charge(0)?;
                drop(completed_origin);
                drop(fresh_origin);
                let recipe = OwnedExecutionRecipe::Transition(Arc::clone(donor));
                #[cfg(test)]
                combined_transition_reuse_tests::poll("post-hit", meter);
                meter.charge(0)?;
                #[cfg(test)]
                combined_transition_reuse_tests::hit(donor, &recipe, meter);
                recipe
            } else {
                #[cfg(test)]
            combined_transition_reuse_tests::boundary_before("origin", RECONSTRUCTED_ORIGIN_PAYMENT, meter);
            meter.charge(RECONSTRUCTED_ORIGIN_PAYMENT)?;
            #[cfg(test)]
            combined_transition_reuse_tests::boundary_after("origin", meter);
            meter.charge(HISTORICAL_INITIAL_TRANSITION_WITH_VIEW_PAYMENT)?;
            let fresh_origin = origin.take();
            let completed_origin = completed_origin.take();
            let transition = match completed_origin.as_ref() {
                Some(completed) => {
                    meter.charge(owned_continuation::STRUCTURAL_ENTRY_PAYMENT)?;
                    meter.charge(owned_continuation::STRUCTURAL_LOAN_PAYMENT)?;
                    let structural = if manifest.transition == ScopeTransition::Evolution {
                        meter.charge(owned_continuation::STRUCTURAL_SELECT_OPEN_PAYMENT)?;
                        let mut candidates = self.registry.recipes.iter();
                        loop {
                            meter.charge(owned_continuation::STRUCTURAL_SELECT_NEXT_PAYMENT)?;
                            let Some((key, recipe)) = candidates.next() else { break None; };
                            meter.charge(owned_continuation::STRUCTURAL_SELECT_COMPARE_PAYMENT)?;
                            // Lossless on every admitted target; checked addition
                            // overflows only into the existing fail-closed debit.
                            meter.charge((key.as_str().len() as u64)
                                .checked_add(manifest.output.as_str().len() as u64)
                                .unwrap_or(u64::MAX))?;
                            if key != &manifest.output { continue; }
                            let OwnedExecutionRecipe::Transition(donor) = recipe else { break None; };
                            if donor.manifest.transition != ScopeTransition::Evolution { break None; }
                            if !Arc::ptr_eq(&donor.original, completed.original) { break None; }
                            break Some(CompletedStructuralLoan { origin: completed, donor });
                        }
                    } else { None };
                    owned_continuation::reconstruct_transition_completed(snapshot, &historical, &result,
                        layer, init, manifest, completed, structural.as_ref(), meter)?
                },
                None => owned_continuation::reconstruct_transition(snapshot, &historical, &result, layer, init,
                    manifest, fresh_origin.as_ref(), meter)?,
            };
            drop(completed_origin);
            drop(fresh_origin);
                OwnedExecutionRecipe::Transition(Arc::new(transition))
            }
        ], [
            meter.charge(HISTORICAL_INITIAL_SLOT_PAYMENT)?;
            let initial_preparation = match self.initial(&proof.manifest.output, meter)? {
                Some(initial) => {
                    let prepared = HistoricalInitialPrepared::Completed(initial.clone());
                    meter.charge(0)?;
                    prepared
                },
                None => HistoricalInitialPrepared::Native(reconstruct_recipe(
                    snapshot, layer, &proof.manifest, init, &graphs, meter)?),
            };
            let prepared = match &initial_preparation {
                HistoricalInitialPrepared::Completed(initial) => &**initial,
                HistoricalInitialPrepared::Native(prepared) => prepared,
            };
        ], [
            meter.charge(HISTORICAL_INITIAL_ARC_PAYMENT)?;
            let prepared = match initial_preparation {
                HistoricalInitialPrepared::Completed(initial) => initial,
                HistoricalInitialPrepared::Native(prepared) => Arc::new(prepared),
            };
        ], [drop(completed_interval); drop(completed_origin);], [
            match meter.charge_cause(HISTORICAL_INITIAL_INTERVAL_SELECT_PAYMENT) {
                Ok(()) => (),
                Err(cause) => return Err(cause.historical_initial_interval_error()),
            }
            'completed_initial_interval: {
                if let Some(_completed) = completed_interval.take() {
                    match meter.charge_cause(0) {
                        Ok(()) => (),
                        Err(cause) => return Err(cause.historical_initial_interval_error()),
                    }
                    if meter.reconstruction_default_window_after_poll() {
                        match meter.charge_cause(0) {
                            Ok(()) => (),
                            Err(cause) => return Err(cause.historical_initial_interval_error()),
                        }
                        match meter.charge_cause(0) {
                            Ok(()) => (),
                            Err(cause) => return Err(cause.historical_initial_interval_error()),
                        }
                        break 'completed_initial_interval;
                    }
                }
                match owned_epoch::validate_interval(snapshot, recipe, epoch.adoption_revision,
                    revision, data, day, meter) {
                    Ok(()) => (),
                    Err(error) => return Err(error),
                }
            }], [
            'completed_transition_interval: {
                let failure = 'completed_tail_failure: {
                completed_charge_to!('completed_tail_failure, meter, COMPLETED_INTERVAL_TAIL_PAYMENT);
                if let Some(pair) = completed {
                    if completed_tail!(pair, &result, recipe, data, day, revision)
                        && completed_window_to!('completed_tail_failure, meter) {
                        #[cfg(test)]
                        completed_interval_tests::stage("tail", meter);
                        completed_charge_to!('completed_tail_failure, meter, 0);
                        break 'completed_transition_interval;
                    }
                }
                None
                };
                completed_failure!(failure);
            match meter.charge_cause(HISTORICAL_INITIAL_INTERVAL_SELECT_PAYMENT) {
                Ok(()) => (),
                Err(cause) => return Err(cause.historical_initial_interval_error()),
            }
            'completed_initial_interval: {
                if let Some(_completed) = completed_interval.take() {
                    match meter.charge_cause(0) {
                        Ok(()) => (),
                        Err(cause) => return Err(cause.historical_initial_interval_error()),
                    }
                    if meter.reconstruction_default_window_after_poll() {
                        match meter.charge_cause(0) {
                            Ok(()) => (),
                            Err(cause) => return Err(cause.historical_initial_interval_error()),
                        }
                        match meter.charge_cause(0) {
                            Ok(()) => (),
                            Err(cause) => return Err(cause.historical_initial_interval_error()),
                        }
                        break 'completed_initial_interval;
                    }
                }
                match owned_epoch::validate_interval(snapshot, recipe, epoch.adoption_revision,
                    revision, data, day, meter) {
                    Ok(()) => (),
                    Err(error) => return Err(error),
                }
            }
            }
        ])
    }
}

pub(super) fn validate(snapshot: &ProjectSnapshot) -> Result<()> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    reconstruct(snapshot, &mut meter).map(|_| ())
}

fn basis_recipe(snapshot: &ProjectSnapshot) -> Result<OwnedBasisV2> {
    Ok(OwnedBasisV2 {
        container_format: super::super::FORMAT,
        family: match &snapshot.scope {
            ScopeEnvelope::ScalarV1(s) if *s == ScopeState::default() => {
                OwnedBasisFamilyV2::ScalarEmpty
            }
            ScopeEnvelope::OwnedV2(s) if s.layers.is_empty() && !s.rehearsals.is_empty() => {
                OwnedBasisFamilyV2::OwnedV2
            }
            _ => return Err(err("first owned recipe needs an empty committed lineage")),
        },
        frozen: basis(snapshot)?,
        sources: snapshot
            .programs
            .iter()
            .map(revision)
            .collect::<std::result::Result<_, _>>()?,
        graph: snapshot.decisions.clone(),
        artifacts: snapshot.artifacts.clone(),
    })
}
/// Private, non-authoritative recipe extraction for the later locked writer.
/// No public prepared/adoption conversion or store mutation is exposed here.
// Added helper frame only on the closed route: actual copied parameters,
// two return-place/status words plus two alignment/cleanup words, and finite
// call/return dispatch64. The ordinary recipe below remains inline and unchanged.
pub(super) const RECIPE_LAYER_CALL: u64 = 4 * (std::mem::size_of::<&PreparedOwnedPreview>()
    + std::mem::size_of::<&str>() + 4 * std::mem::size_of::<usize>()) as u64 + 64;
pub(super) const RECIPE_MANIFEST_CALL: u64 = 4 * (std::mem::size_of::<&OwnedLayerV2>()
    + std::mem::size_of::<&CapturedProgram>() + std::mem::size_of::<&str>()
    + 4 * std::mem::size_of::<usize>()) as u64 + 64;
pub(super) fn recipe_layer(preview: &PreparedOwnedPreview, operation: &str) -> Result<OwnedLayerV2> {
    if !valid_id(operation) || preview.proof.snapshot.operations.contains_key(operation) {
        return Err(err("invalid or reused owned operation"));
    }
    let original = &preview.proof;
    Ok(OwnedLayerV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        operation: operation.into(),
        basis: basis_recipe(&original.snapshot)?,
        candidate: revision(&original.candidate)?,
        request: original.request.clone(),
        lifecycle: original.accepted.recipe(),
        inventory: inventory(original)?,
    })
}
pub(super) fn recipe_manifest(layer: &OwnedLayerV2, target: &CapturedProgram, operation: &str) -> Result<OwnedCompositionV2> {
    Ok(OwnedCompositionV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        basis: layer.basis.clone(),
        transition: ScopeTransition::Adoption,
        business: layer.candidate.clone(),
        layers: vec![layer.identity()?],
        active: BTreeSet::from([layer.identity()?]),
        previous: None,
        operation: operation.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: revision(target)?,
        correspondences: vec![],
    })
}
pub(super) fn recipe(
    preview: &PreparedOwnedPreview,
    operation: &str,
    meter: &mut ProofMeter,
) -> Result<(
    OwnedLayerV2,
    OwnedCompositionV2,
    OwnedInitializationV2,
    PreparedOwnedPreview,
)> {
    if !valid_id(operation) || preview.proof.snapshot.operations.contains_key(operation) {
        return Err(err("invalid or reused owned operation"));
    }
    let original = &preview.proof;
    let layer = OwnedLayerV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        operation: operation.into(),
        basis: basis_recipe(&original.snapshot)?,
        candidate: revision(&original.candidate)?,
        request: original.request.clone(),
        lifecycle: original.accepted.recipe(),
        inventory: inventory(original)?,
    };
    let proof = common_fact_admission::derive_retained(
        &original.snapshot,
        &original.candidate,
        &original.request,
        &original.accepted,
        meter,
    )?;
    let (target, metadata) = owned_lowering::compile_owned(&proof, &layer.identity()?, meter)?;
    let data = owned_preview::initialize(&proof, &target, &metadata, meter)?;
    let manifest = OwnedCompositionV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_LOWERING_VERSION,
        basis: layer.basis.clone(),
        transition: ScopeTransition::Adoption,
        business: layer.candidate.clone(),
        layers: vec![layer.identity()?],
        active: BTreeSet::from([layer.identity()?]),
        previous: None,
        operation: operation.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: revision(&target)?,
        correspondences: vec![],
    };
    let prepared = PreparedOwnedPreview::from_retained(proof, target, metadata, data)?;
    let init = initialization(&layer, &prepared)?;
    Ok((layer, manifest, init, prepared))
}
#[cfg(test)]
pub(crate) fn owned_snapshot_for_test(
    preview: &PreparedOwnedPreview,
    operation: &str,
    prospective: bool,
) -> Result<ProjectSnapshot> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let (layer, manifest, init, prepared) = recipe(preview, operation, &mut meter)?;
    let mut snapshot = preview.proof.snapshot.clone();
    retain(&mut snapshot, &preview.proof.candidate)?;
    retain(&mut snapshot, &prepared.target)?;
    let next = snapshot
        .revision
        .checked_add(1)
        .ok_or_else(|| err("revision overflow"))?;
    let target = if prospective {
        snapshot.program()?.clone()
    } else {
        prepared.target.clone()
    };
    let plan = AdoptionPlan {
        version: CONTRACT_VERSION,
        id: operation.into(),
        project_id: snapshot.data.project_id.clone(),
        expected_generation: snapshot.data.generation,
        expected_data: snapshot.data.identity()?,
        expected_decisions: snapshot.decisions.identity()?,
        expected_session: snapshot.session.identity()?,
        current_source: snapshot.program()?.binding.clone(),
        target: target.artifact.clone(),
        scope: layer.scope(),
        compatibility: LocalRuntime::default().compatibility_at(
            &target,
            &snapshot.data,
            snapshot.clock_day,
        )?,
        required_decisions: snapshot
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.id.clone())
            .collect(),
        checks: vec![],
        evidence: vec![],
        retire_decisions: vec![],
    };
    let mut state = match &snapshot.scope {
        ScopeEnvelope::ScalarV1(_) => OwnedScopeStateV2::default(),
        ScopeEnvelope::OwnedV2(state) => state.clone(),
    };
    let request;
    if prospective {
        let witness =
            canonical_digest(IdentityDomain::Evidence, &"test-only-pending-scene-package")?;
        let decision_id = format!("{operation}-decision");
        snapshot.decisions.revision += 1;
        snapshot.decisions.decisions.push(ScopedDecision {
            id: decision_id.clone(),
            revision: 1,
            request: "Synthetic owned pending choice".into(),
            rationale: None,
            scope: layer.scope(),
            outcome: DecisionOutcome::Deferred,
            status: DecisionStatus::Pending,
            obligations: vec![],
            scenarios: vec![canonical_digest(
                IdentityDomain::Scenario,
                &"test-only-scene",
            )?],
            witness: witness.clone(),
            supersedes: vec![],
        });
        let rehearsal = OwnedRehearsalV2 {
            version: OWNED_SCOPE_VERSION,
            manifest: manifest.clone(),
            seed: prepared.initialized.identity()?,
            layer: Some(layer),
            initialization: Some(init),
            compatibility: LocalRuntime::default().compatibility_at(
                &prepared.target,
                &prepared.initialized,
                snapshot.clock_day,
            )?,
            recorded_by: operation.into(),
            recorded_revision: next,
            witnesses: [(decision_id, witness)].into(),
        };
        request = owned_wire::identity(
            "owned-decision-recording/2",
            &(
                &plan,
                &snapshot.decisions,
                owned_wire::recording_request(&rehearsal, &[])?,
            ),
        )?;
        state.rehearsals.insert(rehearsal.identity()?, rehearsal);
    } else {
        request = owned_wire::adoption_request(&plan, &snapshot.decisions, &manifest, &init)?;
        state.layers.insert(layer.identity()?, layer);
        state
            .compositions
            .insert(manifest.identity()?, manifest.clone());
        state.initializations.push(init.clone());
        state.adoptions.push(OwnedAdoptionV2 {
            version: OWNED_SCOPE_VERSION,
            decisions: vec![],
            plan: plan.identity()?,
            composition: manifest.identity()?,
            initialization: Some(init.identity()?),
            initialized_compatibility: LocalRuntime::default().compatibility_at(
                &prepared.target,
                &prepared.initialized,
                snapshot.clock_day,
            )?,
            revision: next,
        });
        snapshot.data = prepared.initialized;
    }
    snapshot.adoptions.push(AdoptionReceipt {
        previous: snapshot.active_revision.clone(),
        active: revision(&target)?,
        plan,
        revision: next,
    });
    snapshot.operations.insert(
        operation.into(),
        OperationReceipt {
            operation: operation.into(),
            request,
            revision: next,
        },
    );
    snapshot.active_revision = revision(&target)?;
    snapshot.revision = next;
    snapshot.scope = ScopeEnvelope::OwnedV2(state);
    Ok(snapshot)
}

#[cfg(test)]
pub(crate) fn owned_historical_basis_for_test(
    snapshot: &ProjectSnapshot,
    basis: &OwnedBasisV2,
) -> Result<()> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    historical_basis(
        snapshot,
        basis,
        &graph_history(snapshot)?,
        BasisPurpose::FirstEntry,
        &mut meter,
    )
    .map(|_| ())
}
#[cfg(test)]
pub(crate) fn owned_retained_preview_for_test(
    store: &ProductStore,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    accepted: &AcceptedPreviewLifecycle,
) -> Result<PreparedOwnedPreview> {
    let mut meter = ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false)))?;
    let proof = common_fact_admission::derive_retained(
        &store.load()?,
        candidate,
        request,
        accepted,
        &mut meter,
    )?;
    let (target, metadata) = owned_lowering::compile(&proof, &mut meter)?;
    let data = owned_preview::initialize(&proof, &target, &metadata, &mut meter)?;
    PreparedOwnedPreview::from_retained(proof, target, metadata, data)
}

/// A recording changes proof/decision inventories only. Later ordinary inputs
/// may change business work, but their events must continue that preserved frame.
fn validate_recording_continuation(
    snapshot: &ProjectSnapshot,
    meter: &mut ProofMeter,
) -> Result<()> {
    let state = snapshot.owned_scope()?;
    if !state.layers.is_empty() {
        return Ok(()); // The committed initializer and owned history validate this case.
    }
    let Some(recording) = state
        .rehearsals
        .values()
        .filter(|p| p.recorded_revision <= snapshot.revision)
        .max_by_key(|p| p.recorded_revision)
    else {
        return Ok(());
    };
    let basis = &recording.manifest.basis;
    let b = &basis.frozen;
    meter.charge(canonical_bytes(&snapshot.data)?.len() as u64)?;
    if snapshot.active_revision != b.active
        || snapshot.clock_day < b.day
        || snapshot.data.schema != b.data.schema
        || !snapshot.artifacts.starts_with(&basis.artifacts)
    {
        return Err(err(
            "prospective recording changed live source/schema/artifacts/clock",
        ));
    }
    if snapshot.revision == recording.recorded_revision {
        if snapshot.data != b.data
            || snapshot.session != b.session
            || snapshot.clock_day != b.day
            || snapshot.artifacts != basis.artifacts
        {
            return Err(err("prospective recording changed its exact live frame"));
        }
        return Ok(());
    }
    if !snapshot.data.events.starts_with(&b.data.events) {
        return Err(err("ordinary continuation rewrote pre-recording events"));
    }
    let source = snapshot.program()?;
    let mut frame = b.data.clone();
    let mut revisions = revision_history::RevisionHistory::new(&frame, |_| true);
    let mut prior_revision = recording.recorded_revision;
    let mut prior_day = b.day;
    for event in &snapshot.data.events[b.data.events.len()..] {
        meter.charge(1)?;
        let receipt = snapshot
            .operations
            .get(&event.operation_id)
            .ok_or_else(|| err("ordinary event lacks its retained operation"))?;
        let id = canonical_digest(
            IdentityDomain::Input,
            &(&event.operation_id, &source.artifact.program_digest),
        )?;
        if receipt.revision <= prior_revision
            || receipt.revision > snapshot.revision
            || snapshot
                .adoptions
                .iter()
                .any(|a| a.revision == receipt.revision)
            || event.program != source.artifact.program_digest
            || !source.program.actions.iter().any(|a| a.id == event.action)
            || event.id != format!("event-{}", &id.as_str()[..32])
            || frame.generation.checked_add(1) != Some(event.sequence)
            || event.day < prior_day
            || event.day > snapshot.clock_day
        {
            return Err(err(
                "ordinary event is not a post-recording runtime continuation",
            ));
        }
        prior_revision = receipt.revision;
        prior_day = event.day;
        for change in &event.changes {
            meter.charge(1)?;
            if let Some(row) = frame
                .records
                .iter_mut()
                .find(|r| r.entity == change.entity && r.id == change.record)
            {
                if change.before.as_ref() != Some(&row.values) || row.archived {
                    return Err(err("ordinary continuation changed event before-values"));
                }
                row.values = change.after.clone();
                row.archived = change.archived;
                row.revision = row
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| err("ordinary revision overflow"))?;
            } else {
                if change.before.is_some() {
                    return Err(err("ordinary continuation omitted a record birth"));
                }
                frame.records.push(Record {
                    entity: change.entity.clone(),
                    id: change.record.clone(),
                    revision: 1,
                    created_program: event.program.clone(),
                    archived: change.archived,
                    values: change.after.clone(),
                });
            }
        }
        revisions.event(
            event
                .changes
                .iter()
                .map(|c| (c.entity.clone(), c.record.clone()))
                .collect(),
            0,
            meter,
        )?;
        frame.generation = event.sequence;
        frame.events.push(event.clone());
        frame.validate()?;
    }
    if frame.generation != snapshot.data.generation
        || frame.records.len() != snapshot.data.records.len()
    {
        return Err(err(
            "ordinary continuation changed generation or record inventory",
        ));
    }
    for (expected, actual) in frame.records.iter().zip(&snapshot.data.records) {
        if expected.entity != actual.entity
            || expected.id != actual.id
            || expected.values != actual.values
            || expected.archived != actual.archived
            || expected.created_program != actual.created_program
            || actual.revision < expected.revision
        {
            return Err(err(
                "ordinary current row differs from post-recording events",
            ));
        }
    }
    revisions.validate(&snapshot.data, meter)
}

fn validate_revision_ownership(snapshot: &ProjectSnapshot) -> Result<()> {
    let mut revisions = BTreeSet::new();
    for (id, operation) in &snapshot.operations {
        if id != &operation.operation
            || operation.revision == 0
            || operation.revision > snapshot.revision
            || !revisions.insert(operation.revision)
        {
            return Err(err("operation revision is missing, duplicated or foreign"));
        }
    }
    let mut previous = 0;
    for adoption in &snapshot.adoptions {
        if adoption.revision <= previous || adoption.revision > snapshot.revision {
            return Err(err("ordinary adoptions are not strictly chronological"));
        }
        previous = adoption.revision;
    }
    Ok(())
}


impl ReconstructedOwned {
    pub(super) fn check_snapshot(&self, snapshot: &ProjectSnapshot) -> Result<()> {
        if self.snapshot != canonical_digest(IdentityDomain::Data, snapshot)? {
            return Err(err(
                "execution registry belongs to another complete snapshot",
            ));
        }
        Ok(())
    }
    pub fn resolve(&self, source: &CapturedProgram) -> Result<&OwnedExecutionRecipe> {
        let recipe = self
            .recipes
            .get(&revision(source)?)
            .ok_or_else(|| err("source is not a checked owned recipe"))?;
        if recipe.target() != source {
            return Err(err("source capture/provenance differs from checked recipe"));
        }
        Ok(recipe)
    }
    pub fn effective_pair<'a>(
        &'a self,
        snapshot: &'a ProjectSnapshot,
        output: &Digest,
    ) -> Result<(
        &'a CapturedProgram,
        &'a CapturedProgram,
        &'a OwnedSourceRequest,
    )> {
        match self
            .recipes
            .get(output)
            .ok_or_else(|| err("effective recipe missing"))?
        {
            OwnedExecutionRecipe::Initial(p) => Ok((
                program(snapshot, &p.proof.request.inventory.sources.before)?,
                program(snapshot, &p.proof.request.inventory.sources.after)?,
                &p.proof.request.inventory.sources,
            )),
            OwnedExecutionRecipe::Transition(p) => Ok((
                p.continuation.before(),
                p.continuation.after(),
                &p.continuation.request().inventory.sources,
            )),
        }
    }
    pub fn checked_withdrawals(&self) -> Vec<&Arc<VerifiedOwnedTransitionExecution>> {
        self.epochs
            .iter()
            .filter_map(|epoch| match self.recipes.get(&epoch.output) {
                Some(OwnedExecutionRecipe::Transition(p))
                    if p.manifest.transition == ScopeTransition::Withdrawal =>
                {
                    Some(p)
                }
                _ => None,
            })
            .collect()
    }
    /// Every boundary is an exact authenticated data snapshot. The shared-row
    /// max-flow proof finishes at that boundary before the next segment reseeds.
    pub fn validate_endpoint(
        &self,
        snapshot: &ProjectSnapshot,
        data: &DataSnapshot,
        day: i32,
        revision: u64,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        self.check_snapshot(snapshot)?;
        for (index, epoch) in self.epochs.iter().enumerate() {
            if epoch.adoption_revision > revision {
                return Err(err("history endpoint predates registered producer"));
            }
            let recipe = self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| err("registered producer recipe missing"))?;
            let (endpoint, endpoint_day, endpoint_revision) =
                if let Some(next) = self.epochs.get(index + 1) {
                    let next_recipe = self
                        .recipes
                        .get(&next.output)
                        .ok_or_else(|| err("next producer missing"))?;
                    (
                        next_recipe.seed(),
                        next_recipe.basis().clock_day,
                        next_recipe.basis().revision,
                    )
                } else {
                    (data, day, revision)
                };
            owned_epoch::validate_interval(
                snapshot,
                recipe,
                epoch.adoption_revision,
                endpoint_revision,
                endpoint,
                endpoint_day,
                meter,
            )?;
        }
        Ok(())
    }
    /// Additional withdrawal-only value induction over actual protected history.
    /// Pure P4 validity does not call this stricter eligibility check.
    pub fn validate_policy_frames(
        &self,
        snapshot: &ProjectSnapshot,
        surface: &owned_continuation::VerifiedOwnedPolicySurface,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        policy_frames_body!(self, snapshot, surface, meter, [], [
            let mut visit = |frame: &DataSnapshot, meter: &mut ProofMeter| {
                meter.charge(canonical_bytes(frame)?.len() as u64)?;
                #[cfg(test)] completed_interval_tests::visit(index,frame);
                surface.validate_frame(frame)
            };
        ], [
            owned_replay::validate_recipe_history_with(
                recipe,
                recipe.seed(),
                recipe.basis().clock_day,
                endpoint,
                day,
                meter,
                &mut visit,
            )?;
        ], index, recipe, endpoint, day, visit, [Ok(())])
    }
    pub(super) fn validate_policy_frames_reusing(
        &self, snapshot: &ProjectSnapshot,
        surface: &owned_continuation::VerifiedOwnedPolicySurface, layer: &OwnedLayerV2,
        source_recipe: &OwnedExecutionRecipe,
        recorder: &owned_transition::PolicyFrameRecorder<'_>, meter: &mut ProofMeter,
    ) -> Result<()> {
        policy_frames_body!(self, snapshot, surface, meter, [
            meter.charge(owned_transition::POLICY_FRAME_SELECT_PAYMENT)?;
            if recorder.eligible(owned_transition::SelectedFrameCall {
                registry: self, snapshot, recipe: source_recipe, surface, layer, meter,
            })? { return Ok(()); }
        ], [
            let mut visit = |frame: &DataSnapshot, meter: &mut ProofMeter| {
                meter.charge(canonical_bytes(frame)?.len() as u64)?;
                #[cfg(test)] completed_interval_tests::visit(index,frame);
                surface.validate_frame(frame)
            };
        ], [
            owned_replay::validate_recipe_history_with(
                recipe,
                recipe.seed(),
                recipe.basis().clock_day,
                endpoint,
                day,
                meter,
                &mut visit,
            )?;
        ], index, recipe, endpoint, day, visit, [Ok(())])
    }
    /// Authenticate an archived frame under the prefix ending at its genuine
    /// producer. No future implementation may lend event capacity or provenance.
    pub fn validate_historical_frame(
        &self,
        snapshot: &ProjectSnapshot,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
        meter: &mut ProofMeter,
    ) -> Result<()> {
        self.check_snapshot(snapshot)?;
        let output = revision(source)?;
        let index = self
            .epochs
            .iter()
            .position(|e| e.output == output)
            .ok_or_else(|| err("historical source is not a committed producer"))?;
        self.resolve(source)?;
        for (i, epoch) in self.epochs.iter().enumerate().take(index + 1) {
            let recipe = self
                .recipes
                .get(&epoch.output)
                .ok_or_else(|| err("historical producer missing"))?;
            let (endpoint, endpoint_day, end_revision) = if i < index {
                let next = self
                    .epochs
                    .get(i + 1)
                    .ok_or_else(|| err("historical successor missing"))?;
                let next_recipe = self
                    .recipes
                    .get(&next.output)
                    .ok_or_else(|| err("historical successor recipe missing"))?;
                (
                    next_recipe.seed(),
                    next_recipe.basis().clock_day,
                    next_recipe.basis().revision,
                )
            } else {
                let end = self
                    .epochs
                    .get(i + 1)
                    .map(|e| e.adoption_revision - 1)
                    .unwrap_or(snapshot.revision);
                (data, day, end)
            };
            owned_epoch::validate_interval(
                snapshot,
                recipe,
                epoch.adoption_revision,
                end_revision,
                endpoint,
                endpoint_day,
                meter,
            )?;
        }
        Ok(())
    }
}
macro_rules! reconstruct_transition_body {
    ($snapshot:ident, $current:ident, $registry:ident, $layer:ident, $initialization:ident, $manifest:ident, $origin:ident, $meter:ident, $original:ident, $prior_before:ident, $prior_after:ident, $before:ident, $after:ident, $sources:ident, $continuation:ident, $effective_snapshot:expr, [$($continuation_slot:tt)*]) => {{
    let last = $registry
        .epochs
        .last()
        .ok_or_else(|| err("transition has no committed predecessor"))?;
    let predecessor = $registry
        .manifests
        .get(&last.output)
        .ok_or_else(|| err("predecessor manifest missing"))?;
    let $original = $registry
        .proofs
        .get(
            &$registry
                .epochs
                .first()
                .ok_or_else(|| err("original epoch missing"))?
                .output,
        )
        .ok_or_else(|| err("original initializer recipe missing"))?
        .clone();
    let id = $layer.identity()?;
    if $manifest.version != OWNED_SCOPE_VERSION
        || $manifest.lowering != OWNED_TRANSITION_LOWERING_VERSION
        || $manifest.runtime != RUNTIME_VERSION
        || $manifest.driver != DRIVER_VERSION
        || !valid_id(&$manifest.operation)
        || $manifest.previous.as_ref() != Some(&last.output)
        || $current.active_revision != last.output
        || $manifest.layers != [id.clone()]
        || $manifest.basis.frozen != basis($current)?
        || $manifest.basis.graph != $current.decisions
        || $manifest.basis.artifacts != $current.artifacts
        || $manifest.basis.$sources
            != $current
                .programs
                .iter()
                .map(revision)
                .collect::<std::result::Result<Vec<_>, _>>()?
        || $manifest.basis.family != OwnedBasisFamilyV2::OwnedV2
    {
        return Err(err(
            "transition dispatch, immediate predecessor or complete basis differs",
        ));
    }
    let ($prior_before, $prior_after, prior_sources) =
        $registry.effective_pair($effective_snapshot, &last.output)?;
    let ($before, $after, $sources) = match $manifest.transition {
        ScopeTransition::Evolution => {
            if $manifest.active != predecessor.active {
                return Err(err("implementation changed active layer set"));
            }
            let [correspondence] = $manifest.correspondences.as_slice() else {
                return Err(err(
                    "evolution needs one exact implementation correspondence",
                ));
            };
            if correspondence.version != OWNED_SCOPE_VERSION
                || correspondence.source != last.output
                || correspondence.target != $manifest.business
                || correspondence.$sources.$after != $manifest.business
                || correspondence.$sources.root_entity != $layer.request.inventory.$sources.root_entity
            {
                return Err(err("evolution ordinary pair correspondence differs"));
            }
            (
                program($snapshot, &correspondence.$sources.$before)?,
                program($snapshot, &correspondence.$sources.$after)?,
                &correspondence.$sources,
            )
        }
        ScopeTransition::Withdrawal => {
            if predecessor.active != BTreeSet::from([id])
                || !$manifest.active.is_empty()
                || $manifest.correspondences != predecessor.correspondences
                || $manifest.business != predecessor.business
            {
                return Err(err(
                    "withdrawal changed effective correspondence or inactive lineage",
                ));
            }
            ($prior_before, $prior_after, prior_sources)
        }
        _ => return Err(err("unsupported owned transition kind")),
    };
    $($continuation_slot)*
    let (target, metadata) = owned_transition_lowering::compile(
        &$continuation,
        $manifest,
        !$manifest.active.is_empty(),
        $meter,
    )?;
    if revision(&target)? != $manifest.output || program($snapshot, &$manifest.output)? != &target {
        return Err(err("transition source bytes/provenance do not regenerate"));
    }
    Ok(VerifiedOwnedTransitionExecution {
        $continuation: Arc::new($continuation),
        target,
        metadata,
        $manifest: $manifest.clone(),
        $original,
        $initialization: $initialization.clone(),
    })

    }};
}
pub(super) use reconstruct_transition_body;



pub(super) fn transition(
    current: &ProjectSnapshot,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer_id: &Digest,
    operation: &str,
    withdrawal: bool,
    meter: &mut ProofMeter,
) -> Result<Arc<VerifiedOwnedTransitionExecution>> {
    meter.charge(TRANSITION_RECONSTRUCT_PAYMENT)?;
    reconstruct_and_transition(current, before, after, sources, layer_id, operation,
        withdrawal, meter).map(|(_, execution)| execution)
}
// R1 must be prepaid at each of the three audited production call sites.
#[allow(clippy::too_many_arguments)]
pub(super) fn reconstruct_and_transition(
    current: &ProjectSnapshot, before: &CapturedProgram, after: &CapturedProgram,
    sources: &OwnedSourceRequest, layer: &Digest, operation: &str,
    withdrawal: bool, meter: &mut ProofMeter,
) -> Result<(ReconstructedOwned, Arc<VerifiedOwnedTransitionExecution>)> {
    reconstruct_with(current, meter, TransitionFinish {
        before, after, sources, layer, operation, withdrawal,
    })
}
pub(super) fn transition_from_registry(
    current: &ProjectSnapshot,
    registry: &ReconstructedOwned,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer_id: &Digest,
    operation: &str,
    withdrawal: bool,
    meter: &mut ProofMeter,
) -> Result<Arc<VerifiedOwnedTransitionExecution>> {
    meter.charge(GENERAL_TRANSITION_PAYMENT)?;
    transition_with_origin(current, registry, before, after, sources, layer_id,
        operation, withdrawal, None, meter)
}
fn transition_with_origin(
    current: &ProjectSnapshot,
    registry: &ReconstructedOwned,
    before: &CapturedProgram,
    after: &CapturedProgram,
    sources: &OwnedSourceRequest,
    layer_id: &Digest,
    operation: &str,
    withdrawal: bool,
    origin: Option<&FreshInitialOrigin<'_>>,
    meter: &mut ProofMeter,
) -> Result<Arc<VerifiedOwnedTransitionExecution>> {
    registry.check_snapshot(current)?;
    let state = current.owned_scope()?;
    let layer = state
        .layers
        .get(layer_id)
        .ok_or_else(|| err("requested original layer missing"))?;
    let initialization = state
        .initializations
        .iter()
        .find(|i| &i.layer == layer_id)
        .ok_or_else(|| err("requested original initializer missing"))?;
    let last = registry
        .epochs
        .last()
        .ok_or_else(|| err("continuation requires a committed owned layer"))?;
    let predecessor = registry
        .manifests
        .get(&last.output)
        .ok_or_else(|| err("committed predecessor missing"))?;
    if current.operations.contains_key(operation) {
        return Err(StoreError::Conflict(
            "owned transition operation ID was already used".into(),
        ));
    }
    if current.active_revision != last.output || !valid_id(operation) {
        return Err(err("invalid or noncurrent transition operation"));
    }
    let (prior_before, prior_after, prior_sources) =
        registry.effective_pair(match origin { Some(o) => o.retained, None => current }, &last.output)?;
    if withdrawal
        && (predecessor.active != BTreeSet::from([layer_id.clone()])
            || before != prior_before
            || after != prior_after
            || sources != prior_sources)
    {
        return Err(err(
            "withdrawal is inactive or changed its exact effective pair",
        ));
    }
    let original = registry
        .proofs
        .get(
            &registry
                .epochs
                .first()
                .ok_or_else(|| err("original epoch missing"))?
                .output,
        )
        .ok_or_else(|| err("original preview missing"))?
        .clone();
    meter.charge(owned_continuation::STRUCTURAL_ENTRY_PAYMENT)?;
    let continuation = match origin {
        Some(origin) if !withdrawal => {
        meter.charge(owned_continuation::COMPARISON_DISPATCH_PAYMENT)?;
        meter.charge(owned_continuation::COMPARISON_OWNER_PAYMENT)?;
        meter.charge(owned_continuation::STRUCTURAL_PAIR_PAYMENT)?;
        owned_continuation::verify_initial(current, InitialComparison {
            original: &origin.original, predecessor_before: prior_before,
            predecessor_after: prior_after,
        }, layer, initialization, before, after, sources, meter)?
        }
        _ => {
        owned_continuation::verify(current, &original, layer, initialization,
            prior_before, prior_after, before, after, sources, meter)?
        }
    };
    let mut manifest = OwnedCompositionV2 {
        version: OWNED_SCOPE_VERSION,
        lowering: OWNED_TRANSITION_LOWERING_VERSION,
        basis: OwnedBasisV2 {
            container_format: super::super::FORMAT,
            family: OwnedBasisFamilyV2::OwnedV2,
            frozen: basis(current)?,
            sources: current
                .programs
                .iter()
                .map(revision)
                .collect::<std::result::Result<_, _>>()?,
            graph: current.decisions.clone(),
            artifacts: current.artifacts.clone(),
        },
        transition: if withdrawal {
            ScopeTransition::Withdrawal
        } else {
            ScopeTransition::Evolution
        },
        business: revision(after)?,
        layers: vec![layer_id.clone()],
        active: if withdrawal {
            BTreeSet::new()
        } else {
            predecessor.active.clone()
        },
        previous: Some(last.output.clone()),
        operation: operation.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: last.output.clone(), // Excluded by lowering3's recipe digest.
        correspondences: if withdrawal {
            predecessor.correspondences.clone()
        } else {
            vec![OwnedImplementationCorrespondenceV2 {
                version: OWNED_SCOPE_VERSION,
                source: last.output.clone(),
                target: revision(after)?,
                sources: sources.clone(),
            }]
        },
    };
    let (target, metadata) = owned_transition_lowering::compile(
        &continuation,
        &manifest,
        !manifest.active.is_empty(),
        meter,
    )?;
    manifest.output = revision(&target)?;
    Ok(Arc::new(VerifiedOwnedTransitionExecution {
        continuation: Arc::new(continuation),
        target,
        metadata,
        manifest,
        original,
        initialization: initialization.clone(),
    }))
}

/// Extract proposed arguments only. Callers must pass them to `transition`,
/// which reconstructs the complete family before granting any authority.
pub(super) fn effective_pair<'a>(
    snapshot: &'a ProjectSnapshot,
    target: &Digest,
) -> Result<(
    &'a CapturedProgram,
    &'a CapturedProgram,
    &'a OwnedSourceRequest,
)> {
    let state = snapshot.owned_scope()?;
    let manifest = state
        .compositions
        .values()
        .find(|m| &m.output == target)
        .ok_or_else(|| err("effective manifest missing"))?;
    let sources = if manifest.transition == ScopeTransition::Adoption {
        let [id] = manifest.layers.as_slice() else {
            return Err(err("effective original layer ambiguous"));
        };
        &state
            .layers
            .get(id)
            .ok_or_else(|| err("effective original layer missing"))?
            .request
            .inventory
            .sources
    } else if let [correspondence] = manifest.correspondences.as_slice() {
        &correspondence.sources
    } else if manifest.transition == ScopeTransition::Withdrawal
        && manifest.correspondences.is_empty()
    {
        let [id] = manifest.layers.as_slice() else {
            return Err(err("effective original layer ambiguous"));
        };
        &state
            .layers
            .get(id)
            .ok_or_else(|| err("effective original layer missing"))?
            .request
            .inventory
            .sources
    } else {
        return Err(err("effective ordinary source pair ambiguous"));
    };
    Ok((
        program(snapshot, &sources.before)?,
        program(snapshot, &sources.after)?,
        sources,
    ))
}

// Assertion-only fixture bridge: no proof, source, registry or write capability
// leaves this cfg(test) method. It probes one real private mechanism after full
// reconstruction of the unmodified two-epoch fixture has already succeeded.
#[cfg(test)]
impl ProjectSnapshot {
    pub(crate) fn assert_owned_epoch_interval_rejects_unique_swapped_receipts_for_test(&self) {
        self.validate_container().unwrap();
        let mut meter =
            ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap();
        let registry = reconstruct(self, &mut meter).unwrap();
        assert_eq!(
            registry.epochs.len(),
            2,
            "bridge requires the genuine two-epoch fixture"
        );
        let epoch = &registry.epochs[0];
        let recipe = registry.recipes.get(&epoch.output).unwrap();
        let next = registry.recipes.get(&registry.epochs[1].output).unwrap();
        let endpoint = next.seed();
        let endpoint_revision = next.basis().revision;
        let old_revision = self.operations["old-complete"].revision;
        let new_revision = self.operations["new-epoch-add"].revision;
        assert!(epoch.adoption_revision < old_revision && old_revision <= endpoint_revision);
        assert!(new_revision > endpoint_revision);
        assert!(endpoint.events[recipe.seed().events.len()..]
            .iter()
            .any(|event| event.operation_id == "old-complete"
                && event.program == recipe.target().artifact.program_digest));
        assert!(self.data.events[endpoint.events.len()..]
            .iter()
            .any(|event| event.operation_id == "new-epoch-add"
                && event.program == next.target().artifact.program_digest));
        let mut swapped = self.clone();
        swapped.operations.get_mut("old-complete").unwrap().revision = new_revision;
        swapped
            .operations
            .get_mut("new-epoch-add")
            .unwrap()
            .revision = old_revision;
        validate_revision_ownership(&swapped).unwrap();
        let error = owned_epoch::validate_interval(
            &swapped,
            recipe,
            epoch.adoption_revision,
            endpoint_revision,
            endpoint,
            next.basis().clock_day,
            &mut meter,
        )
        .unwrap_err();
        assert!(
            matches!(&error, StoreError::Invalid(message)
            if message == "Common factual admission: owned producer epoch: event is outside its actual producer operation interval"),
            "{error}"
        );
    }
}

#[cfg(test)]
#[path = "owned_persist_registry_controls.rs"]
mod registry_policy_controls;

#[cfg(test)]
#[path = "owned_persist_initial_reuse_tests.rs"]
mod initial_reuse_tests;

// Direct packet/query controls only. Actual lexical-route coverage needs the
// separately admitted genuine histories and fixed-site private sidecar.
#[cfg(test)]
mod ordinary_prefix_controls {
    use super::*;
    // Compiler-evaluated target layouts; no test executable needed for these checks.
    const _: () = {
        assert!(size_of::<&ReconstructedOwned>() == 8);
        assert!(size_of::<&ProjectSnapshot>() == 8);
        assert!(size_of::<&DataSnapshot>() == 8);
        assert!(size_of::<i32>() == 4);
        assert!(size_of::<u64>() == 8);
        assert!(size_of::<&mut ProofMeter>() == 8);
        assert!(size_of::<std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>() == 24);
        assert!(size_of::<std::slice::Iter<'_, OwnedProducerEpoch>>() == 16);
        assert!(size_of::<&[OwnedProducerEpoch]>() == 16);
        assert!(size_of::<&Vec<OwnedProducerEpoch>>() == 8);
        assert!(size_of::<&mut std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>() == 8);
        assert!(size_of::<Option<(usize, &OwnedProducerEpoch)>>() == 16);
        assert!(size_of::<(usize, &OwnedProducerEpoch)>() == 16);
        assert!(size_of::<usize>() == 8);
        assert!(size_of::<&OwnedProducerEpoch>() == 8);
        assert!(size_of::<isize>() == 8);
        assert!(size_of::<Result<()>>() == 152);
        assert!(size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>() == 152);
        assert!(size_of::<Result<std::convert::Infallible>>() == 152);
        assert!(size_of::<(usize, bool)>() == 16);
        assert!(size_of::<Option<&OwnedProducerEpoch>>() == 8);
        assert!(size_of::<&BTreeMap<Digest, OwnedExecutionRecipe>>() == 8);
        assert!(size_of::<&Digest>() == 8);
        assert!(size_of::<Option<&OwnedExecutionRecipe>>() == 8);
        assert!(size_of::<&OwnedExecutionRecipe>() == 8);
        assert!(size_of::<Result<&OwnedExecutionRecipe>>() == 152);
        assert!(size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, &OwnedExecutionRecipe>>() == 152);
        assert!(size_of::<(&DataSnapshot, i32, u64)>() == 24);
        assert!(size_of::<RuntimeLimits>() == 48);
        assert!(size_of::<&std::time::Instant>() == 8);
        assert!(size_of::<std::time::Duration>() == 16);
        assert!(size_of::<&std::time::Duration>() == 8);
        assert!(size_of::<u128>() == 16);
        assert!(size_of::<Option<u128>>() == 32);
        assert!(size_of::<bool>() == 1);
        assert!(size_of::<Result<bool>>() == 152);
        assert!(size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>() == 152);
        assert!(size_of::<StoreError>() == 152);
        assert!(RegistryOnly::PREFIX_SCOPE_PAYMENT == 5296);
        assert!(RegistryOnly::PREFIX_INTERVAL_PAYMENT == 21148);
        assert!(TransitionFinish::PREFIX_SCOPE_PAYMENT == 5328);
        assert!(TransitionFinish::PREFIX_INTERVAL_PAYMENT == 21180);
    };
    #[test]
    fn ordinary_prefix_packet_short_exact_and_actual_layouts() {
        let packets = [RegistryOnly::PREFIX_SCOPE_PAYMENT, RegistryOnly::PREFIX_INTERVAL_PAYMENT,
            TransitionFinish::PREFIX_SCOPE_PAYMENT, TransitionFinish::PREFIX_INTERVAL_PAYMENT];
        for packet in packets {
            assert!(packet > 0 && packet < ProofBudget::default().units);
            let mut short = ProofMeter::new(ProofBudget { units: packet, elapsed_millis: 60_000 },
                Arc::new(AtomicBool::new(false))).unwrap();
            assert!(matches!(short.charge(packet), Err(StoreError::Invalid(text))
                if text == "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
            let mut exact = ProofMeter::new(ProofBudget { units: packet + 1, elapsed_millis: 60_000 },
                Arc::new(AtomicBool::new(false))).unwrap();
            exact.charge(packet).unwrap();
            assert_eq!(exact.remaining_units_for_test(), 0);
        }
        println!("ORDINARY_PREFIX_PACKETS {packets:?}");
        println!("ORDINARY_PREFIX_LAYOUTS {}", serde_json::json!({
            "registry": size_of::<ReconstructedOwned>(),
            "registry_result": size_of::<Result<ReconstructedOwned>>(),
            "transition_result": size_of::<Result<(ReconstructedOwned, Arc<VerifiedOwnedTransitionExecution>)>>(),
            "iterator": size_of::<std::iter::Enumerate<std::slice::Iter<'_, OwnedProducerEpoch>>>(),
            "item": size_of::<(usize, &OwnedProducerEpoch)>(),
            "optional_item": size_of::<Option<(usize, &OwnedProducerEpoch)>>(),
            "endpoint": size_of::<(&DataSnapshot, i32, u64)>(),
            "runtime_limits": size_of::<RuntimeLimits>(),
            "instant": size_of::<std::time::Instant>(),
            "duration": size_of::<std::time::Duration>(),
            "u128": size_of::<u128>(), "optional_u128": size_of::<Option<u128>>(),
            "unit_result": size_of::<Result<()>>(), "bool_result": size_of::<Result<bool>>(),
            "error": size_of::<StoreError>(), "residual": size_of::<Result<std::convert::Infallible>>(),
            "unit_control": size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, ()>>(),
            "bool_control": size_of::<std::ops::ControlFlow<Result<std::convert::Infallible>, bool>>()
        }));
    }
    #[test]
    fn ordinary_prefix_paid_window_default_clipped_and_both_tokens() {
        for elapsed_millis in [60_000, 4999] {
            let packet = RegistryOnly::PREFIX_INTERVAL_PAYMENT;
            let mut meter = ProofMeter::new(ProofBudget { units: packet + 1, elapsed_millis },
                Arc::new(AtomicBool::new(false))).unwrap();
            meter.charge(packet).unwrap();
            let eligible = meter.reconstruction_default_window().unwrap();
            assert_eq!(eligible, elapsed_millis == 60_000);
            assert_eq!(meter.remaining_units_for_test(), 0, "query never reserves or refunds");
        }
        for secondary in [false, true] {
            let primary = Arc::new(AtomicBool::new(false));
            let additional = Arc::new(AtomicBool::new(false));
            let packet = RegistryOnly::PREFIX_INTERVAL_PAYMENT;
            let mut meter = ProofMeter::new(ProofBudget { units: packet + 1, elapsed_millis: 60_000 }, primary.clone()).unwrap();
            meter.watch_cancellation(&additional);
            meter.charge(packet).unwrap();
            if secondary { additional.store(true, std::sync::atomic::Ordering::Relaxed); }
            else { primary.store(true, std::sync::atomic::Ordering::Relaxed); }
            assert!(matches!(meter.reconstruction_default_window(), Err(StoreError::Invalid(text))
                if text == "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
        }
    }
    // Genuine immutable snapshots only. This test entry cannot request omission,
    // choose an epoch cutoff or construct a production proof/registry.
    impl ProjectSnapshot {
        pub(crate) fn assert_ordinary_prefix_for_test(
            &self, initial: &ProjectSnapshot, historical: &[ProjectSnapshot],
            expected_events: &[usize],
        ) {
            self.validate_container().unwrap();
            assert_eq!(historical.len() + 1, expected_events.len());
            let mut meter = ProofMeter::new(ProofBudget::default(),
                Arc::new(AtomicBool::new(false))).unwrap();
            let registry = reconstruct(self, &mut meter).unwrap();
            registry.check_snapshot(self).unwrap();
            registry.resolve(self.program().unwrap()).unwrap();
            assert_eq!(registry.epochs.len(), expected_events.len());
            let state = self.owned_scope().unwrap();
            assert_eq!(state.adoptions.len(), expected_events.len());
            assert_eq!(registry.epochs.iter().map(|e| &e.output).collect::<BTreeSet<_>>().len(),
                expected_events.len());
            assert_eq!(registry.epochs.last().unwrap().output, self.active_revision);
            assert_eq!(registry.recipes[&registry.epochs[0].output].seed(), &initial.data);
            let mut actual_events = Vec::new();
            for (index, epoch) in registry.epochs.iter().enumerate() {
                let adoption = &state.adoptions[index];
                assert_eq!(epoch.adoption_revision, adoption.revision);
                assert_eq!(epoch.output, state.compositions[&adoption.composition].output);
                let recipe = &registry.recipes[&epoch.output];
                let endpoint = if let Some(next) = registry.epochs.get(index + 1) {
                    let completed = &historical[index];
                    let next_recipe = &registry.recipes[&next.output];
                    assert_eq!(next_recipe.seed(), &completed.data);
                    assert_eq!(next_recipe.basis().clock_day, completed.clock_day);
                    assert_eq!(next_recipe.basis().revision, completed.revision);
                    assert!(!std::ptr::eq(next_recipe.seed(), &completed.data),
                        "constructor-retained semantic clone, not a pointer assumption");
                    completed
                } else { self };
                assert!(endpoint.data.events.starts_with(&recipe.seed().events));
                actual_events.push(endpoint.data.events.len() - recipe.seed().events.len());
            }
            assert_eq!(actual_events, expected_events);
            assert_eq!(registry.recipes[&registry.epochs.last().unwrap().output].active(),
                expected_events.len() < 3);

            // The unchanged generic entry receives no lexical induction authority.
            // It must still complete under its ordinary default test-local meter.
            let mut generic = ProofMeter::new(ProofBudget::default(),
                Arc::new(AtomicBool::new(false))).unwrap();
            registry.validate_endpoint(self, &self.data, self.clock_day, self.revision,
                &mut generic).unwrap();

            let mut foreign_current = self.clone();
            foreign_current.clock_day = foreign_current.clock_day.checked_add(1).unwrap();
            assert_eq!(foreign_current.data, self.data);
            assert!(matches!(registry.check_snapshot(&foreign_current), Err(StoreError::Invalid(text))
                if text == "Common factual admission: owned persisted proof: execution registry belongs to another complete snapshot"));
            let mut foreign_source = self.program().unwrap().clone();
            foreign_source.binding.program_path = "foreign-prefix/program.json".into();
            assert_eq!(foreign_source.artifact, self.program().unwrap().artifact);
            assert_eq!(foreign_source.source_bytes, self.program().unwrap().source_bytes);
            foreign_source.validate().unwrap();
            assert!(matches!(registry.resolve(&foreign_source), Err(StoreError::Invalid(text))
                if text == "Common factual admission: owned persisted proof: source is not a checked owned recipe"));
            println!("ORDINARY_PREFIX_GENUINE epochs={} events={actual_events:?} constructor_boundaries={} tail_active={} generic_full_ok=true",
                registry.epochs.len(), historical.len(), expected_events.len() < 3);
        }
    }

}

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 12160] = [(); HISTORICAL_INITIAL_ARC_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 10304] = [(); historical_initial_callback_payment::<()>() as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 7492] = [(); HISTORICAL_INITIAL_COMPARE_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 5952] = [(); HISTORICAL_INITIAL_MINT_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 3200] = [(); HISTORICAL_INITIAL_NEXT_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 3232] = [(); HISTORICAL_INITIAL_SCAN_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 76072] = [(); HISTORICAL_INITIAL_SLOT_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 27577] = [(); HISTORICAL_INITIAL_SURFACE_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 73960] = [(); HISTORICAL_INITIAL_TRANSITION_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 3840] = [(); HISTORICAL_INITIAL_WORKER_PAYMENT as usize];

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 74_536] = [(); HISTORICAL_INITIAL_TRANSITION_WITH_VIEW_PAYMENT as usize];

#[cfg(test)]
#[path = "owned_combined_transition_reuse_tests.rs"]
pub(super) mod combined_transition_reuse_tests;

#[cfg(test)]
#[path = "owned_completed_interval_tests.rs"]
pub(super) mod completed_interval_tests;

#[cfg(test)]
#[path = "owned_cold_serialization_tests.rs"]
pub(super) mod cold_serialization_tests;
