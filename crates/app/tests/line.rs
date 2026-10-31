//! Session-level acceptance tests for the Line / Curve command.

use paint_app::{AppSession, Command, SelectionCombineMode};
use paint_core::{Bgra, Point, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{LineCap, LineDash, LineStyle};

fn session() -> AppSession {
    let size = Size::new(20, 20).expect("test dimensions");
    let surface = Surface::filled(size, Bgra::WHITE).expect("test surface");
    let document =
        Document::from_layers(size, vec![Layer::new(LayerId::new(1), "pixels", surface)])
            .expect("test document");
    AppSession::new(document)
}

fn pixel(session: &AppSession, x: u32, y: u32) -> Bgra {
    *session
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("pixel inside the test canvas")
}

#[test]
fn stroke_line_with_caps_is_one_undoable_history_entry() {
    let mut session = session();
    assert!(
        session
            .execute(Command::StrokeLine {
                label: "Line / Curve",
                points: vec![Point::new(4, 10), Point::new(15, 10)],
                style: LineStyle {
                    width: 5,
                    color: Some(Bgra::BLACK),
                    start_cap: LineCap::Rounded,
                    end_cap: LineCap::Arrow,
                    dash: LineDash::Solid,
                },
            })
            .expect("line command")
    );
    assert_eq!(pixel(&session, 10, 10), Bgra::BLACK);
    assert_eq!(
        pixel(&session, 3, 10),
        Bgra::BLACK,
        "rounded start cap reaches past the first endpoint"
    );
    assert!(session.can_undo());
    assert!(!session.can_redo());

    assert!(session.execute(Command::Undo).expect("undo line"));
    assert_eq!(pixel(&session, 10, 10), Bgra::WHITE);
    assert_eq!(pixel(&session, 3, 10), Bgra::WHITE);
    assert!(!session.can_undo());
    assert!(session.can_redo());
}

#[test]
fn stroke_line_is_clipped_to_the_active_selection() {
    let mut session = session();
    session
        .execute(Command::SelectRect {
            rect: paint_core::Rect::new(6, 0, 12, 20).expect("selection rect"),
            mode: SelectionCombineMode::Replace,
        })
        .expect("rectangle selection");
    assert!(
        session
            .execute(Command::StrokeLine {
                label: "Line / Curve",
                points: vec![Point::new(0, 5), Point::new(19, 5)],
                style: LineStyle {
                    width: 1,
                    color: Some(Bgra::BLACK),
                    start_cap: LineCap::Flat,
                    end_cap: LineCap::Flat,
                    dash: LineDash::Dash,
                },
            })
            .expect("line command")
    );
    assert_eq!(pixel(&session, 6, 5), Bgra::BLACK);
    assert_eq!(
        pixel(&session, 7, 5),
        Bgra::WHITE,
        "the 3:1 dash gap stays unpainted"
    );
    assert_eq!(pixel(&session, 9, 5), Bgra::BLACK);
    assert_eq!(pixel(&session, 5, 5), Bgra::WHITE);
    assert_eq!(pixel(&session, 12, 5), Bgra::WHITE);

    assert!(session.execute(Command::Undo).expect("undo line"));
    assert_eq!(pixel(&session, 6, 5), Bgra::WHITE);
    assert!(
        session.document().selection().is_some(),
        "undo restores pixels without clearing the selection"
    );
}
