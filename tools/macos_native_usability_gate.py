#!/usr/bin/env python3
"""Bounded macOS native keyboard/dirty-close evidence gate for Paint.R.

The orchestration is platform-neutral and unit-tested with fakes. Only the real
CLI backend below compiles/invokes the Swift native transport; no headless UI or
session-only result is accepted as native evidence.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime as dt
import hashlib
import importlib.util
import json
import math
import os
import platform
import re
import struct
import subprocess
import sys
import threading
import time
import zlib
from pathlib import Path
from typing import Any, Callable

WINDOW_TITLE = "Paint.R Development Preview"
INVERTED = "Inverted colors on the active layer"
CLOSE_TITLE = "Discard unsaved changes?"
CLOSE_EXPLANATION = "Closing Paint.R will discard the modified document and its undo history."
CANVAS_LABEL = "Composed document canvas"

MAX_STARTUP_TIMEOUT = 60.0
MAX_OBSERVATION_TIMEOUT = 30.0
MAX_QUIT_TIMEOUT = 30.0
MAX_CLEANUP_TIMEOUT = 20.0
MAX_COMMAND_TIMEOUT = 20.0
MAX_OVERALL_TIMEOUT = 180.0
MAX_CONFIRMATION_HOLD = 3.0
MAX_NATIVE_OUTPUT = 1024 * 1024
MAX_SCREENSHOT_BYTES = 64 * 1024 * 1024
MAX_SCREENSHOT_PIXELS = 100_000_000
MAX_BINARY_BYTES = 512 * 1024 * 1024
MAX_AX_NODES = 512
BLOCKED_CODES = {
    "SCREEN_LOCKED",
    "ACCESSIBILITY_UNAVAILABLE",
    "SCREEN_RECORDING_UNAVAILABLE",
    "NO_CONSOLE_SESSION",
    "SESSION_STATE_UNAVAILABLE",
    "UNSUPPORTED_PLATFORM",
    "HELPER_BUILD_FAILED",
}


_PNG_ORACLE: Any | None = None


def _png_oracle() -> Any:
    """Load the repository's independent bounded PNG-to-RGBA test oracle lazily."""
    global _PNG_ORACLE
    if _PNG_ORACLE is not None:
        return _PNG_ORACLE
    source = Path(__file__).resolve().parent.parent / "tests/e2e/png_rgba.py"
    spec = importlib.util.spec_from_file_location("_paint_r_native_png_oracle", source)
    if spec is None or spec.loader is None:
        raise GateError("SCREENSHOT_DECODER_UNAVAILABLE", f"cannot load PNG oracle: {source}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except Exception as error:
        sys.modules.pop(spec.name, None)
        raise GateError("SCREENSHOT_DECODER_UNAVAILABLE", f"cannot load PNG oracle: {error}") from error
    _PNG_ORACLE = module
    return module


class NativeCommandError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class GateError(Exception):
    outcome = "FAIL"

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class GateBlocked(GateError):
    outcome = "BLOCKED"


@dataclasses.dataclass(frozen=True)
class GateConfig:
    startup_timeout: float = 20.0
    observation_timeout: float = 8.0
    quit_timeout: float = 15.0
    cleanup_timeout: float = 8.0
    command_timeout: float = 8.0
    overall_timeout: float = 120.0
    confirmation_hold: float = 1.0
    poll_interval: float = 0.2

    def validate(self) -> None:
        limits = {
            "startup_timeout": MAX_STARTUP_TIMEOUT,
            "observation_timeout": MAX_OBSERVATION_TIMEOUT,
            "quit_timeout": MAX_QUIT_TIMEOUT,
            "cleanup_timeout": MAX_CLEANUP_TIMEOUT,
            "command_timeout": MAX_COMMAND_TIMEOUT,
            "overall_timeout": MAX_OVERALL_TIMEOUT,
            "poll_interval": 1.0,
        }
        for name, maximum in limits.items():
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise ValueError(f"{name} must be numeric")
            if not math.isfinite(value) or not 0 < value <= maximum:
                raise ValueError(f"{name} must be in (0, {maximum}]")
        if (
            isinstance(self.confirmation_hold, bool)
            or not isinstance(self.confirmation_hold, (int, float))
            or not math.isfinite(self.confirmation_hold)
            or not 0 <= self.confirmation_hold <= MAX_CONFIRMATION_HOLD
        ):
            raise ValueError(f"confirmation_hold must be in [0, {MAX_CONFIRMATION_HOLD}]")


def run_native_command(
    arguments: list[str],
    *,
    timeout: float,
    max_output_bytes: int = MAX_NATIVE_OUTPUT,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[bytes]:
    """Run one native command with streaming time/output bounds and no shell."""
    try:
        process = subprocess.Popen(
            arguments,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
        )
    except OSError as error:
        raise NativeCommandError("COMMAND_EXEC_FAILED", f"cannot execute {arguments[0]}: {error}") from error

    assert process.stdout is not None and process.stderr is not None
    stdout = bytearray()
    stderr = bytearray()
    total_read = 0
    output_overflow = False
    kill_started = False
    reader_errors: list[Exception] = []
    lock = threading.Lock()

    def read_output(stream: Any, destination: bytearray) -> None:
        nonlocal total_read, output_overflow, kill_started
        try:
            while chunk := stream.read(65536):
                should_kill = False
                with lock:
                    total_read += len(chunk)
                    remaining = max_output_bytes - len(stdout) - len(stderr)
                    if remaining > 0:
                        destination.extend(chunk[:remaining])
                    if total_read > max_output_bytes:
                        output_overflow = True
                        if not kill_started:
                            kill_started = True
                            should_kill = True
                if should_kill:
                    try:
                        process.kill()
                    except OSError:
                        # The process may have exited between the read and kill.
                        pass
        except Exception as error:  # Reader failure must fail the command closed.
            with lock:
                reader_errors.append(error)
        finally:
            stream.close()

    readers = [
        threading.Thread(target=read_output, args=(process.stdout, stdout), daemon=True),
        threading.Thread(target=read_output, args=(process.stderr, stderr), daemon=True),
    ]
    for reader in readers:
        reader.start()

    timeout_error: subprocess.TimeoutExpired | None = None
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired as error:
        timeout_error = error
        try:
            process.kill()
        except OSError:
            pass
        process.wait()
    except OSError as error:
        try:
            process.kill()
        except OSError:
            pass
        process.wait()
        for reader in readers:
            reader.join(timeout=1)
        raise NativeCommandError("COMMAND_EXEC_FAILED", f"cannot execute {arguments[0]}: {error}") from error

    for reader in readers:
        reader.join(timeout=1)
    if any(reader.is_alive() for reader in readers):
        raise NativeCommandError("COMMAND_OUTPUT_REAP_FAILED", "native command output readers did not finish")
    if output_overflow:
        raise NativeCommandError("COMMAND_OUTPUT_LIMIT", f"native command output exceeded {max_output_bytes} bytes")
    if timeout_error is not None:
        raise NativeCommandError("COMMAND_TIMEOUT", f"native command timed out: {arguments[0]}") from timeout_error
    if reader_errors:
        raise NativeCommandError("COMMAND_OUTPUT_READ_FAILED", "native command output could not be read") from reader_errors[0]
    assert process.returncode is not None
    return subprocess.CompletedProcess(arguments, process.returncode, bytes(stdout), bytes(stderr))


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path, *, maximum: int = MAX_SCREENSHOT_BYTES) -> tuple[str, int]:
    size = path.stat().st_size
    if size > maximum:
        raise GateError("EVIDENCE_FILE_TOO_LARGE", f"file exceeds {maximum} bytes: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest(), size


def harness_identity() -> dict[str, Any]:
    repository = Path(__file__).resolve().parents[1]
    records = []
    for relative in (
        "tools/macos_native_usability_gate.py",
        "tools/macos_native_usability_gate.swift",
        "tests/e2e/png_rgba.py",
    ):
        digest, size = sha256_file(repository / relative, maximum=MAX_NATIVE_OUTPUT)
        records.append({"path": relative, "sha256": digest, "bytes": size})
    return {"capture_stage": "gate_initialization", "files": records}


def write_json_new(path: Path, value: dict[str, Any]) -> None:
    with path.open("x", encoding="utf-8") as output:
        output.write(json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n")


def preflight_problem(state: dict[str, Any]) -> NativeCommandError | None:
    if state.get("session_dictionary_available") is not True or state.get("screen_lock_state_known") is not True:
        return NativeCommandError("SESSION_STATE_UNAVAILABLE", "macOS console/lock state is inconclusive")
    if state.get("console_session") is not True:
        return NativeCommandError("NO_CONSOLE_SESSION", "no active console session is observable")
    if state.get("screen_locked") is not False:
        return NativeCommandError("SCREEN_LOCKED", "the macOS screen is locked or its state is invalid")
    if state.get("accessibility_trusted") is not True:
        return NativeCommandError("ACCESSIBILITY_UNAVAILABLE", "Accessibility permission is unavailable")
    if state.get("screen_recording") is not True:
        return NativeCommandError("SCREEN_RECORDING_UNAVAILABLE", "Screen Recording permission is unavailable")
    return None


@dataclasses.dataclass
class ObservedWindow:
    raw: dict[str, Any]
    window: dict[str, Any] | None
    texts: set[str]
    buttons: dict[str, dict[str, Any]]

    @property
    def window_id(self) -> int | None:
        return None if self.window is None else self.window["id"]

    def modified(self) -> bool:
        return "Modified" in self.texts and INVERTED in self.texts and CANVAS_LABEL in self.texts and "Clean" not in self.texts

    def clean(self) -> bool:
        return "Clean" in self.texts and "Modified" not in self.texts and CANVAS_LABEL in self.texts

    def close_dialog(self) -> bool:
        cancel = self.buttons.get("Cancel")
        discard = self.buttons.get("Discard")
        return (
            CLOSE_TITLE in self.texts
            and CLOSE_EXPLANATION in self.texts
            and cancel is not None
            and cancel.get("enabled") is True
            and cancel.get("focused") is True
            and discard is not None
            and discard.get("enabled") is True
        )


def parse_observation(raw: dict[str, Any], pid: int) -> ObservedWindow:
    if not isinstance(raw, dict) or raw.get("schema_version") != 1:
        raise GateError("INCONCLUSIVE_AX", "native observation has an invalid schema")
    process = raw.get("process")
    if not isinstance(process, dict) or process.get("pid") != pid or process.get("running") is not True:
        raise GateError("INCONCLUSIVE_PROCESS", f"native process {pid} is not observable as running")
    ax = raw.get("ax")
    if not isinstance(ax, dict) or ax.get("truncated") is not False:
        raise GateError("INCONCLUSIVE_AX", "Accessibility tree is unavailable or truncated")
    nodes = ax.get("nodes")
    if not isinstance(nodes, list) or len(nodes) > MAX_AX_NODES or ax.get("node_count") != len(nodes):
        raise GateError("INCONCLUSIVE_AX", "Accessibility node evidence is invalid or exceeds its bound")
    texts: set[str] = set()
    buttons: dict[str, dict[str, Any]] = {}
    for node in nodes:
        if not isinstance(node, dict):
            raise GateError("INCONCLUSIVE_AX", "Accessibility node is not an object")
        for field in ("title", "description", "value"):
            value = node.get(field)
            if isinstance(value, str) and value:
                texts.add(value)
        if node.get("role") == "AXButton" and isinstance(node.get("title"), str):
            title = node["title"]
            if title in buttons:
                raise GateError("INCONCLUSIVE_AX", f"ambiguous Accessibility button: {title}")
            buttons[title] = node
    windows = raw.get("windows")
    if not isinstance(windows, list) or len(windows) > 1:
        raise GateError("AMBIGUOUS_NATIVE_WINDOW", "expected at most one matching native Paint.R window")
    window: dict[str, Any] | None = None
    if windows:
        candidate = windows[0]
        if not isinstance(candidate, dict):
            raise GateError("INCONCLUSIVE_WINDOW", "native window evidence is not an object")
        bounds = candidate.get("bounds")
        valid_bounds = isinstance(bounds, dict) and all(
            isinstance(bounds.get(key), (int, float)) and math.isfinite(bounds[key]) for key in ("x", "y", "width", "height")
        )
        if (
            candidate.get("pid") != pid
            or candidate.get("title") != WINDOW_TITLE
            or candidate.get("on_screen") is not True
            or candidate.get("main") is not True
            or not isinstance(candidate.get("id"), int)
            or isinstance(candidate.get("id"), bool)
            or candidate["id"] <= 0
            or not valid_bounds
            or bounds["width"] <= 0
            or bounds["height"] <= 0
        ):
            raise GateError("INCONCLUSIVE_WINDOW", "native window identity, visibility, or bounds are invalid")
        window = candidate
    return ObservedWindow(raw, window, texts, buttons)


def validate_screenshot(path: Path, observation: ObservedWindow) -> dict[str, Any]:
    if observation.window_id is None or path.is_symlink() or not path.is_file():
        raise GateError("SCREENSHOT_INVALID", "screenshot or source native window is absent")
    digest, size = sha256_file(path)
    if size < 45:
        raise GateError("SCREENSHOT_INVALID", "screenshot is empty or too small")
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise GateError("SCREENSHOT_INVALID", "screenshot is not a PNG")
    offset = 8
    width = height = None
    saw_idat = False
    saw_iend = False
    while offset < len(data):
        if offset + 12 > len(data):
            raise GateError("SCREENSHOT_INVALID", "truncated PNG chunk")
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4 : offset + 8]
        end = offset + 12 + length
        if end > len(data):
            raise GateError("SCREENSHOT_INVALID", "truncated PNG payload")
        payload = data[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack_from(">I", data, offset + 8 + length)[0]
        if zlib.crc32(kind + payload) != expected_crc:
            raise GateError("SCREENSHOT_INVALID", "PNG checksum mismatch")
        if offset == 8:
            if kind != b"IHDR" or length != 13:
                raise GateError("SCREENSHOT_INVALID", "PNG lacks a leading IHDR")
            width, height = struct.unpack_from(">II", payload, 0)
        elif kind == b"IDAT":
            saw_idat = True
        elif kind == b"IEND":
            if length != 0 or end != len(data):
                raise GateError("SCREENSHOT_INVALID", "invalid PNG IEND")
            saw_iend = True
        offset = end
    if not width or not height or width > 16384 or height > 16384 or width * height > MAX_SCREENSHOT_PIXELS:
        raise GateError("SCREENSHOT_INVALID", "PNG dimensions are absent or outside the evidence bound")
    if not saw_idat or not saw_iend:
        raise GateError("SCREENSHOT_INVALID", "PNG lacks image data or IEND")
    oracle = _png_oracle()
    try:
        decoded = oracle.decode_png(
            data,
            limits=oracle.Limits(max_pixels=MAX_SCREENSHOT_PIXELS, max_compressed_bytes=MAX_SCREENSHOT_BYTES),
        )
    except (ValueError, struct.error, zlib.error) as error:
        raise GateError("SCREENSHOT_INVALID", f"PNG pixel decoding failed: {error}") from error
    if (decoded.width, decoded.height) != (width, height) or len(decoded.pixels) != width * height * 4:
        raise GateError("SCREENSHOT_INVALID", "decoded PNG dimensions or RGBA length are inconsistent")
    return {
        "path": path.name,
        "sha256": digest,
        "rgba_sha256": sha256_bytes(decoded.pixels),
        "bytes": size,
        "width": width,
        "height": height,
        "pid": observation.raw["process"]["pid"],
        "window_id": observation.window_id,
    }


class Evidence:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.parent.mkdir(parents=True, exist_ok=True)
        self.root.mkdir(mode=0o700)
        (self.root / "observations").mkdir()
        self.events_path = self.root / "events.jsonl"
        self.events_file = self.events_path.open("x", encoding="utf-8")
        self.started = time.monotonic()
        self.sequence = 0
        self.assertions: list[dict[str, Any]] = []
        self.observations: list[dict[str, Any]] = []
        self.screenshots: list[dict[str, Any]] = []

    def record(self, kind: str, **fields: Any) -> None:
        self.sequence += 1
        event = {
            "sequence": self.sequence,
            "at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "elapsed_seconds": round(time.monotonic() - self.started, 6),
            "kind": kind,
            **fields,
        }
        self.events_file.write(json.dumps(event, sort_keys=True, ensure_ascii=False) + "\n")
        self.events_file.flush()

    def assertion(self, name: str, passed: bool, **details: Any) -> None:
        entry = {"name": name, "passed": passed, **details}
        self.assertions.append(entry)
        self.record("assertion", **entry)

    def observation(self, label: str, observed: ObservedWindow) -> None:
        name = f"{len(self.observations):02d}-{re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-')}.json"
        relative = Path("observations") / name
        write_json_new(self.root / relative, observed.raw)
        digest, size = sha256_file(self.root / relative, maximum=MAX_NATIVE_OUTPUT)
        entry = {"label": label, "path": relative.as_posix(), "sha256": digest, "bytes": size, "window_id": observed.window_id}
        self.observations.append(entry)
        self.record("observation", **entry)

    def screenshot(self, path: Path, observed: ObservedWindow) -> dict[str, Any]:
        entry = validate_screenshot(path, observed)
        entry["path"] = path.relative_to(self.root).as_posix()
        self.screenshots.append(entry)
        self.record("screenshot", **entry)
        return entry

    def finish(self, manifest: dict[str, Any]) -> dict[str, Any]:
        self.events_file.flush()
        self.events_file.close()
        manifest["assertions"] = self.assertions
        manifest["observations"] = self.observations
        manifest["screenshots"] = self.screenshots
        manifest["events"] = {
            "path": self.events_path.name,
            "sha256": sha256_file(self.events_path, maximum=MAX_NATIVE_OUTPUT)[0],
        }
        destination = self.root / "manifest.json"
        temporary = self.root / ".manifest.json.tmp"
        write_json_new(temporary, manifest)
        temporary.replace(destination)
        return json.loads(destination.read_text(encoding="utf-8"))


class NativeUsabilityGate:
    def __init__(self, backend: Any, binary: Path, output_dir: Path, *, config: GateConfig | None = None) -> None:
        self.config = config or GateConfig()
        self.config.validate()
        self.binary = Path(binary).resolve(strict=True)
        if not self.binary.is_file() or not os.access(self.binary, os.X_OK):
            raise ValueError(f"not an executable Paint.R binary: {self.binary}")
        self.backend = backend
        self.harness_evidence = harness_identity()
        self.evidence = Evidence(Path(output_dir))
        self.process: Any | None = None
        self.deadline = 0.0
        self.cleanup: dict[str, Any] = {"required": False}
        self.binary_evidence = self._binary_evidence()

    def _binary_evidence(self) -> dict[str, Any]:
        digest, size = sha256_file(self.binary, maximum=MAX_BINARY_BYTES)
        return {"path": str(self.binary), "sha256": digest, "bytes": size}

    def _native(self, description: str, operation: Callable[[], Any]) -> Any:
        try:
            return operation()
        except NativeCommandError as error:
            if error.code in BLOCKED_CODES:
                raise GateBlocked(error.code, f"{description}: {error}") from error
            if error.code == "AX_UNAVAILABLE":
                raise GateError("AX_UNAVAILABLE", f"{description}: {error}") from error
            raise GateError("NATIVE_COMMAND_FAILED", f"{description}: {error} ({error.code})") from error

    def _require(self, condition: bool, name: str, code: str, message: str) -> None:
        self.evidence.assertion(name, condition, message=message)
        if not condition:
            raise GateError(code, message)

    def _sleep(self, seconds: float) -> None:
        if seconds > 0:
            time.sleep(min(seconds, max(0.0, self.deadline - time.monotonic())))
        if time.monotonic() >= self.deadline:
            raise GateError("OVERALL_TIMEOUT", "native gate exceeded its overall deadline")

    def _observe(self) -> ObservedWindow:
        assert self.process is not None
        raw = self._native("observe native window", lambda: self.backend.observe(self.process.pid))
        return parse_observation(raw, self.process.pid)

    def _wait_window(
        self,
        assertion_name: str,
        timeout: float,
        predicate: Callable[[ObservedWindow], bool],
    ) -> ObservedWindow:
        assert self.process is not None
        end = min(time.monotonic() + timeout, self.deadline)
        last = "no observation"
        while time.monotonic() < end:
            code = self.process.poll()
            if code is not None:
                self.evidence.assertion(assertion_name, False, exit_code=code)
                raise GateError("UNEXPECTED_PROCESS_EXIT", f"Paint.R exited unexpectedly with status {code}")
            try:
                observed = self._observe()
                last = f"window={observed.window_id}, texts={sorted(observed.texts)}"
                if observed.window is not None and predicate(observed):
                    self.evidence.assertion(assertion_name, True)
                    return observed
            except GateError as error:
                if error.code == "AX_UNAVAILABLE":
                    last = str(error)
                    self._sleep(min(self.config.poll_interval, max(0.0, end - time.monotonic())))
                    continue
                if error.code in {"INCONCLUSIVE_AX", "AMBIGUOUS_NATIVE_WINDOW", "INCONCLUSIVE_WINDOW", "INCONCLUSIVE_PROCESS"}:
                    self.evidence.assertion(assertion_name, False, message=str(error))
                    raise
                raise
            self._sleep(min(self.config.poll_interval, max(0.0, end - time.monotonic())))
        self.evidence.assertion(assertion_name, False, last=last)
        raise GateError("OBSERVATION_TIMEOUT", f"timed out waiting for {assertion_name}; {last}")

    def _capture(self, name: str, observed: ObservedWindow) -> dict[str, Any]:
        path = self.evidence.root / name
        self._native("capture native window", lambda: self.backend.capture(observed.raw, path))
        return self.evidence.screenshot(path, observed)

    def _wait_exit(self) -> int:
        assert self.process is not None
        end = min(time.monotonic() + self.config.quit_timeout, self.deadline)
        while time.monotonic() < end:
            code = self.process.poll()
            if code is not None:
                self.evidence.assertion("explicit_discard_exit_zero", code == 0, exit_code=code)
                if code != 0:
                    raise GateError("NONZERO_NATIVE_EXIT", f"Paint.R exited with status {code} after Discard")
                return code
            self._sleep(min(self.config.poll_interval, max(0.0, end - time.monotonic())))
        self.evidence.assertion("explicit_discard_exit_zero", False)
        raise GateError("NATIVE_EXIT_TIMEOUT", "Paint.R did not exit after explicit Discard")

    def _sequence(self) -> int:
        assert self.process is not None
        pid = self.process.pid
        initial = self._wait_window(
            "initial_clean_native_window",
            self.config.startup_timeout,
            lambda item: item.clean() and not item.close_dialog(),
        )
        self.evidence.observation("before", initial)
        before = self._capture("00-before.png", initial)
        window_id = initial.window_id

        self.evidence.record("native_input", action="key", key="i", modifiers=["command"], result=self._native("send Command-I", lambda: self.backend.key(pid, "i", ("command",))))
        modified = self._wait_window(
            "command_i_modified_document",
            self.config.observation_timeout,
            lambda item: item.window_id == window_id and item.modified() and not item.close_dialog(),
        )
        self.evidence.observation("modified", modified)
        after = self._capture("01-modified.png", modified)
        self._require(
            before["rgba_sha256"] != after["rgba_sha256"],
            "before_after_screenshots_differ",
            "SCREENSHOT_NOT_DISTINCT",
            "before/after native screenshot RGBA pixels are identical; visual modification is inconclusive",
        )

        self.evidence.record("native_input", action="AXPress", title="AXCloseButton", result=self._native("press native window close button", lambda: self.backend.close_window(pid, WINDOW_TITLE)))
        self._wait_window(
            "dirty_close_confirmation_observed",
            self.config.observation_timeout,
            lambda item: item.window_id == window_id and item.modified() and item.close_dialog(),
        )
        self._sleep(self.config.confirmation_hold)
        if self.process.poll() is not None:
            self.evidence.assertion("dirty_close_blocked_by_confirmation", False)
            raise GateError("UNEXPECTED_PROCESS_EXIT", "dirty close exited instead of retaining the confirmation")
        confirmation = self._observe()
        self._require(
            confirmation.window_id == window_id and confirmation.modified() and confirmation.close_dialog(),
            "dirty_close_blocked_by_confirmation",
            "DIRTY_CLOSE_NOT_BLOCKED",
            "dirty close confirmation did not remain observable",
        )
        self.evidence.observation("close-confirmation", confirmation)
        confirmation_image = self._capture("02-close-confirmation.png", confirmation)

        self.evidence.record("native_input", action="key", key="escape", modifiers=[], result=self._native("send Escape to cancel", lambda: self.backend.key(pid, "escape")))
        canceled = self._wait_window(
            "cancel_preserved_modified_session",
            self.config.observation_timeout,
            lambda item: item.window_id == window_id and item.modified() and not item.close_dialog(),
        )
        self.evidence.observation("canceled", canceled)
        self._require(
            canceled.texts == modified.texts,
            "cancel_preserved_accessible_document_state",
            "CANCEL_CHANGED_DOCUMENT_STATE",
            "accessible document text changed while canceling the dirty close",
        )
        canceled_image = self._capture("03-canceled.png", canceled)
        self._require(
            confirmation_image["rgba_sha256"] != canceled_image["rgba_sha256"],
            "cancel_screenshot_differs_from_confirmation",
            "SCREENSHOT_NOT_DISTINCT",
            "confirmation and canceled screenshot RGBA pixels are identical",
        )

        self.evidence.record("native_input", action="AXPress", title="AXCloseButton", result=self._native("press native window close button again", lambda: self.backend.close_window(pid, WINDOW_TITLE)))
        second = self._wait_window(
            "second_dirty_close_confirmation",
            self.config.observation_timeout,
            lambda item: item.window_id == window_id and item.modified() and item.close_dialog(),
        )
        self.evidence.observation("second-close-confirmation", second)
        self._capture("04-second-close-confirmation.png", second)

        self.evidence.record("native_input", action="AXPress", title="Discard", result=self._native("activate Discard", lambda: self.backend.press(pid, "Discard")))
        return self._wait_exit()

    def _cleanup(self) -> None:
        process = self.process
        if process is None or process.poll() is not None:
            return
        self.cleanup = {
            "required": True,
            "pid": process.pid,
            "graceful_native_close_attempted": True,
            "sigterm_sent": False,
            "sigkill_sent": False,
        }
        self.evidence.record("cleanup_start", pid=process.pid)
        end = time.monotonic() + self.config.cleanup_timeout
        try:
            observed = parse_observation(self.backend.observe(process.pid), process.pid)
            if observed.close_dialog():
                self.backend.press(process.pid, "Discard")
            else:
                self.backend.key(process.pid, "q", ("command",))
            while process.poll() is None and time.monotonic() < end:
                try:
                    observed = parse_observation(self.backend.observe(process.pid), process.pid)
                    if observed.close_dialog():
                        self.backend.press(process.pid, "Discard")
                except (NativeCommandError, GateError):
                    pass
                time.sleep(min(0.05, max(0.0, end - time.monotonic())))
            self.cleanup["graceful_close_succeeded"] = process.poll() is not None
        except Exception as error:  # cleanup must still bound and reap only the spawned child
            self.cleanup["graceful_close_succeeded"] = False
            self.cleanup["graceful_error"] = str(error)

        if process.poll() is None:
            self.cleanup["sigterm_sent"] = True
            self.evidence.record("cleanup_signal", signal="TERM", pid=process.pid)
            process.terminate()
            signal_end = time.monotonic() + 2.0
            while process.poll() is None and time.monotonic() < signal_end:
                time.sleep(0.02)
        if process.poll() is None:
            self.cleanup["sigkill_sent"] = True
            self.evidence.record("cleanup_signal", signal="KILL", pid=process.pid)
            process.kill()
            signal_end = time.monotonic() + 2.0
            while process.poll() is None and time.monotonic() < signal_end:
                time.sleep(0.02)
        self.cleanup["exit_code"] = process.poll()
        self.cleanup["process_reaped"] = process.poll() is not None
        self.evidence.record("cleanup_finished", **self.cleanup)

    def run(self) -> dict[str, Any]:
        self.deadline = time.monotonic() + self.config.overall_timeout
        outcome = "PASS"
        error: dict[str, Any] | None = None
        process_evidence: dict[str, Any] = {"pid": None, "exit_code": None}
        self.evidence.record("gate_start", binary=self.binary_evidence, config=dataclasses.asdict(self.config))
        try:
            try:
                state = self.backend.preflight()
            except NativeCommandError as native_error:
                raise GateBlocked(native_error.code, str(native_error)) from native_error
            self.evidence.record("preflight", state=state)
            problem = preflight_problem(state)
            if problem is not None:
                raise GateBlocked(problem.code, str(problem))
            self.process = self.backend.launch(self.binary, self.evidence.root / "paint-ui.log")
            process_evidence["pid"] = self.process.pid
            self.evidence.record("process_launch", pid=self.process.pid, native_arguments=[])
            process_evidence["exit_code"] = self._sequence()
        except GateError as caught:
            outcome = caught.outcome
            error = {"code": caught.code, "message": str(caught)}
            self.evidence.record("gate_error", **error)
        except KeyboardInterrupt:
            outcome = "FAIL"
            error = {"code": "INTERRUPTED", "message": "native gate interrupted"}
            self.evidence.record("gate_error", **error)
        except Exception as caught:
            outcome = "FAIL"
            error = {"code": "GATE_ERROR", "message": f"{type(caught).__name__}: {caught}"}
            self.evidence.record("gate_error", **error)
        finally:
            if self.process is not None:
                process_evidence["exit_code"] = self.process.poll()
            try:
                self._cleanup()
            except Exception as cleanup_error:
                self.cleanup["cleanup_error"] = str(cleanup_error)
            if self.process is not None:
                process_evidence["exit_code"] = self.process.poll()

        native_execution = "NOT_RUN" if self.process is None else "INCOMPLETE"
        if outcome == "PASS" and process_evidence["exit_code"] == 0:
            native_execution = "COMPLETED"
        manifest: dict[str, Any] = {
            "schema_version": 1,
            "gate": "macos-native-keyboard-dirty-close",
            "outcome": outcome,
            "native_execution": native_execution,
            "evidence_scope": getattr(self.backend, "evidence_scope", "fake-no-native-evidence"),
            "error": error,
            "binary": self.binary_evidence,
            "harness": self.harness_evidence,
            "process": process_evidence,
            "cleanup": self.cleanup,
            "limits": dataclasses.asdict(self.config),
            "max_screenshots": 5,
            "native_acceptance": "PASS requires real CGEvent/AX/screencapture evidence with decoded RGBA comparison; semantic/headless tests are not substitutes.",
        }
        log_path = self.evidence.root / "paint-ui.log"
        if log_path.is_file():
            digest, size = sha256_file(log_path, maximum=MAX_BINARY_BYTES)
            manifest["process_log"] = {"path": log_path.name, "sha256": digest, "bytes": size}
        return self.evidence.finish(manifest)


class MacOSNativeBackend:
    evidence_scope = "native-macos"

    def __init__(self, helper_source: Path, work_dir: Path, config: GateConfig) -> None:
        self.helper_source = helper_source
        self.work_dir = work_dir
        self.helper = work_dir / "macos-native-ui"
        self.config = config

    def _ensure_helper(self) -> None:
        if platform.system() != "Darwin":
            raise NativeCommandError("UNSUPPORTED_PLATFORM", "the native gate requires macOS")
        if self.helper.is_file():
            return
        self.work_dir.mkdir(parents=True, exist_ok=True)
        for name in ("clang", "swift", "tmp"):
            (self.work_dir / name).mkdir(exist_ok=True)
        environment = os.environ.copy()
        environment.update(
            CLANG_MODULE_CACHE_PATH=str(self.work_dir / "clang"),
            SWIFT_MODULECACHE_PATH=str(self.work_dir / "swift"),
            TMPDIR=str(self.work_dir / "tmp"),
        )
        result = run_native_command(
            ["/usr/bin/swiftc", "-O", "-swift-version", "5", str(self.helper_source), "-o", str(self.helper)],
            timeout=60.0,
            env=environment,
        )
        if result.returncode != 0:
            raise NativeCommandError("HELPER_BUILD_FAILED", result.stderr.decode("utf-8", "replace")[:4000])

    def _call(self, *arguments: str) -> dict[str, Any]:
        self._ensure_helper()
        result = run_native_command([str(self.helper), *arguments], timeout=self.config.command_timeout)
        if result.returncode != 0:
            message = result.stderr.decode("utf-8", "replace").strip()
            match = re.search(r"native-ui-error: ([A-Z_]+): (.*)", message)
            if match:
                raise NativeCommandError(match.group(1), match.group(2))
            raise NativeCommandError("NATIVE_HELPER_FAILED", message or f"helper exited {result.returncode}")
        try:
            value = json.loads(result.stdout)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise NativeCommandError("NATIVE_PROTOCOL_ERROR", "native helper returned invalid JSON") from error
        if not isinstance(value, dict):
            raise NativeCommandError("NATIVE_PROTOCOL_ERROR", "native helper JSON is not an object")
        return value

    def preflight(self) -> dict[str, Any]:
        return self._call("preflight")

    def launch(self, binary: Path, log_path: Path) -> subprocess.Popen[bytes]:
        with log_path.open("xb") as log:
            return subprocess.Popen(
                [str(binary)],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                close_fds=True,
            )

    def observe(self, pid: int) -> dict[str, Any]:
        return self._call("observe", str(pid), WINDOW_TITLE)

    def key(self, pid: int, name: str, modifiers: tuple[str, ...] = ()) -> dict[str, Any]:
        allowed = {("i", ("command",)), ("q", ("command",)), ("escape", ())}
        if (name, modifiers) not in allowed:
            raise NativeCommandError("INPUT_REJECTED", "native key is outside the gate allowlist")
        return self._call("key", str(pid), WINDOW_TITLE, name, ",".join(modifiers))

    def close_window(self, pid: int, title: str) -> dict[str, Any]:
        if title != WINDOW_TITLE:
            raise NativeCommandError("INPUT_REJECTED", "only the expected Paint.R window may be closed")
        return self._call("close-window", str(pid), title)

    def press(self, pid: int, title: str) -> dict[str, Any]:
        if title != "Discard":
            raise NativeCommandError("INPUT_REJECTED", "only explicit Discard is allowed")
        return self._call("press", str(pid), WINDOW_TITLE, title)

    def capture(self, observed: dict[str, Any], output: Path) -> None:
        problem = preflight_problem(self.preflight())
        if problem is not None:
            raise problem
        window = observed["windows"][0]
        result = run_native_command(
            ["/usr/sbin/screencapture", "-x", "-l", str(window["id"]), str(output)],
            timeout=self.config.command_timeout,
        )
        if result.returncode != 0:
            raise NativeCommandError("SCREENSHOT_FAILED", result.stderr.decode("utf-8", "replace")[:2000])


def parse_arguments(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="current-source native paint-ui executable")
    parser.add_argument("--output-dir", type=Path, required=True, help="new evidence directory (never reused)")
    for name in ("startup", "observation", "quit", "cleanup", "command", "overall"):
        parser.add_argument(f"--{name}-timeout", type=float)
    parser.add_argument("--confirmation-hold", type=float)
    args = parser.parse_args(argv)
    values = {}
    for name in ("startup", "observation", "quit", "cleanup", "command", "overall"):
        value = getattr(args, f"{name}_timeout")
        if value is not None:
            values[f"{name}_timeout"] = value
    if args.confirmation_hold is not None:
        values["confirmation_hold"] = args.confirmation_hold
    args.config = GateConfig(**values)
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_arguments(argv)
    output = args.output_dir.resolve()
    try:
        backend = MacOSNativeBackend(Path(__file__).with_suffix(".swift"), output / ".native-tool", args.config)
        result = NativeUsabilityGate(backend, args.binary, output, config=args.config).run()
    except (OSError, ValueError) as error:
        print(f"native gate setup failed: {error}", file=sys.stderr)
        return 66
    print(json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False))
    if result["outcome"] == "PASS":
        return 0
    if result["outcome"] == "BLOCKED":
        return 75
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
