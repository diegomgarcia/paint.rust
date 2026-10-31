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
    // One run per contiguous selected span, not one probe per pixel.
    for y in bounds.top()..bounds.bottom() {
        for (start, end) in mask.row_runs(y) {
            let min = map(start as f32, y as f32);
            let max = map(end as f32, (y + 1) as f32);
            painter.rect_filled(Rect::from_min_max(min, max), 0, selection_tint());
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

    // Horizontal boundary segments are the parts of a row run not covered by the row above/below.
    for y in bounds.top()..bounds.bottom() {
        let runs = mask.row_runs(y);
        for (neighbor_y, edge_y) in [(y - 1, y), (y + 1, y + 1)] {
            let neighbor = mask.row_runs(neighbor_y);
            for (start, end) in subtract_runs(&runs, &neighbor) {
                dashed(
                    map(start as f32, edge_y as f32),
                    map(end as f32, edge_y as f32),
                );
            }
        }
    }

    // Collect one-pixel-high vertical edges by column, then merge consecutive rows into spans.
    let mut columns: std::collections::BTreeMap<i64, Vec<i64>> = std::collections::BTreeMap::new();
    for y in bounds.top()..bounds.bottom() {
        for (start, end) in mask.row_runs(y) {
            if !mask.contains_xy(start - 1, y) {
                columns.entry(start).or_default().push(y);
            }
            if !mask.contains_xy(end, y) {
                columns.entry(end).or_default().push(y);
            }
        }
    }
    for (x, ys) in columns {
        let mut index = 0;
        while index < ys.len() {
            let start = ys[index];
            let mut end = start;
            while index + 1 < ys.len() && ys[index + 1] == end + 1 {
                index += 1;
                end = ys[index];
            }
            dashed(map(x as f32, start as f32), map(x as f32, (end + 1) as f32));
            index += 1;
        }
    }
}

/// Parts of `base` not covered by `remove`; both inputs are sorted, non-overlapping spans.
fn subtract_runs(base: &[(i64, i64)], remove: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut result = Vec::new();
    for &(mut start, end) in base {
        for &(remove_start, remove_end) in remove {
            if remove_end <= start || remove_start >= end {
                continue;
            }
            if remove_start > start {
                result.push((start, remove_start.min(end)));
            }
            start = start.max(remove_end);
            if start >= end {
                break;
            }
        }
        if start < end {
            result.push((start, end));
        }
    }
    result
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
