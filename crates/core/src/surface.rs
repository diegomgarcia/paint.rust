use std::ops::Range;

use thiserror::Error;

use crate::{Bgra, GeometryError, Point, Rect, Size};

/// Errors produced by checked surface construction, indexing, and region access.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SurfaceError {
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error("pixel buffer length mismatch: expected {expected}, got {actual}")]
    PixelLength { expected: usize, actual: usize },
    #[error("byte buffer length mismatch: expected {expected}, got {actual}")]
    BufferLength { expected: usize, actual: usize },
    #[error("failed to allocate {requested_bytes} bytes for a surface buffer")]
    AllocationFailed { requested_bytes: usize },
    #[error("pixel coordinate ({x}, {y}) is outside surface size {size:?}")]
    OutOfBounds { x: u32, y: u32, size: Size },
    #[error("rectangle {rect:?} is outside surface size {size:?}")]
    RectangleOutOfBounds { rect: Rect, size: Size },
    #[error("surface index arithmetic overflow for size {size:?}")]
    IndexOverflow { size: Size },
}

/// An owned, contiguous, row-major BGRA surface with four bytes per pixel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Surface {
    size: Size,
    stride: usize,
    pixels: Vec<Bgra>,
}

impl Surface {
    /// Allocates a transparent-black surface.
    pub fn new(size: Size) -> Result<Self, SurfaceError> {
        Self::filled(size, Bgra::TRANSPARENT)
    }

    pub fn filled(size: Size, pixel: Bgra) -> Result<Self, SurfaceError> {
        let pixel_count = size.checked_pixel_count()?;
        let requested_bytes = size.checked_buffer_len()?;
        let stride = size.checked_stride()?;
        let mut pixels = pixel_buffer(pixel_count, requested_bytes)?;
        pixels.resize(pixel_count, pixel);
        Ok(Self {
            size,
            stride,
            pixels,
        })
    }

    /// Takes ownership of a pixel buffer after checking its exact dimensions.
    pub fn from_pixels(size: Size, pixels: Vec<Bgra>) -> Result<Self, SurfaceError> {
        size.checked_buffer_len()?;
        let expected = size.checked_pixel_count()?;
        if pixels.len() != expected {
            return Err(SurfaceError::PixelLength {
                expected,
                actual: pixels.len(),
            });
        }
        let stride = size.checked_stride()?;
        Ok(Self {
            size,
            stride,
            pixels,
        })
    }

    pub fn from_pixel_slice(size: Size, pixels: &[Bgra]) -> Result<Self, SurfaceError> {
        let expected = size.checked_pixel_count()?;
        let requested_bytes = size.checked_buffer_len()?;
        if pixels.len() != expected {
            return Err(SurfaceError::PixelLength {
                expected,
                actual: pixels.len(),
            });
        }
        let mut owned = pixel_buffer(expected, requested_bytes)?;
        owned.extend_from_slice(pixels);
        Self::from_pixels(size, owned)
    }

    /// Constructs a surface from explicit blue, green, red, alpha bytes.
    pub fn from_bgra_bytes(size: Size, bytes: &[u8]) -> Result<Self, SurfaceError> {
        let expected = size.checked_buffer_len()?;
        if bytes.len() != expected {
            return Err(SurfaceError::BufferLength {
                expected,
                actual: bytes.len(),
            });
        }
        let pixel_count = size.checked_pixel_count()?;
        let mut pixels = pixel_buffer(pixel_count, expected)?;
        for chunk in bytes.chunks_exact(4) {
            pixels.push(Bgra::new(chunk[0], chunk[1], chunk[2], chunk[3]));
        }
        Self::from_pixels(size, pixels)
    }

    /// Constructs a surface from explicit red, green, blue, alpha bytes.
    pub fn from_rgba_bytes(size: Size, bytes: &[u8]) -> Result<Self, SurfaceError> {
        let expected = size.checked_buffer_len()?;
        if bytes.len() != expected {
            return Err(SurfaceError::BufferLength {
                expected,
                actual: bytes.len(),
            });
        }
        let pixel_count = size.checked_pixel_count()?;
        let mut pixels = pixel_buffer(pixel_count, expected)?;
        for chunk in bytes.chunks_exact(4) {
            pixels.push(Bgra::new(chunk[2], chunk[1], chunk[0], chunk[3]));
        }
        Self::from_pixels(size, pixels)
    }

    pub fn to_bgra_bytes(&self) -> Result<Vec<u8>, SurfaceError> {
        let requested_bytes = self.size.checked_buffer_len()?;
        let mut bytes = byte_buffer(requested_bytes)?;
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_bgra_bytes());
        }
        Ok(bytes)
    }

    pub fn to_rgba_bytes(&self) -> Result<Vec<u8>, SurfaceError> {
        let requested_bytes = self.size.checked_buffer_len()?;
        let mut bytes = byte_buffer(requested_bytes)?;
        for pixel in &self.pixels {
            bytes.extend_from_slice(&pixel.to_rgba_bytes());
        }
        Ok(bytes)
    }

    pub const fn size(&self) -> Size {
        self.size
    }

    pub const fn width(&self) -> u32 {
        self.size.width()
    }

    pub const fn height(&self) -> u32 {
        self.size.height()
    }

    /// Canonical row stride in bytes.
    pub const fn stride(&self) -> usize {
        self.stride
    }

    pub fn len(&self) -> usize {
        self.pixels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    pub fn pixels(&self) -> &[Bgra] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [Bgra] {
        &mut self.pixels
    }

    pub fn index(&self, point: Point) -> Result<usize, SurfaceError> {
        if point.x >= self.width() || point.y >= self.height() {
            return Err(SurfaceError::OutOfBounds {
                x: point.x,
                y: point.y,
                size: self.size,
            });
        }
        let x = usize::try_from(point.x)
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let y = usize::try_from(point.y)
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let width = usize::try_from(self.width())
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        y.checked_mul(width)
            .and_then(|row| row.checked_add(x))
            .ok_or(SurfaceError::IndexOverflow { size: self.size })
    }

    pub fn get(&self, point: Point) -> Option<&Bgra> {
        self.index(point)
            .ok()
            .and_then(|index| self.pixels.get(index))
    }

    pub fn pixel(&self, point: Point) -> Result<&Bgra, SurfaceError> {
        let index = self.index(point)?;
        self.pixels
            .get(index)
            .ok_or(SurfaceError::IndexOverflow { size: self.size })
    }

    pub fn set_pixel(&mut self, point: Point, pixel: Bgra) -> Result<(), SurfaceError> {
        let index = self.index(point)?;
        let size = self.size;
        let destination = self
            .pixels
            .get_mut(index)
            .ok_or(SurfaceError::IndexOverflow { size })?;
        *destination = pixel;
        Ok(())
    }

    pub fn row(&self, y: u32) -> Result<&[Bgra], SurfaceError> {
        let range = self.row_range(y)?;
        self.pixels
            .get(range)
            .ok_or(SurfaceError::IndexOverflow { size: self.size })
    }

    pub fn row_mut(&mut self, y: u32) -> Result<&mut [Bgra], SurfaceError> {
        let range = self.row_range(y)?;
        let size = self.size;
        self.pixels
            .get_mut(range)
            .ok_or(SurfaceError::IndexOverflow { size })
    }

    /// Copies a fully in-bounds half-open region into packed row-major order.
    pub fn read_region(&self, rect: Rect) -> Result<Vec<Bgra>, SurfaceError> {
        let plan = self.plan(rect)?;
        let requested_bytes = rect.size().checked_buffer_len()?;
        let mut output = pixel_buffer(plan.pixel_count, requested_bytes)?;
        for row in 0..plan.height {
            let range = packed_row_range(&plan, row);
            output.extend_from_slice(&self.pixels[range]);
        }
        Ok(output)
    }

    /// Compares a fully in-bounds region against packed row-major pixels.
    pub fn region_matches(&self, rect: Rect, expected: &[Bgra]) -> Result<bool, SurfaceError> {
        let plan = self.plan(rect)?;
        if expected.len() != plan.pixel_count {
            return Err(SurfaceError::PixelLength {
                expected: plan.pixel_count,
                actual: expected.len(),
            });
        }
        for row in 0..plan.height {
            let range = packed_row_range(&plan, row);
            let source_start = row * plan.width;
            if self.pixels[range] != expected[source_start..source_start + plan.width] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Replaces a region after validating every dimension and length, before any mutation.
    pub fn replace_region(&mut self, rect: Rect, pixels: &[Bgra]) -> Result<(), SurfaceError> {
        let plan = self.plan(rect)?;
        if pixels.len() != plan.pixel_count {
            return Err(SurfaceError::PixelLength {
                expected: plan.pixel_count,
                actual: pixels.len(),
            });
        }

        // The plan proves all row ranges and packed offsets fit the existing buffers.
        // Consequently this loop has no failing operations and cannot partially apply an error.
        for row in 0..plan.height {
            let range = packed_row_range(&plan, row);
            let source_start = row * plan.width;
            self.pixels[range].copy_from_slice(&pixels[source_start..source_start + plan.width]);
        }
        Ok(())
    }

    fn row_range(&self, y: u32) -> Result<Range<usize>, SurfaceError> {
        if y >= self.height() {
            return Err(SurfaceError::OutOfBounds {
                x: 0,
                y,
                size: self.size,
            });
        }
        let y = usize::try_from(y).map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let width = usize::try_from(self.width())
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let start = y
            .checked_mul(width)
            .ok_or(SurfaceError::IndexOverflow { size: self.size })?;
        let end = start
            .checked_add(width)
            .ok_or(SurfaceError::IndexOverflow { size: self.size })?;
        Ok(start..end)
    }

    fn plan(&self, rect: Rect) -> Result<RegionPlan, SurfaceError> {
        if !rect.is_within(self.size.rect()) {
            return Err(SurfaceError::RectangleOutOfBounds {
                rect,
                size: self.size,
            });
        }
        let convert = |value: i64| {
            usize::try_from(value).map_err(|_| SurfaceError::IndexOverflow { size: self.size })
        };
        let left = convert(rect.left())?;
        let top = convert(rect.top())?;
        let width = usize::try_from(rect.width())
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let height = usize::try_from(rect.height())
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let surface_width = usize::try_from(self.width())
            .map_err(|_| SurfaceError::IndexOverflow { size: self.size })?;
        let pixel_count = rect.size().checked_pixel_count()?;
        Ok(RegionPlan {
            left,
            top,
            width,
            height,
            surface_width,
            pixel_count,
            size: self.size,
        })
    }
}

#[derive(Clone, Copy)]
struct RegionPlan {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    surface_width: usize,
    pixel_count: usize,
    size: Size,
}

fn packed_row_range(plan: &RegionPlan, row: usize) -> Range<usize> {
    debug_assert!(row < plan.height);
    let start = (plan.top + row) * plan.surface_width + plan.left;
    let end = start + plan.width;
    debug_assert!(end <= plan.size.checked_pixel_count().unwrap_or(usize::MAX));
    start..end
}

fn pixel_buffer(pixel_count: usize, requested_bytes: usize) -> Result<Vec<Bgra>, SurfaceError> {
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| SurfaceError::AllocationFailed { requested_bytes })?;
    Ok(pixels)
}

fn byte_buffer(requested_bytes: usize) -> Result<Vec<u8>, SurfaceError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(requested_bytes)
        .map_err(|_| SurfaceError::AllocationFailed { requested_bytes })?;
    Ok(bytes)
}
