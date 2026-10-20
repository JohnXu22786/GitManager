//! Private bounded payments. Genuine archive controls use the locked writer only.
use super::*;
#[test]
fn locked_archive_payments_have_exact_boundaries() {
    for cost in [LOCKED_ARCHIVE_TRANSPORT_PAYMENT, LOCKED_ARCHIVE_SELECTOR_PAYMENT] {
        assert!(cost > 0);
        for available in [cost - 1, cost] {
            let mut meter = ProofMeter::new(ProofBudget { units: available + 1, elapsed_millis: 60_000 }, Arc::new(AtomicBool::new(false))).unwrap();
            let result = meter.charge(cost);
            assert_eq!(result.is_ok(), available == cost);
            assert_eq!(meter.remaining_units_for_test(), if available == cost { 0 } else { available });
        }
    }
}
#[test]
fn locked_archive_layouts_are_complete() {
    println!("LOCKED_ARCHIVE_LAYOUTS {}", serde_json::json!({
        "capsule": std::mem::size_of::<LockedArchivePolicy<'_>>(),
        "optional_capsule": std::mem::size_of::<Option<LockedArchivePolicy<'_>>>(),
        "optional_borrow": std::mem::size_of::<Option<&LockedArchivePolicy<'_>>>(),
        "context": std::mem::size_of::<OwnedPolicyCheck<'_>>(),
        "store_result": std::mem::size_of::<Result<()>>(),
        "d0": LOCKED_ARCHIVE_TRANSPORT_PAYMENT,
        "d2": LOCKED_ARCHIVE_SELECTOR_PAYMENT,
    }));
    assert!(LOCKED_ARCHIVE_TRANSPORT_PAYMENT > 4 * std::mem::size_of::<Result<()>>() as u64);
    assert!(LOCKED_ARCHIVE_SELECTOR_PAYMENT > 4 * std::mem::size_of::<Result<()>>() as u64);
}

const SENTINEL: &str = "locked archive assertion boundary";
const FUEL: &str = "Common factual admission: budget: cumulative proof/initialization budget exhausted";
fn token() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
fn meter() -> ProofMeter { ProofMeter::new(ProofBudget::default(), token()).unwrap() }
fn assert_invalid<T>(result: &Result<T>, expected: &str) {
    assert!(matches!(result, Err(StoreError::Invalid(text)) if text == expected), "expected {expected}");
}
impl OwnedPolicyCheck<'_> {
    // Assertion-only interfaces never return a capsule, registry, execution or meter.
    pub(crate) fn assert_archive_packet_for_test(cost: u64) {
        for available in [cost - 1, cost] {
            let mut m = ProofMeter::new(ProofBudget { units: available + 1, elapsed_millis: 60_000 }, token()).unwrap();
            let result = m.charge(cost);
            assert_eq!(result.is_ok(), available == cost);
            assert_eq!(m.remaining_units_for_test(), if available == cost { 0 } else { available });
            if available != cost { assert_invalid(&result, FUEL); }
        }
        for secondary in [false, true] {
            let cancel = token();
            let mut m = ProofMeter::new(ProofBudget::default(), if secondary { token() } else { cancel.clone() }).unwrap();
            if secondary { m.watch_cancellation(&cancel); }
            cancel.store(true, Ordering::Release);
            let before = m.remaining_units_for_test();
            assert!(m.charge(cost).unwrap_err().to_string().contains("cancelled or cumulative elapsed"));
            assert_eq!(before, m.remaining_units_for_test());
        }
        let mut m = ProofMeter::new(ProofBudget { units: cost + 1, elapsed_millis: 1 }, token()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(m.charge(cost).unwrap_err().to_string().contains("elapsed"));
        assert_eq!(cost, m.remaining_units_for_test());
    }
    pub(crate) fn assert_locked_archive_origin_for_test(
        &mut self, capsule: &LockedArchivePolicy<'_>, current: &ProjectSnapshot,
        fresh: &PreparedOwnedTransitionV2, prepared: &PreparedOwnedTransitionV2,
        candidate: &ProjectSnapshot,
    ) {
        assert!(std::ptr::eq(self.registry.unwrap(), capsule.registry));
        assert!(std::ptr::eq(self.execution, fresh.execution.as_ref()));
        assert!(!Arc::ptr_eq(&fresh.execution, &prepared.execution));
        assert_eq!(self.execution.current(), current);
        capsule.registry.check_snapshot(current).unwrap();
        assert_eq!(candidate.data, current.data);
        assert_eq!(candidate.session, current.session);
        assert_eq!(candidate.clock_day, current.clock_day);
        assert_eq!(candidate.artifacts, current.artifacts);
        let mut selected_calls = 0;
        check_retained_transition_policies_locked(candidate, capsule, |check| {
            selected_calls += 1;
            assert!(std::ptr::eq(check.registry.unwrap(), capsule.registry));
            assert_eq!(check.execution.current(), current);
            let source = current.program()?.clone();
            let before = check.meter.remaining_units_for_test();
            let selected = check.surface(&source)?;
            let selected_cost = before - check.meter.remaining_units_for_test();
            let mut fallback_meter = meter();
            let mut fallback = OwnedPolicyCheck { execution: check.execution, registry: None, meter: &mut fallback_meter };
            let before = fallback.meter.remaining_units_for_test();
            let original = fallback.surface(&source)?;
            let fallback_cost = before - fallback.meter.remaining_units_for_test();
            selected.assert_same_value_for_test(&original);
            assert!(fallback_cost > selected_cost, "actual old-position reconstruction must be removed");
            let mut foreign = source.clone();
            foreign.binding.program_path.push_str("-foreign");
            let before = check.meter.remaining_units_for_test();
            assert!(check.surface(&foreign).is_err());
            assert_eq!(before - check.meter.remaining_units_for_test(), 5);
            Ok(())
        }).unwrap();
        assert_eq!(selected_calls, 1);
        let mut ordinary_calls = 0;
        check_retained_transition_policies(candidate, |check| {
            ordinary_calls += 1;
            assert!(check.registry.is_none());
            assert_eq!(check.execution.current(), current);
            Ok(())
        }).unwrap();
        assert_eq!(ordinary_calls, selected_calls);
        // An arbitrary changed candidate cannot use a donor to skip its own
        // reconstruction. These are corruptions, not new authoritative recipes.
        for kind in ["graph", "data", "session", "day", "history"] {
            let mut changed = candidate.clone();
            match kind {
                "graph" => changed.decisions.revision += 1,
                "data" => changed.data.records[0].revision += 1,
                "session" => { changed.session.values.insert("forged".into(), DataValue::Boolean { value: true }); }
                "day" => changed.clock_day += 1,
                "history" => changed.adoptions.last_mut().unwrap().revision += 1,
                _ => unreachable!(),
            }
            let mut reached = false;
            assert!(check_retained_transition_policies_locked(&changed, capsule, |_| { reached = true; Ok(()) }).is_err(), "{kind}");
            assert!(!reached, "candidate reconstruction must precede selected policy: {kind}");
        }
        // Force only the digest hint to match an authentic donor while the real
        // execution basis differs. Never turn the forged hint into authority.
        let endpoint = owned_persist::reconstruct(candidate, &mut meter()).unwrap();
        let counterfeit = LockedArchivePolicy { registry: &endpoint };
        assert_ne!(counterfeit.registry.snapshot, capsule.registry.snapshot);
        let mut misses = 0;
        check_retained_transition_policies_locked(candidate, &counterfeit, |check| {
            misses += 1;
            assert!(check.registry.is_none());
            check.surface(current.program().unwrap())?;
            Ok(())
        }).unwrap();
        assert_eq!(misses, 1);
        let mut manifest = fresh.execution.manifest.clone();
        manifest.basis.frozen.snapshot = counterfeit.registry.snapshot.clone();
        let forged_execution = VerifiedOwnedTransitionExecution {
            continuation: fresh.execution.continuation.clone(),
            target: fresh.execution.target.clone(), metadata: fresh.execution.metadata.clone(),
            manifest, original: fresh.execution.original.clone(),
            initialization: fresh.execution.initialization.clone(),
        };
        assert_eq!(counterfeit.registry.snapshot, forged_execution.manifest.basis.frozen.snapshot);
        let mut m = meter();
        let mut forged = OwnedPolicyCheck { execution: &forged_execution, registry: Some(counterfeit.registry), meter: &mut m };
        assert_invalid(&forged.surface(current.program().unwrap()),
            "Common factual admission: owned persisted proof: execution registry belongs to another complete snapshot");

    }
    pub(crate) fn assert_archive_d1_for_test(&mut self, cost: u64, available: u64) -> Result<()> {
        let left = self.meter.remaining_units_for_test();
        assert!(left >= available);
        self.charge(left - available).unwrap();
        let result = self.charge(cost);
        assert_eq!(result.is_ok(), available == cost);
        assert_eq!(self.meter.remaining_units_for_test(), if available == cost { 0 } else { available });
        if available != cost { assert_invalid(&result, FUEL); }
        result
    }
    pub(crate) fn assert_locked_archive_sources_for_test(&mut self, capsule: &LockedArchivePolicy<'_>) {
        let sources = [self.execution.current().program().unwrap(), &self.execution.original.target];
        assert_ne!(sources[0], sources[1]);
        let mut surfaces = Vec::new();
        for source in sources {
            // Reference-only comparisons have independent ordinary meters; the
            // actual selected whole archive already ran intact before this call.
            let mut selected_meter = meter();
            let mut selected = OwnedPolicyCheck { execution: self.execution, registry: Some(capsule.registry), meter: &mut selected_meter };
            let actual = selected.surface(source).unwrap();
            let mut original_meter = meter();
            let mut original = OwnedPolicyCheck { execution: self.execution, registry: None, meter: &mut original_meter };
            let expected = original.surface(source).unwrap();
            actual.assert_same_value_for_test(&expected);
            assert!(selected.meter.remaining_units_for_test() > original.meter.remaining_units_for_test());
            let mut foreign = source.clone(); foreign.binding.program_path.push_str("-foreign-history");
            assert!(selected.surface(&foreign).is_err());
            surfaces.push(actual);
        }
        assert_ne!(surfaces[0].source(), surfaces[1].source());
        assert_ne!(surfaces[0].baseline(), surfaces[1].baseline());
        assert_ne!(surfaces[0].candidate(), surfaces[1].candidate());
    }
}
impl ProductStore {
    pub(crate) fn assert_locked_archive_d0_for_test(
        &self, prepared: &PreparedOwnedTransitionV2, adoption: &PreparedAdoption, graph: &DecisionGraph,
    ) {
        let current = self.load().unwrap();
        let pointer = self.root.read_and_sync("CURRENT", MAX_STORE_BYTES).unwrap();
        let mut prefix = None;
        let result = self.adopt_owned_transition_verified(current.revision, prepared, adoption, graph, None,
            |_, _, _, _, _, check, capsule| {
                assert!(capsule.is_some());
                prefix = Some(prepared.budget.units - check.meter.remaining_units_for_test());
                Err(StoreError::Invalid(SENTINEL.into()))
            }, |_| panic!("not retry"), None);
        assert_invalid(&result, SENTINEL);
        let prefix = prefix.unwrap();
        for extra in [0, 1] {
            let mut bounded = prepared.clone();
            bounded.budget.units = prefix - 1 + extra;
            let mut reached = false;
            let result = self.adopt_owned_transition_verified(current.revision, &bounded, adoption, graph, None,
                |_, _, _, _, _, check, capsule| {
                    reached = true;
                    assert!(capsule.is_some());
                    assert_eq!(check.meter.remaining_units_for_test(), 0);
                    Err(StoreError::Invalid(SENTINEL.into()))
                }, |_| panic!("not retry"), None);
            assert_eq!(reached, extra == 1);
            assert_invalid(&result, if extra == 1 { SENTINEL } else { FUEL });
            assert_eq!(self.root.read_and_sync("CURRENT", MAX_STORE_BYTES).unwrap(), pointer);
            assert_eq!(self.load().unwrap(), current);
        }
    }
}
