//! AST definitions for the Topaz grammar (CDR-001 §6; v5.2 module
//! and base-syntax nodes per CDR-002).
//!
//! Statement, expression, pattern, and type nodes carry a [`Span`];
//! supporting nodes without one (declaration bodies, call arguments,
//! list elements) recover their extent from their children. No node
//! owns source text — identifier and literal text is recovered
//! through the source map.
//! Identifiers are syntactic atoms: no name resolution, no typing.
//! The per-file [`Program`] boundary is explicit (the compilation
//! model is multi-file-ready even though v5.1 is single-file).

use std::rc::Rc;

use topaz_diag::Span;

use crate::DurationUnit;

/// One source file's parse result (SPEC §5 `Program`).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Stmt>,
    pub span: Span,
}

/// A syntactic name; its text is the span's source text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ident {
    pub span: Span,
}

// ---- statements ---------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `import path` / `import path as alias` / `import path { specs }`
    /// (SPEC v5.2 §17). Parsed at `LangVersion::V5_2` and later, only at
    /// `Program` top level, prologue-position (every import precedes
    /// all other items).
    Import(ImportItem),
    /// `export <declaration>` (SPEC v5.2/v5.4 §17): a zero-runtime wrapper
    /// whose inner statement is a `Function`, `TypeAlias`, `Let`, `Const`, or
    /// v5.4 nominal declaration. `export let mut` parses (rejection is static
    /// semantics, resolver-era). Exported nominals publish a type surface only;
    /// their constructors remain syntactic, not runtime namespace fields.
    Export(Rc<Stmt>),
    /// `function name<T>(params) -> T { body }` (SPEC §7).
    Function(FunctionDecl),
    /// `type Name<T> = Type` (SPEC §3).
    TypeAlias(Rc<TypeAlias>),
    /// `enum Name { Variant, Variant(T), … }` (v5.3 user enums). Recognized
    /// CONTEXTUALLY (`enum` is an identifier, not a keyword — ADR-071), at
    /// `LangVersion::V5_3` and later.
    Enum(Rc<EnumDecl>),
    /// `record Name { field: T, field: T = default, … }` (v5.4 nominal records).
    /// Recognized CONTEXTUALLY (`record` is an identifier, not a keyword), gated
    /// to `>= V5_4` in the parser.
    Record(Rc<RecordDecl>),
    /// `newtype Name = BaseType` (v5.4): a distinct nominal wrapper over a base
    /// type with NO implicit coercion (constructor + `.value()` are the only
    /// bridges). Recognized CONTEXTUALLY (`newtype` is an identifier, not a
    /// keyword), gated to `>= V5_4` in the parser.
    Newtype(Rc<NewtypeDecl>),
    /// `impl Name { function m(self, …) -> T { … } … }` (v5.4): user RECEIVER
    /// METHODS on an own-module nominal type (record/enum/newtype). Each method's
    /// first parameter is `self` (the receiver, by value — Topaz is pure).
    /// Recognized CONTEXTUALLY (`impl` is an identifier, not a keyword), gated to
    /// `>= V5_4` in the parser. STATIC dispatch only: `u.m(a)` monomorphizes to a
    /// free call of the method with `u` as the first argument.
    ///
    /// `impl Show<User> { function show(value: User) -> string { … } }` (v5.4 §4):
    /// the SAME node also carries a MANUAL PROTOCOL impl — `target` is `Some` (the
    /// protocol name + conforming type). A protocol method is a FREE-function form
    /// (no `self`); it is dispatched by `Show.show(x)` on the runtime nominal id.
    Impl(ImplDecl),
    /// `protocol Show { function show(value: Self) -> string … }` /
    /// `protocol Show<T> { function show(value: T) -> string }` (v5.4 §4): a
    /// PROTOCOL declaration — a named set of free-function method SIGNATURES the
    /// conforming type must implement (manually via `impl Show<Type>` or by a
    /// `derives Show` clause). Recognized CONTEXTUALLY (`protocol` is an identifier,
    /// not a keyword), gated to `>= V5_4`. Dispatch is STATIC-ONLY: `Show.show(x)`,
    /// never `x.show()` (no trait objects / dynamic dispatch — spec §4).
    Protocol(ProtocolDecl),
    /// `let pattern (: Type)? = expr` / `let mut name (: Type)? = expr`
    /// (SPEC §4); `mutable` requires an identifier pattern.
    Let {
        mutable: bool,
        pattern: Rc<Pattern>,
        ty: Option<Type>,
        value: Expr,
    },
    /// `const name (: Type)? = const-expr` (SPEC §4).
    Const {
        name: Ident,
        ty: Option<Type>,
        value: Expr,
    },
    /// `target op value` (SPEC §5); statements only, never
    /// expressions. The target must be assignable (identifier,
    /// member, or index).
    Assign {
        target: Rc<Expr>,
        op: AssignOp,
        value: Rc<Expr>,
    },
    /// `return expr?` (SPEC §5).
    Return(Option<Expr>),
    /// `defer (block | call)` (SPEC §14).
    Defer(Rc<Expr>),
    /// `using name = expr { body }` (v5.4): a resource block. The initializer
    /// must produce a `File`; the binding is scoped to `body`; the resource closes
    /// deterministically when the block exits, including early `return`/`?`/loop
    /// control. Contextual statement-head syntax, not a reserved keyword.
    Using {
        name: Ident,
        value: Expr,
        body: Rc<Block>,
    },
    /// `while cond { body }` (SPEC §5); statement, no value.
    While {
        cond: Rc<Expr>,
        body: Rc<Block>,
    },
    /// `break (label)? (value)?` (SPEC §5). `label` targets a named
    /// enclosing `loop`; `value` is yielded as that loop expression's result.
    /// `break` alone targets the nearest enclosing loop and yields Unit. The
    /// label `Ident` span covers the NAME (the leading `'` is not included).
    Break {
        label: Option<Ident>,
        value: Option<Expr>,
    },
    /// `continue (label)?` (SPEC §5). `label` targets a named
    /// enclosing loop; bare `continue` targets the nearest enclosing loop.
    Continue {
        label: Option<Ident>,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// `+=`
    Add,
    /// `-=`
    Sub,
    /// `*=`
    Mul,
    /// `/=`
    Div,
    /// `%=`
    Rem,
    /// `??=` (SPEC §12, statement-only).
    Coalesce,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    /// Protocol bounds aligned with `type_params`, e.g. `function f<T: Show>()`.
    pub type_param_bounds: Vec<Vec<Ident>>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Rc<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
    /// `= const-expr` default (SPEC §7).
    pub default: Option<Expr>,
    /// `...name: T` variadic tail (SPEC §7).
    pub variadic: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAlias {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub ty: Rc<Type>,
}

/// `enum Name<T> { Variant, Variant(T1, …), … }` (v5.3/v5.4). A closed
/// nominal sum — variants are payload-less or tuple-payload. Generic enum
/// declarations are a v5.4 surface; the checker admits concrete instantiations
/// first and keeps fully recursive generic ADTs as a named follow-up.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub variants: Vec<EnumVariant>,
    /// `derives Eq, Order, Show` clause (v5.4 §4): a bare comma-separated list of
    /// protocol names after the enum head. Empty when absent. The names are
    /// validated (membership in the derivable set + derivability) at CHECK time;
    /// the parser only records the surface list.
    pub derives: Vec<Ident>,
}

/// One enum variant: `Red` (payload `None`) or `Circle(int)` (tuple payload).
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: Ident,
    pub payload: Option<Vec<Type>>,
    pub span: Span,
}

/// `record Name<T> { field: T, field: T = default, … }` (v5.4). A nominal
/// product. Generic record declarations are admitted for concrete
/// instantiations first; fully recursive generic records are a named follow-up.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordDecl {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub fields: Vec<RecordFieldDecl>,
    /// `derives Eq, Order, Show` clause (v5.4 §4): a bare comma-separated list of
    /// protocol names after the record head. Empty when absent. The names are
    /// validated (membership in the derivable set + derivability) at CHECK time;
    /// the parser only records the surface list.
    pub derives: Vec<Ident>,
}

/// `newtype Name<T> = BaseType` (v5.4). A distinct nominal wrapper over a single
/// base type. Generic newtypes are admitted for concrete instantiations first.
/// `base` is the `= int` part.
#[derive(Debug, Clone, PartialEq)]
pub struct NewtypeDecl {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub base: Type,
}

/// `impl Name { methods… }` (v5.4): a block of receiver methods on the nominal
/// type `name`. MVP: no generics (the `impl` has no type parameters); each method
/// is an ordinary `FunctionDecl` whose first parameter must be `self`. The methods
/// are checked + lowered as if they were free functions over the receiver type.
///
/// `impl Protocol<Type> { methods… }` (v5.4 §4): when `target` is `Some`, this is a
/// MANUAL PROTOCOL impl — `name` is the PROTOCOL, `target` the conforming TYPE, and
/// each method is a FREE function (NO `self`; the conforming value is an ordinary
/// parameter). It registers `(Protocol, Type) ∈ conformances` + the method bodies,
/// dispatched by `Protocol.method(x)` on the runtime nominal id.
#[derive(Debug, Clone, PartialEq)]
pub struct ImplDecl {
    pub name: Ident,
    /// `Some(conforming_type)` for a PROTOCOL impl `impl Show<User>` (then `name` is
    /// the protocol); `None` for an inherent `impl User` (then `name` is the type).
    pub target: Option<Ident>,
    pub methods: Vec<ImplMethod>,
}

/// `protocol Show { function show(value: Self) -> string … }` (v5.4 §4): a protocol
/// declaration. `type_params` carries `<T>` (the conforming type variable) when
/// spelled `protocol Show<T>`; empty when the protocol uses `Self`. The `methods`
/// are SIGNATURE-ONLY `FunctionDecl`s (their bodies are ignored — a protocol method
/// has no implementation; the conforming type supplies it). MVP: no default methods,
/// no associated types, no bounds (spec §4 policy).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecl {
    pub name: Ident,
    pub type_params: Vec<Ident>,
    pub methods: Vec<FunctionDecl>,
}

/// One method inside an `impl` block: an optional `export` flag (re-export of the
/// method's free-function form, mirroring exported declarations) plus the method's
/// `FunctionDecl`. The first parameter is `self` (validated at check time).
#[derive(Debug, Clone, PartialEq)]
pub struct ImplMethod {
    pub exported: bool,
    pub decl: FunctionDecl,
    pub span: Span,
}

/// One record field declaration: `name: T` or `name: T = default-expr`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordFieldDecl {
    pub name: Ident,
    pub ty: Type,
    /// `= const-or-runtime default` (the MVP allows a runtime default; defaults
    /// must not reference `self` or later fields).
    pub default: Option<Rc<Expr>>,
    pub span: Span,
}

/// `{ statements... tail-expression? }` — the tail expression, when
/// present, is the block value (SPEC §5/§1a).
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Rc<Expr>>,
    pub span: Span,
}

// ---- expressions ---------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int,
    Float,
    Duration(DurationUnit),
    Bool(bool),
    Null,
    /// `()` (SPEC §1).
    Unit,
    /// A string or template literal (SPEC §1/§16).
    String(Rc<StringLit>),
    Ident,
    /// `_` in expression position — a pipeline placeholder
    /// (SPEC §11); placement validity is checker-era.
    Placeholder,
    /// `(expr)` — kept so postfix distinctions like `(expr?).field`
    /// stay visible in the AST.
    Paren(Rc<Expr>),
    Block(Rc<Block>),
    If {
        cond: Rc<Expr>,
        then_block: Rc<Block>,
        /// `else` branch: an `if` expression or a block.
        else_branch: Option<Rc<Expr>>,
    },
    Match {
        scrutinee: Rc<Expr>,
        cases: Vec<CaseClause>,
    },
    /// `for pattern in iter { body }` — an expression (SPEC §5).
    For {
        pattern: Rc<Pattern>,
        iter: Rc<Expr>,
        body: Rc<Block>,
    },
    /// `loop (label)? { body }` is an infinite-loop expression. Its
    /// VALUE is the join of every `break <value>` targeting it (Unit when no
    /// break carries a value). The optional `label` (the NAME of a `'name`
    /// loop label) lets an inner `break 'name <value>` target this loop from
    /// inside a NESTED loop.
    Loop {
        label: Option<Ident>,
        body: Rc<Block>,
    },
    /// `concurrent { arms }` / `concurrent(timeout: d) { arms } else
    /// { ... }` (SPEC §15).
    Concurrent {
        timeout: Option<Rc<Expr>>,
        arms: Vec<ConcurrentArm>,
        else_block: Option<Rc<Block>>,
    },
    Call {
        callee: Rc<Expr>,
        args: Vec<CallArg>,
        /// Explicit call-site type arguments `f<T, U>(args)` (v5.4 §3).
        /// Empty when inferred. CHECK-ONLY: consumed by the checker to
        /// pre-seed the callee scheme's type variables; the interpreter
        /// and the native emitter ignore it, so a call lowers
        /// type-erased — `f<int>(x)` and `f(x)` produce identical run
        /// and build bytes.
        type_args: Vec<Type>,
    },
    Member {
        object: Rc<Expr>,
        field: Ident,
    },
    Index {
        object: Rc<Expr>,
        index: Rc<Expr>,
    },
    /// `expr?.field` (SPEC §12). An optional call is a `Call` whose
    /// callee is an `OptionalAccess`.
    OptionalAccess {
        object: Rc<Expr>,
        field: Ident,
    },
    /// Postfix `expr?` (SPEC §13).
    Try(Rc<Expr>),
    Unary {
        op: UnaryOp,
        operand: Rc<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Rc<Expr>,
        rhs: Rc<Expr>,
    },
    /// `lo .. hi (by step)?` / `lo ..< hi (by step)?` (SPEC §10).
    Range {
        lo: Rc<Expr>,
        hi: Rc<Expr>,
        inclusive: bool,
        step: Option<Rc<Expr>>,
    },
    /// `f >> g` (SPEC §11), right-associative.
    Compose {
        lhs: Rc<Expr>,
        rhs: Rc<Expr>,
    },
    /// `lhs |> rhs` (SPEC §11), left-associative.
    Pipe {
        lhs: Rc<Expr>,
        rhs: Rc<PipeRhs>,
    },
    Lambda {
        params: Vec<LambdaParam>,
        body: Rc<Expr>,
    },
    /// `{ field: value, ... }` (SPEC §8).
    RecordLiteral {
        fields: Vec<FieldInit>,
    },
    /// `base { field: value, ... }` (SPEC §8).
    ///
    /// `spread` carries a v5.4 NOMINAL spread-update source — `User { ...u, … }`
    /// parses with `base = User` (the record NAME) and `spread = Some(u)`. It is
    /// `None` for structural updates and for spread-less nominal construction.
    /// MVP: at most ONE leading spread (multiple/non-leading spreads are deferred).
    RecordUpdate {
        base: Rc<Expr>,
        spread: Option<Rc<Expr>>,
        fields: Vec<FieldInit>,
    },
    Array(Vec<ArrayElement>),
    /// `set { a, b, c }` (v5.4 §6) — a SET literal. CONTEXTUAL syntax:
    /// recognized only when the identifier `set` is immediately followed by
    /// `{` in primary/atom position (the identifier `set` is NOT a keyword and
    /// stays an ordinary identifier elsewhere). Empty `set {}` needs an expected
    /// type (mirrors empty `[]`). Duplicate elements SILENTLY COLLAPSE
    /// (`OrderedSet::add` returns false). Gated `>= V5_4`.
    SetLiteral(Vec<Expr>),
    /// `map { k: v, … }` (v5.4 §6) — a MAP literal of `(key, value)` entries.
    /// CONTEXTUAL like `set`. Empty `map {}` needs an expected type. A duplicate
    /// LITERAL key is a runtime FAULT (TPZ4601) — distinct from `Map.insert`'s
    /// silent overwrite — and a statically-obvious duplicate constant key is a
    /// CHECK error (TPZ5602). Gated `>= V5_4`.
    MapLiteral(Vec<(Expr, Expr)>),
    /// `[ for x in xs if p => body ]` / `set { for … => e }` / `map { for … => k: v }`
    /// (v5.4 §6.4) — a collection COMPREHENSION. The `clauses` are a flat,
    /// source-ordered list of `for`/`if` clauses (nested left-to-right like real
    /// loops); the `body` is the per-surviving-iteration element (array/set) or
    /// `key: value` entry (map). Recognized when a `for` clause leads the `[ … ]`
    /// / `set { … }` / `map { … }` (else it is the corresponding LITERAL). Each
    /// engine lowers it to the nested `for`/`if` machinery + a fresh accumulator,
    /// finalizing through the SAME shared `Value::array` / `builtin_set_of` /
    /// `builtin_map_of` leaves the literals use — so run≡build is byte-identical
    /// (including the map duplicate-key fault TPZ4601 and set duplicate collapse).
    /// Gated `>= V5_4`.
    Comprehension {
        kind: CompKind,
        clauses: Vec<CompClause>,
        body: Rc<CompBody>,
    },
}

/// Which collection a comprehension (or its accumulator) builds (v5.4 §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompKind {
    Array,
    Set,
    Map,
}

/// One clause of a comprehension's clause list (v5.4 §6.4). Clauses run
/// left-to-right, nested like loops; a `for` pattern binds in the clauses to its
/// right AND in the body; an `if` filters.
#[derive(Debug, Clone, PartialEq)]
pub enum CompClause {
    For { pattern: Pattern, iter: Rc<Expr> },
    If(Rc<Expr>),
}

/// The body produced once per surviving iteration of a comprehension (v5.4 §6.4):
/// a single element for an array/set comprehension, or a `key: value` entry for a
/// map comprehension.
#[derive(Debug, Clone, PartialEq)]
pub enum CompBody {
    Elem(Rc<Expr>),
    Entry { key: Rc<Expr>, value: Rc<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Pow,
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    In,
    And,
    Or,
    Coalesce,
}

/// The right-hand side of `|>` (SPEC §11).
#[derive(Debug, Clone, PartialEq)]
pub enum PipeRhs {
    Expr(Rc<Expr>),
    /// `.field` pipe sugar.
    Field(Ident),
}

/// A dotted module path (SPEC v5.2 §17): an address, not an
/// expression; only the final segment (or its alias) binds.
#[derive(Debug, Clone, PartialEq)]
pub struct ModulePath {
    pub segments: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportItem {
    pub path: ModulePath,
    pub kind: ImportKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportKind {
    /// Form A: binds the final path segment, or `alias` when present.
    Namespace { alias: Option<Ident> },
    /// Form B: binds each selected name (or its alias); creates no
    /// namespace binding.
    Selected { specs: Vec<ImportSpec> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportSpec {
    pub name: Ident,
    pub alias: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseClause {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: CaseArmBody,
    pub span: Span,
}

/// `CaseArmBody ::= Expression | ReturnStmt` (SPEC v5.2 §5,
/// ADR-074). The `Return` arm parses at `LangVersion::V5_2` and later;
/// it is divergent and contributes no value to match result
/// compatibility (checker-era concern).
#[derive(Debug, Clone, PartialEq)]
pub enum CaseArmBody {
    Expr(Expr),
    Return { value: Option<Expr>, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConcurrentArm {
    pub name: Ident,
    pub value: Rc<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Positional(Expr),
    /// `...expr` (SPEC §5).
    Spread(Expr),
    /// `name: expr` (SPEC §5).
    Named {
        name: Ident,
        value: Expr,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayElement {
    Expr(Expr),
    /// `...expr` (SPEC §9).
    Spread(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Rc<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LambdaParam {
    pub name: Ident,
    pub ty: Option<Type>,
    pub span: Span,
}

/// A string or template literal as a token tree (CDR-001 §4).
#[derive(Debug, Clone, PartialEq)]
pub struct StringLit {
    /// Tag-candidate span for a tagged template (text covers the tag
    /// identifier only); registry-validated at parse time (SPEC §16).
    pub tag: Option<Span>,
    pub multiline: bool,
    pub parts: Vec<StringPart>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// A raw text run; escapes resolve at lowering.
    Text(Span),
    /// `{ expression }` interpolation.
    Interpolation(Expr),
}

// ---- patterns -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatternKind {
    /// `alt | alt | ...` (SPEC v5.2 §6, ADR-073): two or more
    /// alternatives, none of which may bind names. Parsed at
    /// `LangVersion::V5_2` and later; a single-alternative pattern is never
    /// wrapped.
    Or(Vec<Pattern>),
    /// `_` (SPEC §6).
    Wildcard,
    /// A literal pattern; the expression is the literal.
    Literal(Rc<Expr>),
    /// `lo .. hi` / `lo ..< hi` with const-expression endpoints.
    Range {
        lo: Rc<Expr>,
        hi: Rc<Expr>,
        inclusive: bool,
    },
    /// A name that binds the matched value.
    Binding(Ident),
    /// `name: Type` (SPEC §6).
    Typed { name: Ident, ty: Type },
    /// `Name(subpatterns...)`.
    Constructor { name: Ident, args: Vec<Pattern> },
    /// `[elements...]` with at most one `..` rest marker.
    List(Vec<ListPatternElem>),
    /// `{ field (: subpattern)?, ... }` — a STRUCTURAL record pattern.
    Record(Vec<RecordPatternField>),
    /// `Name { field (: subpattern)?, ... }` (v5.4) — a NOMINAL record pattern;
    /// matches only a `Value::NominalRecord` with the same id. Distinct from the
    /// structural `Record` (which never destructures a nominal record).
    NominalRecord {
        name: Ident,
        fields: Vec<RecordPatternField>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListPatternElem {
    Pattern(Pattern),
    /// `.. binding?` rest marker (SPEC §6).
    Rest(Option<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordPatternField {
    pub name: Ident,
    /// Absent for the shorthand `{ x }`, which binds the field name.
    pub pattern: Option<Pattern>,
    pub span: Span,
}

// ---- types ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    /// `Name` or `Name<args...>` (SPEC §3).
    Named { name: Ident, args: Vec<Rc<Type>> },
    /// `ns.Name` / `ns.Name<args...>` (SPEC v5.2 §3/§17,
    /// `QualifiedNamedType`): valid only where name resolution proves
    /// `ns` is a namespace binding and `Name` an exported type alias.
    /// Parsed at `LangVersion::V5_2` and later.
    Qualified {
        ns: Ident,
        name: Ident,
        args: Vec<Rc<Type>>,
    },
    /// A literal type: string, integer, float, bool, or null
    /// (SPEC §3); the span is the literal.
    Literal,
    /// `{ field: Type, ... }` (SPEC §3).
    Record(Vec<FieldType>),
    /// `(params...) -> Ret` (SPEC §3).
    Function {
        params: Vec<FunctionTypeParam>,
        ret: Box<Type>,
    },
    /// `()` (SPEC §3).
    Unit,
    /// `A | B | ...` (SPEC §3); two or more members.
    Union(Vec<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldType {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionTypeParam {
    pub ty: Type,
    /// `...T` variadic tail (SPEC §3).
    pub variadic: bool,
}

/// Whether `e` contains the §11 pipeline placeholder `_` anywhere in
/// its sub-expressions. SHARED by the checker and the interpreter so
/// the "does this stage use `_`?" decision (which suppresses
/// first-argument insertion) cannot diverge between the engines.
pub fn contains_placeholder(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Placeholder => true,
        ExprKind::String(literal) => literal.parts.iter().any(|part| match part {
            StringPart::Text(_) => false,
            StringPart::Interpolation(expr) => contains_placeholder(expr),
        }),
        ExprKind::Paren(inner) | ExprKind::Try(inner) => contains_placeholder(inner),
        ExprKind::Block(block) => block_contains_placeholder(block),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            contains_placeholder(cond)
                || block_contains_placeholder(then_block)
                || else_branch.as_deref().is_some_and(contains_placeholder)
        }
        ExprKind::Match { scrutinee, cases } => {
            contains_placeholder(scrutinee)
                || cases.iter().any(|case| {
                    case.guard.as_ref().is_some_and(contains_placeholder)
                        || match &case.body {
                            CaseArmBody::Expr(expr) => contains_placeholder(expr),
                            CaseArmBody::Return { value, .. } => {
                                value.as_ref().is_some_and(contains_placeholder)
                            }
                        }
                })
        }
        ExprKind::For { iter, body, .. } => {
            contains_placeholder(iter) || block_contains_placeholder(body)
        }
        ExprKind::Loop { body, .. } => block_contains_placeholder(body),
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            timeout.as_deref().is_some_and(contains_placeholder)
                || arms.iter().any(|arm| contains_placeholder(&arm.value))
                || else_block
                    .as_deref()
                    .is_some_and(block_contains_placeholder)
        }
        ExprKind::Unary { operand, .. } => contains_placeholder(operand),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            contains_placeholder(lhs) || contains_placeholder(rhs)
        }
        ExprKind::Range { lo, hi, step, .. } => {
            contains_placeholder(lo)
                || contains_placeholder(hi)
                || step.as_deref().is_some_and(contains_placeholder)
        }
        ExprKind::Call { callee, args, .. } => {
            contains_placeholder(callee) || call_args_contain_placeholder(args)
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            contains_placeholder(object)
        }
        ExprKind::Index { object, index } => {
            contains_placeholder(object) || contains_placeholder(index)
        }
        ExprKind::RecordLiteral { fields } => fields.iter().any(|f| contains_placeholder(&f.value)),
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            contains_placeholder(base)
                || spread.as_ref().is_some_and(|s| contains_placeholder(s))
                || fields.iter().any(|f| contains_placeholder(&f.value))
        }
        ExprKind::Array(elems) => elems.iter().any(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => contains_placeholder(e),
        }),
        ExprKind::SetLiteral(elems) => elems.iter().any(contains_placeholder),
        ExprKind::MapLiteral(entries) => entries
            .iter()
            .any(|(k, v)| contains_placeholder(k) || contains_placeholder(v)),
        ExprKind::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|c| match c {
                CompClause::For { iter, .. } => contains_placeholder(iter),
                CompClause::If(cond) => contains_placeholder(cond),
            }) || match &**body {
                CompBody::Elem(e) => contains_placeholder(e),
                CompBody::Entry { key, value } => {
                    contains_placeholder(key) || contains_placeholder(value)
                }
            }
        }
        ExprKind::Lambda { body, .. } => contains_placeholder(body),
        // A nested pipe's RHS is its own stage; its lhs may still
        // hold this stage's placeholder.
        ExprKind::Pipe { lhs, .. } => contains_placeholder(lhs),
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident => false,
    }
}

fn block_contains_placeholder(block: &Block) -> bool {
    block.stmts.iter().any(stmt_contains_placeholder)
        || block.tail.as_deref().is_some_and(contains_placeholder)
}

fn function_contains_placeholder(decl: &FunctionDecl) -> bool {
    decl.params
        .iter()
        .filter_map(|parameter| parameter.default.as_ref())
        .any(contains_placeholder)
        || block_contains_placeholder(&decl.body)
}

fn stmt_contains_placeholder(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Export(inner) => stmt_contains_placeholder(inner),
        StmtKind::Function(decl) => function_contains_placeholder(decl),
        StmtKind::Record(decl) => decl
            .fields
            .iter()
            .filter_map(|field| field.default.as_deref())
            .any(contains_placeholder),
        StmtKind::Impl(decl) => decl
            .methods
            .iter()
            .any(|method| function_contains_placeholder(&method.decl)),
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => contains_placeholder(value),
        StmtKind::Assign { target, value, .. } => {
            contains_placeholder(target) || contains_placeholder(value)
        }
        StmtKind::Return(value) | StmtKind::Break { value, .. } => {
            value.as_ref().is_some_and(contains_placeholder)
        }
        StmtKind::Defer(value) => contains_placeholder(value),
        StmtKind::Expr(value) => contains_placeholder(value),
        StmtKind::Using { value, body, .. } => {
            contains_placeholder(value) || block_contains_placeholder(body)
        }
        StmtKind::While { cond, body } => {
            contains_placeholder(cond) || block_contains_placeholder(body)
        }
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Newtype(_)
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. } => false,
    }
}

/// Whether any argument of a call stage contains the §11 placeholder.
pub fn call_args_contain_placeholder(args: &[CallArg]) -> bool {
    args.iter().any(|a| match a {
        CallArg::Positional(e) | CallArg::Spread(e) | CallArg::Named { value: e, .. } => {
            contains_placeholder(e)
        }
    })
}

/// Boundary guard: whether a value crossing a function parameter/return
/// boundary declared as `ty` can be RUNTIME-checked structurally. True ONLY for
/// the fully-decidable, alias-free subset that the interpreter's `type_matches`
/// and the emitter's `type_test` decide IDENTICALLY (the differential corpus
/// pins them arm-for-arm): the scalars (`int`/`float`/`string`/`bool`), the
/// structural containers `Option`/`Result`/`Array`/`Set` over a guardable inner
/// type, records (exact field set, each field guardable), and unions whose
/// members are ALL guardable.
///
/// `()` (Unit) is SKIPPED — including ANY `()` nested inside a container, record,
/// or union (the recursion below returns `false` the moment it reaches one). The
/// checker types a `for` / `while` EXPRESSION as `()` (CDR-003 §5), but the
/// interpreter and the emitted code leave that construct's runtime value — a
/// value-collecting `for` yields an Array, not `Unit` — as the result. So a
/// `()`-typed slot can hold a non-`Unit` value at runtime in a program the
/// checker ACCEPTS, at EITHER boundary: a return (`function f() -> Option<()> {
/// Some(for …) }`) OR a parameter (`g(x: ())` called `g(for …)` — an argument is
/// an expression, and a `for` expression is checker-typed `()`). Guarding `()`
/// would therefore false-fault a valid program identically in both engines.
/// A `()` boundary carries no value a caller can use, so the skip costs no
/// soundness.
///
/// Everything else is SKIPPED (no guard): a bare `Named` that is a type alias or
/// the enclosing function's own type parameter, `Map`, function types, qualified
/// names, and literal types — those resolve through engine-specific machinery
/// (the interpreter's lexical alias scope chain vs the emitter's module table +
/// block-shadow poison) and could diverge, so NEITHER engine guards them. This
/// keeps the two engines' skip sets provably identical (run == build). `src` is
/// the defining module's source the type spans index into.
pub fn boundary_guardable(ty: &Type, src: &str, type_params: &[Ident]) -> bool {
    fn name_at(src: &str, span: Span) -> &str {
        &src[span.lo as usize..span.hi as usize]
    }
    // A `Named` whose head SHADOWS one of the enclosing function's type
    // parameters (e.g. `function id<int>(x: int)`) is GENERIC: the checker
    // resolves the type parameter BEFORE the primitive, so `int` here denotes
    // `T`, not the scalar. Such a type must be skipped exactly like any other
    // generic — even when it is SPELLED like a builtin scalar or container —
    // or the guard would reject a value the checker accepts (a false fault that
    // is identical in both engines, so run == build, but wrong against the spec).
    let is_type_param = |name: &str| type_params.iter().any(|tp| name_at(src, tp.span) == name);
    match &ty.kind {
        // `()` is NOT runtime-decidable here — a `for`/`while` expression is
        // checker-typed `()` but runs to a non-`Unit` value (see the doc above).
        TypeKind::Unit => false,
        TypeKind::Named { name, .. } if is_type_param(name_at(src, name.span)) => false,
        TypeKind::Named { name, args } if args.is_empty() => {
            matches!(name_at(src, name.span), "int" | "float" | "string" | "bool")
        }
        TypeKind::Named { name, args } => match (name_at(src, name.span), args.as_slice()) {
            ("Option", [inner]) | ("Array", [inner]) | ("Set", [inner]) => {
                boundary_guardable(inner, src, type_params)
            }
            ("Result", [ok, err]) => {
                boundary_guardable(ok, src, type_params)
                    && boundary_guardable(err, src, type_params)
            }
            _ => false,
        },
        TypeKind::Record(fields) => fields
            .iter()
            .all(|f| boundary_guardable(&f.ty, src, type_params)),
        TypeKind::Union(members) => members
            .iter()
            .all(|m| boundary_guardable(m, src, type_params)),
        _ => false,
    }
}
