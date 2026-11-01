use paint_app::{
    AppSession, Command, Effect, PDR_MAGIC, PDR_VERSION, PdrError, PdrHistoryLabels, PdrLimits,
    ResourceLimitError, ResourceLimits, SessionError,
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{BlendMode, Document, Layer, LayerId, SelectionCombineMode};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("nonzero test dimensions")
}

fn background_session(width: u32, height: u32, color: Bgra) -> AppSession {
    let size = size(width, height);
    let surface = Surface::filled(size, color).expect("test surface");
    let layer = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
    let document = Document::from_layers(size, vec![layer]).expect("test document");
    AppSession::new(document)
}

fn pixel(session: &AppSession, layer: usize, x: u32, y: u32) -> Bgra {
    *session.document().layers()[layer]
        .surface()
        .pixel(Point::new(x, y))
        .expect("in-bounds pixel")
}

fn assert_content_eq(actual: &Document, expected: &Document) {
    assert_eq!(actual.size(), expected.size());
    assert_eq!(actual.active_layer_id(), expected.active_layer_id());
    assert_eq!(actual.selection_model(), expected.selection_model());
    assert_eq!(actual.layers().len(), expected.layers().len());
    for (actual, expected) in actual.layers().iter().zip(expected.layers()) {
        assert_eq!(actual.id(), expected.id());
        assert_eq!(actual.name(), expected.name());
        assert_eq!(actual.is_visible(), expected.is_visible());
        assert_eq!(actual.opacity(), expected.opacity());
        assert_eq!(actual.is_background(), expected.is_background());
        assert_eq!(actual.blend_mode(), expected.blend_mode());
        assert_eq!(actual.surface().size(), expected.surface().size());
        assert_eq!(actual.surface().pixels(), expected.surface().pixels());
    }
}

fn open_bytes(bytes: &[u8]) -> Result<AppSession, SessionError> {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("document.pdr");
    std::fs::write(&path, bytes).expect("write PDR bytes");
    AppSession::open_pdr(&path)
}

#[test]
fn layered_document_with_overlay_and_mask_selection_roundtrips_exactly() {
    let mut session = background_session(4, 3, Bgra::WHITE);
    let mut states = vec![session.document().clone()];

    assert!(session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    states.push(session.document().clone());
    assert!(
        session
            .execute(Command::AddLayer {
                name: "Ink".to_owned(),
                opacity: 180,
                visible: true,
            })
            .unwrap()
    );
    states.push(session.document().clone());
    assert!(
        session
            .execute(Command::SetLayerBlendMode {
                index: 1,
                mode: BlendMode::Overlay,
            })
            .unwrap()
    );
    states.push(session.document().clone());
    assert!(
        session
            .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
            .unwrap()
    );
    states.push(session.document().clone());

    session.begin_preview();
    assert!(session.preview_effect(Effect::Grayscale).unwrap());
    assert!(session.finish_preview().unwrap());
    states.push(session.document().clone());

    let ellipse = Rect::new(1, 0, 4, 3).expect("ellipse rectangle");
    assert!(
        session
            .execute(Command::SelectEllipse {
                rect: ellipse,
                mode: SelectionCombineMode::Replace,
            })
            .unwrap()
    );
    assert!(session.document().selection_mask().is_some());
    let final_document = session.document().clone();
    *states.last_mut().expect("preview state") = final_document.clone();
    assert_eq!(
        session.document().layers()[1].blend_mode(),
        BlendMode::Overlay
    );

    let labels = PdrHistoryLabels {
        undo: vec![
            "Fill".to_owned(),
            "Add layer".to_owned(),
            "Blend mode".to_owned(),
            "Fill ink".to_owned(),
            "Grayscale preview".to_owned(),
        ],
        redo: Vec::new(),
    };
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("session.pdr");
    session
        .save_pdr_atomic_with_labels(&path, Some(&labels))
        .unwrap();
    assert!(!session.dirty());
    assert_eq!(
        std::fs::read(&path).expect("saved bytes")[..PDR_MAGIC.len()],
        PDR_MAGIC
    );

    let (mut loaded, loaded_labels) = AppSession::open_pdr_with_limits_and_labels(
        &path,
        ResourceLimits::default(),
        PdrLimits::default(),
    )
    .unwrap();
    assert_eq!(loaded.document(), &final_document);
    assert_eq!(loaded_labels, Some(labels));
    assert!(!loaded.dirty());
    assert!(loaded.can_undo());
    assert!(!loaded.can_redo());

    for expected in states.iter().rev().skip(1) {
        assert!(loaded.undo().unwrap());
        assert_content_eq(loaded.document(), expected);
    }
    assert!(!loaded.can_undo());
    assert!(loaded.can_redo());

    for expected in states.iter().skip(1) {
        assert!(loaded.redo().unwrap());
        assert_content_eq(loaded.document(), expected);
    }
    assert_eq!(loaded.document(), &final_document);
    assert!(loaded.document().selection_mask().is_some());
}

#[test]
fn undo_and_redo_survive_save_load_then_new_edits_mix_with_snapshots() {
    let mut session = background_session(2, 2, Bgra::WHITE);
    assert!(session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    assert!(
        session
            .execute(Command::AddLayer {
                name: "Top".to_owned(),
                opacity: 255,
                visible: true,
            })
            .unwrap()
    );
    assert!(
        session
            .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
            .unwrap()
    );
    assert!(
        session
            .execute(Command::SetLayerOpacity {
                index: 1,
                opacity: 100,
            })
            .unwrap()
    );

    for _ in 0..4 {
        assert!(session.undo().unwrap());
    }
    assert!(!session.can_undo());
    assert!(session.can_redo());
    let saved_document = session.document().clone();

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("history.pdr");
    session.save_pdr_atomic(&path).unwrap();

    let mut loaded = AppSession::open_pdr(&path).unwrap();
    assert_eq!(loaded.document(), &saved_document);
    assert!(!loaded.dirty());
    assert!(!loaded.can_undo());
    assert!(loaded.can_redo());

    assert!(loaded.redo().unwrap());
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::BLACK);
    assert!(loaded.redo().unwrap());
    assert_eq!(loaded.document().layers().len(), 2);
    assert_eq!(loaded.document().layers()[1].name(), "Top");
    assert!(loaded.redo().unwrap());
    assert_eq!(pixel(&loaded, 1, 0, 0), Bgra::new(0, 0, 255, 255));
    assert_eq!(loaded.document().layers()[1].opacity(), 255);
    assert!(loaded.redo().unwrap());
    assert_eq!(loaded.document().layers()[1].opacity(), 100);
    assert!(!loaded.can_redo());

    assert!(loaded.undo().unwrap());
    assert_eq!(loaded.document().layers()[1].opacity(), 255);
    assert!(loaded.undo().unwrap());
    assert_eq!(
        loaded.document().layers()[1].surface().pixels(),
        &[Bgra::TRANSPARENT; 4]
    );
    assert!(loaded.undo().unwrap());
    assert_eq!(loaded.document().layers().len(), 1);
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::BLACK);
    assert!(loaded.undo().unwrap());
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::WHITE);
    assert!(!loaded.can_undo());
    assert!(loaded.can_redo());

    assert!(
        loaded
            .execute(Command::Fill(Bgra::new(0, 255, 0, 255)))
            .unwrap()
    );
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::new(0, 255, 0, 255));
    assert!(loaded.can_undo());
    assert!(!loaded.can_redo(), "a new edit clears loaded redo history");
    assert!(loaded.undo().unwrap());
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::WHITE);
    assert!(!loaded.dirty());
    assert!(loaded.can_redo());
}

#[test]
fn mixed_undo_and_redo_stacks_survive_save_and_load() {
    let mut session = background_session(2, 2, Bgra::WHITE);
    assert!(session.execute(Command::Fill(Bgra::BLACK)).unwrap());
    assert!(
        session
            .execute(Command::AddLayer {
                name: "Top".to_owned(),
                opacity: 255,
                visible: true,
            })
            .unwrap()
    );
    assert!(
        session
            .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
            .unwrap()
    );
    assert!(
        session
            .execute(Command::SetLayerOpacity {
                index: 1,
                opacity: 100,
            })
            .unwrap()
    );
    assert!(session.undo().unwrap());
    assert!(session.undo().unwrap());
    assert!(session.can_undo());
    assert!(session.can_redo());
    let saved_document = session.document().clone();

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("mixed.pdr");
    session.save_pdr_atomic(&path).unwrap();
    let mut loaded = AppSession::open_pdr(&path).unwrap();
    assert_eq!(loaded.document(), &saved_document);

    assert!(loaded.undo().unwrap());
    assert_eq!(loaded.document().layers().len(), 1);
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::BLACK);
    assert!(loaded.undo().unwrap());
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::WHITE);
    assert!(!loaded.can_undo());
    assert!(loaded.can_redo());

    assert!(loaded.redo().unwrap());
    assert_eq!(pixel(&loaded, 0, 0, 0), Bgra::BLACK);
    assert!(loaded.redo().unwrap());
    assert_eq!(loaded.document().layers().len(), 2);
    assert!(loaded.redo().unwrap());
    assert_eq!(pixel(&loaded, 1, 0, 0), Bgra::new(0, 0, 255, 255));
    assert!(loaded.redo().unwrap());
    assert_eq!(loaded.document().layers()[1].opacity(), 100);
    assert!(!loaded.can_redo());
    assert_eq!(loaded.document().layers()[1].opacity(), 100);
}

#[test]
fn rectangle_selection_roundtrips_without_a_mask() {
    let mut session = background_session(3, 3, Bgra::WHITE);
    let rectangle = Rect::new(-1, 1, 2, 4).expect("signed rectangle");
    assert!(
        session
            .execute(Command::SelectRect {
                rect: rectangle,
                mode: SelectionCombineMode::Replace,
            })
            .unwrap()
    );
    assert!(session.document().selection_mask().is_none());
    let expected = session.document().clone();

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("rect.pdr");
    session.save_pdr_atomic(&path).unwrap();
    let loaded = AppSession::open_pdr(&path).unwrap();

    assert_eq!(loaded.document(), &expected);
    assert_eq!(loaded.document().selection(), Some(rectangle));
}

#[test]
fn encode_failure_preserves_an_existing_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("document.pdr");
    std::fs::write(&path, b"prior contents").expect("seed prior file");

    let mut document = Document::new(size(1, 1)).expect("document");
    document
        .add_layer("x".repeat(PdrLimits::default().max_layer_name_bytes + 1))
        .unwrap();
    let mut session = AppSession::new(document);

    let error = session.save_pdr_atomic(&path).unwrap_err();
    assert!(matches!(
        error,
        SessionError::Pdr(PdrError::ResourceLimit {
            resource: "layer name bytes",
            ..
        })
    ));
    assert_eq!(std::fs::read(&path).expect("prior file"), b"prior contents");
}

#[test]
fn atomic_rename_failure_preserves_the_prior_target() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("document.pdr");
    std::fs::create_dir(&target).expect("directory at target");
    let sentinel = target.join("sentinel");
    std::fs::write(&sentinel, b"prior").expect("seed sentinel");

    let mut session = background_session(1, 1, Bgra::WHITE);
    session.execute(Command::Invert).unwrap();

    let error = session.save_pdr_atomic(&target).unwrap_err();
    assert!(matches!(error, SessionError::AtomicWrite(_)));
    assert!(target.is_dir());
    assert_eq!(std::fs::read(&sentinel).expect("sentinel"), b"prior");
    let names = std::fs::read_dir(directory.path())
        .expect("read directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from("document.pdr")]);
}

#[test]
fn corrupted_and_truncated_inputs_are_rejected_with_typed_errors() {
    let session = background_session(2, 2, Bgra::WHITE);
    let bytes = session.encode_pdr(None).unwrap();

    let mut bad_magic = bytes.clone();
    bad_magic[0] ^= 0xff;
    assert!(matches!(
        open_bytes(&bad_magic),
        Err(SessionError::Pdr(PdrError::InvalidMagic))
    ));

    let mut bad_version = bytes.clone();
    bad_version[8..12].copy_from_slice(&(PDR_VERSION + 1).to_le_bytes());
    assert!(matches!(
        open_bytes(&bad_version),
        Err(SessionError::Pdr(PdrError::UnsupportedVersion {
            version,
            supported: PDR_VERSION,
        })) if version == PDR_VERSION + 1
    ));

    assert!(matches!(
        open_bytes(&bytes[..bytes.len() - 1]),
        Err(SessionError::Pdr(PdrError::Truncated {
            context: "body",
            ..
        }))
    ));
    assert!(matches!(
        open_bytes(&bytes[..10]),
        Err(SessionError::Pdr(PdrError::Truncated {
            context: "header",
            ..
        }))
    ));

    let mut bad_checksum = bytes.clone();
    let last = bad_checksum.len() - 1;
    bad_checksum[last] ^= 0xff;
    assert!(matches!(
        open_bytes(&bad_checksum),
        Err(SessionError::Pdr(PdrError::ChecksumMismatch))
    ));

    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        open_bytes(&trailing),
        Err(SessionError::Pdr(PdrError::Malformed {
            reason: "trailing bytes",
            ..
        }))
    ));
}

#[test]
fn document_and_history_resource_limits_are_enforced_while_opening() {
    let mut session = background_session(2, 2, Bgra::WHITE);
    session
        .execute(Command::AddLayer {
            name: "Second".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("limits.pdr");
    session.save_pdr_atomic(&path).unwrap();

    let layer_error = AppSession::open_pdr_with_limits(
        &path,
        ResourceLimits {
            max_layers: 1,
            ..ResourceLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        layer_error,
        SessionError::ResourceLimit(ResourceLimitError::Layers { actual: 2, max: 1 })
    ));

    let dimension_error = AppSession::open_pdr_with_limits(
        &path,
        ResourceLimits {
            max_width: 1,
            ..ResourceLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        dimension_error,
        SessionError::ResourceLimit(ResourceLimitError::Dimensions { .. })
    ));

    let history_error = AppSession::open_pdr_with_limits_and_labels(
        &path,
        ResourceLimits::default(),
        PdrLimits {
            max_history_entries: 0,
            ..PdrLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        history_error,
        SessionError::Pdr(PdrError::ResourceLimit {
            resource: "history entries",
            ..
        })
    ));

    let surface_error = AppSession::open_pdr_with_limits_and_labels(
        &path,
        ResourceLimits::default(),
        PdrLimits {
            max_decoded_surface_bytes: 0,
            ..PdrLimits::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        surface_error,
        SessionError::Pdr(PdrError::ResourceLimit {
            resource: "decoded surface bytes",
            ..
        })
    ));
}

#[test]
fn history_labels_are_optional_and_count_checked() {
    let mut session = background_session(1, 1, Bgra::WHITE);
    session.execute(Command::Invert).unwrap();
    let directory = tempfile::tempdir().expect("temporary directory");

    let unlabeled = directory.path().join("unlabeled.pdr");
    session.save_pdr_atomic(&unlabeled).unwrap();
    let (_unlabeled_session, labels) = AppSession::open_pdr_with_limits_and_labels(
        &unlabeled,
        ResourceLimits::default(),
        PdrLimits::default(),
    )
    .unwrap();
    assert_eq!(labels, None);

    let labels = PdrHistoryLabels {
        undo: vec!["Invert".to_owned()],
        redo: Vec::new(),
    };
    let labeled = directory.path().join("labeled.pdr");
    session
        .save_pdr_atomic_with_labels(&labeled, Some(&labels))
        .unwrap();
    let (_labeled_session, loaded_labels) = AppSession::open_pdr_with_limits_and_labels(
        &labeled,
        ResourceLimits::default(),
        PdrLimits::default(),
    )
    .unwrap();
    assert_eq!(loaded_labels, Some(labels));

    let mismatch = PdrHistoryLabels {
        undo: Vec::new(),
        redo: Vec::new(),
    };
    let error = session
        .save_pdr_atomic_with_labels(directory.path().join("mismatch.pdr"), Some(&mismatch))
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::Pdr(PdrError::LabelCount {
            stack: "undo",
            expected: 1,
            actual: 0,
        })
    ));
}
