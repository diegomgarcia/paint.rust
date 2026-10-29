use std::fs;
use std::path::Path;

use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Rect, Size, Surface};
use paint_ui::PaintApp;
use tempfile::TempDir;

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness
}

fn press(harness: &mut Harness<'_, PaintApp>, key: Key) {
    harness.key_press(key);
    harness.run();
}

fn press_command(harness: &mut Harness<'_, PaintApp>, key: Key) {
    harness.key_press_modifiers(Modifiers::COMMAND, key);
    harness.run();
}

fn press_command_shift(harness: &mut Harness<'_, PaintApp>, key: Key) {
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, key);
    harness.run();
}

fn focus(harness: &mut Harness<'_, PaintApp>, role: Role, label: &str) {
    harness.get_by_role_and_label(role, label).focus();
    harness.run();
    assert!(harness.get_by_role_and_label(role, label).is_focused());
}

fn assert_spin_value(harness: &Harness<'_, PaintApp>, label: &str, expected: f64) {
    assert_eq!(
        harness
            .get_by_role_and_label(Role::SpinButton, label)
            .accesskit_node()
            .numeric_value(),
        Some(expected),
        "unexpected keyboard-edited value for {label}"
    );
}

fn type_path(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    let field = harness.get_by_label("PNG file path");
    assert!(field.is_focused(), "the path field must receive focus");
    field.type_text(path.to_str().expect("UTF-8 test path"));
    harness.run();
}

fn replace_path(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    press_command(harness, Key::A);
    let path = path.to_str().expect("UTF-8 test path");
    let field = harness.get_by_label("PNG file path");
    assert!(field.is_focused());
    field.type_text(path);
    harness.run();
    assert_eq!(
        harness.get_by_label("PNG file path").value().as_deref(),
        Some(path)
    );
}

fn assert_composed(harness: &Harness<'_, PaintApp>, expected: Bgra) {
    assert!(
        harness
            .state()
            .session()
            .compose()
            .expect("composed surface")
            .pixels()
            .iter()
            .all(|pixel| *pixel == expected),
        "unexpected composed pixels; expected {expected:?}"
    );
}

fn assert_clipped_fill(harness: &Harness<'_, PaintApp>) {
    let document = harness.state().session().document();
    let width = document.size().width() as usize;
    let height = document.size().height() as usize;
    let pixels = document.active_layer().surface().pixels();
    let mismatches = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let expected = if *x < 3 && *y < 2 {
                Bgra::BLACK
            } else {
                Bgra::WHITE
            };
            pixels[*y * width + *x] != expected
        })
        .count();
    assert_eq!(
        mismatches, 0,
        "(-2, -1)..(3, 2) must clip to (0, 0)..(3, 2), excluding x=3 and y=2"
    );
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

#[test]
fn command_q_respects_the_dirty_guard() {
    let mut clean = app();
    clean.key_press_modifiers(Modifiers::COMMAND, Key::Q);
    clean.step();
    assert!(
        clean.state().close_approved(),
        "a clean Command-Q must approve the native close"
    );
    clean.run();

    let mut dirty = app();
    press_command(&mut dirty, Key::I);
    press_command(&mut dirty, Key::Q);
    focus(&mut dirty, Role::Button, "Cancel");
    press(&mut dirty, Key::Enter);
    assert!(dirty.state().session().dirty());
    assert!(
        dirty
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none(),
        "Cancel must preserve the dirty session"
    );

    press_command(&mut dirty, Key::Q);
    focus(&mut dirty, Role::Button, "Discard");
    dirty.key_press(Key::Enter);
    dirty.step();
    assert!(
        dirty.state().close_approved(),
        "explicit Discard must approve the close"
    );
    dirty.run();
}

#[test]
fn command_w_respects_the_dirty_guard() {
    let mut clean = app();
    clean.key_press_modifiers(Modifiers::COMMAND, Key::W);
    clean.step();
    assert!(
        clean.state().close_approved(),
        "a clean Command-W must approve the window close"
    );
    clean.run();

    let mut dirty = app();
    press_command(&mut dirty, Key::I);
    press_command(&mut dirty, Key::W);
    assert!(!dirty.state().close_approved());
    dirty.get_by_role_and_label(Role::Button, "Discard");
    focus(&mut dirty, Role::Button, "Cancel");
    press(&mut dirty, Key::Enter);
    assert!(dirty.state().session().dirty());
    assert!(
        dirty
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none()
    );
}

#[test]
fn tab_enter_space_and_dirty_new_are_keyboard_usable() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "File")
            .is_focused()
    );
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Edit")
            .is_focused()
    );
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "View")
            .is_focused()
    );
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Image")
            .is_focused()
    );
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Layers")
            .is_focused()
    );
    press(&mut harness, Key::Tab);
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Adjustments")
            .is_focused()
    );
    press(&mut harness, Key::Enter);
    let focused_label = harness
        .query_by(|node| node.is_focused())
        .and_then(|node| node.accesskit_node().label());
    let invert = harness.get_by_role_and_label(Role::Button, "Invert Colors");
    assert!(
        invert.is_focused(),
        "focused widget after opening Adjustments: {focused_label:?}"
    );
    press(&mut harness, Key::Space);
    assert_composed(&harness, Bgra::BLACK);
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());
    assert!(!harness.state().session().can_redo());
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");

    press_command(&mut harness, Key::N);
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    assert!(!harness.state().session().dirty());
    assert_composed(&harness, Bgra::WHITE);

    harness
        .get_by_role_and_label(Role::Button, "Document tab: New Image")
        .click();
    harness.run();
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness
        .get_by_role_and_label(Role::Button, "Document tab: New Image 2")
        .click();
    harness.run();
    assert!(!harness.state().session().dirty());

    focus(&mut harness, Role::Button, "Help");
    press(&mut harness, Key::Enter);
    harness.get_by_label_contains("Keyboard shortcuts");
    harness.get_by_label_contains("Tab / Shift+Tab");
}

#[test]
fn keyboard_editing_history_and_text_field_shortcut_isolation() {
    let mut harness = app();
    press_command(&mut harness, Key::I);
    assert_composed(&harness, Bgra::BLACK);
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());

    press_command(&mut harness, Key::Z);
    assert_composed(&harness, Bgra::WHITE);
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());
    assert!(harness.state().session().can_redo());

    press_command_shift(&mut harness, Key::Z);
    assert_composed(&harness, Bgra::BLACK);
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());
    assert!(!harness.state().session().can_redo());

    press_command(&mut harness, Key::Z);
    focus(&mut harness, Role::SpinButton, "Selection left");
    press(&mut harness, Key::ArrowDown);
    press(&mut harness, Key::ArrowDown);
    assert_spin_value(&harness, "Selection left", -2.0);
    focus(&mut harness, Role::SpinButton, "Selection top");
    press(&mut harness, Key::ArrowDown);
    assert_spin_value(&harness, "Selection top", -1.0);
    focus(&mut harness, Role::SpinButton, "Selection right");
    press(&mut harness, Key::ArrowUp);
    press(&mut harness, Key::ArrowUp);
    assert_spin_value(&harness, "Selection right", 3.0);
    focus(&mut harness, Role::SpinButton, "Selection bottom");
    press(&mut harness, Key::ArrowUp);
    assert_spin_value(&harness, "Selection bottom", 2.0);
    press(&mut harness, Key::Enter);
    assert!(
        !harness
            .get_by_role_and_label(Role::SpinButton, "Selection bottom")
            .is_focused()
    );

    press_command(&mut harness, Key::R);
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(-2, -1, 3, 2).expect("signed selection"))
    );
    press_command(&mut harness, Key::F);
    assert_clipped_fill(&harness);
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());
    assert!(!harness.state().session().can_redo());
    let before = harness.state().session().snapshot().expect("snapshot");

    press_command(&mut harness, Key::Z);
    assert_composed(&harness, Bgra::WHITE);
    assert!(!harness.state().session().dirty());
    assert!(harness.state().session().can_redo());
    press_command_shift(&mut harness, Key::Z);
    assert_clipped_fill(&harness);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );

    press_command_shift(&mut harness, Key::E);
    type_path(&mut harness, Path::new("draft.png"));
    for key in [Key::I, Key::F, Key::R, Key::N, Key::O, Key::Z] {
        press_command(&mut harness, key);
    }
    press_command_shift(&mut harness, Key::Z);
    press_command_shift(&mut harness, Key::L);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness.get_by_role_and_label(Role::Button, "Export PNG");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none(),
        "typing in a path field must not trigger document shortcuts"
    );

    press(&mut harness, Key::Escape);
    assert!(harness.query_by_label("PNG file path").is_none());
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    press_command(&mut harness, Key::Z);
    assert_composed(&harness, Bgra::WHITE);
}

#[test]
fn keyboard_layer_visibility_opacity_and_history() {
    let mut harness = app();
    press_command_shift(&mut harness, Key::L);
    assert_eq!(harness.state().session().document().layers().len(), 2);
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        1
    );
    press_command(&mut harness, Key::F);
    assert_composed(&harness, Bgra::BLACK);

    focus(&mut harness, Role::CheckBox, "Layer 1 visible");
    press(&mut harness, Key::Space);
    assert!(!harness.state().session().document().layers()[1].is_visible());
    assert_composed(&harness, Bgra::WHITE);
    press(&mut harness, Key::Space);
    assert!(harness.state().session().document().layers()[1].is_visible());
    assert_composed(&harness, Bgra::BLACK);

    harness
        .get_by_role_and_label(Role::Button, "Layer Properties")
        .click();
    harness.run();
    focus(&mut harness, Role::Slider, "Layer opacity");
    for _ in 0..64 {
        harness.key_press(Key::ArrowLeft);
    }
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Apply").click();
    harness.run();
    assert_eq!(
        harness.state().session().document().layers()[1].opacity(),
        191
    );
    assert_composed(&harness, Bgra::new(64, 64, 64, 255));
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());
    let before = harness.state().session().snapshot().expect("snapshot");

    press_command(&mut harness, Key::Z);
    assert_eq!(
        harness.state().session().document().layers()[1].opacity(),
        255
    );
    assert_composed(&harness, Bgra::BLACK);
    assert!(harness.state().session().can_redo());
    press_command_shift(&mut harness, Key::Z);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert_composed(&harness, Bgra::new(64, 64, 64, 255));
}

#[test]
fn keyboard_open_and_failed_open_recovery_preserve_then_replace_session() {
    let temporary = TempDir::new().expect("temporary directory");
    let invalid = temporary.path().join("invalid.png");
    let valid = temporary.path().join("valid.png");
    fs::write(&invalid, b"not a PNG").expect("write malformed PNG");
    let source = write_input_png(&valid);
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    press_command(&mut harness, Key::I);
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");

    press_command(&mut harness, Key::O);
    type_path(&mut harness, &invalid);
    press(&mut harness, Key::Enter);
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.state().status().contains("Open failed"));
    assert!(harness.get_by_label("PNG file path").is_focused());

    replace_path(&mut harness, &valid);
    press(&mut harness, Key::Enter);
    let session = harness.state().session();
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    assert_eq!(session.document().size(), source.size());
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(session.document().active_layer().surface(), &source);
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
}

#[test]
fn keyboard_export_failure_is_recoverable_without_changing_session() {
    let temporary = TempDir::new().expect("temporary directory");
    let invalid = temporary.path().join("missing/output.png");
    let valid = temporary.path().join("exported.png");
    let mut harness = app();
    press_command_shift(&mut harness, Key::L);
    press_command(&mut harness, Key::F);
    let before = harness.state().session().snapshot().expect("snapshot");
    let expected = harness.state().session().compose().expect("composite");
    assert_eq!(before.layer_count, 2);
    assert!(before.dirty && before.can_undo);

    press_command_shift(&mut harness, Key::E);
    type_path(&mut harness, &invalid);
    press(&mut harness, Key::Enter);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.state().status().contains("Export failed"));
    assert!(!invalid.exists());
    assert!(harness.get_by_label("PNG file path").is_focused());

    replace_path(&mut harness, &valid);
    press(&mut harness, Key::Enter);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.query_by_label("PNG file path").is_none());
    assert!(harness.state().session().dirty());
    assert_eq!(harness.state().session().document().layers().len(), 2);
    let exported = paint_image_io::decode_png(&fs::read(valid).expect("read exported PNG"))
        .expect("decode exported PNG");
    assert_eq!(exported, expected);
}
