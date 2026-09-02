//! Decoder and data model for the Python runtime trace wire format.
//! JSON syntax handling is isolated from trace validation; runners receive only
//! the typed projection re-exported here.

mod decode;
mod json;
mod model;

pub(crate) use decode::parse_trace_v1;
#[cfg(test)]
pub(crate) use model::TraceFault;
pub(crate) use model::{PyTrace, TraceFile, TraceValue};
