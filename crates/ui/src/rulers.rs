//! Document rulers drawn around the composed canvas.
//!
//! The rulers live inside the canvas scroll surface: the top strip occupies the first
//! [`RULER_THICKNESS`] rows of the scroll content and the left strip the first
//! [`RULER_THICKNESS`] columns. Because they are regular scroll content they move with the
//! canvas when it is panned or scrolled instead of staying pinned to the viewport.
//!
//! Ticks are generated in document pixels and converted to the selected [`CanvasUnit`]. The
//! labeled (major) tick interval is the smallest "nice" interval whose on-screen spacing is at
//! least [`MIN_MAJOR_SCREEN`] pixels, so labels stay legible at every supported zoom level.

use egui::{Align2, FontId, Pos2, Rect, Role, Sense, Stroke, Ui, Vec2, WidgetInfo, pos2};
use paint_core::Size;

use crate::app::CanvasUnit;

/// Width of each ruler strip in logical points.
pub const RULER_THICKNESS: f32 = 22.0;

/// Minimum distance between labeled ticks, in screen points.
const MIN_MAJOR_SCREEN: f64 = 48.0;
/// Minimum distance between minor ticks, in screen points.
const MIN_MINOR_SCREEN: f64 = 6.0;
/// Height of a major tick mark, in points.
const MAJOR_TICK: f32 = 8.0;
/// Height of a minor tick mark, in points.
const MINOR_TICK: f32 = 4.0;
/// Tick label font size, in points.
const LABEL_FONT_SIZE: f32 = 9.0;

const INCHES_PER_UNIT: f64 = 96.0;
const CENTIMETERS_PER_INCH: f64 = 2.54;

/// One tick on a ruler axis.
#[derive(Clone, Debug, PartialEq)]
pub struct RulerTick {
    /// Position in document pixels (the same coordinate space used by pointer/selection code).
    pub value: f64,
    /// Screen position at the canvas edge, i.e. the base of the tick mark.
    pub position: Pos2,
    /// Human-readable label for labeled (major) ticks, in the selected unit.
    pub label: Option<String>,
    /// Whether this is a labeled tick.
    pub major: bool,
}

/// Geometry for the current top and left rulers.
#[derive(Clone, Debug, PartialEq)]
pub struct RulerLayout {
    /// Width of each strip in logical points.
    pub thickness: f32,
    /// Top-left corner of the document image on screen.
    pub canvas_origin: Pos2,
    /// On-screen size of the document image.
    pub canvas_size: Vec2,
    /// Unit suffix shown in the corner box.
    pub unit: &'static str,
    /// Ticks on the top ruler, from document x = 0 to the document width.
    pub horizontal: Vec<RulerTick>,
    /// Ticks on the left ruler, from document y = 0 to the document height.
    pub vertical: Vec<RulerTick>,
}

impl RulerLayout {
    /// Returns the horizontal major tick whose document position is `value` pixels.
    pub fn horizontal_tick(&self, value: f64) -> Option<&RulerTick> {
        self.horizontal
            .iter()
            .find(|tick| (tick.value - value).abs() < 1e-6)
    }

    /// Returns the vertical major tick whose document position is `value` pixels.
    pub fn vertical_tick(&self, value: f64) -> Option<&RulerTick> {
        self.vertical
            .iter()
            .find(|tick| (tick.value - value).abs() < 1e-6)
    }
}

/// Computes both rulers for the given document size, zoom, unit, and canvas placement.
pub(crate) fn layout(unit: CanvasUnit, zoom: f32, canvas_origin: Pos2, size: Size) -> RulerLayout {
    let zoom = f64::from(zoom.clamp(0.01, 64.0));
    let horizontal = axis_ticks(unit, zoom, f64::from(size.width()), canvas_origin, false);
    let vertical = axis_ticks(unit, zoom, f64::from(size.height()), canvas_origin, true);
    RulerLayout {
        thickness: RULER_THICKNESS,
        canvas_origin,
        canvas_size: egui::vec2(size.width() as f32, size.height() as f32) * zoom as f32,
        unit: unit.suffix(),
        horizontal,
        vertical,
    }
}

fn axis_ticks(
    unit: CanvasUnit,
    zoom: f64,
    extent: f64,
    canvas_origin: Pos2,
    vertical: bool,
) -> Vec<RulerTick> {
    let major_step = major_step_pixels(unit, zoom);
    let divisions = minor_divisions(major_step, zoom);
    let minor_step = major_step / f64::from(divisions);
    if minor_step <= 0.0 || !minor_step.is_finite() {
        return Vec::new();
    }
    let count = (extent / minor_step + 1e-9).floor().max(0.0) as u64;
    let mut ticks = Vec::with_capacity(usize::try_from(count).unwrap_or(0) + 1);
    for index in 0..=count {
        let value = index as f64 * minor_step;
        let major = index % u64::from(divisions) == 0;
        let position = if vertical {
            pos2(canvas_origin.x, canvas_origin.y + (value * zoom) as f32)
        } else {
            pos2(canvas_origin.x + (value * zoom) as f32, canvas_origin.y)
        };
        ticks.push(RulerTick {
            value,
            position,
            label: major.then(|| format_label(unit, value, major_step)),
            major,
        });
    }
    ticks
}

fn major_step_pixels(unit: CanvasUnit, zoom: f64) -> f64 {
    let candidates = unit_candidates(unit);
    candidates
        .iter()
        .copied()
        .find(|pixels| pixels * zoom >= MIN_MAJOR_SCREEN)
        .unwrap_or_else(|| *candidates.last().expect("unit candidates are never empty"))
}

fn minor_divisions(major_step: f64, zoom: f64) -> u32 {
    let screen = major_step * zoom;
    [10, 5, 4, 2]
        .into_iter()
        .find(|divisions| screen / f64::from(*divisions) >= MIN_MINOR_SCREEN)
        .unwrap_or(1)
}

fn unit_candidates(unit: CanvasUnit) -> Vec<f64> {
    match unit {
        CanvasUnit::Pixels => {
            let mut steps = Vec::new();
            let mut scale = 1.0;
            while scale <= 1.0e9 {
                for multiplier in [1.0, 2.0, 5.0] {
                    steps.push(scale * multiplier);
                }
                scale *= 10.0;
            }
            steps
        }
        CanvasUnit::Inches => {
            let mut steps = vec![1.0 / 16.0, 1.0 / 8.0, 1.0 / 4.0, 1.0 / 2.0];
            let mut scale = 1.0;
            while scale <= 1.0e6 {
                for multiplier in [1.0, 2.0, 5.0] {
                    steps.push(scale * multiplier);
                }
                scale *= 10.0;
            }
            steps
                .into_iter()
                .map(|inches| inches * INCHES_PER_UNIT)
                .collect()
        }
        CanvasUnit::Centimeters => {
            let mut steps = vec![0.1, 0.2, 0.5];
            let mut scale = 1.0;
            while scale <= 1.0e6 {
                for multiplier in [1.0, 2.0, 5.0] {
                    steps.push(scale * multiplier);
                }
                scale *= 10.0;
            }
            steps
                .into_iter()
                .map(|centimeters| centimeters / CENTIMETERS_PER_INCH * INCHES_PER_UNIT)
                .collect()
        }
    }
}

fn format_label(unit: CanvasUnit, value_pixels: f64, major_step_pixels: f64) -> String {
    let value = unit.convert(value_pixels);
    match unit {
        CanvasUnit::Pixels => format!("{}", value.round() as i64),
        _ => {
            let decimals = decimals_for(unit.convert(major_step_pixels));
            format!("{value:.decimals$}")
        }
    }
}

fn decimals_for(step: f64) -> usize {
    let mut decimals = 0;
    let mut scaled = step.abs();
    while scaled < 1.0 - 1e-6 && decimals < 4 {
        scaled *= 10.0;
        decimals += 1;
    }
    decimals
}

/// Draws the ruler strips and registers their accessible nodes.
///
/// `block_rect` is the full scroll-content rectangle (rulers plus canvas) and `canvas_rect` is
/// the document image inside it.
pub(crate) fn paint(ui: &Ui, block_rect: Rect, canvas_rect: Rect, layout: &RulerLayout) {
    let visuals = ui.visuals();
    let background = visuals.widgets.noninteractive.bg_fill;
    let border = visuals.widgets.noninteractive.bg_stroke;
    let text_color = visuals.text_color();
    let tick_color = visuals.weak_text_color();
    let label_font = FontId::proportional(LABEL_FONT_SIZE);

    let top = Rect::from_min_max(block_rect.min, pos2(canvas_rect.max.x, canvas_rect.min.y));
    let left = Rect::from_min_max(block_rect.min, pos2(canvas_rect.min.x, canvas_rect.max.y));
    let corner = Rect::from_min_max(block_rect.min, canvas_rect.min);
    let painter = ui.painter();
    painter.rect_filled(top, 0.0, background);
    painter.rect_filled(left, 0.0, background);
    painter.rect_filled(corner, 0.0, background);
    painter.line_segment(
        [
            pos2(block_rect.left(), canvas_rect.top() - 0.5),
            pos2(canvas_rect.right(), canvas_rect.top() - 0.5),
        ],
        border,
    );
    painter.line_segment(
        [
            pos2(canvas_rect.left() - 0.5, block_rect.top()),
            pos2(canvas_rect.left() - 0.5, canvas_rect.bottom()),
        ],
        border,
    );

    let top_painter = painter.with_clip_rect(top);
    for (index, tick) in layout.horizontal.iter().enumerate() {
        let length = if tick.major { MAJOR_TICK } else { MINOR_TICK };
        top_painter.line_segment(
            [
                pos2(tick.position.x, canvas_rect.top()),
                pos2(tick.position.x, canvas_rect.top() - length),
            ],
            Stroke::new(1.0, tick_color),
        );
        if let Some(label) = &tick.label {
            let galley = top_painter.layout_no_wrap(label.clone(), label_font.clone(), text_color);
            let text_pos = pos2(tick.position.x + 2.0, top.top() + 1.0);
            top_painter.galley(text_pos, galley.clone(), text_color);
            let node_rect = Rect::from_min_size(text_pos, galley.size());
            ruler_node(
                ui,
                ui.scope_id().with(("paint-r-ruler-horizontal-tick", index)),
                node_rect,
                format!("{label} {}", layout.unit),
            );
        }
    }

    let left_painter = painter.with_clip_rect(left);
    for (index, tick) in layout.vertical.iter().enumerate() {
        let length = if tick.major { MAJOR_TICK } else { MINOR_TICK };
        left_painter.line_segment(
            [
                pos2(canvas_rect.left(), tick.position.y),
                pos2(canvas_rect.left() - length, tick.position.y),
            ],
            Stroke::new(1.0, tick_color),
        );
        if let Some(label) = &tick.label {
            let galley = left_painter.layout_no_wrap(label.clone(), label_font.clone(), text_color);
            let text_pos = pos2(left.left() + 2.0, tick.position.y + 1.0);
            left_painter.galley(text_pos, galley.clone(), text_color);
            let node_rect = Rect::from_min_size(text_pos, galley.size());
            ruler_node(
                ui,
                ui.scope_id().with(("paint-r-ruler-vertical-tick", index)),
                node_rect,
                format!("{label} {}", layout.unit),
            );
        }
    }

    painter.text(
        corner.center(),
        Align2::CENTER_CENTER,
        layout.unit,
        label_font,
        text_color,
    );
    ruler_node(
        ui,
        ui.scope_id().with("paint-r-ruler-units"),
        corner,
        format!("Ruler units: {}", layout.unit),
    );
    ruler_node(
        ui,
        ui.scope_id().with("paint-r-ruler-horizontal"),
        top,
        "Horizontal ruler",
    );
    ruler_node(
        ui,
        ui.scope_id().with("paint-r-ruler-vertical"),
        left,
        "Vertical ruler",
    );
}

fn ruler_node(ui: &Ui, id: egui::Id, rect: Rect, label: impl Into<String>) {
    let label = label.into();
    let response = ui.interact(rect, id, Sense::hover());
    response.widget_info(move || WidgetInfo::labeled(Role::Label, true, label.clone()));
}

#[cfg(test)]
mod tests {
    use super::{
        CanvasUnit, MIN_MAJOR_SCREEN, MIN_MINOR_SCREEN, RULER_THICKNESS, layout, major_step_pixels,
        minor_divisions,
    };
    use egui::pos2;
    use paint_core::Size;

    fn size() -> Size {
        Size::new(800, 600).expect("size")
    }

    #[test]
    fn ruler_thickness_is_compact_but_readable() {
        assert!((20.0..=24.0).contains(&RULER_THICKNESS));
    }

    #[test]
    fn major_and_minor_spacing_hold_across_zoom_extremes() {
        for unit in [
            CanvasUnit::Pixels,
            CanvasUnit::Inches,
            CanvasUnit::Centimeters,
        ] {
            for zoom in [0.1, 0.125, 0.25, 0.5, 1.0, 1.5, 2.0, 4.0, 8.0] {
                let major = major_step_pixels(unit, zoom);
                let divisions = minor_divisions(major, zoom);
                assert!(
                    major * zoom >= MIN_MAJOR_SCREEN - 1e-6,
                    "{unit:?} at {zoom}: labeled ticks only {:.1}px apart",
                    major * zoom
                );
                assert!(
                    major / f64::from(divisions) * zoom >= MIN_MINOR_SCREEN - 1e-6,
                    "{unit:?} at {zoom}: minor ticks only {:.1}px apart",
                    major / f64::from(divisions) * zoom
                );
                let layout = layout(unit, zoom as f32, pos2(10.0, 10.0), size());
                let majors: Vec<_> = layout.horizontal.iter().filter(|tick| tick.major).collect();
                assert!(majors.len() >= 2);
                for pair in majors.windows(2) {
                    assert!(
                        pair[1].position.x - pair[0].position.x >= 40.0,
                        "{unit:?} at {zoom}: labels overlap"
                    );
                }
            }
        }
    }

    #[test]
    fn pixel_labels_are_exact_and_inch_and_centimeter_labels_convert() {
        let pixels = layout(CanvasUnit::Pixels, 1.0, pos2(0.0, 0.0), size());
        assert_eq!(
            pixels
                .horizontal_tick(100.0)
                .and_then(|tick| tick.label.as_deref()),
            Some("100")
        );
        let inches = layout(CanvasUnit::Inches, 1.0, pos2(0.0, 0.0), size());
        assert_eq!(
            inches
                .horizontal_tick(96.0)
                .and_then(|tick| tick.label.as_deref()),
            Some("1.0")
        );
        let centimeters = layout(CanvasUnit::Centimeters, 1.0, pos2(0.0, 0.0), size());
        let two_centimeters = 2.0 / 2.54 * 96.0;
        assert_eq!(
            centimeters
                .horizontal_tick(two_centimeters)
                .and_then(|tick| tick.label.as_deref()),
            Some("2")
        );
    }
}
