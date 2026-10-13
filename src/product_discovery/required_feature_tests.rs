//! Typed absence classification only. These synthetic requirements do not
//! substitute for the host's separately tested explicit UI acceptance.
use super::*;
#[path = "../../tests/fixtures/product_runtime/mod.rs"]
mod fixture;
use fixture::*;
use serde_json::json;

fn case(
    kind: &str,
) -> (
    CapturedProgram,
    CapturedProgram,
    ScenarioSpec,
    DiscoveryPolicy,
) {
    let mut current = filtered();
    if kind == "selection" {
        current["views"][0]["kind"]["selection"] = serde_json::Value::Null;
    }
    let before = capture(current);
    let mut raw = filtered();
    let query = raw["actions"][1]["steps"][0]["items"].clone();
    raw["actions"].as_array_mut().unwrap().push(json!({"id":"feature","label":"New feature","parameters":match kind { "control" => json!({"value":{"kind":"text"}}), "selection" => json!({"value":{"kind":"list","item":{"kind":"reference","entity":"person"}}}), _ => json!({}) },"guards":[],"steps":[{"kind":"set_state","state":"selected","value":query}],"ensures":[]}));
    if kind == "activate" {
        raw["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"feature_button","label":"Feature","action":"feature","placement":"toolbar","arguments":{},"enabled":yes()}));
    }
    if kind == "control" {
        raw["views"][0]["kind"]["controls"].as_array_mut().unwrap().push(json!({"id":"feature_control","label":"Feature control","state":"search","on_change":"feature"}));
    }
    if kind == "selection" {
        raw["views"][0]["kind"]["selection"]["on_change"] = json!("feature");
    }
    if kind == "submit" {
        raw["views"].as_array_mut().unwrap().push(json!({"id":"feature_form","label":"Feature form","kind":{"kind":"form","action":"feature","fields":[],"defaults":{}},"actions":[],"keys":[]}));
    }
    let candidate = capture(raw);
    let input = match kind {
        "invoke" => SemanticInput::Invoke {
            action: "feature".into(),
            arguments: Values::new(),
        },
        "activate" => SemanticInput::Activate {
            view: "people".into(),
            binding: "feature_button".into(),
            row: None,
        },
        "submit" => SemanticInput::Submit {
            view: "feature_form".into(),
            arguments: Values::new(),
        },
        "control" => SemanticInput::Control {
            view: "people".into(),
            control: "feature_control".into(),
            value: string("Ada"),
        },
        "selection" => SemanticInput::Control {
            view: "people".into(),
            control: "pick".into(),
            value: DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        },
        _ => unreachable!(),
    };
    let scene = scenario(
        &candidate,
        vec![
            input,
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
    );
    scene.validate(&candidate.program).unwrap();
    assert!(scene.validate(&before.program).is_err());
    let property = AcceptedProperty {
        id: "selected-result".into(),
        description: "Synthetic independently supplied exact result".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "result".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 0 },
            },
        },
    };
    let policy = DiscoveryPolicy {
        required_actions: ["feature".into()].into(),
        requirements: vec![RequirementCase {
            id: "explicit-case".into(),
            scenario: scene.clone(),
            properties: vec![property],
        }],
        ..Default::default()
    };
    (before, candidate, scene, policy)
}
#[test]
fn only_exact_required_invoke_activate_submit_and_control_absence_is_classified() {
    for kind in ["invoke", "activate", "submit", "control", "selection"] {
        let (before, candidate, scene, policy) = case(kind);
        assert!(
            required_feature_absence(&before, &candidate, &scene, &policy).is_some(),
            "{kind}"
        );
        let mut no_case = policy.clone();
        no_case.requirements.clear();
        assert!(
            required_feature_absence(&before, &candidate, &scene, &no_case).is_none(),
            "No independent property: {kind}"
        );
        let mut unrequired = policy.clone();
        unrequired.required_actions.clear();
        assert!(
            required_feature_absence(&before, &candidate, &scene, &unrequired).is_none(),
            "A changed name is not independent acceptance: {kind}"
        );
        let mut unrelated = scene.clone();
        unrelated.inputs.insert(
            0,
            SemanticInput::Invoke {
                action: "unrelated_missing".into(),
                arguments: Values::new(),
            },
        );
        assert!(
            required_feature_absence(&before, &candidate, &unrelated, &policy).is_none(),
            "Unrelated invalid input: {kind}"
        );
        let mut wrong_session = scene.clone();
        wrong_session.session.view = "missing_view".into();
        assert!(
            required_feature_absence(&before, &candidate, &wrong_session, &policy).is_none(),
            "Unrelated view: {kind}"
        );
    }
}
#[test]
fn wrong_ui_target_and_arguments_are_not_explained_as_a_missing_feature() {
    let (before, candidate, mut scene, policy) = case("activate");
    scene.inputs[0] = SemanticInput::Activate {
        view: "new_person".into(),
        binding: "feature_button".into(),
        row: None,
    };
    assert!(required_feature_absence(&before, &candidate, &scene, &policy).is_none());
    for kind in ["invoke", "submit"] {
        let (before, candidate, mut scene, policy) = case(kind);
        match &mut scene.inputs[0] {
            SemanticInput::Invoke { arguments, .. } | SemanticInput::Submit { arguments, .. } => {
                arguments.insert("wrong_argument".into(), string("not declared"));
            }
            _ => unreachable!(),
        }
        assert!(
            required_feature_absence(&before, &candidate, &scene, &policy).is_none(),
            "{kind}"
        );
    }
    let (before, candidate, mut scene, policy) = case("control");
    if let SemanticInput::Control { value, .. } = &mut scene.inputs[0] {
        *value = DataValue::Integer { value: 7 };
    }
    assert!(required_feature_absence(&before, &candidate, &scene, &policy).is_none());
}

#[test]
fn selection_callback_absence_preserves_exact_target_and_list_reference_types() {
    let (before, candidate, scene, policy) = case("selection");
    for (view, control, value) in [
        (
            "new_person",
            "pick",
            DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        ),
        (
            "people",
            "missing_pick",
            DataValue::List {
                item_type: Type::reference("person"),
                items: vec![],
            },
        ),
        ("people", "pick", string("not a reference list")),
        (
            "people",
            "pick",
            DataValue::List {
                item_type: Type::Text,
                items: vec![string("not a reference")],
            },
        ),
        (
            "people",
            "pick",
            DataValue::List {
                item_type: Type::reference("asset"),
                items: vec![reference("asset", "drill")],
            },
        ),
    ] {
        let mut wrong = scene.clone();
        wrong.inputs[0] = SemanticInput::Control {
            view: view.into(),
            control: control.into(),
            value,
        };
        assert!(wrong.validate(&candidate.program).is_err());
        assert!(required_feature_absence(&before, &candidate, &wrong, &policy).is_none());
    }
    // A valid but unrelated Current-incompatible control must survive the
    // projection and make classification fail even beside the required callback.
    let mut raw: serde_json::Value = serde_json::from_slice(&candidate.source_bytes).unwrap();
    raw["state"].as_array_mut().unwrap().push(json!({"id":"extra_text","label":"Extra text","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}}));
    raw["views"][0]["kind"]["controls"].as_array_mut().unwrap().push(json!({"id":"unrelated_control","label":"Unrelated control","state":"extra_text","on_change":null}));
    let candidate = capture(raw);
    let mut unrelated = scene.clone();
    unrelated.session = SessionState::initial(&candidate.program).unwrap();
    unrelated.inputs.insert(
        0,
        SemanticInput::Control {
            view: "people".into(),
            control: "unrelated_control".into(),
            value: string("unrelated"),
        },
    );
    unrelated.validate(&candidate.program).unwrap();
    assert!(required_feature_absence(&before, &candidate, &unrelated, &policy).is_none());
}
