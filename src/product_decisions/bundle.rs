//! Portable immutable objects. Backup validation is not execution authority;
//! restored intentions are always rerun before later behavior adoption.
use super::archive::{decode, validate_rehearsal_scenes, Content, Object};
use super::*;
use crate::product_store::scope::LockedArchivePolicy;
use std::mem::size_of;
// Same checked compile-time packet arithmetic as the scope layer, deliberately
// private here rather than widening the scope helper's visibility.
const fn archive_payment<const N: usize>(base: u64, terms: [(usize, u64); N]) -> u64 {
    assert!(usize::BITS <= u64::BITS);
    let mut total = base;
    let mut i = 0;
    while i < N {
        let term = match (terms[i].0 as u64).checked_mul(terms[i].1) {
            Some(value) => value,
            None => panic!("archive payment multiplication overflow"),
        };
        total = match total.checked_add(term) {
            Some(value) => value,
            None => panic!("archive payment addition overflow"),
        };
        i += 1;
    }
    total
}
pub(super) const LOCKED_ARCHIVE_EXPORT_PAYMENT: u64 = archive_payment(128, [
    (size_of::<&IntentArchive>(), 8),
    (size_of::<&ProjectSnapshot>(), 12),
    (size_of::<&LockedArchivePolicy<'_>>(), 12),
    (size_of::<&IntentionBundle>(), 8),
    (size_of::<IntentionBundle>(), 4),
    (size_of::<Result<IntentionBundle>>(), 4),
    (size_of::<Result<()>>(), 12),
    (size_of::<(&Vec<AcceptedScene>, &BTreeMap<Digest, std::ops::Range<usize>>)>(), 8),
]);
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
    /// Byte freshness only, not validation authority. The caller must already
    /// hold a validated immutable bundle for the exact current snapshot.
    pub(crate) fn matches_store_objects(&self, store: &ProductStore) -> Result<bool> {
        for object in &self.objects {
            if store.read_extension(&object.digest)? != object.bytes {
                return Ok(false);
            }
        }
        Ok(true)
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
// The complete ordinary body is expanded lexically, with only the final
// retained-policy call selected. Object/scene/mapping error order stays shared.
macro_rules! validate_bundle_body {
    ($snapshot:ident, $bundle:ident, $retained:path $(, $capsule:ident)?) => {{
    $snapshot.validate()?;
    validate_withdrawal_history($snapshot)?;
    if $bundle.version != 1
        || $bundle.snapshot != canonical_digest(IdentityDomain::Data, $snapshot)?
        || $bundle.objects.len() > MAX_ITEMS * 2
        || $bundle.objects.iter().map(|o| o.bytes.len()).sum::<usize>() > MAX_BUNDLE_BYTES
        || canonical_bytes($bundle)?.len() > MAX_BUNDLE_BYTES
    {
        return Err(invalid(
            "intention bundle version, snapshot identity or size is invalid",
        ));
    }
    let expected = reachable($snapshot);
    let mut found = BTreeSet::new();
    let mut sources = vec![];
    let mut accepted = vec![];
    let mut accepted_ranges = BTreeMap::<Digest,std::ops::Range<usize>>::new();
    let mut recipes = vec![];
    // One immutable committed basis and fixed runtime apply throughout this
    // call. Each scene is still decoded, validated and admitted independently.
    let mut committed_context = None;
    for object in &$bundle.objects {
        if !found.insert(object.digest.clone()) || !expected.contains(&object.digest) {
            return Err(invalid("duplicate or unrelated intention object"));
        }
        let content = decode(&object.bytes, &object.digest)?;
        match content {
            Content::Scenes { scenes } => {
                if $snapshot
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
                    if committed_context.is_none() {
                        committed_context = Some(ScopedExecutionContext::committed($snapshot)?);
                    }
                    committed_context.as_ref().unwrap().verify_seed(
                        &s.program,
                        &s.scenario.seed,
                        s.scenario.clock_day,
                    )?;
                    if !sources.iter().any(|p: &CapturedProgram| {
                        p.artifact.program_digest == s.program.artifact.program_digest
                    }) {
                        sources.push(s.program.clone());
                    }
                    if s.program.binding.project_id != $snapshot.data.project_id {
                        return Err(invalid("accepted scene belongs to another project"));
                    }
                }
                validate_rehearsal_scenes($snapshot, &object.digest, &scenes)?;
                let first = accepted.len();
                accepted.extend(scenes.clone());
                accepted_ranges.insert(object.digest.clone(),first..accepted.len());
                let ids = scenes
                    .iter()
                    .map(|s| s.scenario.identity())
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                for d in $snapshot
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
                if !$snapshot
                    .decisions
                    .decisions
                    .iter()
                    .any(|d| d.witness == object.digest)
                {
                    return Err(invalid("mapping receipt was replaced with scenes"));
                }
            }
            Content::Mappings { target, mappings } => {
                if $snapshot
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
                for a in $snapshot
                    .adoptions
                    .iter()
                    .filter(|a| a.plan.evidence.first() == Some(&object.digest))
                {
                    if a.plan.target != target {
                        return Err(invalid("restored mappings target another executable"));
                    }
                }
                if !$snapshot
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
        let target = $snapshot
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
    $retained($snapshot, $($capsule,)? |check| {
        let mut verify = || -> Result<()> {
            let execution=check.execution()?;
            let basis=execution.current();
            let layer=crate::product_store::scope::transition_layer(execution);
            let ids=owned_policy::retirement_ids(basis,layer)?;
            for id in &ids {
                check.charge(1)?;
                let decision=basis.decisions.decisions.iter().find(|d|&d.id==id)
                    .ok_or_else(||invalid("historical owned retirement decision missing"))?;
                let range=accepted_ranges.get(&decision.witness)
                    .ok_or_else(||invalid("historical owned retirement witness missing"))?;
                owned_policy::check_decision(check,decision,&accepted[range.clone()])?;
            }
            Ok(())
        };
        verify().map_err(owned_transition::into_store_error)
    })?;
    Ok(())
    }};
}
pub fn validate_bundle(snapshot: &ProjectSnapshot, bundle: &IntentionBundle) -> Result<()> {
    validate_bundle_body!(snapshot, bundle, crate::product_store::scope::check_retained_transition_policies)
}
fn validate_bundle_locked(
    snapshot: &ProjectSnapshot, bundle: &IntentionBundle, capsule: &LockedArchivePolicy<'_>,
) -> Result<()> {
    validate_bundle_body!(snapshot, bundle, crate::product_store::scope::check_retained_transition_policies_locked, capsule)
}

macro_rules! export_bundle_body {
    ($archive:ident, $snapshot:ident, $validate:path $(, $capsule:ident)?) => {{
        let mut objects = vec![];
        for digest in reachable($snapshot) {
            let content = $archive.read(&digest)?;
            let bytes = canonical_bytes(&Object {
                version: 1,
                content,
            })?;
            objects.push(BundledObject { digest, bytes });
        }
        let bundle = IntentionBundle {
            version: 1,
            snapshot: canonical_digest(IdentityDomain::Data, $snapshot)?,
            objects,
        };
        $validate($snapshot, &bundle $(, $capsule)?)?;
        Ok(bundle)
    }};
}
impl IntentArchive {
    pub fn export_for(&self, snapshot: &ProjectSnapshot) -> Result<IntentionBundle> {
        export_bundle_body!(self, snapshot, validate_bundle)
    }
    pub(super) fn export_for_locked_withdrawal(
        &self, snapshot: &ProjectSnapshot, capsule: &LockedArchivePolicy<'_>,
    ) -> Result<IntentionBundle> {
        export_bundle_body!(self, snapshot, validate_bundle_locked, capsule)
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
pub(crate) fn upgrade_bundle_binding(
    conversion: &crate::product_store::VerifiedContainerUpgrade,
    bundle: &IntentionBundle,
) -> Result<IntentionBundle> {
    if bundle.snapshot != *conversion.original_digest() || bundle.version != 1 {
        return Err(invalid(
            "old intention bundle does not bind this verified conversion",
        ));
    }
    let mut upgraded = bundle.clone();
    upgraded.snapshot = canonical_digest(IdentityDomain::Data, conversion.snapshot())?;
    validate_bundle(conversion.snapshot(), &upgraded)?;
    // Cloning changes no object, order, payload or nested source/proof link.
    let mut projected = upgraded.clone();
    projected.snapshot = bundle.snapshot.clone();
    if canonical_bytes(&projected)? != canonical_bytes(bundle)? {
        return Err(invalid("conversion changed retained intention bytes"));
    }
    Ok(upgraded)
}

#[cfg(test)]
#[path = "owned_archive_policy_reuse_tests.rs"]
mod archive_policy_reuse_tests;
