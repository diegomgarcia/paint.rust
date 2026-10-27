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

    def test_post_archive_reader_rejects_missing_extra_and_unsafe_entries(self) -> None:
        payload = self.payload("entry-validation")
        valid = [
            "paint-r-test/docs/README.txt",
            "paint-r-test/docs/nested/parity.txt",
            "paint-r-test/paint-ui",
        ]

        def write_entries(path: Path, names: list[str]) -> None:
            with zipfile.ZipFile(path, "w") as archive:
                for name in names:
                    info = zipfile.ZipInfo(name)
                    info.create_system = 3
                    info.external_attr = 0o100644 << 16
                    archive.writestr(info, b"disposable archive entry\n")

        cases = (
            ("missing", valid[:-1], "entries differ"),
            ("extra", [*valid, "paint-r-test/extra.txt"], "entries differ"),
            ("traversal", [*valid, "paint-r-test/../escape.txt"], "unsafe archive entry"),
            ("absolute", [*valid, "/tmp/escape.txt"], "unsafe archive entry"),
            ("backslash", [*valid, "paint-r-test\\..\\escape.txt"], "unsafe archive entry"),
        )
        for name, entries, diagnostic in cases:
            with self.subTest(case=name):
                archive = self.root / f"{name}.zip"
                destination = self.root / f"extract-{name}"
                destination.mkdir()
                write_entries(archive, entries)
                with self.assertRaisesRegex(package.PackagingError, diagnostic):
                    package._extract_verified_archive(archive, "macos-aarch64", payload, destination, EPOCH)
        self.assertFalse((self.root / "escape.txt").exists())

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
        self.assertEqual(index["verification"], "heuristic-and-curated-upstream-extraction-not-legal-review")
        self.assertEqual(index["packages_with_curated_supplements"], 0)
        discovered = next(entry for entry in index["packages"] if entry["name"] == "epaint_default_fonts")
        self.assertEqual(len(discovered["files"]), 3)
        for record in discovered["files"]:
            self.assertTrue((payload / record["path"]).is_file())
        absent = next(entry for entry in index["packages"] if entry["name"] == "missing-notice")
        self.assertEqual(absent["files"], [])
        self.assertIn("not a legal review", (payload / "THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8"))
        self.assertNotIn(str(self.root), json.dumps(index))


class NoticeProvenanceSupplementTests(DisposableFixtureMixin):
    """Exercise curated upstream notices without network or real license fixtures."""

    COMMIT = "a" * 40
    CHECKSUM = "b" * 64
    SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
    REPOSITORY = "https://github.com/example/project"

    def setUp(self) -> None:
        super().setUp()
        self.crate = self.root / "registry/example"
        self.crate.mkdir(parents=True)
        (self.crate / "Cargo.toml").write_text("# disposable package fixture\n", encoding="utf-8")
        self.write_vcs(self.COMMIT, "crates/example")
        self.missing = self.root / "registry/missing"
        self.missing.mkdir(parents=True)
        self.supplement = self.root / "supplement"
        (self.supplement / "files").mkdir(parents=True)
        self.notice = b"disposable curated upstream notice fixture; not a license\n"
        (self.supplement / "files/NOTICE").write_bytes(self.notice)
        self.manifest = {
            "format": "paint.r.notice-provenance-supplement",
            "version": 1,
            "files": [
                {
                    "id": "example-notice",
                    "path": "files/NOTICE",
                    "source_path": "NOTICE",
                    "source_url": f"https://raw.githubusercontent.com/example/project/{self.COMMIT}/NOTICE",
                    "sha256": hashlib.sha256(self.notice).hexdigest(),
                }
            ],
            "packages": [
                {
                    "name": "example",
                    "version": "1.2.3",
                    "source": self.SOURCE,
                    "checksum": self.CHECKSUM,
                    "repository": self.REPOSITORY,
                    "vcs_commit": self.COMMIT,
                    "path_in_vcs": "crates/example",
                    "file_ids": ["example-notice"],
                }
            ],
        }
        self.write_manifest(self.manifest)
        self.lockfile = self.root / "Cargo.lock"
        self.lockfile.write_text(
            "\n".join(
                [
                    "version = 4",
                    "",
                    "[[package]]",
                    'name = "example"',
                    'version = "1.2.3"',
                    f'source = "{self.SOURCE}"',
                    f'checksum = "{self.CHECKSUM}"',
                    "",
                ]
            ),
            encoding="utf-8",
        )

    def write_vcs(self, commit: str, path_in_vcs: str) -> None:
        (self.crate / ".cargo_vcs_info.json").write_text(
            json.dumps({"git": {"sha1": commit}, "path_in_vcs": path_in_vcs}), encoding="utf-8"
        )

    def write_manifest(self, manifest: dict[str, object]) -> None:
        (self.supplement / "index.json").write_text(json.dumps(manifest), encoding="utf-8")

    def metadata(self) -> dict[str, object]:
        return {
            "packages": [
                {
                    "id": "registry+example#example@1.2.3",
                    "name": "example",
                    "version": "1.2.3",
                    "license": "MIT",
                    "license_file": None,
                    "source": self.SOURCE,
                    "repository": self.REPOSITORY,
                    "manifest_path": str(self.crate / "Cargo.toml"),
                },
                {
                    "id": "registry+example#missing@1.0.0",
                    "name": "missing",
                    "version": "1.0.0",
                    "license": "Apache-2.0",
                    "license_file": None,
                    "source": self.SOURCE,
                    "manifest_path": str(self.missing / "Cargo.toml"),
                },
            ],
            "workspace_members": [],
        }

    def load_supplement(self) -> tuple[dict[tuple[str, str, str], str], object]:
        checksums = package.load_locked_package_checksums(self.lockfile)
        return checksums, package.load_notice_supplement(self.supplement)

    def collect(self, payload_name: str = "payload") -> tuple[dict[str, object], Path]:
        checksums, supplement = self.load_supplement()
        payload = self.root / payload_name
        index = package.collect_third_party_notices(
            self.metadata(), payload, supplement=supplement, locked_checksums=checksums
        )
        return index, payload

    def test_commit_pinned_supplement_is_offline_deterministic_and_auditable(self) -> None:
        first, first_payload = self.collect("first-payload")
        second, second_payload = self.collect("second-payload")

        self.assertEqual(first, second)
        self.assertFalse(first["extraction_complete"])
        self.assertEqual(first["package_count"], 2)
        self.assertEqual(first["packages_with_discovered_texts"], 1)
        self.assertEqual(first["packages_with_curated_supplements"], 1)
        curated = next(entry for entry in first["packages"] if entry["name"] == "example")
        self.assertEqual(
            curated["provenance"],
            {
                "kind": "curated-upstream-vcs",
                "registry_source": self.SOURCE,
                "cargo_checksum": self.CHECKSUM,
                "repository": self.REPOSITORY,
                "vcs_commit": self.COMMIT,
                "path_in_vcs": "crates/example",
            },
        )
        self.assertEqual(len(curated["files"]), 1)
        record = curated["files"][0]
        self.assertEqual(record["source_path"], "NOTICE")
        self.assertEqual(record["source_kind"], "curated-upstream-vcs")
        self.assertEqual(
            record["source_url"],
            f"https://raw.githubusercontent.com/example/project/{self.COMMIT}/NOTICE",
        )
        self.assertEqual(record["vcs_commit"], self.COMMIT)
        self.assertEqual(record["sha256"], hashlib.sha256(self.notice).hexdigest())
        self.assertEqual((first_payload / record["path"]).read_bytes(), self.notice)
        self.assertEqual((second_payload / record["path"]).read_bytes(), self.notice)
        unresolved = next(entry for entry in first["packages"] if entry["name"] == "missing")
        self.assertEqual(unresolved["files"], [])
        self.assertNotIn("provenance", unresolved)
        self.assertEqual(unresolved["license_expression"], "Apache-2.0")
        self.assertNotIn(str(self.root), json.dumps(first))

    def test_supplement_requires_exact_lock_repository_and_vcs_identity(self) -> None:
        identity = ("example", "1.2.3", self.SOURCE)
        cases = ("checksum-mismatch", "checksum-unavailable", "vcs-mismatch", "path-mismatch", "repository-mismatch")
        for case in cases:
            with self.subTest(case=case):
                self.write_vcs(self.COMMIT, "crates/example")
                metadata = self.metadata()
                checksums, supplement = self.load_supplement()
                if case == "checksum-mismatch":
                    checksums[identity] = "0" * 64
                elif case == "checksum-unavailable":
                    del checksums[identity]
                elif case == "vcs-mismatch":
                    self.write_vcs("c" * 40, "crates/example")
                elif case == "path-mismatch":
                    self.write_vcs(self.COMMIT, "different/path")
                elif case == "repository-mismatch":
                    metadata["packages"][0]["repository"] = "https://github.com/example/other"
                payload = self.root / f"payload-{case}"
                index = package.collect_third_party_notices(
                    metadata, payload, supplement=supplement, locked_checksums=checksums
                )
                curated = next(entry for entry in index["packages"] if entry["name"] == "example")
                self.assertEqual(curated["files"], [])
                self.assertNotIn("provenance", curated)
                self.assertTrue(any("curated notice supplement" in error for error in curated["errors"]))
                self.assertFalse(index["extraction_complete"])
                self.assertEqual(index["packages_with_curated_supplements"], 0)

    def test_supplement_rejects_unpinned_urls_bad_hashes_and_unsafe_paths(self) -> None:
        cases = ("moving-url", "hash-mismatch", "path-traversal", "source-path-mismatch")
        for case in cases:
            with self.subTest(case=case):
                manifest = copy.deepcopy(self.manifest)
                resource = manifest["files"][0]
                if case == "moving-url":
                    resource["source_url"] = "https://raw.githubusercontent.com/example/project/main/NOTICE"
                    diagnostic = "commit-pinned"
                elif case == "hash-mismatch":
                    resource["sha256"] = "0" * 64
                    diagnostic = "SHA-256 mismatch"
                elif case == "path-traversal":
                    resource["path"] = "../NOTICE"
                    diagnostic = "unsafe supplement file path"
                else:
                    resource["source_path"] = "docs/NOTICE"
                    diagnostic = "source URL"
                self.write_manifest(manifest)
                with self.assertRaisesRegex(package.PackagingError, diagnostic):
                    package.load_notice_supplement(self.supplement)

    def test_local_package_notice_takes_precedence_over_supplement(self) -> None:
        local_notice = b"disposable package-source notice\n"
        (self.crate / "NOTICE").write_bytes(local_notice)

        index, payload = self.collect()

        curated = next(entry for entry in index["packages"] if entry["name"] == "example")
        self.assertEqual(len(curated["files"]), 1)
        self.assertNotIn("provenance", curated)
        self.assertNotIn("source_kind", curated["files"][0])
        self.assertEqual((payload / curated["files"][0]["path"]).read_bytes(), local_notice)
        self.assertEqual(index["packages_with_curated_supplements"], 0)

    def test_emitted_supplement_hash_is_checked_again(self) -> None:
        checksums, supplement = self.load_supplement()
        with mock.patch.object(package, "_sha256_file", return_value="0" * 64):
            with self.assertRaisesRegex(package.PackagingError, "emitted.*SHA-256 mismatch"):
                package.collect_third_party_notices(
                    self.metadata(),
                    self.root / "tampered-output",
                    supplement=supplement,
                    locked_checksums=checksums,
                )


class CheckedInNoticeSupplementTests(unittest.TestCase):
    def test_accesskit_and_egui_bundles_are_hash_valid_and_match_the_real_lockfile(self) -> None:
        repo = Path(package.__file__).resolve().parents[2]
        supplement = package.load_notice_supplement(repo / "tools/package/notice_supplement")
        checksums = package.load_locked_package_checksums(repo / "Cargo.lock")
        accesskit = {
            ("accesskit", "0.25.2"): "accesskit",
            ("accesskit_atspi_common", "0.21.1"): "adapters/atspi-common",
            ("accesskit_consumer", "0.39.2"): "accesskit_consumer",
            ("accesskit_ios", "0.2.2"): "adapters/ios",
            ("accesskit_macos", "0.27.2"): "adapters/macos",
            ("accesskit_unix", "0.24.1"): "adapters/unix",
            ("accesskit_windows", "0.35.2"): "adapters/windows",
            ("accesskit_winit", "0.34.2"): "adapters/winit",
        }
        egui = {
            ("eframe", "0.37.0"): (
                "https://github.com/emilk/egui/tree/main/crates/eframe",
                "3c6eb8261c3293ad9c7ea1ed58487e038860ebef",
                "crates/eframe",
            ),
            ("egui_kittest", "0.37.0"): (
                "https://github.com/emilk/egui",
                "9d317be94fc96df23bd5d9316f357efa2d3631d8",
                "crates/egui_kittest",
            ),
            ("egui_system_font_provider", "0.37.0"): (
                "https://github.com/emilk/egui/tree/main/crates/egui_system_font_provider",
                "3c6eb8261c3293ad9c7ea1ed58487e038860ebef",
                "crates/egui_system_font_provider",
            ),
        }
        accesskit_hashes = {
            "LICENSE-APACHE": "62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a",
            "LICENSE-MIT": "23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3",
        }
        egui_hashes = {
            "LICENSE-APACHE": "8173d5c29b4f956d532781d2b86e4e30f83e6b7878dce18c919451d6ba707c90",
            "LICENSE-MIT": "95ca92f5f8ea5231f1580b3a2a799e8260af3114b900e1def5355a7f44bcf60c",
        }

        self.assertEqual({identity[:2] for identity in supplement.packages}, set(accesskit) | set(egui))
        for identity, curated in supplement.packages.items():
            with self.subTest(package=identity[:2]):
                self.assertEqual(identity[2], NoticeProvenanceSupplementTests.SOURCE)
                self.assertEqual(checksums[identity], curated.checksum)
                if identity[:2] in accesskit:
                    repository = "https://github.com/AccessKit/accesskit"
                    commit = "f3026222559c7f5f46d8f25cb7404de68eb149f0"
                    path_in_vcs = accesskit[identity[:2]]
                    raw_repository = "AccessKit/accesskit"
                    expected_hashes = accesskit_hashes
                else:
                    repository, commit, path_in_vcs = egui[identity[:2]]
                    raw_repository = "emilk/egui"
                    expected_hashes = egui_hashes
                self.assertEqual(curated.repository, repository)
                self.assertEqual(curated.vcs_commit, commit)
                self.assertEqual(curated.path_in_vcs, path_in_vcs)
                self.assertEqual(len(curated.files), 2)
                self.assertEqual(
                    {resource.source_path.as_posix() for resource in curated.files},
                    {"LICENSE-APACHE", "LICENSE-MIT"},
                )
                for resource in curated.files:
                    source_path = resource.source_path.as_posix()
                    self.assertEqual(hashlib.sha256(resource.content).hexdigest(), resource.sha256)
                    self.assertEqual(resource.sha256, expected_hashes[source_path])
                    self.assertEqual(
                        resource.source_url,
                        f"https://raw.githubusercontent.com/{raw_repository}/{commit}/{source_path}",
                    )


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

    def setUp(self) -> None:
        super().setUp()
        self.repo = self.root / "repo"
        (self.repo / "packaging").mkdir(parents=True)
        (self.repo / "docs").mkdir()
        (self.repo / "packaging/INTERNAL_PREVIEW.md").write_text(
            "Internal disposable preview {{VERSION}} {{PLATFORM}}\n", encoding="utf-8"
        )
        (self.repo / "docs/PARITY_LEDGER.md").write_text("Disposable unverified ledger\n", encoding="utf-8")
        self.trace = self.repo / "trace.json"
        self.trace.write_text("{}\n", encoding="utf-8")
        self.binary = self.repo / "target/release/paint-ui"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(elf_header())
        self.digest = hashlib.sha256(self.binary.read_bytes()).hexdigest()
        package_id = "path+file:///disposable#paint-ui@0.1.0"
        self.metadata = {
            "packages": [
                {
                    "id": package_id,
                    "name": "paint-ui",
                    "version": "0.1.0",
                    "license": None,
                    "license_file": None,
                    "source": None,
                    "manifest_path": str(self.repo / "crates/ui/Cargo.toml"),
                }
            ],
            "workspace_members": [package_id],
            "resolve": {"nodes": [{"id": package_id, "deps": []}]},
        }
        run = {
            "width": 800,
            "height": 600,
            "decoded_rgba_sha256": "a" * 64,
            "png_sha256": "b" * 64,
            "snapshot_sha256": "c" * 64,
            "snapshot": {
                "layer_count": 1,
                "active_layer_index": 0,
                "dirty": False,
                "can_undo": False,
                "can_redo": False,
            },
        }
        self.smoke_evidence = {
            "format": "paint.r.preview-smoke-evidence",
            "version": 1,
            "status": "pass",
            "binary_sha256": self.digest,
            "repeat": 1,
            "traces": [
                {
                    "trace": "trace.json",
                    "status": "pass",
                    "runs": [run],
                    "negative_paths": {"unknown_command": "pass", "invalid_rectangle": "pass"},
                }
            ],
        }
        self.archive_name = "paint-r-0.1.0-linux-x86_64.tar.gz"

    def invoke(self, output: Path) -> package.PackageResult:
        return package.package_release(
            repo_root=self.repo,
            binary=self.binary,
            platform="linux-x86_64",
            output_dir=output,
            traces=[self.trace],
            repeat=1,
            timeout=1.0,
            cargo_command=["must-not-run"],
        )

    def assert_no_success_outputs(self, output: Path) -> None:
        self.assertFalse((output / self.archive_name).exists())
        self.assertFalse((output / f"{self.archive_name}.sha256").exists())
        self.assertFalse((output / f"{self.archive_name}.verification.json").exists())

    def test_package_release_is_deterministic_and_embeds_binary_evidence(self) -> None:
        with (
            mock.patch.dict(os.environ, {"SOURCE_DATE_EPOCH": str(EPOCH)}),
            mock.patch.object(package, "run_preview_smoke", return_value=self.smoke_evidence) as smoke,
            mock.patch.object(package, "load_locked_metadata", return_value=self.metadata) as load_metadata,
        ):
            first = self.invoke(self.root / "dist/first")
            second = self.invoke(self.root / "dist/second")
        self.assertEqual(smoke.call_count, 4)
        self.assertEqual(load_metadata.call_count, 2)
        self.assertEqual(first.archive.read_bytes(), second.archive.read_bytes())
        self.assertEqual(first.sidecar.read_bytes(), second.sidecar.read_bytes())
        self.assertEqual(first.verification.read_bytes(), second.verification.read_bytes())
        self.assertEqual(first.archive.name, self.archive_name)

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
        self.assertEqual(binary_evidence["binary"]["sha256"], self.digest)
        self.assertEqual(binary_evidence["binary"]["scope"], "executable-header-only-not-native-execution-proof")
        manifest = files["SHA256SUMS.txt"].decode("utf-8")
        expected = "".join(
            f"{hashlib.sha256(files[path]).hexdigest()}  {path}\n"
            for path in sorted(files)
            if path != "SHA256SUMS.txt"
        )
        self.assertEqual(manifest, expected)
        verification = json.loads(first.verification.read_text(encoding="utf-8"))
        self.assertEqual(verification["status"], "pass")
        self.assertEqual(verification["archive_sha256"], hashlib.sha256(first.archive.read_bytes()).hexdigest())
        self.assertEqual(verification["payload_entries"], len(files))
        self.assertEqual(verification["binary_sha256"], self.digest)
        self.assertEqual(
            verification["post_extract_smoke"], {"status": "pass", "repeat": 1, "trace_count": 1}
        )

    def test_post_archive_smoke_failure_leaves_no_success_outputs(self) -> None:
        output = self.root / "dist/post-smoke-failure"
        with (
            mock.patch.object(
                package,
                "run_preview_smoke",
                side_effect=[self.smoke_evidence, package.PackagingError("extracted smoke failed")],
            ) as smoke,
            mock.patch.object(package, "load_locked_metadata", return_value=self.metadata),
        ):
            with self.assertRaisesRegex(package.PackagingError, "extracted smoke failed"):
                self.invoke(output)
        self.assertEqual(smoke.call_count, 2)
        self.assert_no_success_outputs(output)

    def test_sidecar_manifest_and_mode_tampering_are_fatal(self) -> None:
        write_sidecar = package.write_archive_sidecar
        create_archive = package.create_tar_gz_archive

        def corrupt_sidecar(archive: Path) -> Path:
            sidecar = write_sidecar(archive)
            sidecar.write_text(f"{'0' * 64}  {archive.name}\n", encoding="utf-8")
            return sidecar

        def stale_manifest(payload: Path, destination: Path, epoch: int) -> None:
            (payload / "PARITY_LEDGER.md").write_bytes(b"tampered after manifest generation\n")
            create_archive(payload, destination, epoch)

        def lost_executable_mode(payload: Path, destination: Path, epoch: int) -> None:
            (payload / "paint-ui").chmod(0o644)
            create_archive(payload, destination, epoch)

        cases = (
            ("sidecar", "write_archive_sidecar", corrupt_sidecar, "sidecar"),
            ("manifest", "create_tar_gz_archive", stale_manifest, "manifest"),
            ("mode", "create_tar_gz_archive", lost_executable_mode, "not executable"),
        )
        for name, target, replacement, diagnostic in cases:
            with self.subTest(case=name):
                output = self.root / f"dist/{name}-failure"
                with (
                    mock.patch.object(package, "run_preview_smoke", return_value=self.smoke_evidence) as smoke,
                    mock.patch.object(package, "load_locked_metadata", return_value=self.metadata),
                    mock.patch.object(package, target, side_effect=replacement),
                ):
                    with self.assertRaisesRegex(package.PackagingError, diagnostic):
                        self.invoke(output)
                self.assertEqual(smoke.call_count, 1)
                self.assert_no_success_outputs(output)


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
