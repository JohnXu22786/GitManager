# Comparison source preparation

`ComparisonEngine::minimize` validates each exact captured program once, then
keeps a call-local immutable owning capability for its source bytes, parsed AST,
artifact digests and complete source binding. Only successful full validation can
create it. The capability is private to the crate, has no mutable fields or
serialization route, and is never stored in a global cache or persisted.

Pure observable types and view schemas are derived once from that validated AST.
An internal source wrapper carries the capability through comparison, replay,
compatibility, session validation and observation rendering. The public APIs
still validate arbitrary caller-supplied captures and application definitions.
Ordinary inputs are borrowed until a validated run takes its own execution copy.

Every reduction still executes both independent runs in the same order. No
scenario, output, observation, admission, dynamic data or session value is cached.
Reference and type checks, guards, constraints, host admissions, cancellation and
all execution meters are fresh. Runtime expression type inference is unchanged:
its fuel charges remain part of actual execution. Trials, audit receipts,
evidence identities, certificate limits and stale-source checks are unchanged.
A faster execution can naturally meet a wall-clock deadline that a slower one
misses; no deadline check or fuel charge is removed.

## Regression checks

Run `cargo test --locked --test product_prepared_comparison_test`. The synthetic
fixture compares the complete ordinary/prepared reports, including all retained
runs, reduction audit entries and the final minimality certificate. Thread-local
test counters assert that immutable validation/metadata work depends on the two
captures, not the number of trials. Admission counters independently establish
that every trial and each executed input still runs. Additional cases mutate
captures and dynamic inputs and exercise rejected guards/constraints, reference
errors, type errors, budgets, admission changes and cancellation.

These are component regressions, not live or held-out acceptance. Timing
measurements belong in an isolated release-profile replay with identical inputs;
CI does not assert a wall-clock speedup or reduce proof steps to meet a deadline.
