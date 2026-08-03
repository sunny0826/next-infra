use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidDomainValue,
    NotFound,
    Conflict,
    AuthenticationFailed,
    CredentialUnavailable,
    PermissionDenied,
    RateLimited,
    NetworkUnreachable,
    HostKeyMismatch,
    ProviderUnavailable,
    InvalidResponse,
    SchemaIncompatible,
    PartialPagination,
    Cancelled,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl DomainError {
    pub fn invalid_value(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidDomainValue,
            message: message.into(),
            retryable: false,
        }
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DomainError {}
