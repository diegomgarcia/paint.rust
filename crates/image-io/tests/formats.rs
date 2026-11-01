use paint_core::{Bgra, Size, Surface};
use paint_image_io::{SaveFormat, decode_png, encode_image};

fn surface() -> Surface {
    let size = Size::new(4, 3).expect("size");
    let colors = [
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 255),
        Bgra::new(255, 255, 255, 128),
        Bgra::new(10, 20, 30, 255),
        Bgra::new(200, 100, 50, 255),
    ];
    let pixels = (0..12).map(|index| colors[index % colors.len()]).collect();
    Surface::from_pixels(size, pixels).expect("surface")
}

fn decoded_rgba(bytes: &[u8], format: image::ImageFormat) -> image::RgbaImage {
    image::load_from_memory_with_format(bytes, format)
        .expect("decode")
        .to_rgba8()
}

#[test]
fn png_jpeg_tiff_bmp_and_tga_encode_real_images() {
    let surface = surface();
    let formats = [
        (SaveFormat::Jpeg, image::ImageFormat::Jpeg),
        (SaveFormat::Tiff, image::ImageFormat::Tiff),
        (SaveFormat::Bmp, image::ImageFormat::Bmp),
        (SaveFormat::Tga, image::ImageFormat::Tga),
        (SaveFormat::Gif, image::ImageFormat::Gif),
        (SaveFormat::WebP, image::ImageFormat::WebP),
    ];
    for (save, decode) in formats {
        let bytes = encode_image(&surface, save).expect("encode");
        let image = decoded_rgba(&bytes, decode);
        assert_eq!(image.dimensions(), (4, 3), "{} dimensions", save.label());
        let pixel = image.get_pixel(0, 0);
        assert!(
            pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40,
            "{} red pixel {pixel:?}",
            save.label()
        );
    }
    let png = encode_image(&surface, SaveFormat::Png).expect("encode PNG");
    let decoded = decode_png(&png).expect("decode PNG");
    assert_eq!(decoded.size(), Size::new(4, 3).unwrap());
    assert_eq!(
        decoded.pixel(paint_core::Point::new(0, 0)).unwrap(),
        &Bgra::new(0, 0, 255, 255)
    );
}

#[test]
fn dds_is_uncompressed_bgra_with_a_valid_header() {
    let bytes = encode_image(&surface(), SaveFormat::Dds).expect("encode");
    assert_eq!(&bytes[0..4], b"DDS ");
    assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 3);
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 4);
    assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 16);
    assert_eq!(bytes.len(), 128 + 4 * 3 * 4);
    assert_eq!(&bytes[128..132], &[0, 0, 255, 255]);
}

#[test]
fn save_format_extensions_match_the_dialog() {
    assert_eq!(
        SaveFormat::from_path(std::path::Path::new("a.JPEG")),
        Some(SaveFormat::Jpeg)
    );
    assert_eq!(
        SaveFormat::from_path(std::path::Path::new("a.tif")),
        Some(SaveFormat::Tiff)
    );
    assert_eq!(
        SaveFormat::from_path(std::path::Path::new("a.dib")),
        Some(SaveFormat::Bmp)
    );
    assert_eq!(SaveFormat::from_path(std::path::Path::new("a.txt")), None);
}
