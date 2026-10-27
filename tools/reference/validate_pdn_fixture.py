#!/usr/bin/env python3
"""Independently validate the local authentic-PDN fixture manifest and artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

from collect_environment import write_json_new
from observe_invert import independent_ffmpeg
from png_evidence import decode_png, png_file_evidence
from utm import ReferenceCommandError, sha256_file


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReferenceCommandError(message)


def artifact(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    require(path.is_relative_to(root), f"artifact escapes run directory: {relative}")
    require(path.is_file(), f"missing artifact: {relative}")
    return path


def action_index(actions: list[dict[str, Any]], text: str, start: int) -> int:
    for index in range(start, len(actions)):
        if actions[index].get("semantic_target") == text:
            return index + 1
    raise ReferenceCommandError(f"missing/out-of-order UI action: {text}")


def validate_pdn_fixture(manifest_path: Path, *, check_ffmpeg: bool = True) -> dict[str, Any]:
    manifest_path = manifest_path.resolve()
    root = manifest_path.parent
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    require(manifest.get("kind") == "authentic-paintnet-pdn-fixture", "wrong fixture kind")
    require(manifest.get("status") == "PASS_UI_SAVE_CLOSE_REOPEN_AND_RGBA", "fixture is not a native save/reopen PASS")
    require(manifest.get("authentic_stock_output") is True, "stock-output claim absent")
    require(manifest.get("ancillary_tool_edited_file") is False, "ancillary edit flag is not false")
    require(manifest.get("checks") and all(manifest["checks"].values()), "a recorded fixture check is false")

    environment_path = artifact(root, manifest["environment"]["path"])
    require(sha256_file(environment_path) == manifest["environment"]["sha256"], "environment hash mismatch")
    environment = json.loads(environment_path.read_text(encoding="utf-8"))
    product = manifest["product"]
    guest_product = environment["guest"]["paintnet"]
    require(product["display_version"] == "5.1.12", "unexpected Paint.NET display version")
    for field in ("file_version", "product_version", "path", "sha256", "signature_status", "signer_subject"):
        target = {"path": "executable_path", "sha256": "source_sha256"}.get(field, field)
        require(product[target] == guest_product[field], f"product/environment mismatch: {field}")
    require(product["signature_status"] == "Valid", "Paint.NET signature is not recorded Valid")

    pdn_path = artifact(root, manifest["pdn"]["host_path"])
    pdn_bytes = pdn_path.read_bytes()
    pdn_sha = hashlib.sha256(pdn_bytes).hexdigest()
    require(pdn_bytes[:4] == b"PDN3" and manifest["pdn"]["magic_ascii"] == "PDN3", "PDN3 magic mismatch")
    require(len(pdn_bytes) == manifest["pdn"]["bytes"] > 4, "PDN length mismatch")
    require(pdn_sha == manifest["pdn"]["sha256"], "PDN host hash mismatch")
    for stage in ("before_reopen", "after_reopen", "after_pull"):
        metadata = manifest["pdn"][stage]
        require(metadata["sha256"] == pdn_sha and metadata["length"] == len(pdn_bytes), f"PDN guest mismatch: {stage}")

    png_record = manifest["reopened_verification_png"]
    png_path = artifact(root, png_record["host_path"])
    decoded = decode_png(png_path)
    fresh_png = png_file_evidence(png_path)
    recorded_png = png_record["host_png"]
    for field in ("sha256", "width", "height", "decoded_rgba_bytes", "rgba_sha256", "first_pixel_rgba", "colors"):
        require(fresh_png[field] == recorded_png[field], f"PNG mismatch: {field}")
    require(png_record["sha256"] == fresh_png["sha256"], "PNG outer hash mismatch")
    require((fresh_png["width"], fresh_png["height"], fresh_png["decoded_rgba_bytes"]) == (2, 2, 16), "fixture is not 2x2 RGBA16")
    require(fresh_png["colors"] == [{"rgba": [0, 0, 0, 255], "count": 4}], "not exactly four opaque-black pixels")
    require(decoded.rgba == bytes([0, 0, 0, 255]) * 4, "decoded RGBA bytes differ")
    require(png_record["guest_png"]["rgba_sha256"] == fresh_png["rgba_sha256"], "guest/host RGBA mismatch")
    require(png_record["guest_png"]["sha256"] == fresh_png["sha256"], "guest/host encoded PNG mismatch")
    for stage in ("before_pull", "after_pull"):
        metadata = png_record[stage]
        require(metadata["sha256"] == fresh_png["sha256"] and metadata["length"] == png_path.stat().st_size, f"PNG guest mismatch: {stage}")

    bgra = bytes(component for offset in range(0, len(decoded.rgba), 4) for component in (
        decoded.rgba[offset + 2], decoded.rgba[offset + 1], decoded.rgba[offset], decoded.rgba[offset + 3]
    ))
    document = manifest["document"]
    require((document["width"], document["height"], document["layer_count"]) == (2, 2, 1), "document semantics mismatch")
    require(document["layer_order"] == "bottom_to_top" and len(document["layers"]) == 1, "layer order/count mismatch")
    layer = document["layers"][0]
    require(layer["index"] == 0 and layer["name"] == "Background", "background layer semantics mismatch")
    require(layer["visible"] is True and layer["background"] is True, "visibility/background mismatch")
    require(layer["opacity"] == 255 and layer["blend_mode"] == "Normal", "opacity/blend mismatch")
    require(layer["straight_alpha_bgra"]["sha256"] == hashlib.sha256(bgra).hexdigest(), "per-layer BGRA hash mismatch")
    require(layer["straight_alpha_bgra"]["bytes"] == len(bgra) == 16, "per-layer BGRA length mismatch")

    actions = manifest["actions"]
    position = 0
    for action in (
        "create 2x2 document",
        "Adjustments > Invert Colors",
        "save pdn",
        "File > Close saved PDN",
        "File > Open",
        "reopen saved PDN",
        "save png",
        "accept Auto-detect PNG configuration",
    ):
        position = action_index(actions, action, position)

    ui_count = 0
    for capture in manifest["ui_evidence"]:
        image = artifact(root, capture["path"])
        require(sha256_file(image) == capture["sha256"], f"UI image hash mismatch: {capture['path']}")
        if capture.get("ocr_path"):
            ocr = artifact(root, capture["ocr_path"])
            require(sha256_file(ocr) == capture["ocr_sha256"], f"UI OCR hash mismatch: {capture['ocr_path']}")
        ui_count += 1
    require(ui_count >= 20, "insufficient UI evidence")

    ffmpeg_match = None
    if check_ffmpeg:
        ffmpeg_match = independent_ffmpeg(png_path, decoded.rgba)["rgba_matches_primary"]
        require(ffmpeg_match is True, "independent FFmpeg RGBA mismatch")

    return {
        "schema_version": 1,
        "status": "PASS",
        "manifest": str(manifest_path),
        "pdn": {"bytes": len(pdn_bytes), "sha256": pdn_sha, "magic": "PDN3"},
        "png": {"bytes": png_path.stat().st_size, "sha256": fresh_png["sha256"], "rgba_sha256": fresh_png["rgba_sha256"]},
        "straight_alpha_bgra_sha256": hashlib.sha256(bgra).hexdigest(),
        "dimensions": [2, 2],
        "pixels": [[0, 0, 0, 255]] * 4,
        "action_count": len(actions),
        "ui_evidence_count": ui_count,
        "ffmpeg_rgba_match": ffmpeg_match,
        "boundary": "Authentic stock save/close/reopen/export only; no independent PDN parser, clone parity, or writer-acceptance claim.",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, help="new validation JSON path")
    parser.add_argument("--skip-ffmpeg", action="store_true")
    args = parser.parse_args(argv)
    try:
        report = validate_pdn_fixture(args.manifest, check_ffmpeg=not args.skip_ffmpeg)
        if args.output:
            write_json_new(args.output.resolve(), report)
    except Exception as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
