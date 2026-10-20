//! Closed capture-local row execution. No checked facts or meters can escape.
use super::*;
use super::capture_cost as cost;
use std::mem::size_of;

pub(crate) trait ProjectionHost: ContractWork {
    fn reserve_projection_row(&mut self) -> std::result::Result<ProjectionReservation,WorkFailure>;
}
pub(crate) struct ProjectionReservation {
    limits: RuntimeLimits,
    host_deadline: Instant,
    row_deadline: Instant,
    cancelled: Arc<AtomicBool>,
    additional_cancelled: Option<Arc<AtomicBool>>,
}
impl ProjectionReservation {
    // Sole production caller: ProofMeter's ProjectionHost implementation.
    pub(crate) fn from_actual_host(limits:RuntimeLimits,host_deadline:Instant,row_deadline:Instant,cancelled:Arc<AtomicBool>,additional_cancelled:Option<Arc<AtomicBool>>)->Self {
        Self{limits,host_deadline,row_deadline,cancelled,additional_cancelled}
    }
}
struct CaptureHost<'a,H:ProjectionHost+?Sized>{
    host:&'a mut H,
    active_row_deadline:Option<Instant>,
    first_failure:Option<WorkFailure>,
}
impl<H:ProjectionHost+?Sized> CaptureHost<'_,H>{
    fn retain<T>(&mut self,result:std::result::Result<T,WorkFailure>)->std::result::Result<T,WorkFailure>{
        if let Err(error)=&result{if self.first_failure.is_none(){self.first_failure=Some(error.clone());}}
        result
    }
    fn reserve(&mut self)->std::result::Result<ProjectionReservation,WorkFailure>{
        self.charge(0)?;
        let result=self.host.reserve_projection_row();self.retain(result)
    }
}
impl<H:ProjectionHost+?Sized> ContractWork for CaptureHost<'_,H>{
    fn charge(&mut self,units:u64)->std::result::Result<(),WorkFailure>{
        if let Some(error)=&self.first_failure{return Err(error.clone())}
        let result=(||{
            self.host.charge(0)?;
            if self.active_row_deadline.is_some_and(|deadline|Instant::now()>=deadline){return Err(WorkFailure::BudgetExhausted("elapsed time limit"))}
            self.host.charge(units)
        })();
        self.retain(result)
    }
}
// Fixed transport keeps caller E an actual error at the source finalizer.
// In particular, it never disguises Err(E) as a successful nested Result.
enum ScopeExit<E>{Caller(E),Contract(ContractWorkError)}
impl<E> From<ContractWorkError> for ScopeExit<E>{fn from(error:ContractWorkError)->Self{Self::Contract(error)}}

enum StepError{Source(AdapterError),Local(AdapterError)}
impl From<AdapterError> for StepError{fn from(error:AdapterError)->Self{Self::Local(error)}}
impl StepError{fn source(error:ContractWorkError)->Self{Self::Source(cost::contract(error))}}

pub(super) struct RowStop{
    host_deadline:Instant,
    row_deadline:Instant,
    additional_cancelled:Option<Arc<AtomicBool>>,
}
impl Meter{
    fn from_reservation(reservation:ProjectionReservation,header:u64)->Result<Self>{
        // Header is a compile-time size expression; this typed debit precedes
        // Meter/RowStop construction and consumes the actual reservation fuel.
        let mut fuel=reservation.limits.fuel;
        row_check(&reservation.cancelled,reservation.additional_cancelled.as_ref(),reservation.host_deadline,reservation.row_deadline,&mut fuel,header).map_err(cost::resource)?;
        let ProjectionReservation{limits,host_deadline,row_deadline,cancelled,additional_cancelled}=reservation;
        let mut meter=Self{limits,fuel,steps:0,writes:0,started:host_deadline,cancelled,
            row:Some(RowStop{host_deadline,row_deadline,additional_cancelled})};
        meter.row_charge(272).map_err(cost::resource)?;
        Ok(meter)
    }
    pub(super) fn row_charge(&mut self,units:u64)->std::result::Result<(),WorkFailure>{
        let stop=self.row.as_ref().expect("row-only work adapter");
        row_check(&self.cancelled,stop.additional_cancelled.as_ref(),stop.host_deadline,stop.row_deadline,&mut self.fuel,units)
    }
    pub(super) fn row_value(&mut self,value:&DataValue)->Result<()>{
        self.tick(1)?;cost::pay(self,2)?;
        match value{
            DataValue::List{items,..}=>{
                self.collection(items.len())?;cost::pay(self,1)?;
                for value in items{cost::pay(self,3)?;self.value(value)?;}
                cost::pay(self,1)?;
            }
            DataValue::Text{value}=>{
                cost::pay(self,3)?;self.tick(value.len() as u64)?;
                if value.len()>MAX_TEXT_BYTES{return Err(exhausted("text byte limit"))}
            }
            _=>{}
        }Ok(())
    }
}
impl ContractWork for Meter{
    fn charge(&mut self,units:u64)->std::result::Result<(),WorkFailure>{self.row_charge(units)}
}
fn row_check(cancelled:&AtomicBool,additional:Option<&Arc<AtomicBool>>,host_deadline:Instant,row_deadline:Instant,fuel:&mut u64,units:u64)->std::result::Result<(),WorkFailure>{
    if cancelled.load(Ordering::Relaxed)||additional.is_some_and(|token|token.load(Ordering::Relaxed)){return Err(WorkFailure::Cancelled)}
    if Instant::now()>=host_deadline{return Err(WorkFailure::BudgetExhausted("cumulative elapsed budget exhausted"))}
    if Instant::now()>=row_deadline{return Err(WorkFailure::BudgetExhausted("elapsed time limit"))}
    if units>*fuel{*fuel=0;return Err(WorkFailure::BudgetExhausted("evaluation fuel exhausted"))}
    *fuel-=units;Ok(())
}

pub(crate) struct ProjectionCapture<'c,'v,'a,'s,H:ProjectionHost+?Sized>{
    validation:&'c mut CaptureValidation<'v,'s,CaptureHost<'a,H>>,
    program:&'s CapturedProgram,
    data:&'s DataSnapshot,
    day:i32,
    cache:Vec<Box<capture_key::OwnedCaptureKey>>,
    cache_bytes:usize,
    first_failure:Option<AdapterError>,
}
impl LocalRuntime{
    pub(crate) fn with_projection_capture<'a,'s,H,R,E>(&self,program:&'s CapturedProgram,data:&'s DataSnapshot,day:i32,host:&'a mut H,
        body:impl for<'c,'v> FnOnce(&mut ProjectionCapture<'c,'v,'a,'s,H>)->std::result::Result<R,E>)->std::result::Result<R,E>
    where H:ProjectionHost+?Sized,E:From<AdapterError>{
        // Includes one136-unit fixed outer report owner for this entire scope.
        host.charge(ProjectionCapture::<H>::ENTRY)
            .map_err(|error|E::from(cost::resource(error)))?;
        let mut host=CaptureHost{host,active_row_deadline:None,first_failure:None};
        let result=with_capture_validation(program,data,&mut host,|validation|{
            let mut capture=ProjectionCapture{validation,program,data,day,cache:Vec::new(),cache_bytes:0,first_failure:None};
            let result=body(&mut capture);
            if let Some(error)=capture.first_failure.take(){return Err(ScopeExit::Caller(E::from(error)))}
            // Source owns its first error; an unrelated caller E passes through.
            result.map_err(ScopeExit::Caller)
        });
        match result{
            Ok(value)=>Ok(value),
            Err(ScopeExit::Caller(error))=>Err(error),
            Err(ScopeExit::Contract(error))=>Err(E::from(cost::contract(error))),
        }
    }
}
impl<'c,'v,'a,'s,H:ProjectionHost+?Sized> ProjectionCapture<'c,'v,'a,'s,H>{
    const ENTRY:u64=153+size_of::<CaptureHost<'a,H>>() as u64+size_of::<Self>() as u64;
    pub(crate) fn charge_host(&mut self,units:u64)->Result<()>{
        // Closed production callbacks immediately propagate this owned report.
        // The outer136 reserve buys one owner, never a swallowed retry/clone.
        self.validation.with_host(|host|host.charge(units)).map_err(cost::resource)
    }
    fn prepare(&mut self,reference:&RecordRef,binding:&str,expression:&Expr,expected:&Type,limits:&RuntimeLimits)->std::result::Result<Option<capture_key::OwnedCaptureKey>,StepError>{
        self.validation.with_host(|work|->Result<()>{
            cost::pay(work,5)?;validate_day(self.day)?;
            cost::pay(work,28)?;limits.validate()?;
            if !cost::eq_string(&self.data.project_id,&self.program.binding.project_id,work)?{return Err(invalid("projection project or binding mismatch"))}
            cost::pay(work,6)?;
            let valid=if binding.is_empty()||binding.len()>128{false}else{
                let mut valid=true;
                for byte in binding.bytes(){cost::pay(work,9)?;if !(byte.is_ascii_lowercase()||byte.is_ascii_digit()||b"_-".contains(&byte)){valid=false;break}}
                cost::pay(work,1)?;valid
            };
            if !valid{return Err(invalid("projection project or binding mismatch"))}
            let value=cost::reference(&reference.entity,&reference.record,work)?;
            cost::record(self.data,&value,work)?;Ok(())
        })?;
        let key=self.validation.with_host(|work|capture_key::key(&reference.entity,binding,expression,expected,limits,work))?;
        let cached=if let Some(key)=&key{
            self.validation.with_host(|work|{
                cost::pay(work,capture_key::SEARCH_FRAME)?;
                Ok::<_,AdapterError>(capture_key::search(&self.cache,key,work)?.is_ok())
            })?
        }else{false};
        if !cached{self.validation.validate_extension(&reference.entity,binding,expression).map_err(StepError::source)?;}
        Ok(if cached{None}else{key})
    }
    fn publish(&mut self,key:Option<capture_key::OwnedCaptureKey>)->Result<()>{
        self.validation.with_host(|work|{
            cost::pay(work,capture_key::PUBLISH_FRAME)?;
            capture_key::publish(&mut self.cache,&mut self.cache_bytes,key,work)
        })
    }

    pub(crate) fn with_row<R,E>(&mut self,reference:&RecordRef,first_binding:&str,first_expression:&Expr,first_expected:&Type,
        body:impl for<'r> FnOnce(&mut ProjectionRow<'r,'c,'v,'a,'s,H>,DataValue)->std::result::Result<R,E>)->std::result::Result<R,E>
    where E:From<AdapterError>{
        let reservation=self.validation.with_host(|host|host.reserve()).map_err(|error|E::from(cost::resource(error)))?;
        let result=(||{
            let mut meter=Meter::from_reservation(reservation,ProjectionRow::<H>::HEADER).map_err(E::from)?;
            self.validation.with_host(|host|host.active_row_deadline=Some(meter.row.as_ref().expect("row reservation").row_deadline));
            self.validation.ensure_source_and_data().map_err(|error|E::from(cost::contract(error)))?;
            // Fund enlarged owned-key/Option/result slots before prepare, even
            // when an unchanged pre-key guard refuses before key construction.
            self.validation.with_host(|work|cost::pay(work,capture_key::KEY_FRAME)).map_err(E::from)?;
            let key=match self.prepare(reference,first_binding,first_expression,first_expected,&meter.limits){
                Ok(key)=>key,
                Err(StepError::Source(error))=>return Err(E::from(error)),
                Err(StepError::Local(error))=>{self.first_failure=Some(error.clone());return Err(E::from(error))},
            };
            let session=self.validation.initial_session(&mut meter).map_err(|error|E::from(cost::contract(error)))?;
            let evaluator=Eval{app:&self.program.program,data:self.data,session:&session,day:self.day,meter:&mut meter};
            // This is the only actual Eval constructor in the paid-row path.
            let mut row=ProjectionRow{evaluator,capture:self,reference,output_bytes:0,first_failure:None};
            let first=match row.finish_expression(first_binding,first_expression,first_expected,key){
                Ok(value)=>value,
                Err(error)=>{row.capture.first_failure=Some(error.clone());return Err(E::from(error))},
            };
            let result=body(&mut row,first);
            if let Some(failure)=row.first_failure.take(){
                return match failure{RowFailure::Local(error)=>Err(E::from(error)),RowFailure::Source=>result};
            }
            match result{Err(error)=>Err(error),Ok(value)=>{row.evaluator.meter.tick(0).map_err(E::from)?;Ok(value)}}
        })();
        self.validation.with_host(|host|host.active_row_deadline=None);
        result
    }
}
enum RowFailure{Local(AdapterError),Source}
pub(crate) struct ProjectionRow<'r,'c,'v,'a,'s,H:ProjectionHost+?Sized>{
    evaluator:Eval<'r>,
    capture:&'r mut ProjectionCapture<'c,'v,'a,'s,H>,
    reference:&'r RecordRef,
    output_bytes:usize,
    first_failure:Option<RowFailure>,
}
impl<H:ProjectionHost+?Sized> ProjectionRow<'_,'_,'_,'_,'_,H>{
    const HEADER:u64=16+size_of::<Meter>() as u64+size_of::<RowStop>() as u64+size_of::<Self>() as u64;
    // Immutable readout of this actually reserved/executing row. It cannot
    // create a reservation, extend a deadline, or transfer runtime authority.
    pub(crate) fn actual_limits(&self)->&RuntimeLimits { &self.evaluator.meter.limits }
    fn remember<T>(&mut self,result:Result<T>)->Result<T>{
        if let Err(error)=&result{if self.first_failure.is_none(){self.first_failure=Some(RowFailure::Local(error.clone()))}}
        result
    }
    fn output(&mut self,value:&DataValue)->Result<()>{
        let bytes=capture_native::count_output_paid(value,self.evaluator.meter)?;
        cost::pay(self.evaluator.meter,3)?;
        let total=self.output_bytes.checked_add(bytes).ok_or_else(||exhausted("cumulative output limit"))?;
        if total>self.evaluator.meter.limits.output_bytes{return Err(exhausted("cumulative output limit"))}
        cost::pay(self.evaluator.meter,cost::number(bytes)?)?;
        cost::pay(self.evaluator.meter,1)?;self.output_bytes=total;Ok(())
    }
    fn finish_expression(&mut self,binding:&str,expression:&Expr,expected:&Type,key:Option<capture_key::OwnedCaptureKey>)->Result<DataValue>{
        cost::pay(self.evaluator.meter,8)?;let mut env=Env::new();
        let typ=cost::reference_type(&self.reference.entity,self.evaluator.meter)?;
        let value=cost::reference(&self.reference.entity,&self.reference.record,self.evaluator.meter)?;
        cost::bind(&mut env,binding,typ,value,self.evaluator.meter)?;
        let typ=self.evaluator.typ(expression,&env)?;
        if !cost::type_equal(&typ,expected,self.evaluator.meter)?{return Err(invalid("projection result type mismatch"))}
        let value=self.evaluator.eval(expression,&env)?;
        validate_value_with_work(&value,expected,0,self.evaluator.meter).map_err(cost::contract)?;
        self.output(&value)?;
        self.capture.publish(key)?;
        Ok(value)
    }
    pub(crate) fn evaluate(&mut self,binding:&str,expression:&Expr,expected:&Type)->Result<DataValue>{
        let entry=self.capture.validation.with_host(|work|cost::pay(work,capture_key::KEY_FRAME));
        self.remember(entry)?;
        let key=match self.capture.prepare(self.reference,binding,expression,expected,&self.evaluator.meter.limits){
            Ok(key)=>key,
            // The source context already owns the only retained semantic B.
            Err(StepError::Source(error))=>{self.first_failure=Some(RowFailure::Source);return Err(error)},
            Err(StepError::Local(error))=>return self.remember(Err(error)),
        };
        let result=self.finish_expression(binding,expression,expected,key);self.remember(result)
    }
    pub(crate) fn account_value(&mut self,value:&DataValue,expected:&Type)->Result<DataValue>{
        let result=(||{
            validate_value_with_work(value,expected,0,self.evaluator.meter).map_err(cost::contract)?;
            self.output(value)?;
            cost::value(value,self.evaluator.meter)?;Ok(value.clone())
        })();self.remember(result)
    }
    pub(crate) fn charge_host(&mut self,units:u64)->Result<()>{
        let result=self.capture.validation.with_host(|host|host.charge(units)).map_err(cost::resource);
        self.remember(result)
    }
    #[cfg(test)]
    pub(super) fn snapshot_for_test(&self)->(usize,usize,usize,u64,usize){
        (&self.evaluator as *const Eval<'_> as usize,self.evaluator.session as *const SessionState as usize,
            &*self.evaluator.meter as *const Meter as usize,self.evaluator.meter.fuel,self.output_bytes)
    }
}
#[cfg(test)]
#[path = "capture/cache_controls.rs"]
mod cache_controls;
