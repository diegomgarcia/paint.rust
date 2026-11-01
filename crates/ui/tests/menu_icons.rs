use eframe::egui::{self, Key, Role};
use egui_kittest::{
    Harness, Node,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Rect, Size};
use paint_ui::PaintApp;

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
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
    menu_node(harness, item).click();
    harness.run();
}

fn close_menu(harness: &mut Harness<'_, PaintApp>) {
    harness.key_press(Key::Escape);
    harness.run();
}

fn command() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    }
}

fn shortcut(chord: &str) -> String {
    format!("{}+{chord}", command())
}

/// Menu rows are the full-width button with a given label; panels and the toolbar reuse
/// some Paint.NET action names at compact icon sizes.
fn menu_node<'a>(harness: &'a Harness<'_, PaintApp>, label: &'a str) -> Node<'a> {
    harness
        .get_all_by_role_and_label(Role::Button, label)
        .max_by(|a, b| a.rect().width().total_cmp(&b.rect().width()))
        .unwrap_or_else(|| panic!("no menu row labeled {label}"))
}

fn description(harness: &Harness<'_, PaintApp>, label: &str) -> String {
    menu_node(harness, label)
        .accesskit_node()
        .description()
        .unwrap_or_default()
}

fn button_is_disabled(harness: &Harness<'_, PaintApp>, label: &str) -> bool {
    menu_node(harness, label).accesskit_node().is_disabled()
}

#[test]
fn file_and_edit_rows_show_shortcuts_and_keep_stable_labels() {
    let mut harness = app();

    click(&mut harness, "File");
    for label in [
        "New",
        "Open PNG…",
        "Save",
        "Save As…",
        "Export PNG…",
        "Close Window",
        "Quit",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }
    assert!(button_is_disabled(&harness, "Save"));
    assert_eq!(description(&harness, "New"), shortcut("N"));
    assert_eq!(description(&harness, "Open PNG…"), shortcut("O"));
    assert_eq!(description(&harness, "Save"), shortcut("S"));
    assert_eq!(description(&harness, "Save As…"), shortcut("Shift+S"));
    assert_eq!(description(&harness, "Export PNG…"), shortcut("Shift+E"));
    assert_eq!(description(&harness, "Close Window"), shortcut("W"));
    assert_eq!(description(&harness, "Quit"), shortcut("Q"));
    close_menu(&mut harness);

    click(&mut harness, "Edit");
    for label in [
        "Undo",
        "Redo",
        "Cut",
        "Copy",
        "Paste",
        "Paste Into New Layer",
        "Paste Into New Image",
        "Erase Selection",
        "Fill Selection",
        "Invert Selection",
        "Select All",
        "Deselect",
    ] {
        menu_node(&harness, label);
    }
    assert!(button_is_disabled(&harness, "Undo"));
    assert!(button_is_disabled(&harness, "Redo"));
    assert!(button_is_disabled(&harness, "Cut"));
    assert!(button_is_disabled(&harness, "Paste"));
    assert!(button_is_disabled(&harness, "Erase Selection"));
    assert!(button_is_disabled(&harness, "Fill Selection"));
    assert!(button_is_disabled(&harness, "Invert Selection"));
    assert!(button_is_disabled(&harness, "Deselect"));
    assert!(!button_is_disabled(&harness, "Copy"));
    assert!(!button_is_disabled(&harness, "Select All"));
    assert_eq!(description(&harness, "Undo"), shortcut("Z"));
    assert_eq!(description(&harness, "Redo"), shortcut("Shift+Z"));
    assert_eq!(description(&harness, "Cut"), shortcut("X"));
    assert_eq!(description(&harness, "Copy"), shortcut("C"));
    assert_eq!(description(&harness, "Paste"), shortcut("V"));
    assert_eq!(
        description(&harness, "Paste Into New Layer"),
        shortcut("Shift+V")
    );
    assert_eq!(
        description(&harness, "Paste Into New Image"),
        shortcut("Alt+V")
    );
    assert_eq!(description(&harness, "Erase Selection"), "Delete");
    assert_eq!(description(&harness, "Fill Selection"), "Backspace");
    assert_eq!(description(&harness, "Invert Selection"), shortcut("I"));
    assert_eq!(description(&harness, "Select All"), shortcut("A"));
    assert_eq!(description(&harness, "Deselect"), shortcut("D"));
}

#[test]
fn icon_menu_rows_invoke_their_actions() {
    let mut harness = app();

    menu_item(&mut harness, "Edit", "Select All");
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(0, 0, 800, 600).expect("full canvas"))
    );
    menu_item(&mut harness, "Edit", "Deselect");
    assert!(harness.state().session().document().selection().is_none());

    menu_item(&mut harness, "Layers", "Add New Layer");
    assert_eq!(harness.state().session().document().layers().len(), 2);

    menu_item(&mut harness, "Image", "Rotate 90° Clockwise");
    assert_eq!(
        harness.state().session().document().size(),
        Size::new(600, 800).expect("rotated canvas")
    );

    menu_item(&mut harness, "Edit", "Undo");
    assert_eq!(
        harness.state().session().document().size(),
        Size::new(800, 600).expect("restored canvas")
    );
}

#[test]
fn image_and_layer_rows_match_paintnet_enablement() {
    let mut harness = app();

    click(&mut harness, "Image");
    assert!(button_is_disabled(&harness, "Crop to Selection"));
    assert!(button_is_disabled(&harness, "Color Profile…"));
    assert!(button_is_disabled(&harness, "Flatten Image"));
    for label in [
        "Resize Image…",
        "Canvas Size…",
        "Flip Horizontal",
        "Flip Vertical",
        "Rotate 90° Clockwise",
        "Rotate 90° Counter-Clockwise",
        "Rotate 180°",
    ] {
        assert!(!button_is_disabled(&harness, label), "{label} is enabled");
    }
    close_menu(&mut harness);

    click(&mut harness, "Layers");
    for label in [
        "Delete Layer",
        "Merge Layer Down",
        "Move Layer Up",
        "Move Layer Down",
        "Move Layer to Top",
        "Move Layer to Bottom",
        "Go to Top Layer",
        "Go to Layer Above",
        "Go to Layer Below",
        "Go to Bottom Layer",
    ] {
        assert!(
            button_is_disabled(&harness, label),
            "{label} is disabled for a single or top layer"
        );
    }
    assert!(!button_is_disabled(&harness, "Add New Layer"));
    assert!(!button_is_disabled(&harness, "Duplicate Layer"));
    assert!(!button_is_disabled(&harness, "Toggle Layer Visibility"));
    assert!(!button_is_disabled(&harness, "Layer Properties…"));
    assert_eq!(description(&harness, "Add New Layer"), shortcut("Shift+L"));
    menu_node(&harness, "Add New Layer").click();
    harness.run();
    assert_eq!(harness.state().session().document().layers().len(), 2);

    click(&mut harness, "Layers");
    assert!(!button_is_disabled(&harness, "Delete Layer"));
    assert!(!button_is_disabled(&harness, "Merge Layer Down"));
    assert!(!button_is_disabled(&harness, "Move Layer Down"));
    assert!(!button_is_disabled(&harness, "Move Layer to Bottom"));
    assert!(!button_is_disabled(&harness, "Go to Layer Below"));
    assert!(!button_is_disabled(&harness, "Go to Bottom Layer"));
    assert!(button_is_disabled(&harness, "Move Layer Up"));
    assert!(button_is_disabled(&harness, "Move Layer to Top"));
    assert!(button_is_disabled(&harness, "Go to Top Layer"));
    assert!(button_is_disabled(&harness, "Go to Layer Above"));

    menu_node(&harness, "Move Layer Down").click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        0
    );
}

#[test]
fn grid_and_rulers_rows_keep_the_checkbox_role_and_toggle() {
    let mut harness = app();

    click(&mut harness, "View");
    assert!(!harness.state().grid_enabled());
    harness
        .get_by_role_and_label(Role::CheckBox, "Grid")
        .click();
    harness.run();
    assert!(harness.state().grid_enabled());
    close_menu(&mut harness);

    click(&mut harness, "View");
    harness
        .get_by_role_and_label(Role::CheckBox, "Grid")
        .click();
    harness.run();
    assert!(!harness.state().grid_enabled());
    close_menu(&mut harness);

    click(&mut harness, "View");
    harness
        .get_by_role_and_label(Role::CheckBox, "Rulers")
        .click();
    harness.run();
    assert!(harness.state().rulers_enabled());
    close_menu(&mut harness);

    click(&mut harness, "View");
    harness
        .get_by_role_and_label(Role::CheckBox, "Rulers")
        .click();
    harness.run();
    assert!(!harness.state().rulers_enabled());
}
