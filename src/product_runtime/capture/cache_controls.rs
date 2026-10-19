//! Reads real private cache fields; never fabricates successful cache state.
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

#[test]
fn capture_row_complete_cache_keys_hit_and_saturate_without_value_reuse() {
    let (program,data,reference)=prepared();
    let mut host=TestHost::new();
    let result:std::result::Result<(),AdapterError>=LocalRuntime::default().with_projection_capture(
        &program,&data,20000,&mut host, |capture| {
            for row_index in 0..12 {
                capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row,first| {
                    assert_eq!(first,DataValue::Boolean{value:true});
                    for column in 0..24 {
                        let binding=format!("bind_{}",row_index*24+column);
                        assert_eq!(row.evaluate(&binding,&yes(),&Type::Boolean)?,DataValue::Boolean{value:true});
                    }
                    Ok::<(),AdapterError>(())
                })?;
                let old_complete_key=canonical_bytes(&(0usize,&reference.entity,"record",&yes(),&Type::Boolean,&RuntimeLimits::default())).unwrap();
                assert!(capture.cache.contains(&old_complete_key));
                assert!(capture.cache.len()<=MAX_ITEMS);
                assert_eq!(capture.cache_bytes,capture.cache.iter().map(Vec::len).sum::<usize>());
                assert!(capture.cache_bytes<=MAX_WIRE_BYTES);
            }
            assert_eq!(capture.cache.len(),MAX_ITEMS);
            let retained=capture.cache.clone();
            capture.with_row(&reference,"record",&yes(),&Type::Boolean, |row,_| {
                let before=row.snapshot_for_test();
                assert_eq!(row.evaluate("bind_0",&yes(),&Type::Boolean)?,DataValue::Boolean{value:true});
                let after=row.snapshot_for_test();
                assert!(after.3<before.3);
                assert_eq!(after.4,before.4+31);
                Ok::<(),AdapterError>(())
            })?;
            assert_eq!(capture.cache,retained);
            Ok(())
        });
    assert_eq!(result,Ok(()));
}
