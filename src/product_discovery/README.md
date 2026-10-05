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
   For KeepCurrent, supply chosen_artifacts only from the controller's verified
   historical adoption/selection receipt; an ambiguous side remains unverified.
   Model claims do not populate
   these fields. Scope predicates not independently resolved stay unknown.
   `workflow_validity` contains independent workflow constraints; proposed model
   scenario validity is not treated as an accepted oracle.
5. Retain the entire report, including failed/inconclusive runs and hypothesis
   logs. Display questions from immutable `VerifiedWitness` artifacts. Both
   compared programs, seed, actions, output bytes, typed observations, original
   scenario and its original runs, reduction audit, and final single-deletion
   certificate are available.
   A provider receipt is authorship provenance, never execution evidence.
6. Recheck actual source identity before showing/adopting results.
   `VerifiedWitness::matches_sources` compares exact captured artifacts. The
   later trusted controller must independently recheck current decisions/data
   and use the store's checked-adoption path. This module cannot commit choices.

## Search and proof boundaries

- Source analysis compares actual typed source by semantic component and follows
  action/state/entity/view/output references. A comparison first checks the actual
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
  business differences. The runtime's actual transactions remain in the traces.
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
  keep a consistent saved-side correspondence across observations. A new
  combination of individually familiar values gets a host-derived compound
  executable property; its reduction cannot discard required coordinates.
- `complete` means every remaining permitted single reduction from the final
  witness was tested and lost the distinction or invalidated the workflow. It
  does not mean globally shortest. Unfinished searches report smallest found and
  incomplete; no witness is finite search coverage, not universal equivalence.
- No-preference/deferred outcomes need a retained accepted witness. Its outcomes
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
  when the provider omits one of their observation points; a newly measured
  retained outcome gets its own independently minimized witness. Settlement is
  keyed by source pair and scene, so one checked context cannot settle another.
- Related hypotheses for the same action/observable or actual contrast are
  grouped; identical
  executable scenes are deduplicated while all hypothesis dispositions remain in
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
