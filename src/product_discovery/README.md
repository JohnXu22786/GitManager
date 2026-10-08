# Independent choice discovery

These modules are library components for the product controller. They do not
register UI modules, edit a source tree, write the live product store, adopt a
program, or call a provider automatically when an app opens.

## Controller integration

1. Capture exact sources and build a `DevelopmentRequest`. For discovery, the
   sources begin with `[baseline, changed_candidate]`; any trailing sources must
   be exact historical artifacts referenced by selected `accepted_scenes`.
   For other operations the current source is last after any historical sources.
   Include authorized intent,
   active decisions, unknown boundaries, and the selected synthetic/sanitized
   examples. Accepted decision scenarios must be available by exact digest.
2. Call `prepare_development` on a worker with the selected real
   `ProviderTransport`. Show its exact `DataDisclosure`. Preparation/probes are
   not authorization. Supply a controller-owned `ConsentReceipt` only after the
   user has authorized that projection, provider, capabilities and usage.
3. The resulting `AuthorizedDevelopmentProvider` implements the frozen
   `DevelopmentProvider`. It accepts only the prepared domain request, consumes
   one job, cooperatively cancels, and never retries by silently invoking again.
   Preserve its receipt/raw response if the controller persists job history.
   `ProviderRequest.source_digest` is the primary captured source's
   `binding.identity()`: `sources[1]` for discovery, the last source otherwise.
   It is never the program digest or bare task fingerprint.
4. Pass the untrusted `DevelopmentResult` to `discover` with a controller-owned
   `DiscoveryPolicy`. Populate explicit feature `required_actions`, independent
   requirement cases, and applicable scope facts. Every newly required action
   needs an independent behavioral requirement case covering it; declaration
   alone cannot establish that an alternative implements the feature. Active chosen outcomes without
   predicate obligations are checked against their selected retained witness side.
   Legacy witness-only concrete choices also require the genuine witness and,
   for KeepCurrent, the exact selected-artifact mapping. `witness_bindings`
   permits an explicit PropertiesOnly independent promise with nonempty
   demonstrated predicates; absence retains ObservedOutcome. Predicate presence
   never silently changes the binding, and missing history stays unverified.
   For V07 store-backed intentions, set `retained_history` using
   `VerifiedRetainedHistory::load(&store)` and build the exact discovery request
   with `DecisionEngine::inherit_request`. This opaque host-only adapter reloads
   the committed snapshot and independently reproduces the immutable packages;
   portable request context alone is not authority. It preserves explicit
   ObservedOutcome versus PropertiesOnly binding. The authoritative V07 gate
   checks concrete outcomes AND additional predicates on the actual candidate
   and every competing executable before an option can be offered.
   `map_target` supplies optional host-proposed semantic renames for one exact
   captured target; they are composed with source-qualified retained mappings
   and independently checked, never accepted as equivalence claims.
   A new snapshot requires reloading the adapter; missing, stale, corrupt or
   ambiguous history blocks questions rather than fabricating a selected side.
   V07 Scenes-package IDs and differential-witness IDs are separate domains.
   Concrete choices need only their real selected scenes. Nonbinary history
   retains both actual source-qualified scenes and feeds them to the existing
   independent replay/reduction/correspondence checks. `discovery_scenes` supplies
   opaque verified pending-scene projections from V07. Their mapped operations
   associate renamed hypotheses, and their independently executed target traces
   establish correspondence. Historical values are sampled from the original
   evidence using the source-qualified semantic names; no RunEvidence or output
   receipt is rewritten to appear as a target execution. Explicit renames that
   collide with unchanged historical identities are unavailable, so a renamed
   channel cannot erase another retained outcome. Current host workflow
   predicates are also evaluated on the actual mapped target runs before either
   a candidate or an alternative is offered. All scene objects and
   mappings remain reachable in V07's ordinary intention bundle, so no new
   persistence object or backup schema is required.
   Missing, incompatible or unrepresentable saved channels remain unknown,
   rather than an observed intention violation.
   Additional chosen scenes use their exact selected-source `accepted_scenes`
   context after an independent archived-source replay; their check identities
   bind those verified historical observations.
   For KeepCurrent, supply chosen_artifacts only from the controller's verified
   historical adoption/selection receipt; an ambiguous side remains unverified.
   Model claims do not populate
   these fields. Scope predicates not independently resolved stay unknown.
   `workflow_validity` contains independent workflow constraints for the
   operations actually executed by either compared program, not the provider's
   chosen action label. They apply to every current scene/replay/reduction and
   independent requirement, chosen-outcome and obligation precheck;
   trusted historical constraints are also retained. Proposed model scenario
   validity is discarded rather than treated as an accepted oracle. Executed
   producer operations also determine relevant retained intentions, so a new
   consumer label cannot reissue their unchanged choices or exclude an
   associated producer-only retained scene. `ChoiceQuestion.action` is a
   grouping/context hint, not adoption-scope authority: use the actual verified
   source scenes, independent intention checks and the user's explicit scope.
5. Retain the entire report, including failed/inconclusive runs and hypothesis
   logs. Display questions from immutable `VerifiedWitness` artifacts. Both
   compared programs, seed, actions, output bytes, typed observations, original
   scenario and its original runs, reduction audit, and final single-deletion
   certificate are available. A present target that exceeds sampling limits
  makes a reduction inconclusive; it cannot be labeled invalid to complete a
  1-minimality certificate.
   A provider receipt is authorship provenance, never execution evidence.
6. Recheck actual source identity before showing/adopting results.
   `VerifiedWitness::matches_sources` compares exact captured artifacts. The
   later trusted controller must independently recheck current decisions/data
   and use the store's checked-adoption path. This module cannot commit choices.

## Search and proof boundaries

- Source analysis compares actual typed source by semantic component and follows
  action/state/entity/view/output references in proposed and applicable retained
  scene contexts, including directly invoked producer actions and every
  declared observable actually evaluated by Observe. A provider cannot hide another observable or a changed view by
  omitting a label or navigation.
  A comparison first checks the actual
  baseline/candidate outcomes on the same observation target; hypothetical third
  programs cannot invent behavior changes for equivalent edits. Newly required
  features have a separate path with two complete implementations. It ignores only declared display
  labels, not arbitrary data fields with matching names. No domain-name or
  request-keyword router selects scenes or outcomes.
- Model alternatives must include the exact captured candidate. Every candidate
  must implement the scenario and pass independent explicit requirements. Saved
  applicable active obligations run even when the provider omits them. Pending
  predicates never become active requirements; Withdrawn and Superseded entries
  remain terminal history. No-preference decisions can retain
  independently active invariants without repeating the settled question. Violations go to
  repair; unknown applicability/execution does not become an approved option.
- Distinctions are sampled from real declared observables, actual local outputs,
  or typed view rows/cells. Identity/provenance-only data-digest changes are not
  business differences. One-sided missing or incompatible typed channels remain
  explicitly unverified rather than silently equal. Channel presence is checked
  separately from sample construction, including cumulative export-size limits;
  both captured declarations
  are considered so removed channels cannot disappear from coverage. The
  runtime's actual transactions remain in the traces. An executed contrast
  outside the supported targets remains explicitly unverified.
- Each comparison uses the same copied data, session, clock, RNG and inputs.
  Failed prefixes are preserved. Preview never resumes historical export receipts
  or writes a live store. Runtime limits, cancellation at every replay boundary, precheck limits, comparison limits, trial
  limits and reduction-evidence byte limits bound work. If a just-executed
  reduction exceeds its full-trace budget, a single bounded overflow receipt
  (at most 8 KiB) records its identity, actual comparison state, and explicitly
  omitted/truncated evidence; it cannot support a minimality claim.
- Declared reductions delete seed records, inputs/observation points, optional
  fields or override conditions with a common initial state value. Referential
  integrity, runtime guards and independent validity constraints must still hold.
  Every trial independently runs both programs. Reductions retain the same target
  and material difference shape (including membership versus ordering); the final
  property binds the actual final observations. For a previously considered
  choice, reduction retains both initial target outcomes so it cannot erase the
  condition that produces a genuinely new third result. Whole material vectors
  keep a consistent saved-side correspondence across observations. Executed,
  unrepresented view controls, action availability and output layout also
  constrain retained equality; unexplained changes cannot be called settled.
  These channels do not create a fabricated executable property. A new
  combination of individually familiar values gets a host-derived compound
  executable property; its reduction cannot discard required coordinates.
- `complete` means every remaining permitted single reduction from the final
  witness was tested and lost the distinction or invalidated the workflow. It
  does not mean globally shortest. Unfinished searches report smallest found and
  incomplete; no witness is finite search coverage, not universal equivalence.
- No-preference/deferred outcomes need a retained accepted witness or the two
  real source-qualified sides of a verified V07 scene package. Its outcomes
  are rerun before suppressing an unchanged scene. Redundant scene inputs are
  matched through independently retained reduction evidence and their original
  executed outcomes, not only provider
  scene IDs or pre-reduction hashes. Changed material outcomes may
  reopen the boundary; unavailable old evidence remains explicitly unverified.
- Question descriptions name independently observed values/views/outputs; provider
  prose remains explicitly labeled as a suggestion in the log.
- All declared typed observables, view/output targets and observation points are
  checked within the comparison budget; a provider's primary observable cannot
  hide another measured consequence. Actual shared contrasts also group related
  hypotheses whose primary labels differ. All targets are checked before settled
  outcomes are suppressed. An earlier familiar contrast cannot hide a later
  newly executed consequence. Complete retained scenes are also checked even
  when the provider omits every revealing observation and reports equal
  outcomes. Every selected scenario digest of an applicable accepted decision,
  not only its primary witness scene, is an independent search input. Missing
  historical correspondence remains unverified. Search does not wait for a
  provider witness; a newly measured
  retained outcome gets its own independently minimized witness. Settlement is
  keyed by source pair and scene, so one checked context cannot settle another.
  Additional accepted inputs use their own `accepted_scenes` outcomes, matched
  to the original source pair and independently replayed from the supplied
  archived sources. Primary-scene values cannot settle or reopen another
  accepted initial condition; missing or mismatched history remains unknown.
  History lookup tolerates observation renaming/removal without changing
  effective inputs or initial conditions; actual replayed trace correspondence
  still decides settlement. Ambiguous or unmatched additional history never
  silently falls back to the primary scene.
  Actual alternative channels also receive presence, type and representability
  checks, including unchanged captured channels and alternative-only
  declarations. The captured-change filter limits choice generation, not this
  independent coverage check.
- Unsupported material view/action/selection or output-layout contrasts remain
  explicit in coverage even when a supported contrast also yields a witness,
  or comparisons run without a witness. This check uses original executed
  source and alternative observations, independently of saved decisions; it
  does not invent a property for the unsupported channel.
- Related hypotheses for the same action/observable or actual contrast are
  grouped. Exact source-pair/scene matches do not depend on provider action
  labels. A bridging hypothesis merges the entire connected group, preserving
  ordered hypothesis associations, unknowns and distinct evidence; identical
  executable scenes are deduplicated while final dispositions remain in
  the log. Comparison-budget exhaustion remains in coverage diagnostics even
  when another completed comparison produced a valid witness. A model's
  defect/requested-change classification remains a suggestion
  unless independently checked against a controller-owned requirement.

## Evidence schema addition

`Observation.view_schema: Option<ViewSchema>` carries the executed list/detail
view's entity and typed columns, including empty/null results. Old absent metadata
is omitted on serialization and cannot prove new view properties. Runtime version
`local-interpreter/2` invalidates earlier version-bound evidence; the executable
language version is unchanged. The semantic input driver is `semantic-input/2`.
`ScenarioSpec::operation_namespace()` hashes the semantic replay tag, project,
seed generation, records sorted by entity/ID, recorded event order, clock and
RNG. It excludes schema additions, display/scenario IDs, session/view annotations,
and action names. `replay_operation_ids()` assigns unique action-bearing mutation
ordinals to Invoke/Control/Activate/Submit and separate indexed step IDs to
Observe/Navigate/AdvanceClock. Instrumentation and mapped action names cannot
change earlier synthetic record IDs or ordered exports. Added/reordered
mutations still require independently verified correspondence. Live apply/store
operation IDs and complete source/input/scenario bindings remain unchanged.
No existing order
format, data snapshot format, or mutable-data migration is changed.

Tests use hand-authored fresh structural edits, recorded responses and explicitly
fake CLI subprocesses. They prove component regressions, not live generation,
post-freeze semantic holdouts, end-to-end UI adoption or human product value.

## Portable accepted context and reconciliation

`DevelopmentRequest.accepted_scenes: Vec<AcceptedSceneContext>` contains selected
host-verified decision/source/scenario/observation/disclosure associations. The
exact ArtifactRef must appear in request sources, the scenario and disclosure
must be selected, and observations must match the scenario's declared points
and source types. The collection is bounded; over-budget context is rejected,
never silently truncated into prose. Empty context is omitted for old request
serialization. It is portable context, not provider-authored execution proof or
permission to transmit data. The controller still authorizes the exact transport
projection.

ScenarioMapping and SuggestedScenarioMapping have optional `source_program`
(program_digest), so two accepted sides can map the same original scene to
different new actions. Uniqueness is the (scenario, source_program) pair. Known
ambiguous missing discriminators and mismatched explicit programs are rejected;
V07 additionally resolves each mapping against the exact accepted package and
refuses absent discriminators whenever host resolution is ambiguous. Absent
legacy fields remain omitted. Proposed retirement only names listed needs with
known Active or Pending status; proposal validation does not activate pending
predicates, retire decisions, or authorize adoption.

## Checked authored alternatives for managed tools

`PreparedDiscoveryCandidate::from_result` binds the exact discovery request,
actual development result, selected candidate ID and freshly regenerated scoped
preparation. The returned candidate bytes are captured with the result's own
producer and project, and must equal `prepared.candidate()`. Register the opaque
link with `VerifiedRetainedHistory::map_prepared_result` before discovery.

Discovery executes the independently generated `prepared.target()` for these
alternatives. `DiscoveryReport::lowerings` retains the actual result and both
captures, so callers can show the explicit authored-to-host-compiled link.
Source locations continue to identify the original request source bytes; they
are never rewritten into pointers into compiled output. Registration is bound to
the complete result and request, not only a candidate label. Freshness, retained
intentions, independent feature requirements and ordinary search/minimization
checks still apply. This local consistency boundary adds no cryptographic
provider or user-approval authority.
