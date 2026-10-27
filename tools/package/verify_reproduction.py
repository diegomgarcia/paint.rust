#!/usr/bin/env python3
"""Require byte-identical rebuild files or nonempty output trees using only stdlib."""

from __future__ import annotations

import argparse
import filecmp
import hashlib
import json
import sys
from pathlib import Path
from typing import Sequence


class ReproductionError(RuntimeError):
    """Two expected reproduction outputs differ or are unsafe to compare."""


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _kind(path: Path) -> str:
    if path.is_symlink():
        raise ReproductionError("symbolic-link roots are not reproduction evidence")
    if path.is_file():
        return "file"
    if path.is_dir():
        return "directory"
    raise ReproductionError("reproduction input is missing or is not a regular file/directory")


def _snapshot(root: Path) -> dict[str, Path]:
    records: dict[str, Path] = {}
    for path in root.rglob("*"):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ReproductionError(f"symbolic-link output is not reproduction evidence: {relative}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ReproductionError(f"non-regular output is not reproduction evidence: {relative}")
        records[relative] = path
    if not records:
        raise ReproductionError("reproduction output directory is empty")
    return records


def _compare_files(first: Path, second: Path, label: str) -> tuple[str, int]:
    if not filecmp.cmp(first, second, shallow=False):
        raise ReproductionError(f"reproduced content differs: {label}")
    first_hash = _sha256(first)
    if first_hash != _sha256(second):
        raise ReproductionError(f"reproduced content differs: {label}")
    return first_hash, first.stat().st_size


def compare_paths(first: Path, second: Path) -> dict[str, object]:
    """Return deterministic evidence only after exact byte/tree equality succeeds."""
    first_kind = _kind(first)
    second_kind = _kind(second)
    if first_kind != second_kind:
        raise ReproductionError("reproduction inputs have different input kinds")
    if first_kind == "file":
        digest, size = _compare_files(first, second, "file")
        return {"status": "pass", "kind": "file", "bytes": size, "sha256": digest}

    first_files = _snapshot(first)
    second_files = _snapshot(second)
    if set(first_files) != set(second_files):
        missing = sorted(set(first_files) - set(second_files))
        extra = sorted(set(second_files) - set(first_files))
        raise ReproductionError(f"reproduction entries differ: missing={missing}, extra={extra}")
    tree = hashlib.sha256()
    files: list[dict[str, object]] = []
    for relative in sorted(first_files):
        digest, size = _compare_files(first_files[relative], second_files[relative], relative)
        encoded = relative.encode("utf-8")
        tree.update(len(encoded).to_bytes(4, "big"))
        tree.update(encoded)
        tree.update(bytes.fromhex(digest))
        files.append({"path": relative, "bytes": size, "sha256": digest})
    return {
        "status": "pass",
        "kind": "directory",
        "entries": len(files),
        "tree_sha256": tree.hexdigest(),
        "files": files,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    arguments = parser.parse_args(argv)
    try:
        report = compare_paths(arguments.first, arguments.second)
    except (OSError, ReproductionError) as error:
        print(f"reproduction verification failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
