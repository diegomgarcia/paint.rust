use paint_app::{AppSession, Command};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, SelectionMask};

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
            .execute(Command::SelectEllipse(rect(1, 1, 6, 6)))
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
            .execute(Command::SelectLasso(points.clone()))
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
                size: 1,
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
        .execute(Command::SelectEllipse(rect(0, 0, 4, 4)))
        .expect("ellipse");
    assert!(session.document().selection_mask().is_some());

    assert!(
        session
            .execute(Command::SelectLasso(vec![
                Point::new(1, 1),
                Point::new(3, 1),
            ]))
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
        })
        .expect("stripe");

    assert!(
        session
            .execute(Command::SelectMagicWand {
                point: Point::new(0, 0),
                tolerance: 0,
                global: false,
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
        })
        .expect("near-white pixel");

    session
        .execute(Command::SelectMagicWand {
            point: Point::new(0, 0),
            tolerance: 0,
            global: true,
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
        .execute(Command::SelectEllipse(rect(0, 0, 5, 5)))
        .expect("ellipse");
    assert!(session.document().selection_mask().is_some());
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectLasso(vec![
            Point::new(0, 0),
            Point::new(4, 0),
            Point::new(0, 4),
        ]))
        .expect("lasso");
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectMagicWand {
            point: Point::new(0, 0),
            tolerance: 0,
            global: false,
        })
        .expect("wand");
    assert!(!session.dirty());
    assert!(!session.can_undo());

    session
        .execute(Command::SelectRect(rect(1, 1, 3, 3)))
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
        .execute(Command::SelectEllipse(rect(0, 0, 4, 4)))
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
