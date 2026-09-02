//! Rust type context, schema emission, and runtime type-test lowering.
//! These leaves consume checked type facts and expose a single internal surface
//! to expressions, patterns, and exported-value adapters.

mod context;
mod schema;
mod type_test;

pub(crate) use context::*;
pub(crate) use schema::*;
pub(crate) use type_test::*;
