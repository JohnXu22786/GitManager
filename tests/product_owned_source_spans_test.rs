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
use product_store::scope::*;
use serde_json::json;

fn recapture(source: &CapturedProgram) -> CapturedProgram {
    capture(serde_json::to_value(&source.program).unwrap())
}
fn source_id(source: &CapturedProgram) -> Digest {
    canonical_digest(IdentityDomain::Source, source).unwrap()
}
fn query(binding: &str) -> Expr {
    Expr::Query {
        entity: "job".into(),
        binding: binding.into(),
        predicate: Box::new(boolean(true)),
        sort: vec![],
        limit: MAX_COLLECTION,
        include_archived: false,
    }
}
fn endpoint(
    source: &CapturedProgram,
    list_path: &[usize],
    start: usize,
    len: usize,
) -> OwnedSpanEndpoint {
    let mut steps = source
        .program
        .actions
        .iter()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps
        .as_slice();
    for index in list_path {
        let Statement::ForEach { steps: nested, .. } = &steps[*index] else {
            panic!()
        };
        steps = nested;
    }
    OwnedSpanEndpoint {
        source: source_id(source),
        action: "calculate".into(),
        list_path: list_path.into(),
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
        root: (!list_path.is_empty()).then(|| OwnedRootBinding {
            name: "sync".into(),
            value_type: Type::reference("job"),
        }),
    }
}
fn fixture_pair() -> (CapturedProgram, CapturedProgram, OwnedSourceRequest) {
    let mut before = program(false);
    let action = before
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps = vec![
        Statement::Assert {
            condition: boolean(true),
            message: "Before common event".into(),
        },
        Statement::Assert {
            condition: boolean(true),
            message: "Common event".into(),
        },
        Statement::ForEach {
            items: query("selected"),
            binding: "sync".into(),
            steps: vec![Statement::Update {
                record: var("sync"),
                values: [("production".into(), int(0))].into(),
            }],
        },
        Statement::Assert {
            condition: boolean(true),
            message: "After synchronization".into(),
        },
    ];
    before = recapture(&before);
    let mut after = before.clone();
    after.program.entities.push(
        serde_json::from_value(json!({"id":"pause","label":"Pause memory","fields":[
        {"id":"job","label":"Owner","value_type":{"kind":"reference","entity":"job"}},
        {"id":"pending","label":"Pending","value_type":{"kind":"boolean"}}
    ],"unique":[["job"]],"constraints":[]}))
        .unwrap(),
    );
    let action = after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps.insert(
        0,
        Statement::ForEach {
            items: query("capture_selection"),
            binding: "capture".into(),
            steps: vec![Statement::Create {
                entity: "pause".into(),
                bind: "new_pause".into(),
                values: [
                    ("job".into(), var("capture")),
                    ("pending".into(), boolean(true)),
                ]
                .into(),
            }],
        },
    );
    let Statement::ForEach { steps, .. } = &mut action.steps[3] else {
        panic!()
    };
    steps[0] = Statement::Update {
        record: var("sync"),
        values: [("production".into(), int(1))].into(),
    };
    let ViewKind::List { columns, .. } = &mut after.program.views[0].kind else {
        panic!()
    };
    columns[1].value = int(1);
    columns.push(Column {
        id: "pending".into(),
        label: "Pending".into(),
        value: boolean(false),
    });
    after.program.observables[0].value = int(1);
    after.program.observables.push(ObservableDefinition {
        id: "pending_count".into(),
        label: "Pending count".into(),
        value: int(0),
    });
    after = recapture(&after);
    let request = OwnedSourceRequest {
        before: source_id(&before),
        after: source_id(&after),
        root_entity: "job".into(),
        edits: vec![
            OwnedSpanEdit {
                id: "capture".into(),
                before: endpoint(&before, &[], 0, 0),
                after: endpoint(&after, &[], 0, 1),
            },
            OwnedSpanEdit {
                id: "synchronize".into(),
                before: endpoint(&before, &[2], 0, 1),
                after: endpoint(&after, &[3], 0, 1),
            },
        ],
    };
    (before, after, request)
}

#[test]
fn resolves_moved_span_and_anchored_gap_without_granting_execution() {
    let (before, after, request) = fixture_pair();
    let report = analyze_owned_sources(&before, &after, &request).unwrap();
    assert_eq!(report.edits, request.edits);
    assert_eq!(
        report
            .common_statements
            .iter()
            .filter(|s| s.action == "calculate")
            .map(|s| (s.before.clone(), s.after.clone()))
            .collect::<Vec<_>>(),
        vec![
            (vec![0], vec![1]),
            (vec![1], vec![2]),
            (vec![2], vec![3]),
            (vec![3], vec![4])
        ]
    );
    assert_eq!(
        report
            .writers
            .iter()
            .filter(|w| w.entity == "job"
                && w.field.as_deref() == Some("production")
                && w.side == OwnedSourceSide::Before)
            .count(),
        3
    );
    assert!(report
        .writers
        .iter()
        .any(|w| w.action == "complete" && w.field.as_deref() == Some("production")));
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::EntityAdded)
            .count(),
        1
    );
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::ColumnAdded)
            .count(),
        1
    );
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::ObservableAdded)
            .count(),
        1
    );
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::ColumnChanged)
            .count(),
        1
    );
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::ObservableChanged)
            .count(),
        1
    );
    assert_eq!(
        serde_json::to_vec(&report).unwrap(),
        serde_json::to_vec(&analyze_owned_sources(&before, &after, &request).unwrap()).unwrap()
    );
}

#[test]
fn rejects_stale_source_path_kind_digest_anchor_and_root_type() {
    let (before, after, request) = fixture_pair();
    for case in 0..7 {
        let mut bad = request.clone();
        match case {
            0 => bad.before = bad.after.clone(),
            1 => bad.edits[1].before.list_path = vec![1],
            2 => bad.edits[1].before.kinds = vec![OwnedStatementKind::Create],
            3 => bad.edits[1].before.digest = bad.after.clone(),
            4 => bad.edits[0].before.next = None,
            5 => bad.edits[1].before.root.as_mut().unwrap().value_type = Type::Integer,
            6 => bad.edits[1].before.source = bad.after.clone(),
            _ => unreachable!(),
        }
        assert!(
            analyze_owned_sources(&before, &after, &bad).is_err(),
            "accepted malformed endpoint case {case}"
        );
    }
    let mut path_changed = before.clone();
    path_changed = path_changed.at_path("other-source.json").unwrap();
    assert!(analyze_owned_sources(&path_changed, &after, &request).is_err());
}

#[test]
fn rejects_missing_duplicate_overlapping_and_reordered_edits() {
    let (before, after, request) = fixture_pair();
    let mut missing = request.clone();
    missing.edits.pop();
    assert!(analyze_owned_sources(&before, &after, &missing).is_err());
    let mut duplicate = request.clone();
    duplicate.edits.push(duplicate.edits[0].clone());
    assert!(analyze_owned_sources(&before, &after, &duplicate).is_err());
    let mut overlapping = request.clone();
    overlapping.edits.push(OwnedSpanEdit {
        id: "overlap".into(),
        before: endpoint(&before, &[], 2, 1),
        after: endpoint(&after, &[], 3, 1),
    });
    assert!(analyze_owned_sources(&before, &after, &overlapping).is_err());
    let mut reordered = after.clone();
    reordered
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps
        .swap(1, 2);
    reordered = recapture(&reordered);
    let mut changed_request = request.clone();
    changed_request.after = source_id(&reordered);
    changed_request.edits[0].after = endpoint(&reordered, &[], 0, 1);
    changed_request.edits[1].after = endpoint(&reordered, &[3], 0, 1);
    assert!(analyze_owned_sources(&before, &reordered, &changed_request).is_err());
}

#[test]
fn rejects_escaping_create_binding_and_changed_author_binder() {
    let (before, mut after, mut request) = fixture_pair();
    let action = after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps[0] = Statement::Create {
        entity: "pause".into(),
        bind: "created".into(),
        values: [
            ("job".into(), var("row")),
            ("pending".into(), boolean(true)),
        ]
        .into(),
    };
    action.steps[1] = Statement::Assert {
        condition: field("created", "pending"),
        message: "Escaped".into(),
    };
    after = recapture(&after);
    request.after = source_id(&after);
    request.edits[0].after = endpoint(&after, &[], 0, 1);
    request.edits[1].after = endpoint(&after, &[3], 0, 1);
    let error = analyze_owned_sources(&before, &after, &request)
        .unwrap_err()
        .to_string();
    assert!(error.contains("escapes"), "{error}");
    let (before, mut after, mut request) = fixture_pair();
    let action = after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    let Statement::ForEach { binding, steps, .. } = &mut action.steps[3] else {
        panic!()
    };
    *binding = "renamed".into();
    let Statement::Update { record, .. } = &mut steps[0] else {
        panic!()
    };
    *record = var("renamed");
    after = recapture(&after);
    request.after = source_id(&after);
    request.edits[0].after = endpoint(&after, &[], 0, 1);
    request.edits[1].after = endpoint(&after, &[3], 0, 1);
    request.edits[1].after.root.as_mut().unwrap().name = "renamed".into();
    assert!(analyze_owned_sources(&before, &after, &request)
        .unwrap_err()
        .to_string()
        .contains("binder"));
}

#[test]
fn rejects_unclassified_shell_changes_and_preserves_v1_request_bytes() {
    let (before, mut after, mut request) = fixture_pair();
    after.program.actions[0].guards.push(boolean(false));
    after = recapture(&after);
    request.after = source_id(&after);
    request.edits[0].after = endpoint(&after, &[], 0, 1);
    request.edits[1].after = endpoint(&after, &[3], 0, 1);
    assert!(analyze_owned_sources(&before, &after, &request).is_err());
    let value = json!({"population":{"kind":"all"},"operations":["calculate"],"excluded_records":[],"lifecycles":[],"patches":[]});
    let old: ScopeRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), value);
    let mut unknown = serde_json::to_value(request).unwrap();
    unknown["prepared"] = json!(true);
    assert!(serde_json::from_value::<OwnedSourceRequest>(unknown).is_err());
}

#[test]
fn resolves_removal_gaps_and_rejects_gap_ambiguity_and_overflow() {
    let (before, after, request) = fixture_pair();
    // Invert the insertion in sources without passive additions, which the
    // initial structural grammar only accepts in the forward direction.
    let mut inserted = before.clone();
    inserted
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps
        .insert(
            0,
            Statement::Assert {
                condition: boolean(true),
                message: "Removed variant".into(),
            },
        );
    inserted = recapture(&inserted);
    let removal = OwnedSourceRequest {
        before: source_id(&inserted),
        after: source_id(&before),
        root_entity: "job".into(),
        edits: vec![OwnedSpanEdit {
            id: "remove".into(),
            before: endpoint(&inserted, &[], 0, 1),
            after: endpoint(&before, &[], 0, 0),
        }],
    };
    assert!(analyze_owned_sources(&inserted, &before, &removal).is_ok());
    let mut two_gaps = request.clone();
    let mut duplicate = two_gaps.edits[0].clone();
    duplicate.id = "second_gap".into();
    two_gaps.edits.push(duplicate);
    assert!(analyze_owned_sources(&before, &after, &two_gaps).is_err());
    let mut overflow = request;
    overflow.edits[0].before.start = usize::MAX;
    overflow.edits[0].before.len = 1;
    assert!(analyze_owned_sources(&before, &after, &overflow).is_err());
}

#[test]
fn writer_inventory_resolves_alias_types_and_implicit_optional_creates() {
    let mut source = program(false);
    source.program.entities[0].fields.push(FieldDefinition {
        id: "memo".into(),
        label: "Memo".into(),
        value_type: Type::Optional {
            item: Box::new(Type::Text),
        },
    });
    source.program.entities.push(EntityDefinition {
        id: "link".into(),
        label: "Link".into(),
        fields: vec![FieldDefinition {
            id: "owner".into(),
            label: "Owner".into(),
            value_type: Type::reference("job"),
        }],
        unique: vec![],
        constraints: vec![],
    });
    let action = source
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps = vec![
        Statement::Create {
            entity: "link".into(),
            bind: "link".into(),
            values: [("owner".into(), var("row"))].into(),
        },
        Statement::Update {
            record: field("link", "owner"),
            values: [("production".into(), int(8))].into(),
        },
    ];
    source = recapture(&source);
    let request = OwnedSourceRequest {
        before: source_id(&source),
        after: source_id(&source),
        root_entity: "job".into(),
        edits: vec![],
    };
    let report = analyze_owned_sources(&source, &source, &request).unwrap();
    assert!(report.writers.iter().any(|w| w.action == "calculate"
        && w.entity == "job"
        && w.field.as_deref() == Some("production")
        && w.value_type == Some(Type::Integer)));
    assert!(report.writers.iter().any(|w| w.action == "add"
        && w.entity == "job"
        && w.field.as_deref() == Some("memo")
        && w.value_type
            == Some(Type::Optional {
                item: Box::new(Type::Text)
            })));
}

#[test]
fn rejects_swapped_lexical_binders_even_when_name_sets_match() {
    let (mut before, after_template, _) = fixture_pair();
    before.program.entities = after_template.program.entities.clone();
    let action = before
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    action.steps = vec![
        Statement::ForEach {
            items: query("jobs"),
            binding: "a".into(),
            steps: vec![Statement::Assert {
                condition: Expr::Equal {
                    left: Box::new(field("a", "production")),
                    right: Box::new(int(0)),
                },
                message: "Job".into(),
            }],
        },
        Statement::ForEach {
            items: Expr::Query {
                entity: "pause".into(),
                binding: "pauses".into(),
                predicate: Box::new(boolean(true)),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            },
            binding: "b".into(),
            steps: vec![Statement::Assert {
                condition: field("b", "pending"),
                message: "Pause".into(),
            }],
        },
    ];
    before = recapture(&before);
    let mut after = before.clone();
    let action = after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    let Statement::ForEach { binding, steps, .. } = &mut action.steps[0] else {
        panic!()
    };
    *binding = "b".into();
    steps[0] = Statement::Assert {
        condition: Expr::Equal {
            left: Box::new(field("b", "production")),
            right: Box::new(int(0)),
        },
        message: "Job".into(),
    };
    let Statement::ForEach { binding, steps, .. } = &mut action.steps[1] else {
        panic!()
    };
    *binding = "a".into();
    steps[0] = Statement::Assert {
        condition: field("a", "pending"),
        message: "Pause".into(),
    };
    after = recapture(&after);
    let request = OwnedSourceRequest {
        before: source_id(&before),
        after: source_id(&after),
        root_entity: "job".into(),
        edits: vec![OwnedSpanEdit {
            id: "sibling_loops".into(),
            before: endpoint(&before, &[], 0, 2),
            after: endpoint(&after, &[], 0, 2),
        }],
    };
    assert!(analyze_owned_sources(&before, &after, &request)
        .unwrap_err()
        .to_string()
        .contains("binder"));
}

#[test]
fn rejects_dropped_reused_expression_binder_occurrences() {
    let mut before = program(false);
    let count = Expr::Count {
        items: Box::new(query("reused")),
    };
    before
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps = vec![Statement::Update {
        record: var("row"),
        values: [(
            "production".into(),
            Expr::Add {
                left: Box::new(count.clone()),
                right: Box::new(count.clone()),
            },
        )]
        .into(),
    }];
    before = recapture(&before);
    let mut after = before.clone();
    after
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps = vec![Statement::Update {
        record: var("row"),
        values: [("production".into(), count)].into(),
    }];
    after = recapture(&after);
    let request = OwnedSourceRequest {
        before: source_id(&before),
        after: source_id(&after),
        root_entity: "job".into(),
        edits: vec![OwnedSpanEdit {
            id: "reused_query".into(),
            before: endpoint(&before, &[], 0, 1),
            after: endpoint(&after, &[], 0, 1),
        }],
    };
    assert!(analyze_owned_sources(&before, &after, &request)
        .unwrap_err()
        .to_string()
        .contains("binder"));
}
