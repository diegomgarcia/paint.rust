#!/usr/bin/env python3
"""Black-box stock Paint.NET open acceptance for an untouched clone PDN input."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any

from collect_environment import collect_environment, write_json_new
from host_ui import LOCAL_ROOT, capture_window
from observe_invert import GUEST_DOCUMENTS, guest_absent, independent_ffmpeg, utc_now
from observe_pdn_fixture import OPEN_ITEM, edit_distance, require_open_filename, save_as
from paintnet_identity import require_reviewed_paintnet
from paintnet_ui import FILE_MENU, _capture, _click_relative, _require_ocr, localize_capture
from host_ui import key, type_text
from png_evidence import decode_png, png_file_evidence
from utm import ReferenceCommandError, guest_file_metadata, guest_png_evidence, pull_file, sha256_file, wait_guest_file


def accepted_title(evidence: dict[str, Any], filename: str) -> bool:
    for line in evidence.get("ocr_text", "").splitlines():
        if not re.search(r"Paint", line, re.IGNORECASE) or ".pdn" not in line.lower():
            continue
        candidates = re.findall(r"[a-z0-9][a-z0-9.-]*\.pdn", line, re.IGNORECASE)
        if any(edit_distance(candidate.lower(), filename.lower()) <= 1 for candidate in candidates):
            return True
    return False


def rejection_dialog(evidence: dict[str, Any]) -> bool:
    text = evidence.get("ocr_text", "")
    return bool(
        re.search(r"error|cannot|can't|could\s+not|unable|unsupported|not\s+supported|invalid", text, re.IGNORECASE)
        and re.search(r"\bOK\b", text)
    )


def classify_open_result(evidence: dict[str, Any], filename: str) -> tuple[bool, bool]:
    accepted = accepted_title(evidence, filename) and re.search(r"Background", evidence.get("ocr_text", ""), re.IGNORECASE) is not None
    rejected = rejection_dialog(evidence)
    if accepted == rejected:
        raise ReferenceCommandError(
            f"INDETERMINATE stock open result (accepted={accepted}, rejected={rejected}): {evidence.get('ocr_text', '')!r}"
        )
    return accepted, rejected


def require_not_already_open(evidence: dict[str, Any], filename: str) -> None:
    if accepted_title(evidence, filename):
        raise ReferenceCommandError(f"input filename is already open before File>Open; refusing stale-title acceptance: {filename}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--host-input", type=Path, required=True)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--variant", choices=("raw", "gzip"), required=True)
    parser.add_argument("--vm", default="Windows")
    args = parser.parse_args(argv)
    if re.fullmatch(r"[a-z0-9][a-z0-9-]{2,63}", args.run_id) is None:
        parser.error("run-id must be 3..64 lowercase letters/digits/hyphens")
    host_input = args.host_input.resolve()
    filename = host_input.name
    if re.fullmatch(r"clone-normal-2x2-(raw|gzip)\.pdn", filename) is None:
        parser.error("input must be an untouched clone-normal-2x2-[raw|gzip].pdn")
    guest_input = GUEST_DOCUMENTS + "\\" + filename
    token = "".join(chr(ord("a") + byte % 26) for byte in hashlib.sha256(args.run_id.encode()).digest()[:6])
    png_name = f"writer-{args.variant}-{token}.png"
    guest_png = GUEST_DOCUMENTS + "\\" + png_name
    output_root = (LOCAL_ROOT / "runs" / args.run_id).resolve()
    command = [os.fspath(Path(sys.argv[0]).resolve()), *(sys.argv[1:] if argv is None else argv)]
    actions: list[dict[str, Any]] = []
    captures: list[dict[str, Any]] = []
    accepted = False
    host_sha: str | None = None
    input_before: dict[str, Any] | None = None
    stage = "initialize"
    try:
        output_root.mkdir(parents=True, exist_ok=False)
        ui = output_root / "ui"
        ui.mkdir()
        stage = "input-preflight"
        host_sha = sha256_file(host_input)
        if host_sha != args.expected_sha256.lower():
            raise ReferenceCommandError(f"host input hash changed: {host_sha}")
        input_before = guest_file_metadata(guest_input, vm=args.vm, timeout=30)
        if input_before["sha256"] != host_sha or input_before["length"] != host_input.stat().st_size:
            raise ReferenceCommandError("guest input does not match untouched host input")
        guest_absent(guest_png, vm=args.vm)
        stage = "environment"
        environment = collect_environment(args.vm)
        write_json_new(output_root / "environment.json", environment)
        stage = "paintnet-preflight"
        paintnet_identity = require_reviewed_paintnet(environment)
        stage = "normalize"
        key("escape")
        time.sleep(0.2)
        key("escape")
        time.sleep(0.7)
        start = _capture(ui, "00-start.png", "start")
        _require_ocr(start, r"Paint\.\s*NET")
        actions.extend([
            {"action": "key", "key": "Escape", "semantic_target": "close inherited popup"},
            {"action": "key", "key": "Escape", "semantic_target": "close inherited modal/menu"},
        ])
        captures.append(start)
        stage = "pre-open-title-guard"
        require_not_already_open(start, filename)
        stage = "stock-file-open"
        actions.append(_click_relative(FILE_MENU, "File menu"))
        time.sleep(0.4)
        actions.append(_click_relative(OPEN_ITEM, "File > Open untouched clone PDN"))
        time.sleep(1.2)
        dialog = _capture(ui, "01-open-dialog.png", "open-dialog")
        _require_ocr(dialog, r"Documents", r"File\s*name", r"All\s*image\s*types")
        captures.append(dialog)
        actions.append({"action": "type_us_ascii", "text": filename, "semantic_target": "untouched clone PDN filename"})
        type_text(filename)
        time.sleep(0.4)
        typed = _capture(ui, "02-open-path.png", "open-path")
        require_open_filename(typed, filename)
        captures.append(typed)
        actions.append({"action": "key", "key": "Enter", "semantic_target": "stock Paint.NET open attempt"})
        key("enter")
        time.sleep(2.5)
        result = _capture(ui, "03-open-result.png", "open-result")
        captures.append(result)
        accepted, rejected = classify_open_result(result, filename)
        input_after_open = guest_file_metadata(guest_input, vm=args.vm, timeout=30)
        if input_after_open["sha256"] != host_sha or input_after_open["length"] != input_before["length"]:
            raise ReferenceCommandError("stock open attempt modified the clone input")

        manifest: dict[str, Any] = {
            "schema_version": 1,
            "kind": "stock-paintnet-writer-acceptance",
            "status": "REJECTED_BY_STOCK" if rejected else "OPENED_VERIFICATION_PENDING",
            "run_id": args.run_id,
            "variant": args.variant,
            "command": command,
            "stock_open_accepted": accepted,
            "product": {
                "name": "Paint.NET",
                **paintnet_identity,
                "native_windows_architecture": environment["guest"]["os"]["native_system_type"],
            },
            "environment": {"path": "environment.json", "sha256": sha256_file(output_root / "environment.json")},
            "input": {
                "host_path": str(host_input),
                "guest_path": guest_input,
                "bytes": input_before["length"],
                "sha256": host_sha,
                "before_open": input_before,
                "after_open": input_after_open,
                "modified_by_this_gate": False,
                "clone_source_commit": "not inferred; input SHA-256 is authoritative",
                "self_round_trip_is_not_acceptance": True,
            },
            "open_result_ocr": result.get("ocr_text", ""),
            "actions": actions,
            "ui_evidence": [localize_capture(capture, output_root) for capture in captures],
            "evidence_boundary": "Black-box stock Paint.NET File>Open only. No REA, IL inspection, decompilation, input conversion, or self-round-trip acceptance inference.",
        }
        if rejected:
            stage = "dismiss-rejection"
            actions.append({"action": "key", "key": "Enter", "semantic_target": "dismiss stock open error with OK"})
            key("enter")
            time.sleep(0.8)
            dismissed = _capture(ui, "04-rejection-dismissed.png", "rejection-dismissed")
            captures.append(dismissed)
            manifest["ui_evidence"] = [localize_capture(capture, output_root) for capture in captures]
            write_json_new(output_root / "manifest.json", manifest)
            print(json.dumps({"status": manifest["status"], "stock_open_accepted": False, "output": str(output_root)}, indent=2))
            return 0

        stage = "export-stock-opened-clone"
        export_actions, export_captures = save_as(ui, png_name, "png", "04-png")
        actions.extend(export_actions)
        captures.extend(export_captures)
        png_before = wait_guest_file(guest_png, vm=args.vm, timeout=45)
        saved = capture_window(ui / "05-verification-saved.png", ocr=True)
        saved["stage"] = "verification-saved"
        captures.append(saved)
        stage = "independent-verification"
        guest_png = guest_png_evidence(guest_png, vm=args.vm, timeout=60)
        host_png_path = output_root / png_name
        pull_file(guest_png["path"], host_png_path, vm=args.vm, timeout=45)
        png_after = guest_file_metadata(guest_png["path"], vm=args.vm, timeout=30)
        input_after_export = guest_file_metadata(guest_input, vm=args.vm, timeout=30)
        host_png = png_file_evidence(host_png_path)
        ffmpeg = independent_ffmpeg(host_png_path, decode_png(host_png_path).rgba)
        checks = {
            "input_unchanged_after_export": input_after_export["sha256"] == host_sha and input_after_export["length"] == input_before["length"],
            "png_guest_host_sha_equal": png_before["sha256"] == guest_png["sha256"] == host_png["sha256"] == png_after["sha256"],
            "png_guest_host_rgba_equal": guest_png["rgba_sha256"] == host_png["rgba_sha256"],
            "png_2x2_rgba16": host_png["width"] == 2 and host_png["height"] == 2 and host_png["decoded_rgba_bytes"] == 16,
            "png_exact_four_black_pixels": host_png["colors"] == [{"rgba": [0, 0, 0, 255], "count": 4}],
            "ffmpeg_rgba_equal": ffmpeg["rgba_matches_primary"],
        }
        manifest["status"] = "PASS_OPEN_AND_PIXELS" if all(checks.values()) else "OPENED_VERIFICATION_FAILED"
        manifest["input"]["after_export"] = input_after_export
        manifest["verification_png"] = {
            "guest_path": guest_png["path"], "host_path": png_name, "before_pull": png_before,
            "after_pull": png_after, "guest_png": guest_png, "host_png": host_png, "independent_ffmpeg": ffmpeg,
        }
        manifest["checks"] = checks
        manifest["actions"] = actions
        manifest["ui_evidence"] = [localize_capture(capture, output_root) for capture in captures]
        write_json_new(output_root / "manifest.json", manifest)
        print(json.dumps({"status": manifest["status"], "stock_open_accepted": True, "checks": checks, "output": str(output_root)}, indent=2))
        return 0 if all(checks.values()) else 3
    except Exception as error:
        failure = {
            "schema_version": 1,
            "status": "INDETERMINATE_OR_AUTOMATION_FAILURE",
            "stock_open_accepted_observed": accepted,
            "stage": stage,
            "error_type": type(error).__name__,
            "error": str(error),
            "command": command,
            "input": {
                "host_path": str(host_input),
                "guest_path": guest_input,
                "expected_sha256": args.expected_sha256.lower(),
                "observed_host_sha256": host_sha,
                "guest_before_open": input_before,
                "modified_by_this_gate": None,
                "note": "Failure JSON does not imply a final post-failure input rehash; only recorded checks are proven.",
            },
            "failed_at_utc": utc_now(),
        }
        if output_root.is_dir():
            failure["actions"] = actions
            failure["ui_evidence"] = [localize_capture(capture, output_root) for capture in captures]
            write_json_new(output_root / "failure.json", failure)
        print(f"error during {stage}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
