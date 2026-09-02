//! `MonoTy` — the codegen-facing monomorphized representation type, and the
//! typed-HIR vehicle the v5.4 native (monomorphized) emit backend consumes.
//!
//! The native backend lowers HOT SCALAR paths to bare `i64`/`f64`/`bool`/`()`,
//! carries exact byte handles in the existing boxed runtime `Value`, and BOXES
//! everything else (strings, arrays, records, enums, Option/Result, and anything
//! not statically known). [`MonoTy`] is exactly that representation decision —
//! it is NOT the checker's rich `Type` (unions, records, generics, literal
//! types). It is the small, closed, codegen alphabet.
//!
//! Dependency direction is the whole point (CDR-006 §7): this type lives in
//! `topaz_hir` (which depends only on the AST + spans), so `topaz_emit` can
//! consume typed HIR WITHOUT depending on `topaz_check`. The CHECKER converts
//! its rich `Type` → `MonoTy` AFTER a clean check and hands the typed HIR across
//! this boundary; the checker's `Type` never leaks into `topaz_hir`/`topaz_emit`.
//!
//! SOUNDNESS RULE (the top risk in the design): a value is native ONLY when its
//! type is a CONCRETE scalar. Anything with an `Unknown`/`Var`/`Skolem`/`Foreign`
//! component, and every non-scalar type, converts to [`MonoTy::Boxed`] — so the
//! native backend can never drop a runtime guard behind an untyped fact.

use topaz_diag::Span;

/// Closed, engine-neutral semantic type algebra retained by a clean checker
/// result. Unlike [`MonoTy`], this records language meaning rather than a Rust
/// representation choice.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticType {
    Primitive(SemanticPrimitive),
    Literal(SemanticLiteral),
    Union(Vec<SemanticType>),
    Record(Vec<SemanticField>),
    Constructor {
        constructor: SemanticConstructor,
        arguments: Vec<SemanticType>,
    },
    Function {
        parameters: Vec<SemanticType>,
        variadic: Option<Box<SemanticType>>,
        result: Box<SemanticType>,
    },
    Foreign {
        identity: String,
        arguments: Vec<SemanticType>,
    },
    Rigid {
        name: String,
        origin: String,
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
    Enum {
        identity: String,
        arguments: Vec<SemanticType>,
    },
    NominalRecord {
        identity: String,
        arguments: Vec<SemanticType>,
    },
    Newtype {
        identity: String,
        arguments: Vec<SemanticType>,
    },
    /// A gradual checker boundary that is deliberately unknown. A clean full
    /// Typed IR must reject this unless the fact is explicitly marked
    /// `ambient`.
    Unknown,
    /// Inference-local variable. A clean full Typed IR must never retain one.
    InferenceVariable,
}

impl SemanticType {
    /// Whether a supposedly clean semantic fact still contains a gradual or
    /// inference-local hole. Bootstrap input rejects these recursively rather
    /// than allowing an implementation-specific display string to conceal one.
    pub fn has_hole(&self) -> bool {
        match self {
            Self::Unknown | Self::InferenceVariable => true,
            Self::Union(values) => values.iter().any(Self::has_hole),
            Self::Record(fields) => fields.iter().any(|field| field.ty.has_hole()),
            Self::Constructor { arguments, .. }
            | Self::Foreign { arguments, .. }
            | Self::Enum { arguments, .. }
            | Self::NominalRecord { arguments, .. }
            | Self::Newtype { arguments, .. } => arguments.iter().any(Self::has_hole),
            Self::Function {
                parameters,
                variadic,
                result,
            } => {
                parameters.iter().any(Self::has_hole)
                    || variadic.as_deref().is_some_and(Self::has_hole)
                    || result.has_hole()
            }
            Self::Primitive(_)
            | Self::Literal(_)
            | Self::Rigid { .. }
            | Self::Template
            | Self::File
            | Self::JsonValue
            | Self::Bytes
            | Self::ByteBuffer
            | Self::Path
            | Self::Regex
            | Self::Match
            | Self::TomlValue
            | Self::Url
            | Self::Date
            | Self::BigInt
            | Self::Decimal
            | Self::RoundingMode => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticPrimitive {
    Int,
    Float,
    String,
    Bool,
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticLiteral {
    String(String),
    Int(i64),
    Float(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemanticField {
    pub name: String,
    pub ty: SemanticType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticConstructor {
    Array,
    Map,
    Set,
    Option,
    Result,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypedNodeKind {
    Expression,
    Pattern,
    Binding,
    Declaration,
    Type,
}

/// One stable semantic fact owned by the checker. `ambient` is true only for a
/// deliberately gradual fragment/extern boundary; a clean current package may
/// not use it to conceal an inference hole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedNode {
    pub module: String,
    pub kind: TypedNodeKind,
    pub span: Span,
    pub ty: SemanticType,
    pub ambient: bool,
}

/// Checker-retained call decision. Evaluation order and written argument shape
/// remain in the existing [`crate::CallPlan`]; this fact adds the semantic
/// callee/result types and resolved runtime identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedCall {
    pub module: String,
    pub span: Span,
    pub callee_span: Span,
    pub callee_type: SemanticType,
    pub result_type: SemanticType,
    pub target_identity: Option<String>,
    /// True only when gradual checking deliberately leaves a callable or result
    /// type open. This makes an unknown explicit protocol fact rather than an
    /// unexplained hole.
    pub ambient: bool,
    pub plan: crate::CallPlan,
}

/// A closure reference whose declaration lives outside the closure's lexical
/// scope. Both ends use declaration/reference spans, never allocation identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedCapture {
    pub module: String,
    pub closure_span: Span,
    pub reference_span: Span,
    pub declaration_span: Span,
    pub name: String,
    pub ty: SemanticType,
    /// True only when this capture crosses a deliberately gradual boundary.
    pub ambient: bool,
}

/// The representation a value carries in monomorphized native code: a bare
/// scalar register, or the boxed runtime `Value` (the fallback for everything
/// non-scalar or not-statically-known).
///
/// This is the SINGLE codegen decision: the native backend uses a bare `i64`
/// for [`MonoTy::I64`], an `f64` for [`MonoTy::F64`], a `bool` for
/// [`MonoTy::Bool`], the unit for [`MonoTy::Unit`], and a boxed `topaz_value::Value`
/// for [`MonoTy::Boxed`] (boxing/unboxing only at island boundaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MonoTy {
    /// A concrete `int` — a bare `i64`.
    I64,
    /// A concrete `float` — a bare `f64`.
    F64,
    /// A concrete `bool`.
    Bool,
    /// The concrete unit `()`.
    Unit,
    /// An exact immutable `Bytes` handle. The runtime carrier remains the boxed
    /// `Value::Bytes`; this is NOT a bare scalar or ABI pointer.
    BytesHandle,
    /// An exact mutable fixed-length `ByteBuffer` handle. The runtime carrier
    /// remains the boxed shared `Value::ByteBuffer`; this is NOT a bare scalar
    /// or ABI pointer.
    ByteBufferHandle,
    /// The boxed runtime `Value` fallback: strings, arrays, maps, sets, records,
    /// enums, Option/Result, ranges, JSON, functions/templates, AND every type
    /// that is not statically a concrete scalar (any `Unknown`/`Var`/`Skolem`/
    /// `Foreign` component, unions, literal types). The native backend keeps
    /// these boxed and only un/boxes at island boundaries.
    Boxed,
}

impl MonoTy {
    /// Whether this is one of the four bare native scalars. Keep this an
    /// explicit allow-list: byte handles and any future non-boxed fact still
    /// ride the runtime `Value`.
    pub fn is_scalar(self) -> bool {
        matches!(
            self,
            MonoTy::I64 | MonoTy::F64 | MonoTy::Bool | MonoTy::Unit
        )
    }

    /// Whether this is one of the exact boxed byte-handle facts.
    pub fn is_byte_handle(self) -> bool {
        matches!(self, MonoTy::BytesHandle | MonoTy::ByteBufferHandle)
    }

    /// A stable, short rendering for snapshot/test assertions.
    pub fn name(self) -> &'static str {
        match self {
            MonoTy::I64 => "i64",
            MonoTy::F64 => "f64",
            MonoTy::Bool => "bool",
            MonoTy::Unit => "unit",
            MonoTy::BytesHandle => "bytes-handle",
            MonoTy::ByteBufferHandle => "byte-buffer-handle",
            MonoTy::Boxed => "boxed",
        }
    }
}

/// One exact byte field declared by a checker-proven, own-module, non-generic
/// nominal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedByteField {
    pub name: String,
    pub mono: MonoTy,
}

/// A read-only function parameter whose direct source annotation names a
/// checker-proven own-module nominal record with at least one exact byte field.
/// The parameter itself remains boxed; this fact only authorizes a separately
/// proved direct field projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedByteRecordParam {
    pub module: String,
    pub function_span: Span,
    pub name: String,
    pub span: Span,
    /// Effective nominal declaration identity for this language profile:
    /// source spelling before 5.20, defining-module-stable identity in 5.20+.
    pub declaration_identity: String,
    pub fields: Vec<TypedByteField>,
}

/// A checker-proven `let local = parameter.byteField` shape. Both the receiver
/// parameter and the projected local are identified by declaration span so
/// emitter syntax cannot forge or accidentally capture the fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedByteProjection {
    pub module: String,
    pub function_span: Span,
    pub receiver_name: String,
    pub receiver_span: Span,
    pub field: String,
    pub expression_span: Span,
    pub local_name: String,
    pub local_span: Span,
    pub mono: MonoTy,
}

/// One typed local binding in the typed HIR: a `let`/`let mut` binding or a
/// function parameter, carrying the [`MonoTy`] the backend lowers it to.
///
/// This is the minimal typed-HIR annotation the FOUNDATION slice lands: enough
/// for a later native backend to allocate a bare scalar register vs a boxed slot
/// per local. Richer per-expression annotations (and the lowered op tree) arrive
/// with the native backend itself; the substrate here is deliberately small,
/// concrete, and testable, so it is exercised rather than dead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedLocal {
    /// The source name of the binding (or parameter).
    pub name: String,
    /// The defining span (the binding/parameter name's span).
    pub span: Span,
    /// The representation the binding lowers to.
    pub mono: MonoTy,
}

/// The typed HIR for one checked compilation unit: the per-local [`MonoTy`]
/// annotations a native backend reads. Produced by `topaz_check::check_unit_typed`
/// ONLY when the unit checks clean, so every annotation rests on a sound type.
///
/// Locals are recorded in deterministic source order (the order the checker
/// visits their declarations), so the typed HIR is reproducible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypedUnit {
    pub locals: Vec<TypedLocal>,
    /// Complete checker-owned semantic nodes in canonical module/span order.
    pub nodes: Vec<TypedNode>,
    /// Complete checked call decisions in canonical module/span order.
    pub calls: Vec<TypedCall>,
    /// Closure captures in canonical closure/reference order.
    pub captures: Vec<TypedCapture>,
    /// Exact read-only record parameter proofs, in deterministic source order.
    pub byte_record_params: Vec<TypedByteRecordParam>,
    /// Exact direct byte-field projection proofs, in deterministic source order.
    pub byte_projections: Vec<TypedByteProjection>,
    /// Whether the WHOLE checked unit contains ANY `concurrent` expression
    /// (CDR-006 §15) — a conservative unit-level fact the native backend uses to
    /// decide loop-`checkpoint().await` ELISION. The loop back-edge checkpoint
    /// exists ONLY so a `while`-spinning `concurrent` ARM yields to the round-robin
    /// scheduler; under the single-future `block_on` driver it is a transparent
    /// re-poll that enforces NO step/fuel/time budget (the only deadline lives in
    /// `concurrent`'s own timeout machinery). So when NO `concurrent` appears
    /// ANYWHERE in the unit, no loop is reachable from a concurrent arm and the
    /// checkpoint is pure overhead — the native backend may drop the `.await`
    /// safely, leaving results, termination, and faults byte-identical. When this
    /// is `true`, native KEEPS every loop checkpoint (no elision).
    pub contains_concurrent: bool,
}

impl TypedUnit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a typed local binding.
    pub fn push_local(&mut self, name: impl Into<String>, span: Span, mono: MonoTy) {
        self.locals.push(TypedLocal {
            name: name.into(),
            span,
            mono,
        });
    }

    pub fn push_byte_record_param(&mut self, fact: TypedByteRecordParam) {
        self.byte_record_params.push(fact);
    }

    pub fn push_byte_projection(&mut self, fact: TypedByteProjection) {
        self.byte_projections.push(fact);
    }

    pub fn push_node(&mut self, fact: TypedNode) {
        self.nodes.push(fact);
    }

    pub fn push_call(&mut self, fact: TypedCall) {
        self.calls.push(fact);
    }

    pub fn push_capture(&mut self, fact: TypedCapture) {
        self.captures.push(fact);
    }

    /// The recorded [`MonoTy`] of the FIRST local with this name — a test/debug
    /// convenience (a later backend keys by span/scope, not name).
    pub fn local_mono(&self, name: &str) -> Option<MonoTy> {
        self.locals.iter().find(|l| l.name == name).map(|l| l.mono)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_diag::{FileId, Span};

    const SP: Span = Span {
        file: FileId(0),
        lo: 0,
        hi: 1,
    };

    #[test]
    fn scalar_classification_and_names() {
        assert!(MonoTy::I64.is_scalar());
        assert!(MonoTy::F64.is_scalar());
        assert!(MonoTy::Bool.is_scalar());
        assert!(MonoTy::Unit.is_scalar());
        assert!(!MonoTy::BytesHandle.is_scalar());
        assert!(!MonoTy::ByteBufferHandle.is_scalar());
        assert!(!MonoTy::Boxed.is_scalar());
        assert!(MonoTy::BytesHandle.is_byte_handle());
        assert!(MonoTy::ByteBufferHandle.is_byte_handle());
        assert!(!MonoTy::I64.is_byte_handle());
        assert_eq!(MonoTy::I64.name(), "i64");
        assert_eq!(MonoTy::BytesHandle.name(), "bytes-handle");
        assert_eq!(MonoTy::ByteBufferHandle.name(), "byte-buffer-handle");
        assert_eq!(MonoTy::Boxed.name(), "boxed");
    }

    #[test]
    fn typed_unit_records_locals_in_order() {
        let mut u = TypedUnit::new();
        u.push_local("n", SP, MonoTy::I64);
        u.push_local("name", SP, MonoTy::Boxed);
        assert_eq!(u.locals.len(), 2);
        assert_eq!(u.local_mono("n"), Some(MonoTy::I64));
        assert_eq!(u.local_mono("name"), Some(MonoTy::Boxed));
        assert_eq!(u.local_mono("missing"), None);
    }
}
