//! Resolved and typed projections from the embedded Topaz front end.
//! Both paths share session and request ownership outside this module and expose
//! typed observation inputs through the crate facade.

mod resolved;
mod typed;

pub use resolved::*;
pub use typed::*;
