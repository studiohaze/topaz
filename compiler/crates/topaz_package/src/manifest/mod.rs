//! Manifest decoding across ABI types, section rules, and root assembly.
//! Section readers remain internal; the package crate exposes only admitted
//! manifests and the established application-binding checks.

mod abi;
pub(crate) mod parse;
mod sections;

pub use abi::parse_abi_type;
use abi::{is_ident_continue, is_ident_start, parse_abi_type_field};
pub(crate) use parse::parse_manifest_with_build_policy;
pub use parse::{manifest_sha256, parse_manifest, validate_lispex_application_binding};
pub(crate) use sections::*;
