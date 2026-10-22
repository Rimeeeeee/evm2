#!/usr/bin/env python3
"""Verify and extract the pinned Bogota frame EEST corpus."""

from __future__ import annotations

import argparse
import hashlib
import shutil
import tarfile
from collections import Counter
from pathlib import Path

RELEASE = "tests-bogota-devnet@v1.1.1"
ARCHIVE_NAME = "fixtures_bogota-devnet.tar.gz"
ARCHIVE_SHA256 = "2b7e0cbfa221f09235212880fc6e2d8ad1189fd1865344491488a8d9c762efa6"
EXPECTED_FILES = 419
EXPECTED_UNITS = 1481
EIPS = [8141, 8250, 8272, 7906]


def validate_extraction(
    destination: Path, selected: set[Path], eips: list[int], expected_files: int
) -> None:
    """Reject partial selections and JSON left over from a different extraction."""
    actual = {path.relative_to(destination) for path in destination.rglob("*.json")}
    if actual != selected:
        missing = sorted(str(path) for path in selected - actual)
        stale = sorted(str(path) for path in actual - selected)
        raise SystemExit(f"fixture selection mismatch: missing={missing}, stale={stale}")
    if len(selected) != expected_files:
        raise SystemExit(
            f"fixture count mismatch: expected {expected_files}, extracted {len(selected)}"
        )
    for eip in eips:
        if not any(any(part.startswith(f"eip{eip}_") for part in path.parts) for path in selected):
            raise SystemExit(f"no JSON fixtures selected for EIP-{eip}")
    for kind in ("state_tests", "blockchain_tests"):
        if not any(path.parts[0] == kind for path in selected):
            raise SystemExit(f"no JSON fixtures selected for {kind}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    with args.archive.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != ARCHIVE_SHA256:
        raise SystemExit(f"fixture digest mismatch: expected {ARCHIVE_SHA256}, got {actual}")
    destination = args.destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    prefixes = tuple(f"eip{eip}_" for eip in EIPS)
    counts: Counter[str] = Counter()
    selected: set[Path] = set()
    with tarfile.open(args.archive, "r|gz") as archive:
        for member in archive:
            parts = Path(member.name).parts
            if (
                not member.isfile()
                or not member.name.endswith(".json")
                or not any(part.startswith(prefixes) for part in parts)
            ):
                continue
            kind = next((part for part in parts if part in ("state_tests", "blockchain_tests")), None)
            if kind is None:
                continue
            relative = Path(*parts[parts.index(kind):])
            output = (destination / relative).resolve()
            if not output.is_relative_to(destination):
                raise SystemExit(f"unsafe fixture path: {member.name}")
            if relative in selected:
                raise SystemExit(f"duplicate fixture path: {member.name}")
            selected.add(relative)
            output.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            assert source is not None
            with source, output.open("wb") as target:
                shutil.copyfileobj(source, target)
            counts[kind] += 1
    validate_extraction(destination, selected, EIPS, EXPECTED_FILES)
    print(f"Release: ethereum/execution-specs {RELEASE}")
    print(f"SHA256: {actual}")
    print(f"Extracted frame fixture files: {dict(sorted(counts.items()))}")
    print("Selection: four EIP families in state_tests and blockchain_tests; engine and execute-only formats excluded.")


if __name__ == "__main__":
    main()
