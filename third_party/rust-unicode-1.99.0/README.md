# Extracted Rust lowercase support — 1.99.0 / Unicode 17.0.0

This is a small **extracted and adapted** subset, not a complete Rust source
distribution or a new Unicode generator. Upstream repository:
<https://github.com/rust-lang/rust>, exact commit
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`.

## Required pin and scope

`src/product_runtime/fallible_lower.rs` uses stable `char::to_lowercase`,
`is_lowercase` and `is_uppercase` from that exact Rust compiler/sysroot. Their
mapping data are **not** vendored here. Qualification requires the supplied
Rust 1.99.0 compiler at that commit and Unicode 17.0.0; changing compiler/sysroot
requires requalification, even if its Unicode version tuple is unchanged.
Supplied rustc SHA-256:
`f3834d26669b03f6855fa54bd2381443123e860167bc0c7a8dd137e5cf6e5e4f`.
This identifies supplied bytes, not a reproducible-build attestation.

The helper is unregistered/unrouted. It does not change evaluator checkpoints,
error plumbing, budgets, paid routes, limits, normalization or locale behavior.
Its finite tests use pinned **whole-string** `input.to_lowercase()` as the old
oracle, outside fault/observation scopes. They are not exhaustive Unicode tests
or evidence of whole-product recoverable-OOM completion.

## Upstream sources and modifications

All paths/line numbers below refer to the exact commit above. A source URL is
`https://github.com/rust-lang/rust/blob/` + commit + `/` + path.

* `library/alloc/src/str.rs`, lines 213–225 and 433–465: original-input final
  sigma context and whole-string lowering. Adapted into an inline mapping
  iterator, two checked-length/emission passes, and one fallible reserve. No
  ASCII allocating fast path was copied. No unstable predicate is called.
  Git blob: `4c86d7e06ae44abdf1c239d8d6966ec983fac206`.
  SHA-256: `f160c025a52379d8300a6d2602d162362568691ff6f3b224ab9d0d92f5c03666`.
* `library/core/src/char/methods.rs`, lines 828–833 and 1299–1304: Cased
  composition and five ASCII Case_Ignorable exceptions. Adapted using stable
  lowercase/uppercase predicates plus exact titlecase data.
  Git blob: `f6930e0a60d429f37c5c63583ecd5a85397eeb2a`.
  SHA-256: `644def5f8476adaa7450a6d677023d9c771eccb42d2b92b0dff61e1218a06546`.
* `library/core/src/unicode/unicode_data.rs`, lines 67–146, 263–343, 718–747:
  skip-search algorithm, all 36 Case_Ignorable header inputs and 919 offset
  bytes, and Lt data decoded into ten ranges/31 scalars. Header numeric inputs
  and offset bytes are unchanged. Header storage is unpacked; binary search,
  unchecked access and intrinsics were replaced by bounded safe linear lookup
  and compile-time invariants. Run-local sentinel exclusion, zero offsets,
  global index parity and strict-greater prefix selection are retained.
  Git blob: `ca3a276b89d90911822f27798f84903cc64d62c1`.
  SHA-256: `d3d218b7574f08efe423e9d4b6b539e700046aca4f2a2793d38164ec5a1b00b2`.

## Notices

Rust algorithm/wrapper material is used under its MIT option. Complete,
unchanged upstream `LICENSE-MIT` is included here:
copyright (c) The Rust Project Contributors.
Git blob: `a2070c4be7acd5316f8d0357ca3c12419a2f3c62`.
SHA-256: `b71bd43a069ca0641a9ecfe585ca7b3c53b5cc1608f8b68321168698e28b5ea1`.

The pinned `REUSE.toml` specifically assigns generated `unicode_data.rs` to
Unicode-3.0 (Git blob `7d72caa6d5c45de02d4115134329d6195c757e5b`). The extracted
CI data and derived Lt ranges retain that license, not merely Rust's general
MIT/Apache choice. Complete, unchanged upstream `LICENSES/Unicode-3.0.txt` is
included as `Unicode-3.0.txt`: copyright © 1991–2024 Unicode, Inc.
Git blob: `ee8e69b233293872fcb4d6d4930ec196b63626e7`.
SHA-256: `f5062c9a188d81dfe66b56db4182dcf9e4b17c0d9b0d311a8e20b3a1b075c443`.
