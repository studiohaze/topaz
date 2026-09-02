//! `topaz_hir` — engine-neutral checked and lowered compiler IR.
//!
//! [`CallPlan`] is the shared call-shape boundary: it records what is evaluated,
//! in what order, and how written arguments map to source positions. The checker
//! attaches semantic types and the resolved target in [`TypedCall`], checked
//! emission retains the same plan, and the native emitter consumes its argument
//! shape for call families that require source-order evaluation.
//!
//! This crate carries the checker-owned typed IR, call-lowering facts, and the
//! whole-program lowered unit consumed by target emitters. The canonical
//! observation schema, rather than these Rust layouts, is the engine-neutral
//! comparison boundary.
//!
//! Dependency direction is fixed: `topaz_hir` sees the AST and diagnostic spans,
//! never `topaz_check`. The checker depends on HIR and enriches these structural
//! facts after type checking; emitters can therefore consume checked HIR without
//! acquiring a checker dependency.

pub mod emission;
pub mod lowered;
pub mod mono;
pub use lowered::{
    LoweredBinding, LoweredControl, LoweredControlKind, LoweredExpressionKind, LoweredModule,
    LoweredOperation, LoweredOperationKind, LoweredPatternKind, LoweredRole, LoweredStorage,
    LoweredUnit, RuntimeLeaf, RuntimeRegistry, RuntimeTemplate,
};
pub use mono::{
    MonoTy, SemanticConstructor, SemanticField, SemanticLiteral, SemanticPrimitive, SemanticType,
    TypedByteField, TypedByteProjection, TypedByteRecordParam, TypedCall, TypedCapture, TypedLocal,
    TypedNode, TypedNodeKind, TypedUnit,
};

use topaz_diag::Span;
use topaz_syntax::ast::{
    ArrayElement, Block, CallArg, CaseArmBody, CompBody, CompClause, Expr, ExprKind, PipeRhs,
    Program, Stmt, StmtKind, StringPart, call_args_contain_placeholder,
};

/// A lowered call: its callee shape, the ordered evaluation steps, the argument
/// shapes (in SOURCE order), and how it binds. Spans index the defining source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPlan {
    pub span: Span,
    pub callee_span: Span,
    pub callee: CalleePlan,
    /// The evaluation steps, in order. NOT a flat "args in source order" list —
    /// member resolution, the optional short-circuit, and the callee/receiver
    /// distinction are explicit phases (the eval-order bugs live exactly here).
    pub eval: Vec<EvalStep>,
    /// The arguments in SOURCE order (so a reordered-named call is representable
    /// faithfully — `reduce(f: …, initial: …)` keeps `f` at source index 0).
    pub args: Vec<ArgPlan>,
}

impl CallPlan {
    /// Whether one resolved source reference can name this call's callable.
    /// Pipeline plans expose the whole stage as `callee_span`, including written
    /// arguments, while member calls also contain a receiver reference. Neither
    /// is a callable target: written arguments are excluded and a member target
    /// must name the method itself.
    pub fn admits_callee_reference(&self, name: &str, span: Span) -> bool {
        if self.args.iter().any(|arg| {
            arg.source_index.is_some()
                && arg.span.file == span.file
                && span.lo >= arg.span.lo
                && span.hi <= arg.span.hi
        }) {
            return false;
        }
        match &self.callee {
            CalleePlan::Member { method, .. } => name == method,
            CalleePlan::Pipe {
                stage_method: Some(method),
            } => name == method,
            CalleePlan::Value | CalleePlan::Pipe { stage_method: None } => true,
        }
    }
}

/// What sits in callee position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalleePlan {
    /// `f(...)` — the callee is any expression value (free fn, builtin, closure
    /// local, composed function). Bound at run time.
    Value,
    /// `recv.method(...)` (or `recv?.method(...)` when `optional`). `shadow_first`
    /// records that a record field named `method` shadows the built-in method
    /// (member_value-first) — the semantics the shadow-label bug turned on.
    Member {
        method: String,
        class: MethodClass,
        optional: bool,
        shadow_first: bool,
    },
    /// `lead |> stage(...)` — the pipe lead is either inserted as an argument or
    /// bound to `_` for written argument expressions. `stage_method` is the
    /// stage's method name when the stage is itself a member call
    /// (`x |> xs.get()`).
    Pipe { stage_method: Option<String> },
}

/// The danger-zone classification of a member method — enough, by NAME alone (no
/// checker), to snapshot the call families where the emitter currently special-cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodClass {
    /// `map` / `filter` / `reduce` / `flatMap` — the name-only higher-order
    /// family. `flatMap` is accepted for Option/Result receivers here; this
    /// classifier is not an Array.flatMap surface declaration.
    Hof,
    /// `okOrElse` — the callback ARGUMENT is evaluated eagerly, but its INVOCATION
    /// is lazy (only the `None` branch calls it). Eval order ≠ invocation order.
    LazyCallback,
    /// A known mutating receiver method (`push`/`insert`/`set`/…).
    Mutator,
    /// A host-resource method (`read`/`write`/`close`/`open`).
    Resource,
    /// Any other member name.
    Other,
}

/// One argument's shape. `source_index` is its position among the WRITTEN args
/// (None only for a pipe lead, which has no written slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgPlan {
    pub source_index: Option<usize>,
    pub binding: ArgBinding,
    pub span: Span,
}

/// How an argument is supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgBinding {
    Positional,
    Named(String),
    Spread,
    /// The pipe lead, inserted as the leading argument with no written source
    /// slot. Placeholder-bearing stages keep every written argument unchanged
    /// and instead evaluate [`EvalStep::PipeLead`].
    InsertedLead,
}

/// One evaluation step, in order. The optional short-circuit and the
/// receiver-before-args rule are first-class so a consumer cannot reorder them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalStep {
    /// Evaluate the callee value (a free/value call).
    Callee,
    /// Evaluate the receiver (a member or pipe call) before any argument.
    Receiver,
    /// Optional access: a `None`/`null` receiver SKIPS every remaining step
    /// (arguments are not evaluated).
    OptionalGuard,
    /// Evaluate a pipeline's left-hand value once and bind it to `_` while the
    /// written stage arguments are evaluated. This is distinct from an inserted
    /// argument because `_` can occur inside an argument expression.
    PipeLead,
    /// Evaluate `args[i]` — `i` indexes [`CallPlan::args`], which is source order.
    Arg(usize),
}

/// The source text a span covers.
fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

/// Classify a member name into its emitter danger zone — by NAME only.
fn classify_method(name: &str) -> MethodClass {
    match name {
        "map" | "filter" | "reduce" | "flatMap" => MethodClass::Hof,
        "okOrElse" => MethodClass::LazyCallback,
        "push" | "insert" | "set" | "remove" | "update" | "clear" | "extend" | "add" => {
            MethodClass::Mutator
        }
        "read" | "write" | "close" | "open" => MethodClass::Resource,
        _ => MethodClass::Other,
    }
}

/// Lower a written argument list into source-ordered [`ArgPlan`]s.
fn lower_args(args: &[CallArg], src: &str) -> Vec<ArgPlan> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let (binding, span) = match a {
                CallArg::Positional(e) => (ArgBinding::Positional, e.span),
                CallArg::Spread(e) => (ArgBinding::Spread, e.span),
                CallArg::Named { name, value } => (
                    ArgBinding::Named(text(src, name.span).to_string()),
                    value.span,
                ),
            };
            ArgPlan {
                source_index: Some(i),
                binding,
                span,
            }
        })
        .collect()
}

/// The callee plan + whether the call is an optional member access.
fn lower_callee(callee: &Expr, src: &str) -> (CalleePlan, bool) {
    match &callee.kind {
        ExprKind::Member { field, .. } => (member_callee(text(src, field.span), false), false),
        ExprKind::OptionalAccess { field, .. } => {
            (member_callee(text(src, field.span), true), true)
        }
        _ => (CalleePlan::Value, false),
    }
}

fn member_callee(method: &str, optional: bool) -> CalleePlan {
    let class = classify_method(method);
    // The emitter resolves a record field FIRST (member_value-first) for the
    // families that can be shadowed by a same-named field; record that here.
    let shadow_first = matches!(class, MethodClass::Hof | MethodClass::LazyCallback);
    CalleePlan::Member {
        method: method.to_string(),
        class,
        optional,
        shadow_first,
    }
}

/// The ordered eval steps for a direct (non-pipe) call.
fn direct_eval(callee: &CalleePlan, optional: bool, n_args: usize) -> Vec<EvalStep> {
    let mut steps = Vec::with_capacity(n_args + 2);
    match callee {
        CalleePlan::Value => steps.push(EvalStep::Callee),
        CalleePlan::Member { .. } | CalleePlan::Pipe { .. } => steps.push(EvalStep::Receiver),
    }
    if optional {
        steps.push(EvalStep::OptionalGuard);
    }
    for i in 0..n_args {
        steps.push(EvalStep::Arg(i));
    }
    steps
}

/// Lower a call expression into a [`CallPlan`]. Handles direct calls
/// (`ExprKind::Call`) and pipe stages (`ExprKind::Pipe`); returns `None` for any
/// non-call expression (including `lhs |> .field` sugar, which is member access).
pub fn lower_call_expr(expr: &Expr, src: &str) -> Option<CallPlan> {
    match &expr.kind {
        ExprKind::Call { callee, args, .. } => {
            let (callee_plan, optional) = lower_callee(callee, src);
            let arg_plans = lower_args(args, src);
            let eval = direct_eval(&callee_plan, optional, arg_plans.len());
            Some(CallPlan {
                span: expr.span,
                callee_span: callee.span,
                callee: callee_plan,
                eval,
                args: arg_plans,
            })
        }
        ExprKind::Pipe { lhs, rhs } => lower_pipe(expr.span, lhs, rhs, src),
        _ => None,
    }
}

/// Lower `lhs |> stage` where the stage is a call (or a bare callee value).
///
/// Placement (SPEC §11): when a written argument expression contains `_`, the
/// lead is bound to `_` while every written argument is evaluated unchanged
/// (`x |> f(a, _, b)` → `f(a, x, b)`, and `x |> f({_})` → `f({x})`). Otherwise
/// the lead is inserted as the leading argument (`x |> f(a)` → `f(x, a)`).
///
/// Evaluation follows the language contract: the lead is evaluated first, then
/// the stage receiver, then the remaining arguments in source order.
fn lower_pipe(span: Span, lhs: &Expr, rhs: &PipeRhs, src: &str) -> Option<CallPlan> {
    let stage = match rhs {
        PipeRhs::Expr(e) => e,
        // `lhs |> .field` is member-access sugar, not a call.
        PipeRhs::Field(_) => return None,
    };
    let (stage_method, optional, raw_args): (Option<String>, bool, &[CallArg]) = match &stage.kind {
        ExprKind::Call { callee, args, .. } => {
            let (method, optional) = match &callee.kind {
                ExprKind::Member { field, .. } => (Some(text(src, field.span).to_string()), false),
                ExprKind::OptionalAccess { field, .. } => {
                    (Some(text(src, field.span).to_string()), true)
                }
                _ => (None, false),
            };
            (method, optional, args.as_slice())
        }
        // `lhs |> f` (no parens) — `f` applied to the lead, no written args.
        _ => (None, false, &[]),
    };

    let mut args = lower_args(raw_args, src);
    let has_placeholder = call_args_contain_placeholder(raw_args);
    let mut eval = Vec::with_capacity(args.len() + 3);
    if has_placeholder {
        eval.push(EvalStep::PipeLead);
        eval.push(EvalStep::Receiver);
        if optional {
            eval.push(EvalStep::OptionalGuard);
        }
        for i in 0..args.len() {
            eval.push(EvalStep::Arg(i));
        }
    } else {
        args.insert(
            0,
            ArgPlan {
                source_index: None,
                binding: ArgBinding::InsertedLead,
                span: lhs.span,
            },
        );
        eval.push(EvalStep::Arg(0));
        eval.push(EvalStep::Receiver);
        if optional {
            eval.push(EvalStep::OptionalGuard);
        }
        for i in 1..args.len() {
            eval.push(EvalStep::Arg(i));
        }
    }

    Some(CallPlan {
        span,
        callee_span: stage.span,
        callee: CalleePlan::Pipe { stage_method },
        eval,
        args,
    })
}

/// Lower EVERY call and pipe stage in a program into [`CallPlan`]s, in a
/// deterministic pre-order walk (outer call before its nested arguments). This
/// is the analysis/audit entry — a consumer that lowers per-call during emit
/// uses [`lower_call_expr`] directly; this walk is for whole-program passes.
///
/// Excluded by design: pattern payloads (a literal/range pattern's constant
/// endpoint exprs). Patterns hold match-time constants, not executed call sites,
/// so they carry no runtime calls to lower.
pub fn collect_call_plans(program: &Program, src: &str) -> Vec<CallPlan> {
    let mut out = Vec::new();
    for stmt in &program.items {
        walk_stmt(stmt, src, &mut out);
    }
    out
}

fn walk_stmt(stmt: &Stmt, src: &str, out: &mut Vec<CallPlan>) {
    match &stmt.kind {
        StmtKind::Import(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::Enum(_)
        | StmtKind::Record(_)
        | StmtKind::Newtype(_)
        // v5.4 §4 protocol: a declaration of method SIGNATURES (empty bodies) — no
        // call sites.
        | StmtKind::Protocol(_)
        | StmtKind::Continue { .. } => {}
        // A `break <value>` value can contain calls.
        StmtKind::Break { value, .. } => {
            if let Some(e) = value {
                walk_expr(e, src, out);
            }
        }
        StmtKind::Export(inner) => walk_stmt(inner, src, out),
        StmtKind::Function(decl) => {
            for p in &decl.params {
                if let Some(d) = &p.default {
                    walk_expr(d, src, out);
                }
            }
            walk_block(&decl.body, src, out);
        }
        // v5.4 impl: walk each method body's call sites (a method is lowered like a
        // free function over the receiver).
        StmtKind::Impl(decl) => {
            for m in &decl.methods {
                for p in &m.decl.params {
                    if let Some(d) = &p.default {
                        walk_expr(d, src, out);
                    }
                }
                walk_block(&m.decl.body, src, out);
            }
        }
        StmtKind::Let { value, .. } | StmtKind::Const { value, .. } => walk_expr(value, src, out),
        StmtKind::Using { value, body, .. } => {
            walk_expr(value, src, out);
            walk_block(body, src, out);
        }
        StmtKind::Assign { target, value, .. } => {
            walk_expr(target, src, out);
            walk_expr(value, src, out);
        }
        StmtKind::Return(value) => {
            if let Some(e) = value {
                walk_expr(e, src, out);
            }
        }
        StmtKind::Defer(e) => walk_expr(e, src, out),
        StmtKind::Expr(e) => walk_expr(e, src, out),
        StmtKind::While { cond, body } => {
            walk_expr(cond, src, out);
            walk_block(body, src, out);
        }
    }
}

fn walk_block(block: &Block, src: &str, out: &mut Vec<CallPlan>) {
    for stmt in &block.stmts {
        walk_stmt(stmt, src, out);
    }
    if let Some(tail) = &block.tail {
        walk_expr(tail, src, out);
    }
}

/// Pre-order: lower THIS expr if it is a call/pipe, then recurse into every
/// sub-expression so nested calls (`f(g(x))`, a call in a `"{…}"`, a lambda
/// body) are all collected.
fn walk_expr(expr: &Expr, src: &str, out: &mut Vec<CallPlan>) {
    if let Some(plan) = lower_call_expr(expr, src) {
        out.push(plan);
    }
    walk_children(expr, src, out);
}

/// Recurse into an expression's sub-expressions WITHOUT lowering the expression
/// itself — used for a pipe stage, whose call is already captured by the pipe plan.
fn walk_children(expr: &Expr, src: &str, out: &mut Vec<CallPlan>) {
    match &expr.kind {
        ExprKind::Int
        | ExprKind::Float
        | ExprKind::Duration(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::Unit
        | ExprKind::Ident
        | ExprKind::Placeholder => {}
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(e) = part {
                    walk_expr(e, src, out);
                }
            }
        }
        ExprKind::Paren(e) | ExprKind::Try(e) | ExprKind::Unary { operand: e, .. } => {
            walk_expr(e, src, out)
        }
        ExprKind::Block(b) => walk_block(b, src, out),
        ExprKind::If {
            cond,
            then_block,
            else_branch,
        } => {
            walk_expr(cond, src, out);
            walk_block(then_block, src, out);
            if let Some(e) = else_branch {
                walk_expr(e, src, out);
            }
        }
        ExprKind::Match { scrutinee, cases } => {
            walk_expr(scrutinee, src, out);
            for case in cases {
                if let Some(g) = &case.guard {
                    walk_expr(g, src, out);
                }
                match &case.body {
                    CaseArmBody::Expr(e) => walk_expr(e, src, out),
                    CaseArmBody::Return { value, .. } => {
                        if let Some(e) = value {
                            walk_expr(e, src, out);
                        }
                    }
                }
            }
        }
        ExprKind::For { iter, body, .. } => {
            walk_expr(iter, src, out);
            walk_block(body, src, out);
        }
        // An infinite-loop expression: walk its body for call sites.
        ExprKind::Loop { body, .. } => {
            walk_block(body, src, out);
        }
        ExprKind::Concurrent {
            timeout,
            arms,
            else_block,
        } => {
            if let Some(t) = timeout {
                walk_expr(t, src, out);
            }
            for arm in arms {
                walk_expr(&arm.value, src, out);
            }
            if let Some(b) = else_block {
                walk_block(b, src, out);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            walk_expr(callee, src, out);
            for a in args {
                match a {
                    CallArg::Positional(e)
                    | CallArg::Spread(e)
                    | CallArg::Named { value: e, .. } => walk_expr(e, src, out),
                }
            }
        }
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            walk_expr(object, src, out)
        }
        ExprKind::Index { object, index } => {
            walk_expr(object, src, out);
            walk_expr(index, src, out);
        }
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
            walk_expr(lhs, src, out);
            walk_expr(rhs, src, out);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            walk_expr(lo, src, out);
            walk_expr(hi, src, out);
            if let Some(s) = step {
                walk_expr(s, src, out);
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            walk_expr(lhs, src, out);
            if let PipeRhs::Expr(e) = rhs.as_ref() {
                // The stage call is captured by the pipe's own plan (with the
                // inserted lead); recurse into its CHILDREN only — never re-lower
                // the bare stage as a phantom zero-arg call.
                walk_children(e, src, out);
            }
        }
        ExprKind::Lambda { body, .. } => walk_expr(body, src, out),
        ExprKind::RecordLiteral { fields } => {
            for f in fields {
                walk_expr(&f.value, src, out);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            walk_expr(base, src, out);
            if let Some(spread) = spread {
                walk_expr(spread, src, out);
            }
            for f in fields {
                walk_expr(&f.value, src, out);
            }
        }
        ExprKind::Array(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk_expr(e, src, out),
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for e in elements {
                walk_expr(e, src, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                walk_expr(k, src, out);
                walk_expr(v, src, out);
            }
        }
        ExprKind::Comprehension { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { iter, .. } => walk_expr(iter, src, out),
                    CompClause::If(cond) => walk_expr(cond, src, out),
                }
            }
            match body.as_ref() {
                CompBody::Elem(e) => walk_expr(e, src, out),
                CompBody::Entry { key, value } => {
                    walk_expr(key, src, out);
                    walk_expr(value, src, out);
                }
            }
        }
    }
}

/// A stable, span-free rendering of a [`CallPlan`] for snapshot assertions.
pub fn snapshot(plan: &CallPlan) -> String {
    let callee = match &plan.callee {
        CalleePlan::Value => "value".to_string(),
        CalleePlan::Member {
            method,
            class,
            optional,
            shadow_first,
        } => format!(
            "member({method}{}{} class={class:?})",
            if *optional { " optional" } else { "" },
            if *shadow_first { " shadow_first" } else { "" },
        ),
        CalleePlan::Pipe { stage_method } => {
            format!("pipe(stage={})", stage_method.as_deref().unwrap_or("value"))
        }
    };
    let eval = plan
        .eval
        .iter()
        .map(|s| match s {
            EvalStep::Callee => "callee".to_string(),
            EvalStep::Receiver => "recv".to_string(),
            EvalStep::OptionalGuard => "optguard".to_string(),
            EvalStep::PipeLead => "lead".to_string(),
            EvalStep::Arg(i) => format!("arg{i}"),
        })
        .collect::<Vec<_>>()
        .join(",");
    let args = plan
        .args
        .iter()
        .map(|a| {
            let b = match &a.binding {
                ArgBinding::Positional => "pos".to_string(),
                ArgBinding::Named(n) => format!("named({n})"),
                ArgBinding::Spread => "spread".to_string(),
                ArgBinding::InsertedLead => "lead".to_string(),
            };
            match a.source_index {
                Some(i) => format!("{b}#{i}"),
                None => b,
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("callee={callee} eval=[{eval}] args=[{args}] binding=runtime")
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_diag::FileId;
    use topaz_parser::{ParseOptions, parse_with_options};
    use topaz_syntax::LangVersion;
    use topaz_syntax::ast::StmtKind;

    /// Parse `let _x = <expr>`, lower the init expression, and snapshot it.
    fn snap(expr_src: &str) -> String {
        let full = format!("let _x = {expr_src}\n");
        let out = parse_with_options(
            FileId(0),
            &full,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        let StmtKind::Let { value, .. } = &out.program.items[0].kind else {
            panic!("expected a let statement");
        };
        let plan = lower_call_expr(value, &full).expect("the init is a call");
        snapshot(&plan)
    }

    #[test]
    fn free_call_preserves_callee_before_args_and_named() {
        // Callee evaluates before the args; a named arg keeps its label + source slot.
        assert_eq!(
            snap("f(a(), y: b())"),
            "callee=value eval=[callee,arg0,arg1] args=[pos#0,named(y)#1] binding=runtime",
        );
    }

    #[test]
    fn member_hof_records_shadow_first() {
        // `xs.map(f: …)` — a member HOF: receiver-first, shadow_first set, named `f`.
        assert_eq!(
            snap("xs.map(f: cb())"),
            "callee=member(map shadow_first class=Hof) eval=[recv,arg0] args=[named(f)#0] binding=runtime",
        );
    }

    #[test]
    fn reordered_named_reduce_is_representable_in_source_order() {
        // The reduce-named follow-up rides on THIS: `f` stays at source index 0,
        // `initial` at 1, so a consumer can evaluate in source order then bind.
        assert_eq!(
            snap("xs.reduce(f: step(), initial: z())"),
            "callee=member(reduce shadow_first class=Hof) eval=[recv,arg0,arg1] \
             args=[named(f)#0,named(initial)#1] binding=runtime",
        );
    }

    #[test]
    fn lazy_callback_class_is_marked() {
        // okOrElse: the callback arg IS an eval step (evaluated eagerly); the lazy
        // INVOCATION is the class, not an eval difference.
        assert_eq!(
            snap("opt.okOrElse(f: mk())"),
            "callee=member(okOrElse shadow_first class=LazyCallback) eval=[recv,arg0] \
             args=[named(f)#0] binding=runtime",
        );
    }

    #[test]
    fn optional_call_guards_before_args() {
        // `opt?.get(side())` — the optional guard precedes (and can skip) the args.
        assert_eq!(
            snap("opt?.get(side())"),
            "callee=member(get optional class=Other) eval=[recv,optguard,arg0] \
             args=[pos#0] binding=runtime",
        );
    }

    #[test]
    fn pipe_inserts_the_lead() {
        // `x |> xs.get()` — the lead has no written slot (InsertedLead), then the stage.
        assert_eq!(
            snap("x |> xs.get()"),
            "callee=pipe(stage=get) eval=[arg0,recv] args=[lead] binding=runtime",
        );
    }

    #[test]
    fn pipe_lead_substitutes_the_placeholder() {
        // Placeholder-bearing stages evaluate the lead once, then preserve every
        // written argument and its source index.
        assert_eq!(
            snap("x |> f(a, _, b)"),
            "callee=pipe(stage=value) eval=[lead,recv,arg0,arg1,arg2] args=[pos#0,pos#1,pos#2] binding=runtime",
        );
    }

    #[test]
    fn pipe_lead_binds_inside_argument_expression() {
        assert_eq!(
            snap("x |> f({_})"),
            "callee=pipe(stage=value) eval=[lead,recv,arg0] args=[pos#0] binding=runtime",
        );
    }

    #[test]
    fn optional_pipe_guards_before_written_arguments() {
        assert_eq!(
            snap("x |> opt?.get(side())"),
            "callee=pipe(stage=get) eval=[arg0,recv,optguard,arg1] args=[lead,pos#0] binding=runtime",
        );
    }

    #[test]
    fn spread_tail_and_post_spread_named() {
        // `f(a, ...xs, c: d)` — spread keeps its source position; named tail follows.
        assert_eq!(
            snap("f(a, ...xs, c: d)"),
            "callee=value eval=[callee,arg0,arg1,arg2] args=[pos#0,spread#1,named(c)#2] binding=runtime",
        );
    }

    #[test]
    fn collect_walks_the_whole_program() {
        // Calls in a function body, a nested arg, an array element, a string
        // interpolation, and a pipe — every position must be reached.
        let src = concat!(
            "function g(x: int) -> int { h(x) }\n",
            "let a = f(outer(1))\n",
            "let arr = [xs.map(cb)]\n",
            "let s = \"{compute(y)}\"\n",
            "let p = z |> w.get()\n",
        );
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        let snaps: Vec<String> = collect_call_plans(&out.program, src)
            .iter()
            .map(snapshot)
            .collect();
        // h(x), f(outer(1)), outer(1), xs.map(cb), compute(y), z|>w.get() — exactly 6.
        assert_eq!(snaps.len(), 6, "{snaps:#?}");
        assert!(
            snaps
                .iter()
                .any(|s| s.contains("member(map shadow_first class=Hof)"))
        );
        assert!(snaps.iter().any(|s| s.contains("pipe(stage=get)")));
        // No phantom zero-arg stage: exactly ONE plan mentions the piped `get`.
        assert_eq!(snaps.iter().filter(|s| s.contains("get")).count(), 1);
    }
}
