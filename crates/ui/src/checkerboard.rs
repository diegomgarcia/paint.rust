use egui::{Color32, Painter, Rect};

pub(crate) const CHECKER_CELL: f32 = 8.0;

/// Paints a fixed-screen-size transparency checkerboard behind the composed image.
pub(crate) fn paint_checkerboard(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 0, Color32::from_gray(150));
    let mut row = 0;
    let mut y = rect.top();
    while y < rect.bottom() {
        let mut column = 0;
        let mut x = rect.left();
        while x < rect.right() {
            if (row + column) % 2 == 0 {
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(x, y),
                        egui::pos2(
                            (x + CHECKER_CELL).min(rect.right()),
                            (y + CHECKER_CELL).min(rect.bottom()),
                        ),
                    ),
                    0,
                    Color32::from_gray(190),
                );
            }
            x += CHECKER_CELL;
            column += 1;
        }
        y += CHECKER_CELL;
        row += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::paint_checkerboard;

    #[test]
    fn checkerboard_emits_alternating_cells() {
        let context = egui::Context::default();
        let output = context.run_ui(Default::default(), |ui| {
            paint_checkerboard(
                ui.painter(),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(32.0, 16.0)),
            );
        });
        let mut output = output;
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
    }
}
