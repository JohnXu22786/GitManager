//! Fictional authored development and two independently compiled prospective designs.
#[path = "../product_scope/mod.rs"]
mod scope_fixture;
use crate::product_contract::*;
use crate::product_decisions::*;
use crate::product_discovery::*;
use crate::product_runtime::LocalRuntime;
use crate::product_store::{scope::*, ProductStore, ProjectSnapshot};
pub use scope_fixture::*;
use serde_json::json;

pub fn managed_store(path: &std::path::Path) -> ProductStore {
    let store = ProductStore::create(path, &program(false), 20000).unwrap();
    let current = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::All),
            "initial-layer",
        )
        .unwrap();
    store.adopt_scoped(current.revision, &prepared).unwrap();
    add(&store, "job", "Retained work");
    store
}
pub fn design(value: i64, producer: Producer, additive: bool) -> CapturedProgram {
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["state"] = json!([{"id":"plan_value","label":"Plan result","value_type":{"kind":"integer"},"initial":{"kind":"integer","value":0}}]);
    raw["actions"].as_array_mut().unwrap().push(json!({"id":"plan","label":"Plan the next step","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"plan_value","value":int(value)}],"ensures":[]}));
    raw["observables"].as_array_mut().unwrap().push(json!({"id":"planned","label":"Planned work","value":{"kind":"state","state":"plan_value"}}));
    if additive {
        raw["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Future note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    }
    CapturedProgram::capture(
        &serde_json::to_vec(&raw).unwrap(),
        "scope-project",
        producer,
        None,
    )
    .unwrap()
}
pub fn fixture_producer() -> Producer {
    Producer::Fixture {
        name: "actual recorded development result".into(),
    }
}
pub fn mappings(current: &ProjectSnapshot, candidate: &CapturedProgram) -> Vec<ScopeSlotMapping> {
    let editable = current.editable_scope_context().unwrap().unwrap();
    editable
        .slots
        .iter()
        .map(|slot| ScopeSlotMapping {
            layer: slot.layer.clone(),
            patch: slot.patch,
            from_source: editable.compiled_source.clone(),
            from: slot.destination.clone(),
            to_source: canonical_digest(IdentityDomain::Source, candidate).unwrap(),
            to: slot.destination.clone(),
            subject: slot.subject.clone(),
        })
        .collect()
}
pub fn prepare(store: &ProductStore, source: &CapturedProgram, id: &str) -> PreparedScopedChange {
    let current = store.load().unwrap();
    store
        .prepare_managed_evolution(source, &mappings(&current, source), id)
        .unwrap()
}
pub fn scene(current: &ProjectSnapshot, target: &CapturedProgram) -> ScenarioSpec {
    ScenarioSpec {
        version: 1,
        id: "prospective-scene".into(),
        label: "Experience a new planning action".into(),
        seed: current.data.clone(),
        session: SessionState::initial(&target.program).unwrap(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("plan", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    }
}
pub fn choice(outcome: DecisionOutcome) -> Choice {
    Choice {
        id: "pending-designs".into(),
        request: "Retain these two new planning designs".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["plan".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    }
}
pub fn discovery(
    store: &ProductStore,
    primary: &PreparedScopedChange,
    alternative: &CapturedProgram,
) -> (DevelopmentRequest, DevelopmentResult, DiscoveryPolicy) {
    let current = store.load().unwrap();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let request = engine
        .inherit_request(
            &current,
            DevelopmentRequest {
                version: 1,
                id: "discover-new-designs".into(),
                project_id: current.data.project_id.clone(),
                operation: DevelopmentOperation::Discover,
                request: "Add useful planning with an explicit result".into(),
                sources: vec![current.program().unwrap().clone(), primary.target().clone()],
                context: DevelopmentContext {
                    view: Some("work".into()),
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity().unwrap()),
                    session_digest: Some(current.session.identity().unwrap()),
                },
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: Default::default(),
            },
        )
        .unwrap();
    let scenario = scene(&current, primary.target());
    let result = DevelopmentResult {
        producer: alternative.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![
                GeneratedCandidate {
                    id: "primary".into(),
                    source_json: String::from_utf8(primary.target().source_bytes.clone()).unwrap(),
                },
                GeneratedCandidate {
                    id: "alternative".into(),
                    source_json: String::from_utf8(alternative.source_bytes.clone()).unwrap(),
                },
            ],
            hypotheses: vec![ChoiceHypothesis {
                id: "planning-choice".into(),
                statement: "Two actual planning outcomes".into(),
                kind: HypothesisKind::UnresolvedChoice,
                action: "plan".into(),
                observable: "planned".into(),
                sources: vec![SourceLocus {
                    relative_path: primary.target().binding.program_path.clone(),
                    raw_digest: primary.target().artifact.raw_digest.clone(),
                    pointer: "/actions/7".into(),
                }],
                alternatives: vec!["primary".into(), "alternative".into()],
                related_decisions: vec![],
                scenario_json: serde_json::to_string(&scenario).unwrap(),
                unknowns: vec![],
            }],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let mut history = VerifiedRetainedHistory::load(store).unwrap();
    history
        .map_prepared_target(primary.clone(), vec![])
        .unwrap();
    let policy = DiscoveryPolicy {
        retained_history: Some(history),
        required_actions: ["plan".into()].into_iter().collect(),
        requirements: vec![RequirementCase {
            id: "positive-plan".into(),
            scenario,
            properties: vec![AcceptedProperty {
                id: "positive".into(),
                description: "The new action actually produces a positive plan".into(),
                predicate: PropertyPredicate::Less {
                    left: PropertyTerm::Literal {
                        value_type: Type::Integer,
                        value: DataValue::Integer { value: 0 },
                    },
                    right: PropertyTerm::Observed {
                        point: "result".into(),
                        observable: "planned".into(),
                        value_type: Type::Integer,
                    },
                },
            }],
        }],
        ..Default::default()
    };
    (request, result, policy)
}

pub fn full_scene(current: &ProjectSnapshot, target: &CapturedProgram) -> ScenarioSpec {
    let row = &current.data.records[0];
    let mut scene = scene(current, target);
    scene.inputs.splice(
        0..0,
        [
            invoke("wait", &[("row", reference(row))]),
            invoke("resume", &[("row", reference(row))]),
            invoke("calculate", &[("row", reference(row))]),
            invoke("complete", &[("row", reference(row))]),
            invoke("export", &[]),
            invoke("archive", &[("row", reference(row))]),
            invoke(
                "add",
                &[
                    ("name", text("Experienced future work")),
                    ("promised", DataValue::Date { days: 20020 }),
                ],
            ),
        ],
    );
    scene
}
