# Unsigned Linux CI candidates

The existing CI workflow creates a test package after successful tests in its
Ubuntu job, only for a push to `main`. Pull requests, other branches and the
Windows/macOS test jobs do not create packages. This does not publish a release,
create a tag, sign a binary or change repository permissions. A Linux artifact
does not mean the other matrix jobs or product acceptance checks have passed.

## Find the exact candidate

Open the repository's **Actions → CI** run for the required full `main` commit
SHA and check its current results. Under that run's artifacts, download
`git_manager-<full-sha>-x86_64-unknown-linux-gnu-unsigned`. Artifacts are retained
for 14 days. Check the run and SHA rather than using an old release asset.

Unzip the Actions download. It contains three files with a common
`git_manager-<full-sha>-x86_64-unknown-linux-gnu` prefix:

- `.tar.gz`: the executable, `metadata.json` and GPL `LICENSE` in one directory
- `.metadata.json`: the same metadata available without extracting the archive
- `.tar.gz.sha256`: the archive checksum in `sha256sum` format

The tar layer preserves the executable's `0755` mode even when the Actions
artifact transport does not preserve executable permissions. Metadata identifies
the complete source SHA, corresponding source URL, target, exact Rust/Cargo
version output, executable size and SHA-256 digest. The checksum detects changed
bytes; it is not a signature or an independent authenticity guarantee. Obtain all
three files from the expected repository's CI run.

## Extract and try it

This candidate targets **Linux x86_64 with glibc**, built on the CI workflow's
`ubuntu-latest` runner. It is not a universal Linux installer and does not bundle
system libraries. Older distributions, headless machines and other architectures
are not established as compatible. A graphical desktop and the native libraries
needed by the application must be available; a missing library or display is a
failed launch, not a successful smoke test.

In the download directory, verify the checksum before extracting. Replace the
placeholder with the full commit SHA from that run:

```sh
sha256sum -c git_manager-<full-sha>-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf git_manager-<full-sha>-x86_64-unknown-linux-gnu.tar.gz
./git_manager-<full-sha>-x86_64-unknown-linux-gnu/git_manager
```

No Rust toolchain or source build is needed to run the extracted executable.
These commands are maintainer verification steps; creating the package alone
does not establish an unguided installation experience or a supported desktop.
Do not point an unvalidated candidate at the only copy of important work.

## Build and packaging contract

CI verifies the checkout matches `GITHUB_SHA`, runs
`cargo build --locked --release --bin git_manager --target x86_64-unknown-linux-gnu`
in a dedicated `CARGO_TARGET_DIR`, then checks that tracked source is unchanged.
It captures `rustc --version --verbose` and `cargo --version` and passes those
files, the built executable and exact SHA to `scripts/package_candidate.py`.
The script does not build, execute or attest the supplied binary; CI owns the
source-to-build association. No credentials or user data enter the package.

The Python standard-library helper requires a regular, nonempty executable, a
full lowercase Git SHA and the native target's compiler metadata. It refuses an
existing output directory, including symlinks, and requires an existing parent
directory. Use a fresh destination for retries. It normalizes tar names, file
modes, owners and timestamps, and omits gzip timestamps and filenames. Given
identical binary, license and metadata bytes under the same Python/zlib versions,
repackaging produces identical archive bytes.

This is **reproducible packaging**, not a claim of bit-identical Rust rebuilds:
the application's existing build script embeds a build date, and the workflow's
runner and compiler can advance. Metadata records the actual compiler versions;
the CI run records the runner image. No release-version rewrite is performed.

## Validation boundaries

`cargo test --test candidate_packaging_test` checks packaging with a harmless
synthetic executable on Linux: archive contents and modes, extracted execution,
source/toolchain metadata, binary and archive hashes, deterministic repackaging,
changed-binary identity, invalid inputs and output collision refusal. Its workflow
guard check also runs in the existing cross-platform `cargo test --all-targets`
jobs. It checks source-level gating and permissions, not GitHub's execution engine.
The test uses only Rust's standard library and Python 3 on Linux, so it can also
be compiled directly with `rustc --test` when Cargo dependencies are unavailable.

An uploaded package proves that CI built and packaged those bytes. Final product
acceptance still requires the final combined source and fresh artifact to be
downloaded, verified and launched on an actual desktop, with default data
locations, first work, close/reopen, recovery and unchanged legacy data checked.
Synthetic fixture execution and automated UI tests cannot replace that evidence.
Windows/macOS packages, signing and public releases remain separate work.
