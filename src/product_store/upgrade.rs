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
    pub project_id: Id,
    pub application_id: Id,
    pub first_program: Digest,
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
    /// Read-only format gate used before offering an upgrade. No legacy state
    /// is returned as writable and no ordinary edit invokes this conversion.
    pub fn inspect_upgrade(&self) -> Result<UpgradeSummary> {
        self.pinned()?;
        let pointer: Pointer = json::parse(&self.root.read("CURRENT", 16 * 1024)?)?;
        if pointer.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        let snapshot = if pointer.version == FORMAT {
            self.load()?
        } else if pointer.version == 1 {
            let old: LegacySnapshot = json::parse(&self.root.read(
                &format!("object-{}.json", pointer.object.as_str()),
                MAX_STORE_BYTES,
            )?)?;
            if canonical_digest(IdentityDomain::Data, &old)? != pointer.object
                || old.revision != pointer.revision
                || old.data.project_id != pointer.project_id
            {
                return Err(StoreError::Corrupt(
                    "legacy snapshot identity mismatch".into(),
                ));
            }
            old.upgraded()?
        } else {
            return Err(StoreError::UnsupportedFormat(pointer.version));
        };
        Ok(UpgradeSummary {
            project_id: snapshot.data.project_id.clone(),
            application_id: snapshot.program()?.program.id.clone(),
            first_program: revision(&snapshot.programs[0])?,
            from_version: pointer.version,
            to_version: FORMAT,
            records: snapshot.data.records.len(),
            events: snapshot.data.events.len(),
            artifacts: snapshot.artifacts.len(),
            restart_required: pointer.version != FORMAT,
        })
    }
    pub fn upgrade_generated_project_verified<F, V>(
        &self,
        progress: F,
        verify: V,
    ) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
        V: FnOnce(&ProjectSnapshot) -> Result<()>,
    {
        self.upgrade_inner(
            progress,
            verify,
            #[cfg(test)]
            None,
        )
    }
    pub fn upgrade_generated_project<F>(&self, progress: F) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        self.upgrade_inner(
            progress,
            |snapshot| {
                if snapshot.decisions.decisions.is_empty()
                    && snapshot
                        .adoptions
                        .iter()
                        .all(|a| a.plan.evidence.is_empty())
                {
                    Ok(())
                } else {
                    Err(StoreError::Invalid(
                        "Upgrade needs verified intention packages; use the tool-open upgrade flow"
                            .into(),
                    ))
                }
            },
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn upgrade_with_fault<F>(&self, progress: F, fault: FaultPoint) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        self.upgrade_inner(
            progress,
            |snapshot| {
                if snapshot.decisions.decisions.is_empty() {
                    Ok(())
                } else {
                    Err(StoreError::Invalid(
                        "fault helper cannot skip intention verification".into(),
                    ))
                }
            },
            Some(fault),
        )
    }
    fn upgrade_inner<F, V>(
        &self,
        mut progress: F,
        verify: V,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
        V: FnOnce(&ProjectSnapshot) -> Result<()>,
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
                project_id: current.data.project_id.clone(),
                application_id: current.program()?.program.id.clone(),
                first_program: revision(&current.programs[0])?,
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
        verify(&target)?;
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
            project_id: target.data.project_id.clone(),
            application_id: target.program()?.program.id.clone(),
            first_program: revision(&target.programs[0])?,
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

/// Read-only old-format conversion used only by explicit backup intake. The
/// original canonical bytes remain available for staged fresh recovery.
pub(crate) struct LegacyUpgrade {
    pub(crate) snapshot: ProjectSnapshot,
    pub(crate) original: Vec<u8>,
    pub(crate) original_digest: Digest,
}
impl LegacyUpgrade {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid("legacy snapshot exceeds limit".into()));
        }
        let legacy: LegacySnapshot = json::parse(bytes)?;
        if canonical_bytes(&legacy)? != bytes {
            return Err(StoreError::Corrupt("noncanonical legacy snapshot".into()));
        }
        let original_digest = canonical_digest(IdentityDomain::Data, &legacy)?;
        let snapshot = legacy.upgraded()?;
        Ok(Self {
            snapshot,
            original: bytes.to_vec(),
            original_digest,
        })
    }
}
impl ProductStore {
    pub(crate) fn recover_upgraded_with<F, P>(
        path: &Path,
        upgrade: &LegacyUpgrade,
        fresh_only: bool,
        before_activate: F,
        progress: P,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<UpgradeSummary>
    where
        F: FnOnce(&ProductStore, &ProjectSnapshot) -> Result<()>,
        P: FnMut(UpgradeProgress),
    {
        Self::recover_upgraded_at(
            path,
            None,
            upgrade,
            fresh_only,
            before_activate,
            progress,
            #[cfg(test)]
            fault,
        )
    }
    pub(crate) fn recover_upgraded_selected<F, P>(
        destination: &RecoveryDestination,
        upgrade: &LegacyUpgrade,
        before_activate: F,
        progress: P,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<UpgradeSummary>
    where
        F: FnOnce(&ProductStore, &ProjectSnapshot) -> Result<()>,
        P: FnMut(UpgradeProgress),
    {
        Self::recover_upgraded_at(
            destination.path(),
            Some(destination),
            upgrade,
            true,
            before_activate,
            progress,
            #[cfg(test)]
            fault,
        )
    }
    fn recover_upgraded_at<F, P>(
        path: &Path,
        selected: Option<&RecoveryDestination>,
        upgrade: &LegacyUpgrade,
        fresh_only: bool,
        before_activate: F,
        mut progress: P,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<UpgradeSummary>
    where
        F: FnOnce(&ProductStore, &ProjectSnapshot) -> Result<()>,
        P: FnMut(UpgradeProgress),
    {
        progress(UpgradeProgress::Validating);
        let verified = LegacyUpgrade::decode(&upgrade.original)?;
        if verified.snapshot != upgrade.snapshot
            || verified.original_digest != upgrade.original_digest
        {
            return Err(StoreError::Corrupt("legacy upgrade input changed".into()));
        }
        let snapshot = &upgrade.snapshot;
        LocalRuntime::default().resume(
            snapshot.program()?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            RuntimeLimits::default(),
            &snapshot.artifacts,
        )?;
        let (parent, name) = match selected {
            Some(selected) => selected.checked_parts()?,
            None => {
                let (parent, name) = Self::location(path)?;
                (Arc::new(parent), name)
            }
        };
        let (root, fresh) = match parent.create_child(&name) {
            Ok(root) => (root, true),
            // Ordinary recovery must claim a new directory atomically. Only
            // the explicit resumable-upgrade API may inspect an existing inbox.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && !fresh_only => {
                (parent.child(&name)?, false)
            }
            Err(error) => return Err(error.into()),
        };
        let store = Self {
            parent,
            root: Arc::new(root),
            name,
            upgrade_requires_reopen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            validation_cache: Arc::new(Mutex::new(ValidationCache::default())),
        };
        let _lock = store.root.lock()?;
        let marker = canonical_bytes(&(
            upgrade.original_digest.clone(),
            canonical_digest(IdentityDomain::Data, snapshot)?,
        ))?;
        if fresh {
            store.publish_upgrade_object("UPGRADE-INBOX", &marker)?;
        } else if store
            .root
            .read("UPGRADE-INBOX", MAX_STORE_BYTES)
            .ok()
            .as_ref()
            != Some(&marker)
        {
            return Err(StoreError::Conflict(
                "destination is not this exact staged upgrade; choose a fresh folder".into(),
            ));
        }
        let summary = UpgradeSummary {
            project_id: snapshot.data.project_id.clone(),
            application_id: snapshot.program()?.program.id.clone(),
            first_program: revision(&snapshot.programs[0])?,
            from_version: 1,
            to_version: FORMAT,
            records: snapshot.data.records.len(),
            events: snapshot.data.events.len(),
            artifacts: snapshot.artifacts.len(),
            restart_required: true,
        };
        match store.root.read("CURRENT", 16 * 1024) {
            Ok(_) => {
                if store.load()? != *snapshot {
                    return Err(StoreError::Conflict("upgrade destination has different or later committed work; keep that work and reopen it".into()));
                }
                before_activate(&store, snapshot)?;
                progress(UpgradeProgress::Verified);
                progress(UpgradeProgress::RestartRequired);
                return Ok(summary);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        store.require_unactivated()?;
        let original_name = format!("object-{}.json", upgrade.original_digest.as_str());
        store.publish_upgrade_object(&original_name, &upgrade.original)?;
        let object = canonical_digest(IdentityDomain::Data, snapshot)?;
        let object_name = format!("object-{}.json", object.as_str());
        store.publish_upgrade_object(&object_name, &canonical_bytes(snapshot)?)?;
        progress(UpgradeProgress::Staged);
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeObject) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeObject));
        }
        let journal = UpgradeJournal {
            version: 1,
            from: Pointer {
                magic: MAGIC.into(),
                version: 1,
                project_id: snapshot.data.project_id.clone(),
                revision: snapshot.revision,
                object: upgrade.original_digest.clone(),
            },
            to: Pointer {
                magic: MAGIC.into(),
                version: FORMAT,
                project_id: snapshot.data.project_id.clone(),
                revision: snapshot.revision,
                object,
            },
            records: snapshot.data.records.len(),
            events: snapshot.data.events.len(),
            artifacts: snapshot.artifacts.len(),
            programs: snapshot.programs.len(),
            decisions: snapshot.decisions.decisions.len(),
        };
        let journal_name = format!("upgrade-1-2-{}.json", upgrade.original_digest.as_str());
        store.publish_upgrade_object(&journal_name, &canonical_bytes(&journal)?)?;
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeJournal) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeJournal));
        }
        before_activate(&store, snapshot)?;
        let reread: ProjectSnapshot =
            json::parse(&store.root.read(&object_name, MAX_STORE_BYTES)?)?;
        let recorded: UpgradeJournal =
            json::parse(&store.root.read(&journal_name, MAX_STORE_BYTES)?)?;
        if reread != *snapshot
            || recorded != journal
            || store.root.read(&original_name, MAX_STORE_BYTES)? != upgrade.original
        {
            return Err(StoreError::Corrupt(
                "fresh upgrade readback mismatch".into(),
            ));
        }
        progress(UpgradeProgress::Verified);
        progress(UpgradeProgress::Activating);
        store.save(
            snapshot,
            false,
            #[cfg(test)]
            fault,
        )?;
        progress(UpgradeProgress::RestartRequired);
        Ok(UpgradeSummary {
            project_id: snapshot.data.project_id.clone(),
            application_id: snapshot.program()?.program.id.clone(),
            first_program: revision(&snapshot.programs[0])?,
            from_version: 1,
            to_version: FORMAT,
            records: snapshot.data.records.len(),
            events: snapshot.data.events.len(),
            artifacts: snapshot.artifacts.len(),
            restart_required: true,
        })
    }
}
