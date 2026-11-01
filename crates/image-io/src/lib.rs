#![doc = "Bounded flat-raster decoding and encoding, plus atomic file output."]

mod atomic;
mod dds;
mod decode;
mod formats;
mod png;

pub use atomic::{AtomicWriteError, AtomicWriteStage, atomic_write};
pub use decode::{
    DecodeError, RasterLimits, decode_image, decode_image_with_limits, open_image,
    open_image_with_limits,
};
pub use formats::{FormatError, RasterFormat, SaveFormat, encode_image, write_image_atomic};
pub use png::{
    PngError, PngLimits, decode_png, decode_png_with_limits, encode_png, encode_png_with_limits,
    write_png_atomic,
};
