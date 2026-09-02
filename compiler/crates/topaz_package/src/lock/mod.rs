//! Lockfile parsing, rendering, and verification behind the package facade.
//! The private model preserves exact document structure; callers receive the
//! established parse and consistency-check entry points re-exported here.

mod model;
mod parse;
mod render;
mod verify;

pub(crate) use model::LockPackage;
use model::{LockExtern, ParsedLock};
pub(crate) use parse::parse_lock_document;
pub use parse::parse_lock_lispex;
pub use render::{render_lockfile, render_lockfile_with_lispex};
use verify::verify_extern_artifact_bytes;
pub use verify::{check_lock, verify_lispex_lock_declarations, verify_lock_text};
