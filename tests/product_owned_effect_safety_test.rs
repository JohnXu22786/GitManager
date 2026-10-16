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
use fixture::{boolean, capture, field, int, program, var};
use product_contract::*;
use product_store::{scope::*, ProductStore};
use serde_json::json;

fn query(entity: &str, binding: &str, predicate: Expr) -> Expr {
    Expr::Query {
        entity: entity.into(),
        binding: binding.into(),
        predicate: Box::new(predicate),
        sort: vec![],
        limit: MAX_COLLECTION,
        include_archived: false,
    }
}
fn eq(left: Expr, right: Expr) -> Expr {
    Expr::Equal {
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn count(items: Expr) -> Expr {
    Expr::Count {
        items: Box::new(items),
    }
}
fn recapture(source: &CapturedProgram) -> CapturedProgram {
    capture(serde_json::to_value(&source.program).unwrap())
}
fn endpoint(source: &CapturedProgram, start: usize, len: usize) -> OwnedSpanEndpoint {
    let steps = &source
        .program
        .actions
        .iter()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps;
    OwnedSpanEndpoint {
        source: canonical_digest(IdentityDomain::Source, source).unwrap(),
        action: "calculate".into(),
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
            name: "row".into(),
            value_type: Type::reference("job"),
        }),
    }
}
fn pair() -> (CapturedProgram, CapturedProgram) {
    let mut before = program(false);
    // Keep fixture observations independent of the changed production field.
    let ViewKind::List { rows, columns, .. } = &mut before.program.views[0].kind else {
        panic!()
    };
    *rows = query("job", "q", boolean(true));
    columns[1].value = field("row", "production");
    before
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps = vec![Statement::Update {
        record: var("row"),
        values: [("production".into(), int(0))].into(),
    }];
    before.program.entities.push(serde_json::from_value(json!({"id":"material","label":"Material","fields":[{"id":"ready","label":"Ready","value_type":{"kind":"boolean"}}],"unique":[],"constraints":[]})).unwrap());
    before.program.entities.push(serde_json::from_value(json!({"id":"requirement","label":"Requirement","fields":[{"id":"job","label":"Job","value_type":{"kind":"reference","entity":"job"}},{"id":"material","label":"Material","value_type":{"kind":"reference","entity":"material"}}],"unique":[],"constraints":[]})).unwrap());
    before = recapture(&before);
    let mut after = before.clone();
    after.program.entities.push(serde_json::from_value(json!({"id":"pause","label":"Memory","fields":[{"id":"job","label":"Owner","value_type":{"kind":"reference","entity":"job"}},{"id":"pending","label":"Pending","value_type":{"kind":"boolean"}}],"unique":[["job"]],"constraints":[]})).unwrap());
    let action = after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps = vec![
        Statement::Update {
            record: var("row"),
            values: [(
                "production".into(),
                count(query(
                    "requirement",
                    "link",
                    eq(field("link", "job"), var("row")),
                )),
            )]
            .into(),
        },
        Statement::ForEach {
            items: query("pause", "p", eq(field("p", "job"), var("row"))),
            binding: "memory".into(),
            steps: vec![Statement::Update {
                record: var("memory"),
                values: [("pending".into(), boolean(false))].into(),
            }],
        },
    ];
    (before, recapture(&after))
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
                id: "calculate".into(),
                before: endpoint(before, 0, 1),
                after: endpoint(
                    after,
                    0,
                    after
                        .program
                        .actions
                        .iter()
                        .find(|a| a.id == "calculate")
                        .unwrap()
                        .steps
                        .len(),
                ),
            }],
        },
        auxiliary: vec![OwnedAuxiliary {
            entity: "pause".into(),
            owner_field: "job".into(),
        }],
    }
}
fn verify(
    before: &CapturedProgram,
    after: &CapturedProgram,
) -> Result<OwnedEffectReport, product_store::StoreError> {
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), before, 20000).unwrap();
    store.analyze_owned_effects(after, &request(&store, before, after))
}
fn steps(source: &mut CapturedProgram) -> &mut Vec<Statement> {
    &mut source
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps
}
fn rejected(before: &CapturedProgram, after: &CapturedProgram, text: &str) {
    let after = recapture(after);
    let error = verify(before, &after).unwrap_err().to_string();
    assert!(error.contains(text), "expected {text}: {error}");
}
#[test]
fn accepts_typed_correlated_reads_and_owned_writes_without_mutation() {
    let (before, mut after) = pair();
    let ready = Expr::All {
        items: Box::new(query(
            "requirement",
            "link",
            eq(field("link", "job"), var("row")),
        )),
        binding: "required".into(),
        predicate: Box::new(Expr::Field {
            record: Box::new(field("required", "material")),
            field: "ready".into(),
        }),
    };
    steps(&mut after).push(Statement::Create {
        entity: "pause".into(),
        bind: "created".into(),
        values: [("job".into(), var("row")), ("pending".into(), ready)].into(),
    });
    let after = recapture(&after);
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let snapshot = store.load().unwrap();
    let report = store
        .analyze_owned_effects(&after, &request(&store, &before, &after))
        .unwrap();
    assert_eq!(
        report.snapshot,
        canonical_digest(IdentityDomain::Data, &snapshot).unwrap()
    );
    assert!(report
        .reads
        .iter()
        .any(|r| r.entity == "material" && r.field == "ready"));
    assert_eq!(store.load().unwrap(), snapshot);
}
#[test]
fn rejects_disjunctive_negated_and_wrong_root_owner_claims() {
    let (before, after) = pair();
    for predicate in [
        Expr::Or {
            values: vec![eq(field("p", "job"), var("row")), boolean(true)],
        },
        Expr::Not {
            value: Box::new(eq(field("p", "job"), var("row"))),
        },
    ] {
        let mut bad = after.clone();
        let Statement::ForEach { items, .. } = &mut steps(&mut bad)[1] else {
            panic!()
        };
        *items = query("pause", "p", predicate);
        rejected(&before, &bad, "calculate [1]");
    }
    let mut before = before;
    before
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .parameters
        .insert("other".into(), Type::reference("job"));
    let before = recapture(&before);
    let mut bad = after;
    bad.program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .parameters
        .insert("other".into(), Type::reference("job"));
    let Statement::ForEach { items, .. } = &mut steps(&mut bad)[1] else {
        panic!()
    };
    *items = query("pause", "p", eq(field("p", "job"), var("other")));
    rejected(&before, &bad, "root");
}
#[test]
fn rejects_owner_transfer_archive_and_shared_writes() {
    let (before, after) = pair();
    for statement in [
        Statement::Update {
            record: var("memory"),
            values: [("job".into(), var("row"))].into(),
        },
        Statement::Archive {
            record: var("memory"),
        },
    ] {
        let mut bad = after.clone();
        let Statement::ForEach { steps: inner, .. } = &mut steps(&mut bad)[1] else {
            panic!()
        };
        inner[0] = statement;
        rejected(&before, &bad, "calculate [1, 0]");
    }
    let mut bad = after;
    steps(&mut bad).push(Statement::ForEach {
        items: query("requirement", "r", eq(field("r", "job"), var("row"))),
        binding: "requirement_row".into(),
        steps: vec![Statement::Update {
            record: field("requirement_row", "material"),
            values: [("ready".into(), boolean(true))].into(),
        }],
    });
    rejected(&before, &bad, "shared");
}
#[test]
fn rejects_truncation_changed_order_and_global_business_aggregates() {
    let (before, after) = pair();
    let mut bad = after.clone();
    let Statement::ForEach {
        items: Expr::Query { limit, .. },
        ..
    } = &mut steps(&mut bad)[1]
    else {
        panic!()
    };
    *limit = 1;
    rejected(&before, &bad, "untruncated");
    let mut bad = after.clone();
    let Statement::ForEach {
        items: Expr::Query { sort, .. },
        ..
    } = &mut steps(&mut bad)[1]
    else {
        panic!()
    };
    sort.push(SortKey {
        value: field("p", "pending"),
        descending: false,
    });
    rejected(&before, &bad, "ordering");
    let mut bad = after;
    let Statement::Update { values, .. } = &mut steps(&mut bad)[0] else {
        panic!()
    };
    values.insert(
        "production".into(),
        count(query(
            "job",
            "foreign",
            eq(field("foreign", "production"), int(1)),
        )),
    );
    rejected(&before, &bad, "global");
}
#[test]
fn rejects_auxiliary_schema_reuse_and_invalid_owner_schema() {
    let (before, after) = pair();
    let dir = fixture::tempdir();
    let source = ProductStore::create(dir.path().join("source"), &before, 20000).unwrap();
    let mut snapshot = source.load().unwrap();
    snapshot
        .data
        .schema
        .push(after.program.entities.last().unwrap().clone());
    let retained = ProductStore::create_recovered(dir.path().join("retained"), &snapshot).unwrap();
    let error = retained
        .analyze_owned_effects(&after, &request(&retained, &before, &after))
        .unwrap_err()
        .to_string();
    assert!(error.contains("retained schema"), "{error}");
    let mut bad = after;
    bad.program.entities.last_mut().unwrap().unique = vec![vec!["pending".into()]];
    rejected(&before, &bad, "owner-only");
}
#[test]
fn rejects_stale_basis_and_uncovered_or_escaping_bindings() {
    let (before, after) = pair();
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let mut req = request(&store, &before, &after);
    req.snapshot = canonical_digest(IdentityDomain::Evidence, &"forged").unwrap();
    assert!(store
        .analyze_owned_effects(&after, &req)
        .unwrap_err()
        .to_string()
        .contains("snapshot"));
    let mut req = request(&store, &before, &after);
    req.sources.edits.clear();
    assert!(store.analyze_owned_effects(&after, &req).is_err());
    let mut bad = after;
    steps(&mut bad).push(Statement::Create {
        entity: "pause".into(),
        bind: "escape".into(),
        values: [
            ("job".into(), var("row")),
            ("pending".into(), boolean(true)),
        ]
        .into(),
    });
    bad.program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .ensures
        .push(field("escape", "pending"));
    // Structural headers and escaped binders are never bypassed by safety DTOs.
    rejected(&before, &bad, "escapes");
}
#[test]
fn rejects_foreign_dependency_in_unchanged_sink() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        source.program.actions.push(ActionDefinition {
            id: "leak".into(),
            label: "Leak".into(),
            parameters: [
                ("row".into(), Type::reference("job")),
                ("other".into(), Type::reference("job")),
            ]
            .into(),
            guards: vec![],
            steps: vec![Statement::Update {
                record: var("other"),
                values: [("waited".into(), field("row", "production"))].into(),
            }],
            ensures: vec![],
        });
    }
    rejected(&recapture(&before), &after, "leak [0]");
}
#[test]
fn rejects_truncated_unchanged_outer_selection_before_gating_bodies() {
    let (mut before, mut after) = pair();
    for (source, value) in [(&mut before, 0), (&mut after, 1)] {
        let mut items = query(
            "job",
            "selection",
            eq(field("selection", "production"), int(1)),
        );
        let Expr::Query { limit, .. } = &mut items else {
            panic!()
        };
        *limit = 1;
        *steps(source) = vec![Statement::ForEach {
            items,
            binding: "selected".into(),
            steps: vec![Statement::Update {
                record: var("selected"),
                values: [("production".into(), int(value))].into(),
            }],
        }];
    }
    let before = recapture(&before);
    let after = recapture(&after);
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let mut req = request(&store, &before, &after);
    let edit = &mut req.sources.edits[0];
    for (source, endpoint) in [(&before, &mut edit.before), (&after, &mut edit.after)] {
        let Statement::ForEach { steps, .. } = &source
            .program
            .actions
            .iter()
            .find(|a| a.id == "calculate")
            .unwrap()
            .steps[0]
        else {
            panic!()
        };
        endpoint.list_path = vec![0];
        endpoint.len = 1;
        endpoint.kinds = vec![OwnedStatementKind::Update];
        endpoint.digest =
            canonical_digest(IdentityDomain::Evidence, &("owned-span/1", steps)).unwrap();
        endpoint.previous = None;
        endpoint.next = None;
        endpoint.root = Some(OwnedRootBinding {
            name: "selected".into(),
            value_type: Type::reference("job"),
        });
    }
    // A limit-1 row leaving selection would otherwise admit a different root.
    let error = store
        .analyze_owned_effects(&after, &req)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("untruncated") && error.contains("calculate [0]"),
        "{error}"
    );
}
#[test]
fn rejects_transitive_and_guard_control_dependencies_on_shared_sinks() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        source.program.entities[0].fields.push(FieldDefinition {
            id: "intermediate".into(),
            label: "Intermediate".into(),
            value_type: Type::Integer,
        });
        let Statement::Create { values, .. } = &mut source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap()
            .steps[0]
        else {
            panic!()
        };
        values.insert("intermediate".into(), int(0));
        source.program.actions.push(ActionDefinition {
            id: "propagate".into(),
            label: "Propagate".into(),
            parameters: [("row".into(), Type::reference("job"))].into(),
            guards: vec![],
            steps: vec![Statement::Update {
                record: var("row"),
                values: [("intermediate".into(), field("row", "production"))].into(),
            }],
            ensures: vec![],
        });
        source.program.actions.push(ActionDefinition {
            id: "leak".into(),
            label: "Leak".into(),
            parameters: [
                ("row".into(), Type::reference("job")),
                ("material".into(), Type::reference("material")),
            ]
            .into(),
            guards: vec![eq(field("row", "intermediate"), int(1))],
            steps: vec![Statement::Update {
                record: var("material"),
                values: [("ready".into(), boolean(true))].into(),
            }],
            ensures: vec![],
        });
    }
    rejected(&recapture(&before), &after, "leak [0]");
}
#[test]
fn accepts_exact_root_alias_and_rejects_unrelated_same_entity_alias() {
    let (before, mut after) = pair();
    steps(&mut after).push(Statement::ForEach {
        items: query("job", "alias_match", eq(var("alias_match"), var("row"))),
        binding: "alias".into(),
        steps: vec![Statement::Create {
            entity: "pause".into(),
            bind: "created".into(),
            values: [
                ("job".into(), var("alias")),
                ("pending".into(), boolean(true)),
            ]
            .into(),
        }],
    });
    assert!(verify(&before, &recapture(&after)).is_ok());
    let Statement::ForEach { items, .. } = &mut steps(&mut after)[2] else {
        panic!()
    };
    *items = query("job", "alias_match", boolean(true));
    rejected(&before, &after, "root");
}

#[test]
fn supports_row_projections_and_partitioned_counts_but_rejects_global_observation_shapes() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        let Expr::Count { items } = &mut source.program.observables[0].value else {
            panic!()
        };
        let Expr::Query { limit, .. } = items.as_mut() else {
            panic!()
        };
        *limit = MAX_COLLECTION;
    }
    let before = recapture(&before);
    let ViewKind::List { columns, .. } = &mut after.program.views[0].kind else {
        panic!()
    };
    columns[1].value = count(query(
        "pause",
        "visible_memory",
        eq(field("visible_memory", "job"), var("row")),
    ));
    after.program.observables[0].value = count(query(
        "job",
        "counted",
        eq(field("counted", "production"), int(1)),
    ));
    assert!(verify(&before, &recapture(&after)).is_ok());
    after.program.observables[0].value = Expr::Add {
        left: Box::new(after.program.observables[0].value.clone()),
        right: Box::new(int(1)),
    };
    rejected(&before, &after, "Count(Query)");
}

#[test]
fn rejects_filter_wrapped_truncation_and_changed_sort_in_unchanged_sink() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        let mut items = query("job", "chosen", boolean(true));
        let Expr::Query { limit, .. } = &mut items else {
            panic!()
        };
        *limit = 1;
        source.program.actions.push(ActionDefinition {
            id: "filtered".into(),
            label: "Filtered".into(),
            parameters: Default::default(),
            guards: vec![],
            steps: vec![Statement::ForEach {
                items: Expr::Filter {
                    items: Box::new(items),
                    binding: "candidate".into(),
                    predicate: Box::new(eq(field("candidate", "production"), int(1))),
                },
                binding: "selected".into(),
                steps: vec![Statement::Update {
                    record: var("selected"),
                    values: [("waited".into(), int(1))].into(),
                }],
            }],
            ensures: vec![],
        });
    }
    rejected(&recapture(&before), &after, "untruncated");
}

#[test]
fn distinguishes_same_named_fields_on_shared_entities_and_requires_typed_owner() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        source
            .program
            .entities
            .iter_mut()
            .find(|e| e.id == "material")
            .unwrap()
            .fields
            .push(FieldDefinition {
                id: "production".into(),
                label: "Unrelated capacity".into(),
                value_type: Type::Integer,
            });
        source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "calculate")
            .unwrap()
            .parameters
            .insert("material".into(), Type::reference("material"));
    }
    let Statement::Update { values, .. } = &mut steps(&mut after)[0] else {
        panic!()
    };
    values.insert("production".into(), field("material", "production"));
    let before = recapture(&before);
    let after = recapture(&after);
    assert!(verify(&before, &after).is_ok());
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let mut req = request(&store, &before, &after);
    req.auxiliary[0].owner_field = "pending".into();
    assert!(store
        .analyze_owned_effects(&after, &req)
        .unwrap_err()
        .to_string()
        .contains("required typed root reference"));
}

#[test]
fn rejects_auxiliary_reads_correlated_by_a_nonowner_root_reference() {
    let (before, mut after) = pair();
    after
        .program
        .entities
        .last_mut()
        .unwrap()
        .fields
        .push(FieldDefinition {
            id: "other_job".into(),
            label: "Another job".into(),
            value_type: Type::reference("job"),
        });
    let Statement::Update { values, .. } = &mut steps(&mut after)[0] else {
        panic!()
    };
    values.insert(
        "production".into(),
        count(query(
            "pause",
            "foreign_memory",
            eq(field("foreign_memory", "other_job"), var("row")),
        )),
    );
    rejected(&before, &after, "owner");
}

#[test]
fn rejects_changed_outer_selection_even_with_exact_binders_and_untruncated_limits() {
    let (mut before, mut after) = pair();
    for (source, predicate) in [
        (&mut before, boolean(true)),
        (
            &mut after,
            eq(field("selected_query", "production"), int(1)),
        ),
    ] {
        let action = source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "calculate")
            .unwrap();
        action.parameters.clear();
        action.steps = vec![Statement::ForEach {
            items: query("job", "selected_query", predicate),
            binding: "selected".into(),
            steps: vec![Statement::Update {
                record: var("selected"),
                values: [("production".into(), int(1))].into(),
            }],
        }];
    }
    let before = recapture(&before);
    let after = recapture(&after);
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
    let mut req = request(&store, &before, &after);
    req.sources.edits[0].before.root = None;
    req.sources.edits[0].after.root = None;
    let error = store
        .analyze_owned_effects(&after, &req)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("selection") && error.contains("calculate"),
        "{error}"
    );
}

#[test]
fn rejects_root_to_root_relations_as_partition_proof() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        source.program.entities[0].fields.push(FieldDefinition {
            id: "parent".into(),
            label: "Another root".into(),
            value_type: Type::reference("job"),
        });
        let action = source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap();
        action
            .parameters
            .insert("parent".into(), Type::reference("job"));
        let Statement::Create { values, .. } = &mut action.steps[0] else {
            panic!()
        };
        values.insert("parent".into(), var("parent"));
    }
    let Statement::Update { values, .. } = &mut steps(&mut after)[0] else {
        panic!()
    };
    values.insert(
        "production".into(),
        count(query(
            "job",
            "child",
            Expr::And {
                values: vec![
                    eq(field("child", "parent"), var("row")),
                    eq(field("child", "production"), int(1)),
                ],
            },
        )),
    );
    rejected(&recapture(&before), &after, "root");
}

#[test]
fn rejects_nonscalar_affected_row_projections() {
    let (before, mut after) = pair();
    let ViewKind::List { columns, .. } = &mut after.program.views[0].kind else {
        panic!()
    };
    columns[1].value = query(
        "pause",
        "memory_row",
        eq(field("memory_row", "job"), var("row")),
    );
    rejected(&before, &after, "scalar");
}

#[test]
fn rejects_implicit_protected_collection_reads_in_unchanged_mutations() {
    let (mut before, mut after) = pair();
    let list_type = Type::List {
        item: Box::new(Type::Integer),
    };
    let empty = fixture::lit(
        DataValue::List {
            item_type: Type::Integer,
            items: vec![],
        },
        list_type.clone(),
    );
    for source in [&mut before, &mut after] {
        source.program.entities[0].fields.push(FieldDefinition {
            id: "tags".into(),
            label: "Tags".into(),
            value_type: list_type.clone(),
        });
        let Statement::Create { values, .. } = &mut source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "add")
            .unwrap()
            .steps[0]
        else {
            panic!()
        };
        values.insert("tags".into(), empty.clone());
        source
            .program
            .actions
            .iter_mut()
            .find(|a| a.id == "calculate")
            .unwrap()
            .parameters
            .insert("tags".into(), list_type.clone());
        source.program.actions.push(ActionDefinition {
            id: "retag".into(),
            label: "Retag".into(),
            parameters: [("row".into(), Type::reference("job"))].into(),
            guards: vec![],
            steps: vec![Statement::Collection {
                target: CollectionTarget::Field {
                    record: var("row"),
                    field: "tags".into(),
                },
                operation: CollectionOperation::Insert,
                items: fixture::lit(
                    DataValue::List {
                        item_type: Type::Integer,
                        items: vec![DataValue::Integer { value: 1 }],
                    },
                    list_type.clone(),
                ),
            }],
            ensures: vec![],
        });
    }
    let Statement::Update { values, .. } = &mut steps(&mut after)[0] else {
        panic!()
    };
    values.insert("tags".into(), var("tags"));
    for operation in [CollectionOperation::Insert, CollectionOperation::Remove] {
        for source in [&mut before, &mut after] {
            let Statement::Collection { operation: op, .. } =
                &mut source.program.actions.last_mut().unwrap().steps[0]
            else {
                panic!()
            };
            *op = operation;
        }
        rejected(&recapture(&before), &after, "retag [0]");
    }
}

#[test]
fn rejects_affected_common_query_ordering_by_unprotected_fields() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        let mut items = query(
            "job",
            "selected_query",
            eq(field("selected_query", "production"), int(1)),
        );
        let Expr::Query { sort, .. } = &mut items else {
            panic!()
        };
        sort.push(SortKey {
            value: field("selected_query", "name"),
            descending: false,
        });
        source.program.actions.push(ActionDefinition {
            id: "ordered".into(),
            label: "Ordered".into(),
            parameters: Default::default(),
            guards: vec![],
            steps: vec![Statement::ForEach {
                items,
                binding: "selected".into(),
                steps: vec![Statement::Update {
                    record: var("selected"),
                    values: [("production".into(), int(2))].into(),
                }],
            }],
            ensures: vec![],
        });
    }
    rejected(&recapture(&before), &after, "ordering");
}

#[test]
fn rejects_mutated_correlation_keys_before_reusing_root_provenance() {
    for mutation_target in [var("link"), var("retarget")] {
        let (mut before, mut after) = pair();
        for (source, value) in [(&mut before, 0), (&mut after, 1)] {
            let action = source
                .program
                .actions
                .iter_mut()
                .find(|a| a.id == "calculate")
                .unwrap();
            action
                .parameters
                .insert("other".into(), Type::reference("job"));
            action
                .parameters
                .insert("retarget".into(), Type::reference("requirement"));
            action.steps = vec![Statement::ForEach {
                items: query(
                    "requirement",
                    "related",
                    eq(field("related", "job"), var("row")),
                ),
                binding: "link".into(),
                steps: vec![
                    Statement::Update {
                        record: mutation_target.clone(),
                        values: [("job".into(), var("other"))].into(),
                    },
                    Statement::Update {
                        record: field("link", "job"),
                        values: [("production".into(), int(value))].into(),
                    },
                ],
            }];
        }
        let before = recapture(&before);
        let after = recapture(&after);
        let dir = fixture::tempdir();
        let store = ProductStore::create(dir.path().join("project"), &before, 20000).unwrap();
        let mut req = request(&store, &before, &after);
        let edit = &mut req.sources.edits[0];
        for (source, endpoint) in [(&before, &mut edit.before), (&after, &mut edit.after)] {
            let Statement::ForEach { steps, .. } = &source
                .program
                .actions
                .iter()
                .find(|a| a.id == "calculate")
                .unwrap()
                .steps[0]
            else {
                panic!()
            };
            endpoint.list_path = vec![0];
            endpoint.start = 1;
            endpoint.len = 1;
            endpoint.kinds = vec![OwnedStatementKind::Update];
            endpoint.digest =
                canonical_digest(IdentityDomain::Evidence, &("owned-span/1", &steps[1..2]))
                    .unwrap();
            endpoint.previous =
                Some(canonical_digest(IdentityDomain::Evidence, &steps[0]).unwrap());
            endpoint.next = None;
        }
        let error = store
            .analyze_owned_effects(&after, &req)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("correlation") && error.contains("calculate [0, 0]"),
            "{error}"
        );
    }
}

#[test]
fn rejects_dependent_detail_record_selection_with_constant_columns() {
    let (mut before, mut after) = pair();
    for source in [&mut before, &mut after] {
        source.program.views.push(ViewDefinition {
            id: "detail".into(),
            label: "Detail".into(),
            kind: ViewKind::Detail {
                entity: "job".into(),
                record: Expr::If {
                    condition: Box::new(eq(
                        count(query(
                            "job",
                            "candidate",
                            eq(field("candidate", "production"), int(1)),
                        )),
                        int(0),
                    )),
                    then_value: Box::new(fixture::lit(
                        DataValue::Reference {
                            entity: "job".into(),
                            record: "job_a".into(),
                        },
                        Type::reference("job"),
                    )),
                    else_value: Box::new(fixture::lit(
                        DataValue::Reference {
                            entity: "job".into(),
                            record: "job_b".into(),
                        },
                        Type::reference("job"),
                    )),
                },
                columns: vec![Column {
                    id: "constant".into(),
                    label: "Constant".into(),
                    value: int(1),
                }],
            },
            actions: vec![],
            keys: vec![],
        });
    }
    rejected(&recapture(&before), &after, "view detail record");
}
