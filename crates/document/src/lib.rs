#![doc = "Ordered layers, rectangle selection, revisions, and atomic validated pixel patches."]

mod document;
mod layer;
mod patch;

pub use document::{Document, DocumentError};
pub use layer::{BlendMode, Layer, LayerId, Revision};
pub use patch::{PatchError, PixelPatch};
