"""Cross-platform tests for the macOS native gate's orchestration.

No test launches Paint.R, accesses AppKit, or sends native input. Native transport
is replaced by a deterministic fake; production failures therefore cannot be
hidden behind a headless/session-only acceptance result.
"""

from __future__ import annotations

import copy
import hashlib
import json
import struct
import subprocess
import sys
import tempfile
import time
import unittest
import zlib
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import macos_native_usability_gate as gate


WINDOW_TITLE = "Paint.R Development Preview"
INVERTED = "Inverted colors on the active layer"
CLOSE_EXPLANATION = "Closing Paint.R will discard the modified document and its undo history."


def png_bytes(red: int, green: int, blue: int, *, metadata: int = 0) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    chunks = [b"\x89PNG\r\n\x1a\n", chunk(b"IHDR", header)]
    if metadata:
        chunks.append(chunk(b"tEXt", f"paint-r\0capture-{metadata}".encode("ascii")))
    chunks.extend([chunk(b"IDAT", zlib.compress(bytes([0, red, green, blue]))), chunk(b"IEND", b"")])
    return b"".join(chunks)


def preflight(**changes: object) -> dict[str, object]:
    result: dict[str, object] = {
        "console_session": True,
        "session_dictionary_available": True,
        "screen_locked": False,
        "screen_lock_state_known": True,
        "accessibility_trusted": True,
        "screen_recording": True,
    }
    result.update(changes)
    return result


def observation(
    pid: int,
    *,
    running: bool = True,
    modified: bool = False,
    dialog: bool = False,
    window_id: int = 71,
    truncated: bool = False,
    cancel_focused: bool = True,
    extra_text: str | None = None,
) -> dict[str, object]:
    status = INVERTED if modified else "New 800×600 opaque-white document"
    nodes: list[dict[str, object]] = [
        {"path": [0], "role": "AXStaticText", "value": "Modified" if modified else "Clean"},
        {"path": [1], "role": "AXStaticText", "value": status},
        {"path": [2], "role": "AXImage", "description": "Composed document canvas"},
    ]
    if extra_text is not None:
        nodes.append({"path": [7], "role": "AXStaticText", "value": extra_text})
    if dialog:
        nodes.extend(
            [
                {"path": [3], "role": "AXWindow", "title": "Discard unsaved changes?"},
                {"path": [4], "role": "AXStaticText", "value": CLOSE_EXPLANATION},
                {"path": [5], "role": "AXButton", "title": "Cancel", "enabled": True, "focused": cancel_focused},
                {"path": [6], "role": "AXButton", "title": "Discard", "enabled": True, "focused": False},
            ]
        )
    windows = []
    if running:
        windows.append(
            {
                "id": window_id,
                "pid": pid,
                "title": WINDOW_TITLE,
                "on_screen": True,
                "main": True,
                "focused": True,
                "bounds": {"x": 10.0, "y": 20.0, "width": 1280.0, "height": 900.0},
            }
        )
    return {
        "schema_version": 1,
        "process": {"pid": pid, "running": running, "frontmost": running},
        "windows": windows,
        "ax": {"node_count": len(nodes), "truncated": truncated, "nodes": nodes},
    }


class FakeProcess:
    def __init__(self, pid: int = 4242) -> None:
        self.pid = pid
        self.returncode: int | None = None
        self.terminate_calls = 0
        self.kill_calls = 0
        self.stubborn = False
        self.never_die = False

    def poll(self) -> int | None:
        return self.returncode

    def wait(self, timeout: float | None = None) -> int:
        del timeout
        if self.returncode is None:
            raise subprocess.TimeoutExpired("paint-ui", 0)
        return self.returncode

    def terminate(self) -> None:
        self.terminate_calls += 1
        if not self.stubborn and not self.never_die:
            self.returncode = -15

    def kill(self) -> None:
        self.kill_calls += 1
        if not self.never_die:
            self.returncode = -9


class FakeBackend:
    """In-memory native model used only to test orchestration decisions."""

    def __init__(self) -> None:
        self.preflight_result = preflight()
        self.process = FakeProcess()
        self.modified = False
        self.dialog = False
        self.actions: list[tuple[object, ...]] = []
        self.ignore_invert = False
        self.exit_on_dirty_close = False
        self.cancel_clears_modified = False
        self.discard_exit_code = 0
        self.ignore_discard = False
        self.fail_cleanup = False
        self.blank_screenshots = False
        self.metadata_only_screenshots = False
        self.metadata_only_close_screenshots = False
        self.capture_sequence = 0
        self.fail_screenshots = False
        self.truncated_ax = False
        self.ax_unavailable_observations = 0
        self.cancel_unfocused = False
        self.cancel_extra_text: str | None = None
        self.cancel_finished = False
        self.window_id = 71
        self.lock_during_invert = False

    def launch(self, binary: Path, log_path: Path) -> FakeProcess:
        self.actions.append(("launch", binary, log_path))
        Path(log_path).write_text("fake native process log\n", encoding="utf-8")
        return self.process

    def preflight(self) -> dict[str, object]:
        self.actions.append(("preflight",))
        return copy.deepcopy(self.preflight_result)

    def _check_lock(self) -> None:
        if self.lock_during_invert and self.modified:
            raise gate.NativeCommandError("SCREEN_LOCKED", "screen became locked")

    def observe(self, pid: int) -> dict[str, object]:
        self._check_lock()
        self.actions.append(("observe", pid))
        if self.ax_unavailable_observations > 0:
            self.ax_unavailable_observations -= 1
            raise gate.NativeCommandError("AX_UNAVAILABLE", "simulated startup AX delay")
        return observation(
            pid,
            running=self.process.returncode is None,
            modified=self.modified,
            dialog=self.dialog,
            window_id=self.window_id,
            truncated=self.truncated_ax,
            cancel_focused=not self.cancel_unfocused,
            extra_text=self.cancel_extra_text if self.cancel_finished else None,
        )

    def close_window(self, pid: int, title: str) -> dict[str, object]:
        self._check_lock()
        self.actions.append(("close_window", pid))
        if self.modified:
            if self.exit_on_dirty_close:
                self.process.returncode = 0
            else:
                self.dialog = True
        else:
            self.process.returncode = 0
        return {"event": "fake AXPress", "pid": pid, "title": title, "role": "AXCloseButton"}

    def key(self, pid: int, name: str, modifiers: tuple[str, ...] = ()) -> dict[str, object]:
        self._check_lock()
        self.actions.append(("key", pid, name, modifiers))
        if self.fail_cleanup:
            return {"event": "simulated native command failure"}
        if (name, modifiers) == ("i", ("command", "shift")):
            if not self.ignore_invert:
                self.modified = True
            if self.lock_during_invert:
                raise gate.NativeCommandError("SCREEN_LOCKED", "screen became locked")
        elif (name, modifiers) in {("q", ("command",)), ("w", ("command",))}:
            if self.modified:
                if self.exit_on_dirty_close:
                    self.process.returncode = 0
                else:
                    self.dialog = True
            else:
                self.process.returncode = 0
        elif (name, modifiers) == ("escape", ()):
            self.dialog = False
            self.cancel_finished = True
            if self.cancel_clears_modified:
                self.modified = False
        else:
            raise AssertionError(f"unexpected native key {name} {modifiers}")
        return {"event": "fake CGEvent", "pid": pid, "key": name, "modifiers": list(modifiers)}

    def press(self, pid: int, title: str) -> dict[str, object]:
        self._check_lock()
        self.actions.append(("press", pid, title))
        if title != "Discard" or not self.dialog:
            raise gate.NativeCommandError("AX_ACTION_FAILED", "Discard confirmation is absent")
        if not self.fail_cleanup and not self.ignore_discard:
            self.dialog = False
            self.process.returncode = self.discard_exit_code
        return {"event": "fake AXPress", "pid": pid, "title": title}

    def capture(self, observed: dict[str, object], output: Path) -> None:
        self.actions.append(("capture", observed["windows"][0]["id"], output.name))
        self.capture_sequence += 1
        if self.fail_screenshots:
            raise gate.NativeCommandError("SCREENSHOT_FAILED", "simulated capture failure")
        metadata = self.capture_sequence if self.metadata_only_screenshots else 0
        if self.metadata_only_close_screenshots and self.capture_sequence >= 3:
            color = (128, 128, 128)
            metadata = self.capture_sequence
        elif self.metadata_only_screenshots or self.blank_screenshots:
            color = (0, 0, 0)
        elif observed["ax"]["nodes"][-1]["title"] == "Discard" if observed["ax"]["nodes"][-1].get("title") else False:
            color = (128, 128, 128)
        elif any(node.get("title") == "Discard unsaved changes?" for node in observed["ax"]["nodes"]):
            color = (128, 128, 128)
        elif any(node.get("value") == "Modified" for node in observed["ax"]["nodes"]):
            color = (255, 255, 255)
        else:
            color = (0, 0, 0)
        output.write_bytes(png_bytes(*color, metadata=metadata))


class NativeGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="paint-r-native-gate-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "paint-ui"
        self.binary.write_bytes(b"fake test binary; never executed\n")
        self.binary.chmod(0o755)
        self.backend = FakeBackend()
        self.config = gate.GateConfig(
            startup_timeout=0.05,
            observation_timeout=0.05,
            quit_timeout=0.05,
            cleanup_timeout=0.05,
            command_timeout=0.05,
            overall_timeout=2.0,
            confirmation_hold=0.0,
            poll_interval=0.002,
        )

    def run_gate(self) -> dict[str, object]:
        output = self.root / "evidence"
        runner = gate.NativeUsabilityGate(self.backend, self.binary, output, config=self.config)
        result = runner.run()
        self.assertEqual(result, json.loads((output / "manifest.json").read_text(encoding="utf-8")))
        return result

    def test_complete_native_sequence_passes_only_with_all_evidence(self) -> None:
        result = self.run_gate()

        self.assertEqual(result["outcome"], "PASS")
        self.assertEqual(result["native_execution"], "COMPLETED")
        self.assertEqual(result["process"]["exit_code"], 0)
        self.assertEqual(self.backend.process.terminate_calls, 0)
        self.assertEqual(self.backend.process.kill_calls, 0)
        native_actions = [action for action in self.backend.actions if action[0] in {"key", "close_window", "press"}]
        self.assertEqual(
            native_actions,
            [
                ("key", 4242, "i", ("command", "shift")),
                ("key", 4242, "q", ("command",)),
                ("key", 4242, "escape", ()),
                ("close_window", 4242),
                ("press", 4242, "Discard"),
            ],
        )
        self.assertEqual(len(result["screenshots"]), 5)
        self.assertEqual(len(result["observations"]), 5)
        self.assertTrue(all(path.is_file() for path in (self.root / "evidence").rglob("*.png")))
        self.assertNotEqual(result["screenshots"][0]["sha256"], result["screenshots"][1]["sha256"])
        self.assertNotEqual(result["screenshots"][0]["rgba_sha256"], result["screenshots"][1]["rgba_sha256"])
        self.assertTrue(all(len(screenshot["rgba_sha256"]) == 64 for screenshot in result["screenshots"]))
        assertion_names = {item["name"] for item in result["assertions"] if item["passed"]}
        self.assertTrue(
            {
                "initial_clean_native_window",
                "invert_colors_modified_document",
                "dirty_close_blocked_by_confirmation",
                "cancel_preserved_modified_session",
                "second_dirty_close_confirmation",
                "explicit_discard_exit_zero",
            }.issubset(assertion_names)
        )

    def test_locked_screen_is_blocked_before_process_launch(self) -> None:
        self.backend.preflight_result = preflight(screen_locked=True)

        result = self.run_gate()

        self.assertEqual(result["outcome"], "BLOCKED")
        self.assertEqual(result["native_execution"], "NOT_RUN")
        self.assertEqual(result["error"]["code"], "SCREEN_LOCKED")
        self.assertFalse(any(action[0] == "launch" for action in self.backend.actions))
        self.assertFalse(any(action[0] in {"key", "press"} for action in self.backend.actions))

    def test_unavailable_permissions_and_inconclusive_session_are_not_run(self) -> None:
        cases = [
            (preflight(accessibility_trusted=False), "ACCESSIBILITY_UNAVAILABLE"),
            (preflight(screen_recording=False), "SCREEN_RECORDING_UNAVAILABLE"),
            (preflight(console_session=False), "NO_CONSOLE_SESSION"),
            (preflight(session_dictionary_available=False), "SESSION_STATE_UNAVAILABLE"),
            (preflight(screen_lock_state_known=False), "SESSION_STATE_UNAVAILABLE"),
        ]
        for state, code in cases:
            with self.subTest(code=code):
                self.setUp()
                self.backend.preflight_result = state
                result = self.run_gate()
                self.assertEqual(result["outcome"], "BLOCKED")
                self.assertEqual(result["native_execution"], "NOT_RUN")
                self.assertEqual(result["error"]["code"], code)
                self.assertFalse(any(action[0] == "launch" for action in self.backend.actions))

    def test_process_exit_on_dirty_close_is_failure_not_success(self) -> None:
        self.backend.exit_on_dirty_close = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["native_execution"], "INCOMPLETE")
        self.assertEqual(result["error"]["code"], "UNEXPECTED_PROCESS_EXIT")

    def test_process_alive_without_modified_ax_state_does_not_pass(self) -> None:
        self.backend.ignore_invert = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "OBSERVATION_TIMEOUT")
        self.assertFalse(any(item["name"] == "invert_colors_modified_document" and item["passed"] for item in result["assertions"]))

    def test_transient_ax_unavailability_is_retried_within_startup(self) -> None:
        self.backend.ax_unavailable_observations = 2

        result = self.run_gate()

        self.assertEqual(result["outcome"], "PASS")
        self.assertEqual(result["native_execution"], "COMPLETED")
        observed = [action for action in self.backend.actions if action[0] == "observe"]
        self.assertGreaterEqual(len(observed), 3)

    def test_cancel_must_preserve_modified_session_and_window(self) -> None:
        self.backend.cancel_clears_modified = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "OBSERVATION_TIMEOUT")
        self.assertFalse(any(item["name"] == "cancel_preserved_modified_session" and item["passed"] for item in result["assertions"]))

    def test_cancel_rejects_unexpected_accessible_document_change(self) -> None:
        self.backend.cancel_extra_text = "Document replaced while canceling"

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "CANCEL_CHANGED_DOCUMENT_STATE")
        self.assertTrue(any(item["name"] == "cancel_preserved_modified_session" and item["passed"] for item in result["assertions"]))
        self.assertFalse(any(item["name"] == "cancel_preserved_accessible_document_state" and item["passed"] for item in result["assertions"]))
        self.assertFalse(any(item["name"] == "second_dirty_close_confirmation" and item["passed"] for item in result["assertions"]))

    def test_cancel_must_be_focused_before_close_can_be_accepted(self) -> None:
        self.backend.cancel_unfocused = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "OBSERVATION_TIMEOUT")
        self.assertFalse(any(item["name"] == "dirty_close_confirmation_observed" and item["passed"] for item in result["assertions"]))
        self.assertFalse(any(action[0] == "press" for action in self.backend.actions))

    def test_explicit_discard_must_exit_zero(self) -> None:
        self.backend.discard_exit_code = 7

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "NONZERO_NATIVE_EXIT")
        self.assertEqual(result["process"]["exit_code"], 7)

    def test_no_exit_after_discard_cannot_pass(self) -> None:
        self.backend.ignore_discard = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "NATIVE_EXIT_TIMEOUT")
        self.assertGreaterEqual(self.backend.process.terminate_calls, 1)

    def test_blank_or_failed_screenshots_fail_closed(self) -> None:
        for attribute in ("blank_screenshots", "fail_screenshots"):
            with self.subTest(attribute=attribute):
                self.setUp()
                setattr(self.backend, attribute, True)
                result = self.run_gate()
                self.assertEqual(result["outcome"], "FAIL")
                self.assertIn(result["error"]["code"], {"SCREENSHOT_NOT_DISTINCT", "NATIVE_COMMAND_FAILED"})

    def test_metadata_only_png_changes_are_not_visual_changes(self) -> None:
        self.backend.metadata_only_screenshots = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "SCREENSHOT_NOT_DISTINCT")
        self.assertNotEqual(result["screenshots"][0]["sha256"], result["screenshots"][1]["sha256"])
        self.assertFalse(any(item["name"] == "before_after_screenshots_differ" and item["passed"] for item in result["assertions"]))

    def test_close_and_cancel_require_different_pixels_not_metadata(self) -> None:
        self.backend.metadata_only_close_screenshots = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "SCREENSHOT_NOT_DISTINCT")
        self.assertTrue(any(item["name"] == "before_after_screenshots_differ" and item["passed"] for item in result["assertions"]))
        self.assertFalse(any(item["name"] == "cancel_screenshot_differs_from_confirmation" and item["passed"] for item in result["assertions"]))
        self.assertNotEqual(result["screenshots"][2]["sha256"], result["screenshots"][3]["sha256"])

    def test_truncated_ax_evidence_is_inconclusive(self) -> None:
        self.backend.truncated_ax = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(result["error"]["code"], "INCONCLUSIVE_AX")

    def test_lock_after_launch_is_blocked_incomplete_not_not_run(self) -> None:
        self.backend.lock_during_invert = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "BLOCKED")
        self.assertEqual(result["native_execution"], "INCOMPLETE")
        self.assertEqual(result["error"]["code"], "SCREEN_LOCKED")

    def test_failed_graceful_cleanup_uses_only_spawned_process_and_force_last(self) -> None:
        self.backend.ignore_discard = True
        self.backend.fail_cleanup = True
        self.backend.process.stubborn = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(self.backend.process.terminate_calls, 1)
        self.assertEqual(self.backend.process.kill_calls, 1)
        cleanup = result["cleanup"]
        self.assertEqual(cleanup["pid"], 4242)
        self.assertEqual(cleanup["sigterm_sent"], True)
        self.assertEqual(cleanup["sigkill_sent"], True)
        self.assertFalse(any(action[0] == "kill_other" for action in self.backend.actions))

    def test_graceful_failure_cleanup_does_not_send_signals(self) -> None:
        self.backend.ignore_invert = True

        result = self.run_gate()

        self.assertEqual(result["outcome"], "FAIL")
        self.assertEqual(self.backend.process.terminate_calls, 0)
        self.assertEqual(self.backend.process.kill_calls, 0)
        self.assertTrue(any(action[0] == "key" and action[2:] == ("q", ("command",)) for action in self.backend.actions))

    def test_output_directory_is_never_reused(self) -> None:
        output = self.root / "evidence"
        output.mkdir()

        with self.assertRaises(FileExistsError):
            gate.NativeUsabilityGate(self.backend, self.binary, output, config=self.config)

    def test_configuration_has_hard_upper_bounds(self) -> None:
        invalid = [
            {"startup_timeout": 0},
            {"startup_timeout": gate.MAX_STARTUP_TIMEOUT + 1},
            {"overall_timeout": gate.MAX_OVERALL_TIMEOUT + 1},
            {"poll_interval": 0},
            {"confirmation_hold": -1},
            {"quit_timeout": gate.MAX_QUIT_TIMEOUT + 1},
        ]
        for changes in invalid:
            with self.subTest(changes=changes):
                with self.assertRaises(ValueError):
                    gate.GateConfig(**changes).validate()

    def test_native_command_wrapper_is_bounded_and_does_not_use_a_shell(self) -> None:
        real_popen = subprocess.Popen
        with mock.patch.object(gate.subprocess, "Popen", side_effect=real_popen) as popen:
            result = gate.run_native_command(
                [sys.executable, "-c", "import os; os.write(1,b'{}\\n')"],
                timeout=2,
                max_output_bytes=3,
            )

        self.assertEqual(result.stdout, b"{}\n")
        self.assertNotIn("shell", popen.call_args.kwargs)
        self.assertEqual(popen.call_args.kwargs["stdin"], subprocess.DEVNULL)
        self.assertEqual(popen.call_args.kwargs["stdout"], subprocess.PIPE)
        self.assertEqual(popen.call_args.kwargs["stderr"], subprocess.PIPE)

    def test_native_command_wrapper_rejects_timeout_and_oversized_output(self) -> None:
        with self.assertRaisesRegex(gate.NativeCommandError, "timed out") as caught:
            gate.run_native_command(
                [sys.executable, "-c", "import time; time.sleep(30)"],
                timeout=0.2,
            )
        self.assertEqual(caught.exception.code, "COMMAND_TIMEOUT")

        with self.assertRaisesRegex(gate.NativeCommandError, "output exceeded") as caught:
            gate.run_native_command(
                [sys.executable, "-c", "import os; os.write(1,b'x'*20)"],
                timeout=2,
                max_output_bytes=10,
            )
        self.assertEqual(caught.exception.code, "COMMAND_OUTPUT_LIMIT")

        with self.assertRaisesRegex(gate.NativeCommandError, "output exceeded") as caught:
            gate.run_native_command(
                [sys.executable, "-c", "import os; os.write(1,b'x'*6); os.write(2,b'y'*6)"],
                timeout=2,
                max_output_bytes=10,
            )
        self.assertEqual(caught.exception.code, "COMMAND_OUTPUT_LIMIT")

    def test_native_command_stops_streaming_output_at_limit(self) -> None:
        script = "import os,time; [os.write(1,b'x'*65536) for _ in range(32)]; time.sleep(30)"
        started = time.monotonic()
        with self.assertRaisesRegex(gate.NativeCommandError, "output exceeded") as caught:
            gate.run_native_command(
                [sys.executable, "-c", script],
                timeout=5,
                max_output_bytes=1024,
            )
        self.assertEqual(caught.exception.code, "COMMAND_OUTPUT_LIMIT")
        self.assertLess(time.monotonic() - started, 2.0)

    def test_native_backend_press_uses_exact_helper_protocol(self) -> None:
        source = self.root / "helper.swift"
        source.write_text("// disposable helper source\n", encoding="utf-8")
        backend = gate.MacOSNativeBackend(source, self.root / "native-tool", self.config)
        completed = subprocess.CompletedProcess(["helper"], 0, stdout=b'{"event":"AXPress"}\n', stderr=b"")
        with mock.patch.object(backend, "_ensure_helper", return_value=None), \
             mock.patch.object(gate, "run_native_command", return_value=completed) as command:
            self.assertEqual(backend.press(4242, "Discard"), {"event": "AXPress"})

        self.assertEqual(
            command.call_args.args[0],
            [str(backend.helper), "press", "4242", gate.WINDOW_TITLE, "Discard"],
        )

    def test_native_backend_allows_only_guarded_close_shortcuts(self) -> None:
        source = self.root / "helper.swift"
        source.write_text("// disposable helper source\n", encoding="utf-8")
        backend = gate.MacOSNativeBackend(source, self.root / "native-tool", self.config)
        completed = subprocess.CompletedProcess(["helper"], 0, stdout=b'{"event":"key"}\n', stderr=b"")
        with mock.patch.object(backend, "_ensure_helper", return_value=None), \
             mock.patch.object(gate, "run_native_command", return_value=completed) as command:
            for name in ("i", "q", "w"):
                self.assertEqual(backend.key(4242, name, ("command",)), {"event": "key"})
            with self.assertRaises(gate.NativeCommandError) as caught:
                backend.key(4242, "x", ("command",))

        self.assertEqual(caught.exception.code, "INPUT_REJECTED")
        self.assertEqual(command.call_count, 3)

    def test_manifest_and_binary_identity_are_structured(self) -> None:
        result = self.run_gate()

        manifest_path = self.root / "evidence" / "manifest.json"
        self.assertEqual(result["binary"]["sha256"], hashlib.sha256(self.binary.read_bytes()).hexdigest())
        self.assertEqual(result["schema_version"], 1)
        self.assertEqual(result["gate"], "macos-native-keyboard-dirty-close")
        self.assertTrue((self.root / "evidence" / "events.jsonl").is_file())
        self.assertTrue((self.root / "evidence" / "paint-ui.log").is_file())
        self.assertTrue(manifest_path.is_file())

    def test_manifest_binds_harness_source_identities(self) -> None:
        result = self.run_gate()

        harness = result["harness"]
        self.assertEqual(harness["capture_stage"], "gate_initialization")
        records = harness["files"]
        self.assertEqual(
            [record["path"] for record in records],
            [
                "tools/macos_native_usability_gate.py",
                "tools/macos_native_usability_gate.swift",
                "tests/e2e/png_rgba.py",
            ],
        )
        repository = Path(gate.__file__).resolve().parents[1]
        for record in records:
            with self.subTest(path=record["path"]):
                content = (repository / record["path"]).read_bytes()
                self.assertEqual(record["bytes"], len(content))
                self.assertEqual(record["sha256"], hashlib.sha256(content).hexdigest())

        self.setUp()
        self.backend.preflight_result = preflight(screen_locked=True)
        blocked = self.run_gate()
        self.assertEqual(blocked["outcome"], "BLOCKED")
        self.assertEqual(blocked["native_execution"], "NOT_RUN")
        self.assertEqual(blocked["harness"], harness)


class ObservationButtonTests(unittest.TestCase):
    def test_duplicate_floating_chrome_buttons_do_not_hide_dialog_actions(self) -> None:
        raw = observation(9001, dialog=True)
        nodes = raw["ax"]["nodes"]
        nodes.extend(
            [
                {"path": [20], "role": "AXButton", "title": "Close window", "enabled": True},
                {"path": [21], "role": "AXButton", "title": "Close window", "enabled": True},
                {"path": [22], "role": "AXButton", "title": "Hide", "enabled": True},
                {"path": [23], "role": "AXButton", "title": "Hide", "enabled": True},
            ]
        )
        raw["ax"]["node_count"] = len(nodes)
        parsed = gate.parse_observation(raw, 9001)
        self.assertIn("Cancel", parsed.buttons)
        self.assertIn("Discard", parsed.buttons)

    def test_duplicate_dialog_action_remains_ambiguous(self) -> None:
        raw = observation(9002, dialog=True)
        nodes = raw["ax"]["nodes"]
        nodes.append({"path": [20], "role": "AXButton", "title": "Discard", "enabled": True})
        raw["ax"]["node_count"] = len(nodes)
        with self.assertRaisesRegex(gate.GateError, "ambiguous Accessibility button"):
            gate.parse_observation(raw, 9002)

if __name__ == "__main__":
    unittest.main()
