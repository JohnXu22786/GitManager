# Actual task sources

`TaskSourceAdapter::link(project_id, task_id)` binds an existing TaskRegistry
entry. It reloads that registry rather than caching task authority. It uses the
unchanged `task_verification::source_fingerprint`, including ignored files, and
pins repository/worktree identity. Missing, removed or moved tasks fail closed.

A supported task contains `.gitmanager/product.json`:

```json
{
  "version": 1,
  "project_id": "my-project",
  "artifact_kind": "generated_app",
  "program_path": "app.json"
}
```

`app.json` is the strict `AppDefinition` source. Declaration and program paths
are relative, bounded, and cannot escape through parents, links, Git metadata
or non-regular files. Captures retain original bytes and separately pin raw,
canonical-program and full-task-source identities. Generated source is parsed,
not executed here. Unknown declarations and general external repositories are
unsupported/unverified; this module never runs their scripts or simulates them
with the local-app interpreter.

## Worker/controller integration

1. Create `SourceWatcher` for the linked adapter and poll it on a worker thread.
   Native events coalesce into a trigger. A periodic full reconciliation covers
   missed events; two equal captures separated by a settling interval are needed
   before emitting a changed program. Partial/invalid input cannot replace the
   last complete capture. Keep that earlier capture visibly stale while pending.
2. Before analysis/execution, use `ensure_fresh` or `with_fresh_source`. The latter
   rejects the worker result if source changes during its execution.
3. Check `ensure_fresh` again immediately before adoption. A changed ignored or
   non-program file still invalidates the old full-task proof. Watch events and
   program-only hashes are never evidence authority.

The controller owns review creation, operation cancellation, persisted pending
state and final adoption. These adapters do not add another writable task or
project store. GUI/main registration belongs to the application integration.

## External Harness handoff

`ExternalHandoff::prepare` writes the validated `DevelopmentRequest` into a new
request-ID directory under an existing app-owned job root **outside** the task
source. This carries the actual baseline, context, active decisions and request
identity automatically. It does not launch a provider or grant disclosure/tool
permissions. The controller supplies only already-authorized projected content.

After the external author edits the declared program, the machine submission
entry calls `submit_current_program`. It computes completion identities and
atomically publishes `complete.json` last, with no manual JSON copy. An author
using the protocol directly can publish the equivalent strict
`ExternalCompletion`. Its exact request/project/task/path, baseline source,
completed full source, raw bytes and canonical program must all match.

The worker polls `ingest`: missing/truncated completion remains pending; forged,
wrong-request, modified bundle and stale content are rejected. Rechecks occur
before and after intake. Preparing a used request ID never overwrites its old
bundle. Capture is not adoption and completion is not execution proof.

External edits remain `ExternalAuthor`, including when a TaskRecord names a
Harness or session. Those fields cannot certify a live invocation. For V02 jobs,
`ingest_transport` delegates to the actual opaque transport, verifies the exact
request and fresh SourceBinding identity on both sides, and returns its original
receipt/provenance unchanged. Domain parsing and independent execution happen
later; a fixture or a JSON member named `passed` creates no live/native evidence.

## Legacy project boundary

`LegacyOrderAdapter` delegates copied native commands, scoped observations and
scenarios to `tool_runtime` and `tool_decisions`. It neither converts old schemas
nor writes their stores. Generic actions, relations, selections and external
source execution are explicitly unsupported. Old records, decisions, snapshots
and runtime semantics remain under the original engine/store.

Filesystem intake uses descriptor-anchored no-follow walks on Linux/macOS and
held no-delete-sharing parent handles with reparse rejection on Windows. Other
platforms fail unsupported. Linux component tests are not native GUI acceptance,
live authoring, discovery, or evidence that Windows/macOS was exercised locally.
