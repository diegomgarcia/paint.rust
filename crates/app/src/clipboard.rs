use paint_core::{Rect, Size, Surface};
use paint_document::{Selection, SelectionMask};

use crate::SessionError;

/// Pixels captured by Edit → Copy or Edit → Cut.
///
/// The surface is stored in clipboard-local coordinates: its size exactly matches `bounds` and
/// pixels outside a non-rectangular selection are transparent. `mask` keeps the exact selected
/// shape so Paste can reinstall the same selection; it is `None` for rectangular content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardContent {
    surface: Surface,
    bounds: Rect,
    mask: Option<SelectionMask>,
}

impl ClipboardContent {
    /// Builds rectangular clipboard content from a surface and its canvas-space bounds.
    pub fn new(surface: Surface, bounds: Rect) -> Result<Self, SessionError> {
        Self::with_mask(surface, bounds, None)
    }

    pub(crate) fn with_mask(
        surface: Surface,
        bounds: Rect,
        mask: Option<SelectionMask>,
    ) -> Result<Self, SessionError> {
        if surface.size() != bounds.size() {
            return Err(SessionError::DocumentInvariant(
                "clipboard surface size must match its bounds",
            ));
        }
        Ok(Self {
            surface,
            bounds,
            mask,
        })
    }

    pub const fn surface(&self) -> &Surface {
        &self.surface
    }

    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn mask(&self) -> Option<&SelectionMask> {
        self.mask.as_ref()
    }

    /// Returns where Paste should place this content on a canvas of `canvas` size.
    ///
    /// On-canvas and partially visible content keeps its original bounds; content that does not
    /// intersect the canvas at all lands at the canvas origin.
    pub fn destination(&self, canvas: Size) -> (i64, i64) {
        if self.bounds.intersection(canvas.rect()).is_some() {
            (self.bounds.left(), self.bounds.top())
        } else {
            (0, 0)
        }
    }

    /// Builds the selection that Paste installs for this content at `dest_x`, `dest_y`.
    ///
    /// Rectangular content produces a rectangle; masked content shifts its exact mask.
    pub fn selection_at(
        &self,
        dest_x: i64,
        dest_y: i64,
    ) -> Result<Option<Selection>, SessionError> {
        let dx = dest_x
            .checked_sub(self.bounds.left())
            .ok_or(SessionError::DocumentInvariant(
                "clipboard paste offset overflowed",
            ))?;
        let dy = dest_y
            .checked_sub(self.bounds.top())
            .ok_or(SessionError::DocumentInvariant(
                "clipboard paste offset overflowed",
            ))?;
        match &self.mask {
            Some(mask) => Ok(Some(Selection::from_mask(mask.translated(dx, dy)?))),
            None => Ok(Some(Selection::rectangle(Rect::from_xywh(
                dest_x,
                dest_y,
                self.bounds.width(),
                self.bounds.height(),
            )?))),
        }
    }
}
