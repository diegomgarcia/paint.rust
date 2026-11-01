use paint_core::{GeometryError, SurfaceError};
use paint_document::{DocumentError, SelectionError};
use paint_raster::RasterError;
use thiserror::Error;

/// A checked resource bound rejected before an unbounded allocation or command loop.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResourceLimitError {
    #[error("trace is {actual} bytes; the configured maximum is {max}")]
    TraceBytes { actual: usize, max: usize },
    #[error("dimensions {width}x{height} exceed the configured maximum {max_width}x{max_height}")]
    Dimensions {
        width: u32,
        height: u32,
        max_width: u32,
        max_height: u32,
    },
    #[error("document has {actual} pixels; the configured maximum is {max}")]
    Pixels { actual: u64, max: u64 },
    #[error("layer count {actual} exceeds the configured maximum {max}")]
    Layers { actual: usize, max: usize },
    #[error("command count {actual} exceeds the configured maximum {max}")]
    Commands { actual: usize, max: usize },
    #[error("layer name is {actual} UTF-8 bytes; the configured maximum is {max}")]
    LayerNameBytes { actual: usize, max: usize },
}

/// A failed session action. The session guarantees failure atomicity for document and history.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Raster(#[from] RasterError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error(transparent)]
    Png(#[from] paint_image_io::PngError),
    #[error(transparent)]
    Decode(#[from] paint_image_io::DecodeError),
    #[error(transparent)]
    Format(#[from] paint_image_io::FormatError),
    #[error(transparent)]
    Pdn(#[from] paint_pdn3::PdnError),
    #[error(transparent)]
    Pdr(#[from] crate::pdr::PdrError),
    #[error(transparent)]
    AtomicWrite(#[from] paint_image_io::AtomicWriteError),
    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize semantic snapshot: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    ResourceLimit(#[from] ResourceLimitError),
    #[error("layer names must contain at least one character")]
    EmptyLayerName,
    #[error("history invariant failed: {0}")]
    HistoryInvariant(&'static str),
    #[error("document invariant failed: {0}")]
    DocumentInvariant(&'static str),
}

/// Semantic snapshot fields supported by trace expectations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectationField {
    Width,
    Height,
    DecodedRgbaSha256,
    LayerCount,
    ActiveLayerIndex,
    Dirty,
    CanUndo,
    CanRedo,
}

impl std::fmt::Display for ExpectationField {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Width => "width",
            Self::Height => "height",
            Self::DecodedRgbaSha256 => "decoded_rgba_sha256",
            Self::LayerCount => "layer_count",
            Self::ActiveLayerIndex => "active_layer_index",
            Self::Dirty => "dirty",
            Self::CanUndo => "can_undo",
            Self::CanRedo => "can_redo",
        };
        formatter.write_str(name)
    }
}

/// One exact mismatch between a trace expectation and the computed semantic snapshot.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("expectation {field} mismatch: expected {expected}, got {actual}")]
pub struct ExpectationError {
    pub field: ExpectationField,
    pub expected: String,
    pub actual: String,
}

/// Versioned trace parsing, validation, command execution, or expectation failure.
#[derive(Debug, Error)]
pub enum TraceError {
    #[error("invalid trace JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid trace command at index {index}: {source}")]
    InvalidCommand {
        index: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported trace format {format:?}")]
    UnsupportedFormat { format: String },
    #[error("unsupported trace version {version}")]
    UnsupportedVersion { version: u32 },
    #[error("initial dimensions must be nonzero, got {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("invalid rectangle in trace command {index}: {source}")]
    InvalidRectangle {
        index: usize,
        #[source]
        source: GeometryError,
    },
    #[error(
        "trace rectangle in command {index} must have nonnegative left/top and positive right/bottom, got ({left}, {top})..({right}, {bottom})"
    )]
    InvalidRectangleCoordinates {
        index: usize,
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
    },
    #[error("layer name in trace command {index} must not be empty")]
    EmptyLayerName { index: usize },
    #[error("invalid trace expectation: {message}")]
    InvalidExpectation { message: String },
    #[error("failed to allocate a command buffer for {commands} commands")]
    CommandAllocation { commands: usize },
    #[error(transparent)]
    ResourceLimit(#[from] ResourceLimitError),
    #[error("resource limit in trace command {index}: {source}")]
    CommandResourceLimit {
        index: usize,
        #[source]
        source: ResourceLimitError,
    },
    #[error("trace command {index} failed: {source}")]
    Command {
        index: usize,
        #[source]
        source: SessionError,
    },
    #[error("trace initialization or composition failed: {0}")]
    Session(#[from] SessionError),
    #[error(transparent)]
    Expectation(#[from] ExpectationError),
}

impl TraceError {
    /// Returns the zero-based command index for command-specific failures.
    pub const fn command_index(&self) -> Option<usize> {
        match self {
            Self::InvalidCommand { index, .. }
            | Self::InvalidRectangle { index, .. }
            | Self::InvalidRectangleCoordinates { index, .. }
            | Self::EmptyLayerName { index }
            | Self::CommandResourceLimit { index, .. }
            | Self::Command { index, .. } => Some(*index),
            _ => None,
        }
    }
}
