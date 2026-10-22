#!/usr/bin/env python3
"""Verify and extract the protocol-manifest-pinned Bogota frame EEST corpus."""

from __future__ import annotations

import argparse
import hashlib
import shutil
import tarfile
import tomllib
from collections import Counter
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    workspace = Path(__file__).resolve().parent.parent
    manifest = tomllib.loads((workspace / "docs/frame-transactions-protocol.toml").read_text())
    expected = manifest["qualification"]["fixture_archive_sha256"]
    with args.archive.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != expected:
        raise SystemExit(f"fixture digest mismatch: expected {expected}, got {actual}")
    destination = args.destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    prefixes = tuple(f"eip{eip}_" for eip in manifest["eips"])
    counts: Counter[str] = Counter()
    with tarfile.open(args.archive, "r|gz") as archive:
        for member in archive:
            parts = Path(member.name).parts
            if not member.isfile() or not any(part.startswith(prefixes) for part in parts):
                continue
            kind = next((part for part in parts if part in ("state_tests", "blockchain_tests")), None)
            if kind is None:
                continue
            relative = Path(*parts[parts.index(kind):])
            output = (destination / relative).resolve()
            if not output.is_relative_to(destination):
                raise SystemExit(f"unsafe fixture path: {member.name}")
            output.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            assert source is not None
            with source, output.open("wb") as target:
                shutil.copyfileobj(source, target)
            counts[kind] += 1
    print(f"Release: {manifest['qualification']['fixture_release']}")
    print(f"SHA256: {actual}")
    print(f"Extracted frame fixture files: {dict(sorted(counts.items()))}")
    print("Selection: four EIP families in state_tests and blockchain_tests; engine and execute-only formats excluded.")


if __name__ == "__main__":
    main()
