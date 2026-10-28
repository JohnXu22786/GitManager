//! Same-call completed interval controls; positive carriers come only from native R.
use super::*;
use std::cell::RefCell;
#[derive(Default)]
struct Observation { enabled: bool, force_native: bool, mints: usize, tails: usize,
    initial: usize, transition: usize, projection_hits: usize, native_projections: usize, row_administrations: usize,
    visits: Vec<(usize, DataSnapshot)>, derive_before: u64, derive_cost: u64, cancel_at: &'static str, prior_initial: Option<OwnedExecutionRecipe> }
thread_local! { static OBSERVATION: RefCell<Observation> = RefCell::new(Observation::default()); }
struct Reset;
impl Drop for Reset { fn drop(&mut self) { OBSERVATION.with(|s| *s.borrow_mut()=Observation::default()); } }
pub(super) fn force_native() -> bool { OBSERVATION.with(|s|s.borrow().force_native) }
pub(super) fn stage(stage: &'static str, meter: &ProofMeter) {
    OBSERVATION.with(|s| { let mut s=s.borrow_mut(); if !s.enabled {return;}
        match stage { "mint"=>s.mints+=1,"tail"=>s.tails+=1,"initial"=>s.initial+=1,
            "transition"=>s.transition+=1,"projection-hit"=>s.projection_hits+=1,
            "projection-native"=>s.native_projections+=1,"row-admin"=>s.row_administrations+=1,_=>() }
        if s.cancel_at==stage {meter.cancelled().store(true,std::sync::atomic::Ordering::Relaxed);}
    });
}
pub(super) fn visit(index: usize, frame: &DataSnapshot) {
    OBSERVATION.with(|s| {let mut s=s.borrow_mut();if s.enabled {s.visits.push((index,frame.clone()));}});
}
impl ProjectSnapshot {
    pub(crate) fn assert_completed_intervals_for_test(&self) {
        self.completed_interval_controls(false);
    }
    pub(crate) fn assert_completed_intervals_with_seals_for_test(&self) {
        self.completed_interval_controls(true);
    }
    fn completed_interval_controls(&self, require_seals: bool) {
        let _reset=Reset;
        let mut traces=Vec::new();
        let mut prior_initial=None;
        for force_native in [false,true] {
            OBSERVATION.with(|s|*s.borrow_mut()=Observation {enabled:true,force_native,prior_initial:prior_initial.clone(),..Observation::default()});
            check_retained_transition_policies_with_initials(self, |check,loan| {
                check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
                let source=loan.execution.current().program()?;
                let surface=check.surface_with_initial_loan(loan,source)?;
                assert_eq!(surface.source(),&revision(source)?);
                Ok(())
            }).unwrap();
            OBSERVATION.with(|s| {let s=s.borrow();
                if !force_native { assert_eq!(s.mints,1);assert_eq!(s.tails,1);
                    assert_eq!(s.initial,1);assert_eq!(s.transition,1);
                    if require_seals {assert_eq!(s.projection_hits,2);}
                    assert_eq!(s.row_administrations,s.projection_hits+s.native_projections,
                        "NEW row administration must exist only at reached native new-seal blocks"); }
                else {assert_eq!(s.mints,0);assert_eq!(s.tails,0);}
                assert!(!s.visits.is_empty());traces.push(s.visits.clone());
                if prior_initial.is_none() {prior_initial=s.prior_initial.clone();}
                println!("COMPLETED_INTERVAL_ROUTE native={force_native} mints={} tails={} initial={} transition={} projections={} native_projections={} visits={}",s.mints,s.tails,s.initial,s.transition,s.projection_hits,s.native_projections,s.visits.len());
                println!("COMPLETED_ROW_ADMIN native={force_native} administrations={}",s.row_administrations);
            });
        }
        assert_eq!(traces[0],traces[1],"all seed/event/final visits remain identical");
        if require_seals {
            for at in ["replay-entry","tail","initial","transition","projection-hit"] {
                OBSERVATION.with(|s|*s.borrow_mut()=Observation {enabled:true,cancel_at:at,prior_initial:prior_initial.clone(),..Observation::default()});
                let error=check_retained_transition_policies_with_initials(self, |check,loan| {
                    check.charge(HISTORICAL_INITIAL_SURFACE_PAYMENT)?;
                    let source=loan.execution.current().program()?;
                    check.surface_with_initial_loan(loan,source)?;
                    panic!("cancellation must prevent surface return at {at}");
                }).unwrap_err();
                assert!(error.to_string().contains("cancelled or cumulative elapsed budget exhausted"));
                OBSERVATION.with(|s| {let s=s.borrow();assert_eq!(s.mints,1);assert_eq!(s.tails,1);
                    if at=="replay-entry" {assert_eq!(s.initial,0);assert_eq!(s.transition,0);assert_eq!(s.projection_hits,0);assert!(s.visits.is_empty());}
                    if at=="initial" {assert_eq!(s.initial,1);assert_eq!(s.transition,0);assert_eq!(s.projection_hits,0);}
                    if at=="projection-hit" {assert_eq!(s.projection_hits,1);assert_eq!(s.transition,0);}
                    if at=="transition" {assert_eq!(s.initial,1);assert_eq!(s.transition,1);assert_eq!(s.projection_hits,1);}
                });
                println!("COMPLETED_GENUINE_CANCELLATION at={at} error={error}");
            }
        }
    }
}
#[test]
fn completed_interval_checked_payments() {
    // These are the real introduced packet families, not surrogate stdlib behavior.
    let prices=[COMPLETED_INTERVAL_MINT_PAYMENT,COMPLETED_INTERVAL_TAIL_PAYMENT,
        COMPLETED_INTERVAL_SURFACE_PAYMENT,COMPLETED_INTERVAL_DISPATCH_PAYMENT,
        COMPLETED_INTERVAL_REPLAY_PAYMENT,COMPLETED_INTERVAL_ROW_PAYMENT,
        COMPLETED_INTERVAL_PROJECTION_PAYMENT];
    assert!(prices.iter().all(|p|*p>0));
    println!("COMPLETED_INTERVAL_PAYMENTS {prices:?}");
}

pub(super) fn derive_boundary(stage: &str, meter: &ProofMeter) {
    OBSERVATION.with(|s| {let mut s=s.borrow_mut(); if !s.enabled {return;}
        if stage=="before" {s.derive_before=meter.remaining_units_for_test();}
        else {s.derive_cost=s.derive_before-meter.remaining_units_for_test();println!("COMPLETED_DERIVE actual={} after={}",s.derive_cost,meter.remaining_units_for_test());}
    });
}

// The actual shared-charge syntax must stop before any later query/operation,
// and map the first returned cause without rereading a subsequently changed token.
fn joined_charge_control(meter: &mut ProofMeter, reached: &mut Vec<u8>, change_token: bool) -> Result<()> {
    let failure = 'test_join: {
        completed_charge_to!('test_join, meter, 2);
        reached.push(1);
        completed_charge_to!('test_join, meter, 3);
        reached.push(2);
        if completed_window_to!('test_join, meter) { reached.push(3); }
        None
    };
    if change_token { meter.cancelled().store(true, std::sync::atomic::Ordering::Relaxed); }
    completed_failure!(failure);
    Ok(())
}
#[test]
fn completed_local_join_preserves_first_cause_and_boundaries() {
    for (units, expected) in [(2,Vec::new()),(5,vec![1]),(6,vec![1,2,3])] {
        let mut meter=ProofMeter::new(ProofBudget {units,elapsed_millis:60_000},Arc::new(AtomicBool::new(false))).unwrap();
        let mut reached=Vec::new();let result=joined_charge_control(&mut meter,&mut reached,units<6);
        assert_eq!(reached,expected);
        if units<6 { assert!(result.unwrap_err().to_string().contains("cumulative proof/initialization budget exhausted")); }
        else {result.unwrap();assert_eq!(meter.remaining_units_for_test(),0);}
    }
    for secondary in [false,true] {
        let token=Arc::new(AtomicBool::new(false));
        let mut meter=ProofMeter::new(ProofBudget {units:5,elapsed_millis:60_000},token.clone()).unwrap();
        let other=Arc::new(AtomicBool::new(false));meter.watch_cancellation(&other);
        if secondary {other.store(true,std::sync::atomic::Ordering::Relaxed);} else {token.store(true,std::sync::atomic::Ordering::Relaxed);}
        let mut reached=Vec::new();let error=joined_charge_control(&mut meter,&mut reached,false).unwrap_err();
        assert!(reached.is_empty());assert_eq!(meter.remaining_units_for_test(),4);
        assert!(error.to_string().contains("cancelled or cumulative elapsed budget exhausted"));
    }
}

impl ProjectSnapshot {
    // Genuine source-only cost carrier: one native registry and one cumulative
    // meter. It does not claim completed-pair hit or whole-suffix affordability.
    pub(crate) fn assert_completed_source_derive_for_test(&self) {
        let mut meter=ProofMeter::new(ProofBudget::default(),Arc::new(AtomicBool::new(false))).unwrap();
        let registry=reconstruct(self,&mut meter).unwrap();
        let source=self.program().unwrap();
        let OwnedExecutionRecipe::Transition(prior)=registry.resolve(source).unwrap() else {panic!("actual Evolution required")};
        assert_eq!(prior.manifest.transition,ScopeTransition::Evolution);
        let layer=self.owned_scope().unwrap().layers.get(prior.continuation.original_layer()).unwrap();
        assert!(matches!(layer.request.population,ScopePopulation::All));
        let before=meter.remaining_units_for_test();
        let surface=owned_continuation::derive_policy_surface(&prior.original,layer,
            Some((&prior.continuation,&prior.target)),&mut meter).unwrap();
        let cost=before-meter.remaining_units_for_test();
        assert_eq!(surface.source(),&revision(source).unwrap());
        println!("COMPLETED_EXACT_SOURCE_DERIVE cost={cost} before={before} after={} source={} layer={} original_before={} original_after={}",
            meter.remaining_units_for_test(),source.artifact.program_digest.as_str(),layer.identity().unwrap().as_str(),
            prior.original.proof.snapshot.program().unwrap().artifact.program_digest.as_str(),
            prior.original.proof.candidate.artifact.program_digest.as_str());
    }
}

#[test]
fn completed_interval_packets_refuse_short_and_accept_exact() {
    fn packet(meter: &mut ProofMeter, units: u64, entered: &mut bool) -> Result<()> {
        let failure = 'packet: {
            completed_charge_to!('packet,meter,units);
            *entered=true;
            completed_charge_to!('packet,meter,0);
            None
        };
        completed_failure!(failure);
        Ok(())
    }
    for price in [COMPLETED_INTERVAL_MINT_PAYMENT,COMPLETED_INTERVAL_TAIL_PAYMENT,
        COMPLETED_INTERVAL_SURFACE_PAYMENT,COMPLETED_INTERVAL_DISPATCH_PAYMENT,
        COMPLETED_INTERVAL_REPLAY_PAYMENT,COMPLETED_INTERVAL_ROW_PAYMENT,COMPLETED_INTERVAL_PROJECTION_PAYMENT] {
        for exact in [false,true] {
            let mut meter=ProofMeter::new(ProofBudget {units:price+u64::from(exact),elapsed_millis:60_000},Arc::new(AtomicBool::new(false))).unwrap();
            let mut entered=false;let result=packet(&mut meter,price,&mut entered);
            assert_eq!(entered,exact);assert_eq!(result.is_ok(),exact);
            if exact {assert_eq!(meter.remaining_units_for_test(),0);} else {assert_eq!(meter.remaining_units_for_test(),price-1);}
        }
    }
}
pub(super) fn guard_cases(pair: &CompletedHistoricalIntervals<'_, '_>, registry: &ReconstructedOwned) {
    if !OBSERVATION.with(|s|s.borrow().enabled) {return;}
    assert!(completed_shape!(pair,registry));
    assert!(completed_recipe!(pair,pair.initial,true));
    assert!(completed_recipe!(pair,pair.transition,false));
    assert!(!completed_recipe!(pair,pair.initial,false));
    assert!(!completed_recipe!(pair,pair.transition,true));
    let current=pair.loan.execution.current();
    assert!(completed_tail!(pair,registry,pair.transition,&current.data,current.clock_day,current.revision));
    let equal_distinct=current.data.clone();
    assert_eq!(equal_distinct,current.data);
    assert!(!completed_tail!(pair,registry,pair.transition,&equal_distinct,current.clock_day,current.revision));
    assert!(!completed_tail!(pair,registry,pair.transition,&current.data,current.clock_day+1,current.revision));
    assert!(!completed_tail!(pair,registry,pair.transition,&current.data,current.clock_day,current.revision+1));
    assert!(!completed_epoch!(pair,registry,2,pair.transition,&current.data,current.clock_day));
    let OwnedExecutionRecipe::Transition(p)=pair.transition else {panic!("actual first Evolution")};
    let different=OwnedExecutionRecipe::Transition(Arc::new(VerifiedOwnedTransitionExecution {
        continuation:p.continuation.clone(),target:p.target.clone(),metadata:p.metadata.clone(),manifest:p.manifest.clone(),
        original:p.original.clone(),initialization:p.initialization.clone(),
    }));
    assert_eq!(different.target(),pair.transition.target());
    assert!(!completed_recipe!(pair,&different,false));
    // Different completed native R calls give genuine equal-valued, distinct A
    // owners; a cloned Arc from the earlier R remains alive during this check.
    OBSERVATION.with(|state| {
        let mut state=state.borrow_mut();
        if let Some(prior)=&state.prior_initial {
            assert_eq!(prior.target(),pair.initial.target());
            assert!(!completed_recipe!(pair,prior,true));
        } else {state.prior_initial=Some(pair.initial.clone());}
    });
    let mut wrong=ReconstructedOwned {
        snapshot:registry.snapshot.clone(),recipes:registry.recipes.clone(),epochs:registry.epochs.clone(),
        proofs:registry.proofs.clone(),manifests:registry.manifests.clone(),initializations:registry.initializations.clone(),
    };
    wrong.epochs.swap(0,1);assert!(!completed_shape!(pair,&wrong));wrong.epochs.swap(0,1);
    wrong.epochs[1].adoption_revision+=1;assert!(!completed_shape!(pair,&wrong));wrong.epochs[1].adoption_revision-=1;
    wrong.epochs[1].output=wrong.epochs[0].output.clone();assert!(!completed_shape!(pair,&wrong));
    wrong.epochs=registry.epochs.clone();wrong.epochs.push(wrong.epochs[0].clone());assert!(!completed_shape!(pair,&wrong));
    wrong.epochs.truncate(1);assert!(!completed_shape!(pair,&wrong));
    // Actual different execution identity refuses before any new packet/query,
    // even when no proof fuel remains. It is not a fabricated positive loan.
    let mut meter=ProofMeter::new(ProofBudget {units:1,elapsed_millis:60_000},Arc::new(AtomicBool::new(false))).unwrap();
    let error=pair.loan.surface(p,pair.loan.execution.current().program().unwrap(),&mut meter).err().expect("wrong actual execution must refuse");
    assert!(error.to_string().contains("historical Initial loan execution differs"));
    assert_eq!(meter.remaining_units_for_test(),0);
}

#[test]
fn completed_window_false_keeps_native_reservation_and_elapsed_error() {
    fn window(meter: &mut ProofMeter, native: &mut bool) -> Result<()> {
        let failure='window: {
            if !completed_window_to!('window,meter) {
                *native=true;
                let _limits=meter.runtime()?;
            }
            None
        };
        completed_failure!(failure);Ok(())
    }
    let millis=RuntimeLimits::default().elapsed_millis-1;
    let mut meter=ProofMeter::new(ProofBudget {units:200_001,elapsed_millis:millis},Arc::new(AtomicBool::new(false))).unwrap();
    let mut native=false;window(&mut meter,&mut native).unwrap();assert!(native);
    assert_eq!(meter.remaining_units_for_test(),100_000,"false live window retains actual native100000 reservation");
    let mut meter=ProofMeter::new(ProofBudget {units:200_001,elapsed_millis:1},Arc::new(AtomicBool::new(false))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut native=false;let error=window(&mut meter,&mut native).unwrap_err();
    assert!(!native);assert_eq!(meter.remaining_units_for_test(),200_000);
    assert!(error.to_string().contains("cancelled or cumulative elapsed budget exhausted"));
}
