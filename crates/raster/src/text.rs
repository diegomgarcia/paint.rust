//! Deterministic CPU text rasterization with embedded permissively licensed fonts.

use ab_glyph::{Font, FontVec, GlyphId, PxScale, VariableFont, point};
use paint_core::{Bgra, Point};
use paint_document::{Document, LayerId, PixelPatch};

use crate::RasterError;
use crate::compose::normal_over;
use crate::operations::{clip_rect, copy_pixels, layer, operation_rect};

/// Largest accepted font size in pixels.
pub const MAX_TEXT_SIZE: u32 = 512;
/// Largest accepted UTF-8 text payload for a single placement.
pub const MAX_TEXT_BYTES: usize = 8192;

/// Embedded font families available to the text tool.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextFontFamily {
    /// The proportional Roboto family.
    Sans,
    /// The monospaced Roboto Mono family.
    Mono,
}

impl TextFontFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sans => "Sans",
            Self::Mono => "Mono",
        }
    }

    const fn font_data(self, italic: bool) -> &'static [u8] {
        match (self, italic) {
            (Self::Sans, false) => include_bytes!("../assets/fonts/Roboto-Variable.ttf"),
            (Self::Sans, true) => include_bytes!("../assets/fonts/Roboto-Italic-Variable.ttf"),
            (Self::Mono, false) => include_bytes!("../assets/fonts/RobotoMono-Variable.ttf"),
            (Self::Mono, true) => include_bytes!("../assets/fonts/RobotoMono-Italic-Variable.ttf"),
        }
    }
}

/// Paint.NET-like text placement settings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextStyle {
    pub family: TextFontFamily,
    /// Font size in canvas pixels.
    pub size: u32,
    pub bold: bool,
    pub italic: bool,
    pub antialias: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: TextFontFamily::Sans,
            size: 48,
            bold: false,
            italic: false,
            antialias: true,
        }
    }
}

/// Rasterizes `content` at `origin` into one undoable patch on the active selection.
///
/// `origin` is the top-left of the first line box. Newlines start new lines, tabs advance
/// four spaces, and bold uses the variable font's 700 weight. Glyph ink outside the
/// canvas or the active selection is clipped away. Blank content and fully clipped text
/// return `Ok(None)` without touching the document.
pub fn text(
    document: &Document,
    layer_id: LayerId,
    origin: Point,
    content: &str,
    style: TextStyle,
    color: Bgra,
) -> Result<Option<PixelPatch>, RasterError> {
    if content.len() > MAX_TEXT_BYTES {
        return Err(RasterError::TextTooLong {
            actual: content.len(),
            max: MAX_TEXT_BYTES,
        });
    }
    if style.size == 0 || style.size > MAX_TEXT_SIZE {
        return Err(RasterError::TextSizeOutOfRange {
            size: style.size,
            max: MAX_TEXT_SIZE,
        });
    }
    let layer = layer(document, layer_id)?;
    let Some(operation) = operation_rect(document) else {
        return Ok(None);
    };

    let mut font =
        FontVec::try_from_vec(style.family.font_data(style.italic).to_vec()).map_err(|_| {
            RasterError::FontUnavailable {
                name: style.family.label(),
            }
        })?;
    if style.bold {
        // The embedded files are variable fonts, so the bold style stays a real weight
        // instead of a synthetic smear. If an axis is unavailable, regular is still usable.
        let _ = font.set_variation(b"wght", 700.0);
    }

    let units_per_em = font.units_per_em().unwrap_or(1000.0);
    let scale = style.size as f32;
    let factor = scale / units_per_em;
    let ascent = font.ascent_unscaled() * factor;
    let descent = font.descent_unscaled() * factor;
    let line_gap = font.line_gap_unscaled() * factor;
    let line_height = (ascent - descent + line_gap).max(scale);
    let px_scale = PxScale::from(scale);
    let tab_advance = font.h_advance_unscaled(font.glyph_id(' ')) * factor * 4.0;

    let mut glyphs = Vec::new();
    let mut pen_y = origin.y as f32 + ascent;
    for (line_index, line) in content.split('\n').enumerate() {
        if line_index > 0 {
            pen_y += line_height;
        }
        let mut pen_x = origin.x as f32;
        let mut previous: Option<GlyphId> = None;
        for character in line.chars() {
            if character == '\r' {
                continue;
            }
            if character == '\t' {
                pen_x += tab_advance;
                previous = None;
                continue;
            }
            let id = font.glyph_id(character);
            if let Some(previous) = previous {
                pen_x += font.kern_unscaled(previous, id) * factor;
            }
            let glyph = id.with_scale_and_position(px_scale, point(pen_x, pen_y));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                if bounds.width() > 0.0 && bounds.height() > 0.0 {
                    glyphs.push(outlined);
                }
            }
            pen_x += font.h_advance_unscaled(id) * factor;
            previous = Some(id);
        }
    }

    let Some(bounds) = glyph_bounds(&glyphs) else {
        return Ok(None);
    };
    let Some(rect) = clip_rect(
        operation,
        bounds.0 as i64,
        bounds.1 as i64,
        bounds.2 as i64,
        bounds.3 as i64,
    )?
    else {
        return Ok(None);
    };
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut coverage = Vec::new();
    coverage
        .try_reserve_exact(requested_bytes)
        .map_err(|_| RasterError::AllocationFailed { requested_bytes })?;
    coverage.resize(requested_bytes, 0u8);
    let left = rect.left();
    let top = rect.top();
    let width = i64::from(rect.width());
    let height = i64::from(rect.height());
    for outlined in &glyphs {
        let bounds = outlined.px_bounds();
        let x_offset = bounds.min.x as i64;
        let y_offset = bounds.min.y as i64;
        outlined.draw(|x, y, coverage_value| {
            let canvas_x = x_offset + i64::from(x);
            let canvas_y = y_offset + i64::from(y);
            if canvas_x < left
                || canvas_x >= left + width
                || canvas_y < top
                || canvas_y >= top + height
            {
                return;
            }
            let value = if style.antialias {
                (coverage_value.clamp(0.0, 1.0) * 255.0).round() as u8
            } else if coverage_value >= 0.5 {
                u8::MAX
            } else {
                0
            };
            if value == 0 {
                return;
            }
            let index = ((canvas_y - top) * width + (canvas_x - left)) as usize;
            coverage[index] = coverage[index].max(value);
        });
    }
    if coverage.iter().all(|value| *value == 0) {
        return Ok(None);
    }

    let before = layer.surface().read_region(rect)?;
    let requested_bytes = rect.size().checked_buffer_len()?;
    let mut after = copy_pixels(&before, requested_bytes)?;
    for (pixel, coverage_value) in after.iter_mut().zip(&coverage) {
        if *coverage_value == 0 {
            continue;
        }
        let alpha = ((u16::from(color.a) * u16::from(*coverage_value) + 127) / 255) as u8;
        if alpha == 0 {
            continue;
        }
        *pixel = normal_over(*pixel, Bgra::new(color.b, color.g, color.r, alpha), u8::MAX);
    }
    if before == after {
        return Ok(None);
    }
    Ok(Some(PixelPatch::new(
        layer_id,
        layer.revision(),
        rect,
        before,
        after,
    )?))
}

/// Returns the floored/ceiled canvas bounds of all outlined glyphs.
fn glyph_bounds(glyphs: &[ab_glyph::OutlinedGlyph]) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for glyph in glyphs {
        let bounds = glyph.px_bounds();
        min_x = min_x.min(bounds.min.x);
        min_y = min_y.min(bounds.min.y);
        max_x = max_x.max(bounds.max.x);
        max_y = max_y.max(bounds.max.y);
    }
    (min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()).then_some((
        min_x.floor(),
        min_y.floor(),
        max_x.ceil(),
        max_y.ceil(),
    ))
}
