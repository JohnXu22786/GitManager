//! Additional small-fixture controls; the strict whole recovery journey is unchanged.
#[path = "fixtures/common_fact/mod.rs"] mod common;
#[path = "fixtures/owned_recovery/mod.rs"] mod fixture;
#[path = "../src/product_backup.rs"] mod product_backup;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "../src/product_decisions/mod.rs"] mod product_decisions;
#[path = "../src/product_locations.rs"] mod product_locations;
#[path = "../src/product_protocol.rs"] mod product_protocol;
#[path = "../src/product_runtime/mod.rs"] mod product_runtime;
#[path = "../src/product_store/mod.rs"] mod product_store;
#[path = "fixtures/product_scope/mod.rs"] mod scalar_fixture;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_store::{scope::*, ProductStore, ProjectSnapshot};

// Reuse the actual owned_recovery pair, lifecycle, source edits and policy.
// Three roots retain two real unfinished contributors plus the completed
// lifecycle witness. This adds a focused fixture and never edits the full one.
fn small_basis() -> (tempfile::TempDir, ProductStore, CapturedProgram, CapturedProgram) {
    let (a, b) = pair();
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(dir.path().join("store"), &a, 20000).unwrap();
    for title in ["live", "reserve", "finished"] {
        add(&store, &format!("add-{title}"), title);
    }
    job_action(&store, "finish-before-policy", "complete", "finished");
    for title in ["live", "reserve"] {
        let owner = row(&store.load().unwrap().data, title);
        apply(&store, &format!("register-{title}"), invoke("register", [
            ("owner".into(), reference(&owner)), ("name".into(), value("shared-material")),
        ].into()));
    }
    apply(&store, "material-arrived", material_input(&store, "arrive", "before policy"));
    let current = store.load().unwrap();
    assert_eq!(current.data.records.iter().filter(|r| r.entity == "job").count(), 3);
    assert!(current.decisions.decisions.is_empty());
    (dir, store, a, b)
}
fn seven_specs(store: &ProductStore, prefix: &str) -> Vec<ScenarioSpec> {
    let current = store.load().unwrap();
    let live = row(&current.data, "live");
    let reserve = row(&current.data, "reserve");
    let link = current.data.records.iter().find(|r| r.entity == "requirement").unwrap();
    let material = reference(&material(&current.data));
    let withdrawal = invoke("withdraw", [
        ("material".into(), material.clone()), ("note".into(), value("scene")),
    ].into());
    vec![
        ("add", vec![invoke("add", [("title".into(), value("scene-new"))].into())]),
        ("complete", vec![invoke("complete", [("job".into(), reference(&reserve))].into())]),
        ("register", vec![invoke("register", [("owner".into(), reference(&live)), ("name".into(), value("scene-material"))].into())]),
        ("remove", vec![invoke("remove", [("link".into(), DataValue::Reference { entity: "requirement".into(), record: link.id.clone() })].into())]),
        ("withdraw", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal.clone()]),
        ("begin", vec![invoke("begin", [("job".into(), reference(&live))].into())]),
        ("arrive", vec![invoke("begin", [("job".into(), reference(&live))].into()), withdrawal, invoke("arrive", [("material".into(), material), ("note".into(), value("scene"))].into())]),
    ].into_iter().map(|(id, inputs)| scenario(store, &format!("{prefix}-{id}"), inputs)).collect()
}
fn adopt_small(store: &ProductStore, a: &CapturedProgram, b: &CapturedProgram, hidden_common: bool) -> ProjectSnapshot {
    let before = store.load().unwrap();
    let prepared = prepare(store, a, b);
    assert_nonvacuity(store, &prepared);
    let mut choice = policy_choice(&prepared);
    if hidden_common {
        choice.obligations[0].predicate = PropertyPredicate::Or { values: vec![
            choice.obligations[0].predicate.clone(),
            PropertyPredicate::Less {
                left: PropertyTerm::Observed { point: "result".into(), observable: "ready_count".into(), value_type: Type::Integer },
                right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: MAX_COLLECTION as i64 + 1 } },
            },
        ] };
    }
    let scenes: Vec<_> = seven_specs(store, "initial").iter().map(|spec| {
        let scene = engine(store).accept_owned_scene(store, &prepared, spec, Disclosure::Synthetic).unwrap();
        assert!(observed_count(&scene) < 2);
        scene
    }).collect();
    let change = engine(store).prepare_owned_choice(store, prepared.clone(), choice, scenes, prepared.operation_id()).unwrap();
    let adopted = engine(store).adopt(store, &change).unwrap();
    assert_eq!(adopted.data.events, before.data.events);
    assert_eq!(adopted.owned_scope().unwrap().adoptions[0].decisions, vec!["owned-policy"]);
    assert_eq!(engine(store).check_current(&adopted).unwrap().disposition, CheckDisposition::Ready);
    adopted
}

#[test]
fn registry_public_retirement_matches_old_reference_and_survives_fresh_archive() {
    let (dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    engine(&store).assert_registry_public_withdrawal_parity_for_test(&store, &layer);
    let (prepared, summary, report) = engine(&store).preview_owned_withdrawal(
        &store, &layer, "small-withdrawal", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
    let change = engine(&store).prepare_owned_withdrawal(
        &store, prepared, &summary.retire_decisions, "small-withdrawal",
    ).unwrap();
    let withdrawn = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(withdrawn.data, current.data);
    assert_eq!(withdrawn.decisions.decisions[0].status, DecisionStatus::Withdrawn { adoption: "small-withdrawal".into() });
    IntentArchive::new(store.clone()).export_for(&withdrawn).unwrap();
    drop(store);
    let reopened = product_backup::open_verified(&dir.path().join("store"), None).unwrap().store;
    assert_eq!(reopened.load().unwrap(), withdrawn);
    assert_eq!(engine(&reopened).check_current(&withdrawn).unwrap().disposition, CheckDisposition::Ready);
}

#[test]
fn registry_distinct_initial_and_evolved_sources_preserve_complete_surfaces() {
    let (_dir, store, a, b) = small_basis();
    let initial = adopt_small(&store, &a, &b, false);
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (next_a, next_b) = evolved(&a, &b, "second-source");
    let (prepared, report) = engine(&store).prepare_owned_evolution(
        &store, &next_a, &next_b, &sources(&next_a, &next_b), &layer,
        "small-evolution", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(&store).prepare_owned_implementation_evolution(&store, prepared, "small-evolution").unwrap();
    let evolved = engine(&store).adopt(&store, &change).unwrap();
    assert_eq!(evolved.decisions, initial.decisions);
    let scenes: Vec<_> = seven_specs(&store, "evolved").iter().map(|spec| {
        let scene = engine(&store).accept_current_scene(&evolved, spec, Disclosure::Synthetic).unwrap();
        assert!(observed_count(&scene) < 2);
        scene
    }).collect();
    // These are genuine observed scenes for the second source, not a forged
    // second retirement decision. Existing writers cannot add that decision.
    assert_eq!(store.load().unwrap(), evolved);
    engine(&store).assert_registry_distinct_sources_for_test(&store, &layer, &scenes);
}

#[test]
fn registry_rejects_full_snapshot_and_foreign_capture_at_exact_guards() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    current.assert_registry_complete_association_for_test();
    engine(&store).assert_registry_surface_rejects_foreign_source_for_test(&store, &layer);
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn registry_and_old_reference_both_reject_hidden_unowned_obligation() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, true);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    engine(&store).assert_registry_hidden_unowned_refusal_for_test(&store, &layer);
}

// Current-scene route controls are deliberately separate tests. The existing
// seven-scene/two-source parity test and strict whole journey stay untouched.
fn exact_empty_graph() -> DecisionGraph {
    DecisionGraph { version: CONTRACT_VERSION, revision: 0, decisions: vec![] }
}
fn assert_native_scene(store: &ProductStore, current: &ProjectSnapshot, spec: &ScenarioSpec) {
    assert!(!current.decisions.decisions.is_empty());
    let context = ScopedExecutionContext::committed(current).unwrap();
    let (native, _) = product_decisions::execute_admitted(
        &product_runtime::LocalRuntime::default(), current.program().unwrap(), spec,
        &current.decisions, RuntimeLimits::default(), "accepted", Some(&context),
    ).unwrap();
    assert_eq!(native.state, EvidenceState::Observed);
    let scene = engine(store).accept_current_scene(current, spec, Disclosure::ExplicitlySelected).unwrap();
    assert_eq!(scene.program(), current.program().unwrap());
    assert_eq!(scene.scenario(), spec);
    assert_eq!(scene.evidence(), &native);
    assert_eq!(scene.evidence().binding.decision_digest, current.decisions.identity().unwrap());
    assert_ne!(scene.evidence().binding.decision_digest, exact_empty_graph().identity().unwrap());
    assert_eq!(scene.evidence().trace.iter().map(|step| &step.input).collect::<Vec<_>>(), spec.inputs.iter().collect::<Vec<_>>());
    assert_eq!(scene.observations(), native.observations);
    let encoded = serde_json::to_value(&scene).unwrap();
    assert_eq!(encoded["disclosure"], serde_json::json!("explicitly_selected"));
    assert_eq!(encoded["bind_outcome"], serde_json::json!(true));
    assert_eq!(store.load().unwrap(), *current);
}
fn evolve_small(store: &ProductStore, a: &CapturedProgram, b: &CapturedProgram, initial: &ProjectSnapshot) -> ProjectSnapshot {
    let layer = initial.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (next_a, next_b) = evolved(a, b, "current-scene-source");
    let (prepared, report) = engine(store).prepare_owned_evolution(
        store, &next_a, &next_b, &sources(&next_a, &next_b), &layer,
        "current-scene-evolution", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Ready);
    let change = engine(store).prepare_owned_implementation_evolution(store, prepared, "current-scene-evolution").unwrap();
    let current = engine(store).adopt(store, &change).unwrap();
    assert_eq!(current.decisions, initial.decisions);
    assert_ne!(current.program().unwrap(), initial.program().unwrap());
    current
}
#[test]
fn current_scene_initial_owned_binds_nonempty_graph_and_complete_native_run() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let spec = seven_specs(&store, "current-initial").remove(0);
    assert_native_scene(&store, &current, &spec);
}
#[test]
fn current_scene_evolved_after_live_work_binds_exact_current_graph() {
    let (_dir, store, a, b) = small_basis();
    let initial = adopt_small(&store, &a, &b, false);
    let evolved = evolve_small(&store, &a, &b, &initial);
    job_action(&store, "current-scene-live-begin", "begin", "live");
    let current = store.load().unwrap();
    assert_ne!(current.data, evolved.data);
    assert_eq!(current.decisions, initial.decisions);
    assert_native_scene(&store, &current, &scenario(&store, "current-after-work", vec![]));
}
fn assert_committed_refusal(result: Result<AcceptedScene, DecisionError>) {
    assert!(matches!(result, Err(DecisionError::Invalid(ref text))
        if text == "retained replay context does not match the committed snapshot"));
}
#[test]
fn current_scene_rejects_full_snapshot_and_graph_only_substitutions_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let spec = scenario(&store, "association", vec![]);
    let mut changed_graph = current.clone();
    changed_graph.decisions.revision += 1;
    let mut changed_source = current.clone();
    let source = changed_source.programs.iter_mut().find(|p| p.artifact == current.program().unwrap().artifact).unwrap();
    source.binding.program_path = "other-current/program.json".into();
    let mut changed_day = current.clone();
    changed_day.clock_day += 1;
    let mut changed_session = current.clone();
    changed_session.session.values.insert(MARKER.into(), value("different-session"));
    for supplied in [changed_graph, changed_source, changed_day, changed_session] {
        assert_ne!(supplied, current);
        let runtime = CurrentSceneRuntime::new(None, RaceAt::Never);
        let counts = runtime.counts.clone();
        let checked = DecisionEngine::new(runtime, IntentArchive::new(store.clone()));
        assert_committed_refusal(checked.accept_current_scene(&supplied, &spec, Disclosure::Synthetic));
        assert_eq!(counts.capabilities.get(), 0);
        assert_eq!(counts.validations.get(), 0);
        assert_eq!(counts.starts.get(), 0);
        assert_eq!(store.load().unwrap(), current);
    }
}
#[test]
fn current_scene_rejects_genuine_stale_and_foreign_archive_snapshots() {
    let (_dir, store, a, b) = small_basis();
    let stale = adopt_small(&store, &a, &b, false);
    let old_spec = scenario(&store, "stale", vec![]);
    let checked = engine(&store);
    checked.retained_replay_context(&stale).unwrap();
    job_action(&store, "stale-live-begin", "begin", "live");
    let current = store.load().unwrap();
    assert_ne!(current, stale);
    assert_eq!(current.program().unwrap(), stale.program().unwrap());
    assert_eq!(current.decisions, stale.decisions);
    assert_committed_refusal(checked.accept_current_scene(&stale, &old_spec, Disclosure::Synthetic));
    let (_foreign_dir, foreign, foreign_a, foreign_b) = small_basis();
    let foreign_current = adopt_small(&foreign, &foreign_a, &foreign_b, false);
    assert_ne!(foreign_current, current);
    assert_committed_refusal(engine(&foreign).accept_current_scene(&current, &scenario(&store, "foreign", vec![]), Disclosure::Synthetic));
    assert_eq!(store.load().unwrap(), current);
    assert_eq!(foreign.load().unwrap(), foreign_current);
}
fn genuine_scalar_authority() -> (tempfile::TempDir, ProductStore, PreparedScopedChange, std::collections::BTreeMap<Digest, ScopeCorrespondence>) {
    let dir = scalar_fixture::tempdir();
    let baseline = scalar_fixture::program(false);
    let store = ProductStore::create(dir.path().join("scalar"), &baseline, 20000).unwrap();
    scalar_fixture::add(&store, "scalar-row", "Scalar authority");
    let current = store.load().unwrap();
    let prepared = store.prepare_scoped_change(
        &scalar_fixture::program(true),
        &scalar_fixture::request(&current, ScopePopulation::FutureWork), "scalar-authority",
    ).unwrap();
    let context = ScopedExecutionContext::prepared(&current, &prepared).unwrap();
    let mapped = product_runtime::merged_data(prepared.target(), &current.data).unwrap();
    let (projected, actual) = context.project_seed(
        &baseline, &current.data, prepared.target(), &mapped, current.clock_day,
    ).unwrap();
    assert_eq!(&actual, prepared.seed());
    let proofs = projected.correspondence_proofs();
    assert!(!proofs.is_empty(), "the correspondence-only case must not be vacuous");
    (dir, store, prepared, proofs)
}
fn assert_pending_scene_refused(store: &ProductStore, current: &ProjectSnapshot, pending: CurrentScenePendingForTest) {
    let runtime = CurrentSceneRuntime::new(None, RaceAt::Never);
    let counts = runtime.counts.clone();
    DecisionEngine::new(runtime, IntentArchive::new(store.clone()))
        .assert_current_scene_pending_mode_for_test(current, &scenario(store, "pending-mode", vec![]), pending);
    assert_eq!(counts.capabilities.get(), 0);
    assert_eq!(counts.validations.get(), 0);
    assert_eq!(counts.starts.get(), 0);
    assert_eq!(store.load().unwrap(), *current);
}
#[test]
fn current_scene_rejects_real_pending_scalar_authority_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let (_scalar_dir, _scalar, prepared, _proofs) = genuine_scalar_authority();
    assert_pending_scene_refused(&store, &current, CurrentScenePendingForTest::Scalar(prepared));
}
#[test]
fn current_scene_rejects_real_pending_rehearsal_authority_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let (_scalar_dir, _scalar, prepared, _proofs) = genuine_scalar_authority();
    assert_pending_scene_refused(&store, &current, CurrentScenePendingForTest::Rehearsal(prepared));
}
#[test]
fn current_scene_rejects_real_pending_owned_authority_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let (_other_dir, other, other_a, other_b) = small_basis();
    let prepared = prepare(&other, &other_a, &other_b);
    assert_pending_scene_refused(&store, &current, CurrentScenePendingForTest::Owned(prepared));
}
#[test]
fn current_scene_rejects_real_pending_transition_authority_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let layer = current.owned_scope().unwrap().layers.keys().next().unwrap().clone();
    let (next_a, next_b) = evolved(&a, &b, "pending-current-scene");
    let (prepared, _) = engine(&store).prepare_owned_evolution(
        &store, &next_a, &next_b, &sources(&next_a, &next_b), &layer,
        "pending-current-scene", ProofBudget::default(), cancel(),
    ).unwrap();
    assert_pending_scene_refused(&store, &current, CurrentScenePendingForTest::Transition(prepared));
}
#[test]
fn current_scene_rejects_real_pending_correspondence_only_before_runtime() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let (_scalar_dir, _scalar, _prepared, proofs) = genuine_scalar_authority();
    assert!(!proofs.is_empty());
    assert_pending_scene_refused(&store, &current, CurrentScenePendingForTest::Correspondences(proofs));
}
#[test]
fn current_scene_preserves_actual_cancellation_cause_before_runtime_start() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let spec = scenario(&store, "cancelled", vec![]);
    let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let native = product_runtime::LocalRuntime::with_cancellation(token.clone());
    let context = ScopedExecutionContext::committed(&current).unwrap();
    let expected = product_decisions::execute_admitted(
        &native, current.program().unwrap(), &spec, &current.decisions,
        RuntimeLimits::default(), "accepted", Some(&context),
    ).unwrap_err();
    let mut runtime = CurrentSceneRuntime::new(None, RaceAt::Never);
    runtime.native = native;
    let counts = runtime.counts.clone();
    let actual = DecisionEngine::new(runtime, IntentArchive::new(store.clone()))
        .accept_current_scene(&current, &spec, Disclosure::Synthetic).unwrap_err();
    assert_eq!(counts.validations.get(), 0);
    assert_eq!(counts.starts.get(), 0);
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert!(matches!(actual, DecisionError::Runtime(AdapterError::Unsupported(ref text))
        if text == "Invalid(\"Common factual admission: budget: cancelled or cumulative elapsed budget exhausted\")"));
    assert_eq!(store.load().unwrap(), current);
}
#[test]
fn current_scene_preserves_native_low_fuel_incomplete_evidence_refusal() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    engine(&store).assert_current_scene_low_fuel_for_test(&current, &scenario(&store, "low-fuel", vec![]));
    assert_eq!(store.load().unwrap(), current);
}

#[derive(Clone, Copy)]
enum RaceAt { Never, Capabilities, Start }
#[derive(Default)]
struct SceneCounts {
    capabilities: std::cell::Cell<usize>,
    validations: std::cell::Cell<usize>,
    starts: std::cell::Cell<usize>,
    mutations: std::cell::Cell<usize>,
}
// A finite test adapter with two concrete write locations. All evaluation and
// evidence are delegated unchanged to LocalRuntime; this is not a production
// observer or callback registry and cannot fabricate an AcceptedScene.
struct CurrentSceneRuntime {
    native: product_runtime::LocalRuntime,
    store: Option<ProductStore>,
    at: RaceAt,
    counts: std::rc::Rc<SceneCounts>,
}
impl CurrentSceneRuntime {
    fn new(store: Option<ProductStore>, at: RaceAt) -> Self {
        Self { native: product_runtime::LocalRuntime::default(), store, at, counts: Default::default() }
    }
    fn change_current(&self) {
        if self.counts.mutations.get() != 0 { return; }
        let store = self.store.as_ref().expect("race has a real store");
        let current = store.load().unwrap();
        store.apply(current.revision, "current-scene-race", &SemanticInput::AdvanceClock { days: 1 }, RuntimeLimits::default()).unwrap();
        self.counts.mutations.set(1);
    }
}
impl RuntimeAdapter for CurrentSceneRuntime {
    type Run = product_runtime::ProductRun;
    fn cancellation_token(&self) -> Option<std::sync::Arc<std::sync::atomic::AtomicBool>> { self.native.cancellation_token() }
    fn capabilities(&self) -> RuntimeCapabilities {
        self.counts.capabilities.set(self.counts.capabilities.get() + 1);
        if matches!(self.at, RaceAt::Capabilities) { self.change_current(); }
        self.native.capabilities()
    }
    fn validate(&self, p: &CapturedProgram) -> Result<(), AdapterError> {
        self.counts.validations.set(self.counts.validations.get() + 1);
        self.native.validate(p)
    }
    fn start(&self, p: &CapturedProgram, d: &DataSnapshot, s: &SessionState, day: i32, seed: u64, limits: RuntimeLimits) -> Result<Self::Run, AdapterError> {
        self.counts.starts.set(self.counts.starts.get() + 1);
        if matches!(self.at, RaceAt::Start) { self.change_current(); }
        self.native.start(p, d, s, day, seed, limits)
    }
    fn apply(&self, run: &mut Self::Run, input: &SemanticInput, id: &str) -> Result<TraceStep, AdapterError> { self.native.apply(run, input, id) }
    fn observe(&self, run: &Self::Run, point: &str) -> Result<Observation, AdapterError> { self.native.observe(run, point) }
    fn emitted_artifacts<'a>(&self, run: &'a Self::Run) -> Result<&'a [LocalArtifact], AdapterError> { self.native.emitted_artifacts(run) }
    fn emitted_record_participants<'a>(&self, run: &'a Self::Run) -> Result<Option<&'a [EmittedRecordParticipants]>, AdapterError> { self.native.emitted_record_participants(run) }
    fn data<'a>(&self, run: &'a Self::Run) -> &'a DataSnapshot { self.native.data(run) }
    fn session<'a>(&self, run: &'a Self::Run) -> &'a SessionState { self.native.session(run) }
    fn compatibility(&self, p: &CapturedProgram, d: &DataSnapshot) -> Result<CompatibilityReport, AdapterError> { self.native.compatibility(p, d) }
    fn prepare_adoption(&self, p: AdoptionPlan, d: &DataSnapshot) -> Result<AdoptionPlan, AdapterError> { self.native.prepare_adoption(p, d) }
}
#[test]
fn current_scene_rechecks_current_after_capability_callback_and_runtime_start() {
    for at in [RaceAt::Capabilities, RaceAt::Start] {
        let (_dir, store, a, b) = small_basis();
        let current = adopt_small(&store, &a, &b, false);
        let spec = scenario(&store, "racing-current", vec![]);
        let runtime = CurrentSceneRuntime::new(Some(store.clone()), at);
        let counts = runtime.counts.clone();
        let checked = DecisionEngine::new(runtime, IntentArchive::new(store.clone()));
        let result = checked.accept_current_scene(&current, &spec, Disclosure::Synthetic);
        assert!(matches!(result, Err(DecisionError::Invalid(ref text))
            if text == "saved work changed during current scene replay"));
        assert_eq!(counts.mutations.get(), 1);
        assert_eq!(counts.starts.get(), 1);
        let actual = store.load().unwrap();
        assert_eq!(actual.clock_day, current.clock_day + 1);
        assert_eq!(actual.program().unwrap(), current.program().unwrap());
        assert_eq!(actual.decisions, current.decisions);
        assert_eq!(actual.data, current.data);
    }
}
#[test]
fn current_scene_keeps_runtime_incomplete_error_ahead_of_post_recheck() {
    let (_dir, store, a, b) = small_basis();
    let current = adopt_small(&store, &a, &b, false);
    let spec = scenario(&store, "runtime-first-cause", vec![]);
    let runtime = CurrentSceneRuntime::new(Some(store.clone()), RaceAt::Start);
    let counts = runtime.counts.clone();
    DecisionEngine::new(runtime, IntentArchive::new(store.clone()))
        .assert_current_scene_low_fuel_for_test(&current, &spec);
    assert_eq!(counts.mutations.get(), 1);
    assert_eq!(counts.starts.get(), 1);
    assert_eq!(store.load().unwrap().clock_day, current.clock_day + 1);
}
#[test]
fn current_scene_fix_leaves_exact_historical_empty_graph_admission_available() {
    use product_runtime::ReplayAdmission;
    let (_dir, store, a, b) = small_basis();
    let basis = store.load().unwrap();
    let original = scenario(&store, "historical-original", vec![]);
    let current = adopt_small(&store, &a, &b, false);
    assert!(basis.decisions.decisions.is_empty());
    assert_ne!(basis.decisions, current.decisions);
    let context = ScopedExecutionContext::committed(&current).unwrap();
    assert!(context.begin_replay(&a, &original, &basis.decisions, cancel()).unwrap().is_some());
    let (native, _) = product_decisions::execute_admitted(
        &product_runtime::LocalRuntime::default(), &a, &original, &basis.decisions,
        RuntimeLimits::default(), "historical", Some(&context),
    ).unwrap();
    assert_eq!(native.state, EvidenceState::Observed);
    assert_eq!(native.binding.decision_digest, basis.decisions.identity().unwrap());
    assert_eq!(store.load().unwrap(), current);
}
#[test]
fn current_scene_fix_does_not_broaden_historical_tuple_graph_source_frame_or_seed() {
    use product_runtime::ReplayAdmission;
    let (_dir, store, a, b) = small_basis();
    let basis = store.load().unwrap();
    let original = scenario(&store, "historical-negative", vec![]);
    let current = adopt_small(&store, &a, &b, false);
    let context = ScopedExecutionContext::committed(&current).unwrap();
    for changed in ["graph", "source", "session", "day", "random", "data"] {
        let mut graph = basis.decisions.clone();
        let mut source = a.clone();
        let mut spec = original.clone();
        match changed {
            "graph" => graph.revision += 1,
            "source" => source.binding.program_path = "foreign-history/program.json".into(),
            "session" => { spec.session.values.insert(MARKER.into(), value("foreign-session")); }
            "day" => spec.clock_day += 1,
            "random" => spec.random_seed = 1,
            "data" => spec.seed.records[0].revision += 1,
            _ => unreachable!(),
        }
        let error = match context.begin_replay(&source, &spec, &graph, cancel()) {
            Ok(_) => panic!("{changed} must not obtain historical admission"),
            Err(error) => error,
        };
        assert_eq!(error, AdapterError::Unsupported("Invalid(\"Common factual admission: owned protected history: replay graph differs from authenticated context\")".into()), "{changed}");
        assert_eq!(store.load().unwrap(), current);
    }
}
fn assert_legacy_current_scene(store: &ProductStore) {
    let current = store.load().unwrap();
    let spec = ScenarioSpec {
        version: CONTRACT_VERSION, id: "legacy-current-scene".into(), label: "Ordinary/scalar native parity".into(),
        seed: current.data.clone(), session: current.session.clone(), clock_day: current.clock_day,
        random_seed: 0, inputs: vec![SemanticInput::Observe { point: "legacy-result".into() }], validity: vec![],
    };
    let context = ScopedExecutionContext::committed(&current).unwrap();
    let (native, _) = product_decisions::execute_admitted(
        &product_runtime::LocalRuntime::default(), current.program().unwrap(), &spec,
        &exact_empty_graph(), RuntimeLimits::default(), "accepted", Some(&context),
    ).unwrap();
    let scene = engine(store).accept_current_scene(&current, &spec, Disclosure::Synthetic).unwrap();
    assert_eq!(scene.evidence(), &native);
    assert_eq!(scene.evidence().binding.decision_digest, exact_empty_graph().identity().unwrap());
    assert_eq!(store.load().unwrap(), current);
}
#[test]
fn current_scene_ordinary_with_nonempty_decisions_keeps_empty_graph_evidence() {
    let (_dir, store, _a, _b) = setup();
    assert!(!store.load().unwrap().decisions.decisions.is_empty());
    assert_legacy_current_scene(&store);
}
#[test]
fn current_scene_committed_scalar_keeps_empty_graph_native_evidence() {
    let (_dir, store, prepared, _) = genuine_scalar_authority();
    let current = store.load().unwrap();
    let adopted = store.adopt_scoped(current.revision, &prepared).unwrap();
    assert!(!adopted.scalar_scope().unwrap().compositions.is_empty());
    assert_legacy_current_scene(&store);
}
