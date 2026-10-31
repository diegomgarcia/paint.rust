use paint_core::Bgra;
use paint_document::{Document, LayerId, PixelPatch};

use crate::RasterError;
use crate::operations::{copy_pixels, layer, operation_clip};

/// Largest accepted Gaussian blur radius. A radius of zero is a no-op.
pub const MAX_GAUSSIAN_BLUR_RADIUS: u32 = 200;

/// Smallest accepted brightness, contrast, saturation, or lightness delta.
pub const MIN_ADJUSTMENT_VALUE: i32 = -100;
/// Largest accepted brightness, contrast, saturation, or lightness delta.
pub const MAX_ADJUSTMENT_VALUE: i32 = 100;
/// Smallest accepted hue rotation in degrees.
pub const MIN_HUE_DELTA: i32 = -180;
/// Largest accepted hue rotation in degrees.
pub const MAX_HUE_DELTA: i32 = 180;

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

/// Applies a brightness/contrast adjustment to the active layer's selection.
pub fn brightness_contrast_active(
    document: &Document,
    brightness: i32,
    contrast: i32,
) -> Result<Option<PixelPatch>, RasterError> {
    brightness_contrast(document, document.active_layer_id(), brightness, contrast)
}

/// Applies a brightness/contrast adjustment to the given layer's selection.
///
/// Brightness is added to each RGB channel in 0..=255 units before contrast is applied
/// around the 127.5 midpoint. Contrast values below zero flatten toward mid-gray, while
/// `+100` becomes a hard 128 threshold. Alpha is preserved exactly. Both deltas are
/// clamped to `MIN_ADJUSTMENT_VALUE..=MAX_ADJUSTMENT_VALUE`. Returns `Ok(None)` when no
/// selected pixel changes.
pub fn brightness_contrast(
    document: &Document,
    layer_id: LayerId,
    brightness: i32,
    contrast: i32,
) -> Result<Option<PixelPatch>, RasterError> {
    let brightness = brightness.clamp(MIN_ADJUSTMENT_VALUE, MAX_ADJUSTMENT_VALUE);
    let contrast = contrast.clamp(MIN_ADJUSTMENT_VALUE, MAX_ADJUSTMENT_VALUE);
    map_selection(document, layer_id, |pixel| {
        brightness_contrast_pixel(pixel, brightness, contrast)
    })
}

/// Applies a hue/saturation/lightness adjustment to the active layer's selection.
pub fn hue_saturation_active(
    document: &Document,
    hue: i32,
    saturation: i32,
    lightness: i32,
) -> Result<Option<PixelPatch>, RasterError> {
    hue_saturation(
        document,
        document.active_layer_id(),
        hue,
        saturation,
        lightness,
    )
}

/// Applies a real HSL hue/saturation/lightness adjustment to the given layer's selection.
///
/// Hue is rotated by `hue` degrees, saturation is multiplied by `1 + saturation / 100`
/// (so `-100` desaturates completely), and lightness is interpolated toward black or
/// white. Every value is clamped to its accepted range and alpha is preserved exactly.
/// Returns `Ok(None)` when the adjustment is the identity or no selected pixel changes.
pub fn hue_saturation(
    document: &Document,
    layer_id: LayerId,
    hue: i32,
    saturation: i32,
    lightness: i32,
) -> Result<Option<PixelPatch>, RasterError> {
    let hue = hue.clamp(MIN_HUE_DELTA, MAX_HUE_DELTA);
    let saturation = saturation.clamp(MIN_ADJUSTMENT_VALUE, MAX_ADJUSTMENT_VALUE);
    let lightness = lightness.clamp(MIN_ADJUSTMENT_VALUE, MAX_ADJUSTMENT_VALUE);
    map_selection(document, layer_id, |pixel| {
        hue_saturation_pixel(pixel, hue, saturation, lightness)
    })
}

/// Stretches the active layer's selected pixels to the full 0..=255 per-channel range.
pub fn auto_level_active(document: &Document) -> Result<Option<PixelPatch>, RasterError> {
    auto_level(document, document.active_layer_id())
}

/// Stretches the given layer's selected pixels to the full 0..=255 per-channel range.
///
/// The black and white points come from the opaque pixels inside the selection (or the
/// whole canvas without a selection). Each channel with a nonzero range is mapped linearly
/// so its minimum becomes zero and its maximum becomes 255; flat channels are unchanged.
/// Fully transparent pixels are ignored and preserved exactly. A selection with no possible
/// stretch is an atomic no-op that returns `Ok(None)`.
pub fn auto_level(
    document: &Document,
    layer_id: LayerId,
) -> Result<Option<PixelPatch>, RasterError> {
    let layer = layer(document, layer_id)?;
    let Some(clip) = operation_clip(document) else {
        return Ok(None);
    };
    let rect = clip.rect;
    let before = layer.surface().read_region(rect)?;
    let width = rect.width() as usize;
    let mut minimums = [u8::MAX; 3];
    let mut maximums = [u8::MIN; 3];
    let mut any_opaque = false;

    for (index, source) in before.iter().enumerate() {
        let x = rect.left() + (index % width) as i64;
        let y = rect.top() + (index / width) as i64;
        if source.a == 0 || !clip.selects_xy(x, y) {
            continue;
        }
        any_opaque = true;
        for (channel, value) in rgb_channels(*source).into_iter().enumerate() {
            minimums[channel] = minimums[channel].min(value);
            maximums[channel] = maximums[channel].max(value);
        }
    }
    if !any_opaque || (0..3).all(|channel| maximums[channel] <= minimums[channel]) {
        return Ok(None);
    }

    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    let mut changed = false;
    for (index, target) in after.iter_mut().enumerate() {
        let x = rect.left() + (index % width) as i64;
        let y = rect.top() + (index / width) as i64;
        let source = before[index];
        if source.a == 0 || !clip.selects_xy(x, y) {
            continue;
        }
        let mut channels = rgb_channels(source);
        for (channel, value) in channels.iter_mut().enumerate() {
            if maximums[channel] > minimums[channel] {
                *value = stretch_channel(*value, minimums[channel], maximums[channel]);
            }
        }
        let stretched = pixel_with_rgb(source, channels);
        if stretched != source {
            *target = stretched;
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

fn rgb_channels(pixel: Bgra) -> [u8; 3] {
    [pixel.r, pixel.g, pixel.b]
}

fn pixel_with_rgb(pixel: Bgra, [red, green, blue]: [u8; 3]) -> Bgra {
    Bgra::new(blue, green, red, pixel.a)
}

fn brightness_contrast_pixel(pixel: Bgra, brightness: i32, contrast: i32) -> Bgra {
    let channel = |value: u8| brightness_contrast_channel(value, brightness, contrast);
    Bgra::new(
        channel(pixel.b),
        channel(pixel.g),
        channel(pixel.r),
        pixel.a,
    )
}

fn brightness_contrast_channel(value: u8, brightness: i32, contrast: i32) -> u8 {
    let brightened = (i32::from(value) + brightness).clamp(0, 255);
    if contrast == 0 {
        return brightened as u8;
    }
    if contrast >= MAX_ADJUSTMENT_VALUE {
        return if brightened < 128 { 0 } else { 255 };
    }
    if contrast <= MIN_ADJUSTMENT_VALUE {
        return 128;
    }
    let factor = if contrast > 0 {
        100.0 / f64::from(100 - contrast)
    } else {
        f64::from(100 + contrast) / 100.0
    };
    ((f64::from(brightened) - 127.5) * factor + 127.5)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn hue_saturation_pixel(pixel: Bgra, hue: i32, saturation: i32, lightness: i32) -> Bgra {
    if hue == 0 && saturation == 0 && lightness == 0 {
        return pixel;
    }
    let (base_hue, base_saturation, base_lightness) = rgb_to_hsl(pixel);
    let hue = (base_hue + f64::from(hue)).rem_euclid(360.0);
    let saturation = (base_saturation * (1.0 + f64::from(saturation) / 100.0)).clamp(0.0, 1.0);
    let lightness = if lightness < 0 {
        base_lightness * (1.0 + f64::from(lightness) / 100.0)
    } else {
        base_lightness + (1.0 - base_lightness) * f64::from(lightness) / 100.0
    };
    pixel_with_rgb(pixel, hsl_to_rgb(hue, saturation, lightness))
}

/// Converts a straight-alpha pixel to HSL with hue in degrees and s/l in `0..=1`.
fn rgb_to_hsl(pixel: Bgra) -> (f64, f64, f64) {
    let red = f64::from(pixel.r) / 255.0;
    let green = f64::from(pixel.g) / 255.0;
    let blue = f64::from(pixel.b) / 255.0;
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let lightness = (maximum + minimum) / 2.0;
    let delta = maximum - minimum;
    if delta <= f64::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = if lightness > 0.5 {
        delta / (2.0 - maximum - minimum)
    } else {
        delta / (maximum + minimum)
    };
    let hue = if maximum == red {
        ((green - blue) / delta) * 60.0
    } else if maximum == green {
        ((blue - red) / delta + 2.0) * 60.0
    } else {
        ((red - green) / delta + 4.0) * 60.0
    };
    (hue.rem_euclid(360.0), saturation, lightness)
}

/// Converts HSL back to rounded straight RGB bytes.
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [u8; 3] {
    if saturation <= f64::EPSILON {
        let value = unit_to_byte(lightness);
        return [value, value, value];
    }
    let high = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let low = 2.0 * lightness - high;
    let fraction = hue / 360.0;
    let channel = |offset: f64| unit_to_byte(hue_channel(low, high, fraction + offset));
    [channel(1.0 / 3.0), channel(0.0), channel(-1.0 / 3.0)]
}

fn hue_channel(low: f64, high: f64, position: f64) -> f64 {
    let position = position.rem_euclid(1.0);
    if position < 1.0 / 6.0 {
        low + (high - low) * 6.0 * position
    } else if position < 0.5 {
        high
    } else if position < 2.0 / 3.0 {
        low + (high - low) * (2.0 / 3.0 - position) * 6.0
    } else {
        low
    }
}

fn unit_to_byte(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn stretch_channel(value: u8, minimum: u8, maximum: u8) -> u8 {
    let range = u32::from(maximum) - u32::from(minimum);
    debug_assert!(range > 0);
    let offset = u32::from(value) - u32::from(minimum);
    ((offset * 255 + range / 2) / range) as u8
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
