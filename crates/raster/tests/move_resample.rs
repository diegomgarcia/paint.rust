//! Real subpixel Move resampling: integer parity, interpolation output, and honest rejection
//! of the scale-only Anisotropic mode.

use paint_core::{Bgra, Point, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{RasterError, ResampleMode, translate, translate_resampled};

const ALL_REAL_MODES: [ResampleMode; 4] = [
    ResampleMode::NearestNeighbor,
    ResampleMode::Bilinear,
    ResampleMode::MultisampleBilinear,
    ResampleMode::Bicubic,
];

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn document(width: u32, height: u32, pixels: Vec<Bgra>) -> Document {
    let surface = Surface::from_pixels(size(width, height), pixels).expect("test surface");
    Document::from_layers(
        size(width, height),
        vec![Layer::new(LayerId::new(1), "pixels", surface)],
    )
    .expect("test document")
}

fn translated(mut document: Document, dx: f64, dy: f64, mode: ResampleMode) -> Vec<Bgra> {
    let patch = translate_resampled(&document, LayerId::new(1), dx, dy, mode)
        .expect("resampled translation")
        .expect("expected a patch");
    document.apply_patch(&patch).expect("apply patch");
    document.active_layer().surface().pixels().to_vec()
}

fn pixel(pixels: &[Bgra], width: u32, x: u32, y: u32) -> Bgra {
    pixels[(y * width + x) as usize]
}

#[test]
fn integer_offsets_stay_byte_identical_to_nearest_translation() {
    let source = vec![
        Bgra::new(1, 2, 3, 255),
        Bgra::new(4, 5, 6, 255),
        Bgra::new(7, 8, 9, 255),
    ];
    for mode in ALL_REAL_MODES {
        let mut exact = document(3, 1, source.clone());
        let patch = translate(&exact, LayerId::new(1), 1, 0)
            .expect("integer translation")
            .expect("expected a patch");
        exact.apply_patch(&patch).expect("apply patch");
        let expected = exact.active_layer().surface().pixels().to_vec();
        assert_eq!(
            translated(document(3, 1, source.clone()), 1.0, 0.0, mode),
            expected,
            "{mode:?} must be exact for an integer offset"
        );
    }
}

#[test]
fn half_pixel_bilinear_blends_and_nearest_snaps() {
    let source = vec![Bgra::BLACK, Bgra::WHITE];
    let bilinear = translated(
        document(2, 1, source.clone()),
        0.5,
        0.0,
        ResampleMode::Bilinear,
    );
    assert_eq!(pixel(&bilinear, 2, 0, 0), Bgra::new(0, 0, 0, 128));
    assert_eq!(pixel(&bilinear, 2, 1, 0), Bgra::new(128, 128, 128, 255));

    let nearest = translated(
        document(2, 1, source),
        0.5,
        0.0,
        ResampleMode::NearestNeighbor,
    );
    assert_eq!(pixel(&nearest, 2, 0, 0), Bgra::TRANSPARENT);
    assert_eq!(pixel(&nearest, 2, 1, 0), Bgra::WHITE);
}

#[test]
fn bicubic_uses_a_wider_kernel_than_bilinear() {
    let source = vec![
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 255),
        Bgra::WHITE,
    ];
    let bilinear = translated(
        document(4, 1, source.clone()),
        0.5,
        0.0,
        ResampleMode::Bilinear,
    );
    let bicubic = translated(document(4, 1, source), 0.5, 0.0, ResampleMode::Bicubic);
    assert_eq!(pixel(&bilinear, 4, 1, 0), Bgra::new(0, 128, 128, 255));
    assert_eq!(
        pixel(&bicubic, 4, 1, 0),
        Bgra::new(0, 135, 135, 255),
        "bicubic must include the neighbouring taps"
    );
    assert_ne!(
        pixel(&bicubic, 4, 1, 0),
        pixel(&bilinear, 4, 1, 0),
        "Bicubic must not silently fall back to Bilinear"
    );
}

#[test]
fn multisample_bilinear_box_filters_across_a_source_boundary() {
    let mut pixels = Vec::new();
    for _y in 0..4 {
        pixels.extend([Bgra::BLACK, Bgra::WHITE, Bgra::BLACK, Bgra::WHITE]);
    }
    let bilinear = translated(
        document(4, 4, pixels.clone()),
        0.25,
        0.0,
        ResampleMode::Bilinear,
    );
    let multisample = translated(
        document(4, 4, pixels),
        0.25,
        0.0,
        ResampleMode::MultisampleBilinear,
    );
    assert_eq!(pixel(&bilinear, 4, 1, 1), Bgra::new(191, 191, 191, 255));
    assert_eq!(
        pixel(&multisample, 4, 1, 1),
        Bgra::new(159, 159, 159, 255),
        "the four-sample footprint average must differ from a single bilinear tap"
    );
}

#[test]
fn anisotropic_is_rejected_instead_of_approximated() {
    let document = document(2, 1, vec![Bgra::BLACK, Bgra::WHITE]);
    let error = translate_resampled(
        &document,
        LayerId::new(1),
        0.5,
        0.0,
        ResampleMode::Anisotropic,
    )
    .expect_err("Anisotropic must not be faked");
    assert!(matches!(
        error,
        RasterError::UnsupportedResampleMode {
            mode: ResampleMode::Anisotropic
        }
    ));
    let point = Point::new(0, 0);
    assert_eq!(
        *document
            .active_layer()
            .surface()
            .pixel(point)
            .expect("pixel"),
        Bgra::BLACK,
        "a rejected mode must not mutate the document"
    );
}
