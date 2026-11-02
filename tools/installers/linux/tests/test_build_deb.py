#!/usr/bin/env python3
"""Tests for the standard-library Linux .deb builder (runnable on macOS)."""

from __future__ import annotations

import configparser
import gzip
import hashlib
import io
import math
import os
import shutil
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
BUILD_SH = ROOT / "tools" / "installers" / "linux" / "build.sh"
BUILDER = ROOT / "tools" / "installers" / "linux" / "build_deb.py"
ICON = ROOT / "tools" / "installers" / "linux" / "assets" / "paint-rust.svg"
README = ROOT / "README.md"
LICENSE = ROOT / "LICENSE"
AR_MAGIC = b"!<arch>\n"
ELF_MAGIC = b"\x7fELF"
EPOCH = 946_684_800
SVG_NAMESPACE = "{http://www.w3.org/2000/svg}"

EXPECTED_FILES = {
    "usr/bin/paint-ui",
    "usr/share/applications/paint-rust.desktop",
    "usr/share/icons/hicolor/scalable/apps/paint-rust.svg",
    "usr/share/doc/paint-rust/README.md",
    "usr/share/doc/paint-rust/LICENSE",
}
EXPECTED_DEPENDS = (
    "libc6, libx11-6, libxkbcommon0, libwayland-client0, libgl1, libegl1, "
    "libxcursor1, libxrandr2, libxi6"
)
EXPECTED_MIME_TYPES = {
    "application/x-paint-rust-pdr",
    "application/x-paintnet-pdn",
    "image/x-pdn",
    "image/png",
    "image/jpeg",
    "image/tiff",
    "image/bmp",
    "image/x-tga",
    "image/x-dds",
    "image/gif",
    "image/webp",
}


class ArMember:
    def __init__(self, name: str, mtime: int, uid: int, gid: int, mode: int, data: bytes) -> None:
        self.name = name
        self.mtime = mtime
        self.uid = uid
        self.gid = gid
        self.mode = mode
        self.data = data


def synthetic_elf(
    machine: int = 62,
    elf_class: int = 2,
    data: int = 1,
    e_type: int = 2,
    e_version: int = 1,
    e_ehsize: int = 64,
) -> bytes:
    """Return a minimal 64-bit ELF header fixture with the requested fields."""
    ident = bytearray(16)
    ident[0:4] = ELF_MAGIC
    ident[4] = elf_class
    ident[5] = data
    ident[6] = 1
    body = struct.pack(
        "<HHIQQQIHHHHHH",
        e_type,
        machine,
        e_version,
        0,
        0,
        0,
        0,
        e_ehsize,
        0,
        0,
        0,
        0,
        0,
    )
    return bytes(ident) + body


def parse_ar(path: Path) -> list[ArMember]:
    data = path.read_bytes()
    if not data.startswith(AR_MAGIC):
        raise AssertionError("missing ar global magic")
    offset = len(AR_MAGIC)
    members: list[ArMember] = []
    while offset < len(data):
        header = data[offset : offset + 60]
        if len(header) != 60 or header[58:] != b"`\n":
            raise AssertionError(f"invalid ar header at offset {offset}")
        name = header[0:16].decode("ascii").strip()
        mtime = int(header[16:28].decode("ascii").strip(), 10)
        uid = int(header[28:34].decode("ascii").strip(), 10)
        gid = int(header[34:40].decode("ascii").strip(), 10)
        mode = int(header[40:48].decode("ascii").strip(), 8)
        size = int(header[48:58].decode("ascii").strip(), 10)
        offset += 60
        payload = data[offset : offset + size]
        if len(payload) != size:
            raise AssertionError(f"truncated ar member {name}")
        members.append(ArMember(name, mtime, uid, gid, mode, payload))
        offset += size
        if size % 2:
            if data[offset : offset + 1] != b"\n":
                raise AssertionError(f"missing ar padding after {name}")
            offset += 1
    if offset != len(data):
        raise AssertionError("trailing ar bytes")
    return members


def parse_tar_gz(data: bytes) -> list[tuple[tarfile.TarInfo, bytes | None]]:
    raw = gzip.decompress(data)
    members: list[tuple[tarfile.TarInfo, bytes | None]] = []
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as archive:
        for member in archive:
            payload = archive.extractfile(member).read() if member.isfile() else None
            members.append((member, payload))
    return members


def tar_entry_name(member: tarfile.TarInfo) -> str:
    """Normalize tarfile's directory-name handling for assertions."""
    if member.isdir():
        if member.name in {".", "./"}:
            return "./"
        return member.name.rstrip("/") + "/"
    return member.name


def parse_control(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    current: str | None = None
    for line in text.splitlines():
        if line.startswith((" ", "\t")):
            if current is None:
                raise AssertionError("control continuation without a field")
            fields[current] += "\n" + line[1:]
        elif line == "":
            continue
        else:
            key, separator, value = line.partition(":")
            if not separator:
                raise AssertionError(f"invalid control line: {line!r}")
            if key in fields:
                raise AssertionError(f"duplicate control field: {key}")
            current = key
            fields[key] = value.lstrip(" ")
    return fields


def parse_desktop(data: bytes) -> configparser.SectionProxy:
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str  # Preserve the desktop entry's exact key case.
    parser.read_string(data.decode("utf-8"))
    return parser["Desktop Entry"]


def run_build(
    version: str,
    binary: Path,
    output: Path,
    *,
    cwd: Path = ROOT,
    epoch: str | None = str(EPOCH),
    extra_env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.pop("SOURCE_DATE_EPOCH", None)
    if epoch is not None:
        env["SOURCE_DATE_EPOCH"] = epoch
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        ["bash", str(BUILD_SH), version, str(binary), str(output)],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )


def run_builder(
    version: str,
    binary: Path,
    output: Path,
    *,
    repo_root: Path,
    icon: Path | None = None,
    epoch: str | None = str(EPOCH),
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.pop("SOURCE_DATE_EPOCH", None)
    if epoch is not None:
        env["SOURCE_DATE_EPOCH"] = epoch
    command = [
        sys.executable,
        str(BUILDER),
        "--version",
        version,
        "--binary",
        str(binary),
        "--output-dir",
        str(output),
        "--repo-root",
        str(repo_root),
    ]
    if icon is not None:
        command.extend(["--icon", str(icon)])
    return subprocess.run(command, env=env, capture_output=True, text=True, check=False)


class DebianPackageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "paint-ui"
        self.binary.write_bytes(synthetic_elf())
        self.binary.chmod(0o644)

    def build(self, version: str = "0.1.1", output_name: str = "out") -> tuple[Path, Path, Path]:
        output = self.root / output_name
        process = run_build(version, self.binary, output)
        self.assertEqual(process.returncode, 0, process.stderr)
        deb = output / f"paint-rust_{version.removeprefix('v')}_amd64.deb"
        sidecar = deb.with_name(deb.name + ".sha256")
        return output, deb, sidecar

    def test_builds_expected_debian_package(self) -> None:
        output, deb, sidecar = self.build()
        self.assertTrue(deb.is_file())
        self.assertTrue(sidecar.is_file())
        self.assertEqual(stat.S_IMODE(deb.stat().st_mode), 0o644)
        self.assertEqual(stat.S_IMODE(sidecar.stat().st_mode), 0o644)
        self.assertEqual(
            sorted(path.name for path in output.iterdir()),
            sorted([deb.name, sidecar.name]),
        )

        members = parse_ar(deb)
        self.assertEqual([member.name for member in members], ["debian-binary", "control.tar.gz", "data.tar.gz"])
        for member in members:
            self.assertEqual(member.mtime, EPOCH)
            self.assertEqual(member.uid, 0)
            self.assertEqual(member.gid, 0)
            self.assertEqual(member.mode, 0o100644)
        debian_binary, control_member, data_member = members
        self.assertEqual(debian_binary.data, b"2.0\n")

        control_entries = parse_tar_gz(control_member.data)
        self.assertEqual(
            [tar_entry_name(member) for member, _payload in control_entries],
            ["./", "./control", "./md5sums"],
        )
        control_map = {
            member.name: payload for member, payload in control_entries if member.isfile()
        }
        for member, _payload in control_entries:
            self.assertEqual(member.mtime, EPOCH)
            self.assertEqual(member.uid, 0)
            self.assertEqual(member.gid, 0)
            self.assertEqual(member.uname, "")
            self.assertEqual(member.gname, "")
        fields = parse_control(control_map["./control"].decode("utf-8"))
        self.assertEqual(fields["Package"], "paint-rust")
        self.assertEqual(fields["Version"], "0.1.1")
        self.assertEqual(fields["Architecture"], "amd64")
        self.assertEqual(fields["Maintainer"], "Diego Garcia <https://github.com/diegomgarcia>")
        self.assertEqual(fields["Section"], "graphics")
        self.assertEqual(fields["Priority"], "optional")
        self.assertEqual(fields["Depends"], EXPECTED_DEPENDS)
        self.assertEqual(fields["Homepage"], "https://github.com/diegomgarcia/paint.rust")
        self.assertTrue(fields["Description"].startswith("Paint.NET-inspired image editor\n"))
        self.assertIn("unsigned development-preview", fields["Description"])
        self.assertNotIn("Signature", fields)

        data_entries = parse_tar_gz(data_member.data)
        payload = {member.name: value for member, value in data_entries if member.isfile()}
        self.assertEqual(set(payload), {f"./{path}" for path in EXPECTED_FILES})
        expected_directories = {"./"}
        for relative in EXPECTED_FILES:
            parts = relative.split("/")
            for index in range(1, len(parts)):
                expected_directories.add(f"./{'/'.join(parts[:index])}/")
        actual_directories = {tar_entry_name(member) for member, _value in data_entries if member.isdir()}
        self.assertEqual(actual_directories, expected_directories)
        names = [tar_entry_name(member) for member, _value in data_entries]
        self.assertEqual(names, sorted(names))
        for member, _value in data_entries:
            self.assertEqual(member.mtime, EPOCH)
            self.assertEqual(member.uid, 0)
            self.assertEqual(member.gid, 0)
            self.assertEqual(member.uname, "")
            self.assertEqual(member.gname, "")
            self.assertEqual(stat.S_IMODE(member.mode), 0o755 if member.isdir() else (0o755 if member.name == "./usr/bin/paint-ui" else 0o644))

        self.assertEqual(payload["./usr/bin/paint-ui"], self.binary.read_bytes())
        self.assertEqual(payload["./usr/share/doc/paint-rust/README.md"], README.read_bytes())
        self.assertEqual(payload["./usr/share/doc/paint-rust/LICENSE"], LICENSE.read_bytes())
        self.assertEqual(payload["./usr/share/icons/hicolor/scalable/apps/paint-rust.svg"], ICON.read_bytes())

        desktop = parse_desktop(payload["./usr/share/applications/paint-rust.desktop"])
        self.assertEqual(desktop["Type"], "Application")
        self.assertEqual(desktop["Name"], "Paint.Rust")
        self.assertEqual(desktop["Comment"], "Paint.NET-inspired image editor")
        self.assertEqual(desktop["Exec"], "paint-ui")
        self.assertEqual(desktop["Icon"], "paint-rust")
        self.assertEqual(desktop["Categories"], "Graphics;RasterGraphics;2DGraphics;")
        self.assertEqual(desktop["Terminal"], "false")
        self.assertEqual(
            {value for value in desktop["MimeType"].split(";") if value},
            EXPECTED_MIME_TYPES,
        )

        icon = ElementTree.fromstring(payload["./usr/share/icons/hicolor/scalable/apps/paint-rust.svg"])
        self.assertEqual(icon.tag, SVG_NAMESPACE + "svg")
        self.assertIn("viewBox", icon.attrib)
        for element in icon.iter():
            self.assertNotEqual(element.tag, SVG_NAMESPACE + "image")
            for attribute, value in element.attrib.items():
                if attribute.rsplit("}", 1)[-1] in {"href", "src"}:
                    self.assertFalse(value and not value.startswith("#"))

        total_bytes = sum(len(payload[f"./{path}"]) for path in EXPECTED_FILES)
        self.assertEqual(fields["Installed-Size"], str(max(1, math.ceil(total_bytes / 1024))))
        md5sums = control_map["./md5sums"].decode("utf-8").splitlines()
        self.assertEqual(len(md5sums), len(EXPECTED_FILES))
        for line in md5sums:
            digest, relative = line.split("  ", 1)
            self.assertEqual(digest, hashlib.md5(payload[f"./{relative}"]).hexdigest())

        digest = hashlib.sha256(deb.read_bytes()).hexdigest()
        self.assertEqual(sidecar.read_bytes(), f"{digest}  {deb.name}\n".encode("utf-8"))

    def test_deterministic_across_two_runs(self) -> None:
        _output_a, deb_a, sidecar_a = self.build(output_name="out-a")
        bytes_a = deb_a.read_bytes()
        sidecar_bytes_a = sidecar_a.read_bytes()

        # Change source mtimes and use a different output directory.  Neither may
        # leak into archive bytes.
        os.utime(self.binary, (1_234_567_890, 1_234_567_890))
        _output_b, deb_b, sidecar_b = self.build(output_name="out-b")
        self.assertEqual(bytes_a, deb_b.read_bytes())
        self.assertEqual(sidecar_bytes_a, sidecar_b.read_bytes())
        self.assertEqual(sidecar_bytes_a.decode("utf-8"), f"{hashlib.sha256(bytes_a).hexdigest()}  {deb_a.name}\n")

    def test_custom_source_date_epoch_is_applied(self) -> None:
        epoch = "1700000000"
        output = self.root / "epoch"
        process = run_build("0.1.1", self.binary, output, epoch=epoch)
        self.assertEqual(process.returncode, 0, process.stderr)
        deb = output / "paint-rust_0.1.1_amd64.deb"
        members = parse_ar(deb)
        self.assertTrue(all(member.mtime == int(epoch) for member in members))
        control_entries = parse_tar_gz(members[1].data)
        data_entries = parse_tar_gz(members[2].data)
        self.assertTrue(all(member.mtime == int(epoch) for member, _ in control_entries + data_entries))

    def test_version_validation_and_normalization(self) -> None:
        invalid_versions = [
            "",
            " 0.1.1",
            "0.1.1 ",
            "v",
            "latest",
            "1.0.0/evil",
            "1.0.0_1",
            "1:2.0.0",
            "1.0-",
            "V1.0.0",
        ]
        for index, version in enumerate(invalid_versions):
            with self.subTest(version=version):
                output = self.root / f"invalid-{index}"
                process = run_build(version, self.binary, output)
                self.assertNotEqual(process.returncode, 0)
                self.assertIn("version", process.stderr)
                if output.exists():
                    self.assertEqual(list(output.glob("*.deb")), [])

        output = self.root / "normalized"
        process = run_build("v0.1.1", self.binary, output)
        self.assertEqual(process.returncode, 0, process.stderr)
        deb = output / "paint-rust_0.1.1_amd64.deb"
        self.assertTrue(deb.is_file())
        control_entries = parse_tar_gz(parse_ar(deb)[1].data)
        control_text = next(
            payload for member, payload in control_entries if member.name == "./control"
        )
        fields = parse_control(control_text.decode("utf-8"))
        self.assertEqual(fields["Version"], "0.1.1")

        output = self.root / "prerelease"
        process = run_build("0.1.1-preview.1", self.binary, output)
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertTrue((output / "paint-rust_0.1.1-preview.1_amd64.deb").is_file())

    def test_rejects_invalid_elf_headers(self) -> None:
        cases = {
            "empty": b"",
            "not-elf": b"not an ELF file at all, but long enough to have a header......",
            "truncated": synthetic_elf()[:32],
            "elf32": synthetic_elf(elf_class=1),
            "big-endian": synthetic_elf(data=2),
            "aarch64": synthetic_elf(machine=183),
            "not-executable": synthetic_elf(e_type=1),
            "bad-elf-version": synthetic_elf(e_version=0),
            "bad-header-size": synthetic_elf(e_ehsize=0),
        }
        for index, (name, content) in enumerate(cases.items()):
            with self.subTest(case=name):
                binary = self.root / f"bad-{index}"
                binary.write_bytes(content)
                output = self.root / f"bad-out-{index}"
                process = run_build("0.1.1", binary, output)
                self.assertNotEqual(process.returncode, 0)
                self.assertIn("ELF", process.stderr)
                if output.exists():
                    self.assertEqual(list(output.glob("*.deb")), [])

    def test_relative_paths_and_output_directory_with_spaces(self) -> None:
        working = self.root / "working copy"
        working.mkdir()
        binary = working / "paint-ui"
        binary.write_bytes(synthetic_elf())
        process = run_build("0.1.1", Path("paint-ui"), Path("out dir"), cwd=working)
        self.assertEqual(process.returncode, 0, process.stderr)
        deb = working / "out dir" / "paint-rust_0.1.1_amd64.deb"
        self.assertTrue(deb.is_file())
        self.assertTrue((deb.parent / (deb.name + ".sha256")).is_file())

    def test_missing_readme_fails_without_output(self) -> None:
        repo = self.root / "repo-without-readme"
        repo.mkdir()
        (repo / "LICENSE").write_text("MIT\n", encoding="utf-8")
        output = self.root / "missing-readme-out"
        process = run_builder("0.1.1", self.binary, output, repo_root=repo)
        self.assertEqual(process.returncode, 1)
        self.assertIn("README.md", process.stderr)
        self.assertFalse(output.exists())

    def test_missing_license_is_optional(self) -> None:
        repo = self.root / "repo-without-license"
        repo.mkdir()
        (repo / "README.md").write_text("# Paint.Rust\n", encoding="utf-8")
        output = self.root / "without-license-out"
        process = run_builder("0.1.1", self.binary, output, repo_root=repo)
        self.assertEqual(process.returncode, 0, process.stderr)
        deb = output / "paint-rust_0.1.1_amd64.deb"
        payload_names = {
            member.name
            for member, value in parse_tar_gz(parse_ar(deb)[2].data)
            if value is not None
        }
        self.assertNotIn("./usr/share/doc/paint-rust/LICENSE", payload_names)
        self.assertIn("./usr/share/doc/paint-rust/README.md", payload_names)

    def test_invalid_icon_is_rejected(self) -> None:
        repo = self.root / "repo-for-icon"
        repo.mkdir()
        (repo / "README.md").write_text("# Paint.Rust\n", encoding="utf-8")
        icon = self.root / "external.svg"
        icon.write_text(
            '<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.invalid/x.png"/></svg>',
            encoding="utf-8",
        )
        output = self.root / "bad-icon-out"
        process = run_builder("0.1.1", self.binary, output, repo_root=repo, icon=icon)
        self.assertEqual(process.returncode, 1)
        self.assertIn("external resources", process.stderr)
        self.assertFalse(output.exists())

    def test_invalid_source_date_epoch_is_rejected(self) -> None:
        output = self.root / "bad-epoch-out"
        process = run_build("0.1.1", self.binary, output, epoch="not-a-number")
        self.assertEqual(process.returncode, 1)
        self.assertIn("SOURCE_DATE_EPOCH", process.stderr)
        self.assertFalse(output.exists())

    def test_build_sh_usage_requires_three_arguments(self) -> None:
        process = subprocess.run(
            ["bash", str(BUILD_SH)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(process.returncode, 2)
        self.assertIn("usage:", process.stderr)

    @unittest.skipUnless(shutil.which("dpkg-deb"), "dpkg-deb is not installed")
    def test_dpkg_deb_accepts_the_package_when_available(self) -> None:
        _output, deb, _sidecar = self.build()
        info = subprocess.run(
            ["dpkg-deb", "--info", str(deb)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(info.returncode, 0, info.stderr)
        self.assertIn("Package: paint-rust", info.stdout)
        self.assertIn("Version: 0.1.1", info.stdout)
        self.assertIn("Architecture: amd64", info.stdout)
        contents = subprocess.run(
            ["dpkg-deb", "--contents", str(deb)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(contents.returncode, 0, contents.stderr)
        self.assertIn("usr/bin/paint-ui", contents.stdout)
        self.assertIn("usr/share/applications/paint-rust.desktop", contents.stdout)
        extracted = self.root / "extracted"
        extracted.mkdir()
        extract = subprocess.run(
            ["dpkg-deb", "--extract", str(deb), str(extracted)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(extract.returncode, 0, extract.stderr)
        extracted_binary = extracted / "usr/bin/paint-ui"
        self.assertEqual(extracted_binary.read_bytes(), self.binary.read_bytes())
        self.assertEqual(stat.S_IMODE(extracted_binary.stat().st_mode), 0o755)
        self.assertEqual(
            stat.S_IMODE((extracted / "usr/share/applications/paint-rust.desktop").stat().st_mode),
            0o644,
        )


if __name__ == "__main__":
    unittest.main()
