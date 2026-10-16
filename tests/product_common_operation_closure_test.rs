//! Generic analysis-only regressions. These are not accepted-scene evidence.
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
use fixture::{boolean, capture, field, int, var};
use product_contract::*;
use product_store::{scope::*, ProductStore};
use serde_json::json;

fn text(value: &str) -> Expr {
    fixture::lit(fixture::text(value), Type::Text)
}
fn eq(left: Expr, right: Expr) -> Expr {
    Expr::Equal {
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn not(value: Expr) -> Expr {
    Expr::Not {
        value: Box::new(value),
    }
}
fn state() -> Expr {
    Expr::State {
        state: "note_input".into(),
    }
}
fn query(entity: &str) -> Expr {
    Expr::Query {
        entity: entity.into(),
        binding: "q".into(),
        predicate: Box::new(boolean(true)),
        sort: vec![],
        limit: MAX_COLLECTION,
        include_archived: true,
    }
}
fn count() -> Expr {
    Expr::Count {
        items: Box::new(query("history")),
    }
}
fn pair() -> (CapturedProgram, CapturedProgram) {
    let mut before = fixture::program(false).program;
    before.entities[0].fields.push(
        serde_json::from_value(
            json!({"id":"latest","label":"Latest","value_type":{"kind":"text"}}),
        )
        .unwrap(),
    );
    before.entities.push(serde_json::from_value(json!({"id":"history","label":"History","fields":[{"id":"job","label":"Owner","value_type":{"kind":"reference","entity":"job"}},{"id":"title","label":"Title","value_type":{"kind":"text"}},{"id":"note","label":"Note","value_type":{"kind":"text"}},{"id":"day","label":"Day","value_type":{"kind":"date"}},{"id":"sequence","label":"Sequence","value_type":{"kind":"integer"}}],"unique":[],"constraints":[]})).unwrap());
    before.entities.push(serde_json::from_value(json!({"id":"material","label":"Material","fields":[{"id":"note","label":"Note","value_type":{"kind":"text"}}],"unique":[],"constraints":[]})).unwrap());
    before.state = vec![StateDefinition {
        id: "note_input".into(),
        label: "Note".into(),
        value_type: Type::Text,
        initial: fixture::text(""),
    }];
    before
        .actions
        .retain(|a| ["add", "calculate", "complete"].contains(&a.id.as_str()));
    let Statement::Create { values, .. } = &mut before.actions[0].steps[0] else {
        panic!()
    };
    values.insert("latest".into(), text(""));
    let calculate = before
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    calculate.steps = vec![Statement::Update {
        record: var("row"),
        values: [("production".into(), int(0))].into(),
    }];
    let complete = before
        .actions
        .iter_mut()
        .find(|a| a.id == "complete")
        .unwrap();
    complete.guards = vec![eq(field("row", "production"), int(1))];
    complete.steps = vec![Statement::Update {
        record: var("row"),
        values: [("done".into(), boolean(true))].into(),
    }];
    before.actions.push(ActionDefinition {
        id: "record_note".into(),
        label: "Record note".into(),
        parameters: [
            ("root".into(), Type::reference("job")),
            ("note".into(), Type::Text),
        ]
        .into(),
        guards: vec![
            not(eq(field("root", "done"), boolean(true))),
            not(eq(var("note"), text(""))),
        ],
        steps: vec![
            Statement::Create {
                entity: "history".into(),
                bind: "entry".into(),
                values: [
                    ("job".into(), var("root")),
                    ("title".into(), field("root", "name")),
                    ("note".into(), var("note")),
                    ("day".into(), Expr::Today),
                    (
                        "sequence".into(),
                        Expr::Add {
                            left: Box::new(count()),
                            right: Box::new(int(1)),
                        },
                    ),
                ]
                .into(),
            },
            Statement::Update {
                record: var("root"),
                values: [("latest".into(), var("note"))].into(),
            },
            Statement::SetState {
                state: "note_input".into(),
                value: text(""),
            },
        ],
        ensures: vec![],
    });
    before.outputs.clear();
    before.observables.clear();
    before.views[0].kind = ViewKind::List {
        entity: "job".into(),
        rows: query("job"),
        columns: vec![Column {
            id: "name".into(),
            label: "Name".into(),
            value: field("row", "name"),
        }],
        controls: vec![StateControl {
            id: "note".into(),
            label: "Note".into(),
            state: "note_input".into(),
            on_change: None,
        }],
        selection: None,
    };
    before.views[0].actions = vec![ActionBinding {
        id: "record".into(),
        label: "Record".into(),
        placement: ActionPlacement::Row,
        action: "record_note".into(),
        arguments: [("root".into(), var("row")), ("note".into(), state())].into(),
        enabled: Expr::And {
            values: vec![
                not(eq(field("row", "done"), boolean(true))),
                not(eq(state(), text(""))),
            ],
        },
    }];
    let mut rows = query("history");
    let Expr::Query {
        sort,
        include_archived,
        ..
    } = &mut rows
    else {
        panic!()
    };
    *include_archived = false;
    sort.push(SortKey {
        value: field("q", "sequence"),
        descending: true,
    });
    before.views.push(ViewDefinition {
        id: "history".into(),
        label: "History".into(),
        kind: ViewKind::List {
            entity: "history".into(),
            rows,
            columns: ["title", "note", "day", "sequence"]
                .into_iter()
                .map(|id| Column {
                    id: id.into(),
                    label: id.into(),
                    value: field("row", id),
                })
                .collect(),
            controls: vec![],
            selection: None,
        },
        actions: vec![],
        keys: vec![],
    });
    let mut after = before.clone();
    let Statement::Update { values, .. } = &mut after
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps[0]
    else {
        panic!()
    };
    values.insert("production".into(), int(1));
    (capture(json!(before)), capture(json!(after)))
}
fn endpoint(source: &CapturedProgram, action: &str, start: usize, len: usize) -> OwnedSpanEndpoint {
    let steps = &source
        .program
        .actions
        .iter()
        .find(|a| a.id == action)
        .unwrap()
        .steps;
    OwnedSpanEndpoint {
        source: canonical_digest(IdentityDomain::Source, source).unwrap(),
        action: action.into(),
        list_path: vec![],
        start,
        len,
        kinds: steps[start..start + len]
            .iter()
            .map(OwnedStatementKind::of)
            .collect(),
        digest: canonical_digest(
            IdentityDomain::Evidence,
            &("owned-span/1", &steps[start..start + len]),
        )
        .unwrap(),
        previous: start
            .checked_sub(1)
            .map(|i| canonical_digest(IdentityDomain::Evidence, &steps[i]).unwrap()),
        next: steps
            .get(start + len)
            .map(|s| canonical_digest(IdentityDomain::Evidence, s).unwrap()),
        root: Some(OwnedRootBinding {
            name: if action == "calculate" { "row" } else { "root" }.into(),
            value_type: Type::reference("job"),
        }),
    }
}
fn request(
    store: &ProductStore,
    before: &CapturedProgram,
    after: &CapturedProgram,
) -> OwnedEffectRequest {
    OwnedEffectRequest {
        snapshot: canonical_digest(IdentityDomain::Data, &store.load().unwrap()).unwrap(),
        sources: OwnedSourceRequest {
            before: canonical_digest(IdentityDomain::Source, before).unwrap(),
            after: canonical_digest(IdentityDomain::Source, after).unwrap(),
            root_entity: "job".into(),
            edits: vec![OwnedSpanEdit {
                id: "calculation".into(),
                before: endpoint(before, "calculate", 0, 1),
                after: endpoint(after, "calculate", 0, 1),
            }],
        },
        auxiliary: vec![],
    }
}
fn verify(
    before: &CapturedProgram,
    after: &CapturedProgram,
) -> Result<OwnedEffectReport, product_store::StoreError> {
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), before, 20000).unwrap();
    let snapshot = store.load().unwrap();
    let result = store.analyze_owned_effects(after, &request(&store, before, after));
    assert_eq!(
        store.load().unwrap(),
        snapshot,
        "analysis must not mutate the store"
    );
    result
}
fn alter_both(mut change: impl FnMut(&mut AppDefinition)) -> (CapturedProgram, CapturedProgram) {
    let (mut before, mut after) = pair();
    change(&mut before.program);
    change(&mut after.program);
    (
        capture(json!(before.program)),
        capture(json!(after.program)),
    )
}
fn record(app: &mut AppDefinition) -> &mut ActionDefinition {
    app.actions
        .iter_mut()
        .find(|a| a.id == "record_note")
        .unwrap()
}
fn shared_action(id: &str, value: Expr, guard: Expr) -> ActionDefinition {
    ActionDefinition {
        id: id.into(),
        label: id.into(),
        parameters: Default::default(),
        guards: vec![guard],
        steps: vec![Statement::Create {
            entity: "material".into(),
            bind: "made".into(),
            values: [("note".into(), value)].into(),
        }],
        ensures: vec![],
    }
}
fn rejects(before: &CapturedProgram, after: &CapturedProgram) {
    assert!(
        verify(before, after).is_err(),
        "unsupported producer/consumer must reject"
    );
}
#[test]
fn common_owned_append_preserves_exact_action() {
    let (before, after) = pair();
    let report = verify(&before, &after).unwrap();
    assert!(report
        .reads
        .iter()
        .any(|r| r.entity == "history" && r.field == "sequence"));
    assert!(report
        .reads
        .iter()
        .any(|r| r.entity == "job" && r.field == "done"));
    assert_eq!(before.program.actions.last(), after.program.actions.last());
    assert_eq!(report.common_operations.len(), 1);
    let operation = &report.common_operations[0];
    assert_eq!(operation.action, "record_note");
    assert_eq!(operation.append_path, vec![0]);
    assert_eq!(operation.reset_path, Some(vec![2]));
    assert_eq!(operation.bookkeeping_field.as_deref(), Some("sequence"));
    assert!(operation
        .permitted_uses
        .iter()
        .any(|u| u.location == "view history"));
    assert!(operation
        .permitted_uses
        .iter()
        .any(|u| u.location == "view work binding record"));
}
#[test]
fn common_owned_append_rejects_global_count_guard() {
    let (before, after) = alter_both(|app| {
        app.actions.push(shared_action(
            "leak",
            text("changed"),
            Expr::Less {
                left: Box::new(int(0)),
                right: Box::new(count()),
            },
        ))
    });
    rejects(&before, &after);
}
#[test]
fn common_owned_append_rejects_foreign_constraint() {
    let (before, after) = alter_both(|app| {
        app.entities
            .iter_mut()
            .find(|e| e.id == "material")
            .unwrap()
            .constraints
            .push(eq(count(), int(0)))
    });
    rejects(&before, &after);
}
#[test]
fn common_reset_rejects_direct_state_sink() {
    for guard in [false, true] {
        let (before, after) = alter_both(|app| {
            app.actions.push(shared_action(
                "leak",
                if guard { text("changed") } else { state() },
                if guard {
                    not(eq(state(), text("")))
                } else {
                    boolean(true)
                },
            ))
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_reset_rejects_binding_laundering() {
    let (before, after) = alter_both(|app| {
        let mut action = shared_action("leak", var("input"), boolean(true));
        action.parameters.insert("input".into(), Type::Text);
        app.actions.push(action);
        app.views[0].actions.push(ActionBinding {
            id: "leak".into(),
            label: "Leak".into(),
            placement: ActionPlacement::Toolbar,
            action: "leak".into(),
            arguments: [("input".into(), state())].into(),
            enabled: boolean(true),
        });
    });
    rejects(&before, &after);
}
#[test]
fn common_history_rejects_business_consumers() {
    for mode in 0..4 {
        let (before, after) = alter_both(|app| match mode {
            0 => app.observables.push(ObservableDefinition {
                id: "count".into(),
                label: "Count".into(),
                value: count(),
            }),
            1 => app.actions.push(ActionDefinition {
                id: "other".into(),
                label: "Other".into(),
                parameters: [
                    ("h".into(), Type::reference("history")),
                    ("root".into(), Type::reference("job")),
                ]
                .into(),
                guards: vec![],
                steps: vec![Statement::Update {
                    record: var("root"),
                    values: [("production".into(), field("h", "sequence"))].into(),
                }],
                ensures: vec![],
            }),
            2 => app.views[0].actions[0].enabled = eq(count(), int(0)),
            _ => {
                app.actions[0].ensures.push(eq(count(), int(0)));
            }
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_history_view_is_terminal() {
    let (before, after) = pair();
    verify(&before, &after).unwrap();
    for mode in 0..4 {
        let (before, after) = alter_both(|app| {
            if mode == 0 {
                app.views[1].actions.push(ActionBinding {
                    id: "read".into(),
                    label: "Read".into(),
                    placement: ActionPlacement::Row,
                    action: "calculate".into(),
                    arguments: [("row".into(), field("row", "job"))].into(),
                    enabled: boolean(true),
                });
                return;
            }
            let ViewKind::List {
                columns,
                controls,
                selection,
                ..
            } = &mut app.views[1].kind
            else {
                panic!()
            };
            match mode {
                1 => {
                    columns[0].value = Expr::Field {
                        record: Box::new(field("row", "job")),
                        field: "name".into(),
                    }
                }
                2 => {
                    columns[0].value = Expr::Concat {
                        left: Box::new(field("row", "title")),
                        right: Box::new(text("!")),
                    }
                }
                _ => {
                    controls.push(StateControl {
                        id: "note".into(),
                        label: "Note".into(),
                        state: "note_input".into(),
                        on_change: None,
                    });
                    assert!(selection.is_none());
                }
            }
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_operation_checks_all_producers() {
    for mode in 0..3 {
        let (before, after) = alter_both(|app| {
            let steps = if mode == 0 {
                vec![record(app).steps[0].clone()]
            } else if mode == 1 {
                vec![Statement::Update {
                    record: var("h"),
                    values: [("job".into(), var("root"))].into(),
                }]
            } else {
                vec![Statement::Archive { record: var("h") }]
            };
            app.actions.push(ActionDefinition {
                id: "hidden_writer".into(),
                label: "Hidden writer".into(),
                parameters: [
                    ("root".into(), Type::reference("job")),
                    ("note".into(), Type::Text),
                    ("h".into(), Type::reference("history")),
                ]
                .into(),
                guards: vec![],
                steps,
                ensures: vec![],
            });
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_operation_rejects_foreign_root_read() {
    for mode in 0..3 {
        let (before, after) = alter_both(|app| {
            let a = record(app);
            if mode == 0 {
                a.parameters
                    .insert("foreign".into(), Type::reference("job"));
                a.guards.push(not(field("foreign", "done")));
                app.views[0].actions[0]
                    .arguments
                    .insert("foreign".into(), var("row"));
            } else if mode == 1 {
                a.guards.push(not(Expr::Field {
                    record: Box::new(fixture::lit(
                        DataValue::Reference {
                            entity: "job".into(),
                            record: "foreign".into(),
                        },
                        Type::reference("job"),
                    )),
                    field: "done".into(),
                }));
            } else {
                a.guards.push(eq(
                    Expr::Count {
                        items: Box::new(query("job")),
                    },
                    int(0),
                ));
            }
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_operation_rejects_changed_identity() {
    for mode in 0..5 {
        let (before, mut after) = pair();
        let a = record(&mut after.program);
        match mode {
            0 => a.guards[1] = boolean(true),
            1 => a.steps.swap(0, 1),
            2 => {
                let Statement::SetState { value, .. } = &mut a.steps[2] else {
                    panic!()
                };
                *value = text("changed");
            }
            3 => {
                let Statement::Create { values, .. } = &mut a.steps[0] else {
                    panic!()
                };
                values.insert("sequence".into(), int(1));
            }
            _ => a.id = "renamed".into(),
        }
        if mode == 4 {
            after.program.views[0].actions[0].action = "renamed".into();
        }
        let after = capture(json!(after.program));
        rejects(&before, &after);
    }
    let (before, after) = pair();
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let mut req = request(&store, &before, &after);
    req.sources.edits.push(OwnedSpanEdit {
        id: "gap".into(),
        before: endpoint(&before, "record_note", 1, 0),
        after: endpoint(&after, "record_note", 1, 0),
    });
    assert!(store.analyze_owned_effects(&after, &req).is_err());
}
#[test]
fn common_operation_rechecks_all_consumers() {
    let (before, after) = pair();
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let req = request(&store, &before, &after);
    let report = store.analyze_owned_effects(&after, &req).unwrap();
    let mut later = after.program.clone();
    later.observables.push(ObservableDefinition {
        id: "later".into(),
        label: "Later".into(),
        value: count(),
    });
    let later = capture(json!(later));
    assert!(store.analyze_owned_effects(&later, &req).is_err());
    let mut fresh = request(&store, &before, &later);
    assert!(store.analyze_owned_effects(&later, &fresh).is_err());
    fixture::add(&store, "new", "new");
    fresh.snapshot = report.snapshot;
    assert!(store.analyze_owned_effects(&after, &fresh).is_err());
}
#[test]
fn common_completion_is_not_global_equivalence() {
    let (before, after) = pair();
    verify(&before, &after).unwrap();
    let mut admissions = vec![];
    for source in [&before, &after] {
        let dir = fixture::tempdir();
        let store = ProductStore::create(dir.path().join("project"), source, 20000).unwrap();
        let root = fixture::add(&store, "add", "job");
        fixture::action(&store, "calculate", "calculate", &root);
        let complete = store
            .apply(
                store.load().unwrap().revision,
                "complete",
                &fixture::invoke("complete", &[("row", fixture::reference(&root))]),
                RuntimeLimits::default(),
            )
            .is_ok();
        let progress = store
            .apply(
                store.load().unwrap().revision,
                "note",
                &fixture::invoke(
                    "record_note",
                    &[
                        ("root", fixture::reference(&root)),
                        ("note", fixture::text("fact")),
                    ],
                ),
                RuntimeLimits::default(),
            )
            .is_ok();
        admissions.push((complete, progress));
    }
    assert_eq!(admissions, vec![(false, true), (true, false)]);
}

#[test]
fn common_operation_keeps_root_and_transitive_field_taint() {
    for protected in ["done", "latest"] {
        let (before, after) = alter_both(|app| {
            let mut a = shared_action(
                "shared_sink",
                text("leak"),
                if protected == "done" {
                    not(field("root", "done"))
                } else {
                    not(eq(field("root", "latest"), text("")))
                },
            );
            a.parameters.insert("root".into(), Type::reference("job"));
            app.actions.push(a);
        });
        let error = verify(&before, &after).unwrap_err().to_string();
        assert!(
            error.contains("shared_sink") && error.contains("dependent create"),
            "{error}"
        );
    }
}
#[test]
fn common_operation_rejects_coupled_or_optional_owner_schema() {
    for mode in 0..3 {
        let (before, after) = alter_both(|app| {
            let h = app.entities.iter_mut().find(|e| e.id == "history").unwrap();
            match mode {
                0 => h.unique = vec![vec!["sequence".into()]],
                1 => h.constraints.push(boolean(true)),
                _ => {
                    h.fields[0].value_type = Type::Optional {
                        item: Box::new(Type::reference("job")),
                    }
                }
            }
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_operation_rejects_computed_reset_and_control_callback() {
    for mode in 0..3 {
        let (before, after) = alter_both(|app| match mode {
            0 => {
                let Statement::SetState { value, .. } = &mut record(app).steps[2] else {
                    panic!()
                };
                *value = field("root", "latest");
            }
            1 => {
                record(app).steps.swap(1, 2);
            }
            _ => {
                let mut callback = shared_action("changed", var("value"), boolean(true));
                callback.parameters.insert("value".into(), Type::Text);
                app.actions.push(callback);
                let ViewKind::List { controls, .. } = &mut app.views[0].kind else {
                    panic!()
                };
                controls[0].on_change = Some("changed".into());
            }
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_history_rejects_implicit_collection_consumer() {
    let (before, after) = alter_both(|app| {
        let ty = Type::list(Type::reference("history"));
        app.entities[0].fields.push(FieldDefinition {
            id: "references".into(),
            label: "References".into(),
            value_type: ty.clone(),
        });
        let empty = fixture::lit(
            DataValue::List {
                item_type: Type::reference("history"),
                items: vec![],
            },
            ty,
        );
        let Statement::Create { values, .. } = &mut app.actions[0].steps[0] else {
            panic!()
        };
        values.insert("references".into(), empty.clone());
        app.actions.push(ActionDefinition {
            id: "collection_consumer".into(),
            label: "Collection consumer".into(),
            parameters: [("root".into(), Type::reference("job"))].into(),
            guards: vec![],
            steps: vec![Statement::Collection {
                target: CollectionTarget::Field {
                    record: var("root"),
                    field: "references".into(),
                },
                operation: CollectionOperation::Remove,
                items: empty,
            }],
            ensures: vec![],
        });
    });
    rejects(&before, &after);
}

#[test]
fn common_history_rejects_unused_reference_admission() {
    for ty in [
        Type::reference("history"),
        Type::Optional {
            item: Box::new(Type::reference("history")),
        },
        Type::list(Type::reference("history")),
    ] {
        let (before, after) = alter_both(|app| {
            let mut action = shared_action("unused_reference", text("shared"), boolean(true));
            action.parameters.insert("unused".into(), ty.clone());
            app.actions.push(action);
        });
        rejects(&before, &after);
    }
}
#[test]
fn common_history_rejects_reference_control_admission() {
    let (before, after) = alter_both(|app| {
        app.state.push(StateDefinition {
            id: "history_reference".into(),
            label: "History reference".into(),
            value_type: Type::Optional {
                item: Box::new(Type::reference("history")),
            },
            initial: DataValue::Null,
        });
        let ViewKind::List { controls, .. } = &mut app.views[0].kind else {
            panic!()
        };
        controls.push(StateControl {
            id: "history_reference".into(),
            label: "History reference".into(),
            state: "history_reference".into(),
            on_change: None,
        });
    });
    rejects(&before, &after);
}
