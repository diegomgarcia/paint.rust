use paint_core::{Rect, Surface};
use paint_document::{BlendMode, Document};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SessionError;

pub const SNAPSHOT_FORMAT: &str = "paint.r.semantic-snapshot";
pub const SNAPSHOT_VERSION: u32 = 1;

/// One layer's protocol-visible metadata. Pixel data is represented by the composite hash.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSnapshot {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub is_background: bool,
    pub blend_mode: String,
}

/// A signed, half-open selection in the semantic snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionSnapshot {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl From<Rect> for SelectionSnapshot {
    fn from(rect: Rect) -> Self {
        Self {
            left: rect.left(),
            top: rect.top(),
            right: rect.right(),
            bottom: rect.bottom(),
        }
    }
}

/// Exact UI-neutral state used by headless smoke tests and external semantic verification.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSnapshot {
    pub format: String,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub layer_count: usize,
    pub active_layer_index: usize,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub layers: Vec<LayerSnapshot>,
    pub selection: Option<SelectionSnapshot>,
    pub decoded_rgba_sha256: String,
}

pub(crate) fn snapshot(
    document: &Document,
    composed: &Surface,
    dirty: bool,
    can_undo: bool,
    can_redo: bool,
) -> Result<SemanticSnapshot, SessionError> {
    if composed.size() != document.size() {
        return Err(SessionError::DocumentInvariant(
            "composed surface size differs from the document",
        ));
    }
    let active_layer_index =
        document
            .layer_index(document.active_layer_id())
            .ok_or(SessionError::DocumentInvariant(
                "active layer is absent from the ordered layer list",
            ))?;
    let rgba = composed.to_rgba_bytes()?;
    let decoded_rgba_sha256 = hex::encode(Sha256::digest(&rgba));
    let layers = document
        .layers()
        .iter()
        .map(|layer| LayerSnapshot {
            name: layer.name().to_owned(),
            visible: layer.is_visible(),
            opacity: layer.opacity(),
            is_background: layer.is_background(),
            blend_mode: blend_mode_name(layer.blend_mode()).to_owned(),
        })
        .collect();

    Ok(SemanticSnapshot {
        format: SNAPSHOT_FORMAT.to_owned(),
        version: SNAPSHOT_VERSION,
        width: document.size().width(),
        height: document.size().height(),
        layer_count: document.layers().len(),
        active_layer_index,
        dirty,
        can_undo,
        can_redo,
        layers,
        selection: document.selection().map(SelectionSnapshot::from),
        decoded_rgba_sha256,
    })
}

fn blend_mode_name(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
    }
}
