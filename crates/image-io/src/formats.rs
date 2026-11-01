//! Raster save formats that can be encoded without an OS codec installation.

use std::io::Cursor;

use image::{ExtendedColorType, ImageEncoder};
use paint_core::{Surface, SurfaceError};
use thiserror::Error;

use crate::{AtomicWriteError, PngError, atomic_write, encode_png};

/// Save formats exposed by the file dialog. PDN is intentionally handled by the
/// layered PDN codec in `paint-pdn3`, not by this flat-raster module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveFormat {
    Png,
    Jpeg,
    Tiff,
    Bmp,
    Tga,
    Dds,
    Gif,
    WebP,
}

impl SaveFormat {
    pub const ALL: [Self; 8] = [
        Self::Png,
        Self::Jpeg,
        Self::Tiff,
        Self::Bmp,
        Self::Tga,
        Self::Dds,
        Self::Gif,
        Self::WebP,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG (*.png)",
            Self::Jpeg => "JPEG (*.jpg; *.jpeg; *.jpe; *.jfif; *.exif)",
            Self::Tiff => "TIFF (*.tiff; *.tif)",
            Self::Bmp => "BMP (*.bmp; *.dib)",
            Self::Tga => "TGA (*.tga)",
            Self::Dds => "DDS (*.dds)",
            Self::Gif => "GIF (*.gif)",
            Self::WebP => "WebP (*.webp)",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
            Self::Bmp => "bmp",
            Self::Tga => "tga",
            Self::Dds => "dds",
            Self::Gif => "gif",
            Self::WebP => "webp",
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" | "jpe" | "jfif" | "exif" => Some(Self::Jpeg),
            "tif" | "tiff" => Some(Self::Tiff),
            "bmp" | "dib" => Some(Self::Bmp),
            "tga" => Some(Self::Tga),
            "dds" => Some(Self::Dds),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::WebP),
            _ => None,
        }
    }

    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error(transparent)]
    Png(#[from] PngError),
    #[error("failed to encode {format}: {source}")]
    Encode {
        format: &'static str,
        #[source]
        source: image::ImageError,
    },
    #[error(transparent)]
    Atomic(#[from] AtomicWriteError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error("image dimensions are too large to encode")]
    Dimensions,
}

/// Encode the composed surface in one of the supported flat raster formats.
pub fn encode_image(surface: &Surface, format: SaveFormat) -> Result<Vec<u8>, FormatError> {
    if format == SaveFormat::Png {
        return Ok(encode_png(surface)?);
    }

    let rgba = surface.to_rgba_bytes()?;
    let width = surface.width();
    let height = surface.height();
    match format {
        SaveFormat::Png => unreachable!("handled above"),
        SaveFormat::Jpeg => encode_jpeg(&rgba, width, height),
        SaveFormat::Tiff => {
            let mut output = Vec::new();
            image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut output))
                .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|source| FormatError::Encode {
                    format: "TIFF",
                    source,
                })?;
            Ok(output)
        }
        SaveFormat::Bmp => {
            let mut output = Vec::new();
            image::codecs::bmp::BmpEncoder::new(&mut output)
                .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|source| FormatError::Encode {
                    format: "BMP",
                    source,
                })?;
            Ok(output)
        }
        SaveFormat::Tga => {
            let mut output = Vec::new();
            image::codecs::tga::TgaEncoder::new(&mut output)
                .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|source| FormatError::Encode {
                    format: "TGA",
                    source,
                })?;
            Ok(output)
        }
        SaveFormat::Dds => encode_dds_bgra(&surface.to_bgra_bytes()?, width, height),
        SaveFormat::Gif => {
            let mut output = Vec::new();
            image::codecs::gif::GifEncoder::new(&mut output)
                .encode(&rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|source| FormatError::Encode {
                    format: "GIF",
                    source,
                })?;
            Ok(output)
        }
        SaveFormat::WebP => {
            let mut output = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut output)
                .encode(&rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|source| FormatError::Encode {
                    format: "WebP",
                    source,
                })?;
            Ok(output)
        }
    }
}

/// Encode and atomically replace `path`.
pub fn write_image_atomic(
    path: impl AsRef<std::path::Path>,
    surface: &Surface,
    format: SaveFormat,
) -> Result<(), FormatError> {
    let bytes = encode_image(surface, format)?;
    atomic_write(path, &bytes)?;
    Ok(())
}

fn encode_jpeg(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, FormatError> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        let alpha = u32::from(pixel[3]);
        let blend = |channel: u8| -> u8 {
            let value = u32::from(channel);
            // JPEG has no alpha; Paint.NET-style export flattens over white.
            ((value * alpha + 255 * (255 - alpha)) / 255) as u8
        };
        rgb.extend_from_slice(&[blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]);
    }
    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 90)
        .encode(&rgb, width, height, ExtendedColorType::Rgb8)
        .map_err(|source| FormatError::Encode {
            format: "JPEG",
            source,
        })?;
    Ok(output)
}

/// Uncompressed BGRA DDS. This is a real DDS accessible to DDS readers without
/// pulling in a block-compression encoder.
fn encode_dds_bgra(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>, FormatError> {
    let pitch = width.checked_mul(4).ok_or(FormatError::Dimensions)?;
    let expected = usize::try_from(pitch)
        .ok()
        .and_then(|pitch| pitch.checked_mul(height as usize))
        .ok_or(FormatError::Dimensions)?;
    if bgra.len() != expected {
        return Err(FormatError::Dimensions);
    }
    let mut output = Vec::with_capacity(128 + bgra.len());
    output.extend_from_slice(b"DDS ");
    let mut header = [0u8; 124];
    put_u32(&mut header, 0, 124); // dwSize
    put_u32(&mut header, 4, 0x0000_1007); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put_u32(&mut header, 8, height);
    put_u32(&mut header, 12, width);
    put_u32(&mut header, 16, pitch);
    put_u32(&mut header, 20, 0); // depth
    put_u32(&mut header, 24, 0); // mipmaps
    put_u32(&mut header, 76, 32); // DDS_PIXELFORMAT.dwSize
    put_u32(&mut header, 80, 0x41); // DDPF_RGB | DDPF_ALPHAPIXELS
    put_u32(&mut header, 88, 32); // bit count
    put_u32(&mut header, 92, 0x00ff_0000); // red
    put_u32(&mut header, 96, 0x0000_ff00); // green
    put_u32(&mut header, 100, 0x0000_00ff); // blue
    put_u32(&mut header, 104, 0xff00_0000); // alpha
    put_u32(&mut header, 108, 0x1000); // DDSCAPS_TEXTURE
    output.extend_from_slice(&header);
    output.extend_from_slice(bgra);
    Ok(output)
}

fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
