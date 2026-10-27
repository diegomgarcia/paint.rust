from __future__ import annotations

import base64
import contextlib
import copy
import hashlib
import io
import json
import struct
import sys
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest import mock

REFERENCE_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REFERENCE_DIR))

from collect_environment import decode_logfont  # noqa: E402
from png_evidence import PngError, decode_png, rgba_evidence  # noqa: E402
from observe_pdn_fixture import edit_distance, require_open_filename  # noqa: E402
from observe_writer_acceptance import (  # noqa: E402
    accepted_title,
    classify_open_result,
    rejection_dialog,
    require_not_already_open,
)
from guest_jobs import guest_absolute, start_guest_job  # noqa: E402
from run_windows_arm64_gate import TRIPLE, command, environment, main as gate_main  # noqa: E402
from utm import ReferenceCommandError  # noqa: E402
from validate_observations import (  # noqa: E402
    ValidationError,
    main as validate_observations_main,
    validate_manifest,
)
from validate_pdn_fixture import validate_pdn_fixture  # noqa: E402


AUTHENTIC_PAINTNET = {
    "file_version": "5.112.9563.32325",
    "product_version": "5.112.9563.32325",
    "path": r"C:\Program Files\paint.net\paintdotnet.exe",
    "sha256": "17b27aee8eb0d18e7b39964e6c1c43b1dac5705f23727af550f2cde7fe737e2e",
    "signature_status": "Valid",
    "signer_subject": "CN=DOTPDN LLC, O=DOTPDN LLC, S=Washington, C=US",
}


def authentic_environment(**paintnet_changes: object) -> dict:
    return {
        "guest": {
            "paintnet": {**AUTHENTIC_PAINTNET, **paintnet_changes},
            "os": {"native_system_type": "ARM64-based PC"},
        },
    }


def chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def png_bytes(
    width: int,
    height: int,
    bit_depth: int,
    color_type: int,
    rows: list[bytes],
    *,
    palette: bytes | None = None,
    transparency: bytes | None = None,
    interlace: int = 0,
) -> bytes:
    ihdr = struct.pack(">IIBBBBB", width, height, bit_depth, color_type, 0, 0, interlace)
    parts = [b"\x89PNG\r\n\x1a\n", chunk(b"IHDR", ihdr)]
    if palette is not None:
        parts.append(chunk(b"PLTE", palette))
    if transparency is not None:
        parts.append(chunk(b"tRNS", transparency))
    parts.extend([chunk(b"IDAT", zlib.compress(b"".join(rows))), chunk(b"IEND", b"")])
    return b"".join(parts)


class PngEvidenceTests(unittest.TestCase):
    def test_one_bit_palette_matches_known_rgba(self) -> None:
        # Four pixels: black, white, black, transparent white. Filter type 0.
        encoded = png_bytes(
            4,
            1,
            1,
            3,
            [b"\x00\x50"],
            palette=b"\x00\x00\x00\xff\xff\xff",
            transparency=b"\xff\x00",
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.png"
            path.write_bytes(encoded)
            decoded = decode_png(path)
        self.assertEqual(decoded.width, 4)
        self.assertEqual(decoded.height, 1)
        self.assertEqual(
            decoded.rgba,
            bytes(
                [
                    0, 0, 0, 255,
                    255, 255, 255, 0,
                    0, 0, 0, 255,
                    255, 255, 255, 0,
                ]
            ),
        )
        evidence = rgba_evidence(decoded)
        self.assertEqual(evidence["rgba_sha256"], hashlib.sha256(decoded.rgba).hexdigest())
        self.assertEqual(evidence["unique_color_count"], 2)
        self.assertFalse(evidence["all_pixels_equal"])

    def test_rgb_filter_zero(self) -> None:
        encoded = png_bytes(1, 1, 8, 2, [b"\x00\x11\x22\x33"])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rgb.png"
            path.write_bytes(encoded)
            decoded = decode_png(path)
        self.assertEqual(decoded.rgba, b"\x11\x22\x33\xff")

    def test_sub_up_average_and_paeth_filters(self) -> None:
        cases = {
            "sub": (2, 1, [b"\x01\x0a\x0a"], [10, 20]),
            "up": (2, 2, [b"\x00\x0a\x14", b"\x02\x05\x05"], [10, 20, 15, 25]),
            "average": (2, 1, [b"\x03\x0a\x0f"], [10, 20]),
            "paeth": (2, 1, [b"\x04\x0a\x0a"], [10, 20]),
        }
        with tempfile.TemporaryDirectory() as directory:
            for name, (width, height, rows, samples) in cases.items():
                path = Path(directory) / f"{name}.png"
                path.write_bytes(png_bytes(width, height, 8, 0, rows))
                with self.subTest(name=name):
                    decoded = decode_png(path)
                    expected = b"".join(bytes([sample, sample, sample, 255]) for sample in samples)
                    self.assertEqual(decoded.rgba, expected)

    def test_invalid_crc_and_interlace_fail(self) -> None:
        valid = png_bytes(1, 1, 8, 6, [b"\x00\x01\x02\x03\x04"])
        bad_crc = bytearray(valid)
        bad_crc[29] ^= 1
        interlaced = png_bytes(1, 1, 8, 6, [b"\x00\x01\x02\x03\x04"], interlace=1)
        with tempfile.TemporaryDirectory() as directory:
            for name, payload in (("bad-crc", bytes(bad_crc)), ("interlaced", interlaced)):
                path = Path(directory) / f"{name}.png"
                path.write_bytes(payload)
                with self.subTest(name=name), self.assertRaises(PngError):
                    decode_png(path)


def observation(run: int, *, encoded: str = "a" * 64, rgba: str = "b" * 64) -> dict:
    return {
        "run": run,
        "document": {
            "width": 800,
            "height": 600,
            "operation": "Invert Colors",
            "source": "new document",
            "paintnet_window_title_version": "5.1.12",
        },
        "guest": {"sha256": encoded, "png": {"rgba_sha256": rgba}},
        "host": {
            "sha256": encoded,
            "png": {
                "width": 800,
                "height": 600,
                "rgba_sha256": rgba,
                "decoded_rgba_bytes": 1_920_000,
                "pixel_count": 480_000,
                "row_stride_bytes": 3200,
                "all_pixels_equal": True,
                "first_pixel_rgba": [0, 0, 0, 255],
                "unique_color_count": 1,
                "colors": [{"rgba": [0, 0, 0, 255], "count": 480_000}],
            },
        },
        "hash_comparison": {"guest_host_encoded_equal": True, "guest_host_rgba_equal": True},
    }


class EnvironmentDecodingTests(unittest.TestCase):
    def test_logfont_w_decoding(self) -> None:
        raw = struct.pack("<5i8B", -12, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0)
        raw += "Segoe UI".encode("utf-16-le") + bytes(64 - 16)
        decoded = decode_logfont({"type": "byte[]", "length": 92, "base64": base64.b64encode(raw).decode("ascii")})
        self.assertEqual(decoded["face_name"], "Segoe UI")
        self.assertEqual(decoded["height"], -12)
        self.assertEqual(decoded["weight"], 400)
        self.assertEqual(decoded["charset"], 1)


class PaintNetIdentityTests(unittest.TestCase):
    def preflight(self, environment: dict) -> dict:
        from paintnet_identity import require_reviewed_paintnet

        return require_reviewed_paintnet(environment)

    def test_valid_reviewed_evidence_passes_and_derives_display_version(self) -> None:
        identity = self.preflight(authentic_environment())
        self.assertEqual(identity["display_version"], "5.1.12")
        self.assertEqual(identity["file_version"], AUTHENTIC_PAINTNET["file_version"])
        self.assertEqual(identity["product_version"], AUTHENTIC_PAINTNET["product_version"])
        self.assertEqual(identity["source_sha256"], AUTHENTIC_PAINTNET["sha256"])
        self.assertEqual(identity["signature_status"], "Valid")
        self.assertEqual(identity["executable_path"], AUTHENTIC_PAINTNET["path"])
        self.assertEqual(identity["signer_subject"], AUTHENTIC_PAINTNET["signer_subject"])

    def test_wrong_file_or_product_version_fails(self) -> None:
        for field in ("file_version", "product_version"):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.preflight(authentic_environment(**{field: "5.113.9563.32325"}))

    def test_wrong_executable_hash_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "sha256"):
            self.preflight(authentic_environment(sha256="0" * 64))

    def test_non_valid_signature_fails(self) -> None:
        with self.assertRaisesRegex(ValueError, "signature_status"):
            self.preflight(authentic_environment(signature_status="NotValid"))


class ManifestValidationTests(unittest.TestCase):
    def manifest(self) -> dict:
        return {"schema_version": 1, "kind": "paintnet-invert-aa", "observations": [observation(i) for i in range(1, 4)]}

    def validate(self, manifest: dict) -> dict:
        return validate_manifest(manifest, environment=authentic_environment())

    def test_three_identical_observations_pass(self) -> None:
        result = self.validate(self.manifest())
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["observation_count"], 3)
        self.assertTrue(result["encoded_bytes_repeatable"])
        self.assertTrue(result["decoded_rgba_repeatable"])

    def test_hash_mismatch_and_wrong_pixel_fail(self) -> None:
        manifest = self.manifest()
        manifest["observations"][1] = copy.deepcopy(manifest["observations"][1])
        manifest["observations"][1]["host"]["png"]["first_pixel_rgba"] = [255, 255, 255, 255]
        with self.assertRaises(ValidationError):
            self.validate(manifest)
        manifest = self.manifest()
        manifest["observations"][0] = observation(1, rgba="c" * 64)
        with self.assertRaises(ValidationError):
            self.validate(manifest)

    def test_two_runs_are_partial_not_aa_pass(self) -> None:
        manifest = self.manifest()
        manifest["observations"] = manifest["observations"][:2]
        with self.assertRaises(ValidationError):
            self.validate(manifest)

    def test_window_title_must_match_observed_identity(self) -> None:
        manifest = self.manifest()
        manifest["observations"][1]["document"]["paintnet_window_title_version"] = "5.1.13"
        with self.assertRaisesRegex(ValidationError, "window_title"):
            self.validate(manifest)

    def test_independent_validator_rejects_self_consistent_wrong_identity(self) -> None:
        cases = (
            (
                "wrong-version",
                {"file_version": "5.113.9563.32325", "product_version": "5.113.9563.32325"},
                "5.1.13",
                "file_version",
            ),
            ("wrong-hash", {"sha256": "0" * 64}, None, "sha256"),
            ("wrong-signature", {"signature_status": "NotValid"}, None, "signature_status"),
        )
        for name, changes, display_version, error_field in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                environment_bytes = (json.dumps(authentic_environment(**changes)) + "\n").encode("utf-8")
                (root / "environment.json").write_bytes(environment_bytes)
                manifest = self.manifest()
                manifest["environment"] = {
                    "path": "environment.json",
                    "sha256": hashlib.sha256(environment_bytes).hexdigest(),
                }
                if display_version is not None:
                    for item in manifest["observations"]:
                        item["document"]["paintnet_window_title_version"] = display_version
                manifest_path = root / "manifest.json"
                manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
                stderr = io.StringIO()
                with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
                    return_code = validate_observations_main([str(manifest_path)])
                self.assertEqual(return_code, 2, stderr.getvalue())
                self.assertIn(error_field, stderr.getvalue())


class PdnIdentityValidationTests(unittest.TestCase):
    def forged_manifest(self, root: Path, changes: dict, display_version: str) -> Path:
        environment = authentic_environment(**changes)
        environment_bytes = (json.dumps(environment) + "\n").encode("utf-8")
        (root / "environment.json").write_bytes(environment_bytes)
        paintnet = environment["guest"]["paintnet"]
        manifest = {
            "kind": "authentic-paintnet-pdn-fixture",
            "status": "PASS_UI_SAVE_CLOSE_REOPEN_AND_RGBA",
            "authentic_stock_output": True,
            "ancillary_tool_edited_file": False,
            "checks": {"otherwise_self_consistent": True},
            "environment": {
                "path": "environment.json",
                "sha256": hashlib.sha256(environment_bytes).hexdigest(),
            },
            "product": {
                "display_version": display_version,
                "file_version": paintnet["file_version"],
                "product_version": paintnet["product_version"],
                "executable_path": paintnet["path"],
                "source_sha256": paintnet["sha256"],
                "signature_status": paintnet["signature_status"],
                "signer_subject": paintnet["signer_subject"],
            },
        }
        path = root / "manifest.json"
        path.write_text(json.dumps(manifest), encoding="utf-8")
        return path

    def test_independent_validator_rejects_self_consistent_wrong_identity(self) -> None:
        cases = (
            (
                "wrong-version",
                {"file_version": "5.113.9563.32325", "product_version": "5.113.9563.32325"},
                "5.1.13",
                "file_version",
            ),
            ("wrong-hash", {"sha256": "0" * 64}, "5.1.12", "sha256"),
            ("wrong-signature", {"signature_status": "NotValid"}, "5.1.12", "signature_status"),
        )
        for name, changes, display_version, error_field in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                manifest_path = self.forged_manifest(Path(directory), changes, display_version)
                with self.assertRaises(ValueError) as raised:
                    validate_pdn_fixture(manifest_path, check_ffmpeg=False)
                self.assertIn(error_field, str(raised.exception))


class PdnUiGateTests(unittest.TestCase):
    def test_edit_distance(self) -> None:
        self.assertEqual(edit_distance("fixture-abc.pdn", "ficture-abc.pdn"), 1)
        self.assertGreater(edit_distance("fixture-abc.pdn", "other.pdn"), 1)

    def test_open_filename_allows_only_one_ocr_edit(self) -> None:
        require_open_filename({"ocr_text": "File name: |ficture-abc.pdn"}, "fixture-abc.pdn")
        with self.assertRaises(ReferenceCommandError):
            require_open_filename({"ocr_text": "File name: |other.pdn"}, "fixture-abc.pdn")


class WriterAcceptanceTests(unittest.TestCase):
    filename = "clone-normal-2x2-raw.pdn"

    def test_open_title_requires_near_exact_filename(self) -> None:
        self.assertTrue(accepted_title({"ocr_text": f"{self.filename} - Paint.NET"}, self.filename))
        self.assertTrue(accepted_title({"ocr_text": "clone-normal-2x2-r4w.pdn - Paint.NET"}, self.filename))
        self.assertFalse(accepted_title({"ocr_text": "other.pdn - Paint.NET"}, self.filename))

    def test_rejection_and_acceptance_are_unambiguous(self) -> None:
        accepted = {"ocr_text": f"{self.filename} - Paint.NET\nBackground"}
        rejected = {"ocr_text": "Paint.NET\nError: could not open file\nOK"}
        self.assertEqual(classify_open_result(accepted, self.filename), (True, False))
        self.assertEqual(classify_open_result(rejected, self.filename), (False, True))
        self.assertTrue(rejection_dialog(rejected))
        with self.assertRaises(ReferenceCommandError):
            classify_open_result({"ocr_text": accepted["ocr_text"] + "\nError\nOK"}, self.filename)
        with self.assertRaises(ReferenceCommandError):
            classify_open_result({"ocr_text": "Paint.NET\nUntitled"}, self.filename)

    def test_stale_open_title_is_rejected_before_file_open(self) -> None:
        with self.assertRaises(ReferenceCommandError):
            require_not_already_open({"ocr_text": f"{self.filename} - Paint.NET"}, self.filename)
        require_not_already_open({"ocr_text": "Untitled - Paint.NET"}, self.filename)


class ObservationGateIdentityTests(unittest.TestCase):
    def assert_preflight_failure(self, root: Path, expected_field: str) -> None:
        failure = json.loads((root / "failure.json").read_text(encoding="utf-8"))
        self.assertEqual(failure["stage"], "paintnet-preflight")
        self.assertIn(expected_field, failure["error"])

    def test_invert_gate_preflights_before_observation(self) -> None:
        import observe_invert

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "run"
            with (
                mock.patch(
                    "observe_invert.collect_environment",
                    return_value=authentic_environment(sha256="0" * 64),
                ),
                mock.patch("observe_invert.run_observation", side_effect=AssertionError("observation reached")) as run,
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                return_code = observe_invert.main([
                    "--run-id", "invert-identity-test", "--output-dir", str(root),
                ])
            self.assertEqual(return_code, 2)
            run.assert_not_called()
            self.assert_preflight_failure(root, "sha256")

    def test_pdn_gate_preflights_before_guest_or_ui_actions(self) -> None:
        import observe_pdn_fixture

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "run"
            with (
                mock.patch(
                    "observe_pdn_fixture.collect_environment",
                    return_value=authentic_environment(signature_status="NotValid"),
                ),
                mock.patch("observe_pdn_fixture.guest_absent") as absent,
                mock.patch("observe_pdn_fixture.new_2x2", side_effect=AssertionError("UI reached")),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                return_code = observe_pdn_fixture.main([
                    "--run-id", "pdn-identity-test", "--output-dir", str(root),
                ])
            self.assertEqual(return_code, 2)
            absent.assert_not_called()
            self.assert_preflight_failure(root, "signature_status")

    def test_writer_gate_preflights_before_ui(self) -> None:
        import observe_writer_acceptance

        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            host_input = temporary / "clone-normal-2x2-raw.pdn"
            payload = b"PDN3 identity preflight test"
            host_input.write_bytes(payload)
            host_sha = hashlib.sha256(payload).hexdigest()
            before = {"sha256": host_sha, "length": len(payload)}
            with (
                mock.patch("observe_writer_acceptance.LOCAL_ROOT", temporary),
                mock.patch("observe_writer_acceptance.guest_file_metadata", return_value=before),
                mock.patch("observe_writer_acceptance.guest_absent"),
                mock.patch(
                    "observe_writer_acceptance.collect_environment",
                    return_value=authentic_environment(
                        file_version="5.113.9563.32325",
                        product_version="5.113.9563.32325",
                    ),
                ),
                mock.patch("observe_writer_acceptance.key", side_effect=AssertionError("UI reached")) as key,
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                return_code = observe_writer_acceptance.main([
                    "--run-id", "writer-identity-test",
                    "--host-input", str(host_input),
                    "--expected-sha256", host_sha,
                    "--variant", "raw",
                ])
            self.assertEqual(return_code, 2)
            key.assert_not_called()
            self.assert_preflight_failure(
                temporary / "runs" / "writer-identity-test", "file_version",
            )


class GuestJobAndGateTests(unittest.TestCase):
    def test_guest_paths_and_secret_environment_are_rejected(self) -> None:
        guest_absolute(r"C:\PaintR-Reference\final-src")
        with self.assertRaises(ValueError):
            guest_absolute(r"C:\PaintR-Reference\..\outside")
        with self.assertRaises(ValueError):
            start_guest_job(
                "unit-secret",
                r"C:\PaintR-Reference\tools\node\node.exe",
                ["--version"],
                working_directory=r"C:\PaintR-Reference",
                environment={"DEEPSEEK_API_KEY": "must-not-be-recorded"},
            )

    def test_gate_cli_requires_explicit_source_and_directories(self) -> None:
        completed = {
            "job_id": "unexpected-guest-call",
            "status": "PASS",
            "exit_code": 0,
            "timed_out": False,
            "seconds": 0.0,
            "exit": {},
            "config": {"executable": "unused", "arguments": []},
            "observation_errors": [],
            "logs": {},
        }
        with (
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()),
            mock.patch("run_windows_arm64_gate.run_guest_job", return_value=completed) as run,
            mock.patch("run_windows_arm64_gate.write_json_new") as write,
            self.assertRaises(SystemExit) as raised,
        ):
            gate_main(["--steps", "fmt", "--summary", "unit.json"])
        self.assertEqual(raised.exception.code, 2)
        run.assert_not_called()
        write.assert_not_called()

    def test_gate_commands_preserve_target_and_fatal_checks(self) -> None:
        root = r"C:\PaintR-Reference\final-src"
        target = r"C:\PaintR-Reference\target-final-gnullvm"
        executable, arguments = command("clippy", root, target, r"C:\PaintR-Reference\evidence\smoke")
        self.assertTrue(executable.endswith("cargo.exe"))
        self.assertEqual(arguments[-2:], ["-D", "warnings"])
        self.assertIn("--all-features", arguments)
        self.assertIn("--locked", arguments)
        executable, arguments = command("smoke", root, target, r"C:\PaintR-Reference\evidence\smoke")
        self.assertIn(TRIPLE, executable)
        self.assertEqual(arguments[0], "--headless-smoke")
        self.assertIn("invert-800x600.json", arguments[arguments.index("--trace") + 1])
        env = environment(target)
        self.assertEqual(env["CARGO_BUILD_TARGET"], TRIPLE)
        self.assertNotIn("DEEPSEEK_API_KEY", env)


if __name__ == "__main__":
    unittest.main(verbosity=2)
