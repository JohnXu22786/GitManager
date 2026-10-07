#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_store::{scope::*, FaultPoint, ProductStore};
#[test]
fn withdrawal_uses_current_data_and_keeps_later_completed_facts() {
    let dir = tempdir();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let old = add(&store, "old", "Existing");
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let new = add(&store, "new", "Later legitimate work");
    action(&store, "start-wait", "wait", &new);
    tick(&store, "elapsed", 20003);
    action(&store, "finish", "complete", &new);
    action(&store, "edit-existing", "wait", &old);
    let facts = apply(&store, "document", invoke("export", &[]));
    let withdrawal = store
        .prepare_scoped_withdrawal(&[layer], "withdraw")
        .unwrap();
    let withdrawn = store.adopt_scoped(facts.revision, &withdrawal).unwrap();
    assert_eq!(withdrawn.data.records, facts.data.records);
    assert_eq!(withdrawn.data.events, facts.data.events);
    assert_eq!(withdrawn.artifacts, facts.artifacts);
    let reopened = ProductStore::open(&path).unwrap();
    let later = add(&reopened, "after-withdrawal", "Continued");
    action(&reopened, "later-wait", "wait", &later);
    tick(&reopened, "later-days", 20005);
    let calculated = action(&reopened, "later-calculate", "calculate", &later);
    assert_eq!(
        row(&calculated, &later).values["production"],
        DataValue::Integer { value: 2 }
    );
    assert_eq!(
        row(&calculated, &new).values["production"],
        DataValue::Integer { value: 0 }
    );
}
#[test]
fn scoped_adoption_retry_is_atomic_and_initialization_is_not_repeated() {
    for fault in [
        FaultPoint::AfterObject,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let dir = tempdir();
        let path = dir.path().join("tool");
        let store = ProductStore::create(&path, &program(false), 20000).unwrap();
        add(&store, "old", "Existing");
        let before = store.load().unwrap();
        let prepared = store
            .prepare_scoped_change(
                &program(true),
                &request(&before, ScopePopulation::FutureWork),
                "scope",
            )
            .unwrap();
        assert!(store
            .adopt_scoped_with_fault(before.revision, &prepared, fault)
            .is_err());
        let reopened = ProductStore::open(&path).unwrap();
        let result = reopened.adopt_scoped(before.revision, &prepared).unwrap();
        assert_eq!(result.scope.layers.len(), 1);
        assert_eq!(result.scope.initializations.len(), 1);
        assert_eq!(result.data.events, before.data.events);
        assert_eq!(
            reopened.adopt_scoped(before.revision, &prepared).unwrap(),
            result
        );
    }
}

#[test]
fn mapped_new_expression_and_structural_evolution_keep_history_usable() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let scoped = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let layer = scoped.layer_id().unwrap().unwrap();
    store.adopt_scoped(before.revision, &scoped).unwrap();
    let job = add(&store, "job", "Later work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let mut candidate = serde_json::to_value(program(true).program).unwrap();
    candidate["entities"].as_array_mut().unwrap().push(serde_json::json!({"id":"material","label":"Materials","fields":[{"id":"name","label":"Material","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}));
    candidate["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"log_material","label":"Log material","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"material","bind":"material","values":{"name":var("name")}}],"ensures":[]}));
    // A different expression tree for the same real waiting-interval behavior.
    fn alternate(binding: &str) -> Expr {
        let Expr::Subtract { left, right } = elapsed(binding, true) else {
            panic!()
        };
        let Expr::Add {
            left: waited,
            right: current,
        } = *right
        else {
            panic!()
        };
        Expr::Subtract {
            left: Box::new(Expr::Subtract {
                left,
                right: waited,
            }),
            right: current,
        }
    }
    candidate["actions"][3]["steps"][0]["values"]["production"] =
        serde_json::to_value(alternate("row")).unwrap();
    candidate["actions"][4]["steps"][0]["values"]["production"] =
        serde_json::to_value(alternate("row")).unwrap();
    candidate["actions"][6]["steps"][0]["columns"]["production"] =
        serde_json::to_value(alternate("row")).unwrap();
    candidate["views"][0]["kind"]["columns"][1]["value"] =
        serde_json::to_value(alternate("row")).unwrap();
    let candidate = capture(candidate);
    let current = store.load().unwrap();
    let target = canonical_digest(IdentityDomain::Source, &candidate).unwrap();
    let mappings: Vec<_> = current.scope.layers[&layer]
        .patches
        .iter()
        .enumerate()
        .map(|(patch, p)| ScopeSlotMapping {
            layer: layer.clone(),
            patch,
            from_source: current.active_revision.clone(),
            from: p.request.destination.clone(),
            to_source: target.clone(),
            to: p.request.destination.clone(),
            subject: p.request.subject.clone(),
        })
        .collect();
    assert!(store
        .prepare_managed_evolution(&candidate, &[], "unmapped")
        .is_err());
    let evolution = store
        .prepare_managed_evolution(&candidate, &mappings, "evolve")
        .unwrap();
    let evolved = store.adopt_scoped(current.revision, &evolution).unwrap();
    assert_eq!(evolved.data.records, current.data.records);
    let calculated = action(&store, "calc", "calculate", &job);
    assert_eq!(
        row(&calculated, &job).values["production"],
        DataValue::Integer { value: 0 }
    );
    action(&store, "complete", "complete", &job);
    let material = apply(
        &store,
        "material",
        invoke("log_material", &[("name", text("Oak"))]),
    );
    let withdrawal = store
        .prepare_scoped_withdrawal(&[layer], "withdraw")
        .unwrap();
    let restored = store.adopt_scoped(material.revision, &withdrawal).unwrap();
    assert_eq!(restored.data.records, material.data.records);
    let continued = apply(
        &store,
        "material-after",
        invoke("log_material", &[("name", text("Pine"))]),
    );
    assert_eq!(
        continued
            .data
            .records
            .iter()
            .filter(|r| r.entity == "material")
            .count(),
        2
    );
}

#[test]
fn durable_slot_retargeting_cannot_unprotect_completed_original_fields() {
    fn with_copy(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        raw["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"copied","label":"Other result","value_type":{"kind":"integer"}}));
        raw["actions"][0]["steps"][0]["values"]["copied"] = serde_json::to_value(int(0)).unwrap();
        capture(raw)
    }
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &with_copy(false), 20000).unwrap();
    let before = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &with_copy(true),
            &request(&before, ScopePopulation::All),
            "first",
        )
        .unwrap();
    let layer = first.layer_id().unwrap().unwrap();
    store.adopt_scoped(before.revision, &first).unwrap();
    let job = add(&store, "job", "Completed");
    action(&store, "complete", "complete", &job);
    let current = store.load().unwrap();
    let mut raw = serde_json::to_value(with_copy(true).program).unwrap();
    for action in [3, 4] {
        let values = raw["actions"][action]["steps"][0]["values"]
            .as_object_mut()
            .unwrap();
        let value = values.remove("production").unwrap();
        values.insert("copied".into(), value);
    }
    raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"reset_original","label":"Reset original","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"production":int(99)}}],"ensures":[]}));
    let candidate = capture(raw);
    let source = canonical_digest(IdentityDomain::Source, &candidate).unwrap();
    let mappings: Vec<_> = current.scope.layers[&layer]
        .patches
        .iter()
        .enumerate()
        .map(|(patch, p)| {
            let mut to = p.request.destination.clone();
            if let EffectDestination::Update { field, .. } = &mut to {
                *field = "copied".into();
            }
            ScopeSlotMapping {
                layer: layer.clone(),
                patch,
                from_source: current.active_revision.clone(),
                from: p.request.destination.clone(),
                to_source: source.clone(),
                to,
                subject: p.request.subject.clone(),
            }
        })
        .collect();
    assert!(store
        .prepare_managed_evolution(&candidate, &mappings, "retarget")
        .is_err());
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn corrupted_earlier_composition_references_fail_before_recovery_activation() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "first",
        )
        .unwrap();
    let first = store.adopt_scoped(before.revision, &first).unwrap();
    add(&store, "job", "Continuing work");
    let before = store.load().unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["actions"][6]["steps"][0]["columns"]["reminder"] =
        serde_json::to_value(boolean(false)).unwrap();
    raw["views"][0]["kind"]["columns"][3]["value"] = serde_json::to_value(boolean(false)).unwrap();
    let req = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![LifecycleBinding {
            entity: "job".into(),
            completed: field("record", "done"),
            source: before.active_revision.clone(),
        }],
        patches: vec![
            EffectPatchRequest {
                destination: EffectDestination::EmitColumn {
                    action: "export".into(),
                    path: vec![0],
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
            EffectPatchRequest {
                destination: EffectDestination::ViewColumn {
                    view: "work".into(),
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
        ],
    };
    let second = store
        .prepare_scoped_change(&capture(raw), &req, "second")
        .unwrap();
    let healthy = store.adopt_scoped(before.revision, &second).unwrap();
    let mut corrupted = healthy.clone();
    let absent = canonical_digest(IdentityDomain::Adoption, &"absent-layer-proof").unwrap();
    assert!(!corrupted.scope.layers.contains_key(&absent));
    corrupted
        .scope
        .compositions
        .get_mut(&first.active_revision)
        .unwrap()
        .layers[0] = absent;
    assert!(corrupted.validate().is_err());
    let destination = dir.path().join("rejected-recovery");
    assert!(ProductStore::create_recovered_with(&destination, &corrupted, |_| Ok(())).is_err());
    assert!(!destination.exists());
    assert_eq!(store.load().unwrap(), healthy);
    add(&store, "later", "Still usable after rejected recovery");
}

#[test]
fn unmatched_and_ambiguous_scope_receipts_cannot_enter_recovery() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    add(&store, "later", "Later legitimate work");
    let withdrawal = store
        .prepare_scoped_withdrawal(&[layer], "withdraw")
        .unwrap();
    let healthy = store
        .adopt_scoped(store.load().unwrap().revision, &withdrawal)
        .unwrap();
    assert_eq!(healthy.scope.adoptions.len(), 2);
    let unknown = canonical_digest(IdentityDomain::Adoption, &"unknown-composition").unwrap();
    let mut extra = healthy.clone();
    let mut forged = extra.scope.adoptions[0].clone();
    forged.composition = unknown.clone();
    forged.decisions = vec!["unrelated-intention".into()];
    // It shadows the real receipt in a revision-only consumer unless the
    // authoritative inventory rejects every unmatched entry first.
    extra.scope.adoptions.insert(0, forged);
    let mut replaced = healthy.clone();
    replaced.scope.adoptions[0].composition = unknown;
    let mut duplicate_revision = healthy.clone();
    duplicate_revision.scope.adoptions[1].revision = duplicate_revision.scope.adoptions[0].revision;
    let mut duplicate_composition = healthy.clone();
    duplicate_composition.scope.adoptions[1] = duplicate_composition.scope.adoptions[0].clone();
    for (index, corrupted) in [extra, replaced, duplicate_revision, duplicate_composition]
        .iter()
        .enumerate()
    {
        assert!(corrupted.validate().is_err());
        let destination = dir.path().join(format!("rejected-{index}"));
        let activated = std::cell::Cell::new(false);
        assert!(
            ProductStore::create_recovered_with(&destination, corrupted, |_| {
                activated.set(true);
                Ok(())
            })
            .is_err()
        );
        assert!(!activated.get());
        assert!(!destination.exists());
        assert_eq!(store.load().unwrap(), healthy);
    }
    let restored =
        ProductStore::create_recovered(dir.path().join("valid-recovery"), &healthy).unwrap();
    assert_eq!(restored.load().unwrap(), healthy);
    add(
        &restored,
        "continued",
        "Continued after valid receipt recovery",
    );
    assert_eq!(store.load().unwrap(), healthy);
}

#[test]
fn rehashed_compositions_require_exact_predecessor_business_and_basis() {
    use product_runtime::LocalRuntime;
    use product_store::ProjectSnapshot;
    fn reseal(snapshot: &mut ProjectSnapshot, mut manifest: CompositionManifest) {
        let old = manifest.output.clone();
        let target = compile_test_manifest(snapshot, &manifest).unwrap();
        let output = canonical_digest(IdentityDomain::Source, &target).unwrap();
        manifest.output = output.clone();
        snapshot.data = product_runtime::merged_data(&target, &snapshot.data).unwrap();
        let runtime = LocalRuntime::default();
        let compatibility = runtime
            .compatibility_at(&target, &manifest.basis.data, manifest.basis.day)
            .unwrap();
        let initialized = runtime
            .compatibility_at(&target, &snapshot.data, manifest.basis.day)
            .unwrap();
        let adoption = snapshot
            .adoptions
            .iter_mut()
            .find(|a| a.plan.id == manifest.operation)
            .unwrap();
        adoption.active = output.clone();
        adoption.plan.target = target.artifact.clone();
        adoption.plan.compatibility = compatibility;
        let receipt = snapshot
            .scope
            .adoptions
            .iter_mut()
            .find(|r| r.revision == adoption.revision)
            .unwrap();
        receipt.plan = adoption.plan.identity().unwrap();
        receipt.composition = canonical_digest(IdentityDomain::Adoption, &manifest).unwrap();
        receipt.initialized_compatibility = initialized;
        snapshot
            .programs
            .retain(|p| canonical_digest(IdentityDomain::Source, p).unwrap() != old);
        snapshot.programs.push(target);
        snapshot.scope.compositions.remove(&old);
        snapshot.scope.compositions.insert(output.clone(), manifest);
        snapshot.active_revision = output;
    }
    fn reminder(snapshot: &ProjectSnapshot, record: &Record) -> DataValue {
        let runtime = LocalRuntime::default();
        let run = runtime
            .start(
                snapshot.program().unwrap(),
                &snapshot.data,
                &snapshot.session,
                snapshot.clock_day,
                0,
                RuntimeLimits::default(),
            )
            .unwrap();
        runtime
            .observe(&run, "result")
            .unwrap()
            .view
            .rows
            .iter()
            .find(|r| r.record.record == record.id)
            .unwrap()
            .cells["reminder"]
            .clone()
    }
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Waiting work");
    action(&store, "wait", "wait", &job);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "first",
        )
        .unwrap();
    let first_layer = prepared.layer_id().unwrap().unwrap();
    let first = store.adopt_scoped(before.revision, &prepared).unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["actions"][6]["steps"][0]["columns"]["reminder"] =
        serde_json::to_value(boolean(false)).unwrap();
    raw["views"][0]["kind"]["columns"][3]["value"] = serde_json::to_value(boolean(false)).unwrap();
    let second_candidate = capture(raw.clone());
    let second_scope = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![LifecycleBinding {
            entity: "job".into(),
            completed: field("record", "done"),
            source: first.active_revision.clone(),
        }],
        patches: vec![
            EffectPatchRequest {
                destination: EffectDestination::EmitColumn {
                    action: "export".into(),
                    path: vec![0],
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
            EffectPatchRequest {
                destination: EffectDestination::ViewColumn {
                    view: "work".into(),
                    column: "reminder".into(),
                },
                entity: "job".into(),
                subject: "row".into(),
                value_type: Type::Boolean,
            },
        ],
    };
    let prepared = store
        .prepare_scoped_change(&second_candidate, &second_scope, "second")
        .unwrap();
    let second = store.adopt_scoped(first.revision, &prepared).unwrap();
    let withdrawal = store
        .prepare_scoped_withdrawal(&[first_layer], "withdraw-first")
        .unwrap();
    let healthy = store.adopt_scoped(second.revision, &withdrawal).unwrap();
    assert_eq!(
        reminder(&healthy, &job),
        DataValue::Boolean { value: false }
    );

    let mut older_predecessor = healthy.clone();
    let mut manifest =
        older_predecessor.scope.compositions[&older_predecessor.active_revision].clone();
    let prior = &first.scope.compositions[&first.active_revision];
    manifest.previous = Some(first.active_revision.clone());
    manifest.business = prior.business.clone();
    manifest.layers = prior.layers.clone();
    manifest.active.clear();
    manifest.rewrites = prior.rewrites.clone();
    reseal(&mut older_predecessor, manifest);
    // The forged bytes really remove the independent reminder rule, despite
    // retaining existing business rows/events and recomputing every hash.
    assert_eq!(older_predecessor.data.records, healthy.data.records);
    assert_eq!(older_predecessor.data.events, healthy.data.events);
    assert_eq!(
        reminder(&older_predecessor, &job),
        DataValue::Boolean { value: true }
    );

    let mut other_business = second.clone();
    raw["actions"][0]["steps"][0]["values"]["promised"] =
        serde_json::to_value(lit(DataValue::Date { days: 20050 }, Type::Date)).unwrap();
    let unrelated = capture(raw);
    let mut manifest = other_business.scope.compositions[&other_business.active_revision].clone();
    manifest.business = canonical_digest(IdentityDomain::Source, &unrelated).unwrap();
    other_business.programs.push(unrelated);
    reseal(&mut other_business, manifest);
    let runtime = LocalRuntime::default();
    let mut run = runtime
        .start(
            other_business.program().unwrap(),
            &other_business.data,
            &other_business.session,
            20000,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(
            &mut run,
            &invoke(
                "add",
                &[
                    ("name", text("Forged later work")),
                    ("promised", DataValue::Date { days: 20020 }),
                ],
            ),
            "forged-input",
        )
        .unwrap();
    assert_eq!(
        runtime
            .observe(&run, "result")
            .unwrap()
            .view
            .rows
            .iter()
            .find(|row| row.cells["name"] == text("Forged later work"))
            .unwrap()
            .cells["promised"],
        DataValue::Date { days: 20050 }
    );

    let mut other_basis = second.clone();
    let mut manifest = other_basis.scope.compositions[&other_basis.active_revision].clone();
    manifest.basis.snapshot =
        canonical_digest(IdentityDomain::Data, &"another frozen basis").unwrap();
    reseal(&mut other_basis, manifest);
    for (index, corrupted) in [older_predecessor, other_business, other_basis]
        .iter()
        .enumerate()
    {
        assert!(corrupted.validate().is_err());
        let destination = dir.path().join(format!("forged-manifest-{index}"));
        assert!(ProductStore::create_recovered_with(&destination, corrupted, |_| Ok(())).is_err());
        assert!(!destination.exists());
        assert_eq!(store.load().unwrap(), healthy);
    }
    let recovered = ProductStore::create_recovered(dir.path().join("valid"), &healthy).unwrap();
    add(&recovered, "continued", "Valid continued work");
    assert_eq!(
        reminder(&recovered.load().unwrap(), &job),
        DataValue::Boolean { value: false }
    );
}
