//! Checked authored-result lowering and paired prospective rehearsal, using fictional data only.
#[path = "fixtures/product_prepared_pairs/mod.rs"]
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
use product_store::{scope::*, ProductStore};
use std::sync::{atomic::AtomicBool, Arc};

#[test]
fn authored_result_resolves_to_host_lowered_execution_without_rewriting_provenance() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let primary = prepare(&store, &design(1, fixture_producer(), false), "primary");
    let authored = design(2, fixture_producer(), false);
    let alternative = prepare(&store, &authored, "alternative");
    let (request, result, mut policy) = discovery(&store, &primary, &authored);
    let original = result.clone();
    let checked = PreparedDiscoveryCandidate::from_result(
        &store.load().unwrap(),
        &request,
        &result,
        "alternative",
        alternative.clone(),
        vec![],
    )
    .unwrap();
    assert_eq!(checked.authored(), &authored);
    assert_eq!(checked.target(), alternative.target());
    assert_ne!(checked.target().binding.producer, result.producer);
    policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(checked)
        .unwrap();
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(result, original);
    assert_eq!(
        report.questions.len(),
        1,
        "{:?} {:?}",
        report.log,
        report.unverified
    );
    assert!(report
        .coverage
        .iter()
        .any(|coverage| coverage.contains("No current-side experience is claimed")));
    assert!(!report
        .runs
        .iter()
        .any(|run| run.binding.artifact == request.sources[0].artifact
            && run.state == EvidenceState::Observed));
    assert_eq!(report.lowerings.len(), 1);
    assert_eq!(report.lowerings[0].result(), &result);
    assert_eq!(report.lowerings[0].authored(), &authored);
    assert_eq!(report.lowerings[0].target(), alternative.target());
    let witness = report.questions[0].witnesses[0].witness();
    assert_eq!(
        witness.before.binding.artifact,
        alternative.target().artifact
    );
    assert_eq!(witness.after.binding.artifact, primary.target().artifact);
    assert_eq!(
        witness.before.binding.input_digest,
        witness.after.binding.input_digest
    );
    assert_eq!(witness.before.state, EvidenceState::Observed);
    assert_eq!(witness.after.state, EvidenceState::Observed);
    assert!(witness.minimization.as_ref().unwrap().complete);
    assert_eq!(store.load().unwrap().scope.rehearsals.len(), 0);
}

#[test]
fn prepared_result_rejects_wrong_request_result_capture_target_and_stale_basis() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let primary = prepare(&store, &design(1, fixture_producer(), false), "primary");
    let authored = design(2, fixture_producer(), false);
    let prepared = prepare(&store, &authored, "alternative");
    let current = store.load().unwrap();
    let (request, result, mut policy) = discovery(&store, &primary, &authored);
    let build = |r: &DevelopmentRequest, out: &DevelopmentResult, p: PreparedScopedChange| {
        PreparedDiscoveryCandidate::from_result(&current, r, out, "alternative", p, vec![])
    };
    let mut wrong = request.clone();
    wrong.request.push_str(" changed");
    assert!(build(&wrong, &result, prepared.clone()).is_err());
    let mut wrong = result.clone();
    wrong.producer = Producer::Fixture {
        name: "another invocation".into(),
    };
    assert!(build(&request, &wrong, prepared.clone()).is_err());
    let mut wrong = result.clone();
    wrong.response.candidates[1].source_json =
        String::from_utf8(primary.target().source_bytes.clone()).unwrap();
    assert!(build(&request, &wrong, prepared.clone()).is_err());
    assert!(build(&request, &result, primary.clone()).is_err());
    assert!(PreparedDiscoveryCandidate::from_result(
        &current,
        &request,
        &result,
        "missing",
        prepared.clone(),
        vec![]
    )
    .is_err());
    let checked = build(&request, &result, prepared.clone()).unwrap();
    policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(checked.clone())
        .unwrap();
    let mut replaced = result.clone();
    replaced
        .response
        .unsupported
        .push("different response".into());
    assert!(discover(
        &request,
        &replaced,
        &policy,
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    add(&store, "later", "New real work");
    assert!(PreparedDiscoveryCandidate::from_result(
        &store.load().unwrap(),
        &request,
        &result,
        "alternative",
        prepared,
        vec![]
    )
    .is_err());
    assert!(policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(checked)
        .is_err());
    let stale = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false)));
    assert!(stale.is_err() || stale.unwrap().questions.is_empty());
}

#[test]
fn prepared_alternative_without_exact_proof_or_independent_feature_requirement_is_not_a_choice() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let primary = prepare(&store, &design(1, fixture_producer(), false), "primary");
    let authored = design(2, fixture_producer(), false);
    let (request, result, mut policy) = discovery(&store, &primary, &authored);
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    let proof = PreparedDiscoveryCandidate::from_result(
        &store.load().unwrap(),
        &request,
        &result,
        "alternative",
        prepare(&store, &authored, "alternative"),
        vec![],
    )
    .unwrap();
    policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(proof)
        .unwrap();
    policy.requirements.clear();
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    assert!(report
        .unverified
        .iter()
        .any(|message| message.contains("independently checked")));
}

#[test]
fn two_prospective_evolutions_retain_all_four_outcomes_without_live_activation() {
    for outcome in [
        DecisionOutcome::EitherAcceptable,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::Deferred,
    ] {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = managed_store(&path);
        let before = store.load().unwrap();
        let a = prepare(&store, &design(1, fixture_producer(), true), "design-a");
        let b = prepare(&store, &design(2, fixture_producer(), false), "design-b");
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let scenario = scene(&before, a.target());
        let scenes = engine
            .accept_paired_scoped_scenes(&store, &a, &b, &scenario, Disclosure::Synthetic)
            .unwrap();
        assert_eq!(scenes[0].scenario(), scenes[1].scenario());
        assert_eq!(
            scenes[0].observations()[0].values["planned"],
            DataValue::Integer { value: 1 }
        );
        assert_eq!(
            scenes[1].observations()[0].values["planned"],
            DataValue::Integer { value: 2 }
        );
        let change = engine
            .prepare_paired_rehearsed_choice(
                &store,
                a.clone(),
                b.clone(),
                choice(outcome.clone()),
                scenes.to_vec(),
                &[],
                "record-pair",
            )
            .unwrap();
        let retained = engine.adopt(&store, &change).unwrap();
        assert_eq!(engine.adopt(&store, &change).unwrap(), retained);
        assert_eq!(retained.active_revision, before.active_revision);
        assert_eq!(retained.data, before.data);
        assert_eq!(retained.session, before.session);
        assert_eq!(retained.clock_day, before.clock_day);
        assert_eq!(retained.artifacts, before.artifacts);
        assert_eq!(retained.scope.layers, before.scope.layers);
        assert_eq!(retained.scope.compositions, before.scope.compositions);
        assert_eq!(retained.scope.initializations, before.scope.initializations);
        assert_eq!(retained.scope.adoptions, before.scope.adoptions);
        assert_eq!(retained.scope.rehearsals.len(), 2);
        assert_eq!(retained.decisions.decisions[0].outcome, outcome);
        assert_eq!(
            retained.decisions.decisions[0].status,
            DecisionStatus::Pending
        );
        for proof in retained.scope.rehearsals.values() {
            assert_eq!(proof.recorded_by, "record-pair");
            assert_eq!(
                proof.witnesses["pending-designs"],
                retained.decisions.decisions[0].witness
            );
        }
        assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), retained);
        let bytes = product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .to_bytes()
            .unwrap();
        let recovered = product_backup::VerifiedBackup::from_bytes(&bytes)
            .unwrap()
            .recover_new(&dir.path().join("recovered"))
            .unwrap();
        assert_eq!(recovered.load().unwrap(), retained);
        let reopened_engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(recovered.clone()),
        );
        let retained_scenes = reopened_engine.discovery_scenes(&retained).unwrap();
        assert_eq!(retained_scenes.len(), 2);
        assert_eq!(
            retained_scenes[0].original().binding.artifact,
            a.target().artifact
        );
        assert_eq!(
            retained_scenes[1].original().binding.artifact,
            b.target().artifact
        );
        let late = add(&recovered, "later-work", "Work after deciding to wait");
        let updated = recovered.load().unwrap();
        assert!(ScopedExecutionContext::rehearsed_pair(&updated, &a, &b).is_err());
        let fresh = prepare(&recovered, a.candidate(), "resolve-pair");
        let engine = DecisionEngine::new(
            LocalRuntime::default(),
            IntentArchive::new(recovered.clone()),
        );
        let accepted = engine
            .accept_scoped_scene(
                &recovered,
                &fresh,
                &full_scene(&updated, fresh.target()),
                Disclosure::Synthetic,
            )
            .unwrap();
        let mut accepted_choice = choice(DecisionOutcome::Accept {
            artifact: fresh.target().artifact.program_digest.clone(),
        });
        accepted_choice.id = "selected-design".into();
        accepted_choice.scope = fresh.scope().clone();
        let resolution = engine
            .prepare_scoped_resolution(
                &recovered,
                fresh,
                accepted_choice,
                vec![accepted],
                &["pending-designs".into()],
                "resolve-pair",
            )
            .unwrap();
        let resolved = engine.adopt(&recovered, &resolution).unwrap();
        assert_eq!(resolved.data.records, updated.data.records);
        assert_eq!(resolved.data.events, updated.data.events);
        assert!(resolved.data.records.iter().any(|row| row.id == late.id));
        assert!(matches!(
            resolved.decisions.decisions[0].status,
            DecisionStatus::Superseded { .. }
        ));
        product_backup::VerifiedBackup::capture(&recovered).unwrap();
    }
}

#[test]
fn paired_admission_rejects_fresh_layers_stale_inputs_mixed_sources_and_forged_observations() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let before = store.load().unwrap();
    let a = prepare(&store, &design(1, fixture_producer(), false), "a");
    let b = prepare(&store, &design(2, fixture_producer(), false), "b");
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenario = scene(&before, a.target());
    assert!(ScopedExecutionContext::rehearsed_pair(&before, &a, &a).is_err());
    let new_layer = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "fresh-layer",
        )
        .unwrap();
    assert!(ScopedExecutionContext::rehearsed_pair(&before, &a, &new_layer).is_err());
    let mut wrong_seed = scenario.clone();
    wrong_seed.seed.records[0]
        .values
        .insert("name".into(), text("Same ID but invented business data"));
    assert!(engine
        .accept_paired_scoped_scenes(&store, &a, &b, &wrong_seed, Disclosure::Synthetic)
        .is_err());
    let good = engine
        .accept_paired_scoped_scenes(&store, &a, &b, &scenario, Disclosure::Synthetic)
        .unwrap();
    let attempt = |scenes: Vec<AcceptedScene>, outcome| {
        engine.prepare_paired_rehearsed_choice(
            &store,
            a.clone(),
            b.clone(),
            choice(outcome),
            scenes,
            &[],
            "record",
        )
    };
    assert!(attempt(vec![good[0].clone()], DecisionOutcome::Deferred).is_err());
    assert!(attempt(
        vec![good[0].clone(), good[0].clone()],
        DecisionOutcome::Deferred
    )
    .is_err());
    assert!(attempt(
        good.to_vec(),
        DecisionOutcome::Accept {
            artifact: a.target().artifact.program_digest.clone()
        }
    )
    .is_err());
    let mut changed = scenario.clone();
    changed.random_seed += 1;
    let other = engine
        .accept_paired_scoped_scenes(&store, &a, &b, &changed, Disclosure::Synthetic)
        .unwrap();
    assert!(attempt(
        vec![good[0].clone(), other[1].clone()],
        DecisionOutcome::Deferred
    )
    .is_err());
    let mut forged = serde_json::to_value(&good[1]).unwrap();
    forged["evidence"]["observations"][0]["values"]["planned"] =
        serde_json::json!({"kind":"integer","value":999});
    let forged: AcceptedScene = serde_json::from_value(forged).unwrap();
    assert!(attempt(vec![good[0].clone(), forged], DecisionOutcome::Deferred).is_err());
    assert_eq!(store.load().unwrap(), before);
    add(&store, "later", "Intervening work");
    let fresh = prepare(&store, a.candidate(), "fresh-a");
    assert!(ScopedExecutionContext::rehearsed_pair(&store.load().unwrap(), &fresh, &b).is_err());
}

#[test]
fn pair_recording_refuses_missing_replaced_proofs_and_mixed_recording_births() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    for index in 0..2 {
        let current = store.load().unwrap();
        let a = prepare(
            &store,
            &design(index * 2 + 1, fixture_producer(), false),
            &format!("a-{index}"),
        );
        let b = prepare(
            &store,
            &design(index * 2 + 2, fixture_producer(), false),
            &format!("b-{index}"),
        );
        let scenes = engine
            .accept_paired_scoped_scenes(
                &store,
                &a,
                &b,
                &scene(&current, a.target()),
                Disclosure::Synthetic,
            )
            .unwrap();
        let mut choice = choice(DecisionOutcome::Deferred);
        choice.id = format!("pending-{index}");
        let change = engine
            .prepare_paired_rehearsed_choice(
                &store,
                a,
                b,
                choice,
                scenes.to_vec(),
                &[],
                &format!("record-{index}"),
            )
            .unwrap();
        engine.adopt(&store, &change).unwrap();
    }
    let healthy = store.load().unwrap();
    let ids: Vec<_> = healthy.scope.rehearsals.keys().cloned().collect();
    let mut missing = healthy.clone();
    missing.scope.rehearsals.remove(&ids[0]);
    missing
        .programs
        .retain(|source| canonical_digest(IdentityDomain::Source, source).unwrap() != ids[0]);
    assert!(missing.validate().is_err());
    let mut absent = healthy.clone();
    absent.scope.rehearsals.clear();
    absent
        .programs
        .retain(|source| !ids.contains(&canonical_digest(IdentityDomain::Source, source).unwrap()));
    assert!(absent.validate().is_err());
    let mut changed = healthy.clone();
    changed.scope.rehearsals.get_mut(&ids[0]).unwrap().seed =
        canonical_digest(IdentityDomain::Data, &"substituted input").unwrap();
    assert!(changed.validate().is_err());
    let mut mixed = healthy.clone();
    let newer = mixed
        .scope
        .rehearsals
        .values_mut()
        .find(|proof| proof.recorded_by == "record-1")
        .unwrap();
    newer.recorded_by = "record-0".into();
    assert!(mixed.validate().is_err());
    let mut wrong_birth = healthy.clone();
    let prior = healthy
        .scope
        .rehearsals
        .values()
        .find(|proof| proof.recorded_by == "record-0")
        .unwrap()
        .clone();
    let later = wrong_birth
        .scope
        .rehearsals
        .values_mut()
        .find(|proof| proof.recorded_by == "record-1")
        .unwrap();
    later.witnesses = prior.witnesses;
    later.recorded_revision = prior.recorded_revision;
    assert!(wrong_birth.validate().is_err());
    assert_eq!(store.load().unwrap(), healthy);
}

#[test]
fn retained_new_action_pair_reopens_for_fresh_discovery_without_repeated_question() {
    for outcome in [DecisionOutcome::EitherAcceptable, DecisionOutcome::Deferred] {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = managed_store(&path);
        let current = store.load().unwrap();
        let first = design(1, fixture_producer(), false);
        let second = design(2, fixture_producer(), false);
        let a = prepare(&store, &first, "a");
        let b = prepare(&store, &second, "b");
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let scenes = engine
            .accept_paired_scoped_scenes(
                &store,
                &a,
                &b,
                &scene(&current, a.target()),
                Disclosure::Synthetic,
            )
            .unwrap();
        let change = engine
            .prepare_paired_rehearsed_choice(
                &store,
                a,
                b,
                choice(outcome),
                scenes.to_vec(),
                &[],
                "record",
            )
            .unwrap();
        engine.adopt(&store, &change).unwrap();
        let reopened = ProductStore::open(&path).unwrap();
        let a = prepare(&reopened, &first, "fresh-a");
        let b = prepare(&reopened, &second, "fresh-b");
        let (request, result, mut policy) = discovery(&reopened, &a, &second);
        assert_eq!(request.accepted_scenes.len(), 2);
        let checked = PreparedDiscoveryCandidate::from_result(
            &reopened.load().unwrap(),
            &request,
            &result,
            "alternative",
            b,
            vec![],
        )
        .unwrap();
        policy
            .retained_history
            .as_mut()
            .unwrap()
            .map_prepared_result(checked)
            .unwrap();
        let report =
            discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(report.questions.is_empty(), "{:?}", report.questions);
        assert!(report.unverified.is_empty(), "{:?}", report.unverified);
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

#[test]
fn prepared_lowering_does_not_turn_an_independent_requirement_violation_into_a_preference() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let primary = prepare(&store, &design(1, fixture_producer(), false), "primary");
    let authored = design(0, fixture_producer(), false);
    let (request, result, mut policy) = discovery(&store, &primary, &authored);
    let proof = PreparedDiscoveryCandidate::from_result(
        &store.load().unwrap(),
        &request,
        &result,
        "alternative",
        prepare(&store, &authored, "invalid-alternative"),
        vec![],
    )
    .unwrap();
    policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(proof)
        .unwrap();
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    assert!(report
        .unverified
        .iter()
        .any(|message| message.contains("violated")));
}

#[test]
fn unexpected_baseline_guard_failure_and_exhaustion_remain_unverified() {
    let dir = tempdir();
    let store = managed_store(&dir.path().join("tool"));
    let mut blocked = serde_json::to_value(design(2, fixture_producer(), false).program).unwrap();
    blocked["actions"][7]["guards"] = serde_json::json!([boolean(false)]);
    let blocked = capture(blocked);
    let prepared = prepare(&store, &blocked, "existing-guard");
    store
        .adopt_scoped(store.load().unwrap().revision, &prepared)
        .unwrap();
    let primary = prepare(&store, &design(1, fixture_producer(), false), "primary");
    let alternative = design(3, fixture_producer(), false);
    let (request, result, mut policy) = discovery(&store, &primary, &alternative);
    let proof = PreparedDiscoveryCandidate::from_result(
        &store.load().unwrap(),
        &request,
        &result,
        "alternative",
        prepare(&store, &alternative, "alternative"),
        vec![],
    )
    .unwrap();
    policy
        .retained_history
        .as_mut()
        .unwrap()
        .map_prepared_result(proof)
        .unwrap();
    let report = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.questions.is_empty());
    assert!(!report.unverified.is_empty());
    assert!(report
        .runs
        .iter()
        .any(|run| run.binding.artifact == request.sources[0].artifact
            && run.state == EvidenceState::Failed));
    policy.search.runtime.fuel = 1;
    let exhausted = discover(&request, &result, &policy, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(exhausted.questions.is_empty());
    assert!(!exhausted.unverified.is_empty());
    let cancelled = discover(&request, &result, &policy, Arc::new(AtomicBool::new(true)));
    assert!(cancelled.is_err() || cancelled.unwrap().questions.is_empty());
}

#[test]
fn legacy_single_rehearsal_preserves_reversed_recorded_decision_order() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = managed_store(&path);
    let current = store.load().unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["observables"][0]["value"] = serde_json::to_value(int(1)).unwrap();
    let prospective = prepare(&store, &capture(raw), "prospective");
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let scenario = ScenarioSpec {
        version: 1,
        id: "legacy-many".into(),
        label: "One current and one prospective outcome".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 0,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let scenes = vec![
        engine
            .accept_prepared_current_scene(&store, &prospective, &scenario, Disclosure::Synthetic)
            .unwrap(),
        engine
            .accept_scoped_scene(&store, &prospective, &scenario, Disclosure::Synthetic)
            .unwrap(),
    ];
    let object = serde_json::json!({"version":1,"content":{"kind":"scenes","scenes":scenes}});
    let witness = store
        .stage_extension(&canonical_bytes(&object).unwrap())
        .unwrap();
    let mut next = current.decisions.clone();
    next.revision += 1;
    for id in ["a", "b"] {
        let mut scope = choice(DecisionOutcome::Deferred).scope;
        scope.operations = ["export".into()].into_iter().collect();
        next.decisions.push(ScopedDecision {
            id: id.into(),
            revision: 1,
            request: "Keep this unresolved historical choice".into(),
            rationale: None,
            scope,
            outcome: DecisionOutcome::Deferred,
            status: DecisionStatus::Pending,
            obligations: vec![],
            scenarios: scenes
                .iter()
                .map(|scene| scene.scenario().identity().unwrap())
                .collect(),
            witness: witness.clone(),
            supersedes: vec![],
        });
    }
    let adoption = store
        .prepare_switch(current.program().unwrap(), "record-reversed")
        .unwrap();
    let reversed = vec!["b".into(), "a".into()];
    let retained = store
        .adopt_rehearsal_verified(
            current.revision,
            &adoption,
            current.program().unwrap(),
            &next,
            &prospective,
            &reversed,
            &Default::default(),
            |_, _, _, _| Ok(()),
        )
        .unwrap();
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), retained);
    assert_eq!(
        store
            .adopt_rehearsal_verified(
                current.revision,
                &adoption,
                current.program().unwrap(),
                &next,
                &prospective,
                &reversed,
                &Default::default(),
                |_, _, _, _| Ok(())
            )
            .unwrap(),
        retained
    );
    let bytes = product_backup::VerifiedBackup::capture(&store)
        .unwrap()
        .to_bytes()
        .unwrap();
    product_backup::VerifiedBackup::from_bytes(&bytes)
        .unwrap()
        .recover_new(&dir.path().join("recovered"))
        .unwrap();
}
