#!/usr/bin/env python3
"""Validate repeated Paint.NET Invert observations without trusting summary flags."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

from paintnet_identity import PaintNetIdentityError, require_reviewed_paintnet


class ValidationError(ValueError):
    """One or more A/A evidence invariants failed."""


def _sha256(value: Any, label: str, errors: list[str]) -> str:
    if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
        errors.append(f"{label} must be a lowercase SHA-256 hex digest")
        return ""
    return value


def validate_manifest(
    manifest: dict[str, Any],
    minimum_runs: int = 3,
    *,
    environment: dict[str, Any],
) -> dict[str, Any]:
    errors: list[str] = []
    try:
        paintnet_identity = require_reviewed_paintnet(environment)
    except PaintNetIdentityError as error:
        paintnet_identity = None
        errors.append(str(error))
    if minimum_runs < 2:
        raise ValueError("minimum_runs must be at least two")
    if manifest.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    if manifest.get("kind") != "paintnet-invert-aa":
        errors.append("kind must be paintnet-invert-aa")
    observations = manifest.get("observations")
    if not isinstance(observations, list):
        raise ValidationError("observations must be a list")
    if len(observations) < minimum_runs:
        errors.append(f"requires at least {minimum_runs} observations; found {len(observations)}")

    encoded_hashes: list[str] = []
    rgba_hashes: list[str] = []
    for index, observation in enumerate(observations, start=1):
        label = f"observation {index}"
        if not isinstance(observation, dict):
            errors.append(f"{label} is not an object")
            continue
        if observation.get("run") != index:
            errors.append(f"{label} has non-sequential run {observation.get('run')!r}")
        document = observation.get("document", {})
        if document.get("width") != 800 or document.get("height") != 600:
            errors.append(f"{label} is not an 800x600 document")
        if document.get("operation") != "Invert Colors":
            errors.append(f"{label} does not identify Invert Colors")
        if document.get("source") != "new document":
            errors.append(f"{label} does not establish a new-document source")
        if paintnet_identity is not None:
            observed_title = document.get("paintnet_window_title_version")
            if observed_title != paintnet_identity["display_version"]:
                errors.append(
                    f"{label} paintnet_window_title_version is {observed_title!r}; "
                    f"expected observed {paintnet_identity['display_version']!r}"
                )
        guest = observation.get("guest", {})
        host = observation.get("host", {})
        guest_encoded = _sha256(guest.get("sha256"), f"{label} guest.sha256", errors)
        host_encoded = _sha256(host.get("sha256"), f"{label} host.sha256", errors)
        guest_png = guest.get("png", {})
        host_png = host.get("png", {})
        guest_rgba = _sha256(guest_png.get("rgba_sha256"), f"{label} guest rgba_sha256", errors)
        host_rgba = _sha256(host_png.get("rgba_sha256"), f"{label} host rgba_sha256", errors)
        if not guest_encoded or guest_encoded != host_encoded:
            errors.append(f"{label} guest/host encoded hashes differ")
        if not guest_rgba or guest_rgba != host_rgba:
            errors.append(f"{label} guest/host decoded RGBA hashes differ")
        comparison = observation.get("hash_comparison", {})
        if comparison.get("guest_host_encoded_equal") is not (guest_encoded == host_encoded and bool(guest_encoded)):
            errors.append(f"{label} encoded comparison flag does not match recomputation")
        if comparison.get("guest_host_rgba_equal") is not (guest_rgba == host_rgba and bool(guest_rgba)):
            errors.append(f"{label} RGBA comparison flag does not match recomputation")
        expected_fields = {
            "width": 800,
            "height": 600,
            "decoded_rgba_bytes": 1_920_000,
            "pixel_count": 480_000,
            "row_stride_bytes": 800 * 4,
            "all_pixels_equal": True,
            "first_pixel_rgba": [0, 0, 0, 255],
            "unique_color_count": 1,
        }
        for field, expected in expected_fields.items():
            if host_png.get(field) != expected:
                errors.append(f"{label} host.png.{field} is {host_png.get(field)!r}, expected {expected!r}")
        colors = host_png.get("colors")
        if colors != [{"rgba": [0, 0, 0, 255], "count": 480_000}]:
            errors.append(f"{label} does not contain the exact all-black RGBA histogram")
        independent = host_png.get("independent_decoder")
        if independent is not None and independent.get("rgba_matches_primary") is not True:
            errors.append(f"{label} independent host decoder disagrees")
        encoded_hashes.append(host_encoded)
        rgba_hashes.append(host_rgba)

    encoded_repeatable = bool(encoded_hashes) and all(value and value == encoded_hashes[0] for value in encoded_hashes)
    rgba_repeatable = bool(rgba_hashes) and all(value and value == rgba_hashes[0] for value in rgba_hashes)
    if not rgba_repeatable:
        errors.append("decoded RGBA is not identical across observations")
    result = {
        "schema_version": 1,
        "status": "FAIL" if errors else "PASS",
        "observation_count": len(observations),
        "minimum_observation_count": minimum_runs,
        "encoded_bytes_repeatable": encoded_repeatable,
        "decoded_rgba_repeatable": rgba_repeatable,
        "encoded_sha256": encoded_hashes[0] if encoded_repeatable else None,
        "rgba_sha256": rgba_hashes[0] if rgba_repeatable else None,
        "errors": errors,
        "note": "Encoded-byte identity is reported, not required; exact decoded RGBA identity and per-run guest/host agreement are required.",
    }
    if errors:
        raise ValidationError(json.dumps(result, indent=2, sort_keys=True))
    return result


def _manifest_environment(manifest_path: Path, manifest: dict[str, Any]) -> dict[str, Any]:
    record = manifest.get("environment")
    if not isinstance(record, dict) or not isinstance(record.get("path"), str):
        raise ValidationError("environment.path must identify the collected environment JSON")
    root = manifest_path.resolve().parent
    environment_path = (root / record["path"]).resolve()
    if not environment_path.is_relative_to(root):
        raise ValidationError("environment artifact escapes the run directory")
    environment_bytes = environment_path.read_bytes()
    observed_hash = hashlib.sha256(environment_bytes).hexdigest()
    if record.get("sha256") != observed_hash:
        raise ValidationError(
            f"environment.sha256 is {record.get('sha256')!r}; recomputed {observed_hash!r}"
        )
    return json.loads(environment_bytes)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--minimum-runs", type=int, default=3)
    parser.add_argument("--output", type=Path, help="also write the validation result to this new JSON file")
    args = parser.parse_args(argv)
    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        environment = _manifest_environment(args.manifest, manifest)
        result = validate_manifest(manifest, args.minimum_runs, environment=environment)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"validation failed: {error}", file=sys.stderr)
        return 2
    rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.output:
        if args.output.exists():
            print(f"error: refusing to overwrite {args.output}", file=sys.stderr)
            return 2
        args.output.write_text(rendered, encoding="utf-8")
    sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
