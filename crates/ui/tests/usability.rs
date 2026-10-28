use std::fs;
use std::path::Path;

use eframe::egui::{self, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_ui::PaintApp;
use tempfile::TempDir;

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

fn enter_path(harness: &mut Harness<'_, PaintApp>, path: &Path) {
    let field = harness.get_by_label("PNG file path");
    field.click();
    harness.run();
    let field = harness.get_by_label("PNG file path");
    assert!(field.is_focused());
    field.type_text(path.to_str().expect("UTF-8 test path"));
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

fn request_close(harness: &mut Harness<'_, PaintApp>) -> Vec<egui::ViewportCommand> {
    harness
        .input_mut()
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("root viewport input")
        .events
        .push(egui::ViewportEvent::Close);
    harness.step();
    let commands = viewport_commands(harness);
    harness.run();
    commands
}

fn viewport_commands(harness: &Harness<'_, PaintApp>) -> Vec<egui::ViewportCommand> {
    harness
        .output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("root viewport output")
        .commands
        .clone()
}

#[test]
fn new_invert_undo_and_redo_operate_on_the_real_session() {
    let mut harness = app();
    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        Size::new(800, 600).expect("size")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert!(button_is_disabled(&harness, "Undo"));
    assert!(button_is_disabled(&harness, "Redo"));
    assert!(
        session
            .document()
            .active_layer()
            .surface()
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );

    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let session = harness.state().session();
    assert!(session.dirty());
    assert!(session.can_undo());
    assert!(!session.can_redo());
    assert!(!button_is_disabled(&harness, "Undo"));
    assert!(button_is_disabled(&harness, "Redo"));
    assert!(
        session
            .compose()
            .expect("composed surface")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::BLACK)
    );

    click(&mut harness, "Undo");
    let session = harness.state().session();
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(session.can_redo());
    assert!(button_is_disabled(&harness, "Undo"));
    assert!(!button_is_disabled(&harness, "Redo"));
    assert!(
        session
            .compose()
            .expect("composed surface")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );

    click(&mut harness, "Redo");
    assert!(harness.state().session().dirty());
    assert!(harness.state().session().can_undo());
    assert!(!harness.state().session().can_redo());

    menu_item(&mut harness, "File", "New");
    click(&mut harness, "Discard");
    let session = harness.state().session();
    assert_eq!(
        session.document().size(),
        Size::new(800, 600).expect("size")
    );
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert!(button_is_disabled(&harness, "Undo"));
    assert!(button_is_disabled(&harness, "Redo"));
    assert_eq!(session.document().layers().len(), 1);
    assert!(
        session
            .compose()
            .expect("composed surface")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
}

#[test]
fn dirty_new_cancel_preserves_document_and_history_and_discard_replaces_it() {
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");
    assert!(before.dirty && before.can_undo && !before.can_redo);

    menu_item(&mut harness, "File", "New");
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness.get_by_role_and_label(Role::Button, "Discard");
    click(&mut harness, "Cancel");
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );

    click(&mut harness, "Undo");
    assert!(!harness.state().session().dirty());
    assert!(harness.state().session().can_redo());
    click(&mut harness, "Redo");
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before,
        "redo must restore pixels and semantic history state, not monotonic revision numbers"
    );

    menu_item(&mut harness, "File", "New");
    click(&mut harness, "Discard");
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());
    assert!(!harness.state().session().can_redo());
    assert_eq!(harness.state().session().document().layers().len(), 1);
    assert!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );

    menu_item(&mut harness, "File", "New");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none(),
        "clean New must proceed without a discard confirmation"
    );
}

#[test]
fn rectangle_selection_chosen_bgra_fill_and_clear_selection_are_usable() {
    let mut harness = app();
    click(&mut harness, "Select Rectangle");
    assert_eq!(harness.state().session().document().selection(), None);
    click(&mut harness, "Apply selection");
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(0, 0, 1, 1).expect("selection"))
    );

    click(&mut harness, "More");
    harness.get_by_role_and_label(Role::Slider, "Red").click();
    harness.run();
    let chosen = harness.state().fill_color();
    assert!(
        chosen.r > 0,
        "physical slider click must choose a new color"
    );

    click(&mut harness, "Fill");
    click(&mut harness, "Fill Selection");
    let session = harness.state().session();
    let pixels = session.document().active_layer().surface().pixels();
    assert_eq!(pixels[0], chosen);
    assert!(pixels[1..].iter().all(|pixel| *pixel == Bgra::WHITE));
    assert_eq!(session.compose().expect("composite").pixels()[0], chosen);

    menu_item(&mut harness, "Edit", "Deselect");
    assert_eq!(harness.state().session().document().selection(), None);
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels()[0],
        chosen
    );
}

#[test]
fn layer_panel_exposes_color_and_layer_previews() {
    let mut harness = app();

    harness.get_by_role_and_label(Role::Button, "New document");
    harness.get_by_role_and_label(Role::Button, "Open PNG file");
    harness.get_by_role_and_label(Role::Button, "Export PNG file");
    harness.get_by_role_and_label(Role::Button, "Primary color");
    harness.get_by_label_contains("Layer 0 thumbnail: Background");

    click(&mut harness, "Add Layer");
    harness.get_by_label_contains("Layer 1 thumbnail: Layer 1");
}

#[test]
fn open_dialog_previews_a_valid_png_without_changing_the_session() {
    let root = TempDir::new().expect("temp dir");
    let path = root.path().join("preview.png");
    write_input_png(&path);
    let mut harness = app();
    let before = harness.state().session().document().clone();

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &path);
    click(&mut harness, "Preview");
    harness.run();

    harness.get_by_label_contains("PNG preview: 2×1 PNG");
    assert_eq!(harness.state().session().document(), &before);

    click(&mut harness, "Cancel");
    let invalid = root.path().join("invalid.png");
    fs::write(&invalid, b"not a PNG").expect("invalid preview file");
    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &invalid);
    click(&mut harness, "Preview");
    harness.run();
    harness.get_by_label_contains("Preview failed:");
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn paint_net_style_layout_exposes_tools_options_colors_and_layers() {
    let harness = app();
    harness.get_by_role_and_label(Role::Label, "Tools");
    harness.get_by_role_and_label(Role::Label, "Tool Options");
    harness.get_by_role_and_label(Role::Label, "Colors");
    harness.get_by_role_and_label(Role::Label, "Layers");
    harness.get_by_role_and_label(Role::Button, "Select Rectangle");
    harness.get_by_role_and_label(Role::Button, "Fill");
    harness.get_by_role_and_label(Role::Button, "Pick color from canvas");
}

#[test]
fn canvas_color_picker_samples_composed_pixels_without_editing() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    click(&mut harness, "Pick color from canvas");
    harness
        .get_by_role_and_label(Role::Image, "Composed document canvas")
        .click();
    harness.run();

    assert_eq!(harness.state().fill_color(), Bgra::WHITE);
    harness.get_by_role_and_label(Role::Button, "Pick color from canvas");
    assert_eq!(harness.state().session().document(), &before);
    assert!(!harness.state().session().dirty());
}

#[test]
fn canvas_drag_creates_a_half_open_rectangle_selection_without_history() {
    let mut harness = app();
    dock_tools(&mut harness);
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + egui::vec2(20.0, 30.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(120.0, 90.0));
    harness.run();
    harness.drop_at(rect.min + egui::vec2(120.0, 90.0));
    harness.run();

    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(20, 30, 121, 91).expect("drag selection"))
    );
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());
}

#[test]
fn rectangle_shape_tool_drag_fills_and_remains_undoable() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle shape");
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + egui::vec2(5.0, 6.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(14.0, 15.0));
    harness.run();
    harness.drop_at(rect.min + egui::vec2(14.0, 15.0));
    harness.run();

    let session = harness.state().session();
    assert_eq!(
        session.document().selection(),
        Some(Rect::new(5, 6, 15, 16).expect("shape selection"))
    );
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(5, 6)).expect("inside"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(14, 15)).expect("inside"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(0, 0)).expect("outside"),
        Bgra::WHITE
    );
    assert!(session.dirty());
    assert!(session.can_undo());
}

#[test]
fn zoom_tool_click_changes_canvas_zoom_without_editing() {
    let mut harness = app();
    click(&mut harness, "Zoom");
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    canvas.click();
    harness.run();
    assert!(harness.get_all_by_label("125%").count() >= 2);
    assert!(!harness.state().session().dirty());
}

#[test]
fn layer_menu_exposes_both_active_layer_flips() {
    let mut harness = app();
    harness
        .get_by_role_and_label(Role::Button, "Layers")
        .click();
    harness.run();
    harness.get_by_label("Flip Active Layer Horizontally");
    harness.get_by_label("Flip Active Layer Vertically");
}

#[test]
fn colors_window_supports_primary_secondary_swap_and_reset() {
    let mut harness = app();
    assert_eq!(harness.state().fill_color(), Bgra::BLACK);
    assert_eq!(harness.state().secondary_color(), Bgra::WHITE);

    click(&mut harness, "Secondary color");
    replace_field(&mut harness, "Hex color", "#112233");
    click(&mut harness, "Apply color");
    assert_eq!(harness.state().fill_color(), Bgra::BLACK);
    assert_eq!(
        harness.state().secondary_color(),
        Bgra::new(0x33, 0x22, 0x11, 255)
    );

    click(&mut harness, "Swap colors");
    assert_eq!(
        harness.state().fill_color(),
        Bgra::new(0x33, 0x22, 0x11, 255)
    );
    assert_eq!(harness.state().secondary_color(), Bgra::BLACK);

    click(&mut harness, "Reset to black and white");
    assert_eq!(harness.state().fill_color(), Bgra::BLACK);
    assert_eq!(harness.state().secondary_color(), Bgra::WHITE);
}

#[test]
fn history_window_tracks_content_edits_and_undo() {
    let mut harness = app();
    assert_eq!(harness.state().history_entries(), ["New Image"]);
    click(&mut harness, "Fill");
    click(&mut harness, "Fill Selection");
    assert_eq!(
        harness.state().history_entries(),
        ["New Image", "Paint Bucket"]
    );
    click(&mut harness, "Undo History");
    assert_eq!(harness.state().history_entries(), ["New Image"]);
    assert!(harness.state().session().can_redo());
}

#[test]
fn window_menu_can_float_and_dock_the_color_and_layer_panels() {
    let mut harness = app();
    assert!(!harness.state().panels_floating());
    menu_item(&mut harness, "Window", "Float Panels");
    assert!(harness.state().panels_floating());
    harness.get_by_role_and_label(Role::Label, "Tools");
    harness.get_by_role_and_label(Role::Label, "Colors");
    harness.get_by_role_and_label(Role::Label, "Layers");
    menu_item(&mut harness, "Window", "Dock Panels");
    assert!(!harness.state().panels_floating());
    harness.get_by_role_and_label(Role::Button, "New Layer");
}

#[test]
fn hex_color_input_applies_exact_rgb_and_alpha() {
    assert_eq!(
        paint_ui::color_from_hex("#FF0000").expect("red"),
        Bgra::new(0, 0, 255, 255)
    );
    assert_eq!(
        paint_ui::color_from_hex("00FF0080").expect("green alpha"),
        Bgra::new(0, 255, 0, 128)
    );
    assert!(paint_ui::color_from_hex("#12345").is_err());
    assert!(paint_ui::color_from_hex("#GG0000").is_err());

    let mut harness = app();
    let before = harness.state().session().document().clone();
    replace_field(&mut harness, "Hex color", "#FF8000");
    click(&mut harness, "Apply color");

    assert_eq!(harness.state().fill_color(), Bgra::new(0, 128, 255, 255));
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn color_wheel_exposes_accessible_hsv_color_selection() {
    assert_eq!(
        paint_ui::color_from_hsv(0.0, 1.0, 1.0, 255),
        Bgra::new(0, 0, 255, 255)
    );
    assert_eq!(
        paint_ui::color_from_hsv(120.0, 1.0, 1.0, 255),
        Bgra::new(0, 255, 0, 255)
    );
    assert_eq!(
        paint_ui::color_from_hsv(240.0, 1.0, 1.0, 128),
        Bgra::new(255, 0, 0, 128)
    );
    assert_eq!(paint_ui::color_from_hsv(0.0, 0.0, 1.0, 255), Bgra::WHITE);

    let harness = app();
    harness.get_by_role_and_label(Role::Button, "Color wheel");
}

#[test]
fn zoom_controls_change_canvas_preview_without_mutating_the_document() {
    let mut harness = app();
    let before = harness.state().session().document().clone();

    click(&mut harness, "Zoom in");
    click(&mut harness, "Zoom in");
    harness.get_by_label_contains("150%");
    click(&mut harness, "Zoom out");
    harness.get_by_label_contains("125%");
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn palette_updates_primary_color_without_mutating_the_document() {
    let mut harness = app();
    let before = harness.state().session().document().clone();

    click(&mut harness, "Preset red");
    assert_eq!(harness.state().fill_color(), Bgra::new(0, 0, 255, 255));

    click(&mut harness, "Preset transparent");
    assert_eq!(harness.state().fill_color(), Bgra::TRANSPARENT);
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn png_browser_navigates_and_previews_without_mutating_the_session() {
    let root = TempDir::new().expect("temp dir");
    let nested = root.path().join("nested");
    fs::create_dir(&nested).expect("nested directory");
    let image = nested.join("image.png");
    write_input_png(&image);
    let mut harness = app();
    let before = harness.state().session().document().clone();

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, root.path());
    click(&mut harness, "Browse folder");
    click(&mut harness, "Folder: nested");
    click(&mut harness, "PNG: image.png");
    harness.run();

    assert_eq!(
        harness.get_by_label("PNG file path").value().as_deref(),
        image.to_str()
    );
    harness.get_by_label_contains("PNG preview: 2×1 PNG");
    assert_eq!(harness.state().session().document(), &before);
}

#[test]
fn layer_add_opacity_active_selection_and_visibility_change_composition() {
    let mut harness = app();
    click(&mut harness, "Add Layer");
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

    click(&mut harness, "Fill");
    click(&mut harness, "Fill Selection");
    assert!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::BLACK)
    );

    harness
        .get_by_role_and_label(Role::Slider, "Layer 1 opacity")
        .click();
    harness.run();
    let opacity = harness.state().session().document().layers()[1].opacity();
    assert!(
        opacity < u8::MAX,
        "physical slider click must change opacity"
    );
    assert_ne!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()[0],
        Bgra::BLACK
    );

    click(&mut harness, "Active layer 0: Background");
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        0
    );

    harness
        .get_by_role_and_label(Role::CheckBox, "Layer 1 visible")
        .click();
    harness.run();
    assert!(!harness.state().session().document().layers()[1].is_visible());
    assert!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
}

#[test]
fn dirty_open_cancel_preserves_session_and_only_discard_replaces_it() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("replacement.png");
    let source = write_input_png(&input);
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &input);
    click(&mut harness, "Open PNG");
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness.get_by_role_and_label(Role::Button, "Discard");

    click(&mut harness, "Cancel");
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness.get_by_label("PNG file path");

    click(&mut harness, "Open PNG");
    harness.get_by_role_and_label(Role::Button, "Discard");
    assert_eq!(harness.state().session().document(), &before_document);
    click(&mut harness, "Discard");

    let session = harness.state().session();
    assert_eq!(session.document().size(), source.size());
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(session.document().active_layer().surface(), &source);
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
}

#[test]
fn png_open_is_flattened_and_export_preserves_the_layered_session() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("input.png");
    let output = temporary.path().join("exported.png");
    let source = write_input_png(&input);
    let mut harness = app();

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &input);
    click(&mut harness, "Open PNG");

    let session = harness.state().session();
    assert_eq!(session.document().size(), source.size());
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(session.document().active_layer().surface(), &source);
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());

    click(&mut harness, "Add Layer");
    click(&mut harness, "Fill");
    click(&mut harness, "Fill Selection");
    let before = harness.state().session().snapshot().expect("snapshot");
    assert_eq!(before.layer_count, 2);
    assert!(before.dirty);
    assert!(before.can_undo);

    menu_item(&mut harness, "File", "Export PNG…");
    enter_path(&mut harness, &output);
    click(&mut harness, "Export PNG");

    let session = harness.state().session();
    assert_eq!(session.snapshot().expect("snapshot"), before);
    assert_eq!(session.document().layers().len(), 2);
    assert!(session.dirty());
    assert!(session.can_undo());
    let expected = session.compose().expect("composite");
    let exported = paint_image_io::decode_png(&fs::read(output).expect("read exported PNG"))
        .expect("decode exported PNG");
    assert_eq!(exported, expected);
}

#[test]
fn failed_export_reports_an_error_and_preserves_modified_state() {
    let temporary = TempDir::new().expect("temporary directory");
    let output = temporary.path().join("missing/output.png");
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before = harness.state().session().snapshot().expect("snapshot");
    assert!(before.dirty);

    menu_item(&mut harness, "File", "Export PNG…");
    enter_path(&mut harness, &output);
    click(&mut harness, "Export PNG");

    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.state().session().dirty());
    assert!(harness.state().status().contains("Export failed"));
    assert!(!output.exists());
}

#[test]
fn failed_open_preserves_the_current_session() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("invalid.png");
    fs::write(&input, b"not a PNG").expect("write malformed PNG");
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before = harness.state().session().snapshot().expect("snapshot");

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &input);
    click(&mut harness, "Open PNG");
    click(&mut harness, "Discard");

    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.state().status().contains("Open failed"));
}

#[test]
fn post_export_close_is_canceled_until_explicit_discard() {
    let temporary = TempDir::new().expect("temporary directory");
    let output = temporary.path().join("exported.png");
    let mut harness = app();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    menu_item(&mut harness, "File", "Export PNG…");
    enter_path(&mut harness, &output);
    click(&mut harness, "Export PNG");
    assert!(output.exists());
    assert!(harness.state().session().dirty());
    let before = harness.state().session().snapshot().expect("snapshot");

    let commands = request_close(&mut harness);
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(!commands.contains(&egui::ViewportCommand::Close));
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    harness.get_by_role_and_label(Role::Button, "Discard");

    click(&mut harness, "Cancel");
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none()
    );
    assert!(!viewport_commands(&harness).contains(&egui::ViewportCommand::Close));

    let commands = request_close(&mut harness);
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    harness
        .get_by_role_and_label(Role::Button, "Discard")
        .click();
    harness.step();
    let commands = viewport_commands(&harness);
    assert!(commands.contains(&egui::ViewportCommand::Close));
    assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    assert!(harness.state().session().dirty());

    let commands = request_close(&mut harness);
    assert!(
        !commands.contains(&egui::ViewportCommand::CancelClose),
        "egui-winit's forwarded close event must be allowed after explicit Discard"
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Discard")
            .is_none()
    );
}
