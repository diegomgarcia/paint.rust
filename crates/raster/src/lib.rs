#![doc = "Deterministic scalar fill, Invert, and straight-alpha Normal composition."]

mod compose;
mod operations;

use paint_core::{GeometryError, SurfaceError};
use paint_document::{BlendMode, DocumentError, LayerId, PatchError};
use thiserror::Error;

pub use compose::{compose, normal_over};
pub use operations::{
    fill, fill_active, fill_ellipse, fill_rect, flip_horizontal, flip_vertical, flood_fill, invert,
    invert_active, linear_gradient, recolor, stroke, translate,
};

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
    #[error("failed to allocate {requested_bytes} bytes for a raster patch")]
    AllocationFailed { requested_bytes: usize },
}
