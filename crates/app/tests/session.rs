use paint_app::{
    AppSession, Command, Effect, ResourceLimitError, ResourceLimits, SNAPSHOT_FORMAT, SessionError,
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId, SelectionCombineMode};
use paint_raster::{MAX_MOTION_BLUR_DISTANCE, MAX_NOISE_INTENSITY, MAX_SHARPEN_AMOUNT};

fn select_rect(rect: Rect) -> Command {
    Command::SelectRect {
        rect,
        mode: SelectionCombineMode::Replace,
    }
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
    assert!(session.execute(select_rect(selection)).unwrap());
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
    assert!(session.execute(select_rect(selection)).unwrap());
    assert!(!session.dirty());
    assert!(session.can_redo());
    assert!(!session.execute(select_rect(selection)).unwrap());
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
    session.execute(select_rect(outside)).unwrap();
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
        .execute(select_rect(Rect::new(0, 0, 1, 1).unwrap()))
        .unwrap();
    assert!(!session.dirty());
}

#[test]
fn layer_flips_are_atomic_undoable_content_edits() {
    let size = Size::new(3, 2).expect("dimensions");
    let original = vec![
        Bgra::new(1, 0, 0, 255),
        Bgra::new(0, 2, 0, 255),
        Bgra::new(0, 0, 3, 255),
        Bgra::new(4, 0, 0, 255),
        Bgra::new(0, 5, 0, 255),
        Bgra::new(0, 0, 6, 255),
    ];
    let surface = Surface::from_pixels(size, original.clone()).expect("surface");
    let document = Document::from_layers(size, vec![Layer::new(LayerId::new(1), "flip", surface)])
        .expect("document");
    let mut session = AppSession::new(document);

    assert!(session.execute(Command::FlipLayerHorizontal).unwrap());
    assert!(session.dirty());
    assert!(session.can_undo());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &[
            original[2],
            original[1],
            original[0],
            original[5],
            original[4],
            original[3],
        ]
    );

    assert!(session.execute(Command::FlipLayerVertical).unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &[
            original[5],
            original[4],
            original[3],
            original[2],
            original[1],
            original[0],
        ]
    );

    assert!(session.undo().unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &[
            original[2],
            original[1],
            original[0],
            original[5],
            original[4],
            original[3],
        ]
    );
    assert!(session.undo().unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &original
    );
    assert!(!session.dirty());
    assert!(session.can_redo());
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
        .execute(select_rect(Rect::new(-1, 0, 1, 1).unwrap()))
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

fn session_from_pixels(width: u32, height: u32, pixels: Vec<Bgra>) -> AppSession {
    let size = Size::new(width, height).expect("nonzero test dimensions");
    let surface = Surface::from_pixels(size, pixels).expect("test surface");
    let layer = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
    let document = Document::from_layers(size, vec![layer]).expect("test document");
    AppSession::new(document)
}

#[test]
fn effect_commands_are_single_undoable_mutations_and_respect_the_selection() {
    let mut session = session_from_pixels(
        3,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 255),
            Bgra::new(255, 0, 0, 255),
        ],
    );
    let selection = Rect::new(1, 0, 2, 1).unwrap();
    session.execute(select_rect(selection)).unwrap();

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::Grayscale))
            .unwrap()
    );
    assert!(session.dirty());
    assert!(session.can_undo());
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 0, 255, 255));
    assert_eq!(pixel(&session, 1, 0), Bgra::new(150, 150, 150, 255));
    assert_eq!(pixel(&session, 2, 0), Bgra::new(255, 0, 0, 255));
    assert_eq!(session.document().selection(), Some(selection));

    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert_eq!(pixel(&session, 1, 0), Bgra::new(0, 255, 0, 255));
    assert!(session.can_redo());

    assert!(session.redo().unwrap());
    assert!(session.dirty());
    assert_eq!(pixel(&session, 1, 0), Bgra::new(150, 150, 150, 255));

    assert!(session.undo().unwrap());
    assert!(
        session
            .execute(Command::ApplyEffect(Effect::Sepia))
            .unwrap()
    );
    assert!(!session.can_redo(), "a new effect must invalidate redo");
    assert_eq!(pixel(&session, 1, 0), Bgra::new(136, 175, 196, 255));
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 0, 255, 255));

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::GaussianBlur { radius: 1 }))
            .unwrap()
    );
    assert_eq!(pixel(&session, 0, 0).a, 255);
    assert_eq!(pixel(&session, 2, 0).a, 255);
    assert!(session.undo().unwrap());
    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert!(!session.can_undo());
}

#[test]
fn zero_radius_blur_is_a_noop_and_invalid_radius_is_rejected_atomically() {
    let mut session = session_from_pixels(3, 1, vec![Bgra::WHITE, Bgra::BLACK, Bgra::WHITE]);

    assert!(
        !session
            .execute(Command::ApplyEffect(Effect::GaussianBlur { radius: 0 }))
            .unwrap()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::GaussianBlur {
                radius: u32::MAX,
            }))
            .is_err()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &[Bgra::WHITE, Bgra::BLACK, Bgra::WHITE]
    );
}

#[test]
fn place_text_is_one_undoable_selection_clipped_content_edit() {
    let mut session = session_with_color(160, 80, Bgra::WHITE);
    session
        .execute(select_rect(Rect::new(0, 0, 48, 80).expect("selection")))
        .unwrap();
    let original = session
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    assert!(
        session
            .execute(Command::PlaceText {
                origin: Point::new(8, 8),
                content: "Ag".to_owned(),
                style: paint_raster::TextStyle {
                    family: paint_raster::TextFontFamily::Sans,
                    size: 48,
                    bold: false,
                    italic: false,
                    antialias: true,
                },
                color: Bgra::BLACK,
            })
            .unwrap()
    );
    assert!(session.dirty());
    assert!(session.can_undo());
    let pixels = session.document().active_layer().surface().pixels();
    assert!(
        pixels.iter().enumerate().any(|(index, pixel)| {
            (index % 160) < 48 && pixel.a == 255 && pixel.r < 128 && pixel.g < 128
        }),
        "committed text should contain opaque ink inside the selection"
    );
    for y in 0..80 {
        for x in 48..160 {
            assert_eq!(
                *session
                    .document()
                    .active_layer()
                    .surface()
                    .pixel(Point::new(x, y))
                    .expect("pixel"),
                Bgra::WHITE,
                "text leaked outside the selection at ({x}, {y})"
            );
        }
    }

    assert!(session.undo().unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        original
    );
    assert!(!session.dirty());
    assert!(session.redo().unwrap());
    assert!(session.dirty());
}

#[test]
fn clone_stamp_is_one_undoable_selection_clipped_content_edit() {
    let size = Size::new(64, 64).expect("size");
    let mut pixels = vec![Bgra::WHITE; 64 * 64];
    for y in 8..16 {
        for x in 8..40 {
            pixels[y * 64 + x] = Bgra::BLACK;
        }
    }
    let source = Surface::from_pixels(size, pixels).expect("source surface");
    let document = Document::from_layers(
        size,
        vec![Layer::new(LayerId::new(30), "clone", source).with_background(true)],
    )
    .expect("document");
    let mut session = AppSession::new(document);
    let original = session
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    session
        .execute(select_rect(Rect::new(44, 0, 46, 64).expect("selection")))
        .unwrap();
    assert!(
        session
            .execute(Command::CloneStamp {
                points: vec![Point::new(40, 12), Point::new(48, 12)],
                size: 1,
                source: Point::new(12, 12),
                hardness: 100,
            })
            .unwrap()
    );
    assert!(session.can_undo());
    for x in 44..46 {
        assert_eq!(
            *session
                .document()
                .active_layer()
                .surface()
                .pixel(Point::new(x, 12))
                .expect("cloned pixel"),
            Bgra::BLACK
        );
    }
    for x in [43, 46, 47] {
        assert_eq!(
            *session
                .document()
                .active_layer()
                .surface()
                .pixel(Point::new(x, 12))
                .expect("unclipped pixel"),
            Bgra::WHITE
        );
    }
    assert!(session.undo().unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        original
    );
}

#[test]
fn rename_layer_is_one_undoable_content_commit_preserving_active_identity() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    session
        .execute(Command::AddLayer {
            name: "Ink".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::SetActiveLayer { index: 0 })
        .unwrap();
    let active = session.document().active_layer_id();
    let target = session.document().layers()[1].id();
    let history_depth = session.document().revision();

    assert!(
        session
            .execute(Command::RenameLayer {
                index: 1,
                name: "Paint".to_owned(),
            })
            .unwrap()
    );
    assert_eq!(session.document().layers()[1].name(), "Paint");
    assert_eq!(session.document().active_layer_id(), active);
    assert_ne!(session.document().layers()[1].id(), active);
    assert_eq!(session.document().layers()[1].id(), target);
    assert!(session.dirty());
    assert!(session.document().revision().get() > history_depth.get());

    assert!(session.undo().unwrap());
    assert_eq!(session.document().layers()[1].name(), "Ink");
    assert_eq!(session.document().active_layer_id(), active);

    assert!(session.redo().unwrap());
    assert_eq!(session.document().layers()[1].name(), "Paint");
}

#[test]
fn rename_layer_rejects_empty_and_overlong_names_without_side_effects() {
    let mut session = session_with_color(1, 1, Bgra::WHITE);
    let before = session.document().clone();
    let error = session
        .execute(Command::RenameLayer {
            index: 0,
            name: String::new(),
        })
        .expect_err("empty names must be rejected");
    assert!(matches!(error, SessionError::EmptyLayerName));
    assert_eq!(session.document(), &before);
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert!(!session.dirty());

    let limits = ResourceLimits {
        max_layer_name_bytes: 3,
        ..ResourceLimits::default()
    };
    let mut limited = AppSession::with_limits(before, limits).expect("limited session");
    let error = limited
        .execute(Command::RenameLayer {
            index: 0,
            name: "four".to_owned(),
        })
        .expect_err("overlong names must be rejected");
    assert!(matches!(
        error,
        SessionError::ResourceLimit(ResourceLimitError::LayerNameBytes { actual: 4, max: 3 })
    ));
    assert_eq!(limited.document().layers()[0].name(), "Background");
    assert!(!limited.can_undo());

    assert!(
        !session
            .execute(Command::RenameLayer {
                index: 0,
                name: "Background".to_owned(),
            })
            .unwrap(),
        "renaming to the same name is a no-op"
    );
    assert!(!session.can_undo());
}

#[test]
fn blend_mode_command_is_one_undoable_layer_property_change() {
    let mut session = session_with_color(2, 2, Bgra::WHITE);
    assert_eq!(
        session.snapshot().expect("snapshot").layers[0].blend_mode,
        "normal"
    );
    assert!(
        session
            .execute(Command::SetLayerBlendMode {
                index: 0,
                mode: paint_document::BlendMode::Multiply,
            })
            .expect("set blend mode")
    );
    assert_eq!(
        session.snapshot().expect("snapshot").layers[0].blend_mode,
        "multiply"
    );
    assert!(session.undo().expect("undo"));
    assert_eq!(
        session.snapshot().expect("snapshot").layers[0].blend_mode,
        "normal"
    );
}

#[test]
fn adjustment_effect_commands_are_single_undoable_mutations_and_noops_are_atomic() {
    let mut session = session_from_pixels(
        3,
        1,
        vec![
            Bgra::new(0, 0, 255, 255),
            Bgra::new(0, 255, 0, 128),
            Bgra::new(255, 0, 0, 0),
        ],
    );

    assert!(
        !session
            .execute(Command::ApplyEffect(Effect::BrightnessContrast {
                brightness: 0,
                contrast: 0,
            }))
            .expect("identity brightness/contrast")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::BrightnessContrast {
                brightness: -100,
                contrast: 0,
            }))
            .expect("brightness")
    );
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 0, 155, 255));
    assert_eq!(pixel(&session, 1, 0), Bgra::new(0, 155, 0, 128));
    assert_eq!(pixel(&session, 2, 0), Bgra::new(155, 0, 0, 0));
    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert!(session.redo().unwrap());
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 0, 155, 255));
    assert!(session.undo().unwrap());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::HueSaturation {
                hue: 120,
                saturation: 0,
                lightness: 0,
            }))
            .expect("hue/saturation")
    );
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 255, 0, 255));
    assert_eq!(pixel(&session, 1, 0), Bgra::new(255, 0, 0, 128));
    assert_eq!(pixel(&session, 2, 0), Bgra::new(0, 0, 255, 0));
    assert!(session.undo().unwrap());
    assert_eq!(pixel(&session, 0, 0), Bgra::new(0, 0, 255, 255));

    let mut stretched = session_from_pixels(
        2,
        1,
        vec![Bgra::new(10, 20, 30, 255), Bgra::new(110, 120, 130, 255)],
    );
    assert!(
        stretched
            .execute(Command::ApplyEffect(Effect::AutoLevel))
            .expect("auto-level")
    );
    assert_eq!(pixel(&stretched, 0, 0), Bgra::new(0, 0, 0, 255));
    assert_eq!(pixel(&stretched, 1, 0), Bgra::new(255, 255, 255, 255));
    assert!(stretched.undo().unwrap());
    assert_eq!(pixel(&stretched, 0, 0), Bgra::new(10, 20, 30, 255));
    assert_eq!(pixel(&stretched, 1, 0), Bgra::new(110, 120, 130, 255));

    let mut flat = session_from_pixels(2, 1, vec![Bgra::new(40, 50, 60, 255); 2]);
    assert!(
        !flat
            .execute(Command::ApplyEffect(Effect::AutoLevel))
            .expect("flat auto-level")
    );
    assert!(!flat.dirty());
    assert!(!flat.can_undo());

    let mut single_selected = session_from_pixels(
        2,
        1,
        vec![Bgra::new(10, 20, 30, 255), Bgra::new(110, 120, 130, 255)],
    );
    assert!(
        single_selected
            .execute(select_rect(Rect::new(0, 0, 1, 1).unwrap()))
            .unwrap()
    );
    assert!(
        !single_selected
            .execute(Command::ApplyEffect(Effect::AutoLevel))
            .expect("single-pixel auto-level")
    );
    assert!(!single_selected.dirty());
    assert!(!single_selected.can_undo());
}

#[test]
fn motion_blur_effect_is_one_undoable_mutation_and_noops_are_atomic() {
    let mut session = session_from_pixels(
        5,
        1,
        vec![
            Bgra::BLACK,
            Bgra::BLACK,
            Bgra::WHITE,
            Bgra::BLACK,
            Bgra::BLACK,
        ],
    );
    assert!(
        session
            .execute(Command::ApplyEffect(Effect::MotionBlur {
                angle: 0,
                distance: 1,
            }))
            .expect("motion blur")
    );
    assert!(session.dirty());
    assert!(session.can_undo());
    assert_eq!(pixel(&session, 2, 0), Bgra::new(85, 85, 85, 255));
    assert_eq!(pixel(&session, 1, 0), Bgra::new(85, 85, 85, 255));
    assert_eq!(pixel(&session, 0, 0), Bgra::BLACK);
    assert_eq!(pixel(&session, 4, 0), Bgra::BLACK);

    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert_eq!(pixel(&session, 2, 0), Bgra::WHITE);
    assert!(session.redo().unwrap());
    assert_eq!(pixel(&session, 2, 0), Bgra::new(85, 85, 85, 255));
    assert!(session.undo().unwrap());

    assert!(
        !session
            .execute(Command::ApplyEffect(Effect::MotionBlur {
                angle: 45,
                distance: 0,
            }))
            .expect("zero distance")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::MotionBlur {
                angle: 0,
                distance: MAX_MOTION_BLUR_DISTANCE + 1,
            }))
            .is_err()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        &[
            Bgra::BLACK,
            Bgra::BLACK,
            Bgra::WHITE,
            Bgra::BLACK,
            Bgra::BLACK,
        ]
    );
}

#[test]
fn sharpen_effect_is_one_undoable_mutation_and_noops_are_atomic() {
    let mut session = session_from_pixels(
        3,
        1,
        vec![
            Bgra::new(100, 100, 100, 255),
            Bgra::new(200, 200, 200, 255),
            Bgra::new(100, 100, 100, 255),
        ],
    );
    assert!(
        session
            .execute(Command::ApplyEffect(Effect::Sharpen { amount: 10 }))
            .expect("sharpen")
    );
    assert!(session.can_undo());
    assert_eq!(pixel(&session, 0, 0), Bgra::new(75, 75, 75, 255));
    assert_eq!(pixel(&session, 1, 0), Bgra::new(250, 250, 250, 255));
    assert_eq!(pixel(&session, 2, 0), Bgra::new(75, 75, 75, 255));

    assert!(session.undo().unwrap());
    assert!(!session.dirty());
    assert_eq!(pixel(&session, 1, 0), Bgra::new(200, 200, 200, 255));
    assert!(session.redo().unwrap());
    assert_eq!(pixel(&session, 1, 0), Bgra::new(250, 250, 250, 255));
    assert!(session.undo().unwrap());

    assert!(
        !session
            .execute(Command::ApplyEffect(Effect::Sharpen { amount: 0 }))
            .expect("zero amount")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::Sharpen {
                amount: MAX_SHARPEN_AMOUNT + 1,
            }))
            .is_err()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(pixel(&session, 1, 0), Bgra::new(200, 200, 200, 255));
}

#[test]
fn add_noise_effect_is_deterministic_undoable_and_noops_are_atomic() {
    let original = vec![
        Bgra::new(10, 20, 30, 0),
        Bgra::new(100, 110, 120, 17),
        Bgra::new(150, 160, 170, 128),
        Bgra::new(200, 210, 220, 255),
    ];
    let mut session = session_from_pixels(4, 1, original.clone());
    assert!(
        session
            .execute(Command::ApplyEffect(Effect::AddNoise {
                intensity: 50,
                seed: 42,
            }))
            .expect("noise")
    );
    let noisy = session
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    for (before, after) in original.iter().zip(&noisy) {
        assert_eq!(after.a, before.a, "noise must preserve alpha");
    }
    assert!(session.undo().unwrap());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        original.as_slice()
    );

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::AddNoise {
                intensity: 50,
                seed: 42,
            }))
            .expect("repeated noise")
    );
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        noisy.as_slice(),
        "the same seed must reproduce the same pixels"
    );
    assert!(session.undo().unwrap());
    assert!(
        session
            .execute(Command::ApplyEffect(Effect::AddNoise {
                intensity: 50,
                seed: 43,
            }))
            .expect("different seed")
    );
    assert_ne!(
        session.document().active_layer().surface().pixels(),
        noisy.as_slice(),
        "a different seed must produce different pixels"
    );
    assert!(session.undo().unwrap());

    assert!(
        !session
            .execute(Command::ApplyEffect(Effect::AddNoise {
                intensity: 0,
                seed: 1,
            }))
            .expect("zero intensity")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(
        session
            .execute(Command::ApplyEffect(Effect::AddNoise {
                intensity: MAX_NOISE_INTENSITY + 1,
                seed: 1,
            }))
            .is_err()
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(
        session.document().active_layer().surface().pixels(),
        original.as_slice()
    );
}
