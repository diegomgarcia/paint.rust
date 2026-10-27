#![doc = "Bounded standard-image decoding, encoding, and atomic file output."]

mod atomic;

pub use atomic::{AtomicWriteError, AtomicWriteStage, atomic_write};
