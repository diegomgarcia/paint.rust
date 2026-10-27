use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Errors produced while validating dimensions, coordinates, or allocation sizes.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GeometryError {
    #[error("surface dimensions must be nonzero, got {width}x{height}")]
    ZeroDimension { width: u32, height: u32 },
    #[error(
        "rectangle must have positive width and height, got ({left}, {top})..({right}, {bottom})"
    )]
    InvalidRectangle {
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
    },
    #[error(
        "rectangle dimensions exceed the supported u32 range: ({left}, {top})..({right}, {bottom})"
    )]
    RectangleTooLarge {
        left: i64,
        top: i64,
        right: i64,
        bottom: i64,
    },
    #[error("rectangle edge overflow at ({x}, {y}) with size {width}x{height}")]
    CoordinateOverflow {
        x: i64,
        y: i64,
        width: u32,
        height: u32,
    },
    #[error("allocation size overflow for dimensions {width}x{height}")]
    AllocationSizeOverflow { width: u32, height: u32 },
}

/// A checked, nonzero two-dimensional size.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Size {
    width: u32,
    height: u32,
}

impl Size {
    pub fn new(width: u32, height: u32) -> Result<Self, GeometryError> {
        if width == 0 || height == 0 {
            return Err(GeometryError::ZeroDimension { width, height });
        }
        Ok(Self { width, height })
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    /// Returns the number of pixels, checking the host `usize` representation.
    pub fn checked_pixel_count(self) -> Result<usize, GeometryError> {
        let width =
            usize::try_from(self.width).map_err(|_| GeometryError::AllocationSizeOverflow {
                width: self.width,
                height: self.height,
            })?;
        let height =
            usize::try_from(self.height).map_err(|_| GeometryError::AllocationSizeOverflow {
                width: self.width,
                height: self.height,
            })?;
        width
            .checked_mul(height)
            .ok_or(GeometryError::AllocationSizeOverflow {
                width: self.width,
                height: self.height,
            })
    }

    /// Returns the canonical row stride in bytes (`width * 4`).
    pub fn checked_stride(self) -> Result<usize, GeometryError> {
        usize::try_from(self.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or(GeometryError::AllocationSizeOverflow {
                width: self.width,
                height: self.height,
            })
    }

    /// Returns the canonical contiguous BGRA buffer length in bytes.
    pub fn checked_buffer_len(self) -> Result<usize, GeometryError> {
        self.checked_pixel_count()?
            .checked_mul(4)
            .ok_or(GeometryError::AllocationSizeOverflow {
                width: self.width,
                height: self.height,
            })
    }

    /// Returns the half-open rectangle covering this size at the origin.
    pub const fn rect(self) -> Rect {
        Rect::from_size(self)
    }
}

/// An integer pixel coordinate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

impl Point {
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

/// A checked, nonempty, half-open rectangle with signed integer edges.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    width: u32,
    height: u32,
}

impl Rect {
    pub fn new(left: i64, top: i64, right: i64, bottom: i64) -> Result<Self, GeometryError> {
        let width = i128::from(right) - i128::from(left);
        let height = i128::from(bottom) - i128::from(top);
        if width <= 0 || height <= 0 {
            return Err(GeometryError::InvalidRectangle {
                left,
                top,
                right,
                bottom,
            });
        }
        let width = u32::try_from(width).map_err(|_| GeometryError::RectangleTooLarge {
            left,
            top,
            right,
            bottom,
        })?;
        let height = u32::try_from(height).map_err(|_| GeometryError::RectangleTooLarge {
            left,
            top,
            right,
            bottom,
        })?;
        Ok(Self {
            left,
            top,
            right,
            bottom,
            width,
            height,
        })
    }

    pub fn from_xywh(x: i64, y: i64, width: u32, height: u32) -> Result<Self, GeometryError> {
        let right = x
            .checked_add(i64::from(width))
            .ok_or(GeometryError::CoordinateOverflow {
                x,
                y,
                width,
                height,
            })?;
        let bottom = y
            .checked_add(i64::from(height))
            .ok_or(GeometryError::CoordinateOverflow {
                x,
                y,
                width,
                height,
            })?;
        Self::new(x, y, right, bottom)
    }

    pub const fn from_size(size: Size) -> Self {
        Self {
            left: 0,
            top: 0,
            right: size.width as i64,
            bottom: size.height as i64,
            width: size.width,
            height: size.height,
        }
    }

    pub const fn left(self) -> i64 {
        self.left
    }

    pub const fn top(self) -> i64 {
        self.top
    }

    pub const fn right(self) -> i64 {
        self.right
    }

    pub const fn bottom(self) -> i64 {
        self.bottom
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub const fn size(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(self, point: Point) -> bool {
        let x = i64::from(point.x);
        let y = i64::from(point.y);
        self.left <= x && x < self.right && self.top <= y && y < self.bottom
    }

    pub fn is_within(self, other: Self) -> bool {
        other.left <= self.left
            && self.right <= other.right
            && other.top <= self.top
            && self.bottom <= other.bottom
    }

    /// Returns the exact intersection, or `None` when the half-open areas do not overlap.
    pub fn intersection(self, other: Self) -> Option<Self> {
        Self::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        )
        .ok()
    }

    pub fn clip_to(self, size: Size) -> Option<Self> {
        self.intersection(size.rect())
    }
}

#[derive(Serialize, Deserialize)]
struct SizeRepr {
    width: u32,
    height: u32,
}

impl Serialize for Size {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SizeRepr {
            width: self.width,
            height: self.height,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let repr = SizeRepr::deserialize(deserializer)?;
        Self::new(repr.width, repr.height).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
struct RectRepr {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Serialize for Rect {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RectRepr {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let repr = RectRepr::deserialize(deserializer)?;
        Self::new(repr.left, repr.top, repr.right, repr.bottom).map_err(serde::de::Error::custom)
    }
}
