//! Ordered Python-wide and multi-module fixture catalogs.
//! The lists name checked-in source trees explicitly, preserving harness order
//! without turning directory discovery into product policy.

mod modules;
mod wide;

pub(crate) use modules::{MODULE_FIXTURES, SERVER_CONTRACT_DEMO};
pub(crate) use wide::WIDE_FIXTURES;
