use crate::*;

#[derive(Clone, Debug)]
pub(crate) enum CallArgumentBinding {
    Positional,
    Named(String),
    Spread,
    InsertedLead,
}

#[derive(Clone, Debug)]
pub(crate) struct CallArgument {
    pub(crate) binding: CallArgumentBinding,
    pub(crate) source_index: Option<usize>,
    pub(crate) lo: u32,
    pub(crate) hi: u32,
}

#[derive(Clone, Debug)]
pub(crate) enum CallEvaluation {
    Callee,
    Receiver,
    OptionalGuard,
    PipeLead,
    Argument(usize),
}

pub(crate) enum ComprehensionClause {
    For { iterator: usize, pattern: usize },
    If { condition: usize },
}

#[derive(Default)]
pub(crate) struct ComprehensionClauseParts {
    pub(crate) iterator: Option<usize>,
    pub(crate) pattern: Option<usize>,
    pub(crate) condition: Option<usize>,
}

pub(crate) enum ComprehensionBody {
    Element(usize),
    Entry { key: usize, value: usize },
}

pub(crate) enum ComprehensionKind {
    Array,
    Set,
    Map,
}

pub(crate) enum ComprehensionOutput {
    Elements(Vec<Value>),
    Entries(Vec<(Value, Value)>),
}

#[derive(Clone, Debug)]
pub(crate) struct Operation {
    pub(crate) id: String,
    pub(crate) module: String,
    /// Runtime `FileId` of the owning module, assigned by [`Program::new`].
    pub(crate) source_ordinal: u32,
    pub(crate) lo: u32,
    pub(crate) hi: u32,
    pub(crate) kind: String,
    pub(crate) detail: String,
    pub(crate) literal_encoding: topaz_value::IrLiteralEncoding,
    pub(crate) operands: Vec<usize>,
    pub(crate) operand_labels: Vec<String>,
    pub(crate) semantic_type: String,
    pub(crate) pattern_type: Option<SemanticType>,
    pub(crate) reference_identity: String,
    pub(crate) binding_name: String,
    pub(crate) declaration_identity: String,
    pub(crate) control_target: String,
    pub(crate) call_target: String,
    pub(crate) call_callee_kind: String,
    pub(crate) call_method: String,
    pub(crate) call_optional: bool,
    pub(crate) call_shadow_first: bool,
    pub(crate) call_stage_method: String,
    pub(crate) call_arguments: Vec<CallArgument>,
    pub(crate) call_type_arguments: Vec<SemanticType>,
    pub(crate) call_evaluations: Vec<CallEvaluation>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SemanticPrimitive {
    Int,
    Float,
    String,
    Bool,
    Unit,
}

#[derive(Clone, Debug)]
pub(crate) enum SemanticLiteral {
    String(String),
    Int(i64),
    Float(String),
    Bool(bool),
    Null,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SemanticConstructor {
    Array,
    Map,
    Set,
    Option,
    Result,
    Range,
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticField {
    pub(crate) name: String,
    pub(crate) ty: SemanticType,
}

#[derive(Clone, Debug)]
pub(crate) enum SemanticType {
    Primitive(SemanticPrimitive),
    Literal(SemanticLiteral),
    Union(Vec<Self>),
    Record(Vec<SemanticField>),
    Constructor {
        constructor: SemanticConstructor,
        arguments: Vec<Self>,
    },
    Function {
        parameters: Vec<Self>,
        variadic: Option<Box<Self>>,
        result: Box<Self>,
    },
    Foreign {
        identity: String,
        arguments: Vec<Self>,
    },
    Rigid {
        name: String,
        _origin: String,
    },
    Enum {
        identity: String,
        arguments: Vec<Self>,
    },
    NominalRecord {
        identity: String,
        arguments: Vec<Self>,
    },
    Newtype {
        identity: String,
        arguments: Vec<Self>,
    },
    Template,
    File,
    JsonValue,
    Bytes,
    ByteBuffer,
    Path,
    Regex,
    Match,
    TomlValue,
    Url,
    Date,
    BigInt,
    Decimal,
    RoundingMode,
    Unknown,
    InferenceVariable,
}

#[derive(Clone, Debug)]
pub(crate) struct Module {
    pub(crate) generated_std: bool,
    pub(crate) identity: String,
    /// Position of this module in the compiler's source list, which is the
    /// `FileId` its runtime diagnostics name.
    pub(crate) source_ordinal: u32,
    pub(crate) entry: bool,
    pub(crate) operations: Vec<usize>,
}

pub(crate) type SelectedImports = BTreeMap<String, BTreeMap<String, (String, String)>>;

/// Decoded operation kind.
///
/// The kind string is the wire form; execution dispatches on this projection so
/// the step loop compares a discriminant instead of rescanning a string for
/// every operation it evaluates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpKind {
    Import,
    Record,
    Enum,
    Newtype,
    TypeAlias,
    Function,
    Implementation,
    Protocol,
    BindingCapture,
    Integer,
    Length,
    Float,
    Boolean,
    Null,
    Unit,
    Duration,
    StringText,
    Identifier,
    Lambda,
    Placeholder,
    Continue,
    Module,
    Export,
    Parenthesized,
    Block,
    Using,
    If,
    Match,
    For,
    Loop,
    While,
    ResultPropagation,
    Return,
    Break,
    Call,
    Pipeline,
    Binary,
    Range,
    Compose,
    Member,
    OptionalMember,
    Index,
    Unary,
    Constant,
    StringInterpolation,
    Array,
    Set,
    Map,
    Comprehension,
    Concurrent,
    RecordLiteral,
    RecordUpdate,
    Let,
    Assignment,
    Defer,
    Other,
}

impl OpKind {
    pub(crate) fn classify(kind: &str) -> Self {
        match kind {
            "import" => Self::Import,
            "record" => Self::Record,
            "enum" => Self::Enum,
            "newtype" => Self::Newtype,
            "type-alias" => Self::TypeAlias,
            "function" => Self::Function,
            "implementation" => Self::Implementation,
            "protocol" => Self::Protocol,
            "binding/capture" => Self::BindingCapture,
            "expression/integer" => Self::Integer,
            "expression/length" => Self::Length,
            "expression/float" => Self::Float,
            "expression/boolean" => Self::Boolean,
            "expression/null" => Self::Null,
            "expression/unit" => Self::Unit,
            "expression/duration" => Self::Duration,
            "expression/string-text" => Self::StringText,
            "expression/identifier" => Self::Identifier,
            "expression/lambda" => Self::Lambda,
            "expression/placeholder" => Self::Placeholder,
            "continue" => Self::Continue,
            "module" => Self::Module,
            "export" => Self::Export,
            "expression/parenthesized" => Self::Parenthesized,
            "expression/block" => Self::Block,
            "using" => Self::Using,
            "expression/if" => Self::If,
            "expression/match" => Self::Match,
            "expression/for" => Self::For,
            "expression/loop" => Self::Loop,
            "while" => Self::While,
            "expression/result-propagation" => Self::ResultPropagation,
            "return" => Self::Return,
            "break" => Self::Break,
            "expression/call" => Self::Call,
            "expression/pipeline" => Self::Pipeline,
            "expression/binary" => Self::Binary,
            "expression/range" => Self::Range,
            "expression/compose" => Self::Compose,
            "expression/member" => Self::Member,
            "expression/optional-member" => Self::OptionalMember,
            "expression/index" => Self::Index,
            "expression/unary" => Self::Unary,
            "constant" => Self::Constant,
            "expression/string" => Self::StringInterpolation,
            "expression/array" => Self::Array,
            "expression/set" => Self::Set,
            "expression/map" => Self::Map,
            "expression/comprehension" => Self::Comprehension,
            "expression/concurrent" => Self::Concurrent,
            "expression/record-literal" => Self::RecordLiteral,
            "expression/record-update" => Self::RecordUpdate,
            "let" => Self::Let,
            "assignment" => Self::Assignment,
            "defer" => Self::Defer,
            _ => Self::Other,
        }
    }

    /// Operations whose value needs no operand evaluation and no suspension.
    pub(crate) fn is_leaf(self) -> bool {
        matches!(
            self,
            Self::Import
                | Self::Record
                | Self::Enum
                | Self::Newtype
                | Self::TypeAlias
                | Self::Function
                | Self::Implementation
                | Self::Protocol
                | Self::BindingCapture
                | Self::Length
                | Self::Integer
                | Self::Float
                | Self::Boolean
                | Self::Null
                | Self::Unit
                | Self::Duration
                | Self::StringText
                | Self::Identifier
                | Self::Lambda
                | Self::Placeholder
                | Self::Continue
        )
    }
}

#[derive(Clone, Debug)]
/// Fields stay private so execution can rely on decoder-established references and ordering.
pub struct Program {
    pub(crate) language_version: topaz_value::LangVersion,
    pub(crate) modules: Vec<Module>,
    pub(crate) operations: Vec<Operation>,
    /// Decoded `operations[i].kind`, established once by the decoder.
    pub(crate) kinds: Vec<OpKind>,
    pub(crate) literal_facts: Vec<Option<Result<topaz_value::IrLiteral, String>>>,
    /// Namespace aliases are scoped to the importing module. They name the
    /// resolved module from the compiler's existing import plan.
    pub(crate) namespace_imports: BTreeMap<String, BTreeMap<String, String>>,
    /// Bound name -> (source module, exported name), scoped to the importer.
    pub(crate) selected_imports: SelectedImports,
}

impl Program {
    pub(crate) fn new(modules: Vec<Module>, mut operations: Vec<Operation>) -> Self {
        let source_ordinals = modules
            .iter()
            .map(|module| (module.identity.as_str(), module.source_ordinal))
            .collect::<BTreeMap<_, _>>();
        for operation in &mut operations {
            operation.source_ordinal = source_ordinals
                .get(operation.module.as_str())
                .copied()
                .unwrap_or(0);
        }
        let kinds = operations
            .iter()
            .map(|operation| OpKind::classify(&operation.kind))
            .collect();
        Self {
            literal_facts: operations
                .iter()
                .map(|op| {
                    topaz_value::decode_ir_literal(&op.kind, &op.detail, op.literal_encoding).map(
                        |fact| {
                            fact.map_err(|message| {
                                if op.kind == "expression/string-text"
                                    && op.literal_encoding
                                        == topaz_value::IrLiteralEncoding::LegacySource
                                {
                                    format!(
                                        "{:?}",
                                        topaz_value::fault(
                                            topaz_value::codes::GUARD_TYPE,
                                            message,
                                            crate::wire::span(op)
                                        )
                                    )
                                } else {
                                    message
                                }
                            })
                        },
                    )
                })
                .collect(),
            language_version: topaz_value::LangVersion::V5_20,
            modules,
            operations,
            kinds,
            namespace_imports: BTreeMap::new(),
            selected_imports: SelectedImports::new(),
        }
    }
}

pub(crate) struct ParsedProgram {
    pub(crate) program: Program,
    pub(crate) requires_host: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramAdmission {
    CompilerImage,
    TargetProduct,
}
