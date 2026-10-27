"""Small independent PNG-to-RGBA oracle for exact end-to-end comparisons.

This is deliberately a bounded test oracle, not the product's image codec. It
supports the non-interlaced 8-bit RGB/RGBA/grayscale forms and 1/2/4/8-bit
indexed/grayscale forms used by the reference corpus. Unsupported encodings are
rejected explicitly rather than decoded approximately.
"""

from __future__ import annotations

import argparse
import binascii
import json
import struct
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


class PngError(ValueError):
    """The PNG is malformed or outside this oracle's supported subset."""


@dataclass(frozen=True)
class RgbaImage:
    width: int
    height: int
    pixels: bytes

    def __post_init__(self) -> None:
        expected = self.width * self.height * 4
        if len(self.pixels) != expected:
            raise ValueError(f"RGBA length {len(self.pixels)} != {expected}")


@dataclass(frozen=True)
class Limits:
    max_pixels: int = 16_777_216
    max_compressed_bytes: int = 67_108_864


def _paeth(a: int, b: int, c: int) -> int:
    estimate = a + b - c
    da = abs(estimate - a)
    db = abs(estimate - b)
    dc = abs(estimate - c)
    if da <= db and da <= dc:
        return a
    if db <= dc:
        return b
    return c


def _unfilter(filter_type: int, source: bytes, previous: bytes, bpp: int) -> bytearray:
    row = bytearray(source)
    if filter_type == 0:
        return row
    if filter_type not in (1, 2, 3, 4):
        raise PngError(f"invalid PNG filter {filter_type}")
    for index in range(len(row)):
        left = row[index - bpp] if index >= bpp else 0
        above = previous[index]
        upper_left = previous[index - bpp] if index >= bpp else 0
        if filter_type == 1:
            predictor = left
        elif filter_type == 2:
            predictor = above
        elif filter_type == 3:
            predictor = (left + above) // 2
        else:
            predictor = _paeth(left, above, upper_left)
        row[index] = (row[index] + predictor) & 0xFF
    return row


def _packed_sample(row: bytes, x: int, depth: int) -> int:
    bit_offset = x * depth
    shift = 8 - depth - (bit_offset % 8)
    return (row[bit_offset // 8] >> shift) & ((1 << depth) - 1)


def _row_to_rgba(
    row: bytes,
    width: int,
    color_type: int,
    depth: int,
    palette: bytes | None,
    transparency: bytes | None,
) -> bytes:
    output = bytearray(width * 4)
    if color_type == 6:
        output[:] = row
        return bytes(output)

    for x in range(width):
        if color_type == 3:
            assert palette is not None
            index = row[x] if depth == 8 else _packed_sample(row, x, depth)
            palette_offset = index * 3
            if palette_offset + 3 > len(palette):
                raise PngError(f"palette index {index} is outside the PLTE table")
            red, green, blue = palette[palette_offset : palette_offset + 3]
            alpha = transparency[index] if transparency is not None and index < len(transparency) else 255
        elif color_type == 2:
            offset = x * 3
            red, green, blue = row[offset : offset + 3]
            alpha = 255
            if transparency is not None:
                transparent_rgb = struct.unpack(">HHH", transparency)
                if (red, green, blue) == transparent_rgb:
                    alpha = 0
        else:
            if color_type == 0:
                sample = row[x] if depth == 8 else _packed_sample(row, x, depth)
                maximum = (1 << depth) - 1
                gray = (sample * 255 + maximum // 2) // maximum
                alpha = 255
                if transparency is not None:
                    transparent_gray = struct.unpack(">H", transparency)[0]
                    if sample == transparent_gray:
                        alpha = 0
            elif color_type == 4:
                gray, alpha = row[x * 2 : x * 2 + 2]
            else:
                raise PngError(f"unsupported PNG color type {color_type}")
            red = green = blue = gray
        offset = x * 4
        output[offset : offset + 4] = bytes((red, green, blue, alpha))
    return bytes(output)


def decode_png(data: bytes, limits: Limits = Limits()) -> RgbaImage:
    if not data.startswith(PNG_SIGNATURE):
        raise PngError("missing PNG signature")
    if limits.max_pixels <= 0 or limits.max_compressed_bytes <= 0:
        raise ValueError("PNG limits must be positive")

    position = len(PNG_SIGNATURE)
    header: tuple[int, int, int, int, int, int, int] | None = None
    palette: bytes | None = None
    transparency: bytes | None = None
    compressed = bytearray()
    saw_idat = False
    saw_end = False

    while position < len(data):
        if len(data) - position < 12:
            raise PngError("truncated PNG chunk header")
        length = struct.unpack_from(">I", data, position)[0]
        kind = data[position + 4 : position + 8]
        end = position + 12 + length
        if end > len(data):
            raise PngError(f"truncated {kind!r} chunk")
        payload = data[position + 8 : position + 8 + length]
        expected_crc = struct.unpack_from(">I", data, position + 8 + length)[0]
        actual_crc = binascii.crc32(payload, binascii.crc32(kind)) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            raise PngError(f"CRC mismatch in {kind.decode('latin1')} chunk")

        if header is None and kind != b"IHDR":
            raise PngError("IHDR must be the first PNG chunk")
        if kind == b"IHDR":
            if header is not None or length != 13:
                raise PngError("invalid or duplicate IHDR")
            header = struct.unpack(">IIBBBBB", payload)
        elif kind == b"PLTE":
            if saw_idat or length == 0 or length > 768 or length % 3:
                raise PngError("invalid PLTE chunk")
            palette = bytes(payload)
        elif kind == b"tRNS":
            if saw_idat:
                raise PngError("tRNS appears after IDAT")
            transparency = bytes(payload)
        elif kind == b"IDAT":
            saw_idat = True
            compressed.extend(payload)
            if len(compressed) > limits.max_compressed_bytes:
                raise PngError("compressed PNG data exceeds the configured limit")
        elif kind == b"IEND":
            if length != 0:
                raise PngError("IEND must be empty")
            saw_end = True
            position = end
            break
        elif kind[0] & 0x20 == 0:
            raise PngError(f"unsupported critical chunk {kind!r}")
        position = end

    if header is None or not saw_idat or not saw_end:
        raise PngError("PNG is missing IHDR, IDAT, or IEND")
    if position != len(data):
        raise PngError("trailing bytes after IEND")

    width, height, depth, color_type, compression, filtering, interlace = header
    if width == 0 or height == 0 or width * height > limits.max_pixels:
        raise PngError("PNG dimensions are zero or exceed the configured limit")
    if compression != 0 or filtering != 0 or interlace != 0:
        raise PngError("only non-interlaced deflate PNGs with adaptive filtering are supported")
    allowed_depths = {
        0: {1, 2, 4, 8},
        2: {8},
        3: {1, 2, 4, 8},
        4: {8},
        6: {8},
    }
    if color_type not in allowed_depths or depth not in allowed_depths[color_type]:
        raise PngError(f"unsupported color-type/bit-depth combination {color_type}/{depth}")
    if color_type == 3 and palette is None:
        raise PngError("indexed PNG is missing PLTE")
    if color_type != 3 and palette is not None and color_type in (0, 4):
        raise PngError("PLTE is forbidden for grayscale PNGs")
    if transparency is not None:
        expected_transparency = {0: 2, 2: 6}.get(color_type)
        if expected_transparency is not None and len(transparency) != expected_transparency:
            raise PngError("invalid tRNS length")
        if color_type == 3 and palette is not None and len(transparency) > len(palette) // 3:
            raise PngError("tRNS has more entries than PLTE")
        if color_type in (4, 6):
            raise PngError("tRNS is forbidden when alpha is already present")

    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color_type]
    row_bytes = (width * channels * depth + 7) // 8
    expected_raw = (row_bytes + 1) * height
    inflater = zlib.decompressobj()
    try:
        raw = inflater.decompress(bytes(compressed), expected_raw + 1)
    except zlib.error as error:
        raise PngError(f"invalid PNG deflate stream: {error}") from error
    if len(raw) > expected_raw:
        raise PngError("PNG decompressed beyond the expected scanline size")
    if not inflater.eof:
        raise PngError("truncated or overlong PNG deflate stream")
    if inflater.unused_data or inflater.unconsumed_tail:
        raise PngError("unexpected data around the PNG deflate stream")
    if len(raw) != expected_raw:
        raise PngError(f"decompressed size {len(raw)} != {expected_raw}")

    bpp = max(1, (channels * depth + 7) // 8)
    previous = bytes(row_bytes)
    rows: list[bytes] = []
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        source = raw[cursor : cursor + row_bytes]
        cursor += row_bytes
        row = _unfilter(filter_type, source, previous, bpp)
        rows.append(_row_to_rgba(bytes(row), width, color_type, depth, palette, transparency))
        previous = row

    return RgbaImage(width, height, b"".join(rows))


def read_png(path: str | Path, limits: Limits = Limits()) -> RgbaImage:
    return decode_png(Path(path).read_bytes(), limits)


def compare_images(expected: RgbaImage, actual: RgbaImage) -> dict[str, object]:
    result: dict[str, object] = {
        "expected": {"width": expected.width, "height": expected.height},
        "actual": {"width": actual.width, "height": actual.height},
    }
    if (expected.width, expected.height) != (actual.width, actual.height):
        result.update(match=False, reason="dimensions")
        return result
    if expected.pixels == actual.pixels:
        result.update(match=True, compared_pixels=expected.width * expected.height, differing_pixels=0)
        return result

    differing = 0
    first: dict[str, object] | None = None
    for pixel in range(expected.width * expected.height):
        offset = pixel * 4
        expected_pixel = expected.pixels[offset : offset + 4]
        actual_pixel = actual.pixels[offset : offset + 4]
        if expected_pixel == actual_pixel:
            continue
        differing += 1
        if first is None:
            channel = next(
                index
                for index, (left, right) in enumerate(zip(expected_pixel, actual_pixel, strict=True))
                if left != right
            )
            first = {
                "x": pixel % expected.width,
                "y": pixel // expected.width,
                "channel": "RGBA"[channel],
                "expected_rgba": list(expected_pixel),
                "actual_rgba": list(actual_pixel),
            }
    result.update(
        match=False,
        reason="pixels",
        compared_pixels=expected.width * expected.height,
        differing_pixels=differing,
        first_difference=first,
    )
    return result


def compare_files(expected: str | Path, actual: str | Path, limits: Limits = Limits()) -> dict[str, object]:
    return compare_images(read_png(expected, limits), read_png(actual, limits))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", type=Path)
    parser.add_argument("actual", type=Path)
    arguments = parser.parse_args(argv)
    try:
        result = compare_files(arguments.expected, arguments.actual)
    except (OSError, PngError, ValueError) as error:
        print(json.dumps({"error": str(error)}), file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result.get("match") else 1


if __name__ == "__main__":
    raise SystemExit(main())
