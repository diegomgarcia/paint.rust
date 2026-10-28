use egui::{
    Color32, Painter, Pos2, Response, Role, Sense, Shape, Stroke, StrokeKind, Ui, WidgetInfo, vec2,
};

/// Project-owned line icons. They are original geometry, not copied Paint.NET assets.
#[derive(Clone, Copy)]
pub(crate) enum Icon {
    NewDocument,
    OpenFile,
    ExportFile,
    Undo,
    Redo,
    SelectRectangle,
    Zoom,
    Fill,
    RectangleShape,
    EyeDropper,
    AddLayer,
}

pub(crate) fn icon_button(ui: &mut Ui, icon: Icon, label: &'static str, enabled: bool) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(38.0, 32.0), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, rect, icon, enabled, false, &response, 6.0);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
}

pub(crate) fn tool_button(
    ui: &mut Ui,
    icon: Icon,
    label: &'static str,
    enabled: bool,
    selected: bool,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(46.0, 42.0), Sense::click());
            response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, rect, icon, enabled, selected, &response, 7.0);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
}

fn paint_button(
    ui: &Ui,
    rect: egui::Rect,
    icon: Icon,
    enabled: bool,
    selected: bool,
    response: &Response,
    icon_inset: f32,
) {
    let widgets = &ui.style().visuals.widgets;
    let (fill, stroke) = if !enabled {
        (Color32::TRANSPARENT, Stroke::NONE)
    } else if response.is_pointer_button_down_on() {
        (widgets.active.bg_fill, widgets.active.bg_stroke)
    } else if selected {
        (
            Color32::from_rgb(31, 78, 116),
            Stroke::new(1.5, Color32::from_rgb(100, 178, 235)),
        )
    } else if response.hovered() {
        (widgets.hovered.bg_fill, widgets.hovered.bg_stroke)
    } else {
        (Color32::TRANSPARENT, Stroke::NONE)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 5, fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, 5, stroke, StrokeKind::Inside);
    }
    let color = if enabled {
        Color32::from_gray(235)
    } else {
        ui.visuals().weak_text_color()
    };
    paint_icon(painter, rect.shrink(icon_inset), icon, color);
}

fn point(rect: egui::Rect, x: f32, y: f32) -> Pos2 {
    rect.min + vec2(rect.width() * x, rect.height() * y)
}

fn polyline(painter: &Painter, rect: egui::Rect, points: &[(f32, f32)], stroke: Stroke) {
    painter.line(
        points.iter().map(|&(x, y)| point(rect, x, y)).collect(),
        stroke,
    );
}

fn paint_icon(painter: &Painter, rect: egui::Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(2.3, color);
    let accent = Color32::from_rgb(88, 170, 235);
    match icon {
        Icon::NewDocument => {
            polyline(
                painter,
                rect,
                &[
                    (0.24, 0.12),
                    (0.56, 0.12),
                    (0.7, 0.28),
                    (0.7, 0.86),
                    (0.24, 0.86),
                    (0.24, 0.12),
                ],
                stroke,
            );
            polyline(
                painter,
                rect,
                &[(0.56, 0.12), (0.56, 0.28), (0.7, 0.28)],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [point(rect, 0.76, 0.62), point(rect, 0.76, 0.84)],
                Stroke::new(1.8, color),
            );
            painter.line_segment(
                [point(rect, 0.65, 0.73), point(rect, 0.87, 0.73)],
                Stroke::new(1.8, color),
            );
        }
        Icon::OpenFile => {
            painter.add(Shape::convex_polygon(
                vec![
                    point(rect, 0.12, 0.28),
                    point(rect, 0.4, 0.28),
                    point(rect, 0.47, 0.4),
                    point(rect, 0.88, 0.4),
                    point(rect, 0.78, 0.84),
                    point(rect, 0.2, 0.84),
                ],
                color.gamma_multiply(0.12),
                stroke,
            ));
            painter.line_segment(
                [point(rect, 0.2, 0.4), point(rect, 0.86, 0.4)],
                Stroke::new(1.2, color.gamma_multiply(0.65)),
            );
        }
        Icon::ExportFile => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(rect, 0.16, 0.22), point(rect, 0.64, 0.84)),
                2,
                Stroke::new(1.6, color),
                StrokeKind::Inside,
            );
            painter.line_segment(
                [point(rect, 0.78, 0.72), point(rect, 0.78, 0.32)],
                Stroke::new(2.8, accent),
            );
            painter.add(Shape::convex_polygon(
                vec![
                    point(rect, 0.78, 0.16),
                    point(rect, 0.65, 0.34),
                    point(rect, 0.91, 0.34),
                ],
                accent,
                Stroke::NONE,
            ));
        }
        Icon::Undo | Icon::Redo => {
            let mut points = vec![
                (0.8, 0.3),
                (0.58, 0.22),
                (0.35, 0.29),
                (0.2, 0.48),
                (0.23, 0.7),
                (0.42, 0.83),
                (0.65, 0.83),
                (0.8, 0.68),
            ];
            if matches!(icon, Icon::Redo) {
                for (x, _) in &mut points {
                    *x = 1.0 - *x;
                }
            }
            polyline(painter, rect, &points, stroke);
            let arrow_x = if matches!(icon, Icon::Redo) {
                0.42
            } else {
                0.58
            };
            painter.add(Shape::convex_polygon(
                vec![
                    point(rect, arrow_x, 0.06),
                    point(rect, arrow_x - 0.13, 0.2),
                    point(rect, arrow_x + 0.13, 0.2),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::SelectRectangle => {
            for y in [0.2, 0.8] {
                for (x0, x1) in [(0.12, 0.28), (0.36, 0.52), (0.6, 0.76), (0.84, 0.9)] {
                    painter.line_segment(
                        [point(rect, x0, y), point(rect, x1, y)],
                        Stroke::new(2.0, color),
                    );
                }
            }
            for x in [0.12, 0.9] {
                for (y0, y1) in [(0.2, 0.36), (0.44, 0.6), (0.68, 0.8)] {
                    painter.line_segment(
                        [point(rect, x, y0), point(rect, x, y1)],
                        Stroke::new(2.0, color),
                    );
                }
            }
            painter.rect_filled(
                egui::Rect::from_min_max(point(rect, 0.12, 0.2), point(rect, 0.9, 0.8)),
                0,
                color.gamma_multiply(0.06),
            );
        }
        Icon::Zoom => {
            painter.circle_stroke(point(rect, 0.4, 0.4), rect.width() * 0.2, stroke);
            painter.line_segment(
                [point(rect, 0.55, 0.55), point(rect, 0.84, 0.84)],
                Stroke::new(3.2, color),
            );
            painter.line_segment(
                [point(rect, 0.29, 0.4), point(rect, 0.51, 0.4)],
                Stroke::new(2.0, accent),
            );
            painter.line_segment(
                [point(rect, 0.4, 0.29), point(rect, 0.4, 0.51)],
                Stroke::new(2.0, accent),
            );
        }
        Icon::RectangleShape => {
            painter.rect_filled(
                egui::Rect::from_min_max(point(rect, 0.14, 0.28), point(rect, 0.86, 0.78)),
                2,
                accent.gamma_multiply(0.42),
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(point(rect, 0.14, 0.28), point(rect, 0.86, 0.78)),
                2,
                Stroke::new(2.2, color),
                StrokeKind::Inside,
            );
        }
        Icon::Fill => {
            painter.add(Shape::convex_polygon(
                vec![
                    point(rect, 0.18, 0.42),
                    point(rect, 0.48, 0.12),
                    point(rect, 0.82, 0.43),
                    point(rect, 0.55, 0.7),
                    point(rect, 0.31, 0.66),
                ],
                color.gamma_multiply(0.22),
                stroke,
            ));
            polyline(
                painter,
                rect,
                &[(0.28, 0.4), (0.38, 0.23), (0.62, 0.23), (0.72, 0.42)],
                Stroke::new(1.3, color),
            );
            painter.circle_filled(point(rect, 0.8, 0.7), rect.width() * 0.08, accent);
        }
        Icon::EyeDropper => {
            painter.circle_stroke(point(rect, 0.72, 0.26), rect.width() * 0.14, stroke);
            painter.line_segment([point(rect, 0.62, 0.37), point(rect, 0.3, 0.69)], stroke);
            painter.add(Shape::convex_polygon(
                vec![
                    point(rect, 0.28, 0.7),
                    point(rect, 0.36, 0.78),
                    point(rect, 0.16, 0.86),
                ],
                accent.gamma_multiply(0.35),
                stroke,
            ));
            painter.circle_filled(point(rect, 0.46, 0.78), rect.width() * 0.06, accent);
        }
        Icon::AddLayer => {
            painter.rect_stroke(
                egui::Rect::from_min_max(point(rect, 0.28, 0.2), point(rect, 0.72, 0.58)),
                2,
                Stroke::new(1.3, color.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(point(rect, 0.18, 0.34), point(rect, 0.62, 0.72)),
                2,
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment(
                [point(rect, 0.76, 0.3), point(rect, 0.76, 0.54)],
                Stroke::new(2.8, accent),
            );
            painter.line_segment(
                [point(rect, 0.64, 0.42), point(rect, 0.88, 0.42)],
                Stroke::new(2.8, accent),
            );
        }
    }
}
