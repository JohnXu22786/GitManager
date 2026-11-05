//! Proposed private necessary-condition tests; no durable authority is fabricated.
//! Include from owned_continuation only after approval of amendment 1.
use super::*;
fn boolean(v: bool) -> Expr {
    owned_lowering::literal(DataValue::Boolean { value: v }, Type::Boolean)
}
fn text(v: &str) -> Expr {
    owned_lowering::literal(DataValue::Text { value: v.into() }, Type::Text)
}
fn variable(v: &str) -> Expr {
    Expr::Variable { name: v.into() }
}
fn field(v: &str, f: &str) -> Expr {
    Expr::Field {
        record: Box::new(variable(v)),
        field: f.into(),
    }
}
fn marker(v: Expr) -> Statement {
    Statement::SetState {
        state: "ui".into(),
        value: v,
    }
}
fn update(f: &str, v: Expr) -> Statement {
    Statement::Update {
        record: variable("r"),
        values: [(f.into(), v)].into(),
    }
}
fn loop_(steps: Vec<Statement>) -> Statement {
    Statement::ForEach {
        items: Expr::Query {
            entity: "root".into(),
            binding: "q".into(),
            predicate: Box::new(field("q", "live")),
            sort: vec![],
            limit: MAX_COLLECTION,
            include_archived: false,
        },
        binding: "r".into(),
        steps,
    }
}
fn program(steps: Vec<Statement>) -> CapturedProgram {
    let app = AppDefinition {
        version: 1,
        id: "scope-fixture".into(),
        label: "Scope fixture".into(),
        entities: vec![EntityDefinition {
            id: "root".into(),
            label: "Root".into(),
            fields: ["live", "policy", "sink", "sink2"]
                .into_iter()
                .map(|id| FieldDefinition {
                    id: id.into(),
                    label: id.into(),
                    value_type: Type::Boolean,
                })
                .collect(),
            unique: vec![],
            constraints: vec![],
        }],
        state: vec![StateDefinition {
            id: "ui".into(),
            label: "UI".into(),
            value_type: Type::Text,
            initial: DataValue::Text {
                value: String::new(),
            },
        }],
        actions: vec![ActionDefinition {
            id: "act".into(),
            label: "Act".into(),
            parameters: BTreeMap::new(),
            guards: vec![],
            steps,
            ensures: vec![],
        }],
        outputs: vec![],
        observables: vec![],
        views: vec![ViewDefinition {
            id: "main".into(),
            label: "Main".into(),
            kind: ViewKind::Form {
                action: "act".into(),
                fields: vec![],
                defaults: Values::new(),
            },
            actions: vec![],
            keys: vec![],
        }],
        initial_view: "main".into(),
    };
    CapturedProgram::capture(
        &serde_json::to_vec(&app).unwrap(),
        "scope-fixture",
        Producer::Fixture {
            name: "continuation scope conditions".into(),
        },
        None,
    )
    .unwrap()
}
fn endpoint(p: &CapturedProgram, path: Vec<usize>, start: usize, len: usize) -> OwnedSpanEndpoint {
    let mut steps = p.program.actions[0].steps.as_slice();
    for i in &path {
        let Statement::ForEach { steps: inner, .. } = &steps[*i] else {
            panic!()
        };
        steps = inner;
    }
    OwnedSpanEndpoint {
        source: revision(p).unwrap(),
        action: "act".into(),
        list_path: path.clone(),
        start,
        len,
        kinds: steps[start..start + len]
            .iter()
            .map(OwnedStatementKind::of)
            .collect(),
        digest: identity(&("owned-span/1", &steps[start..start + len])).unwrap(),
        previous: start.checked_sub(1).map(|i| identity(&steps[i]).unwrap()),
        next: steps.get(start + len).map(|s| identity(s).unwrap()),
        root: if path.is_empty() {
            None
        } else {
            Some(OwnedRootBinding {
                name: "r".into(),
                value_type: Type::reference("root"),
            })
        },
    }
}
fn source(p: &CapturedProgram, path: Vec<usize>, start: usize, len: usize) -> OwnedSourceRequest {
    let ep = endpoint(p, path, start, len);
    OwnedSourceRequest {
        before: revision(p).unwrap(),
        after: revision(p).unwrap(),
        root_entity: "root".into(),
        edits: vec![OwnedSpanEdit {
            id: "slot".into(),
            before: ep.clone(),
            after: ep,
        }],
    }
}
fn meter() -> ProofMeter {
    ProofMeter::new(
        ProofBudget::default(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap()
}
fn check(
    a: &CapturedProgram,
    b: &CapturedProgram,
    sa: &OwnedSourceRequest,
    sb: &OwnedSourceRequest,
) -> AdmissionResult<()> {
    continuation(
        a,
        b,
        sa,
        sb,
        OwnedSourceSide::Before,
        &Dependencies::from([Dependency::Field("root".into(), "policy".into())]),
        &Dependencies::from([Dependency::Field("root".into(), "policy".into())]),
        &mut meter(),
    )
    .map(|_| ())
}
fn boundary_refusal(result: AdmissionResult<()>) {
    let error = result
        .err()
        .expect("expected the continuation boundary to refuse");
    assert!(
        error
            .to_string()
            .contains("original writer/dependency/barrier sequence changed"),
        "unexpected refusal: {error}"
    );
}
#[test]
fn equal_header_sibling_loop_identity_is_accepted() {
    let a = program(vec![
        loop_(vec![update("live", boolean(false))]),
        loop_(vec![update("policy", boolean(false))]),
    ]);
    let s = source(&a, vec![1], 0, 1);
    assert!(check(&a, &a, &s, &s).is_ok());
}
#[test]
fn equal_header_sibling_loop_fusion_cannot_move_query_frontier() {
    let a = program(vec![
        loop_(vec![update("live", boolean(false))]),
        loop_(vec![update("policy", boolean(false))]),
    ]);
    let b = program(vec![
        loop_(vec![
            update("live", boolean(false)),
            update("policy", boolean(false)),
        ]),
        loop_(vec![]),
    ]);
    let original = source(&a, vec![1], 0, 1);
    assert!(check(&a, &a, &original, &original).is_ok());
    boundary_refusal(check(
        &a,
        &b,
        &source(&a, vec![1], 0, 1),
        &source(&b, vec![0], 1, 1),
    ));
}
#[test]
fn disjoint_statement_relocation_does_not_pin_raw_paths() {
    let a = program(vec![
        loop_(vec![update("sink", boolean(true))]),
        loop_(vec![update("live", boolean(false))]),
        loop_(vec![update("policy", boolean(false))]),
    ]);
    let b = program(vec![
        loop_(vec![update("live", boolean(false))]),
        loop_(vec![update("policy", boolean(false))]),
        loop_(vec![update("sink", boolean(true))]),
    ]);
    // A separate common literal-write loop moves; no nonfinal state-reset
    // extension is assumed by this necessary-condition source control.
    let original = source(&a, vec![2], 0, 1);
    assert!(check(&a, &a, &original, &original).is_ok());
    assert!(check(
        &a,
        &b,
        &source(&a, vec![2], 0, 1),
        &source(&b, vec![1], 0, 1)
    )
    .is_ok());
}
#[test]
fn changed_marker_that_feeds_original_variant_is_refused() {
    let value = Expr::Equal {
        left: Box::new(Expr::State { state: "ui".into() }),
        right: Box::new(text("new")),
    };
    let a = program(vec![
        loop_(vec![update("policy", value.clone())]),
        marker(text("")),
    ]);
    let b = program(vec![
        loop_(vec![update("policy", value)]),
        marker(text("new")),
    ]);
    let original = source(&a, vec![0], 0, 1);
    assert!(check(&a, &a, &original, &original).is_ok());
    boundary_refusal(check(
        &a,
        &b,
        &source(&a, vec![0], 0, 1),
        &source(&b, vec![0], 0, 1),
    ));
}
// The collector below is proposed as a pure candidate enumerator. Its result is
// NOT RetainedOwned or checked authority; the verifier must first close sources.
fn candidates(
    a: &CapturedProgram,
    b: &CapturedProgram,
    s: &OwnedSourceRequest,
) -> BTreeSet<Vec<u8>> {
    collect_common_state_markers(&a.program, &b.program, s, &mut meter()).unwrap()
}
#[test]
fn bounded_final_common_literal_marker_has_two_source_sites() {
    let a = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        marker(text("new")),
    ]);
    assert_eq!(candidates(&a, &a, &source(&a, vec![0], 0, 1)).len(), 2);
}
#[test]
fn marker_inside_logical_edit_has_no_continuation_permission() {
    let a = program(vec![marker(text("new"))]);
    assert!(candidates(&a, &a, &source(&a, vec![], 0, 1)).is_empty());
}
#[test]
fn nonliteral_or_nonfinal_or_unpaired_marker_has_no_permission() {
    let a = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        marker(Expr::State { state: "ui".into() }),
    ]);
    assert!(candidates(&a, &a, &source(&a, vec![0], 0, 1)).is_empty());
    let a = program(vec![
        marker(text("new")),
        loop_(vec![update("policy", boolean(false))]),
    ]);
    assert!(candidates(&a, &a, &source(&a, vec![1], 0, 1)).is_empty());
    let a = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        marker(text("left")),
    ]);
    let b = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        marker(text("right")),
    ]);
    assert!(candidates(&a, &b, &source(&a, vec![0], 0, 1)).is_empty());
}
#[test]
fn marker_bound_counts_utf8_bytes_before_permission() {
    let a = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        marker(text("ok")),
    ]);
    let mut raw = a.program.clone();
    let mut exact = "é".repeat(MAX_TEXT_BYTES / 2);
    if MAX_TEXT_BYTES % 2 != 0 {
        exact.push('a');
    }
    assert_eq!(exact.len(), MAX_TEXT_BYTES);
    *raw.actions[0].steps.last_mut().unwrap() = marker(text(&exact));
    // Plain syntax candidates, not a source-authorized continuation capability.
    assert_eq!(
        collect_common_state_markers(&raw, &raw, &source(&a, vec![0], 0, 1), &mut meter())
            .unwrap()
            .len(),
        2
    );
    exact.push('é');
    assert_eq!(exact.len(), MAX_TEXT_BYTES + 2);
    *raw.actions[0].steps.last_mut().unwrap() = marker(text(&exact));
    // A captured oversized app already refuses; the enumerator also enforces it.
    assert!(
        collect_common_state_markers(&raw, &raw, &source(&a, vec![0], 0, 1), &mut meter())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn empty_logical_gap_cannot_switch_equal_header_sibling_scope() {
    let a = program(vec![
        loop_(vec![update("live", boolean(false))]),
        loop_(vec![]),
    ]);
    let old = source(&a, vec![1], 0, 0);
    let new = source(&a, vec![0], 1, 0);
    let seed = Dependencies::from([Dependency::Field("root".into(), "live".into())]);
    assert!(continuation(
        &a,
        &a,
        &old,
        &old,
        OwnedSourceSide::Before,
        &seed,
        &Dependencies::new(),
        &mut meter()
    )
    .is_ok());
    boundary_refusal(
        continuation(
            &a,
            &a,
            &old,
            &new,
            OwnedSourceSide::Before,
            &seed,
            &Dependencies::new(),
            &mut meter(),
        )
        .map(|_| ()),
    );
}

#[test]
fn new_common_sink_of_original_controlled_field_is_not_disjoint() {
    let a = program(vec![loop_(vec![update("policy", boolean(false))])]);
    let original = source(&a, vec![0], 0, 1);
    assert!(check(&a, &a, &original, &original).is_ok());
    let b = program(vec![loop_(vec![
        update("policy", boolean(false)),
        update("sink", field("r", "policy")),
    ])]);
    boundary_refusal(check(&a, &b, &original, &source(&b, vec![0], 0, 1)));

    // The original already has the identical first derived writer. The proposal
    // adds ONLY sink2 <- sink, earlier than its producer in the new traversal.
    // Refusal cannot be satisfied by noticing a newly added policy -> sink.
    let transitive_original = program(vec![loop_(vec![
        update("policy", boolean(false)),
        update("sink", field("r", "policy")),
    ])]);
    let original_slot = source(&transitive_original, vec![0], 0, 1);
    assert!(check(
        &transitive_original,
        &transitive_original,
        &original_slot,
        &original_slot
    )
    .is_ok());
    let transitive_new = program(vec![loop_(vec![
        update("sink2", field("r", "sink")),
        update("policy", boolean(false)),
        update("sink", field("r", "policy")),
    ])]);
    boundary_refusal(check(
        &transitive_original,
        &transitive_new,
        &original_slot,
        &source(&transitive_new, vec![0], 1, 1),
    ));

    let mut controlled = loop_(vec![update("sink", boolean(true))]);
    let Statement::ForEach {
        items: Expr::Query { predicate, .. },
        ..
    } = &mut controlled
    else {
        panic!()
    };
    **predicate = Expr::And {
        values: vec![*predicate.clone(), field("q", "policy")],
    };
    let b = program(vec![
        loop_(vec![update("policy", boolean(false))]),
        controlled,
    ]);
    boundary_refusal(check(&a, &b, &original, &source(&b, vec![0], 0, 1)));
}
#[test]
fn two_counter_binders_preserve_and_validate_both_saved_cells() {
    let life = Expr::Equal {
        left: Box::new(field("record", "done")),
        right: Box::new(boolean(true)),
    };
    let mut entries = Vec::new();
    let mut aliases = Vec::new();
    for (id, binding, saved) in [
        ("first_count", "first", "saved_first"),
        ("second_count", "second", "saved_second"),
    ] {
        let predicate = Expr::And {
            values: vec![
                Expr::Not {
                    value: Box::new(owned_lowering::rebind(&life, "record", binding)),
                },
                field(binding, "policy"),
            ],
        };
        let count = Expr::Count {
            items: Box::new(Expr::Query {
                entity: "root".into(),
                binding: binding.into(),
                predicate: Box::new(predicate.clone()),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            }),
        };
        assert_eq!(
            qualifying_count(&count, "root", "policy", &life),
            Some(binding)
        );
        let p = OwnedProjectionRecipe {
            location: id.into(),
            destination: OwnedProjectionDestination::RootCount {
                observable: id.into(),
                include_archived: false,
            },
            binding: binding.into(),
            value_type: Type::Boolean,
            host_value_type: Type::Boolean,
            absent_value: None,
            baseline: Some(predicate.clone()),
            candidate: predicate,
            capture_required: true,
        };
        aliases.push(alias_key(&p).unwrap());
        entries.push(("policy".into(), saved.into()));
    }
    assert_ne!(aliases[0], aliases[1]);
    // Both helpers operate on plain data and cannot mint a policy surface.
    let fields = group_policy_cells(entries);
    assert_eq!(fields["policy"].len(), 2);
    let row = Record {
        entity: "root".into(),
        id: "sealed-row".into(),
        revision: 1,
        created_program: identity(&"fixture-origin").unwrap(),
        archived: false,
        values: [
            ("policy".into(), DataValue::Boolean { value: false }),
            ("saved_first".into(), DataValue::Boolean { value: false }),
            ("saved_second".into(), DataValue::Boolean { value: false }),
        ]
        .into(),
    };
    assert!(verify_policy_cells(&row, true, true, &fields).is_ok());
    for saved in ["saved_first", "saved_second"] {
        let mut corrupt = row.clone();
        corrupt
            .values
            .insert(saved.into(), DataValue::Boolean { value: true });
        assert!(verify_policy_cells(&corrupt, true, true, &fields).is_err());
        corrupt.values.insert(
            saved.into(),
            DataValue::Text {
                value: "false".into(),
            },
        );
        assert!(verify_policy_cells(&corrupt, true, true, &fields).is_err());
        corrupt.values.remove(saved);
        assert!(verify_policy_cells(&corrupt, true, true, &fields).is_err());
    }
    let mut archived = row.clone();
    archived.archived = true;
    for key in ["policy", "saved_first", "saved_second"] {
        archived
            .values
            .insert(key.into(), DataValue::Boolean { value: true });
    }
    assert!(verify_policy_cells(&archived, false, true, &fields).is_ok());
    let mut unfinished = row.clone();
    unfinished
        .values
        .insert("policy".into(), DataValue::Boolean { value: true });
    assert!(verify_policy_cells(&unfinished, true, false, &fields).is_ok());
    assert!(verify_policy_cells(&unfinished, false, false, &fields).is_err());
}

#[test]
fn independent_literal_write_sharing_receiver_is_not_policy_taint() {
    let a = program(vec![loop_(vec![update("policy", boolean(false))])]);
    let b = program(vec![loop_(vec![
        update("policy", boolean(false)),
        update("sink", boolean(true)),
    ])]);
    let original = source(&a, vec![0], 0, 1);
    assert!(check(&a, &a, &original, &original).is_ok());
    assert!(check(&a, &b, &original, &source(&b, vec![0], 0, 1)).is_ok());
}
