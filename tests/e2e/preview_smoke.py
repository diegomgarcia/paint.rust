"""Black-box development-preview trace, determinism, and failure-path gate."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from png_rgba import PngError, read_png

SNAPSHOT_FIELDS = {
    "format",
    "version",
    "width",
    "height",
    "layer_count",
    "active_layer_index",
    "dirty",
    "can_undo",
    "can_redo",
    "layers",
    "selection",
    "decoded_rgba_sha256",
}


def load_trace(path: Path) -> dict[str, object]:
    trace = json.loads(path.read_text(encoding="utf-8"))
    if trace.get("format") != "paint.r.command-trace" or trace.get("version") != 1:
        raise ValueError(f"unsupported trace format in {path}")
    return trace


def run_product(binary: Path, trace: Path, output: Path, timeout: float) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment.update(LC_ALL="C", RUST_BACKTRACE="1")
    return subprocess.run(
        [str(binary), "--headless-smoke", "--trace", str(trace), "--output-dir", str(output)],
        text=True,
        capture_output=True,
        timeout=timeout,
        env=environment,
        check=False,
    )


def verify_outputs(trace: dict[str, object], output: Path) -> dict[str, object]:
    composite = output / "composite.png"
    snapshot_path = output / "snapshot.json"
    image = read_png(composite)
    pixel_hash = hashlib.sha256(image.pixels).hexdigest()
    snapshot = json.loads(snapshot_path.read_text(encoding="utf-8"))
    missing = SNAPSHOT_FIELDS - snapshot.keys()
    if missing or set(snapshot) != SNAPSHOT_FIELDS:
        raise ValueError(f"snapshot fields differ: missing={sorted(missing)}, actual={sorted(snapshot)}")
    if snapshot["format"] != "paint.r.semantic-snapshot" or type(snapshot["version"]) is not int or snapshot["version"] != 1:
        raise ValueError("unsupported semantic snapshot format")
    for key in ("width", "height", "layer_count", "active_layer_index"):
        if type(snapshot[key]) is not int:
            raise ValueError(f"snapshot {key} must be an integer")
    for key in ("dirty", "can_undo", "can_redo"):
        if type(snapshot[key]) is not bool:
            raise ValueError(f"snapshot {key} must be a boolean")
    if not isinstance(snapshot["layers"], list) or not (snapshot["selection"] is None or isinstance(snapshot["selection"], dict)):
        raise ValueError("snapshot layers/selection have invalid types")
    if (snapshot["width"], snapshot["height"]) != (image.width, image.height):
        raise ValueError("snapshot and PNG dimensions differ")
    if snapshot["layer_count"] != len(snapshot["layers"]):
        raise ValueError("snapshot layer_count does not match layers")
    if snapshot["decoded_rgba_sha256"] != pixel_hash:
        raise ValueError("snapshot pixel hash does not match independently decoded PNG")

    expected = trace.get("expect", {})
    assert isinstance(expected, dict)
    allowed_expectations = {"width", "height", "decoded_rgba_sha256", "layer_count", "active_layer_index", "dirty", "can_undo", "can_redo"}
    unknown_expectations = set(expected) - allowed_expectations
    if unknown_expectations:
        raise ValueError(f"unknown expectation fields: {sorted(unknown_expectations)}")
    for key, expected_value in expected.items():
        if key == "decoded_rgba_sha256":
            actual_value = pixel_hash
        else:
            actual_value = snapshot.get(key)
        if actual_value != expected_value:
            raise ValueError(f"expectation {key}: {actual_value!r} != {expected_value!r}")
    return {
        "width": image.width,
        "height": image.height,
        "decoded_rgba_sha256": pixel_hash,
        "snapshot": {key: snapshot[key] for key in ("layer_count", "active_layer_index", "dirty", "can_undo", "can_redo")},
    }


def expect_rejection(binary: Path, trace: dict[str, object], directory: Path, timeout: float) -> str:
    bad_trace = directory / "bad.json"
    bad_trace.write_text(json.dumps(trace), encoding="utf-8")
    output = directory / "output"
    output.mkdir()
    original_png = b"previous-good-png"
    original_snapshot = b"previous-good-snapshot"
    (output / "composite.png").write_bytes(original_png)
    (output / "snapshot.json").write_bytes(original_snapshot)
    result = run_product(binary, bad_trace, output, timeout)
    if result.returncode == 0:
        raise ValueError("malformed trace returned success")
    if (output / "composite.png").read_bytes() != original_png or (output / "snapshot.json").read_bytes() != original_snapshot:
        raise ValueError("rejected trace overwrote prior outputs")
    diagnostic = result.stderr.strip() or result.stdout.strip()
    if not diagnostic:
        raise ValueError("rejected trace produced no diagnostic")
    return diagnostic


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--trace", type=Path, action="append", required=True)
    parser.add_argument("--repeat", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--report", type=Path)
    arguments = parser.parse_args(argv)
    binary = arguments.binary.resolve()
    if not binary.is_file() or arguments.repeat < 1 or arguments.timeout <= 0:
        parser.error("binary must exist; repeat and timeout must be positive")

    results: list[dict[str, object]] = []
    failed = False
    with tempfile.TemporaryDirectory(prefix="paint-r-preview-") as temporary:
        root = Path(temporary)
        for index, trace_path in enumerate(arguments.trace):
            trace_path = trace_path.resolve()
            record: dict[str, object] = {"trace": str(trace_path), "runs": []}
            results.append(record)
            try:
                trace = load_trace(trace_path)
                output = root / f"trace-{index}"
                output.mkdir()
                for _ in range(arguments.repeat):
                    completed = run_product(binary, trace_path, output, arguments.timeout)
                    if completed.returncode != 0:
                        raise ValueError(f"product exited {completed.returncode}: {completed.stderr.strip()}")
                    stdout = json.loads(completed.stdout)
                    if not isinstance(stdout, dict):
                        raise ValueError("headless-smoke stdout is not a JSON object")
                    evidence = verify_outputs(trace, output)
                    record["runs"].append(evidence)
                if len({json.dumps(run, sort_keys=True) for run in record["runs"]}) != 1:
                    raise ValueError("repeated runs produced different semantic evidence")

                negative_root = root / f"negative-{index}"
                negative_root.mkdir()
                unknown = copy.deepcopy(trace)
                unknown["commands"] = [*unknown["commands"], {"op": "not-a-command"}]
                record["unknown_command_error"] = expect_rejection(binary, unknown, negative_root, arguments.timeout)
                invalid_rect = copy.deepcopy(trace)
                invalid_rect["commands"] = [{"op": "select_rect", "rect": {"left": 2, "top": 0, "right": 1, "bottom": 1}}]
                rejection_dir = root / f"negative-rect-{index}"
                rejection_dir.mkdir()
                record["invalid_rect_error"] = expect_rejection(binary, invalid_rect, rejection_dir, arguments.timeout)
                record["status"] = "pass"
            except (OSError, ValueError, KeyError, TypeError, PngError, subprocess.TimeoutExpired, json.JSONDecodeError) as error:
                failed = True
                record.update(status="fail", error=str(error))

    report = {"status": "fail" if failed else "pass", "traces": results}
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if arguments.report:
        arguments.report.write_text(rendered + "\n", encoding="utf-8")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
