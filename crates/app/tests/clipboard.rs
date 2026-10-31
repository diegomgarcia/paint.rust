use paint_app::{AppSession, ClipboardContent, Command, SelectionCombineMode};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::ShapeStyle;

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

fn copied(session: &AppSession) -> ClipboardContent {
    session
        .copy_selection()
        .expect("copy succeeds")
        .expect("clipboard content")
}

#[test]
fn copy_masked_selection_captures_bounds_shape_and_pixels_without_history() {
    let mut session = session_with_color(8, 8, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(1, 1, 7, 7),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse selection");

    let content = copied(&session);
    assert_eq!(content.bounds(), rect(1, 1, 7, 7));
    assert_eq!(
        content.surface().size(),
        Size::new(6, 6).expect("ellipse bounds")
    );
    assert_eq!(
        *content
            .surface()
            .pixel(Point::new(3, 3))
            .expect("selected center"),
        Bgra::WHITE
    );
    assert_eq!(
        *content
            .surface()
            .pixel(Point::new(0, 0))
            .expect("corner outside the ellipse"),
        Bgra::TRANSPARENT,
        "pixels outside a mask selection are transparent in the clipboard surface"
    );
    let mask = content.mask().expect("mask shape is preserved");
    assert!(mask.contains(Point::new(4, 4)));
    assert!(!mask.contains(Point::new(1, 1)));

    assert_eq!(
        pixel(&session, 3, 3),
        Bgra::WHITE,
        "copy never edits the document"
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn copy_without_a_selection_copies_the_whole_active_layer() {
    let mut session = session_with_color(5, 4, Bgra::WHITE);
    session
        .execute(Command::FillRect {
            rect: rect(1, 1, 3, 3),
            color: Bgra::BLACK,
            style: ShapeStyle::Fill,
            border_width: 0,
        })
        .expect("paint one black pixel");
    assert!(session.can_undo());

    let content = copied(&session);
    assert_eq!(content.bounds(), session.document().canvas_rect());
    assert_eq!(
        content.surface().size(),
        Size::new(5, 4).expect("canvas size")
    );
    assert!(content.mask().is_none(), "a whole layer is rectangular");
    assert_eq!(
        *content
            .surface()
            .pixel(Point::new(1, 1))
            .expect("painted pixel"),
        Bgra::BLACK
    );
    assert_eq!(
        *content
            .surface()
            .pixel(Point::new(0, 0))
            .expect("untouched pixel"),
        Bgra::WHITE
    );

    assert!(
        session.undo().expect("undo the only edit"),
        "copy must not add a history entry"
    );
    assert_eq!(pixel(&session, 1, 1), Bgra::WHITE);
    assert!(!session.can_undo());
}

#[test]
fn cut_erases_only_selected_pixels_and_clears_the_selection_as_one_undo_entry() {
    let mut session = session_with_color(8, 8, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(1, 1, 7, 7),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse selection");
    let selection_before = session
        .document()
        .selection_model()
        .cloned()
        .expect("selection before cut");

    let content = session
        .cut_selection()
        .expect("cut succeeds")
        .expect("cut content");
    assert_eq!(content.bounds(), rect(1, 1, 7, 7));
    assert_eq!(
        *content
            .surface()
            .pixel(Point::new(3, 3))
            .expect("cut center"),
        Bgra::WHITE
    );

    assert_eq!(
        pixel(&session, 3, 3),
        Bgra::TRANSPARENT,
        "selected pixels are erased"
    );
    assert_eq!(
        pixel(&session, 0, 0),
        Bgra::WHITE,
        "pixels outside the mask are untouched"
    );
    assert!(
        session.document().selection().is_none(),
        "Paint.NET clears the selection after Cut"
    );
    assert!(session.dirty());
    assert!(session.can_undo());

    assert!(session.undo().expect("undo cut"));
    assert_eq!(
        pixel(&session, 3, 3),
        Bgra::WHITE,
        "one undo restores the erased pixels"
    );
    assert_eq!(
        session.document().selection_model(),
        Some(&selection_before),
        "undo restores the pre-cut selection shape"
    );
    assert!(!session.can_undo());

    assert!(session.redo().expect("redo cut"));
    assert_eq!(pixel(&session, 3, 3), Bgra::TRANSPARENT);
    assert!(session.document().selection().is_none());
}

#[test]
fn cut_without_a_selection_is_a_no_op_matching_paint_net() {
    let mut session = session_with_color(6, 6, Bgra::WHITE);
    assert!(
        session
            .cut_selection()
            .expect("cut query succeeds")
            .is_none()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(pixel(&session, 2, 2), Bgra::WHITE);
}

#[test]
fn clipboard_destination_uses_original_bounds_or_the_canvas_origin() {
    let surface = Surface::filled(Size::new(3, 4).expect("clipboard size"), Bgra::BLACK)
        .expect("clipboard surface");
    let content =
        ClipboardContent::new(surface, rect(2, 3, 5, 7)).expect("valid clipboard content");
    assert_eq!(
        content.destination(Size::new(100, 100).expect("canvas")),
        (2, 3),
        "an on-canvas clipboard keeps its original bounds"
    );
    assert_eq!(
        content.destination(Size::new(4, 4).expect("partially overlapping canvas")),
        (2, 3),
        "a partially visible clipboard still starts at its original bounds"
    );

    let surface = Surface::filled(Size::new(3, 4).expect("clipboard size"), Bgra::BLACK)
        .expect("clipboard surface");
    let off_canvas =
        ClipboardContent::new(surface, rect(200, 200, 203, 204)).expect("valid clipboard content");
    assert_eq!(
        off_canvas.destination(Size::new(20, 20).expect("small canvas")),
        (0, 0),
        "a fully off-canvas clipboard pastes at the canvas origin"
    );
}

#[test]
fn clipboard_selection_at_rebuilds_the_rectangular_or_masked_shape() {
    let surface = Surface::filled(Size::new(3, 4).expect("clipboard size"), Bgra::BLACK)
        .expect("clipboard surface");
    let rectangular =
        ClipboardContent::new(surface, rect(2, 3, 5, 7)).expect("valid clipboard content");
    let selection = rectangular
        .selection_at(10, 20)
        .expect("selection")
        .expect("rectangular selection");
    assert_eq!(selection.bounds(), rect(10, 20, 13, 24));
    assert!(selection.mask().is_none());

    let mut session = session_with_color(8, 8, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(1, 1, 7, 7),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse selection");
    let content = copied(&session);
    let selection = content
        .selection_at(10, 10)
        .expect("selection")
        .expect("masked selection");
    assert_eq!(selection.bounds(), rect(10, 10, 16, 16));
    let mask = selection.mask().expect("mask shape follows the paste");
    assert!(mask.contains(Point::new(13, 13)));
    assert!(!mask.contains(Point::new(10, 10)));
}

#[test]
fn pasting_stamps_masked_pixels_at_the_destination_and_undoes_in_one_step() {
    let mut session = session_with_color(20, 20, Bgra::WHITE);
    session
        .execute(Command::SelectEllipse {
            rect: rect(1, 1, 7, 7),
            mode: SelectionCombineMode::Replace,
        })
        .expect("ellipse selection");
    session
        .execute(Command::Fill(Bgra::BLACK))
        .expect("paint the selected ellipse");
    let content = copied(&session);

    session
        .execute(Command::ClearSelection)
        .expect("deselect before pasting");
    let (dest_x, dest_y) = content.destination(session.document().size());
    let selection = content
        .selection_at(10, 10)
        .expect("paste selection")
        .expect("selection shape");
    session
        .set_selection_shape(Some(selection))
        .expect("install paste selection");
    assert!(
        session
            .stamp_floating(content.surface(), 10, 10)
            .expect("paste stamp")
    );

    assert_eq!(
        pixel(&session, 13, 13),
        Bgra::BLACK,
        "masked pixels land at the destination"
    );
    assert_eq!(
        pixel(&session, 10, 10),
        Bgra::WHITE,
        "mask corners stay transparent at the destination"
    );
    assert_eq!(
        pixel(&session, 3, 3),
        Bgra::BLACK,
        "the copied source pixels are untouched by copy/paste"
    );

    assert!(session.undo().expect("undo paste"));
    assert_eq!(pixel(&session, 13, 13), Bgra::WHITE);
    assert_eq!(pixel(&session, 3, 3), Bgra::BLACK);
    assert_eq!(
        (dest_x, dest_y),
        (1, 1),
        "the paste destination matches the copied bounds"
    );
}
