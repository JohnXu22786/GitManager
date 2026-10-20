//! These controls enter the real paid capture, never a comparator/proof API.
use super::app_length_tests::{
    capture_minimal, complete_fixture, deepest_valid_fixture, fixture, old, old_app, source_result,
    Work,
};
use super::*;
use serde_json::{json, Value};

// Textual source guards only; never used for JSON or production values.
fn source_guard_text(source: &str) -> String { source.replace("\r\n", "\n") }

#[test]
fn closed_value_source_guard_normalizes_only_crlf_pairs() {
    let lf = "pub enum Type {\n    Text, // é \\r\\n\n}\n\r standalone";
    assert_eq!(source_guard_text(lf), lf);
    assert_eq!(source_guard_text(&lf.replace('\n', "\r\n")), lf);
    assert_eq!(source_guard_text("a\r\nb\nc\rd"), "a\nb\nc\rd");
    for changed in [lf.replace("Text", "Integer"), lf.replace("    ", "   "), lf.replace("é", "e")] {
        assert_ne!(source_guard_text(&changed.replace('\n', "\r\n")), lf);
    }
}

fn parity(capture: &CapturedProgram, expect_hit: Option<bool>) {
    let frozen: old::CapturedProgram =
        serde_json::from_slice(&serde_json::to_vec(capture).unwrap()).unwrap();
    let expected = frozen.validate().map_err(|e| e.0);
    let before = serde_json::to_vec(capture).unwrap();
    assert_eq!(capture.validate().map_err(|e| e.0), expected);
    closed_value::native_outputs::begin();
    let mut work = Work::default();
    let actual = source_result(capture, &mut work).map_err(|e| match e {
        ContractWorkError::Contract(e) => e.0,
        ContractWorkError::Resource(e) => panic!("unexpected resource failure: {e:?}"),
    });
    let outputs = closed_value::native_outputs::take();
    assert_eq!(actual, expected);
    assert_eq!(serde_json::to_vec(capture).unwrap(), before);
    if actual.is_ok() && expect_hit == Some(true) {
        assert_eq!(outputs.len(), 2);
        let original = old_app(&capture.program);
        let mut semantic = capture.program.clone();
        semantic
            .clear_display_labels_audited(Audit::none())
            .unwrap();
        assert_eq!(outputs[0].0, old::canonical_bytes(&original).unwrap());
        assert_eq!(
            outputs[1].0,
            old::canonical_bytes(&old_app(&semantic)).unwrap()
        );
        assert_ne!(
            outputs[0].1, outputs[1].1,
            "both native allocations must be live"
        );
        assert!(outputs.iter().all(|o| o.2 >= 128));
    }
    if expect_hit == Some(false) {
        assert!(outputs.is_empty());
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_representative_sources_keep_reviewed_complete_payments() {
    for (raw, expected) in [
        (
            include_str!("../../tests/fixtures/closed_value/before_a.json"),
            6_459_168,
        ),
        (
            include_str!("../../tests/fixtures/closed_value/after_b.json"),
            8_788_933,
        ),
    ] {
        let app: AppDefinition = serde_json::from_str(raw).unwrap();
        let c = CapturedProgram::capture(
            &serde_json::to_vec(&app).unwrap(),
            "fact-fixture",
            Producer::Fixture {
                name: "generic common factual admission".into(),
            },
            None,
        )
        .unwrap();
        parity(&c, Some(true));
        let mut work = Work::default();
        source_result(&c, &mut work).unwrap();
        // Keep the original frozen complete-source bound. Remove only the
        // reference second descriptor execution, never its native reservation.
        let mut semantic = c.program.clone();
        semantic.clear_display_labels_audited(Audit::none()).unwrap();
        let value = serde_json::to_value(&semantic).unwrap();
        let mut descriptor = Work::default();
        let mut shape = Shape::new(Audit::none()).unwrap();
        shape.canonical = false;
        shape.simple_app = true;
        let state = WorkState::new(&mut descriptor);
        shape.audit = Audit(Some(&state));
        state.finish(value.serialize(&mut shape)).unwrap();
        let removed = descriptor.calls.iter().sum::<u64>();
        let mut labels: Vec<&str> = vec![&c.program.label];
        for e in &c.program.entities { labels.push(&e.label); labels.extend(e.fields.iter().map(|f| f.label.as_str())); }
        labels.extend(c.program.state.iter().map(|v| v.label.as_str()));
        labels.extend(c.program.actions.iter().map(|v| v.label.as_str()));
        labels.extend(c.program.observables.iter().map(|v| v.label.as_str()));
        for o in &c.program.outputs { labels.push(&o.label); labels.extend(o.columns.iter().map(|v| v.label.as_str())); }
        for v in &c.program.views {
            labels.push(&v.label); labels.extend(v.actions.iter().map(|v| v.label.as_str()));
            match &v.kind {
                ViewKind::List { columns, controls, .. } => { labels.extend(columns.iter().map(|v| v.label.as_str())); labels.extend(controls.iter().map(|v| v.label.as_str())); },
                ViewKind::Detail { columns, .. } => labels.extend(columns.iter().map(|v| v.label.as_str())),
                ViewKind::Form { fields, .. } => labels.extend(fields.iter().map(|v| v.label.as_str())),
            }
        }
        let k = labels.len() as u64;
        let l = labels.iter().map(|s| s.len() as u64).sum::<u64>();
        let additions = 6144 + 256 + 128 * k + 14 * l;
        let mut count = Work::default();
        let state = WorkState::new(&mut count);
        state.finish(app_canonical_len_audited(&c.program, Audit(Some(&state)))).unwrap();
        let removed_count = count.calls.iter().sum::<u64>();
        // Only this complete-hit count disappears. Both count polls survive;
        // all three new128 control packets and the actual2240 owner root pay.
        let bound_additions = 3 * 128 + 2240;
        assert_eq!(work.calls.iter().sum::<u64>() + 5,
            expected - removed + additions - removed_count + bound_additions
                - super::checked_tag_tests::removed_tag_units(&c));
        eprintln!("complete source old={expected} removed_descriptor={removed} removed_count={removed_count} K={k} L={l} added={additions} bound_added={bound_additions} actual={}", work.calls.iter().sum::<u64>() + 5);
    }
}

fn graph_walk(
    ty: &str,
    value: &Value,
    path: &str,
    graph: &Value,
    seen: &mut BTreeMap<String, String>,
) {
    let ty = match ty {
        "Id" => "String",
        "Values" => "BTreeMap<Id, DataValue>",
        other => other,
    };
    if ty.starts_with("Box<") {
        return graph_walk(&ty[4..ty.len() - 1], value, path, graph, seen);
    }
    if ty.starts_with("Option<") {
        if !value.is_null() {
            graph_walk(&ty[7..ty.len() - 1], value, path, graph, seen);
        }
        return;
    }
    if ty.starts_with("Vec<") || ty.starts_with("BTreeSet<") {
        let start = if ty.starts_with("Vec<") { 4 } else { 9 };
        for (i, v) in value.as_array().unwrap().iter().enumerate() {
            graph_walk(
                &ty[start..ty.len() - 1],
                v,
                &format!("{path}/{i}"),
                graph,
                seen,
            );
        }
        return;
    }
    if ty.starts_with("BTreeMap<") {
        let child = ty
            .split_once(',')
            .unwrap()
            .1
            .trim()
            .strip_suffix('>')
            .unwrap();
        for (k, v) in value.as_object().unwrap() {
            graph_walk(
                child,
                v,
                &format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")),
                graph,
                seen,
            );
        }
        return;
    }
    if ["String", "bool", "u32", "usize", "i32", "i64"].contains(&ty) {
        return;
    }
    let spec = graph["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["type"] == ty)
        .unwrap();
    seen.entry(format!("type:{ty}"))
        .or_insert_with(|| path.into());
    let (owner, fields) = if spec["kind"] == "struct" {
        (ty.to_owned(), &spec["fields"])
    } else {
        let tag = value.get("kind").unwrap_or(value).as_str().unwrap();
        let variant = spec["variants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["wire_name"] == tag)
            .unwrap();
        let owner = format!("{ty}::{}", variant["name"].as_str().unwrap());
        seen.entry(format!("variant:{owner}"))
            .or_insert_with(|| path.into());
        (owner, &variant["fields"])
    };
    for field in fields.as_array().unwrap() {
        let key = field["name"].as_str().unwrap();
        let next = format!("{path}/{key}");
        seen.entry(format!("field:{owner}.{key}"))
            .or_insert_with(|| next.clone());
        graph_walk(
            field["rust_type"].as_str().unwrap(),
            &value[key],
            &next,
            graph,
            seen,
        );
    }
}
fn changed_leaf(value: &mut Value, last: bool) {
    match value {
        Value::String(s) => s.push('!'),
        Value::Bool(b) => *b = !*b,
        Value::Number(_) => *value = json!(1.5),
        Value::Null => *value = json!(false),
        Value::Array(items) if !items.is_empty() => {
            let i = if last { items.len() - 1 } else { 0 };
            changed_leaf(&mut items[i], last);
        }
        Value::Object(map) if !map.is_empty() => {
            let key = if last {
                map.keys().next_back()
            } else {
                map.keys().next()
            }
            .unwrap()
            .clone();
            changed_leaf(map.get_mut(&key).unwrap(), last);
        }
        _ => *value = json!(false),
    }
}
#[test]
fn closed_value_complete_graph_native_outputs_and_differing_children() {
    let graph: Value = serde_json::from_str(include_str!(
        "../../tests/support/capture_closed_graph.json"
    ))
    .unwrap();
    let value = complete_fixture();
    let c = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&c, Some(true));
    let canonical = serde_json::to_value(&c.program).unwrap();
    let mut seen = BTreeMap::new();
    graph_walk("AppDefinition", &canonical, "", &graph, &mut seen);
    let mut expected = BTreeSet::new();
    for typ in graph["types"].as_array().unwrap() {
        let name = typ["type"].as_str().unwrap();
        expected.insert(format!("type:{name}"));
        let cases = if typ["kind"] == "struct" {
            vec![(name.to_owned(), &typ["fields"])]
        } else {
            typ["variants"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    let name = format!("{name}::{}", v["name"].as_str().unwrap());
                    expected.insert(format!("variant:{name}"));
                    (name, &v["fields"])
                })
                .collect()
        };
        for (owner, fields) in cases {
            for f in fields.as_array().unwrap() {
                expected.insert(format!("field:{owner}.{}", f["name"].as_str().unwrap()));
            }
        }
    }
    assert_eq!(expected.len(), 250);
    assert_eq!(seen.keys().cloned().collect::<BTreeSet<_>>(), expected);
    for (id, path) in seen {
        for last in [false, true] {
            let mut bad = canonical.clone();
            changed_leaf(bad.pointer_mut(&path).unwrap(), last);
            let mut capture = c.clone();
            capture.source_bytes = serde_json::to_vec(&bad).unwrap();
            parity(&capture, Some(false));
        }
        if id.starts_with("field:") {
            let (parent, key) = path.rsplit_once('/').unwrap();
            for unknown in [false, true] {
                let mut bad = canonical.clone();
                let object = bad.pointer_mut(parent).unwrap().as_object_mut().unwrap();
                if unknown {
                    let v = object.remove(key).unwrap();
                    object.insert(format!("unknown_{key}"), v);
                } else {
                    object.remove(key);
                }
                let mut capture = c.clone();
                capture.source_bytes = serde_json::to_vec(&bad).unwrap();
                parity(&capture, Some(false));
            }
        }
    }
    let deep = CapturedProgram::capture(
        &serde_json::to_vec(&deepest_valid_fixture()).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&deep, Some(true));
}

#[test]
fn closed_value_native_alternatives_use_the_complete_miss() {
    fn permutations(
        prefix: Vec<&'static str>,
        left: Vec<&'static str>,
        all: &mut Vec<Vec<&'static str>>,
    ) {
        all.push(prefix.clone());
        for i in 0..left.len() {
            let mut next = prefix.clone();
            next.push(left[i]);
            let mut rest = left.clone();
            rest.remove(i);
            permutations(next, rest, all);
        }
    }
    let mut all = Vec::new();
    let order = vec!["control", "alt", "shift", "command"];
    permutations(vec![], order.clone(), &mut all);
    assert_eq!(all.len(), 65);
    let mut canonical_cases = 0;
    for modifiers in all {
        let mut value = complete_fixture();
        value["views"][0]["keys"][0]["modifiers"] = json!(modifiers);
        let c = CapturedProgram::capture(
            &serde_json::to_vec(&value).unwrap(),
            "project",
            Producer::UserAuthored,
            None,
        )
        .unwrap();
        let sorted = modifiers.windows(2).all(|w| {
            order.iter().position(|x| *x == w[0]).unwrap()
                < order.iter().position(|x| *x == w[1]).unwrap()
        });
        if sorted {
            canonical_cases += 1;
        }
        parity(&c, Some(sorted));
    }
    assert_eq!(canonical_cases, 16);
    for modifier in order {
        for representation in [json!([modifier, modifier]), json!([{modifier:null}])] {
            let mut value = complete_fixture();
            value["views"][0]["keys"][0]["modifiers"] = representation;
            let c = CapturedProgram::capture(
                &serde_json::to_vec(&value).unwrap(),
                "project",
                Producer::UserAuthored,
                None,
            )
            .unwrap();
            parity(&c, Some(false));
        }
    }
    let mut omitted = fixture::minimal();
    omitted["views"][0]["kind"]
        .as_object_mut()
        .unwrap()
        .remove("selection");
    let c = CapturedProgram::capture(
        &serde_json::to_vec(&omitted).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&c, Some(false));
    let value = fixture::minimal();
    let sequence = json!([
        value["version"],
        value["id"],
        value["label"],
        value["entities"],
        value["state"],
        value["actions"],
        value["outputs"],
        value["observables"],
        value["views"],
        value["initial_view"]
    ]);
    let c = CapturedProgram::capture(
        &serde_json::to_vec(&sequence).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&c, Some(false));
    for number in [json!(1.0), json!(-0.0), json!(1e0), json!(u64::MAX)] {
        let mut bad = fixture::minimal();
        bad["version"] = number;
        let mut c = capture_minimal();
        c.source_bytes = serde_json::to_vec(&bad).unwrap();
        parity(&c, Some(false));
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_unadmitted_payloads_stop_at_source_bounds() {
    let base = capture_minimal();
    let fixtures: Vec<(CapturedProgram, CapturedProgram)> = vec![
        {
            let mut small = base.clone();
            small.program.label = "different".into();
            let mut huge = small.clone();
            huge.program.label = "x".repeat(2 * MAX_WIRE_BYTES);
            (small, huge)
        },
        {
            let mut small = base.clone();
            small
                .program
                .entities
                .push(small.program.entities[0].clone());
            let mut huge = small.clone();
            huge.program.entities = vec![huge.program.entities[0].clone(); 4096];
            (small, huge)
        },
        {
            let mut small = base.clone();
            small.program.observables[0].value = Expr::Not {
                value: Box::new(Expr::Today),
            };
            let mut huge = small.clone();
            let mut child = Expr::Today;
            for _ in 0..128 {
                child = Expr::Not {
                    value: Box::new(child),
                };
            }
            huge.program.observables[0].value = child;
            (small, huge)
        },
    ];
    for (small, huge) in fixtures {
        let mut trace = Work::default();
        assert!(source_result(&small, &mut trace).is_err());
        let start = trace
            .calls
            .windows(4)
            .position(|w| w == [96, 149972, 6, 12])
            .unwrap();
        let poll = start + trace.calls[start..].iter().position(|&n| n == 0).unwrap();
        for candidate in [&small, &huge] {
            reset_validation_counts();
            let mut work = Work {
                refuse: Some(poll + 1),
                ..Work::default()
            };
            assert_eq!(
                source_result(candidate, &mut work),
                Err(ContractWorkError::Resource(WorkFailure::Cancelled))
            );
            assert_eq!(work.calls, &trace.calls[..=poll]);
            assert_eq!(validation_counts().apps, 0);
        }
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_every_small_prefix_and_mutation_refuses_without_publication() {
    let value = complete_fixture();
    let valid = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    let kinds: BTreeSet<_> = value["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["kind"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, BTreeSet::from(["list", "detail", "form"]));
    parity(&valid, Some(true));
    let data = DataSnapshot::empty("project", &valid.program).unwrap();
    let mut trace = Work::default();
    source_result(&valid, &mut trace).unwrap();
    let mutation = trace
        .calls
        .windows(6)
        .position(|w| w == [28, 64, 96, 13448, 4608, 8])
        .unwrap()
        + 1;
    let end = mutation
        + trace.calls[mutation..]
            .iter()
            .position(|&n| n == 0)
            .unwrap();
    let relation = trace
        .calls
        .windows(4)
        .position(|w| w == [96, 149972, 6, 12])
        .unwrap();
    let relation_end = relation
        + trace.calls[relation..]
            .iter()
            .position(|&n| n == 0)
            .unwrap();
    // Exhaustive positions, not amount-based equivalence: node12 and Number12
    // are distinct callbacks even when their debit is equal. The complete graph
    // fixture reaches every relation family and all schema-cleared owner kinds.
    let selected: BTreeSet<_> = (relation..=relation_end + 1)
        .chain(mutation..=end)
        .collect();
    eprintln!(
        "closed relation/mutation exhaustive refusal callbacks={}",
        selected.len()
    );
    for index in selected {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("boundary refusal"),
        ] {
            let mut work = Work {
                refuse: Some(index + 5),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            let result = with_capture_validation(
                &valid,
                &data,
                &mut work,
                |ctx| -> ContractWorkResult<()> {
                    assert!(ctx.ensure_source_and_data().is_err());
                    assert!(ctx.facts.is_none());
                    Ok(())
                },
            );
            assert_eq!(result, Err(ContractWorkError::Resource(cause)));
            assert_eq!(&work.calls[4..], &trace.calls[..=index]);
        }
    }
    let mut malformed = valid.clone();
    malformed.source_bytes = b"null".to_vec();
    let mut trace = Work::default();
    assert!(source_result(&malformed, &mut trace).is_err());
    for index in 0..trace.calls.len() {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("boundary refusal"),
        ] {
            let mut work = Work {
                refuse: Some(index + 1),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            assert_eq!(
                source_result(&malformed, &mut work),
                Err(ContractWorkError::Resource(cause))
            );
            assert_eq!(work.calls, &trace.calls[..=index]);
        }
    }
}

fn assert_closed_schema(source: &str, schema: &str) {
    let source = source_guard_text(source);
    let schema = source_guard_text(schema);
    let declaration = source.find("pub enum Type {").unwrap();
    let start = source[..declaration].rfind("#[derive(").unwrap();
    let end = source[declaration..].find("\n\nstruct Validator<").unwrap() + declaration;
    assert_eq!(
        source[start..end].trim_end(),
        schema.trim_end()
    );
    assert_eq!(
        source
            .lines()
            .filter(|s| s.starts_with("pub type Id ="))
            .collect::<Vec<_>>(),
        ["pub type Id = String;"]
    );
    assert_eq!(
        source
            .lines()
            .filter(|s| s.starts_with("pub type Values ="))
            .collect::<Vec<_>>(),
        ["pub type Values = BTreeMap<Id, DataValue>;"]
    );
}

#[test]
fn closed_value_schema_and_native_representation_guards() {
    assert_closed_schema(include_str!("../product_contract.rs"), include_str!("../../tests/support/capture_closed_schema.txt"));
    assert_ne!(
        serde_json::Number::from(1u32),
        serde_json::from_str::<Value>("1.0")
            .unwrap()
            .as_number()
            .unwrap()
            .clone()
    );
    assert_ne!(
        serde_json::Number::from(0i64),
        serde_json::from_str::<Value>("-0.0")
            .unwrap()
            .as_number()
            .unwrap()
            .clone()
    );
    for number in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(
            serde_json::to_value(number).unwrap(),
            Value::Number(serde_json::Number::from(number))
        );
    }
    assert_eq!(
        serde_json::to_value(u32::MAX).unwrap(),
        Value::Number(serde_json::Number::from(u32::MAX))
    );
    assert_eq!(
        serde_json::to_value(usize::MAX).unwrap(),
        Value::Number(serde_json::Number::from(usize::MAX))
    );
    assert_eq!(
        serde_json::to_value(i32::MIN).unwrap(),
        Value::Number(serde_json::Number::from(i32::MIN))
    );
    assert_eq!(
        serde_json::to_value(i32::MAX).unwrap(),
        Value::Number(serde_json::Number::from(i32::MAX))
    );
    let mut map = serde_json::Map::new();
    map.insert("z".into(), json!(1));
    map.insert("a".into(), json!(2));
    assert_eq!(
        map.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "z"]
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_simple_mode_overflow_and_false_byte_boundaries() {
    for (nodes, escaped, native) in [
        (u64::MAX, 0, 0),
        (0, u64::MAX, 0),
        (u64::MAX / 4, 2, 0),
        (0, (u64::MAX - 1) / 2, 0),
        (u64::MAX / 56 + 1, 0, 0),
        (0, 0, u64::MAX),
    ] {
        let mut work = Work::default();
        let state = WorkState::new(&mut work);
        let a = Audit(Some(&state));
        let shape = Shape {
            audit: a,
            bytes: 0,
            nodes,
            pairs: 0,
            native,
            canonical: false,
            digest_copies: 0,
            escaped,
            simple_app: true,
        };
        let result = reserve_app_count(&shape);
        assert!(result.is_err());
        assert!(a.pay(0).is_err());
        assert_eq!(
            state.finish(result),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted(
                "capture work arithmetic overflow"
            )))
        );
        assert_eq!(work.calls, [96, 2]);
    }
    for output in [false, true] {
        let mut work = Work::default();
        let state = WorkState::new(&mut work);
        let a = Audit(Some(&state));
        let mut shape = Shape {
            audit: a,
            bytes: 0,
            nodes: 0,
            pairs: 0,
            native: 0,
            canonical: false,
            digest_copies: 0,
            escaped: u64::MAX,
            simple_app: true,
        };
        let result = if output {
            shape.reserve_output()
        } else {
            shape.string("\"")
        };
        assert!(result.is_err());
        assert!(a.pay(0).is_err());
        assert_eq!(
            state.finish(result),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted(
                "capture work arithmetic overflow"
            )))
        );
        assert_eq!(work.calls.last(), Some(if output { &2 } else { &14 }));
    }
    let expected = [0, 358, 3, 6, 12, 2, 8];
    for index in [5, 6] {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("boundary refusal"),
        ] {
            let mut work = Work {
                refuse: Some(index + 1),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            let state = WorkState::new(&mut work);
            let a = Audit(Some(&state));
            let result = canonical_bytes_audited("x", a);
            assert!(result.is_err());
            assert!(a.pay(0).is_err());
            assert_eq!(
                state.finish(result),
                Err(ContractWorkError::Resource(cause))
            );
            assert_eq!(work.calls, &expected[..=index]);
        }
    }
}

#[test]
fn closed_value_native_writer_chunk_bound_covers_every_failure_prefix() {
    fn census(value: &Value) -> (usize, usize) {
        fn escapes(s: &str) -> usize {
            s.bytes()
                .filter(|b| *b <= 31 || *b == b'"' || *b == b'\\')
                .count()
        }
        match value {
            Value::String(s) => (1, escapes(s)),
            Value::Array(v) => v
                .iter()
                .map(census)
                .fold((1, 0), |(n, x), (m, y)| (n + m, x + y)),
            Value::Object(v) => v.iter().fold((1, 0), |(n, x), (key, value)| {
                let (m, y) = census(value);
                (n + 1 + m, x + escapes(key) + y)
            }),
            _ => (1, 0),
        }
    }
    struct Sink {
        attempts: usize,
        fail: Option<usize>,
        bytes: Vec<u8>,
    }
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.write_all(bytes)?;
            Ok(bytes.len())
        }
        fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.attempts += 1;
            if self.fail == Some(self.attempts) {
                return Err(std::io::ErrorKind::Other.into());
            }
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn check<T: Serialize>(value: &T, bound: usize, expected: &[u8]) {
        let mut success = Sink {
            attempts: 0,
            fail: None,
            bytes: vec![],
        };
        serde_json::to_writer(&mut success, value).unwrap();
        assert_eq!(success.bytes, expected);
        assert!(success.attempts <= bound);
        for stop in 1..=success.attempts {
            let mut sink = Sink {
                attempts: 0,
                fail: Some(stop),
                bytes: vec![],
            };
            assert!(serde_json::to_writer(&mut sink, value).unwrap_err().is_io());
            assert_eq!(sink.attempts, stop);
            assert!(sink.attempts <= bound);
            assert!(expected.starts_with(&sink.bytes));
        }
    }
    for text in [
        "".to_owned(),
        "/éβ🦀\u{2028}\u{2029}".into(),
        (0u8..=31).map(char::from).collect(),
        "\"\\\"\\a\na\tb\r\u{8}\u{c}".into(),
    ] {
        let mut value = fixture::minimal();
        value["label"] = json!(text);
        let app: AppDefinition = serde_json::from_value(value).unwrap();
        let value = serde_json::to_value(&app).unwrap();
        let (n, x) = census(&value);
        let bound = 4 * n + 2 * x + 2;
        check(&app, bound, &serde_json::to_vec(&app).unwrap());
        check(
            &value,
            bound,
            &old::canonical_bytes(&old_app(&app)).unwrap(),
        );
    }
}

#[test]
fn closed_value_label_mutation_leaves_dynamic_label_and_kind_payloads() {
    fn labels(value: &mut Value, serial: &mut usize) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key == "label" && child.is_string() {
                        *serial += 1;
                        *child = json!(format!("Display {serial} \"\\β"));
                    } else {
                        labels(child, serial);
                    }
                }
            }
            Value::Array(items) => {
                for child in items {
                    labels(child, serial);
                }
            }
            _ => (),
        }
    }
    let mut value = complete_fixture();
    value["actions"].as_array_mut().unwrap().push(json!({"id":"extra","label":"Extra","parameters":{"label":{"kind":"text"},"kind":{"kind":"text"}},"guards":[],"steps":[{"kind":"assert","condition":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"message":"KEEP_ASSERT_LABEL"}],"ensures":[]}));
    value["views"].as_array_mut().unwrap().push(json!({"id":"extra_form","label":"Extra form","kind":{"kind":"form","action":"extra","fields":[{"parameter":"label","label":"Field"}],"defaults":{"label":{"kind":"text","value":"KEEP_DEFAULT_LABEL"},"kind":{"kind":"text","value":"KEEP_DEFAULT_KIND"}}},"actions":[],"keys":[]}));
    value["views"][0]["actions"].as_array_mut().unwrap().push(json!({"id":"extra_binding","label":"Extra binding","placement":"toolbar","action":"extra","arguments":{"label":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"KEEP_ARGUMENT_LABEL"}},"kind":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"KEEP_ARGUMENT_KIND"}}},"enabled":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}}));
    let mut serial = 0;
    labels(&mut value, &mut serial);
    assert!(serial > 20);
    let c = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&c, Some(true));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_tag_probe_finishes_before_payload_or_fallback() {
    let mut value = fixture::minimal();
    value["observables"][0]["value"] = json!({"kind":"field","record":{"kind":"literal","value_type":{"kind":"reference","entity":"row"},"value":{"kind":"reference","entity":"row","record":"r1"}},"field":"name"});
    let good = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    for key_mismatch in [false, true] {
        let mut source = value.clone();
        let object = source["observables"][0]["value"].as_object_mut().unwrap();
        if key_mismatch {
            let tag = object.remove("kind").unwrap();
            object.insert("junk".into(), tag);
        } else {
            object.insert("kind".into(), json!("fielx"));
        }
        let mut c = good.clone();
        c.source_bytes = serde_json::to_vec(&source).unwrap();
        parity(&c, Some(false));
        let mut work = Work::default();
        assert!(source_result(&c, &mut work).is_err());
        let start = work
            .calls
            .windows(4)
            .position(|w| w == [96, 149972, 6, 12])
            .unwrap();
        let end = start + work.calls[start..].iter().position(|&n| n == 0).unwrap();
        // Field has three sorted members. kind is index1, so both the remaining
        // source entry and terminal cost6 after remembering the mismatch.
        let signature: &[u64] = if key_mismatch {
            &[3, 5, 8, 6, 6, 4, 2]
        } else {
            &[12, 4, 3, 5, 10, 2, 6, 6, 4, 2]
        };
        let hits: Vec<_> = work.calls[start..end]
            .windows(signature.len())
            .enumerate()
            .filter_map(|(i, w)| (w == signature).then_some(start + i))
            .collect();
        assert_eq!(hits.len(), 1);
        let probe_end = hits[0] + signature.len() - 3;
        for index in [probe_end - 1, probe_end, probe_end + 1] {
            for cause in [
                WorkFailure::Cancelled,
                WorkFailure::BudgetExhausted("boundary refusal"),
            ] {
                reset_validation_counts();
                let mut refused = Work {
                    refuse: Some(index + 1),
                    cause: Some(cause.clone()),
                    ..Work::default()
                };
                assert_eq!(
                    source_result(&c, &mut refused),
                    Err(ContractWorkError::Resource(cause))
                );
                assert_eq!(refused.calls, &work.calls[..=index]);
                assert_eq!(validation_counts().apps, 0);
            }
        }
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_full_miss_identity_selection_and_publication_refusals() {
    let mut value = fixture::minimal();
    value["views"][0]["kind"]
        .as_object_mut()
        .unwrap()
        .remove("selection");
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    parity(&capture, Some(false));
    let before = serde_json::to_vec(&capture).unwrap();
    let data = DataSnapshot::empty("project", &capture.program).unwrap();
    let mut trace = Work::default();
    reset_validation_counts();
    source_result(&capture, &mut trace).unwrap();
    assert_eq!(
        (validation_counts().captures, validation_counts().apps),
        (1, 1)
    );
    let unique = |start: usize, end: usize, signature: &[u64]| {
        let matches: Vec<_> = trace.calls[start..end]
            .windows(signature.len())
            .enumerate()
            .filter_map(|(i, w)| (w == signature).then_some(start + i))
            .collect();
        assert_eq!(matches.len(), 1, "{signature:?} in {start}..{end}");
        matches[0]
    };
    let source = unique(0, trace.calls.len(), &[0, 96, 248]);
    let program = unique(source, trace.calls.len(), &[0, 96, 253]);
    let semantic = unique(program, trace.calls.len(), &[0, 96, 298]);
    let source_end = unique(source, program, &[320, 2144, 12, 412, 0]) + 4;
    let program_end = unique(program, semantic, &[320, 2144, 12, 412, 0]) + 4;
    let semantic_end = unique(semantic, trace.calls.len(), &[320, 2144, 12, 412, 0]) + 4;
    let select1 = source_end + 8;
    let select2 = program_end + 4;
    assert_eq!(
        &trace.calls[source_end + 1..select1 + 3],
        &[3, 5, 128, 3, 5, 128, 1, 28, 0, 358]
    );
    assert_eq!(
        &trace.calls[program_end + 1..select2 + 2],
        &[3, 5, 128, 28, 1]
    );
    assert_eq!(&trace.calls[semantic_end + 1..], &[3, 5, 128, 1]);
    let count = unique(0, source, &[96, 2, 320, 1855, 8, 260, 5, 101, 16343, 0]) + 8;
    // The miss retains full typed equality after V1's count-success. It starts
    // with its own descriptor selector1, not the hit's equality outcome1.
    let equality = count + 5;
    assert_eq!(&trace.calls[count + 2..=equality], &[3, 1, 5, 1]);
    assert_eq!(trace.calls[equality], 1);
    assert!(source > equality + 2);
    assert_eq!(trace.calls[program - 1], 0);
    assert_eq!(trace.calls[semantic - 1], 0);
    let mut all = Work::default();
    with_capture_validation(&capture, &data, &mut all, |ctx| -> ContractWorkResult<()> {
        ctx.ensure_source_and_data()
    })
    .unwrap();
    assert_eq!(&all.calls[4..4 + trace.calls.len()], trace.calls.as_slice());
    let publication = 4 + trace.calls.len();
    assert_eq!(all.calls[publication], 5);
    let mut selected = BTreeSet::from([
        count,
        count + 1,
        equality,
        source_end - 1,
        source_end,
        select1,
        select1 + 1,
        select1 + 2,
        program - 2,
        program - 1,
        program_end - 1,
        program_end,
        select2,
        select2 + 1,
        semantic - 2,
        semantic - 1,
        semantic_end - 1,
        semantic_end,
    ]);
    // Every retained typed-equality descriptor callback, not just its header.
    selected.extend(equality..source);
    selected.extend(select2 + 1..semantic); // Complete retained clone/clear/output continuation.
    for index in selected
        .into_iter()
        .map(|i| i + 4)
        .chain(std::iter::once(publication))
    {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("miss boundary"),
        ] {
            let mut work = Work {
                refuse: Some(index + 1),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            let result = with_capture_validation(
                &capture,
                &data,
                &mut work,
                |ctx| -> ContractWorkResult<()> {
                    assert_eq!(
                        ctx.ensure_source_and_data(),
                        Err(ContractWorkError::Resource(cause.clone()))
                    );
                    assert!(ctx.facts.is_none());
                    Ok(())
                },
            );
            assert_eq!(result, Err(ContractWorkError::Resource(cause)));
            assert_eq!(work.calls, &all.calls[..=index]);
        }
    }
    for (index, next) in [
        (4 + select1, 4 + select1 + 2),
        (4 + select2, 4 + select2 + 1),
    ] {
        let prefix = all.calls[..index]
            .iter()
            .try_fold(0u64, |n, &x| n.checked_add(x))
            .unwrap();
        for (allowance, refused) in [(27u64, index), (28u64, next)] {
            let mut work = Work {
                left: Some(prefix.checked_add(allowance).unwrap()),
                ..Work::default()
            };
            let expected =
                ContractWorkError::Resource(WorkFailure::BudgetExhausted("app length test fuel"));
            let result = with_capture_validation(
                &capture,
                &data,
                &mut work,
                |ctx| -> ContractWorkResult<()> {
                    assert_eq!(ctx.ensure_source_and_data(), Err(expected.clone()));
                    assert!(ctx.facts.is_none());
                    Ok(())
                },
            );
            assert_eq!(result, Err(expected));
            assert_eq!(work.calls, &all.calls[..=refused]);
            assert_eq!(work.left, Some(if allowance == 27 { 27 } else { 0 }));
        }
    }
    assert_eq!(serde_json::to_vec(&capture).unwrap(), before);
}

#[test]
fn closed_value_callers_and_helper_scopes_remain_closed() {
    // This intentionally is a source drift guard, not proof of runtime parity.
    // Count every production-source reference, including imports/aliases, so a
    // new caller cannot hide behind the existing supported call signatures.
    fn references(
        dir: &std::path::Path,
        root: &std::path::Path,
        name: &str,
        found: &mut BTreeMap<String, usize>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                references(&path, root, name, found);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("rs")
                || path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with("_tests.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let count = source
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|token| *token == name)
                .count();
            if count != 0 {
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                    count,
                );
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (name, expected) in [
        (
            "validate_closed_value_capture",
            BTreeMap::from([
                ("product_contract.rs".into(), 1),
                ("product_contract/capture_canonical.rs".into(), 1),
                ("product_contract/capture_closed_value.rs".into(), 1),
            ]),
        ),
        (
            "convert_app",
            BTreeMap::from([
                ("product_contract/capture_typed.rs".into(), 2),
                ("product_contract/capture_closed_value.rs".into(), 1),
            ]),
        ),
    ] {
        let mut actual = BTreeMap::new();
        references(&root, &root, name, &mut actual);
        assert_eq!(actual, expected, "{name} caller/import closure drift");
    }
    assert_closed_source_bodies(include_str!("../product_contract.rs"), include_str!("capture_closed_value.rs"), include_str!("capture_typed.rs"), include_str!("capture_canonical.rs"));
}

fn assert_closed_source_bodies(parent: &str, closed: &str, typed: &str, canonical: &str) {
    // Current bounded helper bodies contain no braces inside literal strings.
    // Balance complete lexical bodies rather than accepting declaration presence.
    fn body(source: &str, signature: &str) -> std::ops::Range<usize> {
        let start = source.find(signature).unwrap();
        assert_eq!(source.matches(signature).count(), 1);
        let open = start + source[start..].find('{').unwrap();
        let mut depth = 0usize;
        for (offset, byte) in source.as_bytes()[open..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return open..open + offset + 1;
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed body: {signature}")
    }
    let parent = source_guard_text(parent);
    let prefix = "fn validate_audited(&self,a:Audit<'_>)->Result<capture_validation::CaptureFacts>";
    let range = body(&parent, prefix);
    let method = &parent[range];
    let call="let actual: AppDefinition = if a.enabled() {return capture_canonical::validate_closed_value_capture(self,a)} else {parse(&self.source_bytes)?};";
    assert!(method.contains(call));
    assert_eq!(
        method[..method.find(call).unwrap()].trim(),
        r#"{
        a.pay(8)?;
        #[cfg(test)]
        count_validation(|counts| counts.captures += 1);
        self.binding.validate_audited(a)?;
        a.version(self.artifact.version)?;
        a.require(self.artifact.kind == ArtifactKind::GeneratedApp
            && a.eq_str(&self.artifact.language, LANGUAGE_VERSION)?, "unsupported executable artifact")?;
        a.require(self.source_bytes.len() <= MAX_WIRE_BYTES, "source exceeds byte limit")?;"#
    );
    let closed = source_guard_text(closed);
    let validate = body(
        &closed,
        "pub(in super::super) fn validate_closed_value_capture(",
    );
    let hit_start = validate.start
        + closed[validate.clone()]
            .find("if equal {\n        fn clear_field")
            .unwrap();
    let hit = body(&closed[hit_start..], "if equal {\n        fn clear_field");
    let hit = (hit_start + hit.start)..(hit_start + hit.end);
    let call_pos = closed
        .find("capture_typed::convert_app(value, a)?")
        .unwrap();
    assert!(call_pos > hit.end && call_pos < validate.end);
    assert!(closed[hit.end..call_pos].contains("// False is not a validation result"));
    let production_end = closed.find("// Test-only scoped capture").unwrap();
    for (offset, _) in closed[..production_end].match_indices("fn ") {
        let tail = &closed[offset + 3..];
        if tail.starts_with("validate_closed_value_capture(") {
            continue;
        }
        assert!(
            offset > validate.start && offset < validate.end,
            "helper escaped capture body"
        );
        if tail.starts_with("rel_") || tail.starts_with("tag_matches(") {
            assert!(
                offset < hit.start,
                "relation helper escaped comparison scope"
            );
        } else {
            assert!(
                offset > hit.start && offset < hit.end,
                "mutation/output helper escaped hit scope"
            );
        }
    }
    let typed = source_guard_text(typed);
    let parse = body(&typed, "pub(super) fn parse_app(");
    assert!(typed[parse].contains("convert_app(value, a)"));
    assert!(typed.contains(
        "pub(super) fn convert_app(value: JsonValue, a: Audit<'_>) -> Result<AppDefinition>"
    ));
    assert!(source_guard_text(canonical).contains(
        "mod closed_value;\npub(super) use closed_value::validate_closed_value_capture;"
    ));
}

#[test]
#[cfg(target_pointer_width = "64")]
fn closed_value_each_fresh_output_callback_refuses_before_publication() {
    let value = complete_fixture();
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap();
    let before = serde_json::to_vec(&capture).unwrap();
    let data = DataSnapshot::empty("project", &capture.program).unwrap();
    let mut trace = Work::default();
    source_result(&capture, &mut trace).unwrap();
    let starts: Vec<_> = trace
        .calls
        .windows(6)
        .enumerate()
        .filter_map(|(i, w)| (w == [0, 96, 170873, 358, 1, 1]).then_some(i))
        .collect();
    assert_eq!(starts.len(), 2);
    let mut boundaries = Vec::new();
    for (ordinal, signature) in [[0, 96, 253], [0, 96, 298]].into_iter().enumerate() {
        let hits: Vec<_> = trace
            .calls
            .windows(3)
            .enumerate()
            .filter_map(|(i, w)| (w == signature).then_some(i))
            .collect();
        assert_eq!(hits.len(), 1);
        let hash = hits[0];
        let success_poll = if ordinal == 0 { hash - 3 } else { hash - 1 };
        assert!(starts[ordinal] < success_poll - 1);
        assert!(trace.calls[success_poll - 1] > 0);
        assert_eq!(trace.calls[success_poll], 0);
        if ordinal == 0 { assert_eq!(&trace.calls[success_poll+1..hash], &[128,5]); }
        boundaries.extend(
            (starts[ordinal]..hash).map(|index| (index, ordinal + usize::from(index >= success_poll))),
        );
    }
    eprintln!(
        "fresh output exhaustive refusal callbacks={}",
        boundaries.len()
    );
    // Includes each setup96/root170873/header358 and both mode flags, every
    // actual Value descriptor/selector/reserve callback, native and success0.
    for (index, outputs_before_refusal) in boundaries {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("fresh output boundary"),
        ] {
            let mut work = Work {
                refuse: Some(index + 5),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            closed_value::native_outputs::begin();
            let result = with_capture_validation(
                &capture,
                &data,
                &mut work,
                |ctx| -> ContractWorkResult<()> {
                    assert_eq!(
                        ctx.ensure_source_and_data(),
                        Err(ContractWorkError::Resource(cause.clone()))
                    );
                    assert!(ctx.facts.is_none());
                    Ok(())
                },
            );
            assert_eq!(
                closed_value::native_outputs::take().len(),
                outputs_before_refusal
            );
            assert_eq!(result, Err(ContractWorkError::Resource(cause)));
            assert_eq!(&work.calls[4..], &trace.calls[..=index]);
        }
    }
    assert_eq!(serde_json::to_vec(&capture).unwrap(), before);
}

#[test]
fn completed_semantic_shape_matches_fresh_descriptor_for_label_bytes() {
    let mut labels = vec![String::new(), "éβ🦀\"\\\n\r\t\u{8}\u{c}".into(),
        (0u8..=31).map(char::from).collect::<String>()];
    for n in [1, 31, 63, 127, 128, 129, 255, 256, 257, 511, 1023] {
        labels.push("x".repeat(n));
    }
    let base = fixture::minimal();
    let c = CapturedProgram::capture(&serde_json::to_vec(&base).unwrap(), "project", Producer::UserAuthored, None).unwrap();
    let without_label = serde_json::to_vec(&c.program).unwrap().len() - c.program.label.len();
    let boundary = (without_label + 2).next_power_of_two();
    for target in [boundary - 1, boundary, boundary + 1] {
        labels.push("g".repeat(target - without_label - 1)); // x prefix below
    }
    for label in labels {
        let mut value = fixture::minimal();
        value["label"] = json!(format!("x{label}"));
        let c = CapturedProgram::capture(&serde_json::to_vec(&value).unwrap(), "project", Producer::UserAuthored, None).unwrap();
        closed_value::semantic_shapes::begin();
        parity(&c, Some(true));
        let metrics = closed_value::semantic_shapes::take();
        assert_eq!(metrics.len(), 2, "only producer and completed semantic descriptors");
        let mut app = c.program.clone();
        app.clear_display_labels_audited(Audit::none()).unwrap();
        let semantic = serde_json::to_value(app).unwrap();
        for (actual, value) in metrics.iter().zip([serde_json::to_value(&c.program).unwrap(), semantic]) {
            let mut fresh = Shape::new(Audit::none()).unwrap();
            fresh.canonical = false;
            fresh.simple_app = true;
            value.serialize(&mut fresh).unwrap();
            assert_eq!(*actual, [fresh.bytes, fresh.nodes, fresh.pairs, fresh.native, fresh.escaped, fresh.digest_copies]);
        }
    }
}

#[test]
fn completed_semantic_shape_new_packets_keep_first_cause() {
    let mut value = fixture::minimal();
    value["label"] = json!("\"\\\né🦀\u{1}");
    let c = CapturedProgram::capture(&serde_json::to_vec(&value).unwrap(), "project", Producer::UserAuthored, None).unwrap();
    let mut trace = Work::default();
    source_result(&c, &mut trace).unwrap();
    let start = trace.calls.windows(6).position(|w| w == [0, 96, 170873, 358, 1, 1]).unwrap();
    for index in start..trace.calls.len() {
        for cause in [WorkFailure::Cancelled, WorkFailure::BudgetExhausted("completed shape") ] {
            let mut work = Work { refuse: Some(index + 1), cause: Some(cause.clone()), ..Work::default() };
            assert_eq!(source_result(&c, &mut work), Err(ContractWorkError::Resource(cause)));
            assert_eq!(work.calls, trace.calls[..=index]);
        }
    }
    let total = trace.calls.iter().sum();
    let mut exact = Work { left: Some(total), ..Work::default() };
    source_result(&c, &mut exact).unwrap();
    assert_eq!(exact.left, Some(0));
    let mut short = Work { left: Some(total - 1), ..Work::default() };
    assert!(matches!(source_result(&c, &mut short), Err(ContractWorkError::Resource(_))));
}

#[test]
fn completed_semantic_shape_delta_arithmetic_refuses_before_second_output() {
    let mut value = fixture::minimal();
    value["label"] = json!("x\n");
    let c = CapturedProgram::capture(&serde_json::to_vec(&value).unwrap(), "project", Producer::UserAuthored, None).unwrap();
    for fault in ["bytes", "native", "escaped", "multiply", "add"] {
        closed_value::semantic_shapes::fault(fault);
        closed_value::native_outputs::begin();
        let mut work = Work::default();
        assert!(matches!(source_result(&c, &mut work), Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted(_)))));
        assert_eq!(closed_value::native_outputs::take().len(), 1);
        closed_value::semantic_shapes::clear_fault();
    }
}

#[test]
fn closed_value_source_guards_accept_line_endings_and_reject_mutations() {
    let raw = [include_str!("../product_contract.rs"), include_str!("capture_closed_value.rs"), include_str!("capture_typed.rs"), include_str!("capture_canonical.rs"), include_str!("../../tests/support/capture_closed_schema.txt")];
    for newline in ["\n", "\r\n"] {
        let texts = raw.map(|s| s.replace("\r\n", "\n").replace('\n', newline));
        assert_closed_schema(&texts[0], &texts[4]);
        assert_closed_source_bodies(&texts[0], &texts[1], &texts[2], &texts[3]);
        let changed_schema = texts[4].replacen("Boolean,", "BooleanChanged,", 1);
        assert_ne!(changed_schema, texts[4]);
        assert!(std::panic::catch_unwind(|| assert_closed_schema(&texts[0], &changed_schema)).is_err());
        let changed_parent = texts[0].replacen("        self.binding.validate_audited(a)?;", "        self.binding.validate_audited(a)?; // changed", 1);
        assert_ne!(changed_parent, texts[0]);
        assert!(std::panic::catch_unwind(|| assert_closed_source_bodies(&changed_parent, &texts[1], &texts[2], &texts[3])).is_err());
    }
}
