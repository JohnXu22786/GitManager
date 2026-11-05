//! Counter seam controls: complete chunks, sticky failure and finite reservations.
use super::*;
use super::app_length_tests::Work;
use std::io::Write;
fn shape(a:Audit<'_>)->Shape<'_> {Shape{audit:a,bytes:100,nodes:10,pairs:4,native:500,canonical:false,digest_copies:0,escaped:3,simple_app:true}}
#[test]
fn synthetic_count_complete_writes_overflow_and_native_errors() {
    let mut w=Work::default();
    {
        let state=WorkState::new(&mut w);let a=Audit(Some(&state));a.pay(SYNTHETIC_COUNT_ROOT as u64).unwrap();reserve_synthetic_count(&shape(a)).unwrap();
        let mut c=AppLengthCounter{audit:a,size:0,overflowed:false};assert_eq!(c.write(&[]).unwrap(),0);c.write_all(&[]).unwrap();assert_eq!(c.write(b"abc").unwrap(),3);
        let large=vec![0;MAX_WIRE_BYTES+1];c.write_all(&large).unwrap();c.flush().unwrap();assert_eq!(state.finish(c.finish(Ok(()))),Ok(MAX_WIRE_BYTES+4));
    }
    assert_eq!(w.calls.last(),Some(&0));
    for native in [false,true] {let mut w=Work::default();
        let result={let state=WorkState::new(&mut w);let a=Audit(Some(&state));a.pay(SYNTHETIC_COUNT_ROOT as u64).unwrap();reserve_synthetic_count(&shape(a)).unwrap();
            let mut c=AppLengthCounter{audit:a,size:usize::MAX-1,overflowed:false};assert_eq!(c.write(b"x").unwrap(),1);
            let result=if native {let e=serde_json::to_writer(&mut c,&true);assert!(e.as_ref().unwrap_err().is_io());e}else{assert!(c.write(b"x").is_err());Ok(())};
            assert!(c.write_all(&[]).is_err());assert!(c.write(b"later").is_err());assert_eq!(c.size,usize::MAX);state.finish(c.finish(result))};
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));assert!(!w.calls.contains(&0));
    }
    let mut w=Work::default();let error=serde_json::from_str::<JsonValue>("{").unwrap_err();let text=error.to_string();
    let result={let state=WorkState::new(&mut w);let a=Audit(Some(&state));a.pay(SYNTHETIC_COUNT_ROOT as u64).unwrap();reserve_synthetic_count(&shape(a)).unwrap();let c=AppLengthCounter{audit:a,size:17,overflowed:false};state.finish(c.finish(Err(error)))};
    assert_eq!(result,Err(ContractWorkError::Contract(ContractError(text))));assert!(!w.calls.contains(&0));
    let mut w=Work{refuse:Some(1),..Work::default()};let result={let state=WorkState::new(&mut w);let a=Audit(Some(&state));assert!(a.pay(0).is_err());let mut c=AppLengthCounter{audit:a,size:usize::MAX,overflowed:false};assert!(c.write(b"x").is_err());state.finish(c.finish(Ok(())))};
    assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::Cancelled)));assert_eq!(w.calls,[0]);
}
#[test]
#[cfg(target_pointer_width="64")]
fn synthetic_count_reservation_overflows_stay_typed() {
    assert_eq!(SYNTHETIC_COUNT_ROOT,304);
    assert_eq!(size_of::<AppLengthCounter<'_>>(),24);
    assert_eq!(size_of::<serde_json::Serializer<&mut AppLengthCounter<'_>>>(),8);
    assert_eq!(size_of::<serde_json::Result<()>>(),8);
    assert_eq!(size_of::<Result<usize>>(),24);
    assert_eq!(size_of::<Result<()>>(),24);
    assert_eq!(size_of::<Option<u64>>(),16);
    for (nodes,escaped,bytes,native) in [(u64::MAX,0,0,0),(0,u64::MAX,0,0),(u64::MAX/4,2,0,0),(u64::MAX/56+1,0,0,0),(0,0,1,0),(0,0,0,u64::MAX)] {
        let mut w=Work::default();let result={let state=WorkState::new(&mut w);let a=Audit(Some(&state));let mut s=shape(a);s.nodes=nodes;s.escaped=escaped;s.bytes=bytes;s.native=native;state.finish(reserve_synthetic_count(&s))};
        assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));assert_eq!(w.calls,[128]);
    }
}
#[test]
#[cfg(target_pointer_width="32")]
fn synthetic_count_usize_overflow_is_checked_on_32_bit() {
    let mut w=Work::default();let result={let state=WorkState::new(&mut w);let a=Audit(Some(&state));let mut s=shape(a);s.bytes=u64::from(u32::MAX)+1;state.finish(reserve_synthetic_count(&s))};
    assert_eq!(result,Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));assert_eq!(w.calls,[128]);
}
