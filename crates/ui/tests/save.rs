use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Size, Surface};
use paint_ui::PaintApp;
use tempfile::TempDir;

const DIRTY: &str = " •";

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness.state_mut().use_native_file_dialogs(false);
    harness
}

fn click(harness: &mut Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run();
}

fn open_menu(harness: &mut Harness<'_, PaintApp>, menu: &str) {
    click(harness, menu);
}

fn menu_item(harness: &mut Harness<'_, PaintApp>, menu: &str, item: &str) {
    open_menu(harness, menu);
    click(harness, item);
}

fn close_menu(harness: &mut Harness<'_, PaintApp>) {
    harness.key_press(Key::Escape);
    harness.run();
}

fn menu_item_disabled(harness: &Harness<'_, PaintApp>, label: &str) -> bool {
    harness
        .get_by_role_and_label(Role::Button, label)
        .accesskit_node()
        .is_disabled()
}

fn press_command(harness: &mut Harness<'_, PaintApp>, key: Key) {
    harness.key_press_modifiers(Modifiers::COMMAND, key);
    harness.run();
}

fn enter_path(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    let field = harness.get_by_label("File path");
    field.click();
    harness.run();
    let field = harness.get_by_label("File path");
    assert!(field.is_focused(), "the path field must receive focus");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness
        .get_by_label("File path")
        .type_text(path.to_str().expect("UTF-8 test path"));
    harness.run();
}

fn write_input_png(path: &Path) -> Surface {
    let source = Surface::from_pixels(
        Size::new(2, 1).expect("test size"),
        vec![Bgra::new(1, 2, 3, 255), Bgra::new(20, 30, 40, 128)],
    )
    .expect("test surface");
    paint_image_io::write_png_atomic(path, &source).expect("write test PNG");
    source
}

fn open_png(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    menu_item(harness, "File", "Open PNG…");
    enter_path(harness, path);
    click(harness, "Open PNG");
}

fn save_as(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    menu_item(harness, "File", "Save As…");
    enter_path(harness, path);
    click(harness, "Save");
}

fn file_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .expect("file name")
        .to_owned()
}

#[test]
fn save_menu_is_enabled_only_for_a_dirty_document_with_a_path() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    write_input_png(&input);
    let mut harness = app();

    open_menu(&mut harness, "File");
    assert!(
        menu_item_disabled(&harness, "Save"),
        "a new document has no path, so Save must be disabled"
    );
    assert!(
        !menu_item_disabled(&harness, "Save As…"),
        "Save As must stay available for a new document"
    );
    close_menu(&mut harness);

    open_png(&mut harness, &input);
    open_menu(&mut harness, "File");
    assert!(
        menu_item_disabled(&harness, "Save"),
        "a clean document has nothing to save"
    );
    close_menu(&mut harness);

    menu_item(&mut harness, "Adjustments", "Invert Colors");
    assert!(harness.state().session().dirty());
    open_menu(&mut harness, "File");
    assert!(
        !menu_item_disabled(&harness, "Save"),
        "a dirty document with a path must enable Save"
    );
    assert!(!menu_item_disabled(&harness, "Save As…"));
    close_menu(&mut harness);
}

#[test]
fn save_as_cancel_preserves_dirty_state_and_the_prior_path() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    let cancelled = temporary.path().join("cancelled.png");
    write_input_png(&input);
    let mut harness = app();
    open_png(&mut harness, &input);
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before = harness.state().session().snapshot().expect("snapshot");
    assert!(before.dirty);
    assert_eq!(
        harness.state().active_document_path(),
        Some(input.as_path())
    );

    menu_item(&mut harness, "File", "Save As…");
    enter_path(&mut harness, &cancelled);
    click(&mut harness, "Cancel");

    assert!(
        !cancelled.exists(),
        "a cancelled Save As must not write a file"
    );
    assert!(harness.state().session().dirty());
    assert_eq!(
        harness.state().active_document_path(),
        Some(input.as_path()),
        "a cancelled Save As must preserve the prior path"
    );
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert_eq!(
        harness.state().document_titles().last().map(String::as_str),
        Some(file_title(&input).as_str()),
        "a cancelled Save As must preserve the prior title"
    );
    harness.get_by_role_and_label(
        Role::Button,
        &format!("Document tab: {}{DIRTY}", file_title(&input)),
    );
}

#[test]
fn save_as_writes_updates_path_and_title_and_clears_dirty() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    let output = temporary.path().join("saved.png");
    write_input_png(&input);
    let mut harness = app();
    open_png(&mut harness, &input);
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before = harness.state().session().snapshot().expect("snapshot");
    let history = harness.state().history_entries().to_vec();
    let composed = harness.state().session().compose().expect("composed");
    assert!(before.dirty);

    save_as(&mut harness, &output);

    assert_eq!(
        harness.state().active_document_path(),
        Some(output.as_path())
    );
    assert!(
        !harness.state().session().dirty(),
        "a successful Save As must mark the document clean"
    );
    assert_eq!(
        harness.state().document_titles().last().map(String::as_str),
        Some(file_title(&output).as_str()),
        "Save As must retitle the tab with the new file name"
    );
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        {
            let mut expected = before;
            expected.dirty = false;
            expected
        },
        "Save As must not change content or history"
    );
    assert_eq!(harness.state().history_entries(), history);
    let decoded = paint_image_io::decode_png(&fs::read(&output).expect("saved PNG"))
        .expect("decode saved PNG");
    assert_eq!(decoded, composed);
    harness.get_by_role_and_label(
        Role::Button,
        &format!("Document tab: {}", file_title(&output)),
    );
}

#[test]
fn command_s_saves_atomically_to_the_tab_path_without_touching_history() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    write_input_png(&input);
    let mut harness = app();
    open_png(&mut harness, &input);
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let history = harness.state().history_entries().to_vec();
    let before = harness.state().session().snapshot().expect("snapshot");
    let composed = harness.state().session().compose().expect("composed");
    assert!(before.dirty);

    press_command(&mut harness, Key::S);

    assert_eq!(
        harness.state().active_document_path(),
        Some(input.as_path()),
        "Save must keep writing to the tab path"
    );
    assert!(!harness.state().session().dirty());
    assert_eq!(harness.state().history_entries(), history);
    assert_eq!(harness.state().session().snapshot().expect("snapshot"), {
        let mut expected = before;
        expected.dirty = false;
        expected
    });
    let decoded = paint_image_io::decode_png(&fs::read(&input).expect("saved PNG"))
        .expect("decode saved PNG");
    assert_eq!(decoded, composed);
}

#[test]
fn failed_save_as_preserves_dirty_state_path_and_title() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    let missing = temporary.path().join("missing/output.png");
    write_input_png(&input);
    let mut harness = app();
    open_png(&mut harness, &input);
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before = harness.state().session().snapshot().expect("snapshot");

    save_as(&mut harness, &missing);

    assert!(!missing.exists());
    assert!(harness.state().session().dirty());
    assert_eq!(
        harness.state().active_document_path(),
        Some(input.as_path()),
        "a failed write must preserve the prior path"
    );
    assert_eq!(
        harness.state().document_titles().last().map(String::as_str),
        Some(file_title(&input).as_str())
    );
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(
        harness.state().status().contains("Save failed"),
        "failed saves must report an error: {}",
        harness.state().status()
    );
}

#[test]
fn save_shortcuts_prompt_for_a_path_when_the_document_has_none() {
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    assert!(harness.state().session().dirty());
    assert_eq!(harness.state().active_document_path(), None);

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    harness.run();
    harness.get_by_label("File path");
    click(&mut harness, "Cancel");
    assert!(harness.state().session().dirty());
    assert_eq!(harness.state().active_document_path(), None);

    press_command(&mut harness, Key::S);
    harness.get_by_label("File path");
    click(&mut harness, "Cancel");
    assert!(harness.state().session().dirty());
    assert_eq!(harness.state().active_document_path(), None);
}

#[test]
fn switching_tabs_preserves_each_document_path_and_dirty_marker() {
    let temporary = TempDir::new().expect("temporary directory");
    let first = temporary.path().join("first.png");
    let second = temporary.path().join("second.png");
    write_input_png(&first);
    let mut harness = app();
    open_png(&mut harness, &first);
    harness.get_by_role_and_label(
        Role::Button,
        &format!("Document tab: {}", file_title(&first)),
    );

    menu_item(&mut harness, "Adjustments", "Invert Colors");
    assert!(harness.state().session().dirty());
    harness.get_by_role_and_label(
        Role::Button,
        &format!("Document tab: {}{DIRTY}", file_title(&first)),
    );

    click(&mut harness, "New document");
    click(&mut harness, "Apply");
    assert_eq!(harness.state().active_document_path(), None);
    assert_eq!(
        harness.state().document_display_titles(),
        [
            "New Image".to_owned(),
            format!("{}{DIRTY}", file_title(&first)),
            "New Image 3".to_owned(),
        ],
        "the inactive dirty tab must keep its dirty marker"
    );
    assert_eq!(
        harness.state().active_document_path(),
        None,
        "a new document has no path"
    );

    menu_item(&mut harness, "Adjustments", "Invert Colors");
    save_as(&mut harness, &second);

    click(
        &mut harness,
        &format!("Document tab: {}{DIRTY}", file_title(&first)),
    );
    assert_eq!(harness.state().active_document_index(), 1);
    assert_eq!(
        harness.state().active_document_path(),
        Some(first.as_path()),
        "switching back must restore the first tab's path"
    );
    assert!(harness.state().session().dirty());
    let first_before_save = harness.state().session().snapshot().expect("snapshot");
    let first_composed = harness.state().session().compose().expect("composed");

    click(
        &mut harness,
        &format!("Document tab: {}", file_title(&second)),
    );
    assert_eq!(harness.state().active_document_index(), 2);
    assert_eq!(
        harness.state().active_document_path(),
        Some(second.as_path()),
        "switching back must restore the second tab's path"
    );
    assert!(!harness.state().session().dirty());
    harness.get_by_role_and_label(
        Role::Button,
        &format!("Document tab: {}", file_title(&second)),
    );

    click(
        &mut harness,
        &format!("Document tab: {}{DIRTY}", file_title(&first)),
    );
    press_command(&mut harness, Key::S);
    assert!(!harness.state().session().dirty());
    assert_eq!(harness.state().session().snapshot().expect("snapshot"), {
        let mut expected = first_before_save;
        expected.dirty = false;
        expected
    });
    let saved_first = paint_image_io::decode_png(&fs::read(&first).expect("first PNG"))
        .expect("decode first PNG");
    assert_eq!(saved_first, first_composed);
    let saved_second = paint_image_io::decode_png(&fs::read(&second).expect("second PNG"))
        .expect("decode second PNG");
    assert_ne!(
        saved_second, first_composed,
        "saving the first tab must not overwrite the second tab's file"
    );
}

#[test]
fn document_paths_accessor_round_trips_through_tab_switches() {
    let temporary = TempDir::new().expect("temporary directory");
    let first = temporary.path().join("first.png");
    let second = temporary.path().join("second.png");
    write_input_png(&first);
    write_input_png(&second);
    let mut harness = app();
    open_png(&mut harness, &first);
    open_png(&mut harness, &second);
    assert_eq!(
        harness.state().document_paths(),
        [
            None,
            Some(PathBuf::from(&first)),
            Some(PathBuf::from(&second)),
        ]
        .to_vec()
    );
}

#[test]
fn save_as_infers_jpeg_from_the_path_and_writes_a_real_jpeg() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("photo.jpg");
    let mut harness = app();
    menu_item(&mut harness, "File", "Save As…");
    enter_path(&mut harness, &path);
    click(&mut harness, "Save");

    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");
    assert_eq!(
        harness.state().status(),
        format!("Saved JPEG: {}", path.display())
    );
    assert_eq!(harness.state().active_document_path(), Some(path.as_path()));
}

#[test]
fn save_as_format_picker_updates_the_extension_and_saves_pdn() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("layered.png");
    let mut harness = app();
    click(&mut harness, "Add Layer");
    menu_item(&mut harness, "File", "Save As…");
    enter_path(&mut harness, &path);
    harness.get_by_role_and_label(Role::Button, "Browse…");
    harness
        .get_by_role_and_label(Role::ComboBox, "File type")
        .click();
    harness.run();
    harness.get_by_label_contains("Paint.NET (*.pdn)").click();
    harness.run();
    assert_eq!(
        harness.get_by_label("File path").value().as_deref(),
        Some(directory.path().join("layered.pdn").to_str().unwrap()),
        "selecting PDN must rewrite the extension"
    );
    click(&mut harness, "Save");

    let bytes = fs::read(directory.path().join("layered.pdn")).unwrap();
    assert_eq!(&bytes[..4], b"PDN3");
    assert_eq!(harness.state().session().document().layers().len(), 2);
}

#[test]
fn export_format_picker_writes_jpeg_without_cleaning_the_document() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("export.jpg");
    let mut harness = app();
    click(&mut harness, "Add Layer");
    menu_item(&mut harness, "File", "Export Image…");
    enter_path(&mut harness, &path);
    click(&mut harness, "Export");

    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
    assert!(harness.state().session().dirty());
    assert!(harness.state().active_document_path().is_none());
}

#[test]
fn save_as_file_type_lists_every_implemented_format() {
    let mut harness = app();
    menu_item(&mut harness, "File", "Save As…");
    harness
        .get_by_role_and_label(Role::ComboBox, "File type")
        .click();
    harness.run();
    for label in [
        "Paint.NET (*.pdn)",
        "PNG (*.png)",
        "JPEG (*.jpg; *.jpeg; *.jpe; *.jfif; *.exif)",
        "TIFF (*.tiff; *.tif)",
        "BMP (*.bmp; *.dib)",
        "TGA (*.tga)",
        "DDS (*.dds)",
        "GIF (*.gif)",
        "WebP (*.webp)",
    ] {
        harness.get_by_label_contains(label);
    }
}
