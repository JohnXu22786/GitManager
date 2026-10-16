//! Trusted host acceptance controller. Replay evidence and explicit UI assent
//! are separate capabilities; neither a DTO nor a Boolean can mint acceptance.
use super::common_fact_admission::{
    authenticate, refuse, request_identity, AdmissionResult, ProofMeter,
};
use super::*;
use crate::product_runtime::{LocalRuntime, ProjectionValidation};
use serde::Serialize;
use std::sync::{atomic::AtomicBool, Arc};
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleWitness {
    pub side: OwnedSourceSide,
    pub scenario: ScenarioSpec,
    pub completed: RecordRef,
    pub unfinished: RecordRef,
}
#[derive(Clone)]
struct Disclosure {
    snapshot: Digest,
    request: Digest,
    before: CapturedProgram,
    after: CapturedProgram,
    witnesses: Vec<LifecycleWitness>,
    evidence: Vec<RunEvidence>,
    identity: Digest,
}
/// Opaque in-memory host event; no deserialization or caller-selected acceptance.
pub struct AcceptedPreviewLifecycle {
    disclosure: Disclosure,
    event: Digest,
}
impl AcceptedPreviewLifecycle {
    pub(super) fn identity(&self) -> &Digest {
        &self.event
    }
    pub(super) fn copy_internal(&self) -> Self {
        Self {
            disclosure: self.disclosure.clone(),
            event: self.event.clone(),
        }
    }
    pub(super) fn verify(
        &self,
        snapshot: &ProjectSnapshot,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        meter: &mut ProofMeter,
    ) -> AdmissionResult<()> {
        if self.disclosure.snapshot != canonical_digest(IdentityDomain::Data, snapshot)?
            || self.disclosure.request != request_identity(request)?
            || self.disclosure.before != *snapshot.program()?
            || self.disclosure.after != *candidate
        {
            return Err(refuse(
                "lifecycle acceptance",
                "stale source, snapshot, policy, meaning or decision basis",
            ));
        }
        let evidence = replay(
            snapshot,
            candidate,
            request,
            &self.disclosure.witnesses,
            meter,
        )?;
        if evidence != self.disclosure.evidence
            || disclosure_id(
                snapshot,
                candidate,
                request,
                &self.disclosure.witnesses,
                &evidence,
            )? != self.disclosure.identity
        {
            return Err(refuse(
                "lifecycle acceptance",
                "independent supporting scene regeneration differs",
            ));
        }
        Ok(())
    }
}
fn disclosure_id(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    witnesses: &[LifecycleWitness],
    evidence: &[RunEvidence],
) -> AdmissionResult<Digest> {
    Ok(canonical_digest(
        IdentityDomain::Evidence,
        &(
            "preview-lifecycle-disclosure/1",
            snapshot,
            candidate,
            request,
            witnesses,
            evidence,
        ),
    )?)
}
fn replay(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    witnesses: &[LifecycleWitness],
    meter: &mut ProofMeter,
) -> AdmissionResult<Vec<RunEvidence>> {
    if witnesses.len() != 2
        || witnesses[0].side != OwnedSourceSide::Before
        || witnesses[1].side != OwnedSourceSide::After
    {
        return Err(refuse(
            "lifecycle witnesses",
            "exact before and after supporting scenes required",
        ));
    }
    let runtime = LocalRuntime::with_cancellation(meter.cancelled());
    let mut checks = ProjectionValidation::default();
    let mut result = vec![];
    for witness in witnesses {
        let scenario = &witness.scenario;
        // The bounded first host challenge observes the authenticated current frame.
        // It does not accept caller-authored replacement seeds or transition claims.
        if scenario.seed != snapshot.data
            || scenario.session != snapshot.session
            || scenario.clock_day != snapshot.clock_day
            || scenario.inputs.len() != 1
            || !matches!(&scenario.inputs[0], SemanticInput::Observe { .. })
            || !scenario.validity.is_empty()
        {
            return Err(refuse(
                "lifecycle witness",
                "supporting scene is not the exact current frame/observation",
            ));
        }
        if witness.completed == witness.unfinished
            || [&witness.completed, &witness.unfinished]
                .iter()
                .any(|r| r.entity != request.inventory.sources.root_entity)
        {
            return Err(refuse(
                "lifecycle witness",
                "distinct exact-root completed and unfinished witnesses required",
            ));
        }
        let (program, predicate) = match witness.side {
            OwnedSourceSide::Before => (snapshot.program()?, &request.inventory.lifecycle.before),
            OwnedSourceSide::After => (candidate, &request.inventory.lifecycle.after),
        };
        let run = runtime.replay(
            program,
            scenario,
            &snapshot.decisions,
            meter.runtime()?,
            "lifecycle-support",
        )?;
        if run.state != EvidenceState::Observed
            || run.observations.len() != 1
            || run.trace.len() != 1
        {
            return Err(refuse(
                "lifecycle witness",
                "concrete LocalRuntime did not complete supporting observation",
            ));
        }
        let data = crate::product_runtime::merged_data(program, &snapshot.data)?;
        for (row, wanted) in [(&witness.completed, true), (&witness.unfinished, false)] {
            if !data
                .records
                .iter()
                .any(|r| r.entity == row.entity && r.id == row.record && !r.archived)
            {
                return Err(refuse("lifecycle witness", "active witness row missing"));
            }
            let actual = runtime.evaluate_record_projection_with(
                &mut checks,
                meter.runtime()?,
                program,
                &data,
                row,
                "record",
                predicate,
                &Type::Boolean,
                snapshot.clock_day,
            )?;
            if actual != (DataValue::Boolean { value: wanted }) {
                return Err(refuse(
                    "lifecycle witness",
                    "replayed completed/unfinished interpretation disagrees",
                ));
            }
        }
        result.push(run);
        meter.charge(1)?;
    }
    Ok(result)
}
/// Production UI boundary. The challenge owns immutable source/frame evidence;
/// only its rendered explicit acceptance button records the production event.
pub struct LifecycleAcceptanceHost {
    disclosure: Option<Disclosure>,
    request: OwnedPreviewRequest,
}
impl LifecycleAcceptanceHost {
    pub fn prepare(
        store: &ProductStore,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        witnesses: &[LifecycleWitness],
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> AdmissionResult<Self> {
        let mut meter = ProofMeter::new(budget, cancelled)?;
        let snapshot = store.load()?;
        authenticate(&snapshot, candidate, request, &mut meter)?;
        let evidence = replay(&snapshot, candidate, request, witnesses, &mut meter)?;
        let identity = disclosure_id(&snapshot, candidate, request, witnesses, &evidence)?;
        Ok(Self {
            disclosure: Some(Disclosure {
                snapshot: request.inventory.snapshot.clone(),
                request: request_identity(request)?,
                before: snapshot.program()?.clone(),
                after: candidate.clone(),
                witnesses: witnesses.to_vec(),
                evidence,
                identity,
            }),
            request: request.clone(),
        })
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &ProductStore,
    ) -> Option<AdmissionResult<AcceptedPreviewLifecycle>> {
        let d = self.disclosure.as_ref()?;
        ui.heading("Completed-history meaning for isolated preview");
        ui.label(&self.request.inventory.lifecycle.meaning);
        ui.label(&self.request.purpose);
        ui.label("This accepts only the disclosed lifecycle interpretation. It does not accept changed outcomes or save/adopt a program.");
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                ui.monospace(format!(
                    "Snapshot: {:?}\nBefore: {:?}\nAfter: {:?}\nChallenge: {:?}",
                    d.snapshot,
                    self.request.inventory.sources.before,
                    self.request.inventory.sources.after,
                    d.identity
                ));
                ui.monospace(serde_json::to_string_pretty(&self.request).unwrap_or_default());
                for (w, e) in d.witnesses.iter().zip(&d.evidence) {
                    ui.monospace(format!(
                        "{:?}: completed {:?}; unfinished {:?}\nScene {:?}",
                        w.side,
                        w.completed,
                        w.unfinished,
                        w.scenario.identity()
                    ));
                    ui.monospace(serde_json::to_string_pretty(&e.observations).unwrap_or_default());
                }
            });
        if ui
            .button("Accept this lifecycle meaning for this preview")
            .clicked()
        {
            return Some(self.record_event(store, "explicit-host-ui"));
        }
        None
    }
    fn record_event(
        &mut self,
        store: &ProductStore,
        origin: &str,
    ) -> AdmissionResult<AcceptedPreviewLifecycle> {
        let d = self
            .disclosure
            .as_ref()
            .ok_or_else(|| refuse("host acceptance", "challenge already consumed"))?;
        if canonical_digest(IdentityDomain::Data, &store.load()?)? != d.snapshot {
            return Err(refuse(
                "host acceptance",
                "current snapshot changed while disclosure was open",
            ));
        }
        let event = canonical_digest(
            IdentityDomain::Evidence,
            &("host-lifecycle-acceptance/1", &d.identity, origin),
        )?;
        Ok(AcceptedPreviewLifecycle {
            disclosure: self.disclosure.take().unwrap(),
            event,
        })
    }
    /// Tests exercise the same private controller event boundary with explicit
    /// synthetic attribution. This entry does not exist in production builds.
    #[cfg(test)]
    pub fn synthetic_operator_accept_for_test(
        &mut self,
        store: &ProductStore,
    ) -> AdmissionResult<AcceptedPreviewLifecycle> {
        self.record_event(store, "synthetic-operator-fixture")
    }
}
