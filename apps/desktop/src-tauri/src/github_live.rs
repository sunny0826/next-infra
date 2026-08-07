use next_infra_core::{ConnectionId, SecretValue};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const SECRET_DIRECTORY: &str = "github-secrets-v1";

#[derive(Debug)]
pub struct GitHubSecretFileError;

#[derive(Clone)]
pub struct GitHubSecretFiles {
    directory: PathBuf,
}

impl GitHubSecretFiles {
    pub fn open(data_directory: &Path) -> Result<Self, GitHubSecretFileError> {
        let directory = data_directory.join(SECRET_DIRECTORY);
        ensure_secret_directory(&directory)?;
        Ok(Self { directory })
    }

    pub fn replace(
        &self,
        connection_id: &ConnectionId,
        secret: &SecretValue,
    ) -> Result<(), GitHubSecretFileError> {
        if secret.expose().is_empty() {
            return Err(GitHubSecretFileError);
        }
        let target = self.path_for(connection_id);
        let temporary = self.directory.join(format!(
            ".{}-{}.tmp",
            connection_id.as_str(),
            uuid::Uuid::new_v4()
        ));
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)
                .map_err(|_| GitHubSecretFileError)?;
            file.write_all(secret.expose())
                .map_err(|_| GitHubSecretFileError)?;
            file.sync_all().map_err(|_| GitHubSecretFileError)?;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                .map_err(|_| GitHubSecretFileError)?;
            fs::rename(&temporary, &target).map_err(|_| GitHubSecretFileError)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    pub fn read(&self, connection_id: &ConnectionId) -> Result<SecretValue, GitHubSecretFileError> {
        let path = self.path_for(connection_id);
        let metadata = fs::symlink_metadata(&path).map_err(|_| GitHubSecretFileError)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() as u32 }
            || metadata.permissions().mode() & 0o7777 != 0o600
        {
            return Err(GitHubSecretFileError);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| GitHubSecretFileError)?;
        let mut value = Vec::new();
        file.read_to_end(&mut value)
            .map_err(|_| GitHubSecretFileError)?;
        if value.is_empty() || value.len() > 16 * 1024 {
            return Err(GitHubSecretFileError);
        }
        Ok(SecretValue::new(value))
    }

    pub fn remove(&self, connection_id: &ConnectionId) -> Result<(), GitHubSecretFileError> {
        let path = self.path_for(connection_id);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || metadata.uid() != unsafe { libc::geteuid() as u32 }
                    || metadata.permissions().mode() & 0o7777 != 0o600
                {
                    return Err(GitHubSecretFileError);
                }
                fs::remove_file(path).map_err(|_| GitHubSecretFileError)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(GitHubSecretFileError),
        }
    }

    fn path_for(&self, connection_id: &ConnectionId) -> PathBuf {
        self.directory
            .join(format!("{}.token", connection_id.as_str()))
    }
}

fn ensure_secret_directory(path: &Path) -> Result<(), GitHubSecretFileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || metadata.uid() != unsafe { libc::geteuid() as u32 }
                || metadata.permissions().mode() & 0o7777 != 0o700
            {
                return Err(GitHubSecretFileError);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| GitHubSecretFileError)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| GitHubSecretFileError)?;
        }
        Err(_) => return Err(GitHubSecretFileError),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_a_token_in_an_owner_only_regular_file() {
        let directory = tempfile::TempDir::new().unwrap();
        let files = GitHubSecretFiles::open(directory.path()).unwrap();
        let connection = ConnectionId::new("github-fixture").unwrap();
        files
            .replace(&connection, &SecretValue::new("fixture-token"))
            .unwrap();
        assert_eq!(files.read(&connection).unwrap().expose(), b"fixture-token");
        let metadata = fs::metadata(files.path_for(&connection)).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    }

    #[test]
    fn rejects_a_secret_file_with_broader_permissions() {
        let directory = tempfile::TempDir::new().unwrap();
        let files = GitHubSecretFiles::open(directory.path()).unwrap();
        let connection = ConnectionId::new("github-fixture").unwrap();
        files
            .replace(&connection, &SecretValue::new("fixture-token"))
            .unwrap();
        fs::set_permissions(
            files.path_for(&connection),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(files.read(&connection).is_err());
    }

    #[test]
    fn removes_only_a_valid_owner_only_secret_file() {
        let directory = tempfile::TempDir::new().unwrap();
        let files = GitHubSecretFiles::open(directory.path()).unwrap();
        let connection = ConnectionId::new("github-fixture").unwrap();
        files
            .replace(&connection, &SecretValue::new("fixture-token"))
            .unwrap();

        files.remove(&connection).unwrap();

        assert!(!files.path_for(&connection).exists());
    }
}
