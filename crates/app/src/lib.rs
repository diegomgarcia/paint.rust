#![doc = "Single-writer application sessions, traces, snapshots, and inverse-based history."]

mod clipboard;
mod command;
mod effect;
mod error;
mod persistence;
mod session;
mod snapshot;
mod trace;

pub use clipboard::ClipboardContent;
pub use command::{CanvasAnchor, Command};
pub use effect::{Effect, EffectCategory};
pub use error::{ExpectationError, ExpectationField, ResourceLimitError, SessionError, TraceError};
pub use paint_document::SelectionCombineMode;
pub use paint_image_io::{DecodeError, RasterFormat, RasterLimits};
pub use paint_raster::{MagicWandSampling, ResampleMode};
pub use session::{AppSession, FloatingSelection, ResourceLimits};
pub use snapshot::{
    LayerSnapshot, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, SelectionSnapshot, SemanticSnapshot,
};
pub use trace::{
    CommandTrace, Expectations, InitialDocument, MAX_TRACE_DIMENSION, TRACE_FORMAT, TRACE_VERSION,
    TraceOutput, execute_trace, execute_trace_with_limits,
};
