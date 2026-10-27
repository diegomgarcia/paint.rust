use paint_app::{AppSession, Command, ResourceLimits, SNAPSHOT_FORMAT, SessionError};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};

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
fn new_document_is_clean_and_has_empty_history() {
    let session = session_with_color(2, 3, Bgra::WHITE);

    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.document().revision().get(), 0);
    assert_eq!(session.document().selection(), None);
}

#[test]
fn fill_clips_signed_selection_and_survives_repeated_history_cycles() {
    let mut session = session_with_color(3, 3, Bgra::WHITE);
    let selection = Rect::new(-2, -1, 2, 2).expect("valid signed selection");
    assert!(session.execute(Command::SelectRect(selection)).unwrap());
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    assert!(session.dirty());
    for y in 0..3 {
        for x in 0..3 {
            let expected = if x < 2 && y < 2 {
                Bgra::BLACK
            } else {
                Bgra::WHITE
            };
            assert_eq!(pixel(&session, x, y), expected, "pixel ({x}, {y})");
        }
    }

    for _ in 0..20 {
        assert!(session.undo().unwrap());
        assert!(!session.dirty());
        assert!(session.can_redo());
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(pixel(&session, x, y), Bgra::WHITE);
            }
        }

        assert!(session.redo().unwrap());
        assert!(session.dirty());
        assert!(session.can_undo());
        assert_eq!(pixel(&session, 0, 0), Bgra::BLACK);
        assert_eq!(pixel(&session, 1, 1), Bgra::BLACK);
        assert_eq!(pixel(&session, 2, 2), Bgra::WHITE);
    }
}

#[test]
fn ui_commands_do_not_edit_content_or_invalidate_redo() {
    let mut session = session_with_color(2, 2, Bgra::WHITE);
    session.execute(Command::Invert).unwrap();
    session.undo().unwrap();
    let clean_revision = session.document().revision();
    assert!(session.can_redo());

    let selection = Rect::new(0, 0, 1, 1).unwrap();
    assert!(session.execute(Command::SelectRect(selection)).unwrap());
    assert!(!session.dirty());
    assert!(session.can_redo());
    assert!(!session.execute(Command::SelectRect(selection)).unwrap());
    assert!(
        !session
            .execute(Command::SetActiveLayer { index: 0 })
            .unwrap()
    );
    assert!(
        !session
            .execute(Command::SetLayerVisibility {
                index: 0,
                visible: true,
            })
            .unwrap()
    );
    assert!(
        !session
            .execute(Command::SetLayerOpacity {
                index: 0,
                opacity: 255,
            })
            .unwrap()
    );
    assert!(!session.dirty());
    assert!(session.can_redo());
    assert!(session.document().revision().get() > clean_revision.get());

    assert!(session.execute(Command::ClearSelection).unwrap());
    assert!(!session.execute(Command::ClearSelection).unwrap());
    assert!(session.can_redo());
    assert!(!session.dirty());
}

#[test]
fn new_content_branch_clears_redo_but_noops_preserve_it() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session.execute(Command::Invert).unwrap();
    session.undo().unwrap();
    assert!(session.can_redo());

    assert!(!session.execute(Command::Fill(Bgra::WHITE)).unwrap());
    assert!(session.can_redo());
    assert!(!session.undo().unwrap());
    assert!(session.can_redo());

    assert!(session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    assert!(!session.can_redo());
    assert!(session.can_undo());
    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert!(session.redo().unwrap());
    assert_eq!(pixel(&session, 0, 0), Bgra::BLACK);
}

#[test]
fn empty_history_and_raster_noops_leave_document_and_stacks_unchanged() {
    let mut session = session_with_color(2, 2, Bgra::WHITE);
    let original = session.document().clone();

    assert!(!session.undo().unwrap());
    assert!(!session.redo().unwrap());
    assert!(!session.execute(Command::Fill(Bgra::WHITE)).unwrap());
    assert!(!session.execute(Command::ClearSelection).unwrap());
    assert_eq!(session.document(), &original);
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert!(!session.dirty());

    let outside = Rect::new(5, 5, 8, 9).unwrap();
    session.execute(Command::SelectRect(outside)).unwrap();
    let with_selection = session.document().clone();
    assert!(!session.execute(Command::Invert).unwrap());
    assert!(!session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    assert_eq!(session.document(), &with_selection);
    assert!(!session.can_undo());
    assert!(!session.dirty());
}

#[test]
fn layer_add_and_properties_have_real_inverses_and_unicode_is_preserved() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    let name = "图层 🎨 café";

    assert!(
        session
            .execute(Command::AddLayer {
                name: name.to_owned(),
                opacity: 128,
                visible: false,
            })
            .unwrap()
    );
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(
        session.document().active_layer_id(),
        session.document().layers()[1].id()
    );
    assert_eq!(session.document().layers()[1].name(), name);
    assert_eq!(session.document().layers()[1].opacity(), 128);
    assert!(!session.document().layers()[1].is_visible());
    assert!(session.dirty());

    assert!(session.undo().unwrap());
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(
        session.document().active_layer_id(),
        session.document().layers()[0].id()
    );
    assert!(!session.dirty());
    assert!(session.redo().unwrap());
    assert_eq!(session.document().layers()[1].name(), name);
    assert_eq!(session.document().layers()[1].opacity(), 128);
    assert!(!session.document().layers()[1].is_visible());

    assert!(
        session
            .execute(Command::SetLayerVisibility {
                index: 1,
                visible: true,
            })
            .unwrap()
    );
    assert!(
        session
            .execute(Command::SetLayerOpacity {
                index: 1,
                opacity: 64,
            })
            .unwrap()
    );
    assert_eq!(session.document().layers()[1].opacity(), 64);
    assert!(session.undo().unwrap());
    assert_eq!(session.document().layers()[1].opacity(), 128);
    assert!(session.document().layers()[1].is_visible());
    assert!(session.undo().unwrap());
    assert!(!session.document().layers()[1].is_visible());
    assert!(session.redo().unwrap());
    assert!(session.document().layers()[1].is_visible());
    assert!(session.redo().unwrap());
    assert_eq!(session.document().layers()[1].opacity(), 64);

    for _ in 0..8 {
        assert!(session.undo().unwrap());
        assert_eq!(session.document().layers()[1].opacity(), 128);
        assert!(session.redo().unwrap());
        assert_eq!(session.document().layers()[1].opacity(), 64);
    }
}

#[test]
fn active_layer_and_selection_changes_do_not_dirty_a_clean_document() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session
        .execute(Command::AddLayer {
            name: "Second".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    session.mark_clean();
    assert!(!session.dirty());

    assert!(
        session
            .execute(Command::SetActiveLayer { index: 0 })
            .unwrap()
    );
    assert!(!session.dirty());
    assert!(
        session
            .execute(Command::SetActiveLayer { index: 1 })
            .unwrap()
    );
    assert!(!session.dirty());
    session
        .execute(Command::SelectRect(Rect::new(0, 0, 1, 1).unwrap()))
        .unwrap();
    assert!(!session.dirty());
}

#[test]
fn dirty_state_tracks_content_not_mutation_count() {
    let mut session = session_with_color(1, 1, Bgra::new(1, 2, 3, 99));
    session.execute(Command::Invert).unwrap();
    assert!(session.dirty());
    session.execute(Command::Invert).unwrap();
    assert!(
        !session.dirty(),
        "two inversions restore the baseline pixels"
    );

    session
        .execute(Command::SetLayerOpacity {
            index: 0,
            opacity: 12,
        })
        .unwrap();
    assert!(session.dirty());
    session
        .execute(Command::SetLayerOpacity {
            index: 0,
            opacity: 255,
        })
        .unwrap();
    assert!(!session.dirty(), "restoring the baseline property is clean");

    session
        .execute(Command::SetLayerOpacity {
            index: 0,
            opacity: 12,
        })
        .unwrap();
    session.mark_clean();
    assert!(!session.dirty());
    session.undo().unwrap();
    assert!(session.dirty(), "leaving a newly saved baseline is dirty");
    session.redo().unwrap();
    assert!(!session.dirty());
}

#[test]
fn failed_commands_are_atomic_and_preserve_both_history_stacks() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session.execute(Command::Invert).unwrap();
    session.undo().unwrap();
    let before = session.document().clone();
    let dirty = session.dirty();

    for command in [
        Command::SetActiveLayer { index: 9 },
        Command::SetLayerVisibility {
            index: 9,
            visible: false,
        },
        Command::SetLayerOpacity {
            index: 9,
            opacity: 0,
        },
        Command::AddLayer {
            name: String::new(),
            opacity: 255,
            visible: true,
        },
    ] {
        assert!(session.execute(command).is_err());
        assert_eq!(session.document(), &before);
        assert_eq!(session.dirty(), dirty);
        assert!(!session.can_undo());
        assert!(session.can_redo());
    }

    assert!(session.redo().unwrap());
    assert_eq!(pixel(&session, 0, 0), Bgra::BLACK);
}

#[test]
fn resource_limit_failure_does_not_add_a_layer_or_clear_redo() {
    let size = Size::new(1, 1).unwrap();
    let document = Document::new(size).unwrap();
    let limits = ResourceLimits {
        max_layers: 1,
        ..ResourceLimits::default()
    };
    let mut session = AppSession::with_limits(document, limits).unwrap();
    session.execute(Command::Invert).unwrap();
    session.undo().unwrap();
    let before = session.document().clone();

    let error = session
        .execute(Command::AddLayer {
            name: "too many".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap_err();
    assert!(matches!(error, SessionError::ResourceLimit(_)));
    assert_eq!(session.document(), &before);
    assert!(session.can_redo());
    assert!(!session.can_undo());
}

#[test]
fn snapshot_has_exact_protocol_metadata_and_serializable_shape() {
    let mut session = session_with_color(1, 1, Bgra::new(10, 20, 30, 255));
    session
        .execute(Command::AddLayer {
            name: "顶层 🎨".to_owned(),
            opacity: 0,
            visible: false,
        })
        .unwrap();
    session
        .execute(Command::SelectRect(Rect::new(-1, 0, 1, 1).unwrap()))
        .unwrap();

    let (surface, snapshot) = session.compose_and_snapshot().unwrap();
    assert_eq!(snapshot.format, SNAPSHOT_FORMAT);
    assert_eq!(snapshot.version, 1);
    assert_eq!((snapshot.width, snapshot.height), (1, 1));
    assert_eq!(snapshot.layer_count, 2);
    assert_eq!(snapshot.active_layer_index, 1);
    assert!(snapshot.dirty);
    assert!(snapshot.can_undo);
    assert!(!snapshot.can_redo);
    assert_eq!(snapshot.layers[0].name, "Background");
    assert!(snapshot.layers[0].is_background);
    assert_eq!(snapshot.layers[0].blend_mode, "normal");
    assert_eq!(snapshot.layers[1].name, "顶层 🎨");
    assert!(!snapshot.layers[1].visible);
    assert_eq!(snapshot.layers[1].opacity, 0);
    assert_eq!(snapshot.selection.unwrap().left, -1);
    assert_eq!(snapshot.decoded_rgba_sha256.len(), 64);
    assert_eq!(surface.pixels(), &[Bgra::new(10, 20, 30, 255)]);

    let value = serde_json::to_value(&snapshot).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 12);
    for key in [
        "format",
        "version",
        "width",
        "height",
        "layer_count",
        "active_layer_index",
        "dirty",
        "can_undo",
        "can_redo",
        "layers",
        "selection",
        "decoded_rgba_sha256",
    ] {
        assert!(object.contains_key(key), "missing snapshot key {key}");
    }
    assert_eq!(object["layers"][0]["blend_mode"], "normal");
    assert_eq!(object["selection"]["right"], 1);
}

#[test]
fn multiple_pixel_edits_rebase_every_older_history_entry() {
    let initial = Bgra::new(1, 2, 3, 255);
    let first = Bgra::new(10, 20, 30, 200);
    let second = Bgra::new(200, 100, 50, 44);
    let third = second.inverted_rgb();
    let mut session = session_with_color(1, 1, initial);
    session.execute(Command::Fill(first)).unwrap();
    session.execute(Command::Fill(second)).unwrap();
    session.execute(Command::Invert).unwrap();
    assert_eq!(pixel(&session, 0, 0), third);

    for _ in 0..6 {
        session.undo().unwrap();
        assert_eq!(pixel(&session, 0, 0), second);
        session.undo().unwrap();
        assert_eq!(pixel(&session, 0, 0), first);
        session.undo().unwrap();
        assert_eq!(pixel(&session, 0, 0), initial);
        assert!(!session.dirty());
        assert!(!session.can_undo());
        assert!(session.can_redo());

        session.redo().unwrap();
        assert_eq!(pixel(&session, 0, 0), first);
        session.redo().unwrap();
        assert_eq!(pixel(&session, 0, 0), second);
        session.redo().unwrap();
        assert_eq!(pixel(&session, 0, 0), third);
        assert!(session.dirty());
        assert!(session.can_undo());
        assert!(!session.can_redo());
    }
}

#[test]
fn added_layer_pixels_and_properties_survive_complete_history_cycles() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session
        .execute(Command::AddLayer {
            name: "编辑 🎨".to_owned(),
            opacity: 200,
            visible: true,
        })
        .unwrap();
    let layer_id = session.document().layers()[1].id();
    session.execute(Command::Fill(Bgra::BLACK)).unwrap();
    session
        .execute(Command::SetLayerOpacity {
            index: 1,
            opacity: 90,
        })
        .unwrap();
    session
        .execute(Command::SetLayerVisibility {
            index: 1,
            visible: false,
        })
        .unwrap();

    for _ in 0..4 {
        session.undo().unwrap();
        assert!(session.document().layers()[1].is_visible());
        session.undo().unwrap();
        assert_eq!(session.document().layers()[1].opacity(), 200);
        session.undo().unwrap();
        assert_eq!(
            session.document().layers()[1].surface().pixels(),
            &[Bgra::TRANSPARENT]
        );
        session.undo().unwrap();
        assert_eq!(session.document().layers().len(), 1);
        assert!(!session.dirty());
        assert!(!session.can_undo());

        session.redo().unwrap();
        assert_eq!(session.document().layers().len(), 2);
        assert_eq!(session.document().layers()[1].id(), layer_id);
        assert_eq!(session.document().layers()[1].name(), "编辑 🎨");
        session.redo().unwrap();
        assert_eq!(
            session.document().layers()[1].surface().pixels(),
            &[Bgra::BLACK]
        );
        session.redo().unwrap();
        assert_eq!(session.document().layers()[1].opacity(), 90);
        session.redo().unwrap();
        assert!(!session.document().layers()[1].is_visible());
        assert!(session.dirty());
        assert!(!session.can_redo());
    }
}

#[test]
fn undoing_an_inactive_layer_add_does_not_undo_later_active_ui_state() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session
        .execute(Command::AddLayer {
            name: "A".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::AddLayer {
            name: "B".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    let background = session.document().layers()[0].id();
    session
        .execute(Command::SetActiveLayer { index: 0 })
        .unwrap();
    let history_revision = session.document().revision();

    session.undo().unwrap();
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(session.document().active_layer_id(), background);
    assert!(session.document().revision().get() > history_revision.get());
}
