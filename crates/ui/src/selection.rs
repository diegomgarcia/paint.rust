use egui::{Color32, Painter, Rect, Shape, Stroke};
use paint_document::Selection;

/// Paint.NET-style selection tint. Kept translucent so the underlying pixels remain readable.
fn selection_tint() -> Color32 {
    Color32::from_rgba_unmultiplied(34, 112, 200, 70)
}

/// Draws a rectangular selection: exact tint plus animated dashes.
pub(crate) fn paint_selection_outline(painter: &Painter, rect: Rect, phase: f32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    painter.rect_filled(rect, 0, selection_tint());
    paint_rect_dashes(painter, rect, phase);
}

/// Draws a document selection in canvas coordinates, following the real mask boundary.
///
/// Rectangle selections use their exact rectangle. Mask-backed selections (ellipse, lasso,
/// magic wand, combined selections) are tinted per row run and outlined along every selected
/// pixel edge that touches an unselected neighbor, so the moving dashes trace the drawn shape
/// instead of its bounding box.
pub(crate) fn paint_selection(
    painter: &Painter,
    canvas: Rect,
    zoom: f32,
    selection: &Selection,
    phase: f32,
) {
    if zoom <= 0.0 {
        return;
    }
    let painter = painter.with_clip_rect(canvas);
    let map = |x: f32, y: f32| canvas.min + egui::vec2(x * zoom, y * zoom);
    let Some(mask) = selection.mask() else {
        let bounds = selection.bounds();
        let min = map(bounds.left() as f32, bounds.top() as f32);
        let max = map(bounds.right() as f32, bounds.bottom() as f32);
        painter.rect_filled(Rect::from_min_max(min, max), 0, selection_tint());
        paint_rect_dashes(&painter, Rect::from_min_max(min, max), phase);
        return;
    };

    let bounds = mask.bounds();
    // Tint one rectangle per contiguous selected run in each row. A lasso is typically a
    // handful of runs per scanline, so this stays cheap without uploading a mask texture.
    for y in bounds.top()..bounds.bottom() {
        let mut x = bounds.left();
        while x < bounds.right() {
            while x < bounds.right() && !mask.contains_xy(x, y) {
                x += 1;
            }
            let start = x;
            while x < bounds.right() && mask.contains_xy(x, y) {
                x += 1;
            }
            if x > start {
                let min = map(start as f32, y as f32);
                let max = map(x as f32, (y + 1) as f32);
                painter.rect_filled(Rect::from_min_max(min, max), 0, selection_tint());
            }
        }
    }

    let dashed = |from: egui::Pos2, to: egui::Pos2| {
        let edge = [from, to];
        let offset = (phase * 12.0) % 8.0;
        let black = Stroke::new(1.5, Color32::BLACK);
        let white = Stroke::new(1.5, Color32::WHITE);
        painter.extend(Shape::dashed_line_with_offset(
            &edge,
            black,
            &[4.0],
            &[4.0],
            offset,
        ));
        painter.extend(Shape::dashed_line_with_offset(
            &edge,
            white,
            &[4.0],
            &[4.0],
            offset + 4.0,
        ));
    };

    // Horizontal edges: group adjacent selected pixels whose neighbor above/below is outside.
    for y in bounds.top()..bounds.bottom() {
        for (neighbor_y, at_bottom) in [(y - 1, false), (y + 1, true)] {
            let mut x = bounds.left();
            while x < bounds.right() {
                let edge = mask.contains_xy(x, y) && !mask.contains_xy(x, neighbor_y);
                if !edge {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < bounds.right()
                    && mask.contains_xy(x, y)
                    && !mask.contains_xy(x, neighbor_y)
                {
                    x += 1;
                }
                let screen_y = if at_bottom { y + 1 } else { y };
                dashed(
                    map(start as f32, screen_y as f32),
                    map(x as f32, screen_y as f32),
                );
            }
        }
    }

    // Vertical edges: the same grouping along columns.
    for x in bounds.left()..bounds.right() {
        for (neighbor_x, at_right) in [(x - 1, false), (x + 1, true)] {
            let mut y = bounds.top();
            while y < bounds.bottom() {
                let edge = mask.contains_xy(x, y) && !mask.contains_xy(neighbor_x, y);
                if !edge {
                    y += 1;
                    continue;
                }
                let start = y;
                while y < bounds.bottom()
                    && mask.contains_xy(x, y)
                    && !mask.contains_xy(neighbor_x, y)
                {
                    y += 1;
                }
                let screen_x = if at_right { x + 1 } else { x };
                dashed(
                    map(screen_x as f32, start as f32),
                    map(screen_x as f32, y as f32),
                );
            }
        }
    }
}

fn paint_rect_dashes(painter: &Painter, rect: Rect, phase: f32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let rect = rect.shrink(0.5);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let offset = (phase * 12.0) % 8.0;
    let black = Stroke::new(1.5, Color32::BLACK);
    let white = Stroke::new(1.5, Color32::WHITE);
    for index in 0..4 {
        let edge = [corners[index], corners[(index + 1) % 4]];
        painter.extend(Shape::dashed_line_with_offset(
            &edge,
            black,
            &[4.0],
            &[4.0],
            offset,
        ));
        painter.extend(Shape::dashed_line_with_offset(
            &edge,
            white,
            &[4.0],
            &[4.0],
            offset + 4.0,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{paint_selection, paint_selection_outline};
    use paint_document::{Selection, SelectionMask};

    fn lasso_selection() -> Selection {
        // A triangle: three rows wide at the bottom, one pixel at the top.
        let mask = SelectionMask::polygon(&[
            paint_core::Point::new(0, 0),
            paint_core::Point::new(8, 8),
            paint_core::Point::new(0, 8),
        ])
        .expect("mask")
        .expect("selection");
        Selection::from_mask(mask)
    }

    fn paint_mask(selection: &Selection, phase: f32) -> Vec<String> {
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| {
            paint_selection(
                ui.painter(),
                egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(210.0, 170.0)),
                10.0,
                selection,
                phase,
            );
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .map(|shape| format!("{shape:?}"))
            .collect()
    }

    #[test]
    fn mask_selection_tints_runs_and_traces_more_than_a_bounding_box() {
        let shapes = paint_mask(&lasso_selection(), 0.0);
        assert!(
            shapes.len() > 12,
            "a triangular mask needs many boundary/tint shapes, got {}",
            shapes.len()
        );
    }

    #[test]
    fn mask_outline_animates() {
        let first = paint_mask(&lasso_selection(), 0.0);
        let second = paint_mask(&lasso_selection(), 0.35);
        assert_ne!(first, second, "selection dashes did not advance");
    }

    #[test]
    fn rectangle_selection_keeps_exact_outline() {
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| {
            paint_selection_outline(
                ui.painter(),
                egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(110.0, 70.0)),
                0.0,
            );
        });
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
    }
}
