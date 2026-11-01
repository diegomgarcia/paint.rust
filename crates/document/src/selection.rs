use paint_core::{GeometryError, Point, Rect, Size};
use thiserror::Error;

/// Errors produced while building, indexing, clipping, or translating a selection mask.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SelectionError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("selection mask flag length mismatch: expected {expected}, got {actual}")]
    FlagLength { expected: usize, actual: usize },
    #[error("selection mask packed-word length mismatch: expected {expected}, got {actual}")]
    PackedLength { expected: usize, actual: usize },
    #[error("selection mask packed bits outside the mask width at row {row}, word {word}")]
    PackedPadding { row: usize, word: usize },
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

    /// Returns the canonical row-major packed bits backing this mask.
    ///
    /// Each row occupies [`Self::words_per_row`] 64-bit words. Bits at or beyond the row width
    /// in the final word are always zero.
    pub fn packed_words(&self) -> &[u64] {
        &self.words
    }

    /// Number of 64-bit words stored for each mask row.
    pub const fn words_per_row(&self) -> usize {
        self.row_words
    }

    /// Rebuilds a mask from packed row-major bits, validating length and padding.
    ///
    /// The selected-pixel count is derived from the bits rather than trusted from the caller.
    pub fn from_packed_words(bounds: Rect, words: Vec<u64>) -> Result<Self, SelectionError> {
        let (row_words, word_count) = layout(bounds)?;
        if words.len() != word_count {
            return Err(SelectionError::PackedLength {
                expected: word_count,
                actual: words.len(),
            });
        }
        let width = bounds.width();
        let height = copy_to_usize(bounds.height(), bounds)?;
        let mut selected = 0usize;
        for row in 0..height {
            for word in 0..row_words {
                let bits = words[row * row_words + word];
                if bits != trim_row_word(bits, word, row_words, width) {
                    return Err(SelectionError::PackedPadding { row, word });
                }
                selected += bits.count_ones() as usize;
            }
        }
        Ok(Self {
            bounds,
            row_words,
            words,
            selected,
        })
    }

    /// Returns the union of both masks, or `None` when neither selects a pixel.
    pub fn union(&self, other: &Self) -> Result<Option<Self>, SelectionError> {
        self.combine(other, MaskOperation::Union)
    }

    /// Returns the pixels selected by `self` but not by `other`.
    pub fn subtract(&self, other: &Self) -> Result<Option<Self>, SelectionError> {
        self.combine(other, MaskOperation::Subtract)
    }

    /// Returns the pixels selected by both masks.
    pub fn intersect(&self, other: &Self) -> Result<Option<Self>, SelectionError> {
        self.combine(other, MaskOperation::Intersect)
    }

    /// Returns the pixels selected by exactly one mask.
    pub fn xor(&self, other: &Self) -> Result<Option<Self>, SelectionError> {
        self.combine(other, MaskOperation::Xor)
    }

    fn combine(
        &self,
        other: &Self,
        operation: MaskOperation,
    ) -> Result<Option<Self>, SelectionError> {
        match operation {
            MaskOperation::Union => {
                if self.is_empty() {
                    return Ok((!other.is_empty()).then(|| other.clone()));
                }
                if other.is_empty() {
                    return Ok(Some(self.clone()));
                }
            }
            MaskOperation::Subtract => {
                if self.is_empty() {
                    return Ok(None);
                }
                if other.is_empty() {
                    return Ok(Some(self.clone()));
                }
            }
            MaskOperation::Intersect => {
                if self.is_empty() || other.is_empty() {
                    return Ok(None);
                }
            }
            MaskOperation::Xor => {
                if self.is_empty() {
                    return Ok((!other.is_empty()).then(|| other.clone()));
                }
                if other.is_empty() {
                    return Ok(Some(self.clone()));
                }
            }
        }

        let bounds = match operation {
            MaskOperation::Union | MaskOperation::Xor => Rect::new(
                self.bounds.left().min(other.bounds.left()),
                self.bounds.top().min(other.bounds.top()),
                self.bounds.right().max(other.bounds.right()),
                self.bounds.bottom().max(other.bounds.bottom()),
            )?,
            MaskOperation::Intersect => {
                let Some(intersection) = self.bounds.intersection(other.bounds) else {
                    return Ok(None);
                };
                intersection
            }
            MaskOperation::Subtract => self.bounds,
        };
        let mut combined = Self::empty(bounds)?;
        let width = bounds.width();
        let mut selected = 0usize;
        for row in 0..bounds.height() {
            let y = bounds.top() + i64::from(row);
            let row_start = row as usize * combined.row_words;
            for word in 0..combined.row_words {
                let x_start = bounds.left() + (word as i64) * 64;
                let left = self.row_word(y, x_start);
                let right = other.row_word(y, x_start);
                let mut bits = match operation {
                    MaskOperation::Union => left | right,
                    MaskOperation::Subtract => left & !right,
                    MaskOperation::Intersect => left & right,
                    MaskOperation::Xor => left ^ right,
                };
                bits = trim_row_word(bits, word, combined.row_words, width);
                combined.words[row_start + word] = bits;
                selected += bits.count_ones() as usize;
            }
        }
        if selected == 0 {
            return Ok(None);
        }
        combined.selected = selected;
        combined.tightened()
    }

    /// Extracts the 64 bits starting at `(x_start, y)` in canvas coordinates.
    ///
    /// Bits outside this mask are zero, including bits before the left edge.
    fn row_word(&self, y: i64, x_start: i64) -> u64 {
        if y < self.bounds.top() || y >= self.bounds.bottom() {
            return 0;
        }
        let row = (y - self.bounds.top()) as usize;
        let base = row * self.row_words;
        let delta = i128::from(x_start) - i128::from(self.bounds.left());
        if delta >= 0 {
            let delta = delta as u128;
            let index = delta / 64;
            if index >= self.row_words as u128 {
                return 0;
            }
            let index = index as usize;
            let shift = (delta % 64) as u32;
            let low = self.words[base + index];
            if shift == 0 {
                low
            } else {
                let high = if index + 1 < self.row_words {
                    self.words[base + index + 1]
                } else {
                    0
                };
                (low >> shift) | (high << (64 - shift))
            }
        } else {
            let lead = (-delta) as u128;
            if lead >= 64 {
                return 0;
            }
            self.words[base] << (lead as u32)
        }
    }

    /// Shrinks the bounds to the selected pixels so combined masks stay tight.
    fn tightened(self) -> Result<Option<Self>, SelectionError> {
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for row in 0..self.bounds.height() {
            let row_start = row as usize * self.row_words;
            let y = self.bounds.top() + i64::from(row);
            for word in 0..self.row_words {
                let bits = self.words[row_start + word];
                if bits == 0 {
                    continue;
                }
                let word_start = self.bounds.left() + (word as i64) * 64;
                let start = word_start + i64::from(bits.trailing_zeros());
                let end = word_start + i64::from(63 - bits.leading_zeros());
                min_x = min_x.min(start);
                max_x = max_x.max(end);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        if min_x == i64::MAX {
            return Ok(None);
        }
        let bounds = Rect::new(min_x, min_y, max_x + 1, max_y + 1)?;
        if bounds == self.bounds {
            return Ok(Some(self));
        }
        let mut tight = Self::empty(bounds)?;
        for row in 0..bounds.height() {
            let y = bounds.top() + i64::from(row);
            let row_start = row as usize * tight.row_words;
            for word in 0..tight.row_words {
                let x_start = bounds.left() + (word as i64) * 64;
                let bits = trim_row_word(
                    self.row_word(y, x_start),
                    word,
                    tight.row_words,
                    bounds.width(),
                );
                tight.words[row_start + word] = bits;
            }
        }
        tight.selected = self.selected;
        Ok(Some(tight))
    }

    pub fn contains(&self, point: Point) -> bool {
        self.contains_xy(i64::from(point.x), i64::from(point.y))
    }

    pub fn contains_xy(&self, x: i64, y: i64) -> bool {
        self.locate(x, y)
            .is_some_and(|(word, bit)| self.words[word] & bit != 0)
    }

    /// Returns the contiguous selected x-ranges in one row, in ascending order.
    ///
    /// This scans the packed words instead of probing each pixel, which keeps UI boundary
    /// tracing and selection tinting fast for large masks.
    pub fn row_runs(&self, y: i64) -> Vec<(i64, i64)> {
        if y < self.bounds.top() || y >= self.bounds.bottom() {
            return Vec::new();
        }
        let Ok(row) = usize::try_from(y - self.bounds.top()) else {
            return Vec::new();
        };
        let Ok(width) = usize::try_from(self.bounds.width()) else {
            return Vec::new();
        };
        let base = row * self.row_words;
        let left = self.bounds.left();
        let mut runs = Vec::new();
        let mut x = 0usize;
        while x < width {
            let word_index = x / 64;
            let bit = x % 64;
            let word = self.words[base + word_index] & (u64::MAX << bit);
            if word == 0 {
                x = (word_index + 1) * 64;
                continue;
            }
            let start = word_index * 64 + word.trailing_zeros() as usize;
            let mut end = start;
            loop {
                let index = end / 64;
                if index >= self.row_words {
                    end = width;
                    break;
                }
                let offset = end % 64;
                let word = self.words[base + index] >> offset;
                if word == 0 {
                    break;
                }
                let ones = word.trailing_ones() as usize;
                end += ones;
                if ones < 64 - offset || end >= width {
                    break;
                }
            }
            end = end.min(width);
            runs.push((left + start as i64, left + end as i64));
            x = end;
        }
        runs
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

/// How a newly drawn selection combines with the existing selection.
///
/// This mirrors Paint.NET's selection combine modes. [`Self::Subtract`] is Paint.NET's
/// "Exclude" mode: it removes the new shape from the existing selection.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum SelectionCombineMode {
    /// Discard the existing selection and keep only the new shape.
    #[default]
    Replace,
    /// Add the new shape to the existing selection.
    Union,
    /// Remove the new shape from the existing selection.
    Subtract,
    /// Keep only pixels selected by both the existing selection and the new shape.
    Intersect,
    /// Keep pixels selected by exactly one of the existing selection and the new shape.
    Xor,
}

impl SelectionCombineMode {
    /// Every mode in the Paint.NET tool-config-strip order.
    pub const ALL: [Self; 5] = [
        Self::Replace,
        Self::Union,
        Self::Subtract,
        Self::Intersect,
        Self::Xor,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Replace => "Replace",
            Self::Union => "Union",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
            Self::Xor => "Xor",
        }
    }
}

/// A rectangle selection with an optional pixel mask.
///
/// A rectangular selection stores only its bounds, matching the historical `Option<Rect>` model.
/// Shape tools install a mask and expose its tight bounds through [`Selection::bounds`] so status
/// text and zoom-to-selection keep working. The UI traces the mask boundary for marching ants and
/// tints the selected runs, not their bounding box.
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

    /// Combines a newly drawn `addition` with the existing `base` selection.
    ///
    /// The result is `None` when the combination selects no pixels. Two rectangular selections
    /// stay on the cheap rectangle path whenever set algebra can express the result as one
    /// rectangle; shapes that would become L-shaped or split fall back to packed mask algebra.
    pub fn combine(
        base: Option<&Self>,
        addition: Option<&Self>,
        mode: SelectionCombineMode,
        canvas: Size,
    ) -> Result<Option<Self>, SelectionError> {
        let base = base.filter(|selection| !selection.is_empty());
        let addition = addition.filter(|selection| !selection.is_empty());
        if mode == SelectionCombineMode::Replace {
            return Ok(addition.cloned());
        }
        let Some(base) = base else {
            return Ok(match mode {
                SelectionCombineMode::Union | SelectionCombineMode::Xor => addition.cloned(),
                SelectionCombineMode::Replace
                | SelectionCombineMode::Subtract
                | SelectionCombineMode::Intersect => None,
            });
        };
        let Some(addition) = addition else {
            return Ok(match mode {
                SelectionCombineMode::Union
                | SelectionCombineMode::Subtract
                | SelectionCombineMode::Xor => Some(base.clone()),
                SelectionCombineMode::Replace | SelectionCombineMode::Intersect => None,
            });
        };

        if base.mask().is_none() && addition.mask().is_none() {
            return match combine_rectangles(base.bounds(), addition.bounds(), mode) {
                RectCombination::Rect(rect) => Ok(Some(Self::rectangle(rect))),
                RectCombination::Empty => Ok(None),
                RectCombination::Mask => combine_as_masks(Some(base), Some(addition), mode, canvas),
            };
        }

        match mode {
            SelectionCombineMode::Union => {
                if base.mask().is_none() && rect_contains(base.bounds(), addition.bounds()) {
                    return Ok(Some(base.clone()));
                }
                if addition.mask().is_none() && rect_contains(addition.bounds(), base.bounds()) {
                    return Ok(Some(addition.clone()));
                }
            }
            SelectionCombineMode::Intersect => {
                if base.mask().is_none() && rect_contains(base.bounds(), addition.bounds()) {
                    return Ok(Some(addition.clone()));
                }
                if addition.mask().is_none() && rect_contains(addition.bounds(), base.bounds()) {
                    return Ok(Some(base.clone()));
                }
            }
            SelectionCombineMode::Replace
            | SelectionCombineMode::Subtract
            | SelectionCombineMode::Xor => {}
        }
        combine_as_masks(Some(base), Some(addition), mode, canvas)
    }

    /// Whether this selection selects nothing. Rectangle selections are always non-empty.
    pub fn is_empty(&self) -> bool {
        self.mask.as_ref().is_some_and(SelectionMask::is_empty)
    }

    pub(crate) fn as_canvas_mask(
        &self,
        canvas: Size,
    ) -> Result<Option<SelectionMask>, SelectionError> {
        match &self.mask {
            Some(mask) => mask.clipped_to(canvas),
            None => {
                let Some(bounds) = self.bounds.intersection(canvas.rect()) else {
                    return Ok(None);
                };
                Ok(Some(SelectionMask::full(bounds)?))
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

#[derive(Clone, Copy)]
enum MaskOperation {
    Union,
    Subtract,
    Intersect,
    Xor,
}

fn trim_row_word(bits: u64, word: usize, row_words: usize, width: u32) -> u64 {
    if word + 1 == row_words {
        let remainder = width % 64;
        if remainder == 0 {
            bits
        } else {
            bits & ((1_u64 << remainder) - 1)
        }
    } else {
        bits
    }
}

fn rect_contains(outer: Rect, inner: Rect) -> bool {
    outer.left() <= inner.left()
        && inner.right() <= outer.right()
        && outer.top() <= inner.top()
        && inner.bottom() <= outer.bottom()
}

#[derive(Clone, Copy)]
enum RectCombination {
    Rect(Rect),
    Empty,
    Mask,
}

fn combine_rectangles(base: Rect, addition: Rect, mode: SelectionCombineMode) -> RectCombination {
    match mode {
        SelectionCombineMode::Replace => RectCombination::Rect(addition),
        SelectionCombineMode::Union => union_rectangles(base, addition),
        SelectionCombineMode::Subtract => subtract_rectangles(base, addition),
        SelectionCombineMode::Intersect => match base.intersection(addition) {
            Some(intersection) => RectCombination::Rect(intersection),
            None => RectCombination::Empty,
        },
        SelectionCombineMode::Xor => {
            if base == addition {
                RectCombination::Empty
            } else if base.intersection(addition).is_none() {
                union_rectangles(base, addition)
            } else {
                RectCombination::Mask
            }
        }
    }
}

fn union_rectangles(base: Rect, addition: Rect) -> RectCombination {
    if rect_contains(base, addition) {
        return RectCombination::Rect(base);
    }
    if rect_contains(addition, base) {
        return RectCombination::Rect(addition);
    }
    let shares_columns = base.left() == addition.left()
        && base.right() == addition.right()
        && base.bottom() >= addition.top()
        && addition.bottom() >= base.top();
    let shares_rows = base.top() == addition.top()
        && base.bottom() == addition.bottom()
        && base.right() >= addition.left()
        && addition.right() >= base.left();
    if !shares_columns && !shares_rows {
        return RectCombination::Mask;
    }
    match Rect::new(
        base.left().min(addition.left()),
        base.top().min(addition.top()),
        base.right().max(addition.right()),
        base.bottom().max(addition.bottom()),
    ) {
        Ok(rect) => RectCombination::Rect(rect),
        Err(_) => RectCombination::Mask,
    }
}

fn subtract_rectangles(base: Rect, addition: Rect) -> RectCombination {
    if base.intersection(addition).is_none() {
        return RectCombination::Rect(base);
    }
    if rect_contains(addition, base) {
        return RectCombination::Empty;
    }
    if addition.left() <= base.left() && addition.right() >= base.right() {
        let above = (base.top() < addition.top())
            .then(|| {
                Rect::new(
                    base.left(),
                    base.top(),
                    base.right(),
                    addition.top().min(base.bottom()),
                )
            })
            .transpose();
        let below = (addition.bottom() < base.bottom())
            .then(|| {
                Rect::new(
                    base.left(),
                    addition.bottom().max(base.top()),
                    base.right(),
                    base.bottom(),
                )
            })
            .transpose();
        return merge_rect_pieces(above, below);
    }
    if addition.top() <= base.top() && addition.bottom() >= base.bottom() {
        let left = (base.left() < addition.left())
            .then(|| {
                Rect::new(
                    base.left(),
                    base.top(),
                    addition.left().min(base.right()),
                    base.bottom(),
                )
            })
            .transpose();
        let right = (addition.right() < base.right())
            .then(|| {
                Rect::new(
                    addition.right().max(base.left()),
                    base.top(),
                    base.right(),
                    base.bottom(),
                )
            })
            .transpose();
        return merge_rect_pieces(left, right);
    }
    RectCombination::Mask
}

fn merge_rect_pieces(
    first: Result<Option<Rect>, GeometryError>,
    second: Result<Option<Rect>, GeometryError>,
) -> RectCombination {
    match (first, second) {
        (Ok(None), Ok(None)) => RectCombination::Empty,
        (Ok(Some(rect)), Ok(None)) | (Ok(None), Ok(Some(rect))) => RectCombination::Rect(rect),
        (Ok(Some(_)), Ok(Some(_))) => RectCombination::Mask,
        (Err(_), _) | (_, Err(_)) => RectCombination::Mask,
    }
}

fn combine_as_masks(
    base: Option<&Selection>,
    addition: Option<&Selection>,
    mode: SelectionCombineMode,
    canvas: Size,
) -> Result<Option<Selection>, SelectionError> {
    let base_mask = base
        .map(|selection| selection.as_canvas_mask(canvas))
        .transpose()?
        .flatten();
    let addition_mask = addition
        .map(|selection| selection.as_canvas_mask(canvas))
        .transpose()?
        .flatten();
    let combined = match mode {
        SelectionCombineMode::Replace => addition_mask,
        SelectionCombineMode::Union => match (base_mask, addition_mask) {
            (None, other) | (other, None) => other,
            (Some(base), Some(addition)) => base.union(&addition)?,
        },
        SelectionCombineMode::Subtract => match (base_mask, addition_mask) {
            (None, _) => None,
            (Some(base), None) => Some(base),
            (Some(base), Some(addition)) => base.subtract(&addition)?,
        },
        SelectionCombineMode::Intersect => match (base_mask, addition_mask) {
            (Some(base), Some(addition)) => base.intersect(&addition)?,
            _ => None,
        },
        SelectionCombineMode::Xor => match (base_mask, addition_mask) {
            (None, other) | (other, None) => other,
            (Some(base), Some(addition)) => base.xor(&addition)?,
        },
    };
    Ok(combined.map(Selection::from_mask))
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
