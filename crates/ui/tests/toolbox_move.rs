//! Paint.NET toolbox order/hotkeys and Move Selected Pixels sampling/Finish parity.

use eframe::egui::{self, Key, Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use paint_core::{Bgra, Point};
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

fn dock_tools(harness: &mut Harness<'_, PaintApp>) {
    harness
        .get_by_role_and_label(Role::Button, "Window")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::CheckBox, "Float Tools")
        .click();
    harness.run();
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

fn button_is_disabled(harness: &Harness<'_, PaintApp>, label: &str) -> bool {
    harness
        .get_by_role_and_label(Role::Button, label)
        .accesskit_node()
        .is_disabled()
}

#[test]
fn toolbox_matches_the_nineteen_tool_paintnet_order() {
    let entries = PaintApp::toolbox_entries();
    assert_eq!(
        entries.iter().map(|entry| entry.label).collect::<Vec<_>>(),
        REFERENCE_TOOLS
    );
    assert_eq!(entries.len(), 19);

    let harness = app();
    for label in REFERENCE_TOOLS {
        let node = harness.get_by_role_and_label(Role::Button, label);
        assert!(
            !node.accesskit_node().is_disabled(),
            "{label} must be an enabled Paint.NET tool"
        );
    }
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Shapes (Freeform)")
            .is_none(),
        "the disabled 20th Freeform cell must be removed"
    );
}

#[test]
fn tooltips_match_reference_hotkeys_and_cycles() {
    let entries = PaintApp::toolbox_entries();
    let shortcut = |label: &str| {
        entries
            .iter()
            .find(|entry| entry.label == label)
            .unwrap_or_else(|| panic!("{label} must be in the toolbox"))
            .shortcut
    };
    let hint = |label: &str| {
        entries
            .iter()
            .find(|entry| entry.label == label)
            .expect("toolbox label")
            .tooltip()
    };

    for (label, key) in [
        ("Rectangle Select", 'S'),
        ("Move Selected Pixels", 'M'),
        ("Lasso Select", 'S'),
        ("Move Selection", 'M'),
        ("Ellipse Select", 'S'),
        ("Zoom", 'Z'),
        ("Magic Wand", 'S'),
        ("Pan", 'H'),
        ("Paint Bucket", 'F'),
        ("Gradient", 'G'),
        ("Paintbrush", 'B'),
        ("Eraser", 'E'),
        ("Pencil", 'P'),
        ("Color Picker", 'K'),
        ("Clone Stamp", 'L'),
        ("Recolor", 'R'),
        ("Text", 'T'),
        ("Line / Curve", 'O'),
        ("Shapes", 'O'),
    ] {
        assert_eq!(shortcut(label), key, "wrong reference hotkey for {label}");
        assert!(
            hint(label).contains(&format!("Shortcut: {key}")),
            "{label} tooltip must show its shortcut: {}",
            hint(label)
        );
    }

    // Cycles are represented honestly: every tool sharing a key is listed.
    let cycle = |key: char| {
        entries
            .iter()
            .filter(|entry| entry.shortcut == key)
            .map(|entry| entry.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        cycle('S'),
        [
            "Rectangle Select",
            "Lasso Select",
            "Ellipse Select",
            "Magic Wand"
        ]
    );
    assert_eq!(cycle('M'), ["Move Selected Pixels", "Move Selection"]);
    assert_eq!(cycle('O'), ["Line / Curve", "Shapes"]);
    for key in ['Z', 'F', 'G', 'B', 'E', 'P', 'K', 'L', 'R', 'T', 'H'] {
        assert_eq!(cycle(key).len(), 1, "hotkey {key} must select one tool");
    }
}

#[test]
fn plain_hotkeys_cycle_and_select_paintnet_tools() {
    let mut harness = app();
    let press = |harness: &mut Harness<'_, PaintApp>, key: Key| {
        harness.key_press(key);
        harness.run();
    };
    let active = |harness: &Harness<'_, PaintApp>| harness.state().active_tool_label();

    press(&mut harness, Key::S);
    assert_eq!(active(&harness), "Lasso Select");
    press(&mut harness, Key::S);
    assert_eq!(active(&harness), "Ellipse Select");
    press(&mut harness, Key::S);
    assert_eq!(active(&harness), "Magic Wand");
    press(&mut harness, Key::S);
    assert_eq!(active(&harness), "Rectangle Select");

    press(&mut harness, Key::M);
    assert_eq!(active(&harness), "Move Selected Pixels");
    press(&mut harness, Key::M);
    assert_eq!(active(&harness), "Move Selection");

    for (key, label) in [
        (Key::Z, "Zoom"),
        (Key::H, "Pan"),
        (Key::F, "Paint Bucket"),
        (Key::G, "Gradient"),
        (Key::B, "Paintbrush"),
        (Key::E, "Eraser"),
        (Key::P, "Pencil"),
        (Key::K, "Color Picker"),
        (Key::L, "Clone Stamp"),
        (Key::R, "Recolor"),
        (Key::T, "Text"),
        (Key::O, "Line / Curve"),
    ] {
        press(&mut harness, key);
        assert_eq!(active(&harness), label);
    }
    press(&mut harness, Key::O);
    assert_eq!(active(&harness), "Shapes");
}

#[test]
fn move_options_expose_reference_sampling_choices_and_default() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Move Selected Pixels");

    harness.get_by_label("Sampling:");
    for label in [
        "Nearest Neighbor",
        "Bilinear",
        "Multisample Bilinear",
        "Anisotropic",
        "Bicubic",
    ] {
        harness.get_by_role_and_label(Role::Button, label);
    }
    assert!(
        button_is_disabled(&harness, "Anisotropic"),
        "Anisotropic must be disabled instead of approximated"
    );
    assert_eq!(harness.state().move_resample_mode_label(), "Bicubic");
    assert!(
        harness.get_by_label("Sampling:").rect().left()
            < harness
                .get_by_role_and_label(Role::Button, "Finish")
                .rect()
                .left(),
        "the sampling section must precede Finish in reference ToolConfigStrip order"
    );

    click(&mut harness, "Bilinear");
    assert_eq!(harness.state().move_resample_mode_label(), "Bilinear");
    click(&mut harness, "Nearest Neighbor");
    assert_eq!(
        harness.state().move_resample_mode_label(),
        "Nearest Neighbor"
    );
}

#[test]
fn enter_finishes_a_previewing_whole_layer_move() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Move Selected Pixels");
    let before = harness.state().history_entries().len();
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    assert_eq!(
        harness.state().history_entries().len(),
        before,
        "the drag must stay an uncommitted preview"
    );

    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().history_entries().len(),
        before + 1,
        "Enter must commit the Move exactly like Finish"
    );
    assert_eq!(
        pixel(&harness, 15, 13),
        Bgra::WHITE,
        "the committed preview must keep the moved pixels"
    );
}

#[test]
fn enter_finishes_a_floating_selection_move() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Rectangle Select");
    drag_canvas(&mut harness, egui::vec2(6.0, 6.0), egui::vec2(26.0, 18.0));
    click(&mut harness, "Move Selected Pixels");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(15.0, 13.0));
    let before = harness.state().history_entries().len();

    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().history_entries().len(),
        before + 1,
        "Enter must place the floating selection move"
    );
    assert!(
        harness.state().floating_selection_bounds().is_none(),
        "the floating selection must be consumed by Finish"
    );
}

#[test]
fn fractional_move_really_resamples_with_the_selected_mode() {
    let mut harness = app();
    dock_tools(&mut harness);
    click(&mut harness, "Shapes");
    click(&mut harness, "Fill");
    drag_canvas(&mut harness, egui::vec2(10.0, 10.0), egui::vec2(30.0, 20.0));
    assert_eq!(pixel(&harness, 30, 15), Bgra::BLACK);

    click(&mut harness, "Move Selected Pixels");
    let rect = canvas_rect(&harness);
    harness.drag_at(rect.min + egui::vec2(100.0, 100.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(100.5, 100.0));
    harness.run();
    harness.drop_at(rect.min + egui::vec2(100.5, 100.0));
    harness.run();

    // dest 31 samples source 30.5: Bilinear halves the black/white edge.
    click(&mut harness, "Bilinear");
    assert_eq!(
        pixel(&harness, 31, 15),
        Bgra::new(128, 128, 128, 255),
        "Bilinear must blend the half-pixel edge"
    );

    // Nearest snaps dest 31 to source 31, which is white.
    click(&mut harness, "Nearest Neighbor");
    assert_eq!(
        pixel(&harness, 31, 15),
        Bgra::WHITE,
        "Nearest Neighbor must not blend the half-pixel edge"
    );

    let before = harness.state().history_entries().len();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().history_entries().len(), before + 1);
    assert_eq!(
        pixel(&harness, 31, 15),
        Bgra::WHITE,
        "the committed preview must keep the selected resampling mode"
    );
}
