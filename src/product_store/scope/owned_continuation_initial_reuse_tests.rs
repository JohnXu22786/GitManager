//! Small private ownership/payment controls; no persisted admission is fabricated.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
fn proof(label: &str) -> VerifiedSourceContinuation {
    VerifiedSourceContinuation {
        identity: identity(&label).unwrap(),
        protected: Dependencies::from([Dependency::State(label.into())]),
        influenced: Dependencies::from([Dependency::Membership(label.into())]),
    }
}
#[test]
fn logical_views_keep_four_ordered_identities_without_cloning_payloads() {
    let hit = ComparisonProofs {
        original: [proof("before"), proof("after")],
        predecessor: None,
    };
    let [a, b, c, d] = hit.logical();
    assert!(std::ptr::eq(a, c));
    assert!(std::ptr::eq(b, d));
    assert_eq!(
        hit.logical().map(|p| &p.identity),
        [&a.identity, &b.identity, &a.identity, &b.identity]
    );
    for p in hit.logical() {
        assert_eq!(p.protected.len(), 1);
        assert_eq!(p.influenced.len(), 1);
    }
    let general = ComparisonProofs {
        original: [proof("before"), proof("after")],
        predecessor: Some([proof("prior-before"), proof("prior-after")]),
    };
    let expected =
        ["before", "after", "prior-before", "prior-after"].map(|s| identity(&s).unwrap());
    assert_eq!(general.logical().map(|p| p.identity.clone()), expected);
    let ids: Vec<_> = general
        .logical()
        .into_iter()
        .map(comparison_identity as fn(&VerifiedSourceContinuation) -> Digest)
        .collect();
    assert_eq!(ids, expected);
}
fn meter(units: u64) -> ProofMeter {
    ProofMeter::new(
        ProofBudget {
            units,
            elapsed_millis: 60_000,
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}
#[test]
fn continuation_payments_enforce_exact_fit_and_refusal() {
    for cost in [
        COMPARISON_DISPATCH_PAYMENT,
        COMPARISON_OWNER_PAYMENT,
        COMPARISON_HIT_PAYMENT,
        COMPARISON_VIEW_PAYMENT,
        COMPARISON_QUERY_PAYMENT,
    ] {
        assert!(cost > 0);
        let mut exact = meter(cost + 1);
        exact.charge(cost).unwrap();
        assert_eq!(exact.remaining_units_for_test(), 0);
        let mut short = meter(cost);
        let left = short.remaining_units_for_test();
        assert!(short.charge(cost).is_err());
        assert_eq!(short.remaining_units_for_test(), left);
        for secondary in [false, true] {
            let token = Arc::new(AtomicBool::new(false));
            let mut m = if secondary {
                meter(cost + 1)
            } else {
                ProofMeter::new(ProofBudget::default(), token.clone()).unwrap()
            };
            if secondary {
                m.watch_cancellation(&token);
            }
            token.store(true, Ordering::Relaxed);
            let left = m.remaining_units_for_test();
            assert!(m
                .charge(cost)
                .unwrap_err()
                .to_string()
                .contains("cancelled or cumulative elapsed"));
            assert_eq!(m.remaining_units_for_test(), left);
        }
        let mut elapsed = ProofMeter::new(
            ProofBudget {
                units: cost + 1,
                elapsed_millis: 1,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(elapsed
            .charge(cost)
            .unwrap_err()
            .to_string()
            .contains("elapsed"));
    }
}
#[test]
fn marker_query_payment_checks_count_overflow() {
    assert_eq!(marker_query_payment(0, 0).unwrap(), 0);
    assert_eq!(
        marker_query_payment(MAX_ITEMS, MAX_ITEMS).unwrap(),
        2 * MAX_ITEMS as u64 * COMPARISON_QUERY_PAYMENT
    );
    assert!(marker_query_payment(usize::MAX, 1).is_none());
    if usize::BITS == 64 {
        assert!(marker_query_payment(usize::MAX / 2, 0).is_none());
    }
    let mut exhausted = meter(10_000_000);
    let left = exhausted.remaining_units_for_test();
    assert!(exhausted.charge(marker_query_payment(usize::MAX, 1).unwrap_or(u64::MAX)).unwrap_err().to_string().contains("cumulative proof/initialization budget exhausted"));
    assert_eq!(exhausted.remaining_units_for_test(), left);

}

#[test]
fn actual_comparison_layouts_bound_added_component_payment() {
    println!(
        "COMPARISON_LAYOUTS {}",
        serde_json::json!({
            "initial_input":size_of::<InitialComparison<'_>>(), "origins":size_of::<ComparisonOrigins<'_>>(),
            "whole_result":size_of::<VerifiedSourceContinuation>(), "pair":size_of::<[VerifiedSourceContinuation;2]>(),
            "optional_pair":size_of::<Option<[VerifiedSourceContinuation;2]>>(), "owner":size_of::<ComparisonProofs>(),
            "rules":size_of::<ClosedContinuationRules>(), "rules_result":size_of::<AdmissionResult<ClosedContinuationRules>>(),
            "retained":size_of::<RetainedOwned>(), "retained_result":size_of::<AdmissionResult<RetainedOwned>>(),
            "continuation_result":size_of::<AdmissionResult<VerifiedOwnedContinuation>>(),
            "view":size_of::<[&VerifiedSourceContinuation;4]>(), "array_iter":size_of::<std::array::IntoIter<&VerifiedSourceContinuation,4>>(),
            "identity_iter":size_of::<IdentityIter<'_>>(), "unit_result":size_of::<AdmissionResult<()>>(),
            "r5":COMPARISON_DISPATCH_PAYMENT,"r6":COMPARISON_OWNER_PAYMENT,"r7_each":COMPARISON_HIT_PAYMENT,
            "r8_view":COMPARISON_VIEW_PAYMENT,"r8_query":COMPARISON_QUERY_PAYMENT,
        })
    );
    let per_continuation = COMPARISON_DISPATCH_PAYMENT
        + COMPARISON_OWNER_PAYMENT
        + 2 * COMPARISON_HIT_PAYMENT
        + COMPARISON_VIEW_PAYMENT
        + marker_query_payment(MAX_ITEMS, MAX_ITEMS).unwrap();
    assert!(per_continuation<897_612, "only a component comparison against individually observed meters 1/2, not a meter19 fit claim");
}

// Assertion-only bridge, no proof or origin can leave the private owner.
impl VerifiedOwnedContinuation {
    pub(in crate::product_store::scope) fn assert_initial_reuse_same_value(&self, other: &Self) {
        assert_eq!(self.current, other.current);
        assert_eq!(self.before, other.before);
        assert_eq!(self.after, other.after);
        assert_eq!(self.request, other.request);
        assert_eq!(self.report, other.report);
        assert_eq!(self.layout, other.layout);
        assert_eq!(self.storage_fields, other.storage_fields);
        assert_eq!(self.layer, other.layer);
        assert_eq!(self.initialization, other.initialization);
        assert_eq!(self.continuation.len(), 4);
        assert_eq!(self.continuation, other.continuation);
        assert_eq!(
            self.continuation_identity().unwrap(),
            other.continuation_identity().unwrap()
        );
    }
}

impl PreparedOwnedPreview {
    pub(in crate::product_store::scope) fn assert_initial_comparison_error_order(&self) {
        let before = self.proof.snapshot.program().unwrap();
        let after = &self.proof.candidate;
        let base_inventory = super::super::common_fact_inventory::derive_pair(
            &self.proof.snapshot,
            before,
            after,
            &self.proof.request.inventory,
        )
        .unwrap();
        for fail_first in [true, false] {
            let mut changed = before.clone();
            if fail_first {
                changed.program.id = "other-program".into();
            }
            let mut request = self.proof.request.clone();
            request.inventory.sources.before = revision(&changed).unwrap();
            request.inventory.sources.edits[0].after.action = "other-action".into();
            let mut inventory = base_inventory.clone();
            inventory.structure.before = revision(&changed).unwrap();
            let origins = ComparisonOrigins::General {
                original: self,
                predecessor_before: before,
                predecessor_after: after,
            };
            let mut actual = meter(10_000_000);
            let error = ClosedContinuationRules::verify(
                self,
                before,
                after,
                &self.proof.request.inventory.sources,
                &changed,
                after,
                &request,
                &inventory,
                &Dependencies::new(),
                &Dependencies::new(),
                &origins,
                &mut actual,
            )
            .err()
            .unwrap();
            let mut expected = meter(10_000_000);
            expected.charge(1).unwrap();
            let first = continuation(
                before,
                &changed,
                &self.proof.request.inventory.sources,
                &request.inventory.sources,
                OwnedSourceSide::Before,
                &Dependencies::new(),
                &Dependencies::new(),
                &mut expected,
            );
            let expected_error = if fail_first {
                first.err().unwrap()
            } else {
                assert!(first.is_ok());
                continuation(
                    after,
                    after,
                    &self.proof.request.inventory.sources,
                    &request.inventory.sources,
                    OwnedSourceSide::After,
                    &Dependencies::new(),
                    &Dependencies::new(),
                    &mut expected,
                )
                .err()
                .unwrap()
            };
            assert_eq!(error.to_string(), expected_error.to_string());
            assert_eq!(
                actual.remaining_units_for_test(),
                expected.remaining_units_for_test(),
                "no later comparison or hit may execute after an original error"
            );
            if fail_first {
                assert!(error.to_string().contains("business schema"));
            } else {
                assert!(error.to_string().contains("logical edit retargeted"));
            }
        }
    }
}
