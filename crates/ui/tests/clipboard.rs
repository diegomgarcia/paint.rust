use eframe::egui::{self, Key, Modifiers, Role};
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
    dock_tools(&mut harness);
    harness
}

fn dock_tools(harness: &mut Harness<'_, PaintApp>) {
    click(harness, "Window");
    harness
        .get_by_role_and_label(Role::CheckBox, "Float Tools")
        .click();
    harness.run();
}

fn click(harness: &mut Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run();
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

fn click_canvas_at(harness: &mut Harness<'_, PaintApp>, offset: egui::Vec2) {
    let canvas = harness.get_by_role_and_label(Role::Image, "Composed document canvas");
    let rect = canvas.rect();
    harness.drag_at(rect.min + offset);
    harness.run();
    harness.drop_at(rect.min + offset);
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

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> Rect {
    Rect::new(left, top, right, bottom).expect("test rectangle")
}

fn paint_selection_black(harness: &mut Harness<'_, PaintApp>) {
    click(harness, "Rectangle Select");
    drag_canvas(harness, egui::vec2(10.0, 10.0), egui::vec2(30.0, 30.0));
    click(harness, "Paint Bucket");
    click_canvas_at(harness, egui::vec2(20.5, 20.5));
    assert_eq!(pixel(harness, 20, 20), Bgra::BLACK);
    assert_eq!(pixel(harness, 5, 5), Bgra::WHITE);
}

#[test]
fn edit_menu_reflects_selection_and_clipboard_state() {
    let mut harness = app();
    click(&mut harness, "Edit");
    assert!(
        !button_is_disabled(&harness, "Copy"),
        "Copy always works because it falls back to the whole active layer"
    );
    assert!(
        button_is_disabled(&harness, "Cut"),
        "Cut needs a selection like Paint.NET"
    );
    assert!(button_is_disabled(&harness, "Paste"), "nothing copied yet");
    harness.key_press(Key::Escape);
    harness.run();

    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(2.0, 2.0), egui::vec2(20.0, 20.0));
    click(&mut harness, "Edit");
    assert!(!button_is_disabled(&harness, "Cut"));
    assert!(button_is_disabled(&harness, "Paste"));
    click(&mut harness, "Copy");
    assert_eq!(
        harness.state().clipboard_bounds(),
        Some(rect(2, 2, 21, 21)),
        "the menu Copy action fills the app clipboard"
    );

    click(&mut harness, "Edit");
    assert!(
        !button_is_disabled(&harness, "Paste"),
        "Paste is enabled after Copy"
    );
    harness.key_press(Key::Escape);
    harness.run();
}

#[test]
fn command_c_and_v_paste_at_original_bounds_and_finish_as_one_undo_entry() {
    let mut harness = app();
    paint_selection_black(&mut harness);
    let history_before_copy = harness.state().history_entries().len();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    assert_eq!(
        harness.state().history_entries().len(),
        history_before_copy,
        "Copy is not a content edit"
    );
    assert_eq!(
        harness.state().clipboard_bounds(),
        Some(rect(10, 10, 31, 31))
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::D);
    harness.run();
    assert!(harness.state().session().document().selection().is_none());

    let history_before_paste = harness.state().history_entries().len();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert_eq!(
        harness.state().floating_selection_bounds(),
        Some(rect(10, 10, 31, 31)),
        "Paste starts a floating selection at the copied bounds"
    );
    assert_eq!(
        harness.state().session().document().selection(),
        Some(rect(10, 10, 31, 31)),
        "Paste installs the pasted area as the selection like Paint.NET"
    );

    drag_canvas(&mut harness, egui::vec2(15.0, 15.0), egui::vec2(20.0, 20.0));
    click(&mut harness, "Finish");
    assert_eq!(
        harness.state().history_entries().len(),
        history_before_paste + 1,
        "moving the floating paste and finishing is one undo entry"
    );
    assert_eq!(
        harness.state().floating_selection_bounds(),
        None,
        "Finish commits the floating paste"
    );
    assert_eq!(pixel(&harness, 34, 34), Bgra::BLACK);
    assert_eq!(pixel(&harness, 15, 15), Bgra::BLACK);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        pixel(&harness, 34, 34),
        Bgra::WHITE,
        "one undo removes the whole moved paste"
    );
    assert_eq!(pixel(&harness, 15, 15), Bgra::BLACK);

    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    assert_eq!(pixel(&harness, 34, 34), Bgra::BLACK);
}

#[test]
fn command_x_cuts_the_selection_clears_it_and_undo_restores_both() {
    let mut harness = app();
    paint_selection_black(&mut harness);
    let history_before = harness.state().history_entries().len();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert_eq!(
        harness.state().history_entries().len(),
        history_before + 1,
        "Cut is one content mutation"
    );
    assert!(
        harness.state().session().document().selection().is_none(),
        "Paint.NET clears the selection after Cut"
    );
    assert_eq!(pixel(&harness, 20, 20), Bgra::TRANSPARENT);
    assert_eq!(pixel(&harness, 5, 5), Bgra::WHITE);
    assert_eq!(
        harness.state().clipboard_bounds(),
        Some(rect(10, 10, 31, 31)),
        "Cut copies the selected pixels"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(pixel(&harness, 20, 20), Bgra::BLACK);
    assert_eq!(
        harness.state().session().document().selection(),
        Some(rect(10, 10, 31, 31)),
        "undo restores the pre-cut selection with the erased pixels"
    );
}

#[test]
fn command_v_without_clipboard_content_is_a_no_op() {
    let mut harness = app();
    let history_before = harness.state().history_entries().len();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert!(harness.state().clipboard_bounds().is_none());
    assert!(harness.state().floating_selection_bounds().is_none());
    assert_eq!(harness.state().history_entries().len(), history_before);
    assert!(
        harness.state().status().contains("clipboard"),
        "the status explains that the clipboard has no image"
    );
    assert_eq!(pixel(&harness, 0, 0), Bgra::WHITE);
}

#[test]
fn text_fields_keep_their_own_copy_cut_and_paste_shortcuts() {
    let mut harness = app();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    assert_eq!(
        harness.state().clipboard_bounds(),
        Some(rect(0, 0, 800, 600)),
        "a whole-layer copy fills the clipboard"
    );
    let history_before = harness.state().history_entries().len();

    click(&mut harness, "More");
    harness.get_by_label("Hex color").click();
    harness.run();
    assert!(harness.get_by_label("Hex color").is_focused());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert!(harness.state().floating_selection_bounds().is_none());
    assert!(harness.state().session().document().selection().is_none());
    assert_eq!(
        harness.state().history_entries().len(),
        history_before,
        "Cmd+X in a text field must not cut image pixels"
    );
    assert_eq!(
        harness.state().clipboard_bounds(),
        Some(rect(0, 0, 800, 600)),
        "the image clipboard is untouched while text editing"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert!(
        harness.state().floating_selection_bounds().is_none(),
        "Cmd+V in a text field must not paste image pixels"
    );
    assert_eq!(harness.state().history_entries().len(), history_before);
    assert_eq!(pixel(&harness, 0, 0), Bgra::WHITE);
}
