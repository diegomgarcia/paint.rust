#!/usr/bin/env python3
"""Collect a secret-free host/UTM/guest environment inventory into new JSON."""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import json
import platform
import struct
import shutil
import sys
from pathlib import Path
from typing import Any

from host_ui import focus
from utm import REFERENCE_DIR, ReferenceCommandError, guest_json_script, run_bounded, sha256_file, utm_inventory


def _text_command(arguments: list[str], timeout: int = 20) -> str:
    return run_bounded(arguments, timeout=timeout, max_output_bytes=2 * 1024 * 1024).stdout.decode("utf-8", "replace")


def _tool_version(executable: str) -> str | None:
    path = shutil.which(executable)
    if path is None:
        return None
    flag = "--version" if executable == "tesseract" else "-version"
    return _text_command([path, flag], timeout=10).splitlines()[0]


def decode_logfont(value: dict[str, Any]) -> dict[str, Any]:
    if value.get("type") != "byte[]" or value.get("length") != 92:
        raise ValueError("LOGFONT evidence must be a 92-byte byte[]")
    raw = base64.b64decode(value["base64"], validate=True)
    if len(raw) != 92:
        raise ValueError("LOGFONT base64 length does not equal 92")
    numeric = struct.unpack("<5i8B", raw[:28])
    face_name = raw[28:].decode("utf-16-le").split("\0", 1)[0]
    names = (
        "height", "width", "escapement", "orientation", "weight", "italic", "underline",
        "strikeout", "charset", "out_precision", "clip_precision", "quality", "pitch_and_family",
    )
    result = dict(zip(names, numeric, strict=True))
    result["face_name"] = face_name
    return result


def collect_environment(vm: str = "Windows") -> dict[str, Any]:
    guest = guest_json_script(REFERENCE_DIR / "guest" / "Collect-Environment.ps1", vm=vm, timeout=150)
    metrics = guest["dpi_and_text"]["window_metrics"]
    for name in ("CaptionFont", "IconFont", "MenuFont", "MessageFont", "SmCaptionFont", "StatusFont"):
        if isinstance(metrics.get(name), dict):
            metrics[name]["decoded_logfont"] = decode_logfont(metrics[name])
    sw_vers = _text_command(["/usr/bin/sw_vers"], timeout=10).splitlines()
    display_text = _text_command(["/usr/sbin/system_profiler", "SPDisplaysDataType"], timeout=45)
    redacted_display_lines = [
        line for line in display_text.splitlines()
        if "serial" not in line.lower() and "uuid" not in line.lower()
    ]
    focused_window = focus()
    tool_hashes = {
        str(path.relative_to(REFERENCE_DIR)): sha256_file(path)
        for path in sorted(REFERENCE_DIR.rglob("*"))
        if path.is_file() and "__pycache__" not in path.parts and path.suffix != ".pyc"
    }
    return {
        "schema_version": 1,
        "kind": "paintnet-reference-environment",
        "status": "observed-not-fully-pinned",
        "collected_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "host": {
            "sw_vers": sw_vers,
            "architecture": platform.machine(),
            "model_class": _text_command(["/usr/sbin/sysctl", "-n", "hw.model"], timeout=10).strip(),
            "python": platform.python_version(),
            "ffmpeg": _tool_version("ffmpeg"),
            "tesseract": _tool_version("tesseract"),
            "accessibility_focus_verified": True,
            "focused_utm_window": focused_window,
            "system_profiler_displays_redacted": "\n".join(redacted_display_lines) + "\n",
            "screen_recording_evidence": "Verified by the associated run's nonblank screencapture artifacts, not inferred from this inventory alone.",
        },
        "utm": utm_inventory(vm),
        "guest": guest,
        "tool_sha256": tool_hashes,
        "secrets_policy": "No credentials, product keys, license identifiers, recent documents, clipboard, user name/SID, or hardware serials are intentionally collected.",
    }


def write_json_new(path: Path, value: dict[str, Any]) -> None:
    path = Path(path)
    if path.exists():
        raise ReferenceCommandError(f"refusing to overwrite {path}")
    if not path.parent.is_dir():
        raise ReferenceCommandError(f"output directory does not exist: {path.parent}")
    rendered = json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    created = False
    try:
        with path.open("x", encoding="utf-8") as output:
            created = True
            output.write(rendered)
            output.flush()
    except Exception:
        if created and path.exists():
            path.unlink()
        raise


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vm", default="Windows")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        environment = collect_environment(args.vm)
        write_json_new(args.output, environment)
    except (ReferenceCommandError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"output": str(args.output), "sha256": sha256_file(args.output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
