use paint_core::{Point, Rect, Size};
use paint_document::{Document, Selection, SelectionMask};

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("valid test rectangle")
}

#[test]
fn ellipse_mask_uses_pixel_centers_and_a_tight_bounding_rectangle() {
    let ellipse = rect(0, 0, 5, 5);
    let mask = SelectionMask::ellipse(ellipse).expect("ellipse mask");

    assert_eq!(mask.bounds(), ellipse);
    assert!(mask.contains(Point::new(2, 2)), "center is selected");
    assert!(
        !mask.contains(Point::new(0, 0)),
        "the corner lies outside the inscribed ellipse"
    );
    assert!(
        mask.contains(Point::new(0, 2)),
        "the left edge midpoint is inside the pixel-center ellipse"
    );
    assert!(
        !mask.is_empty(),
        "a nonzero ellipse always selects at least its center pixel"
    );
    assert!(
        mask.selected_count() < 25,
        "an ellipse must not select its complete bounding rectangle"
    );
}

#[test]
fn polygon_mask_fills_an_implicitly_closed_lasso_path() {
    let points = [Point::new(0, 0), Point::new(5, 0), Point::new(0, 5)];
    let mask = SelectionMask::polygon(&points)
        .expect("polygon mask")
        .expect("triangle has area");

    assert_eq!(mask.bounds(), rect(0, 0, 6, 6));
    assert!(mask.contains(Point::new(0, 0)));
    assert!(mask.contains(Point::new(1, 1)));
    assert!(!mask.contains(Point::new(4, 4)));
    assert!(!mask.is_empty());
}

#[test]
fn degenerate_polygons_select_nothing() {
    assert!(
        SelectionMask::polygon(&[Point::new(1, 1), Point::new(4, 1)])
            .expect("short lasso")
            .is_none()
    );
    assert!(
        SelectionMask::polygon(&[Point::new(1, 1), Point::new(4, 1), Point::new(7, 1),])
            .expect("collinear lasso")
            .is_none()
    );
}

#[test]
fn masks_translate_bits_and_bounds_without_losing_shape() {
    let mut mask = SelectionMask::empty(rect(0, 0, 3, 2)).expect("mask");
    mask.set_xy(0, 0, true).expect("set selected");
    mask.set_xy(2, 1, true).expect("set selected");

    let moved = mask.translated(3, -1).expect("translate mask");
    assert_eq!(moved.bounds(), rect(3, -1, 6, 1));
    assert!(moved.contains_xy(3, -1));
    assert!(moved.contains_xy(5, 0));
    assert!(!moved.contains_xy(0, 0));
    assert!(!moved.contains_xy(4, -1));
    assert_eq!(moved.selected_count(), 2);
}

#[test]
fn document_keeps_rect_selection_semantics_and_exposes_mask_state() {
    let mut document = Document::new(Size::new(4, 4).expect("size")).expect("document");
    let rectangle = rect(-2, -1, 3, 2);
    assert!(
        document
            .set_selection(Some(rectangle))
            .expect("rect selection")
    );
    assert_eq!(document.selection(), Some(rectangle));
    assert!(document.selection_mask().is_none());
    assert!(
        document
            .selection_model()
            .expect("selection")
            .contains_xy(0, 0)
    );

    let mask = SelectionMask::ellipse(rect(0, 0, 4, 4)).expect("mask");
    assert!(document.set_selection_mask(mask.clone()).expect("mask set"));
    assert_eq!(document.selection(), Some(mask.bounds()));
    assert_eq!(
        document.selection_mask().map(SelectionMask::selected_count),
        Some(mask.selected_count())
    );
    let selection = document.selection_model().expect("selection").clone();
    assert_eq!(selection, Selection::from_mask(mask));
    assert!(
        !document
            .set_selection_mask(selection.mask().expect("mask").clone())
            .expect("no-op mask set")
    );

    assert!(
        document
            .set_selection(Some(rect(0, 0, 2, 2)))
            .expect("replace with rect")
    );
    assert!(
        document.selection_mask().is_none(),
        "a rectangle selection replaces any mask"
    );
    assert!(document.clear_selection().expect("clear"));
    assert_eq!(document.selection(), None);
    assert!(document.selection_model().is_none());
}

#[test]
fn partially_off_canvas_ellipse_keeps_its_original_geometry() {
    let mask =
        SelectionMask::ellipse_clipped(rect(-5, 0, 5, 10), Size::new(5, 10).expect("canvas size"))
            .expect("ellipse clip")
            .expect("overlapping ellipse");
    assert_eq!(mask.bounds(), rect(0, 0, 5, 10));
    assert!(
        mask.contains(Point::new(0, 0)),
        "the true ellipse reaches the clipped canvas corner"
    );
    assert!(
        mask.contains(Point::new(2, 5)),
        "the ellipse center row remains selected"
    );

    assert!(
        SelectionMask::ellipse_clipped(rect(10, 10, 20, 20), Size::new(5, 5).expect("canvas size"))
            .expect("outside ellipse")
            .is_none()
    );
}
