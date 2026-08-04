//! Local RPC v1 wire contract.
//!
//! This module deliberately contains only the typed, read-only protocol.  It
//! does not open sockets, execute queries, or provide a generic method/value
//! escape hatch.

mod error;
mod framing;
mod handshake;
mod message;

pub use next_infra_query::dto::{Freshness, ResourceHealth};

pub use error::{ErrorCode, RpcError, RpcErrorEnvelope};
pub use framing::{
    FRAME_HEADER_BYTES, FrameError, FrameErrorKind, MAX_FRAME_BYTES, decode_frame,
    decode_frame_bytes, encode_frame,
};
pub use handshake::{
    Capability, CapabilitySet, ClientHello, HandshakeResponse, HandshakeResult, HostHello,
    MINIMUM_SUPPORTED_MINOR, PROTOCOL_MAJOR, PROTOCOL_MINOR, handshake_response, negotiate,
};
pub use message::{
    Caller, GetResourceQuery, GetTopologyQuery, QueryRequest, QueryResponse, RecentChangesQuery,
    RequestEnvelope, ResourceInclude, ResponseBody, ResponseEnvelope, SearchResourcesQuery,
    SyncStatusQuery,
};

/// Maximum UTF-8 byte length of a request identifier.
pub const MAX_REQUEST_ID_BYTES: usize = 128;

/// Maximum number of requests that may be in flight in one session.
pub const MAX_IN_FLIGHT_REQUESTS: usize = 8;

/// Validate a request identifier at the protocol boundary.
pub fn validate_request_id(request_id: &str) -> Result<(), RpcError> {
    if request_id.is_empty()
        || request_id.len() > MAX_REQUEST_ID_BYTES
        || request_id.chars().any(char::is_control)
    {
        return Err(RpcError::invalid_request_id());
    }
    Ok(())
}

/// Validate the session's in-flight request count before admitting a request.
pub fn validate_in_flight(count: usize) -> Result<(), RpcError> {
    if count >= MAX_IN_FLIGHT_REQUESTS {
        Err(RpcError::too_many_requests())
    } else {
        Ok(())
    }
}
