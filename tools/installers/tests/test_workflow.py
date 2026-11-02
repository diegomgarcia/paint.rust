"""Static policy checks for the installer/release workflow.

The checks use PyYAML when it is installed and otherwise fall back to a small
block-style YAML subset parser so the suite also runs on a bare Python. These
tests do not replace actionlint or a real workflow run.
"""

from __future__ import annotations

import json
import re
import unittest
from pathlib import Path
from typing import Any

WORKFLOW_PATH = Path(__file__).resolve().parents[3] / ".github/workflows/installers.yml"

EXPECTED_ACTION_SHAS = {
    "actions/checkout": "11bd71901bbe5b1630ceea73d27597364c9af683",  # v4.2.2
    "actions/setup-python": "a26af69be951a213d495a4c3e4e4022e16d87065",  # v5.6.0
    "actions/upload-artifact": "ea165f8d65b6e75b540449e92b4886f43607fa02",  # v4.6.2
}

PLATFORMS = {
    "macos-aarch64": {
        "os": "macos-14",
        "binary": "target/release/paint-ui",
        "suffix": ".zip",
        "installer": "tools/installers/macos/build.sh",
    },
    "windows-x86_64": {
        "os": "windows-2022",
        "binary": "target/release/paint-ui.exe",
        "suffix": ".zip",
        "installer": "tools/installers/windows/build.ps1",
    },
    "linux-x86_64": {
        "os": "ubuntu-24.04",
        "binary": "target/release/paint-ui",
        "suffix": ".tar.gz",
        "installer": "tools/installers/linux/build.sh",
    },
}


def _strip_comment(line: str) -> str:
    """Remove a YAML comment that starts outside single or double quotes."""
    single = double = False
    index = 0
    while index < len(line):
        character = line[index]
        if double and character == "\\":
            index += 2
            continue
        if character == "'" and not double:
            single = not single
        elif character == '"' and not single:
            double = not double
        elif character == "#" and not single and not double and (index == 0 or line[index - 1] in " \t"):
            return line[:index].rstrip()
        index += 1
    return line.rstrip()


def _split_key(text: str) -> tuple[str, str]:
    single = double = False
    for index, character in enumerate(text):
        if character == "'" and not double:
            single = not single
        elif character == '"' and not single:
            double = not double
        elif character == ":" and not single and not double and (index + 1 == len(text) or text[index + 1] == " "):
            return text[:index].strip(), text[index + 1 :].strip()
    raise ValueError(f"expected a mapping entry, got: {text!r}")


def _looks_like_key(text: str) -> bool:
    try:
        _split_key(text)
    except ValueError:
        return False
    return True


def _parse_scalar(text: str) -> Any:
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        try:
            return json.loads(text)
        except ValueError:
            return text[1:-1]
    if len(text) >= 2 and text[0] == "'" and text[-1] == "'":
        return text[1:-1].replace("''", "'")
    if text.startswith("[") and text.endswith("]"):
        inner = text[1:-1].strip()
        if not inner:
            return []
        return [_parse_scalar(part.strip()) for part in _split_flow(inner)]
    lowered = text.lower()
    if lowered in {"true", "false"}:
        return lowered == "true"
    if lowered in {"null", "~"}:
        return None
    if re.fullmatch(r"-?[0-9]+", text):
        return int(text)
    return text


def _split_flow(text: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    single = double = False
    start = 0
    for index, character in enumerate(text):
        if character == "'" and not double:
            single = not single
        elif character == '"' and not single:
            double = not double
        elif not single and not double:
            if character in "[{":
                depth += 1
            elif character in "]}":
                depth -= 1
            elif character == "," and depth == 0:
                parts.append(text[start:index])
                start = index + 1
    parts.append(text[start:])
    return parts


class _SubsetParser:
    """Parse the block mappings, sequences, scalars, and block scalars used here."""

    def __init__(self, text: str) -> None:
        self.lines: list[tuple[int, str, str]] = []
        for raw in text.splitlines():
            line = _strip_comment(raw)
            if not line.strip():
                continue
            indent = len(line) - len(line.lstrip(" "))
            self.lines.append((indent, line.strip(), line))

    def parse(self) -> Any:
        if not self.lines:
            return None
        value, index = self._parse_block(0, self.lines[0][0])
        if index != len(self.lines):
            raise ValueError(f"unparsed YAML content at line {index + 1}: {self.lines[index][2]!r}")
        return value

    def _parse_block(self, index: int, indent: int) -> tuple[Any, int]:
        text = self.lines[index][1]
        if text == "-" or text.startswith("- "):
            return self._parse_sequence(index, indent)
        return self._parse_mapping(index, indent)

    def _parse_mapping(self, index: int, indent: int) -> tuple[dict[str, Any], int]:
        result: dict[str, Any] = {}
        while index < len(self.lines):
            current_indent, text, _ = self.lines[index]
            if current_indent < indent:
                break
            if current_indent > indent:
                raise ValueError(f"unexpected indentation at line {index + 1}: {text!r}")
            if text == "-" or text.startswith("- "):
                break
            key, rest = _split_key(text)
            index += 1
            value, index = self._parse_value(rest, index, indent)
            result[key] = value
        return result, index

    def _parse_sequence(self, index: int, indent: int) -> tuple[list[Any], int]:
        result: list[Any] = []
        while index < len(self.lines):
            current_indent, text, _ = self.lines[index]
            if current_indent < indent:
                break
            if current_indent > indent:
                raise ValueError(f"unexpected indentation at line {index + 1}: {text!r}")
            if not (text == "-" or text.startswith("- ")):
                break
            content = text[1:].strip()
            index += 1
            if not content:
                if index < len(self.lines) and self.lines[index][0] > indent:
                    item, index = self._parse_block(index, self.lines[index][0])
                else:
                    item = None
                result.append(item)
                continue
            if _looks_like_key(content):
                key, rest = _split_key(content)
                item: dict[str, Any] = {}
                value, index = self._parse_value(rest, index, indent + 2)
                item[key] = value
                if (
                    index < len(self.lines)
                    and self.lines[index][0] == indent + 2
                    and not (self.lines[index][1] == "-" or self.lines[index][1].startswith("- "))
                ):
                    extra, index = self._parse_mapping(index, indent + 2)
                    item.update(extra)
                result.append(item)
            else:
                result.append(_parse_scalar(content))
        return result, index

    def _parse_value(self, rest: str, index: int, parent_indent: int) -> tuple[Any, int]:
        if rest.startswith("|") or rest.startswith(">"):
            return self._parse_block_scalar(rest, index, parent_indent)
        if rest:
            return _parse_scalar(rest), index
        if index < len(self.lines) and self.lines[index][0] > parent_indent:
            return self._parse_block(index, self.lines[index][0])
        return None, index

    def _parse_block_scalar(self, marker: str, index: int, parent_indent: int) -> tuple[str, int]:
        collected: list[str] = []
        content_indents: list[int] = []
        while index < len(self.lines) and self.lines[index][0] > parent_indent:
            collected.append(self.lines[index][2])
            content_indents.append(self.lines[index][0])
            index += 1
        if not collected:
            return "", index
        base = min(content_indents)
        lines = [line[base:] for line in collected]
        if marker.startswith(">"):
            rendered = " ".join(line for line in lines if line)
        else:
            rendered = "\n".join(lines)
        if marker.endswith("-"):
            return rendered, index
        return rendered + "\n", index


def parse_yaml_subset(text: str) -> Any:
    """Public hook so tests can exercise the fallback even when PyYAML is present."""
    return _SubsetParser(text).parse()


def load_workflow(path: Path = WORKFLOW_PATH) -> tuple[Any, str]:
    text = path.read_text(encoding="utf-8")
    try:
        import yaml  # type: ignore[import-not-found]
    except ImportError:
        return parse_yaml_subset(text), "subset-parser"
    data = yaml.safe_load(text)
    if isinstance(data, dict) and "on" not in data and True in data:
        data["on"] = data.pop(True)
    return data, "pyyaml"


def _text(value: Any) -> str:
    if isinstance(value, str):
        return value
    return json.dumps(value, sort_keys=True)


def _steps(job: dict[str, Any]) -> list[dict[str, Any]]:
    steps = job.get("steps", [])
    if not isinstance(steps, list):
        raise AssertionError(f"job steps are not a list: {steps!r}")
    return [step for step in steps if isinstance(step, dict)]


def _runs(job: dict[str, Any]) -> list[tuple[str, str]]:
    return [(str(step.get("name", "")), str(step.get("run", ""))) for step in _steps(job) if "run" in step]


def _step_named(job: dict[str, Any], fragment: str) -> dict[str, Any]:
    matches = [step for step in _steps(job) if fragment.lower() in str(step.get("name", "")).lower()]
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one step named like {fragment!r}, found {len(matches)}")
    return matches[0]


@unittest.skipUnless(WORKFLOW_PATH.is_file(), "installers workflow is missing")
class InstallersWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow, cls.parser = load_workflow()
        cls.text = WORKFLOW_PATH.read_text(encoding="utf-8")
        cls.jobs = cls.workflow["jobs"]
        cls.installers = cls.jobs["installers"]
        cls.release = cls.jobs["release"]

    def test_workflow_parses_and_is_named_installers(self) -> None:
        self.assertIsInstance(self.workflow, dict)
        self.assertEqual(self.workflow["name"], "installers")
        self.assertEqual(self.workflow["concurrency"]["group"], "installers-${{ github.workflow }}-${{ github.ref }}")
        self.assertIn("SOURCE_DATE_EPOCH", self.workflow["env"])

    def test_fallback_parser_agrees_on_structure(self) -> None:
        fallback = parse_yaml_subset(self.text)
        self.assertEqual(fallback["name"], self.workflow["name"])
        self.assertEqual(list(fallback["jobs"]), list(self.jobs))
        self.assertEqual(
            fallback["jobs"]["installers"]["strategy"]["matrix"]["include"],
            self.installers["strategy"]["matrix"]["include"],
        )
        self.assertEqual(fallback["jobs"]["release"]["needs"], self.release["needs"])
        self.assertEqual(fallback["on"], self.workflow["on"])

    def test_triggers_only_main_tags_and_dispatch(self) -> None:
        triggers = self.workflow["on"]
        self.assertEqual(sorted(triggers), ["push", "workflow_dispatch"])
        self.assertEqual(triggers["push"]["branches"], ["main"])
        self.assertEqual(triggers["push"]["tags"], ["v*"])
        self.assertNotIn("pull_request", triggers)
        self.assertNotIn("pull_request_target", triggers)
        self.assertNotRegex(self.text, r"(?m)^\s*pull_request:")
        dispatch = triggers["workflow_dispatch"]
        self.assertEqual(dispatch["inputs"]["publish"]["default"], False)
        self.assertEqual(dispatch["inputs"]["publish"]["type"], "boolean")
        self.assertIn("workflow_dispatch", self.text)

    def test_default_permissions_are_read_only(self) -> None:
        self.assertEqual(self.workflow["permissions"], {"contents": "read"})
        write_jobs = [
            name
            for name, job in self.jobs.items()
            if isinstance(job.get("permissions"), dict) and job["permissions"].get("contents") == "write"
        ]
        self.assertEqual(write_jobs, ["release"])
        self.assertEqual(self.release["permissions"]["contents"], "write")
        self.assertEqual(self.release["permissions"]["actions"], "read")

    def test_three_platform_matrix_matches_installer_interfaces(self) -> None:
        matrix = self.installers["strategy"]["matrix"]["include"]
        self.assertEqual(len(matrix), 3)
        by_platform = {entry["platform"]: entry for entry in matrix}
        self.assertEqual(set(by_platform), set(PLATFORMS))
        for platform, expected in PLATFORMS.items():
            with self.subTest(platform=platform):
                entry = by_platform[platform]
                self.assertEqual(entry["os"], expected["os"])
                self.assertEqual(entry["binary"], expected["binary"])
                self.assertEqual(entry["portable_suffix"], expected["suffix"])
        self.assertEqual(self.installers["runs-on"], "${{ matrix.os }}")
        self.assertEqual(self.installers["defaults"]["run"]["shell"], "bash")

    def test_builds_release_once_and_uses_the_pinned_wrapper(self) -> None:
        builds = [run for name, run in _runs(self.installers) if "build" in run and "preview_package" not in run]
        self.assertTrue(builds)
        self.assertIn("bash ./tools/cargo-pinned.sh fetch --locked", self.text)
        self.assertIn("bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui", self.text)
        self.assertEqual(
            self.text.count("bash ./tools/cargo-pinned.sh build --release --locked --package paint-ui"),
            1,
        )
        self.assertIn("rustup toolchain install 1.95.0", self.text)
        for name, run in _runs(self.installers):
            with self.subTest(step=name):
                self.assertNotRegex(run, r"(?m)^\s*cargo\s")

    def test_portable_archive_uses_the_canonical_packaging_command(self) -> None:
        step = _step_named(self.installers, "Build portable preview archive")
        run = step["run"]
        for fragment in (
            "python3 tools/package/preview_package.py",
            '--binary "$BINARY"',
            '--platform "$PLATFORM"',
            "--output-dir dist-portable",
            "--repeat 2",
            "--timeout 30",
            "--distribution internal-preview",
            "--cargo-command bash ./tools/cargo-pinned.sh",
        ):
            self.assertIn(fragment, run)
        self.assertEqual(step["env"]["BINARY"], "${{ matrix.binary }}")
        self.assertEqual(step["env"]["PLATFORM"], "${{ matrix.platform }}")

    def test_linux_native_libraries_are_installed(self) -> None:
        step = _step_named(self.installers, "Linux native libraries")
        self.assertEqual(step["if"], "matrix.platform == 'linux-x86_64'")
        run = step["run"]
        for package in (
            "libgtk-3-dev",
            "libxkbcommon-dev",
            "libwayland-dev",
            "libxcb-render0-dev",
            "libxcb-shape0-dev",
            "libxcb-xfixes0-dev",
            "libgl1-mesa-dev",
            "libegl1-mesa-dev",
            "libvulkan-dev",
            "libssl-dev",
            "pkg-config",
        ):
            self.assertIn(package, run)
        self.assertIn("sudo apt-get update", run)

    def test_windows_nsis_is_installed_for_the_installer_step(self) -> None:
        step = _step_named(self.installers, "NSIS")
        self.assertEqual(step["if"], "matrix.platform == 'windows-x86_64'")
        self.assertEqual(step["shell"], "pwsh")
        self.assertIn("choco install nsis", step["run"])
        self.assertIn("makensis.exe", step["run"])

    def test_installer_scripts_are_invoked_with_fixed_interfaces(self) -> None:
        mac = _step_named(self.installers, "macOS installer")
        self.assertEqual(mac["if"], "matrix.platform == 'macos-aarch64'")
        self.assertIn('bash tools/installers/macos/build.sh "$VERSION" "${archives[0]}" "installer-out"', mac["run"])
        self.assertIn("dist-portable/paint-r-*-macos-aarch64.zip", mac["run"])

        windows = _step_named(self.installers, "Windows installer")
        self.assertEqual(windows["if"], "matrix.platform == 'windows-x86_64'")
        self.assertEqual(windows["shell"], "pwsh")
        self.assertIn(
            'pwsh -File tools/installers/windows/build.ps1 -Version "$env:VERSION" '
            '-BinaryPath "target/release/paint-ui.exe" -OutputDir "installer-out"',
            windows["run"],
        )

        linux = _step_named(self.installers, "Linux installer")
        self.assertEqual(linux["if"], "matrix.platform == 'linux-x86_64'")
        self.assertIn(
            'bash tools/installers/linux/build.sh "$VERSION" "target/release/paint-ui" "installer-out"',
            linux["run"],
        )

        versions = {_step_named(self.installers, name)["env"]["VERSION"] for name in ("macOS installer", "Windows installer", "Linux installer")}
        self.assertEqual(versions, {"${{ steps.meta.outputs.version }}"})

    def test_installer_outputs_are_verified_and_hashed(self) -> None:
        step = _step_named(self.installers, "Verify installer files")
        run = step["run"]
        for fragment in (
            "installer-out/*.dmg",
            "installer-out/*.pkg",
            "installer-out/*.exe",
            "installer-out/*.msi",
            "installer-out/*.appimage",
            "installer-out/*.deb",
            "installer-out/*.rpm",
        ):
            self.assertIn(fragment, run)
        hashing = _step_named(self.installers, "SHA-256")
        self.assertIn('path.name + ".sha256"', hashing["run"])
        self.assertIn("hashlib.file_digest", hashing["run"])

    def test_two_pinned_artifacts_per_platform_with_14_day_retention(self) -> None:
        uploads = [step for step in _steps(self.installers) if str(step.get("uses", "")).startswith("actions/upload-artifact@")]
        self.assertEqual(len(uploads), 2)
        expected = {
            "paint-r-portable-${{ matrix.platform }}": "dist-portable/",
            "paint-r-installers-${{ matrix.platform }}": "installer-out/",
        }
        seen = {}
        for step in uploads:
            with_block = step["with"]
            seen[with_block["name"]] = with_block["path"]
            self.assertEqual(with_block["if-no-files-found"], "error")
            self.assertEqual(with_block["retention-days"], 14)
        self.assertEqual(seen, expected)

    def test_release_job_runs_only_after_tagged_installers(self) -> None:
        self.assertEqual(self.release["needs"], ["installers"])
        condition = self.release["if"]
        self.assertIn("startsWith(github.ref, 'refs/tags/v')", condition)
        self.assertIn("github.event_name == 'push'", condition)
        self.assertIn("github.event.inputs.publish == 'true'", condition)
        self.assertEqual(self.release["runs-on"], "ubuntu-24.04")
        self.assertEqual(self.release["env"]["GH_TOKEN"], "${{ github.token }}")
        self.assertEqual(self.release["env"]["RELEASE_TAG"], "${{ github.ref_name }}")

    def test_tag_values_are_passed_through_env_and_quoted(self) -> None:
        for job in self.jobs.values():
            for name, run in _runs(job):
                with self.subTest(step=name):
                    self.assertNotIn("${{ github.ref_name }}", run)
                    self.assertNotIn("${{ github.ref }}", run)
                    self.assertNotIn("${{ github.event.inputs", run)
        publish = _step_named(self.release, "Create or update")
        self.assertIn('tag="$RELEASE_TAG"', publish["run"])
        self.assertIn('$RELEASE_TAG"', self.text)
        self.assertIn('[[ ! "$tag" =~ ^v[0-9]+', publish["run"])
        self.assertIn('gh release view "$tag"', publish["run"])

    def test_artifacts_are_downloaded_with_gh_instead_of_an_action(self) -> None:
        step = _step_named(self.release, "Download installers")
        self.assertIn('gh run download "$GITHUB_RUN_ID" --dir release-assets', step["run"])
        self.assertNotIn("actions/download-artifact", self.text)

    def test_release_assets_are_verified_with_sidecars(self) -> None:
        verify = _step_named(self.release, "Verify downloaded release assets")
        run = verify["run"]
        for fragment in (
            "paint-r-portable-",
            "paint-r-installers-",
            "macos-aarch64",
            "windows-x86_64",
            "linux-x86_64",
            ".sha256",
            "hashlib.file_digest",
            "duplicate release asset name",
        ):
            self.assertIn(fragment, run)

    def test_release_creation_is_a_prerelease_with_documented_notes(self) -> None:
        notes = _step_named(self.release, "Resolve release title")
        run = notes["run"]
        self.assertIn('docs/releases/v0.1.1.md', run)
        self.assertIn("v0.1.1", run)
        self.assertIn("Paint.Rust ${version} — PDR sessions and multi-format open", run)

        publish = _step_named(self.release, "Create or update")
        run = publish["run"]
        self.assertIn('gh release create "$tag"', run)
        self.assertIn("--prerelease", run)
        self.assertIn("--verify-tag", run)
        self.assertIn('--title "$RELEASE_TITLE"', run)
        self.assertIn("--notes-file release-notes.md", run)
        self.assertIn('gh release upload "$tag"', run)
        self.assertIn("--clobber", run)

    def test_release_is_honest_about_unsigned_development_previews(self) -> None:
        lowered = self.text.lower()
        self.assertIn("unsigned", lowered)
        self.assertIn("notar", lowered)
        self.assertIn("No code signing or notarization is claimed", self.text)
        self.assertNotIn("Developer ID Application", self.text)
        self.assertNotIn("codesign ", self.text)
        self.assertNotIn("signtool", lowered)

    def test_no_other_publishing_tooling_is_used(self) -> None:
        lowered = self.text.lower()
        for fragment in ("softprops/action-gh-release", "actions/create-release", "actions/upload-release-asset", "cargo publish", "npm publish"):
            self.assertNotIn(fragment, lowered)
        self.assertEqual(self.workflow["permissions"].get("contents"), "read")

    def test_external_actions_are_pinned_and_allowlisted(self) -> None:
        uses = re.findall(r"(?m)^\s*-?\s*uses:\s*(\S+)", self.text)
        self.assertTrue(uses)
        for use in uses:
            action, separator, revision = use.partition("@")
            self.assertTrue(separator, use)
            self.assertIn(action, EXPECTED_ACTION_SHAS, action)
            self.assertEqual(revision, EXPECTED_ACTION_SHAS[action], action)
            self.assertRegex(revision, r"^[0-9a-f]{40}$")

    def test_no_job_claims_signing_or_uses_a_download_action(self) -> None:
        for name, job in self.jobs.items():
            for step in _steps(job):
                self.assertNotIn("download-artifact", str(step.get("uses", "")), name)


if __name__ == "__main__":
    unittest.main()
