//! Build-time fixture preparation shared by the differential harness.
//! Model and rendering stay separate from source lookup so `build.rs` owns only
//! catalog traversal and generated-module assembly.

pub(super) mod model;
mod provider;
pub(super) mod render;
