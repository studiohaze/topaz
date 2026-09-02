//! Stage 1 response admission and its lowered and generated projections.
//! A decoded response is shared across the two product views; request execution
//! remains owned by the surrounding front-end session.

mod generated;
mod lowered;
mod response;

pub use generated::*;
pub use lowered::*;
pub use response::*;
