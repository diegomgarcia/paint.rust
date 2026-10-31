use paint_app::{AppSession, Command, MagicWandSampling, SelectionCombineMode};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, SelectionMask};
use paint_raster::{BrushOptions, ShapeStyle};

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("test rectangle")
}

fn session_with_color(width: u32, height: u32, color: Bgra) -> AppSession {
    let size = Size::new(width, height).expect("nonzero test dimensions");
    let surface = Surface::filled(size, color).expect("test surface");
    let background = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
    let document = Document::from_layers(size, vec![background]).expect("test document");
    AppSession::new(document)
}

fn pixel(session: &AppSession, x: u32, y: u32) -> Bgra {
    *session.document().layers()[0]
        .surface()
        .pixel(Point::new(x, y))
        .expect("in-bounds pixel")
}

#[test]
fn ellipse_selection_is_a_real_mask_that_clips_fill_and_survives_undo() {
    let mut session = session_with_color(7, 7, Bgra::WHITE);
    assert!(
        session
            .execute(Command::SelectEllipse {
                rect: rect(1, 1, 6, 6),
                mode: SelectionCombineMode::Replace
            })
            .expect("ellipse selection")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(
        session.document().selection(),
        Some(rect(1, 1, 6, 6)),
        "the UI-visible bounds cover the ellipse"
    );
    let mask = session
        .document()
        .selection_mask()
        .expect("non-rectangular mask")
        .clone();
    assert!(mask.contains(Point::new(3, 3)));
    assert!(!mask.contains(Point::new(1, 1)));

    assert!(session.execute(Command::Fill(Bgra::BLACK)).expect("fill"));
    assert_eq!(pixel(&session, 3, 3), Bgra::BLACK);
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);
    assert_eq!(pixel(&session, 1, 1), Bgra::WHITE);
    assert_eq!(pixel(&session, 6, 6), Bgra::WHITE);
    assert!(session.dirty());
    assert!(session.can_undo());

    assert!(session.execute(Command::Undo).expect("undo"));
    assert_eq!(pixel(&session, 3, 3), Bgra::WHITE);
    assert!(!session.dirty());
    assert_eq!(
        session
            .document()
            .selection_mask()
            .map(SelectionMask::selected_count),
        Some(mask.selected_count()),
        "undo restores content while the mask selection remains active"
    );
}

#[test]
fn lasso_selection_masks_freehand_polygon_painting_and_erasing() {
    let mut session = session_with_color(7, 7, Bgra::WHITE);
    let points = vec![Point::new(1, 1), Point::new(6, 1), Point::new(1, 6)];
    assert!(
        session
            .execute(Command::SelectLasso {
                points: points.clone(),
                mode: SelectionCombineMode::Replace,
            })
            .expect("lasso selection")
    );
    let mask = session
        .document()
        .selection_mask()
        .expect("lasso mask")
        .clone();
    assert_eq!(session.document().selection(), Some(rect(1, 1, 7, 7)));
    assert!(mask.contains(Point::new(2, 2)));
    assert!(!mask.contains(Point::new(5, 5)));

    assert!(session.execute(Command::Fill(Bgra::BLACK)).expect("fill"));
    assert_eq!(pixel(&session, 2, 2), Bgra::BLACK);
    assert_eq!(pixel(&session, 5, 5), Bgra::WHITE);
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);
    assert!(session.execute(Command::Undo).expect("undo fill"));
    assert_eq!(pixel(&session, 2, 2), Bgra::WHITE);

    let vertical = [Point::new(0, 3), Point::new(6, 3)];
    assert!(
        session
            .execute(Command::Stroke {
                label: "Eraser",
                points: vertical.to_vec(),
                brush: BrushOptions::new(1, 100, 15),
                color: None,
            })
            .expect("erase")
    );
    assert_eq!(pixel(&session, 2, 3), Bgra::TRANSPARENT);
    assert_eq!(pixel(&session, 5, 3), Bgra::WHITE);
    assert_eq!(pixel(&session, 0, 3), Bgra::WHITE);
    assert!(session.execute(Command::Undo).expect("undo erase"));
    assert_eq!(pixel(&session, 2, 3), Bgra::WHITE);
    assert!(
        session.document().selection_mask().is_some(),
        "selection survives undo"
    );
}

#[test]
fn degenerate_lasso_clears_the_previous_selection() {
    let mut session = session_with_color(4, 4, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(0, 0, 4, 4),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse");
    assert!(session.document().selection_mask().is_some());

    assert!(
        session
            .execute(Command::SelectLasso {
                points: vec![Point::new(1, 1), Point::new(3, 1)],
                mode: SelectionCombineMode::Replace,
            })
            .expect("degenerate lasso")
    );
    assert_eq!(session.document().selection(), None);
    assert!(session.document().selection_mask().is_none());
}

#[test]
fn magic_wand_uses_flood_fill_tolerance_and_contiguous_or_global_modes() {
    let mut session = session_with_color(5, 3, Bgra::WHITE);
    session
        .execute(Command::FillRect {
            rect: rect(2, 0, 3, 3),
            color: Bgra::BLACK,
            style: ShapeStyle::Fill,
            border_width: 0,
        })
        .expect("stripe");

    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(0, 0),
                tolerance: 0,
                global: false,
                mode: SelectionCombineMode::Replace,
                sampling: MagicWandSampling::ActiveLayer,
            })
            .expect("contiguous magic wand")
    );
    assert_eq!(session.document().selection(), Some(rect(0, 0, 2, 3)));
    let mask = session
        .document()
        .selection_mask()
        .expect("wand mask")
        .clone();
    assert!(mask.contains(Point::new(1, 2)));
    assert!(!mask.contains(Point::new(2, 1)));
    assert!(!mask.contains(Point::new(3, 1)));

    assert!(
        session
            .execute(Command::Fill(Bgra::new(10, 20, 30, 255)))
            .expect("fill contiguous region")
    );
    assert_eq!(pixel(&session, 0, 0), Bgra::new(10, 20, 30, 255));
    assert_eq!(pixel(&session, 2, 0), Bgra::BLACK);
    assert_eq!(pixel(&session, 4, 0), Bgra::WHITE);
    session.execute(Command::Undo).expect("undo fill");

    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(0, 0),
                tolerance: 0,
                global: true,
                mode: SelectionCombineMode::Replace,
                sampling: MagicWandSampling::ActiveLayer,
            })
            .expect("global magic wand")
    );
    assert_eq!(session.document().selection(), Some(rect(0, 0, 5, 3)));
    let mask = session
        .document()
        .selection_mask()
        .expect("global wand mask")
        .clone();
    assert!(mask.contains(Point::new(4, 2)));
    assert!(!mask.contains(Point::new(2, 1)));
    session
        .execute(Command::Fill(Bgra::new(10, 20, 30, 255)))
        .expect("fill global region");
    assert_eq!(pixel(&session, 0, 0), Bgra::new(10, 20, 30, 255));
    assert_eq!(pixel(&session, 4, 2), Bgra::new(10, 20, 30, 255));
    assert_eq!(pixel(&session, 2, 1), Bgra::BLACK);
}

#[test]
fn magic_wand_tolerance_expands_the_selected_color_range() {
    let near_white = Bgra::new(245, 245, 245, 255);
    let mut session = session_with_color(3, 1, Bgra::WHITE);
    session
        .execute(Command::FillRect {
            rect: rect(1, 0, 2, 1),
            color: near_white,
            style: ShapeStyle::Fill,
            border_width: 0,
        })
        .expect("near-white pixel");

    session
        .execute(Command::SelectMagicWand {
            point: Point::new(0, 0),
            tolerance: 0,
            global: true,
            mode: SelectionCombineMode::Replace,
            sampling: MagicWandSampling::ActiveLayer,
        })
        .expect("exact wand");
    assert!(
        !session
            .document()
            .selection_mask()
            .expect("mask")
            .contains(Point::new(1, 0))
    );

    session
        .execute(Command::SelectMagicWand {
            point: Point::new(0, 0),
            tolerance: 16,
            global: true,
            mode: SelectionCombineMode::Replace,
            sampling: MagicWandSampling::ActiveLayer,
        })
        .expect("tolerant wand");
    assert!(
        session
            .document()
            .selection_mask()
            .expect("mask")
            .contains(Point::new(1, 0))
    );
}

#[test]
fn selection_commands_replace_each_other_without_dirtying_or_entering_history() {
    let mut session = session_with_color(5, 5, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(0, 0, 5, 5),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse");
    assert!(session.document().selection_mask().is_some());
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectLasso {
            points: vec![Point::new(0, 0), Point::new(4, 0), Point::new(0, 4)],
            mode: SelectionCombineMode::Replace,
        })
        .expect("lasso");
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectMagicWand {
            point: Point::new(0, 0),
            tolerance: 0,
            global: false,
            mode: SelectionCombineMode::Replace,
            sampling: MagicWandSampling::ActiveLayer,
        })
        .expect("wand");
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectRect {
            rect: rect(1, 1, 3, 3),
            mode: SelectionCombineMode::Replace,
        })
        .expect("rectangle replaces mask");
    assert!(session.document().selection_mask().is_none());
    assert_eq!(session.document().selection(), Some(rect(1, 1, 3, 3)));

    assert!(
        session
            .execute(Command::ClearSelection)
            .expect("clear selection")
    );
    assert_eq!(session.document().selection(), None);
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn translating_a_mask_selection_moves_bounds_and_undo_restores_content_and_selection() {
    let a = Bgra::new(1, 1, 1, 255);
    let b = Bgra::new(2, 2, 2, 255);
    let c = Bgra::new(3, 3, 3, 255);
    let size = Size::new(3, 1).expect("size");
    let mut document = Document::from_layers(
        size,
        vec![Layer::new(
            LayerId::new(1),
            "pixels",
            Surface::from_pixels(size, vec![a, b, c]).expect("surface"),
        )],
    )
    .expect("document");
    let mut selected = SelectionMask::empty(rect(1, 0, 2, 1)).expect("mask");
    selected.set_xy(1, 0, true).expect("selected pixel");
    document.set_selection_mask(selected).expect("selection");
    let mut session = AppSession::new(document);

    assert!(
        session
            .execute(Command::Move { dx: 1, dy: 0 })
            .expect("move selected content")
    );
    assert_eq!(pixel(&session, 0, 0), a);
    assert_eq!(pixel(&session, 1, 0), Bgra::TRANSPARENT);
    assert_eq!(pixel(&session, 2, 0), b);
    assert_eq!(session.document().selection(), Some(rect(2, 0, 3, 1)));
    assert!(
        session
            .document()
            .selection_mask()
            .expect("mask moved")
            .contains(Point::new(2, 0))
    );

    assert!(session.execute(Command::Undo).expect("undo move"));
    assert_eq!(pixel(&session, 0, 0), a);
    assert_eq!(pixel(&session, 1, 0), b);
    assert_eq!(pixel(&session, 2, 0), c);
    assert_eq!(session.document().selection(), Some(rect(1, 0, 2, 1)));
    assert!(
        session
            .document()
            .selection_mask()
            .expect("mask restored")
            .contains(Point::new(1, 0))
    );
    assert!(!session.dirty());
}

#[test]
fn translate_selection_command_moves_a_mask_without_content_history() {
    let mut session = session_with_color(5, 5, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(0, 0, 4, 4),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse");
    let before = session
        .document()
        .selection_mask()
        .expect("mask")
        .selected_count();

    assert!(
        session
            .execute(Command::TranslateSelection { dx: 2, dy: 1 })
            .expect("translate selection")
    );
    assert_eq!(session.document().selection(), Some(rect(2, 1, 6, 5)));
    assert!(
        session
            .document()
            .selection_mask()
            .expect("mask")
            .contains(Point::new(3, 2))
    );
    assert_eq!(
        session
            .document()
            .selection_mask()
            .expect("mask")
            .selected_count(),
        before
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn rectangle_combine_modes_keep_rectangles_and_follow_set_algebra() {
    let mut session = session_with_color(8, 8, Bgra::WHITE);
    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(0, 0, 2, 2),
                mode: SelectionCombineMode::Replace,
            })
            .expect("replace selection")
    );
    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(2, 0, 4, 2),
                mode: SelectionCombineMode::Union,
            })
            .expect("union selection")
    );
    assert_eq!(session.document().selection(), Some(rect(0, 0, 4, 2)));
    assert!(session.document().selection_mask().is_none());

    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(2, 0, 4, 2),
                mode: SelectionCombineMode::Subtract,
            })
            .expect("subtract selection")
    );
    assert_eq!(session.document().selection(), Some(rect(0, 0, 2, 2)));
    assert!(session.document().selection_mask().is_none());

    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(1, 1, 4, 4),
                mode: SelectionCombineMode::Intersect,
            })
            .expect("intersect selection")
    );
    assert_eq!(session.document().selection(), Some(rect(1, 1, 2, 2)));
    assert!(session.document().selection_mask().is_none());

    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(2, 1, 3, 2),
                mode: SelectionCombineMode::Xor,
            })
            .expect("xor selection")
    );
    assert_eq!(session.document().selection(), Some(rect(1, 1, 3, 2)));
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn ellipse_combine_modes_match_pixel_mask_algebra() {
    let first = SelectionMask::ellipse(rect(0, 0, 4, 4)).expect("first ellipse");
    let second = SelectionMask::ellipse(rect(2, 0, 6, 4)).expect("second ellipse");
    for (mode, expected) in [
        (SelectionCombineMode::Replace, Some(second.clone())),
        (
            SelectionCombineMode::Union,
            first.union(&second).expect("union"),
        ),
        (
            SelectionCombineMode::Subtract,
            first.subtract(&second).expect("subtract"),
        ),
        (
            SelectionCombineMode::Intersect,
            first.intersect(&second).expect("intersect"),
        ),
        (SelectionCombineMode::Xor, first.xor(&second).expect("xor")),
    ] {
        let mut session = session_with_color(9, 9, Bgra::WHITE);
        assert!(
            session
                .execute(Command::SelectEllipse {
                    rect: rect(0, 0, 4, 4),
                    mode: SelectionCombineMode::Replace,
                })
                .expect("first ellipse")
        );
        assert!(
            session
                .execute(Command::SelectEllipse {
                    rect: rect(2, 0, 6, 4),
                    mode,
                })
                .expect("combined ellipse")
        );
        assert_eq!(
            session.document().selection_mask(),
            expected.as_ref(),
            "{mode:?} mismatch"
        );
        assert!(!session.dirty());
        assert!(!session.can_undo());
    }
}

#[test]
fn lasso_and_magic_wand_combine_against_the_existing_selection() {
    let mut session = session_with_color(6, 3, Bgra::WHITE);
    session
        .execute(Command::FillRect {
            rect: rect(3, 0, 4, 3),
            color: Bgra::BLACK,
            style: ShapeStyle::Fill,
            border_width: 0,
        })
        .expect("stripe");
    session
        .execute(Command::SelectRect {
            rect: rect(0, 0, 2, 3),
            mode: SelectionCombineMode::Replace,
        })
        .expect("rectangle base");
    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(5, 0),
                tolerance: 0,
                global: false,
                mode: SelectionCombineMode::Union,
                sampling: MagicWandSampling::ActiveLayer,
            })
            .expect("wand union")
    );
    let mask = session
        .document()
        .selection_mask()
        .expect("combined mask")
        .clone();
    assert_eq!(mask.selected_count(), 12);
    assert!(mask.contains(Point::new(0, 0)));
    assert!(mask.contains(Point::new(4, 2)));
    assert!(!mask.contains(Point::new(2, 0)));
    assert!(!mask.contains(Point::new(3, 0)));

    assert!(
        session
            .execute(Command::SelectLasso {
                points: vec![
                    Point::new(0, 0),
                    Point::new(6, 0),
                    Point::new(6, 1),
                    Point::new(0, 1),
                ],
                mode: SelectionCombineMode::Subtract,
            })
            .expect("lasso subtract")
    );
    let mask = session
        .document()
        .selection_mask()
        .expect("remaining mask")
        .clone();
    assert_eq!(mask.selected_count(), 8);
    assert!(!mask.contains(Point::new(0, 0)));
    assert!(!mask.contains(Point::new(4, 0)));
    assert!(mask.contains(Point::new(0, 2)));
    assert!(mask.contains(Point::new(4, 2)));
}

#[test]
fn magic_wand_image_sampling_uses_the_composed_surface() {
    let size = Size::new(3, 1).expect("size");
    let bottom = Layer::new(
        LayerId::new(1),
        "bottom",
        Surface::from_pixels(size, vec![Bgra::BLACK, Bgra::WHITE, Bgra::BLACK])
            .expect("bottom surface"),
    );
    let top = Layer::new(
        LayerId::new(2),
        "top",
        Surface::new(size).expect("transparent top"),
    );
    let mut document = Document::from_layers(size, vec![bottom, top]).expect("layered document");
    document
        .set_active_layer(LayerId::new(2))
        .expect("active top layer");
    let mut session = AppSession::new(document);

    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(0, 0),
                tolerance: 0,
                global: false,
                mode: SelectionCombineMode::Replace,
                sampling: MagicWandSampling::ActiveLayer,
            })
            .expect("active-layer wand")
    );
    let active = session
        .document()
        .selection_mask()
        .expect("active-layer mask")
        .clone();
    assert_eq!(active.selected_count(), 3);

    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(0, 0),
                tolerance: 0,
                global: false,
                mode: SelectionCombineMode::Replace,
                sampling: MagicWandSampling::Image,
            })
            .expect("image wand")
    );
    let image = session
        .document()
        .selection_mask()
        .expect("image mask")
        .clone();
    assert_eq!(image.selected_count(), 1);
    assert!(image.contains(Point::new(0, 0)));
    assert!(!image.contains(Point::new(1, 0)));
    assert_eq!(session.document().selection(), Some(rect(0, 0, 1, 1)));
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn select_all_and_invert_selection_are_selection_only_commands() {
    let mut session = session_with_color(8, 6, Bgra::WHITE);
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(session.execute(Command::SelectAll).expect("select all"));
    assert_eq!(
        session.document().selection(),
        Some(rect(0, 0, 8, 6)),
        "Select All covers the complete canvas"
    );
    assert!(
        !session.dirty(),
        "selection changes never dirty the document"
    );
    assert!(
        !session.can_undo(),
        "selection changes stay out of content history"
    );

    assert!(
        session
            .execute(Command::SelectRect {
                rect: rect(2, 1, 5, 4),
                mode: SelectionCombineMode::Replace,
            })
            .expect("inner rectangle")
    );
    assert!(session.execute(Command::InvertSelection).expect("invert"));
    let inverted = session
        .document()
        .selection_model()
        .expect("inverted selection");
    assert!(inverted.contains_xy(0, 0));
    assert!(inverted.contains_xy(7, 5));
    assert!(!inverted.contains_xy(2, 1));
    assert!(!inverted.contains_xy(4, 3));
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(
        session
            .execute(Command::InvertSelection)
            .expect("invert again")
    );
    let restored = session
        .document()
        .selection_model()
        .expect("double inverted selection");
    assert!(!restored.contains_xy(0, 0));
    assert!(restored.contains_xy(2, 1));
    assert!(restored.contains_xy(4, 3));

    assert!(session.execute(Command::ClearSelection).expect("deselect"));
    assert!(
        !session
            .execute(Command::InvertSelection)
            .expect("nothing to invert")
    );
    assert!(
        session
            .execute(Command::SelectAll)
            .expect("select all again"),
        "Select All remains available without a previous selection"
    );
}

#[test]
fn invert_selection_complements_lasso_masks_and_clips_to_the_canvas() {
    let mut session = session_with_color(6, 6, Bgra::WHITE);
    session
        .execute(Command::SelectLasso {
            points: vec![Point::new(1, 1), Point::new(5, 1), Point::new(5, 5)],
            mode: SelectionCombineMode::Replace,
        })
        .expect("lasso selection");
    assert!(session.document().selection_mask().is_some());

    assert!(session.execute(Command::InvertSelection).expect("invert"));
    let selection = session.document().selection_model().expect("inverted mask");
    assert!(
        selection.mask().is_some(),
        "the lasso complement stays a mask"
    );
    assert!(selection.contains_xy(0, 0));
    assert!(selection.contains_xy(5, 5));
    assert!(!selection.contains_xy(2, 2));
    assert_eq!(
        selection.bounds(),
        rect(0, 0, 6, 6),
        "the complement is clipped to the canvas"
    );
}

#[test]
fn erase_selection_clears_pixels_and_selection_then_undo_restores_both() {
    let mut session = session_with_color(6, 4, Bgra::WHITE);
    session
        .execute(Command::SelectRect {
            rect: rect(1, 1, 4, 3),
            mode: SelectionCombineMode::Replace,
        })
        .expect("selection");
    session
        .execute(Command::FillSelection(Bgra::BLACK))
        .expect("fill selection");
    assert_eq!(pixel(&session, 2, 2), Bgra::BLACK);
    assert!(session.can_undo());

    assert!(
        session
            .execute(Command::EraseSelection)
            .expect("erase selection")
    );
    assert_eq!(pixel(&session, 2, 2), Bgra::TRANSPARENT);
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);
    assert!(
        session.document().selection().is_none(),
        "Erase Selection deselects like Paint.NET"
    );

    assert!(session.execute(Command::Undo).expect("undo erase"));
    assert_eq!(pixel(&session, 2, 2), Bgra::BLACK);
    assert_eq!(
        session.document().selection(),
        Some(rect(1, 1, 4, 3)),
        "undo restores the selection with the pixels"
    );
}

#[test]
fn erase_selection_without_a_selection_is_a_no_op_without_history() {
    let mut session = session_with_color(4, 4, Bgra::WHITE);
    assert!(
        !session
            .execute(Command::EraseSelection)
            .expect("no selection")
    );
    assert!(!session.can_undo());
    assert!(!session.dirty());
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);
}

#[test]
fn fill_selection_requires_a_selection_and_is_undoable() {
    let mut session = session_with_color(5, 5, Bgra::WHITE);
    assert!(
        !session
            .execute(Command::FillSelection(Bgra::BLACK))
            .expect("fill without selection")
    );
    assert!(!session.can_undo());
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);

    session
        .execute(Command::SelectRect {
            rect: rect(1, 1, 3, 3),
            mode: SelectionCombineMode::Replace,
        })
        .expect("selection");
    assert!(
        session
            .execute(Command::FillSelection(Bgra::BLACK))
            .expect("fill selection")
    );
    assert_eq!(pixel(&session, 1, 1), Bgra::BLACK);
    assert_eq!(pixel(&session, 2, 2), Bgra::BLACK);
    assert_eq!(pixel(&session, 0, 0), Bgra::WHITE);
    assert_eq!(pixel(&session, 4, 4), Bgra::WHITE);

    assert!(session.execute(Command::Undo).expect("undo fill"));
    assert_eq!(pixel(&session, 1, 1), Bgra::WHITE);
    assert_eq!(
        session.document().selection(),
        Some(rect(1, 1, 3, 3)),
        "Fill Selection leaves the selection installed"
    );
}
