//! Explicit launch/open migration gate. Ordinary load/apply never decodes a
//! legacy project. Immutable old bytes and the durable journal are retained.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeProgress {
    Validating,
    Staged,
    Verified,
    Activating,
    RestartRequired,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeSummary {
    pub from_version: u32,
    pub to_version: u32,
    pub records: usize,
    pub events: usize,
    pub artifacts: usize,
    pub restart_required: bool,
}
/// Exact old wire shape; this type is confined to the upgrade gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySnapshot {
    magic: String,
    version: u32,
    revision: u64,
    programs: Vec<CapturedProgram>,
    active_revision: Digest,
    data: DataSnapshot,
    session: SessionState,
    clock_day: i32,
    decisions: DecisionGraph,
    adoptions: Vec<AdoptionReceipt>,
    operations: BTreeMap<Id, OperationReceipt>,
    artifacts: Vec<LocalArtifact>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeJournal {
    version: u32,
    from: Pointer,
    to: Pointer,
    records: usize,
    events: usize,
    artifacts: usize,
    programs: usize,
    decisions: usize,
}
impl LegacySnapshot {
    fn upgraded(self) -> Result<ProjectSnapshot> {
        if self.magic != MAGIC || self.version != 1 {
            return Err(StoreError::Corrupt(
                "invalid legacy generated-project header".into(),
            ));
        }
        let next = ProjectSnapshot {
            magic: self.magic,
            version: FORMAT,
            revision: self.revision,
            programs: self.programs,
            active_revision: self.active_revision,
            data: self.data,
            session: self.session,
            clock_day: self.clock_day,
            decisions: self.decisions,
            adoptions: self.adoptions,
            operations: self.operations,
            artifacts: self.artifacts,
            scope: scope::ScopeState::default(),
        };
        next.validate()?;
        Ok(next)
    }
}
impl ProductStore {
    pub fn upgrade_generated_project<F>(&self, progress: F) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        self.upgrade_inner(
            progress,
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn upgrade_with_fault<F>(&self, progress: F, fault: FaultPoint) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        self.upgrade_inner(progress, Some(fault))
    }
    fn upgrade_inner<F>(
        &self,
        mut progress: F,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        let _lock = self.root.lock()?;
        self.pinned()?;
        progress(UpgradeProgress::Validating);
        let pointer_bytes = self.root.read("CURRENT", 16 * 1024)?;
        let pointer: Pointer = json::parse(&pointer_bytes)?;
        if pointer.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        if pointer.version == FORMAT {
            let current = self.load()?;
            return Ok(UpgradeSummary {
                from_version: FORMAT,
                to_version: FORMAT,
                records: current.data.records.len(),
                events: current.data.events.len(),
                artifacts: current.artifacts.len(),
                restart_required: false,
            });
        }
        if pointer.version != 1 {
            return Err(StoreError::UnsupportedFormat(pointer.version));
        }
        let old_name = format!("object-{}.json", pointer.object.as_str());
        let old_bytes = self.root.read(&old_name, MAX_STORE_BYTES)?;
        let old: LegacySnapshot = json::parse(&old_bytes)?;
        if canonical_digest(IdentityDomain::Data, &old)? != pointer.object
            || old.revision != pointer.revision
            || old.data.project_id != pointer.project_id
        {
            return Err(StoreError::Corrupt(
                "legacy snapshot checksum or pointer identity mismatch".into(),
            ));
        }
        let target = old.clone().upgraded()?;
        LocalRuntime::default().resume(
            target.program()?,
            &target.data,
            &target.session,
            target.clock_day,
            0,
            RuntimeLimits::default(),
            &target.artifacts,
        )?;
        // Conversion adds only the new empty authority envelope and header. It
        // cannot reinterpret an old intention, event, source or adoption flag.
        let mut projected = serde_json::to_value(&target)?;
        projected.as_object_mut().unwrap().remove("scope");
        projected["version"] = serde_json::json!(1);
        if canonical_bytes(&projected)? != canonical_bytes(&old)? {
            return Err(StoreError::Corrupt("upgrade changed retained data".into()));
        }
        let bytes = canonical_bytes(&target)?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid(
                "upgraded snapshot exceeds store budget".into(),
            ));
        }
        let digest = canonical_digest(IdentityDomain::Data, &target)?;
        let object_name = format!("object-{}.json", digest.as_str());
        self.publish_upgrade_object(&object_name, &bytes)?;
        progress(UpgradeProgress::Staged);
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeObject) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeObject));
        }
        let journal = UpgradeJournal {
            version: 1,
            from: pointer.clone(),
            to: Pointer {
                magic: MAGIC.into(),
                version: FORMAT,
                project_id: target.data.project_id.clone(),
                revision: target.revision,
                object: digest.clone(),
            },
            records: target.data.records.len(),
            events: target.data.events.len(),
            artifacts: target.artifacts.len(),
            programs: target.programs.len(),
            decisions: target.decisions.decisions.len(),
        };
        let journal_name = format!("upgrade-1-2-{}.json", pointer.object.as_str());
        self.publish_upgrade_object(&journal_name, &canonical_bytes(&journal)?)?;
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeJournal) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeJournal));
        }
        let reread: ProjectSnapshot = json::parse(&self.root.read(&object_name, MAX_STORE_BYTES)?)?;
        reread.validate()?;
        let recorded: UpgradeJournal =
            json::parse(&self.root.read(&journal_name, MAX_STORE_BYTES)?)?;
        if reread != target
            || recorded != journal
            || self.root.read(&old_name, MAX_STORE_BYTES)? != old_bytes
            || self.root.read("CURRENT", 16 * 1024)? != pointer_bytes
        {
            return Err(StoreError::Corrupt(
                "upgrade readback or retained-source integrity mismatch".into(),
            ));
        }
        progress(UpgradeProgress::Verified);
        self.pinned()?;
        progress(UpgradeProgress::Activating);
        self.upgrade_requires_reopen
            .store(true, std::sync::atomic::Ordering::Release);
        self.save(
            &target,
            true,
            #[cfg(test)]
            fault,
        )?;
        progress(UpgradeProgress::RestartRequired);
        Ok(UpgradeSummary {
            from_version: 1,
            to_version: FORMAT,
            records: target.data.records.len(),
            events: target.data.events.len(),
            artifacts: target.artifacts.len(),
            restart_required: true,
        })
    }
    fn publish_upgrade_object(&self, name: &str, bytes: &[u8]) -> Result<()> {
        match self.root.publish(name, bytes, false) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if self.root.read(name, MAX_STORE_BYTES)? != bytes {
                    return Err(StoreError::Corrupt(
                        "immutable upgrade object collision".into(),
                    ));
                }
                self.root.sync()?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}
