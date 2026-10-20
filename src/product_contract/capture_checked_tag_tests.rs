//! Independent closed-schema callback oracle. No production hook or receipt.
use super::app_length_tests::{
    capture_minimal, complete_fixture, old, old_app, source_result, Work,
};
use super::*;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    path: String,
    operation: &'static str,
    units: u64,
    removed: bool,
}
struct Model {
    graph: Value,
    events: Vec<Event>,
    keep_tag: bool,
    removed: bool,
    reached: BTreeMap<String, String>,
}
impl Model {
    fn new(keep_tag: bool) -> Self {
        Self {
            graph: serde_json::from_str(include_str!(
                "../../tests/support/capture_closed_graph.json"
            ))
            .unwrap(),
            events: vec![],
            keep_tag,
            removed: false,
            reached: BTreeMap::new(),
        }
    }
    fn pay(&mut self, path: &str, operation: &'static str, units: u64) {
        self.events.push(Event {
            path: path.into(),
            operation,
            units,
            removed: self.removed,
        });
    }
    fn result(&mut self, path: &str, value: bool) -> bool {
        self.pay(path, "result", 2);
        value
    }
    fn map(&mut self, path: &str, n: usize) {
        self.pay(path, "map descriptor", 8);
        self.pay(path, "map iterator", 27 * n as u64 + 8);
    }
    fn slice(&mut self, path: &str, n: usize) {
        self.pay(path, "slice arithmetic", 5);
        self.pay(path, "slice iterator", 3 * n as u64 + 2);
    }
    fn eq(&mut self, path: &str, left: &str, right: &str) -> bool {
        self.pay(path, "string lengths", 3);
        if left.len() != right.len() {
            return false;
        }
        self.pay(path, "string arithmetic", 5);
        self.pay(path, "string bytes", 2 * left.len() as u64);
        left == right
    }
    fn walk(&mut self, ty: &str, v: &Value, p: &Value, path: &str) -> bool {
        let ty = match ty {
            "Id" => "String",
            "Values" => "BTreeMap<Id, DataValue>",
            x => x,
        };
        if let Some(inner) = ty.strip_prefix("Box<") {
            self.pay(path, "box edge", 2);
            return self.walk(&inner[..inner.len() - 1], v, p, path);
        }
        if let Some(inner) = ty.strip_prefix("Option<") {
            self.pay(path, "option edge", 4);
            return self.walk(
                if p.is_null() {
                    "null"
                } else {
                    &inner[..inner.len() - 1]
                },
                v,
                p,
                path,
            );
        }
        self.pay(path, "entry", 12);
        if ty == "null" {
            self.pay(path, "null", 2);
            return self.result(path, v.is_null());
        }
        if ty == "String" {
            let Some(s) = v.as_str() else {
                return self.result(path, false);
            };
            self.pay(path, "string leaf", 4);
            let equal = self.eq(path, s, p.as_str().unwrap());
            return self.result(path, equal);
        }
        if ty == "bool" {
            let Some(s) = v.as_bool() else {
                return self.result(path, false);
            };
            self.pay(path, "boolean leaf", 4);
            return self.result(path, Some(s) == p.as_bool());
        }
        if ["i32", "i64", "u32", "usize"].contains(&ty) {
            if !v.is_number() {
                return self.result(path, false);
            }
            self.pay(path, "native number", 12);
            return self.result(path, v == p);
        }
        if ty.starts_with("Vec<") || ty.starts_with("BTreeSet<") {
            let set = ty.starts_with("BTreeSet<");
            let start = if set { 9 } else { 4 };
            let inner = &ty[start..ty.len() - 1];
            let Some(left) = v.as_array() else {
                return self.result(path, false);
            };
            let right = p.as_array().unwrap();
            self.pay(path, "container length", 8);
            if left.len() != right.len() {
                return self.result(path, false);
            }
            self.slice(path, left.len());
            if set {
                self.map(path, right.len());
            } else {
                self.slice(path, right.len());
            }
            for (i, (l, r)) in left.iter().zip(right).enumerate() {
                self.pay(path, "pair next", if set { 6 } else { 4 });
                self.pay(path, "pair child", 2);
                if !self.walk(inner, l, r, &format!("{path}/{i}")) {
                    return self.result(path, false);
                }
            }
            self.pay(path, "pair terminal", if set { 6 } else { 4 });
            return self.result(path, true);
        }
        if ty.starts_with("BTreeMap<") {
            let inner = ty
                .split_once(',')
                .unwrap()
                .1
                .trim()
                .strip_suffix('>')
                .unwrap();
            let Some(left) = v.as_object() else {
                return self.result(path, false);
            };
            let right = p.as_object().unwrap();
            self.pay(path, "container length", 8);
            if left.len() != right.len() {
                return self.result(path, false);
            }
            self.map(path, left.len());
            self.map(path, right.len());
            for ((lk, l), (rk, r)) in left.iter().zip(right) {
                self.pay(path, "map pair next", 6);
                self.pay(path, "map pair child", 4);
                if !self.eq(path, lk, rk)
                    || !self.walk(inner, l, r, &format!("{path}/{}", pointer(rk)))
                {
                    return self.result(path, false);
                }
            }
            self.pay(path, "map pair terminal", 6);
            return self.result(path, true);
        }
        let spec = self.graph["types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["type"] == ty)
            .unwrap()
            .clone();
        let tagged = spec["serde_attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.as_str().unwrap().contains("tag = \"kind\""));
        if spec["kind"] == "enum" && !tagged {
            let Some(s) = v.as_str() else {
                return self.result(path, false);
            };
            self.pay(path, "unit enum selection", 6);
            let equal = self.eq(path, s, p.as_str().unwrap());
            return self.result(path, equal);
        }
        let Some(left) = v.as_object() else {
            return self.result(path, false);
        };
        let right = p.as_object().unwrap();
        self.pay(path, "object count and selection", 8);
        let (fields, tag) = if tagged {
            let tag = p["kind"].as_str().unwrap();
            let variant = spec["variants"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["wire_name"] == tag)
                .unwrap();
            (&variant["fields"], Some(tag))
        } else {
            (&spec["fields"], None)
        };
        let mut expected: BTreeMap<&str, &str> = fields
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["name"].as_str().unwrap(),
                    f["rust_type"].as_str().unwrap(),
                )
            })
            .collect();
        if tagged {
            expected.insert("kind", "String");
        }
        if left.len() != expected.len() {
            return self.result(path, false);
        }
        if let Some(tag) = tag {
            let index = expected.keys().position(|k| *k == "kind").unwrap();
            self.pay(path, "probe entry", 6);
            self.map(path, left.len());
            let mut found = false;
            for (at, (key, child)) in left.iter().enumerate() {
                self.pay(path, "probe next", 6);
                if at == index {
                    found = self.eq(path, key, "kind")
                        && self.walk("String", child, &json!(tag), &format!("{path}/kind#probe"));
                }
            }
            self.pay(path, "probe terminal", 6);
            self.pay(path, "probe result", 4);
            if !found {
                return self.result(path, false);
            }
        }
        self.map(path, left.len());
        for ((key, child), (wanted, child_ty)) in left.iter().zip(expected) {
            let child_path = format!("{path}/{}", pointer(wanted));
            self.pay(&child_path, "field next", 8);
            if !self.eq(&child_path, key, wanted) {
                return self.result(path, false);
            }
            if let Some(tag) = tag.filter(|_| wanted == "kind") {
                self.reached
                    .insert(child_path.clone(), format!("{ty}:{tag}"));
                if self.keep_tag {
                    self.removed = true;
                    assert!(self.walk("String", child, &json!(tag), &child_path));
                    self.removed = false;
                }
            } else if !self.walk(child_ty, child, &right[wanted], &child_path) {
                return self.result(path, false);
            }
        }
        self.pay(path, "object terminal", 4);
        self.result(path, true)
    }
}
fn pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
fn model(c: &CapturedProgram, keep: bool) -> (Model, bool) {
    let source: Value = serde_json::from_slice(&c.source_bytes).unwrap();
    let retained = serde_json::to_value(&c.program).unwrap();
    let mut m = Model::new(keep);
    let equal = m.walk("AppDefinition", &source, &retained, "");
    (m, equal)
}
pub(super) fn removed_tag_units(c: &CapturedProgram) -> u64 {
    model(c, true)
        .0
        .events
        .iter()
        .filter(|e| e.removed)
        .map(|e| e.units)
        .sum()
}
fn facts(f: capture_validation::CaptureFacts) -> (usize, usize, usize, usize) {
    (
        f.total_nodes,
        f.action_prefix_nodes,
        f.canonical_len,
        f.action_count,
    )
}
fn full(c: &CapturedProgram) -> (ContractWorkResult<(usize, usize, usize, usize)>, Vec<u64>) {
    let mut w = Work::default();
    let state = WorkState::new(&mut w);
    let result = state.finish(c.validate_audited(Audit(Some(&state))).map(facts));
    (result, w.calls)
}
fn check(c: &CapturedProgram) -> (Vec<u64>, usize, Model) {
    let before = serde_json::to_vec(c).unwrap();
    let (expected, equal) = model(c, false);
    let (old_model, old_equal) = model(c, true);
    assert_eq!(equal, old_equal);
    assert_eq!(expected.reached, old_model.reached);
    assert_eq!(
        expected.events,
        old_model
            .events
            .iter()
            .filter(|e| !e.removed)
            .cloned()
            .collect::<Vec<_>>()
    );
    for path in old_model.reached.keys() {
        let leaf: Vec<_> = old_model
            .events
            .iter()
            .filter(|e| e.removed && &e.path == path)
            .map(|e| e.units)
            .collect();
        assert_eq!(leaf.len(), 6);
        assert_eq!(&leaf[..4], &[12, 4, 3, 5]);
        assert_eq!(leaf[5], 2);
    }
    let mut prefix = Work::default();
    let state = WorkState::new(&mut prefix);
    let a = Audit(Some(&state));
    a.format(107).unwrap();
    bounded_input::parse_json_bytes_with_work(&c.source_bytes, &mut |n| a.pay(n)).unwrap();
    // The fixed original stack allowance is deliberately not discounted.
    a.pay(96).unwrap();
    a.pay(149972).unwrap();
    a.pay(6).unwrap();
    let relation_in_closed = prefix.calls.len();
    let mut closed = Work::default();
    let state = WorkState::new(&mut closed);
    let closed_result =
        state.finish(validate_closed_value_capture(c, Audit(Some(&state))).map(facts));
    closed_value::native_outputs::begin();
    let (result, calls) = full(c);
    let outputs = closed_value::native_outputs::take();
    assert_eq!(result, closed_result);
    let start = calls.len() - closed.calls.len();
    assert_eq!(&calls[start..], closed.calls);
    assert_eq!(&closed.calls[..relation_in_closed], prefix.calls);
    let start = start + relation_in_closed;
    for (i, event) in expected.events.iter().enumerate() {
        assert_eq!(calls[start + i], event.units, "callback {i}: {event:?}");
    }
    assert_eq!(
        &calls[start + expected.events.len()..start + expected.events.len() + 2],
        &[0, 4]
    );
    let frozen: old::CapturedProgram = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        result.as_ref().map(|_| ()).map_err(|e| match e {
            ContractWorkError::Contract(e) => e.0.clone(),
            _ => panic!("unexpected resource"),
        }),
        frozen.validate().map_err(|e| e.0)
    );
    assert_eq!(
        c.validate().map_err(|e| e.0),
        frozen.validate().map_err(|e| e.0)
    );
    assert_eq!(serde_json::to_vec(c).unwrap(), before);
    if equal && result.is_ok() {
        let mut semantic = c.program.clone();
        semantic
            .clear_display_labels_audited(Audit::none())
            .unwrap();
        let p = old::canonical_bytes(&old_app(&c.program)).unwrap();
        let s = old::canonical_bytes(&old_app(&semantic)).unwrap();
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].0, p);
        assert_eq!(outputs[1].0, s);
        assert_ne!(outputs[0].1, outputs[1].1);
        assert_eq!(
            bytes_digest(IdentityDomain::Program, &p),
            c.artifact.program_digest
        );
        assert_eq!(
            bytes_digest(IdentityDomain::SemanticProgram, &s),
            c.artifact.semantic_digest
        );
        let disabled = c.validate_audited(Audit::none()).unwrap();
        assert_eq!(result.unwrap(), facts(disabled));
    } else {
        assert!(outputs.is_empty());
    }
    (calls, start, expected)
}
fn capture(v: &Value) -> CapturedProgram {
    CapturedProgram::capture(
        &serde_json::to_vec(v).unwrap(),
        "project",
        Producer::UserAuthored,
        None,
    )
    .unwrap()
}
#[test]
#[cfg(target_pointer_width = "64")]
fn checked_tag_complete_graph_ordered_trace() {
    let c = capture(&complete_fixture());
    let (_, _, m) = check(&c);
    let actual: BTreeSet<_> = m.reached.values().cloned().collect();
    let expected: BTreeSet<_> = m.graph["types"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| {
            s["serde_attributes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x.as_str().unwrap().contains("tag = \"kind\""))
        })
        .flat_map(|s| {
            s["variants"].as_array().unwrap().iter().map(move |v| {
                format!(
                    "{}:{}",
                    s["type"].as_str().unwrap(),
                    v["wire_name"].as_str().unwrap()
                )
            })
        })
        .collect();
    assert_eq!(expected.len(), 54);
    assert_eq!(actual, expected);
    assert!(removed_tag_units(&c) > 0);
}
#[test]
#[cfg(target_pointer_width = "64")]
fn checked_tag_misses_reached_leaf_accounting() {
    let c = capture(&complete_fixture());
    let canonical: Value = serde_json::from_slice(&c.source_bytes).unwrap();
    let (all, _) = model(&c, false);
    let mut one_per_variant = BTreeMap::new();
    for (path, kind) in &all.reached {
        one_per_variant.entry(kind).or_insert(path);
    }
    assert_eq!(one_per_variant.len(), 54);
    for path in one_per_variant.values() {
        let parent = path.strip_suffix("/kind").unwrap();
        for mode in 0..6 {
            let mut source = canonical.clone();
            let object = source.pointer_mut(parent).unwrap().as_object_mut().unwrap();
            match mode {
                0 => {
                    object.insert("kind".into(), json!("wrong"));
                }
                1 => {
                    object.insert("kind".into(), json!(false));
                }
                2 => {
                    object.remove("kind");
                }
                3 => {
                    let v = object.remove("kind").unwrap();
                    object.insert("jind".into(), v);
                }
                4 => {
                    object.insert("unknown".into(), json!(null));
                }
                _ => {
                    let v = object.remove("kind").unwrap();
                    object.insert("zzzz".into(), v);
                }
            }
            let mut bad = c.clone();
            bad.source_bytes = serde_json::to_vec(&source).unwrap();
            let (_, _, reached) = check(&bad);
            assert!(!reached.reached.contains_key(*path));
        }
    }
    // Early, middle and late source failures earn only leaves actually reached.
    let mut counts = Vec::new();
    for path in ["/actions/0/id", "/label", "/views/0/label"] {
        let mut source = canonical.clone();
        source.pointer_mut(path).unwrap().as_str().unwrap();
        *source.pointer_mut(path).unwrap() = json!("mismatch");
        let mut bad = c.clone();
        bad.source_bytes = serde_json::to_vec(&source).unwrap();
        let (_, _, m) = check(&bad);
        counts.push(m.reached.len());
    }
    assert!(counts.windows(2).all(|w| w[0] < w[1]));
    let mut escaped = c.clone();
    escaped.source_bytes = String::from_utf8(c.source_bytes.clone())
        .unwrap()
        .replace("\"kind\":", "\"k\\u0069nd\":")
        .into_bytes();
    check(&escaped);
}
#[test]
#[cfg(target_pointer_width = "64")]
fn checked_tag_surviving_refusals_keep_first_cause() {
    let c = capture_minimal();
    let data = DataSnapshot::empty("project", &c.program).unwrap();
    let (trace, start, model) = check(&c);
    let before = (
        serde_json::to_vec(&c).unwrap(),
        serde_json::to_vec(&data).unwrap(),
    );
    for index in start..start + model.events.len() + 2 {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("checked kind boundary"),
        ] {
            reset_validation_counts();
            closed_value::native_outputs::begin();
            let mut w = Work {
                refuse: Some(index + 5),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            let result =
                with_capture_validation(&c, &data, &mut w, |ctx| -> ContractWorkResult<()> {
                    assert!(ctx.ensure_source_and_data().is_err());
                    assert!(ctx.facts.is_none());
                    Ok(())
                });
            assert_eq!(result, Err(ContractWorkError::Resource(cause)));
            assert_eq!(&w.calls[4..], &trace[..=index]);
            assert_eq!(validation_counts().apps, 0);
            assert!(closed_value::native_outputs::take().is_empty());
        }
    }
    assert_eq!(
        (
            serde_json::to_vec(&c).unwrap(),
            serde_json::to_vec(&data).unwrap()
        ),
        before
    );
    // A full source-only failing tag probe must precede even a huge retained
    // payload. Refuse the surviving relation poll before complete fallback.
    let mut v = super::app_length_tests::fixture::minimal();
    v["observables"][0]["value"] = json!({"kind":"field","record":{"kind":"literal","value_type":{"kind":"reference","entity":"row"},"value":{"kind":"reference","entity":"row","record":"r1"}},"field":"name"});
    let mut small = capture(&v);
    v["observables"][0]["value"]["kind"] = json!("fielx");
    small.source_bytes = serde_json::to_vec(&v).unwrap();
    let (trace, start, m) = check(&small);
    let stop = start + m.events.len();
    let mut huge = small.clone();
    if let Expr::Field { record, .. } = &mut huge.program.observables[0].value {
        *record = Box::new(Expr::Literal {
            value_type: Type::Text,
            value: DataValue::Text {
                value: "x".repeat(2 * MAX_WIRE_BYTES),
            },
        });
    } else {
        panic!("fixture changed");
    }
    for candidate in [&small, &huge] {
        reset_validation_counts();
        let mut w = Work {
            refuse: Some(stop + 1),
            ..Work::default()
        };
        assert_eq!(
            source_result(candidate, &mut w),
            Err(ContractWorkError::Resource(WorkFailure::Cancelled))
        );
        assert_eq!(w.calls, trace[..=stop]);
        assert_eq!(validation_counts().apps, 0);
    }
}
fn assert_checked_tag_schema(source: &str) {
    let source = source.replace("\r\n", "\n");
    let macro_start = source
        .find("    macro_rules! checked_kind_field {")
        .unwrap();
    let macro_end = source[macro_start..].find("    fn rel_type(").unwrap() + macro_start;
    assert_eq!(
        &source[macro_start..macro_end],
        r#"    macro_rules! checked_kind_field {
        ($a:ident, $cursor:ident) => {{
            $a.pay(8)?;
            let Some((key, _)) = $cursor.next() else {
                relation_result!($a, false);
            };
            if !$a.eq_str(key, "kind")? {
                relation_result!($a, false);
            }
        }};
    }

"#
    );
    assert_eq!(
        source.matches("checked_kind_field!(a, cursor);").count(),
        54
    );
    assert_eq!(
        source.matches("if !tag_matches(m, index, tag, a)?").count(),
        6
    );
    assert!(!source.contains("rel_string(source_child, \""));
    let graph = Model::new(false).graph;
    for spec in graph["types"].as_array().unwrap().iter().filter(|s| {
        s["serde_attributes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x.as_str().unwrap().contains("tag = \"kind\""))
    }) {
        let ty = spec["type"].as_str().unwrap();
        let signature = format!("    fn rel_{}(", ty.to_lowercase());
        let tail = source.split_once(&signature).unwrap().1;
        let body = tail.split("\n    fn ").next().unwrap();
        let (table, arms) = body.split_once("        match p {\n").unwrap();
        assert!(table.ends_with("        if !tag_matches(m, index, tag, a)? {\n            relation_result!(a, false);\n        }\n"));
        assert_eq!(
            arms.matches("checked_kind_field!(a, cursor);").count(),
            spec["variants"].as_array().unwrap().len()
        );
        for variant in spec["variants"].as_array().unwrap() {
            let name = variant["name"].as_str().unwrap();
            let tag = variant["wire_name"].as_str().unwrap();
            let keys = variant["canonical_keys"].as_array().unwrap();
            let index = keys.iter().position(|k| k == "kind").unwrap();
            let pattern = if variant["fields"].as_array().unwrap().is_empty() {
                ""
            } else {
                " { .. }"
            };
            assert!(table.contains(&format!(
                "{ty}::{name}{pattern} => ({}, {index}, \"{tag}\"),",
                keys.len()
            )));
            let arm = arms
                .split_once(&format!("{ty}::{name}"))
                .unwrap()
                .1
                .split(&format!("{ty}::"))
                .next()
                .unwrap();
            assert_eq!(arm.matches("checked_kind_field!(a, cursor);").count(), 1);
        }
    }
}

#[test]
fn checked_tag_schema_table_and_scope_drift() {
    let source = include_str!("capture_closed_value.rs").replace("\r\n", "\n");
    // Exercise both possible checkout encodings regardless of the host OS.
    for checkout_source in [source.clone(), source.replace('\n', "\r\n")] {
        // Normalize before synthesis so a CRLF checkout cannot become CRCRLF.
        let source = checkout_source.replace("\r\n", "\n");
        for source in [source.clone(), source.replace('\n', "\r\n")] {
            assert_checked_tag_schema(&source);
            for changed in [
                source.replacen(
                    "Type::Boolean => (1, 0, \"boolean\")",
                    "Type::Boolean => (1, 0, \"integer\")",
                    1,
                ),
                source.replacen("checked_kind_field!(a, cursor);", "", 1),
                source.replacen("if !tag_matches(m, index, tag, a)?", "if false", 1),
                source.replacen(
                    "let Some((key, _)) = $cursor.next()",
                    "let Some((key, _)) = $cursor.next_back()",
                    1,
                ),
                source.replacen(
                    "if !$a.eq_str(key, \"kind\")?",
                    "if !$a.eq_str(key, \"type\")?",
                    1,
                ),
            ] {
                assert_ne!(changed, source);
                assert!(std::panic::catch_unwind(|| assert_checked_tag_schema(&changed)).is_err());
            }
        }
    }
}
