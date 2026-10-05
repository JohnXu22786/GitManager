use crate::product_contract::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct SourceDelta {
    pub behavioral_loci: Vec<SourceLocus>,
    pub changed_symbols: BTreeSet<String>,
    pub baseline: Digest,
    pub candidate: Digest,
}
fn strip_labels(section: &str, value: &mut Value) {
    if let Some(map) = value.as_object_mut() {
        map.remove("label");
    }
    let mut clear = |path: &str| {
        if let Some(items) = value.pointer_mut(path).and_then(Value::as_array_mut) {
            for item in items {
                if let Some(map) = item.as_object_mut() {
                    map.remove("label");
                }
            }
        }
    };
    match section {
        "entities" => clear("/fields"),
        "outputs" => clear("/columns"),
        "views" => {
            clear("/actions");
            clear("/kind/columns");
            clear("/kind/controls");
            clear("/kind/fields");
        }
        _ => {}
    }
}
fn symbols(source: &CapturedProgram) -> Result<BTreeMap<String, (String, Value)>, AdapterError> {
    let value =
        serde_json::to_value(&source.program).map_err(|e| AdapterError::Failed(e.to_string()))?;
    let mut nodes = BTreeMap::new();
    for section in [
        "entities",
        "state",
        "actions",
        "outputs",
        "observables",
        "views",
    ] {
        for (index, node) in value[section].as_array().into_iter().flatten().enumerate() {
            if let Some(id) = node["id"].as_str() {
                let mut node = node.clone();
                strip_labels(section, &mut node);
                nodes.insert(
                    format!("{section}/{id}"),
                    (format!("/{section}/{index}"), node),
                );
            }
        }
    }
    nodes.insert(
        "initial_view".into(),
        ("/initial_view".into(), value["initial_view"].clone()),
    );
    Ok(nodes)
}
pub fn analyze_delta(
    before: &CapturedProgram,
    after: &CapturedProgram,
) -> Result<SourceDelta, AdapterError> {
    before.validate()?;
    after.validate()?;
    let old = symbols(before)?;
    let new = symbols(after)?;
    let mut changed = BTreeSet::new();
    let mut loci = vec![];
    for key in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
        if old.get(key).map(|v| &v.1) != new.get(key).map(|v| &v.1) {
            changed.insert(key.clone());
            for (source, nodes) in [(before, &old), (after, &new)] {
                if let Some((pointer, _)) = nodes.get(key) {
                    loci.push(SourceLocus {
                        relative_path: source.binding.program_path.clone(),
                        raw_digest: source.artifact.raw_digest.clone(),
                        pointer: pointer.clone(),
                    });
                }
            }
        }
    }
    Ok(SourceDelta {
        behavioral_loci: loci,
        changed_symbols: changed,
        baseline: before.binding.identity()?,
        candidate: after.binding.identity()?,
    })
}
fn references(value: &Value, refs: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                let section = match key.as_str() {
                    "action" | "on_change" => Some("actions"),
                    "state" => Some("state"),
                    "entity" => Some("entities"),
                    "view" => Some("views"),
                    "output" => Some("outputs"),
                    _ => None,
                };
                if let (Some(section), Some(id)) = (section, v.as_str()) {
                    refs.insert(format!("{section}/{id}"));
                }
                references(v, refs);
            }
        }
        Value::Array(a) => {
            for v in a {
                references(v, refs)
            }
        }
        _ => {}
    }
}
pub(super) fn relevant(
    delta: &SourceDelta,
    before: &CapturedProgram,
    after: &CapturedProgram,
    h: &ChoiceHypothesis,
) -> Result<bool, AdapterError> {
    let mut reachable = BTreeSet::from([
        format!("actions/{}", h.action),
        format!("observables/{}", h.observable),
    ]);
    let scenario = h.scenario()?;
    reachable.insert(format!("views/{}", scenario.session.view));
    for input in &scenario.inputs {
        if let SemanticInput::Navigate { view }
        | SemanticInput::Control { view, .. }
        | SemanticInput::Activate { view, .. }
        | SemanticInput::Submit { view, .. } = input
        {
            reachable.insert(format!("views/{view}"));
        }
    }
    let old = symbols(before)?;
    let new = symbols(after)?;
    loop {
        let prior = reachable.len();
        for key in reachable.clone() {
            for nodes in [&old, &new] {
                if let Some((_, value)) = nodes.get(&key) {
                    references(value, &mut reachable);
                }
            }
        }
        if reachable.len() == prior {
            break;
        }
    }
    let overlaps = |a: &str, b: &str| {
        a == b
            || a.strip_prefix(b).is_some_and(|s| s.starts_with('/'))
            || b.strip_prefix(a).is_some_and(|s| s.starts_with('/'))
    };
    for key in delta.changed_symbols.intersection(&reachable) {
        for (source, nodes) in [(before, &old), (after, &new)] {
            if let Some((pointer, _)) = nodes.get(key) {
                if h.sources.iter().any(|l| {
                    l.raw_digest == source.artifact.raw_digest
                        && l.relative_path == source.binding.program_path
                        && overlaps(&l.pointer, pointer)
                }) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
