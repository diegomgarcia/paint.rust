use paint_core::{Bgra, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{
    auto_level, brightness_contrast, gaussian_blur, grayscale, hue_saturation, sepia,
};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn document_from_pixels(width: u32, height: u32, pixels: Vec<Bgra>) -> Document {
    let surface = Surface::from_pixels(size(width, height), pixels).expect("test surface");
    let layer = Layer::new(LayerId::new(1), "effects", surface);
    Document::from_layers(size(width, height), vec![layer]).expect("test document")
}

#[test]
fn grayscale_uses_rec601_luminance_and_preserves_alpha() {
    let document = document_from_pixels(
        5,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(255, 0, 0, 0),
            Bgra::new(255, 255, 255, 17),
            Bgra::new(0, 0, 0, 255),
        ],
    );

    let patch = grayscale(&document, LayerId::new(1))
        .expect("grayscale")
        .expect("visible change");

    assert_eq!(patch.rect(), Rect::new(0, 0, 5, 1).unwrap());
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(76, 76, 76, 255),
            Bgra::new(150, 150, 150, 128),
            Bgra::new(29, 29, 29, 0),
            Bgra::new(255, 255, 255, 17),
            Bgra::new(0, 0, 0, 255),
        ]
    );
}

#[test]
fn sepia_applies_the_standard_matrix_and_preserves_alpha() {
    let document = document_from_pixels(
        4,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(255, 0, 0, 0),
            Bgra::new(255, 255, 255, 255),
        ],
    );

    let patch = sepia(&document, LayerId::new(1))
        .expect("sepia")
        .expect("visible change");

    assert_eq!(
        patch.after(),
        &[
            Bgra::new(69, 89, 100, 255),
            Bgra::new(136, 175, 196, 128),
            Bgra::new(33, 43, 48, 0),
            Bgra::new(239, 255, 255, 255),
        ]
    );
}

#[test]
fn color_effects_clip_to_the_selection_and_leave_outside_pixels_untouched() {
    let original = vec![
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 255),
        Bgra::new(255, 255, 255, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let gray = grayscale(&document, LayerId::new(1))
        .expect("grayscale")
        .expect("selected pixels");
    assert_eq!(gray.rect(), selection);
    assert_eq!(
        gray.before(),
        &[Bgra::new(0, 255, 0, 255), Bgra::new(255, 0, 0, 255)]
    );
    assert_eq!(
        gray.after(),
        &[Bgra::new(150, 150, 150, 255), Bgra::new(29, 29, 29, 255)]
    );
    document.apply_patch(&gray).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);

    let sepia_patch = sepia(&document, LayerId::new(1))
        .expect("sepia")
        .expect("selected pixels");
    assert_eq!(sepia_patch.rect(), selection);
    assert_eq!(sepia_patch.before(), &pixels[1..3]);
    assert_eq!(
        sepia_patch.after(),
        &[Bgra::new(141, 180, 203, 255), Bgra::new(27, 35, 39, 255)]
    );
}

#[test]
fn selection_outside_the_canvas_produces_no_effect_patch() {
    let mut document = document_from_pixels(2, 1, vec![Bgra::WHITE, Bgra::BLACK]);
    document
        .set_selection(Some(Rect::new(8, 8, 9, 9).unwrap()))
        .unwrap();

    assert_eq!(grayscale(&document, LayerId::new(1)), Ok(None));
    assert_eq!(sepia(&document, LayerId::new(1)), Ok(None));
    assert_eq!(gaussian_blur(&document, LayerId::new(1), 2), Ok(None));
}

#[test]
fn gaussian_blur_radius_zero_and_uniform_opaque_input_are_noops() {
    let uniform = document_from_pixels(4, 3, vec![Bgra::new(10, 20, 30, 255); 12]);

    assert_eq!(gaussian_blur(&uniform, LayerId::new(1), 0), Ok(None));
    assert_eq!(gaussian_blur(&uniform, LayerId::new(1), 3), Ok(None));
}

#[test]
fn gaussian_blur_radius_above_the_limit_is_rejected() {
    let document = document_from_pixels(1, 1, vec![Bgra::WHITE]);
    let error = gaussian_blur(&document, LayerId::new(1), u32::MAX).expect_err("invalid radius");
    assert!(
        matches!(error, paint_raster::RasterError::BlurRadiusTooLarge { .. }),
        "unexpected error: {error:?}"
    );
}

#[test]
fn gaussian_blur_spreads_an_impulse_symmetrically_and_is_deterministic() {
    let mut pixels = vec![Bgra::TRANSPARENT; 9];
    pixels[4] = Bgra::WHITE;
    let document = document_from_pixels(3, 3, pixels);

    let patch = gaussian_blur(&document, LayerId::new(1), 1)
        .expect("blur")
        .expect("impulse changes");
    let after = patch.after();
    let center = after[4];
    assert!(
        center.a < 255,
        "transparent neighbors must soften the opaque center"
    );
    assert!(
        center.a > after[1].a,
        "the center must be stronger than its neighbors"
    );
    assert_eq!(after[1], after[3], "vertical/horizontal neighbors differ");
    assert_eq!(after[1], after[5], "vertical/horizontal neighbors differ");
    assert_eq!(after[1], after[7], "vertical/horizontal neighbors differ");
    assert_eq!(after[0], after[2], "corner neighbors differ");
    assert_eq!(after[0], after[6], "corner neighbors differ");
    assert_eq!(after[0], after[8], "corner neighbors differ");
    assert!(after[1].a > 0, "blur must spread alpha to neighbors");
    assert!(after[1].a < 255, "neighbor alpha must be partial");

    let repeated = gaussian_blur(&document, LayerId::new(1), 1)
        .expect("blur")
        .expect("impulse changes");
    assert_eq!(patch.after(), repeated.after(), "blur is not deterministic");
}

#[test]
fn gaussian_blur_premultiplies_alpha_to_avoid_dark_halos() {
    let document = document_from_pixels(2, 1, vec![Bgra::new(0, 0, 255, 255), Bgra::TRANSPARENT]);

    let patch = gaussian_blur(&document, LayerId::new(1), 1)
        .expect("blur")
        .expect("asymmetric alpha");
    let left = patch.after()[0];
    let right = patch.after()[1];
    assert!(right.a > 0, "alpha must spread to the transparent neighbor");
    assert!(left.a < 255, "the source edge must lose full opacity");
    assert_eq!(
        (right.b, right.g, right.r),
        (0, 0, 255),
        "straight-alpha unpremultiplication must not darken the halo"
    );
}

#[test]
fn gaussian_blur_clips_its_patch_to_the_selection() {
    let original = vec![
        Bgra::WHITE,
        Bgra::BLACK,
        Bgra::WHITE,
        Bgra::BLACK,
        Bgra::WHITE,
    ];
    let mut document = document_from_pixels(5, 1, original.clone());
    let selection = Rect::new(2, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let patch = gaussian_blur(&document, LayerId::new(1), 1)
        .expect("blur")
        .expect("selected center changes");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &[Bgra::WHITE]);
    assert_ne!(patch.after(), &[Bgra::WHITE]);

    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[1], original[1]);
    assert_eq!(pixels[3], original[3]);
    assert_eq!(pixels[4], original[4]);
    assert_eq!(pixels[2].a, 255, "opaque blur must preserve alpha");
}

#[test]
fn brightness_contrast_identity_extremes_and_channel_math_preserve_alpha() {
    let document = document_from_pixels(
        4,
        1,
        vec![
            Bgra::new(10, 20, 30, 255),
            Bgra::new(250, 5, 128, 17),
            Bgra::new(127, 128, 0, 128),
            Bgra::new(100, 100, 100, 0),
        ],
    );

    assert_eq!(
        brightness_contrast(&document, LayerId::new(1), 0, 0),
        Ok(None)
    );

    let brighter = brightness_contrast(&document, LayerId::new(1), 20, 0)
        .expect("brightness")
        .expect("visible change");
    assert_eq!(
        brighter.after(),
        &[
            Bgra::new(30, 40, 50, 255),
            Bgra::new(255, 25, 148, 17),
            Bgra::new(147, 148, 20, 128),
            Bgra::new(120, 120, 120, 0),
        ]
    );

    let darker = brightness_contrast(&document, LayerId::new(1), -100, 0)
        .expect("brightness")
        .expect("visible change");
    assert_eq!(
        darker.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(150, 0, 28, 17),
            Bgra::new(27, 28, 0, 128),
            Bgra::new(0, 0, 0, 0),
        ]
    );

    let flat = brightness_contrast(&document, LayerId::new(1), 0, -100)
        .expect("contrast")
        .expect("visible change");
    assert_eq!(
        flat.after(),
        &[
            Bgra::new(128, 128, 128, 255),
            Bgra::new(128, 128, 128, 17),
            Bgra::new(128, 128, 128, 128),
            Bgra::new(128, 128, 128, 0),
        ]
    );

    let hard = brightness_contrast(&document, LayerId::new(1), 0, 100)
        .expect("contrast")
        .expect("visible change");
    assert_eq!(
        hard.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(255, 0, 255, 17),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(0, 0, 0, 0),
        ]
    );
}

#[test]
fn brightness_contrast_positive_contrast_scales_around_midpoint() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(100, 100, 100, 255),
            Bgra::new(200, 200, 200, 255),
            Bgra::new(127, 127, 127, 255),
        ],
    );

    let patch = brightness_contrast(&document, LayerId::new(1), 0, 50)
        .expect("contrast")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(73, 73, 73, 255),
            Bgra::new(255, 255, 255, 255),
            Bgra::new(127, 127, 127, 255),
        ]
    );
}

#[test]
fn brightness_contrast_and_hue_saturation_clip_to_the_selection() {
    let original = vec![
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 255),
        Bgra::new(255, 0, 0, 255),
        Bgra::new(255, 255, 255, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let brightness = brightness_contrast(&document, LayerId::new(1), -100, 0)
        .expect("brightness")
        .expect("selected pixels");
    assert_eq!(brightness.rect(), selection);
    assert_eq!(brightness.before(), &original[1..3]);
    assert_eq!(
        brightness.after(),
        &[Bgra::new(0, 155, 0, 255), Bgra::new(155, 0, 0, 255)]
    );
    document.apply_patch(&brightness).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
    assert_eq!(
        hue_saturation(&document, LayerId::new(1), 0, 0, 0),
        Ok(None),
        "identity adjustment must not produce a patch"
    );

    let mut document = document_from_pixels(4, 1, original.clone());
    document.set_selection(Some(selection)).unwrap();
    let hue = hue_saturation(&document, LayerId::new(1), 120, 0, 0)
        .expect("hue")
        .expect("selected pixels");
    assert_eq!(hue.rect(), selection);
    assert_eq!(hue.before(), &original[1..3]);
    assert_eq!(
        hue.after(),
        &[Bgra::new(255, 0, 0, 255), Bgra::new(0, 0, 255, 255),]
    );
    document.apply_patch(&hue).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn hue_saturation_rotates_hue_desaturates_and_adjusts_lightness_in_hsl() {
    let red = document_from_pixels(1, 1, vec![Bgra::new(0, 0, 255, 128)]);
    assert_eq!(hue_saturation(&red, LayerId::new(1), 0, 0, 0), Ok(None));

    let green = hue_saturation(&red, LayerId::new(1), 120, 0, 0)
        .expect("hue")
        .expect("visible change");
    assert_eq!(green.after(), &[Bgra::new(0, 255, 0, 128)]);

    let cyan = hue_saturation(&red, LayerId::new(1), 180, 0, 0)
        .expect("hue")
        .expect("visible change");
    assert_eq!(cyan.after(), &[Bgra::new(255, 255, 0, 128)]);

    let gray = hue_saturation(&red, LayerId::new(1), 0, -100, 0)
        .expect("saturation")
        .expect("visible change");
    assert_eq!(gray.after(), &[Bgra::new(128, 128, 128, 128)]);

    let black = hue_saturation(&red, LayerId::new(1), 0, 0, -100)
        .expect("lightness")
        .expect("visible change");
    assert_eq!(black.after(), &[Bgra::new(0, 0, 0, 128)]);

    let white = hue_saturation(&red, LayerId::new(1), 0, 0, 100)
        .expect("lightness")
        .expect("visible change");
    assert_eq!(white.after(), &[Bgra::new(255, 255, 255, 128)]);

    let saturated = document_from_pixels(1, 1, vec![Bgra::new(63, 63, 192, 255)]);
    let patch = hue_saturation(&saturated, LayerId::new(1), 0, 100, 0)
        .expect("saturation")
        .expect("visible change");
    assert_eq!(patch.after(), &[Bgra::new(0, 0, 255, 255)]);
}

#[test]
fn auto_level_stretches_each_selected_channel_and_preserves_alpha() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(10, 20, 30, 255),
            Bgra::new(110, 120, 130, 128),
            Bgra::new(210, 220, 230, 17),
        ],
    );

    let patch = auto_level(&document, LayerId::new(1))
        .expect("auto-level")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(128, 128, 128, 128),
            Bgra::new(255, 255, 255, 17),
        ]
    );
}

#[test]
fn auto_level_is_a_noop_when_every_selected_channel_is_flat() {
    let uniform = document_from_pixels(2, 2, vec![Bgra::new(40, 50, 60, 255); 4]);
    assert_eq!(auto_level(&uniform, LayerId::new(1)), Ok(None));

    let transparent = document_from_pixels(
        2,
        1,
        vec![Bgra::new(10, 20, 30, 0), Bgra::new(200, 210, 220, 0)],
    );
    assert_eq!(auto_level(&transparent, LayerId::new(1)), Ok(None));
}

#[test]
fn auto_level_only_stretches_channels_that_have_a_range() {
    let document = document_from_pixels(
        2,
        1,
        vec![Bgra::new(50, 10, 200, 255), Bgra::new(50, 210, 200, 255)],
    );

    let patch = auto_level(&document, LayerId::new(1))
        .expect("auto-level")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[Bgra::new(50, 0, 200, 255), Bgra::new(50, 255, 200, 255),]
    );
}

#[test]
fn auto_level_uses_only_selected_opaque_pixels_and_clips_its_patch() {
    let original = vec![
        Bgra::new(0, 0, 0, 255),
        Bgra::new(10, 20, 30, 255),
        Bgra::new(110, 120, 130, 255),
        Bgra::new(255, 255, 255, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let patch = auto_level(&document, LayerId::new(1))
        .expect("auto-level")
        .expect("selected pixels");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    assert_eq!(
        patch.after(),
        &[Bgra::new(0, 0, 0, 255), Bgra::new(255, 255, 255, 255)]
    );
    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn auto_level_ignores_fully_transparent_pixels() {
    let original = vec![
        Bgra::new(10, 20, 30, 255),
        Bgra::new(110, 120, 130, 255),
        Bgra::new(255, 0, 128, 0),
    ];
    let document = document_from_pixels(3, 1, original.clone());

    let patch = auto_level(&document, LayerId::new(1))
        .expect("auto-level")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(255, 255, 255, 255),
            Bgra::new(255, 0, 128, 0),
        ]
    );
    assert_eq!(patch.before(), &original);
}
