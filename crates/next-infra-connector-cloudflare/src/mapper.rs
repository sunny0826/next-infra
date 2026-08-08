use next_infra_connector_api::{RelationObservation, ResourceLocator, ResourceObservation};
use next_infra_core::{
    EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth, ResourceKind,
    SchemaVersion, Scope, Timestamp,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
pub struct AccountDto {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct ZoneDto {
    pub id: String,
    pub name: String,
    pub account: AccountDto,
    pub status: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct DnsRecordDto {
    pub id: String,
    pub zone_id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
    pub proxied: Option<bool>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct TunnelDto {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct WorkerDto {
    pub id: String,
    pub modified_on: Option<String>,
}

/// Wraps a tunnel with the account id it belongs to (account_id is not present in
/// the real GET /accounts/{account_id}/cfd_tunnel response body).
#[derive(Clone, Debug)]
pub struct TunnelWithAccount {
    pub tunnel: TunnelDto,
    pub account_id: String,
}
/// Wraps a worker with the account id it belongs to (account_id is not present in
/// the real GET /accounts/{account_id}/workers/scripts response body).
#[derive(Clone, Debug)]
pub struct WorkerWithAccount {
    pub worker: WorkerDto,
    pub account_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudflareMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
}

pub fn map_resources(
    scope: &Scope,
    observed_at: Timestamp,
    accounts: impl IntoIterator<Item = AccountDto>,
    zones: impl IntoIterator<Item = ZoneDto>,
    records: impl IntoIterator<Item = DnsRecordDto>,
    tunnels: impl IntoIterator<Item = TunnelWithAccount>,
    workers: impl IntoIterator<Item = WorkerWithAccount>,
) -> Result<CloudflareMapperOutput, String> {
    let mut output = CloudflareMapperOutput {
        resources: Vec::new(),
        relations: Vec::new(),
    };
    for value in accounts {
        output.resources.push(resource(
            "cloudflare.account",
            &value.id,
            &value.name,
            scope,
            observed_at,
            json!({}),
        )?);
    }
    for value in zones {
        output.resources.push(resource(
            "cloudflare.zone",
            &value.id,
            &value.name,
            scope,
            observed_at,
            json!({"account_id": value.account.id, "status": value.status}),
        )?);
    }
    for value in records {
        if !matches!(
            value.record_type.as_str(),
            "A" | "AAAA" | "CNAME" | "TXT" | "MX" | "SRV" | "CAA" | "NS"
        ) {
            return Err("Cloudflare DNS record type is unsupported".into());
        }
        output.resources.push(resource("cloudflare.dns_record", &value.id, &value.name, scope, observed_at, json!({"zone_id": value.zone_id, "type": value.record_type, "content": value.content, "proxied": value.proxied}))?);
    }
    for value in tunnels {
        output.resources.push(resource(
            "cloudflare.tunnel",
            &value.tunnel.id,
            &value.tunnel.name,
            scope,
            observed_at,
            json!({"account_id": value.account_id, "status": value.tunnel.status}),
        )?);
    }
    for value in workers {
        output.resources.push(resource(
            "cloudflare.worker",
            &value.worker.id,
            &value.worker.id,
            scope,
            observed_at,
            json!({"account_id": value.account_id, "modified_on": value.worker.modified_on}),
        )?);
    }
    let by_id = output
        .resources
        .iter()
        .map(|resource| (resource.external_id.clone(), resource))
        .collect::<BTreeMap<_, _>>();
    for resource in &output.resources {
        let (source_kind, source_key, relation, field_path) = match resource.kind.as_str() {
            "cloudflare.zone" | "cloudflare.tunnel" | "cloudflare.worker" => (
                "cloudflare.account",
                "account_id",
                "cloudflare.contains",
                "account_id",
            ),
            "cloudflare.dns_record" => (
                "cloudflare.zone",
                "zone_id",
                "cloudflare.contains",
                "zone_id",
            ),
            _ => continue,
        };
        let Some(source_value) = resource
            .attributes
            .get(source_key)
            .and_then(|value| value.as_str())
        else {
            continue;
        };
        let source_id = external(source_kind, source_value)?;
        if by_id.contains_key(&source_id) {
            output.relations.push(RelationObservation {
                source: ResourceLocator {
                    kind: ResourceKind::new(source_kind).map_err(|_| "invalid source kind")?,
                    external_id: source_id,
                },
                target: ResourceLocator {
                    kind: resource.kind.clone(),
                    external_id: resource.external_id.clone(),
                },
                kind: RelationKind::new(relation).map_err(|_| "invalid relation kind")?,
                evidence_key: EvidenceKey::new(format!(
                    "cloudflare:{relation}:{field_path}:{}",
                    resource.external_id
                ))
                .map_err(|_| "invalid evidence")?,
                field_path: FieldPath::new(field_path).map_err(|_| "invalid field path")?,
                observed_at,
            });
        }
    }
    output
        .resources
        .sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
    output.relations.sort_by_key(|relation| {
        (
            relation.source.external_id.clone(),
            relation.target.external_id.clone(),
        )
    });
    Ok(output)
}

fn resource(
    kind: &str,
    id: &str,
    display_name: &str,
    scope: &Scope,
    observed_at: Timestamp,
    attributes: serde_json::Value,
) -> Result<ResourceObservation, String> {
    if id.is_empty() || display_name.is_empty() || id.len() > 512 || display_name.len() > 1024 {
        return Err("Cloudflare resource identity is invalid".into());
    }
    Ok(ResourceObservation {
        kind: ResourceKind::new(kind).map_err(|_| "invalid resource kind")?,
        external_id: external(kind, id)?,
        name: id.into(),
        display_name: display_name.into(),
        scope: scope.clone(),
        labels: BTreeMap::from([(
            LabelKey::new("cloudflare.resource_type").map_err(|_| "invalid label")?,
            kind.trim_start_matches("cloudflare.").into(),
        )]),
        health: ResourceHealth::Unknown,
        attributes,
        attribute_schema_version: SchemaVersion::new(1).map_err(|_| "invalid schema")?,
        observed_at,
    })
}
fn external(kind: &str, id: &str) -> Result<ExternalId, String> {
    ExternalId::new(format!("{kind}:{id}")).map_err(|_| "invalid external id".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapper_preserves_relationships_without_worker_code() {
        let account: AccountDto =
            serde_json::from_str(r#"{"id":"a","name":"Fixture","unknown":"drop"}"#).unwrap();
        let worker: WorkerDto =
            serde_json::from_str(r#"{"id":"w","modified_on":"now","script":"must-drop"}"#).unwrap();
        let output = map_resources(
            &Scope::new("fixture-scope").unwrap(),
            Timestamp::from_unix_millis(1).unwrap(),
            [account],
            [],
            [],
            [],
            [WorkerWithAccount {
                worker,
                account_id: "a".into(),
            }],
        )
        .unwrap();
        assert_eq!(output.relations.len(), 1);
        assert!(
            !serde_json::to_string(&output.resources)
                .unwrap()
                .contains("must-drop")
        );
    }

    #[test]
    fn zone_deserializes_nested_account_shape() {
        // Real Cloudflare GET /zones returns account as a nested {id, name} object,
        // not a flat account_id string.
        let zone: ZoneDto = serde_json::from_str(
            r#"{"id":"z","name":"example.com","account":{"id":"a","name":"My Account"},"status":"active"}"#,
        )
        .unwrap();
        assert_eq!(zone.account.id, "a");
        assert_eq!(zone.account.name, "My Account");
    }

    #[test]
    fn dns_record_deserializes_type_field() {
        // Real Cloudflare DNS records use "type" not "record_type".
        let record: DnsRecordDto = serde_json::from_str(
            r#"{"id":"r","zone_id":"z","type":"A","name":"test.example.com","content":"192.0.2.1","proxied":true}"#,
        )
        .unwrap();
        assert_eq!(record.record_type, "A");
    }

    #[test]
    fn tunnel_and_worker_have_no_account_id_in_payload() {
        // Real Cloudflare tunnel/worker responses do not include account_id;
        // the account context is derived from the request path.
        let tunnel: TunnelDto =
            serde_json::from_str(r#"{"id":"t","name":"My Tunnel","status":"active"}"#).unwrap();
        assert_eq!(tunnel.id, "t");
        let worker: WorkerDto =
            serde_json::from_str(r#"{"id":"w","modified_on":"2024-01-01T00:00:00Z"}"#).unwrap();
        assert_eq!(worker.id, "w");
    }
}
