//! Synthetic controls for the closed source-fact handoff, not product acceptance.
use super::*;
use super::capture_validation::*;
use std::cell::Cell;
#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;
#[path = "../../tests/support/product_contract_fixture.rs"]
mod rich;
#[derive(Default)]
struct Work { calls: Vec<u64>, fail: Option<usize> }
impl ContractWork for Work {
    fn charge(&mut self, n: u64) -> std::result::Result<(), WorkFailure> {
        self.calls.push(n);
        if self.fail == Some(self.calls.len()) { Err(WorkFailure::Cancelled) } else { Ok(()) }
    }
}
#[derive(Debug, PartialEq)]
enum Error { Source(String), Work(ContractWorkError), Prepare }
impl From<ContractError> for Error { fn from(e: ContractError) -> Self { Self::Source(e.0) } }
impl From<ContractWorkError> for Error { fn from(e: ContractWorkError) -> Self { Self::Work(e) } }
struct Prepare<'a> { visits: &'a Cell<usize>, fail: bool }
impl SourcePreparation<Work> for Prepare<'_> {
    type Prepared = ();
    type Error = Error;
    fn prepare(self, _: &mut Work) -> std::result::Result<(), Error> {
        self.visits.set(self.visits.get()+1);
        if self.fail { Err(Error::Prepare) } else { Ok(()) }
    }
}
struct Consume<'s, 'a> { data: &'s DataSnapshot, visits: &'a Cell<usize> }
impl<'s> SourceConsumer<'s, Work, ()> for Consume<'s, '_> {
    type Output = [usize; 4];
    type Error = Error;
    fn consume<'g>(self, ready: ReadyCaptureSource<'g, 's, Work>, _: ()) -> std::result::Result<Self::Output, Error> {
        self.visits.set(self.visits.get()+1);
        ready.into_capture_validation(self.data, 17, |host| ForwardWork(host), |ctx| {
            let f=ctx.facts.as_ref().unwrap();
            let result=[f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count];
            ctx.ensure_source_and_data()?;
            Ok(result)
        })
    }
}
struct ForwardWork<'a>(&'a mut Work);
impl ContractWork for ForwardWork<'_> { fn charge(&mut self,n:u64)->std::result::Result<(),WorkFailure>{self.0.charge(n)} }
fn source(pretty:bool)->CapturedProgram {
    let app: AppDefinition=serde_json::from_value(fixture::minimal()).unwrap();
    let bytes=if pretty {serde_json::to_vec_pretty(&app).unwrap()} else {serde_json::to_vec(&app).unwrap()};
    CapturedProgram::capture(&bytes,"project",Producer::UserAuthored,None).unwrap()
}
#[test]
fn retained_facts_equal_fresh_paid_facts_and_data_stays_cold() {
    for value in [fixture::minimal(),rich::organizer(),rich::equipment(),rich::new_composition()] {
      let app:AppDefinition=serde_json::from_value(value.clone()).unwrap();
      for bytes in [serde_json::to_vec(&app).unwrap(),serde_json::to_vec(&value).unwrap(),serde_json::to_vec_pretty(&value).unwrap()] {
        let source=CapturedProgram::capture(&bytes,"project",Producer::UserAuthored,None).unwrap();
        let data=DataSnapshot::empty("project",&source.program).unwrap();
        let mut work=Work::default();
        let paid=with_capture_validation(&source,&data,&mut work,|ctx|->std::result::Result<_,Error>{
            ctx.ensure_source_and_data()?;let f=ctx.facts.as_ref().unwrap();
            Ok([f.total_nodes,f.action_prefix_nodes,f.canonical_len,f.action_count])
        }).unwrap();
        let prepare=Cell::new(0);let consume=Cell::new(0);reset_validation_counts();
        let actual=with_ordinary_source_facts(&source,&mut Work::default(),Prepare{visits:&prepare,fail:false},Consume{data:&data,visits:&consume}).unwrap();
        assert_eq!(actual,paid); assert_eq!((prepare.get(),consume.get()),(1,1));
        assert_eq!(validation_counts().captures,1);assert_eq!(validation_counts().apps,3);
        let mut bad=data.clone();bad.version=0;
        assert!(with_ordinary_source_facts(&source,&mut Work::default(),Prepare{visits:&prepare,fail:false},Consume{data:&bad,visits:&consume}).is_err());
      }
    }
}
#[test]
fn retained_source_and_preparation_errors_do_not_publish_or_poll() {
    let mut source=source(false);let data=DataSnapshot::empty("project",&source.program).unwrap();
    let prepare=Cell::new(0);let consume=Cell::new(0);let mut work=Work::default();
    assert_eq!(with_ordinary_source_facts(&source,&mut work,Prepare{visits:&prepare,fail:true},Consume{data:&data,visits:&consume}),Err(Error::Prepare));
    assert_eq!((prepare.get(),consume.get()),(1,0));assert!(work.calls.is_empty());
    source.binding.program_path="../escape".into();let expected=source.validate().unwrap_err().0;
    assert_eq!(with_ordinary_source_facts(&source,&mut work,Prepare{visits:&prepare,fail:false},Consume{data:&data,visits:&consume}),Err(Error::Source(expected)));
    assert_eq!((prepare.get(),consume.get()),(1,0));assert!(work.calls.is_empty());
}
#[test]
fn retained_entry_refusal_never_calls_wrap_body_or_cleanup() {
    struct FailConsume<'s>(&'s DataSnapshot);
    impl<'s> SourceConsumer<'s,Work,()> for FailConsume<'s> {
        type Output=();type Error=Error;
        fn consume<'g>(self,ready:ReadyCaptureSource<'g,'s,Work>,_:())->std::result::Result<(),Error>{
            ready.into_capture_validation(self.0,17,|_: &'g mut Work|->ForwardWork<'g>{panic!("wrap after ENTRY refusal")},|_|{panic!("body after ENTRY refusal")})
        }
    }
    let source=source(false);let data=DataSnapshot::empty("project",&source.program).unwrap();let prepare=Cell::new(0);
    let mut work=Work{calls:vec![],fail:Some(1)};
    assert_eq!(with_ordinary_source_facts(&source,&mut work,Prepare{visits:&prepare,fail:false},FailConsume(&data)),Err(Error::Work(ContractWorkError::Resource(WorkFailure::Cancelled))));
    assert_eq!(work.calls,vec![17]);
}

impl CapturedProgram {
    pub(crate) fn retained_source_payments_for_test(&self) -> (u64, u64) {
        use super::capture_work::WorkState;
        let mut work=Work::default();
        let state=WorkState::new(&mut work);
        state.finish(self.validate_audited(Audit(Some(&state)))).unwrap();
        let body=work.calls.iter().sum();
        // Original source run_host Forward/WorkState frame and publication5.
        let wrapper=8 + std::mem::size_of::<&mut Work>() as u64
            + std::mem::size_of::<WorkState>() as u64 + 5;
        (body,wrapper)
    }
}

#[test]
fn retained_context_refusals_never_enter_body_or_poll_after_failure() {
    struct Probe<'s,'a>{data:&'s DataSnapshot,wraps:&'a Cell<usize>,bodies:&'a Cell<usize>}
    impl<'s> SourceConsumer<'s,Work,()> for Probe<'s,'_>{
        type Output=();type Error=Error;
        fn consume<'g>(self,ready:ReadyCaptureSource<'g,'s,Work>,_:())->std::result::Result<(),Error>{
            ready.into_capture_validation(self.data,17,|host|{self.wraps.set(self.wraps.get()+1);ForwardWork(host)},|_|{self.bodies.set(self.bodies.get()+1);Ok(())})
        }
    }
    let source=source(false);let data=DataSnapshot::empty("project",&source.program).unwrap();
    for fail in [2,3] {
        let prepare=Cell::new(0);let wraps=Cell::new(0);let bodies=Cell::new(0);let mut work=Work{calls:vec![],fail:Some(fail)};
        assert_eq!(with_ordinary_source_facts(&source,&mut work,Prepare{visits:&prepare,fail:false},Probe{data:&data,wraps:&wraps,bodies:&bodies}),Err(Error::Work(ContractWorkError::Resource(WorkFailure::Cancelled))));
        assert_eq!(wraps.get(),1);assert_eq!(bodies.get(),0);assert_eq!(work.calls.len(),fail);
        assert_eq!(work.calls[0],17);assert_eq!(work.calls[1],0);
    }
}
