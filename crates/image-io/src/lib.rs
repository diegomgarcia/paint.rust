#![doc = "Bounded PNG decoding, multi-format raster encoding, and atomic file output."]

mod atomic;
mod formats;
mod png;

pub use atomic::{AtomicWriteError, AtomicWriteStage, atomic_write};
pub use formats::{FormatError, SaveFormat, encode_image, write_image_atomic};
pub use png::{
    PngError, PngLimits, decode_png, decode_png_with_limits, encode_png, encode_png_with_limits,
    write_png_atomic,
};
