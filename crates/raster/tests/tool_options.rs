//! Paint.NET-shaped tool options: brush hardness/spacing, shape outline/fill/border,
//! and gradient type/repeat sampling.

use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{
    BrushOptions, GradientOptions, GradientRepeat, GradientType, ShapeStyle, gradient,
    recolor_stroke, shape_ellipse, shape_rect, stroke,
};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn solid_document(width: u32, height: u32, color: Bgra) -> Document {
    let surface = Surface::filled(size(width, height), color).expect("test surface");
    Document::from_layers(
        size(width, height),
        vec![Layer::new(LayerId::new(1), "layer", surface)],
    )
    .expect("test document")
}

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("test rectangle")
}

fn pixel(document: &Document, x: u32, y: u32) -> Bgra {
    *document
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("in-bounds pixel")
}

fn apply(
    document: &mut Document,
    operation: impl FnOnce(&Document) -> Option<paint_document::PixelPatch>,
) {
    let patch = operation(document).expect("expected a raster patch");
    document.apply_patch(&patch).expect("apply patch");
}

fn expected_gray(t: f64) -> i32 {
    (255.0 * t.clamp(0.0, 1.0)).round() as i32
}

#[test]
fn brush_hardness_controls_edge_softness() {
    let mut hard = solid_document(41, 41, Bgra::WHITE);
    apply(&mut hard, |doc| {
        stroke(
            doc,
            LayerId::new(1),
            &[Point::new(20, 20)],
            BrushOptions::new(21, 100, 15),
            Some(Bgra::BLACK),
        )
        .expect("hard stroke")
    });
    assert_eq!(pixel(&hard, 20, 20), Bgra::BLACK, "hard centre");
    assert_eq!(
        pixel(&hard, 30, 20),
        Bgra::BLACK,
        "hardness 100 keeps the brush radius fully opaque"
    );

    let mut soft = solid_document(41, 41, Bgra::WHITE);
    apply(&mut soft, |doc| {
        stroke(
            doc,
            LayerId::new(1),
            &[Point::new(20, 20)],
            BrushOptions::new(21, 0, 15),
            Some(Bgra::BLACK),
        )
        .expect("soft stroke")
    });
    assert_eq!(
        pixel(&soft, 30, 20),
        Bgra::WHITE,
        "hardness 0 fades to zero coverage at the radius"
    );
    let mid = pixel(&soft, 25, 20);
    assert!(
        (60..=200).contains(&mid.r),
        "half-radius coverage should blend into the layer: {mid:?}"
    );
}

#[test]
fn eraser_hardness_reduces_alpha_at_the_edge() {
    let mut document = solid_document(41, 41, Bgra::BLACK);
    apply(&mut document, |doc| {
        stroke(
            doc,
            LayerId::new(1),
            &[Point::new(20, 20)],
            BrushOptions::new(21, 0, 15),
            None,
        )
        .expect("soft erase")
    });
    assert_eq!(pixel(&document, 20, 20).a, 0, "the centre is fully erased");
    let edge = pixel(&document, 25, 20);
    assert!(
        (60..=200).contains(&edge.a),
        "the soft eraser should partially erase: {edge:?}"
    );
    assert_eq!(
        pixel(&document, 30, 20).a,
        255,
        "the outer radius stays untouched"
    );
}

#[test]
fn stroke_spacing_leaves_gaps_between_stamps() {
    let mut wide = solid_document(40, 9, Bgra::WHITE);
    apply(&mut wide, |doc| {
        stroke(
            doc,
            LayerId::new(1),
            &[Point::new(0, 4), Point::new(39, 4)],
            BrushOptions::new(8, 100, 100),
            Some(Bgra::BLACK),
        )
        .expect("spaced stroke")
    });
    assert_eq!(
        pixel(&wide, 4, 4),
        Bgra::WHITE,
        "spacing 100% must leave an uncovered gap between round stamps"
    );
    assert_eq!(pixel(&wide, 8, 4), Bgra::BLACK, "stamp centre is painted");

    let mut dense = solid_document(40, 9, Bgra::WHITE);
    apply(&mut dense, |doc| {
        stroke(
            doc,
            LayerId::new(1),
            &[Point::new(0, 4), Point::new(39, 4)],
            BrushOptions::new(8, 100, 10),
            Some(Bgra::BLACK),
        )
        .expect("dense stroke")
    });
    assert_eq!(
        pixel(&dense, 4, 4),
        Bgra::BLACK,
        "spacing 10% keeps neighbouring stamps connected"
    );
}

#[test]
fn recolor_brush_accepts_hardness_and_spacing() {
    let mut spaced = solid_document(40, 1, Bgra::WHITE);
    apply(&mut spaced, |doc| {
        recolor_stroke(
            doc,
            LayerId::new(1),
            &[Point::new(0, 0), Point::new(39, 0)],
            BrushOptions::new(8, 100, 100),
            Bgra::WHITE,
            Bgra::BLACK,
            0,
        )
        .expect("recolor stroke")
    });
    assert_eq!(
        pixel(&spaced, 4, 0),
        Bgra::WHITE,
        "recolor spacing must leave gaps too"
    );
    assert_eq!(pixel(&spaced, 8, 0), Bgra::BLACK);

    let mut soft = solid_document(21, 21, Bgra::WHITE);
    apply(&mut soft, |doc| {
        recolor_stroke(
            doc,
            LayerId::new(1),
            &[Point::new(10, 10)],
            BrushOptions::new(21, 0, 15),
            Bgra::WHITE,
            Bgra::BLACK,
            0,
        )
        .expect("soft recolor")
    });
    assert_eq!(pixel(&soft, 10, 10), Bgra::BLACK);
    assert_eq!(
        pixel(&soft, 20, 10),
        Bgra::WHITE,
        "hardness 0 leaves the outer radius unchanged"
    );
}

#[test]
fn rectangle_shapes_support_outline_fill_and_both_with_border_width() {
    let bounds = rect(2, 2, 10, 10);

    let mut outline = solid_document(12, 12, Bgra::WHITE);
    apply(&mut outline, |doc| {
        shape_rect(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Outline,
            2,
        )
        .expect("outline")
    });
    assert_eq!(pixel(&outline, 2, 2), Bgra::BLACK, "outer corner");
    assert_eq!(pixel(&outline, 3, 3), Bgra::BLACK, "two-pixel border");
    assert_eq!(pixel(&outline, 5, 5), Bgra::WHITE, "interior stays clear");
    assert_eq!(pixel(&outline, 9, 9), Bgra::BLACK, "far corner border");
    assert_eq!(pixel(&outline, 1, 1), Bgra::WHITE, "outside stays clear");

    let mut narrow = solid_document(12, 12, Bgra::WHITE);
    apply(&mut narrow, |doc| {
        shape_rect(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Outline,
            1,
        )
        .expect("narrow outline")
    });
    assert_eq!(pixel(&narrow, 3, 3), Bgra::WHITE, "one-pixel border");

    let mut fill = solid_document(12, 12, Bgra::WHITE);
    apply(&mut fill, |doc| {
        shape_rect(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Fill,
            2,
        )
        .expect("fill")
    });
    assert_eq!(pixel(&fill, 2, 2), Bgra::BLACK);
    assert_eq!(pixel(&fill, 5, 5), Bgra::BLACK, "fill covers the interior");
    assert_eq!(pixel(&fill, 9, 9), Bgra::BLACK);
    assert_eq!(pixel(&fill, 10, 10), Bgra::WHITE, "half-open bounds");

    let mut both = solid_document(12, 12, Bgra::WHITE);
    apply(&mut both, |doc| {
        shape_rect(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Both,
            1,
        )
        .expect("both")
    });
    assert_eq!(pixel(&both, 2, 2), Bgra::BLACK, "outline");
    assert_eq!(pixel(&both, 5, 5), Bgra::BLACK, "interior");
    assert_eq!(pixel(&both, 1, 1), Bgra::WHITE, "outside");
}

#[test]
fn ellipse_shapes_support_outline_fill_and_both_with_border_width() {
    let bounds = rect(0, 0, 21, 21);

    let mut outline = solid_document(21, 21, Bgra::WHITE);
    apply(&mut outline, |doc| {
        shape_ellipse(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Outline,
            2,
        )
        .expect("ellipse outline")
    });
    assert_eq!(pixel(&outline, 10, 0), Bgra::BLACK, "top rim");
    assert_eq!(pixel(&outline, 10, 10), Bgra::WHITE, "hollow centre");
    assert_eq!(pixel(&outline, 0, 0), Bgra::WHITE, "corner outside");

    let mut fill = solid_document(21, 21, Bgra::WHITE);
    apply(&mut fill, |doc| {
        shape_ellipse(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Fill,
            2,
        )
        .expect("ellipse fill")
    });
    assert_eq!(pixel(&fill, 10, 10), Bgra::BLACK, "filled centre");
    assert_eq!(pixel(&fill, 10, 0), Bgra::BLACK, "filled rim");
    assert_eq!(pixel(&fill, 0, 0), Bgra::WHITE, "corner outside");

    let mut thick = solid_document(21, 21, Bgra::WHITE);
    apply(&mut thick, |doc| {
        shape_ellipse(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Outline,
            20,
        )
        .expect("thick ellipse outline")
    });
    assert_eq!(
        pixel(&thick, 10, 10),
        Bgra::BLACK,
        "a border wider than the radius fills the inscribed ellipse"
    );

    let mut both = solid_document(21, 21, Bgra::WHITE);
    apply(&mut both, |doc| {
        shape_ellipse(
            doc,
            LayerId::new(1),
            bounds,
            Bgra::BLACK,
            ShapeStyle::Both,
            2,
        )
        .expect("ellipse both")
    });
    assert_eq!(pixel(&both, 10, 10), Bgra::BLACK);
    assert_eq!(pixel(&both, 10, 0), Bgra::BLACK);
    assert_eq!(pixel(&both, 0, 0), Bgra::WHITE);
}

#[test]
fn shape_operations_clip_to_the_active_selection() {
    let mut document = solid_document(10, 10, Bgra::WHITE);
    document
        .set_selection(Some(rect(4, 0, 6, 10)))
        .expect("selection");
    apply(&mut document, |doc| {
        shape_rect(
            doc,
            LayerId::new(1),
            rect(0, 0, 10, 10),
            Bgra::BLACK,
            ShapeStyle::Fill,
            2,
        )
        .expect("clipped rectangle")
    });
    for x in 0..10 {
        let expected = if (4..6).contains(&x) {
            Bgra::BLACK
        } else {
            Bgra::WHITE
        };
        assert_eq!(pixel(&document, x, 5), expected, "column {x}");
    }
}

#[test]
fn linear_gradient_repeat_wraps_beyond_the_drag() {
    let mut clamped = solid_document(30, 1, Bgra::WHITE);
    apply(&mut clamped, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(0, 0),
            Point::new(10, 0),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Linear, GradientRepeat::None),
        )
        .expect("clamped gradient")
    });
    let inside = pixel(&clamped, 5, 0);
    assert!((inside.r as i32 - expected_gray(0.55)).abs() <= 2);
    assert_eq!(
        pixel(&clamped, 15, 0),
        Bgra::WHITE,
        "None clamps beyond the end point"
    );

    let mut repeated = solid_document(30, 1, Bgra::WHITE);
    apply(&mut repeated, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(0, 0),
            Point::new(10, 0),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Linear, GradientRepeat::Repeat),
        )
        .expect("repeated gradient")
    });
    assert_eq!(
        pixel(&repeated, 15, 0),
        inside,
        "Repeat wraps t=1.55 back to 0.55"
    );
    assert_eq!(
        pixel(&repeated, 10, 0),
        pixel(&repeated, 0, 0),
        "Repeat wraps t=1.05 back to 0.05"
    );
}

#[test]
fn reflected_gradient_mirrors_before_the_start_point() {
    let mut linear = solid_document(30, 1, Bgra::WHITE);
    apply(&mut linear, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 0),
            Point::new(20, 0),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Linear, GradientRepeat::None),
        )
        .expect("linear gradient")
    });
    assert_eq!(
        pixel(&linear, 5, 0),
        Bgra::BLACK,
        "linear clamping stays at the start colour"
    );

    let mut reflected = solid_document(30, 1, Bgra::WHITE);
    apply(&mut reflected, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 0),
            Point::new(20, 0),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Reflected, GradientRepeat::None),
        )
        .expect("reflected gradient")
    });
    assert!(
        (pixel(&reflected, 5, 0).r as i32 - expected_gray(0.45)).abs() <= 2,
        "Reflected uses |t|: {:?}",
        pixel(&reflected, 5, 0)
    );
    assert!(
        (pixel(&reflected, 15, 0).r as i32 - expected_gray(0.55)).abs() <= 2,
        "inside the drag Reflected equals Linear: {:?}",
        pixel(&reflected, 15, 0)
    );
}

#[test]
fn diamond_gradient_samples_distance_on_both_axes() {
    let mut document = solid_document(21, 21, Bgra::WHITE);
    apply(&mut document, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 10),
            Point::new(15, 10),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Diamond, GradientRepeat::None),
        )
        .expect("diamond gradient")
    });
    assert!(
        (pixel(&document, 10, 8).r as i32 - expected_gray(0.4)).abs() <= 2,
        "diamond combines the axis and perpendicular distances: {:?}",
        pixel(&document, 10, 8)
    );
    assert!(
        (pixel(&document, 10, 10).r as i32 - expected_gray(0.2)).abs() <= 2,
        "the pixel at the start is near the dark centre: {:?}",
        pixel(&document, 10, 10)
    );
    assert_eq!(
        pixel(&document, 15, 10),
        Bgra::WHITE,
        "the drag endpoint reaches the far colour"
    );
}

#[test]
fn radial_gradient_uses_euclidean_distance_from_the_drag_start() {
    let mut document = solid_document(21, 21, Bgra::WHITE);
    apply(&mut document, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 10),
            Point::new(20, 10),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Radial, GradientRepeat::None),
        )
        .expect("radial gradient")
    });
    assert!(
        (pixel(&document, 10, 5).r as i32 - expected_gray(4.527_7 / 10.0)).abs() <= 3,
        "top sample: {:?}",
        pixel(&document, 10, 5)
    );
    assert!(
        (pixel(&document, 10, 15).r as i32 - expected_gray(5.522_7 / 10.0)).abs() <= 3,
        "bottom sample samples its own centre distance: {:?}",
        pixel(&document, 10, 15)
    );
    assert!(
        (pixel(&document, 10, 10).r as i32 - expected_gray(0.5_f64.hypot(0.5) / 10.0)).abs() <= 2,
        "the start pixel samples its centre offset: {:?}",
        pixel(&document, 10, 10)
    );
}

#[test]
fn conical_gradient_sweeps_angles_and_repeat_doubles_the_sweep() {
    let mut single = solid_document(21, 21, Bgra::WHITE);
    apply(&mut single, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 10),
            Point::new(20, 10),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Conical, GradientRepeat::None),
        )
        .expect("conical gradient")
    });
    let bottom = pixel(&single, 10, 15).r;
    let top = pixel(&single, 10, 5).r;
    assert!(
        (40..=90).contains(&bottom),
        "the clockwise sample is in the first sweep: {bottom}"
    );
    assert!(
        (170..=225).contains(&top),
        "the counter-clockwise sample is in the second sweep: {top}"
    );

    let mut repeated = solid_document(21, 21, Bgra::WHITE);
    apply(&mut repeated, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(10, 10),
            Point::new(20, 10),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Conical, GradientRepeat::Repeat),
        )
        .expect("repeated conical gradient")
    });
    let opposite = pixel(&repeated, 0, 10).r;
    let opposite_single = pixel(&single, 0, 10).r;
    assert!(
        (100..=150).contains(&opposite_single),
        "None maps the opposite direction to the middle of one sweep: {opposite_single}"
    );
    assert!(
        opposite >= 240,
        "Repeat wraps the opposite direction into the next period: {opposite}"
    );
}

// --- Tool Options parity batch: sampling, blend, alignment, spacing, picker samples ---

use paint_document::BlendMode;
use paint_raster::{
    RecolorOptions, TextAlignment, TextFontFamily, TextStyle, clone_stamp, flood_fill_sampled,
    recolor_stroke_sampled, text,
};

/// A white bottom layer crossed by a one-row black barrier at y = 5, plus a transparent
/// top layer that is active. Layer sampling sees only the transparent layer; image sampling
/// sees the barrier and must stop there.
fn sampled_document() -> (Document, LayerId) {
    let white = Surface::filled(size(10, 10), Bgra::WHITE).expect("bottom surface");
    let mut bottom = white;
    for x in 0..10 {
        bottom.pixels_mut()[5 * 10 + x] = Bgra::BLACK;
    }
    let top = Surface::new(size(10, 10)).expect("top surface");
    let mut document = Document::from_layers(
        size(10, 10),
        vec![
            Layer::new(LayerId::new(1), "bottom", bottom),
            Layer::new(LayerId::new(2), "top", top),
        ],
    )
    .expect("document");
    document
        .set_active_layer(LayerId::new(2))
        .expect("activate top layer");
    (document, LayerId::new(2))
}

fn active_pixel(document: &Document, x: u32, y: u32) -> Bgra {
    *document
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("active pixel")
}

#[test]
fn flood_fill_image_sampling_matches_the_composed_image() {
    let (mut layer_document, layer_id) = sampled_document();
    apply(&mut layer_document, |document| {
        flood_fill_sampled(
            document,
            layer_id,
            Point::new(2, 2),
            Bgra::BLACK,
            0,
            paint_raster::MagicWandSampling::ActiveLayer,
            BlendMode::Normal,
        )
        .expect("layer-sampled fill")
    });
    assert_eq!(
        active_pixel(&layer_document, 2, 8),
        Bgra::BLACK,
        "layer sampling sees one contiguous transparent layer, so the barrier is invisible"
    );

    let (mut image_document, image_id) = sampled_document();
    apply(&mut image_document, |document| {
        flood_fill_sampled(
            document,
            image_id,
            Point::new(2, 2),
            Bgra::BLACK,
            0,
            paint_raster::MagicWandSampling::Image,
            BlendMode::Normal,
        )
        .expect("image-sampled fill")
    });
    assert_eq!(active_pixel(&image_document, 2, 2), Bgra::BLACK);
    assert_eq!(
        active_pixel(&image_document, 2, 8),
        Bgra::TRANSPARENT,
        "image sampling must stop at the composed black barrier"
    );
}

#[test]
fn flood_fill_honours_a_tool_blend_mode() {
    let mut document = solid_document(4, 1, Bgra::new(128, 128, 128, 255));
    apply(&mut document, |doc| {
        flood_fill_sampled(
            doc,
            LayerId::new(1),
            Point::new(0, 0),
            Bgra::new(0, 0, 255, 255),
            0,
            paint_raster::MagicWandSampling::ActiveLayer,
            BlendMode::Multiply,
        )
        .expect("multiply fill")
    });
    assert_eq!(
        pixel(&document, 2, 0),
        Bgra::new(0, 0, 128, 255),
        "Multiply blends the fill colour with the layer instead of replacing it"
    );
}

#[test]
fn gradient_honours_blend_mode_and_transparency_only() {
    let mut multiplied = solid_document(9, 1, Bgra::new(128, 128, 128, 255));
    apply(&mut multiplied, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(0, 0),
            Point::new(8, 0),
            Bgra::BLACK,
            Bgra::WHITE,
            GradientOptions::new(GradientType::Linear, GradientRepeat::None)
                .with_blend(BlendMode::Multiply),
        )
        .expect("multiply gradient")
    });
    assert!(
        pixel(&multiplied, 0, 0).r < 20,
        "multiplying the near-black gradient stop with grey stays nearly black: {:?}",
        pixel(&multiplied, 0, 0)
    );
    let middle = pixel(&multiplied, 4, 0);
    assert!(
        (55..=75).contains(&middle.r),
        "the grey midpoint is multiplied by the gradient value: {middle:?}"
    );

    let mut alpha_only = solid_document(9, 1, Bgra::new(128, 64, 32, 255));
    apply(&mut alpha_only, |doc| {
        gradient(
            doc,
            LayerId::new(1),
            Point::new(0, 0),
            Point::new(8, 0),
            Bgra::new(255, 255, 255, 255),
            Bgra::new(255, 255, 255, 255),
            GradientOptions::new(GradientType::Linear, GradientRepeat::None).with_alpha_only(true),
        )
        .expect("alpha-only gradient")
    });
    assert_eq!(
        pixel(&alpha_only, 8, 0).a,
        0,
        "transparency-only reverses the second stop alpha, reaching zero"
    );
    assert_eq!(
        pixel(&alpha_only, 8, 0),
        Bgra::new(128, 64, 32, 0),
        "transparency-only must preserve the layer RGB channels"
    );
}

fn ink_bounds(document: &Document, top: u32, bottom: u32) -> (u32, u32) {
    let mut min_x = u32::MAX;
    let mut max_x = 0;
    for y in top..bottom {
        for x in 0..document.size().width() {
            let pixel = *document
                .active_layer()
                .surface()
                .pixel(Point::new(x, y))
                .expect("scan pixel");
            if pixel.r < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    assert!(
        min_x != u32::MAX,
        "expected glyph ink in rows {top}..{bottom}"
    );
    (min_x, max_x)
}

#[test]
fn text_alignment_aligns_each_line_to_the_click_point() {
    let base = TextStyle {
        family: TextFontFamily::Sans,
        size: 64,
        bold: false,
        italic: false,
        antialias: false,
        alignment: TextAlignment::Left,
        blend: BlendMode::Normal,
    };
    let render = |alignment: TextAlignment| {
        let mut document = solid_document(320, 200, Bgra::WHITE);
        apply(&mut document, |doc| {
            text(
                doc,
                LayerId::new(1),
                Point::new(120, 10),
                "I\nWW",
                TextStyle { alignment, ..base },
                Bgra::BLACK,
            )
            .expect("aligned text")
        });
        document
    };

    let left = render(TextAlignment::Left);
    let (left_min, _) = ink_bounds(&left, 0, 80);
    let (left_second_min, _) = ink_bounds(&left, 80, 160);
    assert!(
        left_min.abs_diff(left_second_min) <= 4,
        "left-aligned lines share a start edge: {left_min} vs {left_second_min}"
    );

    let center = render(TextAlignment::Center);
    let (center_min, center_max) = ink_bounds(&center, 0, 80);
    let (center_second_min, center_second_max) = ink_bounds(&center, 80, 160);
    let center_delta = (i64::from(center_min) + i64::from(center_max))
        .abs_diff(i64::from(center_second_min) + i64::from(center_second_max));
    assert!(
        center_delta <= 8,
        "center-aligned lines share a centre: {center_min}..{center_max} vs \
         {center_second_min}..{center_second_max}"
    );

    let right = render(TextAlignment::Right);
    let (_, right_max) = ink_bounds(&right, 0, 80);
    let (_, right_second_max) = ink_bounds(&right, 80, 160);
    assert!(
        right_max.abs_diff(right_second_max) <= 4,
        "right-aligned lines share an end edge: {right_max} vs {right_second_max}"
    );
    assert!(
        right_second_max < center_second_max,
        "right alignment puts the wide line's end before its centred end:          {right_second_max} vs {center_second_max}"
    );
}

#[test]
fn text_honours_a_tool_blend_mode() {
    let base = TextStyle {
        family: TextFontFamily::Sans,
        size: 64,
        bold: false,
        italic: false,
        antialias: false,
        alignment: TextAlignment::Left,
        blend: BlendMode::Normal,
    };
    let render = |blend| {
        let mut document = solid_document(200, 100, Bgra::new(128, 128, 128, 255));
        apply(&mut document, |doc| {
            text(
                doc,
                LayerId::new(1),
                Point::new(10, 10),
                "M",
                TextStyle { blend, ..base },
                Bgra::new(0, 0, 255, 255),
            )
            .expect("blended text")
        });
        document
    };
    let normal = render(BlendMode::Normal);
    assert!(
        normal
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .any(|pixel| *pixel == Bgra::new(0, 0, 255, 255)),
        "Normal text writes the exact text colour"
    );
    let multiply = render(BlendMode::Multiply);
    assert!(
        multiply
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .any(|pixel| *pixel == Bgra::new(0, 0, 128, 255)),
        "Multiply text blends the glyph colour with the layer"
    );
}

#[test]
fn clone_stamp_spacing_leaves_real_gaps() {
    let mut pixels = vec![Bgra::WHITE; 20 * 9];
    for y in 0..2 {
        for x in 0..20 {
            pixels[y * 20 + x] = Bgra::BLACK;
        }
    }
    let surface = Surface::from_pixels(size(20, 9), pixels).expect("clone surface");
    let document = Document::from_layers(
        size(20, 9),
        vec![Layer::new(LayerId::new(7), "clone", surface)],
    )
    .expect("document");
    let render = |spacing| {
        let mut document = document.clone();
        apply(&mut document, |doc| {
            clone_stamp(
                doc,
                LayerId::new(7),
                &[Point::new(0, 4), Point::new(17, 4)],
                8,
                Point::new(2, 0),
                100,
                spacing,
            )
            .expect("spaced clone")
        });
        document
    };

    let spaced = render(100);
    assert_eq!(
        pixel(&spaced, 4, 4),
        Bgra::WHITE,
        "100% spacing leaves a gap between 8 px stamps"
    );
    assert_eq!(
        pixel(&spaced, 8, 4),
        Bgra::BLACK,
        "stamp centres sample black"
    );

    let dense = render(10);
    assert_eq!(
        pixel(&dense, 4, 4),
        Bgra::BLACK,
        "10% spacing keeps neighbouring stamps connected"
    );
}

#[test]
fn recolor_image_sampling_matches_the_composed_image() {
    let (mut layer_document, layer_id) = sampled_document();
    assert_eq!(
        recolor_stroke_sampled(
            &layer_document,
            layer_id,
            &[Point::new(2, 2)],
            BrushOptions::new(1, 100, 15),
            RecolorOptions::new(
                Bgra::WHITE,
                Bgra::BLACK,
                0,
                paint_raster::MagicWandSampling::ActiveLayer,
            ),
        )
        .expect("layer-sampled recolor"),
        None,
        "the transparent active layer has no white pixels to recolor"
    );
    apply(&mut layer_document, |document| {
        recolor_stroke_sampled(
            document,
            layer_id,
            &[Point::new(2, 2)],
            BrushOptions::new(1, 100, 15),
            RecolorOptions::new(
                Bgra::WHITE,
                Bgra::BLACK,
                0,
                paint_raster::MagicWandSampling::Image,
            ),
        )
        .expect("image-sampled recolor")
    });
    assert_eq!(
        active_pixel(&layer_document, 2, 2),
        Bgra::BLACK,
        "image sampling matches the composed white pixel and writes to the active layer"
    );
}
