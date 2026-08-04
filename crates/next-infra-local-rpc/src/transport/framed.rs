use std::fmt;
use std::io::{self, Read, Write};

use serde::{Serialize, de::DeserializeOwned};

use crate::protocol::{FRAME_HEADER_BYTES, MAX_FRAME_BYTES, encode_frame};

/// Errors produced while reading or writing a bounded stream frame.
#[derive(Debug)]
pub enum FramedError {
    Io(io::Error),
    Eof { expected: usize, actual: usize },
    ZeroLength,
    OversizedFrame { length: usize },
    InvalidJson(String),
}

impl FramedError {
    pub fn is_eof(&self) -> bool {
        matches!(self, Self::Eof { .. })
    }

    pub fn is_oversized(&self) -> bool {
        matches!(self, Self::OversizedFrame { .. })
    }

    pub fn is_invalid_frame(&self) -> bool {
        matches!(self, Self::ZeroLength | Self::InvalidJson(_))
    }
}

impl fmt::Display for FramedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Eof { expected, actual } => {
                write!(
                    formatter,
                    "frame ended early: expected {expected} bytes, received {actual}"
                )
            }
            Self::ZeroLength => formatter.write_str("frame body length must be non-zero"),
            Self::OversizedFrame { length } => {
                write!(
                    formatter,
                    "frame body length {length} exceeds the 1 MiB maximum"
                )
            }
            Self::InvalidJson(message) => {
                write!(formatter, "invalid UTF-8 or JSON body: {message}")
            }
        }
    }
}

impl std::error::Error for FramedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Eof { .. }
            | Self::ZeroLength
            | Self::OversizedFrame { .. }
            | Self::InvalidJson(_) => None,
        }
    }
}

impl From<io::Error> for FramedError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Read one exact body from a stream.
///
/// The 4-byte header is read before any body allocation.  A declared length
/// over [`MAX_FRAME_BYTES`] is rejected immediately.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, FramedError> {
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    read_exact_count(reader, &mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(FramedError::ZeroLength);
    }
    if length > MAX_FRAME_BYTES {
        return Err(FramedError::OversizedFrame { length });
    }

    let mut body = vec![0_u8; length];
    read_exact_count(reader, &mut body)?;
    Ok(body)
}

/// Read and deserialize one bounded JSON frame.
pub fn read_json_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, FramedError> {
    let body = read_frame(reader)?;
    serde_json::from_slice(&body).map_err(|error| FramedError::InvalidJson(error.to_string()))
}

/// Encode and write one bounded JSON frame.
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), FramedError> {
    let frame = encode_frame(value).map_err(|error| {
        if error.is_oversized() {
            let length = match error {
                crate::protocol::FrameError::OversizedFrame { length } => length,
                crate::protocol::FrameError::InvalidFrame(_) => 0,
            };
            FramedError::OversizedFrame { length }
        } else {
            FramedError::InvalidJson(error.to_string())
        }
    })?;
    writer.write_all(&frame)?;
    Ok(())
}

/// Encode, write and flush one bounded JSON frame.
pub fn write_json_frame<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), FramedError> {
    write_frame(writer, value)?;
    writer.flush()?;
    Ok(())
}

fn read_exact_count<R: Read>(reader: &mut R, bytes: &mut [u8]) -> Result<(), FramedError> {
    let expected = bytes.len();
    let mut actual = 0;
    while actual < expected {
        match reader.read(&mut bytes[actual..]) {
            Ok(0) => {
                return Err(FramedError::Eof { expected, actual });
            }
            Ok(count) => actual += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(FramedError::Io(error)),
        }
    }
    Ok(())
}
