#!/usr/bin/env python3
"""Package an already-built Linux candidate without signing or publishing it."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import stat
import tarfile
import tempfile


TARGET = "x86_64-unknown-linux-gnu"


def package(args):
    if not re.fullmatch(r"[0-9a-f]{40}", args.source_sha):
        raise ValueError("source SHA must be a full, lowercase 40-character Git commit ID")
    if args.target != TARGET:
        raise ValueError(f"only the native Linux candidate target {TARGET} is supported")

    mode = args.binary.lstat().st_mode
    if not stat.S_ISREG(mode) or not mode & 0o111:
        raise ValueError("binary must be a regular executable file, not a symlink")
    binary = args.binary.read_bytes()
    if not binary:
        raise ValueError("binary must not be empty")

    rustc = args.rustc_info.read_text(encoding="utf-8").strip()
    cargo = args.cargo_info.read_text(encoding="utf-8").strip()
    if not rustc.startswith("rustc ") or rustc.splitlines().count(f"host: {TARGET}") != 1:
        raise ValueError("rustc info must contain rustc --version --verbose for the native target")
    if not cargo.startswith("cargo ") or len(cargo.splitlines()) != 1:
        raise ValueError("cargo info must contain the single-line cargo --version output")

    stem = f"git_manager-{args.source_sha}-{args.target}"
    archive_name = stem + ".tar.gz"
    metadata = {
        "schema_version": 1,
        "distribution": "unsigned-ci-test",
        "source_sha": args.source_sha,
        "source_url": f"https://github.com/JohnXu22786/GitManager/tree/{args.source_sha}",
        "target": args.target,
        "toolchain": {"rustc_verbose": rustc, "cargo": cargo},
        "binary": {
            "path": "git_manager",
            "sha256": hashlib.sha256(binary).hexdigest(),
            "size_bytes": len(binary),
        },
    }
    metadata_bytes = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode("utf-8")
    license_bytes = (Path(__file__).resolve().parent.parent / "LICENSE").read_bytes()

    # Build all files before reserving the destination. Never reuse an old package.
    with tempfile.TemporaryDirectory(prefix=".gitmanager-candidate-", dir=args.output_dir.parent) as staging:
        staging = Path(staging)
        archive_path = staging / archive_name
        with archive_path.open("xb") as output:
            # Omit filename and timestamps from gzip; normalize tar ownership/modes.
            with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                    for name, data, mode in [
                        ("git_manager", binary, 0o755),
                        ("metadata.json", metadata_bytes, 0o644),
                        ("LICENSE", license_bytes, 0o644),
                    ]:
                        info = tarfile.TarInfo(f"{stem}/{name}")
                        info.size = len(data)
                        info.mode = mode
                        info.mtime = 0
                        archive.addfile(info, io.BytesIO(data))
        (staging / (stem + ".metadata.json")).write_bytes(metadata_bytes)
        checksum = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        (staging / (archive_name + ".sha256")).write_text(
            f"{checksum}  {archive_name}\n", encoding="ascii"
        )

        # mkdir is an exclusive reservation, including against concurrent invocations.
        args.output_dir.mkdir()
        published = []
        try:
            for path in sorted(staging.iterdir()):
                destination = args.output_dir / path.name
                path.rename(destination)
                published.append(destination)
        except OSError:
            for path in published:
                path.unlink()
            args.output_dir.rmdir()
            raise
    return args.output_dir / archive_name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--rustc-info", type=Path, required=True)
    parser.add_argument("--cargo-info", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        archive = package(args)
    except (OSError, ValueError) as error:
        parser.exit(1, f"candidate packaging failed: {error}\n")
    print(archive)


if __name__ == "__main__":
    main()
