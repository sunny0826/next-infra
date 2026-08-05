//! Read-only GitHub REST transport and descriptor for Next Infra.

mod auth;
mod client;
mod descriptor;
mod error;
mod transport;

pub use client::*;
pub use descriptor::*;
pub use error::*;
pub use transport::*;

pub const GITHUB_API_VERSION: &str = "2026-03-10";
pub const GITHUB_API_ORIGIN: &str = "https://api.github.com";
