use std::io::{self, Cursor, Write};

use paint_core::{Bgra, GeometryError, Size, Surface, SurfaceError};
use thiserror::Error;

use crate::{AtomicWriteError, atomic_write};

#[derive(Clone, Debug)]
pub struct PngLimits {
    pub max_input_bytes: usize,
    pub max_encoded_bytes: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: usize,
    pub max_decoded_bytes: usize,
    pub max_decoder_bytes: usize,
}

impl Default for PngLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_encoded_bytes: 512 * 1024 * 1024,
            max_width: 32_768,
            max_height: 32_768,
            max_pixels: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_decoder_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum PngError {
    #[error("PNG {resource} limit exceeded: requested {requested}, maximum {maximum}")]
    ResourceLimit {
        resource: &'static str,
        requested: u64,
        maximum: usize,
    },
    #[error("unsupported PNG feature: {0}")]
    Unsupported(&'static str),
    #[error("invalid decoded PNG layout: {0}")]
    InvalidData(&'static str),
    #[error("failed to allocate {requested} bytes for PNG {buffer}")]
    Allocation {
        buffer: &'static str,
        requested: usize,
    },
    #[error(transparent)]
    Decode(#[from] png::DecodingError),
    #[error(transparent)]
    Encode(#[from] png::EncodingError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Atomic(#[from] AtomicWriteError),
}

pub fn decode_png(bytes: &[u8]) -> Result<Surface, PngError> {
    decode_png_with_limits(bytes, &PngLimits::default())
}

pub fn decode_png_with_limits(bytes: &[u8], limits: &PngLimits) -> Result<Surface, PngError> {
    if bytes.len() > limits.max_input_bytes {
        return Err(resource_limit(
            "input bytes",
            bytes.len(),
            limits.max_input_bytes,
        ));
    }
    let decoder_limits = png::Limits {
        bytes: limits.max_decoder_bytes,
    };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), decoder_limits);
    decoder.set_transformations(png::Transformations::EXPAND);
    let (width, height, bit_depth) = {
        let info = decoder
            .read_header_info()
            .map_err(|error| decode_error(error, limits))?;
        (info.width, info.height, info.bit_depth)
    };
    let size = validate_dimensions(width, height, limits)?;
    if bit_depth == png::BitDepth::Sixteen {
        return Err(PngError::Unsupported("16-bit PNG channels"));
    }
    let mut reader = decoder
        .read_info()
        .map_err(|error| decode_error(error, limits))?;
    if reader.info().animation_control.is_some() {
        return Err(PngError::Unsupported("animated PNG"));
    }
    let raw_size = reader
        .output_buffer_size()
        .ok_or_else(|| resource_limit("decoded bytes", usize::MAX, limits.max_decoded_bytes))?;
    if raw_size > limits.max_decoded_bytes {
        return Err(resource_limit(
            "decoded bytes",
            raw_size,
            limits.max_decoded_bytes,
        ));
    }
    let mut raw = allocate_bytes(raw_size, "decoded pixel buffer")?;
    let output = reader
        .next_frame(&mut raw)
        .map_err(|error| decode_error(error, limits))?;
    reader
        .finish()
        .map_err(|error| decode_error(error, limits))?;
    if output.width != width || output.height != height || output.bit_depth != png::BitDepth::Eight
    {
        return Err(PngError::InvalidData(
            "decoded dimensions or bit depth changed unexpectedly",
        ));
    }
    let channels = match output.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(PngError::InvalidData(
                "palette expansion did not produce direct color",
            ));
        }
    };
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(channels))
        .ok_or(PngError::InvalidData("decoded row length overflow"))?;
    let height = usize::try_from(height)
        .map_err(|_| PngError::InvalidData("height does not fit the host address space"))?;
    let expected_decoded_bytes = row_bytes
        .checked_mul(height)
        .ok_or(PngError::InvalidData("decoded byte length overflow"))?;
    if output.line_size != row_bytes
        || output.buffer_size() != expected_decoded_bytes
        || raw.len() < expected_decoded_bytes
    {
        return Err(PngError::InvalidData(
            "decoded byte length or row stride is not exact",
        ));
    }
    let pixel_count = size.checked_pixel_count()?;
    let requested_bytes = size.checked_buffer_len()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| PngError::Allocation {
            buffer: "BGRA pixel buffer",
            requested: requested_bytes,
        })?;
    for row in 0..height {
        let start = row
            .checked_mul(row_bytes)
            .ok_or(PngError::InvalidData("decoded row offset overflow"))?;
        let row_data = &raw[start..start + row_bytes];
        for pixel in row_data.chunks_exact(channels) {
            pixels.push(match output.color_type {
                png::ColorType::Grayscale => Bgra::new(pixel[0], pixel[0], pixel[0], 255),
                png::ColorType::GrayscaleAlpha => Bgra::new(pixel[0], pixel[0], pixel[0], pixel[1]),
                png::ColorType::Rgb => Bgra::new(pixel[2], pixel[1], pixel[0], 255),
                png::ColorType::Rgba => Bgra::new(pixel[2], pixel[1], pixel[0], pixel[3]),
                png::ColorType::Indexed => {
                    return Err(PngError::InvalidData("unexpected indexed output"));
                }
            });
        }
    }
    Ok(Surface::from_pixels(size, pixels)?)
}

pub fn encode_png(surface: &Surface) -> Result<Vec<u8>, PngError> {
    encode_png_with_limits(surface, &PngLimits::default())
}

pub fn encode_png_with_limits(surface: &Surface, limits: &PngLimits) -> Result<Vec<u8>, PngError> {
    validate_dimensions(surface.width(), surface.height(), limits)?;
    let rgba = surface.to_rgba_bytes()?;
    let mut output = BoundedBuffer {
        bytes: Vec::new(),
        maximum: limits.max_encoded_bytes,
        limit_exceeded: false,
    };
    let result = (|| -> Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(&mut output, surface.width(), surface.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgba)?;
        writer.finish()
    })();
    if let Err(error) = result {
        if output.limit_exceeded {
            return Err(resource_limit(
                "encoded bytes",
                output.bytes.len().saturating_add(1),
                limits.max_encoded_bytes,
            ));
        }
        return Err(PngError::Encode(error));
    }
    Ok(output.bytes)
}

pub fn write_png_atomic(
    path: impl AsRef<std::path::Path>,
    surface: &Surface,
) -> Result<(), PngError> {
    let bytes = encode_png(surface)?;
    atomic_write(path, &bytes)?;
    Ok(())
}

fn validate_dimensions(width: u32, height: u32, limits: &PngLimits) -> Result<Size, PngError> {
    let size = Size::new(width, height)?;
    if width > limits.max_width {
        return Err(resource_limit(
            "width",
            width as usize,
            limits.max_width as usize,
        ));
    }
    if height > limits.max_height {
        return Err(resource_limit(
            "height",
            height as usize,
            limits.max_height as usize,
        ));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels as u64 {
        return Err(resource_limit_u64("pixels", pixels, limits.max_pixels));
    }
    let decoded_bytes = pixels
        .checked_mul(4)
        .ok_or_else(|| resource_limit_u64("decoded bytes", u64::MAX, limits.max_decoded_bytes))?;
    if decoded_bytes > limits.max_decoded_bytes as u64 {
        return Err(resource_limit_u64(
            "decoded bytes",
            decoded_bytes,
            limits.max_decoded_bytes,
        ));
    }
    Ok(size)
}

fn decode_error(error: png::DecodingError, limits: &PngLimits) -> PngError {
    if matches!(&error, png::DecodingError::LimitsExceeded) {
        resource_limit(
            "decoder bytes",
            limits.max_decoder_bytes.saturating_add(1),
            limits.max_decoder_bytes,
        )
    } else {
        PngError::Decode(error)
    }
}

fn resource_limit(resource: &'static str, requested: usize, maximum: usize) -> PngError {
    resource_limit_u64(resource, requested as u64, maximum)
}

fn resource_limit_u64(resource: &'static str, requested: u64, maximum: usize) -> PngError {
    PngError::ResourceLimit {
        resource,
        requested,
        maximum,
    }
}

fn allocate_bytes(length: usize, buffer: &'static str) -> Result<Vec<u8>, PngError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| PngError::Allocation {
            buffer,
            requested: length,
        })?;
    bytes.resize(length, 0);
    Ok(bytes)
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    maximum: usize,
    limit_exceeded: bool,
}

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(end) = self.bytes.len().checked_add(bytes.len()) else {
            self.limit_exceeded = true;
            return Err(io::Error::other("encoded PNG length overflow"));
        };
        if end > self.maximum {
            self.limit_exceeded = true;
            return Err(io::Error::other("encoded PNG output limit exceeded"));
        }
        self.bytes.try_reserve_exact(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paint_core::{Bgra, Size};
    use proptest::prelude::*;
    use tempfile::TempDir;

    fn surface(size: Size, pixels: &[[u8; 4]]) -> Surface {
        let pixels = pixels
            .iter()
            .map(|pixel| Bgra::from_bgra_bytes(*pixel))
            .collect::<Vec<_>>();
        Surface::from_pixels(size, pixels).expect("valid test surface")
    }

    fn test_png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().expect("test PNG header");
        writer.write_image_data(data).expect("test PNG pixels");
        drop(writer);
        bytes
    }

    fn raw_png(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>) {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::IDENTITY);
        let mut reader = decoder.read_info().expect("read test PNG");
        let mut raw = vec![0; reader.output_buffer_size().expect("test output size")];
        let info = reader.next_frame(&mut raw).expect("decode test PNG");
        raw.truncate(info.buffer_size());
        (info, raw)
    }

    #[test]
    fn rgba_decode_has_exact_bgra_pixels_including_hidden_rgb() {
        let encoded = test_png(
            2,
            2,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[
                1, 2, 3, 4, // RGBA
                250, 128, 7, 0, // hidden RGB under transparent alpha
                9, 8, 7, 255, 0, 0, 0, 0,
            ],
        );

        let decoded = decode_png(&encoded).expect("decode RGBA");

        assert_eq!(
            decoded,
            surface(
                Size::new(2, 2).expect("size"),
                &[[3, 2, 1, 4], [7, 128, 250, 0], [7, 8, 9, 255], [0, 0, 0, 0]],
            )
        );
    }

    #[test]
    fn encoder_emits_exact_rgba_file_order() {
        let source = surface(
            Size::new(2, 1).expect("size"),
            &[[3, 2, 1, 4], [7, 128, 250, 0]],
        );

        let encoded = encode_png(&source).expect("encode PNG");
        let (info, raw) = raw_png(&encoded);

        assert_eq!(info.width, 2);
        assert_eq!(info.height, 1);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert_eq!(raw, [1, 2, 3, 4, 250, 128, 7, 0]);
    }

    #[test]
    fn rgb_grayscale_and_grayscale_alpha_decode_losslessly_to_bgra() {
        let cases = [
            (
                test_png(
                    1,
                    1,
                    png::ColorType::Rgb,
                    png::BitDepth::Eight,
                    &[10, 20, 30],
                ),
                [30, 20, 10, 255],
            ),
            (
                test_png(1, 1, png::ColorType::Grayscale, png::BitDepth::Eight, &[42]),
                [42, 42, 42, 255],
            ),
            (
                test_png(
                    1,
                    1,
                    png::ColorType::GrayscaleAlpha,
                    png::BitDepth::Eight,
                    &[42, 7],
                ),
                [42, 42, 42, 7],
            ),
        ];
        for (encoded, expected) in cases {
            let decoded = decode_png(&encoded).expect("decode color type");
            assert_eq!(decoded.pixels(), &[Bgra::from_bgra_bytes(expected)]);
        }
    }

    #[test]
    fn low_bit_grayscale_and_palette_transparency_expand_exactly() {
        let grayscale = test_png(
            2,
            1,
            png::ColorType::Grayscale,
            png::BitDepth::One,
            &[0b0100_0000],
        );
        let decoded = decode_png(&grayscale).expect("decode one-bit grayscale");
        assert_eq!(
            decoded.pixels(),
            &[Bgra::new(0, 0, 0, 255), Bgra::new(255, 255, 255, 255)]
        );

        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(vec![1, 2, 3, 250, 128, 7]);
        encoder.set_trns(vec![255, 0]);
        let mut writer = encoder.write_header().expect("palette header");
        writer.write_image_data(&[0, 1]).expect("palette pixels");
        writer.finish().expect("palette finish");
        let decoded = decode_png(&encoded).expect("decode palette");
        assert_eq!(
            decoded.pixels(),
            &[Bgra::new(3, 2, 1, 255), Bgra::new(7, 128, 250, 0)]
        );
    }

    #[test]
    fn stock_paint_net_one_bit_palette_reference_is_opaque_black_bgra() {
        let encoded =
            include_bytes!("../../../artifacts/reference-local/paintnet-5.1.12-invert.png");
        let decoder = png::Decoder::new(Cursor::new(encoded.as_slice()));
        let reader = decoder.read_info().expect("reference PNG header");
        assert_eq!(reader.info().size(), (800, 600));
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        assert_eq!(reader.info().bit_depth, png::BitDepth::One);
        drop(reader);

        let decoded = decode_png(encoded).expect("decode stock Paint.NET PNG");

        assert_eq!(decoded.size(), Size::new(800, 600).expect("reference size"));
        assert_eq!(decoded.pixels().len(), 480_000);
        assert!(decoded.pixels().iter().all(|pixel| *pixel == Bgra::BLACK));
    }

    #[test]
    fn malformed_and_every_truncated_prefix_are_typed_errors() {
        let encoded = test_png(
            1,
            1,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[1, 2, 3, 4],
        );
        for end in 0..encoded.len() {
            assert!(decode_png(&encoded[..end]).is_err(), "prefix {end} parsed");
        }
        assert!(decode_png(b"not a PNG").is_err());
        let mut bad_crc = encoded.clone();
        let index = bad_crc.len() - 5;
        bad_crc[index] ^= 1;
        assert!(decode_png(&bad_crc).is_err());
    }

    #[test]
    fn input_dimension_pixel_and_decoder_limits_are_enforced_before_output_allocation() {
        let encoded = test_png(2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[0; 16]);
        let cases = [
            (
                PngLimits {
                    max_input_bytes: encoded.len() - 1,
                    ..PngLimits::default()
                },
                "input bytes",
            ),
            (
                PngLimits {
                    max_width: 1,
                    ..PngLimits::default()
                },
                "width",
            ),
            (
                PngLimits {
                    max_height: 1,
                    ..PngLimits::default()
                },
                "height",
            ),
            (
                PngLimits {
                    max_pixels: 3,
                    ..PngLimits::default()
                },
                "pixels",
            ),
            (
                PngLimits {
                    max_decoded_bytes: 15,
                    ..PngLimits::default()
                },
                "decoded bytes",
            ),
            (
                PngLimits {
                    max_decoder_bytes: 0,
                    ..PngLimits::default()
                },
                "decoder bytes",
            ),
        ];
        for (limits, resource) in cases {
            assert!(matches!(
                decode_png_with_limits(&encoded, &limits),
                Err(PngError::ResourceLimit {
                    resource: actual,
                    ..
                }) if actual == resource
            ));
        }
    }

    #[test]
    fn sixteen_bit_and_apng_inputs_are_explicitly_unsupported() {
        let sixteen = test_png(
            1,
            1,
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            &[0, 1, 0, 2, 0, 3, 0, 4],
        );
        assert!(matches!(
            decode_png(&sixteen),
            Err(PngError::Unsupported("16-bit PNG channels"))
        ));

        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(1, 0).expect("APNG configuration");
        let mut writer = encoder.write_header().expect("APNG header");
        writer.write_image_data(&[1, 2, 3, 4]).expect("APNG frame");
        drop(writer);
        assert!(matches!(
            decode_png(&bytes),
            Err(PngError::Unsupported("animated PNG"))
        ));
    }

    #[test]
    fn encoded_output_limit_is_enforced_by_the_destination_writer() {
        let source = surface(Size::new(1, 1).expect("size"), &[[1, 2, 3, 4]]);
        let limits = PngLimits {
            max_encoded_bytes: 16,
            ..PngLimits::default()
        };
        assert!(matches!(
            encode_png_with_limits(&source, &limits),
            Err(PngError::ResourceLimit {
                resource: "encoded bytes",
                ..
            })
        ));
    }

    #[test]
    fn atomic_png_write_replaces_and_round_trips_through_the_real_file() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");
        std::fs::write(&output, b"prior file").expect("seed output");
        let source = surface(
            Size::new(2, 1).expect("size"),
            &[[3, 2, 1, 4], [7, 128, 250, 0]],
        );

        write_png_atomic(&output, &source).expect("write PNG atomically");

        let bytes = std::fs::read(&output).expect("read output");
        assert_eq!(decode_png(&bytes).expect("decode output"), source);
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("directory")
                .count(),
            1
        );
    }

    proptest! {
        #[test]
        fn arbitrary_small_bgra_surfaces_round_trip_exactly(
            (width, height, channels) in (1u32..=8, 1u32..=8).prop_flat_map(|(width, height)| {
                let bytes = width as usize * height as usize * 4;
                (
                    Just(width),
                    Just(height),
                    prop::collection::vec(any::<u8>(), bytes..=bytes),
                )
            }),
        ) {
            let pixels = channels
                .chunks_exact(4)
                .map(|chunk| Bgra::new(chunk[0], chunk[1], chunk[2], chunk[3]))
                .collect::<Vec<_>>();
            let source = Surface::from_pixels(Size::new(width, height).expect("size"), pixels)
                .expect("source surface");
            let encoded = encode_png(&source).expect("encode property surface");
            let decoded = decode_png(&encoded).expect("decode property surface");
            prop_assert_eq!(decoded, source);
        }

        #[test]
        fn arbitrary_inputs_respect_default_limits(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
            if let Ok(surface) = decode_png(&bytes) {
                let size = surface.size();
                prop_assert!(size.checked_pixel_count().expect("pixel count") <= PngLimits::default().max_pixels);
            }
        }
    }
}
