use paint_core::{Bgra, Surface};
use paint_document::{BlendMode, Document};

use crate::RasterError;

/// Composites visible layers in bottom-to-top order onto transparent black.
///
/// Background state is metadata and does not alter blending. Invisible layers and
/// zero-opacity layers have no effect. Multiply, Screen, and Overlay use the backdrop
/// channels and then composite the blended color with the layer alpha/opacity.
pub fn compose(document: &Document) -> Result<Surface, RasterError> {
    let mut output = Surface::new(document.size())?;
    for layer in document.layers() {
        if !layer.is_visible() || layer.opacity() == 0 {
            continue;
        }
        for (destination, source) in output.pixels_mut().iter_mut().zip(layer.surface().pixels()) {
            *destination = blend_over(*destination, *source, layer.opacity(), layer.blend_mode());
        }
    }
    Ok(output)
}

fn blend_over(destination: Bgra, source: Bgra, opacity: u8, mode: BlendMode) -> Bgra {
    if mode == BlendMode::Normal {
        return normal_over(destination, source, opacity);
    }
    let blended = Bgra::new(
        blend_channel(mode, source.b, destination.b),
        blend_channel(mode, source.g, destination.g),
        blend_channel(mode, source.r, destination.r),
        source.a,
    );
    normal_over(destination, blended, opacity)
}

fn blend_channel(mode: BlendMode, source: u8, backdrop: u8) -> u8 {
    let source = u32::from(source);
    let backdrop = u32::from(backdrop);
    let value = match mode {
        BlendMode::Normal => source,
        BlendMode::Multiply => (source * backdrop + 127) / 255,
        BlendMode::Screen => 255 - ((255 - source) * (255 - backdrop) + 127) / 255,
        BlendMode::Overlay => {
            if backdrop < 128 {
                (2 * source * backdrop + 127) / 255
            } else {
                255 - (2 * (255 - source) * (255 - backdrop) + 127) / 255
            }
        }
    };
    value.min(255) as u8
}

/// Deterministic straight-alpha source-over with an additional layer-opacity multiplier.
///
/// Alpha products are rounded to the nearest integer with `(value + 127) / 255`. If the
/// effective source alpha is `As`, the retained destination alpha is
/// `round(Ad * (255 - As) / 255)`. The output color is the rounded premultiplied sum divided
/// by output alpha, converted back to straight alpha. Transparent-source and zero-opacity
/// operations preserve the destination exactly, including RGB values under zero alpha.
pub fn normal_over(destination: Bgra, source: Bgra, layer_opacity: u8) -> Bgra {
    let source_alpha = multiply_255(source.a, layer_opacity);
    if source_alpha == 0 {
        return destination;
    }
    if destination.a == 0 {
        return Bgra::new(source.b, source.g, source.r, source_alpha);
    }
    if source_alpha == u8::MAX {
        return Bgra::new(source.b, source.g, source.r, u8::MAX);
    }

    let destination_alpha = multiply_255(destination.a, u8::MAX - source_alpha);
    let output_alpha = u32::from(source_alpha) + u32::from(destination_alpha);
    let source_alpha = u32::from(source_alpha);
    let destination_alpha = u32::from(destination_alpha);
    let channel = |source: u8, destination: u8| {
        rounded_divide(
            u32::from(source) * source_alpha + u32::from(destination) * destination_alpha,
            output_alpha,
        )
    };
    Bgra::new(
        channel(source.b, destination.b),
        channel(source.g, destination.g),
        channel(source.r, destination.r),
        output_alpha as u8,
    )
}

fn multiply_255(left: u8, right: u8) -> u8 {
    ((u32::from(left) * u32::from(right) + 127) / 255) as u8
}

fn rounded_divide(numerator: u32, denominator: u32) -> u8 {
    debug_assert!(denominator > 0);
    let result = (numerator + denominator / 2) / denominator;
    debug_assert!(result <= u32::from(u8::MAX));
    result as u8
}
