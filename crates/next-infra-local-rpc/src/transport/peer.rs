use std::fmt;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

#[derive(Debug)]
pub enum PeerError {
    Io(io::Error),
    Mismatch { expected: u32, actual: u32 },
}

impl PeerError {
    pub fn is_mismatch(&self) -> bool {
        matches!(self, Self::Mismatch { .. })
    }
}

impl fmt::Display for PeerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "peer uid lookup failed: {error}"),
            Self::Mismatch { expected, actual } => {
                write!(
                    formatter,
                    "peer uid {actual} does not match effective uid {expected}"
                )
            }
        }
    }
}

impl std::error::Error for PeerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Mismatch { .. } => None,
        }
    }
}

impl From<io::Error> for PeerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Read the peer's effective uid using the platform Unix-domain credential API.
pub fn default_peer_uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(target_os = "macos")]
    {
        let mut uid = 0 as libc::uid_t;
        let mut gid = 0 as libc::gid_t;
        let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
        if result == 0 {
            Ok(uid as u32)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Linux CI does not provide getpeereid.  SO_PEERCRED is the native
        // equivalent and keeps the same uid-only verification contract.
        let mut credentials = unsafe { std::mem::zeroed::<libc::ucred>() };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if result == 0 {
            Ok(credentials.uid as u32)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// Verify the peer uid against the current effective uid.
pub fn verify_peer_uid(stream: &UnixStream) -> Result<(), PeerError> {
    verify_peer_uid_with(stream, default_peer_uid)
}

/// Verify with an injectable credential lookup, useful for rejection tests.
pub fn verify_peer_uid_with<F>(stream: &UnixStream, verifier: F) -> Result<(), PeerError>
where
    F: FnOnce(&UnixStream) -> io::Result<u32>,
{
    verify_peer_uid_as(stream, super::current_euid(), verifier)
}

/// Verify with an explicit expected uid and an injectable credential lookup.
pub fn verify_peer_uid_as<F>(
    stream: &UnixStream,
    expected_uid: u32,
    verifier: F,
) -> Result<(), PeerError>
where
    F: FnOnce(&UnixStream) -> io::Result<u32>,
{
    let actual_uid = verifier(stream)?;
    if actual_uid == expected_uid {
        Ok(())
    } else {
        Err(PeerError::Mismatch {
            expected: expected_uid,
            actual: actual_uid,
        })
    }
}
