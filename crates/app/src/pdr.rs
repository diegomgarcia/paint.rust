//! Versioned, bounded `.pdr` (Paint.Rust Document) container.
//!
//! A PDR file stores the complete editable session: one current document, every undo and redo
//! state, and optional UI history labels. Documents are stored losslessly with ordered layers,
//! all blend modes, active layer, exact revisions, and rectangle or packed-mask selections.
//!
//! The container is deliberately simple and self-describing:
//!
//! ```text
//! magic "PAINTRDR" | version u32 | flags u32 | body length u64 | body | SHA-256
//! ```
//!
//! The checksum covers everything before it, so random corruption is rejected before structural
//! decoding. Decoding also bounds the input, dimensions, layer counts, history entries, selection
//! mask words, label sizes, and total decoded surface bytes. Pixels are raw little-endian BGRA.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use paint_core::{GeometryError, Rect, Size, Surface, SurfaceError};
use paint_document::{
    BlendMode, Document, DocumentError, Layer, LayerId, Revision, Selection, SelectionError,
    SelectionMask,
};
use paint_image_io::atomic_write;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AppSession, ResourceLimitError, ResourceLimits, SessionError, session::validate_document_shape,
};

/// The eight-byte PDR container magic.
pub const PDR_MAGIC: [u8; 8] = *b"PAINTRDR";
/// The only PDR container version this build reads and writes.
pub const PDR_VERSION: u32 = 1;

const MAGIC_LEN: usize = PDR_MAGIC.len();
const PREFIX_LEN: usize = MAGIC_LEN + 4 + 4 + 8;
const CHECKSUM_LEN: usize = 32;
const FLAGS: u32 = 0;
const LAYER_VISIBLE: u8 = 1 << 0;
const LAYER_BACKGROUND: u8 = 1 << 1;
const LAYER_KNOWN_FLAGS: u8 = LAYER_VISIBLE | LAYER_BACKGROUND;
const SELECTION_NONE: u8 = 0;
const SELECTION_RECTANGLE: u8 = 1;
const SELECTION_MASK: u8 = 2;
const LABELS_ABSENT: u8 = 0;
const LABELS_PRESENT: u8 = 1;

/// Resource bounds applied while encoding or decoding a PDR container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdrLimits {
    /// Maximum file size read from disk.
    pub max_input_bytes: u64,
    /// Maximum encoded container size produced in memory.
    pub max_encoded_bytes: u64,
    /// Maximum combined undo and redo entries.
    pub max_history_entries: usize,
    /// Maximum sum of every document's layer surface bytes after decoding.
    pub max_decoded_surface_bytes: u64,
    /// Maximum UTF-8 byte length of one layer name.
    pub max_layer_name_bytes: usize,
    /// Maximum UTF-8 byte length of one history label.
    pub max_label_bytes: usize,
    /// Maximum packed 64-bit words in one selection mask.
    pub max_mask_words: usize,
}

impl Default for PdrLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_encoded_bytes: 512 * 1024 * 1024,
            max_history_entries: 10_000,
            max_decoded_surface_bytes: 1024 * 1024 * 1024,
            max_layer_name_bytes: 4 * 1024,
            max_label_bytes: 4 * 1024,
            max_mask_words: 64 * 1024 * 1024,
        }
    }
}

/// Optional UI history labels stored alongside undo and redo entries.
///
/// Both vectors use history stack order (bottom to top), exactly like the undo and redo stacks.
/// Labels are payload only: the session never interprets or rewrites them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PdrHistoryLabels {
    pub undo: Vec<String>,
    pub redo: Vec<String>,
}

/// A typed PDR format failure.
#[derive(Debug, Error)]
pub enum PdrError {
    #[error("invalid PDR magic; expected PAINTRDR")]
    InvalidMagic,
    #[error("unsupported PDR version {version}; this build supports version {supported}")]
    UnsupportedVersion { version: u32, supported: u32 },
    #[error("truncated PDR at byte {offset}: {context}")]
    Truncated {
        offset: usize,
        context: &'static str,
    },
    #[error("malformed PDR at byte {offset}: {reason}")]
    Malformed { offset: usize, reason: &'static str },
    #[error("PDR checksum mismatch")]
    ChecksumMismatch,
    #[error("PDR {resource} limit exceeded: requested {requested}, maximum {maximum}")]
    ResourceLimit {
        resource: &'static str,
        requested: u64,
        maximum: u64,
    },
    #[error("failed to allocate {requested_bytes} bytes for PDR {context}")]
    Allocation {
        context: &'static str,
        requested_bytes: usize,
    },
    #[error("invalid UTF-8 in PDR {field}")]
    InvalidUtf8 { field: &'static str },
    #[error("unknown PDR {field} tag {tag}")]
    UnknownTag { field: &'static str, tag: u8 },
    #[error("PDR {stack} label count {actual} does not match {expected} history entries")]
    LabelCount {
        stack: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("cannot save a PDR file while a live preview is active")]
    PreviewActive,
    #[error(transparent)]
    DocumentLimit(#[from] ResourceLimitError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

impl AppSession {
    /// Encodes the complete session into a PDR container using default limits.
    pub fn encode_pdr(&self, labels: Option<&PdrHistoryLabels>) -> Result<Vec<u8>, SessionError> {
        self.encode_pdr_with_limits(labels, &PdrLimits::default())
    }

    /// Atomically saves the complete session and marks the current content clean.
    ///
    /// The current document, both history stacks, and every snapshot are written before the
    /// atomic replacement. A failed encode or write leaves the previous file, the clean baseline,
    /// the document, and both live history stacks unchanged.
    pub fn save_pdr_atomic(&mut self, path: impl AsRef<Path>) -> Result<(), SessionError> {
        self.save_pdr_atomic_with_labels(path, None)
    }

    /// Atomically saves the complete session plus optional UI history labels.
    pub fn save_pdr_atomic_with_labels(
        &mut self,
        path: impl AsRef<Path>,
        labels: Option<&PdrHistoryLabels>,
    ) -> Result<(), SessionError> {
        let bytes = self.encode_pdr(labels)?;
        atomic_write(path, &bytes)?;
        self.mark_clean();
        Ok(())
    }

    /// Opens a PDR session with default document and format limits.
    pub fn open_pdr(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        Self::open_pdr_with_limits(path, ResourceLimits::default())
    }

    /// Opens a PDR session, rejecting dimensions, pixels, or layer counts above `limits`.
    pub fn open_pdr_with_limits(
        path: impl AsRef<Path>,
        limits: ResourceLimits,
    ) -> Result<Self, SessionError> {
        let (session, _labels) =
            Self::open_pdr_with_limits_and_labels(path, limits, PdrLimits::default())?;
        Ok(session)
    }

    /// Opens a PDR session and returns optional stored UI history labels.
    pub fn open_pdr_with_limits_and_labels(
        path: impl AsRef<Path>,
        limits: ResourceLimits,
        pdr_limits: PdrLimits,
    ) -> Result<(Self, Option<PdrHistoryLabels>), SessionError> {
        let path = path.as_ref();
        let bytes = read_bounded(path, pdr_limits.max_input_bytes)?;
        let decoded = decode_container(&bytes, &limits, &pdr_limits).map_err(session_error)?;
        let session =
            Self::from_loaded_history(decoded.document, decoded.undo, decoded.redo, limits)?;
        Ok((session, decoded.labels))
    }

    fn encode_pdr_with_limits(
        &self,
        labels: Option<&PdrHistoryLabels>,
        limits: &PdrLimits,
    ) -> Result<Vec<u8>, SessionError> {
        if self.has_preview() {
            return Err(PdrError::PreviewActive.into());
        }
        if let Some(labels) = labels {
            validate_labels(labels, self.history_lengths(), limits)?;
        }
        let (undo, redo) = self.history_snapshots()?;
        Ok(encode_container(
            self.document(),
            &undo,
            &redo,
            labels,
            limits,
        )?)
    }
}

fn session_error(error: PdrError) -> SessionError {
    match error {
        PdrError::DocumentLimit(error) => SessionError::ResourceLimit(error),
        error => SessionError::Pdr(error),
    }
}

fn validate_labels(
    labels: &PdrHistoryLabels,
    expected: (usize, usize),
    limits: &PdrLimits,
) -> Result<(), PdrError> {
    if labels.undo.len() != expected.0 {
        return Err(PdrError::LabelCount {
            stack: "undo",
            expected: expected.0,
            actual: labels.undo.len(),
        });
    }
    if labels.redo.len() != expected.1 {
        return Err(PdrError::LabelCount {
            stack: "redo",
            expected: expected.1,
            actual: labels.redo.len(),
        });
    }
    for label in labels.undo.iter().chain(&labels.redo) {
        if label.len() > limits.max_label_bytes {
            return Err(PdrError::ResourceLimit {
                resource: "history label bytes",
                requested: u64::try_from(label.len()).unwrap_or(u64::MAX),
                maximum: u64::try_from(limits.max_label_bytes).unwrap_or(u64::MAX),
            });
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, SessionError> {
    let file = File::open(path).map_err(|source| SessionError::Io {
        action: "open PDR",
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = file.take(maximum.saturating_add(1));
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| SessionError::Io {
            action: "read PDR",
            path: path.to_path_buf(),
            source,
        })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > maximum {
        return Err(PdrError::ResourceLimit {
            resource: "input bytes",
            requested: actual,
            maximum,
        }
        .into());
    }
    Ok(bytes)
}

fn encode_container(
    document: &Document,
    undo: &[Document],
    redo: &[Document],
    labels: Option<&PdrHistoryLabels>,
    limits: &PdrLimits,
) -> Result<Vec<u8>, PdrError> {
    let history_entries = undo
        .len()
        .checked_add(redo.len())
        .ok_or(PdrError::Allocation {
            context: "history snapshot table",
            requested_bytes: usize::MAX,
        })?;
    if history_entries > limits.max_history_entries {
        return Err(PdrError::ResourceLimit {
            resource: "history entries",
            requested: u64::try_from(history_entries).unwrap_or(u64::MAX),
            maximum: u64::try_from(limits.max_history_entries).unwrap_or(u64::MAX),
        });
    }
    if let Some(labels) = labels {
        validate_labels(labels, (undo.len(), redo.len()), limits)?;
    }

    let mut body = Writer::new(usize_limit(limits.max_encoded_bytes));
    encode_document(&mut body, document, limits)?;
    body.u32(count_u32(undo.len(), "undo")?)?;
    for snapshot in undo {
        encode_document(&mut body, snapshot, limits)?;
    }
    body.u32(count_u32(redo.len(), "redo")?)?;
    for snapshot in redo {
        encode_document(&mut body, snapshot, limits)?;
    }
    encode_labels(&mut body, labels, limits)?;
    let body = body.into_bytes();

    let total = PREFIX_LEN
        .checked_add(body.len())
        .and_then(|length| length.checked_add(CHECKSUM_LEN))
        .ok_or(PdrError::Allocation {
            context: "container length",
            requested_bytes: usize::MAX,
        })?;
    if total as u64 > limits.max_encoded_bytes {
        return Err(PdrError::ResourceLimit {
            resource: "encoded bytes",
            requested: u64::try_from(total).unwrap_or(u64::MAX),
            maximum: limits.max_encoded_bytes,
        });
    }
    let body_len = u64::try_from(body.len()).map_err(|_| PdrError::Allocation {
        context: "body length",
        requested_bytes: body.len(),
    })?;

    let mut output = Vec::new();
    output
        .try_reserve_exact(total)
        .map_err(|_| PdrError::Allocation {
            context: "container",
            requested_bytes: total,
        })?;
    output.extend_from_slice(&PDR_MAGIC);
    output.extend_from_slice(&PDR_VERSION.to_le_bytes());
    output.extend_from_slice(&FLAGS.to_le_bytes());
    output.extend_from_slice(&body_len.to_le_bytes());
    output.extend_from_slice(&body);
    let checksum = Sha256::digest(&output);
    output.extend_from_slice(&checksum);
    Ok(output)
}

fn encode_document(
    writer: &mut Writer,
    document: &Document,
    limits: &PdrLimits,
) -> Result<(), PdrError> {
    let size = document.size();
    writer.u32(size.width())?;
    writer.u32(size.height())?;
    writer.u64(document.revision().get())?;
    writer.u64(document.next_layer_id().get())?;
    writer.u32(count_u32(document.layers().len(), "layer")?)?;
    for layer in document.layers() {
        writer.u64(layer.id().get())?;
        writer.u64(layer.revision().get())?;
        let name = layer.name().as_bytes();
        if name.len() > limits.max_layer_name_bytes {
            return Err(PdrError::ResourceLimit {
                resource: "layer name bytes",
                requested: u64::try_from(name.len()).unwrap_or(u64::MAX),
                maximum: u64::try_from(limits.max_layer_name_bytes).unwrap_or(u64::MAX),
            });
        }
        writer.u32(count_u32(name.len(), "layer name")?)?;
        writer.bytes(name)?;
        let mut flags = 0;
        if layer.is_visible() {
            flags |= LAYER_VISIBLE;
        }
        if layer.is_background() {
            flags |= LAYER_BACKGROUND;
        }
        writer.u8(flags)?;
        writer.u8(layer.opacity())?;
        writer.u8(blend_mode_tag(layer.blend_mode()))?;
        writer.u8(0)?;
        let pixel_bytes = layer.surface().size().checked_buffer_len()?;
        writer.u64(
            u64::try_from(pixel_bytes).map_err(|_| PdrError::Allocation {
                context: "layer pixel length",
                requested_bytes: pixel_bytes,
            })?,
        )?;
        writer.pixels(layer.surface().pixels())?;
    }
    writer.u64(document.active_layer_id().get())?;
    encode_selection(writer, document.selection_model())?;
    Ok(())
}

fn encode_selection(writer: &mut Writer, selection: Option<&Selection>) -> Result<(), PdrError> {
    match selection {
        None => writer.u8(SELECTION_NONE),
        Some(selection) => {
            match selection.mask() {
                None => {
                    writer.u8(SELECTION_RECTANGLE)?;
                    writer.rect(selection.bounds())?;
                }
                Some(mask) => {
                    writer.u8(SELECTION_MASK)?;
                    writer.rect(mask.bounds())?;
                    writer.u64(u64::try_from(mask.packed_words().len()).map_err(|_| {
                        PdrError::Allocation {
                            context: "selection mask words",
                            requested_bytes: mask.packed_words().len(),
                        }
                    })?)?;
                    for word in mask.packed_words() {
                        writer.u64(*word)?;
                    }
                }
            }
            Ok(())
        }
    }
}

fn encode_labels(
    writer: &mut Writer,
    labels: Option<&PdrHistoryLabels>,
    limits: &PdrLimits,
) -> Result<(), PdrError> {
    let Some(labels) = labels else {
        return writer.u8(LABELS_ABSENT);
    };
    writer.u8(LABELS_PRESENT)?;
    writer.u32(count_u32(labels.undo.len(), "undo label")?)?;
    for label in &labels.undo {
        write_label(writer, label, limits)?;
    }
    writer.u32(count_u32(labels.redo.len(), "redo label")?)?;
    for label in &labels.redo {
        write_label(writer, label, limits)?;
    }
    Ok(())
}

fn write_label(writer: &mut Writer, label: &str, limits: &PdrLimits) -> Result<(), PdrError> {
    if label.len() > limits.max_label_bytes {
        return Err(PdrError::ResourceLimit {
            resource: "history label bytes",
            requested: u64::try_from(label.len()).unwrap_or(u64::MAX),
            maximum: u64::try_from(limits.max_label_bytes).unwrap_or(u64::MAX),
        });
    }
    writer.u32(count_u32(label.len(), "history label")?)?;
    writer.bytes(label.as_bytes())
}

fn count_u32(value: usize, what: &'static str) -> Result<u32, PdrError> {
    u32::try_from(value).map_err(|_| PdrError::ResourceLimit {
        resource: what,
        requested: u64::try_from(value).unwrap_or(u64::MAX),
        maximum: u64::from(u32::MAX),
    })
}

fn blend_mode_tag(mode: BlendMode) -> u8 {
    match mode {
        BlendMode::Normal => 0,
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Overlay => 3,
    }
}

fn blend_mode_from_tag(tag: u8, offset: usize) -> Result<BlendMode, PdrError> {
    match tag {
        0 => Ok(BlendMode::Normal),
        1 => Ok(BlendMode::Multiply),
        2 => Ok(BlendMode::Screen),
        3 => Ok(BlendMode::Overlay),
        _ => Err(PdrError::Malformed {
            offset,
            reason: "unknown blend mode tag",
        }),
    }
}

struct Writer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Writer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn reserve(&self, additional: usize) -> Result<usize, PdrError> {
        let length = self
            .bytes
            .len()
            .checked_add(additional)
            .ok_or(PdrError::Allocation {
                context: "encoded output",
                requested_bytes: usize::MAX,
            })?;
        if length > self.limit {
            return Err(PdrError::ResourceLimit {
                resource: "encoded bytes",
                requested: u64::try_from(length).unwrap_or(u64::MAX),
                maximum: u64::try_from(self.limit).unwrap_or(u64::MAX),
            });
        }
        Ok(length)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), PdrError> {
        let length = self.reserve(bytes.len())?;
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| PdrError::Allocation {
                context: "encoded output",
                requested_bytes: length,
            })?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), PdrError> {
        self.bytes(&[value])
    }

    fn u32(&mut self, value: u32) -> Result<(), PdrError> {
        self.bytes(&value.to_le_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<(), PdrError> {
        self.bytes(&value.to_le_bytes())
    }

    fn rect(&mut self, rect: Rect) -> Result<(), PdrError> {
        self.i64(rect.left())?;
        self.i64(rect.top())?;
        self.i64(rect.right())?;
        self.i64(rect.bottom())
    }

    fn i64(&mut self, value: i64) -> Result<(), PdrError> {
        self.bytes(&value.to_le_bytes())
    }

    fn pixels(&mut self, pixels: &[paint_core::Bgra]) -> Result<(), PdrError> {
        let byte_count = pixels.len().checked_mul(4).ok_or(PdrError::Allocation {
            context: "layer pixels",
            requested_bytes: usize::MAX,
        })?;
        let length = self.reserve(byte_count)?;
        self.bytes
            .try_reserve(byte_count)
            .map_err(|_| PdrError::Allocation {
                context: "layer pixels",
                requested_bytes: length,
            })?;
        for pixel in pixels {
            self.bytes.extend_from_slice(&pixel.to_bgra_bytes());
        }
        Ok(())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8], base: usize) -> Self {
        Self {
            bytes,
            pos: 0,
            base,
        }
    }

    const fn offset(&self) -> usize {
        self.base + self.pos
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn take(&mut self, length: usize, context: &'static str) -> Result<&'a [u8], PdrError> {
        if length > self.remaining() {
            return Err(PdrError::Truncated {
                offset: self.offset(),
                context,
            });
        }
        let start = self.pos;
        self.pos += length;
        Ok(&self.bytes[start..self.pos])
    }

    fn u8(&mut self, context: &'static str) -> Result<u8, PdrError> {
        Ok(self.take(1, context)?[0])
    }

    fn u32(&mut self, context: &'static str) -> Result<u32, PdrError> {
        let bytes = self.take(4, context)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self, context: &'static str) -> Result<u64, PdrError> {
        let bytes = self.take(8, context)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn i64(&mut self, context: &'static str) -> Result<i64, PdrError> {
        let bytes = self.take(8, context)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn rect(&mut self, context: &'static str) -> Result<Rect, PdrError> {
        let left = self.i64(context)?;
        let top = self.i64(context)?;
        let right = self.i64(context)?;
        let bottom = self.i64(context)?;
        Ok(Rect::new(left, top, right, bottom)?)
    }

    fn string(
        &mut self,
        maximum: usize,
        resource: &'static str,
        field: &'static str,
        context: &'static str,
    ) -> Result<String, PdrError> {
        let length = self.u32(context)?;
        let length = usize::try_from(length).map_err(|_| PdrError::Allocation {
            context: "string length",
            requested_bytes: usize::MAX,
        })?;
        if length > maximum {
            return Err(PdrError::ResourceLimit {
                resource,
                requested: u64::try_from(length).unwrap_or(u64::MAX),
                maximum: u64::try_from(maximum).unwrap_or(u64::MAX),
            });
        }
        let bytes = self.take(length, context)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| PdrError::InvalidUtf8 { field })
    }
}

struct DecodedPdr {
    document: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    labels: Option<PdrHistoryLabels>,
}

fn decode_container(
    bytes: &[u8],
    document_limits: &ResourceLimits,
    limits: &PdrLimits,
) -> Result<DecodedPdr, PdrError> {
    if bytes.len() < MAGIC_LEN {
        return Err(PdrError::Truncated {
            offset: bytes.len(),
            context: "magic",
        });
    }
    if bytes[..MAGIC_LEN] != PDR_MAGIC {
        return Err(PdrError::InvalidMagic);
    }
    if bytes.len() < PREFIX_LEN {
        return Err(PdrError::Truncated {
            offset: bytes.len(),
            context: "header",
        });
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if version != PDR_VERSION {
        return Err(PdrError::UnsupportedVersion {
            version,
            supported: PDR_VERSION,
        });
    }
    let flags = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    if flags != FLAGS {
        return Err(PdrError::Malformed {
            offset: 12,
            reason: "unsupported container flags",
        });
    }
    let body_len = u64::from_le_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
    ]);
    let total = body_len
        .checked_add(u64::try_from(PREFIX_LEN).unwrap_or(u64::MAX))
        .and_then(|length| length.checked_add(u64::try_from(CHECKSUM_LEN).unwrap_or(u64::MAX)))
        .ok_or(PdrError::Malformed {
            offset: 16,
            reason: "container length overflow",
        })?;
    let total = usize::try_from(total).map_err(|_| PdrError::ResourceLimit {
        resource: "input bytes",
        requested: total,
        maximum: u64::try_from(usize::MAX).unwrap_or(u64::MAX),
    })?;
    if bytes.len() < total {
        return Err(PdrError::Truncated {
            offset: bytes.len(),
            context: "body",
        });
    }
    if bytes.len() > total {
        return Err(PdrError::Malformed {
            offset: total,
            reason: "trailing bytes",
        });
    }
    let body_end = total - CHECKSUM_LEN;
    let digest = Sha256::digest(&bytes[..body_end]);
    if digest.as_slice() != &bytes[body_end..] {
        return Err(PdrError::ChecksumMismatch);
    }

    let mut reader = Reader::new(&bytes[PREFIX_LEN..body_end], PREFIX_LEN);
    let mut decoded_surface_bytes = 0u64;
    let document = decode_document(
        &mut reader,
        document_limits,
        limits,
        &mut decoded_surface_bytes,
    )?;
    let undo_count = reader.u32("undo history count")?;
    let undo_count = bounded_history_count(undo_count, limits)?;
    let mut undo = Vec::new();
    undo.try_reserve_exact(undo_count)
        .map_err(|_| PdrError::Allocation {
            context: "undo history",
            requested_bytes: undo_count.saturating_mul(std::mem::size_of::<Document>()),
        })?;
    for _ in 0..undo_count {
        undo.push(decode_document(
            &mut reader,
            document_limits,
            limits,
            &mut decoded_surface_bytes,
        )?);
    }
    let redo_count = reader.u32("redo history count")?;
    let redo_count = bounded_history_count(redo_count, limits)?;
    let total_history = undo_count
        .checked_add(redo_count)
        .ok_or(PdrError::Allocation {
            context: "history entries",
            requested_bytes: usize::MAX,
        })?;
    if total_history > limits.max_history_entries {
        return Err(PdrError::ResourceLimit {
            resource: "history entries",
            requested: u64::try_from(total_history).unwrap_or(u64::MAX),
            maximum: u64::try_from(limits.max_history_entries).unwrap_or(u64::MAX),
        });
    }
    let mut redo = Vec::new();
    redo.try_reserve_exact(redo_count)
        .map_err(|_| PdrError::Allocation {
            context: "redo history",
            requested_bytes: redo_count.saturating_mul(std::mem::size_of::<Document>()),
        })?;
    for _ in 0..redo_count {
        redo.push(decode_document(
            &mut reader,
            document_limits,
            limits,
            &mut decoded_surface_bytes,
        )?);
    }
    let labels = decode_labels(&mut reader, limits, undo_count, redo_count)?;
    if reader.remaining() != 0 {
        return Err(PdrError::Malformed {
            offset: reader.offset(),
            reason: "trailing body bytes",
        });
    }
    Ok(DecodedPdr {
        document,
        undo,
        redo,
        labels,
    })
}

fn bounded_history_count(count: u32, limits: &PdrLimits) -> Result<usize, PdrError> {
    let count = usize::try_from(count).map_err(|_| PdrError::Allocation {
        context: "history count",
        requested_bytes: usize::MAX,
    })?;
    if count > limits.max_history_entries {
        return Err(PdrError::ResourceLimit {
            resource: "history entries",
            requested: u64::try_from(count).unwrap_or(u64::MAX),
            maximum: u64::try_from(limits.max_history_entries).unwrap_or(u64::MAX),
        });
    }
    Ok(count)
}

fn decode_document(
    reader: &mut Reader<'_>,
    document_limits: &ResourceLimits,
    limits: &PdrLimits,
    decoded_surface_bytes: &mut u64,
) -> Result<Document, PdrError> {
    let width = reader.u32("document width")?;
    let height = reader.u32("document height")?;
    let size = Size::new(width, height).map_err(|_| PdrError::Malformed {
        offset: reader.offset().saturating_sub(8),
        reason: "document dimensions must be nonzero",
    })?;
    let revision = Revision::new(reader.u64("document revision")?);
    let next_layer_id = LayerId::new(reader.u64("next layer ID")?);
    let layer_count = reader.u32("layer count")?;
    let layer_count = usize::try_from(layer_count).map_err(|_| PdrError::Allocation {
        context: "layer table",
        requested_bytes: usize::MAX,
    })?;
    validate_document_shape(size, layer_count, document_limits)?;
    if layer_count == 0 {
        return Err(PdrError::Malformed {
            offset: reader.offset(),
            reason: "a document must contain at least one layer",
        });
    }
    let mut layers = Vec::new();
    layers
        .try_reserve_exact(layer_count)
        .map_err(|_| PdrError::Allocation {
            context: "layer table",
            requested_bytes: layer_count.saturating_mul(std::mem::size_of::<Layer>()),
        })?;
    let expected_pixel_bytes = size.checked_buffer_len()?;
    for _ in 0..layer_count {
        let id = LayerId::new(reader.u64("layer ID")?);
        let layer_revision = Revision::new(reader.u64("layer revision")?);
        let name = reader.string(
            limits.max_layer_name_bytes,
            "layer name bytes",
            "layer name",
            "layer name length",
        )?;
        let flags = reader.u8("layer flags")?;
        if flags & !LAYER_KNOWN_FLAGS != 0 {
            return Err(PdrError::Malformed {
                offset: reader.offset().saturating_sub(1),
                reason: "unknown layer flags",
            });
        }
        let opacity = reader.u8("layer opacity")?;
        let blend_tag = reader.u8("layer blend mode")?;
        let blend_offset = reader.offset().saturating_sub(1);
        let blend_mode = blend_mode_from_tag(blend_tag, blend_offset)?;
        let reserved = reader.u8("layer reserved byte")?;
        if reserved != 0 {
            return Err(PdrError::Malformed {
                offset: reader.offset().saturating_sub(1),
                reason: "nonzero layer reserved byte",
            });
        }
        let pixel_bytes = reader.u64("layer pixel byte length")?;
        let expected = u64::try_from(expected_pixel_bytes).map_err(|_| PdrError::Allocation {
            context: "expected layer pixels",
            requested_bytes: expected_pixel_bytes,
        })?;
        if pixel_bytes != expected {
            return Err(PdrError::Malformed {
                offset: reader.offset().saturating_sub(8),
                reason: "layer pixel byte length does not match document size",
            });
        }
        *decoded_surface_bytes =
            decoded_surface_bytes
                .checked_add(pixel_bytes)
                .ok_or(PdrError::Allocation {
                    context: "decoded surface bytes",
                    requested_bytes: usize::MAX,
                })?;
        if *decoded_surface_bytes > limits.max_decoded_surface_bytes {
            return Err(PdrError::ResourceLimit {
                resource: "decoded surface bytes",
                requested: *decoded_surface_bytes,
                maximum: limits.max_decoded_surface_bytes,
            });
        }
        let pixels = reader.take(expected_pixel_bytes, "layer pixels")?;
        let surface = Surface::from_bgra_bytes(size, pixels)?;
        let layer = Layer::new(id, name, surface)
            .with_visibility(flags & LAYER_VISIBLE != 0)
            .with_opacity(opacity)
            .with_background(flags & LAYER_BACKGROUND != 0)
            .with_blend_mode(blend_mode)
            .with_revision(layer_revision);
        layers.push(layer);
    }
    let active_layer = LayerId::new(reader.u64("active layer ID")?);
    let selection = decode_selection(reader, limits)?;
    Ok(Document::restore(
        size,
        layers,
        active_layer,
        selection,
        revision,
        next_layer_id,
    )?)
}

fn decode_selection(
    reader: &mut Reader<'_>,
    limits: &PdrLimits,
) -> Result<Option<Selection>, PdrError> {
    let tag = reader.u8("selection tag")?;
    match tag {
        SELECTION_NONE => Ok(None),
        SELECTION_RECTANGLE => Ok(Some(Selection::rectangle(
            reader.rect("selection rectangle")?,
        ))),
        SELECTION_MASK => {
            let bounds = reader.rect("selection mask bounds")?;
            let word_count = reader.u64("selection mask word count")?;
            let expected = expected_mask_words(bounds)?;
            if word_count != expected {
                return Err(PdrError::Malformed {
                    offset: reader.offset().saturating_sub(8),
                    reason: "selection mask word count does not match bounds",
                });
            }
            let word_count = usize::try_from(word_count).map_err(|_| PdrError::Allocation {
                context: "selection mask words",
                requested_bytes: usize::MAX,
            })?;
            if word_count > limits.max_mask_words {
                return Err(PdrError::ResourceLimit {
                    resource: "selection mask words",
                    requested: u64::try_from(word_count).unwrap_or(u64::MAX),
                    maximum: u64::try_from(limits.max_mask_words).unwrap_or(u64::MAX),
                });
            }
            let mut words = Vec::new();
            words
                .try_reserve_exact(word_count)
                .map_err(|_| PdrError::Allocation {
                    context: "selection mask words",
                    requested_bytes: word_count.saturating_mul(std::mem::size_of::<u64>()),
                })?;
            for _ in 0..word_count {
                words.push(reader.u64("selection mask word")?);
            }
            let mask = SelectionMask::from_packed_words(bounds, words)?;
            Ok(Some(Selection::from_mask(mask)))
        }
        tag => Err(PdrError::UnknownTag {
            field: "selection",
            tag,
        }),
    }
}

fn expected_mask_words(bounds: Rect) -> Result<u64, PdrError> {
    let width = u64::from(bounds.width());
    let height = u64::from(bounds.height());
    let row_words = width
        .checked_add(63)
        .and_then(|width| width.checked_div(64))
        .ok_or(PdrError::Malformed {
            offset: 0,
            reason: "selection mask row width overflow",
        })?;
    row_words.checked_mul(height).ok_or(PdrError::Malformed {
        offset: 0,
        reason: "selection mask word count overflow",
    })
}

fn decode_labels(
    reader: &mut Reader<'_>,
    limits: &PdrLimits,
    undo_count: usize,
    redo_count: usize,
) -> Result<Option<PdrHistoryLabels>, PdrError> {
    let tag = reader.u8("history labels tag")?;
    match tag {
        LABELS_ABSENT => Ok(None),
        LABELS_PRESENT => {
            let undo_labels = decode_label_vec(reader, limits, undo_count, "undo")?;
            let redo_labels = decode_label_vec(reader, limits, redo_count, "redo")?;
            Ok(Some(PdrHistoryLabels {
                undo: undo_labels,
                redo: redo_labels,
            }))
        }
        tag => Err(PdrError::UnknownTag {
            field: "history labels",
            tag,
        }),
    }
}

fn decode_label_vec(
    reader: &mut Reader<'_>,
    limits: &PdrLimits,
    expected: usize,
    stack: &'static str,
) -> Result<Vec<String>, PdrError> {
    let count = reader.u32("history label count")?;
    let count = usize::try_from(count).map_err(|_| PdrError::Allocation {
        context: "history labels",
        requested_bytes: usize::MAX,
    })?;
    if count != expected {
        return Err(PdrError::LabelCount {
            stack,
            expected,
            actual: count,
        });
    }
    let mut labels = Vec::new();
    labels
        .try_reserve_exact(count)
        .map_err(|_| PdrError::Allocation {
            context: "history labels",
            requested_bytes: count.saturating_mul(std::mem::size_of::<String>()),
        })?;
    for _ in 0..count {
        labels.push(reader.string(
            limits.max_label_bytes,
            "history label bytes",
            "history label",
            "history label length",
        )?);
    }
    Ok(labels)
}

fn usize_limit(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
