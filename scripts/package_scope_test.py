#!/usr/bin/env python3
"""Export one already-compiled, source-bound Linux test harness; never build it."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile

TARGET_NAME = "product_scope_runtime_view_test"
HOST = "x86_64-unknown-linux-gnu"
REPOSITORY = "JohnXu22786/GitManager"
BRANCH = "refs/heads/feat/enforced-adoption-scope"
CARGO_COMMAND = ["cargo", "test", "--locked", "--no-run", "--test", TARGET_NAME,
                 "--message-format=json-render-diagnostics"]
TEST_PROFILE_ENV = {"CARGO_PROFILE_TEST_OPT_LEVEL": "1",
                    "CARGO_PROFILE_TEST_DEBUG_ASSERTIONS": "true",
                    "CARGO_PROFILE_TEST_OVERFLOW_CHECKS": "true"}


def command(argv, **kwargs):
    return subprocess.run(argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True, timeout=60, **kwargs).stdout


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def regular(path, executable=False):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError("input must be a regular, unaliased file")
    if info.st_mode & 0o6000 or (executable and not info.st_mode & 0o111):
        raise ValueError("unexpected executable permissions")
    return info


def source_identity(root, expected):
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("source SHA must be a complete lowercase Git commit ID")
    env = os.environ
    if (env.get("GITHUB_REPOSITORY") != REPOSITORY or env.get("GITHUB_EVENT_NAME") != "push"
            or env.get("GITHUB_REF") != BRANCH or env.get("GITHUB_SHA") != expected):
        raise ValueError("export requires the exact repository/branch push checkout")
    for name in ["GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"]:
        if not re.fullmatch(r"[1-9][0-9]*", env.get(name, "")):
            raise ValueError("missing run identity")
    if not env.get("GITHUB_JOB"):
        raise ValueError("missing job identity")
    git = lambda *args: command(["git", *args], cwd=root).strip()
    if git("rev-parse", "HEAD") != expected:
        raise ValueError("working source differs from the requested GitHub SHA")
    git("diff", "--exit-code", "HEAD")
    regular(root / "Cargo.lock")
    return {
        "repository": REPOSITORY, "source_sha": expected,
        "source_tree": git("rev-parse", "HEAD^{tree}"),
        "cargo_lock_sha256": digest(root / "Cargo.lock"),
        "workflow_path": ".github/workflows/ci.yml",
        "workflow_blob": git("rev-parse", "HEAD:.github/workflows/ci.yml"),
        "github": {"event": env["GITHUB_EVENT_NAME"], "ref": env["GITHUB_REF"],
                   "run_id": env["GITHUB_RUN_ID"], "run_attempt": env["GITHUB_RUN_ATTEMPT"],
                   "job": env["GITHUB_JOB"]},
        "runner": {"image": env.get("ImageOS"), "image_version": env.get("ImageVersion")},
    }


def profile_environment():
    selected = {name: os.environ.get(name) for name in TEST_PROFILE_ENV}
    if selected != TEST_PROFILE_ENV:
        raise ValueError("test profile environment differs from the complete CI test workload")
    return selected


def select_artifact(build_json, root):
    profile_environment()
    regular(build_json)
    if build_json.stat().st_size > 64 * 1024 * 1024:
        raise ValueError("Cargo message inventory is too large")
    selected, finished = [], []
    with build_json.open(encoding="utf-8") as source:
        for line in source:
            record = json.loads(line)
            if not isinstance(record, dict):
                raise ValueError("Cargo message must be a JSON object")
            if record.get("reason") == "build-finished":
                finished.append(record.get("success"))
            if record.get("reason") == "compiler-artifact" and record.get("target", {}).get("name") == TARGET_NAME:
                selected.append(record)
    if finished != [True] or len(selected) != 1:
        raise ValueError("need one selected compiler artifact and one successful finished build")
    artifact = selected[0]
    target, profile = artifact["target"], artifact.get("profile", {})
    if (target.get("kind") != ["test"] or profile.get("test") is not True
            or profile.get("opt_level") != "1" or profile.get("debug_assertions") is not True
            or profile.get("overflow_checks") is not True
            or artifact.get("manifest_path") != str(root / "Cargo.toml")
            or target.get("src_path") != str(root / "tests" / (TARGET_NAME + ".rs"))):
        raise ValueError("compiler artifact differs from the exact optimized test-profile integration target")
    executable = artifact.get("executable")
    if not isinstance(executable, str):
        raise ValueError("compiler artifact has no executable")
    path = Path(executable)
    if (not path.is_absolute() or path.resolve() != path
            or not path.is_relative_to(root / "target")):
        raise ValueError("executable must be an unsymlinked file in this checkout's target tree")
    regular(path, executable=True)
    return artifact


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def package(args):
    root = args.repo_root.resolve(strict=True)
    if args.output_dir.exists() or args.output_dir.is_symlink():
        raise FileExistsError("output directory must be fresh")
    output_parent = args.output_dir.parent.resolve(strict=True)
    if output_parent == root or output_parent.is_relative_to(root):
        raise ValueError("package output must be outside the checkout")
    identity = source_identity(root, args.source_sha)
    artifact = select_artifact(args.build_json, root)
    original = Path(artifact["executable"])
    rustc = command(["rustc", "--version", "--verbose"]).strip()
    cargo = command(["cargo", "--version"]).strip()
    if not rustc.startswith("rustc ") or rustc.splitlines().count("host: " + HOST) != 1:
        raise ValueError("rustc is not the requested native host toolchain")
    if not cargo.startswith("cargo ") or len(cargo.splitlines()) != 1:
        raise ValueError("invalid Cargo version output")
    stem = "scope-test-" + args.source_sha + "-" + identity["github"]["run_id"] + "-" + identity["github"]["run_attempt"]
    with tempfile.TemporaryDirectory(prefix=".scope-test-package-", dir=output_parent) as temp:
        staging = Path(temp)
        payload = staging / "payload"
        (payload / "bin").mkdir(parents=True)
        copied = payload / "bin" / TARGET_NAME
        before = regular(original, executable=True)
        with original.open("rb") as source, copied.open("xb") as destination:
            shutil.copyfileobj(source, destination, 1024 * 1024)
        after = original.stat()
        attributes = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
        original_digest = digest(original)
        if attributes(before) != attributes(after) or original_digest != digest(copied):
            raise ValueError("executable changed during packaging")
        copied.chmod(stat.S_IMODE(before.st_mode))
        with copied.open("rb") as source:
            header = source.read(20)
        if len(header) < 20 or header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
            raise ValueError("copied executable is not little-endian x86-64 ELF")
        reports = {}
        (payload / "elf").mkdir()
        for name, option in [("header", "--file-header"), ("program", "--program-headers"),
                             ("dynamic", "--dynamic"), ("versions", "--version-info")]:
            report = command(["readelf", "--wide", option, str(copied)])
            reports[name] = report
            (payload / "elf" / (name + ".txt")).write_text(report, encoding="utf-8")
        if re.search(r"\((?:RPATH|RUNPATH)\)", reports["dynamic"]):
            raise ValueError("unsupported loader search path; dependency trees are not exported")
        if "Requesting program interpreter: /lib64/ld-linux-x86-64.so.2" not in reports["program"]:
            raise ValueError("unsupported Linux ELF interpreter")
        runtime_dirs = {name: staging / name for name in ["empty", "home", "tmp", "config", "data", "cache"]}
        for directory in runtime_dirs.values():
            directory.mkdir()
        sanitized = {"PATH": "/usr/bin:/bin", "HOME": str(runtime_dirs["home"]),
                     "TMPDIR": str(runtime_dirs["tmp"]), "XDG_CONFIG_HOME": str(runtime_dirs["config"]),
                     "XDG_DATA_HOME": str(runtime_dirs["data"]), "XDG_CACHE_HOME": str(runtime_dirs["cache"]),
                     "LANG": "C", "LC_ALL": "C"}
        listing = command([str(copied), "--list", "--format", "terse"],
                          cwd=runtime_dirs["empty"], env=sanitized)
        tests = listing.splitlines()
        if (not tests or len(tests) != len(set(tests)) or any(not line.endswith(": test") for line in tests)
                or "future_rehearsals_preserve_selected_promises_without_activating_cohorts: test" not in tests
                or "admission_cache_reuses_only_fresh_exact_contexts: test" not in tests):
            raise ValueError("direct harness listing is missing or ambiguous")
        listed = regular(copied, executable=True)
        if (digest(copied) != original_digest or listed.st_size != before.st_size
                or stat.S_IMODE(listed.st_mode) != stat.S_IMODE(before.st_mode)):
            raise ValueError("copied executable changed during direct listing")
        (payload / "tests.txt").write_text(listing, encoding="utf-8")
        regular(root / "LICENSE")
        shutil.copyfile(root / "LICENSE", payload / "LICENSE")
        inventory = {}
        for path in sorted(payload.rglob("*")):
            if path.is_file():
                if path != copied:
                    path.chmod(0o644)
                inventory[path.relative_to(payload).as_posix()] = {
                    "sha256": digest(path), "size_bytes": path.stat().st_size,
                    "mode": stat.S_IMODE(path.stat().st_mode)}
        metadata = {"schema_version": 1, "distribution": "unsigned-ci-test-harness", "source": identity,
                    "target": HOST, "test_target": TARGET_NAME, "cargo_command": CARGO_COMMAND,
                    "toolchain": {"rustc_verbose": rustc, "cargo": cargo},
                    "profile": artifact["profile"], "features": artifact.get("features", []),
                    "profile_environment": profile_environment(),
                    "original_executable": {"path": original.relative_to(root).as_posix(),
                        "sha256": original_digest, "size_bytes": before.st_size,
                        "mode": stat.S_IMODE(before.st_mode)},
                    "binary": inventory["bin/" + TARGET_NAME], "members": inventory,
                    "archive_members": sorted([*inventory, "metadata.json"]),
                    "direct_list": {"success": True, "test_count": len(tests),
                                    "sha256": inventory["tests.txt"]["sha256"],
                                    "empty_cwd": True, "sanitized_environment": True},
                    "metadata_integrity": "metadata.json hash is in the sidecar complete member inventory"}
        metadata_bytes = encoded(metadata)
        (payload / "metadata.json").write_bytes(metadata_bytes)
        inventory = {**inventory, "metadata.json": {"sha256": hashlib.sha256(metadata_bytes).hexdigest(),
                     "size_bytes": len(metadata_bytes), "mode": 0o644}}
        packed = staging / "packed"
        packed.mkdir()
        archive = packed / (stem + ".tar.gz")
        with archive.open("xb") as output, gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                for name, item in sorted(inventory.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = item["size_bytes"], item["mode"], 0
                    with (payload / name).open("rb") as source:
                        tar.addfile(info, source)
        checksum = digest(archive)
        (packed / (stem + ".metadata.json")).write_bytes(metadata_bytes)
        (packed / (stem + ".members.json")).write_bytes(encoded({"members": inventory,
            "archive": {"name": archive.name, "sha256": checksum, "size_bytes": archive.stat().st_size}}))
        (packed / (archive.name + ".sha256")).write_text(checksum + "  " + archive.name + "\n", encoding="ascii")
        if source_identity(root, args.source_sha) != identity:
            raise ValueError("source identity changed during packaging")
        args.output_dir.mkdir()
        published = []
        try:
            for path in sorted(packed.iterdir()):
                destination = args.output_dir / path.name
                path.rename(destination)
                published.append(destination)
        except OSError:
            for path in published:
                path.unlink()
            args.output_dir.rmdir()
            raise
    return args.output_dir / (stem + ".tar.gz")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, required=True)
    parser.add_argument("--build-json", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        archive = package(args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"scope test packaging failed: {error}\n")
    print(json.dumps({"archive": str(archive), "archive_bytes": archive.stat().st_size,
                      "sha256": digest(archive)}))


if __name__ == "__main__":
    main()
