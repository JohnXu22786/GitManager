//! Concrete assertion bridges for genuine public fixtures and independent gates.
use super::*;
use crate::product_store::scope::{prepare_owned_withdrawal, transition_policy_surface, ProofBudget};
use std::sync::{atomic::AtomicBool, Arc};
#[path = "owned_policy_old_reference.rs"]
mod old_reference;

fn fresh() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
fn surface_parity(current: &ProjectSnapshot, layer: &Digest, source: &CapturedProgram) {
    let mut selected = None;
    prepare_owned_withdrawal(
        current, layer, "surface-parity", ProofBudget::default(), fresh(), None,
        |check| {
            selected = Some(check.surface(source)?);
            Ok(())
        },
    ).unwrap();
    let mut old = None;
    current.run_old_registry_withdrawal_for_test(layer, "surface-parity", |execution, meter| {
        old = Some(transition_policy_surface(execution, source, meter)?);
        Ok(())
    }).unwrap();
    let selected = selected.expect("selected actual surface call was reached");
    let old = old.expect("old reconstructing actual surface call was reached");
    selected.assert_same_value_for_test(&old);
    assert_eq!(selected.source(), &canonical_digest(IdentityDomain::Source, source).unwrap());
    assert_eq!(selected.original_layer(), layer);
    assert_eq!(selected.qualify_observable("policy_memory_count"), Some(MAX_COLLECTION as i64));
    assert_eq!(selected.qualify_observable("ready_count"), None);
    selected.validate_frame(&current.data).unwrap();
}

impl<R: RuntimeAdapter> DecisionEngine<R> {
    pub(crate) fn assert_registry_public_withdrawal_parity_for_test(
        &self, store: &ProductStore, layer: &Digest,
    ) {
        let current = store.load().unwrap();
        // This is the actual complete public selected route, including its
        // healthy-current and remaining-intention checks, not a test context.
        let (prepared, summary, report) = self.preview_owned_withdrawal(
            store, layer, "registry-public-parity", ProofBudget::default(), fresh(),
        ).unwrap();
        assert_eq!(report.disposition, CheckDisposition::Ready);
        assert_eq!(summary.retire_decisions, vec!["owned-policy"]);
        assert_eq!(prepared.seed(), &current.data);
        let mut reached = false;
        current.run_old_registry_withdrawal_for_test(layer, "registry-public-parity", |execution, meter| {
            reached = true;
            assert_eq!(execution.current(), &current);
            let ids = old_reference::check_withdrawal(&self.archive, execution, meter)
                .map_err(super::super::owned_transition::into_store_error)?;
            assert_eq!(ids, summary.retire_decisions);
            Ok(())
        }).unwrap();
        assert!(reached, "old whole qualifier must actually succeed on its independent gate");
        let decision = current.decisions.decisions.iter().find(|d| d.id == "owned-policy").unwrap();
        let scenes = self.archive.load(&decision.witness).unwrap();
        assert_eq!(scenes.len(), 7);
        assert!(scenes.iter().all(|s| s.program() == scenes[0].program()));
        surface_parity(&current, layer, scenes[0].program());
        assert_eq!(store.load().unwrap(), current);
    }

    pub(crate) fn assert_registry_distinct_sources_for_test(
        &self, store: &ProductStore, layer: &Digest, evolved_scenes: &[AcceptedScene],
    ) {
        let current = store.load().unwrap();
        let original = current.decisions.decisions.iter().find(|d| d.id == "owned-policy").unwrap();
        let initial_scenes = self.archive.load(&original.witness).unwrap();
        assert_eq!(initial_scenes.len(), 7);
        assert_eq!(evolved_scenes.len(), 7);
        for scenes in [initial_scenes.as_slice(), evolved_scenes] {
            for scene in scenes { scene.validate().unwrap(); }
            assert!(scenes.iter().all(|s| s.program() == scenes[0].program()));
        }
        let sources = [initial_scenes[0].program(), evolved_scenes[0].program()];
        assert_eq!(sources[1], current.program().unwrap());
        assert_ne!(sources[0], sources[1]);
        assert_ne!(sources[0].artifact.program_digest, sources[1].artifact.program_digest);
        assert_eq!(retirement_ids(&current, layer).unwrap(), vec!["owned-policy"]);
        let mut selected = Vec::new();
        prepare_owned_withdrawal(
            &current, layer, "two-source-surfaces", ProofBudget::default(), fresh(), None,
            |check| {
                for source in sources { selected.push(check.surface(source)?); }
                Ok(())
            },
        ).unwrap();
        assert_eq!(selected.len(), 2, "both actual surface calls must finish on this gate");
        assert_ne!(selected[0].source(), selected[1].source());
        assert_ne!(selected[0].baseline(), selected[1].baseline());
        assert_ne!(selected[0].candidate(), selected[1].candidate());
        for (index, source) in sources.into_iter().enumerate() {
            let mut old = None;
            current.run_old_registry_withdrawal_for_test(layer, "old-source-surface", |execution, meter| {
                old = Some(transition_policy_surface(execution, source, meter)?);
                Ok(())
            }).unwrap();
            selected[index].assert_same_value_for_test(&old.expect("old source/history route must finish"));
            assert_eq!(selected[index].qualify_observable("policy_memory_count"), Some(MAX_COLLECTION as i64));
            let scenes = if index == 0 { initial_scenes.as_slice() } else { evolved_scenes };
            for scene in scenes { selected[index].validate_frame(&scene.scenario().seed).unwrap(); }
        }
        assert_eq!(store.load().unwrap(), current);
    }

    pub(crate) fn assert_registry_hidden_unowned_refusal_for_test(
        &self, store: &ProductStore, layer: &Digest,
    ) {
        const REASON: &str = "Unverified(\"whole obligation contains an unowned observed surface\")";
        let current = store.load().unwrap();
        let public_error = match self.preview_owned_withdrawal(
            store, layer, "hidden-unowned", ProofBudget::default(), fresh(),
        ) {
            Ok(_) => panic!("a hidden common branch must prevent public retirement"),
            Err(error) => error,
        };
        assert!(matches!(public_error, DecisionError::Store(StoreError::Invalid(ref text)) if text == REASON));
        let mut reached = false;
        let selected = prepare_owned_withdrawal(
            &current, layer, "hidden-unowned", ProofBudget::default(), fresh(), None,
            |check| {
                reached = true;
                let result = check_withdrawal(&self.archive, check);
                assert!(matches!(&result, Err(DecisionError::Unverified(text))
                    if text == "whole obligation contains an unowned observed surface"));
                result.map(|_| ()).map_err(super::super::owned_transition::into_store_error)
            },
        );
        assert!(reached, "must reach semantic qualifier before refusal");
        assert!(matches!(selected, Err(StoreError::Invalid(ref text)) if text == REASON));
        let mut old_reached = false;
        let old = current.run_old_registry_withdrawal_for_test(layer, "hidden-unowned", |execution, meter| {
            old_reached = true;
            let result = old_reference::check_withdrawal(&self.archive, execution, meter);
            assert!(matches!(&result, Err(DecisionError::Unverified(text))
                if text == "whole obligation contains an unowned observed surface"));
            result.map(|_| ()).map_err(super::super::owned_transition::into_store_error)
        });
        assert!(old_reached);
        assert!(matches!(old, Err(StoreError::Invalid(ref text)) if text == REASON));
        assert_eq!(store.load().unwrap(), current);
    }

    pub(crate) fn assert_registry_surface_rejects_foreign_source_for_test(
        &self, store: &ProductStore, layer: &Digest,
    ) {
        const REASON: &str = "Common factual admission: owned persisted proof: source is not a checked owned recipe";
        let current = store.load().unwrap();
        let mut foreign = current.program().unwrap().clone();
        foreign.binding.program_path = "foreign-policy/program.json".into();
        foreign.validate().unwrap();
        assert_eq!(foreign.artifact, current.program().unwrap().artifact);
        let mut reached = false;
        let selected = prepare_owned_withdrawal(
            &current, layer, "foreign-source", ProofBudget::default(), fresh(), None,
            |check| {
                reached = true;
                let error = match check.surface(&foreign) {
                    Ok(_) => panic!("foreign capture obtained a policy surface"),
                    Err(error) => error,
                };
                assert!(matches!(&error, StoreError::Invalid(text) if text == REASON));
                Err(error)
            },
        );
        assert!(reached);
        assert!(matches!(selected, Err(StoreError::Invalid(ref text)) if text == REASON));
        let old = current.run_old_registry_withdrawal_for_test(layer, "foreign-source", |execution, meter| {
            let error = match transition_policy_surface(execution, &foreign, meter) {
                Ok(_) => panic!("old route accepted foreign capture"),
                Err(error) => error,
            };
            assert!(matches!(&error, StoreError::Invalid(text) if text == REASON));
            Err(error)
        });
        assert!(matches!(old, Err(StoreError::Invalid(ref text)) if text == REASON));
        assert_eq!(store.load().unwrap(), current);
    }
}
