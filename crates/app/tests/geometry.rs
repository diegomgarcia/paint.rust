use paint_app::{AppSession, CanvasAnchor, Command, ResourceLimits};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, SelectionCombineMode};

fn select_rect(rect: Rect) -> Command {
    Command::SelectRect {
        rect,
        mode: SelectionCombineMode::Replace,
    }
}

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("nonzero test size")
}

fn layer(id: u64, name: &str, width: u32, height: u32, pixels: Vec<Bgra>) -> Layer {
    let surface = Surface::from_pixels(size(width, height), pixels).expect("test surface");
    Layer::new(LayerId::new(id), name, surface)
}

fn distinct(width: u32, height: u32, offset: u8) -> Vec<Bgra> {
    (0..width * height)
        .map(|index| {
            let value = u8::try_from(index).expect("small index");
            Bgra::new(
                value,
                value.wrapping_add(offset),
                value.wrapping_add(2),
                255,
            )
        })
        .collect()
}

fn two_layer_session(width: u32, height: u32) -> AppSession {
    let background =
        layer(1, "Background", width, height, distinct(width, height, 10)).with_background(true);
    let overlay = layer(2, "Overlay", width, height, distinct(width, height, 40))
        .with_opacity(200)
        .with_visibility(false);
    let document = Document::from_layers(size(width, height), vec![background, overlay])
        .expect("two-layer document");
    AppSession::new(document)
}

fn pixel(session: &AppSession, layer: usize, x: u32, y: u32) -> Bgra {
    *session.document().layers()[layer]
        .surface()
        .pixel(Point::new(x, y))
        .expect("in-bounds pixel")
}

#[test]
fn crop_to_selection_clips_bounds_and_undo_restores_the_exact_document() {
    let mut session = two_layer_session(3, 2);
    session
        .execute(select_rect(
            Rect::new(-2, -1, 2, 2).expect("signed selection"),
        ))
        .unwrap();
    let before = session.document().clone();
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(session.execute(Command::CropToSelection).unwrap());
    assert_eq!(session.document().size(), size(2, 2));
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(session.document().selection(), None);
    assert_eq!(session.document().layers()[0].name(), "Background");
    assert_eq!(session.document().layers()[1].name(), "Overlay");
    assert_eq!(session.document().layers()[1].opacity(), 200);
    assert!(!session.document().layers()[1].is_visible());
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(0, 10, 2, 255));
    assert_eq!(pixel(&session, 0, 1, 1), Bgra::new(4, 14, 6, 255));
    assert_eq!(pixel(&session, 1, 0, 0), Bgra::new(0, 40, 2, 255));
    assert!(session.dirty());
    assert!(session.can_undo());

    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
    assert!(!session.dirty());
    assert!(session.can_redo());
    assert!(!session.can_undo());

    assert!(session.redo().unwrap());
    assert_eq!(session.document().size(), size(2, 2));
}

#[test]
fn crop_without_an_intersecting_selection_is_a_no_op() {
    let mut session = two_layer_session(3, 2);
    assert!(!session.execute(Command::CropToSelection).unwrap());
    assert!(!session.can_undo());
    assert_eq!(session.document().size(), size(3, 2));

    session
        .execute(select_rect(
            Rect::new(-9, -9, -4, -4).expect("outside selection"),
        ))
        .unwrap();
    assert!(!session.execute(Command::CropToSelection).unwrap());
    assert!(!session.can_undo());
    assert_eq!(session.document().size(), size(3, 2));
}

#[test]
fn crop_of_the_full_canvas_is_a_no_op_that_keeps_the_selection() {
    let mut session = two_layer_session(3, 2);
    session
        .execute(select_rect(Rect::new(0, 0, 3, 2).expect("full selection")))
        .unwrap();
    let before = session.document().clone();
    assert!(!session.execute(Command::CropToSelection).unwrap());
    assert!(!session.can_undo());
    assert_eq!(session.document().size(), size(3, 2));
    assert_eq!(session.document(), &before);
}

#[test]
fn resize_image_resamples_every_layer_and_undo_restores_exact_state() {
    let background =
        layer(1, "Background", 2, 2, vec![Bgra::new(0, 10, 2, 255); 4]).with_background(true);
    let overlay = layer(2, "Overlay", 2, 2, vec![Bgra::new(0, 40, 2, 255); 4])
        .with_opacity(200)
        .with_visibility(false);
    let document =
        Document::from_layers(size(2, 2), vec![background, overlay]).expect("two-layer document");
    let mut session = AppSession::new(document);
    session
        .execute(select_rect(Rect::new(0, 0, 1, 1).expect("selection")))
        .unwrap();
    let before = session.document().clone();

    assert!(
        session
            .execute(Command::ResizeImage {
                width: 4,
                height: 6,
            })
            .unwrap()
    );
    assert_eq!(session.document().size(), size(4, 6));
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(session.document().layers()[0].id(), LayerId::new(1));
    assert_eq!(session.document().layers()[1].id(), LayerId::new(2));
    assert_eq!(session.document().layers()[0].name(), "Background");
    assert_eq!(session.document().layers()[1].name(), "Overlay");
    assert_eq!(session.document().layers()[1].opacity(), 200);
    assert!(!session.document().layers()[1].is_visible());
    assert_eq!(session.document().selection(), None);
    for layer in session.document().layers() {
        assert_eq!(layer.surface().size(), size(4, 6));
    }
    assert_eq!(pixel(&session, 0, 3, 5), Bgra::new(0, 10, 2, 255));
    assert_eq!(pixel(&session, 1, 3, 5), Bgra::new(0, 40, 2, 255));
    assert!(session.dirty());

    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
    assert!(session.can_redo());
    assert!(!session.can_undo());
}

#[test]
fn resize_image_rejects_zero_and_over_limit_dimensions_without_mutation() {
    let limits = ResourceLimits {
        max_width: 3,
        max_height: 3,
        ..ResourceLimits::default()
    };
    let background = layer(1, "Background", 2, 2, vec![Bgra::WHITE; 4]).with_background(true);
    let document = Document::from_layers(size(2, 2), vec![background]).expect("document");
    let mut session = AppSession::with_limits(document, limits).expect("session");
    let before = session.document().clone();

    assert!(
        session
            .execute(Command::ResizeImage {
                width: 0,
                height: 2,
            })
            .is_err()
    );
    assert_eq!(session.document(), &before);
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ResizeImage {
                width: 4,
                height: 2,
            })
            .is_err()
    );
    assert_eq!(session.document(), &before);
    assert!(!session.can_undo());
}

#[test]
fn canvas_size_anchors_offsets_fills_and_undoes_an_exact_document() {
    let mut session = two_layer_session(2, 2);
    session
        .execute(select_rect(Rect::new(0, 0, 2, 2).expect("selection")))
        .unwrap();
    let before = session.document().clone();

    assert!(
        session
            .execute(Command::CanvasSize {
                width: 4,
                height: 4,
                anchor: CanvasAnchor::BottomRight,
                fill: Bgra::TRANSPARENT,
            })
            .unwrap()
    );
    assert_eq!(session.document().size(), size(4, 4));
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::TRANSPARENT);
    assert_eq!(pixel(&session, 0, 2, 2), Bgra::new(0, 10, 2, 255));
    assert_eq!(pixel(&session, 0, 3, 3), Bgra::new(3, 13, 5, 255));
    assert_eq!(pixel(&session, 1, 2, 2), Bgra::new(0, 40, 2, 255));
    assert_eq!(
        session.document().selection(),
        Some(Rect::new(2, 2, 4, 4).expect("shifted selection"))
    );
    assert_eq!(session.document().layers()[1].opacity(), 200);
    assert!(!session.document().layers()[1].is_visible());

    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);

    assert!(
        session
            .execute(Command::CanvasSize {
                width: 3,
                height: 3,
                anchor: CanvasAnchor::TopLeft,
                fill: Bgra::WHITE,
            })
            .unwrap()
    );
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(0, 10, 2, 255));
    assert_eq!(pixel(&session, 0, 2, 2), Bgra::WHITE);
}

#[test]
fn canvas_size_shrink_crops_relative_to_the_anchor() {
    let background = layer(1, "Background", 3, 3, distinct(3, 3, 5)).with_background(true);
    let document = Document::from_layers(size(3, 3), vec![background]).expect("document");
    let mut session = AppSession::new(document);
    let before = session.document().clone();

    assert!(
        session
            .execute(Command::CanvasSize {
                width: 2,
                height: 2,
                anchor: CanvasAnchor::TopLeft,
                fill: Bgra::TRANSPARENT,
            })
            .unwrap()
    );
    assert_eq!(session.document().size(), size(2, 2));
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(0, 5, 2, 255));
    assert_eq!(pixel(&session, 0, 1, 1), Bgra::new(4, 9, 6, 255));

    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
}

#[test]
fn canvas_size_without_a_dimension_change_is_a_no_op() {
    let mut session = two_layer_session(2, 2);
    assert!(
        !session
            .execute(Command::CanvasSize {
                width: 2,
                height: 2,
                anchor: CanvasAnchor::Middle,
                fill: Bgra::BLACK,
            })
            .unwrap()
    );
    assert!(!session.can_undo());
}

#[test]
fn clockwise_rotation_rotates_every_layer_swaps_dimensions_and_undoes_exactly() {
    let mut session = two_layer_session(2, 3);
    session
        .execute(select_rect(Rect::new(0, 0, 1, 1).expect("selection")))
        .unwrap();
    let before = session.document().clone();

    assert!(session.execute(Command::Rotate90Clockwise).unwrap());
    assert_eq!(session.document().size(), size(3, 2));
    assert_eq!(session.document().selection(), None);
    assert_eq!(session.document().layers().len(), 2);
    // clockwise: new row 0, column 0 is old (0, 2)
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(4, 14, 6, 255));
    assert_eq!(pixel(&session, 0, 2, 0), Bgra::new(0, 10, 2, 255));
    assert_eq!(pixel(&session, 0, 0, 1), Bgra::new(5, 15, 7, 255));
    assert_eq!(pixel(&session, 1, 0, 0), Bgra::new(4, 44, 6, 255));

    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
    assert!(session.can_redo());
}

#[test]
fn counter_clockwise_and_half_turn_rotations_are_exact() {
    let mut session = two_layer_session(2, 3);
    let before = session.document().clone();

    assert!(session.execute(Command::Rotate90CounterClockwise).unwrap());
    assert_eq!(session.document().size(), size(3, 2));
    // counter-clockwise: new row 0, column 0 is old (1, 0)
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(1, 11, 3, 255));
    assert_eq!(pixel(&session, 0, 2, 0), Bgra::new(5, 15, 7, 255));
    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);

    assert!(session.execute(Command::Rotate180).unwrap());
    assert_eq!(session.document().size(), size(2, 3));
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(5, 15, 7, 255));
    assert_eq!(pixel(&session, 0, 1, 2), Bgra::new(0, 10, 2, 255));
    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
}

#[test]
fn image_flips_transform_every_layer_and_remain_single_undo_steps() {
    let mut session = two_layer_session(2, 2);
    let before = session.document().clone();

    assert!(session.execute(Command::FlipImageHorizontal).unwrap());
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(1, 11, 3, 255));
    assert_eq!(pixel(&session, 1, 0, 0), Bgra::new(1, 41, 3, 255));
    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);

    assert!(session.execute(Command::FlipImageVertical).unwrap());
    assert_eq!(pixel(&session, 0, 0, 0), Bgra::new(2, 12, 4, 255));
    assert_eq!(pixel(&session, 1, 0, 0), Bgra::new(2, 42, 4, 255));
    assert!(session.undo().unwrap());
    assert_eq!(session.document(), &before);
    assert!(!session.can_undo());
}
