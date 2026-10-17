//! Private source-bound coverage for an executed action that cannot affect a
//! frozen selection. This is not applicable business-outcome evidence.
use super::common_fact_admission::ProofMeter;
use super::*;

/// Authority is derived from the replay registry, never serialized in evidence.
pub(crate) enum ScopeApplicability {
    Ordinary,
    OwnedRoot(Id),
    UnverifiedOwned,
}
impl ScopeApplicability {
    pub(crate) fn matches(&self, scope: &DecisionScope, context: &ScopeContext) -> ScopeMatch {
        if scope.validate().is_err() {
            return ScopeMatch::Unknown;
        }
        if !scope.operations.contains(&context.operation) {
            return ScopeMatch::Outside;
        }
        match self {
            Self::Ordinary => scope.matches(context),
            Self::UnverifiedOwned => ScopeMatch::Unknown,
            Self::OwnedRoot(root) => match &context.record {
                None => ScopeMatch::Unknown,
                Some(record) if &record.entity != root => ScopeMatch::Outside,
                Some(_) => scope.matches(context),
            },
        }
    }
}
pub(super) fn expected_scope(prepared: &PreparedOwnedPreview) -> DecisionScope {
    let request = &prepared.proof.request;
    DecisionScope {
        operations: request.operations.clone(),
        population: match &request.population {
            ScopePopulation::All => Population::All,
            ScopePopulation::FutureWork => Population::CreatedAfter {
                generation: prepared.proof.snapshot.data.generation,
            },
            ScopePopulation::SelectedUnfinished { records } => Population::Records {
                records: records.clone(),
            },
        },
        conditions: Values::new(),
        excluded_records: request.excluded_records.clone(),
        unknowns: vec![],
    }
}

#[derive(Default)]
pub(crate) struct OwnedBoundaryCoverage {
    operations: BTreeSet<Id>,
}
impl OwnedBoundaryCoverage {
    pub(crate) fn operations(&self) -> &BTreeSet<Id> {
        &self.operations
    }
}
fn primitive(ty: &Type) -> bool {
    matches!(ty, Type::Boolean | Type::Integer | Type::Text | Type::Date)
}
fn local(expr: &Expr, parameters: &BTreeMap<Id, Type>, meter: &mut ProofMeter) -> Result<bool> {
    meter.charge(1)?;
    Ok(match expr {
        Expr::Literal { value_type, value } => {
            primitive(value_type) && validate_value(value, value_type, 0).is_ok()
        }
        Expr::Variable { name } => parameters.get(name).is_some_and(primitive),
        Expr::Today => true,
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            local(left, parameters, meter)? && local(right, parameters, meter)?
        }
        Expr::Lower { value } | Expr::Not { value } => local(value, parameters, meter)?,
        Expr::TextContains { text, search } => {
            local(text, parameters, meter)? && local(search, parameters, meter)?
        }
        Expr::DateAdd { date, days } => {
            local(date, parameters, meter)? && local(days, parameters, meter)?
        }
        Expr::DateDifference { later, earlier } => {
            local(later, parameters, meter)? && local(earlier, parameters, meter)?
        }
        Expr::And { values } | Expr::Or { values } => {
            for value in values {
                if !local(value, parameters, meter)? {
                    return Ok(false);
                }
            }
            true
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            local(condition, parameters, meter)?
                && local(then_value, parameters, meter)?
                && local(else_value, parameters, meter)?
        }
        _ => false,
    })
}
fn pure_creation(action: &ActionDefinition, root: &str, meter: &mut ProofMeter) -> Result<bool> {
    meter.charge(canonical_bytes(action)?.len() as u64)?;
    if action.steps.is_empty()
        || !action.ensures.is_empty()
        || !action.parameters.values().all(primitive)
    {
        return Ok(false);
    }
    for guard in &action.guards {
        if !local(guard, &action.parameters, meter)? {
            return Ok(false);
        }
    }
    let mut bindings = BTreeSet::new();
    for step in &action.steps {
        let Statement::Create {
            entity,
            bind,
            values,
        } = step
        else {
            return Ok(false);
        };
        if entity != root || action.parameters.contains_key(bind) || !bindings.insert(bind) {
            return Ok(false);
        }
        for value in values.values() {
            if !local(value, &action.parameters, meter)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn outside_creation(scope: &DecisionScope, root: &str, generation: u64, c: &ScopeContext) -> bool {
    c.record.as_ref().is_some_and(|r| r.entity == root)
        && c.is_new == Some(true)
        && c.created_generation.is_some_and(|g| g > generation)
        && scope.matches(c) == ScopeMatch::Outside
}
pub(super) fn derive(
    prepared: &PreparedOwnedPreview,
    scope: &DecisionScope,
    contexts: &[ScopeContext],
    meter: &mut ProofMeter,
) -> Result<OwnedBoundaryCoverage> {
    let proof = &prepared.proof;
    let request = &proof.request;
    if !matches!(
        request.population,
        ScopePopulation::SelectedUnfinished { .. }
    ) || scope != &expected_scope(prepared)
    {
        return Ok(OwnedBoundaryCoverage::default());
    }
    let before = &proof.snapshot.program()?.program;
    let after = &proof.candidate.program;
    let root = &request.inventory.sources.root_entity;
    let mut result = OwnedBoundaryCoverage::default();
    for action in &before.actions {
        meter.charge(1)?;
        if !scope.operations.contains(&action.id)
            || after.actions.iter().find(|a| a.id == action.id) != Some(action)
            || request
                .inventory
                .sources
                .edits
                .iter()
                .any(|edit| edit.before.action == action.id || edit.after.action == action.id)
            || !pure_creation(action, root, meter)?
        {
            continue;
        }
        let mut observed = false;
        let mut outside = true;
        for context in contexts {
            meter.charge(1)?;
            if context.operation == action.id {
                observed = true;
                outside &= outside_creation(scope, root, proof.snapshot.data.generation, context);
            }
        }
        if observed && outside {
            result.operations.insert(action.id.clone());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};
    fn meter() -> ProofMeter {
        ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap()
    }
    fn action() -> ActionDefinition {
        ActionDefinition {
            id: "create".into(),
            label: "Create".into(),
            parameters: [("title".into(), Type::Text)].into(),
            guards: vec![],
            ensures: vec![],
            steps: vec![Statement::Create {
                entity: "root".into(),
                bind: "made".into(),
                values: [(
                    "title".into(),
                    Expr::Variable {
                        name: "title".into(),
                    },
                )]
                .into(),
            }],
        }
    }
    #[test]
    fn owned_root_applicability_preserves_unknown_and_ordinary_semantics() {
        let mut scope = DecisionScope {
            operations: ["create".into()].into(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        };
        let mut context = ScopeContext {
            operation: "create".into(),
            record: None,
            is_new: Some(true),
            created_generation: Some(11),
            attributes: Values::new(),
            predicate_result: None,
        };
        let owned = ScopeApplicability::OwnedRoot("root".into());
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Unknown);
        assert_eq!(
            ScopeApplicability::Ordinary.matches(&scope, &context),
            ScopeMatch::Applies
        );
        context.record = Some(RecordRef {
            entity: "relation".into(),
            record: "new".into(),
        });
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Outside);
        scope.population = Population::CreatedAfter { generation: 10 };
        assert_eq!(
            ScopeApplicability::Ordinary.matches(&scope, &context),
            ScopeMatch::Applies
        );
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Outside);
        context.record.as_mut().unwrap().entity = "root".into();
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Applies);
        context.created_generation = None;
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Unknown);
        scope.population = Population::All;
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Applies);
        assert_eq!(
            ScopeApplicability::UnverifiedOwned.matches(&scope, &context),
            ScopeMatch::Unknown
        );
        context.operation = "undeclared".into();
        assert_eq!(owned.matches(&scope, &context), ScopeMatch::Outside);
        assert_eq!(
            ScopeApplicability::UnverifiedOwned.matches(&scope, &context),
            ScopeMatch::Outside
        );
    }
    #[test]
    fn creation_boundary_rejects_nonlocal_and_hidden_reference_expressions() {
        let action = action();
        assert!(pure_creation(&action, "root", &mut meter()).unwrap());
        for expression in [
            Expr::State {
                state: "entry".into(),
            },
            Expr::Field {
                record: Box::new(Expr::Variable {
                    name: "made".into(),
                }),
                field: "title".into(),
            },
            Expr::Literal {
                value_type: Type::reference("root"),
                value: DataValue::Reference {
                    entity: "root".into(),
                    record: "existing".into(),
                },
            },
            Expr::Literal {
                value_type: Type::Optional {
                    item: Box::new(Type::reference("root")),
                },
                value: DataValue::Null,
            },
            Expr::Literal {
                value_type: Type::list(Type::reference("root")),
                value: DataValue::List {
                    item_type: Type::reference("root"),
                    items: vec![],
                },
            },
        ] {
            let mut changed = action.clone();
            let Statement::Create { values, .. } = &mut changed.steps[0] else {
                unreachable!()
            };
            values.insert("title".into(), expression);
            assert!(!pure_creation(&changed, "root", &mut meter()).unwrap());
        }
        let mut changed = action;
        changed.parameters.insert(
            "hidden".into(),
            Type::Optional {
                item: Box::new(Type::reference("root")),
            },
        );
        assert!(!pure_creation(&changed, "root", &mut meter()).unwrap());
    }
    #[test]
    fn creation_boundary_rejects_nonroot_writes_and_extra_effects() {
        for extra in [
            Statement::SetState {
                state: "entry".into(),
                value: Expr::Variable {
                    name: "title".into(),
                },
            },
            Statement::Create {
                entity: "auxiliary".into(),
                bind: "other".into(),
                values: BTreeMap::new(),
            },
            Statement::Update {
                record: Expr::Variable {
                    name: "made".into(),
                },
                values: [(
                    "title".into(),
                    Expr::Variable {
                        name: "title".into(),
                    },
                )]
                .into(),
            },
        ] {
            let mut changed = action();
            changed.steps.push(extra);
            assert!(!pure_creation(&changed, "root", &mut meter()).unwrap());
        }
        let mut changed = action();
        changed.guards.push(Expr::State {
            state: "gate".into(),
        });
        assert!(!pure_creation(&changed, "root", &mut meter()).unwrap());
    }
    #[test]
    fn creation_boundary_requires_actual_new_root_birth_provenance() {
        let scope = DecisionScope {
            operations: ["create".into()].into(),
            population: Population::Records {
                records: vec![RecordRef {
                    entity: "root".into(),
                    record: "existing".into(),
                }],
            },
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        };
        let context = ScopeContext {
            operation: "create".into(),
            record: Some(RecordRef {
                entity: "root".into(),
                record: "new".into(),
            }),
            is_new: Some(true),
            created_generation: Some(11),
            attributes: Values::new(),
            predicate_result: None,
        };
        assert!(outside_creation(&scope, "root", 10, &context));
        let mut cases = vec![];
        let mut c = context.clone();
        c.record = None;
        cases.push(c);
        let mut c = context.clone();
        c.record.as_mut().unwrap().entity = "event".into();
        cases.push(c);
        let mut c = context.clone();
        c.record.as_mut().unwrap().record = "existing".into();
        cases.push(c);
        let mut c = context.clone();
        c.is_new = None;
        cases.push(c);
        let mut c = context.clone();
        c.is_new = Some(false);
        cases.push(c);
        let mut c = context.clone();
        c.created_generation = None;
        cases.push(c);
        let mut c = context;
        c.created_generation = Some(10);
        cases.push(c);
        for c in cases {
            assert!(!outside_creation(&scope, "root", 10, &c));
        }
    }
    #[test]
    fn creation_boundary_source_work_keeps_its_meter_limits() {
        let mut bounded = ProofMeter::new(
            ProofBudget {
                units: 2,
                elapsed_millis: 60_000,
            },
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(pure_creation(&action(), "root", &mut bounded).is_err());
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut meter = ProofMeter::new(ProofBudget::default(), cancelled.clone()).unwrap();
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(pure_creation(&action(), "root", &mut meter).is_err());
    }
}
