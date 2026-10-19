//! Real-row cache controls and isolated private publisher admission mechanics.
use super::super::*;
use super::super::capture_row_controls::{TestHost, prepared, expression, integer, yes};
use serde_json::json;

#[test]
fn capture_row_cache_commits_only_after_expected_eval_and_output_success() {
    let (program,data,reference)=prepared();
    let runtime=LocalRuntime::default();
    for case in 0..3 {
        let mut host=TestHost::new();
        if case==2 {host.output_limit=30;} // true's tagged representation is31 bytes
        let expr=if case==1 {expression(json!({"kind":"add","left":integer(i64::MAX),"right":integer(1)}))}else{yes()};
        let expected=if case==0{Type::Text}else if case==1{Type::Integer}else{Type::Boolean};
        let expected_error=match case {
            0=>AdapterError::Invalid(ContractError("projection result type mismatch".into())),
            1=>AdapterError::Failed("integer overflow".into()),
            _=>AdapterError::BudgetExhausted("cumulative output limit".into()),
        };
        let result:std::result::Result<(),AdapterError>=runtime.with_projection_capture(
            &program,&data,20000,&mut host, |capture| {
                let row_result:std::result::Result<(),AdapterError>=capture.with_row(
                    &reference,"record",&expr,&expected, |_,_|Ok(()));
                assert_eq!(row_result,Err(expected_error.clone()));
                assert!(capture.cache.is_empty());
                assert_eq!(capture.cache_bytes,0);
                Ok(()) // sticky finalization must refuse despite callback success
            });
        assert_eq!(result,Err(expected_error));
    }
}

fn complete_key(reference:&RecordRef,binding:&str,limits:&RuntimeLimits)->Vec<u8> {
    canonical_bytes(&(0usize,&reference.entity,binding,&yes(),&Type::Boolean,limits)).unwrap()
}

// Observe only completed expressions and count only newly retained real keys.
fn observe_publication(cache:&BTreeSet<Vec<u8>>,bytes:usize,key:Vec<u8>,
    successful:&mut BTreeSet<Vec<u8>>,publications:&mut usize)
{
    if !successful.contains(&key) && cache.contains(&key) {
        assert_eq!(bytes,successful.iter().map(Vec::len).sum::<usize>()+key.len());
        successful.insert(key);
        *publications+=1;
    }
    assert_eq!(cache,&*successful);
    assert_eq!(bytes,successful.iter().map(Vec::len).sum::<usize>());
    assert!(cache.len()<=MAX_ITEMS);
    assert!(bytes<=MAX_WIRE_BYTES);
}

#[test]
fn capture_row_real_workload_refuses_without_failed_publication() {
    let (program,data,reference)=prepared();
    let mut host=TestHost::new();
    let expected_error=AdapterError::BudgetExhausted(
        "cumulative proof/initialization budget exhausted".into());
    let mut entered=false;
    let mut publications=0;
    let mut attempted_row=None;
    let mut attempted_evaluate=None;
    let mut caught_row=None;
    let mut caught_evaluate=None;
    let mut failed_row_entered=None;
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture| {
            entered=true;
            assert!(capture.cache.is_empty());
            assert_eq!(capture.cache_bytes,0);
            let mut successful=BTreeSet::new();
            // The original twelve 24-expression rows and final retained-hit tail
            // remain requested work. Stop at the first refusal; no tail credit.
            for row_index in 0..=12 {
                let retained=if row_index==12 {Some(capture.cache.clone())}else{None};
                let mut row_entered=false;
                attempted_row=Some(row_index);
                attempted_evaluate=None;
                let row_result:std::result::Result<(),AdapterError>=capture.with_row(
                    &reference,"record",&yes(),&Type::Boolean, |row,first| {
                        row_entered=true;
                        assert_eq!(first,DataValue::Boolean{value:true});
                        let key=complete_key(&reference,"record",&row.evaluator.meter.limits);
                        observe_publication(&row.capture.cache,row.capture.cache_bytes,key,
                            &mut successful,&mut publications);
                        let columns=if row_index==12 {1}else{24};
                        for column in 0..columns {
                            let binding=if row_index==12 {"bind_0".into()}else{
                                format!("bind_{}",row_index*24+column)};
                            let key=complete_key(&reference,&binding,&row.evaluator.meter.limits);
                            let before_cache=row.capture.cache.clone();
                            let before_bytes=row.capture.cache_bytes;
                            let before=row.snapshot_for_test();
                            attempted_evaluate=Some((row_index,binding.clone()));
                            let evaluated=row.evaluate(&binding,&yes(),&Type::Boolean);
                            match evaluated {
                                Ok(value)=>{
                                    assert_eq!(value,DataValue::Boolean{value:true});
                                    observe_publication(&row.capture.cache,row.capture.cache_bytes,key,
                                        &mut successful,&mut publications);
                                    if row_index==12 {
                                        let after=row.snapshot_for_test();
                                        assert!(after.3<before.3);
                                        assert_eq!(after.4,before.4+31);
                                        assert_eq!(row.capture.cache,before_cache);
                                        assert_eq!(row.capture.cache_bytes,before_bytes);
                                    }
                                }
                                Err(error)=>{
                                    caught_evaluate=Some(error.clone());
                                    assert_eq!(row.capture.cache,before_cache);
                                    assert_eq!(row.capture.cache_bytes,before_bytes);
                                    assert_eq!(row.capture.cache,successful);
                                    return Err(error); // propagate immediately through the row
                                }
                            }
                        }
                        Ok(())
                    });
                if let Err(error)=row_result {
                    caught_row=Some(error);
                    failed_row_entered=Some(row_entered);
                    // A first-expression refusal can precede callback entry.
                    // Use the actual successful ledger; invent no absent row key.
                    assert_eq!(capture.cache,successful);
                    assert_eq!(capture.cache_bytes,successful.iter().map(Vec::len).sum::<usize>());
                    assert!(capture.cache.len()<=MAX_ITEMS);
                    assert!(capture.cache_bytes<=MAX_WIRE_BYTES);
                    return Ok(()); // sole swallowed refusal: sticky capture must still fail
                }
                if let Some(retained)=retained {assert_eq!(capture.cache,retained);}
            }
            Ok(())
        });
    // Reachability is independent of sticky finalization and its outer error.
    assert!(entered);
    assert!(publications>0);
    assert!(attempted_row.is_some());
    assert_eq!(caught_row,Some(expected_error.clone()));
    if let Some(error)=caught_evaluate {
        assert!(attempted_evaluate.is_some());
        assert_eq!(failed_row_entered,Some(true));
        assert_eq!(error,expected_error);
    }else{
        assert_eq!(attempted_evaluate,None);
        assert_eq!(failed_row_entered,Some(false));
    }
    assert_eq!(result,Err(expected_error));
}

#[test]
fn capture_row_retained_hit_across_real_rows_spends_fresh_fuel_and_output() {
    let (program,data,reference)=prepared();
    let mut host=TestHost::new();
    let mut completed_rows=0;
    let mut target_reached=false;
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture| {
            let mut retained_key=None;
            let mut expected=BTreeSet::new();
            capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row,first| {
                assert_eq!(first,DataValue::Boolean{value:true});
                expected.insert(complete_key(&reference,"record",&row.evaluator.meter.limits));
                let key=complete_key(&reference,"bind_0",&row.evaluator.meter.limits);
                assert!(!row.capture.cache.contains(&key));
                assert_eq!(row.evaluate("bind_0",&yes(),&Type::Boolean)?,DataValue::Boolean{value:true});
                assert!(row.capture.cache.contains(&key));
                expected.insert(key.clone());
                retained_key=Some(key);
                Ok::<(),AdapterError>(())
            })?;
            completed_rows+=1;
            assert_eq!(capture.cache,expected);
            assert_eq!(capture.cache_bytes,expected.iter().map(Vec::len).sum::<usize>());
            let retained_key=retained_key.unwrap();
            capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row,first| {
                // This row's first expression already did its own real work.
                assert_eq!(first,DataValue::Boolean{value:true});
                let key=complete_key(&reference,"bind_0",&row.evaluator.meter.limits);
                assert_eq!(key,retained_key);
                assert!(row.capture.cache.contains(&key));
                assert_eq!(row.capture.cache,expected);
                let before_cache=row.capture.cache.clone();
                let before_bytes=row.capture.cache_bytes;
                let before=row.snapshot_for_test();
                assert_eq!(before.4,31);
                target_reached=true;
                assert_eq!(row.evaluate("bind_0",&yes(),&Type::Boolean)?,DataValue::Boolean{value:true});
                let after=row.snapshot_for_test();
                assert!(after.3<before.3);
                assert_eq!(after.4,before.4+31);
                assert_eq!(row.capture.cache,before_cache);
                assert_eq!(row.capture.cache_bytes,before_bytes);
                Ok::<(),AdapterError>(())
            })?;
            completed_rows+=1;
            assert_eq!(capture.cache,expected);
            assert_eq!(capture.cache_bytes,expected.iter().map(Vec::len).sum::<usize>());
            Ok(())
        });
    assert_eq!(completed_rows,2);
    assert!(target_reached);
    assert_eq!(result,Ok(()));
}

#[test]
fn capture_publisher_isolated_entry_cap_preserves_retained_keys() {
    let (program,data,reference)=prepared();
    // Ordinary complete keys are fixture inputs, built before the host exists.
    // Direct publication proves admission mechanics only, not validation,
    // evaluation, key-construction affordability or whole-route saturation.
    let mut keys:Vec<_>=(0..=MAX_ITEMS).map(|i|
        complete_key(&reference,&format!("bind_{}",i),&RuntimeLimits::default())).collect();
    let not_admitted=keys.pop().unwrap();
    let expected:BTreeSet<_>=keys.iter().cloned().collect();
    assert_eq!(expected.len(),MAX_ITEMS);
    assert!(!expected.contains(&not_admitted));
    let bytes=expected.iter().map(Vec::len).sum::<usize>();
    assert!(bytes<=MAX_WIRE_BYTES); // invariant only; the byte boundary is not reached
    let mut host=TestHost::new();
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture| {
            assert!(capture.cache.is_empty());
            assert_eq!(capture.cache_bytes,0);
            for key in keys {capture.publish(Some(key))?;}
            assert_eq!(capture.cache,expected);
            assert_eq!(capture.cache_bytes,bytes);
            capture.publish(Some(not_admitted))?;
            assert_eq!(capture.cache,expected);
            assert_eq!(capture.cache_bytes,bytes);
            capture.publish(None)?; // actual hit preparation uses None, not duplicate Some
            assert_eq!(capture.cache,expected);
            assert_eq!(capture.cache_bytes,bytes);
            Ok(()) // no row/evaluation or authority-bearing value leaves this scope
        });
    assert_eq!(result,Ok(()));
}
