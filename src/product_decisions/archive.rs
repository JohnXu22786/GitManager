use super::*;
#[path = "../tool_proposal_input.rs"]
mod bounded_input;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Content {
    Scenes {
        scenes: Vec<AcceptedScene>,
    },
    Mappings {
        target: ArtifactRef,
        mappings: Vec<ImplementationMapping>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Object {
    pub(super) version: u32,
    pub(super) content: Content,
}
/// Immutable scene and mapping objects, referenced by the store's atomically
/// committed graph/adoption receipt. Staging never activates an intention.
#[derive(Clone)]
pub struct IntentArchive {
    store: ProductStore,
}
impl IntentArchive {
    pub(super) fn snapshot(&self) -> Result<ProjectSnapshot> {
        Ok(self.store.load()?)
    }
    pub fn new(store: ProductStore) -> Self {
        Self { store }
    }
    pub(super) fn stage(&self, scenes: &[AcceptedScene]) -> Result<Digest> {
        if scenes.is_empty() || scenes.len() > MAX_ITEMS {
            return Err(invalid("accepted scene count is outside limits"));
        }
        for scene in scenes {
            scene.validate()?;
        }
        self.write(Content::Scenes {
            scenes: scenes.to_vec(),
        })
    }
    pub(super) fn stage_mappings(
        &self,
        target: &ArtifactRef,
        mappings: &[ImplementationMapping],
    ) -> Result<Digest> {
        target.validate()?;
        if mappings.len() > MAX_ITEMS {
            return Err(invalid("mapping count exceeds limits"));
        }
        self.write(Content::Mappings {
            target: target.clone(),
            mappings: mappings.to_vec(),
        })
    }
    pub(super) fn write(&self, content: Content) -> Result<Digest> {
        let object = Object {
            version: 1,
            content,
        };
        let bytes = canonical_bytes(&object)?;
        if bytes.len() > crate::product_store::MAX_INTENTION_OBJECT_BYTES {
            return Err(invalid("intention object exceeds archive byte limit"));
        }
        let digest = canonical_digest(IdentityDomain::Evidence, &object)?;
        if self.store.stage_extension(&bytes)? != digest {
            return Err(invalid(
                "store extension identity differs from typed intention object",
            ));
        }
        Ok(digest)
    }
    pub(super) fn read(&self, digest: &Digest) -> Result<Content> {
        decode(&self.store.read_extension(digest)?, digest)
    }
    pub(super) fn load(&self, digest: &Digest) -> Result<Vec<AcceptedScene>> {
        let Content::Scenes { scenes } = self.read(digest)? else {
            return Err(invalid("expected accepted scenes, found another object"));
        };
        if scenes.is_empty() || scenes.len() > MAX_ITEMS {
            return Err(invalid("accepted scene count invalid"));
        }
        for s in &scenes {
            s.validate()?;
        }
        Ok(scenes)
    }
    pub(super) fn load_mappings(
        &self,
        digest: &Digest,
        target: &ArtifactRef,
    ) -> Result<Vec<ImplementationMapping>> {
        let Content::Mappings {
            target: bound,
            mappings,
        } = self.read(digest)?
        else {
            return Err(invalid("adoption mapping receipt is missing"));
        };
        if bound != *target || mappings.len() > MAX_ITEMS {
            return Err(invalid("mapping receipt belongs to another program"));
        }
        Ok(mappings)
    }
}
pub(super) fn decode(bytes: &[u8], digest: &Digest) -> Result<Content> {
    let value = bounded_input::parse_json_bytes_with_limit(
        bytes,
        crate::product_store::MAX_INTENTION_OBJECT_BYTES,
    )
    .map_err(|e| invalid(&e.to_string()))?;
    let object: Object = serde_json::from_value(value).map_err(|e| invalid(&e.to_string()))?;
    if bytes != canonical_bytes(&object)?
        || object.version != 1
        || canonical_digest(IdentityDomain::Evidence, &object)? != *digest
    {
        return Err(invalid("intention object version or identity mismatch"));
    }
    Ok(object.content)
}
