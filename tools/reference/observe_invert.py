#!/usr/bin/env python3
"""Run and validate three or more clean Paint.NET Invert -> PNG observations."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import sys
import time
from pathlib import Path
from typing import Any

from collect_environment import collect_environment, write_json_new
from host_ui import LOCAL_ROOT, capture_window
from paintnet_ui import invert_colors, localize_capture, new_default_document, save_png_with_defaults
from png_evidence import decode_png, png_file_evidence, rgba_evidence
from utm import (
    ReferenceCommandError,
    guest_file_metadata,
    guest_json_source,
    guest_png_evidence,
    pull_file,
    run_bounded,
    sha256_file,
    wait_guest_file,
)
from validate_observations import ValidationError, validate_manifest

GUEST_DOCUMENTS = r"C:\Users\CodeFreak\Documents"


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def guest_absent(path: str, *, vm: str = "Windows") -> None:
    result = guest_json_source(
        "[pscustomobject]@{ exists = (Test-Path -LiteralPath $Path) } | ConvertTo-Json -Compress",
        bindings={"Path": path},
        vm=vm,
        timeout=30,
    )
    if result.get("exists") is not False:
        raise ReferenceCommandError(f"guest output already exists or presence is unknown: {path}")


def independent_ffmpeg(path: Path, expected_rgba: bytes) -> dict[str, Any]:
    executable = shutil.which("ffmpeg")
    if executable is None:
        raise ReferenceCommandError("ffmpeg is required for independent host RGBA validation")
    version = run_bounded([executable, "-version"], timeout=10).stdout.decode("utf-8", "replace").splitlines()[0]
    result = run_bounded(
        [
            executable, "-v", "error", "-i", str(path), "-map", "0:v:0", "-frames:v", "1",
            "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
        ],
        timeout=60,
        max_output_bytes=len(expected_rgba) + 1024 * 1024,
    )
    actual = result.stdout
    if len(actual) != len(expected_rgba):
        raise ReferenceCommandError(f"FFmpeg RGBA length {len(actual)} does not equal {len(expected_rgba)}")
    return {
        "name": "FFmpeg rawvideo RGBA",
        "version": version,
        "decoded_rgba_bytes": len(actual),
        "rgba_sha256": hashlib.sha256(actual).hexdigest(),
        "rgba_matches_primary": actual == expected_rgba,
    }


def basic_observation_check(observation: dict[str, Any]) -> None:
    comparison = observation["hash_comparison"]
    if not comparison["guest_host_encoded_equal"] or not comparison["guest_host_rgba_equal"]:
        raise ReferenceCommandError(f"guest/host mismatch in run {observation['run']}")
    if not comparison["guest_stable_during_pull"]:
        raise ReferenceCommandError(f"guest file changed during run {observation['run']}")
    png = observation["host"]["png"]
    expected = {
        "width": 800,
        "height": 600,
        "decoded_rgba_bytes": 1_920_000,
        "pixel_count": 480_000,
        "all_pixels_equal": True,
        "first_pixel_rgba": [0, 0, 0, 255],
        "unique_color_count": 1,
    }
    for field, value in expected.items():
        if png.get(field) != value:
            raise ReferenceCommandError(f"run {observation['run']} host.png.{field}={png.get(field)!r}, expected {value!r}")
    if not png["independent_decoder"]["rgba_matches_primary"]:
        raise ReferenceCommandError(f"FFmpeg and primary host decoder differ in run {observation['run']}")


def run_observation(run: int, run_id: str, output_root: Path, vm: str) -> dict[str, Any]:
    started = utc_now()
    run_directory = output_root / f"run-{run}"
    ui_directory = run_directory / "ui"
    ui_directory.mkdir(parents=True)
    digest = hashlib.sha256(run_id.encode("utf-8")).digest()
    token = "".join(chr(ord("a") + byte % 26) for byte in digest[:10])
    filename = f"pra{chr(ord('a') + run - 1)}-{token}.png"
    guest_path = GUEST_DOCUMENTS + "\\" + filename
    host_path = run_directory / filename
    guest_absent(guest_path)

    actions, captures = new_default_document(ui_directory)
    invert_actions, invert_captures = invert_colors(ui_directory)
    actions.extend(invert_actions)
    captures.extend(invert_captures)
    save_actions, save_captures = save_png_with_defaults(ui_directory, filename)
    actions.extend(save_actions)
    captures.extend(save_captures)

    guest_before = wait_guest_file(guest_path, vm=vm, timeout=45)
    saved = capture_window(ui_directory / "08-saved.png", ocr=True)
    saved["stage"] = "saved"
    captures.append(saved)
    guest_png = guest_png_evidence(guest_path, vm=vm, timeout=60)
    pull_file(guest_path, host_path, vm=vm, timeout=45)
    guest_after = guest_file_metadata(guest_path, vm=vm, timeout=30)
    host_png = png_file_evidence(host_path)
    decoded = decode_png(host_path)
    if rgba_evidence(decoded)["rgba_sha256"] != host_png["rgba_sha256"]:
        raise ReferenceCommandError("host decode was not deterministic within the run")
    host_png["independent_decoder"] = independent_ffmpeg(host_path, decoded.rgba)
    host_sha256 = sha256_file(host_path)
    if host_png["sha256"] != host_sha256:
        raise ReferenceCommandError("host encoded hash was not deterministic within the run")

    encoded_equal = guest_before["sha256"] == guest_png["sha256"] == host_sha256
    rgba_equal = guest_png["rgba_sha256"] == host_png["rgba_sha256"]
    stable = (
        guest_before["sha256"] == guest_after["sha256"]
        and guest_before["length"] == guest_after["length"] == host_path.stat().st_size
    )
    return {
        "run": run,
        "started_at_utc": started,
        "completed_at_utc": utc_now(),
        "document": {
            "source": "new document",
            "width": 800,
            "height": 600,
            "operation": "Invert Colors",
            "save_format": "PNG",
            "save_configuration": "Auto-detect",
            "paintnet_window_title_version": "5.1.12",
        },
        "actions": actions,
        "ui_evidence": [localize_capture(capture, output_root) for capture in captures],
        "guest": {
            "path": guest_path,
            "sha256": guest_png["sha256"],
            "before_pull": guest_before,
            "after_pull": guest_after,
            "png": guest_png,
        },
        "host": {
            "path": str(host_path.relative_to(output_root)),
            "sha256": host_sha256,
            "png": host_png,
        },
        "hash_comparison": {
            "guest_host_encoded_equal": encoded_equal,
            "guest_host_rgba_equal": rgba_equal,
            "guest_stable_during_pull": stable,
        },
    }


def base_manifest(run_id: str, runs: int, output_root: Path, vm: str, command: list[str]) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "kind": "paintnet-invert-aa",
        "run_id": run_id,
        "requested_observation_count": runs,
        "vm_name": vm,
        "command": command,
        "environment": {
            "path": "environment.json",
            "sha256": sha256_file(output_root / "environment.json"),
        },
        "artifact_policy": "Local reference evidence only. Paint.NET UI screenshots and local artifacts are not public release assets or screenshot-parity claims.",
        "encoded_identity_policy": "Encoded identity is measured; exact decoded RGBA and guest/host agreement are the compatibility evidence.",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", default="invert-aa-" + dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--vm", default="Windows")
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args(argv)
    if re.fullmatch(r"[a-z0-9][a-z0-9-]{2,63}", args.run_id) is None:
        parser.error("run-id must be 3..64 lowercase letters/digits/hyphens")
    if args.runs < 3 or args.runs > 10:
        parser.error("runs must be 3..10")
    output_root = args.output_dir or (LOCAL_ROOT / "runs" / args.run_id)
    output_root = output_root.resolve()
    command = [os.fspath(Path(sys.argv[0]).resolve()), *(sys.argv[1:] if argv is None else argv)]
    observations: list[dict[str, Any]] = []
    stage = "initialize"
    try:
        output_root.mkdir(parents=True, exist_ok=False)
        stage = "environment"
        environment = collect_environment(args.vm)
        write_json_new(output_root / "environment.json", environment)
        for run in range(1, args.runs + 1):
            stage = f"observation-{run}"
            observation = run_observation(run, args.run_id, output_root, args.vm)
            observations.append(observation)
            write_json_new(output_root / f"observation-{run}.json", observation)
            basic_observation_check(observation)
        stage = "validation"
        manifest = base_manifest(args.run_id, args.runs, output_root, args.vm, command)
        manifest["status"] = "PASS"
        manifest["observations"] = observations
        validation = validate_manifest(manifest, minimum_runs=args.runs)
        write_json_new(output_root / "manifest.json", manifest)
        write_json_new(output_root / "validation.json", validation)
    except Exception as error:
        failure = {
            "schema_version": 1,
            "status": "PARTIAL" if observations else "FAIL",
            "stage": stage,
            "error_type": type(error).__name__,
            "error": str(error),
            "failed_at_utc": utc_now(),
            "completed_observation_count": len(observations),
            "command": command,
        }
        if output_root.is_dir():
            write_json_new(output_root / "failure.json", failure)
            if observations and not (output_root / "manifest.json").exists():
                partial = base_manifest(args.run_id, args.runs, output_root, args.vm, command)
                partial["status"] = "PARTIAL"
                partial["observations"] = observations
                partial["failure"] = failure
                write_json_new(output_root / "partial-manifest.json", partial)
        print(f"error during {stage}: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"status": "PASS", "output": str(output_root), "validation": validation}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
