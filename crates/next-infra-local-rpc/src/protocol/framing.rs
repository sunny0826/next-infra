use serde::{Serialize, de::DeserializeOwned};
use std::fmt;
use std::io::{self, Write};

use super::error::RpcError;

/// Number of bytes used by the unsigned big-endian body length prefix.
pub const FRAME_HEADER_BYTES: usize = 4;
/// Maximum JSON body size.  The prefix is not included in this value.
pub const MAX_FRAME_BYTES: usize = 1_048_576;

/// Framing failure.  No frame body is returned for any failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    OversizedFrame { length: usize },
    InvalidFrame(FrameErrorKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameErrorKind {
    MissingHeader { actual: usize },
    ZeroLength,
    Incomplete { expected: usize, actual: usize },
    TrailingBytes { expected: usize, actual: usize },
    InvalidJson(String),
}

impl FrameError {
    pub fn is_oversized(&self) -> bool {
        matches!(self, Self::OversizedFrame { .. })
    }

    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::InvalidFrame(_))
    }

    pub fn rpc_error(&self) -> RpcError {
        match self {
            Self::OversizedFrame { length } => RpcError::oversized_frame(format!(
                "frame body length {length} exceeds the 1 MiB maximum"
            )),
            Self::InvalidFrame(kind) => RpcError::invalid_frame(kind.to_string()),
        }
    }
}

impl fmt::Display for FrameErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHeader { actual } => write!(
                formatter,
                "frame is missing its 4-byte length header (received {actual} bytes)"
            ),
            Self::ZeroLength => formatter.write_str("frame body length must be non-zero"),
            Self::Incomplete { expected, actual } => write!(
                formatter,
                "frame is incomplete: expected {expected} bytes, received {actual}"
            ),
            Self::TrailingBytes { expected, actual } => write!(
                formatter,
                "frame has trailing bytes: expected {expected} bytes, received {actual}"
            ),
            Self::InvalidJson(message) => {
                write!(formatter, "invalid UTF-8 or JSON body: {message}")
            }
        }
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OversizedFrame { length } => write!(
                formatter,
                "frame body length {length} exceeds the 1 MiB maximum"
            ),
            Self::InvalidFrame(kind) => kind.fmt(formatter),
        }
    }
}

impl std::error::Error for FrameError {}

/// Encode one JSON value as a 4-byte big-endian length-prefixed frame.
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let mut writer = LimitedWriter::new(MAX_FRAME_BYTES);
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        if writer.exceeded {
            FrameError::OversizedFrame {
                length: writer.bytes_written,
            }
        } else {
            FrameError::InvalidFrame(FrameErrorKind::InvalidJson(error.to_string()))
        }
    })?;

    let body = writer.into_inner();
    if body.is_empty() {
        return Err(FrameError::InvalidFrame(FrameErrorKind::ZeroLength));
    }
    let body_len = body.len();
    let length =
        u32::try_from(body_len).map_err(|_| FrameError::OversizedFrame { length: body_len })?;
    let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + body_len);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Decode a frame and deserialize its exact JSON body.
pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, FrameError> {
    let body = decode_frame_bytes(frame)?;
    serde_json::from_slice(&body)
        .map_err(|error| FrameError::InvalidFrame(FrameErrorKind::InvalidJson(error.to_string())))
}

/// Decode a frame to its exact JSON body without deserializing it.
pub fn decode_frame_bytes(frame: &[u8]) -> Result<Vec<u8>, FrameError> {
    if frame.len() < FRAME_HEADER_BYTES {
        return Err(FrameError::InvalidFrame(FrameErrorKind::MissingHeader {
            actual: frame.len(),
        }));
    }

    let length = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if length == 0 {
        return Err(FrameError::InvalidFrame(FrameErrorKind::ZeroLength));
    }
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::OversizedFrame { length });
    }

    let expected = FRAME_HEADER_BYTES + length;
    if frame.len() < expected {
        return Err(FrameError::InvalidFrame(FrameErrorKind::Incomplete {
            expected,
            actual: frame.len(),
        }));
    }
    if frame.len() > expected {
        return Err(FrameError::InvalidFrame(FrameErrorKind::TrailingBytes {
            expected,
            actual: frame.len(),
        }));
    }
    Ok(frame[FRAME_HEADER_BYTES..expected].to_vec())
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    bytes_written: usize,
    exceeded: bool,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            bytes_written: 0,
            exceeded: false,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            self.bytes_written = self.bytes.len().saturating_add(bytes.len());
            return Err(io::Error::other("JSON body exceeds frame limit"));
        }
        self.bytes.extend_from_slice(bytes);
        self.bytes_written = self.bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
