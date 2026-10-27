#![doc = "Checked geometry, explicit BGRA pixels, and contiguous CPU surfaces."]

mod geometry;
mod pixel;
mod surface;

pub use geometry::{GeometryError, Point, Rect, Size};
pub use pixel::{Bgra, Rgba};
pub use surface::{Surface, SurfaceError};
