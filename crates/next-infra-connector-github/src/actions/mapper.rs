use super::{
    JobDto, MAX_JOBS_PER_RUN, MAX_RUNS_PER_REPOSITORY, MAX_WORKFLOWS_PER_REPOSITORY, WorkflowDto,
    WorkflowRunDto,
};
use next_infra_connector_api::{
    ConnectorFailure, ObservationWarning, RelationObservation, ResourceLocator, ResourceObservation,
};
use next_infra_core::{
    ErrorCode, EvidenceKey, ExternalId, FieldPath, LabelKey, RelationKind, ResourceHealth,
    ResourceKind, SchemaVersion, Scope, Timestamp,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubRepositoryContext {
    pub repository_external_id: ExternalId,
    pub scope: Scope,
    pub observed_at: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionModuleState {
    Complete,
    Partial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionModuleResult {
    pub module: &'static str,
    pub collected: usize,
    pub bounded: bool,
    pub state: ActionModuleState,
    pub failure: Option<ConnectorFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionMapperOutput {
    pub resources: Vec<ResourceObservation>,
    pub relations: Vec<RelationObservation>,
    pub modules: Vec<ActionModuleResult>,
    pub warnings: Vec<ObservationWarning>,
}

impl ActionMapperOutput {
    pub fn merge(mut self, mut other: Self) -> Self {
        self.resources.append(&mut other.resources);
        self.relations.append(&mut other.relations);
        self.modules.append(&mut other.modules);
        self.warnings.append(&mut other.warnings);
        sort_output(&mut self);
        self
    }
}

pub fn map_workflows(
    context: &GitHubRepositoryContext,
    workflows: impl IntoIterator<Item = WorkflowDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<ActionMapperOutput, ConnectorFailure> {
    let mut workflows = workflows.into_iter().collect::<Vec<_>>();
    let bounded = bounded || workflows.len() > MAX_WORKFLOWS_PER_REPOSITORY;
    workflows.truncate(MAX_WORKFLOWS_PER_REPOSITORY);
    let mut resources = Vec::new();
    let mut relations = Vec::new();
    let mut identities = BTreeSet::new();
    for workflow in workflows {
        validate_required(&workflow.name, "workflow name")?;
        validate_required(&workflow.path, "workflow path")?;
        validate_required(&workflow.state, "workflow state")?;
        validate_required(&workflow.created_at, "workflow created_at")?;
        validate_required(&workflow.updated_at, "workflow updated_at")?;
        let resource_external_id = external_id("github-workflow", workflow.id)?;
        if !identities.insert(resource_external_id.clone()) {
            return Err(invalid("GitHub workflows contain duplicate identities"));
        }
        resources.push(ResourceObservation {
            kind: kind("github.workflow")?,
            external_id: resource_external_id.clone(),
            name: workflow.name.clone(),
            display_name: workflow.name,
            scope: context.scope.clone(),
            labels: labels("workflow", None)?,
            health: ResourceHealth::Unknown,
            attributes: json!({
                "workflow_id": workflow.id,
                "path": workflow.path,
                "state": workflow.state,
                "created_at": workflow.created_at,
                "updated_at": workflow.updated_at,
            }),
            attribute_schema_version: schema_version()?,
            observed_at: context.observed_at,
        });
        relations.push(RelationObservation {
            source: ResourceLocator {
                kind: kind("github.repository")?,
                external_id: context.repository_external_id.clone(),
            },
            target: ResourceLocator {
                kind: kind("github.workflow")?,
                external_id: resource_external_id,
            },
            kind: relation_kind("github.contains")?,
            evidence_key: evidence("github-provider-workflow", workflow.id)?,
            field_path: field("attributes.workflow_id")?,
            observed_at: context.observed_at,
        });
    }
    output(
        "github.actions.workflows",
        resources,
        relations,
        bounded,
        failure,
    )
}

pub fn map_runs(
    context: &GitHubRepositoryContext,
    runs: impl IntoIterator<Item = WorkflowRunDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<ActionMapperOutput, ConnectorFailure> {
    let mut runs = runs.into_iter().collect::<Vec<_>>();
    let bounded = bounded || runs.len() > MAX_RUNS_PER_REPOSITORY;
    runs.truncate(MAX_RUNS_PER_REPOSITORY);
    let mut resources = Vec::new();
    let mut relations = Vec::new();
    let mut identities = BTreeSet::new();
    for run in runs {
        validate_optional(run.name.as_deref(), "run name")?;
        validate_required(&run.display_title, "run display title")?;
        validate_required(&run.event, "run event")?;
        validate_required(&run.status, "run status")?;
        validate_optional(run.conclusion.as_deref(), "run conclusion")?;
        validate_optional(run.head_branch.as_deref(), "run branch")?;
        validate_required(&run.created_at, "run created_at")?;
        validate_required(&run.updated_at, "run updated_at")?;
        validate_optional(run.run_started_at.as_deref(), "run started_at")?;
        let resource_external_id = external_id("github-run", run.id)?;
        if !identities.insert(resource_external_id.clone()) {
            return Err(invalid("GitHub workflow runs contain duplicate identities"));
        }
        let health = action_health(&run.status, run.conclusion.as_deref());
        resources.push(ResourceObservation {
            kind: kind("github.workflow_run")?,
            external_id: resource_external_id.clone(),
            name: format!("run-{}", run.run_number),
            display_name: run.display_title,
            scope: context.scope.clone(),
            labels: labels("workflow_run", Some(&run.status))?,
            health,
            attributes: json!({
                "run_id": run.id,
                "workflow_id": run.workflow_id,
                "run_number": run.run_number,
                "run_attempt": run.run_attempt,
                "event": run.event,
                "status": run.status,
                "conclusion": run.conclusion,
                "head_branch": run.head_branch,
                "created_at": run.created_at,
                "updated_at": run.updated_at,
                "run_started_at": run.run_started_at,
            }),
            attribute_schema_version: schema_version()?,
            observed_at: context.observed_at,
        });
        relations.push(RelationObservation {
            source: ResourceLocator {
                kind: kind("github.workflow")?,
                external_id: external_id("github-workflow", run.workflow_id)?,
            },
            target: ResourceLocator {
                kind: kind("github.workflow_run")?,
                external_id: resource_external_id,
            },
            kind: relation_kind("github.executes")?,
            evidence_key: evidence("github-provider-run", run.id)?,
            field_path: field("attributes.workflow_id")?,
            observed_at: context.observed_at,
        });
    }
    output(
        "github.actions.runs",
        resources,
        relations,
        bounded,
        failure,
    )
}

pub fn map_jobs(
    context: &GitHubRepositoryContext,
    jobs: impl IntoIterator<Item = JobDto>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<ActionMapperOutput, ConnectorFailure> {
    let mut jobs = jobs.into_iter().collect::<Vec<_>>();
    let bounded = bounded || jobs.len() > MAX_JOBS_PER_RUN;
    jobs.truncate(MAX_JOBS_PER_RUN);
    let mut resources = Vec::new();
    let mut relations = Vec::new();
    let mut identities = BTreeSet::new();
    for job in jobs {
        validate_required(&job.name, "job name")?;
        validate_required(&job.status, "job status")?;
        validate_optional(job.conclusion.as_deref(), "job conclusion")?;
        validate_optional(job.started_at.as_deref(), "job started_at")?;
        validate_optional(job.completed_at.as_deref(), "job completed_at")?;
        let resource_external_id = external_id("github-job", job.id)?;
        if !identities.insert(resource_external_id.clone()) {
            return Err(invalid("GitHub jobs contain duplicate identities"));
        }
        resources.push(ResourceObservation {
            kind: kind("github.workflow_job")?,
            external_id: resource_external_id.clone(),
            name: job.name.clone(),
            display_name: job.name,
            scope: context.scope.clone(),
            labels: labels("workflow_job", Some(&job.status))?,
            health: action_health(&job.status, job.conclusion.as_deref()),
            attributes: json!({
                "job_id": job.id,
                "run_id": job.run_id,
                "status": job.status,
                "conclusion": job.conclusion,
                "started_at": job.started_at,
                "completed_at": job.completed_at,
            }),
            attribute_schema_version: schema_version()?,
            observed_at: context.observed_at,
        });
        relations.push(RelationObservation {
            source: ResourceLocator {
                kind: kind("github.workflow_run")?,
                external_id: external_id("github-run", job.run_id)?,
            },
            target: ResourceLocator {
                kind: kind("github.workflow_job")?,
                external_id: resource_external_id,
            },
            kind: relation_kind("github.contains")?,
            evidence_key: evidence("github-provider-job", job.id)?,
            field_path: field("attributes.run_id")?,
            observed_at: context.observed_at,
        });
    }
    output(
        "github.actions.jobs",
        resources,
        relations,
        bounded,
        failure,
    )
}

fn output(
    module: &'static str,
    resources: Vec<ResourceObservation>,
    relations: Vec<RelationObservation>,
    bounded: bool,
    failure: Option<ConnectorFailure>,
) -> Result<ActionMapperOutput, ConnectorFailure> {
    let collected = resources.len();
    let state = if bounded || failure.is_some() {
        ActionModuleState::Partial
    } else {
        ActionModuleState::Complete
    };
    let mut output = ActionMapperOutput {
        resources,
        relations,
        modules: vec![ActionModuleResult {
            module,
            collected,
            bounded,
            state,
            failure,
        }],
        warnings: Vec::new(),
    };
    sort_output(&mut output);
    Ok(output)
}

fn sort_output(output: &mut ActionMapperOutput) {
    output
        .resources
        .sort_by_key(|resource| (resource.kind.clone(), resource.external_id.clone()));
    output.relations.sort_by_key(|relation| {
        (
            relation.source.kind.clone(),
            relation.source.external_id.clone(),
            relation.target.kind.clone(),
            relation.target.external_id.clone(),
            relation.kind.clone(),
            relation.evidence_key.clone(),
        )
    });
    output.modules.sort_by_key(|module| module.module);
}

fn labels(
    resource_type: &str,
    status: Option<&str>,
) -> Result<BTreeMap<LabelKey, String>, ConnectorFailure> {
    let mut labels = BTreeMap::new();
    labels.insert(label_key("github.resource_type")?, resource_type.into());
    if let Some(status) = status {
        labels.insert(label_key("github.status")?, status.into());
    }
    Ok(labels)
}

fn action_health(status: &str, conclusion: Option<&str>) -> ResourceHealth {
    if status != "completed" {
        return ResourceHealth::Unknown;
    }
    match conclusion {
        Some("success") => ResourceHealth::Healthy,
        Some("failure" | "timed_out" | "startup_failure" | "action_required") => {
            ResourceHealth::Unhealthy
        }
        Some("cancelled" | "stale") => ResourceHealth::Degraded,
        Some("neutral" | "skipped") | None | Some(_) => ResourceHealth::Unknown,
    }
}

fn validate_required(value: &str, field_name: &str) -> Result<(), ConnectorFailure> {
    if value.is_empty() {
        return Err(invalid(format!("GitHub {field_name} is empty")));
    }
    validate_text(value, field_name)
}

fn validate_optional(value: Option<&str>, field_name: &str) -> Result<(), ConnectorFailure> {
    if let Some(value) = value {
        validate_text(value, field_name)?;
    }
    Ok(())
}

fn validate_text(value: &str, field_name: &str) -> Result<(), ConnectorFailure> {
    let normalized = value.to_ascii_lowercase();
    if value.len() > 1024
        || value.chars().any(char::is_control)
        || normalized.starts_with("bearer ")
        || normalized.contains("-----begin private key-----")
    {
        return Err(invalid(format!(
            "GitHub {field_name} is unsafe or too long"
        )));
    }
    Ok(())
}

fn external_id(prefix: &str, id: u64) -> Result<ExternalId, ConnectorFailure> {
    ExternalId::new(format!("{prefix}:{id}")).map_err(domain_failure)
}

fn evidence(prefix: &str, id: u64) -> Result<EvidenceKey, ConnectorFailure> {
    EvidenceKey::new(format!("{prefix}:{id}")).map_err(domain_failure)
}

fn kind(value: &str) -> Result<ResourceKind, ConnectorFailure> {
    ResourceKind::new(value).map_err(domain_failure)
}

fn relation_kind(value: &str) -> Result<RelationKind, ConnectorFailure> {
    RelationKind::new(value).map_err(domain_failure)
}

fn label_key(value: &str) -> Result<LabelKey, ConnectorFailure> {
    LabelKey::new(value).map_err(domain_failure)
}

fn field(value: &str) -> Result<FieldPath, ConnectorFailure> {
    FieldPath::new(value).map_err(domain_failure)
}

fn schema_version() -> Result<SchemaVersion, ConnectorFailure> {
    SchemaVersion::new(1).map_err(domain_failure)
}

fn domain_failure(_: next_infra_core::DomainError) -> ConnectorFailure {
    invalid("GitHub mapper produced an invalid domain value")
}

fn invalid(message: impl Into<String>) -> ConnectorFailure {
    ConnectorFailure {
        code: ErrorCode::InvalidResponse,
        message: message.into(),
        retryable: false,
        retry_after_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn context() -> GitHubRepositoryContext {
        GitHubRepositoryContext {
            repository_external_id: ExternalId::new("github-repository:10").unwrap(),
            scope: Scope::new("github-repository-scope:10").unwrap(),
            observed_at: Timestamp::from_unix_millis(1_000).unwrap(),
        }
    }

    fn run(id: u64, attempt: u64, status: &str, conclusion: Option<&str>) -> WorkflowRunDto {
        WorkflowRunDto {
            id,
            workflow_id: 20,
            name: Some("Fixture workflow".into()),
            display_title: "Fixture run".into(),
            run_number: 3,
            run_attempt: attempt,
            event: "push".into(),
            status: status.into(),
            conclusion: conclusion.map(str::to_owned),
            head_branch: Some("fixture-branch".into()),
            created_at: "2026-08-05T00:00:00Z".into(),
            updated_at: "2026-08-05T00:01:00Z".into(),
            run_started_at: Some("2026-08-05T00:00:10Z".into()),
        }
    }

    #[test]
    fn run_identity_and_evidence_survive_rerun_attempts() {
        let first = map_runs(
            &context(),
            [run(30, 1, "completed", Some("success"))],
            false,
            None,
        )
        .unwrap();
        let second = map_runs(
            &context(),
            [run(30, 2, "completed", Some("success"))],
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            first.resources[0].external_id,
            second.resources[0].external_id
        );
        assert_eq!(
            first.relations[0].evidence_key,
            second.relations[0].evidence_key
        );
        assert_ne!(
            first.resources[0].attributes,
            second.resources[0].attributes
        );
    }

    #[test]
    fn health_mapping_is_conservative() {
        for (status, conclusion, expected) in [
            ("completed", Some("success"), ResourceHealth::Healthy),
            ("completed", Some("failure"), ResourceHealth::Unhealthy),
            ("completed", Some("cancelled"), ResourceHealth::Degraded),
            ("in_progress", None, ResourceHealth::Unknown),
            ("completed", Some("future"), ResourceHealth::Unknown),
        ] {
            let output =
                map_runs(&context(), [run(30, 1, status, conclusion)], false, None).unwrap();
            assert_eq!(output.resources[0].health, expected);
        }
    }

    #[test]
    fn unknown_dto_fields_and_job_steps_do_not_survive() {
        let value: Value = serde_json::from_str(
            r#"{"total_count":1,"jobs":[{"id":40,"run_id":30,"name":"Fixture job","status":"completed","conclusion":"success","started_at":null,"completed_at":null,"steps":[{"name":"bearer secret-sentinel"}],"runner_name":"private-runner","authorization":"secret-sentinel"}]}"#,
        )
        .unwrap();
        let list: crate::actions::JobListDto = serde_json::from_value(value).unwrap();
        let output = map_jobs(&context(), list.jobs, false, None).unwrap();
        let serialized = serde_json::to_string(&output.resources)
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in [
            "secret-sentinel",
            "private-runner",
            "steps",
            "authorization",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn input_order_does_not_change_sorted_output() {
        let left = map_runs(
            &context(),
            [run(31, 1, "queued", None), run(30, 1, "queued", None)],
            false,
            None,
        )
        .unwrap();
        let right = map_runs(
            &context(),
            [run(30, 1, "queued", None), run(31, 1, "queued", None)],
            false,
            None,
        )
        .unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn bounded_or_failed_modules_are_partial() {
        let failure = ConnectorFailure {
            code: ErrorCode::RateLimited,
            message: "GitHub rate limit prevented the request".into(),
            retryable: true,
            retry_after_ms: Some(1_000),
        };
        let output = map_runs(
            &context(),
            [run(30, 1, "queued", None)],
            true,
            Some(failure),
        )
        .unwrap();
        assert_eq!(output.modules[0].state, ActionModuleState::Partial);
        assert!(output.modules[0].bounded);
        assert!(output.modules[0].failure.is_some());
    }

    #[test]
    fn run_budget_is_enforced_by_the_mapper() {
        let runs = (0..=super::MAX_RUNS_PER_REPOSITORY)
            .map(|offset| run(1_000 + u64::try_from(offset).unwrap(), 1, "queued", None))
            .collect::<Vec<_>>();
        let output = map_runs(&context(), runs, false, None).unwrap();
        assert_eq!(output.resources.len(), super::MAX_RUNS_PER_REPOSITORY);
        assert!(output.modules[0].bounded);
        assert_eq!(output.modules[0].state, ActionModuleState::Partial);
    }

    #[test]
    fn duplicate_id_and_secret_sentinel_are_rejected() {
        assert!(
            map_runs(
                &context(),
                [run(30, 1, "queued", None), run(30, 1, "queued", None)],
                false,
                None
            )
            .is_err()
        );
        let mut unsafe_run = run(30, 1, "queued", None);
        unsafe_run.display_title = "Bearer fixture-secret".into();
        assert!(map_runs(&context(), [unsafe_run], false, None).is_err());
    }
}
