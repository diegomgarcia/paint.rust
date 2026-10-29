use paint_core::Bgra;

pub(crate) const PRESET_COLORS: &[(&str, Bgra)] = &[
    ("Preset black", Bgra::new(52, 52, 52, 255)),
    ("Preset gray", Bgra::new(100, 100, 100, 255)),
    ("Preset red", Bgra::new(52, 52, 243, 255)),
    ("Preset orange", Bgra::new(52, 131, 243, 255)),
    ("Preset yellow", Bgra::new(52, 214, 243, 255)),
    ("Preset lime", Bgra::new(52, 243, 188, 255)),
    ("Preset green", Bgra::new(52, 243, 109, 255)),
    ("Preset light green", Bgra::new(77, 243, 52, 255)),
    ("Preset mint", Bgra::new(160, 243, 52, 255)),
    ("Preset cyan", Bgra::new(243, 243, 52, 255)),
    ("Preset blue", Bgra::new(243, 163, 52, 255)),
    ("Preset dark blue", Bgra::new(243, 80, 52, 255)),
    ("Preset violet", Bgra::new(243, 52, 106, 255)),
    ("Preset purple", Bgra::new(243, 52, 185, 255)),
    ("Preset magenta", Bgra::new(217, 52, 243, 255)),
    ("Preset pink", Bgra::new(134, 52, 243, 255)),
];

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ColorSlot {
    Primary,
    Secondary,
}

pub(crate) fn mix_bgra(from: Bgra, to: Bgra, t: f32) -> Bgra {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Bgra::new(
        channel(from.b, to.b),
        channel(from.g, to.g),
        channel(from.r, to.r),
        channel(from.a, to.a),
    )
}

pub(crate) fn format_hex_color(color: Bgra) -> String {
    if color.a == u8::MAX {
        format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            color.r, color.g, color.b, color.a
        )
    }
}

pub(crate) fn color_slots_widget(
    ui: &mut egui::Ui,
    primary: Bgra,
    secondary: Bgra,
    active: ColorSlot,
) -> (Option<ColorSlot>, egui::Rect) {
    let (bounds, _) = ui.allocate_exact_size(egui::vec2(64.0, 54.0), egui::Sense::hover());
    let primary_rect = egui::Rect::from_min_size(bounds.min, egui::vec2(34.0, 34.0));
    let secondary_rect =
        egui::Rect::from_min_size(bounds.min + egui::vec2(24.0, 20.0), egui::vec2(34.0, 34.0));
    let secondary_response = ui.interact(
        secondary_rect,
        ui.make_persistent_id("paint-r-secondary-color"),
        egui::Sense::click(),
    );
    secondary_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Secondary color"));
    let primary_response = ui.interact(
        primary_rect,
        ui.make_persistent_id("paint-r-primary-color"),
        egui::Sense::click(),
    );
    primary_response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            active == ColorSlot::Primary,
            "Primary color",
        )
    });
    if ui.is_rect_visible(bounds) {
        paint_color_slot(
            ui,
            secondary_rect,
            secondary,
            active == ColorSlot::Secondary,
        );
        paint_color_slot(ui, primary_rect, primary, active == ColorSlot::Primary);
    }
    let slot = if primary_response.clicked() {
        Some(ColorSlot::Primary)
    } else if secondary_response.clicked() {
        Some(ColorSlot::Secondary)
    } else {
        None
    };
    (slot, bounds)
}

fn paint_color_slot(ui: &egui::Ui, rect: egui::Rect, color: Bgra, active: bool) {
    let painter = ui.painter();
    let checker = 7.0;
    let mut row = 0;
    let mut y = rect.top();
    while y < rect.bottom() {
        let mut column = 0;
        let mut x = rect.left();
        while x < rect.right() {
            let cell = egui::Rect::from_min_max(
                egui::pos2(x, y),
                egui::pos2(
                    (x + checker).min(rect.right()),
                    (y + checker).min(rect.bottom()),
                ),
            );
            painter.rect_filled(
                cell,
                0,
                if (row + column) % 2 == 0 {
                    egui::Color32::from_gray(190)
                } else {
                    egui::Color32::from_gray(120)
                },
            );
            x += checker;
            column += 1;
        }
        y += checker;
        row += 1;
    }
    painter.rect_filled(
        rect,
        3,
        egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
    );
    let stroke = if active {
        egui::Stroke::new(2.0, egui::Color32::WHITE)
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_gray(80))
    };
    painter.rect_stroke(rect, 3, stroke, egui::StrokeKind::Inside);
    if active {
        painter.add(egui::Shape::convex_polygon(
            vec![
                rect.left_top() + egui::vec2(1.0, 1.0),
                rect.left_top() + egui::vec2(13.0, 1.0),
                rect.left_top() + egui::vec2(1.0, 13.0),
            ],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
}

pub(crate) fn hsv_from_color(color: Bgra) -> (f32, f32, f32) {
    let maximum = color.r.max(color.g).max(color.b);
    let minimum = color.r.min(color.g).min(color.b);
    let chroma = maximum - minimum;
    let value = f32::from(maximum) / 255.0;
    let saturation = if maximum == 0 {
        0.0
    } else {
        f32::from(chroma) / f32::from(maximum)
    };
    let hue = if chroma == 0 {
        0.0
    } else if maximum == color.r {
        (f32::from(color.g) - f32::from(color.b)) / f32::from(chroma)
    } else if maximum == color.g {
        (f32::from(color.b) - f32::from(color.r)) / f32::from(chroma) + 2.0
    } else {
        (f32::from(color.r) - f32::from(color.g)) / f32::from(chroma) + 4.0
    } * 60.0;
    (hue.rem_euclid(360.0), saturation, value)
}

pub(crate) fn color_wheel(ui: &mut egui::Ui, current: Bgra) -> Option<Bgra> {
    let diameter = ui.available_width().clamp(96.0, 150.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(diameter, diameter),
        egui::Sense::click_and_drag(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Color wheel"));
    if ui.is_rect_visible(rect) {
        let center = rect.center();
        let radius = diameter * 0.5 - 3.0;
        let segments = 64;
        let mut mesh = egui::Mesh::default();
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center,
            uv: egui::epaint::WHITE_UV,
            color: egui::Color32::WHITE,
        });
        for index in 0..=segments {
            let angle = std::f32::consts::TAU * index as f32 / segments as f32;
            let color = crate::color_from_hsv(angle.to_degrees(), 1.0, 1.0, 255);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center + egui::vec2(angle.cos(), angle.sin()) * radius,
                uv: egui::epaint::WHITE_UV,
                color: egui::Color32::from_rgb(color.r, color.g, color.b),
            });
        }
        for index in 1..=segments {
            mesh.indices.extend([0, index as u32, (index + 1) as u32]);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );
        let maximum = current.r.max(current.g).max(current.b);
        let minimum = current.r.min(current.g).min(current.b);
        let chroma = maximum - minimum;
        if chroma > 0 {
            let hue = if maximum == current.r {
                (f32::from(current.g) - f32::from(current.b)) / f32::from(chroma)
            } else if maximum == current.g {
                (f32::from(current.b) - f32::from(current.r)) / f32::from(chroma) + 2.0
            } else {
                (f32::from(current.r) - f32::from(current.g)) / f32::from(chroma) + 4.0
            }
            .rem_euclid(6.0)
                * 60.0;
            let saturation = f32::from(chroma) / f32::from(maximum);
            let angle = hue.to_radians();
            ui.painter().circle_stroke(
                center + egui::vec2(angle.cos(), angle.sin()) * radius * saturation,
                5.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
        }
    }
    if (response.clicked() || response.dragged() || response.is_pointer_button_down_on())
        && let Some(position) = response.interact_pointer_pos()
    {
        let offset = position - rect.center();
        let radius = diameter * 0.5 - 3.0;
        if offset.length() <= radius {
            let hue = offset.y.atan2(offset.x).to_degrees().rem_euclid(360.0);
            let saturation = (offset.length() / radius).clamp(0.0, 1.0);
            return Some(crate::color_from_hsv(hue, saturation, 1.0, current.a));
        }
    }
    None
}

pub(crate) fn color_swatch(
    ui: &mut egui::Ui,
    label: &str,
    color: Bgra,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(12.0, 24.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.to_owned()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            0,
            egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
        );
        let stroke = if selected {
            egui::Stroke::new(1.5, egui::Color32::WHITE)
        } else {
            egui::Stroke::new(0.5, egui::Color32::from_gray(70))
        };
        painter.rect_stroke(rect, 0, stroke, egui::StrokeKind::Inside);
    }
    response.on_hover_text(label.to_owned())
}

fn value_field(ui: &mut egui::Ui, label: &'static str, text: &str, width: f32) -> (bool, bool) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, label));
    let painter = ui.painter();
    painter.rect_filled(rect, 3, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        rect,
        3,
        egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center() - egui::vec2(6.0, 0.0),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    let split = rect.right() - 12.0;
    let up_rect = egui::Rect::from_min_max(
        egui::pos2(split, rect.top()),
        egui::pos2(rect.right(), rect.center().y),
    );
    let down_rect = egui::Rect::from_min_max(
        egui::pos2(split, rect.center().y),
        egui::pos2(rect.right(), rect.bottom()),
    );
    let up = ui.interact(
        up_rect,
        ui.scope_id().with((label, "up")),
        egui::Sense::click(),
    );
    let down = ui.interact(
        down_rect,
        ui.scope_id().with((label, "down")),
        egui::Sense::click(),
    );
    let arrow = egui::Color32::from_gray(210);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(up_rect.center().x, up_rect.center().y - 3.0),
            egui::pos2(up_rect.center().x - 3.0, up_rect.center().y + 2.0),
            egui::pos2(up_rect.center().x + 3.0, up_rect.center().y + 2.0),
        ],
        arrow,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(down_rect.center().x, down_rect.center().y + 3.0),
            egui::pos2(down_rect.center().x - 3.0, down_rect.center().y - 2.0),
            egui::pos2(down_rect.center().x + 3.0, down_rect.center().y - 2.0),
        ],
        arrow,
        egui::Stroke::NONE,
    ));
    (up.clicked(), down.clicked())
}

pub(crate) fn compact_color_slider(
    ui: &mut egui::Ui,
    label: &'static str,
    short: &'static str,
    value: &mut u8,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.allocate_ui_with_layout(
            egui::vec2(26.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(short);
            },
        );
        let channel = match short {
            "R" => GradientChannel::Red,
            "G" => GradientChannel::Green,
            "B" => GradientChannel::Blue,
            _ => GradientChannel::Alpha,
        };
        gradient_slider(ui, label, value, channel);
        let (up, down) = value_field(
            ui,
            match label {
                "Red" => "Red value",
                "Green" => "Green value",
                "Blue" => "Blue value",
                _ => "Opacity value",
            },
            &format!("{value}"),
            44.0,
        );
        if up {
            *value = value.saturating_add(1);
        }
        if down {
            *value = value.saturating_sub(1);
        }
    });
}

enum GradientChannel {
    Red,
    Green,
    Blue,
    Alpha,
}

pub(crate) fn hsv_gradient_slider(
    ui: &mut egui::Ui,
    label: &'static str,
    short: &'static str,
    value: &mut f32,
    minimum: f32,
    maximum: f32,
    color_at: impl Fn(f32) -> egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.allocate_ui_with_layout(
            egui::vec2(26.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(short);
            },
        );
        let width = 120.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label));
        let painter = ui.painter();
        let track = egui::Rect::from_min_size(rect.min, egui::vec2(width, 8.0));
        let columns = width.round() as usize;
        for index in 0..columns {
            let t = if columns > 1 {
                index as f32 / (columns as f32 - 1.0)
            } else {
                0.0
            };
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(track.left() + index as f32, track.top()),
                    egui::vec2(1.0, track.height()),
                ),
                0,
                color_at(t),
            );
        }
        painter.rect_stroke(
            track,
            0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );
        let t = ((*value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
        let arrow_x = rect.left() + t * width;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(arrow_x, rect.top() + 9.0),
                egui::pos2(arrow_x - 4.0, rect.top() + 17.0),
                egui::pos2(arrow_x + 4.0, rect.top() + 17.0),
            ],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
        if (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
            && let Some(position) = response.interact_pointer_pos()
        {
            let fraction = ((position.x - rect.left()) / width).clamp(0.0, 1.0);
            *value = minimum + fraction * (maximum - minimum);
        }
        let (up, down) = value_field(
            ui,
            match label {
                "Hue" => "Hue value",
                "Saturation" => "Saturation value",
                _ => "Value value",
            },
            &format!("{value:.0}"),
            44.0,
        );
        if up {
            *value = (*value + 1.0).min(maximum);
        }
        if down {
            *value = (*value - 1.0).max(minimum);
        }
    });
}

fn gradient_slider(
    ui: &mut egui::Ui,
    label: &'static str,
    value: &mut u8,
    channel: GradientChannel,
) {
    let width = 120.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label));
    let painter = ui.painter();
    let track = egui::Rect::from_min_size(rect.min, egui::vec2(width, 8.0));
    if matches!(channel, GradientChannel::Alpha) {
        let cell: f32 = 4.0;
        let mut row = 0;
        let mut y = track.top();
        while y < track.bottom() {
            let mut col = 0;
            let mut x = track.left();
            while x < track.right() {
                let bright = if (row + col) % 2 == 0 { 90 } else { 150 };
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(cell.min(track.right() - x), cell.min(track.bottom() - y)),
                    ),
                    0,
                    egui::Color32::from_gray(bright),
                );
                x += cell;
                col += 1;
            }
            y += cell;
            row += 1;
        }
    }
    let t = f32::from(*value) / 255.0;
    let columns = width.round() as usize;
    for index in 0..columns {
        let tt = if columns > 1 {
            index as f32 / (columns as f32 - 1.0)
        } else {
            0.0
        };
        let color = match channel {
            GradientChannel::Red => egui::Color32::from_rgb((tt * 255.0) as u8, 0, 0),
            GradientChannel::Green => egui::Color32::from_rgb(0, (tt * 255.0) as u8, 0),
            GradientChannel::Blue => egui::Color32::from_rgb(0, 0, (tt * 255.0) as u8),
            GradientChannel::Alpha => {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, (tt * 255.0) as u8)
            }
        };
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(track.left() + index as f32, track.top()),
                egui::vec2(1.0, track.height()),
            ),
            0,
            color,
        );
    }
    painter.rect_stroke(
        track,
        0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
        egui::StrokeKind::Inside,
    );
    let arrow_x = rect.left() + t * width;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(arrow_x, rect.top() + 9.0),
            egui::pos2(arrow_x - 4.0, rect.top() + 17.0),
            egui::pos2(arrow_x + 4.0, rect.top() + 17.0),
        ],
        egui::Color32::WHITE,
        egui::Stroke::NONE,
    ));
    if (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
        && let Some(position) = response.interact_pointer_pos()
    {
        let fraction = ((position.x - rect.left()) / width).clamp(0.0, 1.0);
        *value = (fraction * 255.0).round() as u8;
    }
}
