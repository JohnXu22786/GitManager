//! Necessary-condition tests. These do not fabricate successful durable proofs.
use super::*;
fn boolean(value: bool) -> Expr {
    owned_lowering::literal(DataValue::Boolean { value }, Type::Boolean)
}
fn row(flag: Option<bool>, completed: bool, archived: bool) -> Record {
    let mut values = Values::from([("done".into(), DataValue::Boolean { value: completed })]);
    if let Some(value) = flag {
        values.insert("policy".into(), DataValue::Boolean { value });
    }
    Record {
        entity: "root".into(),
        id: "r".into(),
        revision: 1,
        created_program: identity(&"ordinary-origin").unwrap(),
        archived,
        values,
    }
}
#[test]
fn policy_neutral_base_does_not_infer_ownership_from_membership() {
    for member in [false, true] {
        let mut r = row(Some(true), false, false);
        r.values.insert(
            "untrusted_member_label".into(),
            DataValue::Boolean { value: member },
        );
        assert!(!neutral_original_rows(
            &[r],
            "root",
            ("done", &DataValue::Boolean { value: true }),
            "policy"
        ));
    }
    assert!(!neutral_original_rows(
        &[row(None, false, false)],
        "root",
        ("done", &DataValue::Boolean { value: true }),
        "policy"
    ));
    assert!(neutral_original_rows(
        &[row(Some(false), false, false)],
        "root",
        ("done", &DataValue::Boolean { value: true }),
        "policy"
    ));
}
#[test]
fn policy_original_completed_or_archived_true_is_not_erased() {
    for r in [row(Some(true), true, false), row(Some(true), false, true)] {
        let old = r.clone();
        assert!(neutral_original_rows(
            std::slice::from_ref(&r),
            "root",
            ("done", &DataValue::Boolean { value: true }),
            "policy"
        ));
        assert_eq!(r, old);
    }
}
fn count(predicate: Expr) -> Expr {
    Expr::Count {
        items: Box::new(Expr::Query {
            entity: "root".into(),
            binding: "r".into(),
            predicate: Box::new(predicate),
            sort: vec![],
            limit: MAX_COLLECTION,
            include_archived: false,
        }),
    }
}
fn completion() -> Expr {
    Expr::Equal {
        left: Box::new(Expr::Field {
            record: Box::new(Expr::Variable {
                name: "record".into(),
            }),
            field: "done".into(),
        }),
        right: Box::new(boolean(true)),
    }
}
fn selector() -> Expr {
    Expr::And {
        values: vec![
            Expr::Not {
                value: Box::new(owned_lowering::rebind(&completion(), "record", "r")),
            },
            Expr::Field {
                record: Box::new(Expr::Variable { name: "r".into() }),
                field: "policy".into(),
            },
        ],
    }
}
#[test]
fn policy_count_rejects_extra_common_branch_and_query_changes() {
    assert!(qualifying_count(&count(selector()), "root", "policy", &completion()).is_some());
    let Expr::And { mut values } = selector() else {
        panic!()
    };
    values.push(boolean(true));
    assert!(qualifying_count(
        &count(Expr::And { values }),
        "root",
        "policy",
        &completion()
    )
    .is_none());
    let Expr::And { mut values } = selector() else {
        panic!()
    };
    values.reverse();
    assert!(qualifying_count(
        &count(Expr::And { values }),
        "root",
        "policy",
        &completion()
    )
    .is_none());
    for kind in 0..2 {
        let mut c = count(selector());
        let Expr::Count { items } = &mut c else {
            panic!()
        };
        let Expr::Query {
            limit,
            include_archived,
            ..
        } = items.as_mut()
        else {
            panic!()
        };
        if kind == 0 {
            *limit = 1;
        } else {
            *include_archived = true;
        }
        assert!(qualifying_count(&c, "root", "policy", &completion()).is_none());
    }
}

fn projection(id: &str) -> OwnedProjectionRecipe {
    OwnedProjectionRecipe {
        location: id.into(),
        destination: OwnedProjectionDestination::ViewColumn {
            view: "v".into(),
            column: id.into(),
        },
        binding: "row".into(),
        value_type: Type::Boolean,
        host_value_type: Type::Boolean,
        absent_value: None,
        baseline: Some(boolean(false)),
        candidate: boolean(true),
        capture_required: true,
    }
}
fn layout(saved: Vec<Id>) -> Metadata {
    Metadata {
        identity: identity(&"original-layout").unwrap(),
        root: "root".into(),
        member: "member".into(),
        sealed: "sealed".into(),
        day: "day".into(),
        origin: "origin".into(),
        saved,
        revision: Some("revision".into()),
    }
}
#[test]
fn continuation_alias_groups_cannot_split_or_merge() {
    let old = vec![projection("a"), projection("b")];
    let mut split = old.clone();
    split[1].candidate = boolean(false);
    assert!(reorder_projections(&old, split, &layout(vec!["same".into(), "same".into()])).is_err());
    assert!(
        reorder_projections(&old, old.clone(), &layout(vec!["one".into(), "two".into()])).is_err()
    );
}
#[test]
fn continuation_storage_mapping_uses_destination_and_refuses_missing_or_extra() {
    let old = vec![projection("a"), projection("b")];
    let saved = layout(vec!["same".into(), "same".into()]);
    let result = reorder_projections(&old, vec![old[1].clone(), old[0].clone()], &saved).unwrap();
    assert_eq!(result, old);
    assert!(reorder_projections(&old, vec![old[0].clone()], &saved).is_err());
    let mut extra = old.clone();
    extra.push(projection("c"));
    assert!(reorder_projections(&old, extra, &saved).is_err());
}
