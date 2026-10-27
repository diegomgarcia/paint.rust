use serde::{Deserialize, Serialize};

/// A straight-alpha pixel in canonical blue, green, red, alpha storage order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Bgra {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl Bgra {
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    pub const fn new(b: u8, g: u8, r: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    pub const fn from_bgra_bytes(bytes: [u8; 4]) -> Self {
        Self::new(bytes[0], bytes[1], bytes[2], bytes[3])
    }

    pub const fn to_bgra_bytes(self) -> [u8; 4] {
        [self.b, self.g, self.r, self.a]
    }

    pub const fn from_rgba(rgba: Rgba) -> Self {
        Self::new(rgba.b, rgba.g, rgba.r, rgba.a)
    }

    pub const fn from_rgba_bytes(bytes: [u8; 4]) -> Self {
        Self::new(bytes[2], bytes[1], bytes[0], bytes[3])
    }

    pub const fn to_rgba(self) -> Rgba {
        Rgba::new(self.r, self.g, self.b, self.a)
    }

    pub const fn to_rgba_bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Inverts color channels only; alpha, including zero and partial alpha, is unchanged.
    pub const fn inverted_rgb(self) -> Self {
        Self::new(255 - self.b, 255 - self.g, 255 - self.r, self.a)
    }
}

/// A straight-alpha pixel in red, green, blue, alpha file/interchange order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_bgra(bgra: Bgra) -> Self {
        bgra.to_rgba()
    }

    pub const fn to_bgra(self) -> Bgra {
        Bgra::from_rgba(self)
    }

    pub const fn to_rgba_bytes(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}
