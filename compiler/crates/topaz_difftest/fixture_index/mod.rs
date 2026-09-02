//! Explicit fixture catalogs in their observable execution order.
//! Boxed programs and module packages keep separate indexes; `build.rs` is the
//! only consumer of the combined lists.

mod boxed;
mod modules;

pub(super) use boxed::FIXTURES;
pub(super) use modules::{EXTERN_MODULE_FIXTURES, MODULE_FIXTURES, VERSIONED_MODULE_FIXTURES};
