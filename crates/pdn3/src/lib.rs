#![doc = "Bounded, versioned PDN3 container and semantic interoperability."]
#![doc = ""]
#![doc = "EXPERIMENTAL: the writer is currently self-round-trip-tested only; stock Paint.NET 5.1.12 acceptance is unverified. The packet remains PARTIAL."]

mod chunks;
mod codec;
mod error;
mod model;
pub mod nrbf;
mod reader;
mod writer;
mod xml;

pub use codec::{
    decode_pdn, decode_pdn_with_limits, encode_pdn, encode_pdn_with_options,
    encode_pdn_with_options_and_limits,
};
pub use error::{PdnError, PdnUnsupported};
pub use model::{
    DecodedPdn, PdnChunkCompression, PdnLimits, PdnReport, PdnUnmappedMember, PdnWriteOptions,
};
