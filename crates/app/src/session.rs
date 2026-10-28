use paint_core::{Bgra, Point, Rect, Surface};
use paint_document::{Document, DocumentError, Layer, LayerId, PatchError, PixelPatch, Revision};
use sha2::{Digest, Sha256};

use crate::{Command, ResourceLimitError, SemanticSnapshot, SessionError, snapshot};

/// Bounds applied to externally supplied traces and session operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_pixels: u64,
    pub max_layers: usize,
    pub max_commands: usize,
    pub max_trace_bytes: usize,
    pub max_layer_name_bytes: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_width: 16_384,
            max_height: 16_384,
            max_pixels: 64 * 1024 * 1024,
            max_layers: 256,
            max_commands: 100_000,
            max_trace_bytes: 16 * 1024 * 1024,
            max_layer_name_bytes: 4 * 1024,
        }
    }
}

/// The sole writer for one document, with inverse-based history and a semantic clean baseline.
///
/// Content mutations are first applied to a candidate document. Only a completely successful
/// mutation is committed, and history stacks are changed afterward. Selection and active-layer
/// commands affect UI state but never enter content history or change the dirty baseline.
#[derive(Debug)]
pub struct AppSession {
    document: Document,
    undo: Vec<Mutation>,
    redo: Vec<Mutation>,
    limits: ResourceLimits,
    clean_fingerprint: [u8; 32],
    content_fingerprint: [u8; 32],
}

impl AppSession {
    /// Opens an already validated document with a clean baseline and default resource limits.
    pub fn new(document: Document) -> Self {
        Self::with_limits_unchecked(document, ResourceLimits::default())
    }

    /// Opens a document after checking its dimensions and layer count against explicit limits.
    pub fn with_limits(document: Document, limits: ResourceLimits) -> Result<Self, SessionError> {
        validate_document_resources(&document, &limits)?;
        Ok(Self::with_limits_unchecked(document, limits))
    }

    fn with_limits_unchecked(document: Document, limits: ResourceLimits) -> Self {
        let fingerprint = content_fingerprint(&document);
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            limits,
            clean_fingerprint: fingerprint,
            content_fingerprint: fingerprint,
        }
    }

    pub const fn document(&self) -> &Document {
        &self.document
    }

    pub fn into_document(self) -> Document {
        self.document
    }

    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }

    pub fn dirty(&self) -> bool {
        self.content_fingerprint != self.clean_fingerprint
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Establishes the current content as clean after a successful open or save.
    /// History remains valid; moving away from and back to this baseline updates dirty exactly.
    pub fn mark_clean(&mut self) {
        self.clean_fingerprint = self.content_fingerprint;
    }

    /// Executes one command and returns whether document or UI state changed.
    pub fn execute(&mut self, command: Command) -> Result<bool, SessionError> {
        match command {
            Command::SelectRect(rect) => self.select_rect(rect),
            Command::ClearSelection => self.clear_selection(),
            Command::Fill(color) => self.fill(color),
            Command::Stroke {
                label: _,
                points,
                size,
                color,
            } => self.stroke(&points, size, color),
            Command::FillRect { rect, color } => self.fill_rect(rect, color),
            Command::FillEllipse { rect, color } => self.fill_ellipse(rect, color),
            Command::Gradient {
                start,
                end,
                from,
                to,
            } => self.linear_gradient(start, end, from, to),
            Command::Recolor {
                target,
                replacement,
                tolerance,
            } => self.recolor(target, replacement, tolerance),
            Command::Move { dx, dy } => self.translate(dx, dy),
            Command::Invert => self.invert(),
            Command::FlipLayerHorizontal => self.flip_layer_horizontal(),
            Command::FlipLayerVertical => self.flip_layer_vertical(),
            Command::AddLayer {
                name,
                opacity,
                visible,
            } => self.add_layer(name, opacity, visible),
            Command::SetActiveLayer { index } => self.set_active_layer(index),
            Command::SetLayerVisibility { index, visible } => {
                let layer_id = self.layer_id(index)?;
                self.apply_new_mutation(Mutation::SetVisibility {
                    layer_id,
                    value: visible,
                })
            }
            Command::SetLayerOpacity { index, opacity } => {
                let layer_id = self.layer_id(index)?;
                self.apply_new_mutation(Mutation::SetOpacity {
                    layer_id,
                    value: opacity,
                })
            }
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
        }
    }

    pub fn select_rect(&mut self, rect: Rect) -> Result<bool, SessionError> {
        Ok(self.document.set_selection(Some(rect))?)
    }

    pub fn clear_selection(&mut self) -> Result<bool, SessionError> {
        Ok(self.document.clear_selection()?)
    }

    pub fn fill(&mut self, color: Bgra) -> Result<bool, SessionError> {
        let patch = paint_raster::fill(&self.document, self.document.active_layer_id(), color)?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn stroke(
        &mut self,
        points: &[Point],
        size: u32,
        color: Option<Bgra>,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::stroke(
            &self.document,
            self.document.active_layer_id(),
            points,
            size,
            color,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Bgra) -> Result<bool, SessionError> {
        let patch =
            paint_raster::fill_rect(&self.document, self.document.active_layer_id(), rect, color)?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn linear_gradient(
        &mut self,
        start: Point,
        end: Point,
        from: Bgra,
        to: Bgra,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::linear_gradient(
            &self.document,
            self.document.active_layer_id(),
            start,
            end,
            from,
            to,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn recolor(
        &mut self,
        target: Bgra,
        replacement: Bgra,
        tolerance: u8,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::recolor(
            &self.document,
            self.document.active_layer_id(),
            target,
            replacement,
            tolerance,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn translate(&mut self, dx: i32, dy: i32) -> Result<bool, SessionError> {
        let patch =
            paint_raster::translate(&self.document, self.document.active_layer_id(), dx, dy)?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn fill_ellipse(&mut self, rect: Rect, color: Bgra) -> Result<bool, SessionError> {
        let patch = paint_raster::fill_ellipse(
            &self.document,
            self.document.active_layer_id(),
            rect,
            color,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn invert(&mut self) -> Result<bool, SessionError> {
        let patch = paint_raster::invert(&self.document, self.document.active_layer_id())?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn flip_layer_horizontal(&mut self) -> Result<bool, SessionError> {
        let patch = paint_raster::flip_horizontal(&self.document, self.document.active_layer_id())?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn flip_layer_vertical(&mut self) -> Result<bool, SessionError> {
        let patch = paint_raster::flip_vertical(&self.document, self.document.active_layer_id())?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn add_layer(
        &mut self,
        name: String,
        opacity: u8,
        visible: bool,
    ) -> Result<bool, SessionError> {
        validate_layer_name(&name, &self.limits)?;
        if self.document.layers().len() >= self.limits.max_layers {
            return Err(ResourceLimitError::Layers {
                actual: self.document.layers().len().saturating_add(1),
                max: self.limits.max_layers,
            }
            .into());
        }

        let mut candidate = self.document.clone();
        let inverse = Mutation::create_layer(&mut candidate, name, opacity, visible)?;
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    pub fn set_active_layer(&mut self, index: usize) -> Result<bool, SessionError> {
        let layer_id = self.layer_id(index)?;
        Ok(self.document.set_active_layer(layer_id)?)
    }

    pub fn undo(&mut self) -> Result<bool, SessionError> {
        let Some(mutation) = self.undo.last() else {
            return Ok(false);
        };
        let mut candidate = self.document.clone();
        let forward = mutation
            .apply(&mut candidate)?
            .ok_or(SessionError::HistoryInvariant(
                "an undo mutation unexpectedly became a no-op",
            ))?;

        self.document = candidate;
        let _applied = self.undo.pop();
        self.redo.push(forward);
        self.refresh_content_fingerprint();
        Ok(true)
    }

    pub fn redo(&mut self) -> Result<bool, SessionError> {
        let Some(mutation) = self.redo.last() else {
            return Ok(false);
        };
        let mut candidate = self.document.clone();
        let inverse = mutation
            .apply(&mut candidate)?
            .ok_or(SessionError::HistoryInvariant(
                "a redo mutation unexpectedly became a no-op",
            ))?;

        self.document = candidate;
        let _applied = self.redo.pop();
        self.undo.push(inverse);
        self.refresh_content_fingerprint();
        Ok(true)
    }

    pub fn compose(&self) -> Result<Surface, SessionError> {
        Ok(paint_raster::compose(&self.document)?)
    }

    pub fn snapshot(&self) -> Result<SemanticSnapshot, SessionError> {
        let (_, snapshot) = self.compose_and_snapshot()?;
        Ok(snapshot)
    }

    /// Composes exactly once and derives the snapshot hash from that returned surface.
    pub fn compose_and_snapshot(&self) -> Result<(Surface, SemanticSnapshot), SessionError> {
        let composed = self.compose()?;
        let snapshot = snapshot::snapshot(
            &self.document,
            &composed,
            self.dirty(),
            self.can_undo(),
            self.can_redo(),
        )?;
        Ok((composed, snapshot))
    }

    fn layer_id(&self, index: usize) -> Result<LayerId, SessionError> {
        self.document
            .layers()
            .get(index)
            .map(Layer::id)
            .ok_or_else(|| {
                DocumentError::LayerIndexOutOfBounds {
                    index,
                    len: self.document.layers().len(),
                }
                .into()
            })
    }

    fn apply_new_mutation(&mut self, mutation: Mutation) -> Result<bool, SessionError> {
        let mut candidate = self.document.clone();
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    fn commit_new(&mut self, candidate: Document, inverse: Mutation) {
        self.document = candidate;
        self.undo.push(inverse);
        self.redo.clear();
        self.refresh_content_fingerprint();
    }

    fn refresh_content_fingerprint(&mut self) {
        self.content_fingerprint = content_fingerprint(&self.document);
    }
}

#[derive(Debug)]
enum Mutation {
    Pixels(PixelPatch),
    AddLayer {
        layer: Layer,
        index: usize,
    },
    RemoveLayer {
        layer: Layer,
        index: usize,
        restore_active: LayerId,
    },
    SetVisibility {
        layer_id: LayerId,
        value: bool,
    },
    SetOpacity {
        layer_id: LayerId,
        value: u8,
    },
}

impl Mutation {
    /// Creates and applies a new layer, returning the mutation that removes it.
    fn create_layer(
        document: &mut Document,
        name: String,
        opacity: u8,
        visible: bool,
    ) -> Result<Self, SessionError> {
        let index = document.layers().len();
        let restore_active = document.active_layer_id();
        let layer_id = document.add_layer(name)?;
        if opacity != u8::MAX {
            document.set_layer_opacity(layer_id, opacity)?;
        }
        if !visible {
            document.set_layer_visibility(layer_id, false)?;
        }
        let layer = document
            .layer(layer_id)
            .ok_or(SessionError::DocumentInvariant(
                "newly inserted layer cannot be found",
            ))?
            .clone();
        Ok(Self::RemoveLayer {
            layer,
            index,
            restore_active,
        })
    }

    /// Applies in one direction and returns a freshly validated mutation for the opposite one.
    fn apply(&self, document: &mut Document) -> Result<Option<Self>, SessionError> {
        match self {
            Self::Pixels(patch) => {
                let layer =
                    document
                        .layer(patch.layer_id())
                        .ok_or(DocumentError::LayerNotFound {
                            layer_id: patch.layer_id(),
                        })?;
                if layer.revision() == patch.expected_revision() {
                    let inverse = patch.inverse().map_err(DocumentError::from)?;
                    document.apply_patch(patch)?;
                    Ok(Some(Self::Pixels(inverse)))
                } else {
                    // Interleaved edits advance this layer even while an older history entry is
                    // dormant. Rebase to the current revision; Document still verifies the exact
                    // before-pixels, so a logically stale or mismatched edit remains an error.
                    let rebased = rebase_patch(patch, layer.revision())?;
                    let inverse = rebased.inverse().map_err(DocumentError::from)?;
                    document.apply_patch(&rebased)?;
                    Ok(Some(Self::Pixels(inverse)))
                }
            }
            Self::AddLayer { layer, index } => {
                let restore_active = document.active_layer_id();
                document.insert_layer(*index, layer.clone())?;
                Ok(Some(Self::RemoveLayer {
                    layer: layer.clone(),
                    index: *index,
                    restore_active,
                }))
            }
            Self::RemoveLayer {
                layer,
                index,
                restore_active,
            } => {
                let actual_index =
                    document
                        .layer_index(layer.id())
                        .ok_or(DocumentError::LayerNotFound {
                            layer_id: layer.id(),
                        })?;
                if actual_index != *index {
                    return Err(SessionError::HistoryInvariant(
                        "layer order changed outside the session history",
                    ));
                }
                let removed_was_active = document.active_layer_id() == layer.id();
                let removed = document.remove_layer(layer.id())?;
                if removed_was_active {
                    document.set_active_layer(*restore_active)?;
                }
                Ok(Some(Self::AddLayer {
                    layer: removed,
                    index: actual_index,
                }))
            }
            Self::SetVisibility { layer_id, value } => {
                let layer = document
                    .layer(*layer_id)
                    .ok_or(DocumentError::LayerNotFound {
                        layer_id: *layer_id,
                    })?;
                let previous = layer.is_visible();
                if previous == *value {
                    return Ok(None);
                }
                document.set_layer_visibility(*layer_id, *value)?;
                Ok(Some(Self::SetVisibility {
                    layer_id: *layer_id,
                    value: previous,
                }))
            }
            Self::SetOpacity { layer_id, value } => {
                let layer = document
                    .layer(*layer_id)
                    .ok_or(DocumentError::LayerNotFound {
                        layer_id: *layer_id,
                    })?;
                let previous = layer.opacity();
                if previous == *value {
                    return Ok(None);
                }
                document.set_layer_opacity(*layer_id, *value)?;
                Ok(Some(Self::SetOpacity {
                    layer_id: *layer_id,
                    value: previous,
                }))
            }
        }
    }
}

fn rebase_patch(
    patch: &PixelPatch,
    expected_revision: Revision,
) -> Result<PixelPatch, DocumentError> {
    let requested_bytes = patch
        .rect()
        .size()
        .checked_buffer_len()
        .map_err(PatchError::from)?;
    let before = copy_patch_pixels(patch.before(), requested_bytes)?;
    let after = copy_patch_pixels(patch.after(), requested_bytes)?;
    Ok(PixelPatch::new(
        patch.layer_id(),
        expected_revision,
        patch.rect(),
        before,
        after,
    )?)
}

fn copy_patch_pixels(pixels: &[Bgra], requested_bytes: usize) -> Result<Vec<Bgra>, PatchError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(pixels.len())
        .map_err(|_| PatchError::AllocationFailed { requested_bytes })?;
    copy.extend_from_slice(pixels);
    Ok(copy)
}

fn validate_document_resources(
    document: &Document,
    limits: &ResourceLimits,
) -> Result<(), ResourceLimitError> {
    let width = document.size().width();
    let height = document.size().height();
    if width > limits.max_width || height > limits.max_height {
        return Err(ResourceLimitError::Dimensions {
            width,
            height,
            max_width: limits.max_width,
            max_height: limits.max_height,
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels {
        return Err(ResourceLimitError::Pixels {
            actual: pixels,
            max: limits.max_pixels,
        });
    }
    if document.layers().len() > limits.max_layers {
        return Err(ResourceLimitError::Layers {
            actual: document.layers().len(),
            max: limits.max_layers,
        });
    }
    Ok(())
}

pub(crate) fn validate_layer_name(name: &str, limits: &ResourceLimits) -> Result<(), SessionError> {
    if name.is_empty() {
        return Err(SessionError::EmptyLayerName);
    }
    if name.len() > limits.max_layer_name_bytes {
        return Err(ResourceLimitError::LayerNameBytes {
            actual: name.len(),
            max: limits.max_layer_name_bytes,
        }
        .into());
    }
    Ok(())
}

/// Hashes complete layered content while excluding revisions, active layer, and selection.
/// A single baseline fingerprint avoids whole-document snapshots in the history stacks.
fn content_fingerprint(document: &Document) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(document.size().width().to_le_bytes());
    hasher.update(document.size().height().to_le_bytes());
    hasher.update((document.layers().len() as u64).to_le_bytes());
    for layer in document.layers() {
        hasher.update((layer.name().len() as u64).to_le_bytes());
        hasher.update(layer.name().as_bytes());
        hasher.update([u8::from(layer.is_visible())]);
        hasher.update([layer.opacity()]);
        hasher.update([u8::from(layer.is_background())]);
        hasher.update([blend_mode_tag(layer.blend_mode())]);
        for pixel in layer.surface().pixels() {
            hasher.update(pixel.to_bgra_bytes());
        }
    }
    hasher.finalize().into()
}

fn blend_mode_tag(mode: paint_document::BlendMode) -> u8 {
    match mode {
        paint_document::BlendMode::Normal => 0,
        paint_document::BlendMode::Multiply => 1,
        paint_document::BlendMode::Screen => 2,
        paint_document::BlendMode::Overlay => 3,
    }
}

#[cfg(test)]
mod tests {
    use paint_core::Size;

    use super::*;

    #[test]
    fn rebasing_a_patch_changes_only_its_expected_revision() {
        let rect = Rect::new(0, 0, 1, 1).unwrap();
        let patch = PixelPatch::new(
            LayerId::new(7),
            Revision::ZERO,
            rect,
            vec![Bgra::WHITE],
            vec![Bgra::BLACK],
        )
        .unwrap();

        let rebased = rebase_patch(&patch, Revision::new(41)).unwrap();
        assert_eq!(rebased.layer_id(), patch.layer_id());
        assert_eq!(rebased.expected_revision(), Revision::new(41));
        assert_eq!(rebased.rect(), patch.rect());
        assert_eq!(rebased.before(), patch.before());
        assert_eq!(rebased.after(), patch.after());
    }

    #[test]
    fn content_fingerprint_excludes_ui_revisions_but_tracks_exact_content() {
        let size = Size::new(1, 1).unwrap();
        let surface = Surface::filled(size, Bgra::new(1, 2, 3, 4)).unwrap();
        let background = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
        let mut document = Document::from_layers(size, vec![background]).unwrap();
        let second = document.add_layer("second").unwrap();
        let baseline = content_fingerprint(&document);

        document.set_active_layer(LayerId::new(1)).unwrap();
        document
            .set_selection(Some(Rect::new(-1, -1, 2, 2).unwrap()))
            .unwrap();
        assert_eq!(content_fingerprint(&document), baseline);

        document.set_layer_visibility(second, false).unwrap();
        assert_ne!(content_fingerprint(&document), baseline);
        document.set_layer_visibility(second, true).unwrap();
        assert_eq!(content_fingerprint(&document), baseline);
    }
}
