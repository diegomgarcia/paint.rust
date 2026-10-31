use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Point, Rect, Size};
use paint_ui::PaintApp;

fn command_shift() -> Modifiers {
    Modifiers::COMMAND.plus(Modifiers::SHIFT)
}

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new_docked(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness
}

fn click(harness: &mut Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run();
}

fn menu_item(harness: &mut Harness<'_, PaintApp>, menu: &str, item: &str) {
    click(harness, menu);
    click(harness, item);
}

fn button_is_disabled(harness: &Harness<'_, PaintApp>, label: &str) -> bool {
    harness
        .get_by_role_and_label(Role::Button, label)
        .accesskit_node()
        .is_disabled()
}

fn close_menu(harness: &mut Harness<'_, PaintApp>) {
    harness.key_press(Key::Escape);
    harness.run();
}

fn replace_field(harness: &mut Harness<'_, PaintApp>, label: &str, text: &str) {
    let field = harness.get_by_label(label);
    field.click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label(label).type_text(text);
    harness.run();
}

fn drag_canvas(harness: &mut Harness<'_, PaintApp>, start: egui::Vec2, end: egui::Vec2) {
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + start);
    harness.run();
    harness.hover_at(rect.min + end);
    harness.run();
    harness.drop_at(rect.min + end);
    harness.run();
}

fn pixel(harness: &Harness<'_, PaintApp>, x: u32, y: u32) -> Bgra {
    *harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("in-bounds canvas pixel")
}

fn select_rect(harness: &mut Harness<'_, PaintApp>) {
    click(harness, "Rectangle Select");
    drag_canvas(harness, egui::vec2(10.0, 10.0), egui::vec2(30.0, 30.0));
}

#[test]
fn new_image_dialog_defaults_escapes_and_validates_size() {
    let mut harness = app();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    assert_eq!(
        harness.get_by_label("New image width").value().as_deref(),
        Some("800")
    );
    assert_eq!(
        harness.get_by_label("New image height").value().as_deref(),
        Some("600")
    );
    assert_eq!(
        harness.state().document_tab_count(),
        1,
        "the dialog blocks creation"
    );

    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("New image width").is_none());
    assert_eq!(harness.state().document_tab_count(), 1);
    assert!(harness.state().status().contains("canceled"));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    replace_field(&mut harness, "New image width", "0");
    assert!(button_is_disabled(&harness, "Apply"));
    assert!(
        harness
            .query_by_label_contains("at least 1 pixel")
            .is_some(),
        "zero dimensions explain the validation error"
    );

    replace_field(&mut harness, "New image width", "20000");
    assert!(button_is_disabled(&harness, "Apply"));
    assert!(
        harness
            .query_by_label_contains("exceeds the maximum")
            .is_some(),
        "dimensions above the session limit are rejected"
    );

    replace_field(&mut harness, "New image width", "3");
    replace_field(&mut harness, "New image height", "2");
    assert!(!button_is_disabled(&harness, "Apply"));
    click(&mut harness, "Apply");

    let session = harness.state().session();
    assert_eq!(session.document().size(), Size::new(3, 2).expect("size"));
    assert_eq!(session.document().layers().len(), 1);
    assert!(
        session
            .document()
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    assert_eq!(
        harness.state().document_titles(),
        ["New Image", "New Image 2"]
    );
}

#[test]
fn new_image_dialog_current_default_stays_one_apply_away() {
    let mut harness = app();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    click(&mut harness, "Apply");
    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        Size::new(800, 600).expect("default size")
    );
    assert!(
        session
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
    assert!(!session.dirty());
    assert_eq!(harness.state().document_tab_count(), 2);
}

#[test]
fn new_image_backgrounds_use_reference_fill_choices() {
    let mut harness = app();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    replace_field(&mut harness, "New image width", "2");
    replace_field(&mut harness, "New image height", "1");
    harness
        .get_by_role_and_label(Role::RadioButton, "Background: transparent")
        .click();
    harness.run();
    click(&mut harness, "Apply");
    assert!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::TRANSPARENT),
        "transparent New Image pixels stay zeroed"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    replace_field(&mut harness, "New image width", "2");
    replace_field(&mut harness, "New image height", "1");
    harness
        .get_by_role_and_label(Role::RadioButton, "Background: black")
        .click();
    harness.run();
    click(&mut harness, "Apply");
    assert!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::BLACK),
        "black New Image pixels use the chosen background"
    );
}

#[test]
fn edit_menu_enablement_and_selection_shortcuts_match_the_reference() {
    let mut harness = app();
    click(&mut harness, "Edit");
    assert!(!button_is_disabled(&harness, "Select All"));
    assert!(button_is_disabled(&harness, "Invert Selection"));
    assert!(button_is_disabled(&harness, "Erase Selection"));
    assert!(button_is_disabled(&harness, "Fill Selection"));
    assert!(button_is_disabled(&harness, "Deselect"));
    assert!(button_is_disabled(&harness, "Paste Into New Layer"));
    assert!(button_is_disabled(&harness, "Paste Into New Image"));
    close_menu(&mut harness);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(0, 0, 800, 600).expect("full canvas"))
    );
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());

    select_rect(&mut harness);
    let inner = Rect::new(10, 10, 31, 31).expect("inner selection");
    assert_eq!(
        harness.state().session().document().selection(),
        Some(inner)
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::I);
    harness.run();
    let selection = harness
        .state()
        .session()
        .document()
        .selection_model()
        .expect("inverted selection")
        .clone();
    assert!(selection.contains_xy(0, 0));
    assert!(!selection.contains_xy(20, 20));
    assert!(
        !harness.state().session().dirty(),
        "Invert Selection is not a pixel edit"
    );
    assert!(!harness.state().session().can_undo());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::I);
    harness.run();
    let selection = harness
        .state()
        .session()
        .document()
        .selection_model()
        .expect("double inverted selection");
    assert!(!selection.contains_xy(0, 0));
    assert!(selection.contains_xy(20, 20));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::D);
    harness.run();
    assert!(harness.state().session().document().selection().is_none());

    click(&mut harness, "Edit");
    assert!(button_is_disabled(&harness, "Invert Selection"));
    close_menu(&mut harness);

    // Paint.NET's Ctrl+Shift+I remains Invert Colors; Ctrl+I must not recolor pixels.
    harness.key_press_modifiers(command_shift(), Key::I);
    harness.run();
    assert!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::BLACK),
        "Ctrl+Shift+I inverts colors as in Paint.NET"
    );
}

#[test]
fn erase_and_fill_selection_shortcuts_edit_pixels_and_undo() {
    let mut harness = app();
    select_rect(&mut harness);
    let inner = Rect::new(10, 10, 31, 31).expect("inner selection");

    harness.key_press(Key::Backspace);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);
    assert_eq!(pixel(&harness, 5, 5), Bgra::WHITE);
    assert_eq!(
        harness.state().session().document().selection(),
        Some(inner)
    );
    assert!(harness.state().session().can_undo());

    harness.key_press_modifiers(Modifiers::SHIFT, Key::Backspace);
    harness.run();
    assert_eq!(
        pixel(&harness, 20, 20),
        Bgra::WHITE,
        "Shift uses the secondary color"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);

    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::TRANSPARENT);
    assert_eq!(pixel(&harness, 5, 5), Bgra::WHITE);
    assert!(
        harness.state().session().document().selection().is_none(),
        "Delete erases and deselects"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);
    assert_eq!(
        harness.state().session().document().selection(),
        Some(inner),
        "undo restores the erased selection"
    );

    menu_item(&mut harness, "Edit", "Erase Selection");
    assert_eq!(pixel(&harness, 20, 20), Bgra::TRANSPARENT);
}

#[test]
fn paste_into_new_layer_and_new_image_use_the_app_clipboard() {
    let mut harness = app();
    select_rect(&mut harness);
    harness.key_press(Key::Backspace);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    let clipboard = Rect::new(10, 10, 31, 31).expect("clipboard bounds");
    assert_eq!(harness.state().clipboard_bounds(), Some(clipboard));

    click(&mut harness, "Edit");
    assert!(!button_is_disabled(&harness, "Paste Into New Layer"));
    assert!(!button_is_disabled(&harness, "Paste Into New Image"));
    close_menu(&mut harness);

    menu_item(&mut harness, "Edit", "Paste Into New Layer");
    let session = harness.state().session();
    assert_eq!(session.document().layers().len(), 2);
    assert_eq!(session.document().layers()[1].name(), "Layer 1");
    assert_eq!(
        harness.state().floating_selection_bounds(),
        Some(clipboard),
        "the paste keeps the clipboard bounds on the new layer"
    );
    assert_eq!(session.document().selection(), Some(clipboard));
    assert_eq!(
        harness.state().history_entries(),
        ["New Image", "Fill Selection", "Add Layer: Layer 1"]
    );

    click(&mut harness, "Finish");
    assert_eq!(harness.state().floating_selection_bounds(), None);
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        harness.state().session().document().layers().len(),
        2,
        "undo removes the placed paste but keeps the new layer"
    );
    assert_eq!(pixel(&harness, 20, 20), Bgra::TRANSPARENT);

    menu_item(&mut harness, "Edit", "Paste Into New Image");
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        Size::new(21, 21).expect("clipboard image size")
    );
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(session.document().active_layer().surface().width(), 21);
    assert!(
        session.document().selection().is_none(),
        "Paste Into New Image clears the selection like Paint.NET"
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert_eq!(harness.state().history_entries(), ["Paste Into New Image"]);
}

#[test]
fn text_editing_keeps_select_all_and_backspace_isolated() {
    let mut harness = app();
    click(&mut harness, "More");
    harness.get_by_label("Hex color").click();
    harness.run();
    assert!(harness.get_by_label("Hex color").is_focused());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    assert!(
        harness.state().session().document().selection().is_none(),
        "Cmd+A in a text field selects text, not the canvas"
    );

    let before = harness.state().session().document().clone();
    harness.key_press(Key::Backspace);
    harness.run();
    assert_eq!(
        harness.state().session().document(),
        &before,
        "Backspace in a text field must not fill the selection"
    );

    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(
        harness.state().session().document(),
        &before,
        "Delete in a text field must not erase canvas pixels"
    );
}
