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

fn choose_combo(harness: &mut Harness<'_, PaintApp>, combo: &str, option: &str) {
    harness.get_by_role_and_label(Role::ComboBox, combo).click();
    harness.run();
    click(harness, option);
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
    harness.get_by_label("Size");
    click(&mut harness, "Color Picker");
    assert!(
        harness.query_by_label("Size").is_none(),
        "the picker must replace the paintbrush options"
    );
    choose_combo(
        &mut harness,
        "Click behavior",
        "Click behavior: Switch to previous tool",
    );
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));

    // Paintbrush is active again only when its Tool Options row is rendered.
    harness.get_by_label("Size");
    assert_eq!(harness.state().fill_color(), Bgra::WHITE);
}

#[test]
fn color_picker_keeps_picking_when_configured_not_to_switch() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(11.0, 10.0));
    click(&mut harness, "Color Picker");
    choose_combo(
        &mut harness,
        "Click behavior",
        "Click behavior: Do not switch",
    );

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

    choose_combo(&mut harness, "Sampling", "Sampling: Layer");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::TRANSPARENT,
        "the new active layer is transparent; layer sampling must not see the white background"
    );

    choose_combo(&mut harness, "Sampling", "Sampling: Composed image");
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
    choose_combo(&mut harness, "Sample size", "Sample size: 3×3 average");
    // The 3x3 neighbourhood around (10, 10) contains two black pixels and seven white
    // ones: round((7 * 255) / 9) = 198.
    click_canvas_at(&mut harness, egui::vec2(10.0, 10.0));
    assert_eq!(harness.state().fill_color(), Bgra::new(198, 198, 198, 255));

    choose_combo(&mut harness, "Sample size", "Sample size: 5×5 average");
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

    harness.get_by_label("Width");
    for label in ["Start cap", "Dash style", "End cap"] {
        harness.get_by_role_and_label(Role::ComboBox, label);
    }
    harness.get_by_role_and_label(Role::Label, "Line color");
    for (combo, options) in [
        (
            "Start cap",
            ["Start cap: None", "Start cap: Rounded", "Start cap: Arrow"],
        ),
        (
            "Dash style",
            ["Dash style: Solid", "Dash style: Dash", "Dash style: Dot"],
        ),
        (
            "End cap",
            ["End cap: None", "End cap: Rounded", "End cap: Arrow"],
        ),
    ] {
        harness.get_by_role_and_label(Role::ComboBox, combo).click();
        harness.run();
        for option in options {
            harness.get_by_role_and_label(Role::Button, option);
        }
        click(&mut harness, options[0]);
    }

    // Compact single row: every line control shares one 24 px-high strip row.
    let width = harness.get_by_label("Width").rect();
    let start_cap = harness
        .get_by_role_and_label(Role::ComboBox, "Start cap")
        .rect();
    assert!(
        (start_cap.center().y - width.center().y).abs() <= 2.0,
        "line options must share the compact strip row: width={width:?} cap={start_cap:?}"
    );
    let strip = harness.state().tool_options_bounds().expect("strip bounds");
    assert!(strip.height() <= 40.0, "strip is too tall: {strip:?}");
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

    choose_combo(&mut harness, "Start cap", "Start cap: Rounded");
    choose_combo(&mut harness, "End cap", "End cap: Rounded");
    drag_canvas(&mut harness, egui::vec2(20.0, 20.0), egui::vec2(30.0, 20.0));
    assert_eq!(
        pixel(&harness, 19, 20),
        Bgra::BLACK,
        "rounded start cap must paint a disc past the endpoint"
    );

    choose_combo(&mut harness, "Start cap", "Start cap: None");
    choose_combo(&mut harness, "End cap", "End cap: Arrow");
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

    choose_combo(&mut harness, "Dash style", "Dash style: Dash");
    drag_canvas(&mut harness, egui::vec2(20.0, 30.0), egui::vec2(40.0, 30.0));
    // 1 px pen, GDI-style 3:1 dash pattern measured from the first endpoint.
    for x in [20, 21, 22, 24, 25, 26] {
        assert_eq!(pixel(&harness, x, 30), Bgra::BLACK, "dash on at x={x}");
    }
    assert_eq!(pixel(&harness, 23, 30), Bgra::WHITE, "dash gap at x=23");

    choose_combo(&mut harness, "Dash style", "Dash style: Dot");
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

// --- Tool Options parity batch: fill sampling/blend, gradient blend/alpha, picker sizes,
// text alignment, and clone spacing ---

fn pixel_at(harness: &Harness<'_, PaintApp>, x: u32, y: u32) -> Bgra {
    *harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixel(Point::new(x, y))
        .expect("pixel inside the test canvas")
}

#[test]
fn fill_sampling_switches_between_the_layer_and_the_composed_image() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "More");
    // Paint a full-width barrier with a rectangle selection plus a bucket fill, then
    // deselect. Selection gestures use the drag endpoints, so this band has no gaps.
    click(&mut harness, "Rectangle Select");
    drag_canvas(
        &mut harness,
        egui::vec2(0.0, 200.0),
        egui::vec2(799.0, 200.0),
    );
    click(&mut harness, "Paint Bucket");
    click_canvas_at(&mut harness, egui::vec2(400.0, 200.0));
    click(&mut harness, "Edit");
    click(&mut harness, "Deselect");
    assert_eq!(pixel_at(&harness, 400, 200), Bgra::BLACK);

    click(&mut harness, "Add Layer");
    replace_field(&mut harness, "Hex color", "#00FF00");
    click(&mut harness, "Paint Bucket");

    choose_combo(&mut harness, "Sampling", "Sampling: Layer");
    click_canvas_at(&mut harness, egui::vec2(400.0, 100.0));
    assert_eq!(
        pixel_at(&harness, 400, 100),
        Bgra::new(0, 255, 0, 255),
        "layer sampling fills the whole transparent top layer"
    );
    assert_eq!(
        pixel_at(&harness, 400, 400),
        Bgra::new(0, 255, 0, 255),
        "the background barrier is invisible to layer sampling"
    );

    click(&mut harness, "Undo");
    choose_combo(&mut harness, "Sampling", "Sampling: Image");
    click_canvas_at(&mut harness, egui::vec2(400.0, 100.0));
    assert_eq!(
        pixel_at(&harness, 400, 100),
        Bgra::new(0, 255, 0, 255),
        "image sampling fills the white side of the composed barrier"
    );
    assert_eq!(
        pixel_at(&harness, 400, 400),
        Bgra::TRANSPARENT,
        "image sampling must stop at the composed black line"
    );
}

#[test]
fn fill_blend_mode_blends_into_the_active_layer() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "More");
    click(&mut harness, "Paint Bucket");
    replace_field(&mut harness, "Hex color", "#808080");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(pixel_at(&harness, 400, 300), Bgra::new(128, 128, 128, 255));

    replace_field(&mut harness, "Hex color", "#FF0000");
    choose_combo(&mut harness, "Blend mode", "Blend mode: Multiply");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        pixel_at(&harness, 400, 300),
        Bgra::new(0, 0, 128, 255),
        "Multiply blends the red fill into the grey layer"
    );
    assert_eq!(
        harness.state().fill_blend(),
        paint_document::BlendMode::Multiply
    );
}

#[test]
fn gradient_blend_mode_and_transparency_only_affect_committed_pixels() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "More");
    click(&mut harness, "Paint Bucket");
    replace_field(&mut harness, "Hex color", "#808080");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));

    replace_field(&mut harness, "Hex color", "#FF0000");
    click(&mut harness, "Gradient");
    choose_combo(&mut harness, "Blend mode", "Blend mode: Multiply");
    drag_canvas(
        &mut harness,
        egui::vec2(100.0, 300.0),
        egui::vec2(200.0, 300.0),
    );
    let multiplied = pixel_at(&harness, 100, 300);
    assert!(
        (120..=135).contains(&multiplied.r) && multiplied.g <= 2 && multiplied.b <= 2,
        "multiplied red on grey keeps the red channel near 128: {multiplied:?}"
    );

    click(&mut harness, "Undo");
    choose_combo(&mut harness, "Blend mode", "Blend mode: Normal");
    click(&mut harness, "Transparency only");
    assert!(harness.state().gradient_alpha_only());
    drag_canvas(
        &mut harness,
        egui::vec2(100.0, 300.0),
        egui::vec2(200.0, 300.0),
    );
    assert_eq!(
        pixel_at(&harness, 200, 300).a,
        0,
        "the second opaque stop becomes fully transparent in transparency-only mode"
    );
    assert_eq!(
        pixel_at(&harness, 200, 300).r,
        128,
        "transparency-only preserves the grey layer colour"
    );
}

#[test]
fn color_picker_offers_real_11_31_and_51_pixel_averages() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Pencil");
    drag_canvas(
        &mut harness,
        egui::vec2(390.0, 300.0),
        egui::vec2(410.0, 300.0),
    );
    click(&mut harness, "Color Picker");

    choose_combo(&mut harness, "Sample size", "Sample size: 11×11 average");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::new(232, 232, 232, 255),
        "11×11 contains 11 black pixels out of 121"
    );

    choose_combo(&mut harness, "Sample size", "Sample size: 31×31 average");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::new(249, 249, 249, 255),
        "31×31 contains 21 black pixels out of 961"
    );

    choose_combo(&mut harness, "Sample size", "Sample size: 51×51 average");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    assert_eq!(
        harness.state().fill_color(),
        Bgra::new(253, 253, 253, 255),
        "51×51 contains 21 black pixels out of 2601"
    );
}

#[test]
fn text_alignment_and_clone_spacing_options_are_wired_to_real_state() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Text");
    for label in ["Align left", "Align center", "Align right"] {
        harness.get_by_role_and_label(Role::Button, label);
    }
    click(&mut harness, "Align right");
    assert_eq!(
        harness.state().text_alignment(),
        paint_raster::TextAlignment::Right
    );

    click(&mut harness, "Clone Stamp");
    harness.get_by_label("Spacing");
    replace_field(&mut harness, "Spacing", "40");
    assert_eq!(harness.state().clone_spacing(), 40);
}

fn text_ink_bounds(harness: &Harness<'_, PaintApp>) -> (u32, u32) {
    let surface = harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface();
    let mut min_x = u32::MAX;
    let mut max_x = 0;
    for y in 280..380 {
        for x in 300..500 {
            let pixel = *surface.pixel(Point::new(x, y)).expect("text scan pixel");
            if pixel.r < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    assert!(min_x != u32::MAX, "expected committed glyph ink");
    (min_x, max_x)
}

#[test]
fn text_alignment_moves_committed_glyph_ink() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Text");
    replace_field(&mut harness, "Content", "M");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    click(&mut harness, "Place text");
    let (_, left_max) = text_ink_bounds(&harness);

    click(&mut harness, "Undo");
    click(&mut harness, "Align right");
    click_canvas_at(&mut harness, egui::vec2(400.0, 300.0));
    click(&mut harness, "Place text");
    let (_, right_max) = text_ink_bounds(&harness);
    assert!(
        right_max + 20 < left_max,
        "right alignment must pull the glyph end back toward the click point: \
         left={left_max} right={right_max}"
    );
    assert!(
        right_max <= 402,
        "right-aligned ink should end at the click x coordinate: {right_max}"
    );
}

#[test]
fn recolor_sampling_reads_the_layer_or_the_composed_image() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Add Layer");
    click(&mut harness, "More");
    replace_field(&mut harness, "Hex color", "#00FF00");
    click(&mut harness, "Recolor");

    choose_combo(&mut harness, "Sampling", "Sampling: Layer");
    drag_canvas(
        &mut harness,
        egui::vec2(400.0, 300.0),
        egui::vec2(401.0, 300.0),
    );
    assert_eq!(
        pixel_at(&harness, 400, 300),
        Bgra::TRANSPARENT,
        "the transparent active layer has no white target pixels"
    );

    choose_combo(&mut harness, "Sampling", "Sampling: Image");
    drag_canvas(
        &mut harness,
        egui::vec2(400.0, 300.0),
        egui::vec2(401.0, 300.0),
    );
    assert_eq!(
        pixel_at(&harness, 400, 300),
        Bgra::new(0, 255, 0, 255),
        "image sampling matches the composed white background and writes green"
    );
}
