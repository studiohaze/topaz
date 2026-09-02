//! Bridge for the checked Topaz-authored front end embedded in the compiler.
//!
//! This crate embeds the exact production Topaz package in the installed
//! binary and invokes only its exported pure exchange step. It does not select
//! a compiler globally and never routes target source through Rust after the
//! preview begins.

use std::rc::Rc;

use topaz_interp::{Machine, TestHost};
use topaz_resolve::{InMemoryProvider, ResolveOutput, resolve};
use topaz_value::{JsonNumber, JsonValue, Value, json_parse, json_stringify};

pub const EXCHANGE_SCHEMA: &str = "topaz.compiler.frontend-preview-exchange/v1";
pub const STAGE1_EXCHANGE_SCHEMA: &str = "topaz.compiler.stage1-exchange/v2";
pub const STAGE1_IR_SCHEMA: &str = "topaz.compiler.stage1-ir/v1";
pub const STAGE1_PROVENANCE_SCHEMA: &str = "topaz.compiler.stage1-provenance/v1";
pub const FIXED_POINT_PAYLOAD_SCHEMA: &str = "topaz.compiler.fixed-point-ir-payload/v1";
pub const FIXED_POINT_RUNTIME_TEMPLATE: &str = "compiler-ir-table/v2";
pub const FIXED_POINT_RUNTIME_TEMPLATE_SHA256: &str =
    "sha256:79b4af8e5544a04eb021fe093dda38ed15b655e8dc56faef151807a8cb925d74";
pub const SELF_COMPILATION_PRODUCT_SCHEMA: &str = "topaz.self-compilation-product/v1";
pub const SELF_TARGET_ADAPTER_FACTS_SCHEMA: &str = "topaz.self-target-adapter-facts/v1";
const SELF_PRODUCT_CATEGORIES: [&str; 8] = [
    "ordered-sources-and-modules",
    "tokens-and-ast",
    "resolution-and-exports",
    "typed-profile-and-diagnostics",
    "lowered-operations-and-runtime-requirements",
    "generated-rust",
    "shared-service-typed-data",
    "complete-provenance",
];
const SELF_PRODUCT_PHASE_TRACE: [&str; 6] = [
    "host.source-and-package-facts",
    "self.c2-front-end",
    "self.c2-profile",
    "self.c2-lowering",
    "self.c2-rust-source",
    "host.mechanical-product-validation",
];
const MAX_AST_NODES: u64 = 2_000_000;
const MAX_AST_DEPTH: u32 = 1_024;

mod json;
mod preview;
mod product;
mod request;
mod session;
mod source_inventory;
mod stage1;
mod target_facts;

pub(crate) use json::*;
pub use preview::*;
pub use product::*;
pub use request::*;
pub use session::*;
pub use source_inventory::*;
pub use stage1::*;
pub use target_facts::*;

#[cfg(test)]
mod tests;
