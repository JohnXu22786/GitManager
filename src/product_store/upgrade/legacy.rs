//! Closed old DTOs exist only behind explicit upgrade/intake gates.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotV1 {
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
struct SnapshotV2 {
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
    scope: scope::ScopeState,
}

/// Integrity authority for one exact wrapping conversion, never adoption
/// authority. Construction and mutation remain confined to this module.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedContainerUpgrade {
    original: Vec<u8>,
    original_digest: Digest,
    source_version: u32,
    snapshot: ProjectSnapshot,
    target_digest: Digest,
    preserved_payload: Digest,
}
impl VerifiedContainerUpgrade {
    /// Backups require canonical bytes; in-place input may retain whitespace.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let conversion = Self::decode_source(bytes)?;
        let original: serde_json::Value = json::parse(bytes)?;
        if canonical_bytes(&original)? != bytes {
            return Err(StoreError::Corrupt("noncanonical old snapshot".into()));
        }
        Ok(conversion)
    }
    pub(super) fn from_pointer(bytes: &[u8], pointer: &Pointer) -> Result<Self> {
        let conversion = Self::decode_source(bytes)?;
        if conversion.original_digest != pointer.object
            || conversion.source_version != pointer.version
            || conversion.snapshot.revision != pointer.revision
            || conversion.snapshot.data.project_id != pointer.project_id
        {
            return Err(StoreError::Corrupt(
                "old snapshot checksum or pointer identity mismatch".into(),
            ));
        }
        Ok(conversion)
    }
    fn decode_source(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid(
                "old snapshot exceeds store budget".into(),
            ));
        }
        let value: serde_json::Value = json::parse(bytes)?;
        let version = value
            .get("version")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| StoreError::Corrupt("invalid old snapshot version".into()))?;
        // Serialize the exact selected DTO, without a new enum wrapper.
        let (canonical, mut target) = match version {
            1 => {
                let old: SnapshotV1 = serde_json::from_value(value)?;
                let canonical = canonical_bytes(&old)?;
                let mut target = serde_json::to_value(old)?;
                target["scope"] = serde_json::to_value(scope::ScopeEnvelope::ScalarV1(
                    scope::ScopeState::default(),
                ))?;
                (canonical, target)
            }
            2 => {
                let old: SnapshotV2 = serde_json::from_value(value)?;
                let canonical = canonical_bytes(&old)?;
                let mut target = serde_json::to_value(&old)?;
                target["scope"] = serde_json::to_value(scope::ScopeEnvelope::ScalarV1(old.scope))?;
                (canonical, target)
            }
            version => return Err(StoreError::UnsupportedFormat(version)),
        };
        target["version"] = serde_json::json!(FORMAT);
        let snapshot: ProjectSnapshot = serde_json::from_value(target)?;
        snapshot.validate()?;
        let mut projected = serde_json::to_value(&snapshot)?;
        projected["version"] = serde_json::json!(version);
        if version == 1 {
            if snapshot.scalar_scope()? != &scope::ScopeState::default() {
                return Err(StoreError::Corrupt(
                    "format-1 conversion added scope history".into(),
                ));
            }
            projected.as_object_mut().unwrap().remove("scope");
        } else {
            projected["scope"] = serde_json::to_value(snapshot.scalar_scope()?)?;
        }
        if canonical_bytes(&projected)? != canonical {
            return Err(StoreError::Corrupt(
                "conversion changed retained payload".into(),
            ));
        }
        if canonical_bytes(&snapshot)?.len() > MAX_STORE_BYTES {
            return Err(StoreError::Invalid(
                "upgraded snapshot exceeds store budget".into(),
            ));
        }
        let original: serde_json::Value = json::parse(&canonical)?;
        Ok(Self {
            original: bytes.to_vec(),
            original_digest: canonical_digest(IdentityDomain::Data, &original)?,
            source_version: version,
            target_digest: canonical_digest(IdentityDomain::Data, &snapshot)?,
            preserved_payload: canonical_digest(
                IdentityDomain::Evidence,
                &("container-preservation/1", canonical),
            )?,
            snapshot,
        })
    }
    pub(crate) fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }
    pub(crate) fn original_digest(&self) -> &Digest {
        &self.original_digest
    }
    pub(super) fn original(&self) -> &[u8] {
        &self.original
    }
    pub(super) fn source_version(&self) -> u32 {
        self.source_version
    }
    pub(super) fn target_digest(&self) -> &Digest {
        &self.target_digest
    }
    pub(super) fn preserved_payload(&self) -> &Digest {
        &self.preserved_payload
    }
}
