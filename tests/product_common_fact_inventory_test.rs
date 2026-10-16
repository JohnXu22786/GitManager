//! Generic, diagnostic-only fixtures. No inventory grants execution authority.
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
use serde_json::{json, Value};

fn query(entity: &str) -> Expr {
    Expr::Query {
        entity: entity.into(),
        binding: "q".into(),
        predicate: Box::new(boolean(true)),
        sort: vec![],
        limit: MAX_COLLECTION,
        include_archived: false,
    }
}
fn text(s: &str) -> Expr {
    fixture::lit(fixture::text(s), Type::Text)
}
fn eq(a: Expr, b: Expr) -> Expr {
    Expr::Equal {
        left: Box::new(a),
        right: Box::new(b),
    }
}
fn action(
    id: &str,
    parameters: std::collections::BTreeMap<Id, Type>,
    steps: Vec<Statement>,
) -> ActionDefinition {
    ActionDefinition {
        id: id.into(),
        label: id.into(),
        parameters,
        guards: vec![],
        steps,
        ensures: vec![],
    }
}
fn pair() -> (CapturedProgram, CapturedProgram) {
    let mut before = fixture::program(false).program;
    before
        .actions
        .retain(|a| ["add", "calculate", "complete"].contains(&a.id.as_str()));
    before
        .actions
        .iter_mut()
        .find(|a| a.id == "complete")
        .unwrap()
        .guards = vec![eq(field("row", "production"), int(1))];
    before.outputs.clear();
    before.observables.clear();
    before.entities.push(serde_json::from_value(json!({"id":"material","label":"Material","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}},{"id":"ready","label":"Ready","value_type":{"kind":"boolean"}}],"unique":[["name"]],"constraints":[]})).unwrap());
    before.entities.push(serde_json::from_value(json!({"id":"requirement","label":"Requirement","fields":[{"id":"job","label":"Owner","value_type":{"kind":"reference","entity":"job"}},{"id":"material","label":"Material","value_type":{"kind":"reference","entity":"material"}}],"unique":[["job","material"]],"constraints":[]})).unwrap());
    before.state.push(StateDefinition {
        id: "input".into(),
        label: "Input".into(),
        value_type: Type::Text,
        initial: fixture::text(""),
    });
    let a = before
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap();
    a.parameters.insert("name".into(), Type::Text);
    a.guards = vec![Expr::Not {
        value: Box::new(field("row", "done")),
    }];
    a.steps = vec![
        Statement::Create {
            entity: "material".into(),
            bind: "made".into(),
            values: [
                ("name".into(), var("name")),
                ("ready".into(), boolean(false)),
            ]
            .into(),
        },
        Statement::ForEach {
            items: query("job"),
            binding: "sync".into(),
            steps: vec![Statement::Update {
                record: var("sync"),
                values: [("production".into(), int(0))].into(),
            }],
        },
        Statement::SetState {
            state: "input".into(),
            value: text(""),
        },
    ];
    before.actions.push(action(
        "create_material",
        [("name".into(), Type::Text)].into(),
        vec![Statement::Create {
            entity: "material".into(),
            bind: "made".into(),
            values: [
                ("name".into(), var("name")),
                ("ready".into(), boolean(false)),
            ]
            .into(),
        }],
    ));
    before.actions.push(action(
        "link",
        [
            ("row".into(), Type::reference("job")),
            ("material".into(), Type::reference("material")),
        ]
        .into(),
        vec![Statement::Create {
            entity: "requirement".into(),
            bind: "link".into(),
            values: [
                ("job".into(), var("row")),
                ("material".into(), var("material")),
            ]
            .into(),
        }],
    ));
    before.views[0].actions = vec![ActionBinding {
        id: "calculate".into(),
        label: "Calculate".into(),
        placement: ActionPlacement::Row,
        action: "calculate".into(),
        arguments: [
            ("row".into(), var("row")),
            (
                "name".into(),
                Expr::State {
                    state: "input".into(),
                },
            ),
        ]
        .into(),
        enabled: boolean(true),
    }];
    let mut after = before.clone();
    let Statement::ForEach { steps, .. } = &mut after
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps[1]
    else {
        panic!()
    };
    steps[0] = Statement::Update {
        record: var("sync"),
        values: [("production".into(), int(1))].into(),
    };
    (
        capture(serde_json::to_value(before).unwrap()),
        capture(serde_json::to_value(after).unwrap()),
    )
}
fn recapture(p: &CapturedProgram) -> CapturedProgram {
    capture(serde_json::to_value(&p.program).unwrap())
}
fn endpoint(
    p: &CapturedProgram,
    action: &str,
    path: &[usize],
    start: usize,
    len: usize,
) -> OwnedSpanEndpoint {
    let mut steps = p
        .program
        .actions
        .iter()
        .find(|a| a.id == action)
        .unwrap()
        .steps
        .as_slice();
    for i in path {
        let Statement::ForEach { steps: inner, .. } = &steps[*i] else {
            panic!()
        };
        steps = inner;
    }
    OwnedSpanEndpoint {
        source: canonical_digest(IdentityDomain::Source, p).unwrap(),
        action: action.into(),
        list_path: path.into(),
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
        root: (!path.is_empty()).then(|| OwnedRootBinding {
            name: "sync".into(),
            value_type: Type::reference("job"),
        }),
    }
}
fn request(
    store: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
) -> CommonFactInventoryRequest {
    CommonFactInventoryRequest {
        snapshot: canonical_digest(IdentityDomain::Data, &store.load().unwrap()).unwrap(),
        sources: OwnedSourceRequest {
            before: canonical_digest(IdentityDomain::Source, a).unwrap(),
            after: canonical_digest(IdentityDomain::Source, b).unwrap(),
            root_entity: "job".into(),
            edits: vec![OwnedSpanEdit {
                id: "sync".into(),
                before: endpoint(a, "calculate", &[1], 0, 1),
                after: endpoint(b, "calculate", &[1], 0, 1),
            }],
        },
        lifecycle: CommonFactLifecycleSuggestion {
            entity: "job".into(),
            before: field("record", "done"),
            after: field("record", "done"),
            meaning: "Suggested completed truth; host acceptance remains unproved".into(),
        },
    }
}
fn inventory(a: &CapturedProgram, b: &CapturedProgram) -> CommonFactInventory {
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), a, 20000).unwrap();
    let r = request(&store, a, b);
    let snapshot = store.load().unwrap();
    let prior = store
        .analyze_owned_effects(
            b,
            &OwnedEffectRequest {
                snapshot: r.snapshot.clone(),
                sources: r.sources.clone(),
                auxiliary: vec![],
            },
        )
        .unwrap_err()
        .to_string();
    let report = store.inventory_common_fact_phases(b, &r).unwrap();
    assert_eq!(store.load().unwrap(), snapshot);
    assert_eq!(
        prior,
        store
            .analyze_owned_effects(
                b,
                &OwnedEffectRequest {
                    snapshot: r.snapshot,
                    sources: r.sources,
                    auxiliary: vec![]
                }
            )
            .unwrap_err()
            .to_string()
    );
    report
}
fn has(report: &CommonFactInventory, code: &str, location: &str) -> bool {
    report
        .obligations
        .iter()
        .any(|o| o.code == code && o.location.contains(location))
}
fn uses(report: &CommonFactInventory, needle: &str) -> Vec<Value> {
    report
        .uses
        .iter()
        .filter(|u| u.location.contains(needle))
        .map(|u| serde_json::to_value(u).unwrap())
        .collect()
}
#[test]
fn inventory_preserves_order_and_does_not_promote_variant_headers() {
    let (a, b) = pair();
    let r = inventory(&a, &b);
    let frame = r.actions.iter().find(|a| a.action == "calculate").unwrap();
    assert_eq!(
        frame.phases.iter().map(|p| p.kind).collect::<Vec<_>>(),
        vec![
            CommonFactPhaseKind::CommonSubtree,
            CommonFactPhaseKind::CommonHeader,
            CommonFactPhaseKind::Variant,
            CommonFactPhaseKind::CommonSubtree
        ]
    );
    assert_eq!(frame.phases[1].before.path, vec![1]);
    assert_ne!(frame.phases[1].before.digest, frame.phases[1].after.digest);
    assert_eq!(
        frame.phases[2].before.environment["sync"],
        Type::reference("job")
    );
    assert_eq!(frame.phases[0].before.digest, frame.phases[0].after.digest);
    assert!(has(&r, "shared_effect", "calculate [0]"));
    assert!(has(&r, "admission_reachability", "calculate guard[0]"));
    assert!(has(&r, "reset_closure", "calculate [2]"));
}
#[test]
fn inventory_rejects_stale_basis_source_and_omitted_correspondence() {
    let (a, b) = pair();
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &a, 20000).unwrap();
    let r = request(&store, &a, &b);
    let mut stale = r.clone();
    stale.snapshot = canonical_digest(IdentityDomain::Data, &"stale").unwrap();
    assert!(store.inventory_common_fact_phases(&b, &stale).is_err());
    let mut stale = r.clone();
    stale.sources.after = stale.sources.before.clone();
    assert!(store.inventory_common_fact_phases(&b, &stale).is_err());
    let mut missing = r.clone();
    missing.sources.edits.clear();
    assert!(store.inventory_common_fact_phases(&b, &missing).is_err());
    let mut wrong = r.clone();
    wrong.sources.edits[0].after.list_path = vec![];
    assert!(store.inventory_common_fact_phases(&b, &wrong).is_err());
}
#[test]
fn inventory_includes_hidden_producers_root_membership_and_legacy_values() {
    let (mut a, mut b) = pair();
    for p in [&mut a, &mut b] {
        p.program.actions.push(action(
            "hidden_material",
            [("name".into(), Type::Text)].into(),
            vec![Statement::Create {
                entity: "material".into(),
                bind: "made".into(),
                values: [
                    ("name".into(), var("name")),
                    ("ready".into(), boolean(false)),
                ]
                .into(),
            }],
        ));
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    assert_eq!(
        r.structure
            .writers
            .iter()
            .filter(|w| w.side == OwnedSourceSide::Before
                && w.entity == "material"
                && w.field.as_deref() == Some("name"))
            .count(),
        3
    );
    assert!(!uses(&r, "hidden_material parameter name").is_empty());
    assert!(!uses(&r, "add [0].value.name").is_empty());
    assert!(uses(&r, "calculate [1].items")
        .iter()
        .any(|u| u["kind"] == "query" && u["entity"] == "job"));
}
#[test]
fn inventory_tracks_mutable_owner_alias_and_retained_uniqueness() {
    let (mut a, mut b) = pair();
    let mutation = action(
        "retarget",
        [("other".into(), Type::reference("job"))].into(),
        vec![Statement::ForEach {
            items: query("requirement"),
            binding: "alias".into(),
            steps: vec![Statement::Update {
                record: var("alias"),
                values: [("job".into(), var("other"))].into(),
            }],
        }],
    );
    for p in [&mut a, &mut b] {
        p.program.actions.push(mutation.clone());
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    let owner = r
        .references
        .iter()
        .find(|f| {
            f.side == OwnedSourceSide::Before && f.entity == "requirement" && f.field == "job"
        })
        .unwrap();
    assert!(!owner.mutations.is_empty());
    assert!(owner.required);
    assert!(owner.matches_retained_schema);
    assert_eq!(
        owner.unique,
        vec![vec!["job".to_string(), "material".to_string()]]
    );
    assert!(has(&r, "reference_mutation", "retarget [0, 0]"));
}
#[test]
fn inventory_visits_state_arguments_callbacks_keys_and_global_constraints() {
    let (mut a, mut b) = pair();
    for p in [&mut a, &mut b] {
        p.program.observables.push(ObservableDefinition {
            id: "input_copy".into(),
            label: "Input".into(),
            value: Expr::State {
                state: "input".into(),
            },
        });
        p.program.entities[1].constraints.push(Expr::All {
            items: Box::new(query("job")),
            binding: "root".into(),
            predicate: Box::new(field("root", "done")),
        });
        p.program.views[0].actions[0].arguments.insert(
            "name".into(),
            Expr::If {
                condition: Box::new(eq(field("row", "production"), int(1))),
                then_value: Box::new(text("active")),
                else_value: Box::new(text("idle")),
            },
        );
        p.program.views[0].keys.push(KeyBinding {
            key: "enter".into(),
            modifiers: Default::default(),
            binding: "calculate".into(),
        });
        let ViewKind::List { controls, .. } = &mut p.program.views[0].kind else {
            panic!()
        };
        controls.push(StateControl {
            id: "input_control".into(),
            label: "Input".into(),
            state: "input".into(),
            on_change: None,
        });
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    assert!(uses(&r, "observable input_copy")
        .iter()
        .any(|u| u["state"] == "input"));
    assert!(!uses(&r, "view work binding calculate argument name").is_empty());
    assert!(!uses(&r, "view work key[0]").is_empty());
    assert!(!uses(&r, "view work control input_control").is_empty());
    assert!(has(&r, "global_admission", "entity material constraint[0]"));
    assert!(has(
        &r,
        "input_argument_closure",
        "view work binding calculate argument name"
    ));
}
#[test]
fn inventory_does_not_accept_counterfeit_or_wrong_root_lifecycle() {
    let (a, b) = pair();
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &a, 20000).unwrap();
    let r = request(&store, &a, &b);
    let mut wrong = r.clone();
    wrong.lifecycle.entity = "material".into();
    assert!(store.inventory_common_fact_phases(&b, &wrong).is_err());
    let mut wrong = r.clone();
    wrong.lifecycle.before = field("other", "done");
    assert!(store.inventory_common_fact_phases(&b, &wrong).is_err());
    let mut counterfeit = r.clone();
    counterfeit.lifecycle.before = field("record", "waiting");
    counterfeit.lifecycle.after = field("record", "waiting");
    let report = store
        .inventory_common_fact_phases(&b, &counterfeit)
        .unwrap();
    assert!(has(&report, "lifecycle_meaning", "lifecycle"));
    assert!(!report.obligations.is_empty());
    assert!(report
        .lifecycle_writers
        .iter()
        .any(|w| w.field.as_deref() == Some("waiting")));
}
#[test]
fn inventory_reports_preceding_create_and_untruncated_order_sensitive_loop() {
    let (a, mut b) = pair();
    b.program.entities.push(serde_json::from_value(json!({"id":"pause","label":"Pause","fields":[{"id":"job","label":"Owner","value_type":{"kind":"reference","entity":"job"}}],"unique":[["job"]],"constraints":[]})).unwrap());
    b.program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps
        .insert(
            0,
            Statement::ForEach {
                items: query("job"),
                binding: "capture".into(),
                steps: vec![Statement::Create {
                    entity: "pause".into(),
                    bind: "pause".into(),
                    values: [("job".into(), var("capture"))].into(),
                }],
            },
        );
    let mut a = a;
    let extra = action(
        "last_writer",
        [("target".into(), Type::reference("material"))].into(),
        vec![Statement::ForEach {
            items: query("material"),
            binding: "alias".into(),
            steps: vec![Statement::Update {
                record: var("target"),
                values: [("name".into(), field("alias", "name"))].into(),
            }],
        }],
    );
    a.program.actions.push(extra.clone());
    b.program.actions.push(extra);
    a = recapture(&a);
    b = recapture(&b);
    let dir = fixture::tempdir();
    let store = ProductStore::create(dir.path().join("project"), &a, 20000).unwrap();
    let mut r = request(&store, &a, &a);
    r.sources.after = canonical_digest(IdentityDomain::Source, &b).unwrap();
    r.sources.edits[0].after = endpoint(&b, "calculate", &[2], 0, 1);
    r.sources.edits.insert(
        0,
        OwnedSpanEdit {
            id: "capture".into(),
            before: endpoint(&a, "calculate", &[], 0, 0),
            after: endpoint(&b, "calculate", &[], 0, 1),
        },
    );
    let report = store.inventory_common_fact_phases(&b, &r).unwrap();
    assert!(report
        .identity_hazards
        .iter()
        .any(|h| h.action == "calculate"
            && h.common_create == vec![1]
            && h.variant_create == vec![0, 0]));
    assert!(has(&report, "identity_order", "last_writer [0].items"));
    assert!(has(&report, "shared_effect", "last_writer [0, 0]"));
}
#[test]
fn inventory_exposes_active_admission_mixed_lifecycle_and_resource_caps() {
    let (mut a, mut b) = pair();
    for p in [&mut a, &mut b] {
        p.program
            .actions
            .iter_mut()
            .find(|a| a.id == "calculate")
            .unwrap()
            .guards = vec![eq(field("row", "production"), int(1))];
    }
    let Statement::ForEach { steps, .. } = &mut b
        .program
        .actions
        .iter_mut()
        .find(|a| a.id == "calculate")
        .unwrap()
        .steps[1]
    else {
        panic!()
    };
    steps[0] = Statement::Update {
        record: var("sync"),
        values: [
            ("production".into(), int(1)),
            ("done".into(), boolean(true)),
        ]
        .into(),
    };
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    assert!(has(&r, "admission_reachability", "calculate guard[0]"));
    assert!(has(&r, "mixed_lifecycle_transition", "calculate [1, 0]"));
    assert_eq!(r.resources.total_record_cap, MAX_COLLECTION);
    assert!(r.resources.includes_archived_and_auxiliary);
    assert!(!r.resources.max_query_limit_proves_order_independence);
    assert!(r.resources.failures_commit_fuel_and_trace);
    assert!(has(&r, "resource_atomicity", "calculate"));
}

#[test]
fn inventory_retains_shared_update_and_exact_requirement_owner_obligations() {
    let (mut a, mut b) = pair();
    let owner = Expr::Field {
        record: Box::new(field("link", "job")),
        field: "done".into(),
    };
    for p in [&mut a, &mut b] {
        let mut related = action(
            "through_link",
            [
                ("link".into(), Type::reference("requirement")),
                ("unrelated".into(), Type::reference("job")),
            ]
            .into(),
            vec![Statement::Update {
                record: field("link", "material"),
                values: [("ready".into(), boolean(true))].into(),
            }],
        );
        related.guards = vec![Expr::Not {
            value: Box::new(owner.clone()),
        }];
        p.program.actions.push(related);
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    assert!(has(&r, "shared_effect", "through_link [0]"));
    assert!(has(&r, "admission_reachability", "through_link guard[0]"));
    assert!(uses(&r, "through_link guard[0]")
        .iter()
        .any(|u| u["entity"] == "requirement" && u["field"] == "job"));
    assert!(uses(&r, "through_link [0].target")
        .iter()
        .any(|u| u["entity"] == "requirement" && u["field"] == "material"));
}

#[test]
fn inventory_reports_direct_cross_root_write_without_query() {
    let (mut a, mut b) = pair();
    for p in [&mut a, &mut b] {
        let mut copy = action(
            "copy_production",
            [
                ("source".into(), Type::reference("job")),
                ("target".into(), Type::reference("job")),
            ]
            .into(),
            vec![Statement::Update {
                record: var("target"),
                values: [("production".into(), field("source", "production"))].into(),
            }],
        );
        copy.guards = vec![eq(field("source", "production"), int(1))];
        p.program.actions.push(copy);
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    for side in [OwnedSourceSide::Before, OwnedSourceSide::After] {
        assert!(r.obligations.iter().any(|o| o.side == Some(side)
            && o.code == "cross_root_coupling"
            && o.location == "copy_production [0]"));
        for (location, expression) in [
            ("copy_production [0].target", var("target")),
            (
                "copy_production [0].value.production",
                field("source", "production"),
            ),
            (
                "copy_production guard[0]",
                eq(field("source", "production"), int(1)),
            ),
        ] {
            assert!(r.uses.iter().any(|u| u.side == side
                && u.location == location
                && u.expression.as_ref() == Some(&expression)
                && u.environment["source"] == Type::reference("job")
                && u.environment["target"] == Type::reference("job")));
        }
    }
    assert!(!r
        .uses
        .iter()
        .any(|u| u.location.starts_with("copy_production") && u.kind == "query"));
}

#[test]
fn inventory_does_not_confuse_action_prefix_with_lifecycle_predicate() {
    let (mut a, mut b) = pair();
    for p in [&mut a, &mut b] {
        let mut read = action(
            "lifecycle_complete",
            [("row".into(), Type::reference("job"))].into(),
            vec![Statement::SetState {
                state: "input".into(),
                value: text(""),
            }],
        );
        read.guards = vec![eq(field("row", "production"), int(1))];
        p.program.actions.push(read);
    }
    a = recapture(&a);
    b = recapture(&b);
    let r = inventory(&a, &b);
    assert!(r
        .uses
        .iter()
        .any(|u| u.location == "lifecycle_complete guard[0].left"
            && u.field.as_deref() == Some("production")));
    assert!(r
        .lifecycle_writers
        .iter()
        .any(|w| w.field.as_deref() == Some("done")));
    assert!(!r
        .lifecycle_writers
        .iter()
        .any(|w| w.field.as_deref() == Some("production")));
    assert!(!has(&r, "mixed_lifecycle_transition", "calculate [1, 0]"));
}
