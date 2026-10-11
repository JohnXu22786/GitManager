//! Current disk representation. Domain requests and wire identities are unchanged.
use super::*;
use crate::product_discovery::ExactUtf8DevelopmentRequest;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredJournalV2 {
    magic: String,
    version: u32,
    need: String,
    last: Option<Association>,
    pending: Option<Interrupted>,
    provider: Option<StoredProviderV2>,
    abandoned_creation: Option<Association>,
    last_unsaved: Option<UnsavedInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    local_file: Option<FileAttempt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recovery_handoffs: Vec<RecoveryHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<TaskAssociation>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProviderV2 {
    request: ExactUtf8DevelopmentRequest,
    provider: ProviderKind,
    profile: CapabilityProfile,
    wire_request: String,
    wire_source: String,
    issued: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modify: Option<(Association, Basis)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reconcile: Option<ReconcileAssociation>,
}
impl StoredJournalV2 {
    pub(super) fn from_domain(value: &Journal) -> Result<Self, String> {
        value.validate()?;
        let provider = value
            .provider
            .as_ref()
            .map(|provider| {
                Ok::<_, String>(StoredProviderV2 {
                    request: ExactUtf8DevelopmentRequest::from_request(&provider.request)
                        .map_err(error)?,
                    provider: provider.provider,
                    profile: provider.profile,
                    wire_request: provider.wire_request.clone(),
                    wire_source: provider.wire_source.clone(),
                    issued: provider.issued,
                    modify: provider.modify.clone(),
                    reconcile: provider.reconcile.clone(),
                })
            })
            .transpose()?;
        Ok(Self {
            magic: value.magic.clone(),
            version: value.version,
            need: value.need.clone(),
            last: value.last.clone(),
            pending: value.pending.clone(),
            provider,
            abandoned_creation: value.abandoned_creation.clone(),
            last_unsaved: value.last_unsaved.clone(),
            local_file: value.local_file.clone(),
            recovery_handoffs: value.recovery_handoffs.clone(),
            task: value.task.clone(),
        })
    }
    fn into_domain(self) -> Result<Journal, String> {
        let provider = self
            .provider
            .map(|provider| {
                Ok::<_, String>(ProviderAssociation {
                    request: provider.request.to_request().map_err(error)?,
                    provider: provider.provider,
                    profile: provider.profile,
                    wire_request: provider.wire_request,
                    wire_source: provider.wire_source,
                    issued: provider.issued,
                    modify: provider.modify,
                    reconcile: provider.reconcile,
                })
            })
            .transpose()?;
        let value = Journal {
            magic: self.magic,
            version: self.version,
            need: self.need,
            last: self.last,
            pending: self.pending,
            provider,
            abandoned_creation: self.abandoned_creation,
            last_unsaved: self.last_unsaved,
            local_file: self.local_file,
            recovery_handoffs: self.recovery_handoffs,
            task: self.task,
        };
        value.validate()?;
        Ok(value)
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<Journal, String> {
    let json = crate::tool_proposal_input::parse_json_bytes(bytes).map_err(error)?;
    if json.get("version").and_then(serde_json::Value::as_u64) == Some(1) {
        return Err("The saved restart information requires a startup upgrade. Open GitManager to finish that upgrade; this read did not change it".into());
    }
    let stored: StoredJournalV2 = serde_json::from_value(json).map_err(|e| {
        format!("The restart information could not be read and was kept unchanged: {e}")
    })?;
    stored.into_domain()
}
pub(super) fn encode(value: &Journal) -> Result<Vec<u8>, String> {
    let bytes = canonical_bytes(&StoredJournalV2::from_domain(value)?).map_err(error)?;
    if bytes.len() > JOURNAL_LIMIT {
        return Err("This input exceeds the bounded restart record; nothing was submitted".into());
    }
    // Check the complete containing record, including the envelope's extra depth.
    crate::tool_proposal_input::parse_json_bytes(&bytes).map_err(error)?;
    Ok(bytes)
}
