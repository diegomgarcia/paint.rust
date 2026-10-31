#![doc = "Deterministic scalar fills, effects, and straight-alpha Normal composition."]

mod compose;
mod effects;
mod operations;
mod text;

use paint_core::{GeometryError, SurfaceError};
use paint_document::{BlendMode, DocumentError, LayerId, PatchError, SelectionError};
use thiserror::Error;

pub use compose::{compose, normal_over};
pub use effects::{
    MAX_ADJUSTMENT_VALUE, MAX_GAUSSIAN_BLUR_RADIUS, MAX_HUE_DELTA, MIN_ADJUSTMENT_VALUE,
    MIN_HUE_DELTA, auto_level, auto_level_active, brightness_contrast, brightness_contrast_active,
    gaussian_blur, gaussian_blur_active, grayscale, grayscale_active, hue_saturation,
    hue_saturation_active, sepia, sepia_active,
};
pub use operations::{
    BrushOptions, GradientOptions, GradientRepeat, GradientType, LineCap, LineDash, LineStyle,
    MagicWandSampling, Rotation, ShapeStyle, canvas_surfaces, clone_stamp, crop_surfaces, fill,
    fill_active, fill_ellipse, fill_rect, flip_horizontal, flip_vertical, flood_fill, global_fill,
    gradient, invert, invert_active, linear_gradient, magic_wand_mask, recolor, recolor_stroke,
    resize_surfaces, rotate_surfaces, shape_ellipse, shape_rect, stamp, stroke, stroke_line,
    translate,
};
pub use text::{MAX_TEXT_BYTES, MAX_TEXT_SIZE, TextFontFamily, TextStyle, text};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RasterError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Patch(#[from] PatchError),
    #[error(
        "blend mode {mode:?} on layer {layer_id:?} is not implemented; only Normal is supported"
    )]
    UnsupportedBlendMode { layer_id: LayerId, mode: BlendMode },
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error("failed to allocate {requested_bytes} bytes for a raster patch")]
    AllocationFailed { requested_bytes: usize },
    #[error("Gaussian blur radius {radius} exceeds the maximum of {max}")]
    BlurRadiusTooLarge { radius: u32, max: u32 },
    #[error("font {name} is unavailable")]
    FontUnavailable { name: &'static str },
    #[error("text is {actual} bytes, exceeding the {max}-byte limit")]
    TextTooLong { actual: usize, max: usize },
    #[error("font size {size} px is outside the supported 1..={max} px range")]
    TextSizeOutOfRange { size: u32, max: u32 },
}
