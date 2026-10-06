# Scoped behavior changes for generated local tools

This module composes a bounded record-local change into one executable
AppDefinition. Daily work, rehearsals, comparisons and intention replay use the
production interpreter. The genuine input captures remain retained. The
deterministic composition has honest ExternalAuthor provenance.

## Supported scope

- Future work is a durable cohort. Actual creation transactions stamp membership;
  subsequent edits continue to use that membership
- Selected unfinished work is an exact frozen list of existing record IDs. The
  host independently evaluates an explicit, source-bound completed predicate.
  Missing, archived or completed selections are refused
- Completed results are terminal. Existing completed and archived results are
  captured at adoption, without claiming to reconstruct their earlier completion
  values. Later completion/archive captures are bound to actual business events
- Record-local assignments and list/detail/output columns may use different
  Boolean, arithmetic, date and text expression trees. Each batch row selects its
  own behavior
- Whole-population operation-result changes can alter an enumerated output
  selection and its preview without changing shared selection mutation
- Source-qualified managed evolution can add structure and replace an expression
  implementation while retaining every protected result slot

The compiler checks complete action/view shells and dependencies. Partial
structural rewrites, foreign-row writes, durable aliases of scoped values, shared selection dependencies, changed
loop membership, unsupported conditions, ambiguous slot mappings and unsafe
completion/output ordering (including loop back edges) are refused with a
forward-repair explanation. Preserved projections on either side and after
mapping must be independent of action parameters.
Shared output definitions require every affected emission to be explicitly
covered. Every writer of a protected durable result must also be enumerated;
an uncovered correction/reset cannot silently overwrite completed work.
Changed durable values cannot be forwarded through view-action arguments or
used to route unrelated write targets, collection membership or outputs. This is
not arbitrary application composition.

## Host integration

1. Open through product_backup::inspect_open. A format-1 store must pass the
   explicit offline upgrade gate before editing. Display UpgradeProgress,
   retain original files, and restart/reopen when requested
2. Call ProductStore::prepare_scoped_change with the exact candidate and
   ScopeRequest. The opaque PreparedScopedChange exposes its compiled target,
   initialized copied seed and actual frozen scope
3. Rehearse that target and seed. ScopedExecutionContext::prepared independently
   regenerates source and metadata. A naked metadata-bearing source or invented
   seed is not scoped execution evidence
4. Capture the experienced result with DecisionEngine::accept_scoped_scene, then
   use prepare_scoped_choice. The choice names the compiled artifact and frozen
   scope. Concrete outcomes stay binding when additional predicates are supplied.
   Use prepare_scoped_resolution for exact pending decision IDs; it retains the
   same frozen target and the existing successor/supersession checks
   For EitherAcceptable, BothNeeded, NeitherFits or Deferred, use
   prepare_rehearsed_choice with both actually experienced current/prospective
   scenes. Capture the current side with accept_prepared_current_scene on the
   same copied seed as accept_scoped_scene. It retains a bounded replay proof while leaving the live source,
   data, schema, session and active layer set unchanged. This also works before
   the first layer exists. Pending scope describes the proposal only. A later
   Accept always needs a fresh preparation and exact pending resolution
5. Adopt through DecisionEngine::adopt. Under the store lock it rechecks the full
   basis, regenerates composition/initialization, replays intentions and installs
   one snapshot through the existing atomic CURRENT pointer
6. For structural evolution, first use prepare_managed_evolution with an exact
   ScopeSlotMapping for every retained protected slot. Pass the resulting handle
   to prepare_managed_change. ProjectSnapshot::editable_scope_context provides
   an honestly host-authored ordinary live-rule capture and every retained slot.
   Durable fields cannot be retargeted without an explicit preservation
   migration; supported expression and unrelated structural changes remain
   possible. Slot coordinates refer to the pre-instrumentation business shell, bound to
   the exact compiled source; they are not compiled JSON pointers. Modify and
   Reconcile requests include this editable source before the unchanged compiled
   current source. Discover preserves its exact source pair and receives only
   slot guidance. Source-count or complete-context overflow fails explicitly.
   For a new reconciliation design, pass the actual provider result and matching
   opaque preparation to develop_prepared_evolution. Its draft retains both the
   authored and compiled captures and carries that preparation through
   prepare_evolution, including exact successors and requested supersessions
7. Discovery loads VerifiedRetainedHistory from the store. Register prospective
   managed targets with map_prepared_target. Source-only mapping cannot supply
   scope authority. Both replay and comparison receive the verified admission
   registry; stale requests remain unverified. Matching pending inputs receive
   independently regenerated initialization before both compared replays.
   Original accepted outcomes stay original; projected correspondence is actually
   executed against the shared initialized input
8. Withdraw exact layers through DecisionEngine::prepare_scoped_withdrawal.
   It compiles remaining behavior on current data and retires only associated
   active intentions. Already withdrawn or superseded intentions keep their
   original history. It never restores an old database snapshot

Protected cohort fields, saved values and provenance are scenario validity
data. Reduction cannot erase them. Unknown correspondence between a synthetic
seed and a real frozen cohort remains unverified. Additive schema projection
of an authenticated accepted seed is checked separately and cannot change its
business facts. When an authenticated original seed exactly matches retained
initialization inputs, replay applies those independently regenerated metadata
receipts and binds evidence to the resulting actual seed. Unknown synthetic
correspondence is still rejected. A genuine older project snapshot can receive
a separate, source-qualified historical correspondence: only independently
derived system cells are added at the original input. Every business record,
event and day stays unchanged, and the exact compiled target executes.
Historical replay captures are labelled CapturedForHistoricalReplay rather
than presented as a real adoption receipt. Bounded typed proofs and original
source/seed/scenario bindings survive restart and backup. Projected scenarios
keep the original deterministic operation namespace, with distinct actual
scenario/source evidence; live adoption freshness is unchanged.

Concrete comparison excludes only compiler-added view/output provenance columns
identified by independently regenerated manifests. All business cells, output
row order/count, column types and predicates stay binding. Full raw observations,
bytes and visible provenance remain retained.

The initialized seed of a managed evolution remains authenticated
through its exact retained basis and source after later edits and restart.

## Persistence, upgrades and recovery

Generated-project format 2 embeds typed layers, compositions, initialization
receipts, scoped adoption links, retained rehearsals and historical replay
correspondences. Ordinary reads accept only this current
format. The format-1 decoder is confined to explicit upgrade gates. Journals,
old snapshot bytes and checkpoints are retained. Legacy order-tool formats are
not changed.

Initialization adds only allocated host fields. It does not increment business
generation, alter existing record revisions or invent business events. Actual
creation and completion transactions record their own metadata changes. Open,
daily saves, adoption and recovery verify sources, receipts, record identity,
membership, monotone seals and saved-value provenance.

Use ProductStore::runtime_view for daily presentation. Its preserved result/event
inspector survives changes to ordinary application views. Exports carry result
origin, capture day and source-record references. An adoption capture is labelled
as such, never as an observed earlier completion.

Backup recovery retains the complete snapshot and exact reachable intention
bundle. Recovery creates a fresh instance and leaves the original tool intact.
Use inspect_backup for backup intake: a legacy snapshot yields an explicit
LegacyBackup upgrade gate. Its upgrade_recover_new stages both original and
converted immutable snapshots, verifies unchanged scene/mapping objects, and
publishes only the new-format pointer in a fresh destination. Exact interrupted
attempts can resume there; an unrelated or later-used destination is never
overwritten. Restart/open and recent-instance registration follow the upgrade.

Imported historical evidence retains its original source and runtime identities.
It is rerun before another behavior change; an upgrade never relabels old proof
as current proof.

## Validation boundary

Synthetic fixtures exercise interpreter transactions, actual waiting intervals,
fixed customer commitments, reminders, mixed output, completion history,
current-data withdrawal, structural evolution, guarded discovery and fault
injection. They are engineering tests, not evidence of live-provider generation,
native desktop usability, end-user success or unrestricted composition.
