#![doc = "Ordered layers, rectangle or pixel-mask selections, revisions, and atomic validated pixel patches."]

mod document;
mod layer;
mod patch;
mod selection;

pub use document::{Document, DocumentError};
pub use layer::{BlendMode, Layer, LayerId, Revision};
pub use patch::{PatchError, PixelPatch};
pub use selection::{Selection, SelectionCombineMode, SelectionError, SelectionMask};
