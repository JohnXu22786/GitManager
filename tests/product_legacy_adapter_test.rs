#[path = "fixtures/tool_project_fixture.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_legacy_adapter.rs"]
mod product_legacy_adapter;
#[path = "../src/tool_decisions.rs"]
mod tool_decisions;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_runtime.rs"]
mod tool_runtime;
#[path = "../src/tool_store.rs"]
mod tool_store;

use product_contract::{AdapterError, ArtifactKind};
use product_legacy_adapter::*;
use std::{collections::BTreeMap, fs};
use tool_project::*;

#[test]
fn legacy_copy_commands_and_observations_equal_actual_runtime() {
    let snapshot = fixture::studio_order_project("legacy-project");
    let before = serde_json::to_vec(&snapshot).unwrap();
    let adapter = LegacyOrderAdapter;
    let mut run = adapter.start(&snapshot).unwrap();
    let command = ToolCommand::EditRecord {
        operation_id: "edit-notes".into(),
        expected_generation: snapshot.generation,
        record_id: "record-1".into(),
        expected_record_revision: snapshot.records[0].record_revision,
        changes: BTreeMap::from([(
            "notes".into(),
            Some(FieldValue::Text("Later legitimate work".into())),
        )]),
        occurred_at: fixture::at(2026, 10, 5, 12),
    };
    let expected =
        tool_runtime::apply_command(&snapshot, &command, fixture::at(2026, 10, 5, 12)).unwrap();
    adapter
        .apply(&mut run, &command, fixture::at(2026, 10, 5, 12))
        .unwrap();
    assert_eq!(
        serde_json::to_vec(run.snapshot()).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert_eq!(
        adapter
            .observe(&run, "record-1", fixture::date(2026, 10, 5))
            .unwrap(),
        tool_decisions::evaluate_bound_record(&expected, "record-1", fixture::date(2026, 10, 5))
            .unwrap()
    );
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
    let preserved = serde_json::to_vec(run.snapshot()).unwrap();
    assert!(adapter
        .apply(&mut run, &command, fixture::at(2026, 10, 5, 12))
        .is_err());
    assert_eq!(serde_json::to_vec(run.snapshot()).unwrap(), preserved);
}
#[test]
fn legacy_capabilities_refuse_generic_programs_without_schema_conversion() {
    let adapter = LegacyOrderAdapter;
    let capabilities = adapter.capabilities();
    assert_eq!(capabilities.artifact_kinds, vec![ArtifactKind::LegacyOrder]);
    for feature in ["generic_actions", "relations", "selection", "external_web"] {
        assert!(matches!(
            adapter.require_capability(feature),
            Err(AdapterError::Unsupported(_))
        ));
    }
    let snapshot = fixture::studio_order_project("legacy-project");
    let mut invalid = snapshot.clone();
    invalid.format_version += 1;
    assert!(adapter.start(&invalid).is_err());
    assert!(adapter.require_capability("legacy_record_commands").is_ok());
}
#[test]
fn opening_and_previewing_legacy_storage_preserves_records_decisions_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = fixture::studio_order_project("legacy-project");
    let store =
        tool_store::ProjectStore::create(dir.path().join("legacy"), &snapshot, "initial").unwrap();
    fn files(path: &std::path::Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(
            root: &std::path::Path,
            dir: &std::path::Path,
            out: &mut BTreeMap<PathBuf, Vec<u8>>,
        ) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(root, &path, out)
                } else {
                    out.insert(
                        path.strip_prefix(root).unwrap().into(),
                        fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut out = BTreeMap::new();
        visit(path, path, &mut out);
        out
    }
    use std::path::PathBuf;
    let before = files(store.root());
    let loaded = match store.load().unwrap() {
        tool_store::StoreLoad::Writable(p) => p,
        _ => panic!(),
    };
    let run = LegacyOrderAdapter.start(&loaded).unwrap();
    LegacyOrderAdapter
        .observe(&run, "record-1", fixture::date(2026, 10, 5))
        .unwrap();
    let reopened = tool_store::ProjectStore::open(store.root()).unwrap();
    assert!(matches!(
        reopened.load().unwrap(),
        tool_store::StoreLoad::Writable(_)
    ));
    assert_eq!(before, files(store.root()));
    assert_eq!(run.snapshot().records, loaded.records);
    assert_eq!(run.snapshot().decisions, loaded.decisions);
    assert_eq!(run.snapshot().event_history, loaded.event_history);
}
