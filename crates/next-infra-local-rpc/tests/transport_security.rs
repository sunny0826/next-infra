#![cfg(unix)]

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};

use next_infra_local_rpc::transport::{
    LOCK_FILE_MODE, RUN_DIR_MODE, SOCKET_MODE, SecureUnixListener, TransportPaths, connect_secure,
    connect_unix, current_euid, read_frame, read_json_frame, verify_peer_uid, verify_peer_uid_with,
    write_frame,
};
use serde_json::{Value, json};
use tempfile::tempdir;

fn mode(path: &std::path::Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

fn assert_owned_mode(path: &std::path::Path, expected_mode: u32) {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert_eq!(metadata.uid(), current_euid());
    assert_eq!(metadata.permissions().mode() & 0o7777, expected_mode);
}

#[test]
fn paths_lock_and_socket_have_owner_only_permissions() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();
    assert_owned_mode(paths.run_dir(), RUN_DIR_MODE);

    let server = SecureUnixListener::bind(paths.clone()).unwrap();
    assert_owned_mode(paths.lock_path(), LOCK_FILE_MODE);
    assert_owned_mode(paths.socket_path(), SOCKET_MODE);
    assert!(server.lock().identity().inode != 0);

    let second = SecureUnixListener::bind(paths.clone()).unwrap_err();
    assert!(second.is_already_running());
    assert!(paths.socket_path().exists());
    drop(server);
    assert!(!paths.socket_path().exists());
    assert!(paths.lock_path().exists());
}

#[test]
fn client_existing_paths_never_create_missing_run_directory() {
    let root = tempdir().unwrap();
    let run_dir = root.path().join("run");
    assert!(TransportPaths::from_existing_root(root.path()).is_err());
    assert!(!run_dir.exists());

    let provisioned = TransportPaths::from_root(root.path()).unwrap();
    assert_eq!(
        TransportPaths::from_existing_root(root.path()).unwrap(),
        provisioned
    );
}

#[test]
fn stale_socket_is_removed_only_after_refused_connect() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();

    let stale = UnixListener::bind(paths.socket_path()).unwrap();
    fs::set_permissions(paths.socket_path(), fs::Permissions::from_mode(SOCKET_MODE)).unwrap();
    drop(stale);
    assert!(paths.socket_path().exists());

    let server = SecureUnixListener::bind(paths.clone()).unwrap();
    assert!(paths.socket_path().exists());
    drop(server);
    assert!(!paths.socket_path().exists());
}

#[test]
fn active_socket_and_replaced_socket_are_preserved() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();
    let server = SecureUnixListener::bind(paths.clone()).unwrap();
    assert!(SecureUnixListener::bind(paths.clone()).is_err());
    assert!(paths.socket_path().exists());

    let original_identity = server.socket_identity();
    fs::remove_file(paths.socket_path()).unwrap();
    let replacement = UnixListener::bind(paths.socket_path()).unwrap();
    fs::set_permissions(paths.socket_path(), fs::Permissions::from_mode(SOCKET_MODE)).unwrap();
    let replacement_identity = fs::symlink_metadata(paths.socket_path()).unwrap();
    assert_ne!(
        original_identity.inode,
        replacement_identity.ino(),
        "replacement must have a distinct inode"
    );

    assert!(!server.cleanup().unwrap());
    assert!(paths.socket_path().exists());
    drop(replacement);
    drop(server);
}

#[test]
fn symlink_regular_file_and_wrong_mode_fail_closed() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();

    let target = root.path().join("target.sock");
    let target_listener = UnixListener::bind(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(SOCKET_MODE)).unwrap();
    std::os::unix::fs::symlink(&target, paths.socket_path()).unwrap();
    assert!(SecureUnixListener::bind(paths.clone()).is_err());
    assert!(
        fs::symlink_metadata(paths.socket_path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    drop(target_listener);
    fs::remove_file(paths.socket_path()).unwrap();

    fs::write(paths.socket_path(), b"not a socket").unwrap();
    assert!(SecureUnixListener::bind(paths.clone()).is_err());
    assert_eq!(fs::read(paths.socket_path()).unwrap(), b"not a socket");
    fs::remove_file(paths.socket_path()).unwrap();

    let wrong_mode = UnixListener::bind(paths.socket_path()).unwrap();
    fs::set_permissions(paths.socket_path(), fs::Permissions::from_mode(0o666)).unwrap();
    drop(wrong_mode);
    assert_eq!(mode(paths.socket_path()), 0o666);
    assert!(SecureUnixListener::bind(paths.clone()).is_err());
    assert_eq!(mode(paths.socket_path()), 0o666);
}

#[test]
fn fragmented_streams_are_bounded_and_strict() {
    let frame = next_infra_local_rpc::protocol::encode_frame(&json!({"hello":"world"})).unwrap();
    let mut reader = FragmentedReader {
        bytes: frame,
        offset: 0,
    };
    let decoded: Value = read_json_frame(&mut reader).unwrap();
    assert_eq!(decoded, json!({"hello":"world"}));

    let mut oversized = FragmentedReader {
        bytes: vec![0, 0x10, 0, 1],
        offset: 0,
    };
    let error = read_frame(&mut oversized).unwrap_err();
    assert!(error.is_oversized());
    assert_eq!(oversized.offset, 4, "body must not be allocated/read");

    let mut eof_header = FragmentedReader {
        bytes: vec![0, 0],
        offset: 0,
    };
    assert!(read_frame(&mut eof_header).unwrap_err().is_eof());

    let mut eof_body = FragmentedReader {
        bytes: vec![0, 0, 0, 2, b'{'],
        offset: 0,
    };
    assert!(read_frame(&mut eof_body).unwrap_err().is_eof());

    let mut invalid = FragmentedReader {
        bytes: vec![0, 0, 0, 1, b'{'],
        offset: 0,
    };
    assert!(
        read_json_frame::<_, Value>(&mut invalid)
            .unwrap_err()
            .is_invalid_frame()
    );

    let mut output = Vec::new();
    write_frame(&mut output, &json!({"ok":true})).unwrap();
    assert_eq!(&output[..4], &[0, 0, 0, 11]);
}

#[test]
fn peer_uid_can_be_verified_and_rejected_with_injected_lookup() {
    let (left, _right) = UnixStream::pair().unwrap();
    verify_peer_uid(&left).unwrap();
    let rejection = verify_peer_uid_with(&left, |_| Ok(current_euid().saturating_add(1)));
    assert!(rejection.unwrap_err().is_mismatch());
}

#[test]
fn client_connect_revalidates_owner_only_socket_path() {
    let root = tempdir().unwrap();
    let paths = TransportPaths::from_root(root.path()).unwrap();
    let server = SecureUnixListener::bind(paths.clone()).unwrap();

    let stream = connect_unix(&paths).unwrap();
    verify_peer_uid(&stream).unwrap();
    drop(stream);

    drop(connect_secure(&paths).unwrap());

    fs::set_permissions(paths.socket_path(), fs::Permissions::from_mode(0o666)).unwrap();
    assert!(connect_unix(&paths).is_err());
    assert_eq!(mode(paths.socket_path()), 0o666);
    drop(server);
}

struct FragmentedReader {
    bytes: Vec<u8>,
    offset: usize,
}

impl Read for FragmentedReader {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        if self.offset == self.bytes.len() {
            return Ok(0);
        }
        let count = target.len().min(1).min(self.bytes.len() - self.offset);
        target[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}
