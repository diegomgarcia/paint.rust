//! Pixel-level tests for the line rasterizer's caps, dash patterns, clipping, and no-ops.

use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, PixelPatch, SelectionMask};
use paint_raster::{LineCap, LineDash, LineStyle, RasterError, stroke_line};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn new_document(width: u32, height: u32) -> Document {
    let surface = Surface::filled(size(width, height), Bgra::WHITE).expect("test surface");
    Document::from_layers(
        size(width, height),
        vec![Layer::new(LayerId::new(1), "pixels", surface)],
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
        .expect("pixel inside the test canvas")
}

fn apply(
    document: &mut Document,
    operation: impl FnOnce(&Document) -> Result<Option<PixelPatch>, RasterError>,
) -> bool {
    let patch = operation(document).expect("line raster operation");
    if let Some(patch) = patch {
        document.apply_patch(&patch).expect("apply line patch");
        true
    } else {
        false
    }
}

#[test]
fn flat_line_paints_one_pixel_wide_from_endpoint_to_endpoint() {
    let mut document = new_document(20, 20);
    assert!(apply(&mut document, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(5, 10), Point::new(14, 10)],
            LineStyle {
                width: 1,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Solid,
            },
        )
    }));

    for x in 5..=14 {
        assert_eq!(
            pixel(&document, x, 10),
            Bgra::BLACK,
            "flat 1 px line at x={x}"
        );
    }
    assert_eq!(pixel(&document, 4, 10), Bgra::WHITE);
    assert_eq!(pixel(&document, 15, 10), Bgra::WHITE);
    assert_eq!(pixel(&document, 10, 9), Bgra::WHITE);
    assert_eq!(pixel(&document, 10, 11), Bgra::WHITE);
}

#[test]
fn rounded_caps_extend_past_both_endpoints() {
    let mut document = new_document(30, 30);
    assert!(apply(&mut document, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(10, 10), Point::new(19, 10)],
            LineStyle {
                width: 5,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Rounded,
                end_cap: LineCap::Rounded,
                dash: LineDash::Solid,
            },
        )
    }));
    assert_eq!(
        pixel(&document, 9, 10),
        Bgra::BLACK,
        "rounded start cap paints past the first endpoint"
    );
    assert_eq!(
        pixel(&document, 20, 10),
        Bgra::BLACK,
        "rounded end cap paints past the last endpoint"
    );
    assert_eq!(
        pixel(&document, 21, 10),
        Bgra::BLACK,
        "half the 5 px line width still reaches column 21"
    );
    assert_eq!(
        pixel(&document, 22, 10),
        Bgra::WHITE,
        "rounded cap stops after half the line width"
    );
    assert_eq!(pixel(&document, 9, 14), Bgra::WHITE);
}

#[test]
fn arrow_caps_paint_chevrons_away_from_the_shaft() {
    let mut document = new_document(40, 60);
    assert!(apply(&mut document, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(10, 10), Point::new(19, 10)],
            LineStyle {
                width: 5,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Arrow,
                dash: LineDash::Solid,
            },
        )
    }));
    assert_eq!(
        pixel(&document, 15, 13),
        Bgra::BLACK,
        "end arrow paints below the shaft"
    );
    assert_eq!(
        pixel(&document, 15, 7),
        Bgra::BLACK,
        "end arrow paints above the shaft"
    );
    assert_eq!(
        pixel(&document, 15, 10),
        Bgra::BLACK,
        "the shaft still reaches the arrow tip"
    );

    let mut start = new_document(40, 60);
    assert!(apply(&mut start, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(10, 40), Point::new(19, 40)],
            LineStyle {
                width: 5,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Arrow,
                end_cap: LineCap::Flat,
                dash: LineDash::Solid,
            },
        )
    }));
    assert_eq!(
        pixel(&start, 14, 43),
        Bgra::BLACK,
        "start arrow paints a chevron into the canvas"
    );
    assert_eq!(pixel(&start, 14, 37), Bgra::BLACK);
}

#[test]
fn dash_and_dot_styles_follow_width_scaled_patterns() {
    let mut down = new_document(30, 30);
    assert!(apply(&mut down, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(5, 5), Point::new(24, 5)],
            LineStyle {
                width: 1,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Solid,
            },
        )
    }));
    for x in 5..=24 {
        assert_eq!(pixel(&down, x, 5), Bgra::BLACK, "solid x={x}");
    }

    let mut dashed = new_document(30, 30);
    assert!(apply(&mut dashed, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(5, 10), Point::new(24, 10)],
            LineStyle {
                width: 1,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Dash,
            },
        )
    }));
    for x in [5, 6, 7, 9, 10, 11] {
        assert_eq!(pixel(&dashed, x, 10), Bgra::BLACK, "dash on x={x}");
    }
    for x in [8, 12] {
        assert_eq!(pixel(&dashed, x, 10), Bgra::WHITE, "dash gap x={x}");
    }

    let mut dotted = new_document(30, 30);
    assert!(apply(&mut dotted, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(5, 15), Point::new(24, 15)],
            LineStyle {
                width: 1,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Dot,
            },
        )
    }));
    for x in [5, 7, 9] {
        assert_eq!(pixel(&dotted, x, 15), Bgra::BLACK, "dot on x={x}");
    }
    for x in [6, 8, 10] {
        assert_eq!(pixel(&dotted, x, 15), Bgra::WHITE, "dot gap x={x}");
    }
}

#[test]
fn line_pixels_are_clipped_to_a_non_rectangular_selection_mask() {
    let mut document = new_document(20, 20);
    let mut mask = SelectionMask::empty(rect(0, 0, 20, 20)).expect("mask");
    for y in 0..20 {
        mask.set_xy(8, y, true).expect("selected column");
        mask.set_xy(9, y, true).expect("selected column");
    }
    document
        .set_selection_mask(mask)
        .expect("set selection mask");
    assert!(apply(&mut document, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(0, 10), Point::new(19, 10)],
            LineStyle {
                width: 3,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Solid,
            },
        )
    }));
    assert_eq!(pixel(&document, 8, 10), Bgra::BLACK);
    assert_eq!(pixel(&document, 9, 10), Bgra::BLACK);
    assert_eq!(pixel(&document, 7, 10), Bgra::WHITE);
    assert_eq!(pixel(&document, 10, 10), Bgra::WHITE);
    assert_eq!(
        pixel(&document, 8, 8),
        Bgra::WHITE,
        "stroke cross-section is clipped too"
    );
}

#[test]
fn degenerate_lines_are_explicit_about_no_ops() {
    let mut document = new_document(10, 10);
    assert!(
        !apply(&mut document, |document| {
            stroke_line(
                document,
                LayerId::new(1),
                &[],
                LineStyle {
                    width: 1,
                    color: Some(Bgra::BLACK),
                    start_cap: LineCap::Flat,
                    end_cap: LineCap::Flat,
                    dash: LineDash::Solid,
                },
            )
        }),
        "no points produce no patch"
    );
    assert!(
        !apply(&mut document, |document| {
            stroke_line(
                document,
                LayerId::new(1),
                &[Point::new(1, 1), Point::new(8, 1)],
                LineStyle {
                    width: 1,
                    color: Some(Bgra::WHITE),
                    start_cap: LineCap::Flat,
                    end_cap: LineCap::Flat,
                    dash: LineDash::Solid,
                },
            )
        }),
        "painting the existing color produces no patch"
    );
    assert!(
        !apply(&mut document, |document| {
            stroke_line(
                document,
                LayerId::new(1),
                &[Point::new(30, 30), Point::new(40, 30)],
                LineStyle {
                    width: 1,
                    color: Some(Bgra::BLACK),
                    start_cap: LineCap::Flat,
                    end_cap: LineCap::Flat,
                    dash: LineDash::Solid,
                },
            )
        }),
        "a line outside the canvas produces no patch"
    );

    assert!(apply(&mut document, |document| {
        stroke_line(
            document,
            LayerId::new(1),
            &[Point::new(4, 4), Point::new(4, 4)],
            LineStyle {
                width: 1,
                color: Some(Bgra::BLACK),
                start_cap: LineCap::Flat,
                end_cap: LineCap::Flat,
                dash: LineDash::Solid,
            },
        )
    }));
    assert_eq!(pixel(&document, 4, 4), Bgra::BLACK);
}
