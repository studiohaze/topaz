//! Closure lowering across capture discovery, factory emission, and flow state.
//! The expression emitter reaches these leaves through this crate-private
//! boundary; runtime callable representation stays outside it.

mod capture;
mod factory;
mod flow;

pub(crate) use capture::*;
pub(crate) use factory::*;
pub(crate) use flow::*;
