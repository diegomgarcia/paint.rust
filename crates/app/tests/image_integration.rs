use paint_app::{AppSession, Command, ResourceLimits, SemanticSnapshot, execute_trace};
use paint_core::{Bgra, Size, Surface};

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
