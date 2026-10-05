use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::{self, Write};

pub const PROJECT_FORMAT_VERSION: u32 = 1;
pub const RUNTIME_SEMANTICS_VERSION: u32 = 2;
pub const MAX_FIELD_DEFINITIONS: usize = 128;
pub const MAX_STAGE_DEFINITIONS: usize = 64;
pub const MAX_SPEC_REVISIONS: usize = 256;
pub const MAX_BEHAVIOR_REVISIONS: usize = 10_000;
pub const MAX_RULE_BINDINGS: usize = 100_000;
pub const MAX_DECISIONS: usize = 10_000;
pub const MAX_SCENARIOS: usize = 1_000;
pub const MAX_EVIDENCE: usize = 10_000;
pub const MAX_PROPOSALS: usize = 1_000;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_EVENTS: usize = 500_000;
pub const MAX_OPERATION_RECEIPTS: usize = MAX_EVENTS;
pub const MAX_SCENARIO_STEPS: usize = 1_000;
pub const MAX_SCENARIO_EXPECTATIONS: usize = 10_000;
pub const MAX_DECISION_EXPECTATIONS: usize = MAX_SCENARIO_EXPECTATIONS;
pub const MAX_EVIDENCE_ERRORS: usize = 128;
pub const MAX_PROPOSAL_UNKNOWN_ITEMS: usize = 128;
pub const MAX_PROPOSAL_CLAIMS: usize = 128;
pub const MAX_REQUIRED_FEATURES: usize = 128;
pub const MAX_EXTENSION_FIELDS: usize = 128;
pub const MAX_EXTENSION_JSON_NODES: usize = 10_000;
pub const MAX_EXTENSION_JSON_DEPTH: usize = 64;
pub const MAX_EXTENSION_JSON_BYTES: usize = 1_048_576;
pub const MAX_EXTENSION_STRING_BYTES: usize = 65_536;
pub const MAX_ID_LENGTH: usize = 128;

pub type ExtensionFields = BTreeMap<String, Value>;
pub type RecordValues = BTreeMap<String, FieldValue>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Date,
    Integer,
    Boolean,
    Enum { options: Vec<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldRole {
    Title,
    WorkDescription,
    WorkStartedOn,
    PromisedDate,
    Notes,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldValue {
    Text(String),
    Date(NaiveDate),
    Integer(i64),
    Boolean(bool),
    Enum(String),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum FieldValueWire {
    Text(String),
    Date(String),
    Integer(i64),
    Boolean(bool),
    Enum(String),
}

impl Serialize for FieldValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            Self::Text(value) => FieldValueWire::Text(value.clone()),
            Self::Date(value) => FieldValueWire::Date(value.to_string()),
            Self::Integer(value) => FieldValueWire::Integer(*value),
            Self::Boolean(value) => FieldValueWire::Boolean(*value),
            Self::Enum(value) => FieldValueWire::Enum(value.clone()),
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FieldValueWire::deserialize(deserializer)?;
        match wire {
            FieldValueWire::Text(value) => Ok(Self::Text(value)),
            FieldValueWire::Date(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")
                .map(Self::Date)
                .map_err(serde::de::Error::custom),
            FieldValueWire::Integer(value) => Ok(Self::Integer(value)),
            FieldValueWire::Boolean(value) => Ok(Self::Boolean(value)),
            FieldValueWire::Enum(value) => Ok(Self::Enum(value)),
        }
    }
}

mod naive_date_serde {
    use chrono::NaiveDate;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &NaiveDate, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.format("%Y-%m-%d").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(serde::de::Error::custom)
    }
}

mod optional_naive_date_serde {
    use chrono::NaiveDate;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<NaiveDate>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(date) => serializer.serialize_some(&date.format("%Y-%m-%d").to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| {
                NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(serde::de::Error::custom)
            })
            .transpose()
    }
}

mod fixed_datetime_serde {
    use chrono::{DateTime, FixedOffset};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &DateTime<FixedOffset>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub id: String,
    pub display_name: String,
    pub kind: FieldKind,
    pub role: FieldRole,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageClock {
    Active,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageDefinition {
    pub id: String,
    pub display_name: String,
    pub clock: StageClock,
    pub terminal: bool,
    pub allowed_next_stage_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub spec_id: String,
    pub revision: u32,
    pub template_id: Option<String>,
    pub entity_type: String,
    pub display_name: String,
    pub fields: Vec<FieldDefinition>,
    pub stages: Vec<StageDefinition>,
    pub default_stage_id: String,
    pub list_field_ids: Vec<String>,
    pub detail_field_ids: Vec<String>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

impl ToolSpec {
    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        if !valid_id(&self.spec_id) || self.revision == 0 {
            return Err(validation(
                ValidationCode::InvalidId,
                "spec_id",
                "spec ID must be a stable lowercase ID and revision must be positive",
            ));
        }
        if self.template_id.as_ref().is_some_and(|id| !valid_id(id))
            || !valid_label(&self.entity_type)
            || !valid_label(&self.display_name)
        {
            return Err(validation(
                ValidationCode::InvalidSpec,
                "spec",
                "entity type, display name, and optional template ID must be valid",
            ));
        }
        if self.fields.is_empty() || self.fields.len() > MAX_FIELD_DEFINITIONS {
            return Err(validation(
                ValidationCode::LimitExceeded,
                "fields",
                "a tool spec must contain between one and the supported maximum of fields",
            ));
        }
        if self.stages.is_empty() || self.stages.len() > MAX_STAGE_DEFINITIONS {
            return Err(validation(
                ValidationCode::LimitExceeded,
                "stages",
                "a tool spec must contain between one and the supported maximum of stages",
            ));
        }

        let mut field_ids = HashSet::new();
        let mut role_counts: HashMap<String, usize> = HashMap::new();
        let mut title_is_required = false;
        for (index, field) in self.fields.iter().enumerate() {
            if !valid_id(&field.id) || !field_ids.insert(field.id.as_str()) {
                return Err(validation(
                    ValidationCode::DuplicateOrInvalidId,
                    format!("fields[{index}].id"),
                    "field IDs must be unique stable lowercase IDs",
                ));
            }
            if !valid_label(&field.display_name) {
                return Err(validation(
                    ValidationCode::InvalidSpec,
                    format!("fields[{index}].display_name"),
                    "field display names cannot be empty",
                ));
            }
            if let FieldKind::Enum { options } = &field.kind {
                let mut seen = HashSet::new();
                if options.is_empty()
                    || options.len() > MAX_FIELD_DEFINITIONS
                    || options
                        .iter()
                        .any(|option| !valid_label(option) || !seen.insert(option.as_str()))
                {
                    return Err(validation(
                        ValidationCode::InvalidSpec,
                        format!("fields[{index}].kind"),
                        "enum fields need a finite, nonempty list of unique labels",
                    ));
                }
            }
            let compatible = match field.role {
                FieldRole::Title | FieldRole::WorkDescription | FieldRole::Notes => {
                    matches!(field.kind, FieldKind::Text)
                }
                FieldRole::WorkStartedOn | FieldRole::PromisedDate => {
                    matches!(field.kind, FieldKind::Date)
                }
                FieldRole::Custom => true,
            };
            if !compatible {
                return Err(validation(
                    ValidationCode::InvalidFieldRole,
                    format!("fields[{index}].role"),
                    "the field kind does not match its semantic role",
                ));
            }
            if field.role == FieldRole::Title && field.required {
                title_is_required = true;
            }
            if field.role != FieldRole::Custom {
                let role = format!("{:?}", field.role);
                *role_counts.entry(role).or_default() += 1;
            }
        }
        if !title_is_required || role_counts.get("Title") != Some(&1) {
            return Err(validation(
                ValidationCode::InvalidSpec,
                "fields",
                "a spec needs exactly one required title field",
            ));
        }
        if role_counts.get("WorkStartedOn").copied().unwrap_or(0) > 1
            || role_counts.get("PromisedDate").copied().unwrap_or(0) > 1
            || role_counts.get("WorkDescription").copied().unwrap_or(0) > 1
            || role_counts.get("Notes").copied().unwrap_or(0) > 1
        {
            return Err(validation(
                ValidationCode::InvalidSpec,
                "fields",
                "each supported semantic field role can appear at most once",
            ));
        }

        let mut stage_ids = HashSet::new();
        for (index, stage) in self.stages.iter().enumerate() {
            if !valid_id(&stage.id) || !stage_ids.insert(stage.id.as_str()) {
                return Err(validation(
                    ValidationCode::DuplicateOrInvalidId,
                    format!("stages[{index}].id"),
                    "stage IDs must be unique stable lowercase IDs",
                ));
            }
            if !valid_label(&stage.display_name) {
                return Err(validation(
                    ValidationCode::InvalidSpec,
                    format!("stages[{index}].display_name"),
                    "stage display names cannot be empty",
                ));
            }
            if stage.terminal && !stage.allowed_next_stage_ids.is_empty() {
                return Err(validation(
                    ValidationCode::InvalidSpec,
                    format!("stages[{index}].allowed_next_stage_ids"),
                    "terminal stages cannot allow outgoing transitions",
                ));
            }
            if stage.terminal && stage.clock != StageClock::Stopped {
                return Err(validation(
                    ValidationCode::InvalidSpec,
                    format!("stages[{index}].clock"),
                    "terminal stages must stop the work timer",
                ));
            }
            let mut transitions = HashSet::new();
            for target in &stage.allowed_next_stage_ids {
                if !valid_id(target) || target == &stage.id || !transitions.insert(target.as_str())
                {
                    return Err(validation(
                        ValidationCode::DuplicateOrInvalidId,
                        format!("stages[{index}].allowed_next_stage_ids"),
                        "transition targets must be unique stable IDs",
                    ));
                }
            }
        }
        if !stage_ids.contains(self.default_stage_id.as_str()) {
            return Err(validation(
                ValidationCode::DanglingReference,
                "default_stage_id",
                "default stage does not exist in this spec",
            ));
        }
        for (index, stage) in self.stages.iter().enumerate() {
            if stage
                .allowed_next_stage_ids
                .iter()
                .any(|target| !stage_ids.contains(target.as_str()))
            {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    format!("stages[{index}].allowed_next_stage_ids"),
                    "every transition target must refer to a stage in this spec",
                ));
            }
        }
        validate_layout_fields(&self.list_field_ids, &field_ids, "list_field_ids")?;
        validate_layout_fields(&self.detail_field_ids, &field_ids, "detail_field_ids")?;
        validate_extension_keys(
            &self.extensions,
            &[
                "spec_id",
                "revision",
                "template_id",
                "entity_type",
                "display_name",
                "fields",
                "stages",
                "default_stage_id",
                "list_field_ids",
                "detail_field_ids",
            ],
            "extensions",
        )
    }

    pub fn field(&self, field_id: &str) -> Option<&FieldDefinition> {
        self.fields.iter().find(|field| field.id == field_id)
    }

    pub fn field_for_role(&self, role: FieldRole) -> Option<&FieldDefinition> {
        self.fields.iter().find(|field| field.role == role)
    }

    pub fn stage(&self, stage_id: &str) -> Option<&StageDefinition> {
        self.stages.iter().find(|stage| stage.id == stage_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecReference {
    pub spec_id: String,
    pub revision: u32,
}

impl From<&ToolSpec> for SpecReference {
    fn from(spec: &ToolSpec) -> Self {
        Self {
            spec_id: spec.spec_id.clone(),
            revision: spec.revision,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDateSettings {
    /// A fixed UTC offset used to map timestamped events to calendar days.
    /// Version 1 deliberately does not infer business days or daylight-saving rules.
    pub calendar_utc_offset_seconds: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerPolicy {
    PauseStagesMarkedPaused,
    CountPausedStages,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateDuePolicy {
    KeepOriginal,
    ExtendByPausedDays,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum ReminderPolicy {
    Never,
    WaitingBeforeDue { days_before_due: u32 },
    WaitingAfterDays { days_waiting: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorPolicy {
    pub timer: TimerPolicy,
    pub due_date: DateDuePolicy,
    pub reminder: ReminderPolicy,
}

impl Default for BehaviorPolicy {
    fn default() -> Self {
        Self {
            timer: TimerPolicy::PauseStagesMarkedPaused,
            due_date: DateDuePolicy::KeepOriginal,
            reminder: ReminderPolicy::Never,
        }
    }
}

impl BehaviorPolicy {
    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        match self.reminder {
            ReminderPolicy::Never => Ok(()),
            ReminderPolicy::WaitingBeforeDue { days_before_due } if days_before_due <= 3650 => {
                Ok(())
            }
            ReminderPolicy::WaitingAfterDays { days_waiting }
                if days_waiting > 0 && days_waiting <= 3650 =>
            {
                Ok(())
            }
            _ => Err(validation(
                ValidationCode::InvalidBehavior,
                "reminder",
                "reminder thresholds must be within the supported finite day range",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorRevision {
    pub revision_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub parent_revision_id: Option<String>,
    pub policy: BehaviorPolicy,
    pub reason: String,
    #[serde(with = "fixed_datetime_serde")]
    pub created_at: DateTime<FixedOffset>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKey {
    Timer,
    DeliveryTarget,
    Reminder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    SingleRecord,
    FutureRecords,
    IncompleteAndFuture,
    AllExistingAndFuture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeSelection {
    pub kind: ScopeKind,
    pub confirmed_generation: u64,
    pub frozen_record_ids: Vec<String>,
    pub applies_to_future_records: bool,
    pub effective_sequence: u64,
}

impl ScopeSelection {
    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        if self.frozen_record_ids.len() > MAX_RECORDS {
            return Err(validation(
                ValidationCode::LimitExceeded,
                "frozen_record_ids",
                "scope exceeds the supported record-reference limit",
            ));
        }
        let mut ids = HashSet::new();
        if self
            .frozen_record_ids
            .iter()
            .any(|id| !valid_id(id) || !ids.insert(id.as_str()))
        {
            return Err(validation(
                ValidationCode::InvalidScope,
                "frozen_record_ids",
                "scope record IDs must be valid and unique",
            ));
        }
        let shape_is_valid = match self.kind {
            ScopeKind::SingleRecord => {
                self.frozen_record_ids.len() == 1 && !self.applies_to_future_records
            }
            ScopeKind::FutureRecords => {
                self.frozen_record_ids.is_empty() && self.applies_to_future_records
            }
            ScopeKind::IncompleteAndFuture | ScopeKind::AllExistingAndFuture => {
                self.applies_to_future_records
            }
        };
        if !shape_is_valid {
            return Err(validation(
                ValidationCode::InvalidScope,
                "scope",
                "scope kind does not match its frozen record set and future-default flag",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleBinding {
    pub binding_id: String,
    pub rule_key: RuleKey,
    pub record_id: Option<String>,
    pub behavior_revision_id: String,
    pub previous_binding_id: Option<String>,
    pub scope: ScopeSelection,
    pub effective_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRecord {
    pub record_id: String,
    pub spec: SpecReference,
    pub record_revision: u64,
    pub created_sequence: u64,
    #[serde(with = "fixed_datetime_serde")]
    pub created_at: DateTime<FixedOffset>,
    pub typed_values: RecordValues,
    pub current_stage_id: String,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldValueChange {
    pub field_id: String,
    pub before: Option<FieldValue>,
    pub after: Option<FieldValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordEventKind {
    Created {
        initial_stage_id: String,
        initial_values: RecordValues,
    },
    FieldsChanged {
        changes: Vec<FieldValueChange>,
    },
    StageChanged {
        from_stage_id: String,
        to_stage_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordEvent {
    pub event_id: String,
    pub record_id: String,
    pub sequence: u64,
    pub record_revision: u64,
    #[serde(with = "fixed_datetime_serde")]
    pub occurred_at: DateTime<FixedOffset>,
    pub kind: RecordEventKind,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionChoice {
    Adopt,
    Both,
    Neither,
    Defer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Pending,
    Active,
    Superseded,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub intent_revision: u32,
    pub choice: DecisionChoice,
    pub expected_outcomes: Vec<EvidenceObservation>,
    pub scope: Option<ScopeSelection>,
    pub rationale: String,
    pub unresolved_questions: Vec<String>,
    pub scenario_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub supersedes: Vec<String>,
    pub status: DecisionStatus,
    #[serde(with = "fixed_datetime_serde")]
    pub created_at: DateTime<FixedOffset>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum ScenarioStep {
    CreateRecord {
        operation_id: String,
        record_id: String,
        initial_stage_id: String,
        values: RecordValues,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    EditRecord {
        operation_id: String,
        record_id: String,
        expected_record_revision: u64,
        changes: BTreeMap<String, Option<FieldValue>>,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    TransitionStage {
        operation_id: String,
        record_id: String,
        expected_record_revision: u64,
        to_stage_id: String,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    AdvanceDate {
        #[serde(with = "naive_date_serde")]
        as_of_date: NaiveDate,
    },
    Observe {
        record_id: String,
    },
}

#[derive(Clone)]
struct ScenarioRecordState {
    spec: SpecReference,
    values: RecordValues,
    current_stage_id: String,
    record_revision: u64,
    created_at: DateTime<FixedOffset>,
    last_event_at: DateTime<FixedOffset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceObservation {
    pub record_id: String,
    pub current_stage_id: String,
    pub elapsed_work_days: Option<i64>,
    pub paused_days: i64,
    #[serde(with = "optional_naive_date_serde")]
    pub original_due_date: Option<NaiveDate>,
    #[serde(with = "optional_naive_date_serde")]
    pub display_due_date: Option<NaiveDate>,
    pub reminder: Option<ReminderObservation>,
}

impl EvidenceObservation {
    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        validate_evidence_observation(self, "observation")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReminderObservation {
    FollowUpWhileWaiting {
        waiting_days: i64,
        days_until_due: Option<i64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    pub scenario_id: String,
    pub name: String,
    pub project_id: String,
    pub base_generation: u64,
    pub spec_revisions: Vec<ToolSpec>,
    pub active_spec: SpecReference,
    pub date_settings: ProjectDateSettings,
    #[serde(with = "fixed_datetime_serde")]
    pub project_created_at: DateTime<FixedOffset>,
    pub event_sequence: u64,
    pub records: Vec<WorkRecord>,
    pub event_history: Vec<RecordEvent>,
    pub candidate_policy: BehaviorPolicy,
    #[serde(with = "naive_date_serde")]
    pub as_of_date: NaiveDate,
    #[serde(with = "fixed_datetime_serde")]
    pub fixed_now: DateTime<FixedOffset>,
    pub steps: Vec<ScenarioStep>,
    pub expected: Vec<EvidenceObservation>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

impl Scenario {
    pub(crate) fn base_snapshot(&self) -> Result<ProjectSnapshot, ProjectValidationError> {
        let initial_spec = self
            .spec_revisions
            .iter()
            .find(|spec| spec.spec_id == self.active_spec.spec_id && spec.revision == 1)
            .ok_or_else(|| {
                validation(
                    ValidationCode::DanglingReference,
                    "scenario.spec_revisions",
                    "scenario spec history lacks revision one",
                )
            })?;
        let mut snapshot = ProjectSnapshot::new(
            self.project_id.clone(),
            self.name.clone(),
            self.project_created_at,
            self.date_settings,
            initial_spec.clone(),
            self.candidate_policy.clone(),
        )?;
        snapshot.spec_revisions = self.spec_revisions.clone();
        snapshot.active_spec = self.active_spec.clone();
        snapshot.generation = self.base_generation;
        snapshot.event_sequence = self.event_sequence;
        snapshot.records = self.records.clone();
        snapshot.event_history = self.event_history.clone();
        snapshot.updated_at = self.fixed_now;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        if self.spec_revisions.len() > MAX_SPEC_REVISIONS
            || self.records.len() > MAX_RECORDS
            || self.event_history.len() > MAX_EVENTS
            || self.steps.len() > MAX_SCENARIO_STEPS
            || self.expected.len() > MAX_SCENARIO_EXPECTATIONS
        {
            return Err(validation(
                ValidationCode::LimitExceeded,
                "scenario",
                "scenario exceeds a supported spec, record, event, step, or expectation limit",
            ));
        }
        if !valid_id(&self.scenario_id) || !valid_id(&self.project_id) || !valid_label(&self.name) {
            return Err(validation(
                ValidationCode::InvalidScenario,
                "scenario",
                "scenario identity and name must be valid and its step count bounded",
            ));
        }
        let specs = validate_spec_revisions(self.spec_revisions.iter(), &self.active_spec)?;
        let active_spec = specs
            .get(&(self.active_spec.spec_id.as_str(), self.active_spec.revision))
            .copied()
            .ok_or_else(|| {
                validation(
                    ValidationCode::DanglingReference,
                    "scenario.active_spec",
                    "scenario active spec revision does not exist",
                )
            })?;
        self.candidate_policy.validate()?;
        let base_snapshot = self.base_snapshot()?;
        base_snapshot.validate()?;
        let calendar_offset = FixedOffset::east_opt(self.date_settings.calendar_utc_offset_seconds)
            .expect("validated project calendar offset");
        let fixed_clock_date = self.fixed_now.with_timezone(&calendar_offset).date_naive();
        if self.as_of_date > fixed_clock_date {
            return Err(validation(
                ValidationCode::InvalidTimestamp,
                "scenario.as_of_date",
                "scenario as-of date cannot exceed its project-local fixed clock date",
            ));
        }
        let mut as_of_date = self.as_of_date;

        let mut record_states = HashMap::new();
        for record in &self.records {
            let last_event_at = self
                .event_history
                .iter()
                .rev()
                .find(|event| event.record_id == record.record_id)
                .expect("validated record history has at least one event")
                .occurred_at;
            record_states.insert(
                record.record_id.clone(),
                ScenarioRecordState {
                    spec: record.spec.clone(),
                    values: record.typed_values.clone(),
                    current_stage_id: record.current_stage_id.clone(),
                    record_revision: record.record_revision,
                    created_at: record.created_at,
                    last_event_at,
                },
            );
        }
        let mut operation_ids: HashSet<String> = self
            .event_history
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        let mut event_count = self.event_history.len();
        for (index, step) in self.steps.iter().enumerate() {
            let operation_id = match step {
                ScenarioStep::CreateRecord { operation_id, .. }
                | ScenarioStep::EditRecord { operation_id, .. }
                | ScenarioStep::TransitionStage { operation_id, .. } => Some(operation_id),
                ScenarioStep::AdvanceDate { .. } | ScenarioStep::Observe { .. } => None,
            };
            if let Some(operation_id) = operation_id {
                if !valid_id(operation_id) {
                    return Err(validation(
                        ValidationCode::InvalidOperation,
                        format!("steps[{index}].operation_id"),
                        "scenario operation ID must be a valid stable ID",
                    ));
                }
                if !operation_ids.insert(operation_id.clone()) {
                    return Err(validation(
                        ValidationCode::DuplicateId,
                        format!("steps[{index}].operation_id"),
                        "scenario operation IDs must be unique across the base history and steps",
                    ));
                }
            }
            match step {
                ScenarioStep::CreateRecord {
                    record_id,
                    initial_stage_id,
                    values,
                    ..
                } if !valid_id(record_id) || active_spec.stage(initial_stage_id).is_none() => {
                    return Err(validation(
                        ValidationCode::InvalidScenario,
                        format!("steps[{index}]"),
                        "scenario create step has an invalid record identity or stage reference",
                    ));
                }
                ScenarioStep::CreateRecord { values, .. }
                    if values.len() > MAX_FIELD_DEFINITIONS =>
                {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        format!("steps[{index}].values"),
                        "scenario record values exceed the supported field limit",
                    ));
                }
                ScenarioStep::CreateRecord {
                    record_id,
                    initial_stage_id,
                    values,
                    occurred_at,
                    ..
                } => {
                    ProjectSnapshot::validate_values(active_spec, values)?;
                    if record_states.contains_key(record_id) {
                        return Err(validation(
                            ValidationCode::DuplicateId,
                            format!("steps[{index}].record_id"),
                            "scenario create step reuses an existing record ID",
                        ));
                    }
                    validate_scenario_event_time(
                        occurred_at,
                        self.project_created_at,
                        self.fixed_now,
                        None,
                        None,
                        &format!("steps[{index}].occurred_at"),
                    )?;
                    if record_states.len() >= MAX_RECORDS || event_count >= MAX_EVENTS {
                        return Err(validation(
                            ValidationCode::LimitExceeded,
                            format!("steps[{index}]"),
                            "scenario create step exceeds the supported record or event limit",
                        ));
                    }
                    record_states.insert(
                        record_id.clone(),
                        ScenarioRecordState {
                            spec: self.active_spec.clone(),
                            values: values.clone(),
                            current_stage_id: initial_stage_id.clone(),
                            record_revision: 1,
                            created_at: *occurred_at,
                            last_event_at: *occurred_at,
                        },
                    );
                    event_count += 1;
                }
                ScenarioStep::EditRecord { record_id, .. }
                | ScenarioStep::TransitionStage { record_id, .. }
                    if !valid_id(record_id) =>
                {
                    return Err(validation(
                        ValidationCode::InvalidScenario,
                        format!("steps[{index}]"),
                        "scenario operation has an invalid record ID",
                    ));
                }
                ScenarioStep::EditRecord { changes, .. }
                    if changes.len() > MAX_FIELD_DEFINITIONS =>
                {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        format!("steps[{index}].changes"),
                        "scenario field changes exceed the supported field limit",
                    ));
                }
                ScenarioStep::EditRecord {
                    record_id,
                    expected_record_revision,
                    changes,
                    occurred_at,
                    ..
                } => {
                    if event_count >= MAX_EVENTS {
                        return Err(validation(
                            ValidationCode::LimitExceeded,
                            format!("steps[{index}]"),
                            "scenario edit step exceeds the supported event limit",
                        ));
                    }
                    let state = record_states.get(record_id).ok_or_else(|| {
                        validation(
                            ValidationCode::DanglingReference,
                            format!("steps[{index}].record_id"),
                            "scenario edit step references a missing record",
                        )
                    })?;
                    if state.record_revision != *expected_record_revision {
                        return Err(validation(
                            ValidationCode::InvalidEvent,
                            format!("steps[{index}].expected_record_revision"),
                            "scenario edit step must reference the current record revision",
                        ));
                    }
                    validate_scenario_event_time(
                        occurred_at,
                        self.project_created_at,
                        self.fixed_now,
                        Some(state.created_at),
                        Some(state.last_event_at),
                        &format!("steps[{index}].occurred_at"),
                    )?;
                    if changes.is_empty() {
                        return Err(validation(
                            ValidationCode::InvalidOperation,
                            format!("steps[{index}].changes"),
                            "scenario edit step must change at least one field",
                        ));
                    }
                    let spec_reference = &state.spec;
                    let spec = specs
                        .get(&(spec_reference.spec_id.as_str(), spec_reference.revision))
                        .copied()
                        .ok_or_else(|| {
                            validation(
                                ValidationCode::DanglingReference,
                                format!("steps[{index}].record_id"),
                                "scenario edit step references an unavailable spec revision",
                            )
                        })?;
                    let mut values = state.values.clone();
                    let mut changed = false;
                    for (field_id, after) in changes {
                        if spec.field(field_id).is_none() {
                            return Err(validation(
                                ValidationCode::UnknownField,
                                format!("steps[{index}].changes.{field_id}"),
                                "scenario edit step references a field not defined by the record spec",
                            ));
                        }
                        if values.get(field_id) != after.as_ref() {
                            changed = true;
                        }
                        match after {
                            Some(value) => {
                                values.insert(field_id.clone(), value.clone());
                            }
                            None => {
                                values.remove(field_id);
                            }
                        }
                    }
                    if !changed {
                        return Err(validation(
                            ValidationCode::InvalidOperation,
                            format!("steps[{index}].changes"),
                            "scenario edit step does not change any field value",
                        ));
                    }
                    ProjectSnapshot::validate_values(spec, &values)?;
                    let state = record_states
                        .get_mut(record_id)
                        .expect("the record state was checked before editing");
                    state.values = values;
                    state.record_revision += 1;
                    state.last_event_at = *occurred_at;
                    event_count += 1;
                }
                ScenarioStep::TransitionStage {
                    record_id,
                    expected_record_revision,
                    to_stage_id,
                    occurred_at,
                    ..
                } => {
                    if event_count >= MAX_EVENTS {
                        return Err(validation(
                            ValidationCode::LimitExceeded,
                            format!("steps[{index}]"),
                            "scenario transition step exceeds the supported event limit",
                        ));
                    }
                    let state = record_states.get(record_id).ok_or_else(|| {
                        validation(
                            ValidationCode::DanglingReference,
                            format!("steps[{index}].record_id"),
                            "scenario transition step references a missing record",
                        )
                    })?;
                    if state.record_revision != *expected_record_revision {
                        return Err(validation(
                            ValidationCode::InvalidEvent,
                            format!("steps[{index}].expected_record_revision"),
                            "scenario transition step must reference the current record revision",
                        ));
                    }
                    validate_scenario_event_time(
                        occurred_at,
                        self.project_created_at,
                        self.fixed_now,
                        Some(state.created_at),
                        Some(state.last_event_at),
                        &format!("steps[{index}].occurred_at"),
                    )?;
                    let spec_reference = &state.spec;
                    let spec = specs
                        .get(&(spec_reference.spec_id.as_str(), spec_reference.revision))
                        .copied()
                        .ok_or_else(|| {
                            validation(
                                ValidationCode::DanglingReference,
                                format!("steps[{index}].record_id"),
                                "scenario transition step references an unavailable spec revision",
                            )
                        })?;
                    let from = spec.stage(&state.current_stage_id).ok_or_else(|| {
                        validation(
                            ValidationCode::DanglingReference,
                            format!("steps[{index}].record_id"),
                            "scenario record stage is not defined by its spec revision",
                        )
                    })?;
                    if spec.stage(to_stage_id).is_none() {
                        return Err(validation(
                            ValidationCode::DanglingReference,
                            format!("steps[{index}].to_stage_id"),
                            "scenario transition target is not defined by the record spec",
                        ));
                    }
                    if from.terminal
                        || from.id == *to_stage_id
                        || !from
                            .allowed_next_stage_ids
                            .iter()
                            .any(|stage_id| stage_id == to_stage_id)
                    {
                        return Err(validation(
                            ValidationCode::InvalidTransition,
                            format!("steps[{index}].to_stage_id"),
                            "scenario transition is not allowed from the record's current stage",
                        ));
                    }
                    let state = record_states
                        .get_mut(record_id)
                        .expect("the record state was checked before transition");
                    state.current_stage_id = to_stage_id.clone();
                    state.record_revision += 1;
                    state.last_event_at = *occurred_at;
                    event_count += 1;
                }
                ScenarioStep::Observe { record_id } if !valid_id(record_id) => {
                    return Err(validation(
                        ValidationCode::InvalidScenario,
                        format!("steps[{index}]"),
                        "scenario observation has an invalid record ID",
                    ));
                }
                ScenarioStep::Observe { record_id } => {
                    let state = record_states.get(record_id).ok_or_else(|| {
                        validation(
                            ValidationCode::DanglingReference,
                            format!("steps[{index}].record_id"),
                            "scenario observation references a missing record",
                        )
                    })?;
                    let spec = specs
                        .get(&(state.spec.spec_id.as_str(), state.spec.revision))
                        .copied()
                        .expect("scenario record spec was validated above");
                    validate_scenario_observation_date(
                        state,
                        spec,
                        as_of_date,
                        calendar_offset,
                        &format!("steps[{index}].record_id"),
                    )?;
                }
                ScenarioStep::AdvanceDate {
                    as_of_date: next_date,
                } => {
                    if *next_date < as_of_date || *next_date > fixed_clock_date {
                        return Err(validation(
                            ValidationCode::InvalidTimestamp,
                            format!("steps[{index}].as_of_date"),
                            "scenario dates must move forward and cannot exceed the fixed clock date",
                        ));
                    }
                    as_of_date = *next_date;
                }
            }
        }
        if !self
            .steps
            .iter()
            .any(|step| matches!(step, ScenarioStep::Observe { .. }))
        {
            for (record_id, state) in &record_states {
                let spec = specs
                    .get(&(state.spec.spec_id.as_str(), state.spec.revision))
                    .copied()
                    .expect("scenario record spec was validated above");
                validate_scenario_observation_date(
                    state,
                    spec,
                    as_of_date,
                    calendar_offset,
                    &format!("records.{record_id}"),
                )?;
            }
        }
        validate_evidence_observations(&self.expected, "scenario.expected")?;
        for (index, observation) in self.expected.iter().enumerate() {
            let state = record_states.get(&observation.record_id).ok_or_else(|| {
                validation(
                    ValidationCode::DanglingReference,
                    format!("scenario.expected[{index}].record_id"),
                    "scenario expectation references a missing record",
                )
            })?;
            let spec = specs
                .get(&(state.spec.spec_id.as_str(), state.spec.revision))
                .copied()
                .expect("scenario record spec was validated above");
            if spec.stage(&observation.current_stage_id).is_none() {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    format!("scenario.expected[{index}].current_stage_id"),
                    "scenario expectation stage is not defined by the record spec",
                ));
            }
        }
        validate_extension_keys(
            &self.extensions,
            &[
                "scenario_id",
                "name",
                "project_id",
                "base_generation",
                "spec_revisions",
                "active_spec",
                "date_settings",
                "project_created_at",
                "event_sequence",
                "records",
                "event_history",
                "candidate_policy",
                "as_of_date",
                "fixed_now",
                "steps",
                "expected",
            ],
            "scenario.extensions",
        )?;
        Ok(())
    }
}

fn validate_scenario_event_time(
    occurred_at: &DateTime<FixedOffset>,
    project_created_at: DateTime<FixedOffset>,
    fixed_now: DateTime<FixedOffset>,
    record_created_at: Option<DateTime<FixedOffset>>,
    previous_event_at: Option<DateTime<FixedOffset>>,
    path: &str,
) -> Result<(), ProjectValidationError> {
    if *occurred_at < project_created_at
        || *occurred_at > fixed_now
        || record_created_at.is_some_and(|created_at| *occurred_at < created_at)
        || previous_event_at.is_some_and(|previous| *occurred_at < previous)
    {
        return Err(validation(
            ValidationCode::InvalidTimestamp,
            path,
            "scenario event time must follow project and record history and not exceed its fixed clock",
        ));
    }
    Ok(())
}

fn validate_scenario_observation_date(
    state: &ScenarioRecordState,
    spec: &ToolSpec,
    as_of_date: NaiveDate,
    calendar_offset: FixedOffset,
    path: &str,
) -> Result<(), ProjectValidationError> {
    let record_created_date = state
        .created_at
        .with_timezone(&calendar_offset)
        .date_naive();
    let last_event_date = state
        .last_event_at
        .with_timezone(&calendar_offset)
        .date_naive();
    let work_start = spec
        .field_for_role(FieldRole::WorkStartedOn)
        .and_then(|field| state.values.get(&field.id))
        .and_then(|value| match value {
            FieldValue::Date(date) => Some(*date),
            _ => None,
        });
    if as_of_date < record_created_date
        || as_of_date < last_event_date
        || work_start.is_some_and(|date| date > as_of_date)
    {
        return Err(validation(
            ValidationCode::InvalidTimestamp,
            path,
            "scenario observation date precedes record history or its work start date",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Executed,
    Passed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    NativeExecution,
    ImportedUntrusted,
    Prototype,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub run_id: String,
    pub scenario_id: String,
    pub status: EvidenceStatus,
    pub observations: Vec<EvidenceObservation>,
    pub expected: Vec<EvidenceObservation>,
    pub input_fingerprint: String,
    pub candidate_fingerprint: String,
    pub runtime_semantics_version: u32,
    pub source: EvidenceSource,
    pub errors: Vec<String>,
    #[serde(with = "fixed_datetime_serde")]
    pub completed_at: DateTime<FixedOffset>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalSource {
    LocalBuiltIn,
    ExternalHarness,
    ImportedFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalEnvelope {
    pub proposal_id: String,
    pub project_id: String,
    pub baseline_fingerprint: String,
    pub source: ProposalSource,
    pub candidate_policies: Vec<BehaviorPolicy>,
    pub rationale: String,
    pub unknowns: Vec<String>,
    pub imported_claims: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub format_version: u32,
    pub required_features: Vec<String>,
    pub project_id: String,
    pub project_name: String,
    pub generation: u64,
    pub event_sequence: u64,
    #[serde(with = "fixed_datetime_serde")]
    pub created_at: DateTime<FixedOffset>,
    #[serde(with = "fixed_datetime_serde")]
    pub updated_at: DateTime<FixedOffset>,
    pub date_settings: ProjectDateSettings,
    pub spec_revisions: Vec<ToolSpec>,
    pub active_spec: SpecReference,
    pub records: Vec<WorkRecord>,
    pub event_history: Vec<RecordEvent>,
    pub behavior_revisions: Vec<BehaviorRevision>,
    pub active_behavior_revision_id: String,
    pub rule_bindings: Vec<RuleBinding>,
    pub decisions: Vec<DecisionRecord>,
    pub scenarios: Vec<Scenario>,
    pub evidence: Vec<Evidence>,
    pub proposals: Vec<ProposalEnvelope>,
    pub operation_receipts: BTreeMap<String, String>,
    #[serde(flatten)]
    pub extensions: ExtensionFields,
}

impl ProjectSnapshot {
    pub fn new(
        project_id: String,
        project_name: String,
        created_at: DateTime<FixedOffset>,
        date_settings: ProjectDateSettings,
        spec: ToolSpec,
        initial_policy: BehaviorPolicy,
    ) -> Result<Self, ProjectValidationError> {
        spec.validate()?;
        initial_policy.validate()?;
        let snapshot = Self {
            format_version: PROJECT_FORMAT_VERSION,
            required_features: Vec::new(),
            project_id,
            project_name,
            generation: 0,
            event_sequence: 0,
            created_at: created_at.clone(),
            updated_at: created_at.clone(),
            date_settings,
            active_spec: SpecReference::from(&spec),
            spec_revisions: vec![spec],
            records: Vec::new(),
            event_history: Vec::new(),
            behavior_revisions: vec![BehaviorRevision {
                revision_id: "behavior-1".to_owned(),
                operation_id: None,
                parent_revision_id: None,
                policy: initial_policy,
                reason: "Initial project behavior".to_owned(),
                created_at,
            }],
            active_behavior_revision_id: "behavior-1".to_owned(),
            rule_bindings: Vec::new(),
            decisions: Vec::new(),
            scenarios: Vec::new(),
            evidence: Vec::new(),
            proposals: Vec::new(),
            operation_receipts: BTreeMap::new(),
            extensions: BTreeMap::new(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn active_spec(&self) -> Option<&ToolSpec> {
        self.spec_revisions.iter().find(|spec| {
            spec.spec_id == self.active_spec.spec_id && spec.revision == self.active_spec.revision
        })
    }

    pub fn behavior_revision(&self, revision_id: &str) -> Option<&BehaviorRevision> {
        self.behavior_revisions
            .iter()
            .find(|revision| revision.revision_id == revision_id)
    }

    pub fn active_behavior(&self) -> Option<&BehaviorPolicy> {
        self.behavior_revision(&self.active_behavior_revision_id)
            .map(|revision| &revision.policy)
    }

    pub fn validate_values(
        spec: &ToolSpec,
        values: &RecordValues,
    ) -> Result<(), ProjectValidationError> {
        for field in &spec.fields {
            match values.get(&field.id) {
                None if field.required => {
                    return Err(validation(
                        ValidationCode::MissingRequiredField,
                        format!("values.{}", field.id),
                        "required field is missing",
                    ));
                }
                None => {}
                Some(value) if !field_accepts_value(field, value) => {
                    return Err(validation(
                        ValidationCode::InvalidFieldValue,
                        format!("values.{}", field.id),
                        "field value does not match its declared type or enum options",
                    ));
                }
                Some(FieldValue::Text(text)) if field.required && text.trim().is_empty() => {
                    return Err(validation(
                        ValidationCode::MissingRequiredField,
                        format!("values.{}", field.id),
                        "required text field cannot be empty",
                    ));
                }
                Some(FieldValue::Text(text)) if text.len() > 16_384 => {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        format!("values.{}", field.id),
                        "text field exceeds the supported size limit",
                    ));
                }
                _ => {}
            }
        }
        if values.keys().any(|field_id| spec.field(field_id).is_none()) {
            return Err(validation(
                ValidationCode::UnknownField,
                "values",
                "record contains a field ID not defined by its spec revision",
            ));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ProjectValidationError> {
        if self.format_version != PROJECT_FORMAT_VERSION {
            return Err(validation(
                ValidationCode::UnsupportedFormat,
                "format_version",
                "project snapshot format is not supported for writing",
            ));
        }
        if !valid_id(&self.project_id) || !valid_label(&self.project_name) {
            return Err(validation(
                ValidationCode::InvalidProject,
                "project",
                "project ID and name must be valid",
            ));
        }
        if self.updated_at < self.created_at {
            return Err(validation(
                ValidationCode::InvalidTimestamp,
                "updated_at",
                "project update time cannot precede project creation",
            ));
        }
        if !(-86_399..=86_399).contains(&self.date_settings.calendar_utc_offset_seconds) {
            return Err(validation(
                ValidationCode::InvalidProject,
                "date_settings.calendar_utc_offset_seconds",
                "calendar offset must be a valid fixed UTC offset",
            ));
        }
        if self.records.len() > MAX_RECORDS
            || self.event_history.len() > MAX_EVENTS
            || self.spec_revisions.len() > MAX_SPEC_REVISIONS
            || self.behavior_revisions.len() > MAX_BEHAVIOR_REVISIONS
            || self.rule_bindings.len() > MAX_RULE_BINDINGS
            || self.decisions.len() > MAX_DECISIONS
            || self.scenarios.len() > MAX_SCENARIOS
            || self.evidence.len() > MAX_EVIDENCE
            || self.proposals.len() > MAX_PROPOSALS
            || self.operation_receipts.len() > MAX_OPERATION_RECEIPTS
            || self.required_features.len() > MAX_REQUIRED_FEATURES
        {
            return Err(validation(
                ValidationCode::LimitExceeded,
                "project.history",
                "project exceeds a supported record, history, evidence, or feature limit",
            ));
        }
        if self
            .required_features
            .iter()
            .any(|feature| !SUPPORTED_REQUIRED_FEATURES.contains(&feature.as_str()))
        {
            return Err(validation(
                ValidationCode::UnsupportedFeature,
                "required_features",
                "project requires a feature this runtime does not support",
            ));
        }
        let specs = validate_spec_revisions(self.spec_revisions.iter(), &self.active_spec)?;

        let mut behavior_ids = HashSet::new();
        for revision in &self.behavior_revisions {
            if !valid_id(&revision.revision_id)
                || behavior_ids.contains(revision.revision_id.as_str())
            {
                return Err(validation(
                    ValidationCode::DuplicateOrInvalidId,
                    "behavior_revisions",
                    "behavior revision IDs must be valid and unique",
                ));
            }
            revision.policy.validate()?;
            if !valid_label(&revision.reason) {
                return Err(validation(
                    ValidationCode::InvalidBehavior,
                    "behavior_revisions.reason",
                    "behavior revisions need a nonempty, bounded reason",
                ));
            }
            if let Some(parent_id) = &revision.parent_revision_id {
                if !behavior_ids.contains(parent_id.as_str()) {
                    return Err(validation(
                        ValidationCode::DanglingReference,
                        "behavior_revisions.parent_revision_id",
                        "a behavior revision parent must precede the revision that references it",
                    ));
                }
                let parent = self
                    .behavior_revisions
                    .iter()
                    .find(|candidate| candidate.revision_id == *parent_id)
                    .expect("parent ID was found in the preceding revisions");
                if revision.created_at < parent.created_at {
                    return Err(validation(
                        ValidationCode::InvalidTimestamp,
                        "behavior_revisions.created_at",
                        "behavior revision time cannot precede its parent",
                    ));
                }
            }
            if revision.created_at < self.created_at || revision.created_at > self.updated_at {
                return Err(validation(
                    ValidationCode::InvalidTimestamp,
                    "behavior_revisions.created_at",
                    "behavior revision time must be within the project lifetime",
                ));
            }
            behavior_ids.insert(revision.revision_id.as_str());
        }
        if !behavior_ids.contains(self.active_behavior_revision_id.as_str()) {
            return Err(validation(
                ValidationCode::DanglingReference,
                "active_behavior_revision_id",
                "active behavior revision does not exist",
            ));
        }

        let mut records = HashMap::new();
        for record in &self.records {
            if !valid_id(&record.record_id)
                || records.insert(record.record_id.as_str(), record).is_some()
            {
                return Err(validation(
                    ValidationCode::DuplicateOrInvalidId,
                    "records.record_id",
                    "record IDs must be valid and unique",
                ));
            }
            let spec = specs
                .get(&(record.spec.spec_id.as_str(), record.spec.revision))
                .ok_or_else(|| {
                    validation(
                        ValidationCode::DanglingReference,
                        format!("records.{}.spec", record.record_id),
                        "record spec revision does not exist",
                    )
                })?;
            Self::validate_values(spec, &record.typed_values)?;
            if spec.stage(&record.current_stage_id).is_none()
                || record.record_revision == 0
                || record.created_sequence == 0
                || record.created_at < self.created_at
                || record.created_at > self.updated_at
            {
                return Err(validation(
                    ValidationCode::InvalidRecord,
                    format!("records.{}", record.record_id),
                    "record stage, revision, sequence, or timestamps are invalid",
                ));
            }
            validate_extension_keys(
                &record.extensions,
                &[
                    "record_id",
                    "spec",
                    "record_revision",
                    "created_sequence",
                    "created_at",
                    "typed_values",
                    "current_stage_id",
                ],
                "record.extensions",
            )?;
        }

        let mut events_by_record: HashMap<&str, Vec<&RecordEvent>> = HashMap::new();
        let mut event_ids = HashSet::new();
        let mut expected_sequence = 1;
        for (index, event) in self.event_history.iter().enumerate() {
            if !valid_id(&event.event_id)
                || !event_ids.insert(event.event_id.as_str())
                || !records.contains_key(event.record_id.as_str())
                || event.sequence != expected_sequence
                || event.sequence > self.event_sequence
                || event.occurred_at < self.created_at
                || event.occurred_at > self.updated_at
            {
                return Err(validation(
                    ValidationCode::InvalidEvent,
                    format!("event_history[{index}]"),
                    "event identity, record reference, sequence, or timestamp is invalid",
                ));
            }
            expected_sequence += 1;
            events_by_record
                .entry(event.record_id.as_str())
                .or_default()
                .push(event);
            validate_extension_keys(
                &event.extensions,
                &[
                    "event_id",
                    "record_id",
                    "sequence",
                    "record_revision",
                    "occurred_at",
                    "kind",
                ],
                "event.extensions",
            )?;
        }
        for revision in &self.behavior_revisions {
            if let Some(operation_id) = &revision.operation_id {
                if !valid_id(operation_id) || !event_ids.insert(operation_id.as_str()) {
                    return Err(validation(
                        ValidationCode::InvalidOperation,
                        "behavior_revisions.operation_id",
                        "behavior command operation IDs must be valid and unique across the project",
                    ));
                }
            }
        }
        if self.event_sequence != self.event_history.len() as u64 {
            return Err(validation(
                ValidationCode::InvalidEvent,
                "event_sequence",
                "project event sequence must match the committed event history",
            ));
        }
        for record in &self.records {
            let spec = specs
                .get(&(record.spec.spec_id.as_str(), record.spec.revision))
                .expect("record spec was checked above");
            let events = events_by_record
                .get(record.record_id.as_str())
                .ok_or_else(|| {
                    validation(
                        ValidationCode::InvalidEvent,
                        format!("records.{}.events", record.record_id),
                        "every record must have a creation event",
                    )
                })?;
            validate_record_history(record, spec, events)?;
        }

        let mut binding_ids = HashSet::new();
        for binding in &self.rule_bindings {
            if !valid_id(&binding.binding_id) || !binding_ids.insert(binding.binding_id.as_str()) {
                return Err(validation(
                    ValidationCode::DuplicateOrInvalidId,
                    "rule_bindings.binding_id",
                    "binding IDs must be valid and unique",
                ));
            }
            if !behavior_ids.contains(binding.behavior_revision_id.as_str())
                || binding
                    .record_id
                    .as_ref()
                    .is_some_and(|id| !records.contains_key(id.as_str()))
            {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    "rule_bindings",
                    "binding references a missing behavior revision or record",
                ));
            }
            binding.scope.validate()?;
            if binding
                .scope
                .frozen_record_ids
                .iter()
                .any(|id| !records.contains_key(id.as_str()))
            {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    "rule_bindings.scope.frozen_record_ids",
                    "a frozen scope record must exist in the project",
                ));
            }
        }
        for (index, binding) in self.rule_bindings.iter().enumerate() {
            let Some(previous_id) = &binding.previous_binding_id else {
                continue;
            };
            let previous = self.rule_bindings[..index]
                .iter()
                .find(|candidate| candidate.binding_id == *previous_id)
                .ok_or_else(|| {
                    validation(
                        ValidationCode::InvalidScope,
                        "rule_bindings.previous_binding_id",
                        "previous binding must refer to an earlier binding",
                    )
                })?;
            if previous.rule_key != binding.rule_key || previous.record_id != binding.record_id {
                return Err(validation(
                    ValidationCode::InvalidScope,
                    "rule_bindings.previous_binding_id",
                    "a binding can only continue history for the same rule and record",
                ));
            }
        }

        let scenario_ids = unique_ids(
            self.scenarios
                .iter()
                .map(|scenario| scenario.scenario_id.as_str()),
            "scenarios.scenario_id",
        )?;
        let scenarios_by_id: HashMap<&str, &Scenario> = self
            .scenarios
            .iter()
            .map(|scenario| (scenario.scenario_id.as_str(), scenario))
            .collect();
        for scenario in &self.scenarios {
            scenario.validate()?;
            if scenario.project_id != self.project_id {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    "scenarios.project_id",
                    "scenario project ID does not match its containing project",
                ));
            }
            if scenario.project_created_at != self.created_at {
                return Err(validation(
                    ValidationCode::InvalidTimestamp,
                    "scenarios.project_created_at",
                    "scenario creation time must match the immutable project creation time",
                ));
            }
        }
        let evidence_ids = unique_ids(
            self.evidence
                .iter()
                .map(|evidence| evidence.run_id.as_str()),
            "evidence.run_id",
        )?;
        for evidence in &self.evidence {
            if evidence.observations.len() > MAX_RECORDS
                || evidence.expected.len() > MAX_RECORDS
                || evidence.errors.len() > MAX_EVIDENCE_ERRORS
            {
                return Err(validation(
                    ValidationCode::LimitExceeded,
                    "evidence",
                    "evidence exceeds a supported observation or error limit",
                ));
            }
            validate_evidence_observations(&evidence.observations, "evidence.observations")?;
            validate_evidence_observations(&evidence.expected, "evidence.expected")?;
            if !scenario_ids.contains(evidence.scenario_id.as_str())
                || evidence.runtime_semantics_version == 0
                || !valid_fingerprint(&evidence.input_fingerprint)
                || !valid_fingerprint(&evidence.candidate_fingerprint)
            {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    "evidence",
                    "evidence has an invalid scenario, runtime version, or fingerprint",
                ));
            }
            let linked_scenario = scenarios_by_id
                .get(evidence.scenario_id.as_str())
                .copied()
                .expect("evidence scenario reference was checked above");
            validate_evidence_observations_in_scenarios(
                &evidence.observations,
                std::slice::from_ref(&linked_scenario),
                "evidence.observations",
            )?;
            validate_evidence_observations_in_scenarios(
                &evidence.expected,
                std::slice::from_ref(&linked_scenario),
                "evidence.expected",
            )?;
            if evidence.errors.iter().any(|error| !valid_label(error)) {
                return Err(validation(
                    ValidationCode::InvalidProposal,
                    "evidence.errors",
                    "evidence error messages must be nonempty and bounded",
                ));
            }
            validate_extension_keys(
                &evidence.extensions,
                &[
                    "run_id",
                    "scenario_id",
                    "status",
                    "observations",
                    "expected",
                    "input_fingerprint",
                    "candidate_fingerprint",
                    "runtime_semantics_version",
                    "source",
                    "errors",
                    "completed_at",
                ],
                "evidence.extensions",
            )?;
        }
        let evidence_by_id: HashMap<&str, &Evidence> = self
            .evidence
            .iter()
            .map(|evidence| (evidence.run_id.as_str(), evidence))
            .collect();
        unique_ids(
            self.decisions
                .iter()
                .map(|decision| decision.decision_id.as_str()),
            "decisions.decision_id",
        )?;
        let mut earlier_decisions = HashMap::new();
        for decision in &self.decisions {
            if decision.expected_outcomes.len() > MAX_DECISION_EXPECTATIONS
                || decision.unresolved_questions.len() > MAX_PROPOSAL_UNKNOWN_ITEMS
                || decision.scenario_ids.len() > MAX_SCENARIOS
                || decision.evidence_ids.len() > MAX_EVIDENCE
                || decision.supersedes.len() > MAX_DECISIONS
            {
                return Err(validation(
                    ValidationCode::LimitExceeded,
                    "decisions",
                    "decision exceeds a supported outcome, question, or reference limit",
                ));
            }
            validate_evidence_observations(
                &decision.expected_outcomes,
                "decisions.expected_outcomes",
            )?;
            if decision.created_at < self.created_at || decision.created_at > self.updated_at {
                return Err(validation(
                    ValidationCode::InvalidTimestamp,
                    "decisions.created_at",
                    "decision time must be within the project lifetime",
                ));
            }
            if decision.intent_revision == 0
                || !valid_label(&decision.rationale)
                || decision
                    .unresolved_questions
                    .iter()
                    .any(|question| !valid_label(question))
                || decision
                    .scope
                    .as_ref()
                    .is_some_and(|scope| scope.validate().is_err())
                || decision.scope.as_ref().is_some_and(|scope| {
                    scope
                        .frozen_record_ids
                        .iter()
                        .any(|id| !records.contains_key(id.as_str()))
                })
                || decision
                    .scenario_ids
                    .iter()
                    .any(|id| !scenario_ids.contains(id.as_str()))
                || decision
                    .evidence_ids
                    .iter()
                    .any(|id| !evidence_ids.contains(id.as_str()))
                || decision
                    .supersedes
                    .iter()
                    .any(|id| !earlier_decisions.contains_key(id.as_str()))
            {
                return Err(validation(
                    ValidationCode::DanglingReference,
                    "decisions",
                    "decision contains an invalid scope or unresolved scenario, evidence, or earlier decision reference",
                ));
            }
            if decision.supersedes.iter().any(|id| {
                earlier_decisions
                    .get(id.as_str())
                    .is_some_and(|earlier: &&DecisionRecord| {
                        earlier.created_at > decision.created_at
                    })
            }) {
                return Err(validation(
                    ValidationCode::InvalidTimestamp,
                    "decisions.created_at",
                    "a replacement decision cannot predate a decision it supersedes",
                ));
            }
            let mut linked_scenarios: Vec<&Scenario> = decision
                .scenario_ids
                .iter()
                .filter_map(|id| scenarios_by_id.get(id.as_str()).copied())
                .collect();
            for evidence_id in &decision.evidence_ids {
                let evidence = evidence_by_id
                    .get(evidence_id.as_str())
                    .expect("decision evidence reference was checked above");
                let scenario = scenarios_by_id
                    .get(evidence.scenario_id.as_str())
                    .expect("evidence scenario reference was checked above");
                linked_scenarios.push(*scenario);
            }
            if linked_scenarios.is_empty() {
                validate_evidence_observations_in_project(
                    &decision.expected_outcomes,
                    &records,
                    &specs,
                    "decisions.expected_outcomes",
                )?;
            } else {
                validate_evidence_observations_in_scenarios(
                    &decision.expected_outcomes,
                    &linked_scenarios,
                    "decisions.expected_outcomes",
                )?;
            }
            validate_extension_keys(
                &decision.extensions,
                &[
                    "decision_id",
                    "intent_revision",
                    "choice",
                    "expected_outcomes",
                    "scope",
                    "rationale",
                    "unresolved_questions",
                    "scenario_ids",
                    "evidence_ids",
                    "supersedes",
                    "status",
                    "created_at",
                ],
                "decision.extensions",
            )?;
            earlier_decisions.insert(decision.decision_id.as_str(), decision);
        }
        let proposal_ids = unique_ids(
            self.proposals
                .iter()
                .map(|proposal| proposal.proposal_id.as_str()),
            "proposals.proposal_id",
        )?;
        let _ = proposal_ids;
        for proposal in &self.proposals {
            if proposal.unknowns.len() > MAX_PROPOSAL_UNKNOWN_ITEMS
                || proposal.imported_claims.len() > MAX_PROPOSAL_CLAIMS
            {
                return Err(validation(
                    ValidationCode::LimitExceeded,
                    "proposals",
                    "proposal exceeds a supported unknown-item or imported-claim limit",
                ));
            }
            if !valid_fingerprint(&proposal.baseline_fingerprint)
                || proposal.project_id != self.project_id
                || proposal.candidate_policies.is_empty()
                || proposal.candidate_policies.len() > 32
                || !valid_label(&proposal.rationale)
                || proposal
                    .unknowns
                    .iter()
                    .any(|unknown| !valid_label(unknown))
            {
                return Err(validation(
                    ValidationCode::InvalidProposal,
                    "proposals",
                    "proposal envelope has an invalid baseline, project reference, or candidate count",
                ));
            }
            for policy in &proposal.candidate_policies {
                policy.validate()?;
            }
            validate_json_entries(
                proposal
                    .imported_claims
                    .iter()
                    .map(|(key, value)| (key.as_str(), value)),
                proposal.imported_claims.len(),
                "proposals.imported_claims",
            )?;
            validate_extension_keys(
                &proposal.extensions,
                &[
                    "proposal_id",
                    "project_id",
                    "baseline_fingerprint",
                    "source",
                    "candidate_policies",
                    "rationale",
                    "unknowns",
                    "imported_claims",
                ],
                "proposal.extensions",
            )?;
        }
        for (operation_id, fingerprint) in &self.operation_receipts {
            if !valid_id(operation_id) || !valid_fingerprint(fingerprint) {
                return Err(validation(
                    ValidationCode::InvalidOperation,
                    "operation_receipts",
                    "committed operation IDs and request fingerprints must be valid",
                ));
            }
        }

        validate_extension_keys(
            &self.extensions,
            &[
                "format_version",
                "required_features",
                "project_id",
                "project_name",
                "generation",
                "event_sequence",
                "created_at",
                "updated_at",
                "date_settings",
                "spec_revisions",
                "active_spec",
                "records",
                "event_history",
                "behavior_revisions",
                "active_behavior_revision_id",
                "rule_bindings",
                "decisions",
                "scenarios",
                "evidence",
                "proposals",
                "operation_receipts",
            ],
            "project.extensions",
        )
    }
}

fn validate_record_history(
    record: &WorkRecord,
    spec: &ToolSpec,
    events: &[&RecordEvent],
) -> Result<(), ProjectValidationError> {
    let first = events.first().ok_or_else(|| {
        validation(
            ValidationCode::InvalidEvent,
            format!("records.{}.events", record.record_id),
            "record event history is empty",
        )
    })?;
    let (mut stage, mut values) = match &first.kind {
        RecordEventKind::Created {
            initial_stage_id,
            initial_values,
        } if first.sequence == record.created_sequence
            && first.record_revision == 1
            && first.occurred_at == record.created_at
            && spec.stage(initial_stage_id).is_some() =>
        {
            ProjectSnapshot::validate_values(spec, initial_values)?;
            (initial_stage_id.clone(), initial_values.clone())
        }
        _ => {
            return Err(validation(
                ValidationCode::InvalidEvent,
                format!("records.{}.events[0]", record.record_id),
                "first event must create the record at its recorded stage and revision",
            ));
        }
    };

    let mut revision = 1_u64;
    let mut previous_time = first.occurred_at;
    for event in events.iter().skip(1) {
        revision += 1;
        if event.record_revision != revision || event.occurred_at < previous_time {
            return Err(validation(
                ValidationCode::InvalidEvent,
                format!("records.{}.events", record.record_id),
                "record event revisions must increase and timestamps cannot move backwards",
            ));
        }
        previous_time = event.occurred_at;
        match &event.kind {
            RecordEventKind::Created { .. } => {
                return Err(validation(
                    ValidationCode::InvalidEvent,
                    format!("records.{}.events", record.record_id),
                    "a record can have only one creation event",
                ));
            }
            RecordEventKind::FieldsChanged { changes } => {
                if changes.is_empty() || changes.len() > MAX_FIELD_DEFINITIONS {
                    return Err(validation(
                        if changes.len() > MAX_FIELD_DEFINITIONS {
                            ValidationCode::LimitExceeded
                        } else {
                            ValidationCode::InvalidEvent
                        },
                        format!("records.{}.events", record.record_id),
                        "field change events must contain a bounded nonempty list of changes",
                    ));
                }
                let mut changed_ids = HashSet::new();
                for change in changes {
                    if !changed_ids.insert(change.field_id.as_str())
                        || spec.field(&change.field_id).is_none()
                        || change.before == change.after
                        || values.get(&change.field_id) != change.before.as_ref()
                    {
                        return Err(validation(
                            ValidationCode::InvalidEvent,
                            format!("records.{}.events.fields_changed", record.record_id),
                            "field change has a duplicate, unknown, or stale before value",
                        ));
                    }
                    match &change.after {
                        Some(value) => {
                            values.insert(change.field_id.clone(), value.clone());
                        }
                        None => {
                            values.remove(&change.field_id);
                        }
                    }
                }
                ProjectSnapshot::validate_values(spec, &values)?;
            }
            RecordEventKind::StageChanged {
                from_stage_id,
                to_stage_id,
            } => {
                let current = spec.stage(&stage).expect("replayed stage was validated");
                if from_stage_id != &stage
                    || to_stage_id == from_stage_id
                    || current.terminal
                    || !current
                        .allowed_next_stage_ids
                        .iter()
                        .any(|target| target == to_stage_id)
                {
                    return Err(validation(
                        ValidationCode::InvalidTransition,
                        format!("records.{}.events.stage_changed", record.record_id),
                        "history contains a disallowed state transition",
                    ));
                }
                stage = to_stage_id.clone();
            }
        }
    }
    if record.record_revision != revision
        || record.current_stage_id != stage
        || record.typed_values != values
    {
        return Err(validation(
            ValidationCode::InvalidEvent,
            format!("records.{}.history", record.record_id),
            "current record state does not match its immutable event history",
        ));
    }
    Ok(())
}

fn field_accepts_value(field: &FieldDefinition, value: &FieldValue) -> bool {
    match (&field.kind, value) {
        (FieldKind::Text, FieldValue::Text(_))
        | (FieldKind::Date, FieldValue::Date(_))
        | (FieldKind::Integer, FieldValue::Integer(_))
        | (FieldKind::Boolean, FieldValue::Boolean(_)) => true,
        (FieldKind::Enum { options }, FieldValue::Enum(value)) => options.contains(value),
        _ => false,
    }
}

fn validate_spec_revisions<'a>(
    revisions: impl IntoIterator<Item = &'a ToolSpec>,
    active_spec: &SpecReference,
) -> Result<HashMap<(&'a str, u32), &'a ToolSpec>, ProjectValidationError> {
    let mut by_reference = HashMap::new();
    let mut revisions_by_spec: HashMap<&str, Vec<&ToolSpec>> = HashMap::new();
    for spec in revisions {
        spec.validate()?;
        if by_reference
            .insert((spec.spec_id.as_str(), spec.revision), spec)
            .is_some()
        {
            return Err(validation(
                ValidationCode::DuplicateId,
                "spec_revisions",
                "spec revisions must be unique",
            ));
        }
        revisions_by_spec
            .entry(spec.spec_id.as_str())
            .or_default()
            .push(spec);
    }
    for revisions in revisions_by_spec.values_mut() {
        revisions.sort_by_key(|spec| spec.revision);
        for (index, spec) in revisions.iter().enumerate() {
            if spec.revision != index as u32 + 1 {
                return Err(validation(
                    ValidationCode::InvalidSpec,
                    "spec_revisions.revision",
                    "spec revisions must be a complete sequence beginning at revision one",
                ));
            }
        }
        for pair in revisions.windows(2) {
            validate_spec_revision(pair[0], pair[1])?;
        }
    }
    if !by_reference.contains_key(&(active_spec.spec_id.as_str(), active_spec.revision)) {
        return Err(validation(
            ValidationCode::DanglingReference,
            "active_spec",
            "active spec revision does not exist",
        ));
    }
    let newest_active_spec_revision = revisions_by_spec
        .get(active_spec.spec_id.as_str())
        .and_then(|revisions| revisions.last())
        .map(|spec| spec.revision);
    if newest_active_spec_revision != Some(active_spec.revision) {
        return Err(validation(
            ValidationCode::InvalidSpec,
            "active_spec",
            "the active spec must reference the newest stored revision",
        ));
    }
    Ok(by_reference)
}

fn validate_spec_revision(
    previous: &ToolSpec,
    next: &ToolSpec,
) -> Result<(), ProjectValidationError> {
    if previous.entity_type != next.entity_type || previous.template_id != next.template_id {
        return Err(validation(
            ValidationCode::InvalidSpec,
            "spec_revisions",
            "a spec revision cannot change its entity or template identity",
        ));
    }
    for old_field in &previous.fields {
        let Some(new_field) = next.field(&old_field.id) else {
            return Err(validation(
                ValidationCode::InvalidSpec,
                format!("spec_revisions.{}.fields", next.revision),
                "a field cannot be removed from a later spec revision",
            ));
        };
        let kind_is_compatible = match (&old_field.kind, &new_field.kind) {
            (FieldKind::Enum { options: old }, FieldKind::Enum { options: new }) => {
                old.iter().all(|option| new.contains(option))
            }
            (old, new) => old == new,
        };
        if old_field.role != new_field.role || !kind_is_compatible {
            return Err(validation(
                ValidationCode::InvalidFieldRole,
                format!("spec_revisions.{}.fields.{}", next.revision, old_field.id),
                "a later spec revision cannot change a field's type or semantic role, or remove enum options",
            ));
        }
    }
    if next
        .fields
        .iter()
        .any(|new_field| previous.field(&new_field.id).is_none() && new_field.required)
    {
        return Err(validation(
            ValidationCode::InvalidSpec,
            format!("spec_revisions.{}.fields", next.revision),
            "fields added in a later spec revision must be optional",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCommand {
    CreateRecord {
        operation_id: String,
        expected_generation: u64,
        record_id: String,
        initial_stage_id: String,
        values: RecordValues,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    EditRecord {
        operation_id: String,
        expected_generation: u64,
        record_id: String,
        expected_record_revision: u64,
        changes: BTreeMap<String, Option<FieldValue>>,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    TransitionStage {
        operation_id: String,
        expected_generation: u64,
        record_id: String,
        expected_record_revision: u64,
        to_stage_id: String,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
    SetBehavior {
        operation_id: String,
        expected_generation: u64,
        revision_id: String,
        parent_revision_id: Option<String>,
        policy: BehaviorPolicy,
        reason: String,
        #[serde(with = "fixed_datetime_serde")]
        occurred_at: DateTime<FixedOffset>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAccess {
    Writable,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRecordView {
    pub record_id: String,
    pub record_revision: u64,
    pub values: RecordValues,
    pub current_stage_id: String,
    pub derived: EvidenceObservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolViewModel {
    pub project_id: String,
    pub project_name: String,
    pub generation: u64,
    pub access: ProjectAccess,
    pub active_spec: ToolSpec,
    pub active_behavior: BehaviorPolicy,
    pub records: Vec<ToolRecordView>,
    pub selected_record: Option<ToolRecordView>,
    pub decisions: Vec<DecisionRecord>,
    pub scenarios: Vec<Scenario>,
    pub evidence: Vec<Evidence>,
    pub proposals: Vec<ProposalEnvelope>,
    pub access_message: Option<String>,
    pub save_message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationCode {
    InvalidId,
    DuplicateId,
    DuplicateOrInvalidId,
    InvalidProject,
    InvalidSpec,
    InvalidFieldRole,
    InvalidFieldValue,
    InvalidRecord,
    InvalidEvent,
    InvalidTransition,
    InvalidTimestamp,
    InvalidBehavior,
    InvalidScope,
    InvalidScenario,
    InvalidObservation,
    InvalidProposal,
    InvalidOperation,
    MissingRequiredField,
    UnknownField,
    DanglingReference,
    LimitExceeded,
    UnsupportedFeature,
    UnsupportedFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectValidationError {
    pub code: ValidationCode,
    pub path: String,
    pub message: String,
}

impl fmt::Display for ProjectValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl fmt::Display for ValidationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ProjectValidationError {}

fn validation(
    code: ValidationCode,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ProjectValidationError {
    ProjectValidationError {
        code,
        path: path.into(),
        message: message.into(),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LENGTH
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}

fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 16_384
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_layout_fields(
    fields: &[String],
    available: &HashSet<&str>,
    path: &str,
) -> Result<(), ProjectValidationError> {
    let mut seen = HashSet::new();
    for field in fields {
        if !available.contains(field.as_str()) || !seen.insert(field.as_str()) {
            return Err(validation(
                ValidationCode::DanglingReference,
                path,
                "layout fields must uniquely reference fields in the same spec",
            ));
        }
    }
    Ok(())
}

fn validate_extension_keys(
    fields: &ExtensionFields,
    reserved: &[&str],
    path: &str,
) -> Result<(), ProjectValidationError> {
    if fields.keys().any(|key| reserved.contains(&key.as_str())) {
        return Err(validation(
            ValidationCode::InvalidSpec,
            path,
            "extension fields cannot shadow a known field",
        ));
    }
    validate_json_entries(
        fields.iter().map(|(key, value)| (key.as_str(), value)),
        fields.len(),
        path,
    )
}

fn validate_evidence_observations(
    observations: &[EvidenceObservation],
    path: &str,
) -> Result<(), ProjectValidationError> {
    for (index, observation) in observations.iter().enumerate() {
        validate_evidence_observation(observation, &format!("{path}[{index}]"))?;
    }
    Ok(())
}

fn validate_evidence_observations_in_scenarios(
    observations: &[EvidenceObservation],
    scenarios: &[&Scenario],
    path: &str,
) -> Result<(), ProjectValidationError> {
    for (index, observation) in observations.iter().enumerate() {
        let record_specs: Vec<_> = scenarios
            .iter()
            .filter_map(|scenario| scenario_spec_for_record(scenario, &observation.record_id))
            .collect();
        if record_specs.is_empty() {
            return Err(validation(
                ValidationCode::DanglingReference,
                format!("{path}[{index}].record_id"),
                "observation record does not exist in its linked scenario",
            ));
        }
        if !record_specs
            .iter()
            .any(|spec| spec.stage(&observation.current_stage_id).is_some())
        {
            return Err(validation(
                ValidationCode::DanglingReference,
                format!("{path}[{index}].current_stage_id"),
                "observation stage is not defined by the linked record spec",
            ));
        }
    }
    Ok(())
}

fn validate_evidence_observations_in_project<'a>(
    observations: &[EvidenceObservation],
    records: &HashMap<&'a str, &'a WorkRecord>,
    specs: &HashMap<(&'a str, u32), &'a ToolSpec>,
    path: &str,
) -> Result<(), ProjectValidationError> {
    for (index, observation) in observations.iter().enumerate() {
        let record = records.get(observation.record_id.as_str()).ok_or_else(|| {
            validation(
                ValidationCode::DanglingReference,
                format!("{path}[{index}].record_id"),
                "observation record does not exist in its project",
            )
        })?;
        let spec = specs
            .get(&(record.spec.spec_id.as_str(), record.spec.revision))
            .expect("project record spec was validated before its observations");
        if spec.stage(&observation.current_stage_id).is_none() {
            return Err(validation(
                ValidationCode::DanglingReference,
                format!("{path}[{index}].current_stage_id"),
                "observation stage is not defined by the project record spec",
            ));
        }
    }
    Ok(())
}

fn scenario_spec_for_record<'a>(scenario: &'a Scenario, record_id: &str) -> Option<&'a ToolSpec> {
    let record_spec = scenario
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .map(|record| &record.spec)
        .or_else(|| {
            scenario.steps.iter().find_map(|step| match step {
                ScenarioStep::CreateRecord {
                    record_id: created_record_id,
                    ..
                } if created_record_id == record_id => Some(&scenario.active_spec),
                _ => None,
            })
        })?;
    scenario
        .spec_revisions
        .iter()
        .find(|spec| spec.spec_id == record_spec.spec_id && spec.revision == record_spec.revision)
}

fn validate_evidence_observation(
    observation: &EvidenceObservation,
    path: &str,
) -> Result<(), ProjectValidationError> {
    if !valid_id(&observation.record_id)
        || !valid_id(&observation.current_stage_id)
        || observation.elapsed_work_days.is_some_and(|days| days < 0)
        || observation.paused_days < 0
        || matches!(
            &observation.reminder,
            Some(ReminderObservation::FollowUpWhileWaiting { waiting_days, .. }) if *waiting_days < 0
        )
    {
        return Err(validation(
            ValidationCode::InvalidObservation,
            path,
            "observation IDs must be valid and calendar-day counts cannot be negative",
        ));
    }
    Ok(())
}

fn validate_json_entries<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a Value)>,
    entry_count: usize,
    path: &str,
) -> Result<(), ProjectValidationError> {
    if entry_count > MAX_EXTENSION_FIELDS {
        return Err(validation(
            ValidationCode::LimitExceeded,
            path,
            "extension maps exceed the supported field limit",
        ));
    }

    let entries = entries.into_iter().collect::<Vec<_>>();
    let mut stack = Vec::new();
    let mut node_count = 0_usize;
    for (key, value) in entries.iter().copied() {
        if key.len() > MAX_EXTENSION_STRING_BYTES {
            return Err(validation(
                ValidationCode::LimitExceeded,
                path,
                "extension keys exceed the supported string limit",
            ));
        }
        node_count += 1;
        stack.push((value, 1_usize));
    }

    while let Some((value, depth)) = stack.pop() {
        if depth > MAX_EXTENSION_JSON_DEPTH || node_count > MAX_EXTENSION_JSON_NODES {
            return Err(validation(
                ValidationCode::LimitExceeded,
                path,
                "extension values exceed the supported depth or node limit",
            ));
        }
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
            Value::String(value) => {
                if value.len() > MAX_EXTENSION_STRING_BYTES {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        path,
                        "extension strings exceed the supported string limit",
                    ));
                }
            }
            Value::Array(values) => {
                if values.len() > MAX_EXTENSION_JSON_NODES
                    || (depth == MAX_EXTENSION_JSON_DEPTH && !values.is_empty())
                {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        path,
                        "extension arrays exceed the supported node or depth limit",
                    ));
                }
                node_count = node_count.saturating_add(values.len());
                if node_count > MAX_EXTENSION_JSON_NODES {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        path,
                        "extension values exceed the supported node limit",
                    ));
                }
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                if values.len() > MAX_EXTENSION_JSON_NODES
                    || (depth == MAX_EXTENSION_JSON_DEPTH && !values.is_empty())
                {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        path,
                        "extension objects exceed the supported node or depth limit",
                    ));
                }
                node_count = node_count.saturating_add(values.len());
                if node_count > MAX_EXTENSION_JSON_NODES {
                    return Err(validation(
                        ValidationCode::LimitExceeded,
                        path,
                        "extension values exceed the supported node limit",
                    ));
                }
                for (key, value) in values {
                    if key.len() > MAX_EXTENSION_STRING_BYTES {
                        return Err(validation(
                            ValidationCode::LimitExceeded,
                            path,
                            "extension keys exceed the supported string limit",
                        ));
                    }
                    stack.push((value, depth + 1));
                }
            }
        }
    }

    let mut writer = BoundedJsonWriter { written: 0 };
    writer
        .write_all(b"{")
        .map_err(|_| extension_size_error(path))?;
    for (index, &(key, value)) in entries.iter().enumerate() {
        if index > 0 {
            writer
                .write_all(b",")
                .map_err(|_| extension_size_error(path))?;
        }
        serde_json::to_writer(&mut writer, key).map_err(|_| extension_size_error(path))?;
        writer
            .write_all(b":")
            .map_err(|_| extension_size_error(path))?;
        serde_json::to_writer(&mut writer, value).map_err(|_| extension_size_error(path))?;
    }
    writer
        .write_all(b"}")
        .map_err(|_| extension_size_error(path))?;
    Ok(())
}

fn extension_size_error(path: &str) -> ProjectValidationError {
    validation(
        ValidationCode::LimitExceeded,
        path,
        "extension values exceed the supported encoded size limit",
    )
}

struct BoundedJsonWriter {
    written: usize,
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_EXTENSION_JSON_BYTES.saturating_sub(self.written) {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "extension JSON size limit exceeded",
            ));
        }
        self.written += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn unique_ids<'a>(
    values: impl Iterator<Item = &'a str>,
    path: &str,
) -> Result<HashSet<&'a str>, ProjectValidationError> {
    let mut result = HashSet::new();
    for value in values {
        if !valid_id(value) || !result.insert(value) {
            return Err(validation(
                ValidationCode::DuplicateOrInvalidId,
                path,
                "IDs must be valid and unique",
            ));
        }
    }
    Ok(result)
}

const SUPPORTED_REQUIRED_FEATURES: &[&str] = &[];

pub fn studio_order_template() -> ToolSpec {
    ToolSpec {
        spec_id: "studio-order".to_owned(),
        revision: 1,
        template_id: Some("studio-order-v1".to_owned()),
        entity_type: "order".to_owned(),
        display_name: "工作室订单进度".to_owned(),
        fields: vec![
            FieldDefinition {
                id: "order_number".to_owned(),
                display_name: "订单编号或标题".to_owned(),
                kind: FieldKind::Text,
                role: FieldRole::Title,
                required: true,
            },
            FieldDefinition {
                id: "work_description".to_owned(),
                display_name: "工作内容".to_owned(),
                kind: FieldKind::Text,
                role: FieldRole::WorkDescription,
                required: true,
            },
            FieldDefinition {
                id: "started_on".to_owned(),
                display_name: "开始制作日期".to_owned(),
                kind: FieldKind::Date,
                role: FieldRole::WorkStartedOn,
                required: false,
            },
            FieldDefinition {
                id: "promised_on".to_owned(),
                display_name: "原承诺交付日期".to_owned(),
                kind: FieldKind::Date,
                role: FieldRole::PromisedDate,
                required: false,
            },
            FieldDefinition {
                id: "notes".to_owned(),
                display_name: "备注".to_owned(),
                kind: FieldKind::Text,
                role: FieldRole::Notes,
                required: false,
            },
        ],
        stages: vec![
            StageDefinition {
                id: "queued".to_owned(),
                display_name: "待制作".to_owned(),
                clock: StageClock::Stopped,
                terminal: false,
                allowed_next_stage_ids: vec!["in_progress".to_owned(), "cancelled".to_owned()],
            },
            StageDefinition {
                id: "in_progress".to_owned(),
                display_name: "制作中".to_owned(),
                clock: StageClock::Active,
                terminal: false,
                allowed_next_stage_ids: vec![
                    "waiting_materials".to_owned(),
                    "completed".to_owned(),
                    "cancelled".to_owned(),
                ],
            },
            StageDefinition {
                id: "waiting_materials".to_owned(),
                display_name: "等材料".to_owned(),
                clock: StageClock::Paused,
                terminal: false,
                allowed_next_stage_ids: vec!["in_progress".to_owned(), "cancelled".to_owned()],
            },
            StageDefinition {
                id: "completed".to_owned(),
                display_name: "已完成".to_owned(),
                clock: StageClock::Stopped,
                terminal: true,
                allowed_next_stage_ids: Vec::new(),
            },
            StageDefinition {
                id: "cancelled".to_owned(),
                display_name: "已取消".to_owned(),
                clock: StageClock::Stopped,
                terminal: true,
                allowed_next_stage_ids: Vec::new(),
            },
        ],
        default_stage_id: "queued".to_owned(),
        list_field_ids: vec![
            "order_number".to_owned(),
            "work_description".to_owned(),
            "promised_on".to_owned(),
        ],
        detail_field_ids: vec![
            "order_number".to_owned(),
            "work_description".to_owned(),
            "started_on".to_owned(),
            "promised_on".to_owned(),
            "notes".to_owned(),
        ],
        extensions: BTreeMap::new(),
    }
}
