//! Exact selected-wrapper layout/payment controls, without genuine fixture setup.
use super::*;
#[test]
fn locked_archive_export_payment_has_exact_boundaries() {
    let cost = LOCKED_ARCHIVE_EXPORT_PAYMENT;
    crate::product_store::scope::OwnedPolicyCheck::assert_archive_packet_for_test(cost);
    println!("LOCKED_ARCHIVE_EXPORT_LAYOUTS {}", serde_json::json!({
        "bundle": std::mem::size_of::<IntentionBundle>(),
        "bundle_result": std::mem::size_of::<Result<IntentionBundle>>(),
        "decision_result": std::mem::size_of::<Result<()>>(),
        "capture": std::mem::size_of::<(&Vec<AcceptedScene>, &BTreeMap<Digest, std::ops::Range<usize>>)>(),
        "d1": cost,
    }));
}

const SENTINEL: &str = "locked archive assertion boundary";
impl DecisionEngine<LocalRuntime> {
    pub(crate) fn assert_locked_archive_policy_for_test(&self, store: &ProductStore, mode: &str) {
        let current = store.load().unwrap();
        let layer = current.owned_scope().unwrap().layers.keys().next().unwrap();
        let operation = format!("archive-{mode}");
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (prepared, summary, report) = self.preview_owned_withdrawal(store, layer, &operation,
            crate::product_store::scope::ProofBudget::default(), cancel.clone()).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        let change = self.prepare_owned_withdrawal(store, prepared.clone(), &summary.retire_decisions, &operation).unwrap();
        assert_eq!(summary.retire_decisions, change.plan().retire_decisions);
        if mode == "payments" {
            store.assert_locked_archive_d0_for_test(&prepared, &change.prepared, &change.decisions);
        }
        let attempts = if mode == "payments" { 4 } else { 1 };
        for attempt in 0..attempts {
            let mut reached = false;
            let stage = std::cell::Cell::new("writer prefix");
            let secondary = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let result = store.adopt_owned_transition_verified(change.revision, &prepared,
                &change.prepared, &change.decisions, Some(secondary.clone()),
                |locked, fresh, graph, plan, candidate, check, capsule| {
                    reached = true;
                    let capsule = capsule.expect("only real fresh withdrawal mints a capsule");
                    assert_eq!(locked, &current);
                    assert_eq!(plan.retire_decisions, summary.retire_decisions);
                    assert_eq!(graph, &change.decisions);
                    stage.set("live withdrawal policy");
                    let ids = owned_policy::check_withdrawal(&self.archive, check).map_err(owned_transition::into_store_error)?;
                    assert_eq!(ids, summary.retire_decisions);
                    if mode == "payments" {
                        if attempt < 2 {
                            check.assert_archive_d1_for_test(LOCKED_ARCHIVE_EXPORT_PAYMENT, LOCKED_ARCHIVE_EXPORT_PAYMENT - 1 + attempt)?;
                        } else {
                            if attempt == 2 { secondary.store(true, std::sync::atomic::Ordering::Release); }
                            else { cancel.store(true, std::sync::atomic::Ordering::Release); }
                            check.charge(LOCKED_ARCHIVE_EXPORT_PAYMENT)?;
                            panic!("cancelled D1 must not enter selected export");
                        }
                    } else {
                        check.charge(LOCKED_ARCHIVE_EXPORT_PAYMENT)?;
                        stage.set("selected whole archive export");
                        let selected = self.archive.export_for_locked_withdrawal(candidate, capsule).map_err(owned_transition::into_store_error)?;
                        stage.set("ordinary whole archive reference");
                        let ordinary = self.archive.export_for(candidate);
                        if mode == "history" {
                            // This larger untouched ordinary path still exceeds its
                            // original budget. No whole output exists to compare;
                            // do not relabel this as public export/recovery success.
                            assert!(matches!(ordinary, Err(DecisionError::Store(StoreError::Invalid(ref text)))
                                if text == "Common factual admission: budget: cumulative proof/initialization budget exhausted"),
                                "ordinary whole archive reference must retain its exact refusal: {ordinary:?}");
                        } else {
                            let ordinary = ordinary.map_err(owned_transition::into_store_error)?;
                            assert_eq!(canonical_bytes(&selected).unwrap(), canonical_bytes(&ordinary).unwrap());
                        }
                        stage.set("archive output assertions");
                        assert_eq!(selected.snapshot_digest(), &canonical_digest(IdentityDomain::Data, candidate).unwrap());
                        assert_eq!(candidate.data, current.data);
                        assert_eq!(candidate.session, current.session);
                        assert_eq!(candidate.artifacts, current.artifacts);
                        if mode == "origin" {
                            check.assert_locked_archive_origin_for_test(capsule, locked, fresh, &prepared, candidate);
                        }
                        if mode == "history" {
                            stage.set("complete independent source references");
                            check.assert_locked_archive_sources_for_test(capsule);
                        }
                        if mode == "bundle" {
                            self.archive.assert_selected_bundle_errors_for_test(candidate, &selected, capsule, locked, check);
                        }
                    }
                    Err(StoreError::Invalid(SENTINEL.into()))
                }, |_| panic!("fresh operation cannot use retry"), None);
            assert!(reached);
            let expected = if mode == "payments" && attempt == 0 {
                "Common factual admission: budget: cumulative proof/initialization budget exhausted"
            } else if mode == "payments" && attempt >= 2 {
                "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"
            } else { SENTINEL };
            assert!(matches!(result, Err(StoreError::Invalid(ref text)) if text == expected), "{}: {result:?}", stage.get());
            assert_eq!(store.load().unwrap(), current);
        }
    }
}
impl IntentArchive {
    fn assert_selected_bundle_errors_for_test(
        &self, snapshot: &ProjectSnapshot, bundle: &IntentionBundle,
        capsule: &LockedArchivePolicy<'_>, current: &ProjectSnapshot,
        check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>,
    ) {
        assert!(!bundle.objects.is_empty());
        let assert_same_error = |changed: &IntentionBundle, expected: &str| {
            let ordinary = validate_bundle(snapshot, changed).unwrap_err();
            let selected = validate_bundle_locked(snapshot, changed, capsule).unwrap_err();
            assert_eq!(format!("{ordinary:?}"), format!("{selected:?}"));
            assert!(matches!(selected, DecisionError::Invalid(ref text) if text == expected), "{selected:?}");
        };
        let mut bad_version = bundle.clone(); bad_version.version += 1;
        // Two faults: the outer bundle check must precede object decoding.
        bad_version.objects[0].bytes.push(0);
        assert_same_error(&bad_version, "intention bundle version, snapshot identity or size is invalid");
        let mut missing = bundle.clone(); missing.objects.pop();
        assert_same_error(&missing, "intention bundle lacks reachable immutable objects");
        let mut duplicate = bundle.clone(); duplicate.objects.push(bundle.objects[0].clone());
        duplicate.objects.last_mut().unwrap().bytes.push(0);
        assert_same_error(&duplicate, "duplicate or unrelated intention object");
        let mut corrupt = bundle.clone();
        let content = decode(&corrupt.objects[0].bytes, &corrupt.objects[0].digest).unwrap();
        corrupt.objects[0].bytes = canonical_bytes(&Object { version: 2, content }).unwrap();
        assert_same_error(&corrupt, "intention object version or identity mismatch");

        // Independently exercise the decoded guards on the genuine retained
        // source scenes. Whole-bundle integrity would reject re-encoded corrupt
        // scenes first; these controls do not bypass that boundary or fabricate
        // a successful bundle/capsule for the changed bytes.
        let decision = &current.decisions.decisions[0];
        let scenes = self.accepted_scenes(decision).unwrap();
        let assert_policy_error = |result: Result<()>, expected: &str| {
            assert!(matches!(result, Err(DecisionError::Invalid(ref text)) if text == expected), "{result:?}");
        };
        let mut mismatched = scenes.clone();
        mismatched[0].program.binding.program_path.push_str("-different-source");
        assert_policy_error(owned_policy::check_decision(check, decision, &mismatched),
            "accepted scene evidence does not bind its exact program and complete input");
        let mut mismatched = scenes.clone();
        mismatched[0].scenario.random_seed += 1;
        // Inventory mismatch is earlier than the scene's evidence mismatch.
        assert_policy_error(owned_policy::check_decision(check, decision, &mismatched),
            "owned retirement scene identities changed");
        let mut bound = decision.clone();
        bound.scenarios = mismatched.iter().map(|scene| scene.scenario.identity().unwrap()).collect();
        assert_policy_error(owned_policy::check_decision(check, &bound, &mismatched),
            "accepted scene evidence does not bind its exact program and complete input");
        let mut mixed = scenes.clone(); mixed[0].bind_outcome = true;
        assert_policy_error(owned_policy::check_decision(check, decision, &mixed),
            "mixed or empty property-only intention binding");
        let mut hidden = decision.clone();
        hidden.obligations[0].predicate = PropertyPredicate::Or { values: vec![
            hidden.obligations[0].predicate.clone(),
            PropertyPredicate::Less {
                left: PropertyTerm::Observed { point: "result".into(), observable: "ready_count".into(), value_type: Type::Integer },
                right: PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: MAX_COLLECTION as i64 + 1 } },
            },
        ] };
        let result = owned_policy::check_decision(check, &hidden, &scenes);
        assert!(matches!(result, Err(DecisionError::Unverified(ref text))
            if text == "whole obligation contains an unowned observed surface"), "{result:?}");
        let source = &scenes[0].program.program;
        let key = SemanticKey { kind: SemanticKind::Action, entity: None, id: source.actions[0].id.clone() };
        let identity = SemanticMapping { from: key.clone(), to: key };
        let result = Mapping::new(source, source, &[identity.clone(), identity]);
        assert!(matches!(result, Err(DecisionError::Invalid(ref text))
            if text == "semantic mapping is missing, ambiguous or changes kind"));

        // Public restore deliberately validates without the capsule, including a
        // fresh ordinary re-export, and preserves complete canonical bytes.
        self.restore_for(snapshot, bundle).unwrap();
        assert_eq!(canonical_bytes(&self.export_for(snapshot).unwrap()).unwrap(), canonical_bytes(bundle).unwrap());
    }
}
