//! Exact mechanism controls after genuine reconstruction, never a fake registry.
use super::*;

impl ProjectSnapshot {
    pub(crate) fn assert_registry_complete_association_for_test(&self) {
        self.validate_container().unwrap();
        let mut meter = ProofMeter::new(
            ProofBudget::default(), Arc::new(AtomicBool::new(false)),
        ).unwrap();
        let registry = reconstruct(self, &mut meter).unwrap();
        registry.check_snapshot(self).unwrap();
        registry.resolve(self.program().unwrap()).unwrap();

        // Change only a non-data field; a data-only association would miss it.
        let mut foreign_current = self.clone();
        foreign_current.clock_day = foreign_current.clock_day.checked_add(1).unwrap();
        assert_eq!(foreign_current.data, self.data);
        let error = registry.check_snapshot(&foreign_current).unwrap_err();
        assert!(matches!(error, StoreError::Invalid(ref text)
            if text == "Common factual admission: owned persisted proof: execution registry belongs to another complete snapshot"));

        // A still-valid capture with the same executable/artifact and bytes,
        // but different provenance, must fail complete-source resolution.
        let mut foreign_source = self.program().unwrap().clone();
        foreign_source.binding.program_path = "foreign-policy/program.json".into();
        assert_ne!(foreign_source, *self.program().unwrap());
        assert_eq!(foreign_source.artifact, self.program().unwrap().artifact);
        assert_eq!(foreign_source.source_bytes, self.program().unwrap().source_bytes);
        foreign_source.validate().unwrap();
        let error = match registry.resolve(&foreign_source) {
            Ok(_) => panic!("foreign complete source unexpectedly resolved"),
            Err(error) => error,
        };
        assert!(matches!(error, StoreError::Invalid(ref text)
            if text == "Common factual admission: owned persisted proof: source is not a checked owned recipe"));
    }
}
