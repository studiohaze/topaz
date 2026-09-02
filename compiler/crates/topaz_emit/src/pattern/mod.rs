//! Pattern emission for destructuring bindings and match decisions.
//! Type formation belongs to the checked input; these leaves only translate the
//! selected pattern plan into Rust control flow.

mod destructure;
mod r#match;

pub(crate) use destructure::*;
pub(crate) use r#match::*;
