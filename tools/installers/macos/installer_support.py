#!/usr/bin/env python3
"""Support code for the unsigned macOS development-preview installers.

This module is deliberately small and standard-library only. It performs the
parts of the installer build that are error-prone in shell: safe ZIP
extraction, app-bundle metadata rewriting, timestamp normalization, and
verification of the staged bundle and the finished component package.

It does not sign, notarize, or claim any public-release readiness.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ElementTree
import zipfile
from pathlib import Path, PurePosixPath
from typing import Iterable

DEFAULT_EPOCH = 946_684_800  # 2000-01-01T00:00:00Z, same default as the packager.
MIN_EPOCH = 315_532_800  # 1980-01-01T00:00:00Z
MAX_EPOCH = 4_354_732_799  # 2107-12-31T23:59:59Z
SOURCE_APP = "Paint.R.app"
TARGET_APP = "Paint.Rust.app"
BUNDLE_IDENTIFIER = "dev.paint-rust.app"
EXECUTABLE_RELATIVE = "Contents/MacOS/paint-ui"
INFO_PLIST_RELATIVE = "Contents/Info.plist"
DOCUMENTS = ("LICENSE", "README-PREVIEW.md")
VERSION_PATTERN = re.compile(r"[0-9A-Za-z][0-9A-Za-z._+-]*")


class InstallerError(RuntimeError):
    """An input or output violates the installer contract."""


def validate_version(version: str) -> str:
    if not version:
        raise InstallerError("version must not be empty")
    if len(version) > 128:
        raise InstallerError("version must be at most 128 characters")
    if not VERSION_PATTERN.fullmatch(version) or ".." in version:
        raise InstallerError(
            "version may contain only letters, digits, '.', '_', '+', and '-', "
            "must start with a letter or digit, and must not contain '..'"
        )
    return version


def epoch_from_environment() -> int:
    raw = os.environ.get("SOURCE_DATE_EPOCH")
    if raw is None:
        return DEFAULT_EPOCH
    try:
        epoch = int(raw, 10)
    except ValueError as error:
        raise InstallerError("SOURCE_DATE_EPOCH must be an integer") from error
    if not MIN_EPOCH <= epoch <= MAX_EPOCH:
        raise InstallerError("SOURCE_DATE_EPOCH must be between 1980-01-01 and 2107-12-31")
    return epoch


def _portable_mode(info: zipfile.ZipInfo, *, directory: bool) -> int:
    mode = info.external_attr >> 16
    permissions = stat.S_IMODE(mode) & 0o777
    if permissions == 0:
        return 0o755 if directory else 0o644
    if directory:
        return 0o755
    return permissions


def _zip_parts(info: zipfile.ZipInfo) -> tuple[PurePosixPath, tuple[str, ...], bool]:
    name = info.filename
    if not name or "\x00" in name or "\\" in name or ":" in name:
        raise InstallerError(f"unsafe ZIP entry name: {name!r}")
    if name.startswith("/"):
        raise InstallerError(f"absolute ZIP entry path: {name!r}")
    directory = info.is_dir() or name.endswith("/")
    pure = PurePosixPath(name)
    if pure.is_absolute() or not pure.parts:
        raise InstallerError(f"unsafe ZIP entry path: {name!r}")
    if any(part in ("", ".", "..") for part in pure.parts):
        raise InstallerError(f"unsafe ZIP entry path: {name!r}")
    normalized = pure.as_posix()
    if name not in (normalized, normalized + "/"):
        raise InstallerError(f"non-canonical ZIP entry path: {name!r}")
    if directory and name != normalized + "/":
        raise InstallerError(f"malformed ZIP directory entry: {name!r}")
    if not directory and name != normalized:
        raise InstallerError(f"malformed ZIP file entry: {name!r}")
    return pure, pure.parts, directory


def _app_location(
    entries: list[tuple[zipfile.ZipInfo, tuple[str, ...], bool]],
) -> tuple[tuple[str, ...], tuple[str, ...]]:
    roots: set[tuple[str, ...]] = set()
    prefixes: set[tuple[str, ...]] = set()
    for _, parts, _ in entries:
        matches = [index for index, part in enumerate(parts) if part == SOURCE_APP]
        if not matches:
            continue
        if len(matches) > 1:
            raise InstallerError(f"nested app bundle path is not supported: {'/'.join(parts)}")
        index = matches[0]
        if index not in (0, 1):
            raise InstallerError(
                "the app bundle must be at the ZIP root or inside exactly one top-level "
                f"payload directory: {'/'.join(parts)}"
            )
        roots.add(parts[: index + 1])
        prefixes.add(parts[:index])
    if not roots:
        raise InstallerError(f"input ZIP does not contain a top-level {SOURCE_APP}")
    if len(roots) != 1:
        raise InstallerError(f"input ZIP contains more than one {SOURCE_APP} bundle")
    return next(iter(roots)), next(iter(prefixes))


def _extract_app(
    archive: zipfile.ZipFile,
    entries: list[tuple[zipfile.ZipInfo, tuple[str, ...], bool]],
    app_root: tuple[str, ...],
    destination: Path,
) -> None:
    destination.mkdir(parents=True)
    for info, parts, directory in entries:
        if parts[: len(app_root)] != app_root:
            continue
        relative = parts[len(app_root) :]
        if not relative:
            continue
        target = destination.joinpath(*relative)
        if directory:
            target.mkdir(parents=True, exist_ok=True)
            target.chmod(0o755)
            continue
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info, "r") as source, target.open("wb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)
            target.chmod(_portable_mode(info, directory=False))
        except (OSError, zipfile.BadZipFile) as error:
            raise InstallerError(f"could not extract {info.filename!r}: {error}") from error


def _file_entries(
    entries: list[tuple[zipfile.ZipInfo, tuple[str, ...], bool]],
) -> dict[tuple[str, ...], zipfile.ZipInfo]:
    files: dict[tuple[str, ...], zipfile.ZipInfo] = {}
    for info, parts, directory in entries:
        if directory:
            continue
        files[parts] = info
    return files


def _copy_documents(
    archive: zipfile.ZipFile,
    files: dict[tuple[str, ...], zipfile.ZipInfo],
    app_root: tuple[str, ...],
    prefix: tuple[str, ...],
    docs_dir: Path,
) -> None:
    docs_dir.mkdir(parents=True)
    preview = app_root + ("Contents", "Resources", "Preview")
    for name in DOCUMENTS:
        candidates = (prefix + (name,), preview + (name,))
        info = next((files[candidate] for candidate in candidates if candidate in files), None)
        if info is None:
            continue
        try:
            data = archive.read(info)
        except (OSError, zipfile.BadZipFile) as error:
            raise InstallerError(f"could not read {info.filename!r}: {error}") from error
        target = docs_dir / name
        target.write_bytes(data)
        target.chmod(0o644)


def _rewrite_plist(app: Path) -> None:
    info_path = app / INFO_PLIST_RELATIVE
    if not info_path.is_file() or info_path.is_symlink():
        raise InstallerError(f"app bundle is missing regular file {INFO_PLIST_RELATIVE}")
    try:
        data = plistlib.loads(info_path.read_bytes())
    except (OSError, plistlib.InvalidFileException, ValueError) as error:
        raise InstallerError(f"could not read {INFO_PLIST_RELATIVE}: {error}") from error
    if not isinstance(data, dict):
        raise InstallerError(f"{INFO_PLIST_RELATIVE} must contain a dictionary")
    executable = data.get("CFBundleExecutable")
    if executable != "paint-ui":
        raise InstallerError(
            "Info.plist CFBundleExecutable must be 'paint-ui', "
            f"found {executable!r}"
        )
    mode = stat.S_IMODE(info_path.stat().st_mode)
    data["CFBundleName"] = "Paint.Rust"
    data["CFBundleDisplayName"] = "Paint.Rust"
    data["CFBundleIdentifier"] = BUNDLE_IDENTIFIER
    data["CFBundleExecutable"] = "paint-ui"
    try:
        info_path.write_bytes(plistlib.dumps(data, fmt=plistlib.FMT_XML, sort_keys=True))
    except (OSError, TypeError, ValueError) as error:
        raise InstallerError(f"could not write {INFO_PLIST_RELATIVE}: {error}") from error
    info_path.chmod(mode)


def _normalize_path(path: Path, epoch: int) -> None:
    try:
        os.utime(path, (epoch, epoch), follow_symlinks=False)
    except (OSError, NotImplementedError) as error:
        raise InstallerError(f"could not normalize {path}: {error}") from error


def _normalize_tree(root: Path) -> None:
    epoch = epoch_from_environment()
    for directory, dirnames, filenames in os.walk(root, topdown=False, followlinks=False):
        for name in filenames:
            _normalize_path(Path(directory) / name, epoch)
        for name in dirnames:
            _normalize_path(Path(directory) / name, epoch)
        _normalize_path(Path(directory), epoch)
    _normalize_path(root, epoch)


def check_app(app: Path) -> None:
    if not app.is_dir() or app.is_symlink():
        raise InstallerError(f"app bundle is missing or is not a directory: {app}")
    info_path = app / INFO_PLIST_RELATIVE
    if not info_path.is_file() or info_path.is_symlink():
        raise InstallerError(f"app bundle is missing regular file {INFO_PLIST_RELATIVE}")
    try:
        data = plistlib.loads(info_path.read_bytes())
    except (OSError, plistlib.InvalidFileException, ValueError) as error:
        raise InstallerError(f"could not read {INFO_PLIST_RELATIVE}: {error}") from error
    if not isinstance(data, dict):
        raise InstallerError(f"{INFO_PLIST_RELATIVE} must contain a dictionary")
    expected = {
        "CFBundleName": "Paint.Rust",
        "CFBundleDisplayName": "Paint.Rust",
        "CFBundleIdentifier": BUNDLE_IDENTIFIER,
        "CFBundleExecutable": "paint-ui",
    }
    for key, value in expected.items():
        if data.get(key) != value:
            raise InstallerError(f"Info.plist {key} must be {value!r}, found {data.get(key)!r}")
    executable = app / EXECUTABLE_RELATIVE
    if not executable.is_file() or executable.is_symlink():
        raise InstallerError(f"app bundle is missing regular file {EXECUTABLE_RELATIVE}")
    if not os.access(executable, os.X_OK):
        raise InstallerError(f"app executable is not executable: {EXECUTABLE_RELATIVE}")


def install_text(version: str) -> str:
    return (
        f"Paint.Rust {version} - macOS (Apple silicon) development preview\n"
        "===================================================================\n"
        "\n"
        "This is an unsigned, unnotarized development-preview build for internal\n"
        "evaluation. It is not a public or production release.\n"
        "\n"
        "To install:\n"
        "  1. Open this disk image.\n"
        "  2. Drag Paint.Rust.app onto the Applications shortcut.\n"
        "  3. Eject the disk image, then open Paint.Rust from Applications.\n"
        "\n"
        "Gatekeeper may warn that the app is from an unidentified developer\n"
        "because it is unsigned. Do not disable Gatekeeper; use normal security\n"
        "judgement or build from source.\n"
        "\n"
        "Package layout:\n"
        "  Paint.Rust.app   Application bundle (CFBundleExecutable: paint-ui)\n"
        "  Applications     Shortcut to /Applications\n"
        "\n"
        "The .pkg installer in the same output directory installs Paint.Rust.app\n"
        "to /Applications. This build makes no code-signing, notarization, or\n"
        "auto-update claim. See README-PREVIEW.md and LICENSE when provided.\n"
    )


def prepare(zip_path: Path, work: Path, version: str) -> None:
    validate_version(version)
    epoch_from_environment()
    if not zip_path.is_file():
        raise InstallerError(f"input ZIP is missing: {zip_path}")
    if not work.is_dir():
        raise InstallerError(f"work directory is missing: {work}")
    try:
        archive = zipfile.ZipFile(zip_path)
    except (OSError, zipfile.BadZipFile) as error:
        raise InstallerError(f"could not open input ZIP: {error}") from error
    with archive:
        entries: list[tuple[zipfile.ZipInfo, tuple[str, ...], bool]] = []
        seen: set[str] = set()
        for info in archive.infolist():
            path, parts, directory = _zip_parts(info)
            key = path.as_posix().casefold()
            if key in seen:
                raise InstallerError(f"duplicate ZIP entry path: {info.filename!r}")
            seen.add(key)
            if info.flag_bits & 0x1:
                raise InstallerError(f"encrypted ZIP entries are not supported: {info.filename!r}")
            mode = info.external_attr >> 16
            kind = stat.S_IFMT(mode)
            if kind not in (0, stat.S_IFREG, stat.S_IFDIR):
                raise InstallerError(f"non-regular ZIP entry is not supported: {info.filename!r}")
            if directory and kind == stat.S_IFREG:
                raise InstallerError(f"ZIP entry is both a file and a directory: {info.filename!r}")
            if not directory and kind == stat.S_IFDIR:
                raise InstallerError(f"ZIP entry is both a directory and a file: {info.filename!r}")
            entries.append((info, parts, directory))
        app_root, prefix = _app_location(entries)
        unpacked = work / "unpacked"
        raw_app = unpacked / SOURCE_APP
        _extract_app(archive, entries, app_root, raw_app)
        files = _file_entries(entries)
        _copy_documents(archive, files, app_root, prefix, work / "docs")
    target_app = work / TARGET_APP
    if target_app.exists():
        raise InstallerError(f"work directory already contains {TARGET_APP}")
    try:
        os.replace(raw_app, target_app)
    except OSError as error:
        raise InstallerError(f"could not rename {SOURCE_APP} to {TARGET_APP}: {error}") from error
    shutil.rmtree(unpacked, ignore_errors=True)
    _rewrite_plist(target_app)
    check_app(target_app)
    _normalize_tree(target_app)
    _normalize_tree(work / "docs")
    install_path = work / "INSTALL.txt"
    install_path.write_text(install_text(version), encoding="utf-8")
    install_path.chmod(0o644)
    epoch = epoch_from_environment()
    try:
        os.utime(install_path, (epoch, epoch))
    except OSError as error:
        raise InstallerError(f"could not normalize timestamp for INSTALL.txt: {error}") from error


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _xml_package_info(path: Path) -> dict[str, str] | None:
    try:
        root = ElementTree.parse(path).getroot()
    except (OSError, ElementTree.ParseError):
        return None
    if root.tag != "pkg-info":
        return None
    return {str(key): str(value) for key, value in root.attrib.items()}


def verify_pkg(pkg: Path, version: str, work: Path, source_app: Path) -> None:
    validate_version(version)
    if not pkg.is_file() or pkg.stat().st_size == 0:
        raise InstallerError(f"package is missing or empty: {pkg}")
    if not work.is_dir():
        raise InstallerError(f"work directory is missing: {work}")
    if not source_app.is_dir():
        raise InstallerError(f"source app bundle is missing: {source_app}")
    with tempfile.TemporaryDirectory(prefix="pkg-expand-", dir=work) as temporary:
        expanded = Path(temporary) / "expanded"
        try:
            result = subprocess.run(
                ["pkgutil", "--expand-full", str(pkg), str(expanded)],
                check=False,
                capture_output=True,
                text=True,
            )
        except OSError as error:
            raise InstallerError(f"could not run pkgutil: {error}") from error
        if result.returncode != 0:
            detail = (result.stderr or result.stdout).strip()
            raise InstallerError(f"pkgutil could not expand the package: {detail}")
        matches: list[dict[str, str]] = []
        for candidate in sorted(expanded.rglob("PackageInfo")):
            data = _xml_package_info(candidate)
            if data is not None and data.get("identifier") == BUNDLE_IDENTIFIER:
                matches.append(data)
        if len(matches) != 1:
            raise InstallerError(
                "package must contain exactly one PackageInfo with identifier "
                f"{BUNDLE_IDENTIFIER}, found {len(matches)}"
            )
        package_info = matches[0]
        if str(package_info.get("version")) != version:
            raise InstallerError(
                f"package version must be {version!r}, found {package_info.get('version')!r}"
            )
        if package_info.get("install-location") != "/Applications":
            raise InstallerError(
                "package install-location must be /Applications, "
                f"found {package_info.get('install-location')!r}"
            )
        distributions = sorted(expanded.rglob("Distribution"))
        if len(distributions) != 1:
            raise InstallerError(
                f"package must contain exactly one Distribution, found {len(distributions)}"
            )
        try:
            distribution = ElementTree.parse(distributions[0]).getroot()
        except (OSError, ElementTree.ParseError) as error:
            raise InstallerError(f"could not read package Distribution: {error}") from error
        options = distribution.find("options")
        if distribution.tag != "installer-gui-script" or options is None:
            raise InstallerError("package Distribution is missing installer options")
        if options.get("hostArchitectures") != "arm64":
            raise InstallerError(
                "package Distribution hostArchitectures must be arm64, "
                f"found {options.get('hostArchitectures')!r}"
            )
        version_refs = [
            element
            for element in distribution.findall("pkg-ref")
            if element.get("id") == BUNDLE_IDENTIFIER and element.get("version") == version
        ]
        if not version_refs:
            raise InstallerError(
                f"package Distribution is missing pkg-ref {BUNDLE_IDENTIFIER} version {version!r}"
            )
        payload_executables = sorted(
            path
            for path in expanded.rglob("paint-ui")
            if path.is_file() and path.parent.name == "MacOS"
        )
        if len(payload_executables) != 1:
            raise InstallerError(
                "package payload must contain exactly one Contents/MacOS/paint-ui, "
                f"found {len(payload_executables)}"
            )
        payload_executable = payload_executables[0]
        payload_parts = payload_executable.relative_to(expanded).parts
        expected_tail = (TARGET_APP, "Contents", "MacOS", "paint-ui")
        if payload_parts[-4:] != expected_tail or SOURCE_APP in payload_parts:
            raise InstallerError(
                "package payload must contain Paint.Rust.app/Contents/MacOS/paint-ui, "
                f"found {'/'.join(payload_parts)}"
            )
        if _sha256(payload_executable) != _sha256(source_app / EXECUTABLE_RELATIVE):
            raise InstallerError("package payload executable differs from the staged app executable")
        payload_info = payload_executable.parents[1] / "Info.plist"
        if not payload_info.is_file():
            raise InstallerError("package payload is missing Contents/Info.plist")
        if _sha256(payload_info) != _sha256(source_app / INFO_PLIST_RELATIVE):
            raise InstallerError("package payload Info.plist differs from the staged app metadata")


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)

    prepare_parser = commands.add_parser("prepare", help="extract and normalize the app bundle")
    prepare_parser.add_argument("--zip", required=True, type=Path)
    prepare_parser.add_argument("--work", required=True, type=Path)
    prepare_parser.add_argument("--version", required=True)

    normalize_parser = commands.add_parser("normalize", help="set tree timestamps to SOURCE_DATE_EPOCH")
    normalize_parser.add_argument("--root", required=True, type=Path)

    check_parser = commands.add_parser("check-app", help="verify the staged app bundle metadata")
    check_parser.add_argument("--app", required=True, type=Path)

    verify_parser = commands.add_parser("verify-pkg", help="expand and verify a built package")
    verify_parser.add_argument("--pkg", required=True, type=Path)
    verify_parser.add_argument("--version", required=True)
    verify_parser.add_argument("--work", required=True, type=Path)
    verify_parser.add_argument("--source-app", required=True, type=Path)
    return parser


def main(argv: Iterable[str] | None = None) -> int:
    args = _build_parser().parse_args(argv)
    try:
        if args.command == "prepare":
            prepare(args.zip, args.work, args.version)
        elif args.command == "normalize":
            _normalize_tree(args.root)
        elif args.command == "check-app":
            check_app(args.app)
        elif args.command == "verify-pkg":
            verify_pkg(args.pkg, args.version, args.work, args.source_app)
        else:  # pragma: no cover - argparse enforces the command set.
            raise InstallerError(f"unknown command: {args.command}")
    except InstallerError as error:
        print(f"installer: {error}", file=sys.stderr)
        return 1
    except OSError as error:
        print(f"installer: unexpected filesystem error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
