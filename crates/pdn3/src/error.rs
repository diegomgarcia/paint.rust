use paint_core::{GeometryError, SurfaceError};
use paint_document::DocumentError;
use thiserror::Error;

use crate::nrbf::NrbfError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PdnUnsupported {
    Class(String),
    ChunkFormat(u8),
    BlendMode(i32),
    BlendOperation(String),
    NonBitmapLayer,
    SurfaceLayout(&'static str),
    SharedSurfaceWithoutDecodedParent,
}

#[derive(Debug, Error)]
pub enum PdnError {
    #[error("invalid PDN3 magic; this codec accepts only the PDN3 container")]
    InvalidMagic,
    #[error("truncated PDN at byte {offset}: {context}")]
    Truncated {
        offset: usize,
        context: &'static str,
    },
    #[error("malformed PDN at byte {offset}: {reason}")]
    Malformed { offset: usize, reason: &'static str },
    #[error("PDN {resource} limit exceeded: requested {requested}, maximum {maximum}")]
    ResourceLimit {
        resource: &'static str,
        requested: u64,
        maximum: usize,
    },
    #[error("failed to allocate {requested} bytes for PDN {context}")]
    Allocation {
        context: &'static str,
        requested: usize,
    },
    #[error("unsupported PDN feature: {0:?}")]
    Unsupported(PdnUnsupported),
    #[error("PDN XML is not valid UTF-8")]
    XmlUtf8,
    #[error("PDN XML error: {0}")]
    Xml(String),
    #[error(transparent)]
    Nrbf(#[from] NrbfError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Compression(#[from] flate2::DecompressError),
    #[error("could not read compressed PDN chunk: {0}")]
    CompressionRead(std::io::Error),
    #[error("could not write compressed PDN chunk: {0}")]
    CompressionWrite(#[from] std::io::Error),
}
