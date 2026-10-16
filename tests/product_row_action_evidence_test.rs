//! Synthetic execution and persistence regressions, not live/native acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_discovery::*;
use product_runtime::LocalRuntime;
use product_scenarios::*;
use product_store::ProductStore;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{atomic::AtomicBool, Arc},
};

fn target() -> ObservationTarget {
    ObservationTarget::RowActionRows {
        point: "done".into(),
        view: "assets".into(),
        entity: "asset".into(),
        binding: "borrow_asset".into(),
        action: "borrow".into(),
    }
}
fn property(records: &[&str]) -> AcceptedProperty {
    AcceptedProperty {
        id: "available-assets".into(),
        description: "These displayed assets can be borrowed".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::RowActionRows {
                point: "done".into(),
                view: "assets".into(),
                entity: "asset".into(),
                binding: "borrow_asset".into(),
                action: "borrow".into(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::reference("asset")),
                value: DataValue::List {
                    item_type: Type::reference("asset"),
                    items: records.iter().map(|id| reference("asset", id)).collect(),
                },
            },
        },
    }
}
fn workflow(p: &CapturedProgram) -> ScenarioSpec {
    let mut s = scenario(
        p,
        vec![SemanticInput::Observe {
            point: "done".into(),
        }],
    );
    for (id, name) in [("drill", "Drill"), ("saw", "Saw"), ("spare", "Spare")] {
        s.seed.records.push(Record {
            entity: "asset".into(),
            id: id.into(),
            revision: 1,
            created_program: p.artifact.program_digest.clone(),
            archived: false,
            values: args(&[("name", string(name))]),
        });
    }
    s.seed.records.push(Record {
        entity: "loan".into(),
        id: "loan-drill".into(),
        revision: 1,
        created_program: p.artifact.program_digest.clone(),
        archived: false,
        values: args(&[
            ("asset", reference("asset", "drill")),
            ("returned", DataValue::Boolean { value: false }),
        ]),
    });
    s
}
fn replay(p: &CapturedProgram, s: &ScenarioSpec) -> RunEvidence {
    LocalRuntime::default()
        .replay(p, s, &decisions(), RuntimeLimits::default(), "actual")
        .unwrap()
}
fn pair() -> (CapturedProgram, CapturedProgram, ScenarioSpec) {
    let a = capture(equipment());
    let mut changed = equipment();
    changed["actions"][0]["guards"] = json!([]);
    let b = capture(changed);
    let s = workflow(&a);
    (a, b, s)
}
fn comparison() -> ComparisonEngine {
    ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
}

#[test]
fn row_action_terms_use_completed_guards_and_canonical_record_sets() {
    let (a, b, s) = pair();
    let before = replay(&a, &s);
    let after = replay(&b, &s);
    assert_eq!(before.state, EvidenceState::Observed);
    assert_eq!(before.observations[0].values, after.observations[0].values);
    assert_eq!(
        before.observations[0]
            .view
            .rows
            .iter()
            .map(|r| &r.cells)
            .collect::<Vec<_>>(),
        after.observations[0]
            .view
            .rows
            .iter()
            .map(|r| &r.cells)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before.observations[0]
            .view_schema
            .as_ref()
            .unwrap()
            .row_actions
            .as_ref()
            .unwrap()
            .get("borrow_asset"),
        Some(&"borrow".to_string())
    );
    assert_eq!(
        property(&["saw", "spare"]).evaluate(&before.observations),
        Some(true)
    );
    assert_eq!(
        property(&["saw", "spare"]).evaluate(&after.observations),
        Some(false)
    );
    assert_eq!(
        target().differs(&before.observations, &after.observations),
        Some(true)
    );
    let mut reordered = before.observations.clone();
    reordered[0].view.rows.reverse();
    assert_eq!(
        target().differs(&before.observations, &reordered),
        Some(false)
    );
    let mut swapped = before.observations.clone();
    for row in &mut swapped[0].view.rows {
        if row.record.record == "drill" {
            row.enabled_actions.insert("borrow_asset".into());
        }
        if row.record.record == "saw" {
            row.enabled_actions.clear();
        }
    }
    assert_eq!(
        target().differs(&before.observations, &swapped),
        Some(true),
        "equal counts must preserve record identity"
    );
    let encoded = serde_json::to_vec(&property(&["saw", "spare"])).unwrap();
    assert_eq!(
        serde_json::from_slice::<AcceptedProperty>(&encoded).unwrap(),
        property(&["saw", "spare"])
    );
}

#[test]
fn row_action_disabled_and_empty_are_known_but_missing_identity_is_unknown() {
    let p = capture(equipment());
    let empty = replay(
        &p,
        &scenario(
            &p,
            vec![SemanticInput::Observe {
                point: "done".into(),
            }],
        ),
    );
    assert_eq!(property(&[]).evaluate(&empty.observations), Some(true));
    assert_eq!(target().channel_presence(&empty.observations), Some(true));
    let mut value = equipment();
    value["views"][0]["actions"][0]["enabled"]["value"]["value"] = json!(false);
    let disabled = capture(value);
    let run = replay(&disabled, &workflow(&disabled));
    assert_eq!(property(&[]).evaluate(&run.observations), Some(true));
    for mutation in ["legacy", "no_schema", "missing", "action", "view", "entity"] {
        let mut old = empty.observations.clone();
        match mutation {
            "legacy" => old[0].view_schema.as_mut().unwrap().row_actions = None,
            "no_schema" => old[0].view_schema = None,
            "missing" => {
                old[0]
                    .view_schema
                    .as_mut()
                    .unwrap()
                    .row_actions
                    .as_mut()
                    .unwrap()
                    .clear();
            }
            "action" => {
                old[0]
                    .view_schema
                    .as_mut()
                    .unwrap()
                    .row_actions
                    .as_mut()
                    .unwrap()
                    .insert("borrow_asset".into(), "return_loan".into());
            }
            "view" => old[0].view.view = "another_view".into(),
            _ => old[0].view_schema.as_mut().unwrap().entity = "loan".into(),
        }
        assert_eq!(property(&[]).evaluate(&old), None, "{mutation}");
        assert!(target().sample(&old).is_none(), "{mutation}");
    }
}

#[test]
fn row_action_provenance_rejects_forged_ids_entities_and_oversized_maps() {
    let p = capture(equipment());
    let run = replay(&p, &workflow(&p));
    for mutation in ["enabled", "duplicate", "entity", "invalid", "oversized"] {
        let mut observations = run.observations.clone();
        let o = &mut observations[0];
        match mutation {
            "enabled" => {
                o.view.rows[0]
                    .enabled_actions
                    .insert("forged_binding".into());
            }
            "duplicate" => {
                o.view.rows.push(o.view.rows[0].clone());
            }
            "entity" => o.view.rows[0].record.entity = "loan".into(),
            "invalid" => {
                o.view_schema
                    .as_mut()
                    .unwrap()
                    .row_actions
                    .as_mut()
                    .unwrap()
                    .insert("bad binding".into(), "borrow".into());
            }
            _ => {
                o.view_schema.as_mut().unwrap().row_actions = Some(
                    (0..=MAX_COLLECTION)
                        .map(|i| (format!("binding-{i}"), "borrow".into()))
                        .collect(),
                );
            }
        }
        assert!(o.validate().is_err(), "{mutation}");
        assert_eq!(
            property(&["saw", "spare"]).evaluate(&observations),
            None,
            "{mutation}"
        );
    }
}

#[test]
fn legacy_row_action_metadata_keeps_bytes_and_never_supplies_proof() {
    let p = capture(equipment());
    let run = replay(&p, &workflow(&p));
    assert_eq!(run.binding.runtime_version, "local-interpreter/4");
    assert_eq!(run.binding.driver_version, "semantic-input/3");
    let mut legacy = serde_json::to_value(&run.observations[0]).unwrap();
    legacy["view_schema"]
        .as_object_mut()
        .unwrap()
        .remove("row_actions");
    let old: Observation = serde_json::from_value(legacy.clone()).unwrap();
    old.validate().unwrap();
    assert_eq!(serde_json::to_value(&old).unwrap(), legacy);
    assert_eq!(
        canonical_bytes(&old).unwrap(),
        canonical_bytes(&legacy).unwrap()
    );
    assert_eq!(property(&["saw", "spare"]).evaluate(&[old]), None);
    let mut stale = run.clone();
    stale.binding.runtime_version = "local-interpreter/3".into();
    assert!(!stale.is_current(&run.binding));
}

#[test]
fn row_action_witnesses_are_independently_minimized_and_fail_closed() {
    let (a, b, s) = pair();
    let engine = comparison();
    let report = engine
        .minimize(&a, &b, &s, &decisions(), target(), SearchBudget::default())
        .unwrap();
    assert_eq!(report.state, EvidenceState::Observed);
    let verified = report.witness.unwrap();
    assert!(verified.matches_sources(&a, &b));
    assert!(!verified.matches_sources(&b, &a));
    let w = verified.witness();
    assert!(w.minimization.as_ref().unwrap().complete);
    assert!(w.scenario.seed.records.len() < s.seed.records.len());
    assert_eq!(w.before.binding.input_digest, w.after.binding.input_digest);
    assert_eq!(w.before.origin, ExecutionOrigin::ProductionRuntime);
    for (_, reduced) in engine.single_reductions(&a, &b, &w.scenario).unwrap() {
        let replay = engine
            .compare(
                &a,
                &b,
                &reduced,
                &decisions(),
                target(),
                RuntimeLimits::default(),
            )
            .unwrap();
        assert_ne!(replay.state, EvidenceState::Observed);
    }
    let exhausted = engine
        .compare(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            RuntimeLimits {
                fuel: 1,
                ..RuntimeLimits::default()
            },
        )
        .unwrap();
    assert!(exhausted.witness.is_none());
    assert_ne!(exhausted.state, EvidenceState::Observed);
    let cancelled = ComparisonEngine::new(Arc::new(AtomicBool::new(true))).compare(
        &a,
        &b,
        &s,
        &decisions(),
        target(),
        RuntimeLimits::default(),
    );
    let cancelled = cancelled.unwrap();
    assert_eq!(cancelled.state, EvidenceState::Inconclusive);
    assert!(cancelled.witness.is_none());
    assert!(cancelled.runs.is_empty());
    for change in ["delete", "rename", "retarget"] {
        let mut v = equipment();
        match change {
            "delete" => v["views"][0]["actions"] = json!([]),
            "rename" => v["views"][0]["actions"][0]["id"] = json!("new_binding"),
            _ => {
                v["actions"][0]["id"] = json!("other_borrow");
                v["views"][0]["actions"][0]["action"] = json!("other_borrow");
            }
        }
        let changed = capture(v);
        let report = engine
            .compare(
                &a,
                &changed,
                &s,
                &decisions(),
                target(),
                RuntimeLimits::default(),
            )
            .unwrap();
        assert!(report.witness.is_none(), "{change}");
        assert_ne!(report.state, EvidenceState::NoDifferenceFound, "{change}");
    }
}

fn exercised(p: &CapturedProgram) -> ScenarioSpec {
    let mut s = workflow(p);
    s.inputs.insert(
        0,
        invoke("borrow", args(&[("asset", reference("asset", "saw"))])),
    );
    s
}
fn discovery_input(
    a: &CapturedProgram,
    b: &CapturedProgram,
    s: &ScenarioSpec,
) -> (DevelopmentRequest, DevelopmentResult) {
    let r = DevelopmentRequest {
        version: 1,
        id: "discover-actions".into(),
        project_id: "runtime-project".into(),
        operation: DevelopmentOperation::Discover,
        request: "Review borrowing availability".into(),
        sources: vec![a.clone(), b.clone()],
        context: DevelopmentContext {
            view: Some("assets".into()),
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: decisions(),
        unknowns: vec![],
        required_capabilities: BTreeSet::new(),
    };
    let response = DevelopmentResponse {
        version: 1,
        request_digest: r.identity().unwrap(),
        candidates: vec![
            GeneratedCandidate {
                id: "before".into(),
                source_json: String::from_utf8(a.source_bytes.clone()).unwrap(),
            },
            GeneratedCandidate {
                id: "after".into(),
                source_json: String::from_utf8(b.source_bytes.clone()).unwrap(),
            },
        ],
        hypotheses: vec![ChoiceHypothesis {
            id: "availability".into(),
            statement: "Borrowing may remain available".into(),
            kind: HypothesisKind::UnresolvedChoice,
            action: "borrow".into(),
            observable: "loan_count".into(),
            sources: vec![SourceLocus {
                relative_path: "program.json".into(),
                raw_digest: b.artifact.raw_digest.clone(),
                pointer: "/actions/0/guards".into(),
            }],
            alternatives: vec!["before".into(), "after".into()],
            related_decisions: vec![],
            scenario_json: serde_json::to_string(s).unwrap(),
            unknowns: vec![],
        }],
        evolutions: vec![],
        unsupported: vec![],
    };
    (
        r,
        DevelopmentResult {
            response,
            producer: Producer::Fixture {
                name: "row-action fixture".into(),
            },
        },
    )
}
fn discover_pair(
    r: &DevelopmentRequest,
    out: &DevelopmentResult,
    policy: DiscoveryPolicy,
) -> DiscoveryReport {
    discover(r, out, &policy, Arc::new(AtomicBool::new(false))).unwrap()
}
#[test]
fn discovery_finds_row_action_only_contrasts_and_keeps_other_material_guards() {
    let (a, b, _) = pair();
    let (request, out) = discovery_input(&a, &b, &exercised(&a));
    let report = discover_pair(&request, &out, DiscoveryPolicy::default());
    assert!(report.unverified.is_empty(), "{:?}", report.unverified);
    assert_eq!(report.questions.len(), 1, "{:?}", report.log);
    assert!(report.questions[0].witnesses.iter().any(|w| matches!(
        &w.witness().distinguishing_properties[0].predicate,
        PropertyPredicate::Equal {
            left: PropertyTerm::RowActionRows { .. },
            ..
        }
    )));
    let mut value: Value = serde_json::from_slice(&b.source_bytes).unwrap();
    value["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"toolbar","label":"Borrow saw","placement":"toolbar","action":"borrow","arguments":{"asset":{"kind":"literal","value_type":{"kind":"reference","entity":"asset"},"value":{"kind":"reference","entity":"asset","record":"saw"}}},"enabled":yes()}));
    let modified = capture(value);
    let (request, out) = discovery_input(&a, &modified, &exercised(&a));
    let report = discover_pair(&request, &out, DiscoveryPolicy::default());
    assert!(report
        .unverified
        .iter()
        .any(|message| message.contains("done/actions")));
}

#[test]
fn discovery_union_rejects_one_sided_and_alternative_only_row_bindings() {
    let (a, b, _) = pair();
    for change in ["deleted", "renamed", "retargeted", "alternative_only"] {
        let (mut r, mut out) = discovery_input(&a, &b, &exercised(&a));
        let mut value: Value = serde_json::from_slice(&b.source_bytes).unwrap();
        match change {
            "deleted" => value["views"][0]["actions"] = json!([]),
            "renamed" => value["views"][0]["actions"][0]["id"] = json!("new_binding"),
            "retargeted" => {
                let mut other = value["actions"][0].clone();
                other["id"] = json!("other_borrow");
                value["actions"].as_array_mut().unwrap().push(other);
                value["views"][0]["actions"][0]["action"] = json!("other_borrow");
            }
            _ => {
                let mut other = value["views"][0]["actions"][0].clone();
                other["id"] = json!("alternative_binding");
                value["views"][0]["actions"]
                    .as_array_mut()
                    .unwrap()
                    .push(other);
            }
        }
        let changed = capture(value);
        if change == "alternative_only" {
            out.response.candidates[0].source_json =
                String::from_utf8(changed.source_bytes).unwrap();
        } else {
            r.sources[1] = changed;
            out.response.candidates[1].source_json =
                String::from_utf8(r.sources[1].source_bytes.clone()).unwrap();
            out.response.hypotheses[0].sources[0].raw_digest =
                r.sources[1].artifact.raw_digest.clone();
        }
        out.response.request_digest = r.identity().unwrap();
        let report = discover_pair(&r, &out, DiscoveryPolicy::default());
        assert!(!report.unverified.is_empty(), "{change}: {:?}", report.log);
        assert!(report.questions.iter().all(|q| q.witnesses.iter().all(|w| w
            .witness()
            .before
            .state
            == EvidenceState::Observed
            && w.witness().after.state == EvidenceState::Observed)));
    }
}
fn saved_property(store: &ProductStore, p: &CapturedProgram) {
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scene = accept_scene(
        &LocalRuntime::default(),
        p,
        &exercised(p),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let ready = engine
        .prepare_choice(
            store,
            p,
            Choice {
                id: "borrow-availability".into(),
                request: "Keep these eligible assets".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["borrow".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::KeepCurrent,
                binding: IntentionBinding::PropertiesOnly,
                obligations: vec![property(&["spare"])],
            },
            vec![scene],
            "save-availability",
        )
        .unwrap();
    engine.adopt(store, &ready).unwrap();
}
#[test]
fn row_action_property_survives_reopen_fresh_replay_and_blocks_violating_adoption() {
    let (a, b, _) = pair();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    saved_property(&store, &a);
    drop(store);
    let store = ProductStore::open(dir.path().join("tool")).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut compatible = equipment();
    compatible["label"] = json!("Reimplemented equipment");
    // Replace executable guard structure, rather than only changing a label.
    compatible["actions"][0]["guards"][0] = json!({
        "kind":"equal", "left": compatible["actions"][0]["guards"][0]["value"].clone(),
        "right":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}
    });
    let compatible = capture(compatible);
    let ready = engine
        .prepare_change(&store, &compatible, &[], "preserve-availability")
        .unwrap();
    assert_eq!(ready.report().disposition, CheckDisposition::Ready);
    assert_eq!(ready.report().business_questions, 0);
    assert!(ready
        .report()
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied && c.binding.artifact == compatible.artifact));
    let saved = engine.adopt(&store, &ready).unwrap();
    let check = engine.check_all(&saved.decisions, &b, &[]).unwrap();
    assert_eq!(check.disposition, CheckDisposition::RepairRequired);
    assert!(check.checks.iter().any(|c| c.state == CheckState::Violated));
    assert!(engine
        .prepare_change(&store, &b, &[], "bad-availability")
        .is_err());
    assert_eq!(store.load().unwrap(), saved);
}
fn rename(kind: SemanticKind, from: &str, to: &str) -> SemanticMapping {
    SemanticMapping {
        from: SemanticKey {
            kind,
            entity: None,
            id: from.into(),
        },
        to: SemanticKey {
            kind,
            entity: None,
            id: to.into(),
        },
    }
}
#[test]
fn row_action_mappings_require_explicit_view_action_and_exact_binding_identity() {
    let a = capture(equipment());
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    saved_property(&store, &a);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let mut value = equipment();
    value["actions"][0]["id"] = json!("checkout");
    value["views"][0]["actions"][0]["action"] = json!("checkout");
    value["views"][0]["id"] = json!("inventory");
    value["initial_view"] = json!("inventory");
    let mapped = capture(value.clone());
    let maps = [
        rename(SemanticKind::Action, "borrow", "checkout"),
        rename(SemanticKind::View, "assets", "inventory"),
    ];
    assert!(engine
        .prepare_change(&store, &mapped, &[], "unmapped")
        .is_err());
    let ready = engine
        .prepare_change(&store, &mapped, &maps, "mapped")
        .unwrap();
    assert_eq!(ready.report().disposition, CheckDisposition::Ready);
    assert!(engine
        .prepare_change(
            &store,
            &mapped,
            &[maps[0].clone(), maps[0].clone(), maps[1].clone()],
            "ambiguous-mapping"
        )
        .is_err());
    assert!(engine
        .prepare_change(
            &store,
            &mapped,
            &[
                rename(SemanticKind::Action, "borrow", "return_loan"),
                maps[1].clone()
            ],
            "colliding-mapping"
        )
        .is_err());
    value["views"][0]["actions"][0]["id"] = json!("renamed_binding");
    assert!(engine
        .prepare_change(&store, &capture(value), &maps, "binding-renamed")
        .is_err());
    let saved = engine.adopt(&store, &ready).unwrap();
    drop(engine);
    drop(store);
    let store = ProductStore::open(dir.path().join("tool")).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    assert_eq!(
        engine.check_current(&saved).unwrap().disposition,
        CheckDisposition::Ready
    );
}

/// Simulates the former observation format without rewriting a saved receipt.
struct LegacyObservationRuntime(LocalRuntime);
impl RuntimeAdapter for LegacyObservationRuntime {
    type Run = product_runtime::ProductRun;
    fn capabilities(&self) -> RuntimeCapabilities {
        let mut c = self.0.capabilities();
        c.version = "local-interpreter/3".into();
        c
    }
    fn validate(&self, p: &CapturedProgram) -> Result<(), AdapterError> {
        self.0.validate(p)
    }
    fn start(
        &self,
        p: &CapturedProgram,
        d: &DataSnapshot,
        s: &SessionState,
        day: i32,
        seed: u64,
        limits: RuntimeLimits,
    ) -> Result<Self::Run, AdapterError> {
        self.0.start(p, d, s, day, seed, limits)
    }
    fn apply(
        &self,
        r: &mut Self::Run,
        i: &SemanticInput,
        id: &str,
    ) -> Result<TraceStep, AdapterError> {
        self.0.apply(r, i, id)
    }
    fn observe(&self, r: &Self::Run, p: &str) -> Result<Observation, AdapterError> {
        let mut o = self.0.observe(r, p)?;
        if let Some(s) = &mut o.view_schema {
            s.row_actions = None;
        }
        Ok(o)
    }
    fn data<'a>(&self, r: &'a Self::Run) -> &'a DataSnapshot {
        self.0.data(r)
    }
    fn session<'a>(&self, r: &'a Self::Run) -> &'a SessionState {
        self.0.session(r)
    }
    fn compatibility(
        &self,
        p: &CapturedProgram,
        d: &DataSnapshot,
    ) -> Result<CompatibilityReport, AdapterError> {
        self.0.compatibility(p, d)
    }
    fn prepare_adoption(
        &self,
        p: AdoptionPlan,
        d: &DataSnapshot,
    ) -> Result<AdoptionPlan, AdapterError> {
        self.0.prepare_adoption(p, d)
    }
}
#[test]
fn legacy_archives_remain_immutable_and_stale_after_reopen() {
    let p = capture(equipment());
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let runtime = LegacyObservationRuntime(LocalRuntime::default());
    let scene = accept_scene(
        &runtime,
        &p,
        &exercised(&p),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let engine = DecisionEngine::new(runtime, IntentArchive::new(store.clone()));
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            Choice {
                id: "old-choice".into(),
                request: "Keep borrowing behavior".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["borrow".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::KeepCurrent,
                binding: IntentionBinding::ObservedOutcome,
                obligations: vec![],
            },
            vec![scene],
            "save-old",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let digest = saved.decisions.decisions[0].witness.clone();
    let original = store.read_extension(&digest).unwrap();
    drop(engine);
    drop(store);
    let store = ProductStore::open(dir.path().join("tool")).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let report = engine.check_current(&store.load().unwrap()).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert!(report.checks.iter().any(|c| c.state == CheckState::Stale));
    assert!(!report
        .checks
        .iter()
        .any(|c| c.state == CheckState::Satisfied));
    assert!(engine
        .prepare_change(&store, &p, &[], "unsupported-old")
        .is_err());
    assert_eq!(store.read_extension(&digest).unwrap(), original);
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn retained_row_action_vectors_settle_only_complete_known_outcomes() {
    for mapped in [false, true] {
        check_retained_row_action_vectors(mapped);
    }
}
fn check_retained_row_action_vectors(mapped: bool) {
    let (a, b, _) = pair();
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &a, 20000).unwrap();
    let s = exercised(&a);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenes = [&a, &b]
        .into_iter()
        .map(|p| {
            accept_scene(
                &LocalRuntime::default(),
                p,
                &s,
                Disclosure::Synthetic,
                RuntimeLimits::default(),
            )
            .unwrap()
        })
        .collect();
    let ready = engine
        .prepare_choice(
            &store,
            &a,
            Choice {
                id: "either-availability".into(),
                request: "Either demonstrated eligibility result is acceptable".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["borrow".into()].into(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::EitherAcceptable,
                binding: IntentionBinding::ObservedOutcome,
                obligations: vec![],
            },
            scenes,
            "save-either",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let mut current = a.clone();
    let mut s = s;
    let remap = |value: &mut Value| {
        value["actions"][0]["id"] = json!("checkout");
        value["views"][0]["actions"][0]["action"] = json!("checkout");
        value["views"][0]["id"] = json!("inventory");
        value["initial_view"] = json!("inventory");
    };
    if mapped {
        let mut value = equipment();
        remap(&mut value);
        current = capture(value);
        let mappings = [
            rename(SemanticKind::Action, "borrow", "checkout"),
            rename(SemanticKind::View, "assets", "inventory"),
        ];
        let ready = engine
            .prepare_change(&store, &current, &mappings, "rename-accepted")
            .unwrap();
        engine.adopt(&store, &ready).unwrap();
        s.session.view = "inventory".into();
        if let SemanticInput::Invoke { action, .. } = &mut s.inputs[0] {
            *action = "checkout".into();
        }
    }
    drop(engine);
    drop(store);
    let store = ProductStore::open(dir.path().join("tool")).unwrap();
    for novel in [false, true] {
        let mut value = equipment();
        value["actions"][0]["guards"] = if novel {
            json!([{"kind":"equal","left":field(var("asset"),"name"),"right":text("Saw")}])
        } else {
            json!([yes()])
        };
        if mapped {
            remap(&mut value);
        }
        let candidate = capture(value);
        let (mut r, mut out) = discovery_input(&current, &candidate, &s);
        if mapped {
            r.context.view = Some("inventory".into());
            out.response.hypotheses[0].action = "checkout".into();
        }
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        if mapped {
            let scenes = engine.discovery_scenes(&store.load().unwrap()).unwrap();
            assert_eq!(scenes.len(), 2);
            assert!(scenes.iter().all(|scene| {
                scene.mapped().session.view == "inventory"
                    && scene.operations() == &["checkout".into()].into()
            }));
        }
        let request = engine.inherit_request(&store.load().unwrap(), r).unwrap();
        out.response.request_digest = request.identity().unwrap();
        let report = discover_pair(
            &request,
            &out,
            DiscoveryPolicy {
                retained_history: Some(VerifiedRetainedHistory::load(&store).unwrap()),
                ..DiscoveryPolicy::default()
            },
        );
        assert!(
            report.unverified.is_empty(),
            "novel={novel}: {:?}",
            report.unverified
        );
        if novel {
            assert!(!report.questions.is_empty(), "{:?}", report.log);
        } else {
            assert!(
                report.questions.is_empty(),
                "mapped={mapped}: {:?}",
                report.log
            );
            assert!(
                report
                    .log
                    .iter()
                    .any(|entry| entry.disposition == Disposition::Settled),
                "{:?}",
                report.log
            );
        }
    }
}

#[test]
fn row_action_budget_limits_never_claim_complete_coverage_or_minimality() {
    let (a, b, s) = pair();
    let limited = comparison()
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            SearchBudget {
                max_trials: 1,
                ..SearchBudget::default()
            },
        )
        .unwrap();
    assert!(
        !limited
            .witness
            .unwrap()
            .witness()
            .minimization
            .as_ref()
            .unwrap()
            .complete
    );
    let (request, out) = discovery_input(&a, &b, &exercised(&a));
    let report = discover_pair(
        &request,
        &out,
        DiscoveryPolicy {
            max_comparisons: 0,
            ..DiscoveryPolicy::default()
        },
    );
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert!(report
        .log
        .iter()
        .all(|entry| entry.state != EvidenceState::NoDifferenceFound));
}
