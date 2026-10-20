use serde_json::{json, Value};

pub fn minimal() -> Value {
    let yes = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}});
    json!({
        "version":1,"id":"source_fixture","label":"Source fixture",
        "entities":[{"id":"row","label":"Rows","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}],
        "state":[],
        "actions":[{"id":"check","label":"Check","parameters":{},"guards":[],"steps":[{"kind":"assert","condition":yes,"message":"yes"}],"ensures":[]}],
        "outputs":[],
        "observables":[{"id":"yes","label":"Yes","value":yes}],
        "views":[{"id":"rows","label":"Rows","kind":{"kind":"list","entity":"row","rows":{"kind":"query","entity":"row","binding":"item","predicate":yes,"sort":[],"limit":1,"include_archived":false},"columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],"controls":[],"selection":null},"actions":[],"keys":[]}],
        "initial_view":"rows"
    })
}

pub fn malformed_sources(max_bytes: usize) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty", vec![]),
        ("invalid_utf8", vec![0xff]),
        ("unfinished_string", br#"{"label":"broken"#.to_vec()),
        ("invalid_escape", br#"{"label":"\q"}"#.to_vec()),
        ("unpaired_surrogate", br#"{"label":"\uD800"}"#.to_vec()),
        ("escaped_duplicate", br#"{"version":1,"\u0076ersion":1}"#.to_vec()),
        ("duplicate_before_missing_value", br#"{"x":0,"x":"#.to_vec()),
        ("syntax_before_duplicate", br#"{"x":,"x":0}"#.to_vec()),
        ("trailing", b"{} false".to_vec()),
        ("number_overflow", b"1e999999999999999999999".to_vec()),
        ("typed_missing", b"{}".to_vec()),
        ("typed_scalar", b"null".to_vec()),
        ("exact_size_typed_failure", [vec![b' '; max_bytes - 2], b"{}".to_vec()].concat()),
        ("oversize_before_syntax", vec![b'!'; max_bytes + 1]),
        ("depth_64_typed_failure", format!("{}null{}", "[".repeat(64), "]".repeat(64)).into_bytes()),
        ("depth_65", format!("{}null{}", "[".repeat(65), "]".repeat(65)).into_bytes()),
        ("malformed_deep_unwind", format!("{}?", "[".repeat(64)).into_bytes()),
        ("early_syntax_long_suffix", [br#"{"x":?,"#.to_vec(), vec![b' '; 512], br#""later":"a\nb"}"#.to_vec()].concat()),
        ("early_duplicate_long_suffix", [br#"{"x":0,"x":1,"#.to_vec(), vec![b' '; 512], br#""later":[{}]}"#.to_vec()].concat()),
    ]
}

pub fn semantic_negatives() -> Vec<(&'static str, Value)> {
    let mut result = Vec::new();
    for (name, pointer, replacement) in [
        ("bad_version", "/version", json!(2)),
        ("bad_id", "/id", json!("BAD")),
        ("empty_label", "/label", json!(" ")),
        ("unknown_initial_view", "/initial_view", json!("missing")),
        ("invalid_field_type", "/entities/0/fields/0/value_type", json!({"kind":"reference","entity":"missing"})),
        ("empty_unique", "/entities/0/unique", json!([[]])),
        ("missing_unique_field", "/entities/0/unique", json!([["missing"]])),
        ("state_in_constraint", "/entities/0/constraints", json!([{"kind":"state","state":"missing"}])),
        ("action_empty_steps", "/actions/0/steps", json!([])),
        ("action_bad_parameter", "/actions/0/parameters", json!({"BAD":{"kind":"text"}})),
        ("action_nonbool_guard", "/actions/0/guards", json!([{"kind":"today"}])),
        ("action_nonbool_ensure", "/actions/0/ensures", json!([{"kind":"today"}])),
        ("latent_invalid_branch", "/observables/0/value", json!({"kind":"if","condition":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"then_value":{"kind":"today"},"else_value":{"kind":"variable","name":"missing"}})),
        ("query_limit", "/views/0/kind/rows/limit", json!(0)),
        ("query_shadowing", "/views/0/kind/rows/predicate", json!({"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":1,"include_archived":false})),
        ("unknown_column_field", "/views/0/kind/columns/0/value/field", json!("missing")),
        ("keyboard_unknown_binding", "/views/0/keys", json!([{"key":"a","modifiers":[],"binding":"missing"}])),
        ("unknown_typed_field", "/actions/0/steps/0", json!({"kind":"assert","condition":{"kind":"today"},"message":"x","unknown":0})),
        ("unknown_typed_variant", "/observables/0/value", json!({"kind":"missing"})),
    ] {
        let mut value = minimal();
        *value.pointer_mut(pointer).unwrap() = replacement;
        result.push((name, value));
    }
    let mut duplicate = minimal();
    let first = duplicate["entities"][0].clone();
    duplicate["entities"].as_array_mut().unwrap().push(first);
    result.push(("duplicate_entity", duplicate));
    let mut deep = minimal();
    let mut expr = json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}});
    for _ in 0..17 { expr = json!({"kind":"not","value":expr}); }
    deep["observables"][0]["value"] = expr;
    result.push(("expression_depth", deep));
    let mut deep_type = minimal();
    let mut typ = json!({"kind":"text"});
    for _ in 0..6 { typ = json!({"kind":"list","item":typ}); }
    deep_type["entities"][0]["fields"][0]["value_type"] = typ;
    result.push(("type_depth", deep_type));
    let mut long = minimal();
    long["label"] = json!("x".repeat(4097));
    result.push(("text_limit", long));
    result
}
