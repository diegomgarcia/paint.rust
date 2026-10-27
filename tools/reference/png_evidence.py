#!/usr/bin/env python3
"""Bounded, dependency-free PNG decoding and RGBA evidence for reference outputs.

The decoder intentionally supports non-interlaced PNG only. Paint.NET's observed
outputs are non-interlaced; Adam7 input fails explicitly rather than being
silently misdecoded. Sixteen-bit samples are reduced to their high byte for the
RGBA8 evidence representation.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import math
import struct
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
MAX_ENCODED_BYTES = 64 * 1024 * 1024
MAX_PIXELS = 16_000_000
MAX_RGBA_BYTES = MAX_PIXELS * 4


class PngError(ValueError):
    """The input is not a supported, well-formed PNG."""


@dataclass(frozen=True)
class DecodedPng:
    width: int
    height: int
    rgba: bytes
    bit_depth: int
    color_type: int
    interlace: int


def _checked_dimensions(width: int, height: int) -> None:
    if width <= 0 or height <= 0:
        raise PngError("PNG dimensions must be positive")
    if width > MAX_PIXELS or height > MAX_PIXELS or width * height > MAX_PIXELS:
        raise PngError(f"PNG exceeds the {MAX_PIXELS}-pixel allocation bound")


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


def _unfilter(filtered: bytes, width: int, height: int, row_bytes: int, filter_bpp: int) -> list[bytes]:
    expected = height * (row_bytes + 1)
    if len(filtered) != expected:
        raise PngError(f"inflated scanline size {len(filtered)} does not equal {expected}")
    rows: list[bytes] = []
    previous = bytes(row_bytes)
    offset = 0
    for row_index in range(height):
        filter_type = filtered[offset]
        offset += 1
        if filter_type > 4:
            raise PngError(f"invalid PNG filter {filter_type} on row {row_index}")
        current = bytearray(row_bytes)
        for index in range(row_bytes):
            encoded = filtered[offset + index]
            left = current[index - filter_bpp] if index >= filter_bpp else 0
            above = previous[index]
            upper_left = previous[index - filter_bpp] if index >= filter_bpp else 0
            if filter_type == 0:
                predictor = 0
            elif filter_type == 1:
                predictor = left
            elif filter_type == 2:
                predictor = above
            elif filter_type == 3:
                predictor = (left + above) // 2
            else:
                predictor = _paeth(left, above, upper_left)
            current[index] = (encoded + predictor) & 0xFF
        offset += row_bytes
        row = bytes(current)
        rows.append(row)
        previous = row
    return rows


def _inflate_bounded(compressed: bytes, expected: int) -> bytes:
    inflater = zlib.decompressobj()
    try:
        output = inflater.decompress(compressed, expected + 1)
        if len(output) > expected or inflater.unconsumed_tail:
            raise PngError("inflated PNG data exceeds the computed scanline bound")
        output += inflater.flush()
    except zlib.error as error:
        raise PngError(f"invalid PNG zlib stream: {error}") from error
    if len(output) != expected:
        raise PngError(f"inflated PNG data has {len(output)} bytes; expected {expected}")
    if not inflater.eof or inflater.unused_data:
        raise PngError("PNG zlib stream is truncated or has trailing compressed data")
    return output


def decode_png(path: Path) -> DecodedPng:
    path = Path(path)
    try:
        size = path.stat().st_size
        if size > MAX_ENCODED_BYTES:
            raise PngError(f"PNG exceeds the {MAX_ENCODED_BYTES}-byte encoded bound")
        data = path.read_bytes()
    except OSError as error:
        raise PngError(f"cannot read {path}: {error}") from error
    if not data.startswith(PNG_SIGNATURE):
        raise PngError("missing PNG signature")

    offset = len(PNG_SIGNATURE)
    ihdr: tuple[int, int, int, int, int, int, int] | None = None
    idat = bytearray()
    palette: bytes | None = None
    transparency: bytes | None = None
    saw_iend = False
    seen_chunks: set[bytes] = set()
    while offset < len(data):
        if len(data) - offset < 12:
            raise PngError("truncated PNG chunk header")
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4 : offset + 8]
        end = offset + 12 + length
        if end > len(data):
            raise PngError(f"truncated {kind!r} chunk")
        payload = data[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack_from(">I", data, offset + 8 + length)[0]
        actual_crc = zlib.crc32(kind + payload) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            raise PngError(f"CRC mismatch in {kind!r} chunk")
        if kind == b"IHDR":
            if ihdr is not None or offset != 8 or length != 13:
                raise PngError("IHDR must be the first and only header chunk")
            ihdr = struct.unpack(">IIBBBBB", payload)
        elif kind == b"PLTE":
            if b"PLTE" in seen_chunks or idat:
                raise PngError("duplicate or misplaced PLTE chunk")
            palette = payload
        elif kind == b"tRNS":
            if b"tRNS" in seen_chunks or idat:
                raise PngError("duplicate or misplaced tRNS chunk")
            transparency = payload
        elif kind == b"IDAT":
            if ihdr is None or saw_iend:
                raise PngError("IDAT without IHDR or after IEND")
            idat.extend(payload)
        elif kind == b"IEND":
            if length != 0 or end != len(data):
                raise PngError("IEND must be empty and terminate the file")
            saw_iend = True
        elif kind[:1].isupper():
            raise PngError(f"unsupported critical PNG chunk {kind!r}")
        seen_chunks.add(kind)
        offset = end
        if saw_iend:
            break

    if ihdr is None or not saw_iend or not idat:
        raise PngError("PNG requires IHDR, IDAT, and IEND")
    width, height, bit_depth, color_type, compression, filtering, interlace = ihdr
    _checked_dimensions(width, height)
    if compression != 0 or filtering != 0:
        raise PngError("unsupported PNG compression or filtering method")
    if interlace != 0:
        raise PngError("interlaced PNG is unsupported; re-export non-interlaced")
    channels_by_type = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
    if color_type not in channels_by_type:
        raise PngError(f"invalid PNG color type {color_type}")
    channels = channels_by_type[color_type]
    allowed_depths = {
        0: {1, 2, 4, 8, 16},
        2: {8, 16},
        3: {1, 2, 4, 8},
        4: {8, 16},
        6: {8, 16},
    }
    if bit_depth not in allowed_depths[color_type]:
        raise PngError(f"bit depth {bit_depth} is invalid for color type {color_type}")
    if color_type == 3:
        if palette is None or not palette or len(palette) % 3 or len(palette) > 768:
            raise PngError("indexed PNG requires a palette of 1..256 RGB entries")
    elif palette is not None and color_type in {0, 4}:
        raise PngError("PLTE is forbidden for grayscale PNG")

    if transparency is not None:
        required = {0: 2, 2: 6}.get(color_type)
        if required is not None and len(transparency) != required:
            raise PngError("invalid grayscale/RGB tRNS length")
        if color_type in {4, 6}:
            raise PngError("tRNS is forbidden when PNG has an alpha channel")
        if color_type == 3 and (palette is None or len(transparency) > len(palette) // 3):
            raise PngError("indexed tRNS exceeds the palette")

    bits_per_pixel = channels * bit_depth
    row_bytes = (width * bits_per_pixel + 7) // 8
    filter_bpp = max(1, (bits_per_pixel + 7) // 8)
    filtered = _inflate_bounded(bytes(idat), height * (row_bytes + 1))
    rows = _unfilter(filtered, width, height, row_bytes, filter_bpp)
    rgba = bytearray(width * height * 4)
    max_sample = (1 << bit_depth) - 1
    output_offset = 0

    transparent_gray = struct.unpack(">H", transparency)[0] if transparency and color_type == 0 else None
    transparent_rgb = struct.unpack(">HHH", transparency) if transparency and color_type == 2 else None
    palette_alphas = list(transparency or b"") if color_type == 3 else []

    for row in rows:
        for x in range(width):
            if bit_depth < 8:
                bit_offset = x * bit_depth
                shift = 8 - bit_depth - (bit_offset % 8)
                samples = [(row[bit_offset // 8] >> shift) & max_sample]
            elif bit_depth == 8:
                start = x * channels
                samples = list(row[start : start + channels])
            else:
                start = x * channels * 2
                samples = [struct.unpack_from(">H", row, start + 2 * channel)[0] for channel in range(channels)]

            if color_type == 3:
                index = samples[0]
                if palette is None or index >= len(palette) // 3:
                    raise PngError(f"palette index {index} is out of range")
                red, green, blue = palette[index * 3 : index * 3 + 3]
                alpha = palette_alphas[index] if index < len(palette_alphas) else 255
            else:
                converted = [(sample >> 8) if bit_depth == 16 else sample for sample in samples]
                if bit_depth < 8:
                    converted = [(sample * 255 + max_sample // 2) // max_sample for sample in converted]
                if color_type == 0:
                    red = green = blue = converted[0]
                    alpha = 0 if transparent_gray is not None and samples[0] == transparent_gray else 255
                elif color_type == 2:
                    red, green, blue = converted
                    alpha = 0 if transparent_rgb is not None and tuple(samples) == transparent_rgb else 255
                elif color_type == 4:
                    red = green = blue = converted[0]
                    alpha = converted[1]
                else:
                    red, green, blue, alpha = converted
            rgba[output_offset : output_offset + 4] = bytes((red, green, blue, alpha))
            output_offset += 4

    return DecodedPng(width, height, bytes(rgba), bit_depth, color_type, interlace)


def rgba_evidence(decoded: DecodedPng, *, histogram_limit: int = 32) -> dict[str, Any]:
    counts: collections.Counter[int] = collections.Counter(
        struct.unpack_from(">I", decoded.rgba, offset)[0]
        for offset in range(0, len(decoded.rgba), 4)
    )
    ordered = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    result: dict[str, Any] = {
        "width": decoded.width,
        "height": decoded.height,
        "decoded_rgba_bytes": len(decoded.rgba),
        "pixel_count": decoded.width * decoded.height,
        "row_stride_bytes": decoded.width * 4,
        "rgba_sha256": hashlib.sha256(decoded.rgba).hexdigest(),
        "first_pixel_rgba": list(decoded.rgba[:4]),
        "all_pixels_equal": len(counts) == 1,
        "unique_color_count": len(counts),
        "source_bit_depth": decoded.bit_depth,
        "source_color_type": decoded.color_type,
        "interlace": decoded.interlace,
    }
    if len(counts) <= histogram_limit:
        result["colors"] = [
            {"rgba": list(struct.pack(">I", value)), "count": count} for value, count in ordered
        ]
    else:
        result["colors_omitted"] = f"histogram exceeds {histogram_limit} colors"
    return result


def png_file_evidence(path: Path) -> dict[str, Any]:
    path = Path(path)
    decoded = decode_png(path)
    result = rgba_evidence(decoded)
    result.update(
        {
            "file": path.name,
            "encoded_bytes": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "decoder": "tools/reference/png_evidence.py (Python stdlib, non-interlaced PNG to RGBA8)",
        }
    )
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("png", type=Path)
    args = parser.parse_args(argv)
    try:
        evidence = png_file_evidence(args.png)
    except (PngError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    json.dump(evidence, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
