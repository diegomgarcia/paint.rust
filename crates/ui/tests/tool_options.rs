//! UI acceptance tests for the Move, Move Selection, Color Picker, and Line tool options.

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
    // The floating History window renders above docked panels; hide it so the taller
    // grouped Tool Options rows stay clickable at the tests' 1000 px window height.
    harness
        .get_by_role_and_label(Role::Button, "Window")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "History window")
        .click();
    harness.run();
}

fn canvas_rect(harness: &Harness<'_, PaintApp>) -> egui::Rect {
    harness
        .get_by_role_and_label(Role::Image, "Composed document canvas")
        .rect()
}

fn drag_canvas(harness: &mut Harness<'_, PaintApp>, start: egui::Vec2, end: egui::Vec2) {
    let rect = canvas_rect(harness);
    harness.drag_at(rect.min + start);
    harness.run();
    harness.hover_at(rect.min + end);
    harness.run();
    harness.drop_at(rect.min + end);
    harness.run();
}

fn click_canvas_at(harness: &mut Harness<'_, PaintApp>, offset: egui::Vec2) {
    let rect = canvas_rect(harness);
    harness.drag_at(rect.min + offset);
    harness.run();
    harness.drop_at(rect.min + offset);
    harness.run();
}

fn button_is_disabled(harness: &Harness<'_, PaintApp>, label: &str) -> bool {
    harness
        .get_by_role_and_label(Role::Button, label)
        .accesskit_node()
        .is_disabled()
}

fn pixel(harness: &Harness<'_, PaintApp>, x: u32, y: u32) -> Bgra {
    *harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("pixel inside the test canvas")
}

fn selection(harness: &Harness<'_, PaintApp>) -> Rect {
    harness
        .state()
        .session()
        .document()
        .selection()
        .expect("a selection is active")
}

#[test]
fn color_picker_switches_to_the_previous_tool_when_configured() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Paintbrush");
    harness.get_by_label("Width");
    click(&mut harness, "Color Picker");
    assert!(
        harness.query_by_label("Width").is_none(),
        "the picker must replace the paintbrush options"
    );
    click(&mut harness, "Switch to previous tool");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));

    // Paintbrush is active again only when its Tool Options row is rendered.
    harness.get_by_label("Width");
    assert_eq!(harness.state().fill_color(), Bgra::WHITE);
}

#[test]
fn color_picker_keeps_picking_when_configured_not_to_switch() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(11.0, 10.0));
    click(&mut harness, "Color Picker");
    click(&mut harness, "Do not switch");

    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    assert_eq!(harness.state().fill_color(), Bgra::BLACK);
    // A second click must still sample instead of leaving a dead picker mode.
    click_canvas_at(&mut harness, egui::vec2(100.0, 100.0));
    assert_eq!(harness.state().fill_color(), Bgra::WHITE);
}

#[test]
fn color_picker_samples_the_active_layer_or_the_composed_image() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Add Layer");
    click(&mut harness, "Color Picker");

    click(&mut harness, "Layer");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::TRANSPARENT,
        "the new active layer is transparent; layer sampling must not see the white background"
    );

    click(&mut harness, "Composed image");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::WHITE,
        "composed sampling must include the white background layer"
    );
}

#[test]
fn color_picker_average_sample_uses_neighbouring_pixels() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(11.0, 10.0));
    assert_eq!(pixel(&harness, 10, 10), Bgra::BLACK);
    assert_eq!(pixel(&harness, 11, 10), Bgra::BLACK);

    click(&mut harness, "Color Picker");
    click(&mut harness, "3×3 average");
    // The 3x3 neighbourhood around (10, 10) contains two black pixels and seven white
    // ones: round((7 * 255) / 9) = 198.
    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    assert_eq!(harness.state().fill_color(), Bgra::new(198, 198, 198, 255));

    click(&mut harness, "5×5 average");
    // Five by five contains the same two black pixels and twenty-three white ones:
    // round((23 * 255) / 25) = 235.
    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    assert_eq!(harness.state().fill_color(), Bgra::new(235, 235, 235, 255));
}

#[test]
fn line_tool_options_expose_grouped_caps_dash_and_width() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Line / Curve");

    for label in ["Width", "Start cap", "Dash style", "End cap"] {
        harness.get_by_label(label);
    }
    harness.get_by_role_and_label(Role::Label, "Primary color");
    for label in [
        "Start cap: None",
        "Start cap: Rounded",
        "Start cap: Arrow",
        "Dash style: Solid",
        "Dash style: Dash",
        "Dash style: Dot",
        "End cap: None",
        "End cap: Rounded",
        "End cap: Arrow",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }

    // Grouped rows: the cap controls render below the width/color row.
    let width = harness.get_by_label("Width").rect();
    let start_cap = harness.get_by_label("Start cap").rect();
    assert!(
        start_cap.top() > width.top() + 4.0,
        "line options should group caps on their own row: width={width:?} cap={start_cap:?}"
    );
}

#[test]
fn line_rounded_and_arrow_caps_paint_real_pixels() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Line / Curve");
    replace_field(&mut harness, "Width", "5");

    drag_canvas(&mut harness, egui::vec2(20.0, 40.0), egui::vec2(30.0, 40.0));
    assert_eq!(
        pixel(&harness, 19, 40),
        Bgra::WHITE,
        "flat start cap must not extend past the endpoint"
    );

    click(&mut harness, "Start cap: Rounded");
    click(&mut harness, "End cap: Rounded");
    drag_canvas(&mut harness, egui::vec2(20.0, 20.0), egui::vec2(30.0, 20.0));
    assert_eq!(
        pixel(&harness, 19, 20),
        Bgra::BLACK,
        "rounded start cap must paint a disc past the endpoint"
    );

    click(&mut harness, "Start cap: None");
    click(&mut harness, "End cap: Arrow");
    drag_canvas(&mut harness, egui::vec2(20.0, 60.0), egui::vec2(30.0, 60.0));
    assert_eq!(
        pixel(&harness, 26, 63),
        Bgra::BLACK,
        "arrow end cap must paint a chevron away from the shaft"
    );

    click(&mut harness, "Undo");
    click(&mut harness, "Undo");
    click(&mut harness, "Undo");
    assert_eq!(pixel(&harness, 19, 20), Bgra::WHITE);
    assert_eq!(pixel(&harness, 19, 40), Bgra::WHITE);
    assert_eq!(pixel(&harness, 26, 63), Bgra::WHITE);
}

#[test]
fn line_dash_and_dot_styles_leave_real_gaps() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Line / Curve");
    replace_field(&mut harness, "Width", "1");

    drag_canvas(&mut harness, egui::vec2(20.0, 20.0), egui::vec2(40.0, 20.0));
    for x in [20, 21, 22, 23, 24, 25, 26, 39] {
        assert_eq!(
            pixel(&harness, x, 20),
            Bgra::BLACK,
            "solid line must paint every x at {x}"
        );
    }

    click(&mut harness, "Dash style: Dash");
    drag_canvas(&mut harness, egui::vec2(20.0, 30.0), egui::vec2(40.0, 30.0));
    // 1 px pen, GDI-style 3:1 dash pattern measured from the first endpoint.
    for x in [20, 21, 22, 24, 25, 26] {
        assert_eq!(pixel(&harness, x, 30), Bgra::BLACK, "dash on at x={x}");
    }
    assert_eq!(pixel(&harness, 23, 30), Bgra::WHITE, "dash gap at x=23");

    click(&mut harness, "Dash style: Dot");
    drag_canvas(&mut harness, egui::vec2(20.0, 40.0), egui::vec2(40.0, 40.0));
    for x in [20, 22, 24] {
        assert_eq!(pixel(&harness, x, 40), Bgra::BLACK, "dot on at x={x}");
    }
    for x in [21, 23, 25] {
        assert_eq!(pixel(&harness, x, 40), Bgra::WHITE, "dot gap at x={x}");
    }
}

#[test]
fn line_stroke_is_clipped_to_the_selection_and_undoable() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(20.0, 20.0));
    click(&mut harness, "Line / Curve");
    replace_field(&mut harness, "Width", "1");
    drag_canvas(&mut harness, egui::vec2(0.0, 15.0), egui::vec2(40.0, 15.0));

    assert_eq!(pixel(&harness, 15, 15), Bgra::BLACK, "inside the selection");
    assert_eq!(
        pixel(&harness, 5, 15),
        Bgra::WHITE,
        "outside the selection must stay untouched"
    );
    assert_eq!(
        pixel(&harness, 35, 15),
        Bgra::WHITE,
        "outside the selection must stay untouched"
    );
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Line / Curve")
    );

    click(&mut harness, "Undo");
    assert_eq!(pixel(&harness, 15, 15), Bgra::WHITE);
}

#[test]
fn move_finish_commits_the_floating_selection() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    click(&mut harness, "Move Selected Pixels");
    assert!(button_is_disabled(&harness, "Finish"));

    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    assert!(
        !button_is_disabled(&harness, "Finish"),
        "Finish is enabled while pixels are floating"
    );
    assert_eq!(
        pixel(&harness, 12, 10),
        Bgra::TRANSPARENT,
        "the lifting step clears the source area until Finish"
    );

    click(&mut harness, "Finish");
    assert_eq!(
        pixel(&harness, 12, 10),
        Bgra::BLACK,
        "Finish stamps the floating pixels at the moved destination"
    );
    assert!(button_is_disabled(&harness, "Finish"));
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Moved selected pixels")
    );
}

#[test]
fn move_selection_numeric_offsets_apply_and_cancel() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    let before = selection(&harness);
    click(&mut harness, "Move Selection");

    replace_field(&mut harness, "Offset X", "5");
    replace_field(&mut harness, "Offset Y", "3");
    click(&mut harness, "Apply offset");
    let after = selection(&harness);
    assert_eq!(after.left(), before.left() + 5);
    assert_eq!(after.top(), before.top() + 3);
    assert_eq!(after.right(), before.right() + 5);
    assert_eq!(after.bottom(), before.bottom() + 3);

    replace_field(&mut harness, "Offset X", "9");
    replace_field(&mut harness, "Offset Y", "9");
    click(&mut harness, "Cancel offset");
    assert_eq!(
        selection(&harness),
        after,
        "Cancel discards the unapplied numeric offset"
    );

    // After Cancel the pending offset is zero, so a second Apply is a no-op.
    click(&mut harness, "Apply offset");
    assert_eq!(selection(&harness), after);
}
