//! Focused fixture assembly for the private decoded OwnedV2 capture tests.
#[path = "fixtures/common_fact/mod.rs"]
mod common;
#[path = "fixtures/product_runtime/mod.rs"]
mod runtime_fixture;
#[path = "fixtures/product_scope/mod.rs"]
mod scope_fixture;
#[path = "fixtures/owned_recovery/mod.rs"]
mod owned_recovery_fixture;

mod fixture {
    use crate::product_contract::{AppDefinition, CapturedProgram};

    pub trait CaptureInput { fn capture(self) -> CapturedProgram; }
    impl CaptureInput for AppDefinition {
        fn capture(self) -> CapturedProgram { crate::common::capture(&self) }
    }
    impl CaptureInput for serde_json::Value {
        fn capture(self) -> CapturedProgram { crate::runtime_fixture::capture(self) }
    }
    pub fn capture<T: CaptureInput>(value: T) -> CapturedProgram { value.capture() }
    pub fn organizer() -> serde_json::Value { crate::runtime_fixture::organizer() }
    pub fn program(pause: bool) -> CapturedProgram { crate::scope_fixture::program(pause) }
    pub fn query(entity: &str) -> serde_json::Value { crate::runtime_fixture::query(entity) }
    pub fn seed(program: &CapturedProgram) -> crate::product_contract::DataSnapshot { crate::runtime_fixture::seed(program) }
    pub fn decisions() -> crate::product_contract::DecisionGraph { crate::runtime_fixture::decisions() }
    pub fn add(name: &str) -> crate::product_contract::SemanticInput { crate::runtime_fixture::add(name) }
}

#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_json.rs"]
mod product_json;
#[path = "../src/product_app.rs"]
mod product_app;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;

use product_contract::*;
use product_store::{ProductStore, scope::*};
use std::sync::{Arc, atomic::AtomicBool};

fn cancel() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
fn witnesses(store: &ProductStore) -> Vec<LifecycleWitness> {
    let snapshot = store.load().unwrap();
    let row = |done: bool| {
        let record = snapshot.data.records.iter().find(|record| {
            record.entity == "job" && (record.values.get("status") == Some(&DataValue::Text { value: "done".into() })) == done
        }).unwrap();
        RecordRef { entity: record.entity.clone(), record: record.id.clone() }
    };
    [OwnedSourceSide::Before, OwnedSourceSide::After].into_iter().map(|side| LifecycleWitness {
        side,
        scenario: ScenarioSpec {
            version: 1, id: "capture-fixture".into(), label: "Capture fixture".into(),
            seed: snapshot.data.clone(), session: snapshot.session.clone(), clock_day: snapshot.clock_day,
            random_seed: 0, inputs: vec![SemanticInput::Observe { point: "capture".into() }], validity: vec![],
        },
        completed: row(true), unfinished: row(false),
    }).collect()
}

#[test]
fn committed_owned_lineage_can_continue_common_fact_recording() {
    let (_dir, store, a, b) = owned_recovery_fixture::setup_unrecorded_basis();
    let first = owned_recovery_fixture::prepare_with_operation(&store, &a, &b, "initial-owned");
    let choice = owned_recovery_fixture::policy_choice(&first);
    let scenes = owned_recovery_fixture::action_scenes(&store, &first);
    let change = owned_recovery_fixture::engine(&store)
        .prepare_owned_choice(&store, first, choice, scenes, "initial-owned")
        .unwrap();
    let committed = owned_recovery_fixture::engine(&store).adopt(&store, &change).unwrap();
    assert!(matches!(&committed.scope, ScopeEnvelope::OwnedV2(state) if !state.layers.is_empty()));

    let before = store.load().unwrap();
    // A new independent layer is authored on the exact active output of the
    // committed predecessor. The next compiler must carry its protected
    // metadata and behavior forward while adding this layer's own gates.
    let baseline = before.program().unwrap().clone();
    let mut app = baseline.program.clone();
    let add = app.actions.iter_mut().find(|action| action.id == "add").unwrap();
    add.steps.push(Statement::SetState {
        state: owned_recovery_fixture::MARKER.into(),
        value: crate::common::text(""),
    });
    let candidate = crate::common::capture(&app);
    let request = OwnedPreviewRequest {
        inventory: CommonFactInventoryRequest {
            snapshot: canonical_digest(IdentityDomain::Data, &before).unwrap(),
            sources: OwnedSourceRequest {
                before: canonical_digest(IdentityDomain::Source, &baseline).unwrap(),
                after: canonical_digest(IdentityDomain::Source, &candidate).unwrap(),
                root_entity: "job".into(),
                edits: vec![OwnedSpanEdit {
                    id: "continue-add".into(),
                    before: crate::common::endpoint(
                        &baseline,
                        "add",
                        vec![],
                        baseline.program.actions.iter().find(|action| action.id == "add").unwrap().steps.len(),
                        0,
                        None,
                    ),
                    after: crate::common::endpoint(
                        &candidate,
                        "add",
                        vec![],
                        baseline.program.actions.iter().find(|action| action.id == "add").unwrap().steps.len(),
                        1,
                        None,
                    ),
                }],
            },
            lifecycle: CommonFactLifecycleSuggestion {
                entity: "job".into(),
                before: crate::common::done("record"),
                after: crate::common::done("record"),
                meaning: "Done jobs remain completed history; other jobs remain unfinished".into(),
            },
        },
        auxiliary: vec![],
        population: ScopePopulation::All,
        excluded_records: vec![],
        operations: baseline.program.actions.iter().map(|action| action.id.clone()).collect(),
        availability: OwnedPreviewAvailability::NullableAddedBoolean,
        purpose: "Continue a committed OwnedV2 lineage".into(),
    };
    let mut host = LifecycleAcceptanceHost::prepare(
        &store,
        &candidate,
        &request,
        &witnesses(&store),
        ProofBudget::default(),
        cancel(),
    )
    .unwrap();
    let accepted = host.synthetic_operator_accept_for_test(&store).unwrap();
    let (continuation, report) = owned_recovery_fixture::engine(&store)
        .prepare_owned_change(
            &store,
            &candidate,
            &request,
            &accepted,
            "owned-continuation",
            ProofBudget::default(),
            cancel(),
        )
        .unwrap();
    // Existing committed decisions replay against the validated candidate.
    // Still require fresh operation-level action scenes below before adoption.
    assert_eq!(report.disposition, product_decisions::CheckDisposition::Ready);
    assert_eq!(store.load().unwrap(), before);
    assert_eq!(continuation.operation_id(), "owned-continuation");
    let scenes = owned_recovery_fixture::action_scenes(&store, &continuation);
    let mut choice = owned_recovery_fixture::policy_choice(&continuation);
    choice.id = "owned-policy-continuation".into();
    let decision = owned_recovery_fixture::engine(&store)
        .prepare_owned_choice(&store, continuation.clone(), choice, scenes, "owned-continuation")
        .unwrap();
    let committed = owned_recovery_fixture::engine(&store).adopt(&store, &decision).unwrap();
    assert!(matches!(&committed.scope, ScopeEnvelope::OwnedV2(state) if state.layers.len() == 2));
    assert_eq!(store.load().unwrap(), committed);

    // Refuse a reused operation ID atomically, and prove first-layer and
    // cumulative-manifest tampering is rejected before continuation admission.
    let retry_before = store.load().unwrap();
    assert!(store
        .apply(
            retry_before.revision,
            "owned-continuation",
            &owned_recovery_fixture::invoke(
                "add",
                [("title".into(), owned_recovery_fixture::value("duplicate retry"))].into(),
            ),
            RuntimeLimits::default(),
        )
        .is_err());
    assert_eq!(store.load().unwrap(), retry_before);
    for tamper in ["first-layer-operation", "latest-previous"] {
        let mut forged = committed.clone();
        let ScopeEnvelope::OwnedV2(state) = &mut forged.scope else { panic!("expected OwnedV2") };
        match tamper {
            "first-layer-operation" => {
                let first_id = state.compositions.values()
                    .find(|manifest| manifest.layers.len() == 1)
                    .unwrap().layers[0].clone();
                state.layers.get_mut(&first_id).unwrap().operation.push_str("-forged");
            }
            "latest-previous" => {
                state.compositions.values_mut()
                    .find(|manifest| manifest.layers.len() == 2)
                    .unwrap().previous = None;
            }
            _ => unreachable!(),
        }
        let untouched = forged.clone();
        assert!(forged.validate().is_err(),
            "tampered {tamper} must fail complete cold validation before admission");
        assert_eq!(forged, untouched);
        assert_eq!(store.load().unwrap(), committed);
    }

    let encoded = product_backup::VerifiedBackup::capture(&store)
        .unwrap()
        .to_bytes()
        .unwrap();
    let backup = product_backup::VerifiedBackup::from_bytes(&encoded).unwrap();
    assert_eq!(backup.snapshot(), &committed);
    let recovery = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let recent = product_locations::RecentTools::open(recovery.path()).unwrap();
    let created = backup
        .recover_tool(&recovery.path().join("recovered"), &recent, 1)
        .unwrap();
    assert!(created.registration.is_ok());
    drop(created);
    let recovered = product_backup::open_verified(&recovery.path().join("recovered"), None)
        .unwrap()
        .store;
    assert_eq!(recovered.load().unwrap(), committed);
    owned_recovery_fixture::add(&recovered, "post-recovery-add", "cold-reopen-job");
    let applied = recovered.load().unwrap();
    assert!(matches!(&applied.scope, ScopeEnvelope::OwnedV2(state) if state.layers.len() == 2));
    assert_eq!(
        product_backup::open_verified(&recovery.path().join("recovered"), None)
            .unwrap()
            .store
            .load()
            .unwrap(),
        applied
    );
    assert_eq!(store.load().unwrap(), committed);
}
