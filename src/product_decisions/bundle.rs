//! Portable immutable objects. Backup validation is not execution authority;
//! restored intentions are always rerun before later behavior adoption.
use super::archive::{decode, validate_rehearsal_scenes, Content, Object};
use super::*;
use crate::product_store::scope::{LockedArchivePolicy, PolicyFrameRecorder, RetainedPolicyDispatch};
use std::mem::size_of;
// Same checked compile-time packet arithmetic as the scope layer, deliberately
// private here rather than widening the scope helper's visibility.
pub(super) const fn archive_payment<const N: usize>(base: u64, terms: [(usize, u64); N]) -> u64 {
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
macro_rules! retained_decision_body {
    ($check:ident, $accepted:ident, $accepted_ranges:ident, $decision:ident, $range:ident, [$($decision_call:tt)*]) => {{
            let execution=$check.execution()?;
            let basis=execution.current();
            let layer=crate::product_store::scope::transition_layer(execution);
            let ids=owned_policy::retirement_ids(basis,layer)?;
            for id in &ids {
                $check.charge(1)?;
                let $decision=basis.decisions.decisions.iter().find(|d|&d.id==id)
                    .ok_or_else(||invalid("historical owned retirement decision missing"))?;
                let $range=$accepted_ranges.get(&$decision.witness)
                    .ok_or_else(||invalid("historical owned retirement witness missing"))?;
                $($decision_call)*
            }
            Ok(())
    }};
}
macro_rules! retained_bundle_body {
    ($snapshot:ident, $retained:path, [$($args:tt)*], $accepted:ident, $accepted_ranges:ident) => {{
    $retained($snapshot, $($args)* |check| {
        let mut verify = || -> Result<()> {
            retained_decision_body!(check, $accepted, $accepted_ranges, decision, range,
                [owned_policy::check_decision(check,decision,&$accepted[range.clone()])?;])
        };
        verify().map_err(owned_transition::into_store_error)
    })?;
    }};
}
macro_rules! validate_bundle_body {
    ($snapshot:ident, $bundle:ident, $accepted:ident, $accepted_ranges:ident, [$($finish:tt)*], [$($terminal:tt)*]) => {{
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
    let mut $accepted = vec![];
    let mut $accepted_ranges = BTreeMap::<Digest,std::ops::Range<usize>>::new();
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
                let first = $accepted.len();
                $accepted.extend(scenes.clone());
                $accepted_ranges.insert(object.digest.clone(),first..$accepted.len());
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
                let original = $accepted
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
    $($finish)*
    $($terminal)*
    }};
}
pub fn validate_bundle(snapshot: &ProjectSnapshot, bundle: &IntentionBundle) -> Result<()> {
    validate_bundle_body!(snapshot, bundle, accepted, accepted_ranges, [
        crate::product_store::scope::check_retained_transition_policies_with_initials(snapshot, |check, loan| {
            check.charge(HISTORICAL_INITIAL_RETAINED_PAYMENT)?;
            RetainedPolicyDecisionCapture { accepted: &accepted, ranges: &accepted_ranges }
                .run_initial(check, loan)
        })?;
    ], [Ok(())])
}
fn validate_bundle_locked(
    snapshot: &ProjectSnapshot, bundle: &IntentionBundle, capsule: &LockedArchivePolicy<'_>,
) -> Result<()> {
    validate_bundle_body!(snapshot, bundle, accepted, accepted_ranges, [
        retained_bundle_body!(snapshot, crate::product_store::scope::check_retained_transition_policies_locked, [capsule,], accepted, accepted_ranges);
    ], [Ok(())])
}

struct RetainedPolicyDecisionCapture<'call> {
    accepted: &'call Vec<AcceptedScene>,
    ranges: &'call BTreeMap<Digest, std::ops::Range<usize>>,
}
pub(super) const POLICY_FRAME_EXPORT_PAYMENT: u64 = {
    let actual = archive_payment(64, [
        (size_of::<&PolicyFrameRecorder<'_>>(), 16),
        (size_of::<IntentArchive>(), 4),
        (size_of::<&IntentionBundle>(), 4),
        (size_of::<RetainedPolicyDecisionCapture<'_>>(), 4),
        (size_of::<RetainedPolicyDispatch<RetainedPolicyDecisionCapture<'_>>>(), 4),
        (size_of::<Result<()>>(), 8),
        (size_of::<std::result::Result<(), StoreError>>(), 8),
    ]);
    if actual > 3040 { actual } else { 3040 }
};
// Caller-prepaid selected ordinary verification transport.
// Complete producing fixed slots: 35; direct activation/control sites: 12.
const HISTORICAL_INITIAL_RETAINED_PAYMENT: u64 = archive_payment(768, [
        (size_of::<&mut crate::product_store::scope::OwnedPolicyCheck<'_>>(), 20),
        (size_of::<&crate::product_decisions::bundle::RetainedPolicyDecisionCapture<'_>>(), 16),
        (size_of::<&crate::product_store::scope::HistoricalInitialLoan<'_>>(), 16),
        (size_of::<&std::collections::BTreeMap<crate::product_contract::Digest, std::ops::Range<usize>>>(), 16),
        (size_of::<&std::vec::Vec<crate::product_decisions::replay::AcceptedScene>>(), 16),
        (size_of::<()>(), 12),
        (size_of::<isize>(), 4),
        (size_of::<crate::product_decisions::bundle::RetainedPolicyDecisionCapture<'_>>(), 4),
        (size_of::<std::ops::ControlFlow<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>>(), 4),
        (size_of::<std::result::Result<(), crate::product_decisions::DecisionError>>(), 8),
        (size_of::<std::result::Result<(), crate::product_store::StoreError>>(), 12),
        (size_of::<std::result::Result<std::convert::Infallible, crate::product_store::StoreError>>(), 8),
        (size_of::<u64>(), 4),
    ]);
impl RetainedPolicyDecisionCapture<'_> {
    fn run_initial(&self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>,
        loan: &crate::product_store::scope::HistoricalInitialLoan<'_>) -> std::result::Result<(), StoreError> {
        self.verify_initial(check, loan).map_err(owned_transition::into_store_error)
    }
    fn verify_initial(&self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>,
        loan: &crate::product_store::scope::HistoricalInitialLoan<'_>) -> Result<()> {
        let accepted = self.accepted;
        let accepted_ranges = self.ranges;
        retained_decision_body!(check, accepted, accepted_ranges, decision, range, [
            check.charge(owned_policy::HISTORICAL_INITIAL_DECISION_PAYMENT)?;
            owned_policy::InitialDecisionCall { check, loan, decision, scenes: &accepted[range.clone()] }.run()?;
        ])
    }
    fn run(&self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>) -> std::result::Result<(), StoreError> {
        self.verify(check, recorder).map_err(owned_transition::into_store_error)
    }
    fn verify(&self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>) -> Result<()> {
        let accepted = self.accepted;
        let accepted_ranges = self.ranges;
        retained_decision_body!(check, accepted, accepted_ranges, decision, range, [
            check.charge(owned_policy::POLICY_FRAME_DECISION_CONSUME_PAYMENT)?;
            owned_policy::ReceiptDecisionCall { check, recorder, decision, scenes: &accepted[range.clone()] }.run()?;
        ])
    }
}
macro_rules! export_bundle_body {
    ($archive:ident, $snapshot:ident, $bundle:ident, [$($validation:tt)*], [$($terminal:tt)*]) => {{
        let mut objects = vec![];
        for digest in reachable($snapshot) {
            let content = $archive.read(&digest)?;
            let bytes = canonical_bytes(&Object {
                version: 1,
                content,
            })?;
            objects.push(BundledObject { digest, bytes });
        }
        let $bundle = IntentionBundle {
            version: 1,
            snapshot: canonical_digest(IdentityDomain::Data, $snapshot)?,
            objects,
        };
        $($validation)*
        $($terminal)*
    }};
}
impl<'gate, R: RuntimeAdapter> owned_transition::RecordContinuationCapture<'_, 'gate, R> {
    pub(super) fn verify(self, check: &mut crate::product_store::scope::OwnedPolicyCheck<'gate>, recorder: &mut PolicyFrameRecorder<'gate>) -> Result<()> {
        let Self { engine, store, current, fresh, next, plan, candidate, change, capsule } = self;
        owned_transition::locked_verification_body!(engine, store, current, fresh, next, plan, candidate, change, check, ids, [
            let ids = owned_policy::check_withdrawal_recorded(&engine.archive, check, recorder).map_err(owned_transition::into_store_error)?;
        ], [
            check.charge(LOCKED_ARCHIVE_EXPORT_PAYMENT)?;
            check.charge(POLICY_FRAME_EXPORT_PAYMENT)?;
            {
                // Move the same bundle beyond the archive scope so successful
                // cleanup remains validation, archive, bundle, then report.
                let _completed_bundle = {
                    let archive = IntentArchive::new(store.clone());
                    export_bundle_body!(archive, candidate, bundle, [
                        {
                            let bundle_ref = &bundle;
                            validate_bundle_body!(candidate, bundle_ref, accepted, accepted_ranges, [
                                let capture = RetainedPolicyDecisionCapture { accepted: &accepted, ranges: &accepted_ranges };
                                let dispatch = RetainedPolicyDispatch { capture, run: RetainedPolicyDecisionCapture::run };
                                crate::product_store::scope::check_retained_transition_policies_with_frames(candidate, capsule, recorder, dispatch)?;
                            ], []);
                        }
                    ], [bundle])
                };
            }
        ])
    }
}
impl IntentArchive {
    pub fn export_for(&self, snapshot: &ProjectSnapshot) -> Result<IntentionBundle> {
        export_bundle_body!(self, snapshot, bundle, [validate_bundle(snapshot, &bundle)?;], [Ok(bundle)])
    }
    pub(super) fn export_for_locked_withdrawal(
        &self, snapshot: &ProjectSnapshot, capsule: &LockedArchivePolicy<'_>,
    ) -> Result<IntentionBundle> {
        export_bundle_body!(self, snapshot, bundle, [validate_bundle_locked(snapshot, &bundle, capsule)?;], [Ok(bundle)])
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

#[cfg(test)]
impl DecisionEngine<LocalRuntime> {
    pub(crate) fn assert_policy_frame_retained_boundary_for_test(
        check: &mut crate::product_store::scope::OwnedPolicyCheck<'_>, recorder: &PolicyFrameRecorder<'_>,
        decision: &ScopedDecision, scenes: &[AcceptedScene], mode: u8,
    ) -> std::result::Result<(), StoreError> {
        let accepted = scenes.to_vec();
        let ranges = [(decision.witness.clone(), 0..accepted.len())].into();
        let capture = RetainedPolicyDecisionCapture { accepted: &accepted, ranges: &ranges };
        let exact = mode % 2 != 0;
        let surface = mode >= 2;
        let packet = if surface { crate::product_store::scope::POLICY_FRAME_CONSUME_PAYMENT }
            else { owned_policy::POLICY_FRAME_DECISION_CONSUME_PAYMENT };
        let prefix = 4 + if surface { owned_policy::POLICY_FRAME_DECISION_CONSUME_PAYMENT + 5 } else { 0 };
        check.leave_policy_frame_units_for_test(prefix + packet - u64::from(!exact));
        let error = capture.verify(check, recorder).unwrap_err();
        assert!(matches!(error, DecisionError::Store(StoreError::Invalid(ref text)) if text ==
            "Common factual admission: budget: cumulative proof/initialization budget exhausted"), "{error:?}");
        check.assert_policy_frame_units_for_test(if exact { 0 } else { packet - 1 });
        println!("POLICY_FRAME_RECIPIENT_BOUNDARY surface={surface} exact={exact}");
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn assert_policy_frame_export_layouts_for_test() {
    let actual = archive_payment(64, [
        (size_of::<&PolicyFrameRecorder<'_>>(), 16),
        (size_of::<IntentArchive>(), 4),
        (size_of::<&IntentionBundle>(), 4),
        (size_of::<RetainedPolicyDecisionCapture<'_>>(), 4),
        (size_of::<RetainedPolicyDispatch<RetainedPolicyDecisionCapture<'_>>>(), 4),
        (size_of::<Result<()>>(), 8),
        (size_of::<std::result::Result<(), StoreError>>(), 8),
    ]);
    assert_eq!(POLICY_FRAME_EXPORT_PAYMENT, actual.max(3040));
    assert_eq!(LOCKED_ARCHIVE_EXPORT_PAYMENT, 3232);
    assert_eq!(crate::product_store::scope::policy_frame_retained_payment::<RetainedPolicyDecisionCapture<'_>>(), 2688);
    println!("POLICY_FRAME_EXPORT_LAYOUTS {}", serde_json::json!({
        "archive_owner": size_of::<IntentArchive>(),
        "bundle_owner": size_of::<IntentionBundle>(),
        "bundle_borrow": size_of::<&IntentionBundle>(),
        "retained_capture": size_of::<RetainedPolicyDecisionCapture<'_>>(),
        "retained_dispatch": size_of::<RetainedPolicyDispatch<RetainedPolicyDecisionCapture<'_>>>(),
        "bundle_result": size_of::<Result<IntentionBundle>>(),
        "decision_error": size_of::<DecisionError>(),
        "store_error": size_of::<StoreError>(),
        "decision_result": size_of::<Result<()>>(),
        "store_result": size_of::<std::result::Result<(), StoreError>>(),
        "d1": LOCKED_ARCHIVE_EXPORT_PAYMENT,
        "f5": POLICY_FRAME_EXPORT_PAYMENT,
        "f6": crate::product_store::scope::policy_frame_retained_payment::<RetainedPolicyDecisionCapture<'_>>(),
    }));
}

#[cfg(all(test, target_pointer_width = "64"))]
const _: [(); 6432] = [(); HISTORICAL_INITIAL_RETAINED_PAYMENT as usize];
