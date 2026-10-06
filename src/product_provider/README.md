# Structured provider transport

This independent module accepts opaque UTF-8 prompts and JSON-schema bytes. It
returns bounded JSON payload bytes with a durable invocation receipt. It does
not define an application language, generate a built-in demo, grant consent,
execute returned commands, validate domain semantics, or certify product behavior.
A payload property such as `passed` is ordinary untrusted data.

## Integration API

1. Select an explicitly trusted installed executable and its normal credential
   HOME with `ProviderTransport::new`. Use a private canonical absolute job root.
   The transport never copies or reads credentials, logs in, installs software,
   accepts provider-proposed permissions, or falls back to another provider/API.
2. `probe` runs only bounded `--version`, `--help`, `exec --help` (Codex), and
   `login status` / `auth status`. It hashes the executable and probe output.
   There is no persistent readiness cache: `submit` repeats the probes and
   compares the executable content and version/help/auth diagnostic outputs
   against the prepared job. It separately rechecks invocation arguments and
   the four environment values (`HOME`, `PATH`, `LANG`, `LC_ALL`). These
   fingerprints are not attestations of effective configuration, the tool
   catalog, injected context or isolation. A configuration change that leaves
   diagnostic outputs unchanged can go undetected.
3. `prepare(ProviderRequest)` requires a supported profile. It reserves a unique
   request ID, generates a random per-job nonce, saves the exact input/schema and
   disclosure, and returns `PreparedJob`. It makes no model request.
   Run probe/prepare/submit off the UI event loop because readiness checks are
   bounded but blocking.
4. The trusted application controller shows `DataDisclosure`, obtains approval,
   and supplies a `ConsentReceipt` bound to its entire digest. It must never
   manufacture this receipt from model output or saved authentication. A changed
   provider, executable route, argv, source, prompt, schema, profile, environment,
   limits or disclosure
   needs a new prepared request and authorization. This Rust API checks scope
   correlation, not the identity of a human approving in a host UI.
5. `submit` returns a `ProviderJob`. Call nonblocking `poll` to obtain
   its terminal receipt, and `cancel` to cancel. Accepted cancellation wins over
   terminal publication. Dropping the job requests cancellation and joins the
   owner worker. Job lifetime must therefore be owned by the controller.
6. `ingest(id, request_digest, fresh_source_digest)` automatically loads only
   the designated, durably published result after exit success, protocol-terminal
   success, session/result correlation and content-digest validation. It never
   ingests an arbitrary model-selected file. Then the domain bridge must validate
   the application schema/types and independently execute the candidate.
7. On restart, `reconcile(id)` obtains the OS job lock. An ownerless `Running`
   receipt becomes `Interrupted`, never successful because a file happens to
   exist. It neither resends requests nor kills saved PIDs, which may be reused.
   A hard process/OS crash can leave provider descendants: cleanup is explicitly
   unconfirmed in this state, and the controller must resolve the old process
   before offering a new job. Partial and late outputs remain quarantined.

The host supplies request IDs (ASCII alphanumeric, `-`, `_`, at most 64 bytes)
and a fresh source/context digest. IDs cannot be reused; a retry is a new job.
There is no task registry replacement or change to existing project storage.
Receipts have an independent version; unknown versions are rejected, not migrated
silently. Private requests/receipts belong outside source control and backups
must follow the host application's consent and data policies.

## Profiles and current limits

`DataOnly` means established absence of model tools, MCP, hooks, plugins and host
context discovery. **Neither reviewed live CLI is currently advertised with
that capability.** Help/auth probes are insufficient to establish it.

`TrustedHarness` is a separate opt-in profile. The CLI and administrator-managed
configuration may apply hooks, plugins/MCP, and host context. This may read or
transmit files outside the job directory and make network calls. The CLI itself
retains ordinary OS-user access and existing authentication. Restrictions are
requested; effective configuration and sandbox enforcement remain unverified,
and the effective tool catalog and injected context remain unknown. These are
possible capabilities, not evidence that extra files were read or transmitted.
The exact notice is part of the approval digest. Generic
approval to send fictional prompt data is not approval for this broader profile.

The application-controlled input is the prepared prompt, transport instructions,
correlation fields and output schema. The saved input/schema digests identify
those bytes, not all context the CLI may supply to a model. Data-category labels
describe the caller's intended projection; they do not sanitize or confine it.
`DataOnly` and `TrustedHarness` are transport capability labels, not installed
CLI named profiles.

- Codex adapter version: exactly `codex-cli 0.159.2`. Uses non-daemon `exec`,
  stdin, JSONL, a generated envelope schema, `--output-last-message`, ephemeral
  state, ignored user config/rules, read-only sandbox and no approval escalation.
  It requests disabling known shell/execution, hook, app, plugin, browser,
  computer, image, multi-agent and skill-integration features. Help probes check
  generic `--disable` syntax, not acceptance or enforcement of each feature name.
  These requests are **not** proof of a tool-free or confined-disclosure profile.
- Claude adapter version: exactly `2.1.286 (Claude Code)`. Uses print/JSON/schema,
  stdin and an explicit unique session. Requests restricted/safe mode, empty
  built-in tools, disallowed tools, strict empty MCP, empty setting sources,
  disabled hooks, disabled Chrome/session persistence, denied permission prompts
  and one turn.
  Managed hooks may still apply. `--bare` is intentionally not used: current docs
  say it ignores subscription auth and requires API credentials.
- Both adapters require existing subscription auth. API-key/third-party auth is
  not silently accepted. `ReadyUntested` means flags and auth are recognized,
  not that live generation, isolation or domain acceptance passed. Unknown CLI
  versions fail closed until reviewed. No auth files are copied into a job.
- Linux/macOS process lifecycle is implemented. Other platforms explicitly return
  unsupported; compilation/CI is not Windows/macOS live-provider acceptance.
  Unix process groups bound ordinary inherited children, timeout and cancellation;
  a malicious process can escape a group. This is not a security sandbox.
- Environment inheritance is cleared. Only credential HOME, a fixed PATH
  (selected executable directory plus `/usr/bin` and `/bin`), and UTF-8 locale
  remain. No ambient API key, proxy, shell startup, preload or plugin environment
  is inherited. Environments requiring other settings may be unavailable.
- Output byte limits do not constitute a hard disk quota for an untrusted CLI.
  The designated output is checked during execution and at ingestion; unrelated
  files written by a trusted provider are not a general arbitrary-code sandbox.
- Job files reject symlinks, hard links, special files, parent traversal,
  duplicate JSON keys, stale correlations, partial publication and mismatched
  routes. The application/CLI and same-UID host are trust boundaries; this is
  not tamper-proof storage against a malicious process with the same user rights.
- Rejection of tool/unsupported items in returned Codex JSONL occurs after
  execution. It does not prevent prior tool use or loading additional context.

## Protocol and provenance

The caller's schema is nested only under an opaque `payload` in a generated
transport envelope. Root-local JSON-pointer references are relocated into the wrapper; external
references, resource IDs, named anchors and legacy recursive keywords are
rejected rather than misrouted.
Other members are request ID, request digest, source digest,
provider and random job nonce. None grants evidence authority. Codex must emit
an ordered successful JSONL lifecycle and a matching designated final envelope;
Claude must return `structured_output`, a successful result and the requested
session. Plain prose in Claude's `result` field is never a substitute.

Receipts distinguish prepared/running, validated transport, cancellation,
timeout, interrupted, auth, expired authorization, quota/network errors, provider failure, invalid
output and output limit. Error-code classification is conservative: an unknown
error remains a provider error rather than guessing from prose. Raw reasoning
and stdout/stderr are not persisted; their digests and bounded metadata are.

Synthetic subprocesses are available only through a `cfg(test)` constructor
and their immutable provenance is `TransportFixture`. They never call a model.
Transport fixture success, live transport success and independent application
execution are three different evidence claims.

## Opt-in probe

Nothing in application startup or `cargo test --all-targets` invokes a real CLI.
`scripts/product_provider_probe.sh` is a manual operator/developer helper that
builds the non-test module without registering a new Cargo target. Run it with
no arguments for guidance. Its operations are:

- `probe codex|claude ABS_EXECUTABLE PRIVATE_JOB_ROOT NORMAL_CREDENTIAL_HOME`
- `prepare-synthetic ... JOB_ID`: locally prepares a fictional greeting, public
  schema and **TrustedHarness** disclosure; sends no model request
- `submit-synthetic ... JOB_ID DISCLOSURE_DIGEST APPROVAL_REFERENCE`: invokes the
  selected provider only after the operator has obtained actual separate
  approval of that disclosure. The application integration should collect this
  through its controller, never require ordinary users to edit JSON

The smallest proposed live check is one 60-second fictional greeting job, with
no repository files or real business data intentionally included. Approval must
also acknowledge the trusted installed Harness's possible host/configuration
reach. Save operational receipts privately. It establishes only transport
readiness; it does not pass generated-application or original-product acceptance.

## Primary CLI references

Flag/configuration decisions were reviewed against installed help and these
current official references; versions still fail closed when unreviewed:

- <https://learn.chatgpt.com/docs/non-interactive-mode>
- <https://learn.chatgpt.com/docs/config-file/config-reference>
- <https://learn.chatgpt.com/docs/permissions>
- <https://code.claude.com/docs/en/cli-reference>
- <https://code.claude.com/docs/en/headless>
- <https://code.claude.com/docs/en/hooks>

Read-only sandbox profiles limit sandboxed tool processes, not model-service
communication, MCP/apps or the provider CLI's own host access. A provider saying
it respected instructions is not an enforcement or execution attestation.
