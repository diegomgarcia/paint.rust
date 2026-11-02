"""Cross-platform contract tests for the Windows installer lane.

No test invokes makensis or claims that an installer was produced. When pwsh is
available, build.ps1 is exercised in -PlanOnly and argument-validation modes
against a synthetic PE fixture. Otherwise those cases are skipped explicitly and
the always-on checks validate the NSIS script, build driver, plan contract, and
documentation.
"""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import struct
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
INSTALLER_DIR = ROOT / "tools" / "installers" / "windows"
NSIS_SCRIPT = INSTALLER_DIR / "paint-rust.nsi"
BUILD_SCRIPT = INSTALLER_DIR / "build.ps1"
INSTALL_TEXT = INSTALLER_DIR / "INSTALL-WINDOWS.txt"
README = INSTALLER_DIR / "README.md"

PWSH = shutil.which("pwsh") or shutil.which("pwsh.exe")
MAKENSIS = shutil.which("makensis") or shutil.which("makensis.exe")

CI_INTERFACE = (
    'pwsh -File tools/installers/windows/build.ps1 -Version "<version>" '
    '-BinaryPath "<target/release/paint-ui.exe>" -OutputDir "<output-dir>"'
)


def read_utf8(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def nsis_code(text: str) -> str:
    """Remove NSIS comments outside double-quoted strings."""
    lines: list[str] = []
    for line in text.splitlines():
        out: list[str] = []
        in_string = False
        for character in line:
            if character == '"':
                in_string = not in_string
                out.append(character)
            elif not in_string and character in ";#":
                break
            else:
                out.append(character)
        lines.append("".join(out))
    return "\n".join(lines)


def numeric_version(version: str) -> str:
    core = version.split("-", 1)[0]
    major, minor, patch = (int(part) for part in core.split("."))
    return f"{major}.{minor}.{patch}.0"


def expected_plan(version: str, binary: Path, output_dir: Path) -> dict[str, str | bool]:
    setup = output_dir / f"Paint.Rust-{version}-windows-x86_64-setup.exe"
    return {
        "setup": str(setup),
        "sidecar": f"{setup}.sha256",
        "version_numeric": numeric_version(version),
        "unsigned": True,
    }


def fake_pe(machine: int = 0x8664) -> bytes:
    data = bytearray(0x200)
    data[0:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\x00\x00"
    struct.pack_into("<H", data, 0x84, machine)
    return bytes(data)


class NsisScriptTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.text = read_utf8(NSIS_SCRIPT)
        cls.code = nsis_code(cls.text)

    def test_unicode_mui2_and_expected_pages(self) -> None:
        self.assertRegex(self.code, r"(?m)^Unicode true\s*$")
        self.assertIn('!include "MUI2.nsh"', self.code)
        for macro in (
            "MUI_PAGE_WELCOME",
            "MUI_PAGE_COMPONENTS",
            "MUI_PAGE_DIRECTORY",
            "MUI_PAGE_INSTFILES",
            "MUI_PAGE_FINISH",
            "MUI_UNPAGE_CONFIRM",
            "MUI_UNPAGE_INSTFILES",
            "MUI_UNPAGE_FINISH",
            "MUI_LANGUAGE",
        ):
            self.assertIn(f"!insertmacro {macro}", self.code)
        self.assertIn('!insertmacro MUI_LANGUAGE "English"', self.code)

    def test_required_defines_and_output_definition(self) -> None:
        for define in ("VERSION", "VI_VERSION", "BINARY", "OUTFILE", "INSTALL_TEXT"):
            self.assertRegex(self.code, rf"(?m)^!ifndef {define}\s*$")
        self.assertGreaterEqual(self.code.count("!error"), 5)
        self.assertIn('OutFile "${OUTFILE}"', self.code)
        self.assertIn('VIProductVersion "${VI_VERSION}"', self.code)

    def test_per_machine_program_files_and_admin_request(self) -> None:
        self.assertIn("RequestExecutionLevel admin", self.code)
        self.assertIn('InstallDir "$PROGRAMFILES64\\Paint.Rust"', self.code)
        self.assertIn("SetShellVarContext all", self.code)
        self.assertIn("${IfNot} ${RunningX64}", self.code)
        self.assertIn("Abort", self.code)

    def test_product_metadata_and_uninstall_registry_entry(self) -> None:
        self.assertIn('!define APP_NAME "Paint.Rust"', self.code)
        self.assertIn('!define APP_PUBLISHER "Diego Garcia"', self.code)
        self.assertIn(
            '!define APP_URL "https://github.com/diegomgarcia/paint.rust"',
            self.code,
        )
        self.assertIn('Name "${APP_NAME}"', self.code)
        self.assertIn(
            '!define UNINSTALL_KEY "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Paint.Rust"',
            self.code,
        )
        for value in (
            "DisplayName",
            "DisplayVersion",
            "Publisher",
            "ProductURL",
            "InstallLocation",
            "DisplayIcon",
            "UninstallString",
            "QuietUninstallString",
            "NoModify",
            "NoRepair",
        ):
            self.assertIn(f'"{value}"', self.code)
        self.assertIn("'\"$INSTDIR\\uninstall.exe\"'", self.code)
        self.assertIn("'\"$INSTDIR\\uninstall.exe\" /S'", self.code)

    def test_start_menu_and_optional_desktop_shortcut(self) -> None:
        self.assertIn(
            'CreateShortCut "$SMPROGRAMS\\Paint.Rust\\Paint.Rust.lnk" '
            '"$INSTDIR\\${APP_EXE}"',
            self.code,
        )
        self.assertIn(
            'CreateShortCut "$SMPROGRAMS\\Paint.Rust\\Uninstall Paint.Rust.lnk" '
            '"$INSTDIR\\uninstall.exe"',
            self.code,
        )
        self.assertRegex(self.code, r'(?m)^Section /o "Desktop shortcut" SEC_DESKTOP\s*$')
        self.assertIn(
            'CreateShortCut "$DESKTOP\\Paint.Rust.lnk" "$INSTDIR\\${APP_EXE}"',
            self.code,
        )

    def test_uninstaller_removes_only_installer_files_and_preserves_user_data(self) -> None:
        self.assertIn('WriteUninstaller "$INSTDIR\\uninstall.exe"', self.code)
        self.assertRegex(self.code, r'(?m)^Section "Uninstall"\s*$')

        expected_deletes = {
            "$SMPROGRAMS\\Paint.Rust\\Paint.Rust.lnk",
            "$SMPROGRAMS\\Paint.Rust\\Uninstall Paint.Rust.lnk",
            "$DESKTOP\\Paint.Rust.lnk",
            "$INSTDIR\\${APP_EXE}",
            "$INSTDIR\\${INSTALL_TEXT_FILE}",
            "$INSTDIR\\LICENSE",
            "$INSTDIR\\uninstall.exe",
        }
        actual_deletes = {
            match.group(1)
            for match in re.finditer(r'(?m)^\s*Delete\s+"([^"]+)"', self.code)
        }
        self.assertEqual(actual_deletes, expected_deletes)

        rmdirs = {
            match.group(1)
            for match in re.finditer(r'(?m)^\s*RMDir\s+"([^"]+)"', self.code)
        }
        self.assertEqual(rmdirs, {"$SMPROGRAMS\\Paint.Rust", "$INSTDIR"})
        self.assertNotRegex(self.code, r"(?i)\bRMDir\s+/r\b")
        self.assertNotRegex(self.code, r"(?i)\bDelete\s+/r\b")
        for forbidden in (
            "$APPDATA",
            "$LOCALAPPDATA",
            "$DOCUMENTS",
            "$PROFILE",
            "$WINDIR",
            "$SYSDIR",
            "*.*",
        ):
            self.assertNotIn(forbidden, self.code)
        self.assertIn("Non-recursive", self.text)
        self.assertIn("never touched", self.text)

    def test_no_signing_commands_or_signing_claim(self) -> None:
        for forbidden in (
            "SignTool",
            "signtool",
            "Set-AuthenticodeSignature",
            "osslsigncode",
            "AzureSignTool",
            "!finalize",
            "!uninstfinalize",
        ):
            self.assertNotIn(forbidden, self.code)
        self.assertNotRegex(
            self.code,
            r"(?i)\b(?:digitally|code)[- ]signed\b",
        )
        self.assertIn("unsigned", self.code.lower())

    def test_payload_has_no_proprietary_asset_files(self) -> None:
        files = [
            match.group(1).strip()
            for match in re.finditer(r"(?m)^\s*File\s+(.+)$", self.code)
        ]
        self.assertEqual(len(files), 3, files)
        self.assertTrue(any("${BINARY}" in entry for entry in files))
        self.assertTrue(
            any(
                "INSTALL_TEXT_FILE" in entry and "${INSTALL_TEXT}" in entry
                for entry in files
            )
        )
        self.assertTrue(any("${LICENSE_FILE}" in entry for entry in files))
        self.assertIn("!ifdef LICENSE_FILE", self.code)
        self.assertNotRegex(
            self.code,
            r"(?i)\b(?:Icon|MUI_ICON|MUI_UNICON|MUI_HEADERIMAGE)\b",
        )
        self.assertNotRegex(
            self.code,
            r"(?i)\.(?:ico|png|jpe?g|bmp|gif|svg)\b",
        )

    def test_embedded_install_text_is_present(self) -> None:
        text = read_utf8(INSTALL_TEXT)
        self.assertIn("UNSIGNED BUILD", text)
        self.assertIn("C:\\Program Files\\Paint.Rust", text)
        self.assertIn("Settings > Apps", text)
        self.assertIn("documents", text)
        self.assertIn("https://github.com/diegomgarcia/paint.rust", text)
        self.assertIn("File /oname=${INSTALL_TEXT_FILE}", self.code)


class BuildScriptTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.text = read_utf8(BUILD_SCRIPT)

    def test_required_interface_parameters(self) -> None:
        for name in ("Version", "BinaryPath", "OutputDir"):
            self.assertRegex(
                self.text,
                rf"\[Parameter\(Mandatory = \$true\)\]\s*\n\s*\[string\] \${name}",
            )
        self.assertRegex(self.text, r"\[string\] \$MakensisPath = ''")
        self.assertRegex(self.text, r"\[switch\] \$PlanOnly")
        self.assertIn("Set-StrictMode -Version Latest", self.text)
        self.assertIn("$ErrorActionPreference = 'Stop'", self.text)

    def test_output_name_and_sha256_sidecar_contract(self) -> None:
        self.assertIn(
            "$setupName = 'Paint.Rust-{0}-windows-x86_64-setup.exe' -f $versionInfo.Text",
            self.text,
        )
        self.assertIn("$sidecarPath = '{0}.sha256' -f $setupPath", self.text)
        self.assertIn("$sidecarContent = '{0}  {1}{2}' -f $setupHash, $setupName", self.text)
        self.assertIn("System.Security.Cryptography.SHA256", self.text)
        self.assertIn("ComputeHash", self.text)
        self.assertIn(".ToLowerInvariant()", self.text)
        self.assertIn("WriteAllText", self.text)
        self.assertIn("ReadAllText", self.text)

    def test_makensis_discovery_and_quoted_invocation(self) -> None:
        self.assertIn("Get-Command -Name $name -CommandType Application", self.text)
        self.assertIn("'makensis.exe', 'makensis'", self.text)
        self.assertIn("'ProgramFiles(x86)'", self.text)
        self.assertIn("'NSIS\\makensis.exe'", self.text)
        self.assertIn("$MakensisPath", self.text)
        self.assertIn("& $makensis @makensisArguments", self.text)
        self.assertIn("$LASTEXITCODE", self.text)
        self.assertIn("$optionPrefix = '/'", self.text)
        self.assertIn("[System.Environment]::OSVersion.Platform", self.text)
        for define in (
            "VERSION",
            "VI_VERSION",
            "BINARY",
            "OUTFILE",
            "INSTALL_TEXT",
            "LICENSE_FILE",
        ):
            self.assertIn(f"($optionPrefix + 'D{define}=' +", self.text)
        self.assertNotIn("Invoke-Expression", self.text)
        self.assertNotRegex(self.text, r"(?i)\bcmd(?:\.exe)?\s+/c\b")

    def test_binary_and_output_validation(self) -> None:
        self.assertIn("'paint-ui.exe'", self.text)
        self.assertIn("-ine 'paint-ui.exe'", self.text)
        self.assertIn("0x8664", self.text)
        self.assertIn("0x4D", self.text)
        self.assertIn("0x5A", self.text)
        self.assertIn("0x50", self.text)
        self.assertIn("0x45", self.text)
        self.assertIn("Assert-X64Pe", self.text)
        self.assertIn("Assert-MzExecutable", self.text)
        self.assertIn("Test-Path -LiteralPath $outputDirFull -PathType Container", self.text)
        self.assertIn("New-Item -ItemType Directory", self.text)
        self.assertIn("Resolve-ExistingFile -Path $licensePath", self.text)

    def test_version_pattern_is_shared_with_static_plan(self) -> None:
        match = re.search(r"(?m)^\s*\$versionPattern = '([^']+)'\s*$", self.text)
        self.assertIsNotNone(match, "build.ps1 must expose one literal version pattern")
        pattern = re.compile(match.group(1))
        for version in (
            "0.1.1",
            "1.2.3",
            "0.0.0",
            "10.20.30-preview.1",
            "0.1.1-preview-1",
        ):
            self.assertIsNotNone(pattern.fullmatch(version), version)
        for version in (
            "",
            "0.1",
            "0.1.1.0",
            "01.2.3",
            "0.1.1+build",
            "0.1.1-preview..1",
            "0.1.1-",
            "v0.1.1",
            "0.1.1/../../x",
            "0.1.1 ",
            "0.1.1\n",
        ):
            self.assertIsNone(pattern.fullmatch(version), repr(version))

        plan = expected_plan("0.1.1-preview.1", Path("paint-ui.exe"), Path("dist"))
        self.assertEqual(plan["version_numeric"], "0.1.1.0")
        self.assertTrue(
            str(plan["setup"]).endswith(
                "Paint.Rust-0.1.1-preview.1-windows-x86_64-setup.exe"
            )
        )
        self.assertEqual(plan["sidecar"], f"{plan['setup']}.sha256")
        self.assertTrue(plan["unsigned"])

    def test_static_build_plan_templates_format_expected_artifacts(self) -> None:
        setup_match = re.search(
            r"(?m)^\s*\$setupName = '([^']+)' -f \$versionInfo\.Text\s*$",
            self.text,
        )
        self.assertIsNotNone(setup_match)
        self.assertEqual(
            setup_match.group(1).format("0.1.1-preview.1"),
            "Paint.Rust-0.1.1-preview.1-windows-x86_64-setup.exe",
        )

        sidecar_match = re.search(
            r"(?m)^\s*\$sidecarPath = '([^']+)' -f \$setupPath\s*$",
            self.text,
        )
        self.assertIsNotNone(sidecar_match)
        sample = "C:\\out\\Paint.Rust-0.1.1-windows-x86_64-setup.exe"
        self.assertEqual(sidecar_match.group(1).format(sample), f"{sample}.sha256")

        numeric_match = re.search(
            r"(?m)^\s*Numeric = \('([^']+)' -f \$major, \$minor, \$patch\)\s*$",
            self.text,
        )
        self.assertIsNotNone(numeric_match)
        self.assertEqual(numeric_match.group(1).format(0, 1, 1), "0.1.1.0")

        self.assertIn('VIProductVersion "${VI_VERSION}"', read_utf8(NSIS_SCRIPT))
        self.assertIn(
            "($optionPrefix + 'DVI_VERSION=' + $versionInfo.Numeric)",
            self.text,
        )

    def test_plan_only_runs_before_side_effects(self) -> None:
        plan_index = self.text.index("if ($PlanOnly)")
        self.assertLess(plan_index, self.text.index("New-Item -ItemType Directory"))
        self.assertLess(plan_index, self.text.index("Remove-Item -LiteralPath $stalePath"))
        self.assertLess(plan_index, self.text.index("& $makensis @makensisArguments"))
        self.assertIn("$plan | ConvertTo-Json", self.text)
        self.assertIn("plan_only = $true", self.text)

    def test_normal_build_removes_stale_outputs_and_checks_exit(self) -> None:
        remove_index = self.text.index("Remove-Item -LiteralPath $stalePath")
        invoke_index = self.text.index("& $makensis @makensisArguments")
        self.assertLess(remove_index, invoke_index)
        self.assertIn("$makensisExit = $LASTEXITCODE", self.text)
        self.assertIn('if ($makensisExit -ne 0)', self.text)
        self.assertIn("did not create the setup executable", self.text)
        self.assertIn("Setup executable is empty", self.text)
        self.assertIn("SHA-256 sidecar did not round-trip exactly", self.text)

    def test_no_signing_logic(self) -> None:
        for forbidden in (
            "Set-AuthenticodeSignature",
            "signtool",
            "SignTool",
            "osslsigncode",
            "AzureSignTool",
        ):
            self.assertNotIn(forbidden, self.text)
        self.assertIn("unsigned", self.text.lower())
        self.assertIn("not code-signed", self.text)


class DocumentationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.readme = read_utf8(README)
        cls.install_text = read_utf8(INSTALL_TEXT)

    def test_readme_documents_exact_ci_interface_and_artifacts(self) -> None:
        self.assertIn(CI_INTERFACE, self.readme)
        self.assertIn("Paint.Rust-<version>-windows-x86_64-setup.exe", self.readme)
        self.assertIn(".sha256", self.readme)
        self.assertIn("python3 -m unittest discover -s tools/tests", self.readme)
        self.assertIn("-PlanOnly", self.readme)
        self.assertIn("-MakensisPath", self.readme)

    def test_readme_states_unsigned_and_unverified_limits(self) -> None:
        lower = " ".join(self.readme.lower().split())
        self.assertIn("unsigned", lower)
        self.assertIn("not been compiled or tested on a windows host", lower)
        self.assertIn("no setup executable", lower)
        self.assertIn("windows verification still required", lower)
        self.assertNotIn("digitally signed", lower)
        self.assertNotIn("code-signed by", lower)

    def test_install_text_is_embedded_and_honest(self) -> None:
        lower = self.install_text.lower()
        self.assertIn("unsigned build", lower)
        self.assertIn("no authenticode", lower)
        self.assertIn("c:\\program files\\paint.rust", lower)
        self.assertIn("settings > apps", lower)
        self.assertIn("documents", lower)
        self.assertIn("uninstall", lower)
        self.assertNotIn("digitally signed", lower)

    def test_sources_are_ascii_and_lf_for_windows_powershell_compatibility(self) -> None:
        for path in (NSIS_SCRIPT, BUILD_SCRIPT, INSTALL_TEXT, README):
            data = path.read_bytes()
            data.decode("ascii")
            self.assertNotIn(b"\r\n", data, f"{path.name} must use LF line endings")

    def test_installer_directory_has_no_binary_or_asset_payloads(self) -> None:
        names = {
            path.name
            for path in INSTALLER_DIR.iterdir()
            if path.is_file() and not path.name.startswith(".")
        }
        self.assertEqual(
            names,
            {"build.ps1", "paint-rust.nsi", "INSTALL-WINDOWS.txt", "README.md"},
        )


@unittest.skipUnless(
    PWSH is not None,
    "pwsh is not installed; PowerShell plan and argument-validation tests skipped",
)
class PowerShellPlanTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "paint-ui.exe"
        self.binary.write_bytes(fake_pe())
        self.output = self.root / "output"

    def run_build(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        command = [
            PWSH or "pwsh",
            "-NoProfile",
            "-NonInteractive",
            "-File",
            str(BUILD_SCRIPT),
            *arguments,
        ]
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )

    def valid_arguments(self) -> list[str]:
        return [
            "-Version",
            "0.1.1-preview.1",
            "-BinaryPath",
            str(self.binary),
            "-OutputDir",
            str(self.output),
            "-PlanOnly",
        ]

    def combined_output(self, result: subprocess.CompletedProcess[str]) -> str:
        return f"{result.stdout}\n{result.stderr}"

    def test_plan_only_emits_resolved_json_without_writing_outputs(self) -> None:
        result = self.run_build(*self.valid_arguments())
        self.assertEqual(result.returncode, 0, self.combined_output(result))
        plan = json.loads(result.stdout)
        self.assertEqual(plan["schema"], "paint-rust/windows-installer-plan@1")
        self.assertEqual(plan["version"], "0.1.1-preview.1")
        self.assertEqual(plan["version_numeric"], "0.1.1.0")
        self.assertTrue(plan["plan_only"])
        self.assertTrue(plan["unsigned"])
        expected_setup = (
            self.output / "Paint.Rust-0.1.1-preview.1-windows-x86_64-setup.exe"
        )
        self.assertEqual(Path(plan["setup"]).name, expected_setup.name)
        self.assertEqual(
            Path(plan["setup"]).parent.resolve(),
            expected_setup.parent.resolve(),
        )
        self.assertEqual(Path(plan["output_dir"]).resolve(), self.output.resolve())
        self.assertEqual(Path(plan["sidecar"]), Path(plan["setup"] + ".sha256"))
        self.assertEqual(
            plan["binary_sha256"],
            hashlib.sha256(self.binary.read_bytes()).hexdigest(),
        )
        self.assertIsInstance(plan["makensis_found"], bool)
        self.assertEqual(plan["makensis_found"], plan["makensis"] is not None)
        self.assertFalse(self.output.exists())
        self.assertFalse(Path(plan["sidecar"]).exists())

    def test_plan_only_rejects_invalid_version(self) -> None:
        result = self.run_build(
            "-Version",
            "0.1",
            "-BinaryPath",
            str(self.binary),
            "-OutputDir",
            str(self.output),
            "-PlanOnly",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Version", self.combined_output(result))
        self.assertFalse(self.output.exists())

    def test_plan_only_rejects_missing_binary(self) -> None:
        result = self.run_build(
            "-Version",
            "0.1.1",
            "-BinaryPath",
            str(self.root / "missing-paint-ui.exe"),
            "-OutputDir",
            str(self.output),
            "-PlanOnly",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("BinaryPath", self.combined_output(result))
        self.assertFalse(self.output.exists())

    def test_plan_only_rejects_wrong_binary_name(self) -> None:
        other = self.root / "not-paint-ui.exe"
        other.write_bytes(fake_pe())
        result = self.run_build(
            "-Version",
            "0.1.1",
            "-BinaryPath",
            str(other),
            "-OutputDir",
            str(self.output),
            "-PlanOnly",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("paint-ui.exe", self.combined_output(result))
        self.assertFalse(self.output.exists())

    def test_plan_only_rejects_non_x64_pe(self) -> None:
        arm64_dir = self.root / "arm64"
        arm64_dir.mkdir()
        arm64 = arm64_dir / "paint-ui.exe"
        arm64.write_bytes(fake_pe(machine=0xAA64))
        result = self.run_build(
            "-Version",
            "0.1.1",
            "-BinaryPath",
            str(arm64),
            "-OutputDir",
            str(self.output),
            "-PlanOnly",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("0x8664", self.combined_output(result))
        self.assertFalse(self.output.exists())

    def test_plan_only_rejects_output_dir_that_is_a_file(self) -> None:
        existing = self.root / "output-file"
        existing.write_text("not a directory", encoding="utf-8")
        result = self.run_build(
            "-Version",
            "0.1.1",
            "-BinaryPath",
            str(self.binary),
            "-OutputDir",
            str(existing),
            "-PlanOnly",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("OutputDir", self.combined_output(result))

    @unittest.skipIf(
        MAKENSIS is not None,
        "makensis is installed; tests never compile an installer",
    )
    def test_normal_build_fails_before_outputs_when_makensis_is_missing(self) -> None:
        result = self.run_build(
            "-Version",
            "0.1.1",
            "-BinaryPath",
            str(self.binary),
            "-OutputDir",
            str(self.output),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("makensis", self.combined_output(result).lower())
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
