use std::fs::File;
use std::io::Read;
use std::path::Path;

use paint_core::Surface;
use paint_document::{Document, Layer, LayerId};
use paint_image_io::{
    PngLimits, atomic_write, decode_png_with_limits, encode_png, write_png_atomic,
};

use crate::{AppSession, ResourceLimits, SessionError, TraceOutput};

impl AppSession {
    /// Decodes a flat PNG as a new one-layer document with a clean baseline.
    pub fn from_png_bytes(bytes: &[u8]) -> Result<Self, SessionError> {
        Self::from_png_bytes_with_limits(bytes, ResourceLimits::default())
    }

    /// Decodes with both PNG codec bounds and this session's dimension/pixel bounds.
    pub fn from_png_bytes_with_limits(
        bytes: &[u8],
        limits: ResourceLimits,
    ) -> Result<Self, SessionError> {
        let png_limits = png_limits_for(&limits);
        let surface = decode_png_with_limits(bytes, &png_limits)?;
        Self::from_decoded_png(surface, limits)
    }

    /// Opens a flat PNG from disk with bounded input and a clean session baseline.
    pub fn open_png(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        Self::open_png_with_limits(path, ResourceLimits::default())
    }

    pub fn open_png_with_limits(
        path: impl AsRef<Path>,
        limits: ResourceLimits,
    ) -> Result<Self, SessionError> {
        let path = path.as_ref();
        let png_limits = png_limits_for(&limits);
        let bytes = read_bounded(path, png_limits.max_input_bytes)?;
        let surface = decode_png_with_limits(&bytes, &png_limits)?;
        Self::from_decoded_png(surface, limits)
    }

    /// Encodes the current composite as a flat PNG export.
    ///
    /// This is not a lossless layered-document save: layers remain in this session, no layer is
    /// discarded, and exporting does not change the clean baseline or history.
    pub fn export_png(&self) -> Result<Vec<u8>, SessionError> {
        let composed = self.compose()?;
        Ok(encode_png(&composed)?)
    }

    /// Atomically replaces a flat PNG export without modifying session content or history.
    pub fn export_png_atomic(&self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        let composed = self.compose()?;
        write_png_atomic(path, &composed)?;
        Ok(())
    }

    fn from_decoded_png(surface: Surface, limits: ResourceLimits) -> Result<Self, SessionError> {
        let size = surface.size();
        let background = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
        let document = Document::from_layers(size, vec![background])?;
        Self::with_limits(document, limits)
    }
}

impl TraceOutput {
    /// Writes the protocol's `composite.png` and `snapshot.json`, replacing each file atomically.
    ///
    /// The two files have separate atomic visibility transitions; this method does not claim that
    /// the directory or the pair is updated as one filesystem transaction.
    pub fn write_to_directory(&self, directory: impl AsRef<Path>) -> Result<(), SessionError> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory).map_err(|source| SessionError::Io {
            action: "create output directory",
            path: directory.to_path_buf(),
            source,
        })?;
        let snapshot = serde_json::to_vec_pretty(&self.snapshot)?;
        write_png_atomic(directory.join("composite.png"), &self.surface)?;
        atomic_write(directory.join("snapshot.json"), &snapshot)?;
        Ok(())
    }
}

fn png_limits_for(limits: &ResourceLimits) -> PngLimits {
    let mut png = PngLimits::default();
    png.max_width = png.max_width.min(limits.max_width);
    png.max_height = png.max_height.min(limits.max_height);
    png.max_pixels = png
        .max_pixels
        .min(usize::try_from(limits.max_pixels).unwrap_or(usize::MAX));
    let decoded_bytes = png.max_pixels.saturating_mul(4);
    png.max_decoded_bytes = png.max_decoded_bytes.min(decoded_bytes);
    png
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, SessionError> {
    let file = File::open(path).map_err(|source| SessionError::Io {
        action: "open PNG",
        path: path.to_path_buf(),
        source,
    })?;
    let maximum_u64 = u64::try_from(maximum).unwrap_or(u64::MAX);
    let mut reader = file.take(maximum_u64.saturating_add(1));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| SessionError::Io {
            action: "read PNG",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > maximum {
        return Err(paint_image_io::PngError::ResourceLimit {
            resource: "input bytes",
            requested: bytes.len() as u64,
            maximum,
        }
        .into());
    }
    Ok(bytes)
}
