use next_infra_connector_api::ConnectorFailure;
use next_infra_core::ErrorCode;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct GitHubError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

impl GitHubError {
    pub(crate) fn authentication(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::AuthenticationFailed, message, false, None)
    }

    pub(crate) fn invalid_response(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidResponse, message, false, None)
    }

    pub(crate) fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NetworkUnreachable, message, true, None)
    }

    pub(crate) fn new(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            retry_after_ms,
        }
    }
}

impl fmt::Debug for GitHubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retryable", &self.retryable)
            .field("retry_after_ms", &self.retry_after_ms)
            .finish()
    }
}

impl fmt::Display for GitHubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GitHubError {}

impl From<GitHubError> for ConnectorFailure {
    fn from(error: GitHubError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
            retry_after_ms: error.retry_after_ms,
        }
    }
}
