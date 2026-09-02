//! Private execution foundation for a future bounded Topaz execution surface.
//!
//! This crate is intentionally not a `topaz` command and is not part of the
//! default workspace build. It keeps three facts separate:
//!
//! - [`protocol`] defines the source-free private pipe contract.
//! - [`worker`] checks and evaluates one request with `NoCapabilityHost`.
//! - [`sandbox`] selects and records an operating-system sandbox backend.
//!
//! Resource completeness and packaged-product behavior are verified by their
//! dedicated integration suites.

pub mod protocol;
pub mod sandbox;
pub mod worker;
