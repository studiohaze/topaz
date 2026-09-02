//! Module-level Rust emission for imports, defaults, exports, and entry wiring.
//! Statement and expression bodies remain with their own emitters; this facade
//! assembles their output in canonical module order.

mod defaults;
mod entry;
mod exports;
mod imports;

pub(crate) use defaults::*;
pub(crate) use entry::*;
pub(crate) use exports::*;
pub(crate) use imports::*;
