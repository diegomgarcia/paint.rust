use std::fs;
use std::path::Path;

use eframe::egui::{self, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_raster::{GradientRepeat, GradientType, ShapeStyle};
use paint_ui::PaintApp;
use tempfile::TempDir;

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new_docked(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness
}

/// A docked app whose simulated frames are close enough together for double-click detection.
fn fast_app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::builder()
        .with_step_dt(0.02)
        .build_eframe(|cc| PaintApp::new_docked(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness
}

fn layer_names(harness: &Harness<'_, PaintApp>) -> Vec<String> {
    harness
        .state()
        .session()
        .document()
        .layers()
        .iter()
        .map(|layer| layer.name().to_owned())
        .collect()
}

fn double_click(harness: &mut Harness<'_, PaintApp>, label: &str) {
    // Let egui's earlier click history expire so the second click is a double, not a triple.
    harness.run_steps(20);
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.step();
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.step();
    harness.run();
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
fn dirty_new_opens_a_new_tab_and_preserves_the_old_document() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");
    assert!(before.dirty && before.can_undo && !before.can_redo);

    menu_item(&mut harness, "File", "New");
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    let session = harness.state().session();
    assert!(!session.dirty());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.document().layers().len(), 1);
    assert!(
        session
            .compose()
            .expect("composite")
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );

    click(&mut harness, "Document tab: New Image");
    assert_eq!(harness.state().active_document_index(), 0);
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    click(&mut harness, "Document tab: New Image 2");
    assert_eq!(harness.state().active_document_index(), 1);
    assert!(!harness.state().session().dirty());
}

#[test]
fn rectangle_selection_chosen_bgra_fill_and_clear_selection_are_usable() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
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

    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(0.5, 0.5));
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
fn selection_tools_expose_combine_modes_and_magic_wand_sampling() {
    let mut harness = app();
    for tool in [
        "Rectangle Select",
        "Lasso Select",
        "Ellipse Select",
        "Magic Wand",
    ] {
        click(&mut harness, tool);
        for mode in ["Replace", "Union", "Subtract", "Intersect", "Xor"] {
            harness.get_by_role_and_label(Role::Button, mode);
        }
    }
    harness.get_by_role_and_label(Role::Button, "Active Layer");
    assert!(
        harness
            .get_all_by_role_and_label(Role::Button, "Image")
            .count()
            >= 1,
        "the magic wand offers Image sampling"
    );
    harness.get_by_role_and_label(Role::Label, "Sampling");
    harness.get_by_role_and_label(Role::Label, "Flood Mode");
    harness.get_by_role_and_label(Role::Label, "Tolerance");
}

#[test]
fn union_combine_mode_paints_a_combined_rectangle_selection() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    replace_field(&mut harness, "Selection left", "0");
    replace_field(&mut harness, "Selection top", "0");
    replace_field(&mut harness, "Selection right", "2");
    replace_field(&mut harness, "Selection bottom", "2");
    click(&mut harness, "Apply selection");
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(0, 0, 2, 2).expect("first selection"))
    );

    click(&mut harness, "Union");
    replace_field(&mut harness, "Selection right", "4");
    click(&mut harness, "Apply selection");
    assert_eq!(
        harness.state().session().document().selection(),
        Some(Rect::new(0, 0, 4, 2).expect("combined selection"))
    );
    assert!(
        harness
            .state()
            .session()
            .document()
            .selection_mask()
            .is_none()
    );
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());

    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(0.5, 0.5));
    let pixels = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    assert_eq!(pixels[0], Bgra::BLACK);
    assert_eq!(pixels[1], Bgra::BLACK);
    assert_eq!(
        pixels[3],
        Bgra::BLACK,
        "the union covers the second rectangle"
    );
    assert_eq!(pixels[800 + 3], Bgra::BLACK);
    assert_eq!(
        pixels[2 * 800],
        Bgra::WHITE,
        "the unselected row below the union is untouched"
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
    harness.get_by_role_and_label(Role::Label, "Tool Options");
    harness.get_by_role_and_label(Role::Label, "Colors");
    harness.get_by_role_and_label(Role::Label, "Layers");
    for label in [
        "Rectangle Select",
        "Move Selected Pixels",
        "Lasso Select",
        "Move Selection",
        "Ellipse Select",
        "Zoom",
        "Magic Wand",
        "Pan",
        "Paint Bucket",
        "Gradient",
        "Paintbrush",
        "Eraser",
        "Pencil",
        "Color Picker",
        "Clone Stamp",
        "Recolor",
        "Text",
        "Line / Curve",
        "Shapes",
        "Shapes (Freeform)",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }
}

#[test]
fn canvas_color_picker_samples_composed_pixels_without_editing() {
    let mut harness = app();
    let before = harness.state().session().document().clone();
    click(&mut harness, "Color Picker");
    harness
        .get_by_role_and_label(Role::Image, "Composed document canvas")
        .click();
    harness.run();

    assert_eq!(harness.state().fill_color(), Bgra::WHITE);
    harness.get_by_role_and_label(Role::Button, "Color Picker");
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
    click(&mut harness, "Shapes");
    click(&mut harness, "Fill");
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + egui::vec2(5.0, 6.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(14.0, 15.0));
    harness.run();
    harness.drop_at(rect.min + egui::vec2(14.0, 15.0));
    harness.run();

    let session = harness.state().session();
    assert_eq!(session.document().selection(), None);
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
    click(&mut harness, "More");
    replace_field(&mut harness, "Hex color", "#112233");
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
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
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
    harness.get_by_role_and_label(Role::Button, "Rectangle Select");
    harness.get_by_role_and_label(Role::Button, "Swap colors");
    harness.get_by_role_and_label(Role::Button, "Add Layer");
    menu_item(&mut harness, "Window", "Dock Panels");
    assert!(!harness.state().panels_floating());
    harness.get_by_role_and_label(Role::Button, "Add Layer");
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
    click(&mut harness, "More");
    replace_field(&mut harness, "Hex color", "#FF8000");

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
    assert_eq!(harness.state().fill_color(), Bgra::new(52, 52, 243, 255));

    click(&mut harness, "Preset blue");
    assert_eq!(harness.state().fill_color(), Bgra::new(243, 163, 52, 255));
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

    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
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

    click(&mut harness, "Layer Properties");
    harness
        .get_by_role_and_label(Role::Slider, "Layer opacity")
        .click();
    harness.run();
    click(&mut harness, "Apply");
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
fn dirty_open_adds_a_new_tab_and_preserves_the_old_session() {
    let temporary = TempDir::new().expect("temporary directory");
    let input = temporary.path().join("replacement.png");
    let source = write_input_png(&input);
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    let before_document = harness.state().session().document().clone();
    let before = harness.state().session().snapshot().expect("snapshot");

    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &input);
    click(&mut harness, "Open PNG");
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    let session = harness.state().session();
    assert_eq!(session.document().size(), source.size());
    assert_eq!(session.document().layers().len(), 1);
    assert_eq!(session.document().active_layer().surface(), &source);
    assert!(!session.dirty());

    click(&mut harness, "Document tab: New Image");
    assert_eq!(harness.state().session().document(), &before_document);
    assert_eq!(
        harness.state().session().snapshot().expect("snapshot"),
        before
    );
    click(&mut harness, "Document tab: replacement.png");
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface(),
        &source
    );
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
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
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

fn click_canvas_at(harness: &mut Harness<'_, PaintApp>, offset: egui::Vec2) {
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + offset);
    harness.run();
    harness.drop_at(rect.min + offset);
    harness.run();
}

#[test]
fn pencil_paintbrush_and_eraser_draw_real_pixels_and_undo() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(20.0, 6.0));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        for x in [5, 12, 20] {
            assert_eq!(
                *surface.pixel(Point::new(x, 6)).expect("pencil pixel"),
                Bgra::BLACK
            );
        }
        assert!(session.can_undo());
    }
    click(&mut harness, "Undo");
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(12, 6))
            .expect("restored"),
        Bgra::WHITE
    );

    click(&mut harness, "Paintbrush");
    drag_canvas(&mut harness, egui::vec2(5.0, 20.0), egui::vec2(10.0, 20.0));
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    assert_eq!(
        *surface.pixel(Point::new(8, 21)).expect("brush width"),
        Bgra::BLACK
    );

    click(&mut harness, "Eraser");
    drag_canvas(&mut harness, egui::vec2(5.0, 20.0), egui::vec2(10.0, 20.0));
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(8, 20))
            .expect("erased"),
        Bgra::TRANSPARENT
    );
}

#[test]
fn line_and_ellipse_shapes_fill_pixels_without_setting_a_selection() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Line / Curve");
    drag_canvas(&mut harness, egui::vec2(5.0, 5.0), egui::vec2(15.0, 5.0));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        assert_eq!(
            *surface.pixel(Point::new(10, 5)).expect("line midpoint"),
            Bgra::BLACK
        );
        assert_eq!(session.document().selection(), None);
        assert!(session.can_undo());
    }
    click(&mut harness, "Undo");

    click(&mut harness, "Shapes");
    click(&mut harness, "Fill");
    click(&mut harness, "Ellipse");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(30.0, 30.0));
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(20, 20)).expect("ellipse center"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(10, 10)).expect("ellipse corner"),
        Bgra::WHITE
    );
    assert_eq!(session.document().selection(), None);
    assert!(session.can_undo());
}

#[test]
fn gradient_tool_paints_primary_to_secondary_across_the_drag() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Gradient");
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(25.0, 6.0));
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    let left = *surface.pixel(Point::new(5, 6)).expect("gradient left");
    let right = *surface.pixel(Point::new(25, 6)).expect("gradient right");
    assert!(left.r < right.r, "left={left:?} right={right:?}");
    assert_eq!(right, Bgra::WHITE);
    assert_eq!(session.document().selection(), None);
    assert!(session.can_undo());
}

#[test]
fn recolor_tool_click_replaces_secondary_pixels_with_primary() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Recolor");
    drag_canvas(
        &mut harness,
        egui::vec2(400.0, 300.0),
        egui::vec2(410.0, 300.0),
    );
    let session = harness.state().session();
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(400, 300))
            .expect("recolored pixel"),
        Bgra::BLACK
    );
    assert!(session.can_undo());
}

#[test]
fn move_tool_shifts_the_active_layer_and_is_undoable() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Move Selected Pixels");
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(6.0, 6.0));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        assert_eq!(
            *surface.pixel(Point::new(0, 0)).expect("vacated pixel"),
            Bgra::TRANSPARENT
        );
        assert_eq!(
            *surface.pixel(Point::new(1, 0)).expect("shifted pixel"),
            Bgra::WHITE
        );
        assert!(session.can_undo());
    }
    click(&mut harness, "Undo");
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(0, 0))
            .expect("restored"),
        Bgra::WHITE
    );
}

#[test]
fn paint_bucket_click_fills_the_contiguous_canvas_area() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Paint Bucket");
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    canvas.click();
    harness.run();
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(400, 300)).expect("filled center"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(0, 0)).expect("filled corner"),
        Bgra::BLACK
    );
    assert!(session.can_undo());
}

#[test]
fn pan_tool_drags_without_editing_the_document() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pan");
    drag_canvas(
        &mut harness,
        egui::vec2(300.0, 300.0),
        egui::vec2(340.0, 320.0),
    );
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());
}

#[test]
fn default_shell_uses_floating_compact_side_windows() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    assert!(harness.state().panels_floating());
    harness.get_by_role_and_label(Role::Button, "Swap colors");
    harness.get_by_role_and_label(Role::Button, "Add Layer");
}

#[test]
fn color_wheel_updates_primary_color_continuously_while_dragging() {
    let mut harness = app();
    let wheel = harness.get_by_role_and_label(Role::Button, "Color wheel");
    let rect = wheel.rect();
    harness.drag_at(rect.center());
    harness.run();
    assert_eq!(harness.state().fill_color(), Bgra::WHITE);

    harness.hover_at(rect.center() + egui::vec2(rect.width() * 0.3, 0.0));
    harness.run();
    let dragged = harness.state().fill_color();
    assert_ne!(dragged, Bgra::WHITE);
    assert!(dragged.r > dragged.g, "expected warm hue, got {dragged:?}");

    harness.drop_at(rect.center() + egui::vec2(rect.width() * 0.3, 0.0));
    harness.run();
    assert_eq!(harness.state().fill_color(), dragged);
}

#[test]
fn colors_more_click_expands_the_whole_window_chrome() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    assert!(harness.state().panels_floating());

    click(&mut harness, "More");
    for _ in 0..5 {
        harness.run();
    }

    let bounds = harness
        .state()
        .colors_window_bounds()
        .expect("Colors window bounds");
    assert!(
        bounds.width() >= 400.0,
        "expanded width was {}",
        bounds.width()
    );
    let close = harness
        .get_by_role_and_label(Role::Button, "Close Colors window")
        .rect();
    // `bounds` includes egui's drop shadow on the right; the visible frame ends earlier.
    assert!(
        bounds.right() - close.right() < 32.0,
        "Colors header close ends at {} but window ends at {}",
        close.right(),
        bounds.right()
    );
}

#[test]
fn hsv_gradient_sliders_react_to_drag_and_window_size_stays_stable() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    click(&mut harness, "More");
    for _ in 0..4 {
        harness.run();
    }

    let start = harness.state().fill_color();
    let saturation = harness
        .get_by_role_and_label(Role::Slider, "Saturation")
        .rect();
    let value_rect = harness.get_by_role_and_label(Role::Slider, "Value").rect();
    let hue_rect = harness.get_by_role_and_label(Role::Slider, "Hue").rect();
    eprintln!(
        "COLORS_BOUNDS={:?} SAT={saturation:?} VAL={value_rect:?} HUE={hue_rect:?}",
        harness.state().colors_window_bounds()
    );
    harness.drag_at(saturation.center());
    harness.run();
    harness.hover_at(saturation.center() + egui::vec2(saturation.width() * 0.7, 0.0));
    harness.run();
    harness.drop_at(saturation.center() + egui::vec2(saturation.width() * 0.7, 0.0));
    harness.run();

    let value = harness.get_by_role_and_label(Role::Slider, "Value").rect();
    harness.drag_at(value.center());
    harness.run();
    harness.hover_at(value.center() + egui::vec2(value.width() * 0.7, 0.0));
    harness.run();
    let after_value = harness.state().fill_color();
    assert_ne!(
        after_value, start,
        "dragging Saturation/Value did not change the color"
    );
    harness.drop_at(value.center() + egui::vec2(value.width() * 0.7, 0.0));
    harness.run();

    let hue = harness.get_by_role_and_label(Role::Slider, "Hue").rect();
    harness.drag_at(hue.center());
    harness.run();
    harness.hover_at(hue.center() + egui::vec2(hue.width() * 0.35, 0.0));
    harness.run();
    let after_hue = harness.state().fill_color();
    assert_ne!(
        after_hue, after_value,
        "dragging Hue did not change the color"
    );
    harness.drop_at(hue.center() + egui::vec2(hue.width() * 0.35, 0.0));
    harness.run();

    let first = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");
    for _ in 0..5 {
        harness.run();
    }
    let second = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");
    assert!(
        (first.width() - second.width()).abs() < 1.0
            && (first.height() - second.height()).abs() < 1.0,
        "Colors window resized across idle frames: {first:?} -> {second:?}"
    );
}

#[test]
fn color_value_fields_keep_fixed_width_when_entering_long_values() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    click(&mut harness, "More");
    for _ in 0..4 {
        harness.run();
    }

    let red_before = harness.get_by_label("Red value").rect();
    let hue_before = harness.get_by_label("Hue value").rect();
    let window_before = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");

    let hue_slider = harness.get_by_role_and_label(Role::Slider, "Hue").rect();
    harness.drag_at(hue_slider.center());
    harness.run();
    harness.hover_at(hue_slider.right_top() + egui::vec2(-2.0, hue_slider.height() / 2.0));
    harness.run();
    harness.drop_at(hue_slider.right_top() + egui::vec2(-2.0, hue_slider.height() / 2.0));
    for _ in 0..4 {
        harness.run();
    }

    let red_after = harness.get_by_label("Red value").rect();
    let hue_after = harness.get_by_label("Hue value").rect();
    let window_after = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");

    eprintln!(
        "WINDOW before={window_before:?} after={window_after:?} RED {red_before:?}->{red_after:?} HUE {hue_before:?}->{hue_after:?}"
    );
    assert!((window_before.width() - window_after.width()).abs() < 0.5);
    assert!((window_before.height() - window_after.height()).abs() < 0.5);
}

#[test]
fn color_controls_share_right_edge_and_more_less_keep_position() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();

    let more = harness.get_by_role_and_label(Role::Button, "More").rect();
    click(&mut harness, "More");
    for _ in 0..4 {
        harness.run();
    }
    let less = harness.get_by_role_and_label(Role::Button, "Less").rect();
    assert!(
        (more.min.x - less.min.x).abs() < 0.5 && (more.min.y - less.min.y).abs() < 0.5,
        "More {more:?} and Less {less:?} are not at the same position"
    );

    let red = harness.get_by_label("Red value").rect();
    let hue = harness.get_by_label("Hue value").rect();
    let hex = harness.get_by_label("Hex color").rect();
    assert!(
        (red.right() - hue.right()).abs() < 2.0,
        "Red {red:?} and Hue {hue:?} do not share a right edge"
    );
    assert!(
        (red.right() - hex.right()).abs() < 2.0,
        "value fields {red:?} and HEX {hex:?} do not share a right edge"
    );
    let bounds = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");
    let primary = harness
        .get_by_role_and_label(Role::Button, "Primary color")
        .rect();
    eprintln!(
        "GEOM bounds={bounds:?} primary={primary:?} red={red:?} hex={hex:?} left={} right={}",
        primary.left() - bounds.left(),
        bounds.right() - red.right()
    );
    assert!(
        bounds.right() - red.right() < 30.0,
        "controls end at {} with window at {}",
        red.right(),
        bounds.right()
    );
}

#[test]
fn expanded_colors_controls_stay_inside_the_window() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    click(&mut harness, "More");
    for _ in 0..4 {
        harness.run();
    }
    let bounds = harness
        .state()
        .colors_window_bounds()
        .expect("Colors bounds");
    let labels = [
        "Red value",
        "Green value",
        "Blue value",
        "Hex color",
        "Hue value",
        "Saturation value",
        "Value value",
        "Opacity value",
    ];
    let mut lowest = bounds.top();
    for label in labels {
        let rect = harness.get_by_label(label).rect();
        assert!(
            rect.bottom() <= bounds.bottom() - 2.0,
            "{label} ends at {} outside window bottom {}",
            rect.bottom(),
            bounds.bottom()
        );
        lowest = lowest.max(rect.bottom());
    }
    assert!(
        bounds.bottom() - lowest >= 2.0,
        "lowest control ends at {lowest} with window bottom {}",
        bounds.bottom()
    );
}

#[test]
fn toolbox_has_twenty_aligned_two_column_cells() {
    let harness = app();
    let labels = [
        "Rectangle Select",
        "Move Selected Pixels",
        "Lasso Select",
        "Move Selection",
        "Ellipse Select",
        "Zoom",
        "Magic Wand",
        "Pan",
        "Paint Bucket",
        "Gradient",
        "Paintbrush",
        "Eraser",
        "Pencil",
        "Color Picker",
        "Clone Stamp",
        "Recolor",
        "Text",
        "Line / Curve",
        "Shapes",
        "Shapes (Freeform)",
    ];
    let mut rects = Vec::new();
    for label in labels {
        rects.push(harness.get_by_role_and_label(Role::Button, label).rect());
    }
    for pair in rects.chunks_exact(2) {
        assert!((pair[0].min.y - pair[1].min.y).abs() < 0.5);
        assert!((pair[1].min.x - pair[0].min.x - 26.0).abs() < 0.5);
    }
    for rows in rects.chunks_exact(4) {
        assert!((rows[2].min.y - rows[0].min.y - 25.0).abs() < 0.5);
    }
    assert!((rects[0].width() - 25.0).abs() < 0.5);
    assert!((rects[0].height() - 25.0).abs() < 0.5);
    let bounds = harness.state().tools_window_bounds().expect("Tools bounds");
    let left_margin = rects[0].left() - bounds.left();
    let right_margin = bounds.right() - rects[1].right();
    assert!(
        (left_margin - right_margin).abs() < 4.0,
        "toolbox margins {left_margin} vs {right_margin} bounds={bounds:?}"
    );
    assert!(bounds.height() < 310.0, "toolbox is too tall: {bounds:?}");
}

#[test]
fn move_selection_drag_translates_the_selection_bounds() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    let before = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("rectangle selection");
    click(&mut harness, "Move Selection");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    let after = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("moved selection");
    assert_eq!(after.left(), before.left() + 5);
    assert_eq!(after.top(), before.top() + 3);
    assert_eq!(after.right(), before.right() + 5);
    assert_eq!(after.bottom(), before.bottom() + 3);
}

#[test]
fn moving_selected_pixels_also_moves_the_selection_outline() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    let before = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("rectangle selection");
    click(&mut harness, "Move Selected Pixels");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    let after = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("selection after moving pixels");
    assert_eq!(after.left(), before.left() + 5);
    assert_eq!(after.top(), before.top() + 3);
    assert_eq!(after.right(), before.right() + 5);
    assert_eq!(after.bottom(), before.bottom() + 3);
}

#[test]
fn moving_pixels_lifts_once_and_reuses_the_floating_selection() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    click(&mut harness, "Move Selected Pixels");
    let before = harness.state().history_entries().len();
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    let after_first = harness.state().history_entries().len();
    assert_eq!(after_first, before + 1, "first move should lift once");
    drag_canvas(&mut harness, egui::vec2(15.0, 13.0), egui::vec2(20.0, 16.0));
    let after_second = harness.state().history_entries().len();
    assert_eq!(
        after_second, after_first,
        "second move must reuse the floating pixels instead of cutting again"
    );
    click(&mut harness, "Paintbrush");
    let after_tool = harness.state().history_entries().len();
    assert_eq!(
        after_tool,
        after_second + 1,
        "tool change should place pixels"
    );
}

#[test]
fn layers_bottom_bar_adds_duplicates_moves_deletes_and_merges() {
    let mut harness = app();
    for label in [
        "Add Layer",
        "Delete Layer",
        "Duplicate Layer",
        "Merge Layer Down",
        "Move Layer Up",
        "Move Layer Down",
        "Layer Properties",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }

    click(&mut harness, "Add Layer");
    assert_eq!(harness.state().session().document().layers().len(), 2);
    click(&mut harness, "Duplicate Layer");
    assert_eq!(harness.state().session().document().layers().len(), 3);
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        2
    );
    click(&mut harness, "Move Layer Down");
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        1
    );
    click(&mut harness, "Delete Layer");
    eprintln!("STATUS {}", harness.state().status());
    eprintln!(
        "LAYERS {:?}",
        harness
            .state()
            .session()
            .document()
            .layers()
            .iter()
            .map(|l| l.name())
            .collect::<Vec<_>>()
    );
    assert_eq!(harness.state().session().document().layers().len(), 2);
    click(&mut harness, "Merge Layer Down");
    assert_eq!(harness.state().session().document().layers().len(), 1);
    click(&mut harness, "Undo");
    assert_eq!(harness.state().session().document().layers().len(), 2);
}

#[test]
fn layers_window_margins_and_bottom_bar_are_symmetric() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    let bounds = harness
        .state()
        .layers_window_bounds()
        .expect("Layers bounds");
    let thumbnail = harness
        .get_by_label_contains("Layer 0 thumbnail: Background")
        .rect();
    let eye = harness
        .get_by_role_and_label(Role::CheckBox, "Layer 0 visible")
        .rect();
    let add = harness
        .get_by_role_and_label(Role::Button, "Add Layer")
        .rect();
    let left = thumbnail.left() - bounds.left();
    let right = bounds.right() - eye.right();
    assert!(
        (left - right).abs() < 4.0,
        "Layers margins {left} vs {right} bounds={bounds:?}"
    );
    assert!(
        bounds.bottom() - add.bottom() < 10.0,
        "bottom bar gap {}",
        bounds.bottom() - add.bottom()
    );
}

#[test]
fn layer_list_uses_paintnet_metrics_and_the_scrollbar_never_covers_text() {
    let mut harness = app();
    let geometry = harness
        .state()
        .layer_list_geometry()
        .expect("layer list geometry while the Layers panel is visible");
    assert!((geometry.item_height - 62.0).abs() < 0.5, "{geometry:?}");
    assert!((geometry.thumbnail_size - 50.0).abs() < 0.5, "{geometry:?}");
    assert!((geometry.padding - 4.0).abs() < 0.5, "{geometry:?}");
    assert!(
        geometry.scrollbar.is_none(),
        "one layer must not need a scrollbar"
    );
    let row = geometry
        .rows
        .iter()
        .find(|row| row.index == 0)
        .expect("background row geometry");
    assert!((row.rect.height() - 62.0).abs() < 0.5, "{row:?}");
    assert!((row.rect.width() - geometry.viewport.width()).abs() < 0.5);
    assert!((row.thumbnail.width() - 50.0).abs() < 0.5, "{row:?}");
    assert!((row.thumbnail.height() - 50.0).abs() < 0.5, "{row:?}");
    assert!(
        (row.thumbnail.left() - row.rect.left() - 4.0).abs() < 0.5,
        "thumbnail padding {row:?}"
    );
    assert!(
        (row.rect.right() - row.visibility.right() - 4.0).abs() < 0.5,
        "visibility hit area must sit 4 px from the row edge: {row:?}"
    );
    assert!(
        row.visibility.height() >= 50.0,
        "visibility hit area is too small: {row:?}"
    );

    for _ in 0..12 {
        click(&mut harness, "Add Layer");
    }
    let scroll_bars: Vec<egui::Rect> = harness
        .get_all_by_role(Role::ScrollBar)
        .map(|node| node.rect())
        .collect();
    let geometry = harness
        .state()
        .layer_list_geometry()
        .expect("layer list geometry after adding layers");
    for pair in geometry.rows.windows(2) {
        assert!(
            (pair[1].rect.top() - pair[0].rect.top() - 62.0).abs() < 0.5,
            "layer rows must keep an exact 62 px pitch: {pair:?}"
        );
    }
    let scrollbar = geometry
        .scrollbar
        .expect("a long layer list must show a scrollbar");
    assert!(scrollbar.width() >= 6.0, "{scrollbar:?}");
    assert!(
        (scrollbar.right() - geometry.outer.right()).abs() < 0.5,
        "scrollbar must sit at the right edge: {scrollbar:?} vs {:?}",
        geometry.outer
    );
    assert!(
        (scrollbar.left() - geometry.viewport.right()).abs() < 0.5,
        "scrollbar must be allocated outside the content viewport: {scrollbar:?} vs {:?}",
        geometry.viewport
    );
    let painted_scrollbar = scroll_bars
        .iter()
        .find(|rect| scrollbar.contains(rect.center()))
        .expect("the real scrollbar must sit inside the reserved strip");
    assert!(
        painted_scrollbar.left() >= geometry.viewport.right() - 0.5,
        "the rendered scrollbar must not overlap layer content: {painted_scrollbar:?}"
    );
    for row in &geometry.rows {
        assert!(
            row.rect.right() <= scrollbar.left() + 0.5,
            "layer row text may be covered by the scrollbar: {row:?} vs {scrollbar:?}"
        );
        assert!(
            (row.rect.right() - row.visibility.right() - 4.0).abs() < 0.5,
            "visibility hit area must stay inside the content viewport: {row:?}"
        );
    }
}

#[test]
fn pointer_dragging_a_layer_reorders_once_preserving_the_active_layer_and_undoes() {
    let mut harness = app();
    click(&mut harness, "Add Layer");
    click(&mut harness, "Add Layer");
    let before = layer_names(&harness);
    assert_eq!(before, ["Background", "Layer 1", "Layer 2"]);
    let active_before = harness
        .state()
        .session()
        .snapshot()
        .expect("snapshot")
        .active_layer_index;
    assert_eq!(active_before, 2);

    let start = harness
        .get_by_label_contains("Layer 2 thumbnail: Layer 2")
        .rect()
        .center();
    let background = harness
        .get_by_label_contains("Layer 0 thumbnail: Background")
        .rect();
    let end = background.center() + egui::vec2(0.0, 40.0);

    harness.drag_at(start);
    harness.run();
    harness.hover_at(end);
    harness.run();
    let geometry = harness
        .state()
        .layer_list_geometry()
        .expect("layer list geometry during a drag");
    assert!(
        geometry.insertion_indicator.is_some(),
        "dragging must paint an insertion indicator: {geometry:?}"
    );

    let history_before = harness.state().history_entries().len();
    harness.drop_at(end);
    harness.run();

    assert_eq!(
        layer_names(&harness),
        ["Layer 2", "Background", "Layer 1"],
        "one drag must move the dragged layer to the drop position"
    );
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        0,
        "the active layer keeps its identity and moves with its row"
    );
    assert_eq!(
        harness.state().history_entries().len(),
        history_before + 1,
        "one drag is one MoveLayer history entry"
    );
    assert_eq!(
        harness.state().session().document().active_layer().name(),
        "Layer 2"
    );

    click(&mut harness, "Undo");
    assert_eq!(layer_names(&harness), before);
    assert_eq!(
        harness.state().session().document().active_layer().name(),
        "Layer 2",
        "undo must not change the active layer identity"
    );
}

#[test]
fn double_clicking_a_layer_opens_properties_for_that_layer() {
    let mut harness = fast_app();
    click(&mut harness, "Add Layer");
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        1
    );

    double_click(&mut harness, "Active layer 0: Background");

    assert!(harness.state().layer_properties_open());
    assert_eq!(harness.state().layer_properties_target(), Some(0));
    harness.get_by_label("Layer: Background");
    harness.get_by_label("Blend Mode");
}

#[test]
fn layer_properties_rename_is_validated_undoable_and_commits_once() {
    let mut harness = app();
    click(&mut harness, "Add Layer");
    click(&mut harness, "Layer Properties");
    let history_before = harness.state().history_entries().len();

    replace_field(&mut harness, "Layer name", "Ink");
    click(&mut harness, "Apply");

    let layers = harness.state().session().document().layers();
    assert_eq!(layers[1].name(), "Ink");
    assert_eq!(
        harness.state().history_entries().len(),
        history_before + 1,
        "a rename is exactly one history commit"
    );
    assert!(harness.state().session().dirty());

    click(&mut harness, "Undo");
    assert_eq!(
        harness.state().session().document().layers()[1].name(),
        "Layer 1"
    );

    click(&mut harness, "Layer Properties");
    harness.get_by_label("Layer name").click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.key_press(egui::Key::Backspace);
    harness.run();
    click(&mut harness, "Apply");

    assert!(
        harness
            .query_by_label_contains("Layer name cannot be empty")
            .is_some(),
        "empty names must be rejected in the properties dialog"
    );
    assert_eq!(
        harness.state().session().document().layers()[1].name(),
        "Layer 1"
    );
    assert_eq!(harness.state().layer_properties_target(), Some(1));
}

#[test]
fn floating_windows_use_the_same_close_button_labels() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    for label in [
        "Close Tools window",
        "Close Colors window",
        "Close Layers window",
        "Close History window",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }
}

#[test]
fn image_effects_menus_and_panel_toggles_are_available() {
    let mut harness = app();
    click(&mut harness, "Add Layer");
    menu_item(&mut harness, "Image", "Flatten Image");
    assert_eq!(harness.state().session().document().layers().len(), 1);
    menu_item(&mut harness, "Effects", "Invert Colors");
    assert_eq!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()[0],
        Bgra::BLACK
    );
}

#[test]
fn panel_toggle_buttons_hide_and_restore_floating_windows() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Colors panel");
    click(&mut harness, "Colors panel");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Color wheel")
            .is_none()
    );
    click(&mut harness, "Colors panel");
    harness.get_by_role_and_label(Role::Button, "Color wheel");
}

#[test]
fn paintbrush_updates_the_document_while_dragging_before_release() {
    let mut harness = app();
    click(&mut harness, "Paintbrush");
    let rect = harness
        .get_by_role_and_label(Role::Image, "Composed document canvas")
        .rect();
    harness.drag_at(rect.min + egui::vec2(10.0, 10.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(30.0, 10.0));
    harness.run();
    let session = harness.state().session();
    let pixel = *session
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(30, 10))
        .expect("live stroke pixel");
    assert_eq!(pixel, Bgra::BLACK, "canvas did not update during the drag");
    assert_eq!(
        harness.state().history_entries().len(),
        1,
        "live preview must not create history before release"
    );
    harness.drop_at(rect.min + egui::vec2(30.0, 10.0));
    harness.run();
    assert_eq!(harness.state().history_entries().len(), 2);
}

#[test]
fn document_tab_shows_a_preview_and_the_opened_filename() {
    let temporary = TempDir::new().expect("temporary directory");
    let path = temporary.path().join("tab-document.png");
    write_input_png(&path);
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    harness.get_by_role_and_label(Role::Image, "Document tab preview");
    menu_item(&mut harness, "File", "Open PNG…");
    enter_path(&mut harness, &path);
    click(&mut harness, "Open PNG");
    harness.get_by_role_and_label(Role::Button, "Document tab: tab-document.png");
    harness.get_by_role_and_label(Role::Image, "Document tab preview");
}

#[test]
fn history_scrollbar_sits_at_the_window_edge_and_footer_is_flush() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    for _ in 0..14 {
        click(&mut harness, "Add Layer");
    }
    let bounds = harness
        .state()
        .history_window_bounds()
        .expect("History bounds");
    let scrollbar = harness
        .get_all_by_role(Role::ScrollBar)
        .map(|node| node.rect())
        .find(|rect| bounds.contains(rect.center()))
        .expect("History scrollbar");
    assert!(
        bounds.right() - scrollbar.right() < 10.0,
        "History scrollbar is not at the right edge: {scrollbar:?} within {bounds:?}"
    );
    let undo = harness
        .get_by_role_and_label(Role::Button, "Undo History")
        .rect();
    assert!(
        bounds.bottom() - undo.bottom() < 10.0,
        "History footer is not flush: {undo:?} within {bounds:?}"
    );
}

#[test]
fn document_tab_strip_keeps_the_file_actions_in_a_fixed_tab() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    for label in [
        "New document",
        "Open PNG file",
        "Export PNG file",
        "Undo",
        "Redo",
        "Document tab preview",
    ] {
        if label == "Document tab preview" {
            harness.get_by_role_and_label(Role::Image, label);
        } else {
            harness.get_by_role_and_label(Role::Button, label);
        }
    }
}

#[test]
fn new_documents_create_switchable_independent_tabs() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    click(&mut harness, "Add Layer");
    assert_eq!(harness.state().document_tab_count(), 1);
    assert_eq!(harness.state().session().document().layers().len(), 2);

    click(&mut harness, "New document");
    assert_eq!(harness.state().document_tab_count(), 2);
    assert_eq!(harness.state().active_document_index(), 1);
    assert_eq!(harness.state().session().document().layers().len(), 1);
    assert_eq!(harness.state().history_entries(), ["New Image"]);
    assert_eq!(
        harness.state().document_titles(),
        ["New Image", "New Image 2"]
    );

    click(&mut harness, "Document tab: New Image");
    assert_eq!(harness.state().active_document_index(), 0);
    assert_eq!(harness.state().session().document().layers().len(), 2);
    assert_eq!(harness.state().history_entries().len(), 2);

    click(&mut harness, "Document tab: New Image 2");
    assert_eq!(harness.state().active_document_index(), 1);
    assert_eq!(harness.state().session().document().layers().len(), 1);
}

#[test]
fn toolbar_icons_are_vertically_centered_with_the_document_tab() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    let mut centers = Vec::new();
    for label in [
        "New document",
        "Open PNG file",
        "Export PNG file",
        "Undo",
        "Redo",
    ] {
        centers.push(
            harness
                .get_by_role_and_label(Role::Button, label)
                .rect()
                .center()
                .y,
        );
    }
    let preview = harness
        .get_by_role_and_label(Role::Image, "Document tab preview")
        .rect()
        .center()
        .y;
    let icon_min = centers.iter().copied().fold(f32::INFINITY, f32::min);
    let icon_max = centers.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        icon_max - icon_min < 1.0,
        "toolbar icons are not vertically centered: {centers:?}"
    );
    assert!(
        (preview - centers[0]).abs() < 6.0,
        "document tab preview is far from the icon center: {preview} vs {}",
        centers[0]
    );
}

#[test]
fn reference_driven_layer_menu_and_zoom_to_selection_work() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    menu_item(&mut harness, "View", "Zoom to Selection");
    assert!(harness.state().zoom() > 1.0);

    menu_item(&mut harness, "Layers", "Add New Layer");
    assert_eq!(harness.state().session().document().layers().len(), 2);
    menu_item(&mut harness, "Layers", "Toggle Layer Visibility");
    assert!(!harness.state().session().document().layers()[1].is_visible());
    menu_item(&mut harness, "Layers", "Move Layer to Top");
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        1
    );
    menu_item(&mut harness, "Layers", "Go to Bottom Layer");
    assert_eq!(
        harness
            .state()
            .session()
            .snapshot()
            .expect("snapshot")
            .active_layer_index,
        0
    );
    menu_item(&mut harness, "Image", "Flatten Image");
    assert_eq!(harness.state().session().document().layers().len(), 1);
}

#[test]
fn effects_menu_uses_paint_net_groups_and_applies_real_effects_undoably() {
    let mut harness = app();

    click(&mut harness, "Effects");
    for group in [
        "Artistic", "Blurs", "Distort", "Noise", "Photo", "Render", "Stylize",
    ] {
        harness.get_by_role_and_label(Role::Button, group);
    }
    harness.get_by_role_and_label(Role::Button, "Invert Colors");

    click(&mut harness, "Photo");
    harness.get_by_role_and_label(Role::Button, "Grayscale");
    click(&mut harness, "Sepia");
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()[0],
        Bgra::new(239, 255, 255, 255)
    );
    click(&mut harness, "Undo");
    assert_eq!(
        harness
            .state()
            .session()
            .compose()
            .expect("composite")
            .pixels()[0],
        Bgra::WHITE
    );

    click(&mut harness, "Shapes");
    click(&mut harness, "Fill");
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(14.0, 15.0));
    menu_item(&mut harness, "Effects", "Blurs");
    harness.get_by_role_and_label(Role::Button, "Gaussian Blur…");
    assert!(button_is_disabled(&harness, "Motion Blur"));
    click(&mut harness, "Gaussian Blur…");
    harness.get_by_role_and_label(Role::Slider, "Radius");
    click(&mut harness, "Apply");

    let session = harness.state().session();
    let blended = *session
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(16, 10))
        .expect("blurred neighbor");
    assert!(
        blended.r < 255,
        "the rectangle edge should blur into the canvas: {blended:?}"
    );
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(30, 10))
            .expect("far white pixel"),
        Bgra::WHITE
    );
    assert!(
        harness
            .state()
            .history_entries()
            .last()
            .is_some_and(|label| label.contains("Gaussian Blur")),
        "history should record the configured blur: {:?}",
        harness.state().history_entries()
    );

    click(&mut harness, "Undo");
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(16, 10))
            .expect("restored pixel"),
        Bgra::WHITE
    );
}

fn alt_click_canvas_at(harness: &mut Harness<'_, PaintApp>, offset: egui::Vec2) {
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let position = canvas.rect().min + offset;
    let modifiers = egui::Modifiers::ALT;
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(modifiers));
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(position));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
}

fn changed_pixel_count(before: &[Bgra], after: &[Bgra]) -> usize {
    before
        .iter()
        .zip(after)
        .filter(|(before, after)| before != after)
        .count()
}

#[test]
fn text_and_clone_stamp_are_enabled_with_target_specific_options() {
    let mut harness = app();
    for label in ["Text", "Clone Stamp"] {
        assert!(
            !button_is_disabled(&harness, label),
            "{label} should no longer be a disabled toolbox slot"
        );
    }

    click(&mut harness, "Text");
    harness.get_by_label("Text content");
    harness.get_by_label("Font family");
    harness.get_by_label("Font size");
    harness.get_by_label("Bold");
    harness.get_by_label("Italic");
    harness.get_by_label("Antialiasing");
    harness.get_by_role_and_label(Role::Button, "Place text");
    harness.get_by_role_and_label(Role::Button, "Cancel text");

    click(&mut harness, "Clone Stamp");
    harness.get_by_label("Hardness");
    harness.get_by_role_and_label(Role::Button, "Clear clone source");
    harness.get_by_label_contains("Alt-click the canvas");
}

#[test]
fn text_tool_previews_commits_and_undoes_real_layer_pixels() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Text");
    replace_field(&mut harness, "Text content", "Ag");
    let before = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();

    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    harness.run();
    let preview = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    assert!(
        changed_pixel_count(&before, &preview) > 0,
        "text should be visible as a live preview before commit"
    );
    assert!(
        preview.iter().any(|pixel| pixel.a == 255 && pixel.r < 128),
        "the live preview should contain opaque glyph ink"
    );
    assert!(
        !harness.state().session().can_undo(),
        "the live text preview must not enter undo history"
    );

    click(&mut harness, "Place text");
    assert!(harness.state().session().can_undo());
    let committed = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    assert_eq!(
        committed, preview,
        "commit should preserve the previewed pixels"
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );

    click(&mut harness, "Undo");
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        before.as_slice()
    );
}

#[test]
fn text_preview_clips_to_the_selection_and_style_flags_restyle_it() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(46.0, 60.0));
    click(&mut harness, "Text");
    replace_field(&mut harness, "Text content", "MM");
    click_canvas_at(&mut harness, egui::vec2(8.0, 8.0));
    harness.run();

    let selection = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("selection");
    let preview = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    let non_white = preview
        .iter()
        .enumerate()
        .filter(|(_, pixel)| **pixel != Bgra::WHITE)
        .count();
    assert!(non_white > 0, "clipped text preview should still draw ink");
    for (index, pixel) in preview.iter().enumerate() {
        if *pixel != Bgra::WHITE {
            let x = (index % 800) as i64;
            let y = (index / 800) as i64;
            assert!(
                x >= selection.left()
                    && x < selection.right()
                    && y >= selection.top()
                    && y < selection.bottom(),
                "text preview leaked outside the selection at ({x}, {y})"
            );
        }
    }
    let regular_ink = preview.iter().filter(|pixel| pixel.r < 128).count();
    assert!(!harness.state().session().can_undo());

    harness.get_by_label("Bold").click();
    harness.run();
    let bold = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    let bold_ink = bold.iter().filter(|pixel| pixel.r < 128).count();
    assert!(
        bold_ink > regular_ink,
        "bold preview should have more ink: bold={bold_ink} regular={regular_ink}"
    );
    assert!(
        !harness.state().session().can_undo(),
        "restyling the preview must remain undo-free"
    );

    click(&mut harness, "Place text");
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        bold.as_slice()
    );
}

#[test]
fn clone_stamp_alt_click_sets_source_and_cloned_strokes_are_undoable() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(8.0, 40.0), egui::vec2(40.0, 40.0));

    click(&mut harness, "Clone Stamp");
    alt_click_canvas_at(&mut harness, egui::vec2(20.0, 40.0));
    harness.get_by_label_contains("Source: (20, 40)");
    drag_canvas(
        &mut harness,
        egui::vec2(100.0, 100.0),
        egui::vec2(120.0, 100.0),
    );

    let session = harness.state().session();
    assert!(session.can_undo());
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(105, 100))
            .expect("cloned pixel"),
        Bgra::BLACK
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Clone Stamp")
    );
    harness.get_by_label_contains("Offset: (80, 60)");

    click(&mut harness, "Undo");
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(105, 100))
            .expect("restored pixel"),
        Bgra::WHITE
    );
}

#[test]
fn clone_stamp_respects_selection_clipping() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(8.0, 40.0), egui::vec2(40.0, 40.0));
    click(&mut harness, "Rectangle Select");
    drag_canvas(
        &mut harness,
        egui::vec2(96.0, 90.0),
        egui::vec2(116.0, 110.0),
    );
    click(&mut harness, "Clone Stamp");
    alt_click_canvas_at(&mut harness, egui::vec2(20.0, 40.0));
    drag_canvas(
        &mut harness,
        egui::vec2(100.0, 100.0),
        egui::vec2(110.0, 100.0),
    );

    let session = harness.state().session();
    let selection = session.document().selection().expect("selection");
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(100, 100))
            .expect("cloned pixel"),
        Bgra::BLACK
    );
    for (x, y) in [(95, 100), (118, 100), (100, 88), (100, 112)] {
        assert_eq!(
            *session
                .document()
                .active_layer()
                .surface()
                .pixel(Point::new(x, y))
                .expect("unclipped pixel"),
            Bgra::WHITE,
            "clone leaked outside the selection at ({x}, {y})"
        );
    }
    assert!(selection.contains(Point::new(100, 100)));
}

#[test]
fn text_cancel_discards_the_preview_and_tool_switch_commits_it() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Text");
    replace_field(&mut harness, "Text content", "Hi");
    let before = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec();
    click_canvas_at(&mut harness, egui::vec2(12.0, 12.0));
    harness.run();
    assert!(
        changed_pixel_count(
            &before,
            harness
                .state()
                .session()
                .document()
                .active_layer()
                .surface()
                .pixels()
        ) > 0
    );

    click(&mut harness, "Cancel text");
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        before.as_slice()
    );
    assert!(!harness.state().session().can_undo());
    assert!(harness.state().status().contains("Canceled"));

    click_canvas_at(&mut harness, egui::vec2(12.0, 12.0));
    harness.run();
    click(&mut harness, "Paintbrush");
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
    assert!(
        changed_pixel_count(
            &before,
            harness
                .state()
                .session()
                .document()
                .active_layer()
                .surface()
                .pixels()
        ) > 0
    );
}

#[test]
fn clone_stamp_without_a_source_does_not_edit_the_document() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Clone Stamp");
    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));
    assert!(
        harness.state().status().contains("Alt-click the canvas"),
        "status should explain how to set a clone source"
    );
    assert!(!harness.state().session().dirty());
    assert!(!harness.state().session().can_undo());
}

#[test]
fn ellipse_select_drag_creates_a_mask_and_fills_only_inside_it() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Ellipse Select");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(39.0, 39.0));

    let session = harness.state().session();
    let document = session.document();
    assert_eq!(
        document.selection(),
        Some(Rect::new(10, 10, 40, 40).expect("ellipse bounds"))
    );
    let mask = document
        .selection_mask()
        .expect("ellipse selection must be a real mask");
    assert!(mask.contains(Point::new(25, 25)));
    assert!(!mask.contains(Point::new(10, 10)));
    assert!(!session.dirty());
    assert!(!session.can_undo());

    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(25.5, 25.5));
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(25, 25)).expect("ellipse center"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(10, 10)).expect("ellipse corner"),
        Bgra::WHITE
    );
    assert_eq!(
        *surface.pixel(Point::new(0, 0)).expect("canvas corner"),
        Bgra::WHITE
    );

    click(&mut harness, "Undo");
    let session = harness.state().session();
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(25, 25))
            .expect("restored center"),
        Bgra::WHITE
    );
    assert!(
        session
            .document()
            .selection_mask()
            .is_some_and(|mask| mask.contains(Point::new(25, 25))),
        "undo must keep the non-rectangular selection active"
    );
}

#[test]
fn lasso_select_freehand_trace_masks_the_eraser() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(0.5, 0.5));
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(0, 0))
            .expect("black canvas"),
        Bgra::BLACK
    );

    click(&mut harness, "Lasso Select");
    let dirty_before = harness.state().session().dirty();
    let undo_before = harness.state().session().can_undo();
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + egui::vec2(5.0, 5.0));
    harness.run();
    for offset in [
        egui::vec2(40.0, 5.0),
        egui::vec2(40.0, 40.0),
        egui::vec2(5.0, 40.0),
    ] {
        harness.hover_at(rect.min + offset);
        harness.run();
    }
    harness.drop_at(rect.min + egui::vec2(5.0, 40.0));
    harness.run();

    let session = harness.state().session();
    let mask = session
        .document()
        .selection_mask()
        .expect("lasso trace must produce a mask")
        .clone();
    assert!(mask.contains(Point::new(20, 20)));
    assert!(!mask.contains(Point::new(1, 1)));
    assert_eq!(session.dirty(), dirty_before);
    assert_eq!(session.can_undo(), undo_before);

    click(&mut harness, "Eraser");
    drag_canvas(&mut harness, egui::vec2(0.0, 20.0), egui::vec2(80.0, 20.0));
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(20, 20)).expect("inside lasso"),
        Bgra::TRANSPARENT
    );
    assert_eq!(
        *surface.pixel(Point::new(0, 20)).expect("outside lasso"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(79, 20)).expect("outside lasso"),
        Bgra::BLACK
    );

    click(&mut harness, "Undo");
    let session = harness.state().session();
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(20, 20))
            .expect("restored by undo"),
        Bgra::BLACK
    );
    assert!(
        session.document().selection_mask().is_some(),
        "undo keeps the lasso mask"
    );
}

#[test]
fn magic_wand_click_selects_a_contiguous_region_and_undo_keeps_the_mask() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Shapes");
    click(&mut harness, "Fill");
    drag_canvas(&mut harness, egui::vec2(20.0, 0.0), egui::vec2(30.0, 599.0));

    let dirty_before = harness.state().session().dirty();
    let undo_before = harness.state().session().can_undo();
    click(&mut harness, "Magic Wand");
    click_canvas_at(&mut harness, egui::vec2(0.5, 0.5));
    let session = harness.state().session();
    let mask = session
        .document()
        .selection_mask()
        .expect("magic wand must produce a mask")
        .clone();
    assert!(mask.contains(Point::new(5, 5)));
    assert!(!mask.contains(Point::new(25, 5)));
    assert!(!mask.contains(Point::new(50, 5)));
    assert_eq!(session.dirty(), dirty_before);
    assert_eq!(session.can_undo(), undo_before);

    click(&mut harness, "Eraser");
    drag_canvas(&mut harness, egui::vec2(0.0, 5.0), egui::vec2(79.0, 5.0));
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(5, 5)).expect("wand region"),
        Bgra::TRANSPARENT
    );
    assert_eq!(
        *surface
            .pixel(Point::new(50, 5))
            .expect("outside wand region"),
        Bgra::WHITE
    );
    assert_eq!(
        *surface.pixel(Point::new(25, 5)).expect("black shape"),
        Bgra::BLACK
    );

    click(&mut harness, "Undo");
    let session = harness.state().session();
    assert_eq!(
        *session
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(5, 5))
            .expect("restored by undo"),
        Bgra::WHITE
    );
    assert!(
        session.document().selection_mask().is_some(),
        "undo keeps the magic wand mask"
    );
}

#[test]
fn moving_a_masked_selection_preserves_its_shape_and_undo_restores_content() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Ellipse Select");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(39.0, 39.0));
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(25.5, 25.5));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        assert_eq!(
            *surface.pixel(Point::new(25, 25)).expect("painted center"),
            Bgra::BLACK
        );
        assert_eq!(
            *surface.pixel(Point::new(10, 10)).expect("ellipse corner"),
            Bgra::WHITE
        );
    }

    click(&mut harness, "Move Selected Pixels");
    drag_canvas(&mut harness, egui::vec2(20.0, 20.0), egui::vec2(30.0, 20.0));
    {
        let session = harness.state().session();
        assert_eq!(
            session.document().selection(),
            Some(Rect::new(20, 10, 50, 40).expect("translated ellipse bounds"))
        );
        let mask = session
            .document()
            .selection_mask()
            .expect("moved mask")
            .clone();
        assert!(mask.contains(Point::new(35, 25)));
        assert!(!mask.contains(Point::new(15, 25)));
    }

    click(&mut harness, "Paintbrush");
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface
            .pixel(Point::new(25, 25))
            .expect("moved black pixel"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface
            .pixel(Point::new(45, 25))
            .expect("moved ellipse right edge"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface
            .pixel(Point::new(15, 25))
            .expect("vacated mask pixel"),
        Bgra::TRANSPARENT
    );
    assert_eq!(
        *surface.pixel(Point::new(5, 5)).expect("outside mask"),
        Bgra::WHITE
    );

    click(&mut harness, "Undo");
    click(&mut harness, "Undo");
    let session = harness.state().session();
    let surface = session.document().active_layer().surface();
    assert_eq!(
        *surface.pixel(Point::new(15, 25)).expect("restored source"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(25, 25)).expect("restored center"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface
            .pixel(Point::new(45, 25))
            .expect("restored right side"),
        Bgra::WHITE
    );
}

#[test]
fn document_tabs_can_be_closed_individually_with_a_dirty_guard() {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    click(&mut harness, "New document");
    assert_eq!(harness.state().document_tab_count(), 2);

    click(&mut harness, "Close document New Image");
    assert_eq!(harness.state().document_tab_count(), 1);
    assert_eq!(harness.state().document_titles(), ["New Image 2"]);

    click(&mut harness, "Close document New Image 2");
    assert_eq!(harness.state().document_tab_count(), 1);
    assert_eq!(harness.state().document_titles(), ["New Image"]);

    click(&mut harness, "New document");
    menu_item(&mut harness, "Adjustments", "Invert Colors");
    assert!(harness.state().session().dirty());
    click(&mut harness, "Close document New Image 2");
    harness.get_by_role_and_label(Role::Button, "Cancel");
    click(&mut harness, "Cancel");
    assert_eq!(harness.state().document_tab_count(), 2);
    click(&mut harness, "Close document New Image 2");
    click(&mut harness, "Discard");
    assert_eq!(harness.state().document_tab_count(), 1);
    assert_eq!(harness.state().document_titles(), ["New Image"]);
}

#[test]
fn view_menu_grid_toggle_enables_the_overlay() {
    let mut harness = app();
    assert!(!harness.state().grid_enabled());
    harness.get_by_role_and_label(Role::Button, "View").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Grid")
        .click();
    harness.run();
    assert!(harness.state().grid_enabled());
}

#[test]
fn view_units_change_status_coordinates() {
    let mut harness = app();
    harness.get_by_role_and_label(Role::Button, "View").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Inches")
        .click();
    harness.run();
    harness.get_by_label("—, — in");
    assert!(harness.query_by_label("—, — px").is_none());

    harness.get_by_role_and_label(Role::Button, "View").click();
    harness.run();
    harness
        .get_by_role_and_label(Role::RadioButton, "Centimeters")
        .click();
    harness.run();
    harness.get_by_label("—, — cm");
}

#[test]
fn brush_options_expose_paint_net_hardness_spacing_and_size_defaults() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Paintbrush");
    assert_eq!(harness.state().brush_size(), 45);
    assert_eq!(harness.state().brush_hardness(), 75);
    assert_eq!(harness.state().brush_spacing(), 15);
    harness.get_by_label("Hardness");
    harness.get_by_label("Spacing");
    harness.get_by_label("Width");

    click(&mut harness, "Eraser");
    harness.get_by_label("Hardness");
    harness.get_by_label("Spacing");

    click(&mut harness, "Recolor");
    harness.get_by_label("Hardness");
    harness.get_by_label("Spacing");
    harness.get_by_label("Tolerance");
}

#[test]
fn brush_hardness_and_spacing_options_change_painted_pixels() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Paintbrush");
    harness.state_mut().set_brush_options(8, 0, 100);
    drag_canvas(&mut harness, egui::vec2(5.0, 50.0), egui::vec2(45.0, 50.0));
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    assert_eq!(
        *surface.pixel(Point::new(17, 50)).expect("spacing gap"),
        Bgra::WHITE,
        "spacing 100% should leave a gap between stamps"
    );
    let soft = *surface.pixel(Point::new(15, 50)).expect("soft edge");
    assert!(
        (60..=200).contains(&soft.r),
        "hardness 0 should blend the edge instead of painting it solid: {soft:?}"
    );

    click(&mut harness, "Undo");
    harness.state_mut().set_brush_options(8, 100, 10);
    drag_canvas(&mut harness, egui::vec2(5.0, 50.0), egui::vec2(45.0, 50.0));
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    assert_eq!(
        *surface.pixel(Point::new(17, 50)).expect("dense bridge"),
        Bgra::BLACK,
        "spacing 10% and hardness 100 should paint a connected hard stroke"
    );
}

#[test]
fn shape_options_support_outline_fill_both_and_border_width() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Shapes");
    assert_eq!(harness.state().shape_style(), ShapeStyle::Outline);
    assert_eq!(harness.state().shape_border_width(), 2);
    harness.get_by_label("Fill style");
    harness.get_by_label("Border width");
    for label in ["Outline", "Fill", "Both", "Rectangle", "Ellipse"] {
        harness.get_by_role_and_label(Role::Button, label);
    }

    drag_canvas(&mut harness, egui::vec2(5.0, 5.0), egui::vec2(15.0, 15.0));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        assert_eq!(
            *surface.pixel(Point::new(5, 5)).expect("outline corner"),
            Bgra::BLACK
        );
        assert_eq!(
            *surface.pixel(Point::new(10, 10)).expect("hollow centre"),
            Bgra::WHITE
        );
        assert_eq!(
            harness.state().history_entries().last().map(String::as_str),
            Some("Rectangle"),
            "history should record the rectangle"
        );
    }
    click(&mut harness, "Undo");

    click(&mut harness, "Both");
    drag_canvas(&mut harness, egui::vec2(5.0, 5.0), egui::vec2(15.0, 15.0));
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    assert_eq!(
        *surface.pixel(Point::new(5, 5)).expect("both outline"),
        Bgra::BLACK
    );
    assert_eq!(
        *surface.pixel(Point::new(10, 10)).expect("both interior"),
        Bgra::BLACK
    );
    click(&mut harness, "Undo");

    harness
        .state_mut()
        .set_shape_options(ShapeStyle::Outline, 3);
    drag_canvas(&mut harness, egui::vec2(5.0, 5.0), egui::vec2(15.0, 15.0));
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    assert_eq!(
        *surface.pixel(Point::new(6, 6)).expect("wide border"),
        Bgra::BLACK,
        "a three-pixel border reaches the second inset ring"
    );
    assert_eq!(
        *surface.pixel(Point::new(10, 10)).expect("still hollow"),
        Bgra::WHITE
    );
}

#[test]
fn gradient_options_select_shape_and_repeat_modes() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Gradient");
    assert_eq!(harness.state().gradient_type(), GradientType::Linear);
    assert_eq!(harness.state().gradient_repeat(), GradientRepeat::None);
    harness.get_by_label("Gradient type");
    harness.get_by_label("Repeat mode");
    for label in ["Linear", "Reflected", "Diamond", "Radial", "Conical"] {
        harness.get_by_role_and_label(Role::Button, label);
    }
    for label in ["No Repeat", "Repeat"] {
        harness.get_by_role_and_label(Role::Button, label);
    }

    click(&mut harness, "Repeat");
    assert_eq!(harness.state().gradient_repeat(), GradientRepeat::Repeat);
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(25.0, 6.0));
    {
        let session = harness.state().session();
        let surface = session.document().active_layer().surface();
        let wrapped = *surface.pixel(Point::new(35, 6)).expect("wrapped pixel");
        assert!(
            wrapped.r > 60 && wrapped.r < 220,
            "Repeat should sample 55% of the next period: {wrapped:?}"
        );
        assert_eq!(
            session.document().selection(),
            None,
            "gradients must not create a selection"
        );
    }
    click(&mut harness, "Undo");

    click(&mut harness, "No Repeat");
    drag_canvas(&mut harness, egui::vec2(5.0, 6.0), egui::vec2(25.0, 6.0));
    assert_eq!(
        *harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixel(Point::new(35, 6))
            .expect("clamped pixel"),
        Bgra::WHITE,
        "No Repeat clamps to the secondary colour beyond the drag"
    );
    click(&mut harness, "Undo");

    click(&mut harness, "Reflected");
    assert_eq!(harness.state().gradient_type(), GradientType::Reflected);
    drag_canvas(&mut harness, egui::vec2(10.0, 20.0), egui::vec2(20.0, 20.0));
    let reflected = *harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(2, 20))
        .expect("reflected before start");
    assert!(
        reflected.r > 100 && reflected.r < 220,
        "Reflected mirrors the gradient before the drag start: {reflected:?}"
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Gradient")
    );
}
