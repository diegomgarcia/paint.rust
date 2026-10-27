use std::io::{Read, Write};

use flate2::Compression;
use flate2::bufread::GzDecoder;
use flate2::write::GzEncoder;

use crate::{PdnChunkCompression, PdnError, PdnLimits, PdnUnsupported, PdnWriteOptions};

pub(crate) struct ChunkReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ChunkReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    fn take(&mut self, length: usize, context: &'static str) -> Result<&'a [u8], PdnError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(PdnError::Truncated {
                offset: self.position,
                context,
            })?;
        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn byte(&mut self, context: &'static str) -> Result<u8, PdnError> {
        Ok(self.take(1, context)?[0])
    }

    fn u32(&mut self, context: &'static str) -> Result<u32, PdnError> {
        Ok(u32::from_be_bytes(
            self.take(4, context)?
                .try_into()
                .map_err(|_| PdnError::Malformed {
                    offset: self.position,
                    reason: "invalid big-endian integer",
                })?,
        ))
    }
}

pub(crate) fn decode_chunks(
    reader: &mut ChunkReader<'_>,
    length: usize,
    limits: &PdnLimits,
) -> Result<Vec<u8>, PdnError> {
    let version = reader.byte("pixel chunk format")?;
    let compression = match version {
        0 => PdnChunkCompression::Gzip,
        1 => PdnChunkCompression::Raw,
        other => return Err(PdnError::Unsupported(PdnUnsupported::ChunkFormat(other))),
    };
    let chunk_size = usize::try_from(reader.u32("pixel chunk size")?)
        .map_err(|_| malformed("chunk size does not fit the host address space"))?;
    let count = chunk_count(length, chunk_size, limits)?;
    let mut output = allocate(length, "pixel storage")?;
    output.resize(length, 0);
    let mut seen = allocate(count, "chunk presence table")?;
    seen.resize(count, false);
    for _ in 0..count {
        let index = usize::try_from(reader.u32("pixel chunk number")?)
            .map_err(|_| malformed("chunk number does not fit the host address space"))?;
        if index >= count {
            return Err(malformed("pixel chunk number is out of range"));
        }
        if seen[index] {
            return Err(malformed("duplicate pixel chunk number"));
        }
        seen[index] = true;
        let stored_size = usize::try_from(reader.u32("stored pixel chunk size")?)
            .map_err(|_| malformed("stored chunk size does not fit the host address space"))?;
        if stored_size > limits.max_compressed_chunk_bytes {
            return Err(PdnError::ResourceLimit {
                resource: "compressed chunk bytes",
                requested: stored_size as u64,
                maximum: limits.max_compressed_chunk_bytes,
            });
        }
        let offset = index
            .checked_mul(chunk_size)
            .ok_or_else(|| malformed("pixel chunk offset overflow"))?;
        let expected = chunk_size.min(length - offset);
        let stored = reader.take(stored_size, "stored pixel chunk")?;
        match compression {
            PdnChunkCompression::Raw => {
                if stored_size != expected {
                    return Err(malformed("raw pixel chunk has the wrong length"));
                }
                output[offset..offset + expected].copy_from_slice(stored);
            }
            PdnChunkCompression::Gzip => {
                let decoded = decompress_exact(stored, expected)?;
                output[offset..offset + expected].copy_from_slice(&decoded);
            }
        }
    }
    Ok(output)
}

pub(crate) fn encode_chunks(
    bytes: &[u8],
    options: &PdnWriteOptions,
    limits: &PdnLimits,
) -> Result<Vec<u8>, PdnError> {
    let count = chunk_count(bytes.len(), options.chunk_size, limits)?;
    let mut output = allocate(5, "pixel chunk headers")?;
    output.push(match options.compression {
        PdnChunkCompression::Gzip => 0,
        PdnChunkCompression::Raw => 1,
    });
    let chunk_size = u32::try_from(options.chunk_size)
        .map_err(|_| malformed("chunk size exceeds the PDN field width"))?;
    output.extend_from_slice(&chunk_size.to_be_bytes());
    for (index, chunk) in bytes.chunks(options.chunk_size).enumerate() {
        debug_assert!(index < count);
        let stored = match options.compression {
            PdnChunkCompression::Raw => chunk.to_vec(),
            PdnChunkCompression::Gzip => {
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(chunk)?;
                encoder.finish()?
            }
        };
        if stored.len() > limits.max_compressed_chunk_bytes {
            return Err(PdnError::ResourceLimit {
                resource: "compressed chunk bytes",
                requested: stored.len() as u64,
                maximum: limits.max_compressed_chunk_bytes,
            });
        }
        let index =
            u32::try_from(index).map_err(|_| malformed("chunk index exceeds PDN field width"))?;
        let stored_size = u32::try_from(stored.len())
            .map_err(|_| malformed("stored chunk size exceeds PDN field width"))?;
        output.extend_from_slice(&index.to_be_bytes());
        output.extend_from_slice(&stored_size.to_be_bytes());
        output.extend_from_slice(&stored);
    }
    Ok(output)
}

fn chunk_count(length: usize, chunk_size: usize, limits: &PdnLimits) -> Result<usize, PdnError> {
    if chunk_size == 0 || chunk_size > limits.max_chunk_bytes {
        return Err(PdnError::ResourceLimit {
            resource: "chunk bytes",
            requested: chunk_size as u64,
            maximum: limits.max_chunk_bytes,
        });
    }
    let count = length
        .checked_add(chunk_size - 1)
        .ok_or_else(|| malformed("chunk count overflow"))?
        / chunk_size;
    if count > limits.max_chunks_per_surface {
        return Err(PdnError::ResourceLimit {
            resource: "chunks per surface",
            requested: count as u64,
            maximum: limits.max_chunks_per_surface,
        });
    }
    Ok(count)
}

fn decompress_exact(input: &[u8], expected: usize) -> Result<Vec<u8>, PdnError> {
    let capacity = expected
        .checked_add(1)
        .ok_or_else(|| malformed("gzip output bound overflow"))?;
    let mut output = allocate(capacity, "bounded gzip output")?;
    let mut decoder = GzDecoder::new(input);
    (&mut decoder)
        .take(capacity as u64)
        .read_to_end(&mut output)
        .map_err(PdnError::CompressionRead)?;
    if output.len() != expected {
        return Err(malformed("gzip chunk has the wrong decoded length"));
    }
    if !decoder.get_ref().is_empty() {
        return Err(malformed("gzip chunk has trailing compressed data"));
    }
    Ok(output)
}

fn allocate<T>(count: usize, context: &'static str) -> Result<Vec<T>, PdnError> {
    let size = std::mem::size_of::<T>()
        .checked_mul(count)
        .ok_or_else(|| malformed("allocation size overflow"))?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| PdnError::Allocation {
            context,
            requested: size,
        })?;
    Ok(values)
}

fn malformed(reason: &'static str) -> PdnError {
    PdnError::Malformed { offset: 0, reason }
}
