//! Real retained-runtime error/lifetime controls; no product operation is run.
use super::*;
use super::super::capture_row_controls::{TestHost, prepared, yes};
use std::time::Duration;
struct Host {
    inner: TestHost,
    primary: Arc<AtomicBool>,
    secondary: Arc<AtomicBool>,
    deadline: Option<Instant>,
    row_short: bool,
}
impl Host {
    fn new() -> Self { Self { inner: TestHost::new(), primary: Arc::new(AtomicBool::new(false)),
        secondary: Arc::new(AtomicBool::new(false)), deadline: None, row_short: false } }
}
impl ContractWork for Host {
    fn charge(&mut self,n:u64)->std::result::Result<(),WorkFailure>{
        if self.primary.load(Ordering::Relaxed)||self.secondary.load(Ordering::Relaxed){return Err(WorkFailure::Cancelled)}
        if self.deadline.is_some_and(|d|Instant::now()>=d){return Err(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))}
        self.inner.charge(n)
    }
}
impl ProjectionHost for Host {
    fn reserve_projection_row(&mut self)->std::result::Result<ProjectionReservation,WorkFailure>{
        let mut r=self.inner.reserve_projection_row()?;
        r.cancelled=self.primary.clone();r.additional_cancelled=Some(self.secondary.clone());
        if let Some(d)=self.deadline{r.host_deadline=d;}
        if self.row_short{r.row_deadline=Instant::now()+Duration::from_millis(100);}
        Ok(r)
    }
}
struct Prepare;
impl SourcePreparation<Host> for Prepare {
    type Prepared=();type Error=AdapterError;
    fn prepare(self,_:&mut Host)->Result<()>{Ok(())}
}
struct Consume<'s>{data:&'s DataSnapshot,reference:&'s RecordRef,mode:&'static str,primary:Arc<AtomicBool>,secondary:Arc<AtomicBool>}
impl<'s> SourceConsumer<'s,Host,()> for Consume<'s>{
    type Output=();type Error=AdapterError;
    fn consume<'g>(self,ready:ReadyCaptureSource<'g,'s,Host>,_:())->Result<()>{
        LocalRuntime::default().with_retained_projection_capture(ready,self.data,20000,move|capture|{
            capture.with_row(self.reference,"record",&yes(),&Type::Boolean,|row,first|{
                assert_eq!(first,DataValue::Boolean{value:true});
                let expected=match self.mode {
                    "primary"=>{self.primary.store(true,Ordering::Relaxed);AdapterError::Cancelled},
                    "secondary"=>{self.secondary.store(true,Ordering::Relaxed);AdapterError::Cancelled},
                    "host_elapsed"=>{std::thread::sleep(Duration::from_millis(120));AdapterError::BudgetExhausted("cumulative elapsed budget exhausted".into())},
                    "row_elapsed"=>{std::thread::sleep(Duration::from_millis(120));AdapterError::BudgetExhausted("elapsed time limit".into())},
                    "fuel"=>AdapterError::BudgetExhausted("cumulative proof/initialization budget exhausted".into()),
                    _=>unreachable!(),
                };
                let amount=if self.mode=="fuel"{u64::MAX}else{0};
                assert_eq!(row.charge_host(amount),Err(expected.clone()));
                self.primary.store(false,Ordering::Relaxed);self.secondary.store(false,Ordering::Relaxed);
                assert_eq!(row.charge_host(0),Err(expected));
                Ok(()) // Deliberately swallow; the unchanged finalizers must refuse.
            })
        })
    }
}
#[test]
fn retained_source_runtime_keeps_sticky_tokens_deadlines_and_fuel(){
    let(program,data,reference)=prepared();
    for mode in ["fuel","primary","secondary","host_elapsed","row_elapsed"] {
        let mut host=Host::new();
        if mode=="host_elapsed"{host.deadline=Some(Instant::now()+Duration::from_millis(100));}
        host.row_short=mode=="row_elapsed";
        let consume=Consume{data:&data,reference:&reference,mode,primary:host.primary.clone(),secondary:host.secondary.clone()};
        reset_validation_counts();
        let result=with_ordinary_source_facts(&program,&mut host,Prepare,consume);
        let expected=match mode {
            "primary"|"secondary"=>AdapterError::Cancelled,
            "host_elapsed"=>AdapterError::BudgetExhausted("cumulative elapsed budget exhausted".into()),
            "row_elapsed"=>AdapterError::BudgetExhausted("elapsed time limit".into()),
            _=>AdapterError::BudgetExhausted("cumulative proof/initialization budget exhausted".into()),
        };
        assert_eq!(result,Err(expected),"{mode}");
        assert_eq!(validation_counts().captures,1,"only original ordinary source validation");
        if mode=="fuel"{host.charge(1).unwrap();}
    }
}
