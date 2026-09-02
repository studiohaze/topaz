//! Completed self-compilation products and their manifest boundaries.
//! Construction, validation, execution, and observation stay separate, then are
//! re-exported as the crate's product-facing API.

mod execution;
mod manifest;
mod model;
mod observation;

pub use execution::*;
pub use manifest::*;
pub use model::*;
pub use observation::*;
