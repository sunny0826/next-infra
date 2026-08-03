use crate::{DomainError, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackend {
    MacosDataProtectionKeychainV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    ApiToken,
    PersonalAccessToken,
    SshPrivateKey,
    SshPassphrase,
    AccessKeyPair,
    DatabasePassword,
}

/// Serializable, non-secret metadata pointing at one Keychain generation.
///
/// This type intentionally contains no persistent Keychain reference, secret
/// fragment, digest, profile, or signing credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    pub backend: SecretBackend,
    pub service: String,
    pub account: String,
    pub secret_kind: SecretKind,
    pub generation_id: String,
    pub created_at: Timestamp,
    pub last_verified_at: Timestamp,
    pub permission_scope_summary: String,
}

pub struct SecretRefInput {
    pub backend: SecretBackend,
    pub service: String,
    pub account: String,
    pub secret_kind: SecretKind,
    pub generation_id: String,
    pub created_at: Timestamp,
    pub last_verified_at: Timestamp,
    pub permission_scope_summary: String,
}

impl SecretRef {
    pub fn new(input: SecretRefInput) -> Result<Self, DomainError> {
        let SecretRefInput {
            backend,
            service,
            account,
            secret_kind,
            generation_id,
            created_at,
            last_verified_at,
            permission_scope_summary,
        } = input;
        validate_service(&service)?;
        validate_account(&account)?;
        validate_generation(&generation_id)?;
        validate_summary(&permission_scope_summary)?;
        if last_verified_at < created_at {
            return Err(DomainError::invalid_value(
                "SecretRef verification time precedes creation",
            ));
        }
        Ok(Self {
            backend,
            service,
            account,
            secret_kind,
            generation_id,
            created_at,
            last_verified_at,
            permission_scope_summary,
        })
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretRef")
            .field("backend", &self.backend)
            .field("service", &"[REDACTED]")
            .field("account", &"[REDACTED]")
            .field("secret_kind", &self.secret_kind)
            .field("generation_id", &self.generation_id)
            .field("created_at", &self.created_at)
            .field("last_verified_at", &self.last_verified_at)
            .field("permission_scope_summary", &"[REDACTED]")
            .finish()
    }
}

fn validate_service(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 255
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err(DomainError::invalid_value(
            "SecretRef service must be a lowercase bundle-derived identifier",
        ));
    }
    Ok(())
}

fn validate_account(value: &str) -> Result<(), DomainError> {
    if value.len() > 512
        || !value.starts_with("connection/")
        || !value.contains("/kind/")
        || !value.contains("/generation/")
        || value.chars().any(char::is_control)
    {
        return Err(DomainError::invalid_value(
            "SecretRef account does not match the Keychain naming contract",
        ));
    }
    Ok(())
}

fn validate_generation(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(DomainError::invalid_value(
            "SecretRef generation must be a lowercase identifier",
        ));
    }
    Ok(())
}

fn validate_summary(value: &str) -> Result<(), DomainError> {
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(DomainError::invalid_value(
            "SecretRef permission summary is invalid",
        ));
    }
    Ok(())
}
