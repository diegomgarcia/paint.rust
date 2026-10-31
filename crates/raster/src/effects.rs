use paint_core::Bgra;
use paint_document::{Document, LayerId, PixelPatch};

use crate::RasterError;
use crate::operations::{copy_pixels, layer, operation_clip};

/// Largest accepted Gaussian blur radius. A radius of zero is a no-op.
pub const MAX_GAUSSIAN_BLUR_RADIUS: u32 = 200;

const FIXED_POINT_SHIFT: u32 = 16;
const FIXED_POINT_ONE: u32 = 1 << FIXED_POINT_SHIFT;
const FIXED_POINT_SCALE: u64 = FIXED_POINT_ONE as u64;
const FIXED_POINT_ROUND_U32: u32 = FIXED_POINT_ONE / 2;

/// Converts the active layer's selection (or whole canvas) to Rec. 601 luminance.
pub fn grayscale_active(document: &Document) -> Result<Option<PixelPatch>, RasterError> {
    grayscale(document, document.active_layer_id())
}

/// Converts the given layer's selection (or whole canvas) to Rec. 601 luminance.
///
/// Alpha is preserved exactly. Returns `Ok(None)` when no selected pixel changes.
pub fn grayscale(
    document: &Document,
    layer_id: LayerId,
) -> Result<Option<PixelPatch>, RasterError> {
    map_selection(document, layer_id, grayscale_pixel)
}

/// Applies a standard sepia color matrix to the active layer's selection.
pub fn sepia_active(document: &Document) -> Result<Option<PixelPatch>, RasterError> {
    sepia(document, document.active_layer_id())
}

/// Applies a standard sepia color matrix to the given layer's selection.
///
/// Alpha is preserved exactly. Returns `Ok(None)` when no selected pixel changes.
pub fn sepia(document: &Document, layer_id: LayerId) -> Result<Option<PixelPatch>, RasterError> {
    map_selection(document, layer_id, sepia_pixel)
}

/// Applies a deterministic separable Gaussian blur to the active layer's selection.
pub fn gaussian_blur_active(
    document: &Document,
    radius: u32,
) -> Result<Option<PixelPatch>, RasterError> {
    gaussian_blur(document, document.active_layer_id(), radius)
}

/// Applies a deterministic separable Gaussian blur to the given layer's selection.
///
/// Only pixels inside the operation rectangle are replaced. The kernel is evaluated on
/// premultiplied alpha and the result is converted back to straight alpha, so transparent
/// neighbors do not create dark halos. Sampling clamps to the canvas edge. A radius of zero
/// returns `Ok(None)`.
pub fn gaussian_blur(
    document: &Document,
    layer_id: LayerId,
    radius: u32,
) -> Result<Option<PixelPatch>, RasterError> {
    if radius > MAX_GAUSSIAN_BLUR_RADIUS {
        return Err(RasterError::BlurRadiusTooLarge {
            radius,
            max: MAX_GAUSSIAN_BLUR_RADIUS,
        });
    }
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    if radius == 0 {
        return Ok(None);
    }

    let canvas = document.canvas_rect();
    let source = layer.surface().read_region(canvas)?;
    let width = canvas.width() as usize;
    let height = canvas.height() as usize;
    let weights = gaussian_weights(radius);
    let total_weight = weights.iter().map(|weight| u64::from(*weight)).sum::<u64>();
    debug_assert!(total_weight > 0);

    let premultiplied = premultiply(&source);
    let horizontal = convolve_rows(&premultiplied, width, height, &weights, total_weight);
    let blurred = convolve_columns(&horizontal, width, height, &weights, total_weight);

    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let rect_left = (rect.left() - canvas.left()) as usize;
    let rect_top = (rect.top() - canvas.top()) as usize;
    let rect_width = rect.width() as usize;

    for row in 0..rect.height() as usize {
        for column in 0..rect_width {
            let destination_x = rect.left() + column as i64;
            let destination_y = rect.top() + row as i64;
            if !clip.selects_xy(destination_x, destination_y) {
                continue;
            }
            let pixel = blurred[(rect_top + row) * width + rect_left + column];
            let alpha = ((pixel[3] + FIXED_POINT_ROUND_U32) >> FIXED_POINT_SHIFT).min(255) as u8;
            after[row * rect_width + column] = Bgra::new(
                channel(pixel[0], alpha),
                channel(pixel[1], alpha),
                channel(pixel[2], alpha),
                alpha,
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

fn map_selection<F>(
    document: &Document,
    layer_id: LayerId,
    transform: F,
) -> Result<Option<PixelPatch>, RasterError>
where
    F: Fn(Bgra) -> Bgra,
{
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let width = rect.width() as usize;
    for (index, (target, source)) in after.iter_mut().zip(&before).enumerate() {
        let x = rect.left() + (index % width) as i64;
        let y = rect.top() + (index / width) as i64;
        if clip.selects_xy(x, y) {
            *target = transform(*source);
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

fn grayscale_pixel(pixel: Bgra) -> Bgra {
    // Rec. 601 weights in 16-bit fixed point: 0.299 R, 0.587 G, 0.114 B.
    let luminance =
        19_595 * u32::from(pixel.r) + 38_470 * u32::from(pixel.g) + 7_471 * u32::from(pixel.b);
    let luminance = ((luminance + 32_768) >> 16) as u8;
    Bgra::new(luminance, luminance, luminance, pixel.a)
}

fn sepia_pixel(pixel: Bgra) -> Bgra {
    // Standard sepia matrix with thousandth fixed-point coefficients.
    let red = u32::from(pixel.r);
    let green = u32::from(pixel.g);
    let blue = u32::from(pixel.b);
    let channel = |value: u32| ((value + 500) / 1000).min(255) as u8;
    Bgra::new(
        channel(272 * red + 534 * green + 131 * blue),
        channel(349 * red + 686 * green + 168 * blue),
        channel(393 * red + 769 * green + 189 * blue),
        pixel.a,
    )
}

fn premultiply(pixels: &[Bgra]) -> Vec<[u32; 4]> {
    let mut premultiplied = Vec::with_capacity(pixels.len());
    for pixel in pixels {
        let alpha = u32::from(pixel.a);
        let channel = |value: u8| ((u32::from(value) * alpha + 127) / 255) << FIXED_POINT_SHIFT;
        premultiplied.push([
            channel(pixel.b),
            channel(pixel.g),
            channel(pixel.r),
            alpha << FIXED_POINT_SHIFT,
        ]);
    }
    premultiplied
}

fn convolve_rows(
    source: &[[u32; 4]],
    width: usize,
    height: usize,
    weights: &[u32],
    total_weight: u64,
) -> Vec<[u32; 4]> {
    let mut output = vec![[0u32; 4]; source.len()];
    let radius = weights.len() / 2;
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            let mut accumulator = [0u64; 4];
            let first = x as isize - radius as isize;
            for (offset, weight) in weights.iter().enumerate() {
                let sample_x = (first + offset as isize).clamp(0, width as isize - 1) as usize;
                let sample = source[row + sample_x];
                let weight = u64::from(*weight);
                for channel in 0..4 {
                    accumulator[channel] += weight * u64::from(sample[channel]);
                }
            }
            output[row + x] = divide(accumulator, total_weight);
        }
    }
    output
}

fn convolve_columns(
    source: &[[u32; 4]],
    width: usize,
    height: usize,
    weights: &[u32],
    total_weight: u64,
) -> Vec<[u32; 4]> {
    let mut output = vec![[0u32; 4]; source.len()];
    let radius = weights.len() / 2;
    for y in 0..height {
        let row = y * width;
        let first = y as isize - radius as isize;
        for x in 0..width {
            let mut accumulator = [0u64; 4];
            for (offset, weight) in weights.iter().enumerate() {
                let sample_y = (first + offset as isize).clamp(0, height as isize - 1) as usize;
                let sample = source[sample_y * width + x];
                let weight = u64::from(*weight);
                for channel in 0..4 {
                    accumulator[channel] += weight * u64::from(sample[channel]);
                }
            }
            output[row + x] = divide(accumulator, total_weight);
        }
    }
    output
}

fn divide(accumulator: [u64; 4], total_weight: u64) -> [u32; 4] {
    let mut output = [0u32; 4];
    for channel in 0..4 {
        output[channel] = ((accumulator[channel] + total_weight / 2) / total_weight) as u32;
    }
    output
}

fn channel(premultiplied: u32, alpha: u8) -> u8 {
    if alpha == 0 {
        return 0;
    }
    let premultiplied = (premultiplied + FIXED_POINT_ROUND_U32) >> FIXED_POINT_SHIFT;
    ((u64::from(premultiplied) * 255 + u64::from(alpha) / 2) / u64::from(alpha)) as u8
}

fn gaussian_weights(radius: u32) -> Vec<u32> {
    let radius = radius as usize;
    let sigma = radius as f64 / 2.0;
    let denominator = 2.0 * sigma * sigma;
    let mut weights = Vec::with_capacity(radius * 2 + 1);
    let mut sum = 0.0;
    for offset in 0..=radius * 2 {
        let distance = offset as f64 - radius as f64;
        let weight = (-(distance * distance) / denominator).exp();
        weights.push(weight);
        sum += weight;
    }
    let mut fixed: Vec<u32> = weights
        .into_iter()
        .map(|weight| (weight / sum * FIXED_POINT_SCALE as f64).round() as u32)
        .collect();
    if fixed.iter().all(|weight| *weight == 0) {
        fixed[radius] = 1;
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::gaussian_weights;

    #[test]
    fn gaussian_weights_are_symmetric_positive_and_peak_at_the_center() {
        for radius in 1..=8 {
            let weights = gaussian_weights(radius);
            assert_eq!(weights.len(), radius as usize * 2 + 1);
            assert!(weights.iter().all(|weight| *weight > 0));
            for offset in 0..weights.len() / 2 {
                assert_eq!(weights[offset], weights[weights.len() - 1 - offset]);
                assert!(weights[offset] < weights[offset + 1]);
            }
        }
    }
}
