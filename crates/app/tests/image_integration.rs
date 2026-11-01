use paint_app::{
    AppSession, Command, RasterFormat, ResourceLimits, SemanticSnapshot, execute_trace,
};
use paint_core::{Bgra, Point, Size, Surface};

#[test]
fn png_bytes_open_as_one_clean_layer_and_export_exact_rgba() {
    let size = Size::new(2, 1).unwrap();
    let pixels = vec![Bgra::new(1, 2, 3, 4), Bgra::new(200, 150, 100, 255)];
    let surface = Surface::from_pixels(size, pixels.clone()).unwrap();
    let encoded = paint_image_io::encode_png(&surface).unwrap();

    let session = AppSession::from_png_bytes(&encoded).unwrap();
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.document().layers().len(), 1);
    assert!(session.document().layers()[0].is_background());
    assert_eq!(session.document().layers()[0].surface().pixels(), pixels);

    let document_before = session.document().clone();
    let exported = session.export_png().unwrap();
    let decoded = paint_image_io::decode_png(&exported).unwrap();
    assert_eq!(decoded.pixels(), pixels);
    assert_eq!(session.document(), &document_before);
    assert!(!session.dirty());
}

#[test]
fn layered_png_export_is_explicitly_composited_and_does_not_flatten_or_clean() {
    let mut session =
        AppSession::new(paint_document::Document::new(Size::new(1, 1).unwrap()).unwrap());
    session
        .execute(Command::AddLayer {
            name: "red overlay".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
        .unwrap();
    let (composed, _) = session.compose_and_snapshot().unwrap();
    assert!(session.dirty());

    let encoded = session.export_png().unwrap();
    let decoded = paint_image_io::decode_png(&encoded).unwrap();
    assert_eq!(decoded, composed);
    assert_eq!(session.document().layers().len(), 2);
    assert!(session.dirty());
    assert!(session.can_undo());

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("composite.png");
    paint_image_io::atomic_write(&path, b"old contents").unwrap();
    session.export_png_atomic(&path).unwrap();
    let on_disk = std::fs::read(&path).unwrap();
    let decoded = paint_image_io::decode_png(&on_disk).unwrap();
    assert_eq!(decoded, composed);
    assert_eq!(session.document().layers().len(), 2);
    assert!(session.dirty());
}

#[test]
fn png_path_open_is_clean_and_reports_io_and_decode_failures() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.png");
    let surface = Surface::filled(Size::new(1, 1).unwrap(), Bgra::new(9, 8, 7, 6)).unwrap();
    paint_image_io::write_png_atomic(&path, &surface).unwrap();

    let session = AppSession::open_png(&path).unwrap();
    assert_eq!(session.document().layers()[0].surface(), &surface);
    assert!(!session.dirty());
    assert!(!session.can_undo());

    assert!(AppSession::open_png(directory.path().join("missing.png")).is_err());
    assert!(AppSession::from_png_bytes(b"not a PNG").is_err());
}

#[test]
fn jpeg_and_webp_open_as_one_clean_layer_named_after_the_file() {
    let size = Size::new(5, 4).unwrap();
    let surface = Surface::filled(size, Bgra::new(20, 40, 80, 255)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    for (format, stem, extension) in [
        (RasterFormat::Jpeg, "photo", "jpg"),
        (RasterFormat::WebP, "icon", "webp"),
    ] {
        let bytes = paint_image_io::encode_image(&surface, format).unwrap();
        let path = directory.path().join(format!("{stem}.{extension}"));
        std::fs::write(&path, &bytes).unwrap();

        let session = AppSession::open_image(&path).unwrap();
        assert_eq!(session.document().layers().len(), 1);
        assert_eq!(session.document().size(), size);
        assert!(!session.dirty());
        assert!(!session.can_undo());
        assert!(!session.can_redo());
        let layer = &session.document().layers()[0];
        assert_eq!(layer.name(), stem);
        assert!(layer.is_background());
        let pixel = layer.surface().pixel(Point::new(2, 2)).unwrap();
        assert_eq!(pixel.a, 255);
        for (actual, expected) in [(pixel.r, 80), (pixel.g, 40), (pixel.b, 20)] {
            assert!(
                actual.abs_diff(expected) <= 30,
                "{} channel {actual} vs {expected}",
                format.name()
            );
        }

        let from_bytes = AppSession::from_image_bytes(&bytes, format).unwrap();
        assert_eq!(
            from_bytes.document().layers()[0].name(),
            format.name(),
            "byte-open layer name"
        );
        assert_eq!(from_bytes.document().layers().len(), 1);
        assert!(!from_bytes.dirty());
        assert!(!from_bytes.can_undo());

        assert!(
            AppSession::open_image_with_limits(
                &path,
                ResourceLimits {
                    max_pixels: 19,
                    ..ResourceLimits::default()
                }
            )
            .is_err(),
            "session limits must be applied while opening {}",
            format.name()
        );
    }
}

#[test]
fn every_flat_format_opens_as_one_clean_background_layer() {
    let size = Size::new(3, 2).unwrap();
    let surface = Surface::filled(size, Bgra::new(30, 60, 90, 255)).unwrap();
    let directory = tempfile::tempdir().unwrap();
    for format in RasterFormat::ALL {
        let bytes = paint_image_io::encode_image(&surface, format).unwrap();
        let path = directory.path().join(format!(
            "{}.{}",
            format.name().to_ascii_lowercase(),
            format.extension()
        ));
        std::fs::write(&path, &bytes).unwrap();

        let session = AppSession::open_image(&path).unwrap();
        assert_eq!(session.document().size(), size, "{} size", format.name());
        assert_eq!(
            session.document().layers().len(),
            1,
            "{} layer count",
            format.name()
        );
        assert!(
            session.document().layers()[0].is_background(),
            "{} background",
            format.name()
        );
        assert!(!session.dirty(), "{} clean", format.name());
        assert!(!session.can_undo(), "{} history", format.name());
        assert!(!session.can_redo(), "{} redo", format.name());
    }
}

#[test]
fn png_open_applies_session_resource_limits_during_decode() {
    let surface = Surface::filled(Size::new(2, 2).unwrap(), Bgra::WHITE).unwrap();
    let encoded = paint_image_io::encode_png(&surface).unwrap();
    let limits = ResourceLimits {
        max_pixels: 3,
        ..ResourceLimits::default()
    };

    assert!(AppSession::from_png_bytes_with_limits(&encoded, limits).is_err());
}

#[test]
fn trace_output_writes_protocol_files_atomically_and_can_replace_them() {
    let output = execute_trace(include_str!(
        "../../../tests/e2e/traces/undo-invert-2x2.json"
    ))
    .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("nested/output");

    output.write_to_directory(&directory).unwrap();
    let composite_path = directory.join("composite.png");
    let snapshot_path = directory.join("snapshot.json");
    let decoded = paint_image_io::decode_png(&std::fs::read(&composite_path).unwrap()).unwrap();
    assert_eq!(decoded, output.surface);
    let snapshot: SemanticSnapshot =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    assert_eq!(snapshot, output.snapshot);

    paint_image_io::atomic_write(&composite_path, b"replace me").unwrap();
    paint_image_io::atomic_write(&snapshot_path, b"replace me too").unwrap();
    output.write_to_directory(&directory).unwrap();
    let decoded = paint_image_io::decode_png(&std::fs::read(composite_path).unwrap()).unwrap();
    assert_eq!(decoded, output.surface);
    let snapshot: SemanticSnapshot =
        serde_json::from_slice(&std::fs::read(snapshot_path).unwrap()).unwrap();
    assert_eq!(snapshot, output.snapshot);
}

#[test]
fn save_png_atomic_clears_dirty_without_touching_content_or_history() {
    let mut session =
        AppSession::new(paint_document::Document::new(Size::new(1, 1).unwrap()).unwrap());
    session
        .execute(Command::AddLayer {
            name: "red overlay".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
        .unwrap();
    assert!(session.dirty());
    assert!(session.can_undo());
    let composed = session.compose().unwrap();
    let before = session.snapshot().unwrap();
    assert!(before.dirty);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.png");
    session.save_png_atomic(&path).unwrap();

    assert!(!session.dirty());
    assert!(session.can_undo());
    assert_eq!(session.document().layers().len(), 2);
    let decoded = paint_image_io::decode_png(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(decoded, composed);
    let mut expected = before;
    expected.dirty = false;
    assert_eq!(
        session.snapshot().unwrap(),
        expected,
        "saving must change only the dirty baseline, not content or history"
    );

    session.undo().unwrap();
    assert!(
        session.dirty(),
        "undo past the saved baseline must become dirty again"
    );
}

#[test]
fn failed_save_preserves_content_dirty_state_and_history() {
    let mut session =
        AppSession::new(paint_document::Document::new(Size::new(1, 1).unwrap()).unwrap());
    session
        .execute(Command::Fill(Bgra::new(1, 2, 3, 255)))
        .unwrap();
    assert!(session.dirty());
    let before = session.snapshot().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing/output.png");

    assert!(session.save_png_atomic(&missing).is_err());
    assert!(session.dirty());
    assert_eq!(session.snapshot().unwrap(), before);
    assert!(!missing.exists());
}
