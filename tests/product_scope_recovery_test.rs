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
