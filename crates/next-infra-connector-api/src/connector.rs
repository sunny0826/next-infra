use crate::{
    ConnectorDescriptor, ConnectorResult, SyncOutcome, SyncRequest, ValidationReport,
    ValidationRequest,
};
use async_trait::async_trait;
use next_infra_core::SecretValue;

#[async_trait]
pub trait ReadConnector: Send + Sync {
    fn descriptor(&self) -> &ConnectorDescriptor;

    async fn validate(
        &self,
        request: ValidationRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<ValidationReport>;

    async fn sync(
        &self,
        request: SyncRequest,
        secret: Option<&SecretValue>,
    ) -> ConnectorResult<SyncOutcome>;
}
