#!/usr/bin/env python3
"""Build a deterministic, unsigned Debian package for the paint-ui release binary.

The archive is written with the Python standard library only.  It uses the ar and
tar/gzip metadata normalization rules described in tools/installers/linux/README.md
and never creates maintainer scripts.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
import re
import shutil
import struct
import sys
import tarfile
import tempfile
import xml.etree.ElementTree as ElementTree
from dataclasses import dataclass
from pathlib import Path

PACKAGE_NAME = "paint-rust"
ARCHITECTURE = "amd64"
MAINTAINER = "Diego Garcia <https://github.com/diegomgarcia>"
HOMEPAGE = "https://github.com/diegomgarcia/paint.rust"
SECTION = "graphics"
PRIORITY = "optional"
DEPENDS = (
    "libc6",
    "libx11-6",
    "libxkbcommon0",
    "libwayland-client0",
    "libgl1",
    "libegl1",
    "libxcursor1",
    "libxrandr2",
    "libxi6",
)

DEFAULT_SOURCE_DATE_EPOCH = 946_684_800  # 2000-01-01T00:00:00Z
ELF_MAGIC = b"\x7fELF"
EM_X86_64 = 62
ET_EXEC = 2
ET_DYN = 3

DESKTOP_FILE = """[Desktop Entry]
Version=1.0
Type=Application
Name=Paint.Rust
Comment=Paint.NET-inspired image editor
Exec=paint-ui
Icon=paint-rust
Terminal=false
Categories=Graphics;RasterGraphics;2DGraphics;
MimeType=application/x-paint-rust-pdr;application/x-paintnet-pdn;image/x-pdn;image/png;image/jpeg;image/tiff;image/bmp;image/x-tga;image/x-dds;image/gif;image/webp;
StartupNotify=true
"""


class BuildError(RuntimeError):
    """An expected input, validation, or packaging failure."""


@dataclass(frozen=True)
class BuildResult:
    deb: Path
    sidecar: Path
    version: str
    installed_size: int
    sha256: str


def _default_repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def normalize_version(raw: str) -> str:
    """Validate a Debian version and strip an optional release-tag ``v`` prefix."""
    if not isinstance(raw, str):
        raise BuildError("version must be a string")
    if raw != raw.strip():
        raise BuildError("version must not have leading or trailing whitespace")
    if not raw:
        raise BuildError("version must not be empty")
    version = raw[1:] if raw.startswith("v") and len(raw) > 1 and raw[1].isdigit() else raw
    if len(version) > 128:
        raise BuildError("version must be at most 128 characters")
    # Debian versions start with a digit.  This intentionally rejects an epoch and
    # characters (such as '_' or '/') that cannot appear in the output filename.
    if re.fullmatch(r"[0-9][A-Za-z0-9.+~]*(?:-[A-Za-z0-9.+~]+)*", version) is None:
        raise BuildError(
            f"invalid Debian version {raw!r}; expected a digit followed by "
            "letters, digits, '.', '+', '~' or a single '-' revision separator"
        )
    return version


def source_date_epoch() -> int:
    """Read SOURCE_DATE_EPOCH, defaulting to 2000-01-01 UTC for reproducibility."""
    raw = os.environ.get("SOURCE_DATE_EPOCH")
    if raw is None:
        return DEFAULT_SOURCE_DATE_EPOCH
    try:
        epoch = int(raw, 10)
    except ValueError as error:
        raise BuildError("SOURCE_DATE_EPOCH must be a decimal integer") from error
    if not 0 <= epoch <= 0xFFFFFFFF:
        raise BuildError("SOURCE_DATE_EPOCH must be between 0 and 4294967295")
    return epoch


def read_elf_x86_64(binary: Path) -> bytes:
    """Read the binary and require a 64-bit little-endian x86_64 ELF header."""
    if not binary.exists():
        raise BuildError(f"binary does not exist: {binary}")
    if not binary.is_file():
        raise BuildError(f"binary is not a regular file: {binary}")
    try:
        data = binary.read_bytes()
    except OSError as error:
        raise BuildError(f"cannot read binary {binary}: {error}") from error
    if len(data) < 64:
        raise BuildError(f"binary is too short to contain an ELF64 header: {binary}")
    if data[:4] != ELF_MAGIC:
        raise BuildError(f"binary is not an ELF file (missing ELF magic): {binary}")
    if data[4] != 2:
        raise BuildError(f"binary is not a 64-bit ELF executable: {binary}")
    if data[5] != 1:
        raise BuildError(f"binary is not a little-endian ELF executable: {binary}")
    if data[6] != 1:
        raise BuildError(f"binary has an unsupported ELF ident version: {binary}")
    e_type, e_machine, e_version = struct.unpack_from("<HHI", data, 16)
    (e_ehsize,) = struct.unpack_from("<H", data, 52)
    if e_type not in (ET_EXEC, ET_DYN):
        raise BuildError(f"binary is not an ELF executable (e_type={e_type}): {binary}")
    if e_machine != EM_X86_64:
        raise BuildError(
            f"binary is not an x86_64 ELF executable (e_machine={e_machine}, "
            f"expected {EM_X86_64}): {binary}"
        )
    if e_version != 1:
        raise BuildError(f"binary has an unsupported ELF version: {binary}")
    if e_ehsize != 64:
        raise BuildError(f"binary has an unexpected ELF64 header size ({e_ehsize}): {binary}")
    return data


def read_first_party_icon(icon: Path) -> bytes:
    if not icon.is_file():
        raise BuildError(f"icon does not exist or is not a regular file: {icon}")
    try:
        data = icon.read_bytes()
    except OSError as error:
        raise BuildError(f"cannot read icon {icon}: {error}") from error
    try:
        root = ElementTree.fromstring(data)
    except ElementTree.ParseError as error:
        raise BuildError(f"icon is not valid SVG XML: {error}") from error
    if root.tag.rsplit("}", 1)[-1] != "svg":
        raise BuildError("icon must have an <svg> root element")
    for element in root.iter():
        for attribute, value in element.attrib.items():
            local_name = attribute.rsplit("}", 1)[-1]
            if local_name in {"href", "src"} and value and not value.startswith("#"):
                raise BuildError("icon must not reference external resources")
    return data


def read_required(path: Path, label: str) -> bytes:
    if not path.is_file():
        raise BuildError(f"{label} is missing: {path}")
    try:
        return path.read_bytes()
    except OSError as error:
        raise BuildError(f"cannot read {label} {path}: {error}") from error


def read_optional(path: Path, label: str) -> bytes | None:
    if not path.exists():
        return None
    return read_required(path, label)


def _payload_files(
    binary: bytes,
    icon: bytes,
    readme: bytes,
    license_text: bytes | None,
    desktop: bytes,
) -> dict[str, tuple[bytes, int]]:
    files: dict[str, tuple[bytes, int]] = {
        "usr/bin/paint-ui": (binary, 0o755),
        "usr/share/applications/paint-rust.desktop": (desktop, 0o644),
        "usr/share/icons/hicolor/scalable/apps/paint-rust.svg": (icon, 0o644),
        "usr/share/doc/paint-rust/README.md": (readme, 0o644),
    }
    if license_text is not None:
        files["usr/share/doc/paint-rust/LICENSE"] = (license_text, 0o644)
    return files


def _archive_entries(
    files: dict[str, tuple[bytes, int]],
) -> list[tuple[str, str, int, bytes | None]]:
    directories: set[str] = set()
    for relative in files:
        parts = relative.split("/")
        for index in range(1, len(parts)):
            directories.add("/".join(parts[:index]))
    entries: list[tuple[str, str, int, bytes | None]] = [("./", "dir", 0o755, None)]
    entries.extend((f"./{directory}/", "dir", 0o755, None) for directory in directories)
    entries.extend((f"./{relative}", "file", mode, content) for relative, (content, mode) in files.items())
    entries.sort(key=lambda entry: entry[0])
    return entries


def _tar_gz(entries: list[tuple[str, str, int, bytes | None]], epoch: int) -> bytes:
    buffer = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=buffer, compresslevel=9, mtime=epoch) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, kind, mode, content in entries:
                info = tarfile.TarInfo(name)
                info.mtime = epoch
                info.mode = mode
                info.uid = 0
                info.gid = 0
                info.uname = ""
                info.gname = ""
                info.pax_headers = {}
                if kind == "dir":
                    info.type = tarfile.DIRTYPE
                    info.size = 0
                    archive.addfile(info)
                else:
                    assert content is not None
                    info.type = tarfile.REGTYPE
                    info.size = len(content)
                    archive.addfile(info, io.BytesIO(content))
    return buffer.getvalue()


def _installed_size(files: dict[str, tuple[bytes, int]]) -> int:
    total_bytes = sum(len(content) for content, _mode in files.values())
    return max(1, (total_bytes + 1023) // 1024)


def _render_control(version: str, installed_size: int) -> bytes:
    description = (
        "Description: Paint.NET-inspired image editor\n"
        " Paint.Rust is a fast, modular image editor written in Rust, inspired by\n"
        " Paint.NET's direct, tool-first workflow.\n"
        " .\n"
        " This package is an unsigned development-preview build of the release\n"
        " binary. It is not code-signed, notarized, or approved for public\n"
        " distribution, and it contains no Paint.NET code or assets.\n"
    )
    fields = (
        f"Package: {PACKAGE_NAME}\n"
        f"Version: {version}\n"
        f"Architecture: {ARCHITECTURE}\n"
        f"Maintainer: {MAINTAINER}\n"
        f"Installed-Size: {installed_size}\n"
        f"Depends: {', '.join(DEPENDS)}\n"
        f"Section: {SECTION}\n"
        f"Priority: {PRIORITY}\n"
        f"Homepage: {HOMEPAGE}\n"
    )
    return (fields + description).encode("utf-8")


def _render_md5sums(files: dict[str, tuple[bytes, int]]) -> bytes:
    lines = []
    for relative in sorted(files):
        content, _mode = files[relative]
        lines.append(f"{hashlib.md5(content).hexdigest()}  {relative}\n")
    return "".join(lines).encode("utf-8")


def _ar_archive(members: list[tuple[str, bytes]], epoch: int) -> bytes:
    chunks = [b"!<arch>\n"]
    for name, data in members:
        encoded_name = name.encode("ascii")
        if len(encoded_name) > 16:
            raise BuildError(f"ar member name is too long: {name}")
        header = (
            encoded_name.ljust(16, b" ")
            + str(epoch).encode("ascii").ljust(12, b" ")
            + b"0".ljust(6, b" ")
            + b"0".ljust(6, b" ")
            + b"100644".ljust(8, b" ")
            + str(len(data)).encode("ascii").ljust(10, b" ")
            + b"`\n"
        )
        if len(header) != 60:
            raise BuildError("internal error: invalid ar header length")
        chunks.append(header)
        chunks.append(data)
        if len(data) % 2:
            chunks.append(b"\n")
    return b"".join(chunks)


def build_deb(
    version: str,
    binary: Path,
    output_dir: Path,
    repo_root: Path | None = None,
    icon_path: Path | None = None,
) -> BuildResult:
    """Build the .deb and .sha256 sidecar, returning their final paths and hashes."""
    normalized_version = normalize_version(version)
    epoch = source_date_epoch()
    root = (repo_root or _default_repo_root()).resolve()
    icon = (icon_path or Path(__file__).resolve().with_name("assets") / "paint-rust.svg").resolve()

    if output_dir.exists() and not output_dir.is_dir():
        raise BuildError(f"output directory path is not a directory: {output_dir}")

    binary_bytes = read_elf_x86_64(binary)
    icon_bytes = read_first_party_icon(icon)
    readme = read_required(root / "README.md", "README.md")
    license_text = read_optional(root / "LICENSE", "LICENSE")
    desktop = DESKTOP_FILE.encode("utf-8")
    files = _payload_files(binary_bytes, icon_bytes, readme, license_text, desktop)
    installed_size = _installed_size(files)

    data_bytes = _tar_gz(_archive_entries(files), epoch)
    control_entries = [
        ("./", "dir", 0o755, None),
        ("./control", "file", 0o644, _render_control(normalized_version, installed_size)),
        ("./md5sums", "file", 0o644, _render_md5sums(files)),
    ]
    control_bytes = _tar_gz(control_entries, epoch)
    deb_bytes = _ar_archive(
        [
            ("debian-binary", b"2.0\n"),
            ("control.tar.gz", control_bytes),
            ("data.tar.gz", data_bytes),
        ],
        epoch,
    )
    digest = hashlib.sha256(deb_bytes).hexdigest()
    deb_name = f"{PACKAGE_NAME}_{normalized_version}_{ARCHITECTURE}.deb"
    sidecar_name = f"{deb_name}.sha256"
    sidecar_bytes = f"{digest}  {deb_name}\n".encode("utf-8")

    try:
        output_dir.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise BuildError(f"cannot create output directory {output_dir}: {error}") from error

    try:
        temporary_dir = Path(tempfile.mkdtemp(prefix=".paint-rust-deb-", dir=output_dir))
    except OSError as error:
        raise BuildError(f"cannot create a temporary directory in {output_dir}: {error}") from error
    try:
        temporary_deb = temporary_dir / deb_name
        temporary_sidecar = temporary_dir / sidecar_name
        temporary_deb.write_bytes(deb_bytes)
        temporary_sidecar.write_bytes(sidecar_bytes)
        temporary_deb.chmod(0o644)
        temporary_sidecar.chmod(0o644)
        final_deb = output_dir / deb_name
        final_sidecar = output_dir / sidecar_name
        os.replace(temporary_deb, final_deb)
        os.replace(temporary_sidecar, final_sidecar)
    except OSError as error:
        raise BuildError(f"cannot write package outputs in {output_dir}: {error}") from error
    finally:
        shutil.rmtree(temporary_dir, ignore_errors=True)

    return BuildResult(
        deb=final_deb,
        sidecar=final_sidecar,
        version=normalized_version,
        installed_size=installed_size,
        sha256=digest,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Build an unsigned, deterministic Paint.Rust x86_64 Debian package.",
    )
    parser.add_argument("--version", required=True, help="Debian version, e.g. 0.1.1")
    parser.add_argument("--binary", required=True, type=Path, help="paint-ui ELF x86_64 release binary")
    parser.add_argument("--output-dir", required=True, type=Path, help="directory for the .deb and .sha256")
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=None,
        help="repository root containing README.md and optional LICENSE",
    )
    parser.add_argument(
        "--icon",
        type=Path,
        default=None,
        help="first-party SVG icon (defaults to assets/paint-rust.svg next to this script)",
    )
    args = parser.parse_args(argv)
    try:
        result = build_deb(
            version=args.version,
            binary=args.binary,
            output_dir=args.output_dir,
            repo_root=args.repo_root,
            icon_path=args.icon,
        )
    except BuildError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"built {result.deb}")
    print(f"wrote {result.sidecar}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
