//! Versioned read-only connector contracts for Next Infra.

mod connector;
mod descriptor;
mod error;
mod observation;

pub use connector::*;
pub use descriptor::*;
pub use error::*;
pub use observation::*;

pub const CONNECTOR_API_SCHEMA_VERSION: u32 = 1;
