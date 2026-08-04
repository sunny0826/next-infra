use std::sync::Arc;

use next_infra_query::service::{QueryService, QuerySource};

use crate::protocol::{QueryRequest, QueryResponse, RpcError};

/// The closed read-only dispatch boundary consumed by an RPC session.
pub trait QueryHandler: Send + Sync + 'static {
    fn handle(&self, query: QueryRequest) -> Result<QueryResponse, RpcError>;
}

impl<T> QueryHandler for Arc<T>
where
    T: QueryHandler,
{
    fn handle(&self, query: QueryRequest) -> Result<QueryResponse, RpcError> {
        self.as_ref().handle(query)
    }
}

/// Adapter that preserves the frozen QueryService semantics.
pub struct QueryServiceHandler<S> {
    service: QueryService<S>,
}

impl<S> QueryServiceHandler<S> {
    pub fn new(service: QueryService<S>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &QueryService<S> {
        &self.service
    }
}

impl<S> QueryHandler for QueryServiceHandler<S>
where
    S: QuerySource + Send + Sync + 'static,
{
    fn handle(&self, query: QueryRequest) -> Result<QueryResponse, RpcError> {
        let result = match query {
            QueryRequest::SearchResources(query) => self
                .service
                .search_resources(query.into())
                .map(QueryResponse::SearchResources),
            QueryRequest::GetResource(query) => self
                .service
                .get_resource(query.into())
                .map(QueryResponse::GetResource),
            QueryRequest::GetTopology(query) => self
                .service
                .get_topology(query.into())
                .map(QueryResponse::GetTopology),
            QueryRequest::GetHealthSummary => self
                .service
                .get_health_summary()
                .map(QueryResponse::GetHealthSummary),
            QueryRequest::GetRecentChanges(query) => self
                .service
                .get_recent_changes(query.into())
                .map(QueryResponse::GetRecentChanges),
            QueryRequest::GetSyncStatus(query) => self
                .service
                .get_sync_status(query.into())
                .map(QueryResponse::GetSyncStatus),
            QueryRequest::ListConnectorCoverage => self
                .service
                .list_connector_coverage()
                .map(QueryResponse::ListConnectorCoverage),
        };

        result.map_err(|error| RpcError::query_failed(error.message, error.retryable))
    }
}
