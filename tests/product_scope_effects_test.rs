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
use product_runtime::LocalRuntime;
use product_store::{scope::*, ProductStore};

fn production(store: &ProductStore, row: &Record) -> i64 {
    let s = store.load().unwrap();
    let runtime = LocalRuntime::default();
    let run = runtime
        .start(
            s.program().unwrap(),
            &s.data,
            &s.session,
            s.clock_day,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    let view = runtime.observe(&run, "result").unwrap().view;
    let actual = view
        .rows
        .iter()
        .find(|r| r.record.record == row.id)
        .unwrap();
    if let DataValue::Integer { value } = actual.cells["production"] {
        value
    } else {
        panic!("wrong type")
    }
}
#[test]
fn future_cohort_survives_waiting_edits_completion_and_mixed_export() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let old = add(&store, "old", "Existing");
    let completed = add(&store, "completed", "Completed");
    let archived = add(&store, "archived", "Archived");
    tick(&store, "before", 20002);
    action(&store, "finish-old", "complete", &completed);
    action(&store, "archive-old", "archive", &archived);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    assert_eq!(store.load().unwrap(), before);
    let after = store.adopt_scoped(before.revision, &prepared).unwrap();
    assert_eq!(after.data.events, before.data.events);
    for historical in [&completed, &archived] {
        let actual = row(&after, historical);
        let prior = row(&before, historical);
        assert_eq!(actual.revision, prior.revision);
        assert_eq!(actual.archived, prior.archived);
        for (k, v) in &prior.values {
            assert_eq!(actual.values.get(k), Some(v));
        }
    }
    let new = add(&store, "new", "Future");
    action(&store, "wait-new", "wait", &new);
    action(&store, "wait-old", "wait", &old);
    tick(&store, "three-days", 20005);
    action(&store, "calculate-new", "calculate", &new);
    assert_eq!(production(&store, &new), 0);
    assert_eq!(production(&store, &old), 5);
    assert_eq!(production(&store, &completed), 2);
    assert_eq!(production(&store, &archived), 2);
    let current = store.load().unwrap();
    assert_eq!(
        row(&current, &new).values["promised"],
        DataValue::Date { days: 20020 }
    );
    assert_eq!(
        row(&current, &new).values["waiting"],
        DataValue::Boolean { value: true }
    );
    action(&store, "resume-new", "resume", &new);
    tick(&store, "production-days", 20007);
    action(&store, "complete-new", "complete", &new);
    tick(&store, "later-clock", 20010);
    assert_eq!(production(&store, &new), 2);
    let exported = apply(&store, "export-all", invoke("export", &[]));
    let output = exported.artifacts.last().unwrap();
    assert_eq!(output.rows.len(), 4);
    assert!(String::from_utf8(output.bytes.clone())
        .unwrap()
        .contains("Future,2"));
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), exported);
    assert_eq!(production(&store, &new), 2);
}
#[test]
fn selected_unfinished_population_is_frozen_and_preparation_goes_stale() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let selected = add(&store, "selected", "Selected");
    let other = add(&store, "other", "Other");
    let before = store.load().unwrap();
    let refs = vec![RecordRef {
        entity: selected.entity.clone(),
        record: selected.id.clone(),
    }];
    let request = request(
        &before,
        ScopePopulation::SelectedUnfinished { records: refs },
    );
    let stale = store
        .prepare_scoped_change(&program(true), &request, "stale")
        .unwrap();
    action(&store, "edit", "wait", &selected);
    assert!(store.adopt_scoped(before.revision, &stale).is_err());
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(&program(true), &request, "chosen")
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let future = add(&store, "future", "Not selected");
    action(&store, "wait-other", "wait", &other);
    action(&store, "wait-future", "wait", &future);
    tick(&store, "wait-three", 20003);
    assert_eq!(production(&store, &selected), 0);
    assert_eq!(production(&store, &other), 3);
    assert_eq!(production(&store, &future), 3);
    action(&store, "finish", "complete", &selected);
    tick(&store, "later", 20005);
    assert_eq!(production(&store, &selected), 0);
}
#[test]
fn partial_structural_changes_and_raw_metadata_spoofing_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    add(&store, "old", "Old");
    let before = store.load().unwrap();
    let req = request(&before, ScopePopulation::FutureWork);
    let mut candidate = serde_json::to_value(program(true).program).unwrap();
    candidate["actions"][6]["steps"][0]["items"]["limit"] = serde_json::json!(1);
    assert!(store
        .prepare_scoped_change(&capture(candidate), &req, "bad-loop")
        .is_err());
    let prepared = store
        .prepare_scoped_change(&program(true), &req, "valid")
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let current = store.load().unwrap();
    let switch = store.prepare_switch(&program(false), "bypass");
    assert!(
        switch.is_err()
            || store
                .adopt(
                    current.revision,
                    &switch.unwrap(),
                    &program(false),
                    &current.decisions
                )
                .is_err()
    );
    let mut forged = current.clone();
    let key = forged.data.records[0]
        .values
        .keys()
        .find(|k| k.starts_with("gm_scope_") && k.ends_with("member"))
        .unwrap()
        .clone();
    forged.data.records[0]
        .values
        .insert(key, DataValue::Boolean { value: true });
    assert!(forged.validate().is_err());
    assert_eq!(store.load().unwrap(), current);
}

#[test]
fn optional_assignment_widening_preserves_types_nulls_and_atomic_constraints() {
    let mut raw = serde_json::to_value(program(false).program).unwrap();
    raw["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"optional_days","label":"Optional days","value_type":{"kind":"optional","item":{"kind":"integer"}}}));
    raw["actions"][0]["steps"][0]["values"]["optional_days"] =
        serde_json::to_value(int(4)).unwrap();
    raw["actions"][3]["steps"][0]["values"]["optional_days"] =
        serde_json::to_value(elapsed("row", false)).unwrap();
    let p = capture(raw.clone());
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("widened"), &p, 20000).unwrap();
    let r = add(&store, "add", "Optional assignment");
    assert_eq!(r.values["optional_days"], DataValue::Integer { value: 4 });
    tick(&store, "advance", 20003);
    let next = action(&store, "update", "calculate", &r);
    assert_eq!(
        row(&next, &r).values["optional_days"],
        DataValue::Integer { value: 3 }
    );
    let mut missing = raw.clone();
    missing["actions"][0]["steps"][0]["values"]
        .as_object_mut()
        .unwrap()
        .remove("optional_days");
    let missing_store =
        ProductStore::create(dir.path().join("missing"), &capture(missing), 20000).unwrap();
    let absent = add(&missing_store, "missing", "Missing optional");
    assert!(!absent.values.contains_key("optional_days"));
    let mut nullable = raw.clone();
    nullable["actions"][0]["steps"][0]["values"]["optional_days"] = serde_json::json!({"kind":"literal","value_type":{"kind":"optional","item":{"kind":"integer"}},"value":{"kind":"null"}});
    let null_store =
        ProductStore::create(dir.path().join("null"), &capture(nullable.clone()), 20000).unwrap();
    assert_eq!(
        add(&null_store, "null", "Null optional").values["optional_days"],
        DataValue::Null
    );
    nullable["entities"][0]["constraints"] = serde_json::json!([{"kind":"not","value":{"kind":"equal","left":field("record","optional_days"),"right":{"kind":"literal","value_type":{"kind":"optional","item":{"kind":"integer"}},"value":{"kind":"null"}}}}]);
    let constrained =
        ProductStore::create(dir.path().join("constraints"), &capture(nullable), 20000).unwrap();
    let before = constrained.load().unwrap();
    assert!(constrained
        .apply(
            0,
            "invalid",
            &invoke(
                "add",
                &[
                    ("name", text("Invalid")),
                    ("promised", DataValue::Date { days: 20020 })
                ]
            ),
            RuntimeLimits::default()
        )
        .is_err());
    assert_eq!(constrained.load().unwrap(), before);
    for kind in ["text", "boolean"] {
        let mut bad = raw.clone();
        bad["actions"][3]["steps"][0]["values"]["optional_days"] = if kind == "text" {
            serde_json::to_value(lit(text("Wrong"), Type::Text)).unwrap()
        } else {
            serde_json::to_value(boolean(true)).unwrap()
        };
        assert!(AppDefinition::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    raw["entities"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["value_type"] =
        serde_json::json!({"kind":"optional","item":{"kind":"optional","item":{"kind":"integer"}}});
    assert!(AppDefinition::parse(&serde_json::to_vec(&raw).unwrap()).is_err());
}

#[path = "fixtures/product_runtime/mod.rs"]
mod contacts;
#[test]
fn export_only_result_changes_do_not_modify_shared_selection_or_delete() {
    let mut source = contacts::organizer();
    source["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"delete_selected","label":"Archive selected","parameters":{},"guards":[],"steps":[{"kind":"for_each","items":{"kind":"state","state":"selected"},"binding":"person","steps":[{"kind":"archive","record":contacts::var("person")}]}],"ensures":[]}));
    let original = contacts::capture(source.clone());
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("contacts"), &original, 20000).unwrap();
    apply(&store, "ada", contacts::add("Ada"));
    apply(&store, "bea", contacts::add("Bea"));
    apply(
        &store,
        "collect",
        contacts::invoke("collect", Values::new()),
    );
    let original_selection = store.load().unwrap().session.values["selected"].clone();
    let mut candidate = source.clone();
    let filtered = serde_json::json!({"kind":"filter","items":{"kind":"state","state":"selected"},"binding":"person","predicate":{"kind":"equal","left":contacts::field(contacts::var("person"),"name"),"right":contacts::text("Ada")}});
    candidate["actions"][2]["steps"][0]["items"] = filtered.clone();
    candidate["observables"][0]["value"]["items"] = filtered;
    let candidate = contacts::capture(candidate);
    let current = store.load().unwrap();
    let request = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export_people".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![],
        patches: vec![
            EffectPatchRequest {
                destination: EffectDestination::EmitItems {
                    action: "export_people".into(),
                    path: vec![0],
                },
                entity: "person".into(),
                subject: "person".into(),
                value_type: Type::list(Type::reference("person")),
            },
            EffectPatchRequest {
                destination: EffectDestination::Observable {
                    observable: "selected_count".into(),
                },
                entity: "person".into(),
                subject: "person".into(),
                value_type: Type::Integer,
            },
        ],
    };
    let prepared = store
        .prepare_scoped_change(&candidate, &request, "export-preference")
        .unwrap();
    store.adopt_scoped(current.revision, &prepared).unwrap();
    // Program adoption resets session by the established store contract. Restore
    // the actual selection through the real collection action before comparison.
    apply(
        &store,
        "reselect",
        contacts::invoke("collect", Values::new()),
    );
    let selected = store.load().unwrap();
    assert_eq!(selected.session.values["selected"], original_selection);
    let runtime = LocalRuntime::default();
    let run = runtime
        .start(
            selected.program().unwrap(),
            &selected.data,
            &selected.session,
            selected.clock_day,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        runtime.observe(&run, "preview").unwrap().values["selected_count"],
        DataValue::Integer { value: 1 }
    );
    let exported = apply(
        &store,
        "export",
        contacts::invoke("export_people", Values::new()),
    );
    assert_eq!(exported.artifacts.last().unwrap().rows.len(), 1);
    assert_eq!(exported.session.values["selected"], original_selection);
    let deleted = apply(
        &store,
        "delete",
        contacts::invoke("delete_selected", Values::new()),
    );
    assert_eq!(
        deleted.data.records.iter().filter(|r| r.archived).count(),
        2
    );
    let mut unsafe_candidate = source;
    unsafe_candidate["actions"][1]["steps"][0]["items"]["limit"] = serde_json::json!(1);
    assert!(store
        .prepare_scoped_change(
            &contacts::capture(unsafe_candidate),
            &request,
            "shared-state-change"
        )
        .is_err());
}

#[test]
fn missing_birth_and_damaged_completed_values_fail_without_live_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let row = add(&store, "later", "Later");
    action(&store, "wait", "wait", &row);
    tick(&store, "time", 20003);
    let complete = action(&store, "complete", "complete", &row);
    let mut missing = complete.clone();
    missing.data.events.retain(|e| e.operation_id != "later");
    assert!(missing.validate().is_err());
    let mut corrupt = complete.clone();
    let saved = corrupt.data.records[0]
        .values
        .keys()
        .find(|key| key.starts_with("gm_scope_") && key.contains("_value_"))
        .unwrap()
        .clone();
    corrupt.data.records[0]
        .values
        .insert(saved, DataValue::Null);
    assert!(corrupt.validate().is_err());
    let mut wrong_producer = complete.clone();
    wrong_producer.data.records[0].created_program =
        wrong_producer.programs[0].artifact.program_digest.clone();
    assert!(wrong_producer.validate().is_err());
    let mut forged = complete.clone();
    forged
        .scope
        .compositions
        .values_mut()
        .next()
        .unwrap()
        .active
        .clear();
    assert!(forged.validate().is_err());
    assert_eq!(store.load().unwrap(), complete);
}
#[test]
fn failed_mixed_transaction_rolls_back_fields_seals_outputs_and_stamps() {
    fn source(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        let query = serde_json::json!({"kind":"query","entity":"job","binding":"item","predicate":boolean(true),"sort":[],"limit":1000,"include_archived":false});
        let emit = raw["actions"][6]["steps"][0].clone();
        raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"batch_fail","label":"Atomic failed batch","parameters":{},"guards":[],"steps":[{"kind":"for_each","items":query,"binding":"item","steps":[{"kind":"update","record":var("item"),"values":{"production":elapsed("item",pause),"done":boolean(true)}}]},emit,{"kind":"assert","condition":boolean(false),"message":"Stop the entire transaction"}],"ensures":[]}));
        capture(raw)
    }
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &source(false), 20000).unwrap();
    add(&store, "old", "Old");
    let before = store.load().unwrap();
    let mut req = request(&before, ScopePopulation::FutureWork);
    req.operations.insert("batch_fail".into());
    req.patches.push(EffectPatchRequest {
        destination: EffectDestination::Update {
            action: "batch_fail".into(),
            path: vec![0, 0],
            field: "production".into(),
        },
        entity: "job".into(),
        subject: "item".into(),
        value_type: Type::Integer,
    });
    req.patches.push(EffectPatchRequest {
        destination: EffectDestination::EmitColumn {
            action: "batch_fail".into(),
            path: vec![1],
            column: "production".into(),
        },
        entity: "job".into(),
        subject: "row".into(),
        value_type: Type::Integer,
    });
    let prepared = store
        .prepare_scoped_change(&source(true), &req, "scope")
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let later = add(&store, "new", "New");
    action(&store, "wait", "wait", &later);
    tick(&store, "days", 20003);
    let before = store.load().unwrap();
    assert!(store
        .apply(
            before.revision,
            "batch-failure",
            &invoke("batch_fail", &[]),
            RuntimeLimits::default()
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn independent_operation_layer_does_not_corrupt_completion_or_withdrawal() {
    fn with_indicator(pause: bool, visible: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        raw["observables"] = serde_json::json!([{"id":"show_summary","label":"Show summary","value":boolean(visible)}]);
        capture(raw)
    }
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(
        dir.path().join("tool"),
        &with_indicator(false, false),
        20000,
    )
    .unwrap();
    let before = store.load().unwrap();
    let first = store
        .prepare_scoped_change(
            &with_indicator(true, false),
            &request(&before, ScopePopulation::FutureWork),
            "timing",
        )
        .unwrap();
    let timing = first.layer_id().unwrap().unwrap();
    store.adopt_scoped(before.revision, &first).unwrap();
    let independent = ScopeRequest {
        population: ScopePopulation::All,
        operations: ["export".into()].into_iter().collect(),
        excluded_records: vec![],
        lifecycles: vec![],
        patches: vec![EffectPatchRequest {
            destination: EffectDestination::Observable {
                observable: "show_summary".into(),
            },
            entity: "job".into(),
            subject: "row".into(),
            value_type: Type::Boolean,
        }],
    };
    let before = store.load().unwrap();
    let second = store
        .prepare_scoped_change(&with_indicator(true, true), &independent, "summary")
        .unwrap();
    store.adopt_scoped(before.revision, &second).unwrap();
    let job = add(&store, "later", "Later");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let completed = action(&store, "finish", "complete", &job);
    assert_eq!(
        row(&completed, &job).values["production"],
        DataValue::Integer { value: 0 }
    );
    let withdrawal = store
        .prepare_scoped_withdrawal(&[timing], "withdraw-timing")
        .unwrap();
    let preserved = store.adopt_scoped(completed.revision, &withdrawal).unwrap();
    let runtime = LocalRuntime::default();
    let run = runtime
        .start(
            preserved.program().unwrap(),
            &preserved.data,
            &preserved.session,
            preserved.clock_day,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        runtime.observe(&run, "independent").unwrap().values["show_summary"],
        DataValue::Boolean { value: true }
    );
}
#[test]
fn foreign_subject_dependency_is_rejected_before_any_adoption() {
    fn coupled(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        raw["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"copied","label":"Copied result","value_type":{"kind":"integer"}}));
        raw["actions"][0]["steps"][0]["values"]["copied"] = serde_json::to_value(int(0)).unwrap();
        raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"copy_result","label":"Copy to another job","parameters":{"source":{"kind":"reference","entity":"job"},"target":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("target"),"values":{"copied":field("source","production")}}],"ensures":[]}));
        capture(raw)
    }
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &coupled(false), 20000).unwrap();
    let row = add(&store, "first", "Selected");
    add(&store, "second", "Unselected");
    let before = store.load().unwrap();
    let req = request(
        &before,
        ScopePopulation::SelectedUnfinished {
            records: vec![RecordRef {
                entity: row.entity,
                record: row.id,
            }],
        },
    );
    assert!(store
        .prepare_scoped_change(&coupled(true), &req, "unsafe-copy")
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn completion_export_order_is_verified_before_activation() {
    fn combined(pause: bool, late_write: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        let complete = raw["actions"][4]["steps"][0].clone();
        let mut emit = raw["actions"][6]["steps"][0].clone();
        fn rename(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Object(m) => {
                    if m.get("kind").and_then(|v| v.as_str()) == Some("variable")
                        && m.get("name").and_then(|v| v.as_str()) == Some("row")
                    {
                        m.insert("name".into(), serde_json::json!("output_row"));
                    }
                    for v in m.values_mut() {
                        rename(v)
                    }
                }
                serde_json::Value::Array(a) => {
                    for v in a {
                        rename(v)
                    }
                }
                _ => {}
            }
        }
        emit["binding"] = serde_json::json!("output_row");
        rename(&mut emit);
        let mut steps = vec![complete, emit];
        if late_write {
            steps.push(serde_json::json!({"kind":"update","record":var("row"),"values":{"waited":int(99)}}));
        }
        raw["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"complete_export","label":"Complete and export","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":steps,"ensures":[]}));
        capture(raw)
    }
    for unsafe_order in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = ProductStore::create(
            dir.path().join("tool"),
            &combined(false, unsafe_order),
            20000,
        )
        .unwrap();
        let before = store.load().unwrap();
        let mut req = request(&before, ScopePopulation::FutureWork);
        req.operations.insert("complete_export".into());
        req.patches.push(EffectPatchRequest {
            destination: EffectDestination::Update {
                action: "complete_export".into(),
                path: vec![0],
                field: "production".into(),
            },
            entity: "job".into(),
            subject: "row".into(),
            value_type: Type::Integer,
        });
        req.patches.push(EffectPatchRequest {
            destination: EffectDestination::EmitColumn {
                action: "complete_export".into(),
                path: vec![1],
                column: "production".into(),
            },
            entity: "job".into(),
            subject: "output_row".into(),
            value_type: Type::Integer,
        });
        let prepared = store.prepare_scoped_change(&combined(true, unsafe_order), &req, "scope");
        if unsafe_order {
            assert!(prepared.is_err());
            assert_eq!(store.load().unwrap(), before);
        } else {
            let prepared = prepared.unwrap();
            store.adopt_scoped(before.revision, &prepared).unwrap();
            let job = add(&store, "later", "Later");
            action(&store, "wait", "wait", &job);
            tick(&store, "days", 20003);
            let completed = action(&store, "complete-export", "complete_export", &job);
            assert_eq!(
                completed.artifacts.last().unwrap().rows[0]["production"],
                DataValue::Integer { value: 0 }
            );
            assert!(
                String::from_utf8(completed.artifacts.last().unwrap().bytes.clone())
                    .unwrap()
                    .contains("ObservedAtCompletion")
            );
        }
    }
}
#[test]
fn unpatched_shared_export_cannot_be_labelled_as_preserved_history() {
    fn shared(pause: bool) -> CapturedProgram {
        let mut raw = serde_json::to_value(program(pause).program).unwrap();
        let mut other = serde_json::to_value(program(false).program.actions[6].clone()).unwrap();
        other["id"] = serde_json::json!("export_other");
        raw["actions"].as_array_mut().unwrap().push(other);
        capture(raw)
    }
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &shared(false), 20000).unwrap();
    let before = store.load().unwrap();
    assert!(store
        .prepare_scoped_change(
            &shared(true),
            &request(&before, ScopePopulation::FutureWork),
            "unsafe-shared-output"
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
}
