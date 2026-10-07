//! Portable immutable objects. Backup validation is not execution authority;
//! restored intentions are always rerun before later behavior adoption.
use super::archive::{decode, validate_rehearsal_scenes, Content, Object};
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledObject {
    digest: Digest,
    bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentionBundle {
    version: u32,
    snapshot: Digest,
    objects: Vec<BundledObject>,
}
const MAX_BUNDLE_BYTES: usize = 32 * 1024 * 1024;
impl IntentionBundle {
    pub fn snapshot_digest(&self) -> &Digest {
        &self.snapshot
    }
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}
fn reachable(snapshot: &ProjectSnapshot) -> BTreeSet<Digest> {
    snapshot
        .decisions
        .decisions
        .iter()
        .map(|d| d.witness.clone())
        .chain(
            snapshot
                .adoptions
                .iter()
                .filter_map(|a| a.plan.evidence.first().cloned()),
        )
        .collect()
}
pub fn validate_bundle(snapshot: &ProjectSnapshot, bundle: &IntentionBundle) -> Result<()> {
    snapshot.validate()?;
    validate_withdrawal_history(snapshot)?;
    if bundle.version != 1
        || bundle.snapshot != canonical_digest(IdentityDomain::Data, snapshot)?
        || bundle.objects.len() > MAX_ITEMS * 2
        || bundle.objects.iter().map(|o| o.bytes.len()).sum::<usize>() > MAX_BUNDLE_BYTES
        || canonical_bytes(bundle)?.len() > MAX_BUNDLE_BYTES
    {
        return Err(invalid(
            "intention bundle version, snapshot identity or size is invalid",
        ));
    }
    let expected = reachable(snapshot);
    let mut found = BTreeSet::new();
    let mut sources = vec![];
    let mut accepted = vec![];
    let mut recipes = vec![];
    for object in &bundle.objects {
        if !found.insert(object.digest.clone()) || !expected.contains(&object.digest) {
            return Err(invalid("duplicate or unrelated intention object"));
        }
        let content = decode(&object.bytes, &object.digest)?;
        match content {
            Content::Scenes { scenes } => {
                if snapshot
                    .adoptions
                    .iter()
                    .any(|a| a.plan.evidence.first() == Some(&object.digest))
                {
                    return Err(invalid(
                        "scene object is also referenced as a mapping receipt",
                    ));
                }
                if scenes.is_empty() || scenes.len() > MAX_ITEMS {
                    return Err(invalid("invalid accepted scene count"));
                }
                for s in &scenes {
                    s.validate()?;
                    ScopedExecutionContext::committed(snapshot)?.verify_seed(
                        &s.program,
                        &s.scenario.seed,
                        s.scenario.clock_day,
                    )?;
                    if !sources.iter().any(|p: &CapturedProgram| {
                        p.artifact.program_digest == s.program.artifact.program_digest
                    }) {
                        sources.push(s.program.clone());
                    }
                    if s.program.binding.project_id != snapshot.data.project_id {
                        return Err(invalid("accepted scene belongs to another project"));
                    }
                }
                validate_rehearsal_scenes(snapshot, &object.digest, &scenes)?;
                accepted.extend(scenes.clone());
                let ids = scenes
                    .iter()
                    .map(|s| s.scenario.identity())
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                for d in snapshot
                    .decisions
                    .decisions
                    .iter()
                    .filter(|d| d.witness == object.digest)
                {
                    validate_scene_bindings(d, &scenes)?;
                    if d.scenarios != ids {
                        return Err(invalid(
                            "restored scene identities do not match the decision",
                        ));
                    }
                }
                if !snapshot
                    .decisions
                    .decisions
                    .iter()
                    .any(|d| d.witness == object.digest)
                {
                    return Err(invalid("mapping receipt was replaced with scenes"));
                }
            }
            Content::Mappings { target, mappings } => {
                if snapshot
                    .decisions
                    .decisions
                    .iter()
                    .any(|d| d.witness == object.digest)
                {
                    return Err(invalid(
                        "mapping object is also referenced as a scene witness",
                    ));
                }
                target.validate()?;
                if mappings.len() > MAX_ITEMS {
                    return Err(invalid("mapping count exceeds limits"));
                }
                for a in snapshot
                    .adoptions
                    .iter()
                    .filter(|a| a.plan.evidence.first() == Some(&object.digest))
                {
                    if a.plan.target != target {
                        return Err(invalid("restored mappings target another executable"));
                    }
                }
                if !snapshot
                    .adoptions
                    .iter()
                    .any(|a| a.plan.evidence.first() == Some(&object.digest))
                {
                    return Err(invalid("decision witness was replaced with mappings"));
                }
                recipes.push((target, mappings));
            }
        }
    }
    for (target, mappings) in recipes {
        let target = snapshot
            .programs
            .iter()
            .find(|p| p.artifact == target)
            .ok_or_else(|| invalid("restored mapping target is not a retained program"))?;
        let mut seen = BTreeSet::new();
        for bound in mappings {
            if !seen.insert(bound.source_program.clone()) || bound.mappings.len() > MAX_ITEMS {
                return Err(invalid(
                    "restored source mappings are ambiguous or exceed limits",
                ));
            }
            let source = sources
                .iter()
                .find(|p| p.artifact.program_digest == bound.source_program)
                .ok_or_else(|| invalid("restored mapping source has no accepted scene"))?;
            let mapping = Mapping::new(&source.program, &target.program, &bound.mappings)?;
            if bound.scenarios.len() > MAX_ITEMS {
                return Err(invalid("restored scenario mapping count exceeds limits"));
            }
            let mut scenarios = BTreeSet::new();
            for replacement in &bound.scenarios {
                if replacement
                    .source_program
                    .as_ref()
                    .is_some_and(|p| p != &bound.source_program)
                {
                    return Err(invalid(
                        "stored scenario mapping disagrees with its accepted source program",
                    ));
                }
                if !scenarios.insert(&replacement.original) {
                    return Err(invalid("ambiguous restored scenario mapping"));
                }
                let original = accepted
                    .iter()
                    .find(|s| {
                        s.program.artifact.program_digest == bound.source_program
                            && s.scenario.identity().ok().as_ref() == Some(&replacement.original)
                    })
                    .ok_or_else(|| {
                        invalid("restored replacement has no accepted source scenario")
                    })?;
                mapping.replacement(
                    &original.scenario,
                    &target.program,
                    &replacement.replacement,
                )?;
            }
        }
    }
    if found != expected {
        return Err(invalid(
            "intention bundle lacks reachable immutable objects",
        ));
    }
    Ok(())
}
impl IntentArchive {
    pub fn export_for(&self, snapshot: &ProjectSnapshot) -> Result<IntentionBundle> {
        let mut objects = vec![];
        for digest in reachable(snapshot) {
            let content = self.read(&digest)?;
            let bytes = canonical_bytes(&Object {
                version: 1,
                content,
            })?;
            objects.push(BundledObject { digest, bytes });
        }
        let bundle = IntentionBundle {
            version: 1,
            snapshot: canonical_digest(IdentityDomain::Data, snapshot)?,
            objects,
        };
        validate_bundle(snapshot, &bundle)?;
        Ok(bundle)
    }
    pub fn restore_for(&self, snapshot: &ProjectSnapshot, bundle: &IntentionBundle) -> Result<()> {
        validate_bundle(snapshot, bundle)?;
        for object in &bundle.objects {
            if self.write(decode(&object.bytes, &object.digest)?)? != object.digest {
                return Err(invalid("restored object identity changed"));
            }
        }
        // No project activation occurs here. The caller publishes CURRENT only after
        // all objects and the exact snapshot are durably validated.
        self.export_for(snapshot)?;
        Ok(())
    }
}

/// Explicit format-upgrade rebinding of the outer snapshot link only. Exact
/// scene/mapping object bytes and their content identities never change.
pub fn upgrade_bundle_binding(
    original: &Digest,
    snapshot: &ProjectSnapshot,
    bundle: &IntentionBundle,
) -> Result<IntentionBundle> {
    if bundle.snapshot != *original || !snapshot.scope.layers.is_empty() {
        return Err(invalid(
            "legacy intention bundle does not bind the original unscoped snapshot",
        ));
    }
    let mut upgraded = bundle.clone();
    upgraded.snapshot = canonical_digest(IdentityDomain::Data, snapshot)?;
    validate_bundle(snapshot, &upgraded)?;
    Ok(upgraded)
}
