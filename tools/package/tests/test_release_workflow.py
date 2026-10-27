"""Release workflow policy checks; these do not replace actionlint or CI execution."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


class ReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[3]
        cls.workflow = (cls.root / ".github/workflows/release.yml").read_text(encoding="utf-8")

    def test_manual_three_os_matrix_and_pinned_tooling(self) -> None:
        self.assertIn("workflow_dispatch:", self.workflow)
        self.assertNotRegex(self.workflow, r"(?m)^\s*(push|pull_request|release):")
        for runner in ("windows-2022", "macos-14", "ubuntu-24.04"):
            self.assertIn(runner, self.workflow)
        self.assertIn("1.95.0", self.workflow)
        self.assertIn("SOURCE_DATE_EPOCH", self.workflow)
        self.assertIn("tools/cargo-pinned.sh", self.workflow)

    def test_workflow_builds_tests_smokes_and_uploads_only_artifacts(self) -> None:
        self.assertIn("test --workspace --all-features --locked", self.workflow)
        self.assertIn("build --release --locked --package paint-ui", self.workflow)
        self.assertIn("tools/package/preview_package.py", self.workflow)
        self.assertIn("actions/upload-artifact@v4", self.workflow)
        self.assertIn("if-no-files-found: error", self.workflow)
        self.assertIn("scan-stubs", self.workflow)
        self.assertIn("actions/setup-dotnet@v4", self.workflow)
        self.assertIn("dotnet restore plugin-host/Paint.PluginHost.sln --locked-mode", self.workflow)
        self.assertIn("dotnet test plugin-host/Paint.PluginHost.sln", self.workflow)
        self.assertIn("plugin-host/test-cross-language.sh", self.workflow)
        self.assertIn("discover -s tests/e2e", self.workflow)
        self.assertIn("discover -s tools/reference/tests", self.workflow)

    def test_workflow_has_no_release_or_publish_capability(self) -> None:
        self.assertIn("contents: read", self.workflow)
        self.assertNotIn("contents: write", self.workflow)
        forbidden = (
            r"\bgh\s+release\b",
            r"actions/create-release",
            r"actions/upload-release-asset",
            r"softprops/action-gh-release",
            r"\bcargo\s+publish\b",
            r"\bnpm\s+publish\b",
        )
        for pattern in forbidden:
            self.assertNotRegex(self.workflow, re.compile(pattern, re.IGNORECASE))


if __name__ == "__main__":
    unittest.main()
