//! Compact Paint.NET-style ToolConfigStrip parity tests.

use eframe::egui::{self, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_ui::PaintApp;

const REFERENCE_TOOLS: [&str; 19] = [
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
];

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

fn open_combo(harness: &mut Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::ComboBox, label).click();
    harness.run();
}

fn choose_combo(harness: &mut Harness<'_, PaintApp>, combo: &str, option: &str) {
    open_combo(harness, combo);
    click(harness, option);
}

fn select_tool(harness: &mut Harness<'_, PaintApp>, label: &str) {
    click(harness, label);
    assert_eq!(harness.state().active_tool_label(), label);
}

fn strip_height(harness: &Harness<'_, PaintApp>) -> f32 {
    harness
        .state()
        .tool_options_bounds()
        .expect("every tool must render a ToolConfigStrip")
        .height()
}

fn assert_button(harness: &Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label);
}

fn assert_combo(harness: &Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::ComboBox, label);
}

fn assert_label(harness: &Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Label, label);
}

fn assert_field(harness: &Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::SpinButton, label);
}

#[test]
fn every_tool_uses_one_compact_strip_row() {
    let mut harness = app();
    for tool in REFERENCE_TOOLS {
        select_tool(&mut harness, tool);
        let bounds = harness
            .state()
            .tool_options_bounds()
            .expect("tool strip bounds");
        assert!(
            (24.0..=40.0).contains(&bounds.height()),
            "{tool} must use one 24 px ToolConfigStrip row, got {bounds:?}"
        );
        assert!(bounds.width() > 1.0, "{tool} strip is empty: {bounds:?}");
    }
}

#[test]
fn strip_text_stays_short_and_free_of_instructions() {
    let mut harness = app();
    for tool in REFERENCE_TOOLS {
        select_tool(&mut harness, tool);
        for forbidden in [
            "Alt-click to set source",
            "Alt-click the canvas",
            "Selection Mode",
            "Selection left",
            "Solid Color",
            "Type text",
            "Anisotropic sampling only differs",
            "Flood Mode",
            "Blend Mode",
            "Sampling:",
        ] {
            assert!(
                harness.query_by_label_contains(forbidden).is_none(),
                "{tool} strip must not contain instructional or long label {forbidden:?}"
            );
        }
    }
}

#[test]
fn each_tool_exposes_its_compact_accessible_controls() {
    let mut harness = app();

    for tool in [
        "Rectangle Select",
        "Lasso Select",
        "Ellipse Select",
        "Magic Wand",
    ] {
        select_tool(&mut harness, tool);
        for mode in ["Replace", "Union", "Subtract", "Intersect", "Xor"] {
            assert_button(&harness, mode);
        }
        assert_button(&harness, "Selection fields");
    }
    select_tool(&mut harness, "Magic Wand");
    assert_combo(&harness, "Flood mode");
    assert_combo(&harness, "Sampling");
    assert_field(&harness, "Tolerance");

    select_tool(&mut harness, "Move Selected Pixels");
    assert_combo(&harness, "Sampling");
    assert_button(&harness, "Finish");

    select_tool(&mut harness, "Move Selection");
    assert_field(&harness, "Offset X");
    assert_field(&harness, "Offset Y");
    assert_button(&harness, "Apply offset");
    assert_button(&harness, "Cancel offset");

    select_tool(&mut harness, "Zoom");
    assert_combo(&harness, "Zoom");
    assert_button(&harness, "Fit canvas");

    select_tool(&mut harness, "Pan");
    assert_eq!(strip_height(&harness), 24.0);

    select_tool(&mut harness, "Color Picker");
    for combo in ["Sampling", "Sample size", "Click behavior"] {
        assert_combo(&harness, combo);
    }

    select_tool(&mut harness, "Paint Bucket");
    assert_combo(&harness, "Flood mode");
    assert_field(&harness, "Tolerance");
    assert_combo(&harness, "Sampling");
    assert_combo(&harness, "Blend mode");
    assert_label(&harness, "Fill color");

    select_tool(&mut harness, "Gradient");
    assert_combo(&harness, "Gradient type");
    assert_label(&harness, "Gradient start color");
    assert_label(&harness, "Gradient end color");
    assert_combo(&harness, "Blend mode");
    assert_combo(&harness, "Repeat mode");
    assert_button(&harness, "Transparency only");

    for tool in ["Paintbrush", "Eraser"] {
        select_tool(&mut harness, tool);
        for field in ["Size", "Hardness", "Spacing"] {
            assert_field(&harness, field);
        }
    }
    select_tool(&mut harness, "Paintbrush");
    assert_label(&harness, "Brush color");
    select_tool(&mut harness, "Pencil");
    assert_label(&harness, "1 px");
    assert_label(&harness, "Pencil color");

    select_tool(&mut harness, "Clone Stamp");
    for field in ["Size", "Hardness", "Spacing"] {
        assert_field(&harness, field);
    }
    assert_label(&harness, "No source");
    assert_button(&harness, "Clear clone source");

    select_tool(&mut harness, "Recolor");
    for field in ["Size", "Hardness", "Spacing", "Tolerance"] {
        assert_field(&harness, field);
    }
    assert_combo(&harness, "Sampling");
    assert_label(&harness, "Recolor target color");
    assert_label(&harness, "Recolor replacement color");

    select_tool(&mut harness, "Text");
    assert_combo(&harness, "Font family");
    assert_field(&harness, "Size");
    for button in [
        "Bold",
        "Italic",
        "Align left",
        "Align center",
        "Align right",
        "Antialiasing",
        "Place text",
        "Cancel text",
    ] {
        assert_button(&harness, button);
    }
    assert_combo(&harness, "Blend mode");
    assert_label(&harness, "Text color");
    assert_label(&harness, "Content");

    select_tool(&mut harness, "Line / Curve");
    assert_field(&harness, "Width");
    for combo in ["Start cap", "Dash style", "End cap"] {
        assert_combo(&harness, combo);
    }
    assert_label(&harness, "Line color");

    select_tool(&mut harness, "Shapes");
    assert_combo(&harness, "Shape type");
    assert_combo(&harness, "Fill style");
    assert_field(&harness, "Border");
    assert_label(&harness, "Shape color");
}

#[test]
fn compact_combos_keep_visible_values_and_change_real_options() {
    let mut harness = app();

    select_tool(&mut harness, "Move Selected Pixels");
    choose_combo(&mut harness, "Sampling", "Bilinear");
    assert_eq!(harness.state().move_resample_mode_label(), "Bilinear");
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Sampling")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Bilinear")
    );

    select_tool(&mut harness, "Color Picker");
    choose_combo(
        &mut harness,
        "Click behavior",
        "Click behavior: Switch to previous tool",
    );
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Click behavior")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Previous tool")
    );
    choose_combo(&mut harness, "Sample size", "Sample size: 3×3 average");
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Sample size")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("3×3")
    );
    choose_combo(&mut harness, "Sampling", "Sampling: Layer");
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Sampling")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Layer")
    );

    select_tool(&mut harness, "Gradient");
    choose_combo(&mut harness, "Gradient type", "Gradient type: Radial");
    assert_eq!(
        harness.state().gradient_type(),
        paint_raster::GradientType::Radial
    );
    choose_combo(&mut harness, "Repeat mode", "Repeat mode: Repeat");
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Repeat mode")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Repeat")
    );

    select_tool(&mut harness, "Shapes");
    choose_combo(&mut harness, "Shape type", "Shape type: Ellipse");
    choose_combo(&mut harness, "Fill style", "Fill style: Both");
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Shape type")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Ellipse")
    );
    assert_eq!(
        harness
            .get_by_role_and_label(Role::ComboBox, "Fill style")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("Both")
    );
}

#[test]
fn selection_fields_open_inside_the_same_compact_row() {
    let mut harness = app();
    select_tool(&mut harness, "Rectangle Select");
    click(&mut harness, "Selection fields");
    for field in ["Left", "Top", "Right", "Bottom"] {
        assert_field(&harness, field);
    }
    assert_button(&harness, "Apply selection");
    assert!(
        strip_height(&harness) <= 40.0,
        "opening selection fields must not add another row"
    );
}

#[test]
fn text_strip_keeps_compact_content_and_icon_actions() {
    let mut harness = app();
    select_tool(&mut harness, "Text");
    assert!(strip_height(&harness) <= 40.0);
    assert_label(&harness, "Content");
    assert_button(&harness, "Place text");
    assert_button(&harness, "Cancel text");
    let buttons = [
        "Bold",
        "Italic",
        "Align left",
        "Align center",
        "Align right",
        "Antialiasing",
    ];
    for label in buttons {
        let rect = harness.get_by_role_and_label(Role::Button, label).rect();
        assert!(
            rect.width() <= 34.0,
            "{label} must be a compact icon-style toggle: {rect:?}"
        );
        assert!(
            rect.height() <= 28.0,
            "{label} must stay on the compact row: {rect:?}"
        );
    }
}
