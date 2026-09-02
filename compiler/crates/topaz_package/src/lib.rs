//! Topaz package manifests and lockfiles.
//!
//! This crate is intentionally semantic-only: it parses and validates
//! `topaz.toml` / `topaz.lock`, but it does not resolve modules or lower code.
//! The CLI feeds the validated entry/root back into the existing compiler front
//! end so package mode cannot drift from explicit-entry mode.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::{Digest, Sha256};
use topaz_lexer::{is_identifier_continue, is_identifier_start};
use topaz_syntax::{Keyword, LangVersion};
use topaz_value::value::sha256;
use topaz_value::{TomlValue, bytes_to_hex_into, toml_parse_document};
mod content;
mod lock;
mod manifest;
mod model;
mod project;
mod strict_io;
mod vendor;

pub use content::{package_content_hash, package_content_relative_path};
pub use lock::{
    check_lock, parse_lock_lispex, render_lockfile, render_lockfile_with_lispex,
    verify_lispex_lock_declarations, verify_lock_text,
};
pub use manifest::{
    manifest_sha256, parse_abi_type, parse_manifest, validate_lispex_application_binding,
};
pub use model::*;
pub use strict_io::{
    read_extern_replay_fixture, read_package_file_strict, read_package_text_strict,
    replace_package_file_strict,
};
pub use vendor::{
    RegistryVendorReplacement, registry_vendor_root, replace_registry_vendor_package,
};

pub(crate) use content::read_package_content;
pub(crate) use manifest::{
    parse_manifest_with_build_policy, validate_package_name, validate_package_version,
    validate_sha256_hash,
};
pub(crate) use strict_io::{extern_artifact_hash, extern_replay_hash};
pub(crate) use vendor::{verify_path_dependency_content, verify_registry_dependency_content};

#[cfg(test)]
mod tests;
