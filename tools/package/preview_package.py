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
import uuid
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence
from urllib.parse import quote

DEFAULT_EPOCH = 946_684_800  # 2000-01-01T00:00:00Z; valid for ZIP and tar.
PLATFORMS = {
    "windows-x86_64": {"binary": "paint-ui.exe", "suffix": ".zip"},
    "macos-aarch64": {"binary": "paint-ui", "suffix": ".zip"},
    "linux-x86_64": {"binary": "paint-ui", "suffix": ".tar.gz"},
}
NOTICE_TOKENS = ("license", "licence", "copying", "notice", "copyright")


class PackagingError(RuntimeError):
    """An expected validation, smoke, metadata, or packaging failure."""


@dataclass(frozen=True)
class PackageResult:
    archive: Path
    sidecar: Path
    version: str
    notice_extraction_complete: bool


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


def write_sha256_manifest(payload_root: Path) -> Path:
    """Hash every payload file except the manifest itself, in POSIX-path order."""
    records = [
        (path, relative)
        for path, relative in _payload_files(payload_root)
        if relative != "SHA256SUMS.txt"
    ]
    rendered = "".join(f"{_sha256_file(path)}  {relative}\n" for path, relative in records)
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


def collect_third_party_notices(metadata: dict[str, object], payload_root: Path) -> dict[str, object]:
    """Copy discoverable license/notice texts and explicitly inventory every gap.

    Discovery is heuristic and is not represented as legal verification. Public
    packaging remains blocked even if every package has a discovered file.
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
        entries.append(entry)

    extraction_complete = bool(entries) and all(
        entry["files"] and not entry["errors"] and entry["license_expression"] for entry in entries
    )
    index: dict[str, object] = {
        "format": "paint.r.third-party-notice-inventory",
        "version": 1,
        "verification": "heuristic-extraction-only-not-legal-review",
        "extraction_complete": extraction_complete,
        "package_count": len(entries),
        "packages_with_discovered_texts": sum(bool(entry["files"]) for entry in entries),
        "packages": entries,
    }
    index_path = payload_root / "third-party-notices/index.json"
    index_path.parent.mkdir(parents=True, exist_ok=True)
    index_path.write_bytes(_json_bytes(index))

    lines = [
        "# Third-party notices — internal development preview",
        "",
        "Paint.R has no selected public project license. This inventory does not grant one.",
        "This is heuristic extraction from locked Cargo package sources, **not a legal review**",
        "or a claim that redistribution obligations are satisfied. Public packaging is blocked.",
        "",
        f"Resolved third-party packages: {len(entries)}.",
        f"Packages with discovered notice texts: {index['packages_with_discovered_texts']}.",
        f"Heuristic file coverage complete: {'yes' if extraction_complete else 'no'}.",
        "The Cargo SBOM is a locked-workspace inventory, not an exact linked-binary or native-dependency audit.",
        "See `third-party-notices/index.json` for paths, source-relative names, hashes, and extraction errors.",
        "",
    ]
    for entry in entries:
        expression = entry["license_expression"] or "NO DECLARED LICENSE EXPRESSION"
        lines.append(f"## {entry['name']} {entry['version']}")
        lines.append("")
        lines.append(f"Cargo license expression: `{expression}`")
        lines.append("")
        if entry["files"]:
            for record in entry["files"]:
                lines.append(f"- [`{record['source_path']}`]({record['path']})")
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
        for key in ("unknown_command_error", "invalid_rect_error"):
            if not isinstance(record.get(key), str) or not record[key].strip():
                raise PackagingError(f"preview smoke negative-path evidence is missing for trace: {trace}")
        normalized.append(
            {
                "trace": _relative_to_repo(trace, repo_root),
                "status": "pass",
                "runs": runs,
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
    """Smoke the actual binary, then build an internal-only deterministic archive."""
    if distribution != "internal-preview":
        raise PackagingError(
            "public redistribution is blocked: the project license/asset rights are unresolved "
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
        if _sha256_file(staged_binary) != smoke_evidence["binary_sha256"]:
            raise PackagingError("paint-ui binary changed between preview smoke and packaging")
        readme = template_path.read_text(encoding="utf-8")
        readme = readme.replace("{{VERSION}}", version).replace("{{PLATFORM}}", platform)
        (payload / "README-PREVIEW.md").write_bytes(readme.encode("utf-8"))
        shutil.copyfile(parity_path, payload / "PARITY_LEDGER.md")
        (payload / "PREVIEW_SMOKE.json").write_bytes(_json_bytes(smoke_evidence))
        (payload / "sbom.cdx.json").write_bytes(_json_bytes(generate_sbom(metadata, epoch)))
        notice_index = collect_third_party_notices(metadata, payload)
        if platform == "macos-aarch64":
            resources = payload / "Paint.R.app/Contents/Resources/Preview"
            resources.mkdir(parents=True, exist_ok=True)
            for name in ("README-PREVIEW.md", "PARITY_LEDGER.md", "PREVIEW_SMOKE.json", "THIRD_PARTY_NOTICES.md", "sbom.cdx.json"):
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
        archive = output_dir / archive_name
        sidecar = output_dir / temporary_sidecar.name
        os.replace(temporary_archive, archive)
        os.replace(temporary_sidecar, sidecar)
    return PackageResult(
        archive=archive,
        sidecar=sidecar,
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
    print(f"locked Cargo/SBOM version: {result.version}")
    print(f"third-party notice extraction complete: {str(result.notice_extraction_complete).lower()}")
    print("public redistribution: blocked; no installer, signing, notarization, or release publication performed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
