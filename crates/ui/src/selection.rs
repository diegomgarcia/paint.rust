use egui::{Color32, Painter, Rect, Shape, Stroke};

/// Draws a high-contrast animated dashed selection border ("marching ants").
///
/// Rectangle selections draw their exact outline. Ellipse, lasso, and magic-wand selections draw
/// their tight bounding rectangle; the document's [`paint_document::SelectionMask`] remains the
/// authoritative shape for every raster operation, and tool options report the real selected-pixel
/// count. Drawing a per-pixel mask contour here would require a marching-squares pass on every
/// repaint, which the current preview intentionally avoids.
pub(crate) fn paint_selection_outline(painter: &Painter, rect: Rect, phase: f32) {
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
    use super::paint_selection_outline;

    fn painted_shapes(phase: f32) -> Vec<String> {
        let context = egui::Context::default();
        let output = context.run_ui(Default::default(), |ui| {
            paint_selection_outline(
                ui.painter(),
                egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(110.0, 70.0)),
                phase,
            );
        });
        let mut output = output;
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .map(|shape| format!("{shape:?}"))
            .collect()
    }

    #[test]
    fn selection_outline_is_drawn_and_animated() {
        let first = painted_shapes(0.0);
        let second = painted_shapes(0.35);
        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert_ne!(first, second, "selection dashes did not advance");
    }
}
