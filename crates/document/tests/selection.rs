use paint_core::{Point, Rect, Size};
use paint_document::{Document, Selection, SelectionCombineMode, SelectionMask};

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("valid test rectangle")
}

fn mask(bounds: Rect, selected: &[(i64, i64)]) -> SelectionMask {
    let mut mask = SelectionMask::empty(bounds).expect("empty mask");
    for &(x, y) in selected {
        mask.set_xy(x, y, true).expect("selected pixel");
    }
    mask
}

fn selected_bits(mask: &SelectionMask, bounds: Rect) -> Vec<bool> {
    (bounds.top()..bounds.bottom())
        .flat_map(|y| (bounds.left()..bounds.right()).map(move |x| mask.contains_xy(x, y)))
        .collect()
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

#[test]
fn mask_algebra_matches_set_operations_for_each_combine_mode() {
    let bounds = rect(0, 0, 4, 1);
    let base = mask(bounds, &[(0, 0), (1, 0)]);
    let addition = mask(bounds, &[(1, 0), (2, 0)]);

    let union = base
        .union(&addition)
        .expect("union")
        .expect("union is not empty");
    assert_eq!(selected_bits(&union, bounds), [true, true, true, false]);
    assert_eq!(union.selected_count(), 3);
    assert_eq!(union.bounds(), rect(0, 0, 3, 1));

    let intersect = base
        .intersect(&addition)
        .expect("intersect")
        .expect("shared pixel");
    assert_eq!(
        selected_bits(&intersect, bounds),
        [false, true, false, false]
    );
    assert_eq!(intersect.bounds(), rect(1, 0, 2, 1));

    let subtract = base
        .subtract(&addition)
        .expect("subtract")
        .expect("remaining pixel");
    assert_eq!(
        selected_bits(&subtract, bounds),
        [true, false, false, false]
    );
    assert_eq!(subtract.bounds(), rect(0, 0, 1, 1));

    let xor = base.xor(&addition).expect("xor").expect("xor is not empty");
    assert_eq!(selected_bits(&xor, bounds), [true, false, true, false]);
    assert_eq!(xor.bounds(), rect(0, 0, 3, 1));
}

#[test]
fn mask_algebra_handles_empty_full_and_disjoint_masks() {
    let bounds = rect(0, 0, 4, 4);
    let empty = SelectionMask::empty(bounds).expect("empty mask");
    let full = SelectionMask::full(bounds).expect("full mask");
    let partial = mask(rect(0, 0, 2, 2), &[(0, 0), (1, 1)]);

    assert_eq!(empty.union(&partial).expect("union"), Some(partial.clone()));
    assert_eq!(partial.union(&empty).expect("union"), Some(partial.clone()));
    assert!(empty.intersect(&partial).expect("intersect").is_none());
    assert!(empty.subtract(&partial).expect("subtract").is_none());
    assert_eq!(
        partial.subtract(&empty).expect("subtract"),
        Some(partial.clone())
    );

    assert_eq!(full.union(&partial).expect("union"), Some(full.clone()));
    assert_eq!(
        full.intersect(&partial).expect("intersect"),
        Some(partial.clone())
    );
    assert!(full.subtract(&full).expect("subtract").is_none());
    assert!(full.xor(&full).expect("xor").is_none());
    let complement = full
        .subtract(&partial)
        .expect("subtract")
        .expect("complement is not empty");
    assert_eq!(
        complement.selected_count(),
        full.selected_count() - partial.selected_count()
    );
    assert!(!complement.contains_xy(0, 0));
    assert!(complement.contains_xy(2, 2));
    assert_eq!(full.xor(&empty).expect("xor"), Some(full));

    let left = mask(rect(0, 0, 2, 2), &[(0, 0), (1, 1)]);
    let right = mask(rect(2, 0, 4, 2), &[(3, 0)]);
    let disjoint_union = left.union(&right).expect("union").expect("disjoint union");
    assert_eq!(disjoint_union.selected_count(), 3);
    assert_eq!(disjoint_union.bounds(), rect(0, 0, 4, 2));
    assert!(disjoint_union.contains_xy(3, 0));
    assert!(!disjoint_union.contains_xy(2, 1));
    assert!(left.intersect(&right).expect("intersect").is_none());
    assert_eq!(left.subtract(&right).expect("subtract"), Some(left.clone()));
    assert_eq!(left.xor(&right).expect("xor"), Some(disjoint_union.clone()));
}

#[test]
fn rectangle_combine_keeps_the_rectangle_fast_path_and_falls_back_to_masks() {
    let canvas = Size::new(10, 10).expect("canvas");
    let base = Selection::rectangle(rect(0, 0, 4, 4));
    let inner = Selection::rectangle(rect(1, 1, 3, 3));

    let union = Selection::combine(
        Some(&base),
        Some(&inner),
        SelectionCombineMode::Union,
        canvas,
    )
    .expect("union")
    .expect("union selection");
    assert!(union.is_rectangular());
    assert_eq!(union.bounds(), base.bounds());

    let intersect = Selection::combine(
        Some(&base),
        Some(&inner),
        SelectionCombineMode::Intersect,
        canvas,
    )
    .expect("intersect")
    .expect("intersection selection");
    assert!(intersect.is_rectangular());
    assert_eq!(intersect.bounds(), inner.bounds());

    let subtract = Selection::combine(
        Some(&base),
        Some(&inner),
        SelectionCombineMode::Subtract,
        canvas,
    )
    .expect("subtract")
    .expect("subtracted selection");
    assert!(!subtract.is_rectangular());
    assert!(subtract.contains(Point::new(0, 0)));
    assert!(!subtract.contains(Point::new(1, 1)));
    assert!(subtract.contains(Point::new(3, 3)));
    assert_eq!(subtract.bounds(), base.bounds());

    let xor = Selection::combine(Some(&base), Some(&inner), SelectionCombineMode::Xor, canvas)
        .expect("xor")
        .expect("xor selection");
    assert!(!xor.is_rectangular());
    assert!(xor.contains(Point::new(0, 0)));
    assert!(!xor.contains(Point::new(1, 1)));
    assert!(xor.contains(Point::new(3, 3)));

    let left = Selection::rectangle(rect(0, 0, 2, 2));
    let touching = Selection::rectangle(rect(2, 0, 4, 2));
    let merged = Selection::combine(
        Some(&left),
        Some(&touching),
        SelectionCombineMode::Union,
        canvas,
    )
    .expect("union")
    .expect("merged selection");
    assert!(merged.is_rectangular());
    assert_eq!(merged.bounds(), rect(0, 0, 4, 2));

    let gap = Selection::rectangle(rect(3, 0, 5, 2));
    let split = Selection::combine(Some(&left), Some(&gap), SelectionCombineMode::Union, canvas)
        .expect("union")
        .expect("split selection");
    assert!(!split.is_rectangular());
    assert!(split.contains(Point::new(0, 0)));
    assert!(!split.contains(Point::new(2, 0)));
    assert!(split.contains(Point::new(3, 0)));

    let band = Selection::rectangle(rect(2, 0, 6, 4));
    let remaining = Selection::combine(
        Some(&base),
        Some(&band),
        SelectionCombineMode::Subtract,
        canvas,
    )
    .expect("subtract")
    .expect("remaining selection");
    assert!(remaining.is_rectangular());
    assert_eq!(remaining.bounds(), rect(0, 0, 2, 4));

    let outside = Selection::rectangle(rect(8, 8, 9, 9));
    assert!(
        Selection::combine(
            Some(&base),
            Some(&outside),
            SelectionCombineMode::Intersect,
            canvas,
        )
        .expect("intersect")
        .is_none()
    );
    let untouched = Selection::combine(
        Some(&base),
        Some(&outside),
        SelectionCombineMode::Subtract,
        canvas,
    )
    .expect("subtract")
    .expect("untouched selection");
    assert!(untouched.is_rectangular());
    assert_eq!(untouched.bounds(), base.bounds());
}

#[test]
fn rectangle_union_with_a_mask_inside_keeps_the_rectangle() {
    let canvas = Size::new(10, 10).expect("canvas");
    let base = Selection::rectangle(rect(0, 0, 6, 6));
    let inner = Selection::from_mask(SelectionMask::ellipse(rect(1, 1, 5, 5)).expect("ellipse"));

    let union = Selection::combine(
        Some(&base),
        Some(&inner),
        SelectionCombineMode::Union,
        canvas,
    )
    .expect("union")
    .expect("union selection");
    assert!(union.is_rectangular());
    assert_eq!(union.bounds(), base.bounds());

    let intersect = Selection::combine(
        Some(&base),
        Some(&inner),
        SelectionCombineMode::Intersect,
        canvas,
    )
    .expect("intersect")
    .expect("intersection selection");
    assert_eq!(intersect, inner);
}

#[test]
fn combine_without_a_base_or_without_an_addition_follows_set_algebra() {
    let canvas = Size::new(10, 10).expect("canvas");
    let addition = Selection::rectangle(rect(0, 0, 2, 2));
    for mode in [
        SelectionCombineMode::Replace,
        SelectionCombineMode::Union,
        SelectionCombineMode::Xor,
    ] {
        assert_eq!(
            Selection::combine(None, Some(&addition), mode, canvas).expect("combine"),
            Some(addition.clone()),
            "{mode:?} with no base keeps the addition"
        );
    }
    for mode in [
        SelectionCombineMode::Subtract,
        SelectionCombineMode::Intersect,
    ] {
        assert!(
            Selection::combine(None, Some(&addition), mode, canvas)
                .expect("combine")
                .is_none(),
            "{mode:?} with no base selects nothing"
        );
    }

    let base = Selection::rectangle(rect(1, 1, 4, 4));
    assert!(
        Selection::combine(Some(&base), None, SelectionCombineMode::Replace, canvas)
            .expect("combine")
            .is_none()
    );
    assert!(
        Selection::combine(Some(&base), None, SelectionCombineMode::Intersect, canvas)
            .expect("combine")
            .is_none()
    );
    for mode in [
        SelectionCombineMode::Union,
        SelectionCombineMode::Subtract,
        SelectionCombineMode::Xor,
    ] {
        assert_eq!(
            Selection::combine(Some(&base), None, mode, canvas).expect("combine"),
            Some(base.clone()),
            "{mode:?} with an empty addition keeps the base"
        );
    }
}

#[test]
fn document_combine_selection_applies_modes_and_reports_real_changes() {
    let mut document = Document::new(Size::new(6, 6).expect("document size")).expect("document");
    let first = Selection::rectangle(rect(0, 0, 2, 2));
    assert!(
        document
            .combine_selection(Some(first.clone()), SelectionCombineMode::Replace)
            .expect("replace")
    );
    assert_eq!(document.selection(), Some(first.bounds()));
    assert!(
        !document
            .combine_selection(Some(first), SelectionCombineMode::Replace)
            .expect("repeat replace"),
        "an identical replacement is a no-op"
    );

    let adjacent = Selection::rectangle(rect(2, 0, 4, 2));
    assert!(
        document
            .combine_selection(Some(adjacent), SelectionCombineMode::Union)
            .expect("union")
    );
    assert_eq!(document.selection(), Some(rect(0, 0, 4, 2)));

    let band = Selection::rectangle(rect(1, 0, 6, 2));
    assert!(
        document
            .combine_selection(Some(band), SelectionCombineMode::Subtract)
            .expect("subtract")
    );
    assert_eq!(document.selection(), Some(rect(0, 0, 1, 2)));

    let outside = Selection::rectangle(rect(4, 4, 6, 6));
    assert!(
        document
            .combine_selection(Some(outside), SelectionCombineMode::Intersect)
            .expect("intersect")
    );
    assert_eq!(document.selection(), None);

    assert!(
        !document
            .combine_selection(None, SelectionCombineMode::Union)
            .expect("empty union"),
        "an empty addition with no selection changes nothing"
    );
}

#[test]
fn mask_algebra_crosses_word_and_signed_boundaries() {
    let base = mask(rect(-70, -2, 70, 2), &[(-70, -2), (0, 0), (69, 1)]);
    let other = mask(
        rect(-10, -2, 130, 2),
        &[(-10, -2), (0, 0), (75, 1), (129, 1)],
    );

    let union = base.union(&other).expect("union").expect("union pixels");
    assert_eq!(union.selected_count(), 6);
    for point in [(-70, -2), (-10, -2), (0, 0), (69, 1), (75, 1), (129, 1)] {
        assert!(
            union.contains_xy(point.0, point.1),
            "union misses {point:?}"
        );
    }

    let intersect = base
        .intersect(&other)
        .expect("intersect")
        .expect("shared pixel");
    assert_eq!(intersect.selected_count(), 1);
    assert!(intersect.contains_xy(0, 0));

    let subtract = base.subtract(&other).expect("subtract").expect("remainder");
    assert_eq!(subtract.selected_count(), 2);
    assert!(subtract.contains_xy(-70, -2));
    assert!(subtract.contains_xy(69, 1));
    assert!(!subtract.contains_xy(0, 0));

    let xor = base.xor(&other).expect("xor").expect("xor pixels");
    assert_eq!(xor.selected_count(), 5);
    assert!(xor.contains_xy(-70, -2));
    assert!(xor.contains_xy(129, 1));
    assert!(!xor.contains_xy(0, 0));
}

#[test]
fn selection_row_runs_match_the_packed_mask_across_word_boundaries() {
    let mut mask = SelectionMask::empty(Rect::new(-5, 3, 130, 4).expect("bounds")).expect("mask");
    for (start, end) in [(0, 3), (63, 70), (127, 130)] {
        for x in start..end {
            mask.set_xy(x - 5, 3, true).expect("set pixel");
        }
    }
    let runs = mask.row_runs(3);
    assert_eq!(runs, vec![(-5, -2), (58, 65), (122, 125)]);
    assert!(mask.row_runs(4).is_empty());
    for (start, end) in runs {
        for x in start..end {
            assert!(mask.contains_xy(x, 3));
        }
    }
}
