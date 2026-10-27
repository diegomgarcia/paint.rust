"""Tests for deterministic file/tree reproduction comparisons using disposable fixtures."""

from __future__ import annotations

import hashlib
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import verify_reproduction as reproduction


class ReproductionComparisonTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="paint-r-reproduction-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def write(self, path: Path, content: bytes) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)

    def test_identical_files_report_size_and_sha256(self) -> None:
        first = self.root / "first.bin"
        second = self.root / "second.bin"
        content = b"disposable reproducible executable fixture\n"
        self.write(first, content)
        self.write(second, content)

        report = reproduction.compare_paths(first, second)

        self.assertEqual(
            report,
            {
                "status": "pass",
                "kind": "file",
                "bytes": len(content),
                "sha256": hashlib.sha256(content).hexdigest(),
            },
        )

    def test_different_file_content_is_rejected(self) -> None:
        first = self.root / "first.bin"
        second = self.root / "second.bin"
        self.write(first, b"first")
        self.write(second, b"second")
        with self.assertRaisesRegex(reproduction.ReproductionError, "content differs"):
            reproduction.compare_paths(first, second)

    def test_identical_trees_ignore_creation_order_and_mtime(self) -> None:
        first = self.root / "first"
        second = self.root / "second"
        third = self.root / "third"
        records = (("archive.zip", b"archive"), ("archive.zip.sha256", b"hash\n"), ("nested/receipt.json", b"{}\n"))
        for index, (relative, content) in enumerate(records):
            self.write(first / relative, content)
            os.utime(first / relative, (1_700_000_000 + index, 1_700_000_000 + index))
        for relative, content in reversed(records):
            self.write(second / relative, content)
            self.write(third / relative, content)

        first_report = reproduction.compare_paths(first, second)
        second_report = reproduction.compare_paths(first, third)

        self.assertEqual(first_report, second_report)
        self.assertEqual(first_report["status"], "pass")
        self.assertEqual(first_report["kind"], "directory")
        self.assertEqual(first_report["entries"], 3)
        self.assertEqual(
            [record["path"] for record in first_report["files"]],
            ["archive.zip", "archive.zip.sha256", "nested/receipt.json"],
        )
        self.assertNotIn(str(self.root), repr(first_report))

    def test_content_missing_extra_empty_and_kind_mismatches_are_rejected(self) -> None:
        for name in ("content", "missing", "extra", "kind"):
            first = self.root / name / "first"
            second = self.root / name / "second"
            self.write(first / "payload.bin", b"payload")
            self.write(second / "payload.bin", b"payload")
            if name == "content":
                self.write(second / "payload.bin", b"changed")
            elif name == "missing":
                (second / "payload.bin").unlink()
            elif name == "extra":
                self.write(second / "extra.bin", b"extra")
            elif name == "kind":
                second = self.root / name / "file.bin"
                self.write(second, b"payload")
            with self.subTest(case=name):
                with self.assertRaises(reproduction.ReproductionError):
                    reproduction.compare_paths(first, second)
        empty_first = self.root / "empty/first"
        empty_second = self.root / "empty/second"
        empty_first.mkdir(parents=True)
        empty_second.mkdir()
        with self.assertRaisesRegex(reproduction.ReproductionError, "empty"):
            reproduction.compare_paths(empty_first, empty_second)


if __name__ == "__main__":
    unittest.main()
