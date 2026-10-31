use paint_core::{GeometryError, Point, Rect, Size};
use thiserror::Error;

/// Errors produced while building, indexing, clipping, or translating a selection mask.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SelectionError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("selection mask flag length mismatch: expected {expected}, got {actual}")]
    FlagLength { expected: usize, actual: usize },
    #[error("selection mask coordinate ({x}, {y}) is outside mask bounds {bounds:?}")]
    OutOfBounds { x: i64, y: i64, bounds: Rect },
    #[error("selection mask dimensions {width}x{height} exceed the addressable range")]
    MaskTooLarge { width: u32, height: u32 },
    #[error("failed to allocate {requested_bytes} bytes for a selection mask")]
    AllocationFailed { requested_bytes: usize },
    #[error("selection mask bounds overflow while translating by ({dx}, {dy})")]
    TranslateOverflow { dx: i64, dy: i64 },
}

/// A packed bitmap covering a signed, half-open rectangle.
///
/// Bits are stored row-major with `bounds.width()` columns. Bits outside the final word of each
/// row are always zero, so structurally equal masks compare equal regardless of construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionMask {
    bounds: Rect,
    row_words: usize,
    words: Vec<u64>,
    selected: usize,
}

impl SelectionMask {
    /// Creates a mask whose rectangle covers `bounds` but selects no pixels.
    pub fn empty(bounds: Rect) -> Result<Self, SelectionError> {
        let (row_words, word_count) = layout(bounds)?;
        let requested_bytes = word_count.saturating_mul(std::mem::size_of::<u64>());
        let mut words = Vec::new();
        words
            .try_reserve_exact(word_count)
            .map_err(|_| SelectionError::AllocationFailed { requested_bytes })?;
        words.resize(word_count, 0);
        Ok(Self {
            bounds,
            row_words,
            words,
            selected: 0,
        })
    }

    /// Creates a mask that selects every pixel in `bounds`.
    pub fn full(bounds: Rect) -> Result<Self, SelectionError> {
        let mut mask = Self::empty(bounds)?;
        let width = usize::try_from(bounds.width()).map_err(|_| SelectionError::MaskTooLarge {
            width: bounds.width(),
            height: bounds.height(),
        })?;
        let height =
            usize::try_from(bounds.height()).map_err(|_| SelectionError::MaskTooLarge {
                width: bounds.width(),
                height: bounds.height(),
            })?;
        for row in 0..height {
            for word in 0..mask.row_words {
                mask.words[row * mask.row_words + word] = u64::MAX;
            }
        }
        if width % 64 != 0 {
            let tail = (1_u64 << (width % 64)) - 1;
            for row in 0..height {
                mask.words[row * mask.row_words + mask.row_words - 1] = tail;
            }
        }
        mask.selected = width
            .checked_mul(height)
            .ok_or(SelectionError::MaskTooLarge {
                width: bounds.width(),
                height: bounds.height(),
            })?;
        Ok(mask)
    }

    /// Rasterizes an ellipse by testing pixel centers, matching `paint_raster::fill_ellipse`.
    pub fn ellipse(bounds: Rect) -> Result<Self, SelectionError> {
        Self::ellipse_region(bounds, bounds)
    }

    /// Rasterizes the part of `ellipse` that overlaps the canvas without distorting its shape.
    ///
    /// The mask bounds are the ellipse/canvas intersection, so a partially off-canvas drag still
    /// selects the original ellipse geometry rather than an ellipse re-inscribed in the clip.
    pub fn ellipse_clipped(ellipse: Rect, canvas: Size) -> Result<Option<Self>, SelectionError> {
        let Some(bounds) = ellipse.intersection(canvas.rect()) else {
            return Ok(None);
        };
        let mask = Self::ellipse_region(ellipse, bounds)?;
        if mask.is_empty() {
            Ok(None)
        } else {
            Ok(Some(mask))
        }
    }

    fn ellipse_region(ellipse: Rect, bounds: Rect) -> Result<Self, SelectionError> {
        let mut mask = Self::empty(bounds)?;
        let center_x = (ellipse.left() as f64 + ellipse.right() as f64) / 2.0;
        let center_y = (ellipse.top() as f64 + ellipse.bottom() as f64) / 2.0;
        let radius_x = f64::from(ellipse.width()) / 2.0;
        let radius_y = f64::from(ellipse.height()) / 2.0;
        for y in 0..bounds.height() {
            let pixel_y = (bounds.top() + i64::from(y)) as f64 + 0.5;
            for x in 0..bounds.width() {
                let pixel_x = (bounds.left() + i64::from(x)) as f64 + 0.5;
                let normalized_x = (pixel_x - center_x) / radius_x;
                let normalized_y = (pixel_y - center_y) / radius_y;
                if normalized_x * normalized_x + normalized_y * normalized_y <= 1.0 {
                    mask.set_xy(
                        bounds.left() + i64::from(x),
                        bounds.top() + i64::from(y),
                        true,
                    )?;
                }
            }
        }
        Ok(mask)
    }

    /// Fills an implicitly closed polygon through the pixel centers it encloses.
    ///
    /// The result is `None` for fewer than three vertices or a path with no enclosed pixels.
    pub fn polygon(points: &[Point]) -> Result<Option<Self>, SelectionError> {
        if points.len() < 3 {
            return Ok(None);
        }
        let mut min_x = points[0].x;
        let mut min_y = points[0].y;
        let mut max_x = points[0].x;
        let mut max_y = points[0].y;
        for point in points {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        let bounds = Rect::new(
            i64::from(min_x),
            i64::from(min_y),
            i64::from(max_x) + 1,
            i64::from(max_y) + 1,
        )?;
        let mut mask = Self::empty(bounds)?;
        let width = copy_to_usize(bounds.width(), bounds)?;
        let height = copy_to_usize(bounds.height(), bounds)?;
        let too_large = || SelectionError::MaskTooLarge {
            width: bounds.width(),
            height: bounds.height(),
        };
        for row in 0..height {
            let row_offset = i64::try_from(row).map_err(|_| too_large())?;
            let scan_y = (bounds.top() + row_offset) as f64 + 0.5;
            let mut crossings = Vec::new();
            for index in 0..points.len() {
                let start = points[index];
                let end = points[(index + 1) % points.len()];
                let start_y = f64::from(start.y);
                let end_y = f64::from(end.y);
                let spans =
                    (start_y <= scan_y && scan_y < end_y) || (end_y <= scan_y && scan_y < start_y);
                if !spans {
                    continue;
                }
                let fraction = (scan_y - start_y) / (end_y - start_y);
                crossings
                    .push(f64::from(start.x) + fraction * (f64::from(end.x) - f64::from(start.x)));
            }
            crossings.sort_by(f64::total_cmp);
            for column in 0..width {
                let column_offset = i64::try_from(column).map_err(|_| too_large())?;
                let pixel_x = (bounds.left() + column_offset) as f64 + 0.5;
                let inside = crossings
                    .chunks_exact(2)
                    .any(|pair| pixel_x >= pair[0] && pixel_x < pair[1]);
                if inside {
                    mask.set_xy(
                        bounds.left() + column_offset,
                        bounds.top() + row_offset,
                        true,
                    )?;
                }
            }
        }
        if mask.is_empty() {
            Ok(None)
        } else {
            Ok(Some(mask))
        }
    }

    /// Converts one boolean per canvas pixel into a tight mask.
    ///
    /// `None` means no pixel was selected. The returned bounds only cover selected pixels.
    pub fn from_canvas_flags(size: Size, flags: &[bool]) -> Result<Option<Self>, SelectionError> {
        let expected = size.checked_pixel_count()?;
        if flags.len() != expected {
            return Err(SelectionError::FlagLength {
                expected,
                actual: flags.len(),
            });
        }
        let width = copy_to_usize(size.width(), size.rect())?;
        let mut min_x = u32::MAX;
        let mut min_y = u32::MAX;
        let mut max_x = 0;
        let mut max_y = 0;
        let mut any = false;
        for (index, selected) in flags.iter().copied().enumerate() {
            if !selected {
                continue;
            }
            let x = u32::try_from(index % width).map_err(|_| SelectionError::MaskTooLarge {
                width: size.width(),
                height: size.height(),
            })?;
            let y = u32::try_from(index / width).map_err(|_| SelectionError::MaskTooLarge {
                width: size.width(),
                height: size.height(),
            })?;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            any = true;
        }
        if !any {
            return Ok(None);
        }
        let bounds = Rect::new(
            i64::from(min_x),
            i64::from(min_y),
            i64::from(max_x) + 1,
            i64::from(max_y) + 1,
        )?;
        let mut mask = Self::empty(bounds)?;
        for (index, selected) in flags.iter().copied().enumerate() {
            if !selected {
                continue;
            }
            let x = u32::try_from(index % width).map_err(|_| SelectionError::MaskTooLarge {
                width: size.width(),
                height: size.height(),
            })?;
            let y = u32::try_from(index / width).map_err(|_| SelectionError::MaskTooLarge {
                width: size.width(),
                height: size.height(),
            })?;
            mask.set_xy(i64::from(x), i64::from(y), true)?;
        }
        Ok(Some(mask))
    }

    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    pub const fn selected_count(&self) -> usize {
        self.selected
    }

    pub fn is_empty(&self) -> bool {
        self.selected == 0
    }

    pub fn is_all_selected(&self) -> bool {
        self.pixel_count() == self.selected
    }

    pub fn contains(&self, point: Point) -> bool {
        self.contains_xy(i64::from(point.x), i64::from(point.y))
    }

    pub fn contains_xy(&self, x: i64, y: i64) -> bool {
        self.locate(x, y)
            .is_some_and(|(word, bit)| self.words[word] & bit != 0)
    }

    pub fn set_xy(&mut self, x: i64, y: i64, selected: bool) -> Result<(), SelectionError> {
        let (word, bit) = self.locate(x, y).ok_or(SelectionError::OutOfBounds {
            x,
            y,
            bounds: self.bounds,
        })?;
        let was_selected = self.words[word] & bit != 0;
        if was_selected == selected {
            return Ok(());
        }
        if selected {
            self.words[word] |= bit;
            self.selected += 1;
        } else {
            self.words[word] &= !bit;
            self.selected -= 1;
        }
        Ok(())
    }

    /// Shifts every selected bit and the mask bounds by `dx` and `dy`.
    pub fn translated(&self, dx: i64, dy: i64) -> Result<Self, SelectionError> {
        let left = self
            .bounds
            .left()
            .checked_add(dx)
            .ok_or(SelectionError::TranslateOverflow { dx, dy })?;
        let top = self
            .bounds
            .top()
            .checked_add(dy)
            .ok_or(SelectionError::TranslateOverflow { dx, dy })?;
        let right = self
            .bounds
            .right()
            .checked_add(dx)
            .ok_or(SelectionError::TranslateOverflow { dx, dy })?;
        let bottom = self
            .bounds
            .bottom()
            .checked_add(dy)
            .ok_or(SelectionError::TranslateOverflow { dx, dy })?;
        Ok(Self {
            bounds: Rect::new(left, top, right, bottom)?,
            row_words: self.row_words,
            words: self.words.clone(),
            selected: self.selected,
        })
    }

    /// Intersects the mask with the half-open canvas rectangle.
    ///
    /// `None` means the mask does not overlap the canvas or selects no pixel inside it.
    pub fn clipped_to(&self, size: Size) -> Result<Option<Self>, SelectionError> {
        let Some(bounds) = self.bounds.intersection(size.rect()) else {
            return Ok(None);
        };
        if bounds == self.bounds {
            return Ok(Some(self.clone()));
        }
        let mut clipped = Self::empty(bounds)?;
        for y in bounds.top()..bounds.bottom() {
            for x in bounds.left()..bounds.right() {
                if self.contains_xy(x, y) {
                    clipped.set_xy(x, y, true)?;
                }
            }
        }
        if clipped.is_empty() {
            Ok(None)
        } else {
            Ok(Some(clipped))
        }
    }

    fn pixel_count(&self) -> usize {
        usize::try_from(self.bounds.width())
            .ok()
            .and_then(|width| {
                usize::try_from(self.bounds.height())
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .unwrap_or(usize::MAX)
    }

    fn locate(&self, x: i64, y: i64) -> Option<(usize, u64)> {
        if x < self.bounds.left()
            || x >= self.bounds.right()
            || y < self.bounds.top()
            || y >= self.bounds.bottom()
        {
            return None;
        }
        let local_x = u64::try_from(x - self.bounds.left()).ok()?;
        let local_y = u64::try_from(y - self.bounds.top()).ok()?;
        let word = usize::try_from(local_y)
            .ok()?
            .checked_mul(self.row_words)?
            .checked_add(usize::try_from(local_x / 64).ok()?)?;
        Some((word, 1_u64 << (local_x % 64)))
    }
}

/// A rectangle selection with an optional pixel mask.
///
/// A rectangular selection stores only its bounds, matching the historical `Option<Rect>` model.
/// Shape tools install a mask and expose its tight bounds through [`Selection::bounds`] so status
/// text, zoom-to-selection, and the bounding-box marching-ants outline keep working.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    bounds: Rect,
    mask: Option<SelectionMask>,
}

impl Selection {
    /// Creates a rectangular selection that selects every pixel in `bounds`.
    pub const fn rectangle(bounds: Rect) -> Self {
        Self { bounds, mask: None }
    }

    /// Creates a selection backed by a real pixel mask; its bounds are the mask's tight bounds.
    pub fn from_mask(mask: SelectionMask) -> Self {
        Self {
            bounds: mask.bounds(),
            mask: Some(mask),
        }
    }

    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn mask(&self) -> Option<&SelectionMask> {
        self.mask.as_ref()
    }

    pub const fn is_rectangular(&self) -> bool {
        self.mask.is_none()
    }

    pub fn contains(&self, point: Point) -> bool {
        self.contains_xy(i64::from(point.x), i64::from(point.y))
    }

    pub fn contains_xy(&self, x: i64, y: i64) -> bool {
        match &self.mask {
            Some(mask) => mask.contains_xy(x, y),
            None => {
                self.bounds.left() <= x
                    && x < self.bounds.right()
                    && self.bounds.top() <= y
                    && y < self.bounds.bottom()
            }
        }
    }

    pub fn translated(&self, dx: i64, dy: i64) -> Result<Self, SelectionError> {
        match &self.mask {
            Some(mask) => Ok(Self::from_mask(mask.translated(dx, dy)?)),
            None => Ok(Self::rectangle(Rect::new(
                self.bounds
                    .left()
                    .checked_add(dx)
                    .ok_or(SelectionError::TranslateOverflow { dx, dy })?,
                self.bounds
                    .top()
                    .checked_add(dy)
                    .ok_or(SelectionError::TranslateOverflow { dx, dy })?,
                self.bounds
                    .right()
                    .checked_add(dx)
                    .ok_or(SelectionError::TranslateOverflow { dx, dy })?,
                self.bounds
                    .bottom()
                    .checked_add(dy)
                    .ok_or(SelectionError::TranslateOverflow { dx, dy })?,
            )?)),
        }
    }
}

fn layout(bounds: Rect) -> Result<(usize, usize), SelectionError> {
    let width = copy_to_usize(bounds.width(), bounds)?;
    let height = copy_to_usize(bounds.height(), bounds)?;
    let row_words = width.checked_add(63).ok_or(SelectionError::MaskTooLarge {
        width: bounds.width(),
        height: bounds.height(),
    })? / 64;
    let word_count = row_words
        .checked_mul(height)
        .ok_or(SelectionError::MaskTooLarge {
            width: bounds.width(),
            height: bounds.height(),
        })?;
    Ok((row_words, word_count))
}

fn copy_to_usize(value: u32, bounds: Rect) -> Result<usize, SelectionError> {
    usize::try_from(value).map_err(|_| SelectionError::MaskTooLarge {
        width: bounds.width(),
        height: bounds.height(),
    })
}
