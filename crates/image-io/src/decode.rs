//! Bounded decoding for the flat raster formats Paint.Rust can save.
//!
//! PNG keeps the dedicated `png` decoder because it exposes exact header,
//! palette, and limit checks. The remaining formats use the `image` crate
//! reader with strict dimension limits and a fully pre-reserved output buffer,
//! so decoded pixels are checked against the caller's bounds before any large
//! allocation happens.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use image::{ColorType, ImageDecoder, ImageError, ImageReader, Limits};
use paint_core::{Bgra, GeometryError, Size, Surface, SurfaceError};
use thiserror::Error;

use crate::dds::decode_uncompressed_dds;
use crate::{PngError, PngLimits, RasterFormat, decode_png_with_limits};

/// Bounds applied to one bounded raster decode.
#[derive(Clone, Debug)]
pub struct RasterLimits {
    pub max_input_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: usize,
    pub max_decoded_bytes: usize,
    pub max_decoder_bytes: usize,
    pub max_alloc_bytes: u64,
}

impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_width: 32_768,
            max_height: 32_768,
            max_pixels: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_decoder_bytes: 64 * 1024 * 1024,
            max_alloc_bytes: 512 * 1024 * 1024,
        }
    }
}

impl RasterLimits {
    /// The equivalent bounds for the dedicated bounded PNG decoder.
    pub fn png_limits(&self) -> PngLimits {
        PngLimits {
            max_input_bytes: self.max_input_bytes,
            max_encoded_bytes: PngLimits::default().max_encoded_bytes,
            max_width: self.max_width,
            max_height: self.max_height,
            max_pixels: self.max_pixels,
            max_decoded_bytes: self.max_decoded_bytes,
            max_decoder_bytes: self.max_decoder_bytes,
        }
    }
}

/// A typed bounded raster decode failure.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unknown image format for path {path}")]
    UnknownFormat { path: PathBuf },
    #[error("unsupported {format} image: {feature}")]
    Unsupported {
        format: &'static str,
        feature: String,
    },
    #[error("failed to decode {format}: {source}")]
    Decode {
        format: &'static str,
        #[source]
        source: image::ImageError,
    },
    #[error("{format} decoder limit exceeded: {reason}")]
    LimitExceeded {
        format: &'static str,
        reason: String,
    },
    #[error("{format} image {resource} limit exceeded: requested {requested}, maximum {maximum}")]
    ResourceLimit {
        format: &'static str,
        resource: &'static str,
        requested: u64,
        maximum: u64,
    },
    #[error("failed to allocate {requested} bytes for {buffer} while decoding {format}")]
    Allocation {
        format: &'static str,
        buffer: &'static str,
        requested: usize,
    },
    #[error("invalid {format} data: {message}")]
    InvalidData {
        format: &'static str,
        message: &'static str,
    },
    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Png(#[from] PngError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
}

/// Decode one flat raster image with the default decode limits.
pub fn decode_image(bytes: &[u8], format: RasterFormat) -> Result<Surface, DecodeError> {
    decode_image_with_limits(bytes, format, &RasterLimits::default())
}

/// Decode one flat raster image, enforcing input, dimension, pixel, decoded
/// byte, and allocation bounds.
pub fn decode_image_with_limits(
    bytes: &[u8],
    format: RasterFormat,
    limits: &RasterLimits,
) -> Result<Surface, DecodeError> {
    match format {
        RasterFormat::Png => Ok(decode_png_with_limits(bytes, &limits.png_limits())?),
        RasterFormat::Dds => {
            // Paint.Rust writes uncompressed BGRA DDS, which the `image` crate's
            // DDS reader rejects. Compressed DXT DDS still goes through the
            // real image decoder.
            match decode_uncompressed_dds(bytes, limits)? {
                Some(surface) => Ok(surface),
                None => decode_with_image_reader(bytes, format, limits),
            }
        }
        _ => decode_with_image_reader(bytes, format, limits),
    }
}

/// Open a flat raster image from disk with the default decode limits.
pub fn open_image(path: impl AsRef<Path>) -> Result<Surface, DecodeError> {
    open_image_with_limits(path, &RasterLimits::default())
}

/// Open a flat raster image from disk, deriving the format from the file
/// extension and applying the supplied bounds to both the read and the decode.
pub fn open_image_with_limits(
    path: impl AsRef<Path>,
    limits: &RasterLimits,
) -> Result<Surface, DecodeError> {
    let path = path.as_ref();
    let format = RasterFormat::from_path(path).ok_or_else(|| DecodeError::UnknownFormat {
        path: path.to_path_buf(),
    })?;
    let bytes = read_bounded(path, format, limits.max_input_bytes)?;
    decode_image_with_limits(&bytes, format, limits)
}

fn decode_with_image_reader(
    bytes: &[u8],
    format: RasterFormat,
    limits: &RasterLimits,
) -> Result<Surface, DecodeError> {
    check_input(format, bytes.len(), limits)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format.image_format());
    // Construction reads headers only. Dimensions are validated below before
    // any pixel buffer is requested, then the strict limits are installed again
    // for the actual read.
    let mut header_limits = Limits::default();
    header_limits.max_alloc = Some(limits.max_alloc_bytes);
    reader.limits(header_limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|source| image_error(format, source))?;
    let (width, height) = decoder.dimensions();
    let size = validate_dimensions(format, width, height, limits)?;
    let color = decoder.color_type();
    if !matches!(
        color,
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8
    ) {
        return Err(DecodeError::Unsupported {
            format: format.name(),
            feature: format!("{color:?} pixel format"),
        });
    }
    let channels = usize::from(color.bytes_per_pixel());
    let total_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(u64::try_from(channels).unwrap_or(u64::MAX)))
        .ok_or_else(|| {
            resource_limit(
                format,
                "decoded bytes",
                u64::MAX,
                limits.max_decoded_bytes as u64,
            )
        })?;
    if total_bytes > limits.max_decoded_bytes as u64 {
        return Err(resource_limit(
            format,
            "decoded bytes",
            total_bytes,
            limits.max_decoded_bytes as u64,
        ));
    }
    if total_bytes > limits.max_alloc_bytes {
        return Err(resource_limit(
            format,
            "allocation bytes",
            total_bytes,
            limits.max_alloc_bytes,
        ));
    }
    let total_bytes = usize::try_from(total_bytes).map_err(|_| {
        resource_limit(
            format,
            "decoded bytes",
            u64::MAX,
            limits.max_decoded_bytes as u64,
        )
    })?;
    let bgra_bytes = u64::try_from(size.checked_buffer_len()?).unwrap_or(u64::MAX);
    let total_allocation = (total_bytes as u64)
        .checked_add(bgra_bytes)
        .ok_or_else(|| {
            resource_limit(format, "allocation bytes", u64::MAX, limits.max_alloc_bytes)
        })?;
    if total_allocation > limits.max_alloc_bytes {
        return Err(resource_limit(
            format,
            "allocation bytes",
            total_allocation,
            limits.max_alloc_bytes,
        ));
    }

    let mut decoder_limits = Limits::default();
    decoder_limits.max_image_width = Some(limits.max_width);
    decoder_limits.max_image_height = Some(limits.max_height);
    decoder_limits.max_alloc = Some(limits.max_alloc_bytes);
    // Account for the exact output buffer before the decoder allocates its own
    // intermediates, so the sum of both stays inside the allocation budget.
    decoder_limits
        .reserve(u64::try_from(total_bytes).unwrap_or(u64::MAX))
        .map_err(|_| DecodeError::Allocation {
            format: format.name(),
            buffer: "decoded image",
            requested: total_bytes,
        })?;
    decoder
        .set_limits(decoder_limits)
        .map_err(|source| image_error(format, source))?;

    let mut raw = allocate_bytes(format, total_bytes, "decoded image")?;
    decoder
        .read_image(&mut raw)
        .map_err(|source| image_error(format, source))?;
    pixels_from_raw(format, size, color, channels, &raw)
}

fn pixels_from_raw(
    format: RasterFormat,
    size: Size,
    color: ColorType,
    channels: usize,
    raw: &[u8],
) -> Result<Surface, DecodeError> {
    let pixel_count = size.checked_pixel_count()?;
    let expected = pixel_count
        .checked_mul(channels)
        .ok_or(DecodeError::InvalidData {
            format: format.name(),
            message: "decoded byte length overflow",
        })?;
    if raw.len() != expected {
        return Err(DecodeError::InvalidData {
            format: format.name(),
            message: "decoded byte length is not exact",
        });
    }
    let requested_bytes = size.checked_buffer_len()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| DecodeError::Allocation {
            format: format.name(),
            buffer: "BGRA pixel buffer",
            requested: requested_bytes,
        })?;
    for chunk in raw.chunks_exact(channels) {
        let pixel = match color {
            ColorType::L8 => Bgra::new(chunk[0], chunk[0], chunk[0], 255),
            ColorType::La8 => Bgra::new(chunk[0], chunk[0], chunk[0], chunk[1]),
            ColorType::Rgb8 => Bgra::new(chunk[2], chunk[1], chunk[0], 255),
            ColorType::Rgba8 => Bgra::new(chunk[2], chunk[1], chunk[0], chunk[3]),
            _ => {
                return Err(DecodeError::Unsupported {
                    format: format.name(),
                    feature: format!("{color:?} pixel format"),
                });
            }
        };
        pixels.push(pixel);
    }
    Ok(Surface::from_pixels(size, pixels)?)
}

pub(crate) fn validate_dimensions(
    format: RasterFormat,
    width: u32,
    height: u32,
    limits: &RasterLimits,
) -> Result<Size, DecodeError> {
    let size = Size::new(width, height)?;
    if width > limits.max_width {
        return Err(resource_limit(
            format,
            "width",
            u64::from(width),
            u64::from(limits.max_width),
        ));
    }
    if height > limits.max_height {
        return Err(resource_limit(
            format,
            "height",
            u64::from(height),
            u64::from(limits.max_height),
        ));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels as u64 {
        return Err(resource_limit(
            format,
            "pixels",
            pixels,
            limits.max_pixels as u64,
        ));
    }
    let decoded_bytes = pixels.checked_mul(4).ok_or_else(|| {
        resource_limit(
            format,
            "decoded bytes",
            u64::MAX,
            limits.max_decoded_bytes as u64,
        )
    })?;
    if decoded_bytes > limits.max_decoded_bytes as u64 {
        return Err(resource_limit(
            format,
            "decoded bytes",
            decoded_bytes,
            limits.max_decoded_bytes as u64,
        ));
    }
    Ok(size)
}

pub(crate) fn allocate_bytes(
    format: RasterFormat,
    length: usize,
    buffer: &'static str,
) -> Result<Vec<u8>, DecodeError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| DecodeError::Allocation {
            format: format.name(),
            buffer,
            requested: length,
        })?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn check_input(
    format: RasterFormat,
    actual: usize,
    limits: &RasterLimits,
) -> Result<(), DecodeError> {
    if actual > limits.max_input_bytes {
        return Err(resource_limit(
            format,
            "input bytes",
            actual as u64,
            limits.max_input_bytes as u64,
        ));
    }
    Ok(())
}

fn read_bounded(path: &Path, format: RasterFormat, maximum: usize) -> Result<Vec<u8>, DecodeError> {
    let file = File::open(path).map_err(|source| DecodeError::Io {
        action: "open image",
        path: path.to_path_buf(),
        source,
    })?;
    let maximum_u64 = u64::try_from(maximum).unwrap_or(u64::MAX);
    let mut reader = file.take(maximum_u64.saturating_add(1));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| DecodeError::Io {
            action: "read image",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > maximum {
        return Err(resource_limit(
            format,
            "input bytes",
            bytes.len() as u64,
            maximum as u64,
        ));
    }
    Ok(bytes)
}

fn image_error(format: RasterFormat, source: ImageError) -> DecodeError {
    match source {
        ImageError::Limits(error) => DecodeError::LimitExceeded {
            format: format.name(),
            reason: format!("{:?}", error.kind()),
        },
        ImageError::Unsupported(error) => DecodeError::Unsupported {
            format: format.name(),
            feature: format!("{:?}", error.kind()),
        },
        source => DecodeError::Decode {
            format: format.name(),
            source,
        },
    }
}

fn resource_limit(
    format: RasterFormat,
    resource: &'static str,
    requested: u64,
    maximum: u64,
) -> DecodeError {
    DecodeError::ResourceLimit {
        format: format.name(),
        resource,
        requested,
        maximum,
    }
}

#[cfg(test)]
mod tests {
    use image::{ExtendedColorType, ImageEncoder};
    use paint_core::{Bgra, Size};

    use super::*;
    use crate::encode_image;

    fn surface() -> Surface {
        Surface::from_pixels(
            Size::new(2, 2).expect("size"),
            vec![
                Bgra::new(1, 2, 3, 4),
                Bgra::new(5, 6, 7, 8),
                Bgra::new(9, 10, 11, 12),
                Bgra::new(13, 14, 15, 16),
            ],
        )
        .expect("surface")
    }

    #[test]
    fn all_encodable_formats_decode_with_exact_or_tolerant_pixels() {
        let source = surface();
        for format in RasterFormat::ALL {
            let bytes = encode_image(&source, format).expect("encode");
            let decoded = decode_image(&bytes, format).expect("decode");
            assert_eq!(decoded.size(), Size::new(2, 2).expect("size"));
            if matches!(format, RasterFormat::Jpeg | RasterFormat::Gif) {
                continue;
            }
            assert_eq!(decoded, source, "{} pixels", format.name());
        }
    }

    #[test]
    fn grayscale_and_grayscale_alpha_decode_exactly_to_bgra() {
        let mut gray = Vec::new();
        image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut gray))
            .write_image(&[42, 7], 2, 1, ExtendedColorType::L8)
            .expect("encode gray TIFF");
        let decoded = decode_image(&gray, RasterFormat::Tiff).expect("decode gray TIFF");
        assert_eq!(
            decoded.pixels(),
            &[Bgra::new(42, 42, 42, 255), Bgra::new(7, 7, 7, 255)]
        );

        let mut gray_alpha = Vec::new();
        image::codecs::tga::TgaEncoder::new(Cursor::new(&mut gray_alpha))
            .write_image(&[42, 7, 9, 4], 2, 1, ExtendedColorType::La8)
            .expect("encode gray alpha TGA");
        let decoded = decode_image(&gray_alpha, RasterFormat::Tga).expect("decode gray alpha TGA");
        assert_eq!(
            decoded.pixels(),
            &[Bgra::new(42, 42, 42, 7), Bgra::new(9, 9, 9, 4)]
        );
    }

    #[test]
    fn sixteen_bit_tiff_is_explicitly_unsupported() {
        let mut bytes = Vec::new();
        image::codecs::tiff::TiffEncoder::new(Cursor::new(&mut bytes))
            .write_image(&[0; 8], 1, 1, ExtendedColorType::Rgba16)
            .expect("encode 16-bit TIFF");
        assert!(matches!(
            decode_image(&bytes, RasterFormat::Tiff),
            Err(DecodeError::Unsupported { format: "TIFF", .. })
        ));
    }

    #[test]
    fn png_keeps_its_dedicated_bounded_decoder() {
        let source = surface();
        let bytes = encode_image(&source, RasterFormat::Png).expect("encode PNG");
        assert_eq!(
            decode_image(&bytes, RasterFormat::Png).expect("decode PNG"),
            source
        );
        let limits = RasterLimits {
            max_width: 1,
            ..RasterLimits::default()
        };
        assert!(matches!(
            decode_image_with_limits(&bytes, RasterFormat::Png, &limits),
            Err(DecodeError::Png(PngError::ResourceLimit {
                resource: "width",
                ..
            }))
        ));
    }

    #[test]
    fn input_limit_is_checked_before_reading_pixels() {
        let source = surface();
        let bytes = encode_image(&source, RasterFormat::Bmp).expect("encode BMP");
        let limits = RasterLimits {
            max_input_bytes: bytes.len() - 1,
            ..RasterLimits::default()
        };
        assert!(matches!(
            decode_image_with_limits(&bytes, RasterFormat::Bmp, &limits),
            Err(DecodeError::ResourceLimit {
                resource: "input bytes",
                ..
            })
        ));
    }

    #[test]
    fn non_file_paths_and_unknown_extensions_are_typed_errors() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        assert!(matches!(
            open_image_with_limits(
                directory.path().join("missing.png"),
                &RasterLimits::default()
            ),
            Err(DecodeError::Io { .. })
        ));
        assert!(matches!(
            open_image_with_limits(directory.path().join("image.txt"), &RasterLimits::default()),
            Err(DecodeError::UnknownFormat { .. })
        ));
        let directory_named_like_an_image = directory.path().join("folder.png");
        std::fs::create_dir(&directory_named_like_an_image).expect("directory");
        assert!(matches!(
            open_image_with_limits(&directory_named_like_an_image, &RasterLimits::default()),
            Err(DecodeError::Io { .. })
        ));
    }
}
