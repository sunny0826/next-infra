//! Read-only GitHub REST transport and descriptor for Next Infra.

pub mod actions;
mod auth;
mod client;
mod connector;
pub mod deployment;
mod descriptor;
pub mod environment;
mod error;
pub mod repository;
mod transport;

pub use client::*;
pub use connector::*;
pub use descriptor::*;
pub use error::*;
pub use transport::*;

pub const GITHUB_API_VERSION: &str = "2026-03-10";
pub const GITHUB_API_ORIGIN: &str = "https://api.github.com";
