//! Bounded restart associations, never a second project store or provider grant.
use super::*;
use crate::product_locations::Folder;
use serde::{Deserialize, Serialize};
use std::fs::File;
const JOURNAL_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Association {
    pub path: PathBuf,
    pub identity: ToolIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Basis {
    pub snapshot: Digest,
    pub source: Digest,
    pub revision: u64,
    pub data_generation: u64,
    pub data: Digest,
    pub session: Digest,
    pub decisions: Digest,
    pub day: i32,
    pub runtime: String,
    pub driver: String,
}
impl Basis {
    pub fn capture(snapshot: &ProjectSnapshot) -> Result<Self, String> {
        Ok(Self {
            snapshot: canonical_digest(IdentityDomain::Evidence, snapshot).map_err(error)?,
            source: snapshot.active_revision.clone(),
            revision: snapshot.revision,
            data_generation: snapshot.data.generation,
            data: canonical_digest(IdentityDomain::Data, &snapshot.data).map_err(error)?,
            session: canonical_digest(IdentityDomain::Session, &snapshot.session).map_err(error)?,
            decisions: canonical_digest(IdentityDomain::Decision, &snapshot.decisions)
                .map_err(error)?,
            day: snapshot.clock_day,
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderAssociation {
    pub request: DevelopmentRequest,
    pub provider: ProviderKind,
    pub profile: CapabilityProfile,
    pub wire_request: String,
    pub wire_source: String,
    pub issued: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Interrupted {
    Create {
        tool: Association,
    },
    Daily {
        tool: Association,
        basis: Basis,
        operation: Id,
        input: SemanticInput,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    magic: String,
    version: u32,
    pub need: String,
    pub last: Option<Association>,
    pub pending: Option<Interrupted>,
    pub provider: Option<ProviderAssociation>,
    pub abandoned_creation: Option<Association>,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            magic: "gitmanager.generated-tool-host".into(),
            version: 1,
            need: String::new(),
            last: None,
            pending: None,
            provider: None,
            abandoned_creation: None,
        }
    }
}
impl Journal {
    fn validate(&self) -> Result<(), String> {
        if self.magic != "gitmanager.generated-tool-host"
            || self.version != 1
            || self.need.len() > MAX_TEXT_BYTES
        {
            return Err("The restart information is damaged or belongs to an unsupported version; it was kept unchanged".into());
        }
        let check = |tool: &Association| -> Result<(), String> {
            if !tool.path.is_absolute() || !valid_id(&tool.identity.project_id) {
                return Err("Invalid restart tool association".into());
            }
            Ok(())
        };
        if let Some(tool) = &self.abandoned_creation {
            check(tool)?;
        }
        if let Some(tool) = &self.last {
            check(tool)?;
        }
        if let Some(ProviderAssociation {
            request,
            provider,
            profile,
            wire_request,
            wire_source,
            ..
        }) = &self.provider
        {
            request.validate().map_err(error)?;
            if request.operation != DevelopmentOperation::Generate
                || !request.sources.is_empty()
                || request.request != self.need
            {
                return Err("Invalid interrupted generation association".into());
            }
            let wire = encode_request(
                request,
                &ProviderOptions {
                    provider: *provider,
                    profile: *profile,
                    ..ProviderOptions::default()
                },
            )
            .map_err(error)?;
            if wire.digest()? != *wire_request || wire.source_digest != *wire_source {
                return Err("Interrupted generation identity changed".into());
            }
        }
        match &self.pending {
            Some(Interrupted::Create { tool }) => check(tool)?,
            Some(Interrupted::Daily {
                tool,
                operation,
                input,
                ..
            }) => {
                check(tool)?;
                if !valid_id(operation) {
                    return Err("Invalid interrupted operation identity".into());
                }
                validate_input_shape(input).map_err(error)?;
            }
            None => (),
        }
        Ok(())
    }
}
pub(super) struct JournalFile {
    folder: Folder,
    // One generated-tool host writer per profile. Release happens on the worker.
    _lock: File,
    pub value: Journal,
}
impl JournalFile {
    pub fn open(root: &Path) -> Result<Self, String> {
        let folder = Folder::ensure(&root.join("studio")).map_err(error)?;
        let lock = folder
            .lock()
            .map_err(|e| format!("Generated-tool restart information is busy: {e}"))?;
        let value = match std::fs::symlink_metadata(folder.path().join("session.json")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Journal::default(),
            Err(e) => return Err(error(e)),
            Ok(_) => {
                let bytes = folder.read("session.json", JOURNAL_LIMIT).map_err(error)?;
                let json = crate::tool_proposal_input::parse_json_bytes(&bytes).map_err(error)?;
                serde_json::from_value(json).map_err(|e| {
                    format!("The restart information could not be read and was kept unchanged: {e}")
                })?
            }
        };
        let result = Self {
            folder,
            _lock: lock,
            value,
        };
        result.value.validate()?;
        Ok(result)
    }
    pub fn write(&mut self, value: Journal) -> Result<(), String> {
        value.validate()?;
        let bytes = canonical_bytes(&value).map_err(error)?;
        if bytes.len() > JOURNAL_LIMIT {
            return Err(
                "This input exceeds the bounded restart record; nothing was submitted".into(),
            );
        }
        self.folder
            .publish("session.json", &bytes, true)
            .map_err(error)?;
        self.folder.sync().map_err(error)?;
        self.value = value;
        Ok(())
    }
}
