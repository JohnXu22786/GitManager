# Scoped intention controller

This module is a host-side component, not an agent prompt or a second project
store. `ProductStore` owns the active program, current data, session and decision
graph. The application controller owns the person's choice and authorization to
use an external development provider.

## Choosing and checking

- Capture accepted scenes through `accept_scene` using the production runtime.
  Store exact source, input, complete execution evidence and explicit disclosure.
  Disclosure describes example data; it does not authorize transmitting it.
- `prepare_choice` prepares atomic persistence of a concrete acceptance or
  nonbinary outcome together
  with its actual scene. Accept names an exact target. KeepCurrent,
  EitherAcceptable, BothNeeded, NeitherFits and Deferred do not select a different
  executable. EitherAcceptable records no preference, without making either
  implementation an active rule. Supplied scenes are independently reproduced
  before preserving even pending history; new history is checked again at adoption.
- `prepare_resolution` resolves specifically named pending choices when the
  person returns to decide, keeping the old outcome and witness as history. It
  cannot retire an active promise through this ordinary choice path.
- `check_all` independently executes every active accepted scene. `check_current`
  also reloads the current implementation mappings. Every result binds exact
  source, scenario/input, data/session, decision graph, runtime and driver.
  Replay uses the shared `semantic-input/2` operation identities, not a private
  retained namespace; added/reordered mutations do not imply record equivalence.
  Exact-source reproduction compares complete actual observations. Cross-program
  checks also verify the mapped action's actual population/conditions/exclusions.
- `Choice.binding` defaults to `IntentionBinding::ObservedOutcome`: the concrete
  observed outcome AND any additional predicates remain binding. Adding a
  predicate never weakens that choice. `PropertiesOnly` is a separate explicit
  host/user choice for an independent promise; it requires nonempty predicates
  demonstrated by applicable scenes. Never derive it from predicate presence or
  accept a provider's suggestion as permission to change the binding.
- Both modes use typed runtime observations and actual output artifacts. The
  mode persists in immutable packages and the fresh provider context, and is
  carried unchanged through evolution, backup and recovery. Use
  `intention_binding(decision)` to display the recorded mode; it is descriptive
  metadata, not a passing-check certificate. Missing observations, unsupported mappings, unknown scopes,
  stale scenes and exhausted/cancelled runs never become passing checks.
- A demonstrated violation returns `RepairRequired`, with zero business-choice
  prompts. The controller repairs the implementation instead of polling the
  person again about an already accepted requirement.

Scopes include operations, record populations, conditions, exclusions and
unresolved boundaries. Record creation history comes from actual business events.
Every promised operation needs an applicable replayed scene. Outside boundary
scenes remain historical evidence without acquiring in-scope obligations. An
action that does not identify affected records cannot prove a record-scoped
requirement. Arbitrary population predicates and entity/field migrations remain
unverified; they are not silently broadened to all work. These checks establish
finite accepted scenes, not universal correctness of every future input.

## Development and evolution

Build future requests from the loaded snapshot and immutable accepted scenes,
not prior chat history. `development_request` builds modifications and
reconciliation requests; `inherit_request` enriches discovery while preserving
its exact baseline/candidate source pair. Other operations keep the current
source last. Carry accepted outcomes, scopes, nonbinary decisions, unknown
boundaries and independently exercised mappings. `accepted_scenes` supplies
actual bounded observations with the exact decision, source artifact, original
scenario and disclosure; these labels do not grant transmission permission. Complete replacement inputs
travel as typed examples, indexed by source and scenario digests. Oversized
context is explicitly unavailable rather than truncated.
The provider bridge is responsible for exact request binding and separately
obtained disclosure/usage authorization. It supplies executable source and
untrusted suggestions, never adoption authority.

`reconciliation_request` identifies at least two accepted work scenes, including
both sides of a single BothNeeded choice. `develop_evolution` calls the supplied
`DevelopmentProvider`, validates the returned program and mappings, replays both
needs, and checks every unrelated active intention. It has no built-in third
program, domain template or fixture fallback. A scenario replacement may add a
new explicit action or parameter, but cannot weaken the accepted seed, clock,
validity conditions or observation points. Ambiguous operation correspondence
remains unverified. Explicit replacement recipes are bound to their accepted
source program and scenario. Ambiguous legacy mappings require an explicit
source discriminator. Recipes persist for retained active decisions, including subsequent
implementation changes and another evolution.

`prepare_evolution` preserves each need's scope and independent properties. Only
the named obsolete decisions can become superseded, with their original witnesses
retained. Preparing, keeping, rejecting or deferring a design does not switch the
active program. The controller presents the new executable design and its exact
retirements before calling `adopt` on the opaque prepared change.

## Atomic adoption and recovery

`adopt` calls `ProductStore::adopt_verified` and reruns obligations against its
locked exact current snapshot. Imported successful-check flags are not used as
permission. A changed source, live data, session, decision graph or runtime needs
a fresh rehearsal. Store revision and business-data generation are different
counters and are checked separately.

`prepare_recovery` applies a retained prior program to current data.
`prepare_withdrawal` additionally requires the controller's explicit choice of
exact decisions to withdraw. Old decisions and witnesses remain as terminal
`Withdrawn { adoption }`
history, bound to the exact receipt and named retirement set; independently
active decisions and their properties stay binding. Withdrawal does not require
executing an obsolete action that the retained program never supported. Later
records, stable IDs, retained
fields, event facts and completed outputs are never replaced with an old database
snapshot. Incompatible restoration is refused in favor of a compatible forward
repair. Retained optional values remain available through the runtime inspector.

## Immutable packages and backups

Accepted scenes and exercised mapping recipes are immutable, content-addressed
objects held by `IntentArchive::new(ProductStore)` through the store's pinned,
strict-canonical extension API. The committed graph references scene objects
through `decision.witness`. Destination objects are checked before preparation
and again inside the locked adoption callback; another archive cannot activate
dangling references.
The first evidence digest in this controller's adoption receipt references its
source-program-bound mapping object; following digests identify rerunnable
execution evidence, not
independently stored authority. Unreferenced prepared objects do not activate
anything.

Backups must use `IntentionBundle` with the exact `ProjectSnapshot`.
`validate_bundle` requires the full snapshot identity, every reachable active and
historical scene/mapping object, strict versions, hashes and no missing or extra
objects, including the expected type of every reference. Limits are 512 objects,
1 MiB per canonical JSON object and 32 MiB for the canonical encoded bundle.
`restore_for` stages and re-reads these objects inside the fresh
`ProductStore::create_recovered_with` callback before CURRENT. `export_for` works
there without loading CURRENT. The original tool and backup are not overwritten.
Restoring a bundle does not certify old checks; future adoption reruns
intentions with the production runtime.

The deterministic tests use explicitly fixture-origin programs/provider responses.
They verify these components and persistence, not live AI generation, a fresh
human user's success, arbitrary framework execution or market value.

## Two prospective managed evolutions

For a new action absent from current, `accept_paired_scoped_scenes` executes two
independently prepared managed evolutions on the same exact frozen business
seed, day, session and semantic inputs. Each runtime start performs its own
checked additive schema projection. `prepare_paired_rehearsed_choice` retains
EitherAcceptable, BothNeeded, NeitherFits or Deferred as one pending scene
package. Both exact authored/compiled proofs share its recording receipt and
birth. The existing one-prepared-versus-current rehearsal remains available.

This bounded pair route accepts Evolution preparations only. It does not combine
fresh-layer cohort initializations, substitute current for a prospective design,
or activate either candidate. Live behavior, records, schema, session, artifacts
and adopted layers stay unchanged. Open and backup admission verify both proofs,
the exact experienced source/input pair, recording inventory and decision birth.
A missing or replaced proof cannot reuse the original operation receipt.

Pending new-action scenes that current cannot perform retain their actual
prospective replay sources; they are not labeled current observations. A fresh
prepared target supplies later discovery correspondence. Later acceptance or
resolution requires a newly prepared selected design against then-current work
and the exact pending decision IDs. Old preparations remain stale after daily
work, while immutable experienced history stays available.
