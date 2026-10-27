//! Versioned, length-bounded binary messages used by the Rust broker and .NET host.
//!
//! Every frame starts with a 16-byte header. Multi-byte integers are little-endian.
//! Pixel payloads are tightly packed, top-down BGRA8 with `width * 4` bytes per row;
//! no padding or premultiplication is implied. The entire payload is copied across
//! the process boundary.

use std::io::{self, Read, Write};

use thiserror::Error;

pub const MAGIC: [u8; 4] = *b"PRBR";
pub const PROTOCOL_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 16;
pub const MAX_BGRA_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PAYLOAD_LEN: usize = MAX_BGRA_BYTES + 64;
pub const MAX_EFFECT_NAME_BYTES: usize = 64;
pub const MAX_ERROR_MESSAGE_BYTES: usize = 4 * 1024;
pub const MAX_EFFECTS: usize = 64;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("I/O failed while transporting a plugin frame: {0}")]
    Io(#[from] io::Error),
    #[error("the plugin stream closed cleanly")]
    Eof,
    #[error("the plugin frame ended before its declared length")]
    Truncated,
    #[error("invalid plugin frame magic: {0:02x?}")]
    BadMagic([u8; 4]),
    #[error("unsupported plugin protocol version {0}; expected {PROTOCOL_VERSION}")]
    UnsupportedVersion(u16),
    #[error("plugin frame flags must be zero, got {0:#04x}")]
    InvalidFlags(u8),
    #[error("unknown plugin message kind {0}")]
    UnknownMessageKind(u8),
    #[error("plugin payload length {length} exceeds the {maximum}-byte bound")]
    PayloadTooLarge { length: usize, maximum: usize },
    #[error("invalid BGRA image: {0}")]
    InvalidImage(String),
    #[error("invalid plugin payload: {0}")]
    InvalidPayload(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MessageKind {
    Apply = 1,
    Cancel = 2,
    ApplyResult = 3,
    Error = 4,
    Hello = 5,
    Ready = 6,
    Shutdown = 7,
    ShutdownAck = 8,
}

impl TryFrom<u8> for MessageKind {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Apply),
            2 => Ok(Self::Cancel),
            3 => Ok(Self::ApplyResult),
            4 => Ok(Self::Error),
            5 => Ok(Self::Hello),
            6 => Ok(Self::Ready),
            7 => Ok(Self::Shutdown),
            8 => Ok(Self::ShutdownAck),
            other => Err(ProtocolError::UnknownMessageKind(other)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RemoteErrorCode {
    InvalidRequest = 1,
    UnsupportedEffect = 2,
    StaleGeneration = 3,
    Cancelled = 4,
    Internal = 5,
}

impl TryFrom<u16> for RemoteErrorCode {
    type Error = ProtocolError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::InvalidRequest),
            2 => Ok(Self::UnsupportedEffect),
            3 => Ok(Self::StaleGeneration),
            4 => Ok(Self::Cancelled),
            5 => Ok(Self::Internal),
            other => Err(ProtocolError::InvalidPayload(format!(
                "unknown remote error code {other}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BgraImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl BgraImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ProtocolError> {
        if width == 0 || height == 0 {
            return Err(ProtocolError::InvalidImage(
                "width and height must both be nonzero".to_owned(),
            ));
        }
        let expected = pixel_len(width, height)?;
        if pixels.len() != expected {
            return Err(ProtocolError::InvalidImage(format!(
                "{}x{} requires exactly {expected} bytes, got {}",
                width,
                height,
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn stride(&self) -> usize {
        // The constructor proved this multiplication cannot overflow.
        self.width as usize * 4
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }
}

fn pixel_len(width: u32, height: u32) -> Result<usize, ProtocolError> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| ProtocolError::InvalidImage("dimensions overflow usize".to_owned()))?;
    if pixels > MAX_BGRA_BYTES {
        return Err(ProtocolError::InvalidImage(format!(
            "pixel buffer length {pixels} exceeds the {MAX_BGRA_BYTES}-byte bound"
        )));
    }
    Ok(pixels)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub kind: MessageKind,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: MessageKind, payload: Vec<u8>) -> Result<Self, ProtocolError> {
        check_payload_len(payload.len())?;
        Ok(Self { kind, payload })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Apply {
        request_id: u64,
        generation: u64,
        effect: String,
        image: BgraImage,
    },
    Cancel {
        request_id: u64,
    },
    ApplyResult {
        request_id: u64,
        generation: u64,
        image: BgraImage,
    },
    Error {
        request_id: u64,
        generation: u64,
        code: RemoteErrorCode,
        message: String,
    },
    Hello {
        request_id: u64,
    },
    Ready {
        request_id: u64,
        effects: Vec<String>,
    },
    Shutdown {
        request_id: u64,
    },
    ShutdownAck {
        request_id: u64,
    },
}

impl Message {
    #[must_use]
    pub const fn kind(&self) -> MessageKind {
        match self {
            Self::Apply { .. } => MessageKind::Apply,
            Self::Cancel { .. } => MessageKind::Cancel,
            Self::ApplyResult { .. } => MessageKind::ApplyResult,
            Self::Error { .. } => MessageKind::Error,
            Self::Hello { .. } => MessageKind::Hello,
            Self::Ready { .. } => MessageKind::Ready,
            Self::Shutdown { .. } => MessageKind::Shutdown,
            Self::ShutdownAck { .. } => MessageKind::ShutdownAck,
        }
    }

    pub fn encode(&self) -> Result<Frame, ProtocolError> {
        let mut payload = Vec::new();
        match self {
            Self::Apply {
                request_id,
                generation,
                effect,
                image,
            } => {
                check_request_id(*request_id)?;
                let name = effect_bytes(effect)?;
                payload.extend_from_slice(&request_id.to_le_bytes());
                payload.extend_from_slice(&generation.to_le_bytes());
                payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
                payload.extend_from_slice(&image.width.to_le_bytes());
                payload.extend_from_slice(&image.height.to_le_bytes());
                payload.extend_from_slice(name);
                payload.extend_from_slice(image.pixels());
            }
            Self::Cancel { request_id }
            | Self::Hello { request_id }
            | Self::Shutdown { request_id }
            | Self::ShutdownAck { request_id } => {
                check_request_id(*request_id)?;
                payload.extend_from_slice(&request_id.to_le_bytes());
            }
            Self::ApplyResult {
                request_id,
                generation,
                image,
            } => {
                check_request_id(*request_id)?;
                payload.extend_from_slice(&request_id.to_le_bytes());
                payload.extend_from_slice(&generation.to_le_bytes());
                payload.extend_from_slice(&image.width.to_le_bytes());
                payload.extend_from_slice(&image.height.to_le_bytes());
                payload.extend_from_slice(image.pixels());
            }
            Self::Error {
                request_id,
                generation,
                code,
                message,
            } => {
                check_request_id(*request_id)?;
                let message = checked_string(message, MAX_ERROR_MESSAGE_BYTES, "error message")?;
                payload.extend_from_slice(&request_id.to_le_bytes());
                payload.extend_from_slice(&generation.to_le_bytes());
                payload.extend_from_slice(&(*code as u16).to_le_bytes());
                payload.extend_from_slice(&0_u16.to_le_bytes());
                payload.extend_from_slice(&(message.len() as u32).to_le_bytes());
                payload.extend_from_slice(message);
            }
            Self::Ready {
                request_id,
                effects,
            } => {
                check_request_id(*request_id)?;
                if effects.is_empty() || effects.len() > MAX_EFFECTS {
                    return Err(ProtocolError::InvalidPayload(format!(
                        "ready effect count must be in 1..={MAX_EFFECTS}"
                    )));
                }
                payload.extend_from_slice(&request_id.to_le_bytes());
                payload.extend_from_slice(&(effects.len() as u16).to_le_bytes());
                payload.extend_from_slice(&0_u16.to_le_bytes());
                for effect in effects {
                    let name = effect_bytes(effect)?;
                    payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
                    payload.extend_from_slice(name);
                }
            }
        }
        Frame::new(self.kind(), payload)
    }

    pub fn decode(frame: Frame) -> Result<Self, ProtocolError> {
        let mut reader = PayloadReader::new(&frame.payload);
        let message = match frame.kind {
            MessageKind::Apply => {
                let request_id = reader.u64()?;
                let generation = reader.u64()?;
                check_request_id(request_id)?;
                let name_len = reader.u16()? as usize;
                let width = reader.u32()?;
                let height = reader.u32()?;
                let effect = reader.string(name_len, MAX_EFFECT_NAME_BYTES, "effect name")?;
                if effect.is_empty() {
                    return Err(ProtocolError::InvalidPayload(
                        "effect name must not be empty".to_owned(),
                    ));
                }
                let expected_pixels = pixel_len(width, height)
                    .map_err(|error| ProtocolError::InvalidPayload(error.to_string()))?;
                if reader.remaining() != expected_pixels {
                    return Err(ProtocolError::InvalidPayload(format!(
                        "apply payload has {} pixel bytes; expected {expected_pixels}",
                        reader.remaining()
                    )));
                }
                let image = BgraImage::new(width, height, reader.take(expected_pixels)?.to_vec())
                    .map_err(|error| ProtocolError::InvalidPayload(error.to_string()))?;
                Self::Apply {
                    request_id,
                    generation,
                    effect,
                    image,
                }
            }
            MessageKind::Cancel => {
                let request_id = reader.u64()?;
                check_request_id(request_id)?;
                Self::Cancel { request_id }
            }
            MessageKind::ApplyResult => {
                let request_id = reader.u64()?;
                let generation = reader.u64()?;
                check_request_id(request_id)?;
                let width = reader.u32()?;
                let height = reader.u32()?;
                let expected_pixels = pixel_len(width, height)
                    .map_err(|error| ProtocolError::InvalidPayload(error.to_string()))?;
                if reader.remaining() != expected_pixels {
                    return Err(ProtocolError::InvalidPayload(format!(
                        "result payload has {} pixel bytes; expected {expected_pixels}",
                        reader.remaining()
                    )));
                }
                let image = BgraImage::new(width, height, reader.take(expected_pixels)?.to_vec())
                    .map_err(|error| ProtocolError::InvalidPayload(error.to_string()))?;
                Self::ApplyResult {
                    request_id,
                    generation,
                    image,
                }
            }
            MessageKind::Error => {
                let request_id = reader.u64()?;
                let generation = reader.u64()?;
                check_request_id(request_id)?;
                let code = RemoteErrorCode::try_from(reader.u16()?)?;
                if reader.u16()? != 0 {
                    return Err(ProtocolError::InvalidPayload(
                        "error reserved field must be zero".to_owned(),
                    ));
                }
                let message_len = reader.u32()? as usize;
                let message =
                    reader.string(message_len, MAX_ERROR_MESSAGE_BYTES, "error message")?;
                if message.is_empty() {
                    return Err(ProtocolError::InvalidPayload(
                        "error message must not be empty".to_owned(),
                    ));
                }
                Self::Error {
                    request_id,
                    generation,
                    code,
                    message,
                }
            }
            MessageKind::Hello => {
                let request_id = reader.u64()?;
                check_request_id(request_id)?;
                Self::Hello { request_id }
            }
            MessageKind::Ready => {
                let request_id = reader.u64()?;
                check_request_id(request_id)?;
                let count = reader.u16()? as usize;
                if reader.u16()? != 0 {
                    return Err(ProtocolError::InvalidPayload(
                        "ready reserved field must be zero".to_owned(),
                    ));
                }
                if count == 0 || count > MAX_EFFECTS {
                    return Err(ProtocolError::InvalidPayload(format!(
                        "ready effect count must be in 1..={MAX_EFFECTS}"
                    )));
                }
                let mut effects = Vec::with_capacity(count);
                for _ in 0..count {
                    let length = reader.u16()? as usize;
                    let effect = reader.string(length, MAX_EFFECT_NAME_BYTES, "effect name")?;
                    if effect.is_empty() {
                        return Err(ProtocolError::InvalidPayload(
                            "effect name must not be empty".to_owned(),
                        ));
                    }
                    effects.push(effect);
                }
                Self::Ready {
                    request_id,
                    effects,
                }
            }
            MessageKind::Shutdown => {
                let request_id = reader.u64()?;
                check_request_id(request_id)?;
                Self::Shutdown { request_id }
            }
            MessageKind::ShutdownAck => {
                let request_id = reader.u64()?;
                check_request_id(request_id)?;
                Self::ShutdownAck { request_id }
            }
        };
        reader.finish()?;
        Ok(message)
    }
}

fn check_request_id(request_id: u64) -> Result<(), ProtocolError> {
    if request_id == 0 {
        return Err(ProtocolError::InvalidPayload(
            "request IDs must be nonzero".to_owned(),
        ));
    }
    Ok(())
}

fn effect_bytes(effect: &str) -> Result<&[u8], ProtocolError> {
    checked_string(effect, MAX_EFFECT_NAME_BYTES, "effect name")
}

fn checked_string<'a>(
    value: &'a str,
    maximum: usize,
    label: &str,
) -> Result<&'a [u8], ProtocolError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(ProtocolError::InvalidPayload(format!(
            "{label} byte length must be in 1..={maximum}, got {}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

fn check_payload_len(length: usize) -> Result<(), ProtocolError> {
    if length > MAX_PAYLOAD_LEN {
        return Err(ProtocolError::PayloadTooLarge {
            length,
            maximum: MAX_PAYLOAD_LEN,
        });
    }
    Ok(())
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Frame, ProtocolError> {
    let mut header = [0_u8; HEADER_LEN];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Err(ProtocolError::Eof),
            Ok(1) => break,
            Ok(_) => {
                return Err(ProtocolError::InvalidPayload(
                    "a one-byte read returned an invalid count".to_owned(),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    read_exact(&mut header[1..], reader)?;
    if header[..4] != MAGIC {
        return Err(ProtocolError::BadMagic([
            header[0], header[1], header[2], header[3],
        ]));
    }
    let version = u16::from_le_bytes([header[4], header[5]]);
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    let kind = MessageKind::try_from(header[6])?;
    if header[7] != 0 {
        return Err(ProtocolError::InvalidFlags(header[7]));
    }
    let length = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if header[12..].iter().any(|byte| *byte != 0) {
        return Err(ProtocolError::InvalidPayload(
            "frame header reserved bytes must be zero".to_owned(),
        ));
    }
    check_payload_len(length)?;
    let mut payload = vec![0_u8; length];
    read_exact(&mut payload, reader)?;
    Frame::new(kind, payload)
}

fn read_exact<R: Read>(mut destination: &mut [u8], reader: &mut R) -> Result<(), ProtocolError> {
    while !destination.is_empty() {
        match reader.read(destination) {
            Ok(0) => return Err(ProtocolError::Truncated),
            Ok(count) => destination = &mut destination[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn write_frame<W: Write>(writer: &mut W, frame: &Frame) -> Result<(), ProtocolError> {
    check_payload_len(frame.payload.len())?;
    let payload_len =
        u32::try_from(frame.payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            length: frame.payload.len(),
            maximum: u32::MAX as usize,
        })?;
    let mut header = [0_u8; HEADER_LEN];
    header[..4].copy_from_slice(&MAGIC);
    header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    header[6] = frame.kind as u8;
    header[8..12].copy_from_slice(&payload_len.to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(&frame.payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(reader: &mut R) -> Result<Message, ProtocolError> {
    Message::decode(read_frame(reader)?)
}

pub fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<(), ProtocolError> {
    write_frame(writer, &message.encode()?)
}

struct PayloadReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PayloadReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProtocolError::InvalidPayload("payload offset overflow".to_owned()))?;
        if end > self.bytes.len() {
            return Err(ProtocolError::InvalidPayload(format!(
                "payload needs {count} more bytes but only {} remain",
                self.remaining()
            )));
        }
        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn u16(&mut self) -> Result<u16, ProtocolError> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| ProtocolError::InvalidPayload("could not read a u16".to_owned()))?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ProtocolError::InvalidPayload("could not read a u32".to_owned()))?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| ProtocolError::InvalidPayload("could not read a u64".to_owned()))?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn string(
        &mut self,
        length: usize,
        maximum: usize,
        label: &str,
    ) -> Result<String, ProtocolError> {
        if length == 0 || length > maximum {
            return Err(ProtocolError::InvalidPayload(format!(
                "{label} byte length must be in 1..={maximum}, got {length}"
            )));
        }
        let bytes = self.take(length)?;
        let value = std::str::from_utf8(bytes).map_err(|error| {
            ProtocolError::InvalidPayload(format!("{label} is not valid UTF-8: {error}"))
        })?;
        Ok(value.to_owned())
    }

    fn finish(self) -> Result<(), ProtocolError> {
        if self.remaining() != 0 {
            return Err(ProtocolError::InvalidPayload(format!(
                "{} trailing payload bytes",
                self.remaining()
            )));
        }
        Ok(())
    }
}
