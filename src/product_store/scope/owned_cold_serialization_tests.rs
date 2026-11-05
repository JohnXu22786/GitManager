//! Genuine cold historical endpoint controls; no caller-forged serialization witness.
use super::*;
use std::cell::RefCell;
#[derive(Clone)]
struct Sample { historical: bool, data: DataSnapshot, day: i32, revision: u64,
    adoption: u64, target: Digest, bytes: u64, packet: u64, before: u64, cost: Option<u64> }
#[derive(Default)]
struct Observation { enabled: bool, samples: Vec<Sample>, cancel_slot: usize,
    polls: usize, packet_units: Option<u64>, secondary: Option<Arc<AtomicBool>> }
thread_local! { static OBSERVATION: RefCell<Observation> = RefCell::new(Observation::default()); }
struct Reset;
impl Drop for Reset { fn drop(&mut self) { OBSERVATION.with(|s| *s.borrow_mut()=Observation::default()); } }
pub(super) fn before(historical: bool, recipe: &OwnedExecutionRecipe, adoption: u64,
    revision: u64, data: &DataSnapshot, day: i32, meter: &mut ProofMeter, packet: u64) {
    OBSERVATION.with(|s| { let mut s=s.borrow_mut(); if !s.enabled {return;}
        assert!(s.samples.len()<16);
        if historical { if let Some(units)=s.packet_units.take() {
            meter.charge(meter.remaining_units_for_test().checked_sub(units).unwrap()).unwrap();
        }}
        s.samples.push(Sample {historical,data:data.clone(),day,revision,adoption,
            target:recipe.target().artifact.program_digest.clone(),bytes:canonical_bytes(data).unwrap().len() as u64,
            packet,before:meter.remaining_units_for_test(),cost:None});
    });
}
pub(super) fn after(meter: &ProofMeter) {
    OBSERVATION.with(|s| {let mut s=s.borrow_mut();if !s.enabled {return;}
        let sample=s.samples.iter_mut().rev().find(|p|p.cost.is_none()).unwrap();
        sample.cost=Some(sample.before-meter.remaining_units_for_test());
    });
}
pub(super) fn poll(meter: &ProofMeter) {
    OBSERVATION.with(|s| {let mut s=s.borrow_mut();if !s.enabled {return;}
        s.polls+=1;
        if s.cancel_slot==s.polls {
            if let Some(token)=&s.secondary {token.store(true,std::sync::atomic::Ordering::Relaxed);}
            else {meter.cancelled().store(true,std::sync::atomic::Ordering::Relaxed);}
        }
    });
}
fn fresh_meter() -> ProofMeter {
    ProofMeter::new(ProofBudget::default(),Arc::new(AtomicBool::new(false))).unwrap()
}
impl ProjectSnapshot {
    pub(crate) fn assert_cold_historical_serialization_for_test(&self) {
        let _reset=Reset;
        OBSERVATION.with(|s|*s.borrow_mut()=Observation {enabled:true,..Observation::default()});
        let mut meter=fresh_meter();
        let registry=reconstruct(self,&mut meter).unwrap();
        let samples=OBSERVATION.with(|s| {let mut s=s.borrow_mut();s.enabled=false;std::mem::take(&mut s.samples)});
        assert_eq!(registry.epochs.len(),4);
        assert_eq!(samples.len(),4);
        assert_eq!(samples.iter().filter(|p|p.historical).count(),3);
        for (index,sample) in samples.iter().enumerate() {
            let epoch=&registry.epochs[index];let recipe=registry.recipes.get(&epoch.output).unwrap();
            assert_eq!(sample.adoption,epoch.adoption_revision);
            assert_eq!(sample.target,recipe.target().artifact.program_digest);
            if index<3 {let next=registry.recipes.get(&registry.epochs[index+1].output).unwrap();
                assert_eq!(&sample.data,next.seed());assert_eq!(sample.revision,next.basis().revision);
                assert_eq!(sample.day,next.basis().clock_day);
            } else {assert_eq!(&sample.data,&self.data);assert_eq!(sample.revision,self.revision);}
            let mut native=fresh_meter();let before=native.remaining_units_for_test();
            owned_epoch::validate_interval(self,recipe,epoch.adoption_revision,sample.revision,
                &sample.data,sample.day,&mut native).unwrap();
            let cost=before-native.remaining_units_for_test();let selected=sample.cost.unwrap();
            println!("COLD_SERIALIZER interval={index} historical={} native={cost} selected={selected} packet={} bytes={}",sample.historical,sample.packet,sample.bytes);
            if sample.historical {
                assert_eq!(cost.checked_add(sample.packet).unwrap().checked_sub(selected).unwrap(),sample.bytes,
                    "exact historical serializer-local debit must disappear, with all new packet work positively retained");
            } else {assert_eq!(selected,cost,"final current tail remains the native default");assert_eq!(sample.packet,0);}
        }
        // Every new selected slot still checks both cancellation sources at its
        // original serializer-poll position after the original receipt loop.
        for secondary in [false,true] {for slot in 1..=3 {
            let token=Arc::new(AtomicBool::new(false));let mut meter=fresh_meter();
            if secondary {meter.watch_cancellation(&token);}
            OBSERVATION.with(|s|*s.borrow_mut()=Observation {enabled:true,cancel_slot:slot,
                secondary:secondary.then_some(token.clone()),..Observation::default()});
            let error=reconstruct(self,&mut meter).err().expect("cancelled serializer slot cannot return registry");
            assert!(error.to_string().contains("cancelled or cumulative elapsed budget exhausted"));
            OBSERVATION.with(|s| {let s=s.borrow();assert_eq!(s.polls,slot);
                assert_eq!(s.samples.len(),slot);assert!(s.samples.last().unwrap().cost.is_none());});
        }}
        // Exercise the actual selected boundary, not merely charge(constant).
        // The first historical interval has no events, so exact payment reaches
        // its zero-poll; the retained native interval finish then refuses.
        for exact in [false,true] {
            let units=COLD_HISTORICAL_SERIALIZATION_PAYMENT-u64::from(!exact);
            OBSERVATION.with(|s|*s.borrow_mut()=Observation {enabled:true,
                packet_units:Some(units),..Observation::default()});
            let mut meter=fresh_meter();
            let error=reconstruct(self,&mut meter).err().expect("bounded actual reconstruction must exhaust");
            assert!(error.to_string().contains("cumulative proof/initialization budget exhausted"));
            OBSERVATION.with(|s| {let s=s.borrow();assert_eq!(s.samples.len(),1);
                assert_eq!(s.polls,usize::from(exact));
                assert_eq!(s.samples[0].cost,None);
                assert_eq!(meter.remaining_units_for_test(),if exact {0} else {units});});
        }
        OBSERVATION.with(|s|*s.borrow_mut()=Observation::default());
        let mut missing=self.clone();let event=missing.data.events.last().unwrap();
        missing.operations.remove(&event.operation_id);
        let error=reconstruct(&missing,&mut fresh_meter()).err().expect("new native event needs receipt");
        assert!(error.to_string().contains("event lacks its retained operation receipt"));
        let recipe=registry.recipes.get(&registry.epochs.last().unwrap().output).unwrap();
        let key=recipe.metadata().saved[0].clone();let mut bad=self.clone();
        let event=bad.data.events.last_mut().unwrap();
        let change=event.changes.iter_mut().find(|c|c.entity==recipe.metadata().root).unwrap();
        let typ=recipe.metadata().fields(&recipe.initial().proof).into_iter()
            .find(|field|field.id==key).unwrap().value_type;
        // Inactive membership can produce a typed Null. Corrupt the real cell
        // with a distinct value admitted by its actual host type, not a guessed
        // integer layout that would fail before native projection comparison.
        let wrong=[DataValue::Boolean {value:false},DataValue::Boolean {value:true},
            DataValue::Integer {value:0},DataValue::Integer {value:1},
            DataValue::Text {value:"altered saved projection".into()},DataValue::Date {days:0}]
            .into_iter().find(|value|Some(value)!=change.after.get(&key)
                && validate_value(value,&typ,0).is_ok()).expect("fixture has a primitive host projection");
        change.after.insert(key.clone(),wrong.clone());let id=change.record.clone();
        bad.data.records.iter_mut().find(|r|r.entity==recipe.metadata().root&&r.id==id).unwrap().values.insert(key,wrong);
        let error=reconstruct(&bad,&mut fresh_meter()).err().expect("new native projection mismatch must refuse");
        assert!(error.to_string().contains("saved completion value does not regenerate from its historical frame"));
        println!("COLD_SERIALIZER_CONTROLS historical=3 native_final=1 primary_secondary_cancels=6 receipt_error=1 native_projection_error=1 actual_packet_boundaries=2");
    }
}
