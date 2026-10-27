"""Unit tests for preview packaging using disposable, non-product fixtures."""

from __future__ import annotations

import gzip
import hashlib
import json
import os
import plistlib
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
        binary.write_bytes(b"disposable failure fixture")
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
