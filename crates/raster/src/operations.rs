use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, DocumentError, Layer, LayerId, PixelPatch, SelectionMask};

use crate::RasterError;

/// Endpoint treatment for [`stroke_line`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LineCap {
    /// A squared-off end that stops exactly at the endpoint.
    #[default]
    Flat,
    /// A disc extending past the endpoint by half the line width.
    Rounded,
    /// A chevron drawn back from the endpoint.
    Arrow,
}

/// Dash pattern for [`stroke_line`].
///
/// `Dash` and `Dot` use GDI-style patterns scaled by the line width: `Dash` is three
/// widths on and one width off, `Dot` is one width on and one width off.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LineDash {
    #[default]
    Solid,
    Dash,
    Dot,
}

/// Width, paint, cap, and dash inputs for [`stroke_line`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LineStyle {
    pub width: u32,
    pub color: Option<Bgra>,
    pub start_cap: LineCap,
    pub end_cap: LineCap,
    pub dash: LineDash,
}

/// The canvas-clipped area an operation may touch and the optional mask inside it.
#[derive(Clone, Copy)]
pub(crate) struct OperationClip<'a> {
    pub(crate) rect: Rect,
    pub(crate) mask: Option<&'a SelectionMask>,
}

impl OperationClip<'_> {
    pub(crate) fn selects_xy(&self, x: i64, y: i64) -> bool {
        self.mask.is_none_or(|mask| mask.contains_xy(x, y))
    }
}

/// Produces a solid-fill patch for the active selection, clipped to the canvas.
/// Returns `Ok(None)` when no selected pixel changes.
pub fn fill(
    document: &Document,
    layer_id: LayerId,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = i64::from(rect.width());
    let mut changed = false;
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            let pixel_x = rect.left() + i64::from(x);
            let pixel_y = rect.top() + i64::from(y);
            if !clip.selects_xy(pixel_x, pixel_y) {
                continue;
            }
            let index = (i64::from(y) * width + i64::from(x)) as usize;
            if after[index] != color {
                after[index] = color;
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

pub fn fill_active(document: &Document, color: Bgra) -> Result<Option<PixelPatch>, RasterError> {
    fill(document, document.active_layer_id(), color)
}

/// Produces an RGB-inversion patch for the active selection, preserving every alpha value.
/// Returns `Ok(None)` only when no selected pixel is touched.
pub fn invert(document: &Document, layer_id: LayerId) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = i64::from(rect.width());
    let mut changed = false;
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            let pixel_x = rect.left() + i64::from(x);
            let pixel_y = rect.top() + i64::from(y);
            if !clip.selects_xy(pixel_x, pixel_y) {
                continue;
            }
            let index = (i64::from(y) * width + i64::from(x)) as usize;
            after[index] = before[index].inverted_rgb();
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

pub fn invert_active(document: &Document) -> Result<Option<PixelPatch>, RasterError> {
    invert(document, document.active_layer_id())
}

/// Flips every pixel in one layer left-to-right, ignoring the current selection.
/// Returns `Ok(None)` when the layer is horizontally symmetric.
pub fn flip_horizontal(
    document: &Document,
    layer_id: LayerId,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let rect = document.canvas_rect();
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = rect.size().width() as usize;
    let height = rect.size().height() as usize;
    for row in 0..height {
        for column in 0..width {
            after[row * width + column] = before[row * width + (width - 1 - column)];
        }
    }
    if after == before {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Flips every pixel in one layer top-to-bottom, ignoring the current selection.
/// Returns `Ok(None)` when the layer is vertically symmetric.
pub fn flip_vertical(
    document: &Document,
    layer_id: LayerId,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let rect = document.canvas_rect();
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = rect.size().width() as usize;
    let height = rect.size().height() as usize;
    for row in 0..height {
        let source_row = height - 1 - row;
        after[row * width..(row + 1) * width]
            .copy_from_slice(&before[source_row * width..(source_row + 1) * width]);
    }
    if after == before {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

pub(crate) fn layer(document: &Document, layer_id: LayerId) -> Result<&Layer, RasterError> {
    document
        .layer(layer_id)
        .ok_or(DocumentError::LayerNotFound { layer_id })
        .map_err(RasterError::from)
}

pub(crate) fn operation_clip(document: &Document) -> Option<OperationClip<'_>> {
    match document.selection_model() {
        Some(selection) => Some(OperationClip {
            rect: selection.bounds().clip_to(document.size())?,
            mask: selection.mask(),
        }),
        None => Some(OperationClip {
            rect: document.canvas_rect(),
            mask: None,
        }),
    }
}

pub(crate) fn copy_pixels(
    pixels: &[Bgra],
    requested_bytes: usize,
) -> Result<Vec<Bgra>, RasterError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(pixels.len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    copy.extend_from_slice(pixels);
    Ok(copy)
}

fn rgb_within_tolerance(pixel: Bgra, target: Bgra, tolerance: i16) -> bool {
    (i16::from(pixel.r) - i16::from(target.r)).abs() <= tolerance
        && (i16::from(pixel.g) - i16::from(target.g)).abs() <= tolerance
        && (i16::from(pixel.b) - i16::from(target.b)).abs() <= tolerance
}

fn bgra_within_tolerance(pixel: Bgra, target: Bgra, tolerance: i16) -> bool {
    rgb_within_tolerance(pixel, target, tolerance)
        && (i16::from(pixel.a) - i16::from(target.a)).abs() <= tolerance
}

/// Round brush geometry shared by the paint, erase, and recolor strokes.
///
/// `hardness` and `spacing` are percentages. Hardness 100 is a hard edge and 0 fades from
/// the centre to the radius; spacing is the distance between stamps as a percentage of the
/// brush diameter. Paint.NET defaults are 75% hardness and 15% spacing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrushOptions {
    /// Brush diameter in pixels.
    pub size: u32,
    /// Edge hardness percentage in `0..=100`.
    pub hardness: u8,
    /// Stamp spacing percentage in `1..=100`.
    pub spacing: u8,
}

impl BrushOptions {
    pub const fn new(size: u32, hardness: u8, spacing: u8) -> Self {
        Self {
            size,
            hardness,
            spacing,
        }
    }

    fn normalized(self) -> Self {
        Self {
            size: if self.size == 0 { 1 } else { self.size },
            hardness: self.hardness.min(100),
            spacing: self.spacing.clamp(1, 100),
        }
    }
}

impl Default for BrushOptions {
    fn default() -> Self {
        Self {
            size: 45,
            hardness: 75,
            spacing: 15,
        }
    }
}

/// Coverage of one brush pixel, fading from 255 at the hard core to 0 at the radius.
fn brush_coverage(distance: f64, radius: f64, hardness: f64) -> u8 {
    let inner_radius = radius * hardness;
    if radius <= 0.0 || distance <= inner_radius {
        return u8::MAX;
    }
    let t = ((distance - inner_radius) / (radius - inner_radius)).clamp(0.0, 1.0);
    let smooth = t * t * (3.0 - 2.0 * t);
    ((1.0 - smooth) * 255.0).round() as u8
}

/// Calls `stamp` at evenly spaced positions along a polyline, always including its start.
fn for_each_stamp(points: &[Point], step: f64, mut stamp: impl FnMut(f64, f64)) {
    let Some(first) = points.first() else {
        return;
    };
    stamp(f64::from(first.x), f64::from(first.y));
    if points.len() < 2 {
        return;
    }
    let step = step.max(1.0);
    let mut travelled = 0.0;
    let mut next = step;
    for pair in points.windows(2) {
        let start_x = f64::from(pair[0].x);
        let start_y = f64::from(pair[0].y);
        let dx = f64::from(pair[1].x) - start_x;
        let dy = f64::from(pair[1].y) - start_y;
        let length = dx.hypot(dy);
        if length <= f64::EPSILON {
            continue;
        }
        while next <= travelled + length {
            let ratio = (next - travelled) / length;
            stamp(start_x + dx * ratio, start_y + dy * ratio);
            next += step;
        }
        travelled += length;
    }
}

/// Paints a round brush stroke through canvas-space points.
///
/// Hardness softens the stamp edge and spacing controls how far apart stamps are placed.
/// `color = None` erases to transparent, preserving a partial alpha for soft coverage. The
/// patch is clipped to the active selection mask.
pub fn stroke(
    document: &Document,
    layer_id: LayerId,
    points: &[Point],
    brush: BrushOptions,
    color: Option<Bgra>,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let Some(first) = points.first() else {
        return Ok(None);
    };
    let brush = brush.normalized();
    let radius = brush.size.saturating_sub(1) / 2;
    let radius_f = f64::from(radius);
    let radius_i64 = i64::from(radius);
    let mut min_x = i64::from(first.x);
    let mut min_y = i64::from(first.y);
    let mut max_x = min_x;
    let mut max_y = min_y;
    for point in points {
        min_x = min_x.min(i64::from(point.x));
        min_y = min_y.min(i64::from(point.y));
        max_x = max_x.max(i64::from(point.x));
        max_y = max_y.max(i64::from(point.y));
    }
    let Some(rect) = clip_rect(
        clip.rect,
        min_x - radius_i64,
        min_y - radius_i64,
        max_x + radius_i64 + 1,
        max_y + radius_i64 + 1,
    )?
    else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let left = rect.left();
    let top = rect.top();
    let width = i64::from(rect.width());
    let height = i64::from(rect.height());
    let hardness = f64::from(brush.hardness) / 100.0;
    let step = f64::from(brush.size) * f64::from(brush.spacing) / 100.0;

    for_each_stamp(points, step, |center_x, center_y| {
        let x0 = (center_x - radius_f).floor() as i64;
        let x1 = (center_x + radius_f).ceil() as i64;
        let y0 = (center_y - radius_f).floor() as i64;
        let y1 = (center_y + radius_f).ceil() as i64;
        for py in y0.max(top)..=y1.min(top + height - 1) {
            for px in x0.max(left)..=x1.min(left + width - 1) {
                let dx = px as f64 - center_x;
                let dy = py as f64 - center_y;
                let distance = dx.hypot(dy);
                if distance > radius_f {
                    continue;
                }
                if !clip.selects_xy(px, py) {
                    continue;
                }
                let coverage = brush_coverage(distance, radius_f, hardness);
                if coverage == 0 {
                    continue;
                }
                let index = ((py - top) * width + (px - left)) as usize;
                match color {
                    Some(source) if coverage == u8::MAX => after[index] = source,
                    Some(source) => {
                        let alpha =
                            (u16::from(source.a) * u16::from(coverage) + 127) / u16::from(u8::MAX);
                        let blended = Bgra::new(source.b, source.g, source.r, alpha as u8);
                        after[index] = crate::normal_over(after[index], blended, u8::MAX);
                    }
                    None => {
                        let remaining = (u16::from(after[index].a) * u16::from(u8::MAX - coverage)
                            + 127)
                            / u16::from(u8::MAX);
                        after[index] = if remaining == 0 {
                            Bgra::TRANSPARENT
                        } else {
                            let pixel = after[index];
                            Bgra::new(pixel.b, pixel.g, pixel.r, remaining as u8)
                        };
                    }
                }
            }
        }
    });

    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Paints a straight line with real end caps and a dash pattern.
///
/// The shaft is clipped to the active selection mask, like every other raster operation.
/// `Flat` stops at the endpoints, `Rounded` adds a half-width disc at each end, and
/// `Arrow` adds a chevron at the requested end. `style.width` is the line width in pixels.
pub fn stroke_line(
    document: &Document,
    layer_id: LayerId,
    points: &[Point],
    style: LineStyle,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let Some(first) = points.first().copied() else {
        return Ok(None);
    };
    let LineStyle {
        width,
        color,
        start_cap,
        end_cap,
        dash,
    } = style;
    let width = width.max(1);
    let half = f64::from(width) / 2.0;
    let arrow_length = (f64::from(width) * 3.0).max(6.0);
    let margin = if matches!(start_cap, LineCap::Arrow) || matches!(end_cap, LineCap::Arrow) {
        arrow_length.max(half).ceil() as i64 + 2
    } else {
        half.ceil() as i64 + 2
    };
    let mut min_x = i64::from(first.x);
    let mut min_y = i64::from(first.y);
    let mut max_x = min_x;
    let mut max_y = min_y;
    for point in points {
        min_x = min_x.min(i64::from(point.x));
        min_y = min_y.min(i64::from(point.y));
        max_x = max_x.max(i64::from(point.x));
        max_y = max_y.max(i64::from(point.y));
    }
    let Some(rect) = clip_rect(
        clip.rect,
        min_x - margin,
        min_y - margin,
        max_x + margin + 1,
        max_y + margin + 1,
    )?
    else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let target = color.unwrap_or(Bgra::TRANSPARENT);
    let left = rect.left();
    let top = rect.top();
    let rect_width = i64::from(rect.width());
    let rect_height = i64::from(rect.height());

    let mut segments = Vec::new();
    let mut total_length = 0.0_f64;
    let mut previous = first;
    for point in points.iter().copied().skip(1) {
        if point == previous {
            continue;
        }
        let ax = f64::from(previous.x) + 0.5;
        let ay = f64::from(previous.y) + 0.5;
        let bx = f64::from(point.x) + 0.5;
        let by = f64::from(point.y) + 0.5;
        let length = (bx - ax).hypot(by - ay);
        segments.push(LineSegment {
            ax,
            ay,
            bx,
            by,
            start: total_length,
            length,
        });
        total_length += length;
        previous = point;
    }
    let last = previous;

    let mut set = |x: i64, y: i64| {
        if x < left || x >= left + rect_width || y < top || y >= top + rect_height {
            return;
        }
        if !clip.selects_xy(x, y) {
            return;
        }
        after[((y - top) * rect_width + (x - left)) as usize] = target;
    };

    if segments.is_empty() {
        // A zero-length line is a single pixel, matching the existing hard round stroke.
        set(i64::from(first.x), i64::from(first.y));
    } else {
        for row in 0..rect_height {
            let center_y = (top + row) as f64 + 0.5;
            for column in 0..rect_width {
                let center_x = (left + column) as f64 + 0.5;
                let painted = segments.iter().any(|segment| {
                    let (distance, t, along) =
                        segment.distance_and_distance_along(center_x, center_y);
                    distance <= half && (0.0..=1.0).contains(&t) && dash_on(dash, along, width)
                });
                if painted {
                    set(left + column, top + row);
                }
            }
        }
    }

    if matches!(start_cap, LineCap::Rounded) {
        stamp_disc(
            f64::from(first.x) + 0.5,
            f64::from(first.y) + 0.5,
            half,
            &mut set,
        );
    }
    if matches!(end_cap, LineCap::Rounded) {
        stamp_disc(
            f64::from(last.x) + 0.5,
            f64::from(last.y) + 0.5,
            half,
            &mut set,
        );
    }
    if let (LineCap::Arrow, Some(segment)) = (start_cap, segments.first()) {
        let (dx, dy) = segment.unit_direction();
        // The start tip points away from the shaft, so its inward direction is +segment.
        stamp_arrow(
            (-dx, -dy),
            f64::from(first.x) + 0.5,
            f64::from(first.y) + 0.5,
            arrow_length,
            half,
            &mut set,
        );
    }
    if let (LineCap::Arrow, Some(segment)) = (end_cap, segments.last()) {
        let (dx, dy) = segment.unit_direction();
        stamp_arrow(
            (dx, dy),
            f64::from(last.x) + 0.5,
            f64::from(last.y) + 0.5,
            arrow_length,
            half,
            &mut set,
        );
    }

    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

struct LineSegment {
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    start: f64,
    length: f64,
}

impl LineSegment {
    fn distance_and_distance_along(&self, x: f64, y: f64) -> (f64, f64, f64) {
        let dx = self.bx - self.ax;
        let dy = self.by - self.ay;
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared == 0.0 {
            0.0
        } else {
            ((x - self.ax) * dx + (y - self.ay) * dy) / length_squared
        };
        let clamped = t.clamp(0.0, 1.0);
        let nearest_x = self.ax + clamped * dx;
        let nearest_y = self.ay + clamped * dy;
        (
            (x - nearest_x).hypot(y - nearest_y),
            t,
            self.start + clamped * self.length,
        )
    }

    fn unit_direction(&self) -> (f64, f64) {
        let dx = self.bx - self.ax;
        let dy = self.by - self.ay;
        let length = dx.hypot(dy);
        if length == 0.0 {
            (1.0, 0.0)
        } else {
            (dx / length, dy / length)
        }
    }
}

fn dash_on(dash: LineDash, distance: f64, width: u32) -> bool {
    let width = f64::from(width.max(1));
    let (on, off) = match dash {
        LineDash::Solid => return true,
        LineDash::Dash => (3.0 * width, width),
        LineDash::Dot => (width, width),
    };
    distance.rem_euclid(on + off) < on
}

fn stamp_disc(center_x: f64, center_y: f64, radius: f64, set: &mut impl FnMut(i64, i64)) {
    let left = (center_x - radius).floor() as i64;
    let right = (center_x + radius).ceil() as i64;
    let top = (center_y - radius).floor() as i64;
    let bottom = (center_y + radius).ceil() as i64;
    for y in top..=bottom {
        for x in left..=right {
            if (x as f64 + 0.5 - center_x).hypot(y as f64 + 0.5 - center_y) <= radius {
                set(x, y);
            }
        }
    }
}

fn stamp_arrow(
    outward: (f64, f64),
    tip_x: f64,
    tip_y: f64,
    length: f64,
    half_width: f64,
    set: &mut impl FnMut(i64, i64),
) {
    // The head points away from the shaft, so its edges run back into the shaft at +/-30
    // degrees from the inward direction.
    let inward = (-outward.0, -outward.1);
    let angle = std::f64::consts::FRAC_PI_6;
    for side in [-angle, angle] {
        let (sin, cos) = side.sin_cos();
        let dx = inward.0 * cos - inward.1 * sin;
        let dy = inward.0 * sin + inward.1 * cos;
        stamp_thick_segment(
            tip_x,
            tip_y,
            tip_x + dx * length,
            tip_y + dy * length,
            half_width,
            set,
        );
    }
}

fn stamp_thick_segment(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    half_width: f64,
    set: &mut impl FnMut(i64, i64),
) {
    let segment = LineSegment {
        ax,
        ay,
        bx,
        by,
        start: 0.0,
        length: (bx - ax).hypot(by - ay),
    };
    let left = (ax.min(bx) - half_width).floor() as i64;
    let right = (ax.max(bx) + half_width).ceil() as i64;
    let top = (ay.min(by) - half_width).floor() as i64;
    let bottom = (ay.max(by) + half_width).ceil() as i64;
    for y in top..=bottom {
        for x in left..=right {
            let (distance, t, _) =
                segment.distance_and_distance_along(x as f64 + 0.5, y as f64 + 0.5);
            if distance <= half_width && (0.0..=1.0).contains(&t) {
                set(x, y);
            }
        }
    }
}

/// Fills an ellipse inscribed in a half-open rectangle, clipped to the selection mask.
/// How a shape is painted: only its outline, only its interior, or both.
///
/// Paint.NET's `ShapeDrawType` uses the same three values. `Fill` covers the whole inscribed
/// shape so it includes the border pixels; `Both` is the union of the outline and the interior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeStyle {
    Outline,
    Fill,
    Both,
}

fn ellipse_contains(px: i64, py: i64, ellipse: Rect) -> bool {
    let cx = (ellipse.left() as f64 + ellipse.right() as f64) / 2.0;
    let cy = (ellipse.top() as f64 + ellipse.bottom() as f64) / 2.0;
    let rx = f64::from(ellipse.width()) / 2.0;
    let ry = f64::from(ellipse.height()) / 2.0;
    let nx = (px as f64 + 0.5 - cx) / rx;
    let ny = (py as f64 + 0.5 - cy) / ry;
    nx * nx + ny * ny <= 1.0
}

fn ellipse_outline(px: i64, py: i64, ellipse: Rect, border_width: u32) -> bool {
    if border_width == 0 || !ellipse_contains(px, py, ellipse) {
        return false;
    }
    let cx = (ellipse.left() as f64 + ellipse.right() as f64) / 2.0;
    let cy = (ellipse.top() as f64 + ellipse.bottom() as f64) / 2.0;
    let inner_rx = f64::from(ellipse.width()) / 2.0 - f64::from(border_width);
    let inner_ry = f64::from(ellipse.height()) / 2.0 - f64::from(border_width);
    if inner_rx <= 0.0 || inner_ry <= 0.0 {
        return true;
    }
    let nx = (px as f64 + 0.5 - cx) / inner_rx;
    let ny = (py as f64 + 0.5 - cy) / inner_ry;
    nx * nx + ny * ny > 1.0
}

fn rectangle_outline(px: i64, py: i64, rect: Rect, border_width: u32) -> bool {
    if border_width == 0 {
        return false;
    }
    let border = i64::from(border_width);
    px < rect.left() + border
        || px >= rect.right() - border
        || py < rect.top() + border
        || py >= rect.bottom() - border
}

fn shape_pixels(
    document: &Document,
    layer_id: LayerId,
    requested: Rect,
    color: Bgra,
    painted: impl Fn(i64, i64) -> bool,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let Some(rect) = intersect(clip.rect, requested)? else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = i64::from(rect.width());
    let mut changed = false;
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            let pixel_x = rect.left() + i64::from(x);
            let pixel_y = rect.top() + i64::from(y);
            if !clip.selects_xy(pixel_x, pixel_y) {
                continue;
            }
            if !painted(pixel_x, pixel_y) {
                continue;
            }
            let index = (i64::from(y) * width + i64::from(x)) as usize;
            if after[index] != color {
                after[index] = color;
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Paints a rectangle with an explicit outline/fill style and inside border width.
pub fn shape_rect(
    document: &Document,
    layer_id: LayerId,
    rect: Rect,
    color: Bgra,
    style: ShapeStyle,
    border_width: u32,
) -> Result<Option<PixelPatch>, RasterError> {
    shape_pixels(document, layer_id, rect, color, |px, py| match style {
        ShapeStyle::Outline => rectangle_outline(px, py, rect, border_width),
        ShapeStyle::Fill | ShapeStyle::Both => true,
    })
}

/// Paints an ellipse with an explicit outline/fill style and inside border width.
pub fn shape_ellipse(
    document: &Document,
    layer_id: LayerId,
    ellipse: Rect,
    color: Bgra,
    style: ShapeStyle,
    border_width: u32,
) -> Result<Option<PixelPatch>, RasterError> {
    shape_pixels(document, layer_id, ellipse, color, |px, py| match style {
        ShapeStyle::Outline => ellipse_outline(px, py, ellipse, border_width),
        ShapeStyle::Fill | ShapeStyle::Both => ellipse_contains(px, py, ellipse),
    })
}

/// Fills an ellipse inscribed in a half-open rectangle, clipped to the selection mask.
pub fn fill_ellipse(
    document: &Document,
    layer_id: LayerId,
    ellipse: Rect,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    shape_ellipse(document, layer_id, ellipse, color, ShapeStyle::Fill, 0)
}

pub(crate) fn clip_rect(
    operation: Rect,
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
) -> Result<Option<Rect>, RasterError> {
    let left = left.max(operation.left());
    let top = top.max(operation.top());
    let right = right.min(operation.right());
    let bottom = bottom.min(operation.bottom());
    if right <= left || bottom <= top {
        return Ok(None);
    }
    Ok(Some(Rect::new(left, top, right, bottom)?))
}

fn intersect(a: Rect, b: Rect) -> Result<Option<Rect>, RasterError> {
    clip_rect(a, b.left(), b.top(), b.right(), b.bottom())
}

/// Fills a half-open rectangle, clipped to the active selection mask.
pub fn fill_rect(
    document: &Document,
    layer_id: LayerId,
    rect: Rect,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    shape_rect(document, layer_id, rect, color, ShapeStyle::Fill, 0)
}

/// The gradient shape sampled across the drag, matching Paint.NET's five shape buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GradientType {
    Linear,
    Reflected,
    Diamond,
    Radial,
    Conical,
}

/// How far outside the drag range the gradient keeps sampling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GradientRepeat {
    /// Clamp to the from/to colours outside the drag.
    None,
    /// Wrap around so the gradient repeats beyond the drag.
    Repeat,
}

/// Gradient shape and repeat mode selected in Tool Options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GradientOptions {
    pub kind: GradientType,
    pub repeat: GradientRepeat,
}

impl GradientOptions {
    pub const fn new(kind: GradientType, repeat: GradientRepeat) -> Self {
        Self { kind, repeat }
    }
}

impl Default for GradientOptions {
    fn default() -> Self {
        Self {
            kind: GradientType::Linear,
            repeat: GradientRepeat::None,
        }
    }
}

fn gradient_lerp(
    kind: GradientType,
    repeat: GradientRepeat,
    start: Point,
    end: Point,
    pixel_x: f64,
    pixel_y: f64,
) -> f64 {
    let start_x = f64::from(start.x);
    let start_y = f64::from(start.y);
    let dx = f64::from(end.x) - start_x;
    let dy = f64::from(end.y) - start_y;
    let length_squared = dx * dx + dy * dy;
    let length = length_squared.sqrt();
    let offset_x = pixel_x - start_x;
    let offset_y = pixel_y - start_y;
    let raw = match kind {
        GradientType::Linear if length_squared == 0.0 => 0.0,
        GradientType::Linear => (offset_x * dx + offset_y * dy) / length_squared,
        GradientType::Reflected if length_squared == 0.0 => 0.0,
        GradientType::Reflected => ((offset_x * dx + offset_y * dy) / length_squared).abs(),
        GradientType::Diamond if length_squared == 0.0 => 0.0,
        GradientType::Diamond => {
            let along = (offset_x * dx + offset_y * dy) / length_squared;
            let across = (offset_x * dy - offset_y * dx) / length_squared;
            along.abs() + across.abs()
        }
        GradientType::Radial if length == 0.0 => 0.0,
        GradientType::Radial => offset_x.hypot(offset_y) / length,
        GradientType::Conical if length == 0.0 => 0.0,
        GradientType::Conical => {
            let direction = dy.atan2(dx);
            let angle = (offset_y.atan2(offset_x) - direction).rem_euclid(std::f64::consts::TAU);
            match repeat {
                GradientRepeat::None => angle / std::f64::consts::TAU,
                GradientRepeat::Repeat => angle / std::f64::consts::PI,
            }
        }
    };
    match repeat {
        GradientRepeat::None => raw.clamp(0.0, 1.0),
        GradientRepeat::Repeat => raw.rem_euclid(1.0),
    }
}

/// Paints a straight-alpha gradient with the selected shape and repeat mode.
pub fn gradient(
    document: &Document,
    layer_id: LayerId,
    start: Point,
    end: Point,
    from: Bgra,
    to: Bgra,
    options: GradientOptions,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = i64::from(rect.width());
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            let pixel_x = (rect.left() + i64::from(x)) as f64 + 0.5;
            let pixel_y = (rect.top() + i64::from(y)) as f64 + 0.5;
            if !clip.selects_xy(rect.left() + i64::from(x), rect.top() + i64::from(y)) {
                continue;
            }
            let t = gradient_lerp(options.kind, options.repeat, start, end, pixel_x, pixel_y);
            after[(i64::from(y) * width + i64::from(x)) as usize] = lerp_bgra(from, to, t);
        }
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Paints the default linear, non-repeating gradient between two canvas points.
pub fn linear_gradient(
    document: &Document,
    layer_id: LayerId,
    start: Point,
    end: Point,
    from: Bgra,
    to: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    gradient(
        document,
        layer_id,
        start,
        end,
        from,
        to,
        GradientOptions::default(),
    )
}

/// Replaces pixels close to `target` with `replacement` through the selection mask.
pub fn recolor(
    document: &Document,
    layer_id: LayerId,
    target: Bgra,
    replacement: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let tolerance = i16::from(tolerance);
    let width = i64::from(rect.width());
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            if !clip.selects_xy(rect.left() + i64::from(x), rect.top() + i64::from(y)) {
                continue;
            }
            let index = (i64::from(y) * width + i64::from(x)) as usize;
            if rgb_within_tolerance(before[index], target, tolerance) {
                after[index] = replacement;
            }
        }
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Translates the selection contents, or the complete layer without a selection.
/// The destination is clipped to the canvas; masked pixels retain their shape.
pub fn translate(
    document: &Document,
    layer_id: LayerId,
    dx: i32,
    dy: i32,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let rect = document.canvas_rect();
    let before = layer.surface().read_region(rect)?;
    let width = i64::from(rect.width());
    let height = i64::from(rect.height());
    let dx = i64::from(dx);
    let dy = i64::from(dy);
    let mut after = vec![Bgra::TRANSPARENT; before.len()];
    match document.selection_model() {
        Some(selection) => {
            after.copy_from_slice(&before);
            for y in 0..height {
                for x in 0..width {
                    let source_x = rect.left() + x;
                    let source_y = rect.top() + y;
                    if selection.contains_xy(source_x, source_y) {
                        after[(y * width + x) as usize] = Bgra::TRANSPARENT;
                    }
                }
            }
            for y in 0..height {
                for x in 0..width {
                    let source_x = rect.left() + x;
                    let source_y = rect.top() + y;
                    if !selection.contains_xy(source_x, source_y) {
                        continue;
                    }
                    let dest_x = source_x + dx;
                    let dest_y = source_y + dy;
                    if dest_x >= rect.left()
                        && dest_x < rect.right()
                        && dest_y >= rect.top()
                        && dest_y < rect.bottom()
                    {
                        let source = (y * width + x) as usize;
                        let dest =
                            ((dest_y - rect.top()) * width + (dest_x - rect.left())) as usize;
                        after[dest] = before[source];
                    }
                }
            }
        }
        None => {
            for y in 0..height {
                for x in 0..width {
                    let dest_x = rect.left() + x + dx;
                    let dest_y = rect.top() + y + dy;
                    if dest_x >= rect.left()
                        && dest_x < rect.right()
                        && dest_y >= rect.top()
                        && dest_y < rect.bottom()
                    {
                        after[((dest_y - rect.top()) * width + (dest_x - rect.left())) as usize] =
                            before[(y * width + x) as usize];
                    }
                }
            }
        }
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

fn lerp_bgra(from: Bgra, to: Bgra, t: f64) -> Bgra {
    let channel = |a: u8, b: u8| {
        (f64::from(a) + (f64::from(b) - f64::from(a)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Bgra::new(
        channel(from.b, to.b),
        channel(from.g, to.g),
        channel(from.r, to.r),
        channel(from.a, to.a),
    )
}

/// Fills the contiguous region around `start`, clipped to the active selection mask.
/// Unselected pixels inside the bounding rectangle act as barriers.
pub fn flood_fill(
    document: &Document,
    layer_id: LayerId,
    start: Point,
    color: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let width = i64::from(rect.width());
    let start_x = i64::from(start.x);
    let start_y = i64::from(start.y);
    if start_x < rect.left()
        || start_x >= rect.right()
        || start_y < rect.top()
        || start_y >= rect.bottom()
        || !clip.selects_xy(start_x, start_y)
    {
        return Ok(None);
    }
    let before = layer.surface().read_region(rect)?;
    let target = before[((start_y - rect.top()) * width + (start_x - rect.left())) as usize];
    if target == color {
        return Ok(None);
    }
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let mut visited = vec![false; before.len()];
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((start_x, start_y));
    let tolerance = i16::from(tolerance);
    let matches = |pixel: Bgra| bgra_within_tolerance(pixel, target, tolerance);
    while let Some((x, y)) = queue.pop_front() {
        if x < rect.left() || x >= rect.right() || y < rect.top() || y >= rect.bottom() {
            continue;
        }
        if !clip.selects_xy(x, y) {
            continue;
        }
        let index = ((y - rect.top()) * width + (x - rect.left())) as usize;
        if visited[index] || !matches(before[index]) {
            continue;
        }
        visited[index] = true;
        after[index] = color;
        queue.push_back((x + 1, y));
        if x > rect.left() {
            queue.push_back((x - 1, y));
        }
        queue.push_back((x, y + 1));
        if y > rect.top() {
            queue.push_back((x, y - 1));
        }
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Fills every matching pixel in the selection mask regardless of connectivity.
pub fn global_fill(
    document: &Document,
    layer_id: LayerId,
    point: Point,
    color: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let width = i64::from(rect.width());
    let start_x = i64::from(point.x);
    let start_y = i64::from(point.y);
    if start_x < rect.left()
        || start_x >= rect.right()
        || start_y < rect.top()
        || start_y >= rect.bottom()
        || !clip.selects_xy(start_x, start_y)
    {
        return Ok(None);
    }
    let before = layer.surface().read_region(rect)?;
    let target = before[((start_y - rect.top()) * width + (start_x - rect.left())) as usize];
    if target == color {
        return Ok(None);
    }
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let tolerance = i16::from(tolerance);
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            if !clip.selects_xy(rect.left() + i64::from(x), rect.top() + i64::from(y)) {
                continue;
            }
            let index = (i64::from(y) * width + i64::from(x)) as usize;
            if bgra_within_tolerance(before[index], target, tolerance) {
                after[index] = color;
            }
        }
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Recolors pixels near `target` only inside a round brush stroke and the selection mask.
///
/// The stroke honours `BrushOptions` hardness and spacing exactly like [`stroke`].
pub fn recolor_stroke(
    document: &Document,
    layer_id: LayerId,
    points: &[Point],
    brush: BrushOptions,
    target: Bgra,
    replacement: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let Some(first) = points.first() else {
        return Ok(None);
    };
    let brush = brush.normalized();
    let radius = brush.size.saturating_sub(1) / 2;
    let radius_f = f64::from(radius);
    let radius_i64 = i64::from(radius);
    let mut min_x = i64::from(first.x);
    let mut min_y = i64::from(first.y);
    let mut max_x = min_x;
    let mut max_y = min_y;
    for point in points {
        min_x = min_x.min(i64::from(point.x));
        min_y = min_y.min(i64::from(point.y));
        max_x = max_x.max(i64::from(point.x));
        max_y = max_y.max(i64::from(point.y));
    }
    let Some(rect) = clip_rect(
        clip.rect,
        min_x - radius_i64,
        min_y - radius_i64,
        max_x + radius_i64 + 1,
        max_y + radius_i64 + 1,
    )?
    else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let left = rect.left();
    let top = rect.top();
    let width = i64::from(rect.width());
    let height = i64::from(rect.height());
    let hardness = f64::from(brush.hardness) / 100.0;
    let step = f64::from(brush.size) * f64::from(brush.spacing) / 100.0;
    let tolerance = i16::from(tolerance);
    let matches = |pixel: Bgra| bgra_within_tolerance(pixel, target, tolerance);

    for_each_stamp(points, step, |center_x, center_y| {
        let x0 = (center_x - radius_f).floor() as i64;
        let x1 = (center_x + radius_f).ceil() as i64;
        let y0 = (center_y - radius_f).floor() as i64;
        let y1 = (center_y + radius_f).ceil() as i64;
        for py in y0.max(top)..=y1.min(top + height - 1) {
            for px in x0.max(left)..=x1.min(left + width - 1) {
                let dx = px as f64 - center_x;
                let dy = py as f64 - center_y;
                let distance = dx.hypot(dy);
                if distance > radius_f {
                    continue;
                }
                if !clip.selects_xy(px, py) {
                    continue;
                }
                let coverage = brush_coverage(distance, radius_f, hardness);
                if coverage == 0 {
                    continue;
                }
                let index = ((py - top) * width + (px - left)) as usize;
                if !matches(before[index]) {
                    continue;
                }
                if coverage == u8::MAX {
                    after[index] = replacement;
                } else {
                    let alpha =
                        (u16::from(replacement.a) * u16::from(coverage) + 127) / u16::from(u8::MAX);
                    let blended =
                        Bgra::new(replacement.b, replacement.g, replacement.r, alpha as u8);
                    after[index] = crate::normal_over(after[index], blended, u8::MAX);
                }
            }
        }
    });

    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Composites a detached floating surface onto a layer at `dest`, clipped to the canvas.
/// When a selection exists, destination pixels outside it are not modified.
pub fn stamp(
    document: &Document,
    layer_id: LayerId,
    surface: &paint_core::Surface,
    dest_x: i64,
    dest_y: i64,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let requested = Rect::new(
        dest_x,
        dest_y,
        dest_x + i64::from(surface.width()),
        dest_y + i64::from(surface.height()),
    )?;
    let Some(rect) = requested.clip_to(document.size()) else {
        return Ok(None);
    };
    let selection = document.selection_model();
    let before = layer.surface().read_region(rect)?;
    let width = i64::from(rect.width());
    let mut after = before.clone();
    for y in 0..i64::from(rect.height()) {
        for x in 0..width {
            let dest_pixel_x = rect.left() + x;
            let dest_pixel_y = rect.top() + y;
            if let Some(selection) = selection
                && !selection.contains_xy(dest_pixel_x, dest_pixel_y)
            {
                continue;
            }
            let source_x = dest_pixel_x - dest_x;
            let source_y = dest_pixel_y - dest_y;
            let source =
                surface.pixels()[(source_y * i64::from(surface.width()) + source_x) as usize];
            if source.a == 0 {
                continue;
            }
            let index = (y * width + x) as usize;
            let target = after[index];
            let source_a = u16::from(source.a);
            let target_a = u16::from(target.a);
            let inverse = 255 - source_a;
            let out_a = source_a + target_a * inverse / 255;
            if out_a == 0 {
                after[index] = Bgra::TRANSPARENT;
                continue;
            }
            let blend = |s: u8, t: u8| {
                ((u16::from(s) * source_a + u16::from(t) * target_a * inverse / 255) / out_a) as u8
            };
            after[index] = Bgra::new(
                blend(source.r, target.r),
                blend(source.g, target.g),
                blend(source.b, target.b),
                out_a as u8,
            );
        }
    }
    if after == before {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Which surface the magic wand samples when matching colors.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MagicWandSampling {
    /// Sample only the active layer, ignoring every other layer.
    #[default]
    ActiveLayer,
    /// Sample the composed image exactly as the canvas displays it.
    Image,
}

impl MagicWandSampling {
    pub const ALL: [Self; 2] = [Self::ActiveLayer, Self::Image];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ActiveLayer => "Active Layer",
            Self::Image => "Image",
        }
    }
}

/// Builds a magic-wand selection mask sampled from the active layer or the composed image.
///
/// Contiguous mode uses the same four-connected flood traversal and tolerance rule as
/// [`flood_fill`]; global mode selects every matching pixel regardless of connectivity. The mask
/// bounds are tightened to the selected pixels. `Ok(None)` means the point is outside the canvas
/// or no pixel matched.
pub fn magic_wand_mask(
    document: &Document,
    layer_id: LayerId,
    point: Point,
    tolerance: u8,
    global: bool,
    sampling: MagicWandSampling,
) -> Result<Option<SelectionMask>, RasterError> {
    let layer = layer(document, layer_id)?;
    let rect = document.canvas_rect();
    let width = i64::from(rect.width());
    let start_x = i64::from(point.x);
    let start_y = i64::from(point.y);
    if start_x < rect.left()
        || start_x >= rect.right()
        || start_y < rect.top()
        || start_y >= rect.bottom()
    {
        return Ok(None);
    }
    let composed;
    let pixels = match sampling {
        MagicWandSampling::ActiveLayer => layer.surface().read_region(rect)?,
        MagicWandSampling::Image => {
            composed = crate::compose(document)?;
            composed.read_region(rect)?
        }
    };
    let target = pixels[((start_y - rect.top()) * width + (start_x - rect.left())) as usize];
    let tolerance = i16::from(tolerance);
    let matches = |pixel: Bgra| bgra_within_tolerance(pixel, target, tolerance);
    let mut flags = vec![false; pixels.len()];
    if global {
        for (index, pixel) in pixels.iter().copied().enumerate() {
            flags[index] = matches(pixel);
        }
    } else {
        let mut visited = vec![false; pixels.len()];
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((start_x, start_y));
        while let Some((x, y)) = queue.pop_front() {
            if x < rect.left() || x >= rect.right() || y < rect.top() || y >= rect.bottom() {
                continue;
            }
            let index = ((y - rect.top()) * width + (x - rect.left())) as usize;
            if visited[index] || !matches(pixels[index]) {
                continue;
            }
            visited[index] = true;
            flags[index] = true;
            queue.push_back((x + 1, y));
            if x > rect.left() {
                queue.push_back((x - 1, y));
            }
            queue.push_back((x, y + 1));
            if y > rect.top() {
                queue.push_back((x, y - 1));
            }
        }
    }
    Ok(SelectionMask::from_canvas_flags(document.size(), &flags)?)
}

/// A whole-image rotation direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Rotate90Clockwise,
    Rotate90CounterClockwise,
    Rotate180,
}

impl Rotation {
    /// Returns the canvas size after this rotation. Quarter turns swap width and height.
    pub fn rotated_size(self, size: paint_core::Size) -> paint_core::Size {
        match self {
            Self::Rotate90Clockwise | Self::Rotate90CounterClockwise => {
                paint_core::Size::new(size.height(), size.width())
                    .expect("a swapped nonzero size is still nonzero")
            }
            Self::Rotate180 => size,
        }
    }
}

/// Copies `bounds` out of every layer as packed surfaces in layer order.
pub fn crop_surfaces(document: &Document, bounds: Rect) -> Result<Vec<Surface>, RasterError> {
    let requested_bytes = bounds.size().checked_buffer_len()?;
    let mut surfaces = Vec::new();
    surfaces
        .try_reserve_exact(document.layers().len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    for layer in document.layers() {
        let pixels = layer.surface().read_region(bounds)?;
        surfaces.push(Surface::from_pixels(bounds.size(), pixels)?);
    }
    Ok(surfaces)
}

/// Resamples every layer to `new_size` with bilinear pixel-centre sampling.
pub fn resize_surfaces(document: &Document, new_size: Size) -> Result<Vec<Surface>, RasterError> {
    let requested_bytes = new_size.checked_buffer_len()?;
    let mut surfaces = Vec::new();
    surfaces
        .try_reserve_exact(document.layers().len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    for layer in document.layers() {
        surfaces.push(resize_surface(layer.surface(), new_size)?);
    }
    Ok(surfaces)
}

fn resize_surface(source: &Surface, new_size: Size) -> Result<Surface, RasterError> {
    let old_width = source.width();
    let old_height = source.height();
    let pixel_count = new_size.checked_pixel_count()?;
    let requested_bytes = new_size.checked_buffer_len()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    let old_width_f = f64::from(old_width);
    let old_height_f = f64::from(old_height);
    let new_width_f = f64::from(new_size.width());
    let new_height_f = f64::from(new_size.height());
    let source_pixels = source.pixels();
    let source_stride = old_width as usize;

    for y in 0..new_size.height() {
        let source_y = (((f64::from(y) + 0.5) * old_height_f / new_height_f) - 0.5)
            .clamp(0.0, old_height_f - 1.0);
        let y0 = source_y.floor();
        let y1 = (y0 + 1.0).min(old_height_f - 1.0);
        let fy = source_y - y0;
        let y0 = y0 as usize;
        let y1 = y1 as usize;
        for x in 0..new_size.width() {
            let source_x = (((f64::from(x) + 0.5) * old_width_f / new_width_f) - 0.5)
                .clamp(0.0, old_width_f - 1.0);
            let x0 = source_x.floor();
            let x1 = (x0 + 1.0).min(old_width_f - 1.0);
            let fx = source_x - x0;
            let x0 = x0 as usize;
            let x1 = x1 as usize;
            let top = lerp_bgra(
                source_pixels[y0 * source_stride + x0],
                source_pixels[y0 * source_stride + x1],
                fx,
            );
            let bottom = lerp_bgra(
                source_pixels[y1 * source_stride + x0],
                source_pixels[y1 * source_stride + x1],
                fx,
            );
            pixels.push(lerp_bgra(top, bottom, fy));
        }
    }
    Ok(Surface::from_pixels(new_size, pixels)?)
}

/// Places every layer's existing pixels at `(offset_x, offset_y)` in a new canvas of
/// `new_size`, filling uncovered pixels with `fill` and clipping content outside the canvas.
pub fn canvas_surfaces(
    document: &Document,
    new_size: Size,
    offset_x: i64,
    offset_y: i64,
    fill: Bgra,
) -> Result<Vec<Surface>, RasterError> {
    let requested_bytes = new_size.checked_buffer_len()?;
    let mut surfaces = Vec::new();
    surfaces
        .try_reserve_exact(document.layers().len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    for layer in document.layers() {
        let mut surface = Surface::filled(new_size, fill)?;
        blit_at(layer.surface(), &mut surface, offset_x, offset_y);
        surfaces.push(surface);
    }
    Ok(surfaces)
}

fn blit_at(source: &Surface, destination: &mut Surface, offset_x: i64, offset_y: i64) {
    let destination_width = destination.width() as usize;
    let destination_height = i64::from(destination.height());
    let destination_width_i64 = i64::from(destination.width());
    let source_width = source.width() as usize;
    let source_pixels = source.pixels();
    let destination_pixels = destination.pixels_mut();
    for source_y in 0..source.height() {
        let destination_y = i64::from(source_y) + offset_y;
        if destination_y < 0 || destination_y >= destination_height {
            continue;
        }
        for source_x in 0..source.width() {
            let destination_x = i64::from(source_x) + offset_x;
            if destination_x < 0 || destination_x >= destination_width_i64 {
                continue;
            }
            let source_index = source_y as usize * source_width + source_x as usize;
            let destination_index =
                destination_y as usize * destination_width + destination_x as usize;
            destination_pixels[destination_index] = source_pixels[source_index];
        }
    }
}

/// Rotates every layer by the requested quarter or half turn.
pub fn rotate_surfaces(
    document: &Document,
    rotation: Rotation,
) -> Result<Vec<Surface>, RasterError> {
    let new_size = rotation.rotated_size(document.size());
    let requested_bytes = new_size.checked_buffer_len()?;
    let mut surfaces = Vec::new();
    surfaces
        .try_reserve_exact(document.layers().len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    for layer in document.layers() {
        surfaces.push(rotate_surface(layer.surface(), rotation, new_size)?);
    }
    Ok(surfaces)
}

fn rotate_surface(
    source: &Surface,
    rotation: Rotation,
    new_size: Size,
) -> Result<Surface, RasterError> {
    let old_width = source.width();
    let old_height = source.height();
    let pixel_count = new_size.checked_pixel_count()?;
    let requested_bytes = new_size.checked_buffer_len()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    let source_pixels = source.pixels();
    let source_width = old_width as usize;
    for y in 0..new_size.height() {
        for x in 0..new_size.width() {
            let (source_x, source_y) = match rotation {
                Rotation::Rotate90Clockwise => (y, old_height - 1 - x),
                Rotation::Rotate90CounterClockwise => (old_width - 1 - y, x),
                Rotation::Rotate180 => (old_width - 1 - x, old_height - 1 - y),
            };
            pixels.push(source_pixels[source_y as usize * source_width + source_x as usize]);
        }
    }
    Ok(Surface::from_pixels(new_size, pixels)?)
}

/// Clones pixels from `source` along a round brush stroke.
///
/// The first point anchors the stroke: every destination pixel samples
/// `source + (destination - points[0])`, so moving the stamp carries the source
/// along with it. `hardness` is a percentage: 100 is a hard edge and 0 fades
/// smoothly from the center to the brush radius. Sampling always reads the layer
/// before the stroke, and the resulting patch is clipped to the active selection.
pub fn clone_stamp(
    document: &Document,
    layer_id: LayerId,
    points: &[paint_core::Point],
    size: u32,
    source: paint_core::Point,
    hardness: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let Some(origin) = points.first().copied() else {
        return Ok(None);
    };
    let radius = size.max(1).saturating_sub(1) / 2;
    let radius_i64 = i64::from(radius);
    let mut min_x = i64::from(origin.x);
    let mut min_y = i64::from(origin.y);
    let mut max_x = min_x;
    let mut max_y = min_y;
    for point in points {
        min_x = min_x.min(i64::from(point.x));
        min_y = min_y.min(i64::from(point.y));
        max_x = max_x.max(i64::from(point.x));
        max_y = max_y.max(i64::from(point.y));
    }
    let Some(rect) = clip_rect(
        clip.rect,
        min_x - radius_i64,
        min_y - radius_i64,
        max_x + radius_i64 + 1,
        max_y + radius_i64 + 1,
    )?
    else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let left = rect.left();
    let top = rect.top();
    let width = i64::from(rect.width());
    let height = i64::from(rect.height());
    let source_x = i64::from(source.x);
    let source_y = i64::from(source.y);
    let origin_x = i64::from(origin.x);
    let origin_y = i64::from(origin.y);
    let radius_f = f64::from(radius);
    let hardness = f64::from(hardness.min(100)) / 100.0;
    let inner_radius = radius_f * hardness;

    let mut stamp = |center_x: i64, center_y: i64| {
        for py in (center_y - radius_i64)..=(center_y + radius_i64) {
            if py < top || py >= top + height {
                continue;
            }
            for px in (center_x - radius_i64)..=(center_x + radius_i64) {
                if px < left || px >= left + width {
                    continue;
                }
                if !clip.selects_xy(px, py) {
                    continue;
                }
                let dx = (px - center_x) as f64;
                let dy = (py - center_y) as f64;
                let distance = dx.hypot(dy);
                if radius > 0 && distance > radius_f {
                    continue;
                }
                let cov = if radius == 0 || distance <= inner_radius {
                    u8::MAX
                } else {
                    let t = (distance - inner_radius) / (radius_f - inner_radius);
                    let smooth = t * t * (3.0 - 2.0 * t);
                    ((1.0 - smooth) * 255.0).round() as u8
                };
                if cov == 0 {
                    continue;
                }
                let sample_x = source_x + (px - origin_x);
                let sample_y = source_y + (py - origin_y);
                if sample_x < 0 || sample_y < 0 {
                    continue;
                }
                let Ok(sample_x) = u32::try_from(sample_x) else {
                    continue;
                };
                let Ok(sample_y) = u32::try_from(sample_y) else {
                    continue;
                };
                let Some(sampled) = layer
                    .surface()
                    .get(paint_core::Point::new(sample_x, sample_y))
                else {
                    continue;
                };
                let alpha = ((u16::from(sampled.a) * u16::from(cov) + 127) / 255) as u8;
                if alpha == 0 {
                    continue;
                }
                let source_pixel = Bgra::new(sampled.b, sampled.g, sampled.r, alpha);
                let index = ((py - top) * width + (px - left)) as usize;
                after[index] = crate::normal_over(after[index], source_pixel, u8::MAX);
            }
        }
    };

    stamp(origin_x, origin_y);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let dx = i64::from(b.x) - i64::from(a.x);
        let dy = i64::from(b.y) - i64::from(a.y);
        let steps = dx.unsigned_abs().max(dy.unsigned_abs());
        for step in 1..=steps {
            let x = i64::from(a.x) + (dx * i64::from(step as u32)) / i64::from(steps as u32);
            let y = i64::from(a.y) + (dy * i64::from(step as u32)) / i64::from(steps as u32);
            stamp(x, y);
        }
    }

    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}
