use crate::tool_project::*;
use crate::tool_workspace_protocol::*;
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::collections::BTreeMap;
pub fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-10-05T09:00:00+08:00").unwrap()
}
pub fn date(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}
pub fn observation(id: &str, due_day: u32) -> EvidenceObservation {
    EvidenceObservation {
        record_id: id.into(),
        current_stage_id: "waiting_materials".into(),
        elapsed_work_days: Some(1),
        paused_days: 3,
        original_due_date: Some(date(6)),
        display_due_date: Some(date(due_day)),
        reminder: Some(ReminderObservation::FollowUpWhileWaiting {
            waiting_days: 3,
            days_until_due: Some(i64::from(due_day) - 5),
        }),
    }
}
pub fn model() -> ToolViewModel {
    let records = ["order-1", "order-2"]
        .iter()
        .enumerate()
        .map(|(i, id)| ToolRecordView {
            record_id: (*id).into(),
            record_revision: 3,
            values: BTreeMap::from([
                (
                    "order_number".into(),
                    FieldValue::Text(if i == 0 { "第一笔" } else { "第二笔" }.into()),
                ),
                (
                    "work_description".into(),
                    FieldValue::Text("手工木盒".into()),
                ),
            ]),
            current_stage_id: "waiting_materials".into(),
            derived: observation(id, 6),
        })
        .collect();
    ToolViewModel {
        project_id: "project-1".into(),
        project_name: "我的订单".into(),
        generation: 4,
        access: ProjectAccess::Writable,
        active_spec: studio_order_template(),
        active_behavior: BehaviorPolicy::default(),
        records,
        selected_record: None,
        decisions: vec![],
        scenarios: vec![],
        evidence: vec![],
        proposals: vec![],
        access_message: None,
        save_message: None,
    }
}
pub fn entry_view<'a>() -> WorkspaceView<'a> {
    WorkspaceView {
        session_id: "session-1",
        next_operation_id: "operation-1",
        project: None,
        now: now(),
        as_of_date: date(5),
        location: None,
        is_example: false,
        decision_requests: decision_requests(),
        record_policies: policies(),
        record_history: &[],
        operation: None,
        rehearsal: None,
        withdrawal: None,
    }
}
pub fn project_view(model: &ToolViewModel) -> WorkspaceView<'_> {
    WorkspaceView {
        project: Some(model),
        location: Some("/home/me/orders"),
        ..entry_view()
    }
}
pub fn rehearsal(context: RequestContext) -> RehearsalView {
    RehearsalView {
        context,
        preview_id: "preview-1".into(),
        fresh: true,
        scope: ScopeSelection {
            kind: ScopeKind::SingleRecord,
            confirmed_generation: 4,
            frozen_record_ids: vec!["order-1".into()],
            applies_to_future_records: false,
            effective_sequence: 5,
        },
        candidates: [6, 9]
            .into_iter()
            .enumerate()
            .map(|(i, day)| CandidateView {
                candidate_id: if i == 0 { "a" } else { "b" }.into(),
                label: if i == 0 { "方案 A" } else { "方案 B" }.into(),
                policy: BehaviorPolicy {
                    due_date: if i == 0 {
                        DateDuePolicy::KeepOriginal
                    } else {
                        DateDuePolicy::ExtendByPausedDays
                    },
                    ..BehaviorPolicy::default()
                },
                comparisons: vec![ObservationComparison {
                    before: observation("order-1", 6),
                    after: observation("order-1", day),
                }],
                evidence: vec![Evidence {
                    run_id: format!("run-{i}"),
                    scenario_id: format!("scenario-{i}"),
                    status: EvidenceStatus::Passed,
                    observations: vec![observation("order-1", day)],
                    expected: vec![],
                    input_fingerprint: "a".repeat(64),
                    candidate_fingerprint: "b".repeat(64),
                    runtime_semantics_version: RUNTIME_SEMANTICS_VERSION,
                    source: EvidenceSource::NativeExecution,
                    errors: vec![],
                    completed_at: now(),
                    extensions: BTreeMap::new(),
                }],
                conflicts: vec![],
                ready_to_adopt: true,
            })
            .collect(),
    }
}
pub fn decision() -> DecisionRecord {
    DecisionRecord {
        decision_id: "decision-1".into(),
        intent_revision: 1,
        choice: DecisionChoice::Adopt,
        expected_outcomes: vec![],
        scope: None,
        rationale: "等待不改变原始承诺".into(),
        unresolved_questions: vec![],
        scenario_ids: vec![],
        evidence_ids: vec![],
        supersedes: vec![],
        status: DecisionStatus::Active,
        created_at: now(),
        extensions: BTreeMap::new(),
    }
}

fn policies() -> &'static BTreeMap<String, BehaviorPolicy> {
    static POLICIES: std::sync::OnceLock<BTreeMap<String, BehaviorPolicy>> =
        std::sync::OnceLock::new();
    POLICIES.get_or_init(|| {
        BTreeMap::from([
            ("order-1".into(), BehaviorPolicy::default()),
            ("order-2".into(), BehaviorPolicy::default()),
        ])
    })
}

fn decision_requests() -> &'static BTreeMap<String, String> {
    static REQUESTS: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    REQUESTS.get_or_init(BTreeMap::new)
}
