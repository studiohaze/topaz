use super::*;

pub(super) type ByteRecordParam = (Span, String, Span, String);

pub(super) type NativeFunctionIndex<'a> = HashMap<String, Rc<NativeFn<'a>>>;

#[derive(Default)]
pub(super) struct GenericFunctionIndex<'a> {
    pub(super) by_name: HashMap<String, HashMap<Vec<NativeTy>, Rc<NativeFn<'a>>>>,
}

impl<'a> GenericFunctionIndex<'a> {
    pub(super) fn insert(&mut self, name: &str, type_args: Vec<NativeTy>, signature: NativeFn<'a>) {
        if let Some(specializations) = self.by_name.get_mut(name) {
            specializations.insert(type_args, Rc::new(signature));
            return;
        }
        let mut specializations = HashMap::new();
        specializations.insert(type_args, Rc::new(signature));
        self.by_name.insert(name.to_string(), specializations);
    }

    pub(super) fn get(&self, name: &str, type_args: &[NativeTy]) -> Option<&Rc<NativeFn<'a>>> {
        self.by_name.get(name)?.get(type_args)
    }
}

#[derive(Default)]
pub(super) struct TypedLocalIndex {
    pub(super) by_name: HashMap<String, HashMap<(FileId, u32, u32), MonoTy>>,
}

impl TypedLocalIndex {
    pub(super) fn from_typed_hir(typed_hir: &TypedUnit) -> Self {
        let mut index = Self::default();
        for local in &typed_hir.locals {
            index
                .by_name
                .entry(local.name.clone())
                .or_default()
                .insert((local.span.file, local.span.lo, local.span.hi), local.mono);
        }
        index
    }

    pub(super) fn get(&self, name: &str, span: Span) -> Option<MonoTy> {
        self.by_name
            .get(name)?
            .get(&(span.file, span.lo, span.hi))
            .copied()
    }
}

/// The scalar representation a native expression/local carries — exactly the
/// concrete scalar subset of [`MonoTy`] (the backend keeps the `Boxed` case OUT
/// of this type: a non-scalar value never enters the native island).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum NativeTy {
    I64,
    F64,
    Bool,
    Str,
    Unit,
}

impl NativeTy {
    /// The bare Rust type this scalar lowers to.
    pub(super) fn rust(self) -> &'static str {
        match self {
            NativeTy::I64 => "i64",
            NativeTy::F64 => "f64",
            NativeTy::Bool => "bool",
            NativeTy::Str => "String",
            NativeTy::Unit => "()",
        }
    }

    pub(super) fn tag(self) -> &'static str {
        match self {
            NativeTy::I64 => "i64",
            NativeTy::F64 => "f64",
            NativeTy::Bool => "bool",
            NativeTy::Str => "str",
            NativeTy::Unit => "unit",
        }
    }

    /// The corresponding `MonoTy` (so a local can be cross-checked against the
    /// typed HIR — the soundness anchor).
    pub(super) fn mono(self) -> MonoTy {
        match self {
            NativeTy::I64 => MonoTy::I64,
            NativeTy::F64 => MonoTy::F64,
            NativeTy::Bool => MonoTy::Bool,
            NativeTy::Str => MonoTy::Boxed,
            NativeTy::Unit => MonoTy::Unit,
        }
    }

    /// Box a native scalar expression of this type into the runtime `Value` (the
    /// island boundary: the entry result and `print`/`render` cross here).
    pub(super) fn box_expr(self, rs: &str) -> String {
        match self {
            NativeTy::I64 => format!("Value::Int({rs})"),
            NativeTy::F64 => format!("Value::Float({rs})"),
            NativeTy::Bool => format!("Value::Bool({rs})"),
            NativeTy::Str => format!("Value::str({rs})"),
            // The unit expression still runs (effects), then yields `Value::Unit`.
            NativeTy::Unit => format!("{{ let _ = {rs}; Value::Unit }}"),
        }
    }
}

/// What a native binding holds: a bare scalar register, or a BOXED
/// `Array<scalar>` boundary value (the array stays a `Value::Array`; only its
/// element reads + `.length` lower native).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalKind {
    /// A bare native scalar register (`i64`/`f64`/`bool`/`()`).
    Scalar(NativeTy),
    /// A boxed `Value::Array` of a CONCRETE scalar element type (the payload is
    /// the element scalar). Native can read `arr[i]` / `arr.length`; mutable
    /// locals additionally support direct index assignment through `index_slot`.
    /// Bare-array reads and broader aggregate mutation still decline.
    ScalarArray(NativeTy),
    /// An exact `Bytes` or `ByteBuffer` handle whose runtime carrier remains a
    /// boxed `Value`.
    ByteHandle(MonoTy),
    /// A checker-proven read-only own-module record parameter. The record stays
    /// boxed and is usable only as the receiver of one proved byte projection.
    ByteRecord,
    /// A read-only boxed `Value` boundary local. Scalar lowering cannot read it;
    /// only the entry-boundary value path can clone it and route aggregate reads
    /// through shared `Value` leaves.
    BoxedValue,
}

/// A native binding in scope: its source name and what it holds. The native
/// island holds scalar locals and boxed scalar-array boundary locals.
#[derive(Clone)]
pub(super) struct NativeLocal {
    pub(super) name: String,
    pub(super) kind: LocalKind,
    /// Whether it is a `let mut` (assignable) — a `let` is not. Array boundary
    /// locals are mutable only for direct `arr[i] = value` writes.
    pub(super) mutable: bool,
}

impl NativeLocal {
    /// The scalar type this local reads AS, when used directly — `Some` only for a
    /// `Scalar` local. A `ScalarArray` local has no direct scalar reading (a bare
    /// `arr` reference would need boxing — declined; only `arr[i]`/`arr.length`).
    pub(super) fn scalar_ty(&self) -> Option<NativeTy> {
        match self.kind {
            LocalKind::Scalar(t) => Some(t),
            LocalKind::ScalarArray(_)
            | LocalKind::ByteHandle(_)
            | LocalKind::ByteRecord
            | LocalKind::BoxedValue => None,
        }
    }

    /// The element scalar type, when this is a boxed scalar-array boundary local.
    pub(super) fn array_elem(&self) -> Option<NativeTy> {
        match self.kind {
            LocalKind::ScalarArray(e) => Some(e),
            LocalKind::Scalar(_)
            | LocalKind::ByteHandle(_)
            | LocalKind::ByteRecord
            | LocalKind::BoxedValue => None,
        }
    }

    pub(super) fn byte_handle(&self) -> Option<MonoTy> {
        match self.kind {
            LocalKind::ByteHandle(mono) => Some(mono),
            _ => None,
        }
    }

    pub(super) fn is_byte_record(&self) -> bool {
        matches!(self.kind, LocalKind::ByteRecord)
    }

    pub(super) fn is_boxed_carrier(&self) -> bool {
        matches!(
            self.kind,
            LocalKind::ScalarArray(_)
                | LocalKind::ByteHandle(_)
                | LocalKind::ByteRecord
                | LocalKind::BoxedValue
        )
    }
}

/// A native function parameter representation.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum NativeParam {
    Scalar(NativeTy),
    ScalarArray(NativeTy),
    ByteHandle(MonoTy),
    ByteRecord(String),
}

impl NativeParam {
    pub(super) fn rust(&self) -> &'static str {
        match self {
            NativeParam::Scalar(ty) => ty.rust(),
            // Read-only array boundary parameters stay boxed `Value`s. Native
            // code only crosses into scalar registers through `.length`/`[i]`.
            NativeParam::ScalarArray(_) => "Value",
            NativeParam::ByteHandle(_) | NativeParam::ByteRecord(_) => "Value",
        }
    }

    pub(super) fn local_kind(&self) -> LocalKind {
        match self {
            NativeParam::Scalar(ty) => LocalKind::Scalar(*ty),
            NativeParam::ScalarArray(elem) => LocalKind::ScalarArray(*elem),
            NativeParam::ByteHandle(mono) => LocalKind::ByteHandle(*mono),
            NativeParam::ByteRecord(_) => LocalKind::ByteRecord,
        }
    }

    pub(super) fn mono(&self) -> MonoTy {
        match self {
            NativeParam::Scalar(ty) => ty.mono(),
            NativeParam::ScalarArray(_) => MonoTy::Boxed,
            NativeParam::ByteHandle(mono) => *mono,
            NativeParam::ByteRecord(_) => MonoTy::Boxed,
        }
    }
}

/// A native scalar-returning function the program declares: the parameter
/// representations (in order) and the return representation. Direct same-module
/// calls to these lower to a real Rust call.
pub(super) struct NativeFn<'a> {
    pub(super) names: Rc<[&'a str]>,
    pub(super) defaults: Rc<[Option<&'a Expr>]>,
    pub(super) params: Vec<NativeParam>,
    pub(super) ret: NativeTy,
    pub(super) rust_name: String,
}

/// A lowered native expression: its Rust source and its scalar type.
pub(super) struct Lowered {
    pub(super) rs: String,
    pub(super) ty: NativeTy,
}

/// The native lowering context for one entry program.
pub(super) struct Ctx<'a> {
    pub(super) src: &'a LoweredText,
    /// The typed HIR's per-local `MonoTy`, keyed by `(name, span byte range)` —
    /// the soundness cross-check oracle.
    pub(super) hir_locals: &'a TypedLocalIndex,
    /// Name span of the top-level function currently being lowered.
    pub(super) current_function: Option<Span>,
    /// Checker-owned read-only record parameter facts.
    pub(super) byte_record_params: &'a [ByteRecordParam],
    /// Checker-owned direct record byte-field projection facts.
    pub(super) byte_projections: &'a [ByteProjectionProof],
    /// The native functions in scope (collected top-level, before bodies).
    pub(super) fns: Cow<'a, NativeFunctionIndex<'a>>,
    /// Top-level generic functions whose narrow scalar monomorphic templates
    /// are pre-generated into `generic_specs`.
    pub(super) generic_fns: HashMap<String, &'a FunctionDecl>,
    /// One generated scalar specialization for a generic function and concrete
    /// type-argument vector.
    pub(super) generic_specs: GenericFunctionIndex<'a>,
    /// The accumulated native function definitions, emitted before `entry`.
    pub(super) fn_defs: String,
    /// Whether the entry's loop back-edge `checkpoint().await` may be ELIDED: true
    /// IFF the WHOLE checked unit contains NO `concurrent` (the typed-HIR fact). The
    /// checkpoint exists only so a `while`-spinning `concurrent` arm yields to the
    /// round-robin scheduler; with no `concurrent` anywhere, no loop is reachable
    /// from an arm and the `block_on` driver enforces no budget — so dropping the
    /// `.await` is byte-identical (results, termination, faults all unchanged) and
    /// removes the per-iteration async suspension (the perf cost). When `false`,
    /// every loop KEEPS its checkpoint.
    pub(super) elide_checkpoints: bool,
    /// Namespace aliases introduced by `import std.math` / `import std.math as x`.
    /// The native backend treats those imports as no-op bindings and lowers
    /// total scalar `math.*` calls directly.
    pub(super) math_namespaces: Vec<String>,
    /// Hybrid helpers keep the boxed closure ABI at their outer boundary. Their
    /// wrapper has already entered the shared recursion guard, so only direct
    /// helper-to-helper calls enter another level.
    pub(super) hybrid: bool,
}

pub(super) struct ByteProjectionProof {
    pub(super) function_span: Span,
    pub(super) receiver_name: String,
    pub(super) receiver_span: Span,
    pub(super) field: String,
    pub(super) expression_span: Span,
    pub(super) local_name: String,
    pub(super) local_span: Span,
    pub(super) mono: MonoTy,
}

pub(super) const GENERIC_NATIVE_TYPES: [NativeTy; 5] = [
    NativeTy::I64,
    NativeTy::F64,
    NativeTy::Bool,
    NativeTy::Str,
    NativeTy::Unit,
];
