use crate::repository::{
    RepositoryMapperOutput, RepositoryRouteContext, domain_failure, module_output,
    provider_external_id, resource_kind, schema_version, validate_required,
};
use next_infra_connector_api::{
    ConnectorFailure, RelationObservation, ResourceLocator, ResourceObservation,
};
use next_infra_core::{EvidenceKey, FieldPath, LabelKey, RelationKind, ResourceHealth};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ENVIRONMENTS_PER_REPOSITORY: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EnvironmentListDto {
    pub total_count: u64,
    pub environments: Vec<EnvironmentDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct EnvironmentDto {
    pub id: u64,
    pub name: String,
    pub deployment_branch_policy: Option<DeploymentBranchPolicyDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct DeploymentBranchPolicyDto {
    pub protected_branches: bool,
    pub custom_branch_policies: bool,
}

pub fn map_environments(
    context: &RepositoryRouteContext,
    environments: impl IntoIterator<Item = EnvironmentDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<RepositoryMapperOutput, ConnectorFailure> {
    let mut environments = environments.into_iter().collect::<Vec<_>>();
    let bounded = bounded || environments.len() > MAX_ENVIRONMENTS_PER_REPOSITORY;
    environments.truncate(MAX_ENVIRONMENTS_PER_REPOSITORY);
    let mut resources = Vec::new();
    let mut relations = Vec::new();
    let mut identities = BTreeSet::new();

    for environment in environments {
        validate_required(&environment.name, "environment name")?;
        let external_id = provider_external_id("github-environment", environment.id)?;
        if !identities.insert(external_id.clone()) {
            return Err(crate::repository::invalid(
                "GitHub environments contain duplicate identities",
            ));
        }
        let (protected_branches, custom_branch_policies) = environment
            .deployment_branch_policy
            .map_or((None, None), |policy| {
                (
                    Some(policy.protected_branches),
                    Some(policy.custom_branch_policies),
                )
            });
        resources.push(ResourceObservation {
            kind: resource_kind("github.environment")?,
            external_id: external_id.clone(),
            name: environment.name.clone(),
            display_name: environment.name,
            scope: context.scope().clone(),
            labels: resource_labels("environment")?,
            health: ResourceHealth::Unknown,
            attributes: json!({
                "environment_id": environment.id,
                "repository_id": context.repository_id(),
                "protected_branches": protected_branches,
                "custom_branch_policies": custom_branch_policies,
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
                kind: resource_kind("github.environment")?,
                external_id,
            },
            kind: RelationKind::new("github.contains").map_err(domain_failure)?,
            evidence_key: EvidenceKey::new(format!(
                "github-provider-environment:{}",
                environment.id
            ))
            .map_err(domain_failure)?,
            field_path: FieldPath::new("attributes.repository_id").map_err(domain_failure)?,
            observed_at: context.observed_at(),
        });
    }

    Ok(module_output(
        "github.environments",
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
    fn policy_and_relation_are_mapped_without_reviewers() {
        let output = map_environments(
            &context(),
            [EnvironmentDto {
                id: 20,
                name: "fixture-environment".into(),
                deployment_branch_policy: Some(DeploymentBranchPolicyDto {
                    protected_branches: true,
                    custom_branch_policies: false,
                }),
            }],
            false,
            None,
        )
        .unwrap();
        assert_eq!(output.resources[0].attributes["repository_id"], 10);
        assert_eq!(output.resources[0].attributes["protected_branches"], true);
        assert_eq!(
            output.relations[0].field_path.as_str(),
            "attributes.repository_id"
        );
    }

    #[test]
    fn missing_policy_is_stable_null() {
        let output = map_environments(
            &context(),
            [EnvironmentDto {
                id: 20,
                name: "fixture-environment".into(),
                deployment_branch_policy: None,
            }],
            false,
            None,
        )
        .unwrap();
        assert!(output.resources[0].attributes["protected_branches"].is_null());
        assert!(output.resources[0].attributes["custom_branch_policies"].is_null());
    }
}
