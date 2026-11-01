use std::path::Path;

use paint_core::{Bgra, Point, Size, Surface};
use paint_image_io::{
    DecodeError, PngError, RasterFormat, RasterLimits, decode_image, decode_image_with_limits,
    encode_image, open_image, open_image_with_limits,
};

fn alpha_surface() -> Surface {
    let size = Size::new(4, 3).expect("size");
    let colors = [
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 128),
        Bgra::new(255, 255, 255, 0),
        Bgra::new(10, 20, 30, 255),
        Bgra::new(200, 100, 50, 255),
    ];
    let pixels = (0..12).map(|index| colors[index % colors.len()]).collect();
    Surface::from_pixels(size, pixels).expect("surface")
}

fn quadrant_surface() -> Surface {
    let size = Size::new(8, 8).expect("size");
    let quadrants = [
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 255),
        Bgra::new(240, 220, 200, 255),
    ];
    let mut pixels = Vec::with_capacity(64);
    for y in 0..8 {
        for x in 0..8 {
            let quadrant = usize::from(x >= 4) + usize::from(y >= 4) * 2;
            pixels.push(quadrants[quadrant]);
        }
    }
    Surface::from_pixels(size, pixels).expect("surface")
}

fn write_image(directory: &Path, format: RasterFormat, bytes: &[u8]) -> std::path::PathBuf {
    let path = directory.join(format!("image.{}", format.extension()));
    std::fs::write(&path, bytes).expect("write encoded image");
    path
}

fn is_limit_error(error: &DecodeError) -> bool {
    matches!(error, DecodeError::ResourceLimit { .. })
        || matches!(error, DecodeError::Png(PngError::ResourceLimit { .. }))
}

#[test]
fn exact_formats_round_trip_every_pixel_and_alpha_through_disk() {
    let source = alpha_surface();
    let directory = tempfile::tempdir().expect("temporary directory");
    for format in [
        RasterFormat::Png,
        RasterFormat::Tiff,
        RasterFormat::Bmp,
        RasterFormat::Tga,
        RasterFormat::Dds,
        RasterFormat::WebP,
    ] {
        let bytes = encode_image(&source, format).expect("encode");
        let path = write_image(directory.path(), format, &bytes);
        let decoded = open_image(&path).expect("open image");
        assert_eq!(decoded.size(), source.size(), "{} size", format.name());
        assert_eq!(decoded, source, "{} pixels", format.name());
        assert_eq!(
            decoded,
            decode_image(&bytes, format).expect("decode bytes"),
            "{} byte decoding",
            format.name()
        );
    }
}

#[test]
fn jpeg_and_gif_round_trip_dimensions_and_color_within_lossy_tolerance() {
    let source = quadrant_surface();
    let directory = tempfile::tempdir().expect("temporary directory");
    for (format, tolerance) in [(RasterFormat::Jpeg, 30), (RasterFormat::Gif, 4)] {
        let bytes = encode_image(&source, format).expect("encode");
        let path = write_image(directory.path(), format, &bytes);
        let decoded = open_image(&path).expect("open lossy image");
        assert_eq!(decoded.size(), source.size(), "{} size", format.name());
        assert_eq!(
            decoded.size(),
            Size::new(8, 8).expect("size"),
            "{} dimensions",
            format.name()
        );
        for y in 0..8 {
            for x in 0..8 {
                if (x == 4 || x == 5) || (y == 4 || y == 5) {
                    // JPEG chroma subsampling is allowed to blur the quadrant seam.
                    continue;
                }
                let actual = decoded.pixel(Point::new(x, y)).expect("decoded pixel");
                let expected = source.pixel(Point::new(x, y)).expect("source pixel");
                assert_eq!(actual.a, 255, "{} alpha at ({x}, {y})", format.name());
                for (channel, (actual, expected)) in [
                    (actual.r, expected.r),
                    (actual.g, expected.g),
                    (actual.b, expected.b),
                ]
                .into_iter()
                .enumerate()
                {
                    let difference = actual.abs_diff(expected);
                    assert!(
                        difference <= tolerance,
                        "{} channel {channel} at ({x}, {y}): {actual} vs {expected}",
                        format.name()
                    );
                }
            }
        }
    }
}

#[test]
fn jpeg_drops_alpha_and_gif_keeps_only_binary_transparency() {
    let source = alpha_surface();
    let jpeg = encode_image(&source, RasterFormat::Jpeg).expect("encode JPEG");
    let decoded = decode_image(&jpeg, RasterFormat::Jpeg).expect("decode JPEG");
    assert!(
        decoded.pixels().iter().all(|pixel| pixel.a == 255),
        "JPEG must flatten absent alpha to opaque"
    );

    let gif = encode_image(&source, RasterFormat::Gif).expect("encode GIF");
    let decoded = decode_image(&gif, RasterFormat::Gif).expect("decode GIF");
    assert!(
        decoded
            .pixels()
            .iter()
            .all(|pixel| pixel.a == 0 || pixel.a == 255),
        "GIF alpha must be binary"
    );
    assert!(
        decoded.pixels().iter().any(|pixel| pixel.a == 0),
        "GIF must retain fully transparent pixels"
    );
}

#[test]
fn oversize_dimensions_pixels_and_decoded_bytes_are_typed_limits() {
    let source = Surface::filled(Size::new(4, 3).expect("size"), Bgra::WHITE).expect("surface");
    for format in RasterFormat::ALL {
        let bytes = encode_image(&source, format).expect("encode");
        for limits in [
            RasterLimits {
                max_width: 3,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_height: 2,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_pixels: 11,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_decoded_bytes: 47,
                ..RasterLimits::default()
            },
        ] {
            let error = decode_image_with_limits(&bytes, format, &limits)
                .expect_err("oversize image must fail");
            assert!(
                is_limit_error(&error),
                "{} produced {error:?}",
                format.name()
            );
        }
    }
}

#[test]
fn truncated_bytes_and_empty_input_are_typed_errors() {
    let source = Surface::filled(Size::new(4, 3).expect("size"), Bgra::WHITE).expect("surface");
    for format in RasterFormat::ALL {
        let bytes = encode_image(&source, format).expect("encode");
        let truncated = &bytes[..bytes.len() / 2];
        assert!(
            decode_image_with_limits(truncated, format, &RasterLimits::default()).is_err(),
            "{} truncated bytes decoded",
            format.name()
        );
        assert!(
            decode_image_with_limits(&[], format, &RasterLimits::default()).is_err(),
            "{} empty bytes decoded",
            format.name()
        );
    }
}

#[test]
fn unknown_extensions_and_non_file_paths_are_typed_errors() {
    let directory = tempfile::tempdir().expect("temporary directory");
    assert!(matches!(
        open_image(directory.path().join("image.txt")),
        Err(DecodeError::UnknownFormat { .. })
    ));
    assert!(matches!(
        open_image(directory.path().join("image")),
        Err(DecodeError::UnknownFormat { .. })
    ));
    assert!(matches!(
        open_image(directory.path().join("missing.png")),
        Err(DecodeError::Io { .. })
    ));
    let directory_named_like_an_image = directory.path().join("folder.png");
    std::fs::create_dir(&directory_named_like_an_image).expect("directory");
    assert!(matches!(
        open_image_with_limits(&directory_named_like_an_image, &RasterLimits::default()),
        Err(DecodeError::Io { .. })
    ));
}

#[test]
fn format_metadata_covers_extensions_names_and_labels() {
    for format in RasterFormat::ALL {
        assert!(
            format.label().contains(format.name()),
            "{} label",
            format.name()
        );
        assert!(
            format.extensions().contains(&format.extension()),
            "{} extension list",
            format.name()
        );
        for extension in format.extensions() {
            assert_eq!(
                RasterFormat::from_extension(&extension.to_ascii_uppercase()),
                Some(format),
                "{} extension {extension}",
                format.name()
            );
            assert_eq!(
                RasterFormat::from_extension(extension),
                Some(format),
                "{} extension {extension}",
                format.name()
            );
        }
    }
    assert_eq!(RasterFormat::from_extension("txt"), None);
}
