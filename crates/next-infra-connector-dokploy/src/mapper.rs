use next_infra_connector_api::{RelationObservation, ResourceLocator, ResourceObservation};
use next_infra_core::{
    EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth, ResourceKind,
    SchemaVersion, Scope, Timestamp,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// DTOs — Dokploy v2 REST API shape (tolerant of unknown fields via serde)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectDto {
    #[serde(rename = "projectId")]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Old-shape applications (top-level field, pre-v2 API shape)
    #[serde(default)]
    pub applications: Vec<ApplicationDto>,
    /// New-shape environments containing nested applications
    #[serde(default)]
    pub environments: Vec<EnvironmentDto>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EnvironmentDto {
    #[serde(rename = "environmentId")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub applications: Vec<ApplicationDto>,
    /// Compose services — tolerated as raw Value (serde drops unknowns)
    #[serde(default)]
    pub compose: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApplicationDto {
    #[serde(rename = "applicationId")]
    pub id: String,
    pub name: String,
    #[serde(rename = "serverId", default)]
    pub server_id: Option<String>,
    #[serde(rename = "environmentId", default)]
    pub environment_id: Option<String>,
    /// Filled by the connector from parent project context — not in JSON
    #[serde(skip, default)]
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeploymentDto {
    #[serde(rename = "deploymentId")]
    pub id: String,
    #[serde(rename = "applicationId", default)]
    pub application_id: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerDto {
    #[serde(rename = "serverId")]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "ipAddress", default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DomainDto {
    #[serde(rename = "domainId")]
    pub id: String,
    /// The JSON field is `host`; we deserialize into `domain` for display compat
    pub host: String,
    #[serde(rename = "applicationId", default)]
    pub application_id: Option<String>,
    pub https: Option<bool>,
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------
// Mapper output
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DokployMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub redacted_fields: u64,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Relation builder
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Resource builders
// ---------------------------------------------------------------------------

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
        json!({
            "project_id": value.project_id,
            "server_id": value.server_id,
            "environment_id": value.environment_id,
        }),
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
        json!({
            "application_id": value.application_id,
            "status": value.status,
            "created_at": value.created_at,
            "title": value.title,
        }),
    )
}

fn server(scope: &Scope, at: Timestamp, value: ServerDto) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.server",
        &value.id,
        &value.name,
        scope,
        at,
        json!({
            "address": value.ip_address,
            "description": value.description,
            "status": value.status,
        }),
    )
}

fn domain(scope: &Scope, at: Timestamp, value: DomainDto) -> Result<ResourceObservation, String> {
    resource(
        "dokploy.domain",
        &value.id,
        &value.host,
        scope,
        at,
        json!({
            "application_id": value.application_id,
            "https": value.https,
            "path": value.path,
        }),
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_is_stable_and_drops_unknown_fields() {
        // v2-shaped JSON with environment nesting + unknown fields
        let project_json = r#"{
            "projectId": "project-1",
            "name": "Fixture Project",
            "description": "safe",
            "token": "must-drop",
            "environments": [{
                "environmentId": "env-1",
                "name": "Production",
                "applications": [{
                    "applicationId": "app-1",
                    "name": "Fixture App",
                    "serverId": "server-1",
                    "secret": "must-drop"
                }],
                "compose": []
            }]
        }"#;
        let project: ProjectDto = serde_json::from_str(project_json).unwrap();
        assert_eq!(project.id, "project-1");
        assert_eq!(project.environments.len(), 1);
        assert_eq!(project.environments[0].applications.len(), 1);
        assert_eq!(project.environments[0].applications[0].id, "app-1");

        // Old-shape top-level applications also tolerated
        let old_shape: ProjectDto = serde_json::from_str(
            r#"{
            "projectId": "project-old",
            "name": "Old Shape",
            "applications": [{"applicationId": "app-old", "name": "Old App"}]
        }"#,
        )
        .unwrap();
        assert_eq!(old_shape.applications.len(), 1);

        let output = map_resources(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            [project, old_shape],
            [], // applications come from embedded flattening in connector
            [],
            [],
            [],
        )
        .unwrap();
        // 2 projects, 0 applications (not passed separately here)
        assert_eq!(output.resources.len(), 2);
        assert_eq!(output.relations.len(), 0);
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
                applications: vec![],
                environments: vec![],
            }],
            [ApplicationDto {
                id: "application".into(),
                name: "Application".into(),
                server_id: Some("server".into()),
                environment_id: Some("env-1".into()),
                project_id: Some("project".into()),
            }],
            [DeploymentDto {
                id: "deployment".into(),
                application_id: Some("application".into()),
                status: Some("running".into()),
                created_at: None,
                title: None,
            }],
            [ServerDto {
                id: "server".into(),
                name: "Server".into(),
                description: None,
                ip_address: Some("10.0.0.1".into()),
                status: None,
            }],
            [DomainDto {
                id: "domain".into(),
                host: "fixture.example.test".into(),
                application_id: Some("application".into()),
                https: Some(true),
                path: Some("/".into()),
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

    #[test]
    fn deployment_and_domain_dto_tolerates_missing_fields() {
        // Deployment with only deploymentId filled
        let dep_json = r#"{"deploymentId": "deploy-1"}"#;
        let dep: DeploymentDto = serde_json::from_str(dep_json).unwrap();
        assert_eq!(dep.id, "deploy-1");
        assert!(dep.application_id.is_none());
        assert!(dep.status.is_none());
        assert!(dep.created_at.is_none());
        assert!(dep.title.is_none());

        // Domain with only domainId and host
        let dom_json = r#"{"domainId": "dom-1", "host": "example.test"}"#;
        let dom: DomainDto = serde_json::from_str(dom_json).unwrap();
        assert_eq!(dom.id, "dom-1");
        assert_eq!(dom.host, "example.test");
        assert!(dom.application_id.is_none());
    }

    #[test]
    fn server_dto_v2_field_names() {
        let json = r#"{
            "serverId": "srv-1",
            "name": "Prod Server",
            "description": "Production",
            "ipAddress": "10.0.0.5",
            "status": "online"
        }"#;
        let srv: ServerDto = serde_json::from_str(json).unwrap();
        assert_eq!(srv.id, "srv-1");
        assert_eq!(srv.name, "Prod Server");
        assert_eq!(srv.description, Some("Production".into()));
        assert_eq!(srv.ip_address, Some("10.0.0.5".into()));
        assert_eq!(srv.status, Some("online".into()));
    }
}
