# Local application protocol, version 1

`src/product_contract.rs` is the authoritative typed contract. The two schemas
in this directory describe the provider-facing JSON structure. They use compact
encoding so each raw schema fits a 32 KiB transport schema budget; structural JSON
Schema acceptance must always be followed by the Rust semantic validator. The
runtime, provider bridge, source adapter and UI consume these shared definitions.
This package is a language/validation contract, not a working application runtime
or evidence that an AI generated a useful application.

## Intake and identities

- `AppDefinition::parse` and `DevelopmentResponse::parse` use the existing bounded
  JSON intake: 1 MiB including whitespace, duplicate decoded object keys rejected,
  64 JSON container levels, no trailing data. Unknown fields and unknown variants
  are rejected at every typed object. Optional wire values may be omitted or null;
  the provider schemas request explicit nulls for predictable structured output.
- Semantic validation additionally checks version 1, references, unique stable IDs,
  operand/argument types, required fields, binding shadowing, view/action wiring,
  text/collection/definition limits and AST fuel-independent depth/node budgets.
  Expressions are limited to 16 levels and 8,192 nodes per application. Types have
  at most four nested list/optional levels. Limits are upper bounds, not a promise
  that a maximally sized application fits every other bound.
- IDs use lowercase ASCII letters, digits, `_` and `-`, 1–128 bytes. Labels are
  display text and never substitute for IDs. Entity fields are entity-scoped;
  definitions in each top-level namespace and view bindings are separately unique.
- `CapturedProgram` retains the exact source bytes. `ArtifactRef.raw_digest`
  hashes those bytes; `program_digest` hashes canonical complete typed source;
  `semantic_digest` only excludes explicitly declared display labels. It does not
  remove literal values or fields named `label`, and is not proof of behavioral
  equivalence. Source/driver/runtime/data/session changes still invalidate proof.
- Digests are SHA-256 with a versioned identity-domain prefix. Canonical JSON sorts
  object keys, preserves array order and uses the typed serializer's exact scalar
  encoding. Source, program, semantic program, data, schema, session, inputs,
  observations, outputs, requests, scenarios, decisions, evolution and adoption
  are distinct domains. This is not an RFC 8785 floating-point implementation.
- `SourceBinding` also pins the project, relative program path, optional actual
  task fingerprint and producer provenance. `SourceLocus::validate_against` checks
  exact raw bytes/path and an existing, valid JSON pointer. The source adapter
  must separately verify real filesystem containment and fresh task provenance.

## Executable language

An application composes arbitrary declared entities, typed reference relations,
constraints, actions, transient state, generated views and observables. It does
not select a domain template. Data types are boolean, signed integer, text,
Gregorian date (days from 1970-01-01, years 0001–9999), record reference, bounded
list and optional value. Literals carry their type, including empty lists/nulls.

Expressions are pure. They support field/state/parameter reads, fixed `today`,
conditional/coalescing logic, typed comparisons, checked integer/date arithmetic,
text concatenation/containment/lowercasing, and bounded query/filter/map/count/
sum/any/all operations. There are no functions, recursion, dynamic imports, eval,
shell, network, paths, credentials or outgoing communication operations. Relation
cycles do not recurse implicitly; expressions and transaction loops are bounded.

The production interpreter must implement these deterministic semantics:

1. Query over records in stable record-ID order, exclude archived records unless
   requested, filter, sort by the listed scalar keys, break equal-key ties by
   stable record ID, then apply the explicit limit. Lists preserve order. Text
   comparison/containment is exact Unicode text; `lower` is Unicode lowercasing.
   Integers and dates use checked arithmetic. Overflow and dangling references
   are runtime errors, never saturated values or successful checks.
2. `Create`, `Update`, `Archive`, `SetState`, collection `Insert`/`Remove`, bounded
   `ForEach`, `Assert` and `Emit` execute in declared order on a copy. Creation
   binds a fresh reference after evaluating values. Loop bindings are lexical
   and cannot escape or shadow other variables. Record IDs are host-generated,
   deterministic within replay, stable thereafter and never derived from labels.
3. Guards run before an action. Ensures, entity constraints and unique tuples run
   against the tentative final state. State changes, durable writes and artifacts
   become visible together only after success. Any error, failed guard/assertion,
   cancellation or exhausted budget discards the entire input's effects.
4. Unique tuples apply to non-archived records; tuples with null/missing optional
   components are excluded. Missing optional fields read as null. Archiving keeps
   the record and reference identity; there is no delete/cascade primitive.
   Runtime reference validation must also cover parameters and session values.
5. Collection insertion appends only previously absent equal values in input
   order. Removal removes every value equal to an input item; other order is
   preserved. A transaction loop iterates a snapshot of its evaluated input
   list, so modifications do not create an unbounded traversal.
6. `Emit` maps each input element into declared scalar columns. `LocalArtifact`
   contains declared typed column definitions (including for zero rows), typed rows
   and the exact local output bytes; its validator
   verifies that they agree. JSON is a canonical array of ordinary row objects.
   Dates serialize as ISO dates and references as entity/record objects. CSV uses
   column IDs as headers, CRLF, standard quote escaping and a leading apostrophe
   for text beginning with spreadsheet formula/control prefixes. No host path is
   part of the instruction. Saving requires an explicit host export action;
   previews keep outputs in memory.

The runtime must meter work inside queries, nested expressions/loops and output
encoding, in addition to enforcing `RuntimeLimits` on collection size, action
steps, writes, total output bytes and elapsed time. A timeout or exhausted budget
is inconclusive/error. Static language acceptance is not execution success.

## Views and semantic inputs

Views are lists, details or forms. List columns are expressions with `row` bound;
list queries and controls use transient state. A selection binds a list of row
references and can survive filtering independently of durable records. A text or
scalar state control can drive search/filter expressions. A control input first
sets its own state, then invokes its optional change action with the single
parameter `value`, in one atomic input. Selection uses the same mechanism. Thus a
program can preserve selection, clear it, or move selected references into a new
persistent entity without product-coded domain policy.

Forms bind named action parameters, with typed initial defaults. `Submit`
contains the full resolved parameter map. Actions explicitly declare `toolbar`
or `row` placement. Only row bindings may refer to `row`, and an activation must
supply exactly the matching row context. A keyboard chord triggers its declared
binding using the focused row when required; it does not bypass enabled/guard
checks. Navigating changes only session state. `Observe` changes neither data nor
session. `AdvanceClock` changes only the run's explicit clock, never the OS clock.

`product_protocol.rs` is presentation-only. A UI emits `WorkspaceRequest` with
session/operation/input-epoch and exact program/data/session context. The
controller owns immutable plans, late-result rejection, retries and commits.
A display flag or preview-ready reply cannot authorize adoption or reconstruct a
plan from edited display values.

## Data, intent and evidence

`DataSnapshot` enforces the same 1 MiB serialized ceiling as its bounded reader,
so an accepted save remains readable. It stores a cumulative schema, stable records and append-only actual
business-event facts. `SessionState` is separate. Adopting a program must not
replace the current data pointer with an old preview. Later fields/entities stay
in the cumulative schema and remain inspectable/exportable, even if an older
program does not render them. Historical events are not recalculated under new
rules. `RecoveryPlan` binds current data and events; incompatible or inaccessible
recovery is reported rather than called successful. These types do not implement
storage, migrations or rollback. Existing order-project formats are untouched.

A `ScenarioSpec` pins seed data/session, fixed clock/random seed, ordered semantic
inputs and independent validity obligations. Both alternatives use the same
input digest. A `RunEvidence` records actual trace effects and observation points,
including runtime/driver versions, origin, source-before/source-after, budgets,
errors and uncovered behavior. Named observations retain declared value_types
from AppDefinition::observable_types, even when a value is null. Property checks
require those types to match rather than interpreting null as any optional type.
Observations contain cumulative emitted artifacts
at that point. Artifact receipts bind output identity/format/columns/rows plus an
independent bytes_digest, preventing output relabeling or loss of empty-column
types. Failed/inconclusive comparisons retain exact executed input prefixes;
completed runs/comparisons require the full planned input sequence. Validation checks
trace/point/state/artifact consistency and selected observable resource limits; it does
not establish that a persisted record was genuinely executed. The trusted runtime
must produce it, and the controller must independently recheck current identities.

`AcceptedProperty` uses source-independent semantic observable/output IDs and
supports count/content assertions. Missing/unmapped/invalid observations evaluate
to unknown, never pass. A demonstrated `DifferentialWitness` requires a declared
property that actually differs on the recorded observations, and every declared
workflow-validity property must evaluate true on both runs. Its minimization
certificate records reduction trials and final single-deletion trials; only the
minimizer can establish completeness by enumerating and testing every permitted
remaining deletion. A certificate is not a global minimum claim.

`DecisionScope` matches explicit operations, populations and conditions using
applies/outside/unknown. Missing exclusion identity or an explicitly unknown
boundary affecting the operation stays unknown unless other context proves the
scope outside. Missing context never broadens it. Decisions preserve
rationale when supplied, unknown boundaries, accepted scenes, obligations and
reciprocal acyclic supersession. Both-needed/neither/deferred remain pending and
cannot activate behavior. `EvolutionProposal` proposes mappings/retirement;
`AdoptionPlan` and `RecoveryPlan` only prepare changes against exact current
source/data/session/decision identities. Their validation is not user approval,
compatibility execution or an automatic adoption gate.

## Provider and adapter boundary

`DevelopmentRequest` contains an explicit context/source/example projection and
portable intentions. Its presence is not permission to disclose it. The bridge
must enforce the user's actual provider/data/tool/usage authorization before
external submission. The opaque transport is defined elsewhere; this module does
not duplicate provider transport types or launch processes.

`DevelopmentResponse` accepts executable candidate source strings, source-bound
hypotheses, encoded scenarios, untrusted evolution suggestions and unsupported
explanations. Evolution suggestions reference candidate IDs and known intentions,
propose semantic/scenario mappings and named retirement, and resolve only through
a matching reconciliation request. The host computes actual artifact identities
and independently checks the design before any adoption or retirement.
It has no evidence,
pass, adoption or retirement-authority member. `validate_for` checks the exact
request digest, actual source loci, related decisions and executable alternative
scenario references. A hypothesis stays unverified until independently run.
Producer metadata distinguishes live agents, external authors, user authors and
component fixtures; it is not itself proof of observed behavior.

`RuntimeAdapter`, `DevelopmentProvider` and `SourceAdapter` declare the boundaries
for the production implementations. `ExternalRuntimeManifest` is only an extension
slot: no external build/server/browser driver or general-code sandbox exists here.
Hand-authored structural fixtures and mock receipts test contract behavior only.
