//! Secure Unix-domain transport primitives for Local RPC.
//!
//! The transport is intentionally small and synchronous.  It owns the
//! filesystem checks that make a local socket single-owner and the bounded
//! stream framing used by the frozen protocol codec.  Session negotiation and
//! query dispatch live above this module.

mod framed;
mod path;
mod peer;

pub use framed::{FramedError, read_frame, read_json_frame, write_frame, write_json_frame};
pub use path::{
    ExpectedType, FileIdentity, LOCK_FILE_MODE, PathViolation, RUN_DIR_MODE, SOCKET_MODE,
    SecureUnixListener, SocketError, SocketErrorKind, TransportPathError, TransportPaths, UnixLock,
    connect_unix, current_euid,
};
pub use peer::{
    PeerError, default_peer_uid, verify_peer_uid, verify_peer_uid_as, verify_peer_uid_with,
};

/// Unified transport error for callers that want one boundary type.
#[derive(Debug)]
pub enum TransportError {
    Path(TransportPathError),
    Socket(SocketError),
    Peer(PeerError),
    Frame(FramedError),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(error) => error.fmt(formatter),
            Self::Socket(error) => error.fmt(formatter),
            Self::Peer(error) => error.fmt(formatter),
            Self::Frame(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<TransportPathError> for TransportError {
    fn from(error: TransportPathError) -> Self {
        Self::Path(error)
    }
}

impl From<SocketError> for TransportError {
    fn from(error: SocketError) -> Self {
        Self::Socket(error)
    }
}

impl From<PeerError> for TransportError {
    fn from(error: PeerError) -> Self {
        Self::Peer(error)
    }
}

impl From<FramedError> for TransportError {
    fn from(error: FramedError) -> Self {
        Self::Frame(error)
    }
}
