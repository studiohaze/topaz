//! Canonical compiler observations from tokens through completed products.
//! Builders own each layer's projection, while validation and comparison consume
//! the same schema-bound bundle exposed by the kernel facade.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use topaz_diag::{Diagnostic, Severity, Span};
use topaz_syntax::ast::*;
use topaz_syntax::{DurationUnit, Token, TokenKind};
use topaz_value::JsonValue;

use crate::canonical::{array, boolean, encode, encode_jsonl, object, signed, string, unsigned};
use crate::{KernelExecution, KernelOutcome, KernelRequest, KernelUnit};

pub const SOURCE_SET_SCHEMA: &str = "topaz.compiler.source-set/v1";
pub const TOKENS_SCHEMA: &str = "topaz.compiler.tokens/v1";
pub const AST_SCHEMA: &str = "topaz.compiler.ast/v1";
pub const RESOLVED_SCHEMA: &str = "topaz.compiler.resolved/v1";
pub const TYPED_SCHEMA: &str = "topaz.compiler.typed/v1";
pub const LOWERED_SCHEMA: &str = "topaz.compiler.lowered/v1";
pub const RUST_SOURCE_SCHEMA: &str = "topaz.compiler.rust-source/v1";
pub const DIAGNOSTICS_SCHEMA: &str = "topaz.compiler.diagnostics/v1";
pub const STAGE1_PRODUCT_SCHEMA: &str = "topaz.compiler.stage1-product/v1";
pub const STAGE2_PRODUCT_SCHEMA: &str = "topaz.compiler.stage2-product/v1";
pub const STAGE2_FIXED_POINT_SCHEMA: &str = "topaz.compiler.stage2-fixed-point/v2";
pub const BUNDLE_SCHEMA: &str = "topaz.compiler.observation-bundle/v1";
pub const COMPARISON_SCHEMA: &str = "topaz.compiler.comparison/v1";
const SCHEMA_REGISTRY: &[u8] = include_bytes!("../../../../contracts/compiler/v1/schemas.json");
const COMPARISON_MISMATCH_LIMIT: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
/// `path` and `bytes` both participate in the bundle's root digest.
pub struct ObservationFile {
    pub path: String,
    pub schema: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Member order is part of the serialized observation contract.
pub struct ObservationBundle {
    pub files: Vec<ObservationFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Path-independent token projection used for compiler agreement.
pub struct CanonicalPreviewToken {
    pub kind: String,
    pub lo: u32,
    pub hi: u32,
    pub synthetic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Diagnostic projection with canonical span and note ordering.
pub struct CanonicalPreviewDiagnostic {
    pub code: String,
    pub message: String,
    pub lo: u32,
    pub hi: u32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Scalar value permitted in canonical AST attributes.
pub enum CanonicalPreviewAstValue {
    String(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Named scalar attribute attached to a canonical AST node.
pub struct CanonicalPreviewAstAttribute {
    pub name: String,
    pub value: CanonicalPreviewAstValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Flattened AST node with stable parent, field, and sibling identity.
pub struct CanonicalPreviewAstNode {
    pub kind: String,
    pub lo: u32,
    pub hi: u32,
    pub parent: Option<u32>,
    pub field: String,
    pub index: u64,
    pub attributes: Vec<CanonicalPreviewAstAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Keeps raw and layout tokens separate so synthetic layout remains observable.
pub struct CanonicalPreviewModule {
    pub identity: String,
    pub path: String,
    pub source: String,
    pub entry: bool,
    pub extern_module: bool,
    pub generated_std: bool,
    pub raw: Vec<CanonicalPreviewToken>,
    pub layout: Vec<CanonicalPreviewToken>,
    pub ast: Vec<CanonicalPreviewAstNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Import edges use module identities rather than source paths.
pub struct CanonicalPreviewImportEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Resolver scope projected with a module-local stable ordinal.
pub struct CanonicalPreviewResolvedScope {
    pub module_index: usize,
    pub ordinal: u32,
    pub parent_ordinal: Option<u32>,
    pub kind: String,
    pub lo: u32,
    pub hi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Imported declarations retain their source target; local declarations leave it unset.
pub struct CanonicalPreviewResolvedDeclaration {
    pub module_index: usize,
    pub scope_ordinal: u32,
    pub name: String,
    pub namespace: String,
    pub declaration_kind: String,
    pub lo: u32,
    pub hi: u32,
    pub exported: bool,
    pub target_module: Option<String>,
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Canonical reference linked to its resolved declaration when available.
pub struct CanonicalPreviewResolvedReference {
    pub module_index: usize,
    pub scope_ordinal: u32,
    pub name: String,
    pub namespace: String,
    pub role: String,
    pub lo: u32,
    pub hi: u32,
    pub target_module_index: Option<usize>,
    pub target_lo: u32,
    pub target_hi: u32,
    pub target_namespace: Option<String>,
    pub target_module: Option<String>,
    pub target_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Exported declaration projected for resolved-output agreement.
pub struct CanonicalPreviewResolvedExport {
    pub module_index: usize,
    pub name: String,
    pub namespace: String,
    pub declaration_lo: u32,
    pub declaration_hi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Resolver diagnostic anchored to a canonical module index.
pub struct CanonicalPreviewResolvedDiagnostic {
    pub module_index: usize,
    pub code: String,
    pub message: String,
    pub lo: u32,
    pub hi: u32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Secondary source label on a canonical checker diagnostic.
pub struct CanonicalPreviewCheckLabel {
    pub module_index: usize,
    pub lo: u32,
    pub hi: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Module indexes replace compiler-private file IDs at the observation boundary.
pub struct CanonicalPreviewCheckDiagnostic {
    pub module_index: usize,
    pub code: String,
    pub message: String,
    pub primary_message: String,
    pub lo: u32,
    pub hi: u32,
    pub secondary: Vec<CanonicalPreviewCheckLabel>,
    pub notes: Vec<String>,
    pub profile_rule: Option<String>,
}

#[derive(Clone, Copy)]
/// Borrowed resolved rows needed to build an observation without cloning payloads.
pub struct ResolvedPreviewObservationInput<'input> {
    pub request: &'input KernelRequest,
    pub modules: &'input [CanonicalPreviewModule],
    pub edges: &'input [CanonicalPreviewImportEdge],
    pub scopes: &'input [CanonicalPreviewResolvedScope],
    pub declarations: &'input [CanonicalPreviewResolvedDeclaration],
    pub references: &'input [CanonicalPreviewResolvedReference],
    pub exports: &'input [CanonicalPreviewResolvedExport],
    pub diagnostics: &'input [CanonicalPreviewResolvedDiagnostic],
}

#[derive(Clone, Copy)]
/// Borrowed typed rows layered over an admitted resolved observation input.
pub struct TypedPreviewObservationInput<'input> {
    pub resolved: ResolvedPreviewObservationInput<'input>,
    pub nodes: &'input [topaz_hir::TypedNode],
    pub calls: &'input [topaz_hir::TypedCall],
    pub captures: &'input [topaz_hir::TypedCapture],
    pub diagnostics: &'input [CanonicalPreviewCheckDiagnostic],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Independently comparable portion of a compiler product.
pub enum ComparisonLayer {
    Semantic,
    GeneratedSource,
    Provenance,
    NativeBinary,
}

impl ComparisonLayer {
    /// Parses the stable command-line spelling of a comparison layer.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "semantic" => Some(Self::Semantic),
            "generated-source" => Some(Self::GeneratedSource),
            "provenance" => Some(Self::Provenance),
            "native-binary" => Some(Self::NativeBinary),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::GeneratedSource => "generated-source",
            Self::Provenance => "provenance",
            Self::NativeBinary => "native-binary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Canonical comparison result and its serialized receipt bytes.
pub struct ComparisonRecord {
    pub equal: bool,
    pub first_failing_phase: Option<String>,
    pub mismatch_count: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
struct SourceIdentity {
    source_id: String,
    module: String,
    path: String,
    ordinal: u64,
}

fn sha256(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut output = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

fn source_id(module: &str, path: &str) -> String {
    let mut identity = String::from("root");
    identity.push('\0');
    identity.push_str(module);
    identity.push('\0');
    identity.push_str(path);
    let mut output = String::from("s:");
    let digest = topaz_value::value::sha256(identity.as_bytes());
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

fn span(source_id: &str, value: Span) -> JsonValue {
    object([
        ("hi", unsigned(u64::from(value.hi))),
        ("lo", unsigned(u64::from(value.lo))),
        ("sourceId", string(source_id)),
    ])
}

mod bundle;
mod comparison;
mod lowered;
mod resolved;
mod tokens_ast;
mod typed;
mod validate;

#[cfg(test)]
mod tests;

/// Lowered and generated inputs needed to finish a compiler-product observation.
pub struct CompilerPreviewCompletion<'input> {
    pub lowered_jsonl: Vec<u8>,
    pub generated_rust: &'input str,
    pub product: Vec<u8>,
    pub runtime_template_identity: &'input str,
    pub runtime_template_sha256: &'input str,
    pub producer_stage: u8,
    pub fixed_point: Option<Vec<u8>>,
}

pub use bundle::{
    build_ast_preview_observation, build_observation, build_token_preview_observation,
};
pub use comparison::{compare_native_binaries, compare_observations};
pub use lowered::complete_compiler_preview_observation;
pub use resolved::build_resolved_preview_observation;
pub use tokens_ast::canonical_token_kind;
pub use typed::{build_typed_preview_observation, semantic_type_atoms_json};

#[cfg(test)]
pub(crate) use comparison::refresh_test_manifest;
pub(crate) use tokens_ast::front_end_counts;
