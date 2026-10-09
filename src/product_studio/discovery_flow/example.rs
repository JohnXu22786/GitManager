//! Exact-witness play has no decision or adoption method. A separate current
//! business-copy experience must be checked before a choice can be recorded.
use super::*;
use crate::product_scenarios::VerifiedWitness;

#[derive(Clone)]
pub(crate) struct ExamplePlayback {
    pub labels: [String; 2],
    pub runs: [RuntimeView; 2],
    pub minimal: bool,
    pub trial_day: i32,
}
#[derive(Clone)]
pub(crate) struct ExampleExperience {
    basis: ProjectSnapshot,
    sides: [Side; 2],
    original: VerifiedWitness,
    scenario: ScenarioSpec,
    context: ScopedExecutionContext,
    runs: [RuntimeView; 2],
    minimal: bool,
    trial_day: i32,
}
impl ExampleExperience {
    pub(super) fn new(
        store: &ProductStore,
        basis: ProjectSnapshot,
        sides: [Side; 2],
        original: VerifiedWitness,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        check(store, &basis, &cancelled)?;
        let scenario = original.witness().scenario.clone();
        let context = pair_context(&basis, &sides)?;
        let runtime = LocalRuntime::with_cancellation(cancelled.clone());
        // Reproduce the exact original input and observed outcomes. No seed,
        // producer, session, day or record identifier is relabeled as current.
        for (side, expected) in sides
            .iter()
            .zip([&original.witness().before, &original.witness().after])
        {
            let actual = runtime
                .replay_admitted(
                    &side.source,
                    &scenario,
                    &basis.decisions,
                    RuntimeLimits::default(),
                    "reopened-example",
                    Some(&context),
                )
                .map_err(error)?;
            if actual.state != EvidenceState::Observed
                || actual.observations != expected.observations
            {
                return Err("The saved example could not reproduce its original outcome".into());
            }
        }
        let (runs, trial_day) = run_views(store, &basis, &sides, &scenario, &context, cancelled)?;
        let minimal = original
            .witness()
            .minimization
            .as_ref()
            .is_some_and(|m| m.complete);
        Ok(Self {
            basis,
            sides,
            original,
            scenario,
            context,
            runs,
            minimal,
            trial_day,
        })
    }
    pub fn original(&self) -> &VerifiedWitness {
        &self.original
    }
    pub fn view(&self) -> ExamplePlayback {
        ExamplePlayback {
            labels: labels(&self.basis, [&self.sides[0].source, &self.sides[1].source]),
            runs: self.runs.clone(),
            minimal: self.minimal,
            trial_day: self.trial_day,
        }
    }
    pub fn trial(
        &mut self,
        store: &ProductStore,
        input: SemanticInput,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        self.minimal = false;
        let mut scenario = self.scenario.clone();
        scenario.inputs.push(input);
        let (runs, day) = run_views(
            store,
            &self.basis,
            &self.sides,
            &scenario,
            &self.context,
            cancelled,
        )?;
        self.scenario = scenario;
        self.runs = runs;
        self.trial_day = day;
        Ok(())
    }
}
