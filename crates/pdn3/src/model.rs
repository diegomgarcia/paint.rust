use paint_document::Document;

use crate::nrbf::{NrbfLimits, NrbfStream, ObjectId};

#[derive(Clone, Debug)]
pub struct PdnLimits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_header_bytes: usize,
    pub max_xml_depth: usize,
    pub max_xml_elements: usize,
    pub max_layers: usize,
    pub max_pixels: usize,
    pub max_surface_bytes: usize,
    pub max_total_surface_bytes: usize,
    pub max_chunk_bytes: usize,
    pub max_compressed_chunk_bytes: usize,
    pub max_chunks_per_surface: usize,
    pub nrbf: NrbfLimits,
}

impl Default for PdnLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 512 * 1024 * 1024,
            max_output_bytes: 512 * 1024 * 1024,
            max_header_bytes: 1024 * 1024,
            max_xml_depth: 64,
            max_xml_elements: 8192,
            max_layers: 256,
            max_pixels: 64 * 1024 * 1024,
            max_surface_bytes: 256 * 1024 * 1024,
            max_total_surface_bytes: 512 * 1024 * 1024,
            max_chunk_bytes: 16 * 1024 * 1024,
            max_compressed_chunk_bytes: 64 * 1024 * 1024,
            max_chunks_per_surface: 1_000_000,
            nrbf: NrbfLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PdnChunkCompression {
    Raw,
    Gzip,
}

#[derive(Clone, Copy, Debug)]
pub struct PdnWriteOptions {
    pub compression: PdnChunkCompression,
    pub chunk_size: usize,
}

impl Default for PdnWriteOptions {
    fn default() -> Self {
        Self {
            compression: PdnChunkCompression::Raw,
            chunk_size: 256 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdnUnmappedMember {
    pub object_id: ObjectId,
    pub class_name: String,
    pub member_name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PdnReport {
    pub unmapped_members: Vec<PdnUnmappedMember>,
    pub unmapped_objects: Vec<ObjectId>,
    pub document_metadata_entries: usize,
    pub layer_metadata_entries: Vec<usize>,
    pub padded_layers: Vec<usize>,
    pub shared_layers: Vec<usize>,
    pub trailing_bytes: usize,
}

/// A semantic decode plus the original data needed to avoid silent metadata loss.
#[derive(Clone, Debug)]
pub struct DecodedPdn {
    pub document: Document,
    pub header_xml: String,
    pub producer_version: String,
    pub nrbf: NrbfStream,
    pub report: PdnReport,
    pub trailing_bytes: Vec<u8>,
}
