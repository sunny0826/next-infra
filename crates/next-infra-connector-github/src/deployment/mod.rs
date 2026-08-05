use crate::repository::{
    RepositoryMapperOutput, RepositoryRouteContext, domain_failure, module_output,
    provider_external_id, resource_kind, schema_version, validate_optional, validate_required,
};
use next_infra_connector_api::{
    ConnectorFailure, RelationObservation, ResourceLocator, ResourceObservation,
};
use next_infra_core::{EvidenceKey, FieldPath, LabelKey, RelationKind, ResourceHealth};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_DEPLOYMENTS_PER_REPOSITORY: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct DeploymentDto {
    pub id: u64,
    pub environment: Option<String>,
    pub task: String,
    pub transient_environment: bool,
    pub production_environment: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub fn map_deployments(
    context: &RepositoryRouteContext,
    deployments: impl IntoIterator<Item = DeploymentDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<RepositoryMapperOutput, ConnectorFailure> {
    let mut deployments = deployments.into_iter().collect::<Vec<_>>();
    let bounded = bounded || deployments.len() > MAX_DEPLOYMENTS_PER_REPOSITORY;
    deployments.truncate(MAX_DEPLOYMENTS_PER_REPOSITORY);
    let mut resources = Vec::new();
    let mut relations = Vec::new();
    let mut identities = BTreeSet::new();

    for deployment in deployments {
        validate_optional(deployment.environment.as_deref(), "deployment environment")?;
        validate_required(&deployment.task, "deployment task")?;
        validate_required(&deployment.created_at, "deployment created_at")?;
        validate_required(&deployment.updated_at, "deployment updated_at")?;
        let external_id = provider_external_id("github-deployment", deployment.id)?;
        if !identities.insert(external_id.clone()) {
            return Err(crate::repository::invalid(
                "GitHub deployments contain duplicate identities",
            ));
        }
        let name = format!("deployment-{}", deployment.id);
        let display_name = deployment
            .environment
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&name)
            .to_owned();
        resources.push(ResourceObservation {
            kind: resource_kind("github.deployment")?,
            external_id: external_id.clone(),
            name,
            display_name,
            scope: context.scope().clone(),
            labels: resource_labels("deployment")?,
            health: ResourceHealth::Unknown,
            attributes: json!({
                "deployment_id": deployment.id,
                "repository_id": context.repository_id(),
                "environment": deployment.environment,
                "task": deployment.task,
                "transient_environment": deployment.transient_environment,
                "production_environment": deployment.production_environment,
                "created_at": deployment.created_at,
                "updated_at": deployment.updated_at,
            }),
            attribute_schema_version: schema_version()?,
            observed_at: context.observed_at(),
        });
        relations.push(RelationObservation {
            source: ResourceLocator {
                kind: resource_kind("github.repository")?,
                external_id: context.repository_external_id().clone(),
            },
            target: ResourceLocator {
                kind: resource_kind("github.deployment")?,
                external_id,
            },
            kind: RelationKind::new("github.contains").map_err(domain_failure)?,
            evidence_key: EvidenceKey::new(format!("github-provider-deployment:{}", deployment.id))
                .map_err(domain_failure)?,
            field_path: FieldPath::new("attributes.repository_id").map_err(domain_failure)?,
            observed_at: context.observed_at(),
        });
    }

    Ok(module_output(
        "github.deployments",
        resources,
        relations,
        Vec::new(),
        bounded,
        failure,
    ))
}

fn resource_labels(resource_type: &str) -> Result<BTreeMap<LabelKey, String>, ConnectorFailure> {
    let mut labels = BTreeMap::new();
    labels.insert(
        LabelKey::new("github.resource_type").map_err(domain_failure)?,
        resource_type.into(),
    );
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{RepositoryDto, RepositoryOwnerDto, map_repositories};
    use next_infra_core::{Scope, Timestamp};

    fn context() -> RepositoryRouteContext {
        map_repositories(
            &Scope::new("github-account-scope").unwrap(),
            Timestamp::from_unix_millis(1_000).unwrap(),
            [RepositoryDto {
                id: 10,
                name: "fixture-repo".into(),
                owner: RepositoryOwnerDto {
                    login: "fixture-owner".into(),
                },
                visibility: "private".into(),
                default_branch: None,
                archived: false,
                disabled: false,
                created_at: "2026-08-05T00:00:00Z".into(),
                updated_at: "2026-08-05T00:01:00Z".into(),
            }],
            false,
            None,
        )
        .unwrap()
        .routes
        .remove(0)
    }

    #[test]
    fn deployment_is_summary_only_and_relation_is_stable() {
        let output = map_deployments(
            &context(),
            [DeploymentDto {
                id: 30,
                environment: Some("fixture-environment".into()),
                task: "deploy".into(),
                transient_environment: false,
                production_environment: true,
                created_at: "2026-08-05T00:00:00Z".into(),
                updated_at: "2026-08-05T00:01:00Z".into(),
            }],
            false,
            None,
        )
        .unwrap();
        assert_eq!(output.resources[0].health, ResourceHealth::Unknown);
        assert_eq!(output.resources[0].attributes["repository_id"], 10);
        assert_eq!(
            output.relations[0].evidence_key.as_str(),
            "github-provider-deployment:30"
        );
    }

    #[test]
    fn empty_environment_uses_stable_deployment_display() {
        let output = map_deployments(
            &context(),
            [DeploymentDto {
                id: 30,
                environment: Some(String::new()),
                task: "deploy".into(),
                transient_environment: true,
                production_environment: false,
                created_at: "2026-08-05T00:00:00Z".into(),
                updated_at: "2026-08-05T00:01:00Z".into(),
            }],
            false,
            None,
        )
        .unwrap();
        assert_eq!(output.resources[0].display_name, "deployment-30");
    }
}
