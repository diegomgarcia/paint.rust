use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, Revision};
use paint_raster::{compose, fill, flip_horizontal, flip_vertical, invert, normal_over};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn solid_layer(id: u64, width: u32, height: u32, color: Bgra) -> Layer {
    Layer::new(
        LayerId::new(id),
        format!("layer {id}"),
        Surface::filled(size(width, height), color).expect("test surface"),
    )
}

#[test]
fn fill_produces_a_clipped_selected_patch_without_mutating_until_apply() {
    let original = vec![
        Bgra::new(1, 2, 3, 4),
        Bgra::new(5, 6, 7, 8),
        Bgra::new(9, 10, 11, 12),
        Bgra::new(13, 14, 15, 16),
        Bgra::new(17, 18, 19, 20),
        Bgra::new(21, 22, 23, 24),
    ];
    let surface = Surface::from_pixels(size(3, 2), original.clone()).expect("surface");
    let mut document = Document::from_layers(
        size(3, 2),
        vec![Layer::new(LayerId::new(7), "fill me", surface)],
    )
    .expect("document");
    document
        .set_selection(Some(Rect::new(-1, 0, 2, 1).expect("selection")))
        .expect("set selection");
    let color = Bgra::new(90, 91, 92, 93);
    let before_generation = document.clone();
    let patch = fill(&document, LayerId::new(7), color)
        .expect("fill operation")
        .expect("selected pixels");
    assert_eq!(document, before_generation);
    assert_eq!(patch.layer_id(), LayerId::new(7));
    assert_eq!(patch.expected_revision(), Revision::new(0));
    assert_eq!(patch.rect(), Rect::new(0, 0, 2, 1).expect("clipped rect"));
    assert_eq!(patch.before(), &original[..2]);
    assert_eq!(patch.after(), &[color, color]);

    document.apply_patch(&patch).expect("apply generated patch");
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(&pixels[..2], &[color, color]);
    assert_eq!(pixels[2], original[2]);
    assert_eq!(&pixels[3..], &original[3..]);
}

#[test]
fn fill_returns_explicit_none_for_empty_or_noop_operations() {
    let document = Document::from_layers(size(1, 1), vec![solid_layer(1, 1, 1, Bgra::BLACK)])
        .expect("document");
    let mut outside = document.clone();
    outside
        .set_selection(Some(Rect::new(4, 4, 5, 5).expect("outside")))
        .expect("selection");
    assert_eq!(fill(&outside, LayerId::new(1), Bgra::WHITE), Ok(None));
    assert_eq!(fill(&document, LayerId::new(1), Bgra::BLACK), Ok(None));
    assert_eq!(outside.active_layer().surface().pixels(), &[Bgra::BLACK]);
}

#[test]
fn invert_changes_only_rgb_and_round_trips_through_new_patches() {
    let original = vec![
        Bgra::new(0, 1, 2, 0),
        Bgra::new(10, 20, 30, 128),
        Bgra::new(250, 251, 252, 255),
    ];
    let surface = Surface::from_pixels(size(3, 1), original.clone()).expect("surface");
    let mut document = Document::from_layers(
        size(3, 1),
        vec![Layer::new(LayerId::new(4), "invert", surface)],
    )
    .expect("document");
    let patch = invert(&document, LayerId::new(4))
        .expect("invert operation")
        .expect("nonempty canvas");
    assert_eq!(patch.before(), original);
    assert_eq!(
        patch.after(),
        &[
            Bgra::new(255, 254, 253, 0),
            Bgra::new(245, 235, 225, 128),
            Bgra::new(5, 4, 3, 255),
        ]
    );
    document.apply_patch(&patch).expect("apply invert");

    let restore = invert(&document, LayerId::new(4))
        .expect("second invert")
        .expect("nonempty canvas");
    assert_eq!(restore.expected_revision(), Revision::new(1));
    document.apply_patch(&restore).expect("restore");
    assert_eq!(document.active_layer().surface().pixels(), original);
}

#[test]
fn flips_are_exact_full_layer_transforms_with_inverse_patches() {
    let original = vec![
        Bgra::new(1, 0, 0, 255),
        Bgra::new(0, 2, 0, 255),
        Bgra::new(0, 0, 3, 255),
        Bgra::new(4, 0, 0, 255),
        Bgra::new(0, 5, 0, 255),
        Bgra::new(0, 0, 6, 255),
    ];
    let surface = Surface::from_pixels(size(3, 2), original.clone()).expect("surface");
    let mut document = Document::from_layers(
        size(3, 2),
        vec![Layer::new(LayerId::new(9), "flip", surface)],
    )
    .expect("document");

    let horizontal = flip_horizontal(&document, LayerId::new(9))
        .expect("flip horizontal")
        .expect("asymmetric pixels");
    assert_eq!(
        horizontal.after(),
        &[
            Bgra::new(0, 0, 3, 255),
            Bgra::new(0, 2, 0, 255),
            Bgra::new(1, 0, 0, 255),
            Bgra::new(0, 0, 6, 255),
            Bgra::new(0, 5, 0, 255),
            Bgra::new(4, 0, 0, 255),
        ]
    );
    document.apply_patch(&horizontal).expect("apply flip");

    let vertical = flip_vertical(&document, LayerId::new(9))
        .expect("flip vertical")
        .expect("asymmetric pixels");
    assert_eq!(
        vertical.after(),
        &[
            Bgra::new(0, 0, 6, 255),
            Bgra::new(0, 5, 0, 255),
            Bgra::new(4, 0, 0, 255),
            Bgra::new(0, 0, 3, 255),
            Bgra::new(0, 2, 0, 255),
            Bgra::new(1, 0, 0, 255),
        ]
    );
    document
        .apply_patch(&vertical)
        .expect("apply vertical flip");

    let horizontal_restore = flip_horizontal(&document, LayerId::new(9))
        .expect("horizontal restore")
        .expect("asymmetric pixels");
    document
        .apply_patch(&horizontal_restore)
        .expect("restore horizontal");
    let vertical_restore = flip_vertical(&document, LayerId::new(9))
        .expect("vertical restore")
        .expect("asymmetric pixels");
    document
        .apply_patch(&vertical_restore)
        .expect("restore vertical");
    assert_eq!(document.active_layer().surface().pixels(), original);
}

#[test]
fn symmetric_surface_flips_are_explicit_noops() {
    let surface = Surface::filled(size(2, 2), Bgra::new(3, 4, 5, 200)).expect("surface");
    let document = Document::from_layers(
        size(2, 2),
        vec![Layer::new(LayerId::new(1), "symmetric", surface)],
    )
    .expect("document");
    assert_eq!(flip_horizontal(&document, LayerId::new(1)), Ok(None));
    assert_eq!(flip_vertical(&document, LayerId::new(1)), Ok(None));
}

#[test]
fn normal_over_matches_hand_computed_partial_alpha_example() {
    let destination = Bgra::new(200, 100, 50, 128);
    let source = Bgra::new(10, 20, 30, 128);
    // Effective source alpha = round(128 * 128 / 255) = 64.
    // Remaining destination alpha = round(128 * (255 - 64) / 255) = 96.
    // Output alpha = 160 and channels are (source * 64 + destination * 96) / 160.
    assert_eq!(
        normal_over(destination, source, 128),
        Bgra::new(124, 68, 42, 160)
    );
}

#[test]
fn normal_over_handles_transparency_and_opacity_boundaries_exactly() {
    let destination = Bgra::new(1, 2, 3, 0);
    let transparent_source = Bgra::new(9, 8, 7, 0);
    assert_eq!(
        normal_over(destination, transparent_source, 255),
        destination
    );
    assert_eq!(normal_over(destination, Bgra::WHITE, 0), destination);
    assert_eq!(
        normal_over(Bgra::new(50, 60, 70, 200), Bgra::WHITE, 255),
        Bgra::WHITE
    );
    assert_eq!(
        normal_over(destination, Bgra::new(10, 20, 30, 128), 128),
        Bgra::new(10, 20, 30, 64)
    );
}

#[test]
fn composition_is_bottom_to_top_and_honors_visibility_and_layer_opacity() {
    let bottom = solid_layer(1, 1, 1, Bgra::new(0, 0, 255, 255)).with_background(true);
    let middle = solid_layer(2, 1, 1, Bgra::new(255, 0, 0, 128)).with_opacity(128);
    let hidden = solid_layer(3, 1, 1, Bgra::new(0, 255, 0, 255)).with_visibility(false);
    let document =
        Document::from_layers(size(1, 1), vec![bottom, middle, hidden]).expect("layered document");
    let composed = compose(&document).expect("Normal composition");
    // Source alpha 64 over opaque destination leaves destination alpha 191.
    assert_eq!(composed.pixels(), &[Bgra::new(64, 0, 191, 255)]);
}

#[test]
fn composition_on_transparent_output_preserves_color_and_scales_alpha() {
    let source = solid_layer(1, 1, 1, Bgra::new(10, 20, 30, 200)).with_opacity(128);
    let document = Document::from_layers(size(1, 1), vec![source]).expect("document");
    let composed = compose(&document).expect("composition");
    // round(200 * 128 / 255) = 100.
    assert_eq!(composed.pixels(), &[Bgra::new(10, 20, 30, 100)]);
}

#[test]
fn composition_of_all_hidden_or_zero_opacity_layers_is_transparent() {
    let hidden = solid_layer(1, 1, 1, Bgra::WHITE).with_visibility(false);
    let zero = solid_layer(2, 1, 1, Bgra::BLACK).with_opacity(0);
    let document = Document::from_layers(size(1, 1), vec![hidden, zero]).expect("document");
    assert_eq!(
        compose(&document).expect("composition").pixels(),
        &[Bgra::TRANSPARENT]
    );
}

#[test]
fn raster_patches_become_stale_and_fail_without_mutation() {
    let mut document = Document::from_layers(size(1, 1), vec![solid_layer(8, 1, 1, Bgra::BLACK)])
        .expect("document");
    let first = fill(&document, LayerId::new(8), Bgra::WHITE)
        .expect("fill")
        .expect("one pixel");
    document.apply_patch(&first).expect("apply first");
    let stale = invert(
        &Document::from_layers(size(1, 1), vec![solid_layer(8, 1, 1, Bgra::BLACK)])
            .expect("old document"),
        LayerId::new(8),
    )
    .expect("invert")
    .expect("one pixel");
    let snapshot = document.clone();
    assert!(document.apply_patch(&stale).is_err());
    assert_eq!(document, snapshot);
    assert_eq!(
        document.active_layer().surface().get(Point::new(0, 0)),
        Some(&Bgra::WHITE)
    );
}

#[test]
fn stroke_connects_points_and_clips_to_the_selection() {
    let surface = Surface::filled(size(6, 4), Bgra::WHITE).expect("surface");
    let mut document = Document::from_layers(
        size(6, 4),
        vec![Layer::new(LayerId::new(9), "stroke", surface)],
    )
    .expect("document");
    document
        .set_selection(Some(Rect::new(1, 1, 5, 3).expect("selection")))
        .expect("selection");
    let patch = paint_raster::stroke(
        &document,
        LayerId::new(9),
        &[Point::new(0, 2), Point::new(5, 2)],
        1,
        Some(Bgra::BLACK),
    )
    .expect("stroke")
    .expect("visible stroke");
    assert_eq!(patch.rect(), Rect::new(1, 2, 5, 3).expect("clipped"));
    document.apply_patch(&patch).expect("apply");
    let pixels = document.active_layer().surface().pixels();
    for x in 1..5 {
        assert_eq!(pixels[12 + x as usize], Bgra::BLACK, "x={x}");
    }
    assert_eq!(pixels[0], Bgra::WHITE);
    assert_eq!(pixels[5], Bgra::WHITE);
}

#[test]
fn eraser_stroke_writes_transparent_and_ellipse_respects_its_inscribed_bounds() {
    let surface = Surface::filled(size(5, 5), Bgra::BLACK).expect("surface");
    let mut document = Document::from_layers(
        size(5, 5),
        vec![Layer::new(LayerId::new(3), "tools", surface)],
    )
    .expect("document");
    let erase = paint_raster::stroke(&document, LayerId::new(3), &[Point::new(2, 2)], 1, None)
        .expect("stroke")
        .expect("erase patch");
    document.apply_patch(&erase).expect("erase");
    assert_eq!(
        document.active_layer().surface().pixels()[12],
        Bgra::TRANSPARENT
    );

    let ellipse = paint_raster::fill_ellipse(
        &document,
        LayerId::new(3),
        Rect::new(0, 0, 5, 5).expect("ellipse bounds"),
        Bgra::new(10, 20, 30, 255),
    )
    .expect("ellipse")
    .expect("ellipse patch");
    document.apply_patch(&ellipse).expect("apply ellipse");
    let pixels = document.active_layer().surface().pixels();
    assert_eq!(pixels[12], Bgra::new(10, 20, 30, 255));
    assert_eq!(pixels[0], Bgra::BLACK);
    assert_eq!(pixels[4], Bgra::BLACK);
    assert_eq!(pixels[20], Bgra::BLACK);
    assert_eq!(pixels[24], Bgra::BLACK);
}

#[test]
fn ellipse_noop_and_out_of_bounds_are_explicit_none() {
    let document = Document::from_layers(
        size(4, 4),
        vec![solid_layer(5, 4, 4, Bgra::new(1, 2, 3, 255))],
    )
    .expect("document");
    assert_eq!(
        paint_raster::fill_ellipse(
            &document,
            LayerId::new(5),
            Rect::new(0, 0, 4, 4).expect("bounds"),
            Bgra::new(1, 2, 3, 255),
        ),
        Ok(None)
    );
    assert_eq!(
        paint_raster::fill_ellipse(
            &document,
            LayerId::new(5),
            Rect::new(9, 9, 12, 12).expect("outside"),
            Bgra::BLACK,
        ),
        Ok(None)
    );
}
