//! Read-only Dokploy descriptor and credential boundary.
//!
//! The connector deliberately excludes database resources until a separate
//! allowlist and credential review expands DEC-G8-01.

mod auth;
mod client;
mod connector;
mod descriptor;
mod mapper;
mod transport;

pub use auth::*;
pub use client::*;
pub use connector::*;
pub use descriptor::*;
pub use mapper::*;
pub use transport::*;
