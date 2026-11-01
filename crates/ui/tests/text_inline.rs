//! UI acceptance tests for Paint.NET-style inline text editing on the canvas.

use eframe::egui::{self, Key, Modifiers, Role};
use egui_kittest::{Harness, kittest::Queryable as _};
use paint_core::Bgra;

use paint_ui::PaintApp;

const INLINE_EDITOR: &str = "Inline text editor";

fn app() -> Harness<'static, PaintApp> {
    let mut harness = Harness::new_eframe(|cc| PaintApp::new_docked(cc));
    harness.set_size(egui::Vec2::new(1_400.0, 1_000.0));
    harness.run();
    dock_tools(&mut harness);
    harness
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

fn click(harness: &mut Harness<'_, PaintApp>, label: &str) {
    harness.get_by_role_and_label(Role::Button, label).click();
    harness.run();
}

fn canvas_rect(harness: &Harness<'_, PaintApp>) -> egui::Rect {
    harness
        .get_by_role_and_label(Role::Image, "Composed document canvas")
        .rect()
}

fn click_canvas_at(harness: &mut Harness<'_, PaintApp>, offset: egui::Vec2) {
    let rect = canvas_rect(harness);
    harness.drag_at(rect.min + offset);
    harness.run();
    harness.drop_at(rect.min + offset);
    harness.run();
}

fn inline_editor_exists(harness: &Harness<'_, PaintApp>) -> bool {
    harness
        .query_by_role_and_label(Role::MultilineTextInput, INLINE_EDITOR)
        .is_some()
}

fn type_inline(harness: &mut Harness<'_, PaintApp>, text: &str) {
    harness
        .get_by_role_and_label(Role::MultilineTextInput, INLINE_EDITOR)
        .type_text(text);
    harness.run();
}

fn pixels(harness: &Harness<'_, PaintApp>) -> Vec<Bgra> {
    harness
        .state()
        .session()
        .document()
        .active_layer()
        .surface()
        .pixels()
        .to_vec()
}

fn changed_pixels(before: &[Bgra], after: &[Bgra]) -> usize {
    before
        .iter()
        .zip(after)
        .filter(|(before, after)| before != after)
        .count()
}

fn ink_pixels(pixels: &[Bgra]) -> usize {
    pixels
        .iter()
        .filter(|pixel| pixel.a == 255 && pixel.r < 128 && pixel.g < 128 && pixel.b < 128)
        .count()
}

fn ink_rows(pixels: &[Bgra]) -> (i64, i64) {
    let width = 800;
    let mut min_y = i64::MAX;
    let mut max_y = i64::MIN;
    for (index, pixel) in pixels.iter().enumerate() {
        if pixel.a == 255 && pixel.r < 128 && pixel.g < 128 && pixel.b < 128 {
            let y = (index / width) as i64;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
    }
    (min_y, max_y)
}

#[test]
fn clicking_with_the_text_tool_starts_a_focused_inline_editor() {
    let mut harness = app();
    click(&mut harness, "Text");
    let before = pixels(&harness);

    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));

    assert!(
        harness
            .get_by_role_and_label(Role::MultilineTextInput, INLINE_EDITOR)
            .is_focused(),
        "the canvas click must focus the inline editor so typing needs no toolbar field"
    );

    type_inline(&mut harness, "Ag");

    assert_eq!(harness.state().text_content(), "Ag");
    assert!(
        changed_pixels(&before, &pixels(&harness)) > 0,
        "typing must update the live text preview"
    );
    assert!(
        ink_pixels(&pixels(&harness)) > 0,
        "the inline preview should contain opaque glyph ink"
    );
    assert!(
        !harness.state().session().can_undo(),
        "the live inline preview must not enter undo history"
    );
}

#[test]
fn inline_enter_commits_one_undo_entry_and_preserves_preview_pixels() {
    let mut harness = app();
    let before = pixels(&harness);
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));
    type_inline(&mut harness, "Ag");
    let preview = pixels(&harness);
    assert!(!harness.state().session().can_undo());

    harness.key_press(Key::Enter);
    harness.run();

    assert!(
        !inline_editor_exists(&harness),
        "Enter must close the inline editor"
    );
    assert_eq!(
        harness.state().text_origin(),
        None,
        "committing must clear the text origin"
    );
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        preview.as_slice(),
        "commit must preserve exactly the previewed pixels"
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
    assert!(!harness.state().session().can_undo());
}

#[test]
fn inline_escape_cancels_the_preview_and_undo_history() {
    let mut harness = app();
    let before = pixels(&harness);
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));
    type_inline(&mut harness, "Hi");
    assert!(changed_pixels(&before, &pixels(&harness)) > 0);

    harness.key_press(Key::Escape);
    harness.run();

    assert!(!inline_editor_exists(&harness));
    assert_eq!(harness.state().text_origin(), None);
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        before.as_slice(),
        "Escape must restore the pre-edit pixels"
    );
    assert!(!harness.state().session().can_undo());
    assert!(
        harness.state().status().contains("Canceled"),
        "the status bar should report the cancellation: {}",
        harness.state().status()
    );
}

#[test]
fn shift_enter_adds_a_line_while_plain_enter_commits() {
    let mut harness = app();
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(20.0, 20.0));
    type_inline(&mut harness, "A");

    harness.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    harness.run();
    type_inline(&mut harness, "B");

    assert_eq!(
        harness.state().text_content(),
        "A\nB",
        "Shift+Enter must insert a newline instead of committing"
    );
    let (min_y, max_y) = ink_rows(&pixels(&harness));
    assert!(
        max_y - min_y > 30,
        "the two lines must render on separate rows: min={min_y} max={max_y}"
    );

    harness.key_press(Key::Enter);
    harness.run();

    assert_eq!(
        harness.state().text_content(),
        "A\nB",
        "plain Enter must commit without appending another newline"
    );
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
    assert!(!inline_editor_exists(&harness));
}

#[test]
fn toolbar_style_changes_restyle_the_inline_preview_and_place_commits() {
    let mut harness = app();
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(30.0, 30.0));
    type_inline(&mut harness, "MM");
    let regular_ink = ink_pixels(&pixels(&harness));
    assert!(regular_ink > 0);
    assert!(!harness.state().session().can_undo());

    click(&mut harness, "Bold");
    harness.run();

    let bold_ink = ink_pixels(&pixels(&harness));
    assert!(
        bold_ink > regular_ink,
        "Bold must restyle the inline preview: bold={bold_ink} regular={regular_ink}"
    );
    assert!(
        !harness.state().session().can_undo(),
        "restyling the inline preview must stay undo-free"
    );
    let preview = pixels(&harness);

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
        preview.as_slice()
    );
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
}

#[test]
fn inline_text_preview_keeps_selection_clipping() {
    let mut harness = app();
    click(&mut harness, "Rectangle Select");
    let rect = canvas_rect(&harness);
    harness.drag_at(rect.min + egui::vec2(6.0, 6.0));
    harness.run();
    harness.hover_at(rect.min + egui::vec2(46.0, 60.0));
    harness.run();
    harness.drop_at(rect.min + egui::vec2(46.0, 60.0));
    harness.run();

    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(8.0, 8.0));
    type_inline(&mut harness, "MM");

    let selection = harness
        .state()
        .session()
        .document()
        .selection()
        .expect("selection");
    let preview = pixels(&harness);
    assert!(ink_pixels(&preview) > 0);
    assert_pixels_inside(&preview, selection);
    assert!(!harness.state().session().can_undo());

    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.state().session().can_undo());
    assert_pixels_inside(&pixels(&harness), selection);
}

fn assert_pixels_inside(pixels: &[Bgra], selection: paint_core::Rect) {
    for (index, pixel) in pixels.iter().enumerate() {
        if *pixel != Bgra::WHITE {
            let x = (index % 800) as i64;
            let y = (index / 800) as i64;
            assert!(
                x >= selection.left()
                    && x < selection.right()
                    && y >= selection.top()
                    && y < selection.bottom(),
                "inline text leaked outside the selection at ({x}, {y})"
            );
        }
    }
}

#[test]
fn inline_editing_isolates_tool_hotkeys_and_clipboard_shortcuts() {
    let mut harness = app();
    let history_before = harness.state().history_entries().len();
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));
    type_inline(&mut harness, "Hi");

    harness.key_press(Key::B);
    harness.run();
    assert_eq!(
        harness.state().active_tool_label(),
        "Text",
        "plain tool hotkeys must not fire while the inline editor has focus"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.event(egui::Event::Copy);
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|command| {
            matches!(
                command,
                egui::OutputCommand::CopyText(text) if text == "Hi"
            )
        });
    assert!(
        copied,
        "Cmd+C must copy the inline text selection, not the image"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert!(
        harness.state().clipboard_bounds().is_none(),
        "Cmd+X while editing text must not cut canvas pixels"
    );
    assert!(
        harness.state().session().document().selection().is_none(),
        "Cmd+X while editing text must not create a canvas selection"
    );
    assert_eq!(
        harness.state().history_entries().len(),
        history_before,
        "typing and unhandled image shortcuts must not add history"
    );

    // Collapse the Cmd+A selection so the paste inserts at the end of the text.
    harness.key_press(Key::ArrowRight);
    harness.run();
    harness.event(egui::Event::Paste(" there".to_owned()));
    harness.run();
    assert_eq!(
        harness.state().text_content(),
        "Hi there",
        "clipboard paste must insert into the inline text, not the image"
    );

    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
}

#[test]
fn switching_tools_commits_the_inline_preview_once() {
    let mut harness = app();
    click(&mut harness, "Text");
    click_canvas_at(&mut harness, egui::vec2(40.0, 40.0));
    type_inline(&mut harness, "Hi");
    let preview = pixels(&harness);

    click(&mut harness, "Paintbrush");
    assert!(!inline_editor_exists(&harness));
    assert!(harness.state().session().can_undo());
    assert_eq!(
        harness.state().history_entries().last().map(String::as_str),
        Some("Text")
    );
    assert_eq!(
        harness
            .state()
            .session()
            .document()
            .active_layer()
            .surface()
            .pixels(),
        preview.as_slice()
    );
    assert_eq!(harness.state().active_tool_label(), "Paintbrush");
}
