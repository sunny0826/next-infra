mod dto;
mod mapper;

pub use dto::*;
pub use mapper::*;

pub const MAX_WORKFLOWS_PER_REPOSITORY: usize = 200;
pub const MAX_RUNS_PER_REPOSITORY: usize = 100;
pub const MAX_JOBS_PER_RUN: usize = 200;
pub const MAX_JOBS_PER_REPOSITORY: usize = 2_000;
