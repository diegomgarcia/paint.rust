use eframe::egui::{self, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Point, Rect};
use paint_ui::PaintApp;

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

fn replace_field(harness: &mut Harness<'_, PaintApp>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_label(label).type_text(text);
    harness.run();
}

fn dock_tools(harness: &mut Harness<'_, PaintApp>) {
    harness
        .get_by_role_and_label(Role::Button, "Window")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Float Tools")
        .click();
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
        .expect("in-bounds pixel")
}

#[test]
fn image_menu_lists_paint_net_actions_and_removes_implemented_placeholders() {
    let mut harness = app();
    click(&mut harness, "Image");
    for label in [
        "Crop to Selection",
        "Resize Image…",
        "Canvas Size…",
        "Flip Horizontal",
        "Flip Vertical",
        "Rotate 90° Clockwise",
        "Rotate 90° Counter-Clockwise",
        "Rotate 180°",
        "Color Profile…",
        "Flatten Image",
    ] {
        harness.get_by_label(label);
    }
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
        assert!(
            !button_is_disabled(&harness, label),
            "{label} should be enabled"
        );
    }
}

#[test]
fn crop_to_selection_uses_the_drag_selection_and_undo_restores_it() {
    let mut harness = app();
    dock_tools(&mut harness);
    drag_canvas(
        &mut harness,
        egui::vec2(20.0, 30.0),
        egui::vec2(120.0, 90.0),
    );
    let selection = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("selection");
    assert_eq!(selection, Rect::new(20, 30, 121, 91).expect("selection"));

    click(&mut harness, "Image");
    assert!(!button_is_disabled(&harness, "Crop to Selection"));
    harness.get_by_label("Crop to Selection").click();
    harness.run();

    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        paint_core::Size::new(101, 61).expect("size")
    );
    assert_eq!(session.document().selection(), None);
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Crop to Selection")
    );
    assert!(session.dirty());

    click(&mut harness, "Undo");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(800, 600).expect("size")
    );
    assert!(harness.state().session().document().selection().is_some());
}

#[test]
fn resize_image_dialog_applies_validated_width_and_height_then_undo_restores() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Resize Image…");
    harness.get_by_label("New width");
    harness.get_by_label("New height");
    replace_field(&mut harness, "New width", "4");
    replace_field(&mut harness, "New height", "3");
    assert!(!button_is_disabled(&harness, "Apply"));
    click(&mut harness, "Apply");

    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(4, 3).expect("size")
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Resize Image")
    );
    assert!(harness.state().status().contains("4×3"));
    assert!(harness.query_by_label("New width").is_none());

    click(&mut harness, "Undo");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(800, 600).expect("size")
    );
}

#[test]
fn resize_image_dialog_supports_percentage_mode() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Resize Image…");
    harness.get_by_label("Percentage").click();
    harness.run();
    replace_field(&mut harness, "Percent", "50");
    click(&mut harness, "Apply");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(400, 300).expect("size")
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Resize Image")
    );
}

#[test]
fn resize_image_dialog_validates_input_and_cancel_does_not_mutate() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    let history_before = harness.state().history_entries().to_vec();
    menu_item(&mut harness, "Image", "Resize Image…");

    replace_field(&mut harness, "New width", "0");
    assert!(button_is_disabled(&harness, "Apply"));
    harness.get_by_label_contains("Error:");

    replace_field(&mut harness, "New width", "not a number");
    assert!(button_is_disabled(&harness, "Apply"));

    replace_field(&mut harness, "New width", "20000");
    assert!(button_is_disabled(&harness, "Apply"));

    click(&mut harness, "Cancel");
    assert!(harness.query_by_label("New width").is_none());
    assert_eq!(harness.state().session().document(), &before);
    assert_eq!(harness.state().history_entries(), history_before);
    assert!(harness.state().status().contains("canceled"));
}

#[test]
fn canvas_size_dialog_applies_anchor_and_fill_and_cancel_keeps_the_document() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    menu_item(&mut harness, "Image", "Canvas Size…");
    replace_field(&mut harness, "New width", "802");
    replace_field(&mut harness, "New height", "602");
    harness.get_by_label("Anchor: bottom right").click();
    harness.run();
    click(&mut harness, "Apply");

    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        paint_core::Size::new(802, 602).expect("size")
    );
    assert_eq!(pixel(&harness, 0, 0), Bgra::TRANSPARENT);
    assert_eq!(pixel(&harness, 2, 2), Bgra::WHITE);
    assert_eq!(pixel(&harness, 801, 601), Bgra::WHITE);
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Canvas Size")
    );
    assert!(harness.state().status().contains("802×602"));

    click(&mut harness, "Undo");
    assert_eq!(harness.state().session().document(), &before);

    menu_item(&mut harness, "Image", "Canvas Size…");
    replace_field(&mut harness, "New width", "10");
    click(&mut harness, "Cancel");
    assert!(harness.query_by_label("New width").is_none());
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn canvas_size_dialog_can_shrink_with_the_top_left_anchor() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Canvas Size…");
    replace_field(&mut harness, "New width", "400");
    replace_field(&mut harness, "New height", "300");
    click(&mut harness, "Apply");
    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        paint_core::Size::new(400, 300).expect("size")
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Canvas Size")
    );
}

#[test]
fn image_rotations_and_flips_update_the_whole_document() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Rotate 90° Clockwise");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(600, 800).expect("size")
    );
    menu_item(&mut harness, "Image", "Rotate 90° Counter-Clockwise");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(800, 600).expect("size")
    );
    menu_item(&mut harness, "Image", "Rotate 180°");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(800, 600).expect("size")
    );
    menu_item(&mut harness, "Image", "Flip Horizontal");
    menu_item(&mut harness, "Image", "Flip Vertical");
    assert_eq!(
        harness.state().history_entries(),
        [
            "New Image",
            "Rotate 90° Clockwise",
            "Rotate 90° Counter-Clockwise",
            "Rotate 180°",
            "Flip Image Horizontal",
            "Flip Image Vertical",
        ]
    );
    for _ in 0..5 {
        click(&mut harness, "Undo");
    }
    assert_eq!(harness.state().history_entries(), ["New Image"]);
    assert!(!harness.state().session().dirty());
}

#[test]
fn flatten_image_is_disabled_for_one_layer_and_enabled_after_adding_one() {
    let mut harness = app();
    click(&mut harness, "Image");
    assert!(button_is_disabled(&harness, "Flatten Image"));
    harness.key_press(egui::Key::Escape);
    harness.run();
    click(&mut harness, "Add Layer");
    click(&mut harness, "Image");
    assert!(!button_is_disabled(&harness, "Flatten Image"));
    harness.get_by_label("Flatten Image").click();
    harness.run();
    assert_eq!(harness.state().session().document().layers().len(), 1);
}

#[test]
fn resize_dialog_escape_cancels_without_mutating() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    menu_item(&mut harness, "Image", "Resize Image…");
    harness.get_by_label("New width");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.query_by_label("New width").is_none());
    assert_eq!(harness.state().session().document(), &before);
    assert_eq!(harness.state().history_entries(), ["New Image"]);
}

#[test]
fn resize_dialog_maintains_the_aspect_ratio_when_requested() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Resize Image…");
    harness.get_by_label("Maintain aspect ratio").click();
    harness.run();
    replace_field(&mut harness, "New width", "1600");
    click(&mut harness, "Apply");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(1600, 1200).expect("size")
    );
}

#[test]
fn canvas_size_dialog_supports_percentage_mode() {
    let mut harness = app();
    menu_item(&mut harness, "Image", "Canvas Size…");
    harness.get_by_label("Percentage").click();
    harness.run();
    replace_field(&mut harness, "Percent", "25");
    click(&mut harness, "Apply");
    assert_eq!(
        harness.state().session().document().size(),
        paint_core::Size::new(200, 150).expect("size")
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Canvas Size")
    );
}
