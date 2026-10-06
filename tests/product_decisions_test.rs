#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::ProductStore;
use serde_json::json;

fn scope() -> DecisionScope {
    DecisionScope {
        operations: ["export_people".into()].into(),
        population: Population::All,
        conditions: Values::new(),
        excluded_records: vec![],
        unknowns: vec![],
    }
}
fn scene(p: &CapturedProgram) -> AcceptedScene {
    accept_scene(
        &LocalRuntime::default(),
        p,
        &scenario(
            p,
            vec![
                add("Ada"),
                invoke("collect", Values::new()),
                invoke("export_people", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        ),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap()
}
fn choice(id: &str, outcome: DecisionOutcome) -> Choice {
    Choice {
        id: id.into(),
        request: "Keep the observed export behavior".into(),
        rationale: None,
        scope: scope(),
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    }
}
fn setup(dir: &tempfile::TempDir) -> (ProductStore, DecisionEngine<LocalRuntime>, CapturedProgram) {
    let p = capture(organizer());
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    (store, engine, p)
}
#[test]
fn scope_is_three_valued_and_never_generalizes_export() {
    let mut s = scope();
    s.population = Population::NewWork;
    s.conditions.insert("area".into(), string("north"));
    let mut c = ScopeContext {
        operation: "export_people".into(),
        record: None,
        is_new: Some(true),
        created_generation: Some(3),
        attributes: args(&[("area", string("north"))]),
        predicate_result: None,
    };
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Applies);
    c.operation = "delete".into();
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Outside);
    c.operation = "export_people".into();
    c.is_new = None;
    let unknown = scope_match(&s, &c);
    assert_eq!(unknown.state, ScopeMatch::Unknown);
    assert!(!unknown.reason.is_empty());
    c.is_new = Some(true);
    c.attributes.clear();
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Unknown);
    c.attributes.insert("area".into(), string("south"));
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Outside);
    s.conditions.clear();
    s.population = Population::All;
    s.excluded_records.push(RecordRef {
        entity: "person".into(),
        record: "one".into(),
    });
    c.record = Some(s.excluded_records[0].clone());
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Outside);
    c.record = None;
    assert_eq!(scope_match(&s, &c).state, ScopeMatch::Unknown);
}
#[test]
fn every_nonbinary_choice_reopens_without_activating_or_switching() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store, engine, p) = setup(&dir);
        let added = store
            .apply(0, "existing", &add("Existing"), RuntimeLimits::default())
            .unwrap();
        let searched = store
            .apply(
                added.revision,
                "search",
                &SemanticInput::Control {
                    view: "people".into(),
                    control: "search_input".into(),
                    value: string("Existing"),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
        let selected = store
            .apply(
                searched.revision,
                "select",
                &SemanticInput::Control {
                    view: "people".into(),
                    control: "pick".into(),
                    value: DataValue::List {
                        item_type: Type::reference("person"),
                        items: vec![reference("person", &added.data.records[0].id)],
                    },
                },
                RuntimeLimits::default(),
            )
            .unwrap();
        let before = store
            .apply(
                selected.revision,
                "navigate",
                &SemanticInput::Navigate {
                    view: "new_person".into(),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
        let prepared = engine
            .prepare_choice(
                &store,
                &p,
                choice("unsettled", outcome.clone()),
                vec![scene(&p)],
                "remember",
            )
            .unwrap();
        let after = engine.adopt(&store, &prepared).unwrap();
        assert_eq!(after.active_revision, before.active_revision);
        assert_eq!(after.data, before.data);
        assert_eq!(after.session, before.session);
        assert_eq!(after.decisions.decisions[0].outcome, outcome);
        assert_eq!(after.decisions.decisions[0].status, DecisionStatus::Pending);
        let reopened = ProductStore::open(dir.path().join("tool"))
            .unwrap()
            .load()
            .unwrap();
        let fresh = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        assert_eq!(reopened, after);
        let request = fresh
            .development_request(
                &reopened,
                "next",
                DevelopmentOperation::Modify,
                "Improve layout",
                DevelopmentContext {
                    view: None,
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(after.data.identity().unwrap()),
                    session_digest: None,
                },
            )
            .unwrap();
        assert_eq!(request.decisions, after.decisions);
        assert_eq!(
            request.unknowns.is_empty(),
            outcome == DecisionOutcome::EitherAcceptable
        );
        assert_eq!(request.examples.len(), 1);
        assert_eq!(
            fresh
                .check_all(&after.decisions, &p, &[])
                .unwrap()
                .disposition,
            CheckDisposition::Ready
        );
    }
}
#[test]
fn fresh_agent_context_and_independent_checks_protect_an_empty_predicate_choice() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let selected = engine
        .prepare_choice(
            &store,
            &p,
            choice(
                "chosen",
                DecisionOutcome::Accept {
                    artifact: p.artifact.program_digest.clone(),
                },
            ),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &selected).unwrap();
    let mut alternate = organizer();
    alternate["actions"][2]["steps"][0]["items"] = json!({"kind":"filter","items":{"kind":"state","state":"selected"},"binding":"item","predicate":yes()});
    let alternate = capture(alternate);
    let checked = engine.check_all(&saved.decisions, &alternate, &[]).unwrap();
    assert_eq!(checked.disposition, CheckDisposition::Ready);
    assert!(!checked.checks.is_empty());
    assert!(checked
        .checks
        .iter()
        .all(|c| c.state == CheckState::Satisfied));
    assert_eq!(checked.runs[0].origin, ExecutionOrigin::ProductionRuntime);
    let mut bad = organizer();
    bad["actions"][2]["steps"][0]["items"] = json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")});
    let bad = capture(bad);
    let rejected = engine.check_all(&saved.decisions, &bad, &[]).unwrap();
    assert_eq!(rejected.disposition, CheckDisposition::RepairRequired);
    assert_eq!(rejected.business_questions, 0);
    assert!(engine.prepare_change(&store, &bad, &[], "bad").is_err());
    assert_eq!(store.load().unwrap(), saved);
    let fresh = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let request = fresh
        .development_request(
            &saved,
            "fresh-agent",
            DevelopmentOperation::Modify,
            "Add a summary",
            DevelopmentContext {
                view: Some("people".into()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: None,
                session_digest: None,
            },
        )
        .unwrap();
    let bytes = canonical_bytes(&request).unwrap();
    let parsed: DevelopmentRequest = serde_json::from_slice(&bytes).unwrap();
    parsed.validate().unwrap();
    assert_eq!(parsed.decisions, saved.decisions);
    assert_eq!(parsed.examples[0].scenario, scene(&p).scenario().clone());
    assert!(parsed.sources.iter().any(|s| s.artifact == p.artifact));
}
#[test]
fn action_mappings_are_executable_and_missing_or_untrusted_mappings_are_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let selected = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &selected).unwrap();
    let mut changed = organizer();
    changed["actions"][2]["id"] = json!("download");
    changed["views"][0]["actions"][0]["action"] = json!("download");
    let changed = capture(changed);
    let report = engine.check_all(&saved.decisions, &changed, &[]).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert!(report.checks.iter().all(|c| !c.explanation.is_empty()));
    let map = SemanticMapping {
        from: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "export_people".into(),
        },
        to: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "download".into(),
        },
    };
    assert_eq!(
        engine
            .check_all(&saved.decisions, &changed, &[map.clone()])
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    let mut fake = map;
    fake.to.id = "missing".into();
    assert!(engine
        .check_all(&saved.decisions, &changed, &[fake])
        .is_err());
}
#[test]
fn exact_current_state_and_runtime_changes_invalidate_prior_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let selected = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &selected).unwrap();
    let prepared = engine.prepare_change(&store, &p, &[], "again").unwrap();
    let later = store
        .apply(
            saved.revision,
            "later",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(engine.adopt(&store, &prepared).is_err());
    assert_eq!(store.load().unwrap(), later);
    let report = engine.check_all(&later.decisions, &p, &[]).unwrap();
    let mut binding = report.runs[0].binding.clone();
    binding.runtime_version = "changed-runtime".into();
    assert!(!report.runs[0].is_current(&binding));
    binding = report.runs[0].binding.clone();
    binding.data_digest = later.data.identity().unwrap();
    assert!(!report.runs[0].is_current(&binding));
}

#[test]
fn adopted_mappings_survive_reopen_and_enter_the_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let saved = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save",
        )
        .unwrap();
    engine.adopt(&store, &saved).unwrap();
    let mut changed = organizer();
    changed["actions"][2]["id"] = json!("download");
    changed["views"][0]["actions"][0]["action"] = json!("download");
    let changed = capture(changed);
    let map = SemanticMapping {
        from: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "export_people".into(),
        },
        to: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "download".into(),
        },
    };
    let ready = engine
        .prepare_change(&store, &changed, &[map], "rename")
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let reopened = ProductStore::open(dir.path().join("tool"))
        .unwrap()
        .load()
        .unwrap();
    let fresh = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    assert_eq!(
        fresh.check_current(&reopened).unwrap().disposition,
        CheckDisposition::Ready
    );
    assert!(fresh
        .prepare_change(&store, &changed, &[], "another")
        .is_ok());
    let request = fresh
        .development_request(
            &reopened,
            "next",
            DevelopmentOperation::Modify,
            "Add a footer",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: None,
                session_digest: None,
            },
        )
        .unwrap();
    assert!(request.request.contains("download"));
    assert!(request.request.contains("export_people"));
    let mut third = organizer();
    third["actions"][2]["id"] = json!("save_file");
    third["views"][0]["actions"][0]["action"] = json!("save_file");
    let third = capture(third);
    let map = SemanticMapping {
        from: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "download".into(),
        },
        to: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "save_file".into(),
        },
    };
    let ready = fresh
        .prepare_change(&store, &third, &[map], "second-rename")
        .unwrap();
    let adopted = fresh.adopt(&store, &ready).unwrap();
    assert_eq!(
        fresh.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
}
#[test]
fn unknown_active_scope_and_tampered_scene_never_authorize_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let before = store.load().unwrap();
    let mut chosen = choice("chosen", DecisionOutcome::KeepCurrent);
    chosen
        .scope
        .conditions
        .insert("area".into(), string("north"));
    assert!(engine
        .prepare_choice(&store, &p, chosen, vec![scene(&p)], "unknown")
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let saved = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save",
        )
        .unwrap();
    let saved = engine.adopt(&store, &saved).unwrap();
    let witness = &saved.decisions.decisions[0].witness;
    std::fs::write(
        dir.path()
            .join("tool")
            .join(format!("extension-{}.json", witness.as_str())),
        b"[]",
    )
    .unwrap();
    assert!(engine.check_current(&saved).is_err());
    assert!(engine.prepare_change(&store, &p, &[], "tampered").is_err());
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn portable_archive_restores_exact_reachable_intentions_without_trusting_checks() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let archive = IntentArchive::new(store.clone());
    let bundle = archive.export_for(&saved).unwrap();
    validate_bundle(&saved, &bundle).unwrap();
    let recovered =
        ProductStore::create_recovered_with(dir.path().join("restored-tool"), &saved, |target| {
            IntentArchive::new(target.clone())
                .restore_for(&saved, &bundle)
                .map_err(|e| product_store::StoreError::Invalid(e.to_string()))
        })
        .unwrap();
    assert_eq!(recovered.load().unwrap(), saved);
    let restored = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(recovered.clone()),
    );
    assert_eq!(
        restored
            .check_current(&recovered.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    let reopened = ProductStore::open(dir.path().join("restored-tool")).unwrap();
    let fresh = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(reopened.clone()),
    );
    assert_eq!(
        fresh
            .check_current(&reopened.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
    assert_eq!(
        bundle.snapshot_digest(),
        &canonical_digest(IdentityDomain::Data, &saved).unwrap()
    );
    assert!(bundle.object_count() >= 2);
    let mut encoded = serde_json::to_value(&bundle).unwrap();
    encoded["objects"] = json!([]);
    let missing: IntentionBundle = serde_json::from_value(encoded).unwrap();
    assert!(validate_bundle(&saved, &missing).is_err());
    let later = store
        .apply(
            saved.revision,
            "later",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(validate_bundle(&later, &bundle).is_err());
    assert_eq!(store.load().unwrap(), later);
}

#[test]
fn missing_observables_and_cancelled_execution_remain_unverified() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let mut chosen = choice("promise", DecisionOutcome::KeepCurrent);
    chosen.obligations.push(AcceptedProperty {
        id: "count-match".into(),
        description: "Preview equals actual exported rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::OutputCount {
                point: "done".into(),
                output: "roster".into(),
            },
        },
    });
    let ready = engine
        .prepare_choice(&store, &p, chosen, vec![scene(&p)], "save")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut missing = organizer();
    missing["observables"] = json!([]);
    let missing = capture(missing);
    let report = engine.check_all(&saved.decisions, &missing, &[]).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert_eq!(report.checks[0].state, CheckState::Unknown);
    assert_eq!(report.business_questions, 0);
    let cancelled = DecisionEngine::new(
        LocalRuntime::with_cancellation(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            true,
        ))),
        IntentArchive::new(store.clone()),
    );
    assert_eq!(
        cancelled.check_current(&saved).unwrap().disposition,
        CheckDisposition::Unverified
    );
    assert_eq!(store.load().unwrap(), saved);
    let other_dir = tempfile::tempdir().unwrap();
    let (other, empty_engine, p) = setup(&other_dir);
    let ready = empty_engine
        .prepare_choice(
            &other,
            &p,
            choice("implicit", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save-empty",
        )
        .unwrap();
    let saved = empty_engine.adopt(&other, &ready).unwrap();
    let report = empty_engine
        .check_all(&saved.decisions, &missing, &[])
        .unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert_eq!(report.checks[0].state, CheckState::Unknown);
}

#[test]
fn captured_scene_uses_the_production_replay_record_identities() {
    let p = capture(organizer());
    let s = scenario(
        &p,
        vec![
            add("Ada"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = LocalRuntime::default();
    let actual = runtime
        .replay(&p, &s, &decisions(), RuntimeLimits::default(), "production")
        .unwrap();
    let captured = accept_scene(
        &runtime,
        &p,
        &s,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    assert_eq!(captured.observations(), actual.observations);
}

struct BudgetOnCandidate {
    runtime: LocalRuntime,
    candidate: Digest,
}
impl RuntimeAdapter for BudgetOnCandidate {
    type Run = product_runtime::ProductRun;
    fn capabilities(&self) -> RuntimeCapabilities {
        self.runtime.capabilities()
    }
    fn validate(&self, p: &CapturedProgram) -> Result<(), AdapterError> {
        self.runtime.validate(p)
    }
    fn start(
        &self,
        p: &CapturedProgram,
        d: &DataSnapshot,
        s: &SessionState,
        day: i32,
        seed: u64,
        mut limits: RuntimeLimits,
    ) -> Result<Self::Run, AdapterError> {
        if p.artifact.program_digest == self.candidate {
            limits.fuel = 1;
        }
        self.runtime.start(p, d, s, day, seed, limits)
    }
    fn apply(
        &self,
        r: &mut Self::Run,
        i: &SemanticInput,
        id: &str,
    ) -> Result<TraceStep, AdapterError> {
        self.runtime.apply(r, i, id)
    }
    fn observe(&self, r: &Self::Run, p: &str) -> Result<Observation, AdapterError> {
        self.runtime.observe(r, p)
    }
    fn data<'a>(&self, r: &'a Self::Run) -> &'a DataSnapshot {
        self.runtime.data(r)
    }
    fn session<'a>(&self, r: &'a Self::Run) -> &'a SessionState {
        self.runtime.session(r)
    }
    fn compatibility(
        &self,
        p: &CapturedProgram,
        d: &DataSnapshot,
    ) -> Result<CompatibilityReport, AdapterError> {
        self.runtime.compatibility(p, d)
    }
    fn prepare_adoption(
        &self,
        p: AdoptionPlan,
        d: &DataSnapshot,
    ) -> Result<AdoptionPlan, AdapterError> {
        self.runtime.prepare_adoption(p, d)
    }
}
#[test]
fn an_exhausted_candidate_is_unverified_after_the_accepted_source_passes() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut target = organizer();
    target["label"] = json!("Candidate budget test");
    let target = capture(target);
    let limited = DecisionEngine::new(
        BudgetOnCandidate {
            runtime: LocalRuntime::default(),
            candidate: target.artifact.program_digest.clone(),
        },
        IntentArchive::new(store.clone()),
    );
    let report = limited.check_all(&saved.decisions, &target, &[]).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert_eq!(report.runs[0].state, EvidenceState::Inconclusive);
    assert_eq!(report.checks[0].state, CheckState::Unknown);
    assert_eq!(report.business_questions, 0);
}

#[test]
fn old_mappings_do_not_redirect_intentions_accepted_on_the_new_design() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("original", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "save-original",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let mut design = organizer();
    let mut collected = design["actions"][2].clone();
    collected["id"] = json!("download");
    design["actions"].as_array_mut().unwrap().push(collected);
    design["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"download_button","label":"Download collection","placement":"toolbar","action":"download","arguments":{},"enabled":yes()}));
    design["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}));
    let target = capture(design.clone());
    let map = SemanticMapping {
        from: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "export_people".into(),
        },
        to: SemanticKey {
            kind: SemanticKind::Action,
            entity: None,
            id: "download".into(),
        },
    };
    let ready = engine
        .prepare_change(&store, &target, &[map], "evolve")
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let ready = engine
        .prepare_choice(
            &store,
            &target,
            choice("new-default", DecisionOutcome::KeepCurrent),
            vec![scene(&target)],
            "save-new",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    design["label"] = json!("Later implementation");
    let later = capture(design);
    let ready = engine.prepare_change(&store, &later, &[], "later").unwrap();
    let adopted = engine.adopt(&store, &ready).unwrap();
    assert_eq!(
        engine.check_current(&adopted).unwrap().disposition,
        CheckDisposition::Ready
    );
    assert_eq!(adopted.decisions.decisions.len(), 2);
}

#[test]
fn a_later_choice_resolves_exact_pending_history_without_reasking() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("later", DecisionOutcome::Deferred),
            vec![scene(&p)],
            "save-later",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let ready = engine
        .prepare_resolution(
            &store,
            &p,
            choice("settled", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            &["later".into()],
            "settle",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    assert!(
        matches!(&saved.decisions.decisions[0].status,DecisionStatus::Superseded{by} if by=="settled")
    );
    assert_eq!(saved.decisions.decisions[1].supersedes, vec!["later"]);
    assert_eq!(saved.decisions.decisions[1].status, DecisionStatus::Active);
    let request = engine
        .development_request(
            &saved,
            "next",
            DevelopmentOperation::Modify,
            "Change the label",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: None,
                session_digest: None,
            },
        )
        .unwrap();
    assert!(request.unknowns.is_empty());
    assert_eq!(engine.check_current(&saved).unwrap().business_questions, 0);
    assert!(engine
        .prepare_resolution(
            &store,
            &p,
            choice("unapproved", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            &["settled".into()],
            "wrong"
        )
        .is_err());
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn accepted_toolbar_and_row_action_availability_survives_implementation_changes() {
    for placement in ["toolbar", "row"] {
        let dir = tempfile::tempdir().unwrap();
        let mut source = organizer();
        source["views"][0]["actions"][0]["placement"] = json!(placement);
        let p = capture(source.clone());
        let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let ready = engine
            .prepare_choice(
                &store,
                &p,
                choice(
                    "available",
                    DecisionOutcome::Accept {
                        artifact: p.artifact.program_digest.clone(),
                    },
                ),
                vec![scene(&p)],
                "accept",
            )
            .unwrap();
        let saved = engine.adopt(&store, &ready).unwrap();
        source["views"][0]["actions"][0]["enabled"] = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}});
        let disabled = capture(source);
        let report = engine.check_all(&saved.decisions, &disabled, &[]).unwrap();
        assert_eq!(
            report.disposition,
            CheckDisposition::RepairRequired,
            "{placement}"
        );
        assert!(engine
            .prepare_change(&store, &disabled, &[], "disable")
            .is_err());
        assert_eq!(store.load().unwrap(), saved);
    }
}
#[test]
fn definite_output_violation_is_not_hidden_by_an_unmapped_observable() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice(
                "accepted",
                DecisionOutcome::Accept {
                    artifact: p.artifact.program_digest.clone(),
                },
            ),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut bad = organizer();
    bad["observables"].as_array_mut().unwrap().remove(0);
    bad["actions"][2]["steps"][0]["items"] = json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")});
    let report = engine
        .check_all(&saved.decisions, &capture(bad), &[])
        .unwrap();
    assert_eq!(report.disposition, CheckDisposition::RepairRequired);
    assert_eq!(report.business_questions, 0);
}

#[test]
fn discovery_enrichment_preserves_the_candidate_pair_and_current_context() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice(
                "accepted",
                DecisionOutcome::Accept {
                    artifact: p.artifact.program_digest.clone(),
                },
            ),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    engine.adopt(&store, &ready).unwrap();
    let mut changed = organizer();
    changed["label"] = json!("Current display");
    let current_program = capture(changed.clone());
    let ready = engine
        .prepare_change(&store, &current_program, &[], "display")
        .unwrap();
    let current = engine.adopt(&store, &ready).unwrap();
    let request = engine
        .development_request(
            &current,
            "fresh",
            DevelopmentOperation::Modify,
            "Improve this tool",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(current.data.identity().unwrap()),
                session_digest: Some(current.session.identity().unwrap()),
            },
        )
        .unwrap();
    assert_eq!(request.sources.last(), Some(&current_program));
    changed["label"] = json!("Candidate display");
    let candidate = capture(changed);
    let mut discovery = request.clone();
    discovery.operation = DevelopmentOperation::Discover;
    discovery.sources = vec![current_program.clone(), candidate.clone()];
    let enriched = engine.inherit_request(&current, discovery).unwrap();
    assert_eq!(enriched.sources[0], current_program);
    assert_eq!(enriched.sources[1], candidate);
    assert!(enriched.sources[2..].contains(&p));
    let mut stale = request;
    stale.context.data_digest =
        Some(canonical_digest(IdentityDomain::Data, &"older data").unwrap());
    assert!(engine.inherit_request(&current, stale).is_err());
}

#[test]
fn scoped_properties_do_not_reject_real_outside_boundary_scenes() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let inside = scene(&p);
    let outside_spec = scenario(
        &p,
        vec![
            invoke(
                "add_person",
                args(&[("name", string("South")), ("area", string("south"))]),
            ),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let outside = accept_scene(
        &LocalRuntime::default(),
        &p,
        &outside_spec,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let mut scoped = choice("north", DecisionOutcome::KeepCurrent);
    scoped.scope.operations = ["add_person".into()].into();
    scoped.scope.conditions = args(&[("area", string("north"))]);
    scoped.obligations = vec![AcceptedProperty {
        id: "one-selected".into(),
        description: "One selected item in this north workflow".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 1 },
            },
        },
    }];
    let ready = engine
        .prepare_choice(&store, &p, scoped, vec![inside, outside], "accept-north")
        .unwrap();
    assert!(ready
        .report()
        .checks
        .iter()
        .any(|c| c.state == CheckState::Outside));
    let saved = engine.adopt(&store, &ready).unwrap();
    assert_eq!(
        engine.check_current(&saved).unwrap().disposition,
        CheckDisposition::Ready
    );
}

#[test]
fn every_promised_operation_needs_an_applicable_replayed_scene() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let export_only = accept_scene(
        &LocalRuntime::default(),
        &p,
        &scenario(
            &p,
            vec![
                invoke("export_people", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        ),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let mut promised = choice("two-operations", DecisionOutcome::KeepCurrent);
    promised.scope.operations.insert("add_person".into());
    assert!(engine
        .prepare_choice(
            &store,
            &p,
            promised.clone(),
            vec![export_only.clone()],
            "accept"
        )
        .is_err());
    promised.outcome = DecisionOutcome::Deferred;
    let ready = engine
        .prepare_choice(&store, &p, promised, vec![export_only], "pending")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut unsupported = saved.decisions.clone();
    unsupported.decisions[0].status = DecisionStatus::Active;
    unsupported.decisions[0].outcome = DecisionOutcome::KeepCurrent;
    let report = engine.check_all(&unsupported, &p, &[]).unwrap();
    assert_eq!(report.disposition, CheckDisposition::Unverified);
    assert!(report
        .checks
        .iter()
        .any(|c| c.explanation.contains("add_person") && c.state == CheckState::Unknown));
}

#[test]
fn pending_history_is_independently_executed_before_it_is_stored() {
    for outcome in [
        DecisionOutcome::Deferred,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::EitherAcceptable,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store, engine, p) = setup(&dir);
        let mut value = serde_json::to_value(scene(&p)).unwrap();
        value["evidence"]["observations"][0]["values"]["selected_count"]["value"] = json!(999);
        let forged: AcceptedScene = serde_json::from_value(value).unwrap();
        let before = store.load().unwrap();
        assert!(engine
            .prepare_choice(
                &store,
                &p,
                choice("pending", outcome),
                vec![forged],
                "remember"
            )
            .is_err());
        assert_eq!(store.load().unwrap(), before);
    }
}

#[test]
fn empty_view_column_types_and_absent_old_metadata_do_not_become_passes() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let s = accept_scene(
        &LocalRuntime::default(),
        &p,
        &scenario(
            &p,
            vec![
                invoke("export_people", Values::new()),
                SemanticInput::Observe {
                    point: "done".into(),
                },
            ],
        ),
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    assert!(s.observations()[0].view.rows.is_empty());
    assert!(s.observations()[0].view_schema.is_some());
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("typed-view", DecisionOutcome::KeepCurrent),
            vec![s.clone()],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut different = organizer();
    different["views"][0]["kind"]["columns"][0]["value"] = json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":0}});
    let report = engine
        .check_all(&saved.decisions, &capture(different), &[])
        .unwrap();
    assert_eq!(report.disposition, CheckDisposition::RepairRequired);
    let mut old = serde_json::to_value(&s).unwrap();
    old["evidence"]["observations"][0]
        .as_object_mut()
        .unwrap()
        .remove("view_schema");
    let old: AcceptedScene = serde_json::from_value(old).unwrap();
    assert!(engine
        .prepare_choice(
            &store,
            &p,
            choice("old-metadata", DecisionOutcome::Deferred),
            vec![old],
            "save-old"
        )
        .is_err());
}

#[test]
fn imported_history_cannot_omit_observables_or_view_cells() {
    for remove_cells in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (store, engine, p) = setup(&dir);
        let mut raw = serde_json::to_value(scene(&p)).unwrap();
        let observation = &mut raw["evidence"]["observations"][0];
        if remove_cells {
            observation["view"]["rows"][0]["cells"]
                .as_object_mut()
                .unwrap()
                .remove("name");
            observation["view_schema"]["columns"]
                .as_object_mut()
                .unwrap()
                .remove("name");
        } else {
            observation["values"]
                .as_object_mut()
                .unwrap()
                .remove("selected_count");
            observation["value_types"]
                .as_object_mut()
                .unwrap()
                .remove("selected_count");
        }
        let omitted: AcceptedScene = serde_json::from_value(raw).unwrap();
        let before = store.load().unwrap();
        assert!(engine
            .prepare_choice(
                &store,
                &p,
                choice("omitted", DecisionOutcome::Deferred),
                vec![omitted],
                "store-history"
            )
            .is_err());
        assert_eq!(store.load().unwrap(), before);
    }
}

#[test]
fn one_bundle_digest_cannot_alias_scene_and_mapping_reference_roles() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("chosen", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let archive = IntentArchive::new(store.clone());
    let bundle = archive.export_for(&saved).unwrap();
    let mut forged = saved.clone();
    let old_mapping = forged.adoptions[0].plan.evidence[0].clone();
    forged.adoptions[0].plan.evidence[0] = forged.decisions.decisions[0].witness.clone();
    forged.validate().unwrap();
    let mut raw = serde_json::to_value(&bundle).unwrap();
    raw["snapshot"] =
        serde_json::to_value(canonical_digest(IdentityDomain::Data, &forged).unwrap()).unwrap();
    raw["objects"]
        .as_array_mut()
        .unwrap()
        .retain(|o| o["digest"] != serde_json::to_value(&old_mapping).unwrap());
    let forged_bundle: IntentionBundle = serde_json::from_value(raw).unwrap();
    assert!(validate_bundle(&forged, &forged_bundle).is_err());
    assert!(archive.restore_for(&forged, &forged_bundle).is_err());
}

#[test]
fn adding_a_predicate_never_weakens_a_concrete_chosen_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let mut chosen = choice("concrete", DecisionOutcome::KeepCurrent);
    chosen.obligations = vec![AcceptedProperty {
        id: "count-match".into(),
        description: "Preview equals exported rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::OutputCount {
                point: "done".into(),
                output: "roster".into(),
            },
        },
    }];
    let ready = engine
        .prepare_choice(&store, &p, chosen, vec![scene(&p)], "accept")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let mut empty = organizer();
    empty["actions"][2]["steps"].as_array_mut().unwrap().insert(0,json!({"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":fixture::empty("person")}}));
    assert_eq!(
        engine
            .check_all(&saved.decisions, &capture(empty), &[])
            .unwrap()
            .disposition,
        CheckDisposition::RepairRequired
    );
}

#[test]
fn property_only_mode_requires_explicit_nonempty_verified_predicates() {
    assert_eq!(
        IntentionBinding::default(),
        IntentionBinding::ObservedOutcome
    );
    let dir = tempfile::tempdir().unwrap();
    let (store, engine, p) = setup(&dir);
    let mut c = choice("empty-rule", DecisionOutcome::KeepCurrent);
    c.binding = IntentionBinding::PropertiesOnly;
    let before = store.load().unwrap();
    assert!(engine
        .prepare_choice(&store, &p, c, vec![scene(&p)], "invalid-empty")
        .is_err());
    let mut c = choice("false-rule", DecisionOutcome::KeepCurrent);
    c.binding = IntentionBinding::PropertiesOnly;
    c.obligations = vec![AcceptedProperty {
        id: "false-count".into(),
        description: "An unverified count".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 999 },
            },
        },
    }];
    assert!(engine
        .prepare_choice(&store, &p, c, vec![scene(&p)], "invalid-false")
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let ready = engine
        .prepare_choice(
            &store,
            &p,
            choice("normal", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "accept",
        )
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    let archive = IntentArchive::new(store.clone());
    let mut bundle = serde_json::to_value(archive.export_for(&saved).unwrap()).unwrap();
    let mut forged = saved.clone();
    let object = bundle["objects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|o| {
            o["digest"] == serde_json::to_value(&saved.decisions.decisions[0].witness).unwrap()
        })
        .unwrap();
    let bytes: Vec<u8> = serde_json::from_value(object["bytes"].clone()).unwrap();
    let mut content: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    content["content"]["scenes"][0]["bind_outcome"] = json!(false);
    let bytes = canonical_bytes(&content).unwrap();
    let digest = canonical_digest(IdentityDomain::Evidence, &content).unwrap();
    object["bytes"] = serde_json::to_value(bytes).unwrap();
    object["digest"] = serde_json::to_value(&digest).unwrap();
    forged.decisions.decisions[0].witness = digest;
    bundle["snapshot"] =
        serde_json::to_value(canonical_digest(IdentityDomain::Data, &forged).unwrap()).unwrap();
    let forged_bundle: IntentionBundle = serde_json::from_value(bundle).unwrap();
    assert!(validate_bundle(&forged, &forged_bundle).is_err());
}

#[test]
fn another_archives_objects_cannot_activate_dangling_store_references() {
    let dir = tempfile::tempdir().unwrap();
    let (_source, engine, p) = setup(&dir);
    let destination = ProductStore::create(dir.path().join("different-tool"), &p, 20000).unwrap();
    let before = destination.load().unwrap();
    assert!(engine
        .prepare_choice(
            &destination,
            &p,
            choice("wrong-destination", DecisionOutcome::KeepCurrent),
            vec![scene(&p)],
            "adopt-wrong"
        )
        .is_err());
    assert_eq!(destination.load().unwrap(), before);
}

#[test]
fn mixed_scenes_require_only_their_applicable_operations() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = organizer();
    let mut second_action = source["actions"][0].clone();
    second_action["id"] = json!("add_guest");
    source["actions"]
        .as_array_mut()
        .unwrap()
        .push(second_action);
    let p = capture(source);
    let store = ProductStore::create(dir.path().join("tool"), &p, 20000).unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenes = [("north", "south"), ("south", "north")]
        .into_iter()
        .map(|(first, second)| {
            accept_scene(
                &LocalRuntime::default(),
                &p,
                &scenario(
                    &p,
                    vec![
                        invoke(
                            "add_person",
                            args(&[("name", string("Person")), ("area", string(first))]),
                        ),
                        invoke(
                            "add_guest",
                            args(&[("name", string("Guest")), ("area", string(second))]),
                        ),
                        SemanticInput::Observe {
                            point: "done".into(),
                        },
                    ],
                ),
                Disclosure::Synthetic,
                RuntimeLimits::default(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut choice = choice("north-actions", DecisionOutcome::KeepCurrent);
    choice.scope.operations = ["add_person".into(), "add_guest".into()].into();
    choice.scope.conditions = args(&[("area", string("north"))]);
    assert!(engine
        .prepare_choice(
            &store,
            &p,
            choice.clone(),
            vec![scenes[0].clone()],
            "incomplete"
        )
        .is_err());
    let ready = engine
        .prepare_choice(&store, &p, choice, scenes, "accept-mixed")
        .unwrap();
    let saved = engine.adopt(&store, &ready).unwrap();
    assert_eq!(
        engine.check_current(&saved).unwrap().disposition,
        CheckDisposition::Ready
    );
    let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
    let fresh = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(reopened.clone()),
    );
    assert_eq!(
        fresh
            .check_current(&reopened.load().unwrap())
            .unwrap()
            .disposition,
        CheckDisposition::Ready
    );
}
