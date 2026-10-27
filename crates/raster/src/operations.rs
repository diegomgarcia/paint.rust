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
