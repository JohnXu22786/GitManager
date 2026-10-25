//! Private reconstruction packet controls, independent of recovery setup.
use super::*;
#[test]
fn reconstruction_payments_enforce_exact_fit_and_refusal() {
    for cost in [
        RegistryOnly::FRAME_PAYMENT,
        RegistryOnly::FINISH_PAYMENT,
        TRANSITION_RECONSTRUCT_PAYMENT,
        TransitionFinish::FINISH_PAYMENT,
        INITIAL_ORIGIN_PAYMENT,
        HISTORICAL_ORIGIN_PAYMENT,
        GENERAL_TRANSITION_PAYMENT,
    ] {
        assert!(cost > 0);
        let make = |units| {
            ProofMeter::new(
                ProofBudget {
                    units,
                    elapsed_millis: 60_000,
                },
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
        };
        let mut exact = make(cost + 1);
        exact.charge(cost).unwrap();
        assert_eq!(exact.remaining_units_for_test(), 0);
        let mut short = make(cost);
        let left = short.remaining_units_for_test();
        assert!(short.charge(cost).is_err());
        assert_eq!(short.remaining_units_for_test(), left);
        for secondary in [false, true] {
            let token = Arc::new(AtomicBool::new(false));
            let mut m = if secondary {
                make(cost + 1)
            } else {
                ProofMeter::new(ProofBudget::default(), token.clone()).unwrap()
            };
            if secondary {
                m.watch_cancellation(&token);
            }
            token.store(true, std::sync::atomic::Ordering::Relaxed);
            let left = m.remaining_units_for_test();
            assert!(m
                .charge(cost)
                .unwrap_err()
                .to_string()
                .contains("cancelled or cumulative elapsed"));
            assert_eq!(m.remaining_units_for_test(), left);
        }
        let mut elapsed = ProofMeter::new(
            ProofBudget {
                units: cost + 1,
                elapsed_millis: 1,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(elapsed
            .charge(cost)
            .unwrap_err()
            .to_string()
            .contains("elapsed"));
    }
}

#[test]
fn actual_reconstruction_layouts_cover_complete_headers() {
    // These independent 64-bit pins are also used by the route oracles below.
    // In particular, all four Result<Self::Output> transports include the
    // transition's extra Arc, not only the successful output payload.
    assert_eq!(RegistryOnly::FRAME_PAYMENT, REGISTRY_FRAME);
    assert_eq!(RegistryOnly::FINISH_PAYMENT, REGISTRY_FINISH);
    assert_eq!(RegistryOnly::PREFIX_SCOPE_PAYMENT, REGISTRY_PREFIX_SCOPE);
    assert_eq!(RegistryOnly::PREFIX_INTERVAL_PAYMENT, REGISTRY_PREFIX_INTERVAL);
    assert_eq!(TRANSITION_RECONSTRUCT_PAYMENT, TRANSITION_FRAME);
    assert_eq!(TransitionFinish::FINISH_PAYMENT, TRANSITION_FINISH);
    assert_eq!(TransitionFinish::PREFIX_SCOPE_PAYMENT, TRANSITION_PREFIX_SCOPE);
    assert_eq!(TransitionFinish::PREFIX_INTERVAL_PAYMENT, TRANSITION_PREFIX_INTERVAL);
    assert_eq!(GENERAL_TRANSITION_PAYMENT, GENERAL_TRANSPORT);
    assert_eq!(owned_continuation::STRUCTURAL_ENTRY_PAYMENT, STRUCTURAL_ENTRY);
    assert_eq!(owned_continuation::RECONSTRUCTED_STRUCTURAL_ENTRY_PAYMENT, RECONSTRUCTED_ENTRY);
    assert_eq!(owned_continuation::STRUCTURAL_PAIR_PAYMENT, STRUCTURAL_PAIR);
    assert_eq!(RECONSTRUCTED_ORIGIN_PAYMENT, RECONSTRUCTED_ORIGIN);
    assert_eq!(COLD_HISTORICAL_SERIALIZATION_PAYMENT, COLD_SERIALIZATION);
    assert_eq!(owned_continuation::COMPARISON_DISPATCH_PAYMENT, COMPARISON_DISPATCH);
    println!(
        "RECONSTRUCTION_LAYOUTS {}",
        serde_json::json!({
            "fresh_origin":size_of::<FreshInitialOrigin<'_>>(),
            "optional_origin":size_of::<Option<FreshInitialOrigin<'_>>>(),
            "optional_origin_borrow":size_of::<Option<&FreshInitialOrigin<'_>>>(),
            "registry":size_of::<ReconstructedOwned>(), "registry_result":size_of::<Result<ReconstructedOwned>>(),
            "registry_finish":size_of::<RegistryOnly>(), "transition_finish":size_of::<TransitionFinish<'_>>(),
            "transition_output":size_of::<(ReconstructedOwned,Arc<VerifiedOwnedTransitionExecution>)>(),
            "transition_output_result":size_of::<Result<(ReconstructedOwned,Arc<VerifiedOwnedTransitionExecution>)>>(),
            "historical_result":size_of::<Result<VerifiedOwnedTransitionExecution>>(),
            "r1_registry":RegistryOnly::FRAME_PAYMENT, "r1_transition":TRANSITION_RECONSTRUCT_PAYMENT,
            "r2":INITIAL_ORIGIN_PAYMENT, "r3":HISTORICAL_ORIGIN_PAYMENT,
            "r4_registry":RegistryOnly::FINISH_PAYMENT, "r4_transition":TransitionFinish::FINISH_PAYMENT,
            "direct_general":GENERAL_TRANSITION_PAYMENT,
        })
    );
    assert!(RegistryOnly::FRAME_PAYMENT > 2 * size_of::<Result<ReconstructedOwned>>() as u64);
    assert!(
        GENERAL_TRANSITION_PAYMENT
            > 4 * size_of::<Result<Arc<VerifiedOwnedTransitionExecution>>>() as u64
    );
}

#[path = "../../../tests/fixtures/common_fact/mod.rs"]
mod fixture;
fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn meter() -> ProofMeter {
    ProofMeter::new(ProofBudget::default(), cancel()).unwrap()
}
fn text(value: &str) -> DataValue {
    DataValue::Text {
        value: value.into(),
    }
}
fn invoke(action: &str, arguments: Values) -> SemanticInput {
    SemanticInput::Invoke {
        action: action.into(),
        arguments,
    }
}
fn apply(store: &ProductStore, operation: &str, input: SemanticInput) {
    let current = store.load().unwrap();
    store
        .apply(
            current.revision,
            operation,
            &input,
            RuntimeLimits::default(),
        )
        .unwrap();
}
fn row(snapshot: &ProjectSnapshot, title: &str) -> RecordRef {
    let row = snapshot
        .data
        .records
        .iter()
        .find(|r| r.entity == "job" && r.values.get("title") == Some(&text(title)))
        .unwrap();
    RecordRef {
        entity: row.entity.clone(),
        record: row.id.clone(),
    }
}
// Small synthetic component fixture, not the owned_recovery setup/strict journey.
// Only this helper uses the pre-existing synthetic receipt builder; opaque
// preview preparation and all reconstruction checks still execute normally.
fn initial_fixture() -> (
    ProjectSnapshot,
    CapturedProgram,
    CapturedProgram,
    OwnedSourceRequest,
) {
    initial_fixture_with_alternatives(0)
}
fn initial_fixture_with_alternatives(
    alternatives: usize,
) -> (
    ProjectSnapshot,
    CapturedProgram,
    CapturedProgram,
    OwnedSourceRequest,
) {
    let (a, b) = fixture::pair();
    let (mut a, mut b) = (a.program, b.program);
    for app in [&mut a, &mut b] {
        app.state.push(StateDefinition {
            id: "reuse_marker".into(),
            label: "Reuse marker".into(),
            value_type: Type::Text,
            initial: text(""),
        });
        app.actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap()
            .steps
            .push(Statement::SetState {
                state: "reuse_marker".into(),
                value: fixture::text(""),
            });
    }
    let (a, b) = (fixture::capture(&a), fixture::capture(&b));
    let dir = tempfile::tempdir().unwrap();
    let mut store = ProductStore::create(dir.path().join("component"), &a, 20000).unwrap();
    for title in ["live", "finished"] {
        apply(
            &store,
            &format!("add-{title}"),
            invoke("add", [("title".into(), text(title))].into()),
        );
    }
    let finished = row(&store.load().unwrap(), "finished");
    apply(
        &store,
        "finish",
        invoke(
            "complete",
            [(
                "job".into(),
                DataValue::Reference {
                    entity: finished.entity,
                    record: finished.record,
                },
            )]
            .into(),
        ),
    );
    for index in 0..alternatives {
        let preview = preview_component(&store, &a, &b);
        let pending =
            owned_snapshot_for_test(&preview, &format!("alternative-{index}"), true).unwrap();
        pending.validate().unwrap();
        store = ProductStore::create_recovered_with(
            dir.path().join(format!("pending-{index}")),
            &pending,
            |_| Ok(()),
        )
        .unwrap();
    }
    let request = fixture::request(&store, &a, &b);
    let preview = preview_component(&store, &a, &b);
    let snapshot = owned_snapshot_for_test(&preview, "reuse-initial", false).unwrap();
    snapshot.validate().unwrap();
    (snapshot, a, b, request.inventory.sources)
}
fn preview_component(
    store: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
) -> PreparedOwnedPreview {
    let current = store.load().unwrap();
    let request = fixture::request(store, a, b);
    let witnesses: Vec<_> = [OwnedSourceSide::Before, OwnedSourceSide::After]
        .into_iter()
        .map(|side| LifecycleWitness {
            side,
            scenario: ScenarioSpec {
                version: 1,
                id: "reuse-lifecycle".into(),
                label: "Synthetic reuse component".into(),
                seed: current.data.clone(),
                session: current.session.clone(),
                clock_day: current.clock_day,
                random_seed: 0,
                inputs: vec![SemanticInput::Observe {
                    point: "witness".into(),
                }],
                validity: vec![],
            },
            completed: row(&current, "finished"),
            unfinished: row(&current, "live"),
        })
        .collect();
    let accepted = LifecycleAcceptanceHost::prepare(
        store,
        b,
        &request,
        &witnesses,
        ProofBudget::default(),
        cancel(),
    )
    .unwrap()
    .synthetic_operator_accept_for_test(store)
    .unwrap();
    let proof = store
        .verify_owned_preview(b, &request, &accepted, ProofBudget::default(), cancel())
        .unwrap();
    let preview = store
        .prepare_owned_preview(&proof, ProofBudget::default(), cancel())
        .unwrap();
    preview
}
fn evolved(
    a: &CapturedProgram,
    b: &CapturedProgram,
    sources: &OwnedSourceRequest,
    marker: &str,
) -> (CapturedProgram, CapturedProgram, OwnedSourceRequest) {
    let (mut a, mut b) = (a.program.clone(), b.program.clone());
    for app in [&mut a, &mut b] {
        let Statement::SetState { value, .. } = app
            .actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap()
            .steps
            .last_mut()
            .unwrap()
        else {
            panic!()
        };
        *value = fixture::text(marker);
    }
    let (a, b) = (fixture::capture(&a), fixture::capture(&b));
    let mut sources = sources.clone();
    sources.before = revision(&a).unwrap();
    sources.after = revision(&b).unwrap();
    for edit in &mut sources.edits {
        for (source, endpoint) in [(&a, &mut edit.before), (&b, &mut edit.after)] {
            endpoint.source = revision(source).unwrap();
        }
    }
    (a, b, sources)
}
fn assert_execution_equal(
    a: &VerifiedOwnedTransitionExecution,
    b: &VerifiedOwnedTransitionExecution,
) {
    a.continuation
        .assert_initial_reuse_same_value(&b.continuation);
    assert_eq!(a.target, b.target);
    assert_eq!(a.target.source_bytes, b.target.source_bytes);
    assert_eq!(a.metadata, b.metadata);
    assert_eq!(a.manifest, b.manifest);
    assert_eq!(a.initialization, b.initialization);
    assert_eq!(a.seed(), b.seed());
    assert_eq!(a.original.target, b.original.target);
    assert_eq!(a.original.proof.request, b.original.proof.request);
    assert_eq!(a.original.proof.report, b.original.proof.report);
}
#[test]
fn fresh_first_evolution_matches_general() {
    let (current, a, b, sources) = initial_fixture();
    let (a, b, sources) = evolved(&a, &b, &sources, "first");
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
    let mut fresh_meter = meter();
    fresh_meter.charge(TRANSITION_RECONSTRUCT_PAYMENT).unwrap();
    let (fresh_registry, fresh) = reconstruct_and_transition(
        &current,
        &a,
        &b,
        &sources,
        layer,
        "first-evolution",
        false,
        &mut fresh_meter,
    )
    .unwrap();
    let mut general_meter = meter();
    let general_registry = reconstruct(&current, &mut general_meter).unwrap();
    let general = transition_from_registry(
        &current,
        &general_registry,
        &a,
        &b,
        &sources,
        layer,
        "first-evolution",
        false,
        &mut general_meter,
    )
    .unwrap();
    assert_execution_equal(&fresh, &general);
    assert_eq!(fresh_registry.snapshot, general_registry.snapshot);
    assert_eq!(fresh_registry.epochs.len(), 1);
    assert!(fresh_meter.remaining_units_for_test() > general_meter.remaining_units_for_test());
    assert_eq!(
        Arc::strong_count(fresh_registry.proofs.values().next().unwrap()),
        3,
        "fresh origin must not escape finish"
    );
    drop(fresh);
    assert_eq!(
        Arc::strong_count(fresh_registry.proofs.values().next().unwrap()),
        2
    );
    let mut calls = 0;
    let prepared = owned_transition::prepare_owned_evolution(
        &current,
        &a,
        &b,
        &sources,
        layer,
        "callback",
        false,
        ProofBudget::default(),
        cancel(),
        None,
        |gate| {
            calls += 1;
            assert_eq!(gate.execution()?.current(), &current);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(prepared.seed(), &current.data);
    println!(
        "FRESH_GENERAL_COMPONENT {}",
        serde_json::json!({"fresh_paid":10_000_000-fresh_meter.remaining_units_for_test(),"general_paid":10_000_000-general_meter.remaining_units_for_test(),"net_saved":fresh_meter.remaining_units_for_test()-general_meter.remaining_units_for_test()})
    );
    let (with_alternatives, a, b, s) = initial_fixture_with_alternatives(2);
    assert_eq!(with_alternatives.owned_scope().unwrap().rehearsals.len(), 2);
    let (a, b, s) = evolved(&a, &b, &s, "after-alternatives");
    compare_routes(
        &with_alternatives,
        &a,
        &b,
        &s,
        "after-alternatives",
        false,
        true,
        1,
    );
}

// Frozen packet ledger, independent of production formulas and final balances.
// Values are pinned separately above so a production accounting change cannot
// silently update either normalization or the short/exact boundary oracle.
const REGISTRY_FRAME: u64 = 1312;
const REGISTRY_FINISH: u64 = 1376;
const REGISTRY_PREFIX_SCOPE: u64 = 5296;
const REGISTRY_PREFIX_INTERVAL: u64 = 21148;
const TRANSITION_FRAME: u64 = 1456;
const TRANSITION_FINISH: u64 = 1664;
const TRANSITION_PREFIX_SCOPE: u64 = 5328;
const TRANSITION_PREFIX_INTERVAL: u64 = 21180;
const GENERAL_TRANSPORT: u64 = 1028;
const STRUCTURAL_ENTRY: u64 = 97216;
const RECONSTRUCTED_ENTRY: u64 = 43136;
const STRUCTURAL_PAIR: u64 = 25104;
const RECONSTRUCTED_ORIGIN: u64 = 4736;
const COLD_SERIALIZATION: u64 = 8980;
// Two complete continuation results grew by one Arc (8 bytes) each.
const COMPARISON_DISPATCH: u64 = 6016;
const DISPATCH_GROWTH: u64 = COMPARISON_DISPATCH - 6000;
// The immutable fixture/input digests asserted below bind these independently
// pinned Before/After evidence lengths, including historical/later pairs.
// Canonical reuse replaced 1 + 2*bytes with 16_785 + bytes per comparison;
// derive the removed work, never calibrate from a completed route's balance.
// The complete 16_785 packet is pinned by the continuation layout controls.
const CANONICAL_EVIDENCE_BYTES: [u64; 2] = [119_626, 104_174];
const CANONICAL_SAVING_PER_PAIR: u64 =
    CANONICAL_EVIDENCE_BYTES[0] + CANONICAL_EVIDENCE_BYTES[1] - 2 * (16_785 - 1);

fn assert_accounting_fixture(
    current: &ProjectSnapshot, before: &CapturedProgram, after: &CapturedProgram,
    sources: &OwnedSourceRequest, expected: &str,
) {
    // Bind all canonical-evidence inputs, not a meter result, to the frozen
    // byte-length ledger. Fixture/source drift requires explicit reconciliation.
    assert_eq!(
        canonical_digest(IdentityDomain::Evidence, &(current, before, after, sources))
            .unwrap().as_str(),
        expected,
        "accounting fixture/input identity changed",
    );
}

fn assert_epoch_topology(current: &ProjectSnapshot, registry: &ReconstructedOwned, expected: usize) {
    let state = current.owned_scope().unwrap();
    assert_eq!(state.layers.len(), 1);
    assert_eq!(state.initializations.len(), 1);
    assert_eq!(state.adoptions.len(), expected);
    assert_eq!(state.compositions.len(), expected);
    assert_eq!(registry.epochs.len(), expected);
    for (epoch, receipt) in registry.epochs.iter().zip(&state.adoptions) {
        assert_eq!(epoch.adoption_revision, receipt.revision);
        assert_eq!(epoch.output, state.compositions[&receipt.composition].output);
    }
}

fn compare_routes(
    current: &ProjectSnapshot,
    a: &CapturedProgram,
    b: &CapturedProgram,
    sources: &OwnedSourceRequest,
    operation: &str,
    withdrawal: bool,
    hit: bool,
    epochs: usize,
) -> Arc<VerifiedOwnedTransitionExecution> {
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
    let mut fresh_meter = meter();
    fresh_meter.charge(TRANSITION_RECONSTRUCT_PAYMENT).unwrap();
    let (fresh_registry, fresh) = reconstruct_and_transition(
        current,
        a,
        b,
        sources,
        layer,
        operation,
        withdrawal,
        &mut fresh_meter,
    )
    .unwrap();
    let mut general_meter = meter();
    let registry = reconstruct(current, &mut general_meter).unwrap();
    let general = transition_from_registry(
        current,
        &registry,
        a,
        b,
        sources,
        layer,
        operation,
        withdrawal,
        &mut general_meter,
    )
    .unwrap();
    assert_execution_equal(&fresh, &general);
    assert_epoch_topology(current, &fresh_registry, epochs);
    assert_epoch_topology(current, &registry, epochs);
    // Prefixes contain 1..k-1 epochs at historical reconstruction and k at
    // the final endpoint: k scopes and k(k+1)/2 intervals. Prospective
    // rehearsals are not committed producer epochs.
    let k = epochs as u64;
    let intervals = k * (k + 1) / 2;
    let registry_wrappers = REGISTRY_FRAME + REGISTRY_FINISH
        + k * REGISTRY_PREFIX_SCOPE + intervals * REGISTRY_PREFIX_INTERVAL;
    let transition_wrappers = TRANSITION_FRAME + TRANSITION_FINISH
        + k * TRANSITION_PREFIX_SCOPE + intervals * TRANSITION_PREFIX_INTERVAL;
    let wrapper_saving = registry_wrappers as i64 + GENERAL_TRANSPORT as i64
        - transition_wrappers as i64;
    let saving = fresh_meter.remaining_units_for_test() as i64
        - general_meter.remaining_units_for_test() as i64;
    if hit {
        assert!(saving > wrapper_saving);
    } else {
        assert_eq!(
            saving, wrapper_saving,
            "missing provenance must retain all four comparisons"
        );
    }
    println!(
        "ROUTE_COMPONENT {}",
        serde_json::json!({"operation":operation,"historical_epochs":registry.epochs.len(),"new_hit":hit,"fresh_paid":10_000_000-fresh_meter.remaining_units_for_test(),"general_paid":10_000_000-general_meter.remaining_units_for_test(),"net_saved":saving})
    );
    fresh
}
fn adopt_component(
    store: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
    sources: &OwnedSourceRequest,
    operation: &str,
    withdrawal: bool,
) -> ProjectSnapshot {
    let current = store.load().unwrap();
    assert!(
        current.decisions.decisions.is_empty(),
        "no-op policy callback is limited to the empty synthetic graph"
    );
    assert!(current
        .owned_scope()
        .unwrap()
        .adoptions
        .iter()
        .all(|r| r.decisions.is_empty()));
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
    let mut prepare_calls = 0;
    let prepared = owned_transition::prepare_owned_evolution(
        &current,
        a,
        b,
        sources,
        layer,
        operation,
        withdrawal,
        ProofBudget::default(),
        cancel(),
        None,
        |gate| {
            prepare_calls += 1;
            assert_eq!(gate.execution()?.current(), &current);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(prepare_calls, 1);
    let plan = AdoptionPlan {
        version: CONTRACT_VERSION,
        id: operation.into(),
        project_id: current.data.project_id.clone(),
        expected_generation: current.data.generation,
        expected_data: current.data.identity().unwrap(),
        expected_decisions: current.decisions.identity().unwrap(),
        expected_session: current.session.identity().unwrap(),
        current_source: current.program().unwrap().binding.clone(),
        target: prepared.target().artifact.clone(),
        scope: prepared.scope(),
        compatibility: LocalRuntime::default()
            .compatibility_at(prepared.target(), &current.data, current.clock_day)
            .unwrap(),
        required_decisions: vec![],
        checks: vec![],
        evidence: vec![],
        retire_decisions: vec![],
    };
    let adoption = store
        .prepare_owned_transition_adoption(plan, &prepared, &current.decisions)
        .unwrap();
    let mut adoption_calls = 0;
    let next = store
        .adopt_owned_transition_verified(
            current.revision,
            &prepared,
            &adoption,
            &current.decisions,
            None,
            |before, fresh, graph, _plan, next, gate, _archive_policy| {
                adoption_calls += 1;
                assert_eq!(before, &current);
                assert_eq!(fresh.seed(), &current.data);
                assert_eq!(graph, &current.decisions);
                assert_eq!(next.data, current.data);
                assert_eq!(gate.execution()?.current(), &current);
                Ok(())
            },
            |_| panic!("not a retry"),
            None,
        )
        .unwrap();
    assert_eq!(adoption_calls, 1);
    assert_eq!(next.data, current.data);
    assert_eq!(next.session, current.session);
    assert_eq!(next.clock_day, current.clock_day);
    assert_eq!(
        &next.adoptions[..current.adoptions.len()],
        current.adoptions.as_slice()
    );
    next
}
#[test]
fn historical_and_later_routes_preserve_fallback() {
    let (initial, a0, b0, s0) = initial_fixture();
    compare_routes(
        &initial,
        &a0,
        &b0,
        &s0,
        "first-withdrawal-control",
        true,
        false,
        1,
    );
    let dir = tempfile::tempdir().unwrap();
    let store =
        ProductStore::create_recovered_with(dir.path().join("history"), &initial, |_| Ok(()))
            .unwrap();
    let (a1, b1, s1) = evolved(&a0, &b0, &s0, "one");
    let expected = compare_routes(&initial, &a1, &b1, &s1, "evolve-one", false, true, 1);
    let first = adopt_component(&store, &a1, &b1, &s1, "evolve-one", false);
    let registry = reconstruct(&first, &mut meter()).unwrap();
    let OwnedExecutionRecipe::Transition(actual) =
        registry.recipes.get(&first.active_revision).unwrap()
    else {
        panic!()
    };
    assert_execution_equal(actual, &expected);
    apply(
        &store,
        "add-after-first",
        invoke("add", [("title".into(), text("after-first"))].into()),
    );
    let current = store.load().unwrap();
    assert!(current.data.events.len() > first.data.events.len());
    assert_eq!(
        &current.data.events[..first.data.events.len()],
        first.data.events.as_slice()
    );
    assert_eq!(row(&current, "after-first").entity, "job");
    let (a2, b2, s2) = evolved(&a1, &b1, &s1, "two");
    compare_routes(&current, &a2, &b2, &s2, "evolve-two", false, false, 2);
    assert_historical_route_boundaries(&current, &a2, &b2, &s2);
    let second = adopt_component(&store, &a2, &b2, &s2, "evolve-two", false);
    compare_routes(&second, &a2, &b2, &s2, "withdraw", true, false, 3);
    let withdrawn = adopt_component(&store, &a2, &b2, &s2, "withdraw", true);
    let (a3, b3, s3) = evolved(&a2, &b2, &s2, "three");
    compare_routes(&withdrawn, &a3, &b3, &s3, "re-evolve", false, false, 4);
    let last = adopt_component(&store, &a3, &b3, &s3, "re-evolve", false);
    let registry = reconstruct(&last, &mut meter()).unwrap();
    assert_eq!(registry.epochs.len(), 5);
    assert_eq!(last.data, current.data);
    assert_eq!(last.data.events, current.data.events);
    // A later invalid endpoint must still invalidate the complete historical reconstruction.
    let mut bad = last.clone();
    bad.clock_day += 1;
    assert!(reconstruct(&bad, &mut meter()).is_err());
}

// Frozen prior-prefix vectors were observed on the real closed/general routes,
// not inferred from a successful final balance. Compose later packet additions
// explicitly from the independent ledger above, rather than remeasuring balances.
// They include ProofMeter::new's unit and use the original host without refunds:
// removing or moving an upstream production debit must still break the oracle.
#[allow(clippy::too_many_arguments)]
fn assert_route_boundary(
    current: &ProjectSnapshot, before: &CapturedProgram, after: &CapturedProgram,
    sources: &OwnedSourceRequest, registry: Option<&ReconstructedOwned>,
    operation: &str, route: u8, label: &str, prefix: u64, cost: u64,
    exact_success: bool,
) {
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
    let frozen = canonical_bytes(current).unwrap();
    for exact in [false, true] {
        let units = prefix + cost - u64::from(!exact);
        let mut limited = ProofMeter::new(ProofBudget { units, ..ProofBudget::default() }, cancel()).unwrap();
        let result = match route {
            0 => reconstruct(current, &mut limited).map(|_| ()),
            1 => transition(current, before, after, sources, layer, operation, false, &mut limited).map(|_| ()),
            2 => transition_from_registry(current, registry.unwrap(), before, after, sources, layer, operation, false, &mut limited).map(|_| ()),
            _ => unreachable!(),
        };
        if exact && exact_success { assert!(result.is_ok(), "{label}"); }
        else {
            assert!(result.expect_err(label).to_string().contains("cumulative proof/initialization budget exhausted"), "{label}");
        }
        assert_eq!(limited.remaining_units_for_test(), if exact { 0 } else { cost - 1 }, "{label}: refused debit is not paid and no later debit runs");
        assert_eq!(canonical_bytes(current).unwrap(), frozen);
        println!("ROUTE_BOUNDARY {{\"label\":\"{label}\",\"exact\":{exact},\"prefix\":{prefix},\"cost\":{cost},\"remaining\":{}}}", limited.remaining_units_for_test());
    }
    if route == 1 {
        let mut callbacks = 0;
        let result = owned_transition::prepare_owned_evolution(current, before, after, sources, layer, operation, false,
            ProofBudget { units: prefix + cost - 1, ..ProofBudget::default() }, cancel(), None,
            |_| { callbacks += 1; Ok(()) });
        assert!(result.err().unwrap().to_string().contains("cumulative proof/initialization budget exhausted"));
        assert_eq!(callbacks, 0, "{label}: no downstream policy callback");
    }
}
fn assert_initial_route_boundaries(current: &ProjectSnapshot, before: &CapturedProgram, after: &CapturedProgram,
    sources: &OwnedSourceRequest, registry: &ReconstructedOwned) {
    assert_accounting_fixture(current, before, after, sources,
        "20bac07bcd3177776d9f2bfd7fa547e8e12162c65e2f35911eb59f93042b9ab7");
    assert_epoch_topology(current, registry, 1);
    let registry_prefix = REGISTRY_PREFIX_SCOPE + REGISTRY_PREFIX_INTERVAL;
    let transition_prefix = TRANSITION_PREFIX_SCOPE + TRANSITION_PREFIX_INTERVAL;
    let fresh_entry = transition_prefix + STRUCTURAL_ENTRY;
    let fresh_pair = fresh_entry + DISPATCH_GROWTH + STRUCTURAL_PAIR;
    let general_pair = STRUCTURAL_ENTRY + DISPATCH_GROWTH + STRUCTURAL_PAIR;
    for (label, prefix, cost, end) in [
        ("registry-r1", 1, 1312, false),
        ("registry-r2", 2_555_127, 208, false),
        ("registry-r4", 2_562_272 + registry_prefix, 1376, true),
    ] { assert_route_boundary(current, before, after, sources, None, "drop-control", 0, label, prefix, cost, end); }
    for (label, prefix, cost) in [
        ("fresh-r1", 1, 1456), ("fresh-r2", 2_555_271, 208),
        ("fresh-r4", 2_562_416 + transition_prefix, 1664), ("fresh-r5", 2_564_080 + fresh_entry, COMPARISON_DISPATCH),
        ("fresh-r6", 2_570_080 + fresh_entry + DISPATCH_GROWTH, 8128), ("fresh-first-hit", 3_248_886 + fresh_pair - CANONICAL_SAVING_PER_PAIR, 328),
        ("fresh-second-hit", 3_249_214 + fresh_pair - CANONICAL_SAVING_PER_PAIR, 328), ("fresh-r8-view", 3_249_552 + fresh_pair - CANONICAL_SAVING_PER_PAIR, 704),
        ("fresh-r8-query", 3_250_256 + fresh_pair - CANONICAL_SAVING_PER_PAIR, 1840),
    ] { assert_route_boundary(current, before, after, sources, None, "drop-control", 1, label, prefix, cost, false); }
    for (label, prefix, cost) in [
        ("general-transport", 1, 1028), ("general-r5", 1029 + STRUCTURAL_ENTRY, COMPARISON_DISPATCH),
        ("general-r6", 7029 + STRUCTURAL_ENTRY + DISPATCH_GROWTH, 8128), ("general-r8-view", 1_315_405 + general_pair - 2 * CANONICAL_SAVING_PER_PAIR, 704),
        ("general-r8-query", 1_316_109 + general_pair - 2 * CANONICAL_SAVING_PER_PAIR, 1840),
    ] { assert_route_boundary(current, before, after, sources, Some(registry), "ordinary", 2, label, prefix, cost, false); }
}
fn assert_historical_route_boundaries(current: &ProjectSnapshot, before: &CapturedProgram, after: &CapturedProgram,
    sources: &OwnedSourceRequest) {
    assert_accounting_fixture(current, before, after, sources,
        "fed424ae9e77bdb90745bbed2ae89f4c3d2741ccdc259ebf59d88fccca913718");
    let registry = reconstruct(current, &mut meter()).unwrap();
    assert_epoch_topology(current, &registry, 2);
    let state = current.owned_scope().unwrap();
    let historical = &state.compositions[&state.adoptions[1].composition].basis.frozen.data;
    // Independently pin the fixture bytes whose formerly redundant native
    // serialization was removed. This is not a measured route debit/balance.
    const HISTORICAL_DATA_BYTES: u64 = 6934;
    assert_eq!(canonical_bytes(historical).unwrap().len() as u64, HISTORICAL_DATA_BYTES);
    let first_recipe = &registry.recipes[&registry.epochs[0].output];
    let next_recipe = &registry.recipes[&registry.epochs[1].output];
    assert_eq!(first_recipe.seed(), historical);
    assert_eq!(next_recipe.seed(), historical);
    assert_eq!(historical.records.len(), 2);
    assert!(historical.records.iter().all(|row| owned_replay::owned_entity(first_recipe, &row.entity)));
    // The final default-window traversal reuses this completed interval:
    // no new events/shared rows, two record-inventory units and one empty
    // revision-allocation unit, in addition to its canonical data bytes.
    let reused_interval = HISTORICAL_DATA_BYTES + 2 + 1;
    let historical_prefix = TRANSITION_PREFIX_SCOPE + TRANSITION_PREFIX_INTERVAL
        + COLD_SERIALIZATION - HISTORICAL_DATA_BYTES;
    let historical_entry = historical_prefix + (RECONSTRUCTED_ORIGIN - 3584) + RECONSTRUCTED_ENTRY;
    let historical_pair = historical_entry + DISPATCH_GROWTH + STRUCTURAL_PAIR;
    // The second traversal adds one scope and two intervals before final R4.
    let final_prefix = historical_pair + TRANSITION_PREFIX_SCOPE + 2 * TRANSITION_PREFIX_INTERVAL
        - reused_interval;
    let final_entry = final_prefix + STRUCTURAL_ENTRY;
    let final_pair = final_entry + DISPATCH_GROWTH + STRUCTURAL_PAIR;
    for (label, prefix, cost) in [
        ("history-r3", 2_576_424 + historical_prefix, RECONSTRUCTED_ORIGIN), ("history-r5", 2_580_008 + historical_entry, COMPARISON_DISPATCH),
        ("history-r6", 2_586_008 + historical_entry + DISPATCH_GROWTH, 8128), ("history-first-hit", 3_264_820 + historical_pair - CANONICAL_SAVING_PER_PAIR, 328),
        ("history-second-hit", 3_265_148 + historical_pair - CANONICAL_SAVING_PER_PAIR, 328), ("history-r8-view", 3_267_126 + historical_pair - CANONICAL_SAVING_PER_PAIR, 704),
        ("history-r8-query", 3_267_830 + historical_pair - CANONICAL_SAVING_PER_PAIR, 1840), ("later-r4", 3_348_552 + final_prefix - CANONICAL_SAVING_PER_PAIR, 1664),
        ("later-general-r5", 3_350_216 + final_entry - CANONICAL_SAVING_PER_PAIR, COMPARISON_DISPATCH), ("later-general-r6", 3_356_216 + final_entry + DISPATCH_GROWTH - CANONICAL_SAVING_PER_PAIR, 8128),
        ("later-general-r8-view", 4_666_238 + final_pair - 3 * CANONICAL_SAVING_PER_PAIR, 704), ("later-general-r8-query", 4_666_942 + final_pair - 3 * CANONICAL_SAVING_PER_PAIR, 1840),
    ] { assert_route_boundary(current, before, after, sources, None, "evolve-two", 1, label, prefix, cost, false); }
}

#[test]
fn original_errors_and_current_frame_remain_visible() {
    let (current, a, b, sources) = initial_fixture();
    let registry = reconstruct(&current, &mut meter()).unwrap();
    assert_initial_route_boundaries(&current, &a, &b, &sources, &registry);
    let original = registry.proofs.values().next().unwrap();
    original.assert_initial_comparison_error_order();
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
    // A caller-built Initial-looking registry is still the direct General API.
    let mut forged = ReconstructedOwned {
        snapshot: registry.snapshot.clone(),
        recipes: registry.recipes.clone(),
        epochs: registry
            .epochs
            .iter()
            .map(|e| OwnedProducerEpoch {
                output: e.output.clone(),
                adoption_revision: e.adoption_revision,
            })
            .collect(),
        proofs: registry.proofs.clone(),
        manifests: registry.manifests.clone(),
        initializations: registry.initializations.clone(),
    };
    forged.recipes.insert(
        current.active_revision.clone(),
        OwnedExecutionRecipe::Initial(original.clone()),
    );
    let mut ordinary_meter = meter();
    let ordinary = transition_from_registry(
        &current,
        &registry,
        &a,
        &b,
        &sources,
        layer,
        "ordinary",
        false,
        &mut ordinary_meter,
    )
    .unwrap();
    let mut forged_meter = meter();
    let forged_result = transition_from_registry(
        &current,
        &forged,
        &a,
        &b,
        &sources,
        layer,
        "ordinary",
        false,
        &mut forged_meter,
    )
    .unwrap();
    assert_execution_equal(&ordinary, &forged_result);
    assert_eq!(
        ordinary_meter.remaining_units_for_test(),
        forged_meter.remaining_units_for_test()
    );
    for case in 0..7 {
        let mut bad = current.clone();
        match case {
            0 => bad.clock_day += 1,
            1 => bad.session.view = "foreign".into(),
            2 => bad.data.generation += 1,
            3 => bad.programs[0].binding.program_path = "foreign/program.json".into(),
            4 => {
                if let ScopeEnvelope::OwnedV2(state) = &mut bad.scope {
                    state.initializations[0].version += 1;
                }
            }
            5 => {
                if let ScopeEnvelope::OwnedV2(state) = &mut bad.scope {
                    state.adoptions[0].revision += 1;
                }
            }
            _ => bad.active_revision = revision(&a).unwrap(),
        }
        assert!(reconstruct(&bad, &mut meter()).is_err(), "case {case}");
        assert!(transition_from_registry(
            &bad,
            &registry,
            &a,
            &b,
            &sources,
            layer,
            "bad-current",
            false,
            &mut meter()
        )
        .is_err());
    }
    let mut calls = 0;
    let error = owned_transition::prepare_owned_evolution(
        &current,
        &a,
        &b,
        &sources,
        layer,
        "reuse-initial",
        false,
        ProofBudget::default(),
        cancel(),
        None,
        |_| {
            calls += 1;
            Ok(())
        },
    )
    .err()
    .unwrap();
    assert_eq!(calls, 0);
    assert!(matches!(error, StoreError::Conflict(_)));
    for case in 0..8 {
        let (mut bad_before, mut bad_after, mut bad_sources) =
            (a.clone(), b.clone(), sources.clone());
        match case {
            0 => bad_before.source_bytes.push(b' '),
            1 => bad_after.binding.program_path = "foreign/program.json".into(),
            2 => bad_before.program.id = "foreign-app".into(),
            3 => bad_after.artifact.program_digest = bad_before.artifact.program_digest.clone(),
            4 => bad_sources.root_entity = "foreign-root".into(),
            5 => bad_sources.edits[0].id = "foreign-slot".into(),
            6 => bad_sources.before = bad_sources.after.clone(),
            _ => bad_sources.edits[0].after.start += 1,
        }
        let mut fresh_meter = meter();
        fresh_meter.charge(TRANSITION_RECONSTRUCT_PAYMENT).unwrap();
        let fresh_error = reconstruct_and_transition(
            &current,
            &bad_before,
            &bad_after,
            &bad_sources,
            layer,
            "bad-source",
            false,
            &mut fresh_meter,
        )
        .err()
        .unwrap();
        let general_error = transition_from_registry(
            &current,
            &registry,
            &bad_before,
            &bad_after,
            &bad_sources,
            layer,
            "bad-source",
            false,
            &mut meter(),
        )
        .err()
        .unwrap();
        assert_eq!(
            fresh_error.to_string(),
            general_error.to_string(),
            "source case {case}"
        );
    }
    // Both ordinary error and unwind cleanup destroy all fresh operation-local
    // owners. The observer is only a Weak reference captured by the real callback.
    for unwind in [false, true] {
        let mut weak = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owned_transition::prepare_owned_evolution(
                &current,
                &a,
                &b,
                &sources,
                layer,
                "drop-control",
                false,
                ProofBudget::default(),
                cancel(),
                None,
                |gate| {
                    weak = Some(Arc::downgrade(&gate.execution()?.original));
                    if unwind {
                        panic!("component callback unwind");
                    }
                    Err(StoreError::Invalid("component callback refusal".into()))
                },
            )
        }));
        assert!(weak.unwrap().upgrade().is_none());
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
    }
    // No hit authority survives either public reconstruction or cloned/deserialized current data.
    let decoded: ProjectSnapshot =
        serde_json::from_slice(&canonical_bytes(&current).unwrap()).unwrap();
    let cloned_registry = reconstruct(&decoded, &mut meter()).unwrap();
    let mut cloned_meter = meter();
    let cloned = transition_from_registry(
        &decoded,
        &cloned_registry,
        &a,
        &b,
        &sources,
        layer,
        "ordinary",
        false,
        &mut cloned_meter,
    )
    .unwrap();
    assert_execution_equal(&ordinary, &cloned);
    assert_eq!(
        cloned_meter.remaining_units_for_test(),
        ordinary_meter.remaining_units_for_test()
    );
}
