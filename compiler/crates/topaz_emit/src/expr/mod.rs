//! Expression emission split by aggregate, call, control, pipe, and core forms.
//! The parent emitter sees one crate-private surface, while each leaf owns a
//! grammar family and no source analysis policy.

mod aggregate;
mod call;
mod concurrent;
mod control;
mod core;
mod pipe;

pub(crate) use aggregate::*;
pub(crate) use call::*;
pub(crate) use concurrent::*;
pub(crate) use control::*;
pub(crate) use core::*;
pub(crate) use pipe::*;
