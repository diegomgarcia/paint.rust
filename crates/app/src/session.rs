use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{
    Document, DocumentError, Layer, LayerId, PatchError, PixelPatch, Revision, Selection,
    SelectionMask,
};
use sha2::{Digest, Sha256};

use crate::{
    CanvasAnchor, Command, Effect, ResourceLimitError, SemanticSnapshot, SessionError, snapshot,
};

/// Bounds applied to externally supplied traces and session operations.
/// Selected pixels detached from the layer while they are being moved.
pub struct FloatingSelection {
    pub surface: Surface,
    pub bounds: Rect,
}

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
    preview: Option<Document>,
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
            preview: None,
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
            Command::SelectEllipse(rect) => self.select_ellipse(rect),
            Command::SelectLasso(points) => self.select_lasso(points),
            Command::SelectMagicWand {
                point,
                tolerance,
                global,
            } => self.select_magic_wand(point, tolerance, global),
            Command::TranslateSelection { dx, dy } => self.translate_selection(dx, dy),
            Command::ClearSelection => self.clear_selection(),
            Command::Fill(color) => self.fill(color),
            Command::Stroke {
                label: _,
                points,
                size,
                color,
            } => self.stroke(&points, size, color),
            Command::FloodFill {
                point,
                color,
                tolerance,
                global,
            } => self.flood_fill(point, color, tolerance, global),
            Command::FillRect { rect, color } => self.fill_rect(rect, color),
            Command::FillEllipse { rect, color } => self.fill_ellipse(rect, color),
            Command::Gradient {
                start,
                end,
                from,
                to,
            } => self.linear_gradient(start, end, from, to),
            Command::Recolor {
                points,
                size,
                target,
                replacement,
                tolerance,
            } => self.recolor(&points, size, target, replacement, tolerance),
            Command::PlaceText {
                origin,
                content,
                style,
                color,
            } => self.place_text(origin, &content, style, color),
            Command::CloneStamp {
                points,
                size,
                source,
                hardness,
            } => self.clone_stamp(&points, size, source, hardness),
            Command::Move { dx, dy } => self.translate(dx, dy),
            Command::Invert => self.invert(),
            Command::ApplyEffect(effect) => self.apply_effect(effect),
            Command::FlipLayerHorizontal => self.flip_layer_horizontal(),
            Command::FlipLayerVertical => self.flip_layer_vertical(),
            Command::FlipImageHorizontal => self.flip_image(true),
            Command::FlipImageVertical => self.flip_image(false),
            Command::CropToSelection => self.crop_to_selection(),
            Command::ResizeImage { width, height } => self.resize_image(width, height),
            Command::CanvasSize {
                width,
                height,
                anchor,
                fill,
            } => self.resize_canvas(width, height, anchor, fill),
            Command::Rotate90Clockwise => {
                self.rotate_image(paint_raster::Rotation::Rotate90Clockwise)
            }
            Command::Rotate90CounterClockwise => {
                self.rotate_image(paint_raster::Rotation::Rotate90CounterClockwise)
            }
            Command::Rotate180 => self.rotate_image(paint_raster::Rotation::Rotate180),
            Command::FlattenImage => self.flatten_image(),
            Command::AddLayer {
                name,
                opacity,
                visible,
            } => self.add_layer(name, opacity, visible),
            Command::SetActiveLayer { index } => self.set_active_layer(index),
            Command::RemoveLayer { index } => self.remove_layer(index),
            Command::DuplicateLayer { index } => self.duplicate_layer(index),
            Command::MergeLayerDown { index } => self.merge_layer_down(index),
            Command::MoveLayer { index, offset } => self.move_layer(index, offset),
            Command::RenameLayer { index, name } => self.rename_layer(index, name),
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

    /// Replaces the selection with a real pixel-mask ellipse clipped to the canvas.
    pub fn select_ellipse(&mut self, rect: Rect) -> Result<bool, SessionError> {
        let Some(mask) = SelectionMask::ellipse_clipped(rect, self.document.size())? else {
            return self.clear_selection();
        };
        Ok(self.document.set_selection_mask(mask)?)
    }

    /// Replaces the selection with the pixel mask enclosed by an implicitly closed lasso path.
    pub fn select_lasso(&mut self, points: Vec<Point>) -> Result<bool, SessionError> {
        let Some(mask) = SelectionMask::polygon(&points)? else {
            return self.clear_selection();
        };
        let Some(mask) = mask.clipped_to(self.document.size())? else {
            return self.clear_selection();
        };
        Ok(self.document.set_selection_mask(mask)?)
    }

    /// Replaces the selection with a magic-wand mask sampled from the active layer.
    pub fn select_magic_wand(
        &mut self,
        point: Point,
        tolerance: u8,
        global: bool,
    ) -> Result<bool, SessionError> {
        let mask = paint_raster::magic_wand_mask(
            &self.document,
            self.document.active_layer_id(),
            point,
            tolerance,
            global,
        )?;
        match mask {
            Some(mask) => Ok(self.document.set_selection_mask(mask)?),
            None => self.clear_selection(),
        }
    }

    /// Translates the rectangle or mask selection without editing pixels or content history.
    pub fn translate_selection(&mut self, dx: i32, dy: i32) -> Result<bool, SessionError> {
        let Some(selection) = self.document.selection_model().cloned() else {
            return Ok(false);
        };
        let translated = selection.translated(i64::from(dx), i64::from(dy))?;
        Ok(self.document.set_selection_shape(Some(translated))?)
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

    /// Starts a live preview; the document is rewound to this snapshot on every preview update.
    pub fn begin_preview(&mut self) {
        if self.preview.is_none() {
            self.preview = Some(self.document.clone());
        }
    }

    /// Whether a live preview is currently holding an undo-free document snapshot.
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }

    /// Starts a live stroke preview; the document is rewound to this snapshot each frame.
    pub fn begin_stroke_preview(&mut self) {
        self.begin_preview();
    }

    fn rewind_preview(&mut self) -> Result<Document, SessionError> {
        let Some(before) = self.preview.clone() else {
            return Err(SessionError::DocumentInvariant("preview was not started"));
        };
        self.document = before.clone();
        Ok(before)
    }

    /// Recomputes the whole in-progress stroke against the preview snapshot.
    pub fn preview_stroke(
        &mut self,
        points: &[Point],
        size: u32,
        color: Option<Bgra>,
    ) -> Result<bool, SessionError> {
        self.rewind_preview()?;
        let patch = paint_raster::stroke(
            &self.document,
            self.document.active_layer_id(),
            points,
            size,
            color,
        )?;
        self.apply_preview_patch(patch)
    }

    /// Recomputes a text placement against the preview snapshot.
    pub fn preview_text(
        &mut self,
        origin: Point,
        content: &str,
        style: paint_raster::TextStyle,
        color: Bgra,
    ) -> Result<bool, SessionError> {
        self.rewind_preview()?;
        let patch = paint_raster::text(
            &self.document,
            self.document.active_layer_id(),
            origin,
            content,
            style,
            color,
        )?;
        self.apply_preview_patch(patch)
    }

    /// Recomputes a clone stamp stroke against the preview snapshot.
    pub fn preview_clone_stamp(
        &mut self,
        points: &[Point],
        size: u32,
        source: Point,
        hardness: u8,
    ) -> Result<bool, SessionError> {
        self.rewind_preview()?;
        let patch = paint_raster::clone_stamp(
            &self.document,
            self.document.active_layer_id(),
            points,
            size,
            source,
            hardness,
        )?;
        self.apply_preview_patch(patch)
    }

    fn apply_preview_patch(&mut self, patch: Option<PixelPatch>) -> Result<bool, SessionError> {
        let changed = if let Some(patch) = patch {
            self.document.apply_patch(&patch)?;
            true
        } else {
            false
        };
        self.refresh_content_fingerprint();
        Ok(changed)
    }

    /// Commits the live preview as one undoable document mutation.
    pub fn finish_preview(&mut self) -> Result<bool, SessionError> {
        let Some(before) = self.preview.take() else {
            return Ok(false);
        };
        if before == self.document {
            return Ok(false);
        }
        let after = self.document.clone();
        let inverse = Mutation::DocumentSnapshot {
            before: Box::new(after),
            after: Box::new(before),
        };
        self.undo.push(inverse);
        self.redo.clear();
        self.refresh_content_fingerprint();
        Ok(true)
    }

    /// Commits the live stroke as one undoable document mutation.
    pub fn finish_stroke_preview(&mut self) -> Result<bool, SessionError> {
        self.finish_preview()
    }

    /// Discards a live preview and restores the exact pre-preview document.
    pub fn cancel_preview(&mut self) {
        if let Some(before) = self.preview.take() {
            self.document = before;
            self.refresh_content_fingerprint();
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

    pub fn flood_fill(
        &mut self,
        point: Point,
        color: Bgra,
        tolerance: u8,
        global: bool,
    ) -> Result<bool, SessionError> {
        let patch = if global {
            paint_raster::global_fill(
                &self.document,
                self.document.active_layer_id(),
                point,
                color,
                tolerance,
            )?
        } else {
            paint_raster::flood_fill(
                &self.document,
                self.document.active_layer_id(),
                point,
                color,
                tolerance,
            )?
        };
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
        points: &[Point],
        size: u32,
        target: Bgra,
        replacement: Bgra,
        tolerance: u8,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::recolor_stroke(
            &self.document,
            self.document.active_layer_id(),
            points,
            size,
            target,
            replacement,
            tolerance,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn place_text(
        &mut self,
        origin: Point,
        content: &str,
        style: paint_raster::TextStyle,
        color: Bgra,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::text(
            &self.document,
            self.document.active_layer_id(),
            origin,
            content,
            style,
            color,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn clone_stamp(
        &mut self,
        points: &[Point],
        size: u32,
        source: Point,
        hardness: u8,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::clone_stamp(
            &self.document,
            self.document.active_layer_id(),
            points,
            size,
            source,
            hardness,
        )?;
        match patch {
            Some(patch) => self.apply_new_mutation(Mutation::Pixels(patch)),
            None => Ok(false),
        }
    }

    pub fn translate(&mut self, dx: i32, dy: i32) -> Result<bool, SessionError> {
        let transform_selection = |selection: &Selection| {
            selection
                .translated(i64::from(dx), i64::from(dy))
                .map_err(SessionError::from)
        };
        let before_selection = self.document.selection_model().cloned();
        let after_selection = before_selection
            .as_ref()
            .map(transform_selection)
            .transpose()?;
        let mut candidate = self.document.clone();
        let patch = paint_raster::translate(&candidate, candidate.active_layer_id(), dx, dy)?;
        let Some(patch) = patch else {
            return Ok(false);
        };
        // Moving selected content changes both pixels and selection state. One patch mutation
        // keeps a single undo step restoring each side, including a non-rectangular mask.
        let mutation = Mutation::PixelsWithSelection {
            patch,
            before_selection,
            after_selection,
        };
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    /// Lifts the selected pixels into a floating surface and clears their source area.
    /// Pixels outside a mask selection stay transparent in the floating surface.
    pub fn lift_selection(&mut self) -> Result<Option<FloatingSelection>, SessionError> {
        let Some(selection) = self.document.selection_model().cloned() else {
            return Ok(None);
        };
        let Some(bounds) = selection.bounds().clip_to(self.document.size()) else {
            return Ok(None);
        };
        let layer = self.document.active_layer();
        let size = Size::new(bounds.width(), bounds.height())
            .map_err(|_| SessionError::DocumentInvariant("invalid floating selection size"))?;
        let mut surface = Surface::new(size)?;
        for y in 0..bounds.height() {
            for x in 0..bounds.width() {
                let canvas_x = bounds.left() + i64::from(x);
                let canvas_y = bounds.top() + i64::from(y);
                if !selection.contains_xy(canvas_x, canvas_y) {
                    continue;
                }
                let point = Point::new(
                    (bounds.left() as u32).saturating_add(x),
                    (bounds.top() as u32).saturating_add(y),
                );
                let pixel = *layer.surface().pixel(point)?;
                let target = Point::new(x, y);
                surface.set_pixel(target, pixel)?;
            }
        }
        let patch = paint_raster::fill(
            &self.document,
            self.document.active_layer_id(),
            Bgra::TRANSPARENT,
        )?;
        if let Some(patch) = patch {
            self.apply_new_mutation(Mutation::Pixels(patch))?;
        }
        Ok(Some(FloatingSelection { surface, bounds }))
    }

    /// Composites a floating surface at the given canvas destination.
    pub fn stamp_floating(
        &mut self,
        surface: &Surface,
        dest_x: i64,
        dest_y: i64,
    ) -> Result<bool, SessionError> {
        let patch = paint_raster::stamp(
            &self.document,
            self.document.active_layer_id(),
            surface,
            dest_x,
            dest_y,
        )?;
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

    /// Applies one implemented effect through a single generic dispatch.
    ///
    /// The resulting `PixelPatch` is committed as one inverse-based mutation, so undo and redo
    /// treat the complete effect as a single history entry. No-op effects return `Ok(false)`.
    pub fn apply_effect(&mut self, effect: Effect) -> Result<bool, SessionError> {
        let layer_id = self.document.active_layer_id();
        let patch = match effect {
            Effect::Grayscale => paint_raster::grayscale(&self.document, layer_id)?,
            Effect::Sepia => paint_raster::sepia(&self.document, layer_id)?,
            Effect::GaussianBlur { radius } => {
                paint_raster::gaussian_blur(&self.document, layer_id, radius)?
            }
        };
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

    pub fn flip_image(&mut self, horizontal: bool) -> Result<bool, SessionError> {
        let before = self.document.clone();
        let mut candidate = self.document.clone();
        let ids: Vec<LayerId> = candidate.layers().iter().map(Layer::id).collect();
        for id in ids {
            let patch = if horizontal {
                paint_raster::flip_horizontal(&candidate, id)?
            } else {
                paint_raster::flip_vertical(&candidate, id)?
            };
            if let Some(patch) = patch {
                candidate.apply_patch(&patch)?;
            }
        }
        let mutation = Mutation::DocumentSnapshot {
            before: Box::new(before),
            after: Box::new(candidate.clone()),
        };
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    /// Crops every layer to the selection clipped to the canvas.
    ///
    /// A missing, non-intersecting, or full-canvas selection is a no-op. On success the
    /// selection is cleared because its coordinates are relative to the old canvas.
    pub fn crop_to_selection(&mut self) -> Result<bool, SessionError> {
        let Some(selection) = self.document.selection() else {
            return Ok(false);
        };
        let Some(bounds) = selection.clip_to(self.document.size()) else {
            return Ok(false);
        };
        if bounds == self.document.canvas_rect() {
            return Ok(false);
        }
        let surfaces = paint_raster::crop_surfaces(&self.document, bounds)?;
        let mut candidate = self.document.clone();
        candidate.replace_geometry(bounds.size(), surfaces)?;
        candidate.clear_selection()?;
        self.apply_document_candidate(candidate)
    }

    /// Bilinearly resamples every layer to `width × height` and clears the selection.
    pub fn resize_image(&mut self, width: u32, height: u32) -> Result<bool, SessionError> {
        let new_size = Size::new(width, height)?;
        self.validate_requested_size(new_size)?;
        if new_size == self.document.size() {
            return Ok(false);
        }
        let surfaces = paint_raster::resize_surfaces(&self.document, new_size)?;
        let mut candidate = self.document.clone();
        candidate.replace_geometry(new_size, surfaces)?;
        candidate.clear_selection()?;
        self.apply_document_candidate(candidate)
    }

    /// Pads or crops the canvas around the existing layers using `anchor` and `fill`.
    ///
    /// Existing pixels move by [`CanvasAnchor::offsets`]. The selection is shifted by the
    /// same offset and clipped to the new canvas, or cleared when it no longer intersects.
    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        anchor: CanvasAnchor,
        fill: Bgra,
    ) -> Result<bool, SessionError> {
        let new_size = Size::new(width, height)?;
        self.validate_requested_size(new_size)?;
        if new_size == self.document.size() {
            return Ok(false);
        }
        let (offset_x, offset_y) = anchor.offsets(self.document.size(), new_size);
        let surfaces =
            paint_raster::canvas_surfaces(&self.document, new_size, offset_x, offset_y, fill)?;
        let mut candidate = self.document.clone();
        candidate.replace_geometry(new_size, surfaces)?;
        if let Some(selection) = self.document.selection() {
            let shifted = selection
                .left()
                .checked_add(offset_x)
                .zip(selection.top().checked_add(offset_y))
                .zip(selection.right().checked_add(offset_x))
                .zip(selection.bottom().checked_add(offset_y))
                .and_then(|(((left, top), right), bottom)| Rect::new(left, top, right, bottom).ok())
                .and_then(|rect| rect.clip_to(new_size));
            match shifted {
                Some(rect) => {
                    candidate.set_selection(Some(rect))?;
                }
                None => {
                    candidate.clear_selection()?;
                }
            }
        }
        self.apply_document_candidate(candidate)
    }

    /// Rotates every layer and swaps dimensions for quarter turns. The selection is cleared.
    pub fn rotate_image(&mut self, rotation: paint_raster::Rotation) -> Result<bool, SessionError> {
        let new_size = rotation.rotated_size(self.document.size());
        let surfaces = paint_raster::rotate_surfaces(&self.document, rotation)?;
        let mut candidate = self.document.clone();
        candidate.replace_geometry(new_size, surfaces)?;
        candidate.clear_selection()?;
        self.apply_document_candidate(candidate)
    }

    /// Applies a fully built candidate document as one snapshot-based history mutation.
    fn apply_document_candidate(&mut self, candidate: Document) -> Result<bool, SessionError> {
        if candidate == self.document {
            return Ok(false);
        }
        let mutation = Mutation::DocumentSnapshot {
            before: Box::new(self.document.clone()),
            after: Box::new(candidate),
        };
        let mut applied = self.document.clone();
        let Some(inverse) = mutation.apply(&mut applied)? else {
            return Ok(false);
        };
        self.commit_new(applied, inverse);
        Ok(true)
    }

    fn validate_requested_size(&self, size: Size) -> Result<(), SessionError> {
        let width = size.width();
        let height = size.height();
        if width > self.limits.max_width || height > self.limits.max_height {
            return Err(ResourceLimitError::Dimensions {
                width,
                height,
                max_width: self.limits.max_width,
                max_height: self.limits.max_height,
            }
            .into());
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > self.limits.max_pixels {
            return Err(ResourceLimitError::Pixels {
                actual: pixels,
                max: self.limits.max_pixels,
            }
            .into());
        }
        Ok(())
    }

    pub fn flatten_image(&mut self) -> Result<bool, SessionError> {
        let before = self.document.clone();
        let composed = paint_raster::compose(&self.document)?;
        let mut candidate = Document::new(self.document.size())?;
        let layer_id = candidate.active_layer_id();
        let rect = candidate.canvas_rect();
        let prior = candidate.active_layer().surface().read_region(rect)?;
        let patch = PixelPatch::new(
            layer_id,
            candidate.active_layer().revision(),
            rect,
            prior,
            composed.pixels().to_vec(),
        )
        .map_err(DocumentError::from)?;
        candidate.apply_patch(&patch)?;
        let mutation = Mutation::DocumentSnapshot {
            before: Box::new(before),
            after: Box::new(candidate.clone()),
        };
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
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

    pub fn remove_layer(&mut self, index: usize) -> Result<bool, SessionError> {
        let layer = self
            .document
            .layers()
            .get(index)
            .ok_or(DocumentError::LayerIndexOutOfBounds {
                index,
                len: self.document.layers().len(),
            })?
            .clone();
        let restore_active = if self.document.active_layer_id() == layer.id() {
            self.document
                .layers()
                .get(index + 1)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|index| self.document.layers().get(index))
                })
                .map_or(self.document.active_layer_id(), |layer| layer.id())
        } else {
            self.document.active_layer_id()
        };
        let mutation = Mutation::RemoveLayer {
            layer,
            index,
            restore_active,
        };
        let mut candidate = self.document.clone();
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    pub fn duplicate_layer(&mut self, index: usize) -> Result<bool, SessionError> {
        if self.document.layers().len() >= self.limits.max_layers {
            return Err(ResourceLimitError::Layers {
                actual: self.document.layers().len().saturating_add(1),
                max: self.limits.max_layers,
            }
            .into());
        }
        let restore_active = self.document.active_layer_id();
        let mut candidate = self.document.clone();
        let duplicate = candidate.duplicate_layer(index)?;
        let layer = candidate
            .layer(duplicate)
            .ok_or(SessionError::DocumentInvariant(
                "duplicated layer cannot be found",
            ))?
            .clone();
        let inverse = Mutation::RemoveLayer {
            layer,
            index: index + 1,
            restore_active,
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    pub fn move_layer(&mut self, index: usize, offset: i32) -> Result<bool, SessionError> {
        let len = self.document.layers().len();
        let target = (index as i64 + i64::from(offset)).clamp(0, len as i64 - 1) as usize;
        if target == index {
            return Ok(false);
        }
        let layer_id = self.layer_id(index)?;
        let mutation = Mutation::MoveLayer {
            layer_id,
            from: index,
            to: target,
        };
        let mut candidate = self.document.clone();
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
        self.commit_new(candidate, inverse);
        Ok(true)
    }

    /// Renames a layer as one undoable content edit without changing the active layer.
    pub fn rename_layer(&mut self, index: usize, name: String) -> Result<bool, SessionError> {
        validate_layer_name(&name, &self.limits)?;
        let layer_id = self.layer_id(index)?;
        self.apply_new_mutation(Mutation::SetName {
            layer_id,
            value: name,
        })
    }

    pub fn merge_layer_down(&mut self, index: usize) -> Result<bool, SessionError> {
        if index == 0 {
            return Ok(false);
        }
        let upper = self
            .document
            .layers()
            .get(index)
            .ok_or(DocumentError::LayerIndexOutOfBounds {
                index,
                len: self.document.layers().len(),
            })?
            .clone();
        let lower = self
            .document
            .layers()
            .get(index - 1)
            .ok_or(DocumentError::LayerIndexOutOfBounds {
                index: index - 1,
                len: self.document.layers().len(),
            })?
            .clone();
        let rect = self.document.canvas_rect();
        let before = lower.surface().read_region(rect)?;
        let mut after = before.clone();
        if upper.is_visible() {
            for y in 0..rect.height() {
                for x in 0..rect.width() {
                    let point = Point::new(rect.left() as u32 + x, rect.top() as u32 + y);
                    let source = *upper.surface().pixel(point)?;
                    let target = *lower.surface().pixel(point)?;
                    let index = (y * rect.width() + x) as usize;
                    after[index] = paint_raster::normal_over(target, source, upper.opacity());
                }
            }
        }
        let patch = PixelPatch::new(lower.id(), lower.revision(), rect, before, after)
            .map_err(DocumentError::from)?;
        let mutation = Mutation::MergeDown {
            patch,
            upper,
            upper_index: index,
            restore_active: self.document.active_layer_id(),
        };
        let mut candidate = self.document.clone();
        let Some(inverse) = mutation.apply(&mut candidate)? else {
            return Ok(false);
        };
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
    PixelsWithSelection {
        patch: PixelPatch,
        before_selection: Option<Selection>,
        after_selection: Option<Selection>,
    },
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
    SetName {
        layer_id: LayerId,
        value: String,
    },
    MoveLayer {
        layer_id: LayerId,
        from: usize,
        to: usize,
    },
    MergeDown {
        patch: PixelPatch,
        upper: Layer,
        upper_index: usize,
        restore_active: LayerId,
    },
    UnmergeDown {
        patch: PixelPatch,
        upper: Layer,
        upper_index: usize,
        restore_active: LayerId,
    },
    DocumentSnapshot {
        before: Box<Document>,
        after: Box<Document>,
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
            Self::Pixels(patch) => Ok(Some(Self::Pixels(apply_pixel_patch(document, patch)?))),
            Self::PixelsWithSelection {
                patch,
                before_selection,
                after_selection,
            } => {
                let inverse_patch = apply_pixel_patch(document, patch)?;
                document.set_selection_shape(after_selection.clone())?;
                Ok(Some(Self::PixelsWithSelection {
                    patch: inverse_patch,
                    before_selection: after_selection.clone(),
                    after_selection: before_selection.clone(),
                }))
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
            Self::MoveLayer { layer_id, from, to } => {
                document.move_layer(*layer_id, *to)?;
                Ok(Some(Self::MoveLayer {
                    layer_id: *layer_id,
                    from: *to,
                    to: *from,
                }))
            }
            Self::MergeDown {
                patch,
                upper,
                upper_index,
                restore_active,
            } => {
                let inverse_patch = patch.inverse().map_err(DocumentError::from)?;
                document.apply_patch(patch)?;
                let removed = document.remove_layer(upper.id())?;
                document.set_active_layer(patch.layer_id())?;
                Ok(Some(Self::UnmergeDown {
                    patch: inverse_patch,
                    upper: removed,
                    upper_index: *upper_index,
                    restore_active: *restore_active,
                }))
            }
            Self::UnmergeDown {
                patch,
                upper,
                upper_index,
                restore_active,
            } => {
                let forward = patch.inverse().map_err(DocumentError::from)?;
                document.insert_layer(*upper_index, upper.clone())?;
                document.apply_patch(patch)?;
                document.set_active_layer(*restore_active)?;
                Ok(Some(Self::MergeDown {
                    patch: forward,
                    upper: upper.clone(),
                    upper_index: *upper_index,
                    restore_active: *restore_active,
                }))
            }
            Self::DocumentSnapshot { before, after } => {
                let inverse = Self::DocumentSnapshot {
                    before: after.clone(),
                    after: before.clone(),
                };
                *document = (**after).clone();
                Ok(Some(inverse))
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
            Self::SetName { layer_id, value } => {
                let layer = document
                    .layer(*layer_id)
                    .ok_or(DocumentError::LayerNotFound {
                        layer_id: *layer_id,
                    })?;
                let previous = layer.name().to_owned();
                if previous == *value {
                    return Ok(None);
                }
                document.set_layer_name(*layer_id, value.clone())?;
                Ok(Some(Self::SetName {
                    layer_id: *layer_id,
                    value: previous,
                }))
            }
        }
    }
}

/// Applies one forward or inverse pixel patch and returns the opposite direction's patch.
fn apply_pixel_patch(
    document: &mut Document,
    patch: &PixelPatch,
) -> Result<PixelPatch, SessionError> {
    let layer = document
        .layer(patch.layer_id())
        .ok_or(DocumentError::LayerNotFound {
            layer_id: patch.layer_id(),
        })?;
    if layer.revision() == patch.expected_revision() {
        let inverse = patch.inverse().map_err(DocumentError::from)?;
        document.apply_patch(patch)?;
        Ok(inverse)
    } else {
        // Interleaved edits advance this layer even while an older history entry is dormant.
        // Rebase to the current revision; Document still verifies the exact before-pixels, so a
        // logically stale or mismatched edit remains an error.
        let rebased = rebase_patch(patch, layer.revision())?;
        let inverse = rebased.inverse().map_err(DocumentError::from)?;
        document.apply_patch(&rebased)?;
        Ok(inverse)
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
