//! Deterministic observation normalization for Next Infra.

use next_infra_connector_api::{
    ObservationBatch, ObservationWarning, ProviderRequestSummary, RedactionReport,
    RelationObservation, ResourceObservation, SyncRequest,
};
use next_infra_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeSchema {
    pub kind: ResourceKind,
    pub schema_version: SchemaVersion,
    pub allowed_attributes: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationSchema {
    pub kind: RelationKind,
    pub source_kind: ResourceKind,
    pub target_kind: ResourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValidatedRelationKey {
    pub source: ResourceKey,
    pub target: ResourceKey,
    pub kind: RelationKind,
    pub evidence_key: EvidenceKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedResource {
    pub key: ResourceKey,
    pub name: String,
    pub display_name: String,
    pub scope: Scope,
    pub labels: BTreeMap<LabelKey, String>,
    pub health: ResourceHealth,
    pub attributes: Value,
    pub attribute_schema_version: SchemaVersion,
    pub observed_at: Timestamp,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedRelation {
    pub key: ValidatedRelationKey,
    pub evidence: RelationEvidence,
    pub observed_at: Timestamp,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedBatch {
    pub connection_id: ConnectionId,
    pub sync_run_id: SyncRunId,
    pub resources: Vec<ValidatedResource>,
    pub relations: Vec<ValidatedRelation>,
    pub coverage: SyncCoverage,
    pub next_cursor: Option<SyncCursor>,
    pub warnings: Vec<ObservationWarning>,
    pub redaction_report: RedactionReport,
    pub provider_request_summary: ProviderRequestSummary,
}

pub struct Normalizer {
    attribute_schemas: BTreeMap<(ResourceKind, u32), AttributeSchema>,
    relation_schemas: Vec<RelationSchema>,
}

impl Normalizer {
    pub fn new(
        attribute_schemas: impl IntoIterator<Item = AttributeSchema>,
        relation_schemas: impl IntoIterator<Item = RelationSchema>,
    ) -> Result<Self, DomainError> {
        let mut attributes = BTreeMap::new();
        for schema in attribute_schemas {
            if schema
                .allowed_attributes
                .iter()
                .any(|path| path.split('.').any(str::is_empty) || is_sensitive_field_name(path))
            {
                return Err(DomainError::invalid_value(
                    "attribute schema contains an invalid or sensitive path",
                ));
            }
            let key = (schema.kind.clone(), schema.schema_version.get());
            if attributes.insert(key, schema).is_some() {
                return Err(DomainError::invalid_value(
                    "duplicate attribute schema registration",
                ));
            }
        }
        let relations = relation_schemas.into_iter().collect::<Vec<_>>();
        let unique_relations = relations
            .iter()
            .map(|schema| (&schema.kind, &schema.source_kind, &schema.target_kind))
            .collect::<BTreeSet<_>>();
        if unique_relations.len() != relations.len() {
            return Err(DomainError::invalid_value(
                "duplicate relation schema registration",
            ));
        }
        Ok(Self {
            attribute_schemas: attributes,
            relation_schemas: relations,
        })
    }

    pub fn normalize(
        &self,
        request: &SyncRequest,
        batch: ObservationBatch,
    ) -> Result<ValidatedBatch, DomainError> {
        batch.validate_for(request)?;
        for warning in &batch.warnings {
            scan_text(&warning.message)?;
        }
        let mut redaction_report = batch.redaction_report;
        let mut resources = BTreeMap::<ResourceKey, ValidatedResource>::new();
        for observation in batch.resources {
            let (resource, dropped) = self.normalize_resource(request, observation)?;
            redaction_report.unknown_fields_dropped += dropped;
            match resources.get(&resource.key) {
                Some(existing) if existing.fingerprint != resource.fingerprint => {
                    return Err(contract_error(
                        ErrorCode::InvalidResponse,
                        "conflicting observations share one resource identity",
                    ));
                }
                Some(existing) if existing.observed_at >= resource.observed_at => {}
                _ => {
                    resources.insert(resource.key.clone(), resource);
                }
            }
        }

        let mut relations = BTreeMap::<ValidatedRelationKey, ValidatedRelation>::new();
        for observation in batch.relations {
            let relation = self.normalize_relation(request, observation)?;
            match relations.get(&relation.key) {
                Some(existing) if existing.fingerprint != relation.fingerprint => {
                    return Err(contract_error(
                        ErrorCode::InvalidResponse,
                        "conflicting observations share one relation identity",
                    ));
                }
                Some(existing) if existing.observed_at >= relation.observed_at => {}
                _ => {
                    relations.insert(relation.key.clone(), relation);
                }
            }
        }

        Ok(ValidatedBatch {
            connection_id: request.connection.connection_id.clone(),
            sync_run_id: request.sync_run_id.clone(),
            resources: resources.into_values().collect(),
            relations: relations.into_values().collect(),
            coverage: batch.coverage,
            next_cursor: batch.next_cursor,
            warnings: batch.warnings,
            redaction_report,
            provider_request_summary: batch.provider_request_summary,
        })
    }

    fn normalize_resource(
        &self,
        request: &SyncRequest,
        observation: ResourceObservation,
    ) -> Result<(ValidatedResource, u64), DomainError> {
        scan_for_secrets(&observation.attributes)?;
        scan_text(&observation.name)?;
        scan_text(&observation.display_name)?;
        for (key, value) in &observation.labels {
            if is_sensitive_field_name(key.as_str()) {
                return Err(contract_error(
                    ErrorCode::InvalidResponse,
                    "resource labels contain a forbidden secret field",
                ));
            }
            scan_text(value)?;
        }
        let schema = self
            .attribute_schemas
            .get(&(
                observation.kind.clone(),
                observation.attribute_schema_version.get(),
            ))
            .ok_or_else(|| {
                contract_error(
                    ErrorCode::SchemaIncompatible,
                    "no matching resource attribute schema",
                )
            })?;
        let object = observation.attributes.as_object().ok_or_else(|| {
            contract_error(
                ErrorCode::InvalidResponse,
                "resource attributes must be a JSON object",
            )
        })?;
        let (attributes, dropped) = project_attributes(object, &schema.allowed_attributes);
        let key = ResourceKey {
            connection_id: request.connection.connection_id.clone(),
            kind: observation.kind,
            external_id: observation.external_id,
        };
        let fingerprint = fingerprint(&json!({
            "key": &key,
            "name": &observation.name,
            "display_name": &observation.display_name,
            "scope": &observation.scope,
            "labels": &observation.labels,
            "health": observation.health,
            "attributes": &attributes,
            "attribute_schema_version": observation.attribute_schema_version,
        }))?;
        Ok((
            ValidatedResource {
                key,
                name: observation.name,
                display_name: observation.display_name,
                scope: observation.scope,
                labels: observation.labels,
                health: observation.health,
                attributes,
                attribute_schema_version: observation.attribute_schema_version,
                observed_at: observation.observed_at,
                fingerprint,
            },
            dropped,
        ))
    }

    fn normalize_relation(
        &self,
        request: &SyncRequest,
        observation: RelationObservation,
    ) -> Result<ValidatedRelation, DomainError> {
        if is_sensitive_field_name(observation.field_path.as_str()) {
            return Err(contract_error(
                ErrorCode::InvalidResponse,
                "relation evidence points at a forbidden secret field",
            ));
        }
        let schema_exists = self.relation_schemas.iter().any(|schema| {
            schema.kind == observation.kind
                && schema.source_kind == observation.source.kind
                && schema.target_kind == observation.target.kind
        });
        if !schema_exists {
            return Err(contract_error(
                ErrorCode::SchemaIncompatible,
                "relation kind or endpoint kinds are not registered",
            ));
        }
        let source = ResourceKey {
            connection_id: request.connection.connection_id.clone(),
            kind: observation.source.kind,
            external_id: observation.source.external_id,
        };
        let target = ResourceKey {
            connection_id: request.connection.connection_id.clone(),
            kind: observation.target.kind,
            external_id: observation.target.external_id,
        };
        let key = ValidatedRelationKey {
            source,
            target,
            kind: observation.kind,
            evidence_key: observation.evidence_key,
        };
        let evidence = RelationEvidence::Provider {
            connection_id: request.connection.connection_id.clone(),
            sync_run_id: request.sync_run_id.clone(),
            field_path: observation.field_path,
        };
        let fingerprint = fingerprint(&json!({
            "key": &key,
            "evidence": &evidence,
        }))?;
        Ok(ValidatedRelation {
            key,
            evidence,
            observed_at: observation.observed_at,
            fingerprint,
        })
    }
}

fn fingerprint(value: &Value) -> Result<Fingerprint, DomainError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        contract_error(
            ErrorCode::Internal,
            format!("could not serialize normalized value: {error}"),
        )
    })?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Fingerprint::new(encoded)
}

fn scan_for_secrets(value: &Value) -> Result<(), DomainError> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_sensitive_field_name(key) {
                    return Err(contract_error(
                        ErrorCode::InvalidResponse,
                        "resource attributes contain a forbidden secret field",
                    ));
                }
                scan_for_secrets(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                scan_for_secrets(value)?;
            }
        }
        Value::String(value) => {
            scan_text(value)?;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn scan_text(value: &str) -> Result<(), DomainError> {
    let normalized = value.to_ascii_lowercase();
    if normalized.contains("-----begin private key-----") || normalized.starts_with("bearer ") {
        return Err(contract_error(
            ErrorCode::InvalidResponse,
            "normalized text contains a secret sentinel",
        ));
    }
    Ok(())
}

fn is_sensitive_field_name(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    normalized.split('.').any(|segment| {
        matches!(
            segment,
            "token"
                | "password"
                | "secret"
                | "access_key"
                | "access_token"
                | "secret_key"
                | "authorization"
                | "cookie"
                | "private_key"
                | "connection_string"
        )
    })
}

fn project_attributes(
    source: &Map<String, Value>,
    allowed_paths: &BTreeSet<String>,
) -> (Value, u64) {
    let source_value = Value::Object(source.clone());
    let mut projected = Map::new();
    let mut selected_leaves = 0_u64;
    for path in allowed_paths {
        let segments = path.split('.').collect::<Vec<_>>();
        if let Some(value) = value_at_path(&source_value, &segments) {
            selected_leaves += count_leaf_fields(value);
            insert_at_path(&mut projected, &segments, value.clone());
        }
    }
    let total_leaves = count_leaf_fields(&source_value);
    (
        Value::Object(projected),
        total_leaves.saturating_sub(selected_leaves),
    )
}

fn value_at_path<'a>(value: &'a Value, segments: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in segments {
        current = current.as_object()?.get(*segment)?;
    }
    Some(current)
}

fn insert_at_path(target: &mut Map<String, Value>, segments: &[&str], value: Value) {
    if let Some((first, rest)) = segments.split_first() {
        if rest.is_empty() {
            target.insert((*first).to_owned(), value);
            return;
        }
        let entry = target
            .entry((*first).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(object) = entry.as_object_mut() {
            insert_at_path(object, rest, value);
        }
    }
}

fn count_leaf_fields(value: &Value) -> u64 {
    match value {
        Value::Object(object) => object.values().map(count_leaf_fields).sum(),
        Value::Array(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

fn contract_error(code: ErrorCode, message: impl Into<String>) -> DomainError {
    DomainError {
        code,
        message: message.into(),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use next_infra_connector_api::{
        ConnectionInput, ProviderRequestSummary, RedactionReport, ResourceLocator,
    };

    fn schema() -> SchemaVersion {
        SchemaVersion::new(1).unwrap()
    }

    fn request() -> SyncRequest {
        SyncRequest {
            sync_run_id: SyncRunId::new("fixture-run").unwrap(),
            connection: ConnectionInput {
                connection_id: ConnectionId::new("fixture-connection").unwrap(),
                connector_type: ConnectorType::new("fixture").unwrap(),
                config: json!({}),
                config_schema_version: schema(),
            },
            mode: SyncMode::Full,
            scope: Scope::new("fixture-scope").unwrap(),
            cursor: None,
            targeted_resources: Vec::new(),
        }
    }

    fn resource(external_id: &str, attributes: Value) -> ResourceObservation {
        ResourceObservation {
            kind: ResourceKind::new("fixture.resource").unwrap(),
            external_id: ExternalId::new(external_id).unwrap(),
            name: external_id.into(),
            display_name: external_id.into(),
            scope: Scope::new("fixture-scope").unwrap(),
            labels: BTreeMap::new(),
            health: ResourceHealth::Healthy,
            attributes,
            attribute_schema_version: schema(),
            observed_at: Timestamp::from_unix_millis(1).unwrap(),
        }
    }

    fn batch(resources: Vec<ResourceObservation>) -> ObservationBatch {
        ObservationBatch {
            resources,
            relations: Vec::new(),
            coverage: SyncCoverage::AuthoritativeFull {
                scope: Scope::new("fixture-scope").unwrap(),
            },
            next_cursor: None,
            warnings: Vec::new(),
            redaction_report: RedactionReport::default(),
            provider_request_summary: ProviderRequestSummary::default(),
        }
    }

    fn normalizer() -> Normalizer {
        Normalizer::new(
            [AttributeSchema {
                kind: ResourceKind::new("fixture.resource").unwrap(),
                schema_version: schema(),
                allowed_attributes: BTreeSet::from(["state".into()]),
            }],
            [RelationSchema {
                kind: RelationKind::new("fixture.depends_on").unwrap(),
                source_kind: ResourceKind::new("fixture.resource").unwrap(),
                target_kind: ResourceKind::new("fixture.resource").unwrap(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn input_order_does_not_change_resource_order_or_fingerprints() {
        let first = normalizer()
            .normalize(
                &request(),
                batch(vec![
                    resource("b", json!({"state": "ready"})),
                    resource("a", json!({"state": "ready"})),
                ]),
            )
            .unwrap();
        let second = normalizer()
            .normalize(
                &request(),
                batch(vec![
                    resource("a", json!({"state": "ready"})),
                    resource("b", json!({"state": "ready"})),
                ]),
            )
            .unwrap();

        assert_eq!(first.resources, second.resources);
        assert_eq!(first.resources[0].key.external_id.as_str(), "a");
    }

    #[test]
    fn unknown_fields_are_dropped_before_fingerprinting() {
        let normalized = normalizer()
            .normalize(
                &request(),
                batch(vec![resource(
                    "a",
                    json!({"state": "ready", "unknown": "discarded"}),
                )]),
            )
            .unwrap();

        assert_eq!(
            normalized.resources[0].attributes,
            json!({"state": "ready"})
        );
        assert_eq!(normalized.redaction_report.unknown_fields_dropped, 1);
    }

    #[test]
    fn secret_fields_and_values_are_rejected_even_when_not_allowlisted() {
        for attributes in [
            json!({"state": "ready", "token": "fixture-secret"}),
            json!({"state": "Bearer fixture-secret"}),
        ] {
            assert!(
                normalizer()
                    .normalize(&request(), batch(vec![resource("a", attributes)]))
                    .is_err()
            );
        }
    }

    #[test]
    fn dotted_allowlist_drops_unknown_nested_fields() {
        let nested = Normalizer::new(
            [AttributeSchema {
                kind: ResourceKind::new("fixture.resource").unwrap(),
                schema_version: schema(),
                allowed_attributes: BTreeSet::from(["nested.allowed".into()]),
            }],
            Vec::<RelationSchema>::new(),
        )
        .unwrap();
        let normalized = nested
            .normalize(
                &request(),
                batch(vec![resource(
                    "a",
                    json!({"nested": {"allowed": 1, "unknown": 2}}),
                )]),
            )
            .unwrap();

        assert_eq!(
            normalized.resources[0].attributes,
            json!({"nested": {"allowed": 1}})
        );
        assert_eq!(normalized.redaction_report.unknown_fields_dropped, 1);
    }

    #[test]
    fn relation_endpoint_schema_is_enforced() {
        let mut observations = batch(vec![resource("a", json!({"state": "ready"}))]);
        observations.relations.push(RelationObservation {
            source: ResourceLocator {
                kind: ResourceKind::new("fixture.resource").unwrap(),
                external_id: ExternalId::new("a").unwrap(),
            },
            target: ResourceLocator {
                kind: ResourceKind::new("fixture.other").unwrap(),
                external_id: ExternalId::new("b").unwrap(),
            },
            kind: RelationKind::new("fixture.depends_on").unwrap(),
            evidence_key: EvidenceKey::new("fixture-evidence").unwrap(),
            field_path: FieldPath::new("attributes.target").unwrap(),
            observed_at: Timestamp::from_unix_millis(1).unwrap(),
        });

        assert!(normalizer().normalize(&request(), observations).is_err());
    }
}
