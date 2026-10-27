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

    def test_linux_ci_shellchecks_build_and_native_launchers(self) -> None:
        expected = (
            "- name: ShellCheck build and native launchers\n"
            "        if: matrix.os == 'ubuntu-24.04'\n"
            "        run: shellcheck tools/cargo-pinned.sh plugin-host/test-cross-language.sh tools/macos_native_window_smoke.sh"
        )
        for workflow_name, text in (("ci", self.ci), ("release", self.workflow)):
            with self.subTest(workflow=workflow_name):
                self.assertIn(expected, text)

    def test_ci_runs_native_gate_orchestration_without_claiming_native_input(self) -> None:
        expected = (
            "- name: Native gate orchestration tests (no native input)\n"
            "        run: python3 -m unittest discover -s tools/tests -p 'test_*.py' -v"
        )
        for workflow_name, text in (("ci", self.ci), ("release", self.workflow)):
            with self.subTest(workflow=workflow_name):
                self.assertIn(expected, text)

    def test_ci_runs_the_release_package_gate_without_uploading(self) -> None:
        build = self.ci.index("- name: Release build")
        package = self.ci.index("- name: Reproduce and verify release binary and complete internal package")
        self.assertLess(build, package)
        for fragment in (
            'platform: windows-x86_64\n            binary: target/release/paint-ui.exe',
            'platform: macos-aarch64\n            binary: target/release/paint-ui',
            'platform: linux-x86_64\n            binary: target/release/paint-ui',
            '--output-dir ci-dist',
            '--repeat 2',
            '--distribution internal-preview',
            '--cargo-command bash ./tools/cargo-pinned.sh',
        ):
            self.assertIn(fragment, self.ci)
        self.assertNotIn("actions/upload-artifact@", self.ci)

    def test_internal_upload_is_source_qualified_and_follows_verification(self) -> None:
        package = self.workflow.index("- name: Reproduce and verify internal preview packages")
        upload = self.workflow.index("- name: Upload internal CI artifact")
        self.assertLess(package, upload)
        self.assertIn("name: paint-r-internal-preview-${{ matrix.platform }}-${{ github.sha }}", self.workflow)
        self.assertIn("path: dist-a/", self.workflow)

    def test_workflows_compare_two_clean_builds_and_complete_package_outputs(self) -> None:
        for workflow_name, text, output in (
            ("ci", self.ci, "ci-dist"),
            ("release", self.workflow, "dist"),
        ):
            with self.subTest(workflow=workflow_name):
                first_build = text.index(
                    "CARGO_TARGET_DIR=repro-a bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui"
                )
                second_build = text.index(
                    "CARGO_TARGET_DIR=repro-b bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui"
                )
                binary_compare = text.index(
                    'verify_reproduction.py "repro-a/release/${binary_name}" "repro-b/release/${binary_name}"'
                )
                first_package = text.index(f'--output-dir {output}-a')
                second_package = text.index(f'--output-dir {output}-b')
                tree_compare = text.index(f'verify_reproduction.py {output}-a {output}-b')
                self.assertLess(first_build, second_build)
                self.assertLess(second_build, binary_compare)
                self.assertLess(binary_compare, first_package)
                self.assertLess(first_package, second_package)
                self.assertLess(second_package, tree_compare)
                self.assertIn('--binary "repro-a/release/${binary_name}"', text)
                self.assertIn('--binary "repro-b/release/${binary_name}"', text)
        self.assertEqual(self.workflow.count('binary_name="$(basename "${{ matrix.binary }}")"'), 2)

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
