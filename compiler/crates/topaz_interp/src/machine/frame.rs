use super::*;

/// Continuation frames. `Eval`/`Exec` dispatch; `K*` frames resume
/// after sub-values arrive on the value stack. The interpreter's
/// value model is lifetime-free (CDR-006 E-1b): frames own their AST
/// fragments as reference-counted clones (`Rc<Expr>`/`Rc<Block>`/…),
/// so a closure that owns its body can be executed by frames that no
/// longer borrow the program.
pub(super) enum Frame {
    Eval(Rc<Expr>),
    /// Execute one statement borrowed from an already shared block. Keeping the
    /// owner and index together avoids rebuilding the statement subtree solely
    /// to satisfy the frame lifetime.
    ExecBlockStatement {
        block: Rc<Block>,
        idx: usize,
    },
    /// Restore a source view after a cross-module expression subtree finishes.
    RestoreSource(Rc<str>),
    /// Restore an environment after a definition-site default expression finishes.
    RestoreEnv(EnvRef),
    /// Restore the saved environment on normal flow (scope exit);
    /// drains the exiting scope's deferred actions first (§14).
    PopScope(EnvRef),
    /// Resume an in-progress unwind after a deferred action ran.
    KUnwind(UnwindAction),
    /// §15 cooperative round-robin scheduler round.
    KConcurrent(Box<ConcurrentState>),

    /// Sequence a block: run stmt `idx`, then the rest, then `tail`.
    KBlock {
        block: Rc<Block>,
        idx: usize,
    },
    KDiscard,
    KLet {
        name: Ident,
        mutable: bool,
    },
    KLetPattern {
        pattern: Rc<Pattern>,
        mutable: bool,
        span: Span,
    },
    KUsingBind {
        name: Ident,
        body: Rc<Block>,
        saved: EnvRef,
        span: Span,
    },
    /// §4/§5 member assignment, pure record chain on an Ident root:
    /// pops the value, functionally updates root.f1.f2..., rebinds.
    KRecordPathAssign {
        root: Rc<Expr>,
        fields: Vec<Ident>,
        op: AssignOp,
        span: Span,
        /// Pre-RHS read of the leaf slot for compound ops.
        current: Option<Value>,
    },
    /// §4/§5 member/index assignment through an Array cell: stack
    /// holds [.., base, index]. Resolves the slot (and for compound
    /// ops and ??= pre-reads the leaf) BEFORE the RHS evaluates.
    KCellPathAssign {
        fields: Vec<Ident>,
        op: AssignOp,
        value: Rc<Expr>,
        span: Span,
    },
    /// Deferred cell write with base/index already captured (the
    /// target reference evaluated exactly once); `current` carries
    /// the pre-RHS leaf read for compound ops.
    KCellWrite {
        base: Value,
        index: Value,
        fields: Vec<Ident>,
        op: AssignOp,
        current: Option<Value>,
        span: Span,
    },
    KAssign {
        target: Rc<Expr>,
        op: AssignOp,
        span: Span,
        /// §2: compound assignments read the target BEFORE the RHS
        /// evaluates (read-operation-write, left to right).
        current: Option<Value>,
    },
    KIf {
        then_block: Rc<Block>,
        else_branch: Option<Rc<Expr>>,
        span: Span,
    },
    KWhile {
        cond: Rc<Expr>,
        body: Rc<Block>,
        span: Span,
    },
    /// Loop boundary marker for `break`/`continue` unwinding.
    LoopBody {
        cond: Rc<Expr>,
        body: Rc<Block>,
        span: Span,
        vstack: usize,
    },
    /// The boundary marker for a `loop` expression. On normal body
    /// completion it re-enters the body (infinite); a `break` targeting it yields
    /// the break value as the loop expression's result, a `continue` re-enters the
    /// body. `label` is the loop's optional label NAME (so a labeled `break 'l`
    /// from a nested loop resolves here).
    LoopExprBody {
        body: Rc<Block>,
        label: Option<String>,
        span: Span,
        vstack: usize,
    },
    KUnary {
        op: UnaryOp,
        span: Span,
    },
    KBinaryRhs {
        op: BinaryOp,
        rhs: Rc<Expr>,
        span: Span,
    },
    KBinaryApply {
        op: BinaryOp,
        span: Span,
    },
    KInterp {
        lit: Rc<StringLit>,
        idx: usize,
        buf: String,
    },
    /// §16 tagged-template construction: collect decoded parts and
    /// evaluated interpolation values.
    KTemplate {
        lit: Rc<StringLit>,
        idx: usize,
        tag: String,
        parts: Vec<String>,
        buf: String,
        values: Vec<Value>,
    },
    KArray {
        elements: Rc<[ArrayElement]>,
        idx: usize,
        acc: Vec<Value>,
        spread: bool,
        span: Span,
    },
    /// §6 (v5.4) `set { e, e, … }` LITERAL: evaluate the elements LEFT TO RIGHT,
    /// accumulating their values; at the end, build the set through the SHARED
    /// `builtin_set_of` leaf (duplicates SILENTLY collapse). `span` is the
    /// literal's span (where a non-keyable element faults).
    KSetLiteral {
        elements: Rc<[Expr]>,
        idx: usize,
        acc: Vec<Value>,
        span: Span,
    },
    /// §6 (v5.4) `map { k: v, … }` LITERAL: evaluate each entry's KEY then VALUE
    /// in source order, accumulating `(key, value)` pairs; at the end, build the
    /// map through the SHARED `builtin_map_of` leaf (a DUPLICATE key faults
    /// TPZ4601). `pending_key` holds the just-evaluated key while its value is
    /// being evaluated. `span` is the literal's span.
    KMapLiteral {
        entries: Rc<[(Expr, Expr)]>,
        idx: usize,
        acc: Vec<(Value, Value)>,
        pending_key: Option<Value>,
        span: Span,
    },
    /// §6 (v5.4) `map { … }` LITERAL helper: the KEY of `entries[idx]` is on the
    /// value stack; pop it and start evaluating that entry's VALUE (re-entering
    /// `KMapLiteral` with the key pending). Splitting key/value across two frames
    /// keeps the value stack discipline simple and source-ordered.
    KMapLiteralKey {
        entries: Rc<[(Expr, Expr)]>,
        idx: usize,
        acc: Vec<(Value, Value)>,
        span: Span,
    },
    /// §6.4 (v5.4) COMPREHENSION clause driver: process `clauses[idx]` (a `for`
    /// iteration or an `if` filter), recursing to `idx + 1` per surviving
    /// iteration; at `idx == len`, evaluate the `body` and append to the TOP
    /// accumulator in `self.comp_accs`. Pushes/runs no `values`-stack entry — the
    /// loops drive purely for effect; the final collection comes from `KCompFinish`.
    KCompClause {
        kind: CompKind,
        clauses: Rc<[CompClause]>,
        idx: usize,
        body: Rc<CompBody>,
        span: Span,
    },
    /// §6.4 `for`-clause start: the iterable is on the stack; materialize it via the
    /// SHARED `for_items` leaf (same not-iterable fault as a real `for`) and begin
    /// iterating clause `idx`'s elements.
    KCompForStart {
        kind: CompKind,
        clauses: Rc<[CompClause]>,
        idx: usize,
        body: Rc<CompBody>,
        span: Span,
    },
    /// §6.4 `for`-clause driver: bind `items[next]` against the clause pattern in a
    /// fresh per-iteration scope, recurse into clause `idx + 1`, then advance.
    KCompForNext {
        kind: CompKind,
        clauses: Rc<[CompClause]>,
        idx: usize,
        body: Rc<CompBody>,
        items: Rc<Vec<Value>>,
        next: usize,
        span: Span,
    },
    /// §6.4 `if`-clause: the condition is on the stack; recurse into clause `idx + 1`
    /// only when it is `true` (the §5 bool guard is the SHARED `condition_bool` leaf).
    KCompIf {
        kind: CompKind,
        clauses: Rc<[CompClause]>,
        idx: usize,
        body: Rc<CompBody>,
        span: Span,
    },
    /// §6.4 yield helper for a MAP comprehension: the KEY is on the stack; pop it and
    /// evaluate the VALUE, then append `(key, value)` to the top accumulator.
    KCompYieldMapValue {
        value: Rc<Expr>,
    },
    /// §6.4 yield: pop the just-evaluated body element (array/set) or map value and
    /// append it to the TOP accumulator. `pending_key` is `Some(key)` for a map (the
    /// key awaiting this value) and `None` for an array/set element — which alone tells
    /// the accumulator shape. The iteration continues — nothing is left on the stack.
    KCompYield {
        pending_key: Option<Value>,
    },
    /// §6.4 finalize: pop the top `self.comp_accs` entry and build the final value
    /// through the SAME shared leaf the literal uses (array / `builtin_set_of` /
    /// `builtin_map_of`), pushing it as the comprehension's value.
    KCompFinish {
        kind: CompKind,
        span: Span,
    },
    KRecord {
        fields: Rc<[FieldInit]>,
        idx: usize,
        acc: Vec<(String, Value)>,
        base: Option<Rc<BTreeMap<String, Value>>>,
        span: Span,
    },
    KRecordUpdateBase {
        fields: Rc<[FieldInit]>,
        span: Span,
    },
    /// §3 (v5.4) NOMINAL record construction `User { … }`: evaluate the build
    /// PLAN's exprs in their DETERMINISTIC order (explicit fields L→R, then missing
    /// defaults in decl-order — the SAME order the emitter uses), accumulate
    /// `(field, value)`, then assemble the value in DECLARATION order. `plan` is
    /// the ordered `(field name, expr)` list; `decl_order` is the final field
    /// order for assembly.
    KNominalRecord {
        record_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        plan: Rc<[NominalFieldPlan]>,
        idx: usize,
        acc: Vec<(Rc<str>, Value)>,
        decl_order: Rc<[Rc<str>]>,
        span: Span,
    },
    /// §3 (v5.4) NOMINAL spread-update `User { ...base, … }`: the spread base has
    /// just been evaluated; validate it is a `NominalRecord` of the SAME id, seed
    /// the accumulator with its fields (so explicit fields/defaults can override),
    /// then continue with the explicit-fields/defaults `KNominalRecord` plan. The
    /// validation faults BYTE-IDENTICALLY to the emitter under `--unchecked`.
    KNominalSpread {
        record_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        plan: Rc<[NominalFieldPlan]>,
        decl_order: Rc<[Rc<str>]>,
        span: Span,
    },
    KRecordDefaultExit,
    KMember {
        field: Ident,
        span: Span,
        /// §9 root of a mutator-method access (`recv.push`), so the
        /// collection-mutator arms can require `let mut`.
        root: Option<Rc<str>>,
    },
    KOptional {
        field: Ident,
        span: Span,
        root: Option<Rc<str>>,
    },
    KIndexObj {
        index: Rc<Expr>,
        span: Span,
    },
    KIndexApply {
        span: Span,
    },
    KCallArgs {
        args: Rc<[CallArg]>,
        idx: usize,
        acc: Vec<Value>,
        named: Vec<(Rc<str>, Value)>,
        spread: Vec<Value>,
        seen_spread: bool,
        span: Span,
    },
    KPositionalArgs {
        args: Rc<[CallArg]>,
        idx: usize,
    },
    KCallApplyWithArg {
        arg: Value,
        span: Span,
    },
    KJsonDecode {
        schema: Rc<Schema>,
        span: Span,
        parse_text: bool,
    },
    KCtor {
        name: Rc<str>,
        span: Span,
    },
    /// §3 (v5.3/v5.4) an N-payload enum construction `Bin(a, b, c)`: after the N
    /// payload args evaluate (left-to-right onto the value stack), pop them in
    /// reverse and wrap into a `Value::Enum` with this nominal id + variant.
    /// Payload-less construction does not use this frame (it builds the value
    /// directly in `member_access`).
    KEnumCtor {
        enum_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
        variant: Rc<str>,
        variant_index: u32,
        arity: usize,
    },
    /// §3 (v5.4) a newtype construction `UserId(5)`: after the single arg evaluates
    /// onto the value stack, wrap it into a `Value::Newtype` with this nominal id.
    KNewtypeCtor {
        newtype_id: Rc<str>,
        declaration_identity: Option<Rc<str>>,
        method_identity: Option<Rc<str>>,
    },
    /// §4 (v5.4) a user METHOD call `recv.m(args)`: after the RECEIVER evaluates onto
    /// the value stack, read its runtime nominal id, look up the method `(id, m)`,
    /// and schedule the method closure call with the receiver prepended as the first
    /// argument (STATIC dispatch → a free call). A non-nominal receiver or an absent
    /// method falls back to ordinary member access (so `--unchecked` run≡build with
    /// the emitter, which faults at the member-access leaf).
    KMethodCall {
        field: Ident,
        args: Rc<[CallArg]>,
        /// The CALL expression's span (`recv.m(args)`) — the dispatched method call's
        /// fault span.
        span: Span,
        /// The MEMBER expression's span (`recv.m`) — the no-member fallback's fault
        /// span, so an absent-member fault matches the emitter (which uses the member
        /// span) byte-identically.
        member_span: Span,
        /// §9 the mutator root (for the no-method fallback to ordinary member access
        /// — so a user-method-NAME that is actually a builtin mutator on a non-nominal
        /// receiver still enforces `let mut`).
        root: Option<Rc<str>>,
    },
    /// §4 (v5.4) a PROTOCOL static dispatch `Show.show(x)` / `Order.compare(a, b)`:
    /// after the `arity` positional args evaluate onto the value stack (last on top),
    /// read arg0's runtime nominal id and dispatch — a MANUAL impl `("{protocol}<
    /// {id}>", method)` in `method_defs` is called with all args; else the DERIVED
    /// `builtin_protocol_dispatch` leaf runs (Show→render, Eq→values_equal,
    /// Order→values_compare). Byte-identical to the emitter (run≡build).
    KProtocolCall {
        protocol: Rc<str>,
        method: Rc<str>,
        arity: usize,
        span: Span,
    },
    /// §12 optional call `recv?.m(args)`: after the receiver
    /// evaluates, short-circuit None/null, else call on the
    /// unwrapped value (KWrapSome restores the Option container).
    KOptionalCall {
        field: Ident,
        args: Rc<[CallArg]>,
        span: Span,
        root: Option<Rc<str>>,
        /// §11 first-argument insertion when this optional call is a
        /// pipe stage: the piped value becomes the first positional.
        lead: Option<Value>,
    },
    /// Wrap an optional-call result back into `Some` (§12 container
    /// preservation on an `Option` receiver).
    KWrapSome,
    /// Shared `map`/`filter`/`reduce` callback result, resumed through the
    /// shared callback-HOF state machine.
    KCallbackHof {
        pending: CallbackHofPending,
        span: Span,
    },
    /// A `sortedBy` or `sortBy` callback key, resumed through the shared key-
    /// collection state machine. The destination selects return-new or write-back
    /// only after every key has been collected and the stable sort succeeds.
    KCallbackKey {
        pending: CallbackKeyPending,
        destination: CallbackKeyDestination,
        span: Span,
    },
    /// A `retain` predicate result resumed through the shared evaluator-independent
    /// state. The receiver cell is written only when every predicate succeeds.
    KCallbackRetain {
        cell: Rc<RefCell<Vec<Value>>>,
        pending: CallbackRetainPending,
        span: Span,
    },
    /// A `Map.filter` or `mapValues` callback result resumed through the shared
    /// pair-order and ordered-result state machine.
    KCallbackMapHof {
        pending: CallbackMapHofPending,
        span: Span,
    },
    /// A present-key `Map.update` callback result resumed through the shared
    /// probe/commit transition. The absent-key path completes without a frame.
    KCallbackMapUpdate {
        pending: CallbackMapUpdatePending,
        span: Span,
    },
    /// An `Option.okOrElse` callback result resumed through the shared lazy
    /// Option-to-Result transition.
    KCallbackOkOrElse {
        pending: CallbackOkOrElsePending,
    },
    /// An `Option.map` or `Result.map` callback result resumed through the
    /// shared receiver-map transition.
    KCallbackReceiverMap {
        pending: CallbackReceiverMapPending,
    },
    /// Function-call boundary: restores env and catches `return`.
    /// `vstack` is the operand-stack height at entry — unwinding
    /// truncates to it so callee-local partials never leak.
    CallBoundary {
        saved: EnvRef,
        vstack: usize,
        saved_src: Rc<str>,
        /// The caller's type-param scope, restored when this boundary pops.
        saved_type_params: Rc<[Ident]>,
        /// §6 return guard: the declared return type paired with the
        /// callee's source it spans into — `Some` ONLY when that type is
        /// `boundary_guardable` (concrete, alias-free, not the function's own
        /// type parameter). The value leaving the call — the body tail
        /// (normal completion) OR any unwound `return` / `?` / case-arm return —
        /// is checked against it HERE, the single choke point every return path
        /// funnels through, AFTER this scope's defers drain. The emitter guards
        /// the matching unified `__ret`, so the fault is byte-identical.
        return_guard: Option<(Type, Rc<str>)>,
    },
    KReturn {
        span: Span,
    },
    /// Pops the evaluated `break <value>` value, then starts a `Break`
    /// unwind carrying it (and the optional resolved label).
    KBreak {
        span: Span,
        label: Option<String>,
    },
    KPipe {
        rhs: Rc<PipeRhs>,
        span: Span,
        /// §9 root for a `coll |> .push` mutator-field pipe.
        root: Option<Rc<str>>,
    },
    /// Scrutinee pending on the value stack.
    KMatchDispatch {
        cases: Rc<[CaseClause]>,
        span: Span,
    },
    /// lhs,rhs of `>>` evaluated; build the composed value.
    KComposePair,
    /// `for` iterable evaluated; snapshot and start.
    KForStart {
        pattern: Rc<Pattern>,
        body: Rc<Block>,
        span: Span,
        is_stmt: bool,
    },
    /// Match dispatch: scrutinee evaluated, try cases from `idx`.
    KMatchCase {
        scrutinee: Value,
        cases: Rc<[CaseClause]>,
        idx: usize,
        span: Span,
    },
    /// Guard evaluated; bindings scope already pushed.
    KMatchGuard {
        scrutinee: Value,
        cases: Rc<[CaseClause]>,
        idx: usize,
        span: Span,
    },
    /// `?` propagation (§13).
    KTry {
        span: Span,
    },
    /// Apply `g` to the result of `f` (§11 compose call).
    KComposeAfter {
        g: Value,
        span: Span,
    },
    /// Range endpoints evaluated (lo, hi on stack; step next/None).
    KRange {
        inclusive: bool,
        step: Option<Rc<Expr>>,
        span: Span,
    },
    KRangeStep {
        inclusive: bool,
        span: Span,
    },
    /// For-loop driver: iterate `items` from `next`, collecting body
    /// values (§5 for-expression).
    KForNext {
        pattern: Rc<Pattern>,
        body: Rc<Block>,
        items: Rc<Vec<Value>>,
        next: usize,
        acc: Vec<Value>,
        span: Span,
        is_stmt: bool,
    },
    /// For-loop boundary for break/continue; collects body value.
    ForBody {
        pattern: Rc<Pattern>,
        body: Rc<Block>,
        items: Rc<Vec<Value>>,
        next: usize,
        acc: Vec<Value>,
        span: Span,
        vstack: usize,
        is_stmt: bool,
    },
}

#[derive(Clone, Copy)]
pub(super) enum FrameFamily {
    Lifecycle,
    Value,
    Aggregate,
    AccessAndCall,
    HigherOrder,
    CallBoundary,
    PatternControl,
    PipeAndDecode,
}

impl Frame {
    pub(super) fn family(&self) -> FrameFamily {
        match self {
            Frame::ExecBlockStatement { .. }
            | Frame::Eval(_)
            | Frame::RestoreSource(_)
            | Frame::RestoreEnv(_)
            | Frame::PopScope(_)
            | Frame::KUnwind(_)
            | Frame::KConcurrent(_)
            | Frame::KDiscard
            | Frame::KBlock { .. }
            | Frame::KLet { .. }
            | Frame::KLetPattern { .. }
            | Frame::KUsingBind { .. }
            | Frame::KAssign { .. }
            | Frame::KRecordPathAssign { .. }
            | Frame::KCellPathAssign { .. }
            | Frame::KCellWrite { .. } => FrameFamily::Lifecycle,
            Frame::KIf { .. }
            | Frame::KWhile { .. }
            | Frame::LoopBody { .. }
            | Frame::LoopExprBody { .. }
            | Frame::KUnary { .. }
            | Frame::KBinaryRhs { .. }
            | Frame::KBinaryApply { .. }
            | Frame::KInterp { .. }
            | Frame::KTemplate { .. }
            | Frame::KArray { .. }
            | Frame::KSetLiteral { .. }
            | Frame::KMapLiteral { .. }
            | Frame::KMapLiteralKey { .. } => FrameFamily::Value,
            Frame::KCompClause { .. }
            | Frame::KCompForStart { .. }
            | Frame::KCompForNext { .. }
            | Frame::KCompIf { .. }
            | Frame::KCompYieldMapValue { .. }
            | Frame::KCompYield { .. }
            | Frame::KCompFinish { .. }
            | Frame::KRecord { .. }
            | Frame::KRecordUpdateBase { .. }
            | Frame::KNominalRecord { .. }
            | Frame::KRecordDefaultExit
            | Frame::KNominalSpread { .. } => FrameFamily::Aggregate,
            Frame::KMember { .. }
            | Frame::KOptional { .. }
            | Frame::KIndexObj { .. }
            | Frame::KIndexApply { .. }
            | Frame::KCallArgs { .. }
            | Frame::KPositionalArgs { .. }
            | Frame::KCtor { .. }
            | Frame::KEnumCtor { .. }
            | Frame::KNewtypeCtor { .. }
            | Frame::KMethodCall { .. }
            | Frame::KProtocolCall { .. }
            | Frame::KOptionalCall { .. }
            | Frame::KWrapSome => FrameFamily::AccessAndCall,
            Frame::KCallbackHof { .. }
            | Frame::KCallbackKey { .. }
            | Frame::KCallbackRetain { .. }
            | Frame::KCallbackMapHof { .. }
            | Frame::KCallbackMapUpdate { .. }
            | Frame::KCallbackOkOrElse { .. }
            | Frame::KCallbackReceiverMap { .. } => FrameFamily::HigherOrder,
            Frame::CallBoundary { .. } | Frame::KReturn { .. } | Frame::KBreak { .. } => {
                FrameFamily::CallBoundary
            }
            Frame::KMatchDispatch { .. }
            | Frame::KComposePair
            | Frame::KForStart { .. }
            | Frame::KMatchCase { .. }
            | Frame::KMatchGuard { .. }
            | Frame::KTry { .. }
            | Frame::KComposeAfter { .. }
            | Frame::KRange { .. }
            | Frame::KRangeStep { .. }
            | Frame::KForNext { .. }
            | Frame::ForBody { .. } => FrameFamily::PatternControl,
            Frame::KPipe { .. } | Frame::KCallApplyWithArg { .. } | Frame::KJsonDecode { .. } => {
                FrameFamily::PipeAndDecode
            }
        }
    }
}
