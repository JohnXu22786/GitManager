# Owned-effect safety analysis (not executable composition)

`ProductStore::analyze_owned_effects` analyzes an exact current/candidate source
pair against the store's validated current snapshot. `OwnedEffectRequest` is an
untrusted suggestion: the host checks its snapshot/source identities and
reconstructs the complete structural report. No caller-built report can certify
an effect, and the internal verified analysis cannot be deserialized.

This API neither changes data nor prepares, lowers, executes, persists, or adopts
a scoped implementation. Its serializable report is diagnostic evidence only.
Historical completion values, lifecycle seals, bounded executable lowering,
replay, adoption, and withdrawal remain separate implementation gates.

## Initial grammar

- One unchanged typed root and an otherwise empty scope lineage
- Additive auxiliary entities absent from the authenticated retained schema,
  with a required root-reference owner, exactly owner-only uniqueness, and no
  constraints
- Variant writes to the exact root or proven owned auxiliary rows, with no
  owner transfer, archives, collection mutation, shared writes, state writes,
  dependent assertions/outputs, or variant root creation
- Positive mandatory conjunctions of typed reference equality establish
  correlation; disjunction, negation, same-entity aliases, and arbitrary
  reference expressions do not establish ownership; correlation keys used as
  ownership evidence have no non-create writers in either source, including
  writes through aliases
- Bounded correlated queries and read-only traversal through shared references;
  scoped field dependencies are resolved by entity/type and lexical identity
- Affected queries use MAX_COLLECTION and unsupported ordering fails closed;
  existing loop headers remain exact across paired spans, and no query rewriting,
  membership pushdown, or binder substitution occurs
- Root-local scalar row projections and direct Count(Query(root)) observations;
  no general aggregate algebra, detail/history reconstruction, or global
  business aggregates dependent on a variant

All actions are scanned, including unchanged statements and their guard/control
dependencies. A monotone typed-field closure catches indirect dependencies through
common writes. Another root's value, a global aggregate, or a shared target cannot
become local merely because the originating expression sits inside an owned span.
Common control-dependent effects outside this grammar produce an explicit source
side/action/path error; they are not silently exempted.

The original structural resolver still enforces exact span coverage, ordered
common statements, lexical binder correspondence, and nonescaping bindings. All
scalar-v1 serialization, compiler versions, preparation/adoption APIs, and stored
formats are unchanged.

## Common factual phase inventory

`ProductStore::inventory_common_fact_phases` is a separate, opt-in diagnostic.
Its `CommonFactInventoryRequest` binds the exact source correspondence and
validated current snapshot, plus an explicitly untrusted lifecycle suggestion.
A correctly typed own-row predicate does not authenticate its business meaning:
calling an active-status predicate “completed” grants no exemption. The before
and after interpretations, meaning text, and every lifecycle-field writer remain
in the report for review.

The inventory covers both complete programs, including unchanged and Invoke-only
actions. Ordered action frames distinguish complete identical subtrees, common
loop headers with variant descendants, and declared variant spans. They retain
exact subtree digests, paths, adjacent statements, entering typed environments,
and enclosing controls. A matching loop header is never promoted to a common
subtree. These are potential factual phases, not verified common effects.

Typed expression and ingress inventories retain field/reference provenance,
queries and membership, implicit reference validation, order and sort limits,
state keys, inputs/arguments, controls/callbacks, selection, keys, constraints,
uniqueness, observables, and emitted output expressions. Required reference
fields are only immutable-owner candidates: all producers and mutations are
listed, and following a shared reference confers no write authority. The full
cumulative retained schema and stored-row count are bound to the snapshot,
rather than inferred from equal source definitions.

Unmet obligations identify concrete sites for ordinary admission and phase
reachability, shared writes, root/lifecycle coupling, owned-span lifecycle
preservation, retained owner proof, input/reset closure, global constraints,
resource atomicity, and fresh-identity/order dependencies. A preceding variant
Create can shift later common IDs. A repeated loop can carry that dependency
across iterations. Untruncated queries still use ID order and tie-breaking;
`MAX_COLLECTION` does not establish order independence. The record cap is global
and includes archived and auxiliary records. Runtime fuel, collection, statement,
write, output, elapsed, and cancellation limits remain real. Failed inputs do not
commit business/session/output changes, but can update fuel and trace/outcome.

The serializable inventory is not deserializable proof authority and is never
accepted by preparation or adoption. Every invocation re-derives it from current
sources and basis. It does not relax the existing effect analyzer, recognize a
new certificate, alter scalar-v1, lower or execute a program, or change persisted
state. An inventory may succeed while listing unsupported obligations and while
`analyze_owned_effects` continues to refuse the exact same shared effect.
