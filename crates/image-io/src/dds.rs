//! Bounded decoding for uncompressed DDS surfaces.
//!
//! Paint.Rust saves uncompressed BGRA DDS, which the `image` crate's DDS
//! reader does not accept (it only implements the DXT/BC variants). This
//! module reads the real DDS header, validates every dimension and data bound,
//! and expands the declared channel masks to exact BGRA pixels. Compressed DDS
//! is left to the `image` crate.

use paint_core::{Bgra, Surface};

use crate::decode::validate_dimensions;
use crate::{DecodeError, RasterFormat, RasterLimits};

const DDS_MAGIC: &[u8; 4] = b"DDS ";
const DDS_HEADER_BYTES: usize = 128;
const DDS_PIXEL_FORMAT_BYTES: u32 = 32;
const DDSD_CAPS: u32 = 0x1_000;
const DDSD_PIXELFORMAT: u32 = 0x4;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;

/// Decode an uncompressed DDS surface.
///
/// Returns `Ok(None)` for compressed (FourCC) surfaces so the caller can use
/// the real `image` crate DDS decoder.
pub(crate) fn decode_uncompressed_dds(
    bytes: &[u8],
    limits: &RasterLimits,
) -> Result<Option<Surface>, DecodeError> {
    if bytes.len() < DDS_HEADER_BYTES {
        return Err(invalid("file is shorter than the DDS header"));
    }
    if &bytes[0..4] != DDS_MAGIC {
        return Err(invalid("DDS signature is missing"));
    }
    let header = &bytes[4..DDS_HEADER_BYTES];
    if read_u32(header, 0) != 124 {
        return Err(invalid("DDS header size is not 124"));
    }
    let flags = read_u32(header, 4);
    if flags & DDSD_PIXELFORMAT == 0 || flags & DDSD_CAPS == 0 {
        return Err(invalid("DDS header is missing required flags"));
    }
    let height = read_u32(header, 8);
    let width = read_u32(header, 12);
    let header_pitch = read_u32(header, 16);
    let depth = read_u32(header, 20);
    let pixel_format_size = read_u32(header, 76);
    let pixel_format_flags = read_u32(header, 80);
    if pixel_format_flags & DDPF_FOURCC != 0 {
        return Ok(None);
    }
    if pixel_format_size != DDS_PIXEL_FORMAT_BYTES {
        return Err(invalid("DDS pixel format size is not 32"));
    }
    if pixel_format_flags & DDPF_RGB == 0 {
        return Err(unsupported("DDS pixel format is not RGB"));
    }
    if depth > 1 {
        return Err(unsupported("3D DDS volumes"));
    }
    let size = validate_dimensions(RasterFormat::Dds, width, height, limits)?;

    let bit_count = read_u32(header, 88);
    let bytes_per_pixel = match bit_count {
        24 => 3,
        32 => 4,
        other => {
            return Err(unsupported(format!("DDS {other}-bit uncompressed pixels")));
        }
    };
    let red_mask = read_u32(header, 92);
    let green_mask = read_u32(header, 96);
    let blue_mask = read_u32(header, 100);
    let alpha_mask = read_u32(header, 104);
    if red_mask == 0 || green_mask == 0 || blue_mask == 0 {
        return Err(unsupported("DDS RGB channel masks are incomplete"));
    }

    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(bytes_per_pixel))
        .ok_or_else(|| invalid("DDS row length does not fit the host address space"))?;
    let pitch = usize::try_from(header_pitch)
        .ok()
        .filter(|pitch| *pitch >= row_bytes)
        .unwrap_or(row_bytes);
    let height = usize::try_from(height)
        .map_err(|_| invalid("DDS height does not fit the host address space"))?;
    let required = pitch
        .checked_mul(height.saturating_sub(1))
        .and_then(|bytes| bytes.checked_add(row_bytes))
        .ok_or_else(|| invalid("DDS pixel data length overflow"))?;
    let data = &bytes[DDS_HEADER_BYTES..];
    if data.len() < required {
        return Err(invalid("DDS pixel data is truncated"));
    }

    let requested_bytes = size.checked_buffer_len()?;
    if requested_bytes as u64 > limits.max_alloc_bytes {
        return Err(DecodeError::ResourceLimit {
            format: RasterFormat::Dds.name(),
            resource: "allocation bytes",
            requested: requested_bytes as u64,
            maximum: limits.max_alloc_bytes,
        });
    }
    let pixel_count = size.checked_pixel_count()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| DecodeError::Allocation {
            format: RasterFormat::Dds.name(),
            buffer: "BGRA pixel buffer",
            requested: requested_bytes,
        })?;
    for row in 0..height {
        let start = row
            .checked_mul(pitch)
            .ok_or_else(|| invalid("DDS row offset overflow"))?;
        let row_data = &data[start..start + row_bytes];
        for pixel in row_data.chunks_exact(bytes_per_pixel) {
            let value = u32::from_le_bytes([
                pixel[0],
                pixel[1],
                pixel[2],
                if bytes_per_pixel == 4 { pixel[3] } else { 0 },
            ]);
            let alpha = if alpha_mask == 0 {
                255
            } else {
                channel(value, alpha_mask)
            };
            pixels.push(Bgra::new(
                channel(value, blue_mask),
                channel(value, green_mask),
                channel(value, red_mask),
                alpha,
            ));
        }
    }
    Ok(Some(Surface::from_pixels(size, pixels)?))
}

fn channel(value: u32, mask: u32) -> u8 {
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    if maximum == 0 {
        return 0;
    }
    let component = (value & mask) >> shift;
    let scaled = (u64::from(component) * 255 + u64::from(maximum) / 2) / u64::from(maximum);
    scaled as u8
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn invalid(message: &'static str) -> DecodeError {
    DecodeError::InvalidData {
        format: RasterFormat::Dds.name(),
        message,
    }
}

fn unsupported(feature: impl Into<String>) -> DecodeError {
    DecodeError::Unsupported {
        format: RasterFormat::Dds.name(),
        feature: feature.into(),
    }
}
