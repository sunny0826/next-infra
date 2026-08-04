use std::borrow::Borrow;
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use libc::{
    EEXIST, EINVAL, ELOOP, ENOENT, EWOULDBLOCK, LOCK_EX, LOCK_NB, O_CLOEXEC, O_DIRECTORY,
    O_NOFOLLOW, O_RDONLY,
};

pub const RUN_DIR_MODE: u32 = 0o700;
pub const LOCK_FILE_MODE: u32 = 0o600;
pub const SOCKET_MODE: u32 = 0o600;

/// The filesystem identity used to make cleanup safe across path replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedType {
    Directory,
    RegularFile,
    Socket,
}

impl fmt::Display for ExpectedType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Directory => "directory",
            Self::RegularFile => "regular file",
            Self::Socket => "socket",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathViolation {
    Symlink,
    Replaced,
    WrongType { expected: ExpectedType },
    WrongOwner { expected: u32, actual: u32 },
    WrongMode { expected: u32, actual: u32 },
}

impl fmt::Display for PathViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symlink => formatter.write_str("symlink is not allowed"),
            Self::Replaced => formatter.write_str("path was replaced during validation"),
            Self::WrongType { expected } => write!(formatter, "expected {expected}"),
            Self::WrongOwner { expected, actual } => {
                write!(
                    formatter,
                    "owner is {actual}, expected effective uid {expected}"
                )
            }
            Self::WrongMode { expected, actual } => {
                write!(formatter, "mode is {actual:04o}, expected {expected:04o}")
            }
        }
    }
}

#[derive(Debug)]
pub enum TransportPathError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Missing {
        path: PathBuf,
    },
    Invalid {
        path: PathBuf,
        violation: PathViolation,
    },
}

impl TransportPathError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn invalid(path: &Path, violation: PathViolation) -> Self {
        Self::Invalid {
            path: path.to_path_buf(),
            violation,
        }
    }

    pub fn is_security_failure(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Io { path, .. } | Self::Missing { path } | Self::Invalid { path, .. } => path,
        }
    }
}

impl fmt::Display for TransportPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Missing { path } => write!(formatter, "{} does not exist", path.display()),
            Self::Invalid { path, violation } => {
                write!(formatter, "{}: {violation}", path.display())
            }
        }
    }
}

impl std::error::Error for TransportPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Missing { .. } | Self::Invalid { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketErrorKind {
    Active,
    Security(PathViolation),
    AlreadyRunning,
    Replaced,
}

#[derive(Debug)]
pub enum SocketError {
    Io(io::Error),
    Path(TransportPathError),
    Kind {
        path: PathBuf,
        kind: SocketErrorKind,
    },
}

impl SocketError {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Kind {
                kind: SocketErrorKind::Active,
                ..
            }
        )
    }

    pub fn is_already_running(&self) -> bool {
        matches!(
            self,
            Self::Kind {
                kind: SocketErrorKind::AlreadyRunning,
                ..
            }
        )
    }

    pub fn is_security_failure(&self) -> bool {
        match self {
            Self::Path(error) => error.is_security_failure(),
            Self::Kind {
                kind: SocketErrorKind::Security(_),
                ..
            } => true,
            Self::Io(_) | Self::Kind { .. } => false,
        }
    }
}

impl fmt::Display for SocketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Path(error) => error.fmt(formatter),
            Self::Kind { path, kind } => match kind {
                SocketErrorKind::Active => {
                    write!(formatter, "{} is already active", path.display())
                }
                SocketErrorKind::Security(violation) => {
                    write!(formatter, "{}: {violation}", path.display())
                }
                SocketErrorKind::AlreadyRunning => {
                    write!(formatter, "another server holds {}", path.display())
                }
                SocketErrorKind::Replaced => {
                    write!(formatter, "{} was replaced during startup", path.display())
                }
            },
        }
    }
}

impl std::error::Error for SocketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Path(error) => Some(error),
            Self::Kind { .. } => None,
        }
    }
}

impl From<TransportPathError> for SocketError {
    fn from(error: TransportPathError) -> Self {
        Self::Path(error)
    }
}

/// Canonical names for the per-user Local RPC run directory and endpoints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportPaths {
    run_dir: PathBuf,
    socket_path: PathBuf,
    lock_path: PathBuf,
}

impl TransportPaths {
    /// Use an explicit run directory.  The directory is created when absent.
    pub fn new(run_dir: impl AsRef<Path>) -> Result<Self, TransportPathError> {
        let run_dir = run_dir.as_ref().to_path_buf();
        ensure_run_dir(&run_dir)?;
        Ok(Self::from_validated_run_dir(run_dir))
    }

    /// Derive `<app-support>/run` and create/validate it.
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self, TransportPathError> {
        Self::new(root.as_ref().join("run"))
    }

    /// Open an already provisioned run directory without creating filesystem
    /// state. This is the Bridge/client path; only the Host may provision it.
    pub fn existing(run_dir: impl Into<PathBuf>) -> Result<Self, TransportPathError> {
        let run_dir = run_dir.into();
        validate_existing_run_dir(&run_dir)?;
        Ok(Self::from_validated_run_dir(run_dir))
    }

    pub fn from_existing_root(root: impl AsRef<Path>) -> Result<Self, TransportPathError> {
        Self::existing(root.as_ref().join("run"))
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn validate(&self) -> Result<(), TransportPathError> {
        ensure_run_dir(&self.run_dir)
    }

    fn from_validated_run_dir(run_dir: PathBuf) -> Self {
        Self {
            socket_path: run_dir.join("next-infra-v1.sock"),
            lock_path: run_dir.join("next-infra-v1.lock"),
            run_dir,
        }
    }
}

/// A held non-blocking, exclusive lock for the Local RPC server.
#[derive(Debug)]
pub struct UnixLock {
    file: File,
    path: PathBuf,
    identity: FileIdentity,
}

impl UnixLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self, SocketError> {
        let path = path.as_ref();
        let (file, created) = open_lock_file(path).map_err(SocketError::Path)?;
        if created {
            set_fd_mode(file.as_raw_fd(), LOCK_FILE_MODE)
                .map_err(|source| SocketError::Path(TransportPathError::io(path, source)))?;
        }

        let metadata = fs::symlink_metadata(path)
            .map_err(|source| SocketError::Path(TransportPathError::io(path, source)))?;
        validate_metadata(path, &metadata, ExpectedType::RegularFile, LOCK_FILE_MODE)
            .map_err(SocketError::Path)?;

        let identity = identity_from_fd(file.as_raw_fd())
            .map_err(|source| SocketError::Path(TransportPathError::io(path, source)))?;
        if identity != FileIdentity::from_metadata(&metadata) {
            return Err(SocketError::Kind {
                path: path.to_path_buf(),
                kind: SocketErrorKind::Replaced,
            });
        }
        let result = unsafe { libc::flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(EWOULDBLOCK) {
                return Err(SocketError::Kind {
                    path: path.to_path_buf(),
                    kind: SocketErrorKind::AlreadyRunning,
                });
            }
            return Err(SocketError::Path(TransportPathError::io(path, error)));
        }

        Ok(Self {
            file,
            path: path.to_path_buf(),
            identity,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn as_file(&self) -> &File {
        &self.file
    }
}

/// A listener that owns the server lock for its whole lifetime.
#[derive(Debug)]
pub struct SecureUnixListener {
    listener: UnixListener,
    lock: UnixLock,
    paths: TransportPaths,
    identity: FileIdentity,
}

impl SecureUnixListener {
    pub fn bind(paths: impl Borrow<TransportPaths>) -> Result<Self, SocketError> {
        let paths = paths.borrow().clone();
        paths.validate().map_err(SocketError::Path)?;
        let lock = UnixLock::acquire(paths.lock_path())?;
        prepare_socket_path(paths.socket_path())?;

        let listener = UnixListener::bind(paths.socket_path()).map_err(SocketError::Io)?;
        set_socket_mode(&listener, paths.socket_path(), SOCKET_MODE).map_err(|source| {
            SocketError::Path(TransportPathError::io(paths.socket_path(), source))
        })?;

        let metadata = fs::symlink_metadata(paths.socket_path()).map_err(|source| {
            SocketError::Path(TransportPathError::io(paths.socket_path(), source))
        })?;
        validate_metadata(
            paths.socket_path(),
            &metadata,
            ExpectedType::Socket,
            SOCKET_MODE,
        )
        .map_err(SocketError::Path)?;
        let identity = FileIdentity::from_metadata(&metadata);

        Ok(Self {
            listener,
            lock,
            paths,
            identity,
        })
    }

    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }

    pub fn listener_mut(&mut self) -> &mut UnixListener {
        &mut self.listener
    }

    /// Validate the bound socket path before a stream operation.
    pub fn validate_socket(&self) -> Result<(), SocketError> {
        let metadata = fs::symlink_metadata(self.paths.socket_path()).map_err(|source| {
            SocketError::Path(TransportPathError::io(self.paths.socket_path(), source))
        })?;
        validate_metadata(
            self.paths.socket_path(),
            &metadata,
            ExpectedType::Socket,
            SOCKET_MODE,
        )
        .map_err(SocketError::Path)?;
        if FileIdentity::from_metadata(&metadata) != self.identity {
            return Err(SocketError::Kind {
                path: self.paths.socket_path().to_path_buf(),
                kind: SocketErrorKind::Replaced,
            });
        }
        Ok(())
    }

    /// Accept one stream while validating the socket path on both sides.
    pub fn accept(&self) -> Result<(UnixStream, std::os::unix::net::SocketAddr), SocketError> {
        self.validate_socket()?;
        let accepted = self.listener.accept().map_err(SocketError::Io)?;
        self.validate_socket()?;
        Ok(accepted)
    }

    pub fn paths(&self) -> &TransportPaths {
        &self.paths
    }

    pub fn lock(&self) -> &UnixLock {
        &self.lock
    }

    pub fn socket_identity(&self) -> FileIdentity {
        self.identity
    }

    /// Remove this instance's socket only if the path still names the same inode.
    pub fn cleanup(&self) -> Result<bool, SocketError> {
        let metadata = match fs::symlink_metadata(self.paths.socket_path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(SocketError::Path(TransportPathError::io(
                    self.paths.socket_path(),
                    error,
                )));
            }
        };

        if metadata.file_type().is_symlink() {
            return Ok(false);
        }
        if FileIdentity::from_metadata(&metadata) != self.identity {
            return Ok(false);
        }
        validate_metadata(
            self.paths.socket_path(),
            &metadata,
            ExpectedType::Socket,
            SOCKET_MODE,
        )
        .map_err(SocketError::Path)?;
        fs::remove_file(self.paths.socket_path()).map_err(|source| {
            SocketError::Path(TransportPathError::io(self.paths.socket_path(), source))
        })?;
        Ok(true)
    }
}

/// Connect to an existing server after validating its path and permissions.
pub fn connect_unix(paths: impl Borrow<TransportPaths>) -> Result<UnixStream, SocketError> {
    let paths = paths.borrow();
    paths.validate().map_err(SocketError::Path)?;
    let before = fs::symlink_metadata(paths.socket_path())
        .map_err(|source| SocketError::Path(TransportPathError::io(paths.socket_path(), source)))?;
    validate_metadata(
        paths.socket_path(),
        &before,
        ExpectedType::Socket,
        SOCKET_MODE,
    )
    .map_err(SocketError::Path)?;
    let stream = UnixStream::connect(paths.socket_path()).map_err(SocketError::Io)?;
    let after = fs::symlink_metadata(paths.socket_path())
        .map_err(|source| SocketError::Path(TransportPathError::io(paths.socket_path(), source)))?;
    validate_metadata(
        paths.socket_path(),
        &after,
        ExpectedType::Socket,
        SOCKET_MODE,
    )
    .map_err(SocketError::Path)?;
    if FileIdentity::from_metadata(&before) != FileIdentity::from_metadata(&after) {
        return Err(SocketError::Kind {
            path: paths.socket_path().to_path_buf(),
            kind: SocketErrorKind::Replaced,
        });
    }
    Ok(stream)
}

impl Drop for SecureUnixListener {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn ensure_run_dir(path: &Path) -> Result<(), TransportPathError> {
    let mut created = false;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(TransportPathError::invalid(path, PathViolation::Symlink));
            }
            validate_metadata(path, &metadata, ExpectedType::Directory, RUN_DIR_MODE)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = DirBuilder::new();
            builder.mode(RUN_DIR_MODE);
            builder
                .create(path)
                .map_err(|source| TransportPathError::io(path, source))?;
            created = true;
        }
        Err(error) => return Err(TransportPathError::io(path, error)),
    }

    let fd = open_dir(path).map_err(|source| TransportPathError::io(path, source))?;
    if created {
        set_fd_mode(fd.as_raw_fd(), RUN_DIR_MODE)
            .map_err(|source| TransportPathError::io(path, source))?;
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|source| TransportPathError::io(path, source))?;
    if identity_from_fd(fd.as_raw_fd()).map_err(|source| TransportPathError::io(path, source))?
        != FileIdentity::from_metadata(&metadata)
    {
        return Err(TransportPathError::invalid(path, PathViolation::Replaced));
    }
    validate_metadata(path, &metadata, ExpectedType::Directory, RUN_DIR_MODE)
}

fn validate_existing_run_dir(path: &Path) -> Result<(), TransportPathError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(TransportPathError::Missing {
                path: path.to_path_buf(),
            });
        }
        Err(error) => return Err(TransportPathError::io(path, error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(TransportPathError::invalid(path, PathViolation::Symlink));
    }
    validate_metadata(path, &metadata, ExpectedType::Directory, RUN_DIR_MODE)
}

fn prepare_socket_path(path: &Path) -> Result<(), SocketError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SocketError::Path(TransportPathError::io(path, error))),
    };

    if metadata.file_type().is_symlink() {
        return Err(SocketError::Path(TransportPathError::invalid(
            path,
            PathViolation::Symlink,
        )));
    }
    validate_metadata(path, &metadata, ExpectedType::Socket, SOCKET_MODE)
        .map_err(SocketError::Path)?;
    let identity = FileIdentity::from_metadata(&metadata);

    match UnixStream::connect(path) {
        Ok(_) => Err(SocketError::Kind {
            path: path.to_path_buf(),
            kind: SocketErrorKind::Active,
        }),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ECONNREFUSED) | Some(ENOENT)
            ) =>
        {
            let latest = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => {
                    return Err(SocketError::Path(TransportPathError::io(path, error)));
                }
            };
            if latest.file_type().is_symlink() || FileIdentity::from_metadata(&latest) != identity {
                return Ok(());
            }
            validate_metadata(path, &latest, ExpectedType::Socket, SOCKET_MODE)
                .map_err(SocketError::Path)?;
            fs::remove_file(path)
                .map_err(|source| SocketError::Path(TransportPathError::io(path, source)))?;
            Ok(())
        }
        Err(error) => Err(SocketError::Io(error)),
    }
}

fn validate_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    expected_type: ExpectedType,
    expected_mode: u32,
) -> Result<(), TransportPathError> {
    let file_type = metadata.file_type();
    let type_matches = match expected_type {
        ExpectedType::Directory => file_type.is_dir(),
        ExpectedType::RegularFile => file_type.is_file(),
        ExpectedType::Socket => file_type.is_socket(),
    };
    if !type_matches {
        return Err(TransportPathError::invalid(
            path,
            PathViolation::WrongType {
                expected: expected_type,
            },
        ));
    }

    let expected_owner = current_euid();
    let actual_owner = metadata.uid();
    if actual_owner != expected_owner {
        return Err(TransportPathError::invalid(
            path,
            PathViolation::WrongOwner {
                expected: expected_owner,
                actual: actual_owner,
            },
        ));
    }

    let actual_mode = metadata.mode() & 0o7777;
    if actual_mode != expected_mode {
        return Err(TransportPathError::invalid(
            path,
            PathViolation::WrongMode {
                expected: expected_mode,
                actual: actual_mode,
            },
        ));
    }
    Ok(())
}

fn open_lock_file(path: &Path) -> Result<(File, bool), TransportPathError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .custom_flags(O_NOFOLLOW | O_CLOEXEC);
    match options.create_new(true).mode(LOCK_FILE_MODE).open(path) {
        Ok(file) => Ok((file, true)),
        Err(error) if error.raw_os_error() == Some(EEXIST) => {
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .custom_flags(O_NOFOLLOW | O_CLOEXEC);
            options
                .open(path)
                .map(|file| (file, false))
                .map_err(|source| {
                    if source.raw_os_error() == Some(ELOOP) {
                        TransportPathError::invalid(path, PathViolation::Symlink)
                    } else {
                        TransportPathError::io(path, source)
                    }
                })
        }
        Err(error) if error.raw_os_error() == Some(ELOOP) => {
            Err(TransportPathError::invalid(path, PathViolation::Symlink))
        }
        Err(error) => Err(TransportPathError::io(path, error)),
    }
}

fn open_dir(path: &Path) -> io::Result<File> {
    let fd = unsafe {
        libc::open(
            path_to_c_string(path)?.as_ptr(),
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

fn path_to_c_string(path: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "transport path contains an embedded NUL",
        )
    })
}

fn identity_from_fd(fd: RawFd) -> io::Result<FileIdentity> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let result = unsafe { libc::fstat(fd, &mut stat) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
    })
}

fn set_fd_mode(fd: RawFd, mode: u32) -> io::Result<()> {
    let result = unsafe { libc::fchmod(fd, mode as libc::mode_t) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn set_socket_mode(listener: &UnixListener, path: &Path, mode: u32) -> io::Result<()> {
    match set_fd_mode(listener.as_raw_fd(), mode) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(EINVAL) => {
            // macOS rejects fchmod on a Unix socket descriptor.  The path was
            // freshly bound by this listener; chmod it and verify the inode
            // remains the same before returning to the caller.
            let before = fs::symlink_metadata(path)?;
            if before.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "socket path was replaced by a symlink",
                ));
            }
            if !before.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "socket path was replaced by a non-socket",
                ));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            let after = fs::symlink_metadata(path)?;
            if !after.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "socket path was replaced by a non-socket",
                ));
            }
            if FileIdentity::from_metadata(&before) != FileIdentity::from_metadata(&after) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "socket path was replaced while setting permissions",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub fn current_euid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}
