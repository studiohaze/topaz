//! Top-level init-order analysis (the §4 forward-reference rule).
//!
//! The interpreter binds a module's top-level items in this order
//! (`topaz_interp::machine`): a load-time **const pass** evaluates and
//! binds every `const` in textual order (each const sees only EARLIER
//! consts), and only then do the remaining statements run in textual
//! order — a `function` declaration binds its closure when its
//! statement is reached, a `let` binds when its statement is reached.
//! A `function`/lambda BODY resolves its names at CALL time, against
//! whatever is bound then.
//!
//! The type checker hoists every signature (so bodies type against the
//! whole top-level namespace), which is the right TYPE VISIBILITY model
//! but says nothing about RUNTIME AVAILABILITY. §4 narrows the static
//! verdict to exactly one shape:
//!
//! > Only a top-level NON-function STATEMENT's OWN, immediately-
//! > evaluated forward reference is a static error (TPZ5002). A
//! > function/lambda/defer BODY that NAMES a later binding is
//! > §4-ALLOWED (mutual recursion); calling such a body before the
//! > name is bound is a DYNAMIC runtime fault that `check` is not
//! > obligated to catch (like `1/0`).
//!
//! So the rule here is purely SYNTACTIC over the statement's own
//! IMMEDIATELY-EVALUATED expression: a read, call-by-name, or capture
//! of a top-level `let`/`function` whose textual index is `>= i` (the
//! statement's own index) is the error. It NEVER follows a call into
//! another body, never tracks `let`-aliases, never descends into a
//! delayed position (short-circuit RHS, optional-call arguments,
//! lambda/function/defer body, conditional branch, match arm, loop
//! body, default parameter, `concurrent` arm).
//! `const`s are exempt — the load-time const pass binds them all
//! before any statement runs.

use std::collections::{BTreeMap, BTreeSet};

use topaz_diag::Span;
use topaz_syntax::ast::*;

use crate::codes;
use crate::form::Former;

/// What a top-level name denotes, and where it is declared.
#[derive(Clone, Copy)]
enum TopKind {
    Const,
    /// A runtime binding (`function`, `let`, or `let mut`).
    Runtime,
}

struct TopInfo {
    kind: TopKind,
    /// Textual index among the top-level items.
    index: usize,
}

/// The top-level namespace of one program/module: every name a
/// statement could resolve to, with its kind and textual position.
pub(crate) struct TopLevel {
    names: BTreeMap<String, TopInfo>,
}

impl TopLevel {
    /// Whether `name` is a top-level `let`/`function` — the references
    /// this pass owns. The type checker stays silent on such a name
    /// being unbound at a use site (a forward reference reported here),
    /// rather than double-reporting it as a plain "not bound". A
    /// forward `const` reference is deliberately excluded: a const can
    /// only see EARLIER consts, so an unbound const name is a genuine
    /// const-pass error the type check must keep reporting.
    pub(crate) fn is_forward_runtime_name(&self, name: &str) -> bool {
        matches!(
            self.names.get(name).map(|i| &i.kind),
            Some(TopKind::Runtime)
        )
    }

    fn is_unavailable_runtime_name(&self, name: &str, cutoff: usize) -> bool {
        matches!(
            self.names.get(name),
            Some(TopInfo {
                kind: TopKind::Runtime,
                index,
            }) if *index >= cutoff
        )
    }

    pub(crate) fn build(items: &[Stmt], src: &str) -> TopLevel {
        let mut names: BTreeMap<String, TopInfo> = BTreeMap::new();
        for (index, stmt) in items.iter().enumerate() {
            collect_item(stmt, index, src, &mut names);
        }
        TopLevel { names }
    }

    /// Reports a forward reference for the runtime-binding initializer
    /// or top-level statement expression at textual index `cutoff`.
    pub(crate) fn check_item(&self, cutoff: usize, init: &Expr, former: &mut Former<'_>) {
        self.run(cutoff, init, former);
    }

    /// Reports a forward reference in an immediately executed block at textual
    /// index `cutoff`.
    pub(crate) fn check_block(&self, cutoff: usize, block: &Block, former: &mut Former<'_>) {
        self.run_block(cutoff, block, former);
    }

    fn run(&self, cutoff: usize, init: &Expr, former: &mut Former<'_>) {
        let src = former.source();
        let mut violation: Option<Span> = None;
        let mut sink = |name: &str, span: Span| {
            if violation.is_some() {
                return;
            }
            // Apply the runtime-availability policy. A const reference
            // is always satisfied (the const pass binds every const
            // before any statement runs); a `let`/`function` at index
            // `>= cutoff` is a forward reference.
            if self.is_unavailable_runtime_name(name, cutoff) {
                violation = Some(span);
            }
        };
        scan_expr(init, src, &mut Scope::default(), &mut sink);
        Self::report(violation, former);
    }

    fn run_block(&self, cutoff: usize, block: &Block, former: &mut Former<'_>) {
        let src = former.source();
        let mut violation: Option<Span> = None;
        let mut sink = |name: &str, span: Span| {
            if violation.is_some() {
                return;
            }
            if self.is_unavailable_runtime_name(name, cutoff) {
                violation = Some(span);
            }
        };
        scan_block(block, src, &mut Scope::default(), &mut sink);
        Self::report(violation, former);
    }

    fn report(violation: Option<Span>, former: &mut Former<'_>) {
        if let Some(span) = violation {
            let name = former.text(span).to_string();
            former.error(codes::UNBOUND, format!("`{name}` is not bound"), span);
        }
    }
}

fn unwrap_export(stmt: &Stmt) -> &Stmt {
    match &stmt.kind {
        StmtKind::Export(inner) => inner,
        _ => stmt,
    }
}

fn collect_item(stmt: &Stmt, index: usize, src: &str, names: &mut BTreeMap<String, TopInfo>) {
    let text = |span: Span| src[span.lo as usize..span.hi as usize].to_string();
    let inner = unwrap_export(stmt);
    match &inner.kind {
        StmtKind::Function(decl) => {
            names.insert(
                text(decl.name.span),
                TopInfo {
                    kind: TopKind::Runtime,
                    index,
                },
            );
        }
        StmtKind::Const { name, .. } => {
            names.insert(
                text(name.span),
                TopInfo {
                    kind: TopKind::Const,
                    index,
                },
            );
        }
        StmtKind::Let { pattern, .. } => {
            let mut bound = Vec::new();
            pattern_names(pattern, src, &mut bound);
            for n in bound {
                names.insert(
                    n,
                    TopInfo {
                        kind: TopKind::Runtime,
                        index,
                    },
                );
            }
        }
        _ => {}
    }
}

/// Lexical scope stack: locals (`let`/`const` introduced inside a
/// top-level block expression, match/for bindings on the IMMEDIATE
/// scrutinee/iterable side) shadow top-level names.
#[derive(Default)]
struct Scope {
    frames: Vec<BTreeSet<String>>,
}

impl Scope {
    fn contains(&self, name: &str) -> bool {
        self.frames.iter().any(|f| f.contains(name))
    }
}

fn pattern_names(pattern: &Pattern, src: &str, out: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
            out.push(src[name.span.lo as usize..name.span.hi as usize].to_string());
        }
        PatternKind::Or(alts) => {
            for alt in alts {
                pattern_names(alt, src, out);
            }
        }
        PatternKind::Constructor { args, .. } => {
            for arg in args {
                pattern_names(arg, src, out);
            }
        }
        PatternKind::List(elems) => {
            for elem in elems {
                match elem {
                    ListPatternElem::Pattern(p) | ListPatternElem::Rest(Some(p)) => {
                        pattern_names(p, src, out)
                    }
                    ListPatternElem::Rest(None) => {}
                }
            }
        }
        PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
            for field in fields {
                match &field.pattern {
                    Some(p) => pattern_names(p, src, out),
                    None => out.push(
                        src[field.name.span.lo as usize..field.name.span.hi as usize].to_string(),
                    ),
                }
            }
        }
        _ => {}
    }
}

/// Scans an expression's IMMEDIATELY-EVALUATED positions for resolved
/// top-level name references (read, call-by-name, or capture). The
/// callback is `(name, span)`. Delayed positions — short-circuit RHS
/// and optional-call arguments, lambda/defer bodies, branch/arm/loop
/// bodies, default parameters, `concurrent` arms — are NOT entered: a
/// reference there is §4-allowed.
fn scan_expr(expr: &Expr, src: &str, scope: &mut Scope, on_ref: &mut impl FnMut(&str, Span)) {
    let text = |span: Span| src[span.lo as usize..span.hi as usize].to_string();
    match &expr.kind {
        ExprKind::Ident => {
            let name = text(expr.span);
            if !scope.contains(&name) {
                on_ref(&name, expr.span);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            let arguments_are_immediate = !matches!(&callee.kind, ExprKind::OptionalAccess { .. });
            match &callee.kind {
                ExprKind::Ident => {
                    let name = text(callee.span);
                    if !scope.contains(&name) {
                        on_ref(&name, callee.span);
                    }
                }
                // An immediately-invoked lambda's BODY is delayed (§4):
                // `(() => f())()` is allowed even when `f` is later. We
                // descend the callee only insofar as it is itself an
                // immediate expression; a `Lambda` callee contributes
                // nothing (its body is not scanned).
                ExprKind::Lambda { .. } => {}
                _ => scan_expr(callee, src, scope, on_ref),
            }
            // Ordinary call arguments are immediate. Optional-call arguments
            // are a delayed branch: an empty receiver skips every argument.
            if arguments_are_immediate {
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Spread(e) => {
                            scan_expr(e, src, scope, on_ref)
                        }
                        CallArg::Named { value, .. } => scan_expr(value, src, scope, on_ref),
                    }
                }
            }
        }
        // A lambda body is delayed: creating a closure does not evaluate
        // its body, so a later binding it names is §4-allowed.
        ExprKind::Lambda { .. } => {}
        ExprKind::Block(block) => scan_block(block, src, scope, on_ref),
        // The `if` CONDITION is immediate; the branch bodies are not.
        ExprKind::If { cond, .. } => scan_expr(cond, src, scope, on_ref),
        // The `match` SCRUTINEE is immediate; the arm bodies/guards are
        // not.
        ExprKind::Match { scrutinee, .. } => scan_expr(scrutinee, src, scope, on_ref),
        // The `for` ITERABLE is immediate; the loop body is not.
        ExprKind::For { iter, .. } => scan_expr(iter, src, scope, on_ref),
        // §6.4 comprehension: like a `for`, only the LEADING clause's iterable is
        // evaluated immediately; later clauses, filters, and the body are delayed.
        ExprKind::Comprehension { clauses, .. } => {
            if let Some(CompClause::For { iter, .. }) = clauses.first() {
                scan_expr(iter, src, scope, on_ref);
            }
        }
        // `concurrent` arms are delayed (spawned tasks); only the
        // timeout is evaluated immediately.
        ExprKind::Concurrent {
            timeout: Some(t), ..
        } => scan_expr(t, src, scope, on_ref),
        ExprKind::Paren(inner) | ExprKind::Try(inner) => scan_expr(inner, src, scope, on_ref),
        ExprKind::Member { object, .. } | ExprKind::OptionalAccess { object, .. } => {
            scan_expr(object, src, scope, on_ref)
        }
        ExprKind::Index { object, index } => {
            scan_expr(object, src, scope, on_ref);
            scan_expr(index, src, scope, on_ref);
        }
        ExprKind::Unary { operand, .. } => scan_expr(operand, src, scope, on_ref),
        ExprKind::Binary { op, lhs, rhs } => {
            scan_expr(lhs, src, scope, on_ref);
            // The RHS of `&&`, `||`, and `??` is a delayed branch.
            if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) {
                scan_expr(rhs, src, scope, on_ref);
            }
        }
        ExprKind::Compose { lhs, rhs } => {
            scan_expr(lhs, src, scope, on_ref);
            scan_expr(rhs, src, scope, on_ref);
        }
        ExprKind::Range { lo, hi, step, .. } => {
            scan_expr(lo, src, scope, on_ref);
            scan_expr(hi, src, scope, on_ref);
            if let Some(s) = step {
                scan_expr(s, src, scope, on_ref);
            }
        }
        ExprKind::Pipe { lhs, rhs } => {
            scan_expr(lhs, src, scope, on_ref);
            // The immediate rhs stage is evaluated now; `.field` pipe
            // sugar names a field, not a top-level binding.
            if let PipeRhs::Expr(e) = rhs.as_ref() {
                scan_expr(e, src, scope, on_ref);
            }
        }
        ExprKind::RecordLiteral { fields } => {
            for f in fields {
                scan_expr(&f.value, src, scope, on_ref);
            }
        }
        ExprKind::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            scan_expr(base, src, scope, on_ref);
            if let Some(spread) = spread {
                scan_expr(spread, src, scope, on_ref);
            }
            for f in fields {
                scan_expr(&f.value, src, scope, on_ref);
            }
        }
        ExprKind::Array(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        scan_expr(e, src, scope, on_ref)
                    }
                }
            }
        }
        ExprKind::SetLiteral(elements) => {
            for e in elements {
                scan_expr(e, src, scope, on_ref);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (k, v) in entries {
                scan_expr(k, src, scope, on_ref);
                scan_expr(v, src, scope, on_ref);
            }
        }
        ExprKind::String(lit) => {
            for part in &lit.parts {
                if let StringPart::Interpolation(e) = part {
                    scan_expr(e, src, scope, on_ref);
                }
            }
        }
        _ => {}
    }
}

/// Scans a top-level block expression's OWN statements and tail — all
/// immediately evaluated when the block is reached. A nested
/// `function`/`defer` body inside it is delayed (not entered); a `while`
/// condition is immediate but its loop body is delayed.
fn scan_block(block: &Block, src: &str, scope: &mut Scope, on_ref: &mut impl FnMut(&str, Span)) {
    let text = |span: Span| src[span.lo as usize..span.hi as usize].to_string();
    scope.frames.push(BTreeSet::new());
    for stmt in &block.stmts {
        match &stmt.kind {
            // A nested function only binds a closure; its body is
            // delayed. We still record the name so a later reference in
            // this block resolves to the local, not a top-level name.
            StmtKind::Function(decl) => {
                if let Some(frame) = scope.frames.last_mut() {
                    frame.insert(text(decl.name.span));
                }
            }
            StmtKind::Let { pattern, value, .. } => {
                scan_expr(value, src, scope, on_ref);
                let mut bound = Vec::new();
                pattern_names(pattern, src, &mut bound);
                if let Some(frame) = scope.frames.last_mut() {
                    frame.extend(bound);
                }
            }
            StmtKind::Const { name, value, .. } => {
                scan_expr(value, src, scope, on_ref);
                if let Some(frame) = scope.frames.last_mut() {
                    frame.insert(text(name.span));
                }
            }
            StmtKind::Using { name, value, body } => {
                scan_expr(value, src, scope, on_ref);
                let mut using_scope = BTreeSet::new();
                using_scope.insert(text(name.span));
                scope.frames.push(using_scope);
                scan_block(body, src, scope, on_ref);
                scope.frames.pop();
            }
            StmtKind::Assign { target, value, .. } => {
                if let ExprKind::Ident = &target.kind {
                    let name = text(target.span);
                    if !scope.contains(&name) {
                        on_ref(&name, target.span);
                    }
                } else {
                    scan_expr(target, src, scope, on_ref);
                }
                scan_expr(value, src, scope, on_ref);
            }
            StmtKind::Return(Some(value)) => scan_expr(value, src, scope, on_ref),
            // A `defer` body is delayed (runs at scope exit); a bare
            // expression statement is immediate.
            StmtKind::Expr(body) => scan_expr(body, src, scope, on_ref),
            StmtKind::While { cond, .. } => {
                // The condition is immediate; the loop body is delayed.
                scan_expr(cond, src, scope, on_ref);
            }
            _ => {}
        }
    }
    if let Some(tail) = &block.tail {
        scan_expr(tail, src, scope, on_ref);
    }
    scope.frames.pop();
}
