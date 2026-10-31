use paint_core::{Bgra, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{
    LevelsChannel, LevelsSettings, MAX_MOTION_BLUR_DISTANCE, MAX_NOISE_INTENSITY,
    MAX_SHARPEN_AMOUNT, add_noise, auto_level, black_and_white, brightness_contrast, gaussian_blur,
    grayscale, hue_saturation, levels, motion_blur, posterize, sepia, sharpen, threshold,
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

#[test]
fn motion_blur_spreads_an_impulse_along_the_configured_direction() {
    let mut pixels = vec![Bgra::TRANSPARENT; 49];
    pixels[3 * 7 + 3] = Bgra::WHITE;
    let document = document_from_pixels(7, 7, pixels);

    let horizontal = motion_blur(&document, LayerId::new(1), 0, 2)
        .expect("horizontal motion blur")
        .expect("impulse changes");
    let after = horizontal.after();
    for offset in -2i64..=2 {
        assert!(
            after[3 * 7 + (3 + offset) as usize].a > 0,
            "horizontal blur must spread to column offset {offset}"
        );
    }
    assert_eq!(
        after[2 * 7 + 3].a,
        0,
        "horizontal blur must not spread vertically"
    );
    assert_eq!(
        after[4 * 7 + 3].a,
        0,
        "horizontal blur must not spread vertically"
    );
    assert_eq!(after[3 * 7 + 1], after[3 * 7 + 5]);
    assert_eq!(after[3 * 7 + 2], after[3 * 7 + 4]);

    let vertical = motion_blur(&document, LayerId::new(1), 90, 2)
        .expect("vertical motion blur")
        .expect("impulse changes");
    let after = vertical.after();
    for offset in -2i64..=2 {
        assert!(
            after[(3 + offset) as usize * 7 + 3].a > 0,
            "vertical blur must spread to row offset {offset}"
        );
    }
    assert_eq!(
        after[3 * 7 + 2].a,
        0,
        "vertical blur must not spread horizontally"
    );
    assert_eq!(
        after[3 * 7 + 4].a,
        0,
        "vertical blur must not spread horizontally"
    );
    assert_eq!(after[7 + 3], after[5 * 7 + 3]);
    assert_eq!(after[2 * 7 + 3], after[4 * 7 + 3]);
}

#[test]
fn motion_blur_premultiplies_alpha_and_is_deterministic() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::TRANSPARENT,
            Bgra::TRANSPARENT,
        ],
    );

    let patch = motion_blur(&document, LayerId::new(1), 0, 1)
        .expect("motion blur")
        .expect("asymmetric alpha");
    let after = patch.after();
    assert_eq!(after[2], Bgra::TRANSPARENT);
    assert!(after[0].a > 0, "alpha must spread to the source edge");
    assert!(after[0].a < 255, "the source edge must lose full opacity");
    assert!(
        after[1].a > 0,
        "alpha must spread to the transparent neighbor"
    );
    for pixel in after.iter().filter(|pixel| pixel.a > 0) {
        assert_eq!(
            (pixel.b, pixel.g, pixel.r),
            (0, 0, 255),
            "straight-alpha unpremultiplication must not darken the halo"
        );
    }

    let repeated = motion_blur(&document, LayerId::new(1), 0, 1)
        .expect("motion blur")
        .expect("asymmetric alpha");
    assert_eq!(
        patch.after(),
        repeated.after(),
        "motion blur is not deterministic"
    );
}

#[test]
fn motion_blur_clips_its_patch_to_the_selection() {
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

    let patch = motion_blur(&document, LayerId::new(1), 0, 1)
        .expect("motion blur")
        .expect("selected center changes");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &[Bgra::WHITE]);
    assert_eq!(patch.after(), &[Bgra::new(85, 85, 85, 255)]);

    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[1], original[1]);
    assert_eq!(pixels[3], original[3]);
    assert_eq!(pixels[4], original[4]);
}

#[test]
fn motion_blur_zero_distance_and_uniform_opaque_input_are_noops() {
    let uniform = document_from_pixels(4, 3, vec![Bgra::new(10, 20, 30, 255); 12]);
    assert_eq!(motion_blur(&uniform, LayerId::new(1), 0, 0), Ok(None));
    assert_eq!(motion_blur(&uniform, LayerId::new(1), 45, 3), Ok(None));
}

#[test]
fn motion_blur_distance_above_the_limit_is_rejected() {
    let document = document_from_pixels(1, 1, vec![Bgra::WHITE]);
    let error = motion_blur(&document, LayerId::new(1), 0, MAX_MOTION_BLUR_DISTANCE + 1)
        .expect_err("invalid distance");
    assert!(
        matches!(
            error,
            paint_raster::RasterError::MotionBlurDistanceTooLarge { .. }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn sharpen_increases_local_contrast_by_amount_and_preserves_alpha() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(100, 100, 100, 255),
            Bgra::new(200, 200, 200, 255),
            Bgra::new(100, 100, 100, 255),
        ],
    );

    let patch = sharpen(&document, LayerId::new(1), 10)
        .expect("sharpen")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(75, 75, 75, 255),
            Bgra::new(250, 250, 250, 255),
            Bgra::new(75, 75, 75, 255),
        ]
    );

    let alpha_document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(10, 20, 30, 0),
            Bgra::new(100, 110, 120, 17),
            Bgra::new(200, 210, 220, 128),
        ],
    );
    let alpha_patch = sharpen(&alpha_document, LayerId::new(1), 10)
        .expect("sharpen")
        .expect("visible change");
    assert_eq!(alpha_patch.before()[0], alpha_patch.after()[0]);
    for (before, after) in alpha_patch.before().iter().zip(alpha_patch.after()) {
        assert_eq!(after.a, before.a, "sharpen must preserve alpha exactly");
    }
}

#[test]
fn sharpen_zero_amount_and_uniform_opaque_input_are_noops() {
    let uniform = document_from_pixels(4, 3, vec![Bgra::new(10, 20, 30, 255); 12]);
    assert_eq!(sharpen(&uniform, LayerId::new(1), 0), Ok(None));
    assert_eq!(sharpen(&uniform, LayerId::new(1), 5), Ok(None));
}

#[test]
fn sharpen_amount_above_the_limit_is_rejected() {
    let document = document_from_pixels(1, 1, vec![Bgra::WHITE]);
    let error =
        sharpen(&document, LayerId::new(1), MAX_SHARPEN_AMOUNT + 1).expect_err("invalid amount");
    assert!(
        matches!(
            error,
            paint_raster::RasterError::SharpenAmountTooLarge { .. }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn sharpen_clips_its_patch_to_the_selection() {
    let original = vec![
        Bgra::new(100, 100, 100, 255),
        Bgra::new(200, 200, 200, 255),
        Bgra::new(100, 100, 100, 255),
    ];
    let mut document = document_from_pixels(3, 1, original.clone());
    let selection = Rect::new(1, 0, 2, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let patch = sharpen(&document, LayerId::new(1), 10)
        .expect("sharpen")
        .expect("selected center changes");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &[Bgra::new(200, 200, 200, 255)]);
    assert_eq!(patch.after(), &[Bgra::new(250, 250, 250, 255)]);

    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[2], original[2]);
}

#[test]
fn add_noise_is_deterministic_for_a_seed_and_varies_with_the_seed() {
    let document = document_from_pixels(8, 8, vec![Bgra::new(128, 128, 128, 255); 64]);

    let first = add_noise(&document, LayerId::new(1), 50, 42)
        .expect("noise")
        .expect("visible change");
    let repeated = add_noise(&document, LayerId::new(1), 50, 42)
        .expect("noise")
        .expect("visible change");
    let other_seed = add_noise(&document, LayerId::new(1), 50, 43)
        .expect("noise")
        .expect("visible change");

    assert_eq!(
        first.after(),
        repeated.after(),
        "noise is not deterministic"
    );
    assert_ne!(
        first.after(),
        other_seed.after(),
        "a different seed must produce different noise"
    );
    let amplitude = 50 * 255 / 100;
    for (source, after) in first.before().iter().zip(first.after()) {
        assert_eq!(after.a, source.a, "noise must preserve alpha exactly");
        for (before, after) in [
            (source.b, after.b),
            (source.g, after.g),
            (source.r, after.r),
        ] {
            let delta = i32::from(after) - i32::from(before);
            assert!(
                delta.abs() <= amplitude,
                "noise delta {delta} exceeds the intensity amplitude {amplitude}"
            );
        }
    }
}

#[test]
fn add_noise_preserves_alpha_and_clips_its_patch_to_the_selection() {
    let original = vec![
        Bgra::new(10, 20, 30, 0),
        Bgra::new(100, 110, 120, 17),
        Bgra::new(150, 160, 170, 128),
        Bgra::new(200, 210, 220, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let patch = add_noise(&document, LayerId::new(1), 100, 7)
        .expect("noise")
        .expect("selected pixels change");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    for (before, after) in patch.before().iter().zip(patch.after()) {
        assert_eq!(after.a, before.a);
    }

    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn add_noise_zero_intensity_is_a_noop_and_above_the_limit_is_rejected() {
    let document = document_from_pixels(2, 1, vec![Bgra::new(10, 20, 30, 255); 2]);
    assert_eq!(add_noise(&document, LayerId::new(1), 0, 1), Ok(None));

    let error = add_noise(&document, LayerId::new(1), MAX_NOISE_INTENSITY + 1, 1)
        .expect_err("invalid intensity");
    assert!(
        matches!(
            error,
            paint_raster::RasterError::NoiseIntensityTooLarge { .. }
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn levels_identity_settings_are_an_atomic_noop() {
    let document = document_from_pixels(2, 1, vec![Bgra::new(10, 20, 30, 40); 2]);
    assert_eq!(
        levels(&document, LayerId::new(1), LevelsSettings::IDENTITY),
        Ok(None)
    );
}

#[test]
fn levels_clips_input_points_stretches_the_midtones_and_remaps_output_points() {
    let document = document_from_pixels(
        6,
        1,
        [0u8, 63, 64, 128, 192, 255]
            .into_iter()
            .map(|value| Bgra::new(value, value, value, 255))
            .collect(),
    );
    let clipped = LevelsSettings::new(
        LevelsChannel::new(64, 192, 100, 0, 255),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), clipped)
        .expect("levels")
        .expect("visible change");
    let values: Vec<u8> = patch.after().iter().map(|pixel| pixel.r).collect();
    assert_eq!(values, [0, 0, 0, 128, 255, 255]);
    assert!(patch.after().iter().all(|pixel| pixel.a == 255));

    let gamma = LevelsSettings::new(
        LevelsChannel::new(0, 255, 200, 0, 255),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), gamma)
        .expect("gamma levels")
        .expect("visible change");
    assert_eq!(patch.after()[0].r, 0);
    assert_eq!(patch.after()[3].r, 64, "0.5^2 of the range rounds to 64");
    assert_eq!(patch.after()[5].r, 255);

    let output = LevelsSettings::new(
        LevelsChannel::new(0, 255, 100, 32, 224),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), output)
        .expect("output levels")
        .expect("visible change");
    let values: Vec<u8> = patch.after().iter().map(|pixel| pixel.r).collect();
    assert_eq!(values, [32, 79, 80, 128, 177, 224]);
}

#[test]
fn levels_channel_normalizes_inverted_points_and_clamps_gamma() {
    assert_eq!(LevelsChannel::new(200, 100, 0, 0, 255).input_black, 100);
    assert_eq!(LevelsChannel::new(10, 20, 0, 240, 100).output_black, 100);
    assert_eq!(LevelsChannel::new(0, 255, 0, 0, 255).gamma_hundredths, 10);
    assert_eq!(
        LevelsChannel::new(0, 255, u16::MAX, 0, 255).gamma_hundredths,
        1000
    );

    let document = document_from_pixels(
        2,
        1,
        vec![Bgra::new(99, 99, 99, 255), Bgra::new(100, 100, 100, 255)],
    );
    let settings = LevelsSettings::new(
        LevelsChannel::new(200, 100, 100, 0, 255),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), settings)
        .expect("degenerate levels")
        .expect("visible change");
    let values: Vec<u8> = patch.after().iter().map(|pixel| pixel.r).collect();
    assert_eq!(values, [0, 255]);
}

#[test]
fn levels_applies_the_composite_channel_before_the_per_channel_pass() {
    let document = document_from_pixels(1, 1, vec![Bgra::new(0, 0, 0, 77)]);
    let settings = LevelsSettings::new(
        LevelsChannel::new(0, 255, 100, 100, 255),
        LevelsChannel::new(100, 255, 100, 0, 255),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), settings)
        .expect("levels")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[Bgra::new(100, 100, 0, 77)],
        "the red channel must refine the composite output, not the source"
    );
}

#[test]
fn levels_clips_to_the_selection_and_preserves_alpha() {
    let original = vec![
        Bgra::new(10, 20, 30, 0),
        Bgra::new(40, 50, 60, 17),
        Bgra::new(70, 80, 90, 128),
        Bgra::new(100, 110, 120, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();

    let settings = LevelsSettings::new(
        LevelsChannel::new(0, 255, 100, 50, 255),
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
        LevelsChannel::IDENTITY,
    );
    let patch = levels(&document, LayerId::new(1), settings)
        .expect("levels")
        .expect("selected pixels change");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    assert_eq!(
        patch.after(),
        &[Bgra::new(82, 90, 98, 17), Bgra::new(106, 114, 122, 128)]
    );

    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn posterize_quantizes_each_channel_to_the_requested_level_count() {
    let values = [0u8, 64, 127, 128, 191, 192, 255];
    let document = document_from_pixels(
        values.len() as u32,
        1,
        values
            .iter()
            .map(|value| Bgra::new(*value, *value, *value, 200))
            .collect(),
    );
    let patch = posterize(&document, LayerId::new(1), 2)
        .expect("posterize")
        .expect("visible change");
    let channels: Vec<[u8; 3]> = patch
        .after()
        .iter()
        .map(|pixel| [pixel.r, pixel.g, pixel.b])
        .collect();
    assert_eq!(
        channels,
        [
            [0, 0, 0],
            [0, 0, 0],
            [0, 0, 0],
            [255, 255, 255],
            [255, 255, 255],
            [255, 255, 255],
            [255, 255, 255],
        ]
    );

    let patch = posterize(&document, LayerId::new(1), 3)
        .expect("three-level posterize")
        .expect("visible change");
    let red: Vec<u8> = patch.after().iter().map(|pixel| pixel.r).collect();
    assert_eq!(red, [0, 128, 128, 128, 128, 255, 255]);
    assert!(patch.after().iter().all(|pixel| pixel.a == 200));
}

#[test]
fn posterize_clamps_levels_and_returns_none_for_an_already_quantized_selection() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(0, 0, 0, 255),
            Bgra::new(200, 200, 200, 255),
            Bgra::new(255, 255, 255, 255),
        ],
    );
    let below = posterize(&document, LayerId::new(1), 1)
        .expect("clamped low")
        .expect("visible change");
    assert_eq!(
        below.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(255, 255, 255, 255),
            Bgra::new(255, 255, 255, 255),
        ]
    );

    let already = document_from_pixels(
        2,
        1,
        vec![Bgra::new(0, 255, 0, 255), Bgra::new(255, 0, 255, 255)],
    );
    assert_eq!(posterize(&already, LayerId::new(1), 2), Ok(None));
    assert_eq!(posterize(&already, LayerId::new(1), 255), Ok(None));

    let ramp = document_from_pixels(
        2,
        1,
        vec![Bgra::new(10, 10, 10, 255), Bgra::new(200, 200, 200, 255)],
    );
    let at_64 = posterize(&ramp, LayerId::new(1), 64)
        .expect("64 levels")
        .expect("visible change");
    let at_above = posterize(&ramp, LayerId::new(1), 255)
        .expect("clamped high")
        .expect("visible change");
    assert_eq!(
        at_64.after(),
        at_above.after(),
        "levels above 64 must clamp to 64"
    );
}

#[test]
fn posterize_clips_to_the_selection_and_keeps_the_patch_rect() {
    let original = vec![
        Bgra::new(10, 20, 30, 255),
        Bgra::new(40, 50, 60, 255),
        Bgra::new(70, 80, 90, 255),
        Bgra::new(100, 110, 120, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();
    let patch = posterize(&document, LayerId::new(1), 2)
        .expect("posterize")
        .expect("selected pixels change");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    assert_eq!(
        patch.after(),
        &[Bgra::new(0, 0, 0, 255), Bgra::new(0, 0, 0, 255)]
    );
    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn black_and_white_uses_the_configured_channel_weight_average() {
    let document = document_from_pixels(
        3,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(255, 0, 0, 0),
        ],
    );
    let patch = black_and_white(&document, LayerId::new(1), 30, 59, 11)
        .expect("black and white")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(77, 77, 77, 255),
            Bgra::new(150, 150, 150, 128),
            Bgra::new(28, 28, 28, 0),
        ]
    );

    let red_only = black_and_white(&document, LayerId::new(1), 100, 0, 0)
        .expect("red-only weights")
        .expect("visible change");
    assert_eq!(
        red_only.after(),
        &[
            Bgra::new(255, 255, 255, 255),
            Bgra::new(0, 0, 0, 128),
            Bgra::new(0, 0, 0, 0),
        ]
    );
}

#[test]
fn black_and_white_zero_weights_are_a_noop_and_neutral_pixels_are_unchanged() {
    let color = document_from_pixels(1, 1, vec![Bgra::new(10, 200, 30, 255)]);
    assert_eq!(black_and_white(&color, LayerId::new(1), 0, 0, 0), Ok(None));
    let neutral = document_from_pixels(2, 1, vec![Bgra::new(40, 40, 40, 255); 2]);
    assert_eq!(
        black_and_white(&neutral, LayerId::new(1), 30, 59, 11),
        Ok(None)
    );
}

#[test]
fn black_and_white_clips_to_the_selection_and_preserves_alpha() {
    let original = vec![
        Bgra::new(0, 0, 255, 0),
        Bgra::new(0, 255, 0, 17),
        Bgra::new(255, 0, 0, 128),
        Bgra::new(255, 255, 255, 255),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();
    let patch = black_and_white(&document, LayerId::new(1), 30, 59, 11)
        .expect("black and white")
        .expect("selected pixels change");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    assert_eq!(
        patch.after(),
        &[Bgra::new(150, 150, 150, 17), Bgra::new(28, 28, 28, 128),]
    );
    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}

#[test]
fn threshold_compares_rounded_rec601_luminance_and_preserves_alpha() {
    let document = document_from_pixels(
        4,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(255, 0, 0, 0),
            Bgra::new(128, 128, 128, 255),
        ],
    );
    let patch = threshold(&document, LayerId::new(1), 128)
        .expect("threshold")
        .expect("visible change");
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(0, 0, 0, 255),
            Bgra::new(255, 255, 255, 128),
            Bgra::new(0, 0, 0, 0),
            Bgra::new(255, 255, 255, 255),
        ]
    );

    let at_luminance = threshold(&document, LayerId::new(1), 76)
        .expect("threshold at the luminance")
        .expect("visible change");
    assert_eq!(at_luminance.after()[0], Bgra::new(255, 255, 255, 255));
}

#[test]
fn threshold_noops_on_an_already_black_and_white_selection() {
    let document = document_from_pixels(
        2,
        1,
        vec![Bgra::new(0, 0, 0, 255), Bgra::new(255, 255, 255, 255)],
    );
    assert_eq!(threshold(&document, LayerId::new(1), 128), Ok(None));
}

#[test]
fn threshold_clips_to_the_selection_and_keeps_outside_pixels() {
    let original = vec![
        Bgra::new(0, 0, 0, 255),
        Bgra::new(255, 255, 255, 255),
        Bgra::new(0, 0, 255, 255),
        Bgra::new(0, 255, 0, 128),
    ];
    let mut document = document_from_pixels(4, 1, original.clone());
    let selection = Rect::new(1, 0, 3, 1).unwrap();
    document.set_selection(Some(selection)).unwrap();
    let patch = threshold(&document, LayerId::new(1), 100)
        .expect("threshold")
        .expect("selected pixels change");
    assert_eq!(patch.rect(), selection);
    assert_eq!(patch.before(), &original[1..3]);
    assert_eq!(
        patch.after(),
        &[Bgra::new(255, 255, 255, 255), Bgra::new(0, 0, 0, 255)]
    );
    document.apply_patch(&patch).unwrap();
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[0], original[0]);
    assert_eq!(pixels[3], original[3]);
}
