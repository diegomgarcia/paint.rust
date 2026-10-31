use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, PixelPatch, SelectionMask};
use paint_raster::{
    BrushOptions, RasterError, fill, fill_ellipse, fill_rect, flood_fill, global_fill, invert,
    linear_gradient, recolor, stamp, stroke, translate,
};

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

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("test rectangle")
}

fn mask(bounds: Rect, selected: &[Point]) -> SelectionMask {
    let mut mask = SelectionMask::empty(bounds).expect("selection mask");
    for point in selected {
        mask.set_xy(i64::from(point.x), i64::from(point.y), true)
            .expect("selected point inside mask bounds");
    }
    mask
}

fn apply(
    document: &mut Document,
    operation: impl FnOnce(&Document) -> Result<Option<PixelPatch>, RasterError>,
) {
    let patch = operation(document)
        .expect("raster operation")
        .expect("expected a non-empty patch");
    document.apply_patch(&patch).expect("apply patch");
}

#[test]
fn fill_and_invert_change_only_masked_pixels_inside_the_bounding_rect() {
    let first = Bgra::new(10, 20, 30, 255);
    let second = Bgra::new(200, 210, 220, 255);
    let pixels = vec![first, second, first];
    let selection = mask(rect(0, 0, 3, 1), &[Point::new(0, 0), Point::new(2, 0)]);

    let mut filled = document(3, 1, pixels.clone());
    filled
        .set_selection_mask(selection.clone())
        .expect("selection");
    let black = Bgra::BLACK;
    apply(&mut filled, |document| {
        fill(document, LayerId::new(1), black)
    });
    assert_eq!(
        filled.active_layer().surface().pixels(),
        &[black, second, black]
    );

    let mut inverted = document(3, 1, pixels.clone());
    inverted.set_selection_mask(selection).expect("selection");
    apply(&mut inverted, |document| invert(document, LayerId::new(1)));
    assert_eq!(
        inverted.active_layer().surface().pixels(),
        &[first.inverted_rgb(), second, first.inverted_rgb()]
    );
}

#[test]
fn fill_returns_none_when_every_masked_pixel_already_has_the_color() {
    let black = Bgra::BLACK;
    let white = Bgra::WHITE;
    let mut document = document(3, 1, vec![black, white, black]);
    document
        .set_selection_mask(mask(
            rect(0, 0, 3, 1),
            &[Point::new(0, 0), Point::new(2, 0)],
        ))
        .expect("selection");

    assert_eq!(fill(&document, LayerId::new(1), black), Ok(None));
}

#[test]
fn stroke_and_eraser_clip_every_stamped_pixel_to_the_mask() {
    let selection = mask(
        rect(0, 0, 5, 5),
        &[
            Point::new(2, 0),
            Point::new(2, 1),
            Point::new(2, 2),
            Point::new(2, 3),
            Point::new(2, 4),
        ],
    );
    let mut stroked = document(5, 5, vec![Bgra::WHITE; 25]);
    stroked
        .set_selection_mask(selection.clone())
        .expect("selection");

    let points = [Point::new(0, 2), Point::new(4, 2)];
    apply(&mut stroked, |document| {
        stroke(
            document,
            LayerId::new(1),
            &points,
            BrushOptions::new(1, 100, 15),
            Some(Bgra::BLACK),
        )
    });
    let pixels = stroked.active_layer().surface().pixels();
    for y in 0..5 {
        for x in 0..5 {
            let expected = if x == 2 && y == 2 {
                Bgra::BLACK
            } else {
                Bgra::WHITE
            };
            assert_eq!(pixels[y * 5 + x], expected, "pixel ({x}, {y})");
        }
    }

    let mut erasing = document(5, 5, vec![Bgra::BLACK; 25]);
    erasing.set_selection_mask(selection).expect("selection");
    apply(&mut erasing, |document| {
        stroke(
            document,
            LayerId::new(1),
            &points,
            BrushOptions::new(1, 100, 15),
            None,
        )
    });
    let pixels = erasing.active_layer().surface().pixels();
    assert_eq!(pixels[2 * 5 + 2], Bgra::TRANSPARENT);
    assert_eq!(pixels[2 * 5], Bgra::BLACK);
    assert_eq!(pixels[2 * 5 + 4], Bgra::BLACK);
}

#[test]
fn flood_fill_treats_unmasked_pixels_as_barriers_while_global_fill_does_not() {
    let mut selected = Vec::new();
    for y in 0..3 {
        selected.push(Point::new(0, y));
        selected.push(Point::new(2, y));
    }
    let selection = mask(rect(0, 0, 3, 3), &selected);

    let mut contiguous = document(3, 3, vec![Bgra::WHITE; 9]);
    contiguous
        .set_selection_mask(selection.clone())
        .expect("selection");
    apply(&mut contiguous, |document| {
        flood_fill(document, LayerId::new(1), Point::new(0, 0), Bgra::BLACK, 0)
    });
    let pixels = contiguous.active_layer().surface().pixels();
    for y in 0..3 {
        assert_eq!(pixels[y * 3], Bgra::BLACK);
        assert_eq!(pixels[y * 3 + 1], Bgra::WHITE);
        assert_eq!(pixels[y * 3 + 2], Bgra::WHITE);
    }

    let mut global = document(3, 3, vec![Bgra::WHITE; 9]);
    global.set_selection_mask(selection).expect("selection");
    apply(&mut global, |document| {
        global_fill(document, LayerId::new(1), Point::new(0, 0), Bgra::BLACK, 0)
    });
    let pixels = global.active_layer().surface().pixels();
    for y in 0..3 {
        assert_eq!(pixels[y * 3], Bgra::BLACK);
        assert_eq!(pixels[y * 3 + 1], Bgra::WHITE);
        assert_eq!(pixels[y * 3 + 2], Bgra::BLACK);
    }
}

#[test]
fn rect_ellipse_gradient_and_recolor_operations_obey_the_mask() {
    let selection = mask(rect(0, 0, 3, 2), &[Point::new(1, 0), Point::new(1, 1)]);

    let mut rectangle = document(3, 2, vec![Bgra::WHITE; 6]);
    rectangle
        .set_selection_mask(selection.clone())
        .expect("selection");
    apply(&mut rectangle, |document| {
        fill_rect(document, LayerId::new(1), rect(0, 0, 3, 2), Bgra::BLACK)
    });
    assert_eq!(
        rectangle.active_layer().surface().pixels(),
        &[
            Bgra::WHITE,
            Bgra::BLACK,
            Bgra::WHITE,
            Bgra::WHITE,
            Bgra::BLACK,
            Bgra::WHITE,
        ]
    );

    let mut ellipse = document(3, 2, vec![Bgra::WHITE; 6]);
    ellipse.set_selection_mask(selection).expect("selection");
    apply(&mut ellipse, |document| {
        fill_ellipse(document, LayerId::new(1), rect(0, 0, 3, 2), Bgra::BLACK)
    });
    let pixels = ellipse.active_layer().surface().pixels();
    assert_eq!(pixels[0], Bgra::WHITE);
    assert_eq!(pixels[1], Bgra::BLACK);
    assert_eq!(pixels[2], Bgra::WHITE);

    let mut gradient = document(3, 1, vec![Bgra::WHITE; 3]);
    gradient
        .set_selection_mask(mask(rect(0, 0, 3, 1), &[Point::new(1, 0)]))
        .expect("selection");
    apply(&mut gradient, |document| {
        linear_gradient(
            document,
            LayerId::new(1),
            Point::new(0, 0),
            Point::new(2, 0),
            Bgra::BLACK,
            Bgra::WHITE,
        )
    });
    let pixels = gradient.active_layer().surface().pixels();
    assert_eq!(pixels[0], Bgra::WHITE);
    assert_ne!(pixels[1], Bgra::WHITE);
    assert_eq!(pixels[2], Bgra::WHITE);

    let red = Bgra::new(200, 20, 20, 255);
    let mut recolored = document(3, 1, vec![Bgra::WHITE, red, Bgra::WHITE]);
    recolored
        .set_selection_mask(mask(
            rect(0, 0, 3, 1),
            &[Point::new(0, 0), Point::new(2, 0)],
        ))
        .expect("selection");
    apply(&mut recolored, |document| {
        recolor(document, LayerId::new(1), Bgra::WHITE, Bgra::BLACK, 0)
    });
    assert_eq!(
        recolored.active_layer().surface().pixels(),
        &[Bgra::BLACK, red, Bgra::BLACK]
    );
}

#[test]
fn translate_moves_only_masked_content_and_leaves_unselected_pixels_alone() {
    let a = Bgra::new(1, 1, 1, 255);
    let b = Bgra::new(2, 2, 2, 255);
    let c = Bgra::new(3, 3, 3, 255);
    let mut document = document(3, 1, vec![a, b, c]);
    document
        .set_selection_mask(mask(rect(1, 0, 2, 1), &[Point::new(1, 0)]))
        .expect("selection");

    apply(&mut document, |document| {
        translate(document, LayerId::new(1), 1, 0)
    });
    assert_eq!(
        document.active_layer().surface().pixels(),
        &[a, Bgra::TRANSPARENT, b]
    );
}

#[test]
fn stamp_composites_only_inside_the_mask() {
    let a = Bgra::new(1, 1, 1, 255);
    let b = Bgra::new(2, 2, 2, 255);
    let c = Bgra::new(3, 3, 3, 255);
    let incoming = Surface::from_pixels(
        size(3, 1),
        vec![
            Bgra::new(10, 10, 10, 255),
            Bgra::new(20, 20, 20, 255),
            Bgra::new(30, 30, 30, 255),
        ],
    )
    .expect("surface");
    let mut document = document(3, 1, vec![a, b, c]);
    document
        .set_selection_mask(mask(rect(0, 0, 3, 1), &[Point::new(0, 0)]))
        .expect("selection");

    apply(&mut document, |document| {
        stamp(document, LayerId::new(1), &incoming, 0, 0)
    });
    assert_eq!(
        document.active_layer().surface().pixels(),
        &[Bgra::new(10, 10, 10, 255), b, c]
    );
}
