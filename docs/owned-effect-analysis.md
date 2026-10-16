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
