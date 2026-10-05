use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use std::collections::BTreeMap;

use crate::tool_project::{
    studio_order_template, BehaviorPolicy, DateDuePolicy, FieldKind, FieldRole, FieldValue,
    ProjectDateSettings, ProjectSnapshot, ReminderPolicy, StageClock, TimerPolicy, ToolCommand,
    ToolSpec,
};
use crate::tool_runtime::apply_command;

pub fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

pub fn at(year: i32, month: u32, day: u32, hour: u32) -> DateTime<FixedOffset> {
    FixedOffset::east_opt(8 * 60 * 60)
        .unwrap()
        .with_ymd_and_hms(year, month, day, hour, 0, 0)
        .single()
        .unwrap()
}

pub fn standard_behavior() -> BehaviorPolicy {
    BehaviorPolicy {
        timer: TimerPolicy::PauseStagesMarkedPaused,
        due_date: DateDuePolicy::KeepOriginal,
        reminder: ReminderPolicy::WaitingBeforeDue { days_before_due: 1 },
    }
}

pub fn maintenance_task_spec() -> ToolSpec {
    use crate::tool_project::{FieldDefinition, StageDefinition};

    ToolSpec {
        spec_id: "repair-task".to_owned(),
        revision: 1,
        template_id: None,
        entity_type: "repair_task".to_owned(),
        display_name: "个人维修任务".to_owned(),
        fields: vec![
            FieldDefinition {
                id: "repair_title".to_owned(),
                display_name: "任务".to_owned(),
                kind: FieldKind::Text,
                role: FieldRole::Title,
                required: true,
            },
            FieldDefinition {
                id: "opened_on".to_owned(),
                display_name: "开始日期".to_owned(),
                kind: FieldKind::Date,
                role: FieldRole::WorkStartedOn,
                required: false,
            },
            FieldDefinition {
                id: "target_date".to_owned(),
                display_name: "目标日期".to_owned(),
                kind: FieldKind::Date,
                role: FieldRole::PromisedDate,
                required: false,
            },
            FieldDefinition {
                id: "repair_notes".to_owned(),
                display_name: "备注".to_owned(),
                kind: FieldKind::Text,
                role: FieldRole::Notes,
                required: false,
            },
            FieldDefinition {
                id: "repair_kind".to_owned(),
                display_name: "类型".to_owned(),
                kind: FieldKind::Enum {
                    options: vec!["bike".to_owned(), "home".to_owned()],
                },
                role: FieldRole::Custom,
                required: false,
            },
        ],
        stages: vec![
            StageDefinition {
                id: "open".to_owned(),
                display_name: "待处理".to_owned(),
                clock: StageClock::Stopped,
                terminal: false,
                allowed_next_stage_ids: vec!["repairing".to_owned(), "cancelled".to_owned()],
            },
            StageDefinition {
                id: "repairing".to_owned(),
                display_name: "处理中".to_owned(),
                clock: StageClock::Active,
                terminal: false,
                allowed_next_stage_ids: vec![
                    "waiting_parts".to_owned(),
                    "done".to_owned(),
                    "cancelled".to_owned(),
                ],
            },
            StageDefinition {
                id: "waiting_parts".to_owned(),
                display_name: "等配件".to_owned(),
                clock: StageClock::Paused,
                terminal: false,
                allowed_next_stage_ids: vec!["repairing".to_owned(), "cancelled".to_owned()],
            },
            StageDefinition {
                id: "done".to_owned(),
                display_name: "完成".to_owned(),
                clock: StageClock::Stopped,
                terminal: true,
                allowed_next_stage_ids: vec![],
            },
            StageDefinition {
                id: "cancelled".to_owned(),
                display_name: "取消".to_owned(),
                clock: StageClock::Stopped,
                terminal: true,
                allowed_next_stage_ids: vec![],
            },
        ],
        default_stage_id: "open".to_owned(),
        list_field_ids: vec!["repair_title".to_owned(), "target_date".to_owned()],
        detail_field_ids: vec![
            "repair_title".to_owned(),
            "repair_kind".to_owned(),
            "opened_on".to_owned(),
            "target_date".to_owned(),
            "repair_notes".to_owned(),
        ],
        extensions: BTreeMap::new(),
    }
}

pub fn empty_project(spec: ToolSpec, project_id: &str) -> ProjectSnapshot {
    ProjectSnapshot::new(
        project_id.to_owned(),
        "U01 fixture".to_owned(),
        at(2026, 10, 1, 8),
        ProjectDateSettings {
            calendar_utc_offset_seconds: 8 * 60 * 60,
        },
        spec,
        standard_behavior(),
    )
    .unwrap()
}

pub fn active_waiting_project(spec: ToolSpec, project_id: &str) -> ProjectSnapshot {
    let mut snapshot = empty_project(spec.clone(), project_id);
    let mut values = BTreeMap::new();
    for field in &spec.fields {
        let value = match field.role {
            FieldRole::Title => Some(FieldValue::Text("示例工作".to_owned())),
            FieldRole::WorkDescription => Some(FieldValue::Text("完成一项虚构工作".to_owned())),
            FieldRole::WorkStartedOn => Some(FieldValue::Date(date(2026, 10, 1))),
            FieldRole::PromisedDate => Some(FieldValue::Date(date(2026, 10, 6))),
            FieldRole::Notes => Some(FieldValue::Text(String::new())),
            FieldRole::Custom => match &field.kind {
                FieldKind::Text => Some(FieldValue::Text("fixture".to_owned())),
                FieldKind::Date => None,
                FieldKind::Integer => Some(FieldValue::Integer(1)),
                FieldKind::Boolean => Some(FieldValue::Boolean(false)),
                FieldKind::Enum { options } => options.first().cloned().map(FieldValue::Enum),
            },
        };
        if let Some(value) = value {
            values.insert(field.id.clone(), value);
        }
    }
    let active_stage = spec
        .stages
        .iter()
        .find(|stage| stage.clock == StageClock::Active)
        .unwrap()
        .id
        .clone();
    let waiting_stage = spec
        .stages
        .iter()
        .find(|stage| stage.clock == StageClock::Paused)
        .unwrap()
        .id
        .clone();
    snapshot = apply_command(
        &snapshot,
        &ToolCommand::CreateRecord {
            operation_id: "create-record-1".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            initial_stage_id: active_stage,
            values,
            occurred_at: at(2026, 10, 1, 9),
        },
        at(2026, 10, 1, 9),
    )
    .unwrap();
    snapshot = apply_command(
        &snapshot,
        &ToolCommand::TransitionStage {
            operation_id: "wait-1".to_owned(),
            expected_generation: 0,
            record_id: "record-1".to_owned(),
            expected_record_revision: 1,
            to_stage_id: waiting_stage,
            occurred_at: at(2026, 10, 2, 9),
        },
        at(2026, 10, 2, 9),
    )
    .unwrap();
    snapshot
}

pub fn studio_order_project(project_id: &str) -> ProjectSnapshot {
    active_waiting_project(studio_order_template(), project_id)
}
