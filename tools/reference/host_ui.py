#!/usr/bin/env python3
"""Build and invoke the durable UTM window/input helper, with bounded capture."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from utm import ReferenceCommandError, run_bounded, sha256_file

REFERENCE_DIR = Path(__file__).resolve().parent
REPOSITORY_ROOT = REFERENCE_DIR.parents[1]
LOCAL_ROOT = REPOSITORY_ROOT / "artifacts" / "reference-local"
HOST_TOOL = LOCAL_ROOT / "bin" / "host-window"
HOST_SOURCE = REFERENCE_DIR / "host_window.swift"


def ensure_host_tool() -> Path:
    if HOST_TOOL.exists() and HOST_TOOL.stat().st_mtime_ns >= HOST_SOURCE.stat().st_mtime_ns:
        return HOST_TOOL
    cache = LOCAL_ROOT / ".cache"
    temporary = HOST_TOOL.with_suffix(".tmp")
    for directory in (HOST_TOOL.parent, cache / "clang", cache / "swift", cache / "tmp"):
        directory.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update(
        CLANG_MODULE_CACHE_PATH=str(cache / "clang"),
        SWIFT_MODULECACHE_PATH=str(cache / "swift"),
        TMPDIR=str(cache / "tmp"),
    )
    try:
        result = subprocess.run(
            ["/usr/bin/swiftc", "-O", str(HOST_SOURCE), "-o", str(temporary)],
            capture_output=True,
            timeout=120,
            check=False,
            env=environment,
        )
    except (subprocess.TimeoutExpired, OSError) as error:
        raise ReferenceCommandError(f"Swift host helper build failed: {error}") from error
    if result.returncode != 0:
        raise ReferenceCommandError(
            "Swift host helper build exited "
            f"{result.returncode}: {result.stderr.decode('utf-8', 'replace')[:4000]}"
        )
    temporary.replace(HOST_TOOL)
    return HOST_TOOL


def host_call(*arguments: str, timeout: int = 20) -> bytes:
    tool = ensure_host_tool()
    return run_bounded([tool, *arguments], timeout=timeout, max_output_bytes=1024 * 1024).stdout


def host_json(*arguments: str) -> Any:
    output = host_call(*arguments)
    try:
        return json.loads(output)
    except json.JSONDecodeError as error:
        raise ReferenceCommandError(f"host helper returned invalid JSON: {output[:500]!r}") from error


def focus() -> dict[str, Any]:
    result = host_json("focus")
    if not isinstance(result, dict):
        raise ReferenceCommandError("focus result is not an object")
    return result


def key(name: str, *modifiers: str) -> None:
    host_call("key", name, ",".join(modifiers))


def type_text(text: str) -> None:
    host_call("type", text, timeout=max(20, len(text) // 5 + 10))


def click(x: float, y: float) -> None:
    host_call("click", str(x), str(y))


def wait_stable_window(*, required_size: tuple[int, int] | None = None, timeout: float = 12) -> dict[str, Any]:
    focus()
    deadline = time.monotonic() + timeout
    previous: tuple[Any, ...] | None = None
    last: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        windows = host_json("list")
        if isinstance(windows, list) and windows and isinstance(windows[0], dict):
            candidate = windows[0]
            last = candidate
            signature = tuple(round(float(candidate.get(field, -1)), 2) for field in ("id", "x", "y", "width", "height"))
            size_ok = required_size is None or (round(candidate["width"]), round(candidate["height"])) == required_size
            if size_ok and signature == previous:
                return candidate
            previous = signature if size_ok else None
        time.sleep(0.4)
    raise ReferenceCommandError(f"UTM Windows window did not stabilize at required size {required_size}; last={last}")


def capture_window(output: Path, *, ocr: bool = True) -> dict[str, Any]:
    output = Path(output).resolve()
    if output.exists():
        raise ReferenceCommandError(f"refusing to overwrite screenshot {output}")
    if not output.parent.is_dir():
        raise ReferenceCommandError(f"screenshot directory does not exist: {output.parent}")
    window = wait_stable_window()
    run_bounded(
        ["/usr/sbin/screencapture", "-x", "-l", str(window["id"]), str(output)],
        timeout=20,
        max_output_bytes=1024 * 1024,
    )
    if not output.is_file() or output.stat().st_size == 0:
        raise ReferenceCommandError(f"screencapture did not create {output}")
    evidence: dict[str, Any] = {
        "path": str(output),
        "sha256": sha256_file(output),
        "bytes": output.stat().st_size,
        "window": window,
        "capture": "macOS screencapture -x -l <CGWindowID>",
    }
    if ocr:
        executable = shutil.which("tesseract")
        if executable is None:
            raise ReferenceCommandError("tesseract is required for OCR evidence (use ocr=False to bypass explicitly)")
        result = run_bounded(
            [executable, str(output), "stdout"],
            timeout=45,
            max_output_bytes=1024 * 1024,
        )
        text = result.stdout.decode("utf-8", "replace")
        ocr_path = output.with_suffix(output.suffix + ".ocr.txt")
        ocr_path.write_text(text, encoding="utf-8")
        evidence["ocr_path"] = str(ocr_path)
        evidence["ocr_sha256"] = sha256_file(ocr_path)
        evidence["ocr_text"] = text
    return evidence


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("build")
    subparsers.add_parser("list")
    subparsers.add_parser("focus")
    key_parser = subparsers.add_parser("key")
    key_parser.add_argument("name")
    key_parser.add_argument("modifiers", nargs="?", default="")
    type_parser = subparsers.add_parser("type")
    type_parser.add_argument("text")
    click_parser = subparsers.add_parser("click")
    click_parser.add_argument("x", type=float)
    click_parser.add_argument("y", type=float)
    capture_parser = subparsers.add_parser("capture")
    capture_parser.add_argument("output", type=Path)
    capture_parser.add_argument("--no-ocr", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.command == "build":
            print(ensure_host_tool())
        elif args.command == "list":
            print(json.dumps(host_json("list"), indent=2, sort_keys=True))
        elif args.command == "focus":
            print(json.dumps(focus(), indent=2, sort_keys=True))
        elif args.command == "key":
            key(args.name, *filter(None, args.modifiers.split(",")))
        elif args.command == "type":
            type_text(args.text)
        elif args.command == "click":
            click(args.x, args.y)
        else:
            print(json.dumps(capture_window(args.output, ocr=not args.no_ocr), indent=2, sort_keys=True))
    except (ReferenceCommandError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
