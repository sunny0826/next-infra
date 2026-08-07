use next_infra_connector_api::{RelationObservation, ResourceLocator, ResourceObservation};
use next_infra_core::{
    EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth, ResourceKind,
    SchemaVersion, Scope, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApplicationDto {
    pub id: String,
    pub name: String,
    pub project_id: Option<String>,
    pub server_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeploymentDto {
    pub id: String,
    pub application_id: String,
    pub status: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerDto {
    pub id: String,
    pub name: String,
    pub address: Option<String>,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DomainDto {
    pub id: String,
    pub domain: String,
    pub application_id: Option<String>,
    pub zone: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DokployMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub redacted_fields: u64,
}

pub fn map_resources(
    scope: &Scope,
    observed_at: Timestamp,
    projects: impl IntoIterator<Item = ProjectDto>,
    applications: impl IntoIterator<Item = ApplicationDto>,
    deployments: impl IntoIterator<Item = DeploymentDto>,
    servers: impl IntoIterator<Item = ServerDto>,
    domains: impl IntoIterator<Item = DomainDto>,
) -> Result<DokployMapperOutput, String> {
    let mut output = DokployMapperOutput {
        resources: Vec::new(),
        relations: Vec::new(),
        redacted_fields: 0,
    };
    for value in projects {
        output.resources.push(project(scope, observed_at, value)?);
    }
    for value in applications {
        output
            .resources
            .push(application(scope, observed_at, value)?);
    }
    for value in deployments {
        output
            .resources
            .push(deployment(scope, observed_at, value)?);
    }
    for value in servers {
        output.resources.push(server(scope, observed_at, value)?);
    }
    for value in domains {
        output.resources.push(domain(scope, observed_at, value)?);
    }
    let by_id = output
        .resources
        .iter()
        .map(|resource| (resource.external_id.clone(), resource))
        .collect::<BTreeMap<_, _>>();
    for resource in &output.resources {
        match resource.kind.as_str() {
            "dokploy.application" => {
                if let Some(project_id) = resource
                    .attributes
                    .get("project_id")
                    .and_then(|value| value.as_str())
                {
                    let project = external("dokploy.project", project_id)?;
                    if by_id.contains_key(&project) {
                        output.relations.push(provider_relation(
                            "dokploy.project",
                            project,
                            resource,
                            "dokploy.contains",
                            "project_id",
                            observed_at,
                        )?);
                    }
                }
                if let Some(server_id) = resource
                    .attributes
                    .get("server_id")
                    .and_then(|value| value.as_str())
                {
                    let server = external("dokploy.server", server_id)?;
                    if by_id.contains_key(&server) {
                        output.relations.push(provider_relation(
                            "dokploy.application",
                            resource.external_id.clone(),
                            by_id[&server],
                            "dokploy.runs_on",
                            "server_id",
                            observed_at,
                        )?);
                    }
                }
            }
            "dokploy.deployment" | "dokploy.domain" => {
                if let Some(application_id) = resource
                    .attributes
                    .get("application_id")
                    .and_then(|value| value.as_str())
                {
                    let application = external("dokploy.application", application_id)?;
                    if by_id.contains_key(&application) {
                        let kind = if resource.kind.as_str() == "dokploy.deployment" {
                            "dokploy.deploys"
                        } else {
                            "dokploy.exposes"
                        };
                        output.relations.push(provider_relation(
                            "dokploy.application",
                            application,
                            resource,
                            kind,
                            "application_id",
                            observed_at,
                        )?);
                    }
                }
            }
            _ => {}
        }
    }
    output
        .resources
        .sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
    output.relations.sort_by_key(|relation| {
        (
            relation.source.external_id.clone(),
            relation.target.external_id.clone(),
            relation.kind.clone(),
        )
    });
    Ok(output)
}

fn provider_relation(
    source_kind: &str,
    source_id: ExternalId,
    target: &ResourceObservation,
    kind: &str,
    field_path: &str,
    observed_at: Timestamp,
) -> Result<RelationObservation, String> {
    Ok(RelationObservation {
        source: ResourceLocator {
            kind: ResourceKind::new(source_kind).map_err(|_| "invalid relation source kind")?,
            external_id: source_id,
        },
        target: ResourceLocator {
            kind: target.kind.clone(),
            external_id: target.external_id.clone(),
        },
        kind: RelationKind::new(kind).map_err(|_| "invalid relation kind")?,
        evidence_key: EvidenceKey::new(format!(
            "dokploy:{kind}:{field_path}:{}",
            target.external_id
        ))
        .map_err(|_| "invalid evidence key")?,
        field_path: FieldPath::new(field_path).map_err(|_| "invalid field path")?,
        observed_at,
    })
}

fn project(scope: &Scope, at: Timestamp, value: ProjectDto) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.project",
        &value.id,
        &value.name,
        scope,
        at,
        json!({"description": value.description}),
    )
}
fn application(
    scope: &Scope,
    at: Timestamp,
    value: ApplicationDto,
) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.application",
        &value.id,
        &value.name,
        scope,
        at,
        json!({"project_id": value.project_id, "server_id": value.server_id}),
    )
}
fn deployment(
    scope: &Scope,
    at: Timestamp,
    value: DeploymentDto,
) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.deployment",
        &value.id,
        &value.id,
        scope,
        at,
        json!({"application_id": value.application_id, "status": value.status, "created_at": value.created_at}),
    )
}
fn server(scope: &Scope, at: Timestamp, value: ServerDto) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.server",
        &value.id,
        &value.name,
        scope,
        at,
        json!({"address": value.address, "status": value.status}),
    )
}
fn domain(scope: &Scope, at: Timestamp, value: DomainDto) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.domain",
        &value.id,
        &value.domain,
        scope,
        at,
        json!({"application_id": value.application_id, "zone": value.zone}),
    )
}

fn resource(
    kind: &str,
    id: &str,
    name: &str,
    scope: &Scope,
    at: Timestamp,
    attributes: serde_json::Value,
) -> Result<ResourceObservation, String> {
    if id.is_empty() || name.is_empty() || id.len() > 512 || name.len() > 1024 {
        return Err("Dokploy resource identity is invalid".into());
    }
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid resource kind")?,
        external_id: external(kind, id)?,
        name: id.into(),
        display_name: name.into(),
        scope: scope.clone(),
        labels: BTreeMap::from([(
            LabelKey::new("dokploy.resource_type").map_err(|_| "invalid label")?,
            kind.trim_start_matches("dokploy.").into(),
        )]),
        health: ResourceHealth::Unknown,
        attributes,
        attribute_schema_version: SchemaVersion::new(1).map_err(|_| "invalid schema")?,
        observed_at: at,
    })
}

fn external(kind: &str, id: &str) -> Result<ExternalId, String> {
    ExternalId::new(format!("{kind}:{id}")).map_err(|_| "invalid external id".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_is_stable_and_drops_unknown_fields() {
        let application: ApplicationDto = serde_json::from_str(r#"{"id":"app-1","name":"Fixture App","project_id":"project-1","server_id":"server-1","secret":"must-drop"}"#).unwrap();
        let project: ProjectDto = serde_json::from_str(r#"{"id":"project-1","name":"Fixture Project","description":"safe","token":"must-drop"}"#).unwrap();
        let output = map_resources(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            [project],
            [application],
            [],
            [],
            [],
        )
        .unwrap();
        assert_eq!(output.resources.len(), 2);
        assert_eq!(output.relations.len(), 1);
        let json = serde_json::to_string(&output.resources).unwrap();
        assert!(!json.contains("must-drop"));
    }

    #[test]
    fn maps_only_explicit_provider_relationships() {
        let output = map_resources(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            [ProjectDto {
                id: "project".into(),
                name: "Project".into(),
                description: None,
            }],
            [ApplicationDto {
                id: "application".into(),
                name: "Application".into(),
                project_id: Some("project".into()),
                server_id: Some("server".into()),
            }],
            [DeploymentDto {
                id: "deployment".into(),
                application_id: "application".into(),
                status: Some("running".into()),
                created_at: None,
            }],
            [ServerDto {
                id: "server".into(),
                name: "Server".into(),
                address: None,
                status: None,
            }],
            [DomainDto {
                id: "domain".into(),
                domain: "fixture.example.test".into(),
                application_id: Some("application".into()),
                zone: None,
            }],
        )
        .unwrap();
        assert_eq!(output.relations.len(), 4);
        assert_eq!(
            output
                .relations
                .iter()
                .map(|relation| relation.kind.as_str())
                .collect::<Vec<_>>(),
            vec![
                "dokploy.deploys",
                "dokploy.exposes",
                "dokploy.runs_on",
                "dokploy.contains"
            ],
        );
    }
}
