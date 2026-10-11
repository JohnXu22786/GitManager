//! Startup-only v1 intake. No provider lookup, request renewal or data-store upgrade.
use super::*;

pub(super) enum Outcome {
    Current,
    // An inert byte identity, never a decoded request or an open writer handle.
    RestartRequired { digest: String },
}

// Frozen v1 inventory. Ordinary reads never deserialize this representation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJournalV1 {
    magic: String,
    version: u32,
    need: String,
    last: Option<Association>,
    pending: Option<Interrupted>,
    provider: Option<LegacyProviderV1>,
    abandoned_creation: Option<Association>,
    last_unsaved: Option<UnsavedInput>,
    #[serde(default)]
    local_file: Option<FileAttempt>,
    #[serde(default)]
    recovery_handoffs: Vec<RecoveryHandoff>,
    #[serde(default)]
    task: Option<TaskAssociation>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProviderV1 {
    request: DevelopmentRequest,
    provider: ProviderKind,
    profile: CapabilityProfile,
    wire_request: String,
    wire_source: String,
    issued: bool,
    #[serde(default)]
    modify: Option<(Association, Basis)>,
    #[serde(default)]
    reconcile: Option<ReconcileAssociation>,
}
impl LegacyJournalV1 {
    fn into_domain(self) -> Result<Journal, String> {
        if self.magic != "gitmanager.generated-tool-host" || self.version != 1 {
            return Err(
                "The restart information belongs to an unsupported version; it was kept unchanged"
                    .into(),
            );
        }
        let value = Journal {
            magic: self.magic,
            version: 2,
            need: self.need,
            last: self.last,
            pending: self.pending,
            provider: self.provider.map(|provider| ProviderAssociation {
                request: provider.request,
                provider: provider.provider,
                profile: provider.profile,
                wire_request: provider.wire_request,
                wire_source: provider.wire_source,
                issued: provider.issued,
                modify: provider.modify,
                reconcile: provider.reconcile,
            }),
            abandoned_creation: self.abandoned_creation,
            last_unsaved: self.last_unsaved,
            local_file: self.local_file,
            recovery_handoffs: self.recovery_handoffs,
            task: self.task,
        };
        // The exact pre-upgrade semantics, including purpose, source, disclosure,
        // task and legacy wire digest checks, are shared with the domain writer.
        value.validate()?;
        Ok(value)
    }
}
fn progress(root: &Path, report: &mut impl FnMut(u8), stage: u8) -> Result<(), String> {
    report(stage);
    #[cfg(test)]
    checkpoint(root, TestPoint::Stage(stage))?;
    #[cfg(not(test))]
    let _ = root;
    Ok(())
}
fn verify_backup(folder: &Folder, name: &str, original: &[u8]) -> Result<(), String> {
    let retained = folder
        .read(name, JOURNAL_LIMIT)
        .map_err(|e| format!("The original restart backup could not be verified: {e:?}"))?;
    if retained != original
        || crate::product_provider::digest(&retained) != crate::product_provider::digest(original)
    {
        return Err("The original restart backup has different bytes. Both files were kept; the upgrade is blocked".into());
    }
    folder
        .sync()
        .map_err(|e| format!("The original restart backup could not be synced: {e:?}"))
}
fn preserve(root: &Path, folder: &Folder, original: &[u8]) -> Result<(), String> {
    let name = format!(
        "session-v1-{}.json",
        crate::product_provider::digest(original)
    );
    match std::fs::symlink_metadata(folder.path().join(&name)) {
        Ok(_) => return verify_backup(folder, &name, original),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(format!("The original restart backup is unavailable: {e}")),
    }
    #[cfg(test)]
    checkpoint(root, TestPoint::BeforeBackupPublish)?;
    #[cfg(not(test))]
    let _ = root;
    // No-clobber publication also covers a competitor creating the name after
    // inspection. Only exact, bounded ordinary bytes may satisfy that race.
    let published = folder.publish(&name, original, false);
    verify_backup(folder, &name, original)
        .map_err(|e| format!("{e}. Backup publication: {published:?}"))
}
fn activate(root: &Path, folder: &Folder, original: &[u8], current: &[u8]) -> Result<(), String> {
    #[cfg(test)]
    checkpoint(root, TestPoint::BeforePublish)?;
    let published = folder.publish("session.json", current, true).map_err(error);
    #[cfg(test)]
    let published = published.and_then(|()| checkpoint(root, TestPoint::AfterRename));
    // Folder::publish can fail after rename. Never infer non-activation from
    // its error, restore a backup blindly, or install the old decoded value.
    #[cfg(test)]
    checkpoint(root, TestPoint::BeforeReadback).map_err(|e| {
        format!("Restart activation outcome is uncertain: {e}. Keep both files and restart")
    })?;
    let observed = folder.read("session.json", JOURNAL_LIMIT).map_err(|e| {
        format!("Restart activation outcome is uncertain: {e:?}. Keep both files and restart")
    })?;
    if observed == current {
        format::decode(&observed)
            .map_err(|e| format!("Activated restart information could not be verified: {e}"))?;
        #[cfg(test)]
        checkpoint(root, TestPoint::BeforeSync).map_err(|e| format!("Restart information was activated but durability is unconfirmed: {e}. Restart before continuing"))?;
        #[cfg(not(test))]
        let _ = root;
        folder.sync().map_err(|e| format!("Restart information was activated but durability is unconfirmed: {e:?}. Restart before continuing"))?;
        Ok(())
    } else if observed == original {
        Err(format!("Restart information could not be activated; the original is unchanged and its backup is kept. {published:?}"))
    } else {
        Err("Restart activation outcome is uncertain: the current file differs from both verified records. Both files were kept; restart information remains blocked".into())
    }
}
fn run_locked(root: &Path, report: &mut impl FnMut(u8)) -> Result<Outcome, String> {
    let folder = Folder::ensure(&root.join("studio")).map_err(error)?;
    let _lock = folder
        .lock()
        .map_err(|e| format!("Generated-tool restart information is busy: {e}"))?;
    let original = match read_optional(&folder)? {
        None => return Ok(Outcome::Current),
        Some(bytes) => bytes,
    };
    let json = crate::tool_proposal_input::parse_json_bytes(&original).map_err(error)?;
    match json.get("version").and_then(serde_json::Value::as_u64) {
        Some(2) => { format::decode(&original)?; return Ok(Outcome::Current); }
        Some(1) => (),
        _ => return Err("The restart information is damaged or belongs to an unsupported version; it was kept unchanged".into()),
    }
    progress(root, report, 6)?;
    let legacy: LegacyJournalV1 = serde_json::from_value(json).map_err(error)?;
    let value = legacy.into_domain()?;
    let current = format::encode(&value)?;
    let reconstructed = format::decode(&current)?;
    if canonical_bytes(&value).map_err(error)? != canonical_bytes(&reconstructed).map_err(error)? {
        return Err(
            "The exact restart information did not survive verification; the original was kept"
                .into(),
        );
    }
    #[cfg(test)]
    checkpoint(root, TestPoint::BeforeBackup)?;
    preserve(root, &folder, &original)?;
    progress(root, report, 7)?;
    #[cfg(test)]
    checkpoint(root, TestPoint::AfterBackup)?;
    progress(root, report, 8)?;
    progress(root, report, 9)?;
    activate(root, &folder, &original, &current)?;
    Ok(Outcome::RestartRequired {
        digest: crate::product_provider::digest(&current),
    })
}
pub(super) fn run(root: &Path, mut report: impl FnMut(u8)) -> Result<Outcome, String> {
    let result = run_locked(root, &mut report);
    // The Folder, writer lock and every decoded migration value have gone out
    // of scope. Only the current byte identity can cross this restart boundary.
    #[cfg(test)]
    checkpoint(root, TestPoint::Released)?;
    result
}

// Root-keyed deterministic fault/pause seam. No hooks exist in production, and
// callbacks never run under the global registry mutex or affect another root.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TestPoint {
    Stage(u8),
    BeforeBackup,
    BeforeBackupPublish,
    AfterBackup,
    BeforePublish,
    AfterRename,
    BeforeReadback,
    BeforeSync,
    Released,
    BeforeOpen,
    Opened,
}
#[cfg(test)]
type Hook = Arc<dyn Fn(TestPoint) -> Result<(), String> + Send + Sync>;
#[cfg(test)]
fn hooks() -> &'static std::sync::Mutex<std::collections::BTreeMap<PathBuf, Hook>> {
    static HOOKS: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<PathBuf, Hook>>> =
        std::sync::OnceLock::new();
    HOOKS.get_or_init(Default::default)
}
#[cfg(test)]
pub(super) struct TestHook(PathBuf);
#[cfg(test)]
impl Drop for TestHook {
    fn drop(&mut self) {
        hooks().lock().unwrap().remove(&self.0);
    }
}
#[cfg(test)]
pub(super) fn test_hook(
    root: &Path,
    hook: impl Fn(TestPoint) -> Result<(), String> + Send + Sync + 'static,
) -> TestHook {
    assert!(hooks()
        .lock()
        .unwrap()
        .insert(root.into(), Arc::new(hook))
        .is_none());
    TestHook(root.into())
}
#[cfg(test)]
pub(super) fn checkpoint(root: &Path, point: TestPoint) -> Result<(), String> {
    let hook = hooks().lock().unwrap().get(root).cloned();
    hook.map_or(Ok(()), |hook| hook(point))
}
