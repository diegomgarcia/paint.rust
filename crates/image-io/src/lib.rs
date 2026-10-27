#![doc = "Bounded standard-image decoding, encoding, and atomic file output."]

mod atomic;
mod png;

pub use atomic::{AtomicWriteError, AtomicWriteStage, atomic_write};
pub use png::{
    PngError, PngLimits, decode_png, decode_png_with_limits, encode_png, encode_png_with_limits,
    write_png_atomic,
};
