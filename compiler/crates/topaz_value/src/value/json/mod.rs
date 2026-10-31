//! JSON parsing, value codecs, ABI projection, and typed schema lowering.
//! Syntax admission is independent of Topaz type projection; the value facade
//! exposes both through this shared runtime boundary.

mod abi;
mod codec;
mod parser;
mod schema;

pub use abi::*;
pub use codec::*;
pub use parser::*;
pub use schema::*;
