use super::*;

/// Add schema definitions, never remove or reinterpret a recorded field. Active
/// uniqueness/constraints may change; actual event values are never recomputed.
pub(super) fn merge(program: &CapturedProgram, current: &DataSnapshot) -> Result<DataSnapshot> {
    let mut next = current.clone();
    for entity in &program.program.entities {
        if let Some(stored) = next.schema.iter_mut().find(|e| e.id == entity.id) {
            for field in &entity.fields {
                if let Some(old) = stored.fields.iter().find(|f| f.id == field.id) {
                    if old.value_type != field.value_type {
                        return Err(invalid(&format!("Field {}.{} changes its recorded type; a compatible forward repair is required",entity.id,field.id)));
                    }
                } else {
                    stored.fields.push(field.clone());
                }
            }
            stored.label = entity.label.clone();
            stored.unique = entity.unique.clone();
            stored.constraints = entity.constraints.clone();
        } else {
            next.schema.push(entity.clone());
        }
    }
    next.validate()?;
    Ok(next)
}
pub(super) fn check(
    program: &CapturedProgram,
    current: &DataSnapshot,
    day: Option<i32>,
) -> Result<CompatibilityReport> {
    program.validate()?;
    current.validate()?;
    if program.binding.project_id != current.project_id {
        return Err(invalid("compatibility project mismatch"));
    }
    let mut report = CompatibilityReport {
        state: CompatibilityState::Compatible,
        current_data: current.identity()?,
        target_program: program.artifact.program_digest.clone(),
        retained_records: current
            .records
            .iter()
            .map(|r| RecordRef {
                entity: r.entity.clone(),
                record: r.id.clone(),
            })
            .collect(),
        retained_events: current.events.iter().map(|e| e.id.clone()).collect(),
        retained_fields: current
            .schema
            .iter()
            .flat_map(|e| {
                e.fields.iter().map(|f| SemanticKey {
                    kind: SemanticKind::Field,
                    entity: Some(e.id.clone()),
                    id: f.id.clone(),
                })
            })
            .collect(),
        issues: vec![],
    };
    for stored in &current.schema {
        if let Some(target) = program.program.entities.iter().find(|e| e.id == stored.id) {
            for field in &stored.fields {
                if !target.fields.iter().any(|f| f.id == field.id)
                    && !matches!(field.value_type, Type::Optional { .. })
                {
                    report.issues.push(format!("Required stored field {}.{} is missing from the target; keep it in a compatible forward repair",stored.id,field.id));
                }
            }
        } else if current.records.iter().any(|r| r.entity == stored.id) {
            report.state = CompatibilityState::RetainedButUnavailable;
            report.issues.push(format!("Entity {} remains inspectable but the target has no workflow for its current records",stored.id));
        }
    }
    if let Ok(data) = merge(program, current) {
        let state = State {
            data,
            session: SessionState::initial(&program.program)?,
            day: day.unwrap_or(0),
            outputs: vec![],
        };
        let limits = RuntimeLimits {
            fuel: 10_000_000,
            ..RuntimeLimits::default()
        };
        let mut meter = Meter::new(
            limits.clone(),
            limits.fuel,
            Arc::new(AtomicBool::new(false)),
        );
        // Clock-sensitive constraints cannot be safely checked at an invented
        // time here. They are checked with the actual operation clock by start,
        // adoption rehearsal and every transaction.
        for entity in &program.program.entities {
            if day.is_none() && entity.constraints.iter().any(uses_today) {
                report.state = CompatibilityState::Unknown;
                report.issues.push(format!("Entity {} has clock-sensitive constraints; use compatibility_at with the current operation day",entity.id));
            }
        }
        if report.state != CompatibilityState::Unknown {
            if let Err(error) = execute::constraints(program, &state, &mut meter) {
                report.issues.push(format!(
                    "Current records violate target constraints: {error:?}"
                ));
                report.state = CompatibilityState::Incompatible;
            }
        }
    } else if let Err(error) = merge(program, current) {
        report.issues.push(format!(
            "Current schema/data cannot be represented safely: {error:?}"
        ));
        report.state = CompatibilityState::Incompatible;
    }
    if report.state == CompatibilityState::Compatible && !report.issues.is_empty() {
        report.state = CompatibilityState::Incompatible;
    }
    report.issues.truncate(MAX_ITEMS);
    report.validate()?;
    Ok(report)
}
fn uses_today(expr: &Expr) -> bool {
    // The serialized discriminant search is structural (not domain text); a
    // literal containing "today" cannot match this recursively inspected AST.
    fn visit(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(m) => {
                m.get("kind").and_then(|v| v.as_str()) == Some("today") || m.values().any(visit)
            }
            serde_json::Value::Array(a) => a.iter().any(visit),
            _ => false,
        }
    }
    serde_json::to_value(expr)
        .map(|v| visit(&v))
        .unwrap_or(true)
}
