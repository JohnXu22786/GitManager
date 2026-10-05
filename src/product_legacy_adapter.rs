//! The genuine existing order runtime, with an explicitly limited interface.
//! This does not implement the generated-app RuntimeAdapter: converting its
//! generic relations/selections into legacy fields would change their meaning.
use crate::{
    product_contract::{AdapterError, ArtifactKind, ContractError, RuntimeCapabilities},
    tool_decisions,
    tool_project::{Evidence, ProjectSnapshot, Scenario, ToolCommand, RUNTIME_SEMANTICS_VERSION},
    tool_runtime::{self, RecordEvaluation, RuntimeError},
};
use chrono::{DateTime, FixedOffset, NaiveDate};

pub struct LegacyOrderAdapter;
pub struct LegacyRun {
    snapshot: ProjectSnapshot,
}
impl LegacyRun {
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }
}
impl LegacyOrderAdapter {
    pub fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            adapter: "legacy_order".into(),
            version: RUNTIME_SEMANTICS_VERSION.to_string(),
            artifact_kinds: vec![ArtifactKind::LegacyOrder],
            features: [
                "legacy_record_commands",
                "legacy_stage_transitions",
                "legacy_bound_observations",
                "legacy_scenarios",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            unavailable: vec![
                "Generic generated-app actions, relations, selections and outputs are unsupported"
                    .into(),
                "External repository programs are not executed by this adapter".into(),
            ],
        }
    }
    pub fn require_capability(&self, feature: &str) -> Result<(), AdapterError> {
        if self.capabilities().features.contains(feature) {
            Ok(())
        } else {
            Err(AdapterError::Unsupported(format!(
                "Legacy order runtime cannot execute {feature}"
            )))
        }
    }
    /// Copies the original schema and history. Opening/previewing never writes
    /// a store, migrates a project or recomputes historical business facts.
    pub fn start(&self, snapshot: &ProjectSnapshot) -> Result<LegacyRun, AdapterError> {
        snapshot
            .validate()
            .map_err(|e| AdapterError::Invalid(ContractError(e.to_string())))?;
        Ok(LegacyRun {
            snapshot: snapshot.clone(),
        })
    }
    pub fn apply(
        &self,
        run: &mut LegacyRun,
        command: &ToolCommand,
        now: DateTime<FixedOffset>,
    ) -> Result<(), RuntimeError> {
        let next = tool_runtime::apply_command(&run.snapshot, command, now)?;
        run.snapshot = next;
        Ok(())
    }
    /// Resolves actual scoped old decisions rather than silently using only the
    /// project's default policy or reimplementing its timer/date calculations.
    pub fn observe(
        &self,
        run: &LegacyRun,
        record_id: &str,
        day: NaiveDate,
    ) -> Result<RecordEvaluation, tool_decisions::DecisionError> {
        tool_decisions::evaluate_bound_record(&run.snapshot, record_id, day)
    }
    pub fn run_scenario(&self, scenario: &Scenario) -> Result<Evidence, RuntimeError> {
        tool_runtime::run_scenario(scenario)
    }
}
