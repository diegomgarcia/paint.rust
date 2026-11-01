use std::path::Path;

use eframe::egui;
use egui_kittest::Harness;
use paint_app::{AppSession, Command};
use paint_core::{Bgra, Size, Surface};
use paint_document::{Document, LayerId};
use paint_ui::PaintApp;
use tempfile::TempDir;

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness.state_mut().use_native_file_dialogs(false);
    harness
}

fn sample() -> Surface {
    Surface::from_pixels(
        Size::new(2, 1).unwrap(),
        vec![Bgra::new(0, 0, 255, 255), Bgra::new(0, 255, 0, 128)],
    )
    .unwrap()
}

#[test]
fn ui_opens_a_saved_jpeg_webp_and_png_as_one_clean_layer() {
    let directory = TempDir::new().unwrap();
    let formats = [
        (paint_image_io::RasterFormat::Png, "sample.png"),
        (paint_image_io::RasterFormat::Jpeg, "sample.jpg"),
        (paint_image_io::RasterFormat::WebP, "sample.webp"),
    ];
    for (format, name) in formats {
        let path = directory.path().join(name);
        paint_image_io::write_image_atomic(&path, &sample(), format).unwrap();
        let mut harness = app();
        harness.state_mut().open_path_for_test(&path);
        harness.run();
        assert_eq!(harness.state().session().document().layers().len(), 1);
        assert_eq!(
            harness.state().session().document().size(),
            Size::new(2, 1).unwrap()
        );
        assert!(!harness.state().session().dirty());
        assert_eq!(harness.state().active_document_path(), Some(path.as_path()));
    }
}

#[test]
fn ui_opens_pdr_with_layers_and_undo_history() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("session.pdr");
    let mut session = AppSession::new(Document::new(Size::new(3, 2).unwrap()).unwrap());
    session
        .execute(Command::AddLayer {
            name: "Ink".to_owned(),
            opacity: 240,
            visible: true,
        })
        .unwrap();
    session
        .execute(Command::Fill(Bgra::new(0, 0, 255, 255)))
        .unwrap();
    session
        .save_pdr_atomic_with_labels(
            &path,
            Some(&paint_app::PdrHistoryLabels {
                undo: vec!["Add Layer".to_owned(), "Fill".to_owned()],
                redo: Vec::new(),
            }),
        )
        .unwrap();

    let mut harness = app();
    harness.state_mut().open_path_for_test(&path);
    harness.run();
    assert_eq!(harness.state().session().document().layers().len(), 2);
    assert_eq!(
        harness.state().session().document().layers()[1].name(),
        "Ink"
    );
    assert_eq!(harness.state().session().undo_len(), 2);
    assert_eq!(harness.state().history_entries().len(), 2);
    assert!(harness.state().session().can_undo());
}

#[test]
fn ui_opens_pdn_with_layers() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("layers.pdn");
    let mut session = AppSession::new(Document::new(Size::new(2, 2).unwrap()).unwrap());
    session
        .execute(Command::AddLayer {
            name: "Second".to_owned(),
            opacity: 255,
            visible: true,
        })
        .unwrap();
    session.save_pdn_atomic(&path).unwrap();

    let mut harness = app();
    harness.state_mut().open_path_for_test(&path);
    harness.run();
    assert_eq!(harness.state().session().document().layers().len(), 2);
    assert_eq!(
        harness.state().session().document().layers()[1].id(),
        LayerId::new(2)
    );
}

#[test]
fn ui_rejects_unknown_extensions_without_replacing_the_tab() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    harness
        .state_mut()
        .open_path_for_test(Path::new("/tmp/not-an-image.txt"));
    harness.run();
    assert_eq!(harness.state().session().document(), &before);
    assert!(harness.state().status().contains("Unsupported file type"));
}
