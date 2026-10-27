use paint_core::{Bgra, GeometryError, Rect};
use thiserror::Error;

use crate::{LayerId, Revision};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PatchError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("patch before-pixel length mismatch: expected {expected}, got {actual}")]
    BeforeLength { expected: usize, actual: usize },
    #[error("patch after-pixel length mismatch: expected {expected}, got {actual}")]
    AfterLength { expected: usize, actual: usize },
    #[error("failed to allocate {requested_bytes} bytes for an inverse patch")]
    AllocationFailed { requested_bytes: usize },
    #[error("patch revision overflow while constructing an inverse")]
    RevisionOverflow,
}

/// A validated, packed, row-major pixel change for one layer and revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PixelPatch {
    layer_id: LayerId,
    expected_revision: Revision,
    rect: Rect,
    before: Vec<Bgra>,
    after: Vec<Bgra>,
}

impl PixelPatch {
    pub fn new(
        layer_id: LayerId,
        expected_revision: Revision,
        rect: Rect,
        before: Vec<Bgra>,
        after: Vec<Bgra>,
    ) -> Result<Self, PatchError> {
        let patch = Self {
            layer_id,
            expected_revision,
            rect,
            before,
            after,
        };
        patch.validate()?;
        Ok(patch)
    }

    /// Rechecks all intrinsic patch invariants without touching a document.
    pub fn validate(&self) -> Result<(), PatchError> {
        let expected = self.expected_pixel_count()?;
        if self.before.len() != expected {
            return Err(PatchError::BeforeLength {
                expected,
                actual: self.before.len(),
            });
        }
        if self.after.len() != expected {
            return Err(PatchError::AfterLength {
                expected,
                actual: self.after.len(),
            });
        }
        Ok(())
    }

    pub fn expected_pixel_count(&self) -> Result<usize, PatchError> {
        Ok(self.rect.size().checked_pixel_count()?)
    }

    pub const fn layer_id(&self) -> LayerId {
        self.layer_id
    }

    pub const fn expected_revision(&self) -> Revision {
        self.expected_revision
    }

    pub const fn rect(&self) -> Rect {
        self.rect
    }

    pub fn before(&self) -> &[Bgra] {
        &self.before
    }

    pub fn after(&self) -> &[Bgra] {
        &self.after
    }

    /// Creates the undo patch expected at the revision after this patch is applied.
    pub fn inverse(&self) -> Result<Self, PatchError> {
        let expected_revision = self
            .expected_revision
            .checked_next()
            .ok_or(PatchError::RevisionOverflow)?;
        let requested_bytes = self.rect.size().checked_buffer_len()?;
        let before = copy_pixels(&self.after, requested_bytes)?;
        let after = copy_pixels(&self.before, requested_bytes)?;
        Self::new(self.layer_id, expected_revision, self.rect, before, after)
    }

    /// Consuming inverse variant that swaps owned buffers without allocating.
    pub fn into_inverse(self) -> Result<Self, PatchError> {
        let expected_revision = self
            .expected_revision
            .checked_next()
            .ok_or(PatchError::RevisionOverflow)?;
        Ok(Self {
            layer_id: self.layer_id,
            expected_revision,
            rect: self.rect,
            before: self.after,
            after: self.before,
        })
    }
}

fn copy_pixels(pixels: &[Bgra], requested_bytes: usize) -> Result<Vec<Bgra>, PatchError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(pixels.len())
        .map_err(|_| PatchError::AllocationFailed { requested_bytes })?;
    copy.extend_from_slice(pixels);
    Ok(copy)
}
