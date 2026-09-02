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
    pub(crate) lo: u32,
    pub(crate) hi: u32,
    pub(crate) kind: String,
    pub(crate) detail: String,
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
    pub(crate) identity: String,
    pub(crate) entry: bool,
    pub(crate) operations: Vec<usize>,
}

#[derive(Clone, Debug)]
/// Fields stay private so execution can rely on decoder-established references and ordering.
pub struct Program {
    pub(crate) modules: Vec<Module>,
    pub(crate) operations: Vec<Operation>,
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
