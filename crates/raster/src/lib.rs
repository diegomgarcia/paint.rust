#![doc = "Deterministic scalar fills, effects, and straight-alpha Normal composition."]

mod compose;
mod effects;
mod operations;
mod text;

use paint_core::{GeometryError, SurfaceError};
use paint_document::{BlendMode, DocumentError, LayerId, PatchError, SelectionError};
use thiserror::Error;

pub use compose::{blend_tool_color, compose, normal_over};
pub use effects::{
    DEFAULT_LEVELS_GAMMA_HUNDREDTHS, LevelsChannel, LevelsSettings, MAX_ADJUSTMENT_VALUE,
    MAX_BLACK_AND_WHITE_WEIGHT, MAX_GAUSSIAN_BLUR_RADIUS, MAX_HUE_DELTA,
    MAX_LEVELS_GAMMA_HUNDREDTHS, MAX_MOTION_BLUR_DISTANCE, MAX_NOISE_INTENSITY,
    MAX_POSTERIZE_LEVELS, MAX_SHARPEN_AMOUNT, MIN_ADJUSTMENT_VALUE, MIN_HUE_DELTA,
    MIN_LEVELS_GAMMA_HUNDREDTHS, MIN_POSTERIZE_LEVELS, add_noise, add_noise_active, auto_level,
    auto_level_active, black_and_white, black_and_white_active, brightness_contrast,
    brightness_contrast_active, gaussian_blur, gaussian_blur_active, grayscale, grayscale_active,
    hue_saturation, hue_saturation_active, levels, levels_active, motion_blur, motion_blur_active,
    posterize, posterize_active, sepia, sepia_active, sharpen, sharpen_active, threshold,
    threshold_active,
};
pub use operations::{
    BrushOptions, GradientOptions, GradientRepeat, GradientType, LineCap, LineDash, LineStyle,
    MagicWandSampling, RecolorOptions, ResampleMode, Rotation, ShapeStyle, canvas_surfaces,
    clone_stamp, crop_surfaces, fill, fill_active, fill_ellipse, fill_rect, flip_horizontal,
    flip_vertical, flood_fill, flood_fill_sampled, global_fill, global_fill_sampled, gradient,
    invert, invert_active, linear_gradient, magic_wand_mask, recolor, recolor_stroke,
    recolor_stroke_sampled, resize_surfaces, rotate_surfaces, shape_ellipse, shape_rect, stamp,
    stamp_resampled, stroke, stroke_line, translate, translate_resampled,
};
pub use text::{MAX_TEXT_BYTES, MAX_TEXT_SIZE, TextAlignment, TextFontFamily, TextStyle, text};

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
    #[error("motion blur distance {distance} exceeds the maximum of {max}")]
    MotionBlurDistanceTooLarge { distance: u32, max: u32 },
    #[error("sharpen amount {amount} exceeds the maximum of {max}")]
    SharpenAmountTooLarge { amount: u32, max: u32 },
    #[error("noise intensity {intensity} exceeds the maximum of {max}")]
    NoiseIntensityTooLarge { intensity: u32, max: u32 },
    #[error("font {name} is unavailable")]
    FontUnavailable { name: &'static str },
    #[error("text is {actual} bytes, exceeding the {max}-byte limit")]
    TextTooLong { actual: usize, max: usize },
    #[error("font size {size} px is outside the supported 1..={max} px range")]
    TextSizeOutOfRange { size: u32, max: u32 },
    #[error("resampling mode {mode:?} requires a scale and is not supported by Move")]
    UnsupportedResampleMode { mode: ResampleMode },
    #[error("resampling offset must be finite")]
    NonFiniteResampleOffset,
}
