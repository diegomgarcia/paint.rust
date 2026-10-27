"""Unit tests for preview packaging using disposable, non-product fixtures."""

from __future__ import annotations

import copy
import gzip
import hashlib
import json
import os
import plistlib
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import preview_package as package


EPOCH = 946_684_800  # 2000-01-01T00:00:00Z


def elf_header(machine: int = 62) -> bytes:
    data = bytearray(64)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<H", data, 18, machine)
    return bytes(data)


def macho_header(cpu_type: int = 0x0100000C) -> bytes:
    return struct.pack("<IIIIIIII", 0xFEEDFACF, cpu_type, 0, 2, 0, 0, 0, 0)


def pe_header(machine: int = 0x8664) -> bytes:
    data = bytearray(0x100)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", data, 0x84, machine)
    return bytes(data)


class DisposableFixtureMixin(unittest.TestCase):
    """Create clearly disposable payloads; none is evidence of a product smoke."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="paint-r-packaging-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def payload(self, parent: str, *, reversed_order: bool = False) -> Path:
        root = self.root / parent / "paint-r-test"
        root.mkdir(parents=True)
        files = [
            ("docs/README.txt", b"disposable packaging fixture\n"),
            ("paint-ui", b"not a product binary; archive fixture only\n"),
            ("docs/nested/parity.txt", b"unverified\n"),
        ]
        if reversed_order:
            files.reverse()
        for index, (relative, content) in enumerate(files):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
            path.chmod(0o755 if relative == "paint-ui" else 0o644)
            os.utime(path, (1_700_000_000 + index, 1_700_000_000 + index))
        return root


class ArchiveDeterminismTests(DisposableFixtureMixin):
    def test_zip_is_byte_deterministic_and_normalizes_metadata(self) -> None:
        first = self.payload("first")
        second = self.payload("second", reversed_order=True)
        first_archive = self.root / "first.zip"
        second_archive = self.root / "second.zip"

        package.create_zip_archive(first, first_archive, EPOCH)
        package.create_zip_archive(second, second_archive, EPOCH)

        self.assertEqual(first_archive.read_bytes(), second_archive.read_bytes())
        with zipfile.ZipFile(first_archive) as archive:
            names = archive.namelist()
            self.assertEqual(names, sorted(names))
            self.assertEqual(
                names,
                [
                    "paint-r-test/docs/README.txt",
                    "paint-r-test/docs/nested/parity.txt",
                    "paint-r-test/paint-ui",
                ],
            )
            for info in archive.infolist():
                self.assertEqual(info.date_time, (2000, 1, 1, 0, 0, 0))
                self.assertEqual(info.create_system, 3)
                self.assertEqual(info.extra, b"")
            self.assertEqual(archive.getinfo("paint-r-test/paint-ui").external_attr >> 16, 0o100755)
            self.assertEqual(archive.getinfo("paint-r-test/docs/README.txt").external_attr >> 16, 0o100644)

    def test_tar_gzip_is_byte_deterministic_and_normalizes_metadata(self) -> None:
        first = self.payload("first")
        second = self.payload("second", reversed_order=True)
        first_archive = self.root / "first.tar.gz"
        second_archive = self.root / "second.tar.gz"

        package.create_tar_gz_archive(first, first_archive, EPOCH)
        package.create_tar_gz_archive(second, second_archive, EPOCH)

        self.assertEqual(first_archive.read_bytes(), second_archive.read_bytes())
        self.assertEqual(first_archive.read_bytes()[4:8], EPOCH.to_bytes(4, "little"))
        with gzip.open(first_archive, "rb") as compressed:
            self.assertTrue(compressed.read())
        with tarfile.open(first_archive, "r:gz") as archive:
            members = archive.getmembers()
            names = [member.name for member in members]
            self.assertEqual(names, sorted(names))
            for member in members:
                self.assertEqual(member.mtime, EPOCH)
                self.assertEqual((member.uid, member.gid), (0, 0))
                self.assertEqual((member.uname, member.gname), ("", ""))
            self.assertEqual(archive.getmember("paint-r-test/paint-ui").mode, 0o755)
            self.assertEqual(archive.getmember("paint-r-test/docs/README.txt").mode, 0o644)

    def test_sha256_manifest_is_sorted_exact_and_excludes_itself(self) -> None:
        root = self.payload("manifest")
        manifest = package.write_sha256_manifest(root)
        files = sorted(
            path for path in root.rglob("*") if path.is_file() and path.name != "SHA256SUMS.txt"
        )
        expected = "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.relative_to(root).as_posix()}\n"
            for path in files
        )
        self.assertEqual(manifest.read_text(encoding="utf-8"), expected)
        self.assertNotIn("SHA256SUMS.txt  SHA256SUMS.txt", expected)
        first = manifest.read_bytes()
        package.write_sha256_manifest(root)
        self.assertEqual(manifest.read_bytes(), first)

    def test_archive_sidecar_contains_only_basename_and_digest(self) -> None:
        archive = self.root / "preview.zip"
        archive.write_bytes(b"archive fixture")
        sidecar = package.write_archive_sidecar(archive)
        self.assertEqual(
            sidecar.read_text(encoding="utf-8"),
            f"{hashlib.sha256(b'archive fixture').hexdigest()}  preview.zip\n",
        )


class MetadataAndNoticeTests(DisposableFixtureMixin):
    def metadata(self) -> dict[str, object]:
        first = "path+file:///private/should-not-leak#paint-ui@0.1.0"
        second = "registry+https://github.com/rust-lang/crates.io-index#example@1.2.3"
        return {
            "packages": [
                {
                    "id": first,
                    "name": "paint-ui",
                    "version": "0.1.0",
                    "license": None,
                    "license_file": None,
                    "source": None,
                    "manifest_path": str(self.root / "crates/ui/Cargo.toml"),
                },
                {
                    "id": second,
                    "name": "example",
                    "version": "1.2.3",
                    "license": "MIT OR Apache-2.0",
                    "license_file": None,
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                    "manifest_path": str(self.root / "registry/example/Cargo.toml"),
                },
            ],
            "workspace_members": [first],
            "resolve": {
                "nodes": [
                    {"id": first, "deps": [{"pkg": second}]},
                    {"id": second, "deps": []},
                ]
            },
        }

    def test_sbom_uses_locked_graph_without_leaking_local_paths(self) -> None:
        metadata = self.metadata()
        first = package.generate_sbom(metadata, EPOCH)
        relocated = json.loads(json.dumps(metadata).replace("/private/should-not-leak", "/different/checkout"))
        second = package.generate_sbom(relocated, EPOCH)
        self.assertEqual(first, second)
        self.assertEqual(first["bomFormat"], "CycloneDX")
        self.assertEqual(first["specVersion"], "1.6")
        self.assertEqual(first["metadata"]["timestamp"], "2000-01-01T00:00:00Z")
        self.assertEqual(first["metadata"]["component"]["name"], "paint-ui")
        self.assertEqual(len(first["components"]), 2)
        example = next(component for component in first["components"] if component["name"] == "example")
        self.assertEqual(example["licenses"], [{"expression": "MIT OR Apache-2.0"}])
        root_ref = first["metadata"]["component"]["bom-ref"]
        dependencies = {entry["ref"]: entry["dependsOn"] for entry in first["dependencies"]}
        self.assertEqual(dependencies[root_ref], ["pkg:cargo/example@1.2.3"])
        self.assertNotIn("/private/should-not-leak", json.dumps(first))

    def test_notice_collection_copies_font_texts_and_reports_gaps(self) -> None:
        metadata = self.metadata()
        external = metadata["packages"][1]
        external.update(name="epaint_default_fonts", version="0.37.0", license="OFL-1.1 AND Ubuntu-font-1.0")
        font_root = self.root / "registry/example"
        fonts = font_root / "fonts"
        fonts.mkdir(parents=True)
        (fonts / "OFL.txt").write_text("disposable OFL text\n", encoding="utf-8")
        (fonts / "UFL.txt").write_text("disposable Ubuntu font text\n", encoding="utf-8")
        (fonts / "Hack-Regular.txt").write_text("disposable Hack notice\n", encoding="utf-8")
        missing = dict(external)
        missing.update(id="registry+example#missing@1.0.0", name="missing-notice", version="1.0.0")
        missing["manifest_path"] = str(self.root / "missing/Cargo.toml")
        metadata["packages"].append(missing)
        payload = self.root / "payload"
        payload.mkdir()

        index = package.collect_third_party_notices(metadata, payload)

        self.assertFalse(index["extraction_complete"])
        self.assertEqual(index["verification"], "heuristic-extraction-only-not-legal-review")
        discovered = next(entry for entry in index["packages"] if entry["name"] == "epaint_default_fonts")
        self.assertEqual(len(discovered["files"]), 3)
        for record in discovered["files"]:
            self.assertTrue((payload / record["path"]).is_file())
        absent = next(entry for entry in index["packages"] if entry["name"] == "missing-notice")
        self.assertEqual(absent["files"], [])
        self.assertIn("not a legal review", (payload / "THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8"))
        self.assertNotIn(str(self.root), json.dumps(index))


class BinaryArchitectureTests(DisposableFixtureMixin):
    def test_exact_pe_macho_and_elf_architectures_are_identified(self) -> None:
        cases = (
            ("windows-x86_64", "paint-ui.exe", pe_header(), "PE", "0x8664", "x86_64"),
            ("macos-aarch64", "paint-ui", macho_header(), "Mach-O 64", "0x100000c", "aarch64"),
            ("linux-x86_64", "paint-ui", elf_header(), "ELF 64", "0x003e", "x86_64"),
        )
        for platform, name, content, format_name, machine, architecture in cases:
            with self.subTest(platform=platform):
                binary = self.root / platform / name
                binary.parent.mkdir()
                binary.write_bytes(content)
                evidence = package.inspect_binary_architecture(binary, platform)
                self.assertEqual(evidence["format"], format_name)
                self.assertEqual(evidence["machine"], machine)
                self.assertEqual(evidence["architecture"], architecture)
                self.assertEqual(evidence["bytes"], len(content))
                self.assertEqual(evidence["sha256"], hashlib.sha256(content).hexdigest())

    def test_wrong_architecture_is_rejected_with_expected_and_actual_machines(self) -> None:
        cases = (
            ("windows-x86_64", "paint-ui.exe", pe_header(0xAA64)),
            ("macos-aarch64", "paint-ui", macho_header(0x01000007)),
            ("linux-x86_64", "paint-ui", elf_header(183)),
        )
        for platform, name, content in cases:
            with self.subTest(platform=platform):
                binary = self.root / f"wrong-{platform}" / name
                binary.parent.mkdir()
                binary.write_bytes(content)
                with self.assertRaisesRegex(package.PackagingError, "architecture mismatch.*expected.*got"):
                    package.inspect_binary_architecture(binary, platform)

    def test_invalid_or_truncated_headers_are_typed_errors(self) -> None:
        for platform, name in (("windows-x86_64", "paint-ui.exe"), ("macos-aarch64", "paint-ui"), ("linux-x86_64", "paint-ui")):
            with self.subTest(platform=platform):
                binary = self.root / f"invalid-{platform}" / name
                binary.parent.mkdir()
                binary.write_bytes(b"not an executable")
                with self.assertRaisesRegex(package.PackagingError, platform):
                    package.inspect_binary_architecture(binary, platform)

    def test_package_rejects_wrong_architecture_before_smoke_or_metadata(self) -> None:
        binary = self.root / "paint-ui"
        binary.write_bytes(elf_header(183))
        with (
            mock.patch.object(package, "run_preview_smoke") as smoke,
            mock.patch.object(package, "load_locked_metadata") as metadata,
        ):
            with self.assertRaisesRegex(package.PackagingError, "architecture mismatch"):
                package.package_release(
                    repo_root=self.root,
                    binary=binary,
                    platform="linux-x86_64",
                    output_dir=self.root / "dist",
                    traces=[],
                    repeat=1,
                    timeout=1.0,
                    cargo_command=["must-not-run"],
                )
        smoke.assert_not_called()
        metadata.assert_not_called()
        self.assertFalse((self.root / "dist").exists())


class PlatformLayoutTests(DisposableFixtureMixin):
    def test_macos_layout_is_an_app_zip_payload(self) -> None:
        binary = self.root / "paint-ui"
        binary.write_bytes(b"disposable layout fixture")
        payload = self.root / "payload"
        package.stage_platform_files("macos-aarch64", "0.1.0", binary, payload)
        executable = payload / "Paint.R.app/Contents/MacOS/paint-ui"
        self.assertEqual(executable.read_bytes(), binary.read_bytes())
        self.assertTrue(executable.stat().st_mode & 0o111)
        info = plistlib.loads((payload / "Paint.R.app/Contents/Info.plist").read_bytes())
        self.assertEqual(info["CFBundleExecutable"], "paint-ui")
        self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
        self.assertNotIn("sign", json.dumps(info).lower())

    def test_windows_and_linux_layout_names(self) -> None:
        for platform, binary_name in (("windows-x86_64", "paint-ui.exe"), ("linux-x86_64", "paint-ui")):
            with self.subTest(platform=platform):
                binary = self.root / platform / binary_name
                binary.parent.mkdir()
                binary.write_bytes(b"disposable layout fixture")
                payload = self.root / f"{platform}-payload"
                package.stage_platform_files(platform, "0.1.0", binary, payload)
                self.assertEqual((payload / binary_name).read_bytes(), binary.read_bytes())


class SmokeEvidenceValidationTests(DisposableFixtureMixin):
    def setUp(self) -> None:
        super().setUp()
        self.repo = self.root / "repo"
        self.trace = self.repo / "tests/e2e/traces/invert.json"
        self.trace.parent.mkdir(parents=True)
        self.trace.write_text("{}\n", encoding="utf-8")
        self.binary = self.repo / "paint-ui"
        self.binary.write_bytes(b"disposable smoke evidence binary\n")
        self.run = {
            "width": 800,
            "height": 600,
            "decoded_rgba_sha256": "a" * 64,
            "png_sha256": "b" * 64,
            "snapshot_sha256": "c" * 64,
            "snapshot": {
                "layer_count": 1,
                "active_layer_index": 0,
                "dirty": True,
                "can_undo": True,
                "can_redo": False,
            },
        }

    def report(self) -> dict[str, object]:
        return {
            "status": "pass",
            "traces": [
                {
                    "trace": str(self.trace),
                    "status": "pass",
                    "runs": [copy.deepcopy(self.run), copy.deepcopy(self.run)],
                    "unknown_command_error": "expected unknown-command rejection",
                    "invalid_rect_error": "expected invalid-rectangle rejection",
                }
            ],
        }

    def normalize(self, report: dict[str, object]) -> dict[str, object]:
        return package.normalize_smoke_report(report, [self.trace], self.binary, self.repo, 2)

    def test_fully_populated_repeat_evidence_is_preserved(self) -> None:
        report = self.report()
        normalized = self.normalize(report)
        self.assertEqual(normalized["status"], "pass")
        self.assertEqual(normalized["repeat"], 2)
        self.assertEqual(normalized["binary_sha256"], hashlib.sha256(self.binary.read_bytes()).hexdigest())
        self.assertEqual(normalized["traces"][0]["trace"], "tests/e2e/traces/invert.json")
        self.assertEqual(normalized["traces"][0]["runs"], report["traces"][0]["runs"])

    def test_empty_missing_invalid_or_extra_run_evidence_is_rejected(self) -> None:
        missing_hash = copy.deepcopy(self.run)
        del missing_hash["snapshot_sha256"]
        empty_hash = copy.deepcopy(self.run)
        empty_hash["decoded_rgba_sha256"] = ""
        non_hex_hash = copy.deepcopy(self.run)
        non_hex_hash["png_sha256"] = "A" * 64
        invalid_snapshot = copy.deepcopy(self.run)
        del invalid_snapshot["snapshot"]["dirty"]
        invalid_dimensions = copy.deepcopy(self.run)
        invalid_dimensions["width"] = True
        extra_field = copy.deepcopy(self.run)
        extra_field["absolute_path"] = "/must-not-be-retained"
        cases = (
            ("empty", {}),
            ("missing hash", missing_hash),
            ("empty hash", empty_hash),
            ("non-hex hash", non_hex_hash),
            ("invalid snapshot", invalid_snapshot),
            ("invalid dimensions", invalid_dimensions),
            ("extra field", extra_field),
        )
        for name, run in cases:
            with self.subTest(case=name):
                report = self.report()
                report["traces"][0]["runs"] = [run, copy.deepcopy(run)]
                with self.assertRaises(package.PackagingError):
                    self.normalize(report)

    def test_differing_repeat_entries_are_rejected(self) -> None:
        report = self.report()
        report["traces"][0]["runs"][1]["png_sha256"] = "d" * 64
        with self.assertRaisesRegex(package.PackagingError, "not deterministic"):
            self.normalize(report)


class PackageReleaseIntegrationTests(DisposableFixtureMixin):
    """Exercise packaging end to end with disposable external-command doubles.

    The archive and manifest logic is real; this is not a product smoke or native-binary claim.
    """

    def test_package_release_is_deterministic_and_embeds_binary_evidence(self) -> None:
        repo = self.root / "repo"
        (repo / "packaging").mkdir(parents=True)
        (repo / "docs").mkdir()
        (repo / "packaging/INTERNAL_PREVIEW.md").write_text(
            "Internal disposable preview {{VERSION}} {{PLATFORM}}\n", encoding="utf-8"
        )
        (repo / "docs/PARITY_LEDGER.md").write_text("Disposable unverified ledger\n", encoding="utf-8")
        trace = repo / "trace.json"
        trace.write_text("{}\n", encoding="utf-8")
        binary = repo / "target/release/paint-ui"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(elf_header())
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        package_id = "path+file:///disposable#paint-ui@0.1.0"
        metadata = {
            "packages": [
                {
                    "id": package_id,
                    "name": "paint-ui",
                    "version": "0.1.0",
                    "license": None,
                    "license_file": None,
                    "source": None,
                    "manifest_path": str(repo / "crates/ui/Cargo.toml"),
                }
            ],
            "workspace_members": [package_id],
            "resolve": {"nodes": [{"id": package_id, "deps": []}]},
        }
        smoke_evidence = {
            "format": "paint.r.preview-smoke-evidence",
            "version": 1,
            "status": "pass",
            "binary_sha256": digest,
            "repeat": 1,
            "traces": [],
        }

        def invoke(output: Path) -> package.PackageResult:
            return package.package_release(
                repo_root=repo,
                binary=binary,
                platform="linux-x86_64",
                output_dir=output,
                traces=[trace],
                repeat=1,
                timeout=1.0,
                cargo_command=["must-not-run"],
            )

        with (
            mock.patch.dict(os.environ, {"SOURCE_DATE_EPOCH": str(EPOCH)}),
            mock.patch.object(package, "run_preview_smoke", return_value=smoke_evidence) as smoke,
            mock.patch.object(package, "load_locked_metadata", return_value=metadata) as load_metadata,
        ):
            first = invoke(self.root / "dist/first")
            second = invoke(self.root / "dist/second")
        self.assertEqual(smoke.call_count, 2)
        self.assertEqual(load_metadata.call_count, 2)
        self.assertEqual(first.archive.read_bytes(), second.archive.read_bytes())
        self.assertEqual(first.sidecar.read_bytes(), second.sidecar.read_bytes())
        self.assertEqual(first.archive.name, "paint-r-0.1.0-linux-x86_64.tar.gz")

        with tarfile.open(first.archive, "r:gz") as archive:
            files = {
                member.name.split("/", 1)[1]: archive.extractfile(member).read()
                for member in archive.getmembers()
                if member.isfile()
            }
        binary_evidence = json.loads(files["PREVIEW_BINARY.json"])
        self.assertEqual(binary_evidence["format"], "paint.r.preview-binary-evidence")
        self.assertEqual(binary_evidence["version"], 1)
        self.assertEqual(binary_evidence["platform"], "linux-x86_64")
        self.assertEqual(binary_evidence["binary"]["format"], "ELF 64")
        self.assertEqual(binary_evidence["binary"]["machine"], "0x003e")
        self.assertEqual(binary_evidence["binary"]["architecture"], "x86_64")
        self.assertEqual(binary_evidence["binary"]["sha256"], digest)
        self.assertEqual(binary_evidence["binary"]["scope"], "executable-header-only-not-native-execution-proof")
        manifest = files["SHA256SUMS.txt"].decode("utf-8")
        expected = "".join(
            f"{hashlib.sha256(files[path]).hexdigest()}  {path}\n"
            for path in sorted(files)
            if path != "SHA256SUMS.txt"
        )
        self.assertEqual(manifest, expected)


class MandatorySmokeFailureTests(DisposableFixtureMixin):
    def binary(self, name: str = "paint-ui") -> Path:
        binary = self.root / name
        binary.write_bytes(b"disposable failure fixture, not product evidence")
        return binary

    def test_missing_smoke_script_fails(self) -> None:
        binary = self.binary()
        trace = self.root / "trace.json"
        trace.write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(package.PackagingError, "smoke runner is missing"):
            package.run_preview_smoke(
                binary=binary,
                traces=[trace],
                smoke_script=self.root / "absent.py",
                repeat=1,
                timeout=1.0,
                repo_root=self.root,
            )

    def test_nonzero_smoke_fails_and_is_not_replaced_by_a_report(self) -> None:
        binary = self.binary()
        trace = self.root / "trace.json"
        trace.write_text("{}", encoding="utf-8")
        runner = self.root / "preview_smoke.py"
        runner.write_text("# disposable test double; never used as a passing product smoke\n", encoding="utf-8")
        completed = subprocess.CompletedProcess(args=[], returncode=7, stdout='{"status":"fail"}', stderr="expected failure")
        with mock.patch.object(package.subprocess, "run", return_value=completed) as run:
            with self.assertRaisesRegex(package.PackagingError, "exit code 7"):
                package.run_preview_smoke(
                    binary=binary,
                    traces=[trace],
                    smoke_script=runner,
                    repeat=1,
                    timeout=1.0,
                    repo_root=self.root,
                )
        self.assertIn("--report", run.call_args.args[0])

    def test_zero_exit_without_smoke_report_fails(self) -> None:
        binary = self.binary()
        trace = self.root / "trace.json"
        trace.write_text("{}", encoding="utf-8")
        runner = self.root / "preview_smoke.py"
        runner.write_text("# disposable test double for missing-report handling\n", encoding="utf-8")
        completed = subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr="")
        with mock.patch.object(package.subprocess, "run", return_value=completed):
            with self.assertRaisesRegex(package.PackagingError, "without writing its report"):
                package.run_preview_smoke(
                    binary=binary,
                    traces=[trace],
                    smoke_script=runner,
                    repeat=1,
                    timeout=1.0,
                    repo_root=self.root,
                )

    def test_failed_smoke_prevents_any_archive(self) -> None:
        repo = self.root / "repo"
        binary = repo / "target/release/paint-ui"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(elf_header())
        output = self.root / "dist"
        with mock.patch.object(package, "run_preview_smoke", side_effect=package.PackagingError("smoke failed")):
            with self.assertRaisesRegex(package.PackagingError, "smoke failed"):
                package.package_release(
                    repo_root=repo,
                    binary=binary,
                    platform="linux-x86_64",
                    output_dir=output,
                    traces=[],
                    repeat=1,
                    timeout=1.0,
                    cargo_command=["must-not-run"],
                )
        self.assertFalse(output.exists())

    def test_missing_binary_fails_before_smoke(self) -> None:
        with mock.patch.object(package, "run_preview_smoke") as smoke:
            with self.assertRaisesRegex(package.PackagingError, "binary is missing"):
                package.package_release(
                    repo_root=self.root,
                    binary=self.root / "paint-ui",
                    platform="linux-x86_64",
                    output_dir=self.root / "dist",
                    traces=[],
                    repeat=1,
                    timeout=1.0,
                    cargo_command=["must-not-run"],
                )
        smoke.assert_not_called()

    def test_wrong_binary_name_is_rejected(self) -> None:
        binary = self.binary("placeholder")
        with self.assertRaisesRegex(package.PackagingError, "expected paint-ui"):
            package.package_release(
                repo_root=self.root,
                binary=binary,
                platform="windows-x86_64",
                output_dir=self.root / "dist",
                traces=[],
                repeat=1,
                timeout=1.0,
                cargo_command=["must-not-run"],
            )

    def test_public_packaging_is_refused_before_smoke(self) -> None:
        with mock.patch.object(package, "run_preview_smoke") as smoke:
            with self.assertRaisesRegex(package.PackagingError, "public redistribution is blocked"):
                package.package_release(
                    repo_root=self.root,
                    binary=self.root / "paint-ui",
                    platform="linux-x86_64",
                    output_dir=self.root / "dist",
                    traces=[],
                    repeat=1,
                    timeout=1.0,
                    cargo_command=["must-not-run"],
                    distribution="public",
                )
        smoke.assert_not_called()


if __name__ == "__main__":
    unittest.main()
