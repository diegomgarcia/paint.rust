use paint_core::{Bgra, Rect, Size, Surface, SurfaceError};
use thiserror::Error;

use crate::{BlendMode, Layer, LayerId, PatchError, PixelPatch, Revision};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DocumentError {
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    #[error(transparent)]
    Patch(#[from] PatchError),
    #[error("a document must contain at least one layer")]
    EmptyDocument,
    #[error("duplicate layer ID {layer_id:?}")]
    DuplicateLayerId { layer_id: LayerId },
    #[error("layer {layer_id:?} has size {actual:?}, but document size is {expected:?}")]
    LayerSizeMismatch {
        layer_id: LayerId,
        expected: Size,
        actual: Size,
    },
    #[error("layer {layer_id:?} was not found")]
    LayerNotFound { layer_id: LayerId },
    #[error("expected {expected} replacement layer surfaces, got {actual}")]
    SurfaceCountMismatch { expected: usize, actual: usize },
    #[error("layer index {index} is invalid for {len} layers")]
    LayerIndexOutOfBounds { index: usize, len: usize },
    #[error("the final layer cannot be removed from a document")]
    CannotRemoveLastLayer,
    #[error(
        "blend mode {mode:?} on layer {layer_id:?} is not implemented; only Normal is supported"
    )]
    UnsupportedBlendMode { layer_id: LayerId, mode: BlendMode },
    #[error("layer {layer_id:?} revision overflow")]
    LayerRevisionOverflow { layer_id: LayerId },
    #[error("document revision overflow")]
    DocumentRevisionOverflow,
    #[error("layer ID space exhausted after {layer_id:?}")]
    LayerIdExhausted { layer_id: LayerId },
    #[error("failed to reserve space for a new layer")]
    LayerAllocationFailed,
    #[error(
        "stale patch for layer {layer_id:?}: expected revision {expected:?}, actual revision {actual:?}"
    )]
    StalePatch {
        layer_id: LayerId,
        expected: Revision,
        actual: Revision,
    },
    #[error("before-pixels mismatch for layer {layer_id:?} in rectangle {rect:?}")]
    BeforePixelsMismatch { layer_id: LayerId, rect: Rect },
}

/// A document with bottom-to-top ordered layers and one active layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    size: Size,
    layers: Vec<Layer>,
    active_layer: LayerId,
    selection: Option<Rect>,
    revision: Revision,
    next_layer_id: LayerId,
}

impl Document {
    /// Creates one active, opaque-white background layer named `Background`.
    pub fn new(size: Size) -> Result<Self, DocumentError> {
        let surface = Surface::filled(size, Bgra::WHITE)?;
        let background = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
        Self::from_layers(size, vec![background])
    }

    /// Takes ownership of ordered layers after validating IDs and every surface size.
    pub fn from_layers(size: Size, layers: Vec<Layer>) -> Result<Self, DocumentError> {
        if layers.is_empty() {
            return Err(DocumentError::EmptyDocument);
        }
        for (index, layer) in layers.iter().enumerate() {
            if layer.surface().size() != size {
                return Err(DocumentError::LayerSizeMismatch {
                    layer_id: layer.id(),
                    expected: size,
                    actual: layer.surface().size(),
                });
            }
            if layers[..index].iter().any(|other| other.id() == layer.id()) {
                return Err(DocumentError::DuplicateLayerId {
                    layer_id: layer.id(),
                });
            }
        }
        let greatest_id = layers
            .iter()
            .map(|layer| layer.id())
            .max()
            .ok_or(DocumentError::EmptyDocument)?;
        let next_layer_id = greatest_id
            .checked_next()
            .ok_or(DocumentError::LayerIdExhausted {
                layer_id: greatest_id,
            })?;
        let active_layer = layers[0].id();
        Ok(Self {
            size,
            layers,
            active_layer,
            selection: None,
            revision: Revision::ZERO,
            next_layer_id,
        })
    }

    pub const fn size(&self) -> Size {
        self.size
    }

    pub fn canvas_rect(&self) -> Rect {
        self.size.rect()
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, layer_id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id() == layer_id)
    }

    pub fn layer_index(&self, layer_id: LayerId) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id() == layer_id)
    }

    pub const fn active_layer_id(&self) -> LayerId {
        self.active_layer
    }

    pub fn active_layer(&self) -> &Layer {
        self.layer(self.active_layer)
            .expect("document invariant: the active layer always exists")
    }

    pub const fn selection(&self) -> Option<Rect> {
        self.selection
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Replaces the canvas size and every layer surface as one validated mutation.
    ///
    /// Layer IDs, bottom-to-top order, names, visibility, opacity, background flags, blend
    /// modes, the active layer, the next layer ID, and the selection are preserved. Every layer
    /// and the document advance exactly one revision. The caller is responsible for remapping
    /// or clearing any selection whose coordinates no longer make sense.
    pub fn replace_geometry(
        &mut self,
        new_size: Size,
        surfaces: Vec<Surface>,
    ) -> Result<(), DocumentError> {
        if surfaces.len() != self.layers.len() {
            return Err(DocumentError::SurfaceCountMismatch {
                expected: self.layers.len(),
                actual: surfaces.len(),
            });
        }
        for (index, surface) in surfaces.iter().enumerate() {
            if surface.size() != new_size {
                return Err(DocumentError::LayerSizeMismatch {
                    layer_id: self.layers[index].id(),
                    expected: new_size,
                    actual: surface.size(),
                });
            }
        }
        let next_document_revision = self.next_document_revision()?;
        let mut next_layer_revisions = Vec::new();
        next_layer_revisions
            .try_reserve_exact(self.layers.len())
            .map_err(|_| DocumentError::LayerAllocationFailed)?;
        for layer in &self.layers {
            next_layer_revisions.push(layer.revision.checked_next().ok_or(
                DocumentError::LayerRevisionOverflow {
                    layer_id: layer.id(),
                },
            )?);
        }

        // All fallible work is complete; the remaining assignments cannot fail.
        for (layer, surface) in self.layers.iter_mut().zip(surfaces) {
            layer.surface = surface;
        }
        for (layer, revision) in self.layers.iter_mut().zip(next_layer_revisions) {
            layer.revision = revision;
        }
        self.size = new_size;
        self.revision = next_document_revision;
        Ok(())
    }

    /// Adds a transparent, non-background layer at the top and makes it active.
    pub fn add_layer(&mut self, name: impl Into<String>) -> Result<LayerId, DocumentError> {
        let id = self.next_layer_id;
        let layer = Layer::new(id, name, Surface::new(self.size)?);
        let index = self.layers.len();
        self.insert_layer(index, layer)?;
        Ok(id)
    }

    /// Inserts a layer at a bottom-to-top order index and makes it active.
    pub fn insert_layer(&mut self, index: usize, layer: Layer) -> Result<(), DocumentError> {
        if index > self.layers.len() {
            return Err(DocumentError::LayerIndexOutOfBounds {
                index,
                len: self.layers.len(),
            });
        }
        if layer.surface().size() != self.size {
            return Err(DocumentError::LayerSizeMismatch {
                layer_id: layer.id(),
                expected: self.size,
                actual: layer.surface().size(),
            });
        }
        if self.layer(layer.id()).is_some() {
            return Err(DocumentError::DuplicateLayerId {
                layer_id: layer.id(),
            });
        }
        let next_layer_id = if layer.id() >= self.next_layer_id {
            layer
                .id()
                .checked_next()
                .ok_or(DocumentError::LayerIdExhausted {
                    layer_id: layer.id(),
                })?
        } else {
            self.next_layer_id
        };
        let next_revision = self.next_document_revision()?;
        self.layers
            .try_reserve(1)
            .map_err(|_| DocumentError::LayerAllocationFailed)?;
        self.layers.insert(index, layer);
        self.active_layer = self.layers[index].id();
        self.next_layer_id = next_layer_id;
        self.revision = next_revision;
        Ok(())
    }

    pub fn move_layer(
        &mut self,
        layer_id: LayerId,
        new_index: usize,
    ) -> Result<bool, DocumentError> {
        let old_index = self.index_of(layer_id)?;
        if new_index >= self.layers.len() {
            return Err(DocumentError::LayerIndexOutOfBounds {
                index: new_index,
                len: self.layers.len(),
            });
        }
        if old_index == new_index {
            return Ok(false);
        }
        let next_revision = self.next_document_revision()?;
        let layer = self.layers.remove(old_index);
        self.layers.insert(new_index, layer);
        self.revision = next_revision;
        Ok(true)
    }

    /// Duplicates a layer immediately above the source and makes it active.
    pub fn duplicate_layer(&mut self, index: usize) -> Result<LayerId, DocumentError> {
        let source = self
            .layers
            .get(index)
            .ok_or(DocumentError::LayerIndexOutOfBounds {
                index,
                len: self.layers.len(),
            })?;
        let id = self.next_layer_id;
        let name = format!("{} copy", source.name());
        let duplicate = source.duplicate(id, name);
        self.insert_layer(index + 1, duplicate)?;
        Ok(id)
    }

    pub fn remove_layer(&mut self, layer_id: LayerId) -> Result<Layer, DocumentError> {
        let index = self.index_of(layer_id)?;
        if self.layers.len() == 1 {
            return Err(DocumentError::CannotRemoveLastLayer);
        }
        let next_revision = self.next_document_revision()?;
        let removed = self.layers.remove(index);
        if self.active_layer == layer_id {
            let new_index = index.min(self.layers.len() - 1);
            self.active_layer = self.layers[new_index].id();
        }
        self.revision = next_revision;
        Ok(removed)
    }

    pub fn set_active_layer(&mut self, layer_id: LayerId) -> Result<bool, DocumentError> {
        self.index_of(layer_id)?;
        if self.active_layer == layer_id {
            return Ok(false);
        }
        let next_revision = self.next_document_revision()?;
        self.active_layer = layer_id;
        self.revision = next_revision;
        Ok(true)
    }

    /// Sets an unbounded signed selection. Raster operations clip it to the canvas.
    pub fn set_selection(&mut self, selection: Option<Rect>) -> Result<bool, DocumentError> {
        if self.selection == selection {
            return Ok(false);
        }
        let next_revision = self.next_document_revision()?;
        self.selection = selection;
        self.revision = next_revision;
        Ok(true)
    }

    pub fn clear_selection(&mut self) -> Result<bool, DocumentError> {
        self.set_selection(None)
    }

    pub fn set_layer_name(
        &mut self,
        layer_id: LayerId,
        name: impl Into<String>,
    ) -> Result<bool, DocumentError> {
        let index = self.index_of(layer_id)?;
        let name = name.into();
        if self.layers[index].name == name {
            return Ok(false);
        }
        let (next_document, next_layer) = self.next_revisions(layer_id)?;
        self.layers[index].name = name;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(true)
    }

    pub fn set_layer_visibility(
        &mut self,
        layer_id: LayerId,
        visible: bool,
    ) -> Result<bool, DocumentError> {
        let index = self.index_of(layer_id)?;
        if self.layers[index].visible == visible {
            return Ok(false);
        }
        let (next_document, next_layer) = self.next_revisions(layer_id)?;
        self.layers[index].visible = visible;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(true)
    }

    pub fn set_layer_opacity(
        &mut self,
        layer_id: LayerId,
        opacity: u8,
    ) -> Result<bool, DocumentError> {
        let index = self.index_of(layer_id)?;
        if self.layers[index].opacity == opacity {
            return Ok(false);
        }
        let (next_document, next_layer) = self.next_revisions(layer_id)?;
        self.layers[index].opacity = opacity;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(true)
    }

    pub fn set_layer_background(
        &mut self,
        layer_id: LayerId,
        background: bool,
    ) -> Result<bool, DocumentError> {
        let index = self.index_of(layer_id)?;
        if self.layers[index].background == background {
            return Ok(false);
        }
        let (next_document, next_layer) = self.next_revisions(layer_id)?;
        self.layers[index].background = background;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(true)
    }

    /// Rejects every non-Normal mode without changing layer or document state.
    pub fn set_layer_blend_mode(
        &mut self,
        layer_id: LayerId,
        mode: BlendMode,
    ) -> Result<bool, DocumentError> {
        let index = self.index_of(layer_id)?;
        if mode != BlendMode::Normal {
            return Err(DocumentError::UnsupportedBlendMode { layer_id, mode });
        }
        if self.layers[index].blend_mode == mode {
            return Ok(false);
        }
        let (next_document, next_layer) = self.next_revisions(layer_id)?;
        self.layers[index].blend_mode = mode;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(true)
    }

    /// Applies a patch atomically after structural, revision, bounds, and before-pixel checks.
    pub fn apply_patch(&mut self, patch: &PixelPatch) -> Result<Revision, DocumentError> {
        patch.validate()?;
        let index = self.index_of(patch.layer_id())?;
        let layer = &self.layers[index];
        if layer.revision() != patch.expected_revision() {
            return Err(DocumentError::StalePatch {
                layer_id: patch.layer_id(),
                expected: patch.expected_revision(),
                actual: layer.revision(),
            });
        }
        let (next_document, next_layer) = self.next_revisions(patch.layer_id())?;
        if !self.layers[index]
            .surface()
            .region_matches(patch.rect(), patch.before())?
        {
            return Err(DocumentError::BeforePixelsMismatch {
                layer_id: patch.layer_id(),
                rect: patch.rect(),
            });
        }

        // All fallible validation is complete. Region replacement performs no fallible work
        // after its own validation, so a rejected patch cannot leave a partial pixel change.
        self.layers[index]
            .surface_mut()
            .replace_region(patch.rect(), patch.after())?;
        self.layers[index].revision = next_layer;
        self.revision = next_document;
        Ok(next_layer)
    }

    /// Applies the inverse expected after `patch`, returning the new layer revision.
    pub fn apply_inverse(&mut self, patch: &PixelPatch) -> Result<Revision, DocumentError> {
        let inverse = patch.inverse()?;
        self.apply_patch(&inverse)
    }

    fn index_of(&self, layer_id: LayerId) -> Result<usize, DocumentError> {
        self.layer_index(layer_id)
            .ok_or(DocumentError::LayerNotFound { layer_id })
    }

    fn next_document_revision(&self) -> Result<Revision, DocumentError> {
        self.revision
            .checked_next()
            .ok_or(DocumentError::DocumentRevisionOverflow)
    }

    fn next_revisions(&self, layer_id: LayerId) -> Result<(Revision, Revision), DocumentError> {
        let document = self.next_document_revision()?;
        let layer = self
            .layer(layer_id)
            .ok_or(DocumentError::LayerNotFound { layer_id })?
            .revision()
            .checked_next()
            .ok_or(DocumentError::LayerRevisionOverflow { layer_id })?;
        Ok((document, layer))
    }
}

#[cfg(test)]
mod tests {
    use paint_core::{Bgra, Rect, Size};

    use super::*;

    fn one_pixel_document() -> Document {
        Document::new(Size::new(1, 1).expect("nonzero test size")).expect("default document")
    }

    fn white_to_black(expected_revision: Revision) -> PixelPatch {
        PixelPatch::new(
            LayerId::new(1),
            expected_revision,
            Rect::new(0, 0, 1, 1).expect("one-pixel rectangle"),
            vec![Bgra::WHITE],
            vec![Bgra::BLACK],
        )
        .expect("valid one-pixel patch")
    }

    #[test]
    fn document_revision_overflow_rejects_patch_without_mutation() {
        let mut document = one_pixel_document();
        document.revision = Revision::MAX;
        let snapshot = document.clone();
        assert_eq!(
            document.apply_patch(&white_to_black(Revision::ZERO)),
            Err(DocumentError::DocumentRevisionOverflow)
        );
        assert_eq!(document, snapshot);
    }

    #[test]
    fn layer_revision_overflow_rejects_patch_without_mutation() {
        let mut document = one_pixel_document();
        document.layers[0].revision = Revision::MAX;
        let snapshot = document.clone();
        assert_eq!(
            document.apply_patch(&white_to_black(Revision::MAX)),
            Err(DocumentError::LayerRevisionOverflow {
                layer_id: LayerId::new(1),
            })
        );
        assert_eq!(document, snapshot);
    }
}
