//! Statement emission organized by bindings, assignment, loops, and sequencing.
//! Expression rendering is delegated to `expr`; this boundary owns statement
//! order and control-flow placement in generated Rust.

mod assignment;
mod bindings;
mod loops;
mod sequence;

pub(crate) use assignment::*;
pub(crate) use bindings::*;
pub(crate) use loops::*;
pub(crate) use sequence::*;
