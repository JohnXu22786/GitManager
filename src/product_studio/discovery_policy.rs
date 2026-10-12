//! Explicit selected typed output from actual copied UI execution. Viewing,
//! replaying and provider disclosure are never requirement acceptance.
use super::*;
use crate::product_decisions::AcceptedScene;
use crate::product_discovery::{DiscoveryPolicy, RequirementCase};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AcceptedCase {
    pub basis: Basis,
    pub original_request: Digest,
    pub authored_source: Digest,
    pub target_source: Digest,
    pub source_operation: Id,
    pub approval_operation: Id,
    /// Editable exact copied trace; final observation is added by real replay.
    pub scenario: ScenarioSpec,
    pub property: AcceptedProperty,
    pub required_actions: BTreeSet<Id>,
}
#[derive(Clone)]
pub(super) struct Offer {
    pub id: String,
    pub label: String,
    pub property: AcceptedProperty,
}
#[derive(Clone)]
pub(super) struct Trial {
    pub basis: Basis,
    pub original_request: Digest,
    pub source_operation: Id,
    pub authored_source: Digest,
    pub scenario: ScenarioSpec,
    pub scene: AcceptedScene,
    pub model: RuntimeView,
    pub day: i32,
    pub offers: Vec<Offer>,
    pub required_actions: BTreeSet<Id>,
    pub accepted: bool,
}
#[derive(Clone)]
pub(super) struct View {
    pub basis: Basis,
    pub model: RuntimeView,
    pub day: i32,
    pub ticket: Digest,
    pub offers: Vec<Offer>,
    pub can_accept: bool,
    pub accepted: bool,
}
fn new_actions(
    before: &AppDefinition,
    candidate: &AppDefinition,
    scenario: &ScenarioSpec,
) -> BTreeSet<Id> {
    scenario
        .inputs
        .iter()
        .filter_map(|input| change::operation(candidate, input))
        .filter(|id| {
            let Some(action) = candidate.actions.iter().find(|a| &a.id == id) else {
                return false;
            };
            !before.actions.iter().any(|old| {
                &old.id == id || {
                    // A pure action rename/alias is not automatically a newly required
                    // feature. No labels or natural-language keywords decide this.
                    let mut comparable = old.clone();
                    comparable.id = action.id.clone();
                    comparable.label = action.label.clone();
                    &comparable == action
                }
            })
        })
        .collect()
}
fn offers(scene: &AcceptedScene) -> Result<Vec<Offer>, String> {
    let observed = scene
        .observations()
        .last()
        .ok_or("The copied example has no actual final observation")?;
    let point = observed.point.clone();
    let mut result = vec![];
    let mut add = |id: String,
                   label: String,
                   left: PropertyTerm,
                   value_type: Type,
                   value: DataValue|
     -> Result<(), String> {
        let property = AcceptedProperty {
            id: format!("feature-property-{}", result.len()),
            description: label.clone(),
            predicate: PropertyPredicate::Equal {
                left,
                right: PropertyTerm::Literal { value_type, value },
            },
        };
        property.validate().map_err(error)?;
        if property.evaluate(scene.observations()) != Some(true) {
            return Err(
                "The displayed result cannot be represented exactly as this typed property".into(),
            );
        }
        result.push(Offer {
            id,
            label,
            property,
        });
        Ok(())
    };
    for (id, value) in &observed.values {
        let typ = observed
            .value_types
            .get(id)
            .ok_or("Missing actual observation type")?
            .clone();
        add(
            format!("observed.{id}"),
            format!(
                "Keep {id} = {} for this exact new-feature example",
                crate::ui::product_runtime_view::value_text(value)
            ),
            PropertyTerm::Observed {
                point: point.clone(),
                observable: id.clone(),
                value_type: typ.clone(),
            },
            typ,
            value.clone(),
        )?;
    }
    if let Some(schema) = &observed.view_schema {
        let typ = Type::reference(&schema.entity);
        add(
            "view.rows".into(),
            "Keep exactly these displayed rows for this example".into(),
            PropertyTerm::ViewRows {
                point: point.clone(),
                entity: schema.entity.clone(),
            },
            Type::list(typ.clone()),
            DataValue::List {
                item_type: typ,
                items: observed
                    .view
                    .rows
                    .iter()
                    .map(|r| DataValue::Reference {
                        entity: r.record.entity.clone(),
                        record: r.record.record.clone(),
                    })
                    .collect(),
            },
        )?;
        for (column, typ) in &schema.columns {
            let items = observed
                .view
                .rows
                .iter()
                .map(|r| {
                    r.cells
                        .get(column)
                        .cloned()
                        .ok_or("Missing actual typed view cell")
                })
                .collect::<Result<Vec<_>, _>>()?;
            add(
                format!("view.column.{column}"),
                format!("Keep these displayed {column} values for this example"),
                PropertyTerm::ViewColumn {
                    point: point.clone(),
                    column: column.clone(),
                    value_type: typ.clone(),
                },
                Type::list(typ.clone()),
                DataValue::List {
                    item_type: typ.clone(),
                    items,
                },
            )?;
        }
    }
    for output in observed
        .outputs
        .iter()
        .map(|o| o.output.clone())
        .collect::<BTreeSet<_>>()
    {
        let artifacts: Vec<_> = observed
            .outputs
            .iter()
            .filter(|a| a.output == output)
            .collect();
        let count: usize = artifacts.iter().map(|a| a.rows.len()).sum();
        add(
            format!("output.count.{output}"),
            format!("Keep {count} exported {output} rows for this example"),
            PropertyTerm::OutputCount {
                point: point.clone(),
                output: output.clone(),
            },
            Type::Integer,
            DataValue::Integer {
                value: i64::try_from(count).map_err(error)?,
            },
        )?;
        for column in &artifacts[0].columns {
            let items = artifacts
                .iter()
                .flat_map(|a| &a.rows)
                .map(|r| {
                    r.get(&column.id)
                        .cloned()
                        .ok_or("Missing actual typed output cell")
                })
                .collect::<Result<Vec<_>, _>>()?;
            add(
                format!("output.column.{output}.{}", column.id),
                format!(
                    "Keep these exported {output} / {} values for this example",
                    column.label
                ),
                PropertyTerm::OutputColumn {
                    point: point.clone(),
                    output: output.clone(),
                    column: column.id.clone(),
                    value_type: column.value_type.clone(),
                },
                Type::list(column.value_type.clone()),
                DataValue::List {
                    item_type: column.value_type.clone(),
                    items,
                },
            )?;
        }
    }
    Ok(result)
}
impl Trial {
    pub fn open(
        change: &ChangeDraft,
        record: &discovery_host::Record,
        store: &ProductStore,
        scenario: Option<&ScenarioSpec>,
        gate: &Gate,
    ) -> Result<Self, String> {
        let (scenario, scene, model, day) = change.candidate_case(store, scenario, gate)?;
        let offers = offers(&scene)?;
        Ok(Self {
            basis: record.basis.clone(),
            original_request: record.original_request()?.identity().map_err(error)?,
            source_operation: change.operation.clone(),
            authored_source: canonical_digest(IdentityDomain::Source, &change.candidate)
                .map_err(error)?,
            required_actions: new_actions(
                &change.snapshot.program().map_err(error)?.program,
                &scene.program().program,
                &scenario,
            ),
            scenario,
            scene,
            model,
            day,
            offers,
            accepted: false,
        })
    }
    pub fn ticket(&self) -> Result<Digest, String> {
        canonical_digest(
            IdentityDomain::Evidence,
            &(
                &self.basis,
                &self.original_request,
                &self.source_operation,
                &self.authored_source,
                self.scene.evidence(),
            ),
        )
        .map_err(error)
    }
    pub fn view(&self) -> Result<View, String> {
        Ok(View {
            basis: self.basis.clone(),
            model: self.model.clone(),
            day: self.day,
            ticket: self.ticket()?,
            offers: self.offers.clone(),
            can_accept: !self.required_actions.is_empty(),
            accepted: self.accepted,
        })
    }
    pub fn accept(
        &self,
        ticket: &Digest,
        offered: &str,
        approval: &str,
    ) -> Result<AcceptedCase, String> {
        if &self.ticket()? != ticket || !valid_id(approval) || self.required_actions.is_empty() {
            return Err(
                "Try an actual newly added action and explicitly choose its latest typed result"
                    .into(),
            );
        }
        let offer = self
            .offers
            .iter()
            .find(|o| o.id == offered)
            .ok_or("Choose one exact visible typed result")?;
        Ok(AcceptedCase {
            basis: self.basis.clone(),
            original_request: self.original_request.clone(),
            authored_source: self.authored_source.clone(),
            target_source: canonical_digest(IdentityDomain::Source, self.scene.program())
                .map_err(error)?,
            source_operation: self.source_operation.clone(),
            approval_operation: approval.into(),
            scenario: self.scenario.clone(),
            property: offer.property.clone(),
            required_actions: self.required_actions.clone(),
        })
    }
}
impl AcceptedCase {
    pub fn validate(&self) -> Result<(), String> {
        self.scenario.validate_structure().map_err(error)?;
        self.property.validate().map_err(error)?;
        if !valid_id(&self.source_operation)
            || !valid_id(&self.approval_operation)
            || self.required_actions.is_empty()
            || self.required_actions.len() > MAX_ITEMS
            || !self.scenario.validity.is_empty()
        {
            return Err("Invalid explicit typed-result acceptance".into());
        }
        Ok(())
    }
    pub fn check(
        &self,
        change: &ChangeDraft,
        record: &discovery_host::Record,
        store: &ProductStore,
        gate: &Gate,
    ) -> Result<(DiscoveryPolicy, SelectedScenario), String> {
        self.validate()?;
        let actual = Trial::open(change, record, store, Some(&self.scenario), gate)?;
        if self.basis != actual.basis
            || self.original_request != actual.original_request
            || self.authored_source != actual.authored_source
            || self.source_operation != actual.source_operation
            || self.target_source
                != canonical_digest(IdentityDomain::Source, actual.scene.program())
                    .map_err(error)?
            || self.required_actions != actual.required_actions
            || !actual
                .offers
                .iter()
                .any(|offer| offer.property == self.property)
            || self.property.evaluate(actual.scene.observations()) != Some(true)
        {
            return Err("The accepted typed result, exact source, trace or saved work changed. Try and select it again".into());
        }
        let mut example = actual.scene.scenario().clone();
        let policy = DiscoveryPolicy {
            required_actions: self.required_actions.clone(),
            requirements: vec![RequirementCase {
                id: self.approval_operation.clone(),
                scenario: example.clone(),
                properties: vec![self.property.clone()],
            }],
            ..DiscoveryPolicy::default()
        };
        example.validity = vec![self.property.clone()];
        Ok((
            policy,
            SelectedScenario {
                disclosure: Disclosure::ExplicitlySelected,
                scenario: example,
            },
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/fixtures/product_runtime/mod.rs"]
mod policy_fixture;
#[cfg(test)]
mod tests {
    use super::policy_fixture as fixture;
    use super::*;
    #[test]
    fn renamed_or_aliased_existing_actions_cannot_create_a_new_feature_acceptance() {
        let before = fixture::filtered();
        let mut candidate = before.clone();
        let mut alias = candidate["actions"][1].clone();
        alias["id"] = serde_json::json!("renamed_collect");
        alias["label"] = serde_json::json!("Brand new feature label");
        candidate["actions"].as_array_mut().unwrap().push(alias);
        let before = fixture::capture(before);
        let candidate = fixture::capture(candidate);
        let scene = fixture::scenario(
            &candidate,
            vec![SemanticInput::Invoke {
                action: "renamed_collect".into(),
                arguments: Values::new(),
            }],
        );
        assert!(new_actions(&before.program, &candidate.program, &scene).is_empty());
    }
}
