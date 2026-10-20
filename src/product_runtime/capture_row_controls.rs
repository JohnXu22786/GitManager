//! Private test-first API controls. The referenced capture APIs do not yet exist.
//! Hand-authored fixtures; no production-default fit or execution evidence.
use super::*;
use crate::product_contract::{ContractWork, WorkFailure};
use serde_json::json;
use std::time::Duration;

#[path = "../../tests/fixtures/product_runtime/mod.rs"]
mod fixture;

#[derive(Debug, PartialEq, Eq)]
enum OuterError {
    Runtime(AdapterError),
    Scope(&'static str),
}
impl From<AdapterError> for OuterError {
    fn from(value: AdapterError) -> Self { Self::Runtime(value) }
}

// Real decreasing account, actual Instant and actual cancellation tokens.
// Only output_limit and host duration may be stricter test configurations;
// every reservation still contains exactly100000 fuel and is consumed once.
pub(super) struct TestHost {
    left: u64,
    started: Instant,
    millis: u64,
    cancelled: Arc<AtomicBool>,
    additional: Option<Arc<AtomicBool>>,
    pub(super) output_limit: usize,
    diagnostic_fuel_refusal: bool,
}
impl TestHost {
    pub(super) fn new() -> Self {
        Self { left: 10_000_000, started: Instant::now(), millis: 60_000,
            cancelled: Arc::new(AtomicBool::new(false)),
            additional: Some(Arc::new(AtomicBool::new(false))),
            output_limit: MAX_OUTPUT_BYTES, diagnostic_fuel_refusal: false }
    }
}
impl ContractWork for TestHost {
    fn charge(&mut self, units: u64) -> std::result::Result<(), WorkFailure> {
        if self.cancelled.load(Ordering::Relaxed)
            || self.additional.as_ref().is_some_and(|x| x.load(Ordering::Relaxed)) {
            return Err(WorkFailure::Cancelled);
        }
        if self.started.elapsed().as_millis() >= u128::from(self.millis) {
            return Err(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"));
        }
        self.left = self.left.checked_sub(units)
            .ok_or(WorkFailure::BudgetExhausted("cumulative proof/initialization budget exhausted"))
            .inspect_err(|_| {
                if !self.diagnostic_fuel_refusal {
                    self.diagnostic_fuel_refusal = true;
                    eprintln!("P5_TEST_HOST_FIRST_FUEL_REFUSAL left={} requested={}", self.left, units);
                }
            })?;
        Ok(())
    }
}
impl ProjectionHost for TestHost {
    fn reserve_projection_row(&mut self)
        -> std::result::Result<ProjectionReservation, WorkFailure>
    {
        self.charge(40 + std::mem::size_of::<ProjectionReservation>() as u64)?;
        let mut limits = RuntimeLimits::default();
        let host_deadline = self.started.checked_add(Duration::from_millis(self.millis))
            .ok_or(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))?;
        let reservation_now = Instant::now();
        let remaining = self.millis.saturating_sub(
            self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
        if remaining == 0 { return Err(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted")); }
        let row_window = limits.elapsed_millis;
        limits.elapsed_millis = row_window.min(remaining);
        limits.output_bytes = self.output_limit;
        let row_deadline = reservation_now.checked_add(Duration::from_millis(row_window))
            .ok_or(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))?.min(host_deadline);
        self.charge(limits.fuel)?;
        Ok(ProjectionReservation::from_actual_host(limits, host_deadline, row_deadline,
            self.cancelled.clone(), self.additional.clone()))
    }
}

pub(super) fn prepared() -> (CapturedProgram, DataSnapshot, RecordRef) {
    let mut app = fixture::organizer();
    app["state"].as_array_mut().unwrap().truncate(1);
    app["actions"] = json!([]);
    app["outputs"] = json!([]);
    app["observables"] = json!([]);
    app["views"].as_array_mut().unwrap().truncate(1);
    app["views"][0]["actions"] = json!([]);
    app["views"][0]["keys"] = json!([]);
    app["views"][0]["kind"]["controls"] = json!([]);
    app["views"][0]["kind"]["selection"] = serde_json::Value::Null;
    let program = fixture::capture(app);
    let mut data = fixture::seed(&program);
    data.records.push(Record { entity: "person".into(), id: "r0".into(), revision: 1,
        created_program: program.artifact.program_digest.clone(), archived: false,
        values: fixture::args(&[("name", fixture::string("Ada")), ("area", fixture::string("north"))]) });
    let reference = RecordRef { entity: "person".into(), record: "r0".into() };
    (program, data, reference)
}
pub(super) fn expression(value: serde_json::Value) -> Expr { serde_json::from_value(value).unwrap() }
pub(super) fn yes() -> Expr { expression(fixture::yes()) }
pub(super) fn integer(value: i64) -> serde_json::Value {
    json!({"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":value}})
}
fn bool_expr(value: bool) -> serde_json::Value {
    json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":value}})
}
fn int_list() -> serde_json::Value {
    json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"integer"}},
        "value":{"kind":"list","item_type":{"kind":"integer"},"items":[
            {"kind":"integer","value":2},{"kind":"integer","value":3}]}})
}
fn semantic_cases() -> Vec<(Expr, Type)> {
    vec![
        (yes(), Type::Boolean),
        (expression(fixture::field(fixture::var("record"), "name")), Type::Text),
        (expression(json!({"kind":"state","state":"search"})), Type::Text),
        (Expr::Today, Type::Date),
        (expression(json!({"kind":"count","items":fixture::query("person")})), Type::Integer),
        (expression(json!({"kind":"filter","items":int_list(),"binding":"x","predicate":fixture::yes()})), Type::list(Type::Integer)),
        (expression(json!({"kind":"map","items":int_list(),"binding":"x","value":{"kind":"add","left":fixture::var("x"),"right":integer(1)}})), Type::list(Type::Integer)),
        (expression(json!({"kind":"any","items":int_list(),"binding":"x","predicate":fixture::yes()})), Type::Boolean),
        (expression(json!({"kind":"all","items":int_list(),"binding":"x","predicate":fixture::yes()})), Type::Boolean),
        (expression(json!({"kind":"sum","items":int_list()})), Type::Integer),
        (expression(json!({"kind":"contains","items":int_list(),"value":integer(3)})), Type::Boolean),
        (expression(json!({"kind":"equal","left":int_list(),"right":int_list()})), Type::Boolean),
        (expression(json!({"kind":"less","left":fixture::text("a"),"right":fixture::text("b")})), Type::Boolean),
        (expression(json!({"kind":"add","left":integer(4),"right":integer(3)})), Type::Integer),
        (expression(json!({"kind":"subtract","left":integer(4),"right":integer(3)})), Type::Integer),
        (expression(json!({"kind":"concat","left":fixture::text("a"),"right":fixture::text("β")})), Type::Text),
        (expression(json!({"kind":"text_contains","text":fixture::text("aβc"),"search":fixture::text("β")})), Type::Boolean),
        (expression(json!({"kind":"lower","value":fixture::text("AΣ")})), Type::Text),
        (expression(json!({"kind":"and","values":[bool_expr(false),{"kind":"equal","left":{"kind":"add","left":integer(i64::MAX),"right":integer(1)},"right":integer(0)}]})), Type::Boolean),
        (expression(json!({"kind":"or","values":[fixture::yes(),bool_expr(false)]})), Type::Boolean),
        (expression(json!({"kind":"not","value":bool_expr(false)})), Type::Boolean),
        (expression(json!({"kind":"if","condition":fixture::yes(),"then_value":integer(7),"else_value":{"kind":"add","left":integer(i64::MAX),"right":integer(1)}})), Type::Integer),
        (expression(json!({"kind":"coalesce","value":{"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"null"}},"fallback":fixture::text("fallback")})), Type::Text),
        (expression(json!({"kind":"date_add","date":{"kind":"today"},"days":integer(1)})), Type::Date),
        (expression(json!({"kind":"date_difference","later":{"kind":"today"},"earlier":{"kind":"literal","value_type":{"kind":"date"},"value":{"kind":"date","days":19999}}})), Type::Integer),
        (expression(json!({"kind":"literal","value_type":{"kind":"list","item":{"kind":"list","item":{"kind":"integer"}}},"value":{"kind":"list","item_type":{"kind":"list","item":{"kind":"integer"}},"items":[{"kind":"list","item_type":{"kind":"integer"},"items":[]}]}})), Type::list(Type::list(Type::Integer))),
    ]
}

#[test]
fn capture_row_semantics_one_actual_evaluator_and_fresh_results() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    let cases = semantic_cases();
    // Independent current production API; no new descriptor or result cache is an oracle.
    let expected: Vec<_> = cases.iter().map(|(expr, typ)| runtime
        .evaluate_record_projection(&program,&data,&reference,"record",expr,typ,20000).unwrap()).collect();
    reset_validation_counts();
    let mut host = TestHost::new();
    let result: std::result::Result<(), OuterError> = runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| {
            capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row, first| {
                assert_eq!(first, DataValue::Boolean { value: true });
                let identity = row.snapshot_for_test();
                let mut bytes = serde_json::to_vec(&first).unwrap().len();
                let mut prior_fuel = identity.3;
                for ((expr, typ), expected) in cases.iter().zip(&expected) {
                    assert_eq!(row.evaluate("record",expr,typ)?, *expected);
                    bytes += serde_json::to_vec(expected).unwrap().len();
                    let now = row.snapshot_for_test();
                    assert_eq!((now.0,now.1,now.2),(identity.0,identity.1,identity.2));
                    assert!(now.3 < prior_fuel);
                    assert_eq!(now.4, bytes);
                    prior_fuel = now.3;
                }
                Ok(())
            })
        });
    assert_eq!(result, Ok(()));
    assert_eq!(validation_counts().captures,1);
    assert_eq!(validation_counts().apps,2); // Native attempt plus freshly parsed alternate-source V1.
}

#[test]
fn capture_row_no_row_is_lazy_and_contexts_never_share_source_or_frame() {
    let (program, data, reference) = prepared();
    let mut invalid_program = program.clone();
    invalid_program.source_bytes = b"not json".to_vec();
    let runtime = LocalRuntime::default();
    let mut host = TestHost::new();
    reset_validation_counts();
    let no_row: std::result::Result<(),OuterError> = runtime.with_projection_capture(
        &invalid_program,&data,-800000,&mut host, |_| Ok(()));
    assert_eq!(no_row,Ok(()));
    assert_eq!(validation_counts(),ValidationCounts::default());
    assert!(host.left<10_000_000); // positive runtime/source scope header, no reserve or validation
    assert!(host.left>9_900_000);
    for bad_frame in [false,true] {
        let mut frame = data.clone();
        if bad_frame { frame.records[0].revision = 0; }
        let source = if bad_frame { &program } else { &invalid_program };
        let old = runtime.evaluate_record_projection(source,&frame,&reference,"record",&yes(),&Type::Boolean,20000);
        let mut host = TestHost::new();
        let new: std::result::Result<DataValue,AdapterError> = runtime.with_projection_capture(
            source,&frame,20000,&mut host, |capture| capture.with_row(
                &reference,"record",&yes(),&Type::Boolean, |_,value| Ok(value)));
        assert_eq!(new,old);
        assert!(host.left < 9_900_000); // the failed factory spent its one actual reserve
    }
}

#[test]
fn capture_row_bootstrap_error_order_matches_ordinary_api() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    for case in 0..7 {
        let mut source = program.clone();
        let mut frame = data.clone();
        let mut row = reference.clone();
        let mut binding = "record";
        let mut day = 20000;
        let mut expr = yes();
        let mut expected = Type::Boolean;
        match case {
            0 => { source.source_bytes = b"bad".to_vec(); frame.version = 0; day = -800000; },
            1 => { frame.version = 0; day = -800000; binding = "bad binding"; },
            2 => { day = -800000; binding = "bad binding"; row.record = "missing".into(); },
            3 => { binding = "bad binding"; row.record = "missing".into(); },
            4 => { row.record = "missing".into(); expr = Expr::Variable { name:"missing".into() }; },
            5 => { expr = Expr::Variable { name:"missing".into() }; expected = Type::Text; },
            _ => expected = Type::Text,
        }
        let old = runtime.evaluate_record_projection(&source,&frame,&row,binding,&expr,&expected,day);
        assert!(old.is_err());
        let mut host = TestHost::new();
        let new: std::result::Result<DataValue,AdapterError> = runtime.with_projection_capture(
            &source,&frame,day,&mut host, |capture| capture.with_row(
                &row,binding,&expr,&expected, |_,value| Ok(value)));
        assert_eq!(new,old,"case {case}");
    }
}

#[test]
fn capture_row_runtime_error_is_sticky_but_unrelated_scope_error_is_preserved() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    let mut host = TestHost::new();
    let scope: std::result::Result<(),OuterError> = runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |_,_| Err(OuterError::Scope("selected root is already completed"))));
    assert_eq!(scope,Err(OuterError::Scope("selected root is already completed")));
    let overflow = expression(json!({"kind":"add","left":integer(i64::MAX),"right":integer(1)}));
    let mut host = TestHost::new();
    let sticky: std::result::Result<(),OuterError> = runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |row,_| {
                let first = row.evaluate("record",&overflow,&Type::Integer).unwrap_err();
                assert_eq!(first,AdapterError::Failed("integer overflow".into()));
                Err(OuterError::Scope("must not replace first runtime cause"))
            }));
    assert_eq!(sticky,Err(OuterError::Runtime(AdapterError::Failed("integer overflow".into()))));
}

#[test]
fn capture_row_cumulative_real_fuel_refuses_while_ordinary_calls_still_succeed() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    let expr = expression(fixture::text(&"x".repeat(256)));
    assert!(runtime.evaluate_record_projection(&program,&data,&reference,"record",&expr,&Type::Text,20000).is_ok());
    let mut host = TestHost::new();
    let mut entered = false;
    let mut successful_evaluations = 0usize;
    let result: std::result::Result<(),AdapterError> = runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |row,_| {
                entered = true;
                let identity = row.snapshot_for_test();
                // Native/old ticks plus emitted bytes alone force refusal well before1000 calls.
                for _ in 0..1000 {
                    match row.evaluate("record",&expr,&Type::Text) {
                        Ok(_) => {
                            let now=row.snapshot_for_test();
                            assert_eq!((now.0,now.1,now.2),(identity.0,identity.1,identity.2));
                            successful_evaluations += 1;
                        },
                        Err(error) => {
                            assert_eq!(error,AdapterError::BudgetExhausted("evaluation fuel exhausted".into()));
                            return Ok(()); // outer must reject a swallowed failure
                        }
                    }
                }
                panic!("one actual100k meter was replenished")
            }));
    assert!(entered, "bootstrap must reach the row callback");
    assert!(successful_evaluations > 0, "at least one same-row evaluation must succeed before fuel refusal");
    assert_eq!(result,Err(AdapterError::BudgetExhausted("evaluation fuel exhausted".into())));
    assert!(runtime.evaluate_record_projection(&program,&data,&reference,"record",&expr,&Type::Text,20000).is_ok());
}

#[test]
fn capture_row_null_output_boundary_uses_host_type_and_counts_lifecycle() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    let lifecycle_bytes = serde_json::to_vec(&DataValue::Boolean { value:true }).unwrap().len();
    assert_eq!(serde_json::to_vec(&DataValue::Null).unwrap(),br#"{"kind":"null"}"#);
    for remaining in [14,15,29,30] {
        let mut host = TestHost::new();
        host.output_limit = lifecycle_bytes + remaining;
        let typ = Type::Optional { item:Box::new(Type::Boolean) };
        let mut entered = false;
        let result: std::result::Result<(),AdapterError> = runtime.with_projection_capture(
            &program,&data,20000,&mut host, |capture| capture.with_row(
                &reference,"record",&yes(),&Type::Boolean, |row,_| {
                    entered = true;
                    if remaining == 14 {
                        assert_eq!(row.account_value(&DataValue::Null,&typ),
                            Err(AdapterError::BudgetExhausted("cumulative output limit".into())));
                        return Ok(()); // owning finalizer must retain this exact first refusal
                    }
                    row.account_value(&DataValue::Null,&typ)?;
                    assert_eq!(row.snapshot_for_test().4,lifecycle_bytes+15);
                    if remaining >=29 {
                        let second = row.account_value(&DataValue::Null,&typ);
                        if remaining == 29 {
                            assert_eq!(second,Err(AdapterError::BudgetExhausted("cumulative output limit".into())));
                            return Ok(());
                        }
                        second?;
                        assert_eq!(row.snapshot_for_test().4,lifecycle_bytes+30);
                    }
                    Ok(())
                }));
        assert!(entered, "bootstrap must reach the intended output accounting callback");
        if remaining==15 || remaining==30 { assert_eq!(result,Ok(())); }
        else { assert_eq!(result,Err(AdapterError::BudgetExhausted("cumulative output limit".into()))); }
    }
}

#[test]
fn capture_row_both_actual_tokens_latch_first_cause_after_token_clears() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    for additional in [false,true] {
        let mut host = TestHost::new();
        let token = if additional { host.additional.as_ref().unwrap().clone() } else { host.cancelled.clone() };
        let result: std::result::Result<(),AdapterError> = runtime.with_projection_capture(
            &program,&data,20000,&mut host, |capture| capture.with_row(
                &reference,"record",&yes(),&Type::Boolean, |row,_| {
                    token.store(true,Ordering::Relaxed);
                    assert_eq!(row.evaluate("record",&yes(),&Type::Boolean),Err(AdapterError::Cancelled));
                    token.store(false,Ordering::Relaxed);
                    Ok(())
                }));
        assert_eq!(result,Err(AdapterError::Cancelled));
    }
}

#[test]
fn capture_row_actual_host_deadline_expires_between_expressions() {
    let (program, data, reference) = prepared();
    let runtime = LocalRuntime::default();
    let mut host = TestHost::new();
    // Give complete cold validation time; the callback then waits past the actual host deadline.
    host.millis = 2_000;
    let deadline = host.started + Duration::from_millis(host.millis);
    let mut entered = false;
    let result: std::result::Result<(),AdapterError> = runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |row,_| {
                entered = true;
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5));
                row.evaluate("record",&yes(),&Type::Boolean)?;
                Ok(())
            }));
    assert!(matches!(result,Err(AdapterError::BudgetExhausted(_))));
    assert!(entered, "cold bootstrap must have reached the callback before this timing assertion");
}

#[test]
fn capture_row_reservation_to_factory_delay_cannot_restart_five_second_window() {
    struct DelayedHost(TestHost);
    impl ContractWork for DelayedHost {
        fn charge(&mut self, units:u64)->std::result::Result<(),WorkFailure> { self.0.charge(units) }
    }
    impl ProjectionHost for DelayedHost {
        fn reserve_projection_row(&mut self)->std::result::Result<ProjectionReservation,WorkFailure> {
            let reserved = self.0.reserve_projection_row()?;
            // Actual elapsed time, after the real reservation and before the row factory.
            std::thread::sleep(Duration::from_millis(5_020));
            Ok(reserved)
        }
    }
    let (program,data,reference)=prepared();
    let mut host=DelayedHost(TestHost::new());
    reset_validation_counts();
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |_,_| panic!("expired row escaped bootstrap")));
    assert!(matches!(result,Err(AdapterError::BudgetExhausted(_))));
    assert_eq!(validation_counts(),ValidationCounts::default());
    assert!(host.0.left <9_900_000);
}

#[test]
fn capture_row_query_keeps_id_order_for_first_predicate_failure() {
    let (program,mut data,reference)=prepared();
    let mut second=data.records[0].clone();
    second.id="a".into();
    second.values.insert("name".into(),fixture::string("overflow"));
    data.records[0].id="z".into();
    data.records.push(second);
    let first_ref=RecordRef{entity:reference.entity,record:"z".into()};
    let missing=json!({"kind":"literal","value_type":{"kind":"reference","entity":"person"},
        "value":{"kind":"reference","entity":"person","record":"missing"}});
    let predicate=json!({"kind":"if","condition":{"kind":"equal","left":fixture::field(fixture::var("q"),"name"),"right":fixture::text("overflow")},
        "then_value":{"kind":"equal","left":{"kind":"add","left":integer(i64::MAX),"right":integer(1)},"right":integer(0)},
        "else_value":{"kind":"equal","left":missing.clone(),"right":missing}});
    let expr=expression(json!({"kind":"query","entity":"person","binding":"q","predicate":predicate,
        "sort":[],"limit":100,"include_archived":false}));
    let typ=Type::list(Type::reference("person"));
    let runtime=LocalRuntime::default();
    let old=runtime.evaluate_record_projection(&program,&data,&first_ref,"record",&expr,&typ,20000);
    assert_eq!(old,Err(AdapterError::Failed("integer overflow".into())));
    let mut host=TestHost::new();
    let new:std::result::Result<DataValue,AdapterError>=runtime.with_projection_capture(
        &program,&data,20000,&mut host, |capture| capture.with_row(
            &first_ref,"record",&expr,&typ, |_,value| Ok(value)));
    assert_eq!(new,old);
}

#[test]
fn capture_row_source_session_checks_are_once_per_capture_and_never_reused_next_capture() {
    let (program,data,reference)=prepared();
    let runtime=LocalRuntime::default();
    for _ in 0..2 {
        reset_validation_counts();
        let mut host=TestHost::new();
        let result:std::result::Result<(),AdapterError>=runtime.with_projection_capture(
            &program,&data,20000,&mut host, |capture| {
                for _ in 0..3 {
                    capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row,_| {
                        assert_eq!(row.evaluate("record",&Expr::Today,&Type::Date)?,DataValue::Date{days:20000});
                        Ok::<(),AdapterError>(())
                    })?;
                }
                Ok(())
            });
        assert_eq!(result,Ok(()));
        assert_eq!(validation_counts().captures,1);
        assert_eq!(validation_counts().apps,2); // Native attempt plus freshly parsed alternate-source V1.
        assert!(host.left < 9_700_000);
    }
}

#[test]
fn capture_row_query_preserves_mismatched_schema_comparator_outcome() {
    let (program,mut data,reference)=prepared();
    data.schema[0].fields.iter_mut().find(|x|x.id=="name").unwrap().value_type=
        Type::Optional{item:Box::new(Type::Text)};
    let mut null_row=data.records[0].clone();
    null_row.id="a".into();
    null_row.values.insert("name".into(),DataValue::Null);
    data.records.push(null_row);
    data.validate().unwrap(); // Both inputs are valid independently, without schema equality.
    let expr=expression(json!({"kind":"query","entity":"person","binding":"q","predicate":fixture::yes(),
        "sort":[{"value":fixture::field(fixture::var("q"),"name"),"descending":false}],
        "limit":100,"include_archived":true}));
    let typ=Type::list(Type::reference("person"));
    let runtime=LocalRuntime::default();
    let old=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
        runtime.evaluate_record_projection(&program,&data,&reference,"record",&expr,&typ,20000)));
    let mut host=TestHost::new();
    let new=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result:std::result::Result<DataValue,AdapterError>=runtime.with_projection_capture(
            &program,&data,20000,&mut host, |capture|capture.with_row(
                &reference,"record",&expr,&typ, |_,value|Ok(value)));
        result
    }));
    match (old,new) {
        (Ok(left),Ok(right))=>assert_eq!(left,right),
        (Err(_),Err(_))=>{},
        _=>panic!("the existing native comparator result/panic boundary changed"),
    }
}

#[test]
fn capture_row_host_refusal_is_operation_sticky_without_changing_underlying_host_rule() {
    let (program,data,reference)=prepared();
    let mut host=TestHost::new();
    let mut reached_deliberate_charge=false;
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture|capture.with_row(
            &reference,"record",&yes(),&Type::Boolean, |row,_| {
                reached_deliberate_charge=true;
                let first=row.charge_host(u64::MAX).unwrap_err();
                assert_eq!(first,AdapterError::BudgetExhausted("cumulative proof/initialization budget exhausted".into()));
                Ok(())
            }));
    assert!(reached_deliberate_charge, "bootstrap must reach the deliberate host refusal");
    assert_eq!(result,Err(AdapterError::BudgetExhausted("cumulative proof/initialization budget exhausted".into())));
    host.charge(1).unwrap(); // only this closed operation latches; the underlying host does not
}
