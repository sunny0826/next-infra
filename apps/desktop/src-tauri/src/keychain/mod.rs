//! Secret replacement and resolution policy for the macOS Data Protection Keychain.
//!
//! Platform calls stay behind `KeychainBackend`; this module owns naming,
//! validation, replacement order, rollback, no-interaction reads, and safe
//! error classification.

use next_infra_core::{
    ConnectionId, SecretBackend, SecretKind, SecretProvider, SecretRef, SecretRefInput,
    SecretValue, Timestamp,
};

mod platform;

pub use platform::MacosDataProtectionKeychainBackend;

pub const SECRET_BACKEND: SecretBackend = SecretBackend::MacosDataProtectionKeychainV1;
const SERVICE_SUFFIX: &str = ".provider-secret.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeychainBackendError {
    Locked,
    Missing,
    SigningConfigurationInvalid,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretStoreError {
    Conflict,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretManagerError {
    InvalidReference,
    CredentialUnavailable,
    SigningConfigurationInvalid,
    VerificationFailed,
    StoreConflict,
    StoreUnavailable,
    Backend,
}

pub trait KeychainBackend {
    fn add(
        &mut self,
        access_group: &str,
        service: &str,
        account: &str,
        value: &[u8],
    ) -> Result<(), KeychainBackendError>;

    /// Read without allowing UI or authentication prompts.
    fn read_no_ui(
        &self,
        access_group: &str,
        service: &str,
        account: &str,
    ) -> Result<SecretValue, KeychainBackendError>;

    fn delete(
        &mut self,
        access_group: &str,
        service: &str,
        account: &str,
    ) -> Result<(), KeychainBackendError>;
}

pub trait SecretRefStore {
    fn active_ref(
        &self,
        connection_id: &ConnectionId,
    ) -> Result<Option<SecretRef>, SecretStoreError>;

    /// Atomically replace the active reference when `expected` still matches.
    fn switch_ref(
        &mut self,
        connection_id: &ConnectionId,
        expected: Option<&SecretRef>,
        replacement: &SecretRef,
    ) -> Result<(), SecretStoreError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceSecretRequest {
    pub connection_id: ConnectionId,
    pub secret_kind: SecretKind,
    pub generation_id: String,
    pub created_at: Timestamp,
    pub verified_at: Timestamp,
    pub permission_scope_summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceSecretOutcome {
    pub secret_ref: SecretRef,
    pub cleanup_pending: bool,
}

pub struct SecretManager<B, S> {
    backend: B,
    store: S,
    bundle_id: String,
    access_group: String,
}

impl<B, S> SecretManager<B, S>
where
    B: KeychainBackend,
    S: SecretRefStore,
{
    pub fn new(
        backend: B,
        store: S,
        bundle_id: impl Into<String>,
        access_group: impl Into<String>,
    ) -> Result<Self, SecretManagerError> {
        let bundle_id = bundle_id.into();
        let access_group = access_group.into();
        if !valid_bundle_id(&bundle_id) || !valid_access_group(&access_group, &bundle_id) {
            return Err(SecretManagerError::SigningConfigurationInvalid);
        }
        Ok(Self {
            backend,
            store,
            bundle_id,
            access_group,
        })
    }

    pub fn replace(
        &mut self,
        request: ReplaceSecretRequest,
        value: SecretValue,
    ) -> Result<ReplaceSecretOutcome, SecretManagerError> {
        let old_ref = self
            .store
            .active_ref(&request.connection_id)
            .map_err(map_store_error)?;
        let service = self.service();
        let account = account_name(
            &request.connection_id,
            request.secret_kind,
            &request.generation_id,
        )?;

        self.backend
            .add(&self.access_group, &service, &account, value.expose())
            .map_err(map_backend_error)?;

        let verification = self
            .backend
            .read_no_ui(&self.access_group, &service, &account);
        let verified = match verification {
            Ok(read_back) => read_back.expose() == value.expose(),
            Err(error) => {
                let _ = self.backend.delete(&self.access_group, &service, &account);
                return Err(map_backend_error(error));
            }
        };
        if !verified {
            let _ = self.backend.delete(&self.access_group, &service, &account);
            return Err(SecretManagerError::VerificationFailed);
        }

        let secret_ref = SecretRef::new(SecretRefInput {
            backend: SECRET_BACKEND,
            service: service.clone(),
            account: account.clone(),
            secret_kind: request.secret_kind,
            generation_id: request.generation_id,
            created_at: request.created_at,
            last_verified_at: request.verified_at,
            permission_scope_summary: request.permission_scope_summary,
        })
        .map_err(|_| SecretManagerError::InvalidReference)?;

        if let Err(error) =
            self.store
                .switch_ref(&request.connection_id, old_ref.as_ref(), &secret_ref)
        {
            let _ = self.backend.delete(&self.access_group, &service, &account);
            return Err(map_store_error(error));
        }

        let cleanup_pending = old_ref.as_ref().is_some_and(|old| {
            self.backend
                .delete(&self.access_group, &old.service, &old.account)
                .is_err()
        });

        Ok(ReplaceSecretOutcome {
            secret_ref,
            cleanup_pending,
        })
    }

    pub fn into_parts(self) -> (B, S) {
        (self.backend, self.store)
    }

    fn service(&self) -> String {
        format!("{}{SERVICE_SUFFIX}", self.bundle_id)
    }

    fn validate_ref(&self, secret_ref: &SecretRef) -> Result<(), SecretManagerError> {
        if secret_ref.backend != SECRET_BACKEND
            || secret_ref.service != self.service()
            || !secret_ref.account.starts_with("connection/")
            || !secret_ref
                .account
                .ends_with(&format!("/generation/{}", secret_ref.generation_id))
        {
            return Err(SecretManagerError::InvalidReference);
        }
        Ok(())
    }
}

impl<B, S> SecretProvider for SecretManager<B, S>
where
    B: KeychainBackend,
    S: SecretRefStore,
{
    type Error = SecretManagerError;

    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue, Self::Error> {
        self.validate_ref(secret_ref)?;
        self.backend
            .read_no_ui(&self.access_group, &secret_ref.service, &secret_ref.account)
            .map_err(map_backend_error)
    }
}

fn account_name(
    connection_id: &ConnectionId,
    secret_kind: SecretKind,
    generation_id: &str,
) -> Result<String, SecretManagerError> {
    if generation_id.is_empty()
        || !generation_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SecretManagerError::InvalidReference);
    }
    Ok(format!(
        "connection/{}/kind/{}/generation/{generation_id}",
        connection_id.as_str(),
        secret_kind_name(secret_kind)
    ))
}

fn secret_kind_name(kind: SecretKind) -> &'static str {
    match kind {
        SecretKind::ApiToken => "api-token",
        SecretKind::PersonalAccessToken => "personal-access-token",
        SecretKind::SshPrivateKey => "ssh-private-key",
        SecretKind::SshPassphrase => "ssh-passphrase",
        SecretKind::AccessKeyPair => "access-key-pair",
        SecretKind::DatabasePassword => "database-password",
    }
}

fn valid_bundle_id(value: &str) -> bool {
    value.contains('.')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn valid_access_group(value: &str, bundle_id: &str) -> bool {
    value.ends_with(bundle_id)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn map_backend_error(error: KeychainBackendError) -> SecretManagerError {
    match error {
        KeychainBackendError::Locked | KeychainBackendError::Missing => {
            SecretManagerError::CredentialUnavailable
        }
        KeychainBackendError::SigningConfigurationInvalid => {
            SecretManagerError::SigningConfigurationInvalid
        }
        KeychainBackendError::Other => SecretManagerError::Backend,
    }
}

fn map_store_error(error: SecretStoreError) -> SecretManagerError {
    match error {
        SecretStoreError::Conflict => SecretManagerError::StoreConflict,
        SecretStoreError::Unavailable => SecretManagerError::StoreUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeBackend {
        items: BTreeMap<(String, String, String), Vec<u8>>,
        events: Vec<&'static str>,
        fail_read: Option<KeychainBackendError>,
        fail_delete: bool,
        corrupt_read_back: bool,
    }

    impl KeychainBackend for FakeBackend {
        fn add(
            &mut self,
            access_group: &str,
            service: &str,
            account: &str,
            value: &[u8],
        ) -> Result<(), KeychainBackendError> {
            self.events.push("add");
            self.items.insert(
                (access_group.into(), service.into(), account.into()),
                value.to_vec(),
            );
            Ok(())
        }

        fn read_no_ui(
            &self,
            access_group: &str,
            service: &str,
            account: &str,
        ) -> Result<SecretValue, KeychainBackendError> {
            if let Some(error) = self.fail_read {
                return Err(error);
            }
            let mut value = self
                .items
                .get(&(access_group.into(), service.into(), account.into()))
                .cloned()
                .ok_or(KeychainBackendError::Missing)?;
            if self.corrupt_read_back {
                value.push(0);
            }
            Ok(SecretValue::new(value))
        }

        fn delete(
            &mut self,
            access_group: &str,
            service: &str,
            account: &str,
        ) -> Result<(), KeychainBackendError> {
            self.events.push("delete");
            if self.fail_delete {
                return Err(KeychainBackendError::Other);
            }
            self.items
                .remove(&(access_group.into(), service.into(), account.into()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeStore {
        active: Option<SecretRef>,
        events: Vec<&'static str>,
        fail: Option<SecretStoreError>,
    }

    impl SecretRefStore for FakeStore {
        fn active_ref(
            &self,
            _connection_id: &ConnectionId,
        ) -> Result<Option<SecretRef>, SecretStoreError> {
            Ok(self.active.clone())
        }

        fn switch_ref(
            &mut self,
            _connection_id: &ConnectionId,
            expected: Option<&SecretRef>,
            replacement: &SecretRef,
        ) -> Result<(), SecretStoreError> {
            self.events.push("switch");
            if let Some(error) = self.fail {
                return Err(error);
            }
            if self.active.as_ref() != expected {
                return Err(SecretStoreError::Conflict);
            }
            self.active = Some(replacement.clone());
            Ok(())
        }
    }

    fn manager(backend: FakeBackend, store: FakeStore) -> SecretManager<FakeBackend, FakeStore> {
        SecretManager::new(
            backend,
            store,
            "dev.example.next-infra",
            "team.dev.example.next-infra",
        )
        .unwrap()
    }

    fn request(generation_id: &str) -> ReplaceSecretRequest {
        ReplaceSecretRequest {
            connection_id: ConnectionId::new("fixture-connection").unwrap(),
            secret_kind: SecretKind::ApiToken,
            generation_id: generation_id.into(),
            created_at: Timestamp::from_unix_millis(1).unwrap(),
            verified_at: Timestamp::from_unix_millis(2).unwrap(),
            permission_scope_summary: "fixture read-only".into(),
        }
    }

    #[test]
    fn replace_adds_verifies_switches_then_deletes_old_generation() {
        let mut manager = manager(FakeBackend::default(), FakeStore::default());
        let first = manager
            .replace(
                request("generation-one"),
                SecretValue::new(b"first".to_vec()),
            )
            .unwrap();
        assert!(!first.cleanup_pending);

        let second = manager
            .replace(
                request("generation-two"),
                SecretValue::new(b"second".to_vec()),
            )
            .unwrap();
        assert!(!second.cleanup_pending);
        let (backend, store) = manager.into_parts();
        assert_eq!(store.active, Some(second.secret_ref));
        assert_eq!(store.events, ["switch", "switch"]);
        assert_eq!(backend.events, ["add", "add", "delete"]);
        assert_eq!(backend.items.len(), 1);
    }

    #[test]
    fn verification_failure_removes_new_item_and_keeps_old_reference() {
        let backend = FakeBackend {
            corrupt_read_back: true,
            ..FakeBackend::default()
        };
        let mut manager = manager(backend, FakeStore::default());
        assert_eq!(
            manager.replace(
                request("generation-one"),
                SecretValue::new(b"value".to_vec())
            ),
            Err(SecretManagerError::VerificationFailed)
        );
        let (backend, store) = manager.into_parts();
        assert!(backend.items.is_empty());
        assert!(store.active.is_none());
    }

    #[test]
    fn store_failure_rolls_back_new_item() {
        let store = FakeStore {
            fail: Some(SecretStoreError::Unavailable),
            ..FakeStore::default()
        };
        let mut manager = manager(FakeBackend::default(), store);
        assert_eq!(
            manager.replace(
                request("generation-one"),
                SecretValue::new(b"value".to_vec())
            ),
            Err(SecretManagerError::StoreUnavailable)
        );
        assert!(manager.into_parts().0.items.is_empty());
    }

    #[test]
    fn old_delete_failure_keeps_new_reference_and_reports_cleanup_pending() {
        let mut manager = manager(FakeBackend::default(), FakeStore::default());
        manager
            .replace(
                request("generation-one"),
                SecretValue::new(b"first".to_vec()),
            )
            .unwrap();
        manager.backend.fail_delete = true;
        let outcome = manager
            .replace(
                request("generation-two"),
                SecretValue::new(b"second".to_vec()),
            )
            .unwrap();
        assert!(outcome.cleanup_pending);
        assert_eq!(manager.store.active, Some(outcome.secret_ref));
    }

    #[test]
    fn locked_and_missing_reads_are_credential_unavailable_without_prompt_fallback() {
        for error in [KeychainBackendError::Locked, KeychainBackendError::Missing] {
            let backend = FakeBackend {
                fail_read: Some(error),
                ..FakeBackend::default()
            };
            let manager = manager(backend, FakeStore::default());
            let reference = SecretRef::new(SecretRefInput {
                backend: SECRET_BACKEND,
                service: "dev.example.next-infra.provider-secret.v1".into(),
                account: "connection/fixture-connection/kind/api-token/generation/generation-one"
                    .into(),
                secret_kind: SecretKind::ApiToken,
                generation_id: "generation-one".into(),
                created_at: Timestamp::from_unix_millis(1).unwrap(),
                last_verified_at: Timestamp::from_unix_millis(2).unwrap(),
                permission_scope_summary: "fixture read-only".into(),
            })
            .unwrap();
            assert!(matches!(
                manager.resolve(&reference),
                Err(SecretManagerError::CredentialUnavailable)
            ));
        }
    }

    #[test]
    fn references_from_another_bundle_are_rejected_before_backend_access() {
        let manager = manager(FakeBackend::default(), FakeStore::default());
        let reference = SecretRef::new(SecretRefInput {
            backend: SECRET_BACKEND,
            service: "other.example.app.provider-secret.v1".into(),
            account: "connection/fixture-connection/kind/api-token/generation/generation-one"
                .into(),
            secret_kind: SecretKind::ApiToken,
            generation_id: "generation-one".into(),
            created_at: Timestamp::from_unix_millis(1).unwrap(),
            last_verified_at: Timestamp::from_unix_millis(2).unwrap(),
            permission_scope_summary: "fixture read-only".into(),
        })
        .unwrap();
        assert!(matches!(
            manager.resolve(&reference),
            Err(SecretManagerError::InvalidReference)
        ));
    }
}
