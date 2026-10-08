//! Synthetic saved-output checks, not native-dialog or live-provider acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_export.rs"]
mod product_export;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_export::*;
use product_store::*;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

struct Saved {
    _temp: tempfile::TempDir,
    root: PathBuf,
    store: ProductStore,
}
impl Saved {
    fn new(format: OutputFormat, names: &[&str], twice: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let mut source = organizer();
        // Query defaults to generated record-ID order. Declare the row order
        // this fixture expects rather than depending on hash ordering.
        source["actions"][1]["steps"][0]["items"]["sort"] =
            json!([{"value":field(var("item"),"name"),"descending":false}]);
        source["outputs"][0]["format"] = json!(format);
        source["outputs"][0]["columns"] = json!([
            {"id":"z_name","label":"Name","value_type":{"kind":"text"}},
            {"id":"-area","label":"Area","value_type":{"kind":"text"}},
            {"id":"a_note","label":"Note","value_type":{"kind":"text"}}
        ]);
        source["actions"][2]["steps"][0]["columns"] = json!({
            "z_name":field(var("person"),"name"),
            "-area":field(var("person"),"area"),
            "a_note":text("你好,\"quoted\"")
        });
        if twice {
            let step = source["actions"][2]["steps"][0].clone();
            source["actions"][2]["steps"]
                .as_array_mut()
                .unwrap()
                .push(step);
        }
        let store = ProductStore::create(root.join("tool"), &capture(source), 20000).unwrap();
        let saved = Self {
            _temp: temp,
            root,
            store,
        };
        for (index, name) in names.iter().enumerate() {
            saved.apply(
                &format!("add-{index}"),
                invoke(
                    "add_person",
                    args(&[("name", string(name)), ("area", string("-north"))]),
                ),
            );
        }
        saved.apply("select", invoke("collect", Values::new()));
        saved.apply("emit-first", invoke("export_people", Values::new()));
        saved
    }
    fn snapshot(&self) -> ProjectSnapshot {
        self.store.load().unwrap()
    }
    fn apply(&self, id: &str, input: SemanticInput) -> ProjectSnapshot {
        self.store
            .apply(
                self.snapshot().revision,
                id,
                &input,
                RuntimeLimits::default(),
            )
            .unwrap()
    }
    fn prepare(&self, index: usize) -> PreparedExport {
        let snapshot = self.snapshot();
        PreparedExport::capture(&self.store, &basis(&snapshot), selection(&snapshot, index))
            .unwrap()
    }
}
fn basis(snapshot: &ProjectSnapshot) -> Digest {
    canonical_digest(IdentityDomain::Evidence, snapshot).unwrap()
}
fn selection(snapshot: &ProjectSnapshot, inventory_index: usize) -> ArtifactSelection {
    ArtifactSelection {
        inventory_index,
        expected_digest: snapshot.artifacts[inventory_index].digest.clone(),
    }
}
fn verified(outcome: ExportOutcome) -> ExportReceipt {
    match outcome {
        ExportOutcome::Verified(receipt) => receipt,
        other => panic!("expected verified publication, got {other:?}"),
    }
}
fn publish(prepared: PreparedExport, path: &Path) -> ExportOutcome {
    prepared
        .select_destination(path)
        .unwrap()
        .publish(&AtomicBool::new(false))
}

#[test]
fn csv_export_preserves_original_order_unicode_and_encoder_formula_prefixes() {
    let saved = Saved::new(OutputFormat::Csv, &["=SUM(1,2)", "Zoë\n李"], false);
    let snapshot = saved.snapshot();
    assert_eq!(
        saved.store.runtime_view().unwrap().artifacts,
        snapshot.artifacts
    );
    let expected = "z_name,'-area,a_note\r\n\"'=SUM(1,2)\",'-north,\"你好,\"\"quoted\"\"\"\r\n\"Zoë\n李\",'-north,\"你好,\"\"quoted\"\"\"\r\n".as_bytes();
    assert_eq!(snapshot.artifacts[0].bytes, expected);
    let path = saved.root.join("chosen.csv");
    let receipt = verified(publish(saved.prepare(0), &path));
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(receipt.destination, path);
    assert_eq!(receipt.output.row_count, 2);
    assert_eq!(receipt.output.byte_count, expected.len());
    assert_eq!(
        receipt.output.bytes_digest,
        LocalArtifact::bytes_identity(expected)
    );
    assert_eq!(receipt.output.snapshot_basis, basis(&snapshot));
    assert_eq!(receipt.output.snapshot_revision, snapshot.revision);
    assert_eq!(
        saved.snapshot(),
        snapshot,
        "export must not execute or commit a business action"
    );
}

#[test]
fn json_and_empty_outputs_use_only_the_existing_emitted_bytes() {
    for names in [vec!["=SUM(1,2)", "Zoë\n李"], vec![]] {
        for format in [OutputFormat::Csv, OutputFormat::Json] {
            let saved = Saved::new(format, &names, false);
            let artifact = saved.snapshot().artifacts.remove(0);
            let path = saved.root.join("user-chosen.data");
            let receipt = verified(publish(saved.prepare(0), &path));
            assert_eq!(fs::read(path).unwrap(), artifact.bytes);
            assert_eq!(receipt.output.format, format);
            assert_eq!(receipt.output.row_count, names.len());
            if names.is_empty() {
                assert_eq!(
                    artifact.bytes,
                    if format == OutputFormat::Json {
                        b"[]".as_slice()
                    } else {
                        b"z_name,'-area,a_note\r\n".as_slice()
                    }
                );
            } else if format == OutputFormat::Json {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&artifact.bytes).unwrap(),
                    json!([
                        {"z_name":"=SUM(1,2)","-area":"-north","a_note":"你好,\"quoted\""},
                        {"z_name":"Zoë\n李","-area":"-north","a_note":"你好,\"quoted\""}
                    ])
                );
                assert!(artifact.bytes.starts_with(b"[{\"-area\":"));
            }
        }
    }
}

#[test]
fn identical_digests_resolve_the_selected_event_and_output_ordinal() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], true);
    saved.apply("tomorrow", SemanticInput::AdvanceClock { days: 1 });
    let snapshot = saved.apply("emit-second", invoke("export_people", Values::new()));
    assert_eq!(snapshot.artifacts.len(), 4);
    assert!(snapshot
        .artifacts
        .windows(2)
        .all(|a| a[0].digest == a[1].digest));
    let events: Vec<_> = snapshot
        .data
        .events
        .iter()
        .filter(|e| !e.outputs.is_empty())
        .collect();
    for index in 0..4 {
        let prepared = saved.prepare(index);
        let summary = prepared.summary();
        let event = events[index / 2];
        assert_eq!(summary.inventory_index, index);
        assert_eq!(summary.event_output_ordinal, index % 2);
        assert_eq!(summary.event_id, event.id);
        assert_eq!(summary.event_sequence, event.sequence);
        assert_eq!(summary.operation_id, event.operation_id);
        assert_eq!(summary.produced_day, event.day);
        assert_eq!(summary.producing_program, event.program);
        assert_eq!(
            summary.event_receipt,
            canonical_digest(IdentityDomain::Evidence, event).unwrap()
        );
        verified(publish(
            prepared,
            &saved.root.join(format!("occurrence-{index}.csv")),
        ));
    }
}

#[test]
fn stale_basis_and_invalid_selection_never_prepare_an_export() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    let snapshot = saved.snapshot();
    let mut foreign = selection(&snapshot, 0);
    foreign.expected_digest = LocalArtifact::bytes_identity(b"different");
    assert!(matches!(
        PreparedExport::capture(&saved.store, &basis(&snapshot), foreign),
        Err(ExportError::SelectionChanged)
    ));
    let mut absent = selection(&snapshot, 0);
    absent.inventory_index = usize::MAX;
    assert!(matches!(
        PreparedExport::capture(&saved.store, &basis(&snapshot), absent),
        Err(ExportError::SelectionChanged)
    ));
    // Each host-captured component must participate, not only data or revision.
    for field in 0..7 {
        let mut displayed = snapshot.clone();
        match field {
            0 => displayed.revision += 1,
            1 => displayed.clock_day += 1,
            2 => displayed.data.generation += 1,
            3 => {
                displayed
                    .session
                    .values
                    .insert("search".into(), string("changed"));
            }
            4 => {
                displayed.decisions.revision += 1;
            }
            5 => {
                displayed.artifacts.clear();
            }
            _ => {
                displayed.programs[0].source_bytes.push(b' ');
            }
        };
        assert!(matches!(
            PreparedExport::capture(&saved.store, &basis(&displayed), selection(&snapshot, 0)),
            Err(ExportError::StaleBasis)
        ));
    }
    saved.apply("later", add("Zoe"));
    assert!(matches!(
        PreparedExport::capture(&saved.store, &basis(&snapshot), selection(&snapshot, 0)),
        Err(ExportError::StaleBasis)
    ));
}

#[test]
fn changes_after_destination_selection_are_rejected_before_writing() {
    for input in [
        add("Zoe"),
        SemanticInput::Control {
            view: "people".into(),
            control: "search_input".into(),
            value: string("Ada"),
        },
        SemanticInput::AdvanceClock { days: 1 },
        invoke("export_people", Values::new()),
    ] {
        let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
        let path = saved.root.join("not-created.csv");
        let selected = saved.prepare(0).select_destination(&path).unwrap();
        let current = saved.apply("after-selection", input);
        assert!(matches!(
            selected.publish(&AtomicBool::new(false)),
            ExportOutcome::NotAttempted(ExportError::StaleBasis)
        ));
        assert!(!path.exists());
        assert_eq!(saved.snapshot(), current);
    }
}

#[test]
fn historical_output_remains_exportable_after_adoption_more_work_and_reopen() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    let original = saved.snapshot();
    let before = saved.prepare(0);
    let mut source = organizer();
    source["label"] = json!("Different program and output definition");
    source["outputs"][0]["label"] = json!("Today's label is not the historical label");
    let target = capture(source);
    let plan = saved
        .store
        .prepare_switch(&target, "change-program")
        .unwrap();
    saved
        .store
        .adopt(original.revision, &plan, &target, &original.decisions)
        .unwrap();
    saved.apply("later-work", add("Zoe"));
    saved.apply("later-day", SemanticInput::AdvanceClock { days: 7 });
    assert!(matches!(
        publish(before, &saved.root.join("stale.csv")),
        ExportOutcome::NotAttempted(ExportError::StaleBasis)
    ));
    let reopened = ProductStore::open(saved.root.join("tool")).unwrap();
    let current = reopened.load().unwrap();
    let prepared =
        PreparedExport::capture(&reopened, &basis(&current), selection(&current, 0)).unwrap();
    assert_eq!(prepared.summary().produced_day, 20000);
    assert_eq!(
        prepared.summary().producing_program,
        original.program().unwrap().artifact.program_digest
    );
    assert_eq!(prepared.summary().output_id, "roster");
    assert_eq!(prepared.summary().row_count, 1);
    assert_ne!(
        prepared.summary().producing_program,
        current.program().unwrap().artifact.program_digest
    );
    let path = saved.root.join("historical.csv");
    verified(publish(prepared, &path));
    assert_eq!(fs::read(path).unwrap(), original.artifacts[0].bytes);
    assert_eq!(reopened.load().unwrap(), current);
}

fn replace_snapshot(path: &Path, snapshot: &ProjectSnapshot) {
    let digest = canonical_digest(IdentityDomain::Data, snapshot).unwrap();
    fs::write(
        path.join(format!("object-{}.json", digest.as_str())),
        canonical_bytes(snapshot).unwrap(),
    )
    .unwrap();
    let mut pointer: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("CURRENT")).unwrap()).unwrap();
    pointer["object"] = json!(digest);
    fs::write(path.join("CURRENT"), canonical_bytes(&pointer).unwrap()).unwrap();
}

#[test]
fn corrupted_artifact_or_event_receipt_is_refused_even_with_a_rehashed_snapshot() {
    for corrupt_event in [false, true] {
        let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
        let prepared = saved.prepare(0);
        let mut damaged = saved.snapshot();
        if corrupt_event {
            damaged.data.events.last_mut().unwrap().outputs[0] =
                LocalArtifact::bytes_identity(b"forged receipt");
        } else {
            damaged.artifacts[0].bytes.push(b'!');
            damaged.artifacts[0].bytes_digest =
                LocalArtifact::bytes_identity(&damaged.artifacts[0].bytes);
            damaged.artifacts[0].digest = damaged.artifacts[0].receipt_identity().unwrap();
            damaged.data.events.last_mut().unwrap().outputs[0] =
                damaged.artifacts[0].digest.clone();
        }
        replace_snapshot(&saved.root.join("tool"), &damaged);
        assert!(matches!(
            PreparedExport::capture(&saved.store, &basis(&damaged), selection(&damaged, 0)),
            Err(ExportError::Issue(_))
        ));
        let path = saved.root.join("not-created.csv");
        assert!(matches!(
            publish(prepared, &path),
            ExportOutcome::NotAttempted(ExportError::Issue(_))
        ));
        assert!(!path.exists());
    }
}

#[test]
fn cancellation_before_publication_does_not_create_a_file_or_change_saved_work() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    let before = saved.snapshot();
    let path = saved.root.join("cancelled.csv");
    let selected = saved.prepare(0).select_destination(&path).unwrap();
    assert!(matches!(
        selected.publish(&AtomicBool::new(true)),
        ExportOutcome::NotAttempted(ExportError::Cancelled)
    ));
    assert!(!path.exists());
    assert_eq!(saved.snapshot(), before);
}

#[test]
fn existing_files_remain_untouched_and_write_errors_are_publication_uncertain() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    for (name, bytes) in [
        ("different.csv", b"precious existing file".to_vec()),
        ("identical.csv", saved.snapshot().artifacts[0].bytes.clone()),
    ] {
        let path = saved.root.join(name);
        fs::write(&path, &bytes).unwrap();
        // write_new does not expose its publication stage; never infer that
        // every returned error means no file could have been published.
        assert!(matches!(
            publish(saved.prepare(0), &path),
            ExportOutcome::PublicationUncertain { .. }
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn traversal_and_replaced_selected_parent_are_refused_without_writes() {
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    // Joining onto a canonical Windows verbatim path normalizes away `..`.
    // Preserve the raw traversal component so the helper actually receives it.
    let mut traversal = saved.root.as_os_str().to_os_string();
    traversal.push(std::path::MAIN_SEPARATOR_STR);
    traversal.push("..");
    traversal.push(std::path::MAIN_SEPARATOR_STR);
    traversal.push("escape.csv");
    let traversal = PathBuf::from(traversal);
    assert!(traversal
        .components()
        .any(|component| component == std::path::Component::ParentDir));
    assert!(saved.prepare(0).select_destination(&traversal).is_err());
    let parent = saved.root.join("chosen");
    fs::create_dir(&parent).unwrap();
    let path = parent.join("output.csv");
    let selected = saved.prepare(0).select_destination(&path).unwrap();
    let parked = saved.root.join("parked");
    #[cfg(windows)]
    {
        // Windows pins directory handles by denying rename/delete outright.
        assert!(fs::rename(&parent, &parked).is_err());
        verified(selected.publish(&AtomicBool::new(false)));
        assert_eq!(
            fs::read(&path).unwrap(),
            saved.snapshot().artifacts[0].bytes
        );
    }
    #[cfg(not(windows))]
    {
        fs::rename(&parent, &parked).unwrap();
        fs::create_dir(&parent).unwrap();
        assert!(matches!(
            selected.publish(&AtomicBool::new(false)),
            ExportOutcome::NotAttempted(ExportError::Issue(_))
        ));
        assert!(!path.exists());
        assert!(!parked.join("output.csv").exists());
    }
}

#[cfg(unix)]
#[test]
fn symlink_hardlink_and_special_destinations_preserve_the_originals() {
    use std::os::unix::fs::symlink;
    let saved = Saved::new(OutputFormat::Csv, &["Ada"], false);
    let original = saved.root.join("original.csv");
    fs::write(&original, b"keep exactly").unwrap();
    let symlinked = saved.root.join("symlink.csv");
    symlink(&original, &symlinked).unwrap();
    let hardlink = saved.root.join("hardlink.csv");
    fs::hard_link(&original, &hardlink).unwrap();
    let directory = saved.root.join("directory.csv");
    fs::create_dir(&directory).unwrap();
    for destination in [&symlinked, &hardlink, &directory] {
        assert!(matches!(
            publish(saved.prepare(0), destination),
            ExportOutcome::PublicationUncertain { .. }
        ));
        assert_eq!(fs::read(&original).unwrap(), b"keep exactly");
    }
    let foreign = saved.root.join("foreign");
    fs::create_dir(&foreign).unwrap();
    symlink(&foreign, saved.root.join("linked-parent")).unwrap();
    assert!(saved
        .prepare(0)
        .select_destination(&saved.root.join("linked-parent/output.csv"))
        .is_err());
    assert!(!foreign.join("output.csv").exists());
}
