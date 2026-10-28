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

fn p24(rect: egui::Rect, x: f32, y: f32) -> Pos2 {
    rect.min + vec2(rect.width() * x / 24.0, rect.height() * y / 24.0)
}

fn s24(rect: egui::Rect, width: f32) -> f32 {
    rect.width() * width / 24.0
}

fn line24(painter: &Painter, rect: egui::Rect, a: (f32, f32), b: (f32, f32), stroke: Stroke) {
    painter.line_segment([p24(rect, a.0, a.1), p24(rect, b.0, b.1)], stroke);
}

fn poly24(
    painter: &Painter,
    rect: egui::Rect,
    points: &[(f32, f32)],
    closed: bool,
    stroke: Stroke,
    fill: Color32,
) {
    let mut mapped: Vec<Pos2> = points.iter().map(|&(x, y)| p24(rect, x, y)).collect();
    if closed && let Some(first) = mapped.first().copied() {
        mapped.push(first);
    }
    painter.add(Shape::line(mapped, stroke));
    if fill != Color32::TRANSPARENT && points.len() >= 3 {
        painter.add(Shape::convex_polygon(
            points.iter().map(|&(x, y)| p24(rect, x, y)).collect(),
            fill,
            Stroke::NONE,
        ));
    }
}

fn arc24(
    painter: &Painter,
    rect: egui::Rect,
    center: (f32, f32),
    radius: f32,
    angles: (f32, f32),
    stroke: Stroke,
) {
    let steps = 5;
    let points: Vec<Pos2> = (0..=steps)
        .map(|index| {
            let angle = angles.0 + (angles.1 - angles.0) * index as f32 / steps as f32;
            p24(
                rect,
                center.0 + radius * angle.cos(),
                center.1 + radius * angle.sin(),
            )
        })
        .collect();
    painter.add(Shape::line(points, stroke));
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
            let dash = Stroke::new(s24(rect, 2.0), color);
            for (x0, x1) in [(6.0, 9.0), (11.0, 14.0), (16.0, 19.0)] {
                line24(painter, rect, (x0, 3.0), (x1, 3.0), dash);
                line24(painter, rect, (x0, 21.0), (x1, 21.0), dash);
            }
            for (y0, y1) in [(6.0, 9.0), (11.0, 14.0), (16.0, 19.0)] {
                line24(painter, rect, (3.0, y0), (3.0, y1), dash);
                line24(painter, rect, (21.0, y0), (21.0, y1), dash);
            }
            arc24(
                painter,
                rect,
                (5.0, 5.0),
                2.0,
                (std::f32::consts::PI, 1.5 * std::f32::consts::PI),
                dash,
            );
            arc24(
                painter,
                rect,
                (19.0, 5.0),
                2.0,
                (1.5 * std::f32::consts::PI, std::f32::consts::TAU),
                dash,
            );
            arc24(
                painter,
                rect,
                (19.0, 19.0),
                2.0,
                (0.0, 0.5 * std::f32::consts::PI),
                dash,
            );
            arc24(
                painter,
                rect,
                (5.0, 19.0),
                2.0,
                (0.5 * std::f32::consts::PI, std::f32::consts::PI),
                dash,
            );
        }
        Icon::Zoom => {
            painter.circle_stroke(p24(rect, 11.0, 11.0), s24(rect, 8.0), stroke);
            line24(
                painter,
                rect,
                (21.0, 21.0),
                (16.65, 16.65),
                Stroke::new(s24(rect, 2.4), color),
            );
            line24(painter, rect, (11.0, 8.0), (11.0, 14.0), stroke);
            line24(painter, rect, (8.0, 11.0), (14.0, 11.0), stroke);
        }
        Icon::RectangleShape => {
            let shape = egui::Rect::from_min_max(p24(rect, 2.0, 6.0), p24(rect, 22.0, 18.0));
            painter.rect_filled(shape, s24(rect, 2.0), color.gamma_multiply(0.14));
            painter.rect_stroke(
                shape,
                s24(rect, 2.0),
                Stroke::new(s24(rect, 2.0), color),
                StrokeKind::Inside,
            );
        }
        Icon::Fill => {
            let body = [
                (19.0, 11.0),
                (11.0, 3.0),
                (2.4, 11.6),
                (2.4, 13.0),
                (7.6, 18.2),
                (9.0, 19.0),
                (10.6, 18.2),
            ];
            poly24(
                painter,
                rect,
                &body,
                true,
                stroke,
                color.gamma_multiply(0.12),
            );
            line24(painter, rect, (5.0, 2.0), (10.0, 7.0), stroke);
            line24(
                painter,
                rect,
                (2.0, 13.0),
                (17.0, 13.0),
                Stroke::new(s24(rect, 1.8), color),
            );
            let droplet = [
                (21.8, 19.8),
                (20.6, 18.2),
                (19.2, 20.2),
                (19.7, 21.5),
                (20.9, 21.8),
                (21.8, 20.8),
            ];
            poly24(
                painter,
                rect,
                &droplet,
                true,
                stroke,
                color.gamma_multiply(0.25),
            );
        }
        Icon::EyeDropper => {
            line24(painter, rect, (2.0, 22.0), (3.0, 21.0), stroke);
            line24(painter, rect, (3.0, 21.0), (6.0, 21.0), stroke);
            line24(painter, rect, (6.0, 21.0), (15.0, 12.0), stroke);
            line24(painter, rect, (3.0, 21.0), (3.0, 18.0), stroke);
            line24(painter, rect, (3.0, 18.0), (12.0, 9.0), stroke);
            line24(
                painter,
                rect,
                (15.0, 6.0),
                (18.4, 2.6),
                Stroke::new(s24(rect, 2.6), color),
            );
            painter.circle_stroke(p24(rect, 19.4, 3.6), s24(rect, 2.0), stroke);
            line24(painter, rect, (12.0, 9.0), (15.0, 6.0), stroke);
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
