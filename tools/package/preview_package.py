#!/usr/bin/env python3
"""Deterministic internal-development-preview packaging for the real paint-ui binary.

This tool intentionally has no skip-smoke option and refuses public redistribution.
It uses only the Python standard library.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import uuid
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Mapping, Sequence
from urllib.parse import quote, urlsplit

DEFAULT_EPOCH = 946_684_800  # 2000-01-01T00:00:00Z; valid for ZIP and tar.
PLATFORMS = {
    "windows-x86_64": {
        "binary": "paint-ui.exe",
        "suffix": ".zip",
        "format": "PE",
        "machine": 0x8664,
        "architecture": "x86_64",
    },
    "macos-aarch64": {
        "binary": "paint-ui",
        "suffix": ".zip",
        "format": "Mach-O 64",
        "machine": 0x0100000C,
        "architecture": "aarch64",
    },
    "linux-x86_64": {
        "binary": "paint-ui",
        "suffix": ".tar.gz",
        "format": "ELF 64",
        "machine": 62,
        "architecture": "x86_64",
    },
}
NOTICE_TOKENS = ("license", "licence", "copying", "notice", "copyright")


class PackagingError(RuntimeError):
    """An expected validation, smoke, metadata, or packaging failure."""


@dataclass(frozen=True)
class PackageResult:
    archive: Path
    sidecar: Path
    verification: Path
    version: str
    notice_extraction_complete: bool


@dataclass(frozen=True)
class CuratedNoticeFile:
    path: PurePosixPath
    source_path: PurePosixPath
    source_url: str
    sha256: str
    content: bytes


@dataclass(frozen=True)
class CuratedNoticePackage:
    checksum: str
    repository: str
    vcs_commit: str
    path_in_vcs: str
    files: tuple[CuratedNoticeFile, ...]


@dataclass(frozen=True)
class NoticeSupplement:
    packages: Mapping[tuple[str, str, str], CuratedNoticePackage]


def _json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _epoch_from_environment() -> int:
    raw = os.environ.get("SOURCE_DATE_EPOCH")
    if raw is None:
        return DEFAULT_EPOCH
    try:
        epoch = int(raw, 10)
    except ValueError as error:
        raise PackagingError("SOURCE_DATE_EPOCH must be an integer") from error
    # ZIP's representation ends in 2107; tar and JSON can use the same bound.
    if not 315_532_800 <= epoch <= 4_354_732_799:
        raise PackagingError("SOURCE_DATE_EPOCH must be between 1980-01-01 and 2107-12-31")
    return epoch


def _timestamp(epoch: int) -> str:
    return datetime.fromtimestamp(epoch, timezone.utc).isoformat().replace("+00:00", "Z")


def _payload_files(root: Path) -> list[tuple[Path, str]]:
    if not root.is_dir():
        raise PackagingError(f"payload directory is missing: {root}")
    records: list[tuple[Path, str]] = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PackagingError(f"payload symlinks are not supported: {path}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise PackagingError(f"payload entry is not a regular file: {path}")
        records.append((path, path.relative_to(root).as_posix()))
    records.sort(key=lambda record: record[1])
    if not records:
        raise PackagingError("refusing to package an empty payload")
    return records


def _portable_mode(path: Path) -> int:
    return 0o755 if path.stat().st_mode & 0o111 else 0o644


def create_zip_archive(payload_root: Path, destination: Path, epoch: int = DEFAULT_EPOCH) -> None:
    """Create a sorted ZIP with fixed timestamps, modes, and no host-specific extras."""
    if not 315_532_800 <= epoch <= 4_354_732_799:
        raise PackagingError("ZIP timestamp is outside its supported range")
    files = _payload_files(payload_root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    zip_time = datetime.fromtimestamp(epoch, timezone.utc).timetuple()[:6]
    with zipfile.ZipFile(
        destination,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        allowZip64=True,
        compresslevel=9,
    ) as archive:
        archive.comment = b""
        for path, relative in files:
            info = zipfile.ZipInfo(f"{payload_root.name}/{relative}", date_time=zip_time)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.extra = b""
            info.comment = b""
            info.internal_attr = 0
            info.external_attr = (stat.S_IFREG | _portable_mode(path)) << 16
            with path.open("rb") as source, archive.open(info, mode="w") as target:
                shutil.copyfileobj(source, target, length=1024 * 1024)


def create_tar_gz_archive(payload_root: Path, destination: Path, epoch: int = DEFAULT_EPOCH) -> None:
    """Create a sorted tar.gz with fixed gzip/tar metadata and portable ownership."""
    if epoch < 0:
        raise PackagingError("tar timestamp cannot be negative")
    files = _payload_files(payload_root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=epoch) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for path, relative in files:
                    info = tarfile.TarInfo(f"{payload_root.name}/{relative}")
                    info.size = path.stat().st_size
                    info.mtime = epoch
                    info.mode = _portable_mode(path)
                    info.type = tarfile.REGTYPE
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.linkname = ""
                    info.pax_headers = {}
                    with path.open("rb") as source:
                        archive.addfile(info, source)


def _render_sha256_manifest(payload_root: Path) -> str:
    records = [
        (path, relative)
        for path, relative in _payload_files(payload_root)
        if relative != "SHA256SUMS.txt"
    ]
    return "".join(f"{_sha256_file(path)}  {relative}\n" for path, relative in records)


def write_sha256_manifest(payload_root: Path) -> Path:
    """Hash every payload file except the manifest itself, in POSIX-path order."""
    rendered = _render_sha256_manifest(payload_root)
    manifest = payload_root / "SHA256SUMS.txt"
    manifest.write_bytes(rendered.encode("utf-8"))
    manifest.chmod(0o644)
    return manifest


def write_archive_sidecar(archive: Path) -> Path:
    sidecar = archive.with_name(archive.name + ".sha256")
    sidecar.write_bytes(f"{_sha256_file(archive)}  {archive.name}\n".encode("utf-8"))
    return sidecar


def _package_purl(record: dict[str, object]) -> str:
    name = quote(str(record["name"]), safe="")
    version = quote(str(record["version"]), safe="")
    return f"pkg:cargo/{name}@{version}"


def generate_sbom(metadata: dict[str, object], epoch: int = DEFAULT_EPOCH) -> dict[str, object]:
    """Translate locked Cargo metadata into a deterministic CycloneDX 1.6 inventory.

    The locked workspace graph is deliberately labeled as a superset of the files
    linked into paint-ui; license expressions are metadata, not compliance claims.
    """
    packages = metadata.get("packages")
    if not isinstance(packages, list) or not packages:
        raise PackagingError("locked Cargo metadata contains no packages")
    workspace = set(metadata.get("workspace_members", []))
    by_id: dict[str, dict[str, object]] = {}
    references: dict[str, str] = {}
    owners: dict[str, str] = {}
    for record in packages:
        if not isinstance(record, dict) or not all(key in record for key in ("id", "name", "version")):
            raise PackagingError("locked Cargo metadata has an invalid package record")
        package_id = str(record["id"])
        reference = _package_purl(record)
        if reference in owners and owners[reference] != package_id:
            raise PackagingError(f"duplicate Cargo package URL in SBOM: {reference}")
        by_id[package_id] = record
        references[package_id] = reference
        owners[reference] = package_id
    roots = [record for record in packages if record.get("name") == "paint-ui"]
    if len(roots) != 1:
        raise PackagingError("locked Cargo metadata must contain exactly one paint-ui package")

    components: list[dict[str, object]] = []
    for package_id, record in by_id.items():
        licenses = []
        if record.get("license"):
            licenses.append({"expression": str(record["license"])})
        component: dict[str, object] = {
            "type": "application" if record.get("name") == "paint-ui" else "library",
            "bom-ref": references[package_id],
            "name": str(record["name"]),
            "version": str(record["version"]),
            "scope": "required",
            "purl": references[package_id],
            "properties": [
                {"name": "paint.r:workspace-member", "value": "true" if package_id in workspace else "false"}
            ],
        }
        if licenses:
            component["licenses"] = licenses
        components.append(component)
    components.sort(key=lambda component: (str(component["name"]), str(component["version"]), str(component["bom-ref"])))
    root_id = str(roots[0]["id"])
    root_component = next(component for component in components if component["bom-ref"] == references[root_id])

    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
        raise PackagingError("locked Cargo metadata has no resolved dependency graph")
    nodes = resolve["nodes"]
    node_by_id = {str(node["id"]): node for node in nodes if isinstance(node, dict) and "id" in node}
    dependencies: list[dict[str, object]] = []
    for package_id in sorted(by_id, key=lambda item: references[item]):
        node = node_by_id.get(package_id, {})
        depends_on = sorted(
            {references[str(dependency["pkg"])] for dependency in node.get("deps", []) if str(dependency.get("pkg")) in references}
        )
        dependencies.append({"ref": references[package_id], "dependsOn": depends_on})
    dependencies.sort(key=lambda dependency: str(dependency["ref"]))

    identity = {
        "components": components,
        "dependencies": dependencies,
        "timestamp": _timestamp(epoch),
    }
    serial = uuid.uuid5(uuid.NAMESPACE_URL, json.dumps(identity, sort_keys=True, separators=(",", ":")))
    return {
        "$schema": "https://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        "metadata": {
            "timestamp": _timestamp(epoch),
            "component": root_component,
            "properties": [
                {"name": "paint.r:distribution", "value": "internal-development-preview"},
                {"name": "paint.r:inventory-scope", "value": "locked-workspace-graph-not-exact-linked-binary"},
                {"name": "paint.r:license-data-source", "value": "cargo-metadata-declarations-not-legal-verification"},
            ],
        },
        "components": components,
        "dependencies": dependencies,
    }


def _safe_component(value: str) -> str:
    rendered = re.sub(r"[^A-Za-z0-9._-]+", "_", value).strip("._") or "package"
    # Keep deep notice destinations practical under legacy Windows path limits.
    return rendered[:40]


def _is_notice_candidate(path: Path, root: Path) -> bool:
    name = path.name.casefold()
    if any(token in name for token in NOTICE_TOKENS) or name in {"ofl", "ofl.txt", "ufl", "ufl.txt"}:
        return True
    relative = path.relative_to(root)
    parent_names = {part.casefold() for part in relative.parts[:-1]}
    return "fonts" in parent_names and path.suffix.casefold() in {".txt", ".md"}


def _notice_candidates(root: Path, record: dict[str, object]) -> list[Path]:
    candidates: set[Path] = set()
    license_file = record.get("license_file")
    if license_file:
        explicit = (root / str(license_file)).resolve()
        try:
            explicit.relative_to(root.resolve())
        except ValueError as error:
            raise PackagingError(f"license_file escapes package source for {record.get('name')}") from error
        candidates.add(explicit)
    if root.is_dir():
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink() and _is_notice_candidate(path, root):
                candidates.add(path)
    return sorted(candidates, key=lambda path: path.relative_to(root).as_posix())


def _exact_record(value: object, fields: set[str], label: str) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != fields:
        raise PackagingError(f"{label} has invalid fields")
    return value


def _supplement_relative_path(value: object, label: str) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or ":" in value or "\x00" in value:
        raise PackagingError(f"unsafe supplement file path in {label}")
    path = PurePosixPath(value)
    if path.is_absolute() or path.as_posix() != value or any(part in ("", ".", "..") for part in path.parts):
        raise PackagingError(f"unsafe supplement file path in {label}")
    return path


def _sha256_value(value: object, label: str) -> str:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise PackagingError(f"{label} must be a lowercase SHA-256 digest")
    return value


def load_locked_package_checksums(lockfile: Path) -> dict[tuple[str, str, str], str]:
    """Return exact registry/package checksums from the locked Cargo graph."""
    try:
        with lockfile.open("rb") as source:
            locked = tomllib.load(source)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise PackagingError(f"could not read Cargo.lock for notice provenance: {error}") from error
    records = locked.get("package")
    if not isinstance(records, list):
        raise PackagingError("Cargo.lock has no package records for notice provenance")
    checksums: dict[tuple[str, str, str], str] = {}
    for record in records:
        if not isinstance(record, dict) or not isinstance(record.get("name"), str) or not isinstance(record.get("version"), str):
            raise PackagingError("Cargo.lock has an invalid package record for notice provenance")
        source = record.get("source")
        checksum = record.get("checksum")
        if source is None and checksum is None:
            continue
        if not isinstance(source, str) or not source:
            raise PackagingError("Cargo.lock checksummed package has no exact source")
        identity = (record["name"], record["version"], source)
        digest = _sha256_value(checksum, f"Cargo.lock checksum for {record['name']} {record['version']}")
        if identity in checksums:
            raise PackagingError(f"duplicate Cargo.lock package identity for notice provenance: {identity}")
        checksums[identity] = digest
    return checksums


def load_notice_supplement(root: Path) -> NoticeSupplement:
    """Load and hash curated notice bytes; never retrieve or synthesize text.

    Every source URL is checked again against the consuming package's exact VCS
    commit. Curated bytes are retained so later emission cannot observe changed
    source files between validation and packaging.
    """
    index_path = root / "index.json"
    try:
        manifest = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        raise PackagingError(f"could not read curated notice supplement: {error}") from error
    manifest = _exact_record(manifest, {"format", "version", "files", "packages"}, "notice supplement")
    if manifest["format"] != "paint.r.notice-provenance-supplement" or type(manifest["version"]) is not int:
        raise PackagingError("notice supplement has an unsupported format or version")
    if manifest["version"] != 1 or not isinstance(manifest["files"], list) or not isinstance(manifest["packages"], list):
        raise PackagingError("notice supplement has an unsupported format or version")

    resolved_root = root.resolve()
    resources: dict[str, CuratedNoticeFile] = {}
    for raw_resource in manifest["files"]:
        resource = _exact_record(
            raw_resource,
            {"id", "path", "source_path", "source_url", "sha256"},
            "notice supplement file",
        )
        resource_id = resource["id"]
        if not isinstance(resource_id, str) or not resource_id or resource_id in resources:
            raise PackagingError("notice supplement file id is empty or duplicated")
        relative = _supplement_relative_path(resource["path"], f"file {resource_id}")
        source_path = _supplement_relative_path(resource["source_path"], f"source_path for {resource_id}")
        source_url = resource["source_url"]
        if not isinstance(source_url, str):
            raise PackagingError(f"notice supplement source URL for {resource_id} is invalid")
        parsed_url = urlsplit(source_url)
        if (
            parsed_url.scheme != "https"
            or not parsed_url.hostname
            or parsed_url.username is not None
            or parsed_url.password is not None
            or parsed_url.query
            or parsed_url.fragment
        ):
            raise PackagingError(f"notice supplement source URL for {resource_id} must be immutable HTTPS")
        digest = _sha256_value(resource["sha256"], f"notice supplement hash for {resource_id}")
        source_file = resolved_root.joinpath(*relative.parts)
        try:
            source_file.resolve().relative_to(resolved_root)
        except ValueError as error:
            raise PackagingError(f"unsafe supplement file path in file {resource_id}") from error
        cursor = resolved_root
        for part in relative.parts:
            cursor = cursor / part
            if cursor.is_symlink():
                raise PackagingError(f"unsafe supplement file path in file {resource_id}: symbolic link")
        if not source_file.is_file():
            raise PackagingError(f"curated notice supplement file is unavailable: {relative.as_posix()}")
        try:
            content = source_file.read_bytes()
        except OSError as error:
            raise PackagingError(f"could not read curated notice supplement file {resource_id}: {error}") from error
        if not content or hashlib.sha256(content).hexdigest() != digest:
            raise PackagingError(f"curated notice supplement SHA-256 mismatch for {resource_id}")
        resources[resource_id] = CuratedNoticeFile(relative, source_path, source_url, digest, content)

    packages: dict[tuple[str, str, str], CuratedNoticePackage] = {}
    used_resources: set[str] = set()
    for raw_package in manifest["packages"]:
        record = _exact_record(
            raw_package,
            {
                "name",
                "version",
                "source",
                "checksum",
                "repository",
                "vcs_commit",
                "path_in_vcs",
                "file_ids",
            },
            "notice supplement package",
        )
        string_fields = ("name", "version", "source", "repository", "vcs_commit", "path_in_vcs")
        if any(not isinstance(record[key], str) for key in string_fields):
            raise PackagingError("notice supplement package identity has invalid strings")
        if any(not record[key] for key in ("name", "version", "source", "repository", "vcs_commit")):
            raise PackagingError("notice supplement package identity has an empty field")
        if not str(record["source"]).startswith("registry+"):
            raise PackagingError("notice supplement packages must have an exact registry source")
        repository_url = urlsplit(str(record["repository"]))
        if repository_url.scheme != "https" or not repository_url.hostname or repository_url.query or repository_url.fragment:
            raise PackagingError("notice supplement package repository must be an HTTPS URL")
        commit = str(record["vcs_commit"])
        if re.fullmatch(r"[0-9a-f]{40}", commit) is None:
            raise PackagingError("notice supplement VCS commit must be a full lowercase Git commit")
        checksum = _sha256_value(record["checksum"], f"notice supplement checksum for {record['name']}")
        raw_ids = record["file_ids"]
        if (
            not isinstance(raw_ids, list)
            or not raw_ids
            or any(not isinstance(resource_id, str) for resource_id in raw_ids)
            or len(set(raw_ids)) != len(raw_ids)
        ):
            raise PackagingError(f"notice supplement file ids are invalid for {record['name']} {record['version']}")
        identity = (str(record["name"]), str(record["version"]), str(record["source"]))
        if identity in packages:
            raise PackagingError(f"duplicate notice supplement package identity: {identity}")
        selected: list[CuratedNoticeFile] = []
        for resource_id in raw_ids:
            resource = resources.get(resource_id)
            if resource is None:
                raise PackagingError(f"notice supplement references unknown file id: {resource_id}")
            parsed_url = urlsplit(resource.source_url)
            expected_suffix = (commit, *resource.source_path.parts)
            url_parts = tuple(part for part in parsed_url.path.split("/") if part)
            if len(url_parts) < len(expected_suffix) or url_parts[-len(expected_suffix) :] != expected_suffix:
                raise PackagingError(
                    f"notice supplement source URL for {resource_id} must be commit-pinned to {commit} "
                    "and end at its source_path"
                )
            selected.append(resource)
            used_resources.add(resource_id)
        packages[identity] = CuratedNoticePackage(
            checksum=checksum,
            repository=str(record["repository"]),
            vcs_commit=commit,
            path_in_vcs=str(record["path_in_vcs"]),
            files=tuple(selected),
        )
    if used_resources != set(resources):
        raise PackagingError("notice supplement contains unreferenced files")
    return NoticeSupplement(packages)


def _apply_notice_supplement(
    *,
    entry: dict[str, object],
    record: dict[str, object],
    root: Path,
    destination_key: str,
    payload_root: Path,
    supplement: NoticeSupplement,
    locked_checksums: Mapping[tuple[str, str, str], str],
) -> None:
    name = str(entry["name"])
    version = str(entry["version"])
    identity = (name, version, str(record.get("source") or ""))
    curated = supplement.packages.get(identity)
    if curated is None:
        return
    errors = entry["errors"]
    assert isinstance(errors, list)
    actual_checksum = locked_checksums.get(identity)
    if actual_checksum != curated.checksum:
        errors.append("curated notice supplement rejected: exact Cargo.lock checksum is unavailable or mismatched")
        return
    if str(record.get("repository") or "") != curated.repository:
        errors.append("curated notice supplement rejected: package repository is unavailable or mismatched")
        return
    try:
        vcs = json.loads((root / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError) as error:
        errors.append(f"curated notice supplement rejected: .cargo_vcs_info.json is unavailable or invalid: {error}")
        return
    git = vcs.get("git") if isinstance(vcs, dict) else None
    actual_commit = git.get("sha1") if isinstance(git, dict) else None
    actual_path = vcs.get("path_in_vcs", "") if isinstance(vcs, dict) else None
    if actual_commit != curated.vcs_commit or actual_path != curated.path_in_vcs:
        errors.append("curated notice supplement rejected: exact VCS commit or package path is mismatched")
        return

    file_records = entry["files"]
    assert isinstance(file_records, list)
    for resource in curated.files:
        filename = f"{len(file_records):03d}-{_safe_component(resource.source_path.name)}"
        destination = Path("third-party-notices/files") / destination_key / filename
        target = payload_root / destination
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(resource.content)
        target.chmod(0o644)
        emitted_digest = _sha256_file(target)
        if emitted_digest != resource.sha256 or target.read_bytes() != resource.content:
            raise PackagingError(f"emitted curated notice SHA-256 mismatch for {name} {version}: {resource.source_path}")
        file_records.append(
            {
                "path": destination.as_posix(),
                "source_path": resource.source_path.as_posix(),
                "source_kind": "curated-upstream-vcs",
                "source_url": resource.source_url,
                "vcs_commit": curated.vcs_commit,
                "sha256": emitted_digest,
            }
        )
    entry["provenance"] = {
        "kind": "curated-upstream-vcs",
        "registry_source": identity[2],
        "cargo_checksum": actual_checksum,
        "repository": curated.repository,
        "vcs_commit": curated.vcs_commit,
        "path_in_vcs": curated.path_in_vcs,
    }


def collect_third_party_notices(
    metadata: dict[str, object],
    payload_root: Path,
    *,
    supplement: NoticeSupplement | None = None,
    locked_checksums: Mapping[tuple[str, str, str], str] | None = None,
) -> dict[str, object]:
    """Copy discoverable or exactly curated license/notice texts and inventory gaps.

    Discovery is heuristic and curated retrieval is not represented as legal
    verification. Public packaging remains blocked even with complete file coverage.
    """
    workspace = set(metadata.get("workspace_members", []))
    packages = [
        record
        for record in metadata.get("packages", [])
        if isinstance(record, dict) and str(record.get("id")) not in workspace
    ]
    packages.sort(key=lambda record: (str(record.get("name")), str(record.get("version")), str(record.get("id"))))
    entries: list[dict[str, object]] = []
    used_keys: set[str] = set()
    for record in packages:
        name = str(record.get("name"))
        version = str(record.get("version"))
        source = str(record.get("source") or "")
        identity = f"{name}@{version}\n{source}"
        key_base = _safe_component(f"{name}-{version}")
        key = f"{key_base}-{hashlib.sha256(identity.encode('utf-8')).hexdigest()[:10]}"
        if key in used_keys:
            raise PackagingError(f"notice destination collision for {name} {version}")
        used_keys.add(key)
        manifest_path = record.get("manifest_path")
        root = Path(str(manifest_path)).parent if manifest_path else None
        entry: dict[str, object] = {
            "name": name,
            "version": version,
            "license_expression": record.get("license"),
            "files": [],
            "errors": [],
        }
        if root is None:
            entry["errors"].append("package manifest path is unavailable")
        elif not root.is_dir():
            entry["errors"].append("package source directory is unavailable")
        else:
            for source_path in _notice_candidates(root, record):
                try:
                    relative = source_path.relative_to(root)
                except ValueError:
                    entry["errors"].append("notice candidate escapes package source")
                    continue
                if not source_path.is_file():
                    entry["errors"].append(f"declared notice is unavailable: {relative.as_posix()}")
                    continue
                file_records = entry["files"]
                assert isinstance(file_records, list)
                filename = f"{len(file_records):03d}-{_safe_component(source_path.name)}"
                destination = Path("third-party-notices/files") / key / filename
                target = payload_root / destination
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source_path, target)
                target.chmod(0o644)
                file_records.append(
                    {
                        "path": destination.as_posix(),
                        "source_path": relative.as_posix(),
                        "sha256": _sha256_file(target),
                    }
                )
            if supplement is not None and not entry["files"] and not entry["errors"]:
                _apply_notice_supplement(
                    entry=entry,
                    record=record,
                    root=root,
                    destination_key=key,
                    payload_root=payload_root,
                    supplement=supplement,
                    locked_checksums=locked_checksums or {},
                )
        entries.append(entry)

    extraction_complete = bool(entries) and all(
        entry["files"] and not entry["errors"] and entry["license_expression"] for entry in entries
    )
    index: dict[str, object] = {
        "format": "paint.r.third-party-notice-inventory",
        "version": 1,
        "verification": "heuristic-and-curated-upstream-extraction-not-legal-review",
        "extraction_complete": extraction_complete,
        "package_count": len(entries),
        "packages_with_discovered_texts": sum(bool(entry["files"]) for entry in entries),
        "packages_with_curated_supplements": sum("provenance" in entry for entry in entries),
        "packages": entries,
    }
    index_path = payload_root / "third-party-notices/index.json"
    index_path.parent.mkdir(parents=True, exist_ok=True)
    index_path.write_bytes(_json_bytes(index))

    lines = [
        "# Third-party notices — internal development preview",
        "",
        "Paint.R first-party code is licensed under the MIT License (see `LICENSE`).",
        "This inventory covers third-party components and does not relicense them.",
        "Package-source discovery is heuristic; curated supplements contain hash-verified bytes from",
        "commit-pinned upstream URLs. This is **not a legal review** or a claim that redistribution",
        "obligations are satisfied. Cargo license expressions are metadata, not license texts.",
        "Public packaging is blocked.",
        "",
        f"Resolved third-party packages: {len(entries)}.",
        f"Packages with package-source or curated notice texts: {index['packages_with_discovered_texts']}.",
        f"Packages with curated upstream supplements: {index['packages_with_curated_supplements']}.",
        f"Notice text file coverage complete: {'yes' if extraction_complete else 'no'}.",
        "The Cargo SBOM is a locked-workspace inventory, not an exact linked-binary or native-dependency audit.",
        "See `third-party-notices/index.json` for paths, sources, hashes, provenance, and extraction errors.",
        "",
    ]
    for entry in entries:
        expression = entry["license_expression"] or "NO DECLARED LICENSE EXPRESSION"
        lines.append(f"## {entry['name']} {entry['version']}")
        lines.append("")
        lines.append(f"Cargo license expression: `{expression}`")
        lines.append("")
        if "provenance" in entry:
            provenance = entry["provenance"]
            lines.append(
                f"Curated provenance: Cargo checksum `{provenance['cargo_checksum']}`; "
                f"VCS commit `{provenance['vcs_commit']}`; package path `{provenance['path_in_vcs'] or '.'}`."
            )
            lines.append("")
        if entry["files"]:
            for record in entry["files"]:
                upstream = f" — [immutable upstream]({record['source_url']})" if "source_url" in record else ""
                lines.append(f"- [`{record['source_path']}`]({record['path']}){upstream}")
        else:
            lines.append("- **No license/notice text discovered; redistribution review remains required.**")
        for error in entry["errors"]:
            lines.append(f"- Extraction warning: {error}")
        lines.append("")
    (payload_root / "THIRD_PARTY_NOTICES.md").write_bytes("\n".join(lines).encode("utf-8"))
    return index


def _relative_to_repo(path: Path, repo_root: Path) -> str:
    try:
        return path.resolve().relative_to(repo_root.resolve()).as_posix()
    except ValueError as error:
        raise PackagingError(f"trace must be inside the repository: {path}") from error


def normalize_smoke_report(
    report: dict[str, object],
    traces: Sequence[Path],
    binary: Path,
    repo_root: Path,
    repeat: int,
) -> dict[str, object]:
    """Retain deterministic evidence only after the real runner reports success."""
    if report.get("status") != "pass":
        raise PackagingError("preview smoke did not return an overall pass status")
    records = report.get("traces")
    if not isinstance(records, list) or len(records) != len(traces):
        raise PackagingError("preview smoke report does not cover every requested trace")
    by_trace: dict[Path, dict[str, object]] = {}
    for record in records:
        if not isinstance(record, dict) or "trace" not in record:
            raise PackagingError("preview smoke report has an invalid trace record")
        by_trace[Path(str(record["trace"])).resolve()] = record
    normalized: list[dict[str, object]] = []
    for trace in traces:
        record = by_trace.get(trace.resolve())
        if record is None or record.get("status") != "pass":
            raise PackagingError(f"preview smoke did not pass trace: {trace}")
        runs = record.get("runs")
        if not isinstance(runs, list) or len(runs) != repeat or not all(isinstance(run, dict) for run in runs):
            raise PackagingError(f"preview smoke repeat evidence is invalid for trace: {trace}")
        run_fields = {"width", "height", "decoded_rgba_sha256", "png_sha256", "snapshot_sha256", "snapshot"}
        hash_fields = ("decoded_rgba_sha256", "png_sha256", "snapshot_sha256")
        snapshot_fields = {"layer_count", "active_layer_index", "dirty", "can_undo", "can_redo"}
        normalized_runs: list[dict[str, object]] = []
        for run in runs:
            if set(run) != run_fields:
                raise PackagingError(f"preview smoke run evidence has invalid fields for trace: {trace}")
            if any(
                not isinstance(run[key], str) or re.fullmatch(r"[0-9a-f]{64}", run[key]) is None
                for key in hash_fields
            ):
                raise PackagingError(f"preview smoke run hashes are invalid for trace: {trace}")
            if any(type(run[key]) is not int or run[key] <= 0 for key in ("width", "height")):
                raise PackagingError(f"preview smoke run dimensions are invalid for trace: {trace}")
            snapshot = run["snapshot"]
            if not isinstance(snapshot, dict) or set(snapshot) != snapshot_fields:
                raise PackagingError(f"preview smoke snapshot evidence is invalid for trace: {trace}")
            if type(snapshot["layer_count"]) is not int or snapshot["layer_count"] < 1:
                raise PackagingError(f"preview smoke snapshot layer count is invalid for trace: {trace}")
            if (
                type(snapshot["active_layer_index"]) is not int
                or not 0 <= snapshot["active_layer_index"] < snapshot["layer_count"]
            ):
                raise PackagingError(f"preview smoke active layer is invalid for trace: {trace}")
            if any(type(snapshot[key]) is not bool for key in ("dirty", "can_undo", "can_redo")):
                raise PackagingError(f"preview smoke snapshot history metadata is invalid for trace: {trace}")
            if run != runs[0]:
                raise PackagingError(f"preview smoke repeated evidence is not deterministic for trace: {trace}")
            normalized_runs.append(
                {
                    "width": run["width"],
                    "height": run["height"],
                    "decoded_rgba_sha256": run["decoded_rgba_sha256"],
                    "png_sha256": run["png_sha256"],
                    "snapshot_sha256": run["snapshot_sha256"],
                    "snapshot": {
                        key: snapshot[key]
                        for key in ("layer_count", "active_layer_index", "dirty", "can_undo", "can_redo")
                    },
                }
            )
        for key in ("unknown_command_error", "invalid_rect_error"):
            if not isinstance(record.get(key), str) or not record[key].strip():
                raise PackagingError(f"preview smoke negative-path evidence is missing for trace: {trace}")
        normalized.append(
            {
                "trace": _relative_to_repo(trace, repo_root),
                "status": "pass",
                "runs": normalized_runs,
                "negative_paths": {"unknown_command": "pass", "invalid_rectangle": "pass"},
            }
        )
    return {
        "format": "paint.r.preview-smoke-evidence",
        "version": 1,
        "status": "pass",
        "runner": "tests/e2e/preview_smoke.py",
        "binary_sha256": _sha256_file(binary),
        "repeat": repeat,
        "traces": normalized,
    }


def run_preview_smoke(
    *,
    binary: Path,
    traces: Sequence[Path],
    smoke_script: Path,
    repeat: int,
    timeout: float,
    repo_root: Path,
) -> dict[str, object]:
    """Run the repository's actual black-box smoke; any failure is fatal."""
    binary = binary.resolve()
    smoke_script = smoke_script.resolve()
    if not binary.is_file():
        raise PackagingError(f"binary is missing: {binary}")
    if not smoke_script.is_file():
        raise PackagingError(f"preview smoke runner is missing: {smoke_script}")
    if not traces or repeat < 1 or timeout <= 0:
        raise PackagingError("at least one trace, a positive repeat count, and a positive timeout are required")
    resolved_traces = [trace.resolve() for trace in traces]
    for trace in resolved_traces:
        if not trace.is_file():
            raise PackagingError(f"preview trace is missing: {trace}")
        _relative_to_repo(trace, repo_root)
    with tempfile.TemporaryDirectory(prefix="paint-r-real-smoke-") as temporary:
        report_path = Path(temporary) / "smoke-report.json"
        command = [sys.executable, str(smoke_script), "--binary", str(binary)]
        for trace in resolved_traces:
            command.extend(("--trace", str(trace)))
        command.extend(("--repeat", str(repeat), "--timeout", str(timeout), "--report", str(report_path)))
        overall_timeout = max(30.0, timeout * len(resolved_traces) * (repeat + 2) + 10.0)
        try:
            completed = subprocess.run(
                command,
                cwd=repo_root,
                text=True,
                capture_output=True,
                check=False,
                timeout=overall_timeout,
            )
        except (OSError, subprocess.SubprocessError) as error:
            raise PackagingError(f"could not execute preview smoke: {error}") from error
        if completed.returncode != 0:
            diagnostic = (completed.stderr.strip() or completed.stdout.strip() or "no diagnostic")[:4000]
            raise PackagingError(f"preview smoke failed with exit code {completed.returncode}: {diagnostic}")
        if not report_path.is_file():
            raise PackagingError("preview smoke returned success without writing its report")
        try:
            report = json.loads(report_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            raise PackagingError(f"preview smoke report is unreadable: {error}") from error
        if not isinstance(report, dict):
            raise PackagingError("preview smoke report is not a JSON object")
        return normalize_smoke_report(report, resolved_traces, binary, repo_root, repeat)


def load_locked_metadata(cargo_command: Sequence[str], repo_root: Path) -> dict[str, object]:
    if not cargo_command:
        raise PackagingError("Cargo metadata command cannot be empty")
    command = [*cargo_command, "metadata", "--locked", "--format-version", "1"]
    try:
        completed = subprocess.run(
            command,
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=False,
            timeout=120.0,
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise PackagingError(f"could not run locked Cargo metadata: {error}") from error
    if completed.returncode != 0:
        diagnostic = (completed.stderr.strip() or completed.stdout.strip() or "no diagnostic")[:4000]
        raise PackagingError(f"locked Cargo metadata failed with exit code {completed.returncode}: {diagnostic}")
    try:
        metadata = json.loads(completed.stdout)
    except ValueError as error:
        raise PackagingError(f"locked Cargo metadata is not valid JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise PackagingError("locked Cargo metadata is not a JSON object")
    return metadata


def inspect_binary_architecture(binary: Path, platform: str) -> dict[str, object]:
    """Return executable-header evidence and reject a mislabeled target machine.

    This identifies the file's declared format/machine only. It does not prove native
    execution, runner architecture, code signing, or application behavior.
    """
    if platform not in PLATFORMS:
        raise PackagingError(f"unsupported preview platform: {platform}")
    if not binary.is_file():
        raise PackagingError(f"binary is missing: {binary}")
    expected = PLATFORMS[platform]
    format_name = str(expected["format"])
    try:
        with binary.open("rb") as source:
            if format_name == "PE":
                dos = source.read(64)
                if len(dos) != 64 or dos[:2] != b"MZ":
                    raise PackagingError(f"binary format mismatch for {platform}: expected PE with an MZ header")
                pe_offset = int.from_bytes(dos[0x3C:0x40], "little")
                if pe_offset < 64:
                    raise PackagingError(f"binary format mismatch for {platform}: invalid PE header offset")
                source.seek(pe_offset)
                header = source.read(24)
                if len(header) != 24 or header[:4] != b"PE\0\0":
                    raise PackagingError(f"binary format mismatch for {platform}: expected a complete PE/COFF header")
                machine = int.from_bytes(header[4:6], "little")
            elif format_name == "Mach-O 64":
                header = source.read(8)
                if len(header) != 8 or int.from_bytes(header[:4], "little") != 0xFEEDFACF:
                    raise PackagingError(f"binary format mismatch for {platform}: expected a thin 64-bit Mach-O header")
                machine = int.from_bytes(header[4:8], "little")
            else:
                header = source.read(20)
                if (
                    len(header) != 20
                    or header[:4] != b"\x7fELF"
                    or header[4] != 2
                    or header[5] != 1
                ):
                    raise PackagingError(f"binary format mismatch for {platform}: expected a little-endian 64-bit ELF header")
                machine = int.from_bytes(header[18:20], "little")
        size = binary.stat().st_size
        digest = _sha256_file(binary)
    except OSError as error:
        raise PackagingError(f"could not inspect binary architecture for {platform}: {error}") from error
    expected_machine = int(expected["machine"])
    if machine != expected_machine:
        raise PackagingError(
            f"binary architecture mismatch for {platform}: expected {format_name} machine "
            f"0x{expected_machine:04x}, got 0x{machine:04x}"
        )
    return {
        "architecture": str(expected["architecture"]),
        "bytes": size,
        "format": format_name,
        "machine": f"0x{machine:04x}",
        "sha256": digest,
        "scope": "executable-header-only-not-native-execution-proof",
    }


def stage_platform_files(platform: str, version: str, binary: Path, payload_root: Path) -> None:
    if platform not in PLATFORMS:
        raise PackagingError(f"unsupported preview platform: {platform}")
    if not binary.is_file():
        raise PackagingError(f"binary is missing: {binary}")
    payload_root.mkdir(parents=True, exist_ok=True)
    if platform == "macos-aarch64":
        contents = payload_root / "Paint.R.app/Contents"
        executable = contents / "MacOS/paint-ui"
        executable.parent.mkdir(parents=True, exist_ok=True)
        (contents / "Resources").mkdir(parents=True, exist_ok=True)
        info = {
            "CFBundleDevelopmentRegion": "en",
            "CFBundleExecutable": "paint-ui",
            "CFBundleIdentifier": "dev.paint-r.internal-preview",
            "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleName": "Paint.R Development Preview",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
        }
        (contents / "Info.plist").write_bytes(plistlib.dumps(info, fmt=plistlib.FMT_XML, sort_keys=True))
    else:
        executable = payload_root / str(PLATFORMS[platform]["binary"])
    shutil.copyfile(binary, executable)
    executable.chmod(0o755)


def _extract_verified_archive(
    archive: Path,
    platform: str,
    payload_root: Path,
    destination: Path,
    epoch: int,
) -> Path:
    """Extract only the exact staged payload, preserving and checking every byte/mode."""
    expected = {relative: path for path, relative in _payload_files(payload_root)}
    extracted_root = destination / payload_root.name
    extracted_root.mkdir()

    def archive_relative(name: str) -> str:
        pure = PurePosixPath(name)
        if (
            not name
            or pure.is_absolute()
            or pure.as_posix() != name
            or "\\" in name
            or ":" in name
            or "\x00" in name
            or len(pure.parts) < 2
            or pure.parts[0] != payload_root.name
            or any(part in ("", ".", "..") for part in pure.parts)
        ):
            raise PackagingError(f"unsafe archive entry path: {name!r}")
        return PurePosixPath(*pure.parts[1:]).as_posix()

    def write_entry(relative: str, data: bytes, mode: int) -> None:
        source = expected[relative]
        if mode != _portable_mode(source):
            raise PackagingError(f"archive executable/file mode mismatch for {relative}")
        if len(data) != source.stat().st_size or data != source.read_bytes():
            raise PackagingError(f"archived payload bytes differ from staged payload: {relative}")
        target = extracted_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        target.chmod(mode)

    try:
        if platform == "linux-x86_64":
            with tarfile.open(archive, "r:gz") as source:
                members: dict[str, tarfile.TarInfo] = {}
                for member in source.getmembers():
                    relative = archive_relative(member.name)
                    if not member.isfile() or relative in members:
                        raise PackagingError(f"archive contains a duplicate or non-regular entry: {member.name}")
                    if (
                        member.uid != 0
                        or member.gid != 0
                        or member.mtime != epoch
                        or member.uname
                        or member.gname
                    ):
                        raise PackagingError(f"archive ownership/timestamp mismatch for {member.name}")
                    members[relative] = member
                if set(members) != set(expected):
                    raise PackagingError("archive payload entries differ from the staged payload")
                for relative in sorted(members):
                    member = members[relative]
                    if member.size != expected[relative].stat().st_size:
                        raise PackagingError(f"archive payload size mismatch for {relative}")
                    extracted = source.extractfile(member)
                    if extracted is None:
                        raise PackagingError(f"archive payload is unreadable: {relative}")
                    write_entry(relative, extracted.read(), member.mode)
        else:
            with zipfile.ZipFile(archive) as source:
                entries: dict[str, zipfile.ZipInfo] = {}
                modes: dict[str, int] = {}
                for entry in source.infolist():
                    relative = archive_relative(entry.filename)
                    mode = entry.external_attr >> 16
                    if entry.is_dir() or not stat.S_ISREG(mode) or relative in entries:
                        raise PackagingError(f"archive contains a duplicate or non-regular entry: {entry.filename}")
                    entries[relative] = entry
                    modes[relative] = mode & 0o777
                if set(entries) != set(expected):
                    raise PackagingError("archive payload entries differ from the staged payload")
                for relative in sorted(entries):
                    entry = entries[relative]
                    if entry.file_size != expected[relative].stat().st_size:
                        raise PackagingError(f"archive payload size mismatch for {relative}")
                    write_entry(relative, source.read(entry), modes[relative])
        archived_manifest = (extracted_root / "SHA256SUMS.txt").read_text(encoding="utf-8")
        if (
            archived_manifest != _render_sha256_manifest(payload_root)
            or archived_manifest != _render_sha256_manifest(extracted_root)
        ):
            raise PackagingError("archive SHA-256 manifest mismatch after extraction")
    except (OSError, EOFError, UnicodeError, zipfile.BadZipFile, tarfile.TarError) as error:
        raise PackagingError(f"archive verification failed: {error}") from error
    return extracted_root


def verify_packaged_archive(
    *,
    archive: Path,
    sidecar: Path,
    payload_root: Path,
    platform: str,
    traces: Sequence[Path],
    smoke_script: Path,
    repo_root: Path,
    smoke_evidence: dict[str, object],
    repeat: int,
    timeout: float,
    epoch: int,
) -> dict[str, object]:
    """Verify final archive bytes and smoke the extracted executable before publication."""
    try:
        archive_sha256 = _sha256_file(archive)
        expected_sidecar = f"{archive_sha256}  {archive.name}\n"
        if sidecar.read_text(encoding="utf-8") != expected_sidecar:
            raise PackagingError("archive sidecar does not match the archive bytes")
        archive_bytes = archive.stat().st_size
    except (OSError, UnicodeError) as error:
        raise PackagingError(f"archive sidecar verification failed: {error}") from error

    with tempfile.TemporaryDirectory(prefix="paint-r-archive-verify-") as temporary:
        extracted_root = _extract_verified_archive(archive, platform, payload_root, Path(temporary), epoch)
        binary_relative = (
            "Paint.R.app/Contents/MacOS/paint-ui"
            if platform == "macos-aarch64"
            else str(PLATFORMS[platform]["binary"])
        )
        extracted_binary = extracted_root / binary_relative
        if platform != "windows-x86_64" and extracted_binary.stat().st_mode & 0o111 == 0:
            raise PackagingError("extracted executable is not executable")
        extracted_evidence = inspect_binary_architecture(extracted_binary, platform)
        staged_evidence = inspect_binary_architecture(payload_root / binary_relative, platform)
        if extracted_evidence != staged_evidence:
            raise PackagingError("extracted executable identity differs from the staged binary")
        post_smoke = run_preview_smoke(
            binary=extracted_binary,
            traces=traces,
            smoke_script=smoke_script,
            repeat=repeat,
            timeout=timeout,
            repo_root=repo_root,
        )
        if post_smoke != smoke_evidence:
            raise PackagingError("post-archive smoke evidence differs from pre-archive evidence")

    return {
        "format": "paint.r.preview-package-verification",
        "version": 1,
        "status": "pass",
        "archive_name": archive.name,
        "archive_bytes": archive_bytes,
        "archive_sha256": archive_sha256,
        "payload_entries": len(_payload_files(payload_root)),
        "binary_sha256": extracted_evidence["sha256"],
        "architecture": extracted_evidence["architecture"],
        "post_extract_smoke": {"status": "pass", "repeat": repeat, "trace_count": len(traces)},
        "scope": "archive-sidecar-payload-modes-and-headless-only-no-native-window",
    }


def package_release(
    *,
    repo_root: Path,
    binary: Path,
    platform: str,
    output_dir: Path,
    traces: Sequence[Path],
    repeat: int = 2,
    timeout: float = 30.0,
    cargo_command: Sequence[str] = ("./tools/cargo-pinned.sh",),
    distribution: str = "internal-preview",
) -> PackageResult:
    """Build, reopen, and smoke an internal-only deterministic archive before returning it."""
    if distribution != "internal-preview":
        raise PackagingError(
            "public redistribution is blocked: third-party asset/notice rights are unresolved "
            "and third-party notice extraction is not legally verified"
        )
    if platform not in PLATFORMS:
        raise PackagingError(f"unsupported preview platform: {platform}")
    repo_root = repo_root.resolve()
    binary = binary.resolve()
    if not binary.is_file():
        raise PackagingError(f"binary is missing: {binary}")
    expected_binary = str(PLATFORMS[platform]["binary"])
    if binary.name != expected_binary:
        raise PackagingError(f"expected paint-ui binary named {expected_binary}, got {binary.name}")
    binary_evidence = inspect_binary_architecture(binary, platform)
    selected_traces = [Path(trace) for trace in traces]
    if not selected_traces:
        selected_traces = sorted((repo_root / "tests/e2e/traces").glob("*.json"))
    smoke_evidence = run_preview_smoke(
        binary=binary,
        traces=selected_traces,
        smoke_script=repo_root / "tests/e2e/preview_smoke.py",
        repeat=repeat,
        timeout=timeout,
        repo_root=repo_root,
    )
    metadata = load_locked_metadata(cargo_command, repo_root)
    paint_ui = [
        record for record in metadata.get("packages", []) if isinstance(record, dict) and record.get("name") == "paint-ui"
    ]
    if len(paint_ui) != 1:
        raise PackagingError("locked Cargo metadata must contain exactly one paint-ui package")
    version = str(paint_ui[0]["version"])
    epoch = _epoch_from_environment()
    root_name = f"paint-r-{version}-{platform}"
    archive_name = root_name + str(PLATFORMS[platform]["suffix"])
    template_path = repo_root / "packaging/INTERNAL_PREVIEW.md"
    parity_path = repo_root / "docs/PARITY_LEDGER.md"
    if not template_path.is_file() or not parity_path.is_file():
        raise PackagingError("preview README template or parity ledger is missing")

    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".paint-r-package-", dir=output_dir) as temporary:
        work = Path(temporary)
        payload = work / root_name
        stage_platform_files(platform, version, binary, payload)
        staged_binary = (
            payload / "Paint.R.app/Contents/MacOS/paint-ui"
            if platform == "macos-aarch64"
            else payload / expected_binary
        )
        staged_evidence = inspect_binary_architecture(staged_binary, platform)
        if staged_evidence != binary_evidence or staged_evidence["sha256"] != smoke_evidence["binary_sha256"]:
            raise PackagingError("paint-ui binary changed between architecture inspection, preview smoke, and packaging")
        readme = template_path.read_text(encoding="utf-8")
        readme = readme.replace("{{VERSION}}", version).replace("{{PLATFORM}}", platform)
        (payload / "README-PREVIEW.md").write_bytes(readme.encode("utf-8"))
        shutil.copyfile(parity_path, payload / "PARITY_LEDGER.md")
        license_path = repo_root / "LICENSE"
        if license_path.is_file():
            shutil.copyfile(license_path, payload / "LICENSE")
        lucide_license = repo_root / "docs/licenses/LUCIDE-ISC.txt"
        if lucide_license.is_file():
            shutil.copyfile(lucide_license, payload / "LUCIDE-ISC.txt")
        (payload / "PREVIEW_SMOKE.json").write_bytes(_json_bytes(smoke_evidence))
        (payload / "PREVIEW_BINARY.json").write_bytes(
            _json_bytes(
                {
                    "format": "paint.r.preview-binary-evidence",
                    "version": 1,
                    "platform": platform,
                    "binary": staged_evidence,
                }
            )
        )
        (payload / "sbom.cdx.json").write_bytes(_json_bytes(generate_sbom(metadata, epoch)))
        supplement_root = repo_root / "tools/package/notice_supplement"
        supplement = load_notice_supplement(supplement_root) if supplement_root.exists() else None
        locked_checksums = load_locked_package_checksums(repo_root / "Cargo.lock") if supplement is not None else {}
        notice_index = collect_third_party_notices(
            metadata, payload, supplement=supplement, locked_checksums=locked_checksums
        )
        if platform == "macos-aarch64":
            resources = payload / "Paint.R.app/Contents/Resources/Preview"
            resources.mkdir(parents=True, exist_ok=True)
            for name in ("README-PREVIEW.md", "PARITY_LEDGER.md", "LICENSE", "LUCIDE-ISC.txt", "PREVIEW_SMOKE.json", "PREVIEW_BINARY.json", "THIRD_PARTY_NOTICES.md", "sbom.cdx.json"):
                if not (payload / name).is_file():
                    continue
                shutil.copyfile(payload / name, resources / name)
        for path, _ in _payload_files(payload):
            if path.stat().st_mode & 0o111 == 0:
                path.chmod(0o644)
        write_sha256_manifest(payload)
        temporary_archive = work / archive_name
        if platform == "linux-x86_64":
            create_tar_gz_archive(payload, temporary_archive, epoch)
        else:
            create_zip_archive(payload, temporary_archive, epoch)
        temporary_sidecar = write_archive_sidecar(temporary_archive)
        verification_report = verify_packaged_archive(
            archive=temporary_archive,
            sidecar=temporary_sidecar,
            payload_root=payload,
            platform=platform,
            traces=selected_traces,
            smoke_script=repo_root / "tests/e2e/preview_smoke.py",
            repo_root=repo_root,
            smoke_evidence=smoke_evidence,
            repeat=repeat,
            timeout=timeout,
            epoch=epoch,
        )
        temporary_verification = work / f"{archive_name}.verification.json"
        temporary_verification.write_bytes(_json_bytes(verification_report))
        temporary_verification.chmod(0o644)
        archive = output_dir / archive_name
        sidecar = output_dir / temporary_sidecar.name
        verification = output_dir / temporary_verification.name
        os.replace(temporary_archive, archive)
        os.replace(temporary_sidecar, sidecar)
        os.replace(temporary_verification, verification)
    return PackageResult(
        archive=archive,
        sidecar=sidecar,
        verification=verification,
        version=version,
        notice_extraction_complete=bool(notice_index["extraction_complete"]),
    )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="actual release paint-ui executable")
    parser.add_argument("--platform", choices=sorted(PLATFORMS), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--trace", type=Path, action="append", help="repository trace; defaults to every tests/e2e/traces/*.json")
    parser.add_argument("--repeat", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument(
        "--distribution",
        choices=("internal-preview", "public"),
        default="internal-preview",
        help="public is deliberately refused while licensing and notice verification remain unresolved",
    )
    parser.add_argument(
        "--cargo-command",
        nargs="+",
        default=["./tools/cargo-pinned.sh"],
        help="command used for locked Cargo metadata; put this option last (Windows: bash ./tools/cargo-pinned.sh)",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    arguments = _parser().parse_args(argv)
    repo_root = Path(__file__).resolve().parents[2]
    try:
        result = package_release(
            repo_root=repo_root,
            binary=arguments.binary,
            platform=arguments.platform,
            output_dir=arguments.output_dir,
            traces=arguments.trace or [],
            repeat=arguments.repeat,
            timeout=arguments.timeout,
            cargo_command=arguments.cargo_command,
            distribution=arguments.distribution,
        )
    except (PackagingError, OSError, ValueError) as error:
        print(f"preview packaging failed: {error}", file=sys.stderr)
        return 1
    print(f"internal preview archive: {result.archive}")
    print(f"archive SHA-256: {result.sidecar}")
    print(f"post-archive verification: {result.verification}")
    print(f"locked Cargo/SBOM version: {result.version}")
    print(f"third-party notice extraction complete: {str(result.notice_extraction_complete).lower()}")
    print("public redistribution: blocked; no installer, signing, notarization, or release publication performed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
