//! End-to-end tests of the `topaz` developer driver (CDR-001 §8
//! item 11), driving the built binary directly.

#[path = "cli/support.rs"]
mod support;

#[path = "cli/backends.rs"]
mod backends;
#[path = "cli/compiler.rs"]
mod compiler;
#[path = "cli/execution.rs"]
mod execution;
#[path = "cli/exports.rs"]
mod exports;
#[path = "cli/fmt_refactor.rs"]
mod fmt_refactor;
#[path = "cli/init.rs"]
mod init;
#[path = "cli/install.rs"]
mod install;
#[path = "cli/lsp.rs"]
mod lsp;
#[path = "cli/package.rs"]
mod package;
#[path = "cli/profiles.rs"]
mod profiles;
#[path = "cli/stdlib.rs"]
mod stdlib;
#[path = "cli/vendor.rs"]
mod vendor;
