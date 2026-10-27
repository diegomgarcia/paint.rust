#!/usr/bin/env python3
"""Create, close, reopen, and decode an authentic tiny Paint.NET PDN fixture."""

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
from paintnet_identity import require_reviewed_paintnet
from paintnet_ui import (
    ADJUSTMENTS_MENU,
    FILE_MENU,
    INVERT_COLORS_ITEM,
    NEW_ITEM,
    SAVE_AS_ITEM,
    _capture,
    _click_relative,
    _require_ocr,
    invert_colors,
    localize_capture,
)
from host_ui import key, type_text
from png_evidence import decode_png, png_file_evidence
from utm import (
    ReferenceCommandError,
    guest_file_metadata,
    guest_png_evidence,
    pull_file,
    sha256_file,
    wait_guest_file,
)

OPEN_ITEM = (57, 139)
CLOSE_ITEM = (57, 325)


def edit_distance(left: str, right: str) -> int:
    previous = list(range(len(right) + 1))
    for i, left_character in enumerate(left, start=1):
        current = [i]
        for j, right_character in enumerate(right, start=1):
            current.append(min(
                current[-1] + 1,
                previous[j] + 1,
                previous[j - 1] + (left_character != right_character),
            ))
        previous = current
    return previous[-1]


def require_open_filename(evidence: dict[str, Any], filename: str) -> None:
    text = evidence.get("ocr_text", "")
    candidates = re.findall(r"[a-z0-9][a-z0-9.-]*\.pdn", text, re.IGNORECASE)
    if any(edit_distance(candidate.lower(), filename.lower()) <= 1 for candidate in candidates):
        return
    raise ReferenceCommandError(f"Open filename OCR differs by more than one edit from {filename}: {candidates}")


def require_filter(evidence: dict[str, Any], extension: str) -> None:
    for line in evidence.get("ocr_text", "").splitlines():
        if not re.search(r"Save\s*as\s*type", line, re.IGNORECASE):
            continue
        if extension == "png" and re.search(r"png", line, re.IGNORECASE):
            return
        # Full-page OCR sometimes reads the selected PDN value as "pda" and
        # Paint as "Paine". Reject PNG explicitly; final PDN3/reopen gates are authoritative.
        if extension == "pdn" and not re.search(r"png", line, re.IGNORECASE) and re.search(r"pdn|pda|paint|paine|net", line, re.IGNORECASE):
            return
    raise ReferenceCommandError(f"Save as type did not show {extension}: {evidence.get('ocr_text', '')!r}")


def new_2x2(directory: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    key("escape")
    time.sleep(0.2)
    key("escape")
    time.sleep(0.7)
    start = _capture(directory, "00-start.png", "start")
    _require_ocr(start, r"Paint\.\s*NET")
    actions = [
        {"action": "key", "key": "Escape", "semantic_target": "close inherited popup"},
        {"action": "key", "key": "Escape", "semantic_target": "close inherited menu or modal"},
    ]
    actions.append(_click_relative(FILE_MENU, "File menu"))
    time.sleep(0.4)
    actions.append(_click_relative(NEW_ITEM, "File > New"))
    time.sleep(1.2)
    dialog = _capture(directory, "01-new-dialog.png", "new-dialog")
    new_captures = [start, dialog]
    if re.search(r"Pixel\s+size", dialog.get("ocr_text", ""), re.IGNORECASE) is None and re.search(r"Open\s+Recent", dialog.get("ocr_text", ""), re.IGNORECASE):
        actions.append(_click_relative(NEW_ITEM, "File > New (single retry after visible File menu)"))
        time.sleep(1.2)
        dialog = _capture(directory, "01b-new-dialog-retry.png", "new-dialog-retry")
        new_captures.append(dialog)
    _require_ocr(dialog, r"\bNew\b", r"Pixel\s+size")
    actions.append({"action": "type_us_ascii", "text": "2", "semantic_target": "selected pixel width"})
    type_text("2")
    time.sleep(0.4)
    width = _capture(directory, "02-width-two.png", "width-two")
    _require_ocr(width, r"4\.7\s*KB")
    actions.append({"action": "key", "key": "Tab", "semantic_target": "move width to height"})
    key("tab")
    actions.append({"action": "type_us_ascii", "text": "2", "semantic_target": "pixel height"})
    type_text("2")
    time.sleep(0.4)
    dimensions = _capture(directory, "03-dimensions-2x2.png", "dimensions-2x2")
    _require_ocr(dimensions, r"16\s*bytes")
    actions.append({"action": "key", "key": "Enter", "semantic_target": "create 2x2 document"})
    key("enter")
    time.sleep(2)
    created = _capture(directory, "04-new-document.png", "new-document")
    _require_ocr(created, r"Untitled", r"Background")
    return actions, [*new_captures, width, dimensions, created]


def save_as(directory: Path, filename: str, image_format: str, prefix: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    actions = [_click_relative(FILE_MENU, "File menu")]
    time.sleep(0.4)
    actions.append(_click_relative(SAVE_AS_ITEM, "File > Save As"))
    time.sleep(1.2)
    dialog = _capture(directory, f"{prefix}-save-as.png", f"{prefix}-save-as")
    _require_ocr(dialog, r"Save\s+As", r"Documents")
    actions.append({"action": "type_us_ascii", "text": filename, "semantic_target": "Save As filename"})
    type_text(filename)
    time.sleep(0.4)
    typed = _capture(directory, f"{prefix}-path.png", f"{prefix}-path")
    _require_ocr(typed, re.escape(filename))
    actions.append(_click_relative((147, 413), "Save as type visible value"))
    time.sleep(0.4)
    dropdown = capture_window(directory / f"{prefix}-dropdown.png", ocr=True, focus_before=False)
    dropdown["stage"] = f"{prefix}-dropdown"
    _require_ocr(dropdown, r"\*\.pdn", r"PNG")
    actions.append({"action": "key", "key": "Home", "semantic_target": "select Paint.NET PDN in the open format dropdown"})
    key("home")
    if image_format == "png":
        actions.append({"action": "key", "key": "Down", "semantic_target": "select PNG after PDN in the open format dropdown"})
        key("down")
    actions.append({"action": "key", "key": "Enter", "semantic_target": f"confirm {image_format} dropdown selection"})
    key("enter")
    time.sleep(0.5)
    selected = capture_window(directory / f"{prefix}-type.png", ocr=True, ocr_psm=6)
    selected["stage"] = f"{prefix}-type"
    require_filter(selected, "pdn" if image_format == "pdn" else "png")
    actions.append({"action": "key", "key": "Enter", "semantic_target": f"save {image_format}"})
    key("enter")
    captures = [dialog, typed, dropdown, selected]
    if image_format == "png":
        time.sleep(1.2)
        configuration = _capture(directory, f"{prefix}-configuration.png", f"{prefix}-configuration")
        _require_ocr(configuration, r"Save\s+Configuration", r"Auto-detect")
        actions.append({"action": "key", "key": "Enter", "semantic_target": "accept Auto-detect PNG configuration"})
        key("enter")
        captures.append(configuration)
    return actions, captures


def close_and_reopen(directory: Path, filename: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    actions = [_click_relative(FILE_MENU, "File menu")]
    time.sleep(0.4)
    actions.append(_click_relative(CLOSE_ITEM, "File > Close saved PDN"))
    time.sleep(1.2)
    closed = _capture(directory, "07-pdn-closed.png", "pdn-closed")
    if re.search(r"Unsaved\s+Changes", closed.get("ocr_text", ""), re.IGNORECASE):
        raise ReferenceCommandError("saved PDN unexpectedly produced an Unsaved Changes prompt")
    actions.append(_click_relative(FILE_MENU, "File menu"))
    time.sleep(0.4)
    actions.append(_click_relative(OPEN_ITEM, "File > Open"))
    time.sleep(1.2)
    dialog = _capture(directory, "08-open-dialog.png", "open-dialog")
    _require_ocr(dialog, r"Documents", r"File\s*name", r"All\s*image\s*types")
    actions.append({"action": "type_us_ascii", "text": filename, "semantic_target": "Open PDN filename"})
    type_text(filename)
    time.sleep(0.4)
    typed = _capture(directory, "09-open-path.png", "open-path")
    require_open_filename(typed, filename)
    actions.append({"action": "key", "key": "Enter", "semantic_target": "reopen saved PDN"})
    key("enter")
    time.sleep(2.5)
    reopened = _capture(directory, "10-pdn-reopened.png", "pdn-reopened")
    _require_ocr(reopened, r"\.pdn", r"Paint\.\s*NET", r"Background")
    return actions, [closed, dialog, typed, reopened]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", default="pdn-fixture-" + dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    parser.add_argument("--vm", default="Windows")
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args(argv)
    if re.fullmatch(r"[a-z0-9][a-z0-9-]{2,63}", args.run_id) is None:
        parser.error("run-id must be 3..64 lowercase letters/digits/hyphens")
    output_root = (args.output_dir or (LOCAL_ROOT / "runs" / args.run_id)).resolve()
    command = [os.fspath(Path(sys.argv[0]).resolve()), *(sys.argv[1:] if argv is None else argv)]
    token = "".join(chr(ord("a") + byte % 26) for byte in hashlib.sha256(args.run_id.encode()).digest()[:6])
    base = f"fixture-{token}"
    pdn_name = base + ".pdn"
    png_name = base + "-reopened.png"
    pdn_guest = GUEST_DOCUMENTS + "\\" + pdn_name
    png_guest = GUEST_DOCUMENTS + "\\" + png_name
    actions: list[dict[str, Any]] = []
    captures: list[dict[str, Any]] = []
    stage = "initialize"
    try:
        output_root.mkdir(parents=True, exist_ok=False)
        ui_directory = output_root / "ui"
        ui_directory.mkdir()
        stage = "environment"
        environment = collect_environment(args.vm)
        write_json_new(output_root / "environment.json", environment)
        stage = "paintnet-preflight"
        paintnet_identity = require_reviewed_paintnet(environment)
        stage = "preflight"
        guest_absent(pdn_guest)
        guest_absent(png_guest)
        stage = "new-2x2"
        new_actions, new_captures = new_2x2(ui_directory)
        actions.extend(new_actions)
        captures.extend(new_captures)
        stage = "invert"
        invert_actions, invert_captures = invert_colors(ui_directory)
        actions.extend(invert_actions)
        captures.extend(invert_captures)
        stage = "save-pdn"
        pdn_actions, pdn_captures = save_as(ui_directory, pdn_name, "pdn", "05-pdn")
        actions.extend(pdn_actions)
        captures.extend(pdn_captures)
        pdn_before = wait_guest_file(pdn_guest, vm=args.vm, timeout=45)
        saved = capture_window(ui_directory / "06-pdn-saved.png", ocr=True)
        saved["stage"] = "pdn-saved"
        _require_ocr(saved, r"\.pdn", r"Paint\.\s*NET", r"Background")
        captures.append(saved)
        stage = "close-reopen-pdn"
        reopen_actions, reopen_captures = close_and_reopen(ui_directory, pdn_name)
        actions.extend(reopen_actions)
        captures.extend(reopen_captures)
        stage = "export-reopened-png"
        png_actions, png_captures = save_as(ui_directory, png_name, "png", "11-png")
        actions.extend(png_actions)
        captures.extend(png_captures)
        png_before = wait_guest_file(png_guest, vm=args.vm, timeout=45)
        saved_png = capture_window(ui_directory / "12-verification-saved.png", ocr=True)
        saved_png["stage"] = "verification-saved"
        captures.append(saved_png)
        stage = "independent-verification"
        pdn_after = guest_file_metadata(pdn_guest, vm=args.vm, timeout=30)
        png_guest_evidence = guest_png_evidence(png_guest, vm=args.vm, timeout=60)
        pdn_host = output_root / pdn_name
        png_host = output_root / png_name
        pull_file(pdn_guest, pdn_host, vm=args.vm, timeout=45)
        pull_file(png_guest, png_host, vm=args.vm, timeout=45)
        pdn_after_pull = guest_file_metadata(pdn_guest, vm=args.vm, timeout=30)
        png_after_pull = guest_file_metadata(png_guest, vm=args.vm, timeout=30)
        host_png = png_file_evidence(png_host)
        decoded = decode_png(png_host)
        host_png["independent_decoder"] = independent_ffmpeg(png_host, decoded.rgba)
        bgra = bytes(component for offset in range(0, len(decoded.rgba), 4) for component in (
            decoded.rgba[offset + 2], decoded.rgba[offset + 1], decoded.rgba[offset], decoded.rgba[offset + 3]
        ))
        bgra_sha = hashlib.sha256(bgra).hexdigest()
        with pdn_host.open("rb") as source:
            magic = source.read(4)
        pdn_sha = sha256_file(pdn_host)
        png_sha = sha256_file(png_host)
        expected_colors = [{"rgba": [0, 0, 0, 255], "count": 4}]
        checks = {
            "pdn3_magic": magic == b"PDN3",
            "pdn_guest_host_sha_equal": pdn_before["sha256"] == pdn_sha == pdn_after["sha256"] == pdn_after_pull["sha256"],
            "pdn_stable_during_reopen": pdn_before["sha256"] == pdn_after["sha256"],
            "png_guest_host_sha_equal": png_before["sha256"] == png_guest_evidence["sha256"] == png_sha == png_after_pull["sha256"],
            "png_guest_host_rgba_equal": png_guest_evidence["rgba_sha256"] == host_png["rgba_sha256"],
            "png_2x2_rgba16": host_png["width"] == 2 and host_png["height"] == 2 and host_png["decoded_rgba_bytes"] == 16,
            "png_exact_four_black_pixels": host_png["colors"] == expected_colors,
            "single_layer_straight_bgra_exact": bgra == bytes([0, 0, 0, 255]) * 4,
            "ffmpeg_rgba_equal": host_png["independent_decoder"]["rgba_matches_primary"],
        }
        failed = [name for name, passed in checks.items() if not passed]
        if failed:
            raise ReferenceCommandError(f"fixture verification failed: {failed}")
        manifest = {
            "schema_version": 1,
            "kind": "authentic-paintnet-pdn-fixture",
            "status": "PASS_UI_SAVE_CLOSE_REOPEN_AND_RGBA",
            "run_id": args.run_id,
            "command": command,
            "environment": {"path": "environment.json", "sha256": sha256_file(output_root / "environment.json")},
            "product": {
                "name": "Paint.NET",
                **paintnet_identity,
                "native_windows_architecture": environment["guest"]["os"]["native_system_type"],
            },
            "authentic_stock_output": True,
            "ancillary_tool_edited_file": False,
            "document": {
                "width": 2,
                "height": 2,
                "layer_count": 1,
                "layer_order": "bottom_to_top",
                "layers": [{
                    "index": 0,
                    "name": "Background",
                    "visible": True,
                    "background": True,
                    "opacity": 255,
                    "blend_mode": "Normal",
                    "attribute_provenance": "stock defaults of the newly created Background layer; not separately queried through Layer Properties",
                    "straight_alpha_bgra": {
                        "bytes": 16,
                        "sha256": bgra_sha,
                        "pixel_order": "row-major, top-to-bottom, left-to-right",
                        "expected_pixels": [[0, 0, 0, 255]] * 4,
                        "derivation": "R/B swap of the independently decoded post-reopen composite; this one visible layer covers the entire canvas",
                    },
                }],
                "operation": "Invert Colors on the default new Background layer",
                "composite_pixel_order": "row-major RGBA8",
                "expected_rgba": [[0, 0, 0, 255]] * 4,
                "coverage_limitations": "One 2x2 opaque-black Normal/background fixture only; not the full requested semantic corpus and not a writer-acceptance result.",
            },
            "pdn": {
                "guest_path": pdn_guest,
                "host_path": pdn_name,
                "bytes": pdn_host.stat().st_size,
                "sha256": pdn_sha,
                "magic_ascii": magic.decode("ascii"),
                "before_reopen": pdn_before,
                "after_reopen": pdn_after,
                "after_pull": pdn_after_pull,
            },
            "reopened_verification_png": {
                "guest_path": png_guest,
                "host_path": png_name,
                "sha256": png_sha,
                "before_pull": png_before,
                "after_pull": png_after_pull,
                "guest_png": png_guest_evidence,
                "host_png": host_png,
            },
            "checks": checks,
            "actions": actions,
            "ui_evidence": [localize_capture(capture, output_root) for capture in captures],
            "evidence_boundary": "Paint.NET saved, closed, and reopened the PDN; the PNG was exported after reopening. One Background layer was observed. No independent PDN parser, clone comparison, REA, IL inspection, or decompilation was used.",
            "artifact_policy": "Local reference fixture and proprietary UI evidence; not a public release or screenshot-parity claim.",
        }
        write_json_new(output_root / "manifest.json", manifest)
    except Exception as error:
        failure = {
            "schema_version": 1,
            "status": "PARTIAL_NO_CERTIFIED_FIXTURE",
            "stage": stage,
            "error_type": type(error).__name__,
            "error": str(error),
            "command": command,
            "guest_pdn_path": pdn_guest,
            "guest_png_path": png_guest,
            "failed_at_utc": utc_now(),
        }
        if output_root.is_dir():
            write_json_new(output_root / "failure.json", failure)
        print(f"error during {stage}: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"status": "PASS", "output": str(output_root), "pdn_sha256": pdn_sha, "png_rgba_sha256": host_png["rgba_sha256"], "checks": checks}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
