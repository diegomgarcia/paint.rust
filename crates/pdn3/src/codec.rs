use paint_document::Document;

use crate::{DecodedPdn, PdnError, PdnLimits, PdnWriteOptions};

pub fn decode_pdn(bytes: &[u8]) -> Result<DecodedPdn, PdnError> {
    decode_pdn_with_limits(bytes, &PdnLimits::default())
}

pub fn decode_pdn_with_limits(bytes: &[u8], limits: &PdnLimits) -> Result<DecodedPdn, PdnError> {
    crate::reader::decode(bytes, limits)
}

/// Experimental Normal/BGRA PDN writer.
///
/// This is self-round-trip-tested only; stock Paint.NET 5.1.12 acceptance is unverified.
pub fn encode_pdn(document: &Document) -> Result<Vec<u8>, PdnError> {
    encode_pdn_with_options(document, &PdnWriteOptions::default())
}

/// Experimental writer with chunk options; see [`encode_pdn`]'s compatibility limitation.
pub fn encode_pdn_with_options(
    document: &Document,
    options: &PdnWriteOptions,
) -> Result<Vec<u8>, PdnError> {
    encode_pdn_with_options_and_limits(document, options, &PdnLimits::default())
}

/// Experimental writer with chunk and resource limits; see [`encode_pdn`]'s compatibility limitation.
pub fn encode_pdn_with_options_and_limits(
    document: &Document,
    options: &PdnWriteOptions,
    limits: &PdnLimits,
) -> Result<Vec<u8>, PdnError> {
    crate::writer::encode(document, options, limits)
}

#[cfg(test)]
mod tests;
