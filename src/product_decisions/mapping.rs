use super::*;
/// An exercised mapping applies to this accepted implementation, never to every
/// later program that happens to reuse one of its semantic IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImplementationMapping {
    pub source_program: Digest,
    pub mappings: Vec<SemanticMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenarios: Vec<ScenarioMapping>,
}

/// A validated rename bridge, never a provider's assertion of equivalence.
/// Entity/field migrations are deliberately unsupported; current durable data
/// may not be rewritten by a semantic-input mapping.
pub(super) struct Mapping {
    mappings: Vec<SemanticMapping>,
    source_views: Vec<ViewDefinition>,
    target_views: Vec<ViewDefinition>,
}
impl Mapping {
    pub fn new(
        source: &AppDefinition,
        target: &AppDefinition,
        mappings: &[SemanticMapping],
    ) -> Result<Self> {
        let mut from = BTreeSet::new();
        let mut to = BTreeSet::new();
        for m in mappings {
            if m.from.kind != m.to.kind
                || !m.from.exists_in(source)
                || !m.to.exists_in(target)
                || !from.insert(canonical_bytes(&m.from)?)
                || !to.insert(canonical_bytes(&m.to)?)
            {
                return Err(invalid(
                    "semantic mapping is missing, ambiguous or changes kind",
                ));
            }
            if matches!(m.from.kind, SemanticKind::Entity | SemanticKind::Field) && m.from != m.to {
                return Err(DecisionError::Unverified(
                    "Entity/field migration needs a separately verified data mapping".into(),
                ));
            }
        }
        let mapping = Self {
            mappings: mappings.to_vec(),
            source_views: source.views.clone(),
            target_views: target.views.clone(),
        };
        // An explicit rename must not merge with an unchanged identity. Check
        // implicit destinations too, for active checks, pending projections
        // and adoption alike, even when two observed values happen to match.
        for (kind, ids) in [
            (
                SemanticKind::Observable,
                source
                    .observables
                    .iter()
                    .map(|value| &value.id)
                    .collect::<Vec<_>>(),
            ),
            (
                SemanticKind::Output,
                source.outputs.iter().map(|value| &value.id).collect(),
            ),
            (
                SemanticKind::View,
                source.views.iter().map(|value| &value.id).collect(),
            ),
            (
                SemanticKind::Action,
                source.actions.iter().map(|value| &value.id).collect(),
            ),
        ] {
            let mut projected = BTreeSet::new();
            if ids
                .into_iter()
                .any(|id| !projected.insert(mapping.id(kind, id)))
            {
                return Err(DecisionError::Unverified(
                    "Retained semantic mapping collides with an unchanged identity".into(),
                ));
            }
        }
        Ok(mapping)
    }
    pub fn id(&self, kind: SemanticKind, id: &str) -> Id {
        self.mappings
            .iter()
            .find(|m| m.from.kind == kind && m.from.id == id)
            .map(|m| m.to.id.clone())
            .unwrap_or_else(|| id.into())
    }
    /// Preserve each previously exposed binding's availability. New bindings
    /// may be added; an ambiguous rename remains unverified.
    pub fn requires_view_schema(&self, view: &str) -> bool {
        self.source_views
            .iter()
            .find(|v| v.id == view)
            .is_some_and(|v| matches!(v.kind, ViewKind::List { .. } | ViewKind::Detail { .. }))
    }
    pub fn availability(
        &self,
        view: &str,
        placement: ActionPlacement,
        before: &BTreeSet<Id>,
        after: &BTreeSet<Id>,
    ) -> Option<bool> {
        let source = self.source_views.iter().find(|v| v.id == view)?;
        let target_id = self.id(SemanticKind::View, view);
        let target = self.target_views.iter().find(|v| v.id == target_id)?;
        let mut unknown = false;
        for old in source.actions.iter().filter(|a| a.placement == placement) {
            let action = self.id(SemanticKind::Action, &old.action);
            let candidates: Vec<_> = target
                .actions
                .iter()
                .filter(|a| a.placement == placement && a.action == action)
                .collect();
            let found = candidates
                .iter()
                .find(|a| a.id == old.id)
                .copied()
                .or_else(|| {
                    if candidates.len() == 1 {
                        Some(candidates[0])
                    } else {
                        None
                    }
                });
            match found {
                Some(new) if before.contains(&old.id) != after.contains(&new.id) => {
                    return Some(false)
                }
                Some(_) => {}
                None => unknown = true,
            }
        }
        if unknown {
            None
        } else {
            Some(true)
        }
    }
    pub fn context(&self, source: &ScenarioSpec, target: &AppDefinition) -> Result<ScenarioSpec> {
        let mut s = source.clone();
        s.seed = crate::product_runtime::merged_definition_data(target, &s.seed)?;
        let mut session = SessionState::initial(target)?;
        for (key, value) in &s.session.values {
            if let Some(v) = session.values.get_mut(key) {
                *v = value.clone();
            } else {
                return Err(DecisionError::Unverified(format!(
                    "Session state {key} has no mapping"
                )));
            }
        }
        session.view = self.id(SemanticKind::View, &s.session.view);
        session.focused_record = s.session.focused_record.clone();
        s.session = session;
        s.validity = s.validity.iter().map(|p| self.property(p)).collect();
        s.seed.validate()?;
        s.session.validate(target)?;
        Ok(s)
    }
    pub fn input(&self, input: &SemanticInput) -> SemanticInput {
        let mut input = input.clone();
        match &mut input {
            SemanticInput::Invoke { action, .. } => *action = self.id(SemanticKind::Action, action),
            SemanticInput::Control { view, .. }
            | SemanticInput::Activate { view, .. }
            | SemanticInput::Submit { view, .. }
            | SemanticInput::Navigate { view } => *view = self.id(SemanticKind::View, view),
            _ => {}
        }
        input
    }
    pub fn replacement(
        &self,
        source: &ScenarioSpec,
        target: &AppDefinition,
        replacement: &ScenarioSpec,
    ) -> Result<ScenarioSpec> {
        let protected = self.context(source, target)?;
        let identity = Mapping::new(target, target, &[])?;
        let normalized = identity.context(replacement, target)?;
        normalized.validate(target)?;
        let points = |s: &ScenarioSpec| {
            s.inputs
                .iter()
                .filter_map(|i| match i {
                    SemanticInput::Observe { point } => Some(point.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        if normalized.seed != protected.seed
            || normalized.session != protected.session
            || normalized.clock_day != protected.clock_day
            || normalized.random_seed != protected.random_seed
            || normalized.validity != protected.validity
            || points(&normalized) != points(&protected)
        {
            return Err(invalid("replacement weakened an accepted seed, context, validity condition or observation point"));
        }
        Ok(normalized)
    }
    pub fn scenario(&self, source: &ScenarioSpec, target: &AppDefinition) -> Result<ScenarioSpec> {
        let mut s = self.context(source, target)?;
        s.inputs = s.inputs.iter().map(|i| self.input(i)).collect();
        s.validate(target)?;
        Ok(s)
    }
    pub fn property(&self, p: &AcceptedProperty) -> AcceptedProperty {
        let mut p = p.clone();
        self.predicate(&mut p.predicate);
        p
    }
    fn predicate(&self, p: &mut PropertyPredicate) {
        match p {
            PropertyPredicate::Equal { left, right } | PropertyPredicate::Less { left, right } => {
                self.term(left);
                self.term(right);
            }
            PropertyPredicate::And { values } | PropertyPredicate::Or { values } => {
                for v in values {
                    self.predicate(v)
                }
            }
            PropertyPredicate::Not { value } => self.predicate(value),
        }
    }
    fn term(&self, t: &mut PropertyTerm) {
        match t {
            PropertyTerm::Observed { observable, .. } => {
                *observable = self.id(SemanticKind::Observable, observable)
            }
            PropertyTerm::OutputCount { output, .. }
            | PropertyTerm::OutputColumn { output, .. } => {
                *output = self.id(SemanticKind::Output, output)
            }
            PropertyTerm::Count { value } => self.term(value),
            _ => {}
        }
    }
}
