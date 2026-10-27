"""Release workflow policy checks; these do not replace actionlint or CI execution."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


EXPECTED_ACTION_SHAS = {
    "actions/checkout": "11bd71901bbe5b1630ceea73d27597364c9af683",
    "actions/setup-python": "a26af69be951a213d495a4c3e4e4022e16d87065",
    "actions/setup-dotnet": "67a3573c9a986a3f9c594539f4ab511d57bb3ce9",
    "actions/upload-artifact": "ea165f8d65b6e75b540449e92b4886f43607fa02",
}


class ReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[3]
        cls.workflow = (cls.root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        cls.ci = (cls.root / ".github/workflows/ci.yml").read_text(encoding="utf-8")

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
        self.assertIn("actions/upload-artifact@", self.workflow)
        self.assertIn("if-no-files-found: error", self.workflow)
        self.assertIn("scan-stubs", self.workflow)
        self.assertIn("actions/setup-dotnet@", self.workflow)
        self.assertIn("dotnet restore plugin-host/Paint.PluginHost.sln --locked-mode", self.workflow)
        self.assertIn("dotnet test plugin-host/Paint.PluginHost.sln", self.workflow)
        self.assertIn("plugin-host/test-cross-language.sh", self.workflow)
        self.assertIn("discover -s tests/e2e", self.workflow)
        self.assertIn("discover -s tools/reference/tests", self.workflow)

    def test_external_actions_are_immutable_and_allowlisted(self) -> None:
        for workflow_name, text in (("ci", self.ci), ("release", self.workflow)):
            uses = re.findall(r"(?m)^\s*-?\s*uses:\s*(\S+)", text)
            self.assertTrue(uses, workflow_name)
            for use in uses:
                action, separator, revision = use.partition("@")
                self.assertTrue(separator, f"{workflow_name}: {use}")
                self.assertIn(action, EXPECTED_ACTION_SHAS, f"{workflow_name}: {action}")
                self.assertEqual(
                    revision,
                    EXPECTED_ACTION_SHAS[action],
                    f"{workflow_name}: {action} must use its reviewed immutable commit",
                )

    def test_ci_uses_pinned_cargo_wrapper_on_three_os_matrix(self) -> None:
        for runner in ("windows-2022", "macos-14", "ubuntu-24.04"):
            self.assertIn(runner, self.ci)
        self.assertIn("1.95.0", self.ci)
        self.assertIn("defaults:\n      run:\n        shell: bash", self.ci)
        for workflow_name, text in (("ci", self.ci), ("release", self.workflow)):
            self.assertNotRegex(
                text,
                r"(?m)^\s*run:\s*cargo\s",
                f"{workflow_name}: direct Cargo bypasses the pinned companion-tool wrapper",
            )
            self.assertIn("bash ./tools/cargo-pinned.sh fetch --locked", text)
            self.assertIn("bash ./tools/cargo-pinned.sh test --workspace", text)

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
