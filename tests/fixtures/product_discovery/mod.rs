//! Shared fictional discovery request/response; no live-provider evidence.
use crate::fixture::*;
use crate::product_contract::*;
use serde_json::json;
use std::collections::BTreeSet;

pub fn input() -> (DevelopmentRequest, DevelopmentResult) {
    let a = capture(filtered());
    let mut v = filtered();
    v["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":v["actions"][1]["steps"][0]["items"].clone()});
    let b = capture(v);
    let scene = scenario(
        &a,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("Ada"),
            },
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let request = DevelopmentRequest {
        version: 1,
        id: "discover-test".into(),
        project_id: "runtime-project".into(),
        operation: DevelopmentOperation::Discover,
        request: "Make collecting the current search results work smoothly".into(),
        sources: vec![a.clone(), b.clone()],
        context: DevelopmentContext {
            view: Some("people".into()),
            selected: vec![],
            recent_inputs: vec![invoke("collect", Values::new())],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: decisions(),
        unknowns: vec![],
        required_capabilities: BTreeSet::new(),
    };
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![
            GeneratedCandidate {
                id: "retain".into(),
                source_json: String::from_utf8(a.source_bytes.clone()).unwrap(),
            },
            GeneratedCandidate {
                id: "replace".into(),
                source_json: String::from_utf8(b.source_bytes.clone()).unwrap(),
            },
        ],
        hypotheses: vec![ChoiceHypothesis {
            id: "choice".into(),
            statement: "Repeated collection may keep or replace previous results".into(),
            kind: HypothesisKind::UnresolvedChoice,
            action: "collect".into(),
            observable: "selected_count".into(),
            sources: vec![SourceLocus {
                relative_path: "program.json".into(),
                raw_digest: b.artifact.raw_digest.clone(),
                pointer: "/actions/1/steps/0".into(),
            }],
            alternatives: vec!["retain".into(), "replace".into()],
            related_decisions: vec![],
            scenario_json: serde_json::to_string(&scene).unwrap(),
            unknowns: vec![],
        }],
        evolutions: vec![],
        unsupported: vec![],
    };
    (
        request,
        DevelopmentResult {
            response,
            producer: Producer::Fixture {
                name: "recorded untrusted hypotheses".into(),
            },
        },
    )
}
