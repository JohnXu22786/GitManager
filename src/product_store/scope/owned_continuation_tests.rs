//! Necessary-condition tests. These do not fabricate successful durable proofs.
use super::*;

// Compiler-checked complete layouts for the producing 64-bit toolchain.
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<&CanonicalEvidenceBytes>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<&CanonicalEvidenceTuple<'_>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<&[u8]>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<&mut ProofMeter>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<&std::vec::Vec<u8>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<&str>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); size_of::<()>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<CanonicalEvidenceBytes>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 112] = [(); size_of::<CanonicalEvidenceTuple<'_>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<Dependencies>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<Digest>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 1] = [(); size_of::<IdentityDomain>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<StoreError>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 72] = [(); size_of::<VerifiedSourceContinuation>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 1] = [(); size_of::<bool>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<isize>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 1] = [(); size_of::<std::num::TryFromIntError>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, ContractError>, CanonicalEvidenceBytes>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, ContractError>, std::vec::Vec<u8>>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, StoreError>, u64>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, StoreError>>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::result::Result<(), StoreError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); size_of::<std::result::Result<CanonicalEvidenceBytes, ContractError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::result::Result<VerifiedSourceContinuation, StoreError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<std::result::Result<std::convert::Infallible, ContractError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::result::Result<std::convert::Infallible, StoreError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); size_of::<std::result::Result<std::vec::Vec<u8>, ContractError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 152] = [(); size_of::<std::result::Result<u64, StoreError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<std::result::Result<u64, std::num::TryFromIntError>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); size_of::<std::vec::Vec<u8>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<u64>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<usize>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16785] = [(); CONTINUATION_CANONICAL_PAYMENT as usize];
#[test]
fn continuation_canonical_packet_complete_actual_layouts() {
    // Independent source/MIR census supplies multiplicities; these are real types.
    assert_eq!(CONTINUATION_CANONICAL_PAYMENT, comparison_payment(
        comparison_payment(1, [(64, 26)]), [
            (size_of::<&CanonicalEvidenceBytes>(), 8),
            (size_of::<&CanonicalEvidenceTuple<'_>>(), 16),
            (size_of::<&[u8]>(), 8),
            (size_of::<&mut ProofMeter>(), 12),
            (size_of::<&std::vec::Vec<u8>>(), 12),
            (size_of::<&str>(), 16),
            (size_of::<()>(), 28),
            (size_of::<CanonicalEvidenceBytes>(), 20),
            (size_of::<CanonicalEvidenceTuple<'_>>(), 4),
            (size_of::<Dependencies>(), 8),
            (size_of::<Digest>(), 12),
            (size_of::<IdentityDomain>(), 4),
            (size_of::<StoreError>(), 4),
            (size_of::<VerifiedSourceContinuation>(), 4),
            (size_of::<bool>(), 8),
            (size_of::<isize>(), 24),
            (size_of::<std::num::TryFromIntError>(), 4),
            (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, ContractError>, CanonicalEvidenceBytes>>(), 4),
            (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, ContractError>, std::vec::Vec<u8>>>(), 4),
            (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, StoreError>, u64>>(), 4),
            (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, StoreError>>>(), 12),
            (size_of::<std::result::Result<(), StoreError>>(), 12),
            (size_of::<std::result::Result<CanonicalEvidenceBytes, ContractError>>(), 8),
            (size_of::<std::result::Result<VerifiedSourceContinuation, StoreError>>(), 4),
            (size_of::<std::result::Result<std::convert::Infallible, ContractError>>(), 16),
            (size_of::<std::result::Result<std::convert::Infallible, StoreError>>(), 32),
            (size_of::<std::result::Result<std::vec::Vec<u8>, ContractError>>(), 4),
            (size_of::<std::result::Result<u64, StoreError>>(), 4),
            (size_of::<std::result::Result<u64, std::num::TryFromIntError>>(), 4),
            (size_of::<std::vec::Vec<u8>>(), 8),
            (size_of::<u64>(), 20),
            (size_of::<usize>(), 8),
        ]));
    println!("CONTINUATION_CANONICAL_LAYOUTS actual={} slots=90 direct=2 activations=26", CONTINUATION_CANONICAL_PAYMENT);
}

#[test]
fn continuation_canonical_concrete_tuple_identity_parity() {
    // Pure encoding controls: these tuples are not successful source proofs.
    let old = identity(&"old").unwrap();
    let new = identity(&"new").unwrap();
    let action = String::from("action-β");
    for side in [OwnedSourceSide::Before, OwnedSourceSide::After] {
        for populated in [false, true] {
            let closed = if populated {
                Dependencies::from([
                    Dependency::Field("root".into(), "done".into()),
                    Dependency::Membership("root".into()),
                    Dependency::Binding("receiver".into(), old.clone()),
                ])
            } else { Dependencies::new() };
            let influenced = if populated {
                Dependencies::from([Dependency::Clock, Dependency::CreationOrdinal])
            } else { Dependencies::new() };
            let frozen = if populated { vec![(&action, vec![0u8, 1, 255])] } else { Vec::new() };
            let evidence = (
                "owned-baseline-continuation/3", old.clone(), new.clone(), side,
                &closed, &influenced, frozen,
            );
            let bytes = canonical_bytes(&evidence).unwrap();
            let expected = identity(&evidence).unwrap();
            let owner = crate::product_contract::CanonicalEvidenceBytes::from_value(&evidence).unwrap();
            assert_eq!(owner.len(), bytes.len());
            assert_eq!(owner.into_evidence_digest(), expected);
        }
    }
}

#[test]
fn continuation_canonical_packet_direct_short_exact_and_cancel() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    // Direct payment/poll control, not evidence that a genuine route reaches it.
    let cost = CONTINUATION_CANONICAL_PAYMENT;
    assert!(cost > 0 && cost < ProofBudget::default().units);
    for exact in [false, true] {
        let mut meter = ProofMeter::new(ProofBudget {
            units: cost + u64::from(exact), elapsed_millis: 60_000,
        }, Arc::new(AtomicBool::new(false))).unwrap();
        let before = meter.remaining_units_for_test();
        let result = meter.charge(cost);
        if exact {
            result.unwrap();
            assert_eq!(meter.remaining_units_for_test(), 0);
        } else {
            assert_eq!(result.unwrap_err().to_string(),
                "Invalid(\"Common factual admission: budget: cumulative proof/initialization budget exhausted\")");
            assert_eq!(meter.remaining_units_for_test(), before);
        }
    }
    for secondary in [false, true] {
        let token = Arc::new(AtomicBool::new(false));
        let mut meter = ProofMeter::new(ProofBudget::default(), if secondary {
            Arc::new(AtomicBool::new(false))
        } else { token.clone() }).unwrap();
        if secondary { meter.watch_cancellation(&token); }
        meter.charge(cost).unwrap();
        let owner = crate::product_contract::CanonicalEvidenceBytes::from_value(&"poll-only").unwrap();
        token.store(true, Ordering::Relaxed);
        let before = meter.remaining_units_for_test();
        assert_eq!(meter.charge(0).unwrap_err().to_string(),
            "Invalid(\"Common factual admission: budget: cancelled or cumulative elapsed budget exhausted\")");
        assert_eq!(meter.remaining_units_for_test(), before);
        drop(owner);
    }
}
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

// This controls the independent completed-origin packet. It grants no origin
// and does not replace genuine four-logical-slot or fallback route evidence.
#[test]
fn completed_initial_packets_cover_full_results() {
    for packet in [COMPLETED_INITIAL_DISPATCH_PAYMENT,
        COMPLETED_INITIAL_CLOSED_PAYMENT, COMPLETED_INITIAL_WINDOW_PAYMENT,
        COMPLETED_INITIAL_HIT_PAYMENT] {
        assert!(packet >= 4 * size_of::<AdmissionResult<()>>() as u64);
        assert_ne!(packet, u64::MAX, "compile-only measurement scaffold cannot run");
    }
}

#[cfg(target_pointer_width = "64")]
const _: [(); 144] = [(); size_of::<[VerifiedSourceContinuation; 2]>()];

// V13 necessary accounting conditions only. These do not mint completed
// origins, prove actual route entry, or imply that a full strict journey fits.
#[test]
fn completed_transport_census_and_retained_floors() {
    assert!(COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT > 0);
    // V14 removes duplicate variable source validation, not fixed transports.
    assert_eq!(COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT, 576);
    assert_eq!(COMPLETED_INITIAL_DISPATCH_PAYMENT, 46_912);
    assert_ne!(COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT, u64::MAX);
    assert_ne!(COMPLETED_INITIAL_DISPATCH_PAYMENT, u64::MAX);
    assert!(COMPLETED_INITIAL_DISPATCH_PAYMENT >= 4 * size_of::<AdmissionResult<VerifiedOwnedContinuation>>() as u64);
    assert_eq!(COMPLETED_INITIAL_CLOSED_PAYMENT, 13_600);
    assert_eq!(COMPLETED_INITIAL_WINDOW_PAYMENT, 18_940);
    assert_eq!(COMPLETED_INITIAL_HIT_PAYMENT, 5_632);
    println!("COMPLETED_TRANSPORT_CENSUS view={} dispatch={} full_result={}",
        COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT, COMPLETED_INITIAL_DISPATCH_PAYMENT,
        size_of::<AdmissionResult<VerifiedOwnedContinuation>>());
}

fn assert_completed_transport_packet(packet: u64, name: &str) {
    use super::super::common_fact_admission::ProofBudget;
    use std::sync::{atomic::AtomicBool, Arc};
    assert!(packet > 0 && packet < ProofBudget::default().units);
    let mut short = ProofMeter::new(ProofBudget { units: packet, elapsed_millis: 60_000 },
        Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(short.remaining_units_for_test(), packet - 1);
    assert!(matches!(short.charge(packet), Err(StoreError::Invalid(message))
        if message == "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
    assert_eq!(short.remaining_units_for_test(), packet - 1);
    let mut exact = ProofMeter::new(ProofBudget {
        units: packet.checked_add(1).unwrap(), elapsed_millis: 60_000,
    }, Arc::new(AtomicBool::new(false))).unwrap();
    exact.charge(packet).unwrap();
    assert_eq!(exact.remaining_units_for_test(), 0);
    exact.charge(0).unwrap();
    println!("COMPLETED_TRANSPORT_PACKET name={name} packet={packet} short=refused exact=paid");
}

#[test]
fn completed_transport_view_entry_short_exact() {
    assert_completed_transport_packet(73_960_u64.checked_add(
        COMPLETED_INITIAL_ORIGIN_VIEW_PAYMENT).unwrap(), "transition_plus_view");
}

#[test]
fn completed_transport_dispatch_short_exact() {
    assert_completed_transport_packet(COMPLETED_INITIAL_DISPATCH_PAYMENT, "dispatch");
}

#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<super::super::owned_persist::CompletedInitialOrigin<'_, '_>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); size_of::<&super::super::owned_persist::CompletedInitialOrigin<'_, '_>>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<(&ProjectSnapshot, &std::sync::Arc<PreparedOwnedPreview>)>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 2696 + size_of::<Arc<[VerifiedSourceContinuation; 2]>>()] =
    [(); size_of::<AdmissionResult<VerifiedOwnedContinuation>>()];

// V15 synthetic ownership only. These values cannot create a registry loan,
// historical authority, lifecycle proof or publication permission.
#[test]
fn completed_structural_pair_retains_payloads_and_order_without_deep_clone() {
    let before = VerifiedSourceContinuation {
        identity: identity(&"structural-before").unwrap(),
        protected: Dependencies::from([Dependency::State("before-protected".into())]),
        influenced: Dependencies::from([Dependency::Membership("before-influenced".into())]),
    };
    let after = VerifiedSourceContinuation {
        identity: identity(&"structural-after").unwrap(),
        protected: Dependencies::from([Dependency::State("after-protected".into())]),
        influenced: Dependencies::from([Dependency::Membership("after-influenced".into())]),
    };
    let donor = Arc::new([before, after]);
    let proofs = ComparisonProofs { original: Arc::clone(&donor), predecessor: None };
    assert!(Arc::ptr_eq(&proofs.original, &donor));
    let logical = proofs.logical();
    assert!(std::ptr::eq(logical[0], &donor[0]));
    assert!(std::ptr::eq(logical[1], &donor[1]));
    assert!(std::ptr::eq(logical[0], logical[2]));
    assert!(std::ptr::eq(logical[1], logical[3]));
    assert!(logical[0].protected.contains(&Dependency::State("before-protected".into())));
    assert!(logical[0].influenced.contains(&Dependency::Membership("before-influenced".into())));
    assert!(logical[1].protected.contains(&Dependency::State("after-protected".into())));
    assert!(logical[1].influenced.contains(&Dependency::Membership("after-influenced".into())));
    drop(donor);
    assert_eq!(proofs.logical()[0].identity, identity(&"structural-before").unwrap());
    assert_eq!(proofs.logical()[1].identity, identity(&"structural-after").unwrap());
    println!("COMPLETED_STRUCTURAL_PAIR payloads=retained ordered=1 shared=1 synthetic_only=1");
}

#[test]
fn completed_structural_packets_are_positive_complete_and_short_exact() {
    for (name, packet) in [
        ("entry", STRUCTURAL_ENTRY_PAYMENT), ("pair", STRUCTURAL_PAIR_PAYMENT),
        ("select_open", STRUCTURAL_SELECT_OPEN_PAYMENT),
        ("select_next", STRUCTURAL_SELECT_NEXT_PAYMENT),
        ("select_compare", STRUCTURAL_SELECT_COMPARE_PAYMENT),
        ("loan", STRUCTURAL_LOAN_PAYMENT), ("hit", STRUCTURAL_HIT_PAYMENT),
    ] {
        assert_completed_transport_packet(packet, name);
    }
    assert!(COMPARISON_OWNER_PAYMENT >= comparison_payment(64, [
        (144, 4), (144, 4), (288, 6), (384, 4), (480, 4), (384, 2), (480, 2),
    ]));
    assert!(COMPLETED_INITIAL_CLOSED_PAYMENT >= 13_600);
    println!("COMPLETED_STRUCTURAL_PACKETS fixed_only=1 route_entry_unproved=1");
}

// V15 producing compiler assertions: complete actual layouts, not success-only proxies.
#[cfg(target_pointer_width = "64")]
const _: [(); 97216] = [(); STRUCTURAL_ENTRY_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 25104] = [(); STRUCTURAL_PAIR_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 3776] = [(); STRUCTURAL_SELECT_OPEN_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 3712] = [(); STRUCTURAL_SELECT_NEXT_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 8784] = [(); STRUCTURAL_SELECT_COMPARE_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 41516] = [(); STRUCTURAL_LOAN_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 3776] = [(); STRUCTURAL_HIT_PAYMENT as usize];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<(&ProjectSnapshot, &Arc<super::super::owned_persist::VerifiedOwnedTransitionExecution>)>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); size_of::<super::super::owned_persist::CompletedStructuralLoan<'_, '_, '_>>()];

#[test]
fn completed_structural_scan_byte_sum_is_checked_and_fail_closed() {
    // Pure arithmetic/exclusion only. No synthetic key can mint a loan.
    assert_eq!(0_u64.checked_add(0), Some(0));
    assert_eq!(5_u64.checked_add(7), Some(12));
    assert_eq!(u64::MAX.checked_add(0), Some(u64::MAX));
    assert_eq!(u64::MAX.checked_add(1), None);
    let mut meter = ProofMeter::new(
        super::super::common_fact_admission::ProofBudget { units: 10_000, elapsed_millis: 60_000 },
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    ).unwrap();
    let before = meter.remaining_units_for_test();
    let cost = u64::MAX.checked_add(1).unwrap_or(u64::MAX);
    assert!(matches!(meter.charge(cost), Err(StoreError::Invalid(message))
        if message == "Common factual admission: budget: cumulative proof/initialization budget exhausted"));
    assert_eq!(meter.remaining_units_for_test(), before);
    println!("COMPLETED_STRUCTURAL_SCAN checked=1 overflow=typed_refusal synthetic_only=1");
}
