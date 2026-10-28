use paint_core::{Bgra, Rect};
use paint_document::{Document, DocumentError, Layer, LayerId, PixelPatch};

use crate::RasterError;

/// Produces a solid-fill patch for the active selection, clipped to the canvas.
/// Returns `Ok(None)` for an empty clip or when every selected pixel already has `color`.
pub fn fill(
    document: &Document,
    layer_id: LayerId,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(rect) = operation_rect(document) else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    if before.iter().all(|pixel| *pixel == color) {
        return Ok(None);
    }
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    after.fill(color);
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
/// Returns `Ok(None)` only when the selection does not intersect the canvas.
pub fn invert(document: &Document, layer_id: LayerId) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(rect) = operation_rect(document) else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    for pixel in &mut after {
        *pixel = pixel.inverted_rgb();
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

fn layer(document: &Document, layer_id: LayerId) -> Result<&Layer, RasterError> {
    document
        .layer(layer_id)
        .ok_or(DocumentError::LayerNotFound { layer_id })
        .map_err(RasterError::from)
}

fn operation_rect(document: &Document) -> Option<Rect> {
    match document.selection() {
        Some(selection) => selection.clip_to(document.size()),
        None => Some(document.canvas_rect()),
    }
}

fn copy_pixels(pixels: &[Bgra], requested_bytes: usize) -> Result<Vec<Bgra>, RasterError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(pixels.len())
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    copy.extend_from_slice(pixels);
    Ok(copy)
}

/// Paints a round, hard-edged stroke through canvas-space points.
/// `color = None` erases to transparent. The patch is clipped to the active selection.
pub fn stroke(
    document: &Document,
    layer_id: LayerId,
    points: &[paint_core::Point],
    size: u32,
    color: Option<Bgra>,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(operation) = operation_rect(document) else {
        return Ok(None);
    };
    let Some(first) = points.first() else {
        return Ok(None);
    };
    let radius = size.max(1).saturating_sub(1) / 2;
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
        operation,
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
    let target = color.unwrap_or(Bgra::TRANSPARENT);
    let left = rect.left();
    let top = rect.top();
    let width = i64::from(rect.width());
    let radius_squared = i128::from(radius) * i128::from(radius);

    let mut stamp = |x: i64, y: i64| {
        for py in (y - radius_i64)..=(y + radius_i64) {
            if py < top || py >= top + i64::from(rect.height()) {
                continue;
            }
            for px in (x - radius_i64)..=(x + radius_i64) {
                if px < left || px >= left + width {
                    continue;
                }
                let dx = i128::from(px - x);
                let dy = i128::from(py - y);
                if dx * dx + dy * dy > radius_squared {
                    continue;
                }
                let index = ((py - top) * width + (px - left)) as usize;
                after[index] = target;
            }
        }
    };

    stamp(i64::from(first.x), i64::from(first.y));
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

/// Fills an ellipse inscribed in a half-open rectangle, clipped to the selection.
pub fn fill_ellipse(
    document: &Document,
    layer_id: LayerId,
    ellipse: Rect,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(operation) = operation_rect(document) else {
        return Ok(None);
    };
    let Some(rect) = intersect(operation, ellipse)? else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let cx = (ellipse.left() as f64 + ellipse.right() as f64) / 2.0;
    let cy = (ellipse.top() as f64 + ellipse.bottom() as f64) / 2.0;
    let rx = f64::from(ellipse.width()) / 2.0;
    let ry = f64::from(ellipse.height()) / 2.0;
    let width = i64::from(rect.width());
    for y in 0..rect.height() {
        let py = (rect.top() + i64::from(y)) as f64 + 0.5;
        for x in 0..rect.width() {
            let px = (rect.left() + i64::from(x)) as f64 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            if nx * nx + ny * ny <= 1.0 {
                after[(i64::from(y) * width + i64::from(x)) as usize] = color;
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

fn clip_rect(
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

/// Fills a half-open rectangle, clipped to the active selection.
pub fn fill_rect(
    document: &Document,
    layer_id: LayerId,
    rect: Rect,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(operation) = operation_rect(document) else {
        return Ok(None);
    };
    let Some(rect) = intersect(operation, rect)? else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    if before.iter().all(|pixel| *pixel == color) {
        return Ok(None);
    }
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    after.fill(color);
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Paints a straight-alpha linear gradient from `start` to `end` through the selection.
pub fn linear_gradient(
    document: &Document,
    layer_id: LayerId,
    start: paint_core::Point,
    end: paint_core::Point,
    from: Bgra,
    to: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(rect) = operation_rect(document) else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let dx = f64::from(end.x) - f64::from(start.x);
    let dy = f64::from(end.y) - f64::from(start.y);
    let length_squared = dx * dx + dy * dy;
    let width = i64::from(rect.width());
    for y in 0..rect.height() {
        for x in 0..rect.width() {
            let px = (rect.left() + i64::from(x)) as f64 + 0.5;
            let py = (rect.top() + i64::from(y)) as f64 + 0.5;
            let t = if length_squared == 0.0 {
                0.0
            } else {
                (((px - f64::from(start.x)) * dx + (py - f64::from(start.y)) * dy) / length_squared)
                    .clamp(0.0, 1.0)
            };
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

/// Replaces pixels close to `target` with `replacement` through the selection.
pub fn recolor(
    document: &Document,
    layer_id: LayerId,
    target: Bgra,
    replacement: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(rect) = operation_rect(document) else {
        return Ok(None);
    };
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let tolerance = i16::from(tolerance);
    for (before_pixel, after_pixel) in before.iter().zip(&mut after) {
        let close = (i16::from(before_pixel.r) - i16::from(target.r)).abs() <= tolerance
            && (i16::from(before_pixel.g) - i16::from(target.g)).abs() <= tolerance
            && (i16::from(before_pixel.b) - i16::from(target.b)).abs() <= tolerance;
        if close {
            *after_pixel = replacement;
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
    match document.selection() {
        Some(selection) => {
            after.copy_from_slice(&before);
            for y in 0..height {
                for x in 0..width {
                    let source_x = rect.left() + x;
                    let source_y = rect.top() + y;
                    if source_x >= selection.left()
                        && source_x < selection.right()
                        && source_y >= selection.top()
                        && source_y < selection.bottom()
                    {
                        after[(y * width + x) as usize] = Bgra::TRANSPARENT;
                    }
                }
            }
            for y in 0..height {
                for x in 0..width {
                    let source_x = rect.left() + x;
                    let source_y = rect.top() + y;
                    if source_x < selection.left()
                        || source_x >= selection.right()
                        || source_y < selection.top()
                        || source_y >= selection.bottom()
                    {
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

/// Fills the contiguous region around `start`, clipped to the active selection.
pub fn flood_fill(
    document: &Document,
    layer_id: LayerId,
    start: paint_core::Point,
    color: Bgra,
    tolerance: u8,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(rect) = operation_rect(document) else {
        return Ok(None);
    };
    let width = i64::from(rect.width());
    let start_x = i64::from(start.x);
    let start_y = i64::from(start.y);
    if start_x < rect.left()
        || start_x >= rect.right()
        || start_y < rect.top()
        || start_y >= rect.bottom()
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
    let matches = |pixel: Bgra| {
        (i16::from(pixel.b) - i16::from(target.b)).abs() <= tolerance
            && (i16::from(pixel.g) - i16::from(target.g)).abs() <= tolerance
            && (i16::from(pixel.r) - i16::from(target.r)).abs() <= tolerance
            && (i16::from(pixel.a) - i16::from(target.a)).abs() <= tolerance
    };
    while let Some((x, y)) = queue.pop_front() {
        if x < rect.left() || x >= rect.right() || y < rect.top() || y >= rect.bottom() {
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
