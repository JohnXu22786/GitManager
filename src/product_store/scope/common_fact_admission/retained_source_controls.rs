//! Bounded synthetic retained seam controls, never the owned-recovery journey.
use super::*;
use crate::product_contract::{reset_validation_counts, validation_counts};
#[path = "../../../../tests/fixtures/common_fact/mod.rs"]
mod fixture;
use fixture::{pair, request};
fn meter() -> ProofMeter { ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap() }
fn setup(a: &CapturedProgram) -> (tempfile::TempDir, ProductStore) {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(dir.path().join("store"), a, 20000).unwrap();
    for (id, title) in [("one", "live"), ("two", "finished")] {
        store
            .apply(
                store.load().unwrap().revision,
                id,
                &SemanticInput::Invoke {
                    action: "add".into(),
                    arguments: [(
                        "title".into(),
                        DataValue::Text {
                            value: title.into(),
                        },
                    )]
                    .into(),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
    }
    let s = store.load().unwrap();
    let row = s
        .data
        .records
        .iter()
        .find(|x| {
            x.values.get("title")
                == Some(&DataValue::Text {
                    value: "finished".into(),
                })
        })
        .unwrap();
    store
        .apply(
            s.revision,
            "finish",
            &SemanticInput::Invoke {
                action: "complete".into(),
                arguments: [(
                    "job".into(),
                    DataValue::Reference {
                        entity: "job".into(),
                        record: row.id.clone(),
                    },
                )]
                .into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    (dir, store)
}
fn witnesses(store: &ProductStore) -> Vec<LifecycleWitness> {
    let s = store.load().unwrap();
    let row = |done: bool| {
        let r = s
            .data
            .records
            .iter()
            .find(|r| {
                r.entity == "job"
                    && (r.values["status"]
                        == DataValue::Text {
                            value: "done".into(),
                        })
                        == done
            })
            .unwrap();
        RecordRef {
            entity: r.entity.clone(),
            record: r.id.clone(),
        }
    };
    [OwnedSourceSide::Before, OwnedSourceSide::After]
        .into_iter()
        .map(|side| LifecycleWitness {
            side,
            scenario: ScenarioSpec {
                version: 1,
                id: "lifecycle".into(),
                label: "Synthetic lifecycle witnesses".into(),
                seed: s.data.clone(),
                session: s.session.clone(),
                clock_day: s.clock_day,
                random_seed: 7,
                inputs: vec![SemanticInput::Observe {
                    point: "witness".into(),
                }],
                validity: vec![],
            },
            completed: row(true),
            unfinished: row(false),
        })
        .collect()
}
fn acceptance(
    store: &ProductStore,
    b: &CapturedProgram,
    r: &OwnedPreviewRequest,
) -> AcceptedPreviewLifecycle {
    let mut host = LifecycleAcceptanceHost::prepare(
        store,
        b,
        r,
        &witnesses(store),
        ProofBudget::default(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    host.synthetic_operator_accept_for_test(store).unwrap()
}
fn same_proof(a:&VerifiedOwnedPreviewPlan,b:&VerifiedOwnedPreviewPlan) {
    assert_eq!(a.snapshot,b.snapshot);assert_eq!(a.candidate,b.candidate);assert_eq!(a.request,b.request);
    assert_eq!(a.accepted.recipe(),b.accepted.recipe());assert_eq!(a.report,b.report);assert_eq!(a.captured,b.captured);
}
fn original(snapshot:&ProjectSnapshot,candidate:&CapturedProgram,request:&OwnedPreviewRequest,
    lifecycle:&OwnedLifecycleV2,meter:&mut ProofMeter)->AdmissionResult<(AcceptedPreviewLifecycle,VerifiedOwnedPreviewPlan)>{
    let accepted=AcceptedPreviewLifecycle::reconstruct(snapshot,candidate,request,lifecycle,meter)?;
    let inventory=authenticate_inner(snapshot,candidate,request,meter,false)?;
    meter.charge(32)?;
    let proof=finish_reconstructed_plan(snapshot,candidate,request,lifecycle,&accepted,inventory,meter)?;
    Ok((accepted,proof))
}
#[test]
fn retained_source_layout_and_complete_positive_payment() {
    use std::mem::size_of;
    let widths=[size_of::<RetainedFinish>(),size_of::<super::super::common_fact_inventory::InventoryTail>(),
        size_of::<super::super::owned_resolve::StructuralTail>(),size_of::<CaptureTail>(),
        size_of::<PreparedRetainedCapture>(),size_of::<CapturedRetainedCapture>(),size_of::<RetainedCaptureBody>(),
        size_of::<AdmissionResult<PreparedRetainedCapture>>(),size_of::<AdmissionResult<CapturedRetainedCapture>>(),
        size_of::<AdmissionResult<VerifiedOwnedPreviewPlan>>(),LocalRuntime::capture_result_bytes::<BTreeMap<String,Values>,StoreError>()];
    eprintln!("RETAINED_WIDTHS {:?} REGIONS {:?} G {}",widths,RETAINED_SOURCE_REGIONS,RETAINED_SOURCE_HANDOFF_G);
    type Ready=ReadyCaptureSource<'static,'static,ProofMeter>;
    let all=[("r",size_of::<&()>()),("p",size_of::<usize>()),("s",size_of::<&str>()),("j",size_of::<i32>()),("u",size_of::<u64>()),
        ("F",Ready::FACT_BYTES),("O",size_of::<Ready>()),("OptionalF",Ready::OPTIONAL_FACT_BYTES),
        ("T",size_of::<RetainedFinish>()),("Ix",size_of::<super::super::common_fact_inventory::InventoryTail>()),
        ("Sx",size_of::<super::super::owned_resolve::StructuralTail>()),("Cx",size_of::<CaptureTail>()),
        ("P",size_of::<PreparedRetainedCapture>()),("K",size_of::<CapturedRetainedCapture>()),("L",size_of::<LocalRuntime>()),
        ("A",size_of::<CommonFactInventory>()),("S",size_of::<OwnedStructuralReport>()),("D",size_of::<Digest>()),
        ("V",size_of::<VerifiedOwnedPreviewPlan>()),("B",size_of::<CommonFactAdmissionReport>()),("M",size_of::<BTreeMap<String,Values>>()),
        ("Q",Ready::FACT_RESULT_BYTES),("Z",size_of::<AdmissionResult<PreparedRetainedCapture>>()),
        ("R",size_of::<AdmissionResult<CapturedRetainedCapture>>()),("J",size_of::<AdmissionResult<VerifiedOwnedPreviewPlan>>()),
        ("Y",size_of::<AdmissionResult<(Vec<CommonFactCertificate>,Vec<OwnedProjectionRecipe>,Vec<String>)>>()),
        ("AS",size_of::<AdmissionResult<OwnedStructuralReport>>()),("AA",size_of::<AdmissionResult<CommonFactInventory>>()),
        ("AM",size_of::<AdmissionResult<BTreeMap<String,Values>>>()),("X",LocalRuntime::capture_result_bytes::<BTreeMap<String,Values>,StoreError>()),
        ("CBody",size_of::<RetainedCaptureBody>())];
    eprintln!("RETAINED_ALL_WIDTHS {:?}",all);

    assert_eq!(&widths[..7],&[40,96,120,16,864,840,64]);
    assert_eq!(RETAINED_SOURCE_HANDOFF_G,4 * RETAINED_SOURCE_REGIONS.iter().map(|n|*n as u64).sum::<u64>()+14*128);
    assert!(RETAINED_SOURCE_REGIONS.iter().all(|n|*n>0 && *n<16384));
    assert!(RETAINED_SOURCE_HANDOFF_G<5_647_146);
}
#[test]
fn retained_source_full_synthetic_output_and_exact_saving() {
    let(a,b)=pair();let(_dir,store)=setup(&a);let request=request(&store,&a,&b);let accepted=acceptance(&store,&b,&request);
    let snapshot=store.load().unwrap();let lifecycle=accepted.recipe();let before=store.root.read("CURRENT",16384).unwrap();
    let mut old_meter=meter();reset_validation_counts();let(old_accepted,old)=original(&snapshot,&b,&request,&lifecycle,&mut old_meter).unwrap();let old_counts=validation_counts();
    let mut new_meter=meter();reset_validation_counts();let(new_accepted,new)=reconstruct_and_derive_retained(&snapshot,&b,&request,&lifecycle,&mut new_meter).unwrap();let new_counts=validation_counts();
    assert_eq!(old_accepted.recipe(),new_accepted.recipe());same_proof(&old,&new);
    assert_eq!(old_counts.captures,new_counts.captures+1);assert_eq!(old_counts.apps,new_counts.apps+1);
    let(body,wrapper)=a.retained_source_payments_for_test();
    assert_eq!(new_meter.left+RETAINED_SOURCE_HANDOFF_G,old_meter.left+body+wrapper);
    assert!(body>RETAINED_SOURCE_HANDOFF_G);
    eprintln!("RETAINED_EXACT body={} wrapper={} G={} old={} new={}",body,wrapper,RETAINED_SOURCE_HANDOFF_G,9_999_999-old_meter.left,9_999_999-new_meter.left);
    let(cb,wb)=LocalRuntime::retained_callback_widths_for_test();
    let envelope=std::mem::size_of::<RetainedCaptureBody>()+8*std::mem::size_of::<usize>();
    assert!(cb>0 && cb<=envelope);assert!(wb>0 && wb<=envelope+2*std::mem::size_of::<&()>()+std::mem::size_of::<i32>()+8*std::mem::size_of::<usize>());
    eprintln!("RETAINED_CLOSURES caller={} bridge={} CB={}",cb,wb,envelope);
    let p=store.prepare_owned_preview(&old,ProofBudget::default(),Arc::new(AtomicBool::new(false))).unwrap();
    let q=store.prepare_owned_preview(&new,ProofBudget::default(),Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(p.target(),q.target());assert_eq!(p.initialized(),q.initialized());assert_eq!(p.receipt(),q.receipt());
    assert!(new.captured.values().any(|values|values.values().any(|v|*v==DataValue::Null)));
    assert_eq!(store.load().unwrap(),snapshot);assert_eq!(store.root.read("CURRENT",16384).unwrap(),before);
}
#[test]
fn retained_source_root_refusal_stops_before_authentication() {
    let(a,b)=pair();let(_dir,store)=setup(&a);let request=request(&store,&a,&b);let accepted=acceptance(&store,&b,&request);
    let snapshot=store.load().unwrap();let lifecycle=accepted.recipe();let mut measured=meter();
    reset_validation_counts();
    AcceptedPreviewLifecycle::reconstruct(&snapshot,&b,&request,&lifecycle,&mut measured).unwrap();
    let lifecycle_counts=validation_counts();
    let lifecycle_units=9_999_999-measured.left;
    for exact in [false,true] {
        let units=1+lifecycle_units+RETAINED_SOURCE_HANDOFF_G-u64::from(!exact);
        let mut limited=ProofMeter::new(ProofBudget{units,elapsed_millis:60000},Arc::new(AtomicBool::new(false))).unwrap();
        reset_validation_counts();
        let error=reconstruct_and_derive_retained(&snapshot,&b,&request,&lifecycle,&mut limited).err().unwrap();
        assert!(matches!(error,StoreError::Invalid(ref text) if text=="Common factual admission: budget: cumulative proof/initialization budget exhausted"));
        assert_eq!(limited.left,if exact {0}else{RETAINED_SOURCE_HANDOFF_G-1});
        // Exact G reaches only the original source-byte authentication debit.
        assert_eq!(validation_counts(),lifecycle_counts);
    }
}

#[test]
fn retained_source_new_packet_preserves_real_cancellation_and_elapsed_causes() {
    for mode in ["primary", "secondary", "elapsed"] {
        let mut m=meter();let secondary=Arc::new(AtomicBool::new(false));m.watch_cancellation(&secondary);
        match mode {
            "primary"=>m.cancelled.store(true,Ordering::Relaxed),
            "secondary"=>secondary.store(true,Ordering::Relaxed),
            _=>m.started=Instant::now()-std::time::Duration::from_millis(60_001),
        }
        let before=m.left;
        assert!(matches!(m.charge(RETAINED_SOURCE_HANDOFF_G),Err(StoreError::Invalid(ref text))
            if text=="Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
        assert_eq!(m.left,before);
    }
}
fn region<'a>(source:&'a str,from:&str,to:&str)->&'a str {
    source.split_once(from).unwrap().1.split_once(to).unwrap().0
}
fn closed_sources(contract:&str,runtime:&str,admission:&str,inventory:&str,structural:&str) {
    let normalized=[contract,runtime,admission,inventory,structural].map(|text|text.replace("\r\n","\n"));
    let [contract,runtime,admission,inventory,structural]=normalized.each_ref().map(String::as_str);
    let factory=region(contract,"pub(crate) fn with_ordinary_source_facts", "impl<'g, 's, H:");
    assert_eq!(factory.matches("source.validate_audited(Audit::none())?").count(),1);
    assert!(!factory.contains("charge("));
    let a=factory.find("let facts = source.validate_audited(Audit::none())?").unwrap();
    let b=factory.find("let prepared = prepare.prepare(host)?").unwrap();
    let c=factory.find("consume.consume(ReadyCaptureSource { source, facts: &facts, host }, prepared)").unwrap();
    assert!(a<b && b<c);
    let owner=region(contract,"pub(crate) struct ReadyCaptureSource", "pub(crate) fn with_ordinary_source_facts");
    assert!(owner.contains("source: &'s CapturedProgram"));assert!(owner.contains("facts: &'g CaptureFacts"));assert!(owner.contains("host: &'g mut H"));
    assert!(!owner.contains("pub "));assert!(!owner.contains("derive"));
    let owner_impl=region(contract,"impl<'g, 's, H:","struct Forward");
    assert_eq!(owner_impl.matches("pub(crate) fn ").count(),2);
    assert!(owner_impl.contains("wrap: impl FnOnce(&'g mut H) -> W"));
    assert!(owner_impl.contains("body: impl for<'v> FnOnce"));
    assert!(owner_impl.contains("frame_valid")==false); // shared constructor alone owns this state
    assert_eq!(contract.matches("frame_valid: false,").count(),1);
    let runtime_entry=region(runtime,"pub(crate) fn with_retained_projection_capture", "impl<'c,'v,'a,'s");
    assert!(runtime_entry.contains("let program=ready.source();"));
    assert!(runtime_entry.contains("let bridge = move |validation: &mut CaptureValidation<'_, 's, CaptureHost<'g, H>>| {\n            projection_capture_body!(validation, program, data, day, body)\n        };"));
    assert!(runtime_entry.contains("|host| CaptureHost{host,active_row_deadline:None,first_failure:None}, bridge"));
    assert_eq!(runtime_entry.matches("ready.into_capture_validation(").count(),1);
    assert!(!runtime_entry.contains("FnMut"));
    let capture=region(admission,"fn capture_frame_retained", "#[cfg(test)]\nimpl ProofMeter");
    assert!(capture.contains("let body = RetainedCaptureBody { captured, data, request, projections, profile: None };"));
    assert!(capture.contains("move |capture| {\n        let RetainedCaptureBody { mut captured, data, request, projections, mut profile } = body;\n        capture_frame_body!(capture, data, request, projections, profile, captured)\n    }"));
    assert!(!capture.contains("clone("));assert!(!capture.contains("meter"));
    let root=region(admission,"pub(super) fn reconstruct_and_derive_retained", "macro_rules! retained_guard_body");
    assert_eq!(root.matches("meter.charge(RETAINED_SOURCE_HANDOFF_G)?").count(),1);
    assert!(root.find("AcceptedPreviewLifecycle::reconstruct").unwrap()<root.find("meter.charge(RETAINED_SOURCE_HANDOFF_G)?").unwrap());
    assert!(root.find("meter.charge(RETAINED_SOURCE_HANDOFF_G)?").unwrap()<root.find("RetainedFinish {").unwrap());
    assert_eq!(structural.matches("with_ordinary_source_facts(before, meter,").count(),1);
    assert_eq!(structural.matches("impl SourcePreparation<").count(),1);
    assert_eq!(admission.matches("impl<'s> SourceConsumer<'s, ProofMeter,").count(),1);
    let tail=region(inventory,"impl InventoryTail", "pub(super) fn derive_retained");
    assert!(!tail.contains("program()?"));
    let entry=inventory.split_once("pub(super) fn derive_retained").unwrap().1;
    assert_eq!(entry.matches("snapshot.program()?").count(),1);
    assert!(entry.find("let before = &baseline.program;").unwrap()<entry.find("analyze_retained_sources(").unwrap());
    assert!(entry.find("let after = &candidate.program;").unwrap()<entry.find("analyze_retained_sources(").unwrap());
}
#[test]
fn retained_source_closed_call_and_capture_sets_reject_mutations() {
    let sources=[include_str!("../../../product_contract/capture_validation.rs"),include_str!("../../../product_runtime/capture.rs"),include_str!("../common_fact_admission.rs"),include_str!("../common_fact_inventory.rs"),include_str!("../owned_resolve.rs")];
    let crlf=sources.map(|s|s.replace("\r\n","\n").replace('\n',"\r\n"));
    closed_sources(&crlf[0],&crlf[1],&crlf[2],&crlf[3],&crlf[4]);
    let text=sources.map(|s|s.replace("\r\n","\n"));
    for newline in ["\n","\r\n"] {
        let variant=text.each_ref().map(|s|s.replace('\n',newline));
        closed_sources(&variant[0],&variant[1],&variant[2],&variant[3],&variant[4]);
        for (index,from,to) in [
            (0,"facts: &facts","facts: caller_facts"),
            (0,"let prepared = prepare.prepare(host)?","let prepared = prepare.prepare(host).unwrap()"),
            (1,"active_row_deadline:None,first_failure:None}, bridge","active_row_deadline:replacement,first_failure:None}, bridge"),
            (1,"projection_capture_body!(validation, program, data, day, body)\n        };","projection_capture_body!(validation, other_source, data, day, body)\n        };"),
            (2,"profile: None };","profile: previous_profile };"),
            (2,"meter.charge(RETAINED_SOURCE_HANDOFF_G)?","meter.charge(0)?"),
            (3,"let baseline = snapshot.program()?;\n    let before = &baseline.program;","let baseline = snapshot.program()?;\n    let before = &snapshot.program()?.program;"),
        ] {
            let mut changed=text.clone();changed[index]=changed[index].replacen(from,to,1);assert_ne!(changed[index],text[index]);
            let changed=changed.map(|s|s.replace('\n',newline));
            assert!(std::panic::catch_unwind(||closed_sources(&changed[0],&changed[1],&changed[2],&changed[3],&changed[4])).is_err(),"guard missed {from}");
        }
    }
}

#[test]
fn retained_source_protocol_has_no_other_source_or_consumer_call_sites() {
    fn walk(path:&std::path::Path,files:&mut Vec<std::path::PathBuf>){
        for entry in std::fs::read_dir(path).unwrap(){let p=entry.unwrap().path();
            if p.is_dir(){walk(&p,files)}else if p.extension().is_some_and(|e|e=="rs"){files.push(p)}
        }
    }
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let mut files=vec![];walk(&root.join("src"),&mut files);
    let allowed=["src/product_contract.rs","src/product_contract/capture_validation.rs","src/product_contract/retained_source_controls.rs",
        "src/product_runtime/capture.rs","src/product_runtime/capture/retained_source_controls.rs",
        "src/product_store/scope/owned_resolve.rs","src/product_store/scope/common_fact_admission.rs",
        "src/product_store/scope/common_fact_admission/retained_source_controls.rs"];
    for path in files {let text=std::fs::read_to_string(&path).unwrap();
        if ["with_ordinary_source_facts","ReadyCaptureSource","SourcePreparation","SourceConsumer","with_retained_projection_capture"].iter().any(|name|text.contains(name)) {
            let path=path.strip_prefix(root).unwrap();assert!(allowed.iter().any(|allowed|std::path::Path::new(allowed)==path),"unexpected protocol site: {}",path.display());
        }
    }
}
