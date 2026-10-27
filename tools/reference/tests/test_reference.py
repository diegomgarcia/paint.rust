from __future__ import annotations

import base64
import copy
import hashlib
import json
import struct
import sys
import tempfile
import unittest
import zlib
from pathlib import Path

REFERENCE_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REFERENCE_DIR))

from collect_environment import decode_logfont  # noqa: E402
from png_evidence import PngError, decode_png, rgba_evidence  # noqa: E402
from observe_pdn_fixture import edit_distance, require_open_filename  # noqa: E402
from utm import ReferenceCommandError  # noqa: E402
from validate_observations import ValidationError, validate_manifest  # noqa: E402


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
        "document": {"width": 800, "height": 600, "operation": "Invert Colors", "source": "new document"},
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


class ManifestValidationTests(unittest.TestCase):
    def manifest(self) -> dict:
        return {"schema_version": 1, "kind": "paintnet-invert-aa", "observations": [observation(i) for i in range(1, 4)]}

    def test_three_identical_observations_pass(self) -> None:
        result = validate_manifest(self.manifest())
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["observation_count"], 3)
        self.assertTrue(result["encoded_bytes_repeatable"])
        self.assertTrue(result["decoded_rgba_repeatable"])

    def test_hash_mismatch_and_wrong_pixel_fail(self) -> None:
        manifest = self.manifest()
        manifest["observations"][1] = copy.deepcopy(manifest["observations"][1])
        manifest["observations"][1]["host"]["png"]["first_pixel_rgba"] = [255, 255, 255, 255]
        with self.assertRaises(ValidationError):
            validate_manifest(manifest)
        manifest = self.manifest()
        manifest["observations"][0] = observation(1, rgba="c" * 64)
        with self.assertRaises(ValidationError):
            validate_manifest(manifest)

    def test_two_runs_are_partial_not_aa_pass(self) -> None:
        manifest = self.manifest()
        manifest["observations"] = manifest["observations"][:2]
        with self.assertRaises(ValidationError):
            validate_manifest(manifest)


class PdnUiGateTests(unittest.TestCase):
    def test_edit_distance(self) -> None:
        self.assertEqual(edit_distance("fixture-abc.pdn", "ficture-abc.pdn"), 1)
        self.assertGreater(edit_distance("fixture-abc.pdn", "other.pdn"), 1)

    def test_open_filename_allows_only_one_ocr_edit(self) -> None:
        require_open_filename({"ocr_text": "File name: |ficture-abc.pdn"}, "fixture-abc.pdn")
        with self.assertRaises(ReferenceCommandError):
            require_open_filename({"ocr_text": "File name: |other.pdn"}, "fixture-abc.pdn")


if __name__ == "__main__":
    unittest.main(verbosity=2)
