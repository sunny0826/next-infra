//! SQLite storage adapter boundary for Next Infra.

mod migrations;
mod projection;
mod query_projection;

pub use query_projection::*;

use rusqlite::Connection;
use std::{fmt, fs, path::Path, time::Duration};

pub const STORE_SCHEMA_VERSION: u32 = migrations::LATEST_SCHEMA_VERSION;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Store {
    pub(crate) connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        prepare_path(path)?;
        let mut connection = Connection::open(path).map_err(StoreError::Sqlite)?;
        secure_database_file(path)?;
        configure_connection(&connection)?;
        verify_integrity(&connection)?;
        migrations::apply(&mut connection)?;
        verify_integrity(&connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<u32, StoreError> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(StoreError::Sqlite)
    }

    pub fn journal_mode(&self) -> Result<String, StoreError> {
        self.connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(StoreError::Sqlite)
    }

    pub fn foreign_keys_enabled(&self) -> Result<bool, StoreError> {
        self.connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .map_err(StoreError::Sqlite)
    }

    pub fn busy_timeout_ms(&self) -> Result<i64, StoreError> {
        self.connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .map_err(StoreError::Sqlite)
    }

    pub fn integrity_check(&self) -> Result<(), StoreError> {
        verify_integrity(&self.connection)
    }

    /// Checkpoint every committed WAL frame and truncate the WAL file.
    ///
    /// Runtime calls this only after the single writer queue has drained.
    pub fn checkpoint_wal(&self) -> Result<(), StoreError> {
        let (busy, _log_frames, _checkpointed_frames): (u32, u32, u32) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(StoreError::Sqlite)?;
        if busy == 0 {
            Ok(())
        } else {
            Err(StoreError::Contract(
                "WAL checkpoint remained busy after writer drain".into(),
            ))
        }
    }

    pub fn projection_metadata(&self) -> Result<ProjectionMetadata, StoreError> {
        query_projection::read_projection_metadata(&self.connection)
    }
}

fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(StoreError::Sqlite)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(StoreError::Sqlite)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(StoreError::Sqlite)?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(StoreError::Sqlite)?;
    Ok(())
}

fn verify_integrity(connection: &Connection) -> Result<(), StoreError> {
    let result: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(StoreError::Sqlite)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(StoreError::Integrity(result))
    }
}

fn prepare_path(path: &Path) -> Result<(), StoreError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(StoreError::UnsafePath(
            "database path cannot be a symbolic link".into(),
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        StoreError::UnsafePath("database path must have a parent directory".into())
    })?;
    fs::create_dir_all(parent).map_err(StoreError::Io)?;
    secure_directory(parent)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(StoreError::Io)
}

#[cfg(not(unix))]
fn secure_directory(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

#[cfg(unix)]
fn secure_database_file(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(StoreError::Io)
}

#[cfg(not(unix))]
fn secure_database_file(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Integrity(String),
    UnsupportedSchema { found: u32, supported: u32 },
    UnsafePath(String),
    Contract(String),
    Json(serde_json::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store I/O error: {error}"),
            Self::Sqlite(error) => write!(formatter, "store SQLite error: {error}"),
            Self::Integrity(result) => write!(formatter, "store integrity check failed: {result}"),
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "store schema version {found} is newer than supported version {supported}"
            ),
            Self::UnsafePath(message) => write!(formatter, "unsafe store path: {message}"),
            Self::Contract(message) => write!(formatter, "store contract error: {message}"),
            Self::Json(error) => write!(formatter, "store JSON error: {error}"),
        }
    }
}

impl std::error::Error for StoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn path(directory: &TempDir) -> std::path::PathBuf {
        directory.path().join("data").join("next-infra.db")
    }

    #[test]
    fn migrations_open_with_required_pragmas_and_schema() {
        let directory = TempDir::new().unwrap();
        let store = Store::open(&path(&directory)).unwrap();

        assert_eq!(store.schema_version().unwrap(), STORE_SCHEMA_VERSION);
        assert_eq!(store.journal_mode().unwrap(), "wal");
        assert!(store.foreign_keys_enabled().unwrap());
        assert_eq!(store.busy_timeout_ms().unwrap(), 5_000);
        store.integrity_check().unwrap();
        store.checkpoint_wal().unwrap();
    }

    #[test]
    fn migrations_are_idempotent_across_reopen() {
        let directory = TempDir::new().unwrap();
        let database = path(&directory);

        drop(Store::open(&database).unwrap());
        let reopened = Store::open(&database).unwrap();
        assert_eq!(reopened.schema_version().unwrap(), STORE_SCHEMA_VERSION);
        let migration_count: u32 = reopened
            .connection
            .query_row("SELECT count(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(migration_count, STORE_SCHEMA_VERSION);
    }

    #[test]
    fn migrations_refuse_newer_schema() {
        let directory = TempDir::new().unwrap();
        let database = path(&directory);
        let store = Store::open(&database).unwrap();
        store
            .connection
            .pragma_update(None, "user_version", STORE_SCHEMA_VERSION + 1)
            .unwrap();
        drop(store);

        assert!(matches!(
            Store::open(&database),
            Err(StoreError::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn migrations_refuse_corrupt_database() {
        let directory = TempDir::new().unwrap();
        let database = path(&directory);
        fs::create_dir_all(database.parent().unwrap()).unwrap();
        fs::write(&database, b"not a sqlite database").unwrap();

        assert!(Store::open(&database).is_err());
    }

    #[test]
    fn migrations_do_not_depend_on_fts() {
        let directory = TempDir::new().unwrap();
        let store = Store::open(&path(&directory)).unwrap();
        let virtual_tables: u32 = store
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE sql LIKE '%VIRTUAL TABLE%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(virtual_tables, 0);
    }

    #[cfg(unix)]
    #[test]
    fn migrations_secure_database_and_parent_for_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().unwrap();
        let database = path(&directory);
        drop(Store::open(&database).unwrap());

        assert_eq!(
            fs::metadata(database.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(database).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
