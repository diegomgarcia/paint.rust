"""Unit tests for the black-box preview gate's independent evidence checks."""

from __future__ import annotations

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import preview_smoke

ROOT = Path(__file__).resolve().parents[2]
STOCK_PNG = ROOT / "artifacts/reference-local/paintnet-5.1.12-invert.png"
RGBA_SHA256 = "a7aff0f8e3f44663c369e1e75a981b90b66949586d71035771921871d67e334e"
PNG_SHA256 = "0c8cdc8f496907b88ef445cfdfa8f88e36cf09863a5cdde4b8e7243637708163"


def snapshot() -> dict[str, object]:
    return {
        "format": "paint.r.semantic-snapshot",
        "version": 1,
        "width": 800,
        "height": 600,
        "layer_count": 1,
        "active_layer_index": 0,
        "dirty": False,
        "can_undo": False,
        "can_redo": True,
        "layers": [
            {
                "name": "Background",
                "visible": True,
                "opacity": 255,
                "is_background": True,
                "blend_mode": "normal",
            }
        ],
        "selection": None,
        "decoded_rgba_sha256": RGBA_SHA256,
    }


def write_case(directory: Path, document: dict[str, object], *, indent: int | None = None) -> bytes:
    directory.mkdir()
    (directory / "composite.png").write_bytes(STOCK_PNG.read_bytes())
    encoded = (json.dumps(document, sort_keys=True, indent=indent) + "\n").encode("utf-8")
    (directory / "snapshot.json").write_bytes(encoded)
    return encoded


class OutputEvidenceTests(unittest.TestCase):
    def test_encoded_and_snapshot_bytes_are_part_of_deterministic_evidence(self) -> None:
        with tempfile.TemporaryDirectory(prefix="paint-r-smoke-test-") as temporary:
            root = Path(temporary)
            compact = write_case(root / "compact", snapshot())
            pretty = write_case(root / "pretty", snapshot(), indent=2)
            trace = {"expect": {"width": 800, "height": 600, "decoded_rgba_sha256": RGBA_SHA256}}

            first = preview_smoke.verify_outputs(trace, root / "compact")
            second = preview_smoke.verify_outputs(trace, root / "pretty")

            self.assertEqual(first["png_sha256"], PNG_SHA256)
            self.assertEqual(first["snapshot_sha256"], hashlib.sha256(compact).hexdigest())
            self.assertEqual(second["snapshot_sha256"], hashlib.sha256(pretty).hexdigest())
            self.assertNotEqual(first["snapshot_sha256"], second["snapshot_sha256"])
            self.assertEqual(first["decoded_rgba_sha256"], second["decoded_rgba_sha256"])

    def test_invalid_layer_active_index_and_selection_metadata_are_rejected(self) -> None:
        cases = []
        boolean_opacity = snapshot()
        boolean_opacity["layers"][0]["opacity"] = True
        cases.append(boolean_opacity)
        missing_layer_field = snapshot()
        del missing_layer_field["layers"][0]["name"]
        cases.append(missing_layer_field)
        out_of_range = snapshot()
        out_of_range["active_layer_index"] = 1
        cases.append(out_of_range)
        invalid_selection = snapshot()
        invalid_selection["selection"] = {"left": 0, "top": 0, "right": 1}
        cases.append(invalid_selection)

        for index, document in enumerate(cases):
            with self.subTest(case=index), tempfile.TemporaryDirectory(prefix="paint-r-smoke-test-") as temporary:
                root = Path(temporary)
                write_case(root / "output", document)
                with self.assertRaises(ValueError):
                    preview_smoke.verify_outputs({}, root / "output")


if __name__ == "__main__":
    unittest.main()
