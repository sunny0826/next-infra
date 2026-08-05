use next_infra_connector_api::{
    ConnectorFailure, ObservationWarning, RelationObservation, ResourceObservation,
};
use next_infra_core::{
    ErrorCode, ExternalId, LabelKey, ResourceHealth, ResourceKind, SchemaVersion, Scope, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const MAX_REPOSITORIES_PER_BATCH: usize = 2_000;

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct RepositoryDto {
    pub id: u64,
    pub name: String,
    pub owner: RepositoryOwnerDto,
    pub visibility: String,
    pub default_branch: Option<String>,
    pub archived: bool,
    pub disabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct RepositoryOwnerDto {
    pub login: String,
}

impl fmt::Debug for RepositoryDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryDto")
            .field("id", &self.id)
            .field("name", &"[REDACTED]")
            .field("owner", &"[REDACTED]")
            .field("visibility", &self.visibility)
            .field("archived", &self.archived)
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl fmt::Debug for RepositoryOwnerDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryOwnerDto")
            .field("login", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RepositoryRouteContext {
    repository_id: u64,
    repository_external_id: ExternalId,
    owner: String,
    name: String,
    scope: Scope,
    observed_at: Timestamp,
}

impl RepositoryRouteContext {
    pub fn repository_id(&self) -> u64 {
        self.repository_id
    }
    pub fn repository_external_id(&self) -> &ExternalId {
        &self.repository_external_id
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }
}

impl fmt::Debug for RepositoryRouteContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepositoryRouteContext")
            .field("repository_id", &self.repository_id)
            .field("repository_external_id", &self.repository_external_id)
            .field("owner", &"[REDACTED]")
            .field("name", &"[REDACTED]")
            .field("scope", &self.scope)
            .field("observed_at", &self.observed_at)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryModuleState {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryModuleResult {
    pub module: &'static str,
    pub collected: usize,
    pub bounded: bool,
    pub state: RepositoryModuleState,
    pub failure: Option<ConnectorFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub modules: Vec<RepositoryModuleResult>,
    pub warnings: Vec<ObservationWarning>,
    pub routes: Vec<RepositoryRouteContext>,
}

impl RepositoryMapperOutput {
    pub fn merge(mut self, mut other: Self) -> Self {
        self.resources.append(&mut other.resources);
        self.relations.append(&mut other.relations);
        self.modules.append(&mut other.modules);
        self.warnings.append(&mut other.warnings);
        self.routes.append(&mut other.routes);
        sort_output(&mut self);
        self
    }
}

pub fn find_targeted_route<'a>(
    routes: &'a [RepositoryRouteContext],
    repository_external_id: &ExternalId,
) -> Result<&'a RepositoryRouteContext, ConnectorFailure> {
    routes
        .iter()
        .find(|route| route.repository_external_id() == repository_external_id)
        .ok_or_else(|| invalid("GitHub targeted repository route context is unavailable"))
}

pub fn map_repositories(
    scope: &Scope,
    observed_at: Timestamp,
    repositories: impl IntoIterator<Item = RepositoryDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<RepositoryMapperOutput, ConnectorFailure> {
    let mut repositories = repositories.into_iter().collect::<Vec<_>>();
    let bounded = bounded || repositories.len() > MAX_REPOSITORIES_PER_BATCH;
    repositories.truncate(MAX_REPOSITORIES_PER_BATCH);
    let mut resources = Vec::new();
    let mut routes = Vec::new();
    let mut identities = BTreeSet::new();

    for repository in repositories {
        validate_required(&repository.name, "repository name")?;
        validate_required(&repository.owner.login, "repository owner")?;
        validate_required(&repository.visibility, "repository visibility")?;
        validate_optional(
            repository.default_branch.as_deref(),
            "repository default branch",
        )?;
        validate_required(&repository.created_at, "repository created_at")?;
        validate_required(&repository.updated_at, "repository updated_at")?;
        if !matches!(
            repository.visibility.as_str(),
            "public" | "private" | "internal"
        ) {
            return Err(invalid("GitHub repository visibility is unsupported"));
        }

        let external_id = provider_external_id("github-repository", repository.id)?;
        if !identities.insert(external_id.clone()) {
            return Err(invalid("GitHub repositories contain duplicate identities"));
        }
        resources.push(ResourceObservation {
            kind: resource_kind("github.repository")?,
            external_id: external_id.clone(),
            name: repository.name.clone(),
            display_name: repository.name.clone(),
            scope: scope.clone(),
            labels: repository_labels(&repository.visibility)?,
            health: ResourceHealth::Unknown,
            attributes: json!({
                "repository_id": repository.id,
                "visibility": repository.visibility,
                "default_branch": repository.default_branch,
                "archived": repository.archived,
                "disabled": repository.disabled,
                "created_at": repository.created_at,
                "updated_at": repository.updated_at,
            }),
            attribute_schema_version: schema_version()?,
            observed_at,
        });
        routes.push(RepositoryRouteContext {
            repository_id: repository.id,
            repository_external_id: external_id,
            owner: repository.owner.login,
            name: repository.name,
            scope: scope.clone(),
            observed_at,
        });
    }

    Ok(module_output(
        "github.repositories",
        resources,
        Vec::new(),
        routes,
        bounded,
        failure,
    ))
}

pub(crate) fn module_output(
    module: &'static str,
    resources: Vec<ResourceObservation>,
    relations: Vec<RelationObservation>,
    routes: Vec<RepositoryRouteContext>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> RepositoryMapperOutput {
    let collected = resources.len();
    let state = if bounded || failure.is_some() {
        RepositoryModuleState::Partial
    } else {
        RepositoryModuleState::Complete
    };
    let mut output = RepositoryMapperOutput {
        resources,
        relations,
        modules: vec![RepositoryModuleResult {
            module,
            collected,
            bounded,
            state,
            failure,
        }],
        warnings: Vec::new(),
        routes,
    };
    sort_output(&mut output);
    output
}

fn sort_output(output: &mut RepositoryMapperOutput) {
    output
        .resources
        .sort_by_key(|r| (r.kind.clone(), r.external_id.clone()));
    output.relations.sort_by_key(|r| {
        (
            r.source.kind.clone(),
            r.source.external_id.clone(),
            r.target.kind.clone(),
            r.target.external_id.clone(),
            r.kind.clone(),
            r.evidence_key.clone(),
        )
    });
    output.modules.sort_by_key(|module| module.module);
    output
        .routes
        .sort_by_key(|route| route.repository_external_id.clone());
}

pub(crate) fn validate_required(value: &str, field: &str) -> Result<(), ConnectorFailure> {
    if value.is_empty() {
        return Err(invalid(format!("GitHub {field} is empty")));
    }
    validate_text(value, field)
}

pub(crate) fn validate_optional(value: Option<&str>, field: &str) -> Result<(), ConnectorFailure> {
    if let Some(value) = value {
        validate_text(value, field)?;
    }
    Ok(())
}

fn validate_text(value: &str, field: &str) -> Result<(), ConnectorFailure> {
    let lower = value.to_ascii_lowercase();
    if value.len() > 1024
        || value.chars().any(char::is_control)
        || lower.starts_with("bearer ")
        || lower.contains("-----begin private key-----")
    {
        return Err(invalid(format!("GitHub {field} is unsafe or too long")));
    }
    Ok(())
}

pub(crate) fn provider_external_id(prefix: &str, id: u64) -> Result<ExternalId, ConnectorFailure> {
    ExternalId::new(format!("{prefix}:{id}")).map_err(domain_failure)
}

pub(crate) fn resource_kind(value: &str) -> Result<ResourceKind, ConnectorFailure> {
    ResourceKind::new(value).map_err(domain_failure)
}

pub(crate) fn schema_version() -> Result<SchemaVersion, ConnectorFailure> {
    SchemaVersion::new(1).map_err(domain_failure)
}

pub(crate) fn invalid(message: impl Into<String>) -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidResponse,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

pub(crate) fn domain_failure(_: next_infra_core::DomainError) -> ConnectorFailure {
    invalid("GitHub repository mapper produced an invalid domain value")
}

fn repository_labels(visibility: &str) -> Result<BTreeMap<LabelKey, String>, ConnectorFailure> {
    let mut labels = BTreeMap::new();
    labels.insert(
        LabelKey::new("github.resource_type").map_err(domain_failure)?,
        "repository".into(),
    );
    labels.insert(
        LabelKey::new("github.visibility").map_err(domain_failure)?,
        visibility.into(),
    );
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(id: u64, name: &str) -> RepositoryDto {
        RepositoryDto {
            id,
            name: name.into(),
            owner: RepositoryOwnerDto {
                login: "fixture-owner".into(),
            },
            visibility: "private".into(),
            default_branch: Some("main".into()),
            archived: false,
            disabled: false,
            created_at: "2026-08-05T00:00:00Z".into(),
            updated_at: "2026-08-05T00:01:00Z".into(),
        }
    }

    #[test]
    fn rename_and_order_do_not_change_identity() {
        let scope = Scope::new("github-account-scope").unwrap();
        let at = Timestamp::from_unix_millis(1_000).unwrap();
        let left = map_repositories(
            &scope,
            at,
            [repository(2, "fixture-b"), repository(1, "fixture-a")],
            false,
            None,
        )
        .unwrap();
        let right = map_repositories(
            &scope,
            at,
            [repository(1, "renamed-a"), repository(2, "renamed-b")],
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            left.resources[0].external_id,
            right.resources[0].external_id
        );
        assert_eq!(
            left.resources[1].external_id,
            right.resources[1].external_id
        );
    }

    #[test]
    fn route_debug_redacts_owner_and_name() {
        let output = map_repositories(
            &Scope::new("github-account-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            [repository(1, "fixture-private-repo")],
            false,
            None,
        )
        .unwrap();
        let debug = format!("{:?}", output.routes[0]);
        assert!(!debug.contains("fixture-owner"));
        assert!(!debug.contains("fixture-private-repo"));
    }

    #[test]
    fn duplicate_visibility_and_secret_values_are_rejected() {
        let scope = Scope::new("github-account-scope").unwrap();
        let at = Timestamp::from_unix_millis(1_000).unwrap();
        assert!(
            map_repositories(
                &scope,
                at,
                [repository(1, "fixture-a"), repository(1, "fixture-b")],
                false,
                None
            )
            .is_err()
        );
        let mut unsupported = repository(1, "fixture-a");
        unsupported.visibility = "future".into();
        assert!(map_repositories(&scope, at, [unsupported], false, None).is_err());
        let mut unsafe_repository = repository(1, "fixture-a");
        unsafe_repository.name = "Bearer fixture-secret".into();
        assert!(map_repositories(&scope, at, [unsafe_repository], false, None).is_err());
    }

    #[test]
    fn dto_debug_hides_route_values() {
        let dto = repository(1, "fixture-private-repo");
        let debug = format!("{dto:?}");
        assert!(!debug.contains("fixture-owner"));
        assert!(!debug.contains("fixture-private-repo"));
    }

    #[test]
    fn targeted_lookup_never_guesses_owner_or_name_from_external_id() {
        let output = map_repositories(
            &Scope::new("github-account-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            [repository(1, "fixture-private-repo")],
            false,
            None,
        )
        .unwrap();
        assert!(
            find_targeted_route(
                &output.routes,
                &ExternalId::new("github-repository:1").unwrap()
            )
            .is_ok()
        );
        assert!(
            find_targeted_route(
                &output.routes,
                &ExternalId::new("github-repository:999").unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn repository_budget_and_failure_force_partial_state() {
        let repositories = (0..=MAX_REPOSITORIES_PER_BATCH)
            .map(|index| repository(u64::try_from(index).unwrap(), &format!("fixture-{index}")))
            .collect::<Vec<_>>();
        let output = map_repositories(
            &Scope::new("github-account-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            repositories,
            false,
            None,
        )
        .unwrap();
        assert_eq!(output.resources.len(), MAX_REPOSITORIES_PER_BATCH);
        assert!(output.modules[0].bounded);
        assert_eq!(output.modules[0].state, RepositoryModuleState::Partial);

        let failure = ConnectorFailure {
            code: ErrorCode::PermissionDenied,
            message: "GitHub token lacks permission for the requested module".into(),
            retryable: false,
            retry_after_ms: None,
        };
        let output = map_repositories(
            &Scope::new("github-account-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            [repository(1, "fixture-a")],
            false,
            Some(failure),
        )
        .unwrap();
        assert_eq!(output.modules[0].state, RepositoryModuleState::Partial);
    }
}
