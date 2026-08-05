use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WorkflowListDto {
    pub total_count: u64,
    pub workflows: Vec<WorkflowDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WorkflowDto {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WorkflowRunListDto {
    pub total_count: u64,
    pub workflow_runs: Vec<WorkflowRunDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WorkflowRunDto {
    pub id: u64,
    pub workflow_id: u64,
    pub name: Option<String>,
    pub display_title: String,
    pub run_number: u64,
    pub run_attempt: u64,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub head_branch: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub run_started_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct JobListDto {
    pub total_count: u64,
    pub jobs: Vec<JobDto>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct JobDto {
    pub id: u64,
    pub run_id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}
