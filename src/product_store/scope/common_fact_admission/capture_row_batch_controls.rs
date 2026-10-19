//! Test-only actual-capture controls. No proof, meter or retained facts escape.
use super::*;
use crate::product_contract::{reset_validation_counts, validation_counts, WorkFailure};
use crate::product_runtime::{ProjectionHost, ProjectionReservation};
use std::time::Duration;

const FUEL: &str = "cumulative proof/initialization budget exhausted";
const ELAPSED: &str = "cumulative elapsed budget exhausted";
fn meter() -> ProofMeter {
    ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap()
}
fn text(value: &str) -> DataValue { DataValue::Text { value: value.into() } }
fn literal(value: DataValue, value_type: Type) -> Expr { Expr::Literal { value, value_type } }
fn named(data: &DataSnapshot, title: &str) -> RecordRef {
    let record = data.records.iter().find(|r| r.entity == "job"
        && r.values.get("title") == Some(&text(title))).unwrap();
    RecordRef { entity: record.entity.clone(), record: record.id.clone() }
}
fn assert_error<T>(result: AdmissionResult<T>, expected: &StoreError) {
    match (result, expected) {
        (Err(StoreError::Invalid(actual)), StoreError::Invalid(expected)) => assert_eq!(&actual, expected),
        (Err(StoreError::Runtime(actual)), StoreError::Runtime(expected)) => assert_eq!(&actual, expected),
        (Err(actual), _) => panic!("wrong typed failure: {actual:?}; expected {expected:?}"),
        (Ok(_), _) => panic!("unexpected capture success; expected {expected:?}"),
    }
}
fn recipe(base: &OwnedProjectionRecipe, key: &str, value: DataValue, typ: Type) -> OwnedProjectionRecipe {
    let mut result = base.clone();
    result.location = key.into();
    result.value_type = typ.clone();
    result.host_value_type = typ;
    result.baseline = Some(literal(value, result.value_type.clone()));
    result.absent_value = None;
    result
}
impl ProductStore {
    pub(crate) fn assert_capture_row_values_for_test(
        &self, proof: &VerifiedOwnedPreviewPlan, case: &str,
    ) {
        assert_eq!(self.load().unwrap(), proof.snapshot);
        let current = self.root.read("CURRENT", 16 * 1024).unwrap();
        let mut source = proof.snapshot.program().unwrap().clone();
        let mut data = proof.snapshot.data.clone();
        let mut request = proof.request.clone();
        let mut projections = proof.report.projections.clone();
        let mut day = proof.snapshot.clock_day;
        let live = named(&data, "live");
        let finished = named(&data, "finished");
        assert_eq!(data.records.iter().filter(|r| r.entity == "job").count(), 2);
        assert_eq!(projections.len(), 4);
        assert_eq!(projections.iter().filter(|p| p.baseline.is_some()).count(), 3);
        let mut expected: BTreeMap<String, Values> = [(finished.record.clone(), [
            ("view/jobs/title".into(), text("finished")),
            ("view/jobs/status".into(), text("done")),
            ("view/jobs/pending".into(), DataValue::Null),
            ("count/ready_count".into(), DataValue::Boolean { value: false }),
        ].into())].into();
        match case {
            "full" => {},
            "selected-live" => request.population = ScopePopulation::SelectedUnfinished { records: vec![live.clone()] },
            "excluded-live" => request.excluded_records.push(live.clone()),
            "future" => request.population = ScopePopulation::FutureWork,
            "archived-unfinished" => {
                data.records.iter_mut().find(|r| r.entity == live.entity && r.id == live.record).unwrap().archived = true;
                data.validate().unwrap();
                // The actual archived row still has its original authored values.
                expected.insert(live.record.clone(), [
                    ("view/jobs/title".into(), text("live")),
                    ("view/jobs/status".into(), text("ready")),
                    ("view/jobs/pending".into(), DataValue::Null),
                    ("count/ready_count".into(), DataValue::Boolean { value: true }),
                ].into());
            },
            "zero-projections" => { projections.clear(); expected.get_mut(&finished.record).unwrap().clear(); },
            "nullable-absence" => {
                let mut p = projections[0].clone();
                p.location = "absence".into(); p.baseline = None;
                p.value_type = Type::Boolean;
                p.host_value_type = Type::Optional { item: Box::new(Type::Boolean) };
                p.absent_value = Some(DataValue::Null);
                projections = vec![p];
                expected.insert(finished.record.clone(), [("absence".into(), DataValue::Null)].into());
            },
            "duplicate-location" => {
                projections = vec![recipe(&projections[0], "same", text("first"), Type::Text),
                    recipe(&projections[0], "same", text("last"), Type::Text)];
                expected.insert(finished.record.clone(), [("same".into(), text("last"))].into());
            },
            "actual-day" => {
                day += 1;
                let mut p = recipe(&projections[0], "today", DataValue::Date { days: day }, Type::Date);
                p.baseline = Some(Expr::Today); projections = vec![p];
                expected.insert(finished.record.clone(), [("today".into(), DataValue::Date { days: day })].into());
            },
            "distinct-source" => {
                let old = source.clone();
                let mut app = source.program.clone(); app.label.push_str(" changed label");
                source = CapturedProgram::capture(&serde_json::to_vec(&app).unwrap(),
                    &source.binding.project_id, source.binding.producer.clone(), source.binding.task.clone()).unwrap();
                assert_ne!(source, old);
            },
            "unfinished-skips-poison" => {
                request.inventory.lifecycle.before = literal(DataValue::Boolean { value: false }, Type::Boolean);
                assert!(data.records.iter().all(|r| !r.archived));
                projections[0].baseline = Some(Expr::Variable { name: "missing".into() });
                expected.clear();
            },
            _ => panic!("unknown capture value case: {case}"),
        }
        let inputs = (source.clone(), data.clone(), request.clone(), projections.clone());
        let mut old_meter = meter(); let old_start = old_meter.remaining_units_for_test();
        reset_validation_counts();
        let old = frozen_capture_frame_before_row_batch(&source, &data, day, &request, &projections, &mut old_meter).unwrap();
        let old_counts = validation_counts();
        assert_eq!(old, expected, "independent old value oracle: {case}");
        let mut new_meter = meter();
        reset_validation_counts();
        let actual = super::capture_frame_batched(&source, &data, day, &request, &projections, &mut new_meter).unwrap();
        let new_counts = validation_counts();
        assert_eq!(actual, old, "old capture parity: {case}");
        assert_eq!(actual, expected, "literal capture values/presence: {case}");
        assert_eq!(old_counts.captures, 1); assert_eq!(new_counts.captures, 1);
        assert_eq!(new_counts.apps, 3, "all three real external-source validators: {case}");
        if case == "full" {
            // Two lifecycle calls, three present projections, four inserts, two old row visits.
            assert_eq!(old_start - old_meter.remaining_units_for_test(), 500_006);
            assert_eq!(old_counts.apps, 12); // 3 source + 4 static + 5 session validations.
            assert_eq!(proof.captured, expected);
        }
        assert_eq!((source, data, request, projections), inputs);
        assert_eq!(self.root.read("CURRENT", 16 * 1024).unwrap(), current);
        assert_eq!(self.load().unwrap(), proof.snapshot);
    }
    pub(crate) fn assert_capture_row_order_for_test(
        &self, proof: &VerifiedOwnedPreviewPlan, case: &str,
    ) {
        assert_eq!(self.load().unwrap(), proof.snapshot);
        let mut source = proof.snapshot.program().unwrap().clone();
        let mut data = proof.snapshot.data.clone();
        let mut request = proof.request.clone();
        let mut projections = proof.report.projections.clone();
        let finished = named(&data, "finished");
        let expected = match case {
            "source-before-data" => {
                source.binding.program_path = "../escape".into(); data.version = 0;
                StoreError::Runtime(AdapterError::Invalid(ContractError("unsafe relative source path".into())))
            },
            "binding-before-expression" => {
                projections[0].binding = "invalid/name".into();
                projections[0].baseline = Some(Expr::Variable { name: "missing".into() });
                StoreError::Runtime(AdapterError::Invalid(ContractError("projection project or binding mismatch".into())))
            },
            "selected-completed-before-projection" => {
                request.population = ScopePopulation::SelectedUnfinished { records: vec![finished] };
                projections[0].binding = "invalid/name".into();
                StoreError::Invalid("Common factual admission: scope: selected root is already completed".into())
            },
            "absent-is-lazy" => {
                projections[0].location = "missing-cell".into();
                projections[0].baseline = None; projections[0].absent_value = None;
                StoreError::Invalid("Common factual admission: missing-cell: absent baseline has no explicit availability rule".into())
            },
            "no-root-is-lazy" => {
                request.inventory.sources.root_entity = "not_present".into();
                source.binding.program_path = "../escape".into(); data.version = 0;
                let mut old_meter = meter(); let mut new_meter = meter();
                reset_validation_counts();
                assert!(frozen_capture_frame_before_row_batch(&source, &data, 0, &request, &projections, &mut old_meter).unwrap().is_empty());
                assert_eq!(validation_counts(), ValidationCounts::default());
                let before = new_meter.remaining_units_for_test();
                assert!(super::capture_frame_batched(&source, &data, 0, &request, &projections, &mut new_meter).unwrap().is_empty());
                assert_eq!(validation_counts(), ValidationCounts::default());
                assert!(new_meter.remaining_units_for_test() < before, "actual no-root scan is paid");
                return;
            },
            _ => panic!("unknown capture order case: {case}"),
        };
        assert_error(frozen_capture_frame_before_row_batch(&source, &data, proof.snapshot.clock_day,
            &request, &projections, &mut meter()), &expected);
        assert_error(super::capture_frame_batched(&source, &data, proof.snapshot.clock_day,
            &request, &projections, &mut meter()), &expected);
        assert_eq!(self.load().unwrap(), proof.snapshot);
    }
    pub(crate) fn assert_capture_row_host_for_test(
        &self, proof: &VerifiedOwnedPreviewPlan,
    ) {
        assert_eq!(self.load().unwrap(), proof.snapshot);
        let header = 40 + u64::try_from(std::mem::size_of::<ProjectionReservation>()).unwrap();
        for extra in [0, 1, 2] {
            // ProofMeter::new consumes1. The real header precedes the unchanged100k reserve.
            let units = 1 + header + 99_999 + extra;
            let mut host = ProofMeter::new(ProofBudget { units, elapsed_millis: 60_000 },
                Arc::new(AtomicBool::new(false))).unwrap();
            reset_validation_counts();
            let reserved = <ProofMeter as ProjectionHost>::reserve_projection_row(&mut host);
            if extra == 0 {
                assert!(matches!(reserved, Err(WorkFailure::BudgetExhausted(FUEL))));
                assert_eq!(host.remaining_units_for_test(), 99_999);
                // Ordinary ProofMeter fuel is intentionally nonlatching.
                host.charge(1).unwrap(); assert_eq!(host.remaining_units_for_test(), 99_998);
            } else {
                assert!(reserved.is_ok());
                assert_eq!(host.remaining_units_for_test(), extra - 1);
                drop(reserved); // No refund on an unused consumed reservation.
                assert_eq!(host.remaining_units_for_test(), extra - 1);
            }
            assert_eq!(validation_counts(), ValidationCounts::default());
        }
        for secondary in [false, true] {
            let primary = Arc::new(AtomicBool::new(false));
            let token = Arc::new(AtomicBool::new(false));
            let mut host = ProofMeter::new(ProofBudget::default(), primary.clone()).unwrap();
            if secondary { host.watch_cancellation(&token); } else { primary.store(true, Ordering::Relaxed); }
            if secondary { token.store(true, Ordering::Relaxed); }
            let before = host.remaining_units_for_test();
            assert!(matches!(<ProofMeter as ProjectionHost>::reserve_projection_row(&mut host), Err(WorkFailure::Cancelled)));
            assert_eq!(host.remaining_units_for_test(), before);
            primary.store(false, Ordering::Relaxed); token.store(false, Ordering::Relaxed);
            host.charge(1).unwrap(); // No global latch is introduced by the row adapter.
        }
        let mut expired = ProofMeter::new(ProofBudget { units: 10_000_000, elapsed_millis: 20 },
            Arc::new(AtomicBool::new(false))).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        let before = expired.remaining_units_for_test();
        assert!(matches!(<ProofMeter as ProjectionHost>::reserve_projection_row(&mut expired), Err(WorkFailure::BudgetExhausted(ELAPSED))));
        assert_eq!(expired.remaining_units_for_test(), before);

        // No successful Data cap applies to this no-root input. The equal-length
        // entity comparison must be funded before inspecting its bytes.
        let mut data = proof.snapshot.data.clone(); data.records.truncate(1); data.version = 0;
        data.records[0].entity = format!("{}a", "x".repeat(20_000));
        let mut request = proof.request.clone(); request.inventory.sources.root_entity = format!("{}b", "x".repeat(20_000));
        let mut source = proof.snapshot.program().unwrap().clone(); source.binding.program_path = "../escape".into();
        let mut old = ProofMeter::new(ProofBudget { units: 10_000, elapsed_millis: 60_000 }, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(frozen_capture_frame_before_row_batch(&source, &data, 0, &request, &[], &mut old).unwrap().is_empty());
        assert_eq!(old.remaining_units_for_test(), 9_999);
        // The same finite budget reaches the comparison on a short, otherwise
        // identical no-root frame; setup/header refusal cannot pass this test.
        let mut short_data = data.clone(); short_data.records[0].entity = "xa".into();
        let mut short_request = request.clone(); short_request.inventory.sources.root_entity = "xb".into();
        let mut short = ProofMeter::new(ProofBudget { units: 10_000, elapsed_millis: 60_000 }, Arc::new(AtomicBool::new(false))).unwrap();
        reset_validation_counts();
        assert!(super::capture_frame_batched(&source, &short_data, 0, &short_request, &[], &mut short).unwrap().is_empty());
        assert_eq!(validation_counts(), ValidationCounts::default());
        let mut actual = ProofMeter::new(ProofBudget { units: 10_000, elapsed_millis: 60_000 }, Arc::new(AtomicBool::new(false))).unwrap();
        reset_validation_counts();
        assert_error(super::capture_frame_batched(&source, &data, 0, &request, &[], &mut actual),
            &StoreError::Runtime(AdapterError::BudgetExhausted(FUEL.into())));
        assert_eq!(validation_counts(), ValidationCounts::default());
        assert_eq!(self.load().unwrap(), proof.snapshot);
    }
}

fn frozen_capture_frame_before_row_batch(
    baseline: &CapturedProgram,
    data: &DataSnapshot,
    day: i32,
    request: &OwnedPreviewRequest,
    projections: &[OwnedProjectionRecipe],
    meter: &mut ProofMeter,
) -> AdmissionResult<BTreeMap<String, Values>> {
    let mut captured = BTreeMap::new();
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    let mut checks = ProjectionValidation::default();
    for row in data
        .records
        .iter()
        .filter(|r| r.entity == request.inventory.sources.root_entity)
    {
        meter.charge(1)?;
        let reference = RecordRef {
            entity: row.entity.clone(),
            record: row.id.clone(),
        };
        let completed = runtime.evaluate_record_projection_with(
            &mut checks,
            meter.runtime()?,
            baseline,
            data,
            &reference,
            "record",
            &request.inventory.lifecycle.before,
            &Type::Boolean,
            day,
        )?;
        let is_completed = completed == DataValue::Boolean { value: true };
        if matches!(&request.population,ScopePopulation::SelectedUnfinished{records} if records.contains(&reference))
            && is_completed
        {
            return Err(refuse("scope", "selected root is already completed"));
        }
        if !row.archived && !is_completed {
            continue;
        }
        let mut values = Values::new();
        for projection in projections {
            let value = if let Some(expr) = &projection.baseline {
                runtime.evaluate_record_projection_with(
                    &mut checks,
                    meter.runtime()?,
                    baseline,
                    data,
                    &reference,
                    &projection.binding,
                    expr,
                    &projection.value_type,
                    day,
                )?
            } else {
                projection.absent_value.clone().ok_or_else(|| {
                    refuse(
                        &projection.location,
                        "absent baseline has no explicit availability rule",
                    )
                })?
            };
            // Inserting even Null is intentional; presence is part of the capture receipt.
            values.insert(projection.location.clone(), value);
            meter.charge(1)?;
        }
        captured.insert(row.id.clone(), values);
    }
    Ok(captured)
}
