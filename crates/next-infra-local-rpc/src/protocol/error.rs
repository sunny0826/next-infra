use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable error codes exposed by Local RPC v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    HostUnavailable,
    ProtocolMismatch,
    CapabilityMismatch,
    InvalidFrame,
    FrameTooLarge,
    InvalidRequestId,
    TooManyRequests,
    QueryFailed,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostUnavailable => "host_unavailable",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::CapabilityMismatch => "capability_mismatch",
            Self::InvalidFrame => "invalid_frame",
            Self::FrameTooLarge => "frame_too_large",
            Self::InvalidRequestId => "invalid_request_id",
            Self::TooManyRequests => "too_many_requests",
            Self::QueryFailed => "query_failed",
        }
    }
}

/// Structured, user-safe Local RPC failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

/// Alias retained for callers that refer to the wire object as an envelope.
pub type RpcErrorEnvelope = RpcError;

impl RpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn host_unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::HostUnavailable, message, true)
    }

    pub fn protocol_mismatch(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ProtocolMismatch, message, false)
    }

    pub fn capability_mismatch(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CapabilityMismatch, message, false)
    }

    pub fn invalid_frame(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidFrame, message, false)
    }

    pub fn oversized_frame(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::FrameTooLarge, message, false)
    }

    pub fn invalid_request_id() -> Self {
        Self::new(
            ErrorCode::InvalidRequestId,
            "request_id must be a non-empty UTF-8 string no longer than 128 bytes",
            false,
        )
    }

    pub fn too_many_requests() -> Self {
        Self::new(
            ErrorCode::TooManyRequests,
            "the session has reached its in-flight request limit",
            true,
        )
    }

    pub fn query_failed(message: impl Into<String>, retryable: bool) -> Self {
        Self::new(ErrorCode::QueryFailed, message, retryable)
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for RpcError {}
