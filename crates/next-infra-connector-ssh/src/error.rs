use next_infra_connector_api::ConnectorFailure;
use next_infra_core::ErrorCode;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct SshError {
    pub code: ErrorCode,
    pub message: &'static str,
    pub retryable: bool,
}

impl SshError {
    pub(crate) const fn invalid_config() -> Self {
        Self::new(
            ErrorCode::InvalidDomainValue,
            "SSH configuration is invalid",
            false,
        )
    }

    pub(crate) const fn internal() -> Self {
        Self::new(
            ErrorCode::Internal,
            "system OpenSSH could not be started",
            false,
        )
    }

    pub(crate) const fn host_key_mismatch() -> Self {
        Self::new(
            ErrorCode::HostKeyMismatch,
            "SSH host key verification failed",
            false,
        )
    }

    pub(crate) const fn authentication() -> Self {
        Self::new(
            ErrorCode::AuthenticationFailed,
            "SSH authentication failed",
            false,
        )
    }

    pub(crate) const fn network() -> Self {
        Self::new(
            ErrorCode::NetworkUnreachable,
            "SSH host is unreachable",
            true,
        )
    }

    pub(crate) const fn timeout() -> Self {
        Self::new(ErrorCode::ProviderUnavailable, "SSH probe timed out", true)
    }

    pub(crate) const fn output_limit() -> Self {
        Self::new(
            ErrorCode::InvalidResponse,
            "SSH probe output exceeded its limit",
            false,
        )
    }

    pub(crate) const fn remote_failure() -> Self {
        Self::new(ErrorCode::ProviderUnavailable, "SSH probe failed", false)
    }

    pub(crate) const fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "SSH probe was cancelled", false)
    }

    pub(crate) const fn new(code: ErrorCode, message: &'static str, retryable: bool) -> Self {
        Self {
            code,
            message,
            retryable,
        }
    }
}

impl fmt::Debug for SshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SshError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retryable", &self.retryable)
            .finish()
    }
}

impl fmt::Display for SshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for SshError {}

impl From<SshError> for ConnectorFailure {
    fn from(error: SshError) -> Self {
        Self {
            code: error.code,
            message: error.message.to_owned(),
            retryable: error.retryable,
            retry_after_ms: None,
        }
    }
}
