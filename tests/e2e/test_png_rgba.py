from __future__ import annotations

import binascii
import struct
import unittest
import zlib
from pathlib import Path

from png_rgba import Limits, PngError, RgbaImage, compare_images, decode_png


def chunk(kind: bytes, payload: bytes) -> bytes:
    crc = binascii.crc32(payload, binascii.crc32(kind)) & 0xFFFFFFFF
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", crc)


def make_png(
    width: int,
    height: int,
    depth: int,
    color_type: int,
    raw: bytes,
    *,
    palette: bytes | None = None,
    transparency: bytes | None = None,
    interlace: int = 0,
) -> bytes:
    header = struct.pack(">IIBBBBB", width, height, depth, color_type, 0, 0, interlace)
    pieces = [b"\x89PNG\r\n\x1a\n", chunk(b"IHDR", header)]
    if palette is not None:
        pieces.append(chunk(b"PLTE", palette))
    if transparency is not None:
        pieces.append(chunk(b"tRNS", transparency))
    pieces.extend((chunk(b"IDAT", zlib.compress(raw)), chunk(b"IEND", b"")))
    return b"".join(pieces)


def filter_source(filter_type: int, desired: bytes, previous: bytes, bpp: int) -> bytes:
    source = bytearray(len(desired))
    for index, value in enumerate(desired):
        left = desired[index - bpp] if index >= bpp else 0
        above = previous[index]
        upper_left = previous[index - bpp] if index >= bpp else 0
        if filter_type == 0:
            predictor = 0
        elif filter_type == 1:
            predictor = left
        elif filter_type == 2:
            predictor = above
        elif filter_type == 3:
            predictor = (left + above) // 2
        else:
            estimate = left + above - upper_left
            distances = (abs(estimate - left), abs(estimate - above), abs(estimate - upper_left))
            predictor = (left, above, upper_left)[distances.index(min(distances))]
        source[index] = (value - predictor) & 0xFF
    return bytes(source)


class DecodeTests(unittest.TestCase):
    def test_stock_paintnet_indexed_png_decodes_to_opaque_black(self) -> None:
        path = Path(__file__).parents[2] / "artifacts/reference-local/paintnet-5.1.12-invert.png"
        image = decode_png(path.read_bytes())
        self.assertEqual((800, 600), (image.width, image.height))
        self.assertEqual(bytes((0, 0, 0, 255)) * (800 * 600), image.pixels)

    def test_rgba_and_every_adaptive_filter(self) -> None:
        rows = [
            bytes((1, 2, 3, 4, 5, 6, 7, 8)),
            bytes((10, 20, 30, 40, 50, 60, 70, 80)),
            bytes((9, 8, 7, 6, 5, 4, 3, 2)),
            bytes((0, 255, 128, 255, 255, 0, 64, 32)),
            bytes((3, 3, 3, 3, 200, 100, 50, 25)),
        ]
        raw = bytearray()
        previous = bytes(8)
        for filter_type, desired in enumerate(rows):
            raw.append(filter_type)
            raw.extend(filter_source(filter_type, desired, previous, 4))
            previous = desired
        image = decode_png(make_png(2, 5, 8, 6, bytes(raw)))
        self.assertEqual(b"".join(rows), image.pixels)

    def test_packed_palette_and_transparency(self) -> None:
        # Three 1-bit indexes: 0, 1, 1.
        # Two palette entries with alpha; indexes are 0, 1, 1.
        encoded = make_png(
            3,
            1,
            1,
            3,
            bytes((0, 0b0110_0000)),
            palette=bytes((10, 20, 30, 200, 150, 100)),
            transparency=bytes((255, 128)),
        )
        image = decode_png(encoded)
        self.assertEqual(
            bytes((10, 20, 30, 255, 200, 150, 100, 128, 200, 150, 100, 128)),
            image.pixels,
        )

    def test_packed_grayscale_scaling(self) -> None:
        # Four 2-bit samples: 0, 1, 2, 3.
        image = decode_png(make_png(4, 1, 2, 0, bytes((0, 0b0001_1011))))
        self.assertEqual(bytes((0, 0, 0, 255, 85, 85, 85, 255, 170, 170, 170, 255, 255, 255, 255, 255)), image.pixels)

    def test_crc_failure_is_rejected(self) -> None:
        encoded = bytearray(make_png(1, 1, 8, 6, bytes((0, 1, 2, 3, 4))))
        encoded[-20] ^= 1
        with self.assertRaisesRegex(PngError, "CRC mismatch"):
            decode_png(bytes(encoded))

    def test_truncation_is_rejected(self) -> None:
        encoded = make_png(1, 1, 8, 6, bytes((0, 1, 2, 3, 4)))
        with self.assertRaises(PngError):
            decode_png(encoded[:-1])

    def test_pixel_limit_is_checked_before_inflate(self) -> None:
        encoded = make_png(100, 100, 8, 6, b"not a valid stream")
        with self.assertRaisesRegex(PngError, "dimensions"):
            decode_png(encoded, Limits(max_pixels=99))

    def test_interlace_is_explicitly_unsupported(self) -> None:
        encoded = make_png(1, 1, 8, 6, bytes((0, 1, 2, 3, 4)), interlace=1)
        with self.assertRaisesRegex(PngError, "non-interlaced"):
            decode_png(encoded)

    def test_palette_index_is_bounds_checked(self) -> None:
        encoded = make_png(
            1,
            1,
            1,
            3,
            bytes((0, 0b1000_0000)),
            palette=bytes((10, 20, 30)),
        )
        with self.assertRaisesRegex(PngError, "palette index"):
            decode_png(encoded)


class ComparisonTests(unittest.TestCase):
    def test_equal_images_have_exact_counts(self) -> None:
        image = RgbaImage(1, 2, bytes((1, 2, 3, 4)) * 2)
        result = compare_images(image, image)
        self.assertTrue(result["match"])
        self.assertEqual(2, result["compared_pixels"])
        self.assertEqual(0, result["differing_pixels"])

    def test_counts_each_differing_pixel_even_when_red_matches(self) -> None:
        expected = RgbaImage(2, 1, bytes((1, 2, 3, 4, 5, 6, 7, 8)))
        actual = RgbaImage(2, 1, bytes((1, 20, 3, 4, 5, 60, 7, 8)))
        result = compare_images(expected, actual)
        self.assertFalse(result["match"])
        self.assertEqual(2, result["differing_pixels"])
        self.assertEqual("G", result["first_difference"]["channel"])

    def test_dimension_mismatch_is_not_a_pixel_match(self) -> None:
        expected = RgbaImage(1, 1, bytes(4))
        actual = RgbaImage(2, 1, bytes(8))
        result = compare_images(expected, actual)
        self.assertFalse(result["match"])
        self.assertEqual("dimensions", result["reason"])


if __name__ == "__main__":
    unittest.main()
