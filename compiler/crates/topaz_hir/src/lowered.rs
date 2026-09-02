//! Backend-neutral structured lowering model.
//!
//! Every operation owns a stable source-derived identity and names its operand
//! operations in evaluation order. Control, cleanup, calls, captures, runtime
//! leaves, and representation evidence are explicit data. The model carries no
//! parser AST, resolver output, source buffer, Rust identifier, or generated
//! source fragment.

use topaz_diag::Span;
use topaz_syntax::LangVersion;

use crate::{
    CallPlan, MonoTy, SemanticType, TypedCall, TypedCapture, TypedUnit,
    emission::{LoweredText, Program},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredUnit {
    pub language_version: LangVersion,
    pub modules: Vec<LoweredModule>,
    pub operations: Vec<LoweredOperation>,
    pub calls: Vec<TypedCall>,
    pub captures: Vec<TypedCapture>,
    pub typed: Option<TypedUnit>,
    pub import_edges: Vec<(String, String)>,
    pub runtime: RuntimeRegistry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredModule {
    pub identity: String,
    pub path: String,
    pub file: topaz_diag::FileId,
    pub initialization_ordinal: u32,
    pub is_entry: bool,
    pub is_extern: bool,
    /// Resolver-owned provenance for compiler-generated package capability
    /// modules. Emitters and interpreters must not infer this from the path.
    pub is_generated_std: bool,
    pub extern_replay_error: Option<String>,
    pub program: Program,
    pub text: LoweredText,
    pub operation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredOperation {
    pub id: String,
    pub module: String,
    pub span: Span,
    pub parent: Option<String>,
    pub role: LoweredRole,
    pub kind: LoweredOperationKind,
    /// Operand operation identities in semantic evaluation order.
    pub operands: Vec<String>,
    pub semantic_type: Option<SemanticType>,
    pub representation: Option<MonoTy>,
    pub binding: Option<LoweredBinding>,
    pub control: Option<LoweredControl>,
    pub call: Option<CallPlan>,
    pub runtime_leaf: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoweredRole {
    ModuleInitialization,
    Statement,
    Expression,
    Pattern,
    Binding,
    Declaration,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredOperationKind {
    Module,
    Import,
    Export,
    Function,
    TypeAlias,
    Enum,
    Record,
    Newtype,
    Protocol,
    Implementation,
    Let,
    Constant,
    Assignment,
    Return,
    Defer,
    Using,
    While,
    Break,
    Continue,
    Expression(LoweredExpressionKind),
    Pattern(LoweredPatternKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredExpressionKind {
    Integer {
        spelling: String,
    },
    Float {
        spelling: String,
    },
    Duration {
        spelling: String,
    },
    Boolean(bool),
    Null,
    Unit,
    String {
        tag: Option<String>,
        multiline: bool,
    },
    Identifier {
        name: String,
        target: Option<String>,
    },
    Placeholder,
    Parenthesized,
    Block,
    If,
    Match,
    For,
    Loop,
    Concurrent,
    Call,
    Member {
        name: String,
        target: Option<String>,
    },
    Index,
    OptionalMember {
        name: String,
        target: Option<String>,
    },
    ResultPropagation,
    Unary {
        operator: String,
    },
    Binary {
        operator: String,
    },
    Range {
        inclusive: bool,
    },
    Compose,
    Pipeline,
    Lambda,
    RecordLiteral,
    RecordUpdate,
    Array,
    Set,
    Map,
    Comprehension {
        collection: String,
    },
    StringText {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredPatternKind {
    Alternatives,
    Wildcard,
    Literal,
    Range { inclusive: bool },
    Binding { name: String },
    TypedBinding { name: String },
    Constructor { name: String },
    List,
    Record,
    NominalRecord { name: String },
    Rest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredBinding {
    pub name: String,
    pub mutable: bool,
    pub storage: LoweredStorage,
    pub declaration_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredStorage {
    Local,
    Module,
    Captured,
    Parameter,
    Temporary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredControl {
    pub kind: LoweredControlKind,
    pub target: Option<String>,
    pub cleanup_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredControlKind {
    Branch,
    Match,
    Loop,
    Break,
    Continue,
    Return,
    Cleanup,
    Propagate,
    Concurrent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRegistry {
    pub schema: String,
    pub leaves: Vec<RuntimeLeaf>,
    pub templates: Vec<RuntimeTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeLeaf {
    pub identity: String,
    pub deterministic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeTemplate {
    pub identity: String,
    pub sha256: String,
}

impl RuntimeRegistry {
    pub fn for_operations(operations: &[LoweredOperation]) -> Self {
        let leaves = operations
            .iter()
            .filter_map(|operation| operation.runtime_leaf.as_deref())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|identity| RuntimeLeaf {
                identity: identity.to_string(),
                deterministic: true,
            })
            .collect();
        Self {
            schema: "topaz.compiler.runtime-registry/v1".to_string(),
            leaves,
            templates: vec![
                RuntimeTemplate {
                    identity: "topaz_emit/boxed-runtime-items/v1".to_string(),
                    sha256:
                        "sha256:6e450737da8c783da097d4b74f942c21435c94256c7f2fc2d85257648cd751f9"
                            .to_string(),
                },
                RuntimeTemplate {
                    identity: "topaz_emit/native-hybrid-items/v1".to_string(),
                    sha256:
                        "sha256:5c8dfa5c92a6a8a98e8bfcfc29b38ae216e9840c93d4be04a31d663f669fb648"
                            .to_string(),
                },
            ],
        }
    }
}

impl LoweredUnit {
    pub fn explicit_main_span(&self) -> Option<Span> {
        use crate::emission::{StmtKind, text};

        let entry = self.modules.iter().find(|module| module.is_entry)?;
        entry
            .program
            .items
            .iter()
            .find_map(|statement| match &statement.kind {
                StmtKind::Export(inner) => match &inner.kind {
                    StmtKind::Function(declaration)
                        if text(&entry.text, declaration.name.span) == Some("main") =>
                    {
                        Some(declaration.name.span)
                    }
                    _ => None,
                },
                _ => None,
            })
    }

    pub fn import_chain(&self, target: &str) -> String {
        let entry = self
            .modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.identity.as_str())
            .unwrap_or_default();
        topaz_diag::render_import_chain(entry, &self.import_edges, target)
    }
}
