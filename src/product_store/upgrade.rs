//! Explicit launch/open migration gate. Normal loading never converts old data.
//! Immutable source bytes and deterministic journals remain retained forever.
use super::*;
mod legacy;
pub(crate) use legacy::VerifiedContainerUpgrade;
const MIGRATION: &str = "generated-project-container/3";

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
fn summary(snapshot: &ProjectSnapshot, from_version: u32) -> Result<UpgradeSummary> {
    Ok(UpgradeSummary {
        project_id: snapshot.data.project_id.clone(),
        application_id: snapshot.program()?.program.id.clone(),
        first_program: revision(&snapshot.programs[0])?,
        from_version,
        to_version: FORMAT,
        records: snapshot.data.records.len(),
        events: snapshot.data.events.len(),
        artifacts: snapshot.artifacts.len(),
        restart_required: from_version != FORMAT,
    })
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeJournal {
    version: u32,
    migration: String,
    from: Pointer,
    to: Pointer,
    preserved_payload: Digest,
    records: usize,
    events: usize,
    artifacts: usize,
    programs: usize,
    decisions: usize,
}
impl VerifiedContainerUpgrade {
    fn pointer(&self, current: bool) -> Pointer {
        Pointer {
            magic: MAGIC.into(),
            version: if current {
                FORMAT
            } else {
                self.source_version()
            },
            project_id: self.snapshot().data.project_id.clone(),
            revision: self.snapshot().revision,
            object: if current {
                self.target_digest()
            } else {
                self.original_digest()
            }
            .clone(),
        }
    }
    fn journal(&self) -> UpgradeJournal {
        let s = self.snapshot();
        UpgradeJournal {
            version: 2,
            migration: MIGRATION.into(),
            from: self.pointer(false),
            to: self.pointer(true),
            preserved_payload: self.preserved_payload().clone(),
            records: s.data.records.len(),
            events: s.data.events.len(),
            artifacts: s.artifacts.len(),
            programs: s.programs.len(),
            decisions: s.decisions.decisions.len(),
        }
    }
    fn journal_name(&self) -> String {
        format!(
            "upgrade-{}-3-{}.json",
            self.source_version(),
            self.original_digest().as_str()
        )
    }
}
fn resume(snapshot: &ProjectSnapshot) -> Result<()> {
    LocalRuntime::default().resume(
        snapshot.program()?,
        &snapshot.data,
        &snapshot.session,
        snapshot.clock_day,
        0,
        RuntimeLimits::default(),
        &snapshot.artifacts,
    )?;
    Ok(())
}
fn uncertain(error: StoreError) -> StoreError {
    StoreError::ActivationUnconfirmed(format!("Activation durability is unconfirmed; reopen this exact folder and explicitly retry verification: {error}"))
}
fn unscoped_packages(snapshot: &ProjectSnapshot) -> Result<()> {
    if snapshot.decisions.decisions.is_empty()
        && snapshot
            .adoptions
            .iter()
            .all(|a| a.plan.evidence.is_empty())
    {
        Ok(())
    } else {
        Err(StoreError::Invalid(
            "Upgrade needs verified intention packages; use the tool-open upgrade flow".into(),
        ))
    }
}
impl ProductStore {
    pub fn inspect_upgrade(&self) -> Result<UpgradeSummary> {
        self.pinned()?;
        let pointer: Pointer = json::parse(&self.root.read("CURRENT", 16 * 1024)?)?;
        if pointer.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        match pointer.version {
            FORMAT => summary(&self.load()?, FORMAT),
            1 | 2 => {
                let bytes = self.root.read(
                    &format!("object-{}.json", pointer.object.as_str()),
                    MAX_STORE_BYTES,
                )?;
                let conversion = VerifiedContainerUpgrade::from_pointer(&bytes, &pointer)?;
                summary(conversion.snapshot(), pointer.version)
            }
            v => Err(StoreError::UnsupportedFormat(v)),
        }
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
            unscoped_packages,
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn upgrade_with_fault<F>(&self, progress: F, fault: FaultPoint) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
    {
        self.upgrade_inner(progress, unscoped_packages, Some(fault))
    }
    #[cfg(test)]
    pub fn upgrade_verified_with_fault<F, V>(
        &self,
        progress: F,
        verify: V,
        fault: FaultPoint,
    ) -> Result<UpgradeSummary>
    where
        F: FnMut(UpgradeProgress),
        V: FnOnce(&ProjectSnapshot) -> Result<()>,
    {
        self.upgrade_inner(progress, verify, Some(fault))
    }
    /// Retain the exact package bytes used by the one-shot semantic verifier.
    /// Recheck and content-sync them immediately before activation/durable return.
    fn upgrade_packages(&self, snapshot: &ProjectSnapshot) -> Result<BTreeMap<Digest, Vec<u8>>> {
        snapshot
            .decisions
            .decisions
            .iter()
            .map(|d| &d.witness)
            .chain(
                snapshot
                    .adoptions
                    .iter()
                    .filter_map(|a| a.plan.evidence.first()),
            )
            .map(|digest| Ok((digest.clone(), self.read_extension(digest)?)))
            .collect()
    }
    fn sync_upgrade_packages(&self, packages: &BTreeMap<Digest, Vec<u8>>) -> Result<()> {
        for (digest, bytes) in packages {
            self.require_synced_bytes(&format!("extension-{}.json", digest.as_str()), bytes)?;
        }
        Ok(())
    }
    fn require_synced_bytes(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if self.root.read_and_sync(name, MAX_STORE_BYTES)? != bytes {
            return Err(StoreError::Corrupt(format!(
                "upgrade retained bytes changed: {name}"
            )));
        }
        Ok(())
    }
    fn complete_current_durability(
        &self,
        snapshot: &ProjectSnapshot,
        pointer_bytes: &[u8],
        packages: &BTreeMap<Digest, Vec<u8>>,
    ) -> Result<()> {
        self.pinned()?;
        let current_bytes = self.root.read_and_sync(
            &format!(
                "object-{}.json",
                canonical_digest(IdentityDomain::Data, snapshot)?.as_str()
            ),
            MAX_STORE_BYTES,
        )?;
        let current: ProjectSnapshot = json::parse(&current_bytes)?;
        if current != *snapshot {
            return Err(StoreError::Corrupt(
                "current snapshot changed during durability completion".into(),
            ));
        }
        self.sync_upgrade_packages(packages)?;
        self.require_synced_bytes("CURRENT", pointer_bytes)?;
        self.root.sync()?;
        self.parent.sync()?;
        self.pinned()
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
        if self
            .upgrade_requires_reopen
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StoreError::RestartRequired);
        }
        self.pinned()?;
        let _lock = self.root.lock()?;
        progress(UpgradeProgress::Validating);
        self.pinned()?;
        let pointer_bytes = self.root.read("CURRENT", 16 * 1024)?;
        let pointer: Pointer = json::parse(&pointer_bytes)?;
        if pointer.magic != MAGIC {
            return Err(StoreError::Corrupt("not a generated-tool project".into()));
        }
        if pointer.version == FORMAT {
            let current = self.load()?;
            let packages = self.upgrade_packages(&current)?;
            verify(&current)?;
            resume(&current)?;
            self.complete_current_durability(&current, &pointer_bytes, &packages)
                .map_err(uncertain)?;
            return summary(&current, FORMAT);
        }
        if !matches!(pointer.version, 1 | 2) {
            return Err(StoreError::UnsupportedFormat(pointer.version));
        }
        let old_name = format!("object-{}.json", pointer.object.as_str());
        let old_bytes = self.root.read(&old_name, MAX_STORE_BYTES)?;
        let conversion = VerifiedContainerUpgrade::from_pointer(&old_bytes, &pointer)?;
        let target = conversion.snapshot();
        let packages = self.upgrade_packages(target)?;
        verify(target)?;
        resume(target)?;
        let target_bytes = canonical_bytes(target)?;
        let target_name = format!("object-{}.json", conversion.target_digest().as_str());
        self.publish_upgrade_object(&target_name, &target_bytes)?;
        progress(UpgradeProgress::Staged);
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeObject) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeObject));
        }
        let journal_name = conversion.journal_name();
        let journal_bytes = canonical_bytes(&conversion.journal())?;
        self.publish_upgrade_object(&journal_name, &journal_bytes)?;
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeJournal) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeJournal));
        }
        self.require_synced_bytes(&target_name, &target_bytes)?;
        self.require_synced_bytes(&journal_name, &journal_bytes)?;
        self.require_synced_bytes(&old_name, &old_bytes)?;
        progress(UpgradeProgress::Verified);
        progress(UpgradeProgress::Activating);
        self.pinned()?;
        self.sync_upgrade_packages(&packages)?;
        self.require_synced_bytes(&target_name, &target_bytes)?;
        self.require_synced_bytes(&journal_name, &journal_bytes)?;
        self.require_synced_bytes(&old_name, &old_bytes)?;
        self.require_synced_bytes("CURRENT", &pointer_bytes)?;
        self.root.sync()?;
        // Once activation is attempted, this handle cannot edit or retry.
        // An error may follow a visible pointer: reopen and explicitly finish
        // the durability barriers, never claim rollback or delete staged data.
        self.upgrade_requires_reopen
            .store(true, std::sync::atomic::Ordering::Release);
        self.save(
            target,
            true,
            #[cfg(test)]
            fault,
        )
        .map_err(uncertain)?;
        self.parent
            .sync()
            .map_err(StoreError::from)
            .map_err(uncertain)?;
        progress(UpgradeProgress::RestartRequired);
        summary(target, pointer.version)
    }
    fn publish_upgrade_object(&self, name: &str, bytes: &[u8]) -> Result<()> {
        match self.root.publish(name, bytes, false) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                self.require_synced_bytes(name, bytes)?;
                self.root.sync()?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeInbox {
    version: u32,
    migration: String,
    backup: Digest,
    original: Digest,
    target: Digest,
    bundle: Digest,
}
impl ProductStore {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recover_upgraded_with<F, P>(
        path: &Path,
        upgrade: &VerifiedContainerUpgrade,
        backup: &Digest,
        bundle: &Digest,
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
            backup,
            bundle,
            fresh_only,
            before_activate,
            progress,
            #[cfg(test)]
            fault,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recover_upgraded_selected<F, P>(
        destination: &RecoveryDestination,
        upgrade: &VerifiedContainerUpgrade,
        backup: &Digest,
        bundle: &Digest,
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
            backup,
            bundle,
            true,
            before_activate,
            progress,
            #[cfg(test)]
            fault,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn recover_upgraded_at<F, P>(
        path: &Path,
        selected: Option<&RecoveryDestination>,
        upgrade: &VerifiedContainerUpgrade,
        backup: &Digest,
        bundle: &Digest,
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
        let verified = VerifiedContainerUpgrade::decode(upgrade.original())?;
        if verified.snapshot() != upgrade.snapshot()
            || verified.original_digest() != upgrade.original_digest()
        {
            return Err(StoreError::Corrupt("upgrade input changed".into()));
        }
        let snapshot = verified.snapshot();
        resume(snapshot)?;
        let marker = canonical_bytes(&UpgradeInbox {
            version: 2,
            migration: MIGRATION.into(),
            backup: backup.clone(),
            original: verified.original_digest().clone(),
            target: verified.target_digest().clone(),
            bundle: bundle.clone(),
        })?;
        let (parent, name) = match selected {
            Some(selected) => selected.checked_parts()?,
            None => {
                let (parent, name) = Self::location(path)?;
                (Arc::new(parent), name)
            }
        };
        let (root, fresh) = match parent.create_child(&name) {
            Ok(root) => (root, true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && !fresh_only => {
                (parent.child(&name)?, false)
            }
            Err(e) => return Err(e.into()),
        };
        // Inspect before even creating a lock file in a legacy/unknown inbox.
        if !fresh && root.read("UPGRADE-INBOX", MAX_STORE_BYTES).ok().as_ref() != Some(&marker) {
            return Err(StoreError::Conflict("destination is not this exact new upgrade inbox; preserve it and recover the original backup into a fresh folder".into()));
        }
        let store = Self {
            parent,
            root: Arc::new(root),
            name,
            upgrade_requires_reopen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            validation_cache: Arc::new(Mutex::new(ValidationCache::default())),
        };
        let _lock = store.root.lock()?;
        store.pinned()?;
        if fresh {
            store.publish_upgrade_object("UPGRADE-INBOX", &marker)?;
        } else {
            store.require_synced_bytes("UPGRADE-INBOX", &marker)?;
        }
        let original_name = format!("object-{}.json", verified.original_digest().as_str());
        let object_name = format!("object-{}.json", verified.target_digest().as_str());
        let object_bytes = canonical_bytes(snapshot)?;
        let journal_name = verified.journal_name();
        let journal_bytes = canonical_bytes(&verified.journal())?;
        match store.root.read("CURRENT", 16 * 1024) {
            Ok(pointer_bytes) => {
                if store.load()? != *snapshot {
                    return Err(StoreError::Conflict("upgrade destination has different or later work; keep that work and reopen it".into()));
                }
                // Activated retries never repair missing preserved inputs.
                store.require_synced_bytes(&original_name, verified.original())?;
                store.require_synced_bytes(&journal_name, &journal_bytes)?;
                before_activate(&store, snapshot)?;
                let packages = store.upgrade_packages(snapshot)?;
                progress(UpgradeProgress::Verified);
                // Verified is a pre-completion notification. Retained inputs
                // must still match after callbacks, before durable success.
                store.require_synced_bytes(&original_name, verified.original())?;
                store.require_synced_bytes(&journal_name, &journal_bytes)?;
                store.require_synced_bytes("UPGRADE-INBOX", &marker)?;
                store
                    .complete_current_durability(snapshot, &pointer_bytes, &packages)
                    .map_err(uncertain)?;
                progress(UpgradeProgress::RestartRequired);
                return summary(snapshot, verified.source_version());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        store.require_unactivated()?;
        store.publish_upgrade_object(&original_name, verified.original())?;
        store.publish_upgrade_object(&object_name, &object_bytes)?;
        progress(UpgradeProgress::Staged);
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeObject) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeObject));
        }
        store.publish_upgrade_object(&journal_name, &journal_bytes)?;
        #[cfg(test)]
        if fault == Some(FaultPoint::AfterUpgradeJournal) {
            return Err(StoreError::Interrupted(FaultPoint::AfterUpgradeJournal));
        }
        before_activate(&store, snapshot)?;
        let packages = store.upgrade_packages(snapshot)?;
        progress(UpgradeProgress::Verified);
        progress(UpgradeProgress::Activating);
        if let Some(selected) = selected {
            selected.check()?;
        }
        store.pinned()?;
        store.require_synced_bytes(&object_name, &object_bytes)?;
        store.require_synced_bytes(&journal_name, &journal_bytes)?;
        store.require_synced_bytes(&original_name, verified.original())?;
        store.require_synced_bytes("UPGRADE-INBOX", &marker)?;
        store.sync_upgrade_packages(&packages)?;
        store.root.sync()?;
        store
            .upgrade_requires_reopen
            .store(true, std::sync::atomic::Ordering::Release);
        store
            .save(
                snapshot,
                false,
                #[cfg(test)]
                fault,
            )
            .map_err(uncertain)?;
        store
            .parent
            .sync()
            .map_err(StoreError::from)
            .map_err(uncertain)?;
        progress(UpgradeProgress::RestartRequired);
        summary(snapshot, verified.source_version())
    }
}
