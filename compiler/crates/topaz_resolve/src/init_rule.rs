//! The initializer reference rule (SPEC v5.2 §17): an
//! imported module's runtime-binding initializer (or top-level
//! statement) must not, in its OWN immediately-evaluated expression,
//! reach a same-module runtime binding whose initializer has not
//! completed.
//!
//! This is the DIRECT-only rule, consistent with the §4 entry rule
//! (`topaz_check::forward`): the violation fires only when the
//! reference is in an IMMEDIATELY-EVALUATED position of the
//! initializer's own expression — a read, call-by-name, or capture
//! of a `let`/`function` whose textual index is `>= k` (the item's
//! own index). It NEVER descends through a delayed position: a
//! short-circuit RHS, optional-call arguments, a lambda/function/defer
//! body, a conditional branch, a match arm, a loop body, a default
//! parameter, or a `concurrent` arm. A reference
//! to a later binding from such a delayed position is §17-ALLOWED
//! (mutual recursion and higher-order init); if that body is actually
//! invoked too early during module init it faults DYNAMICALLY ("not
//! bound"), exactly as the entry would (§4), not statically.
//!
//! `const`s are exempt: the load-time const pass binds every const
//! before any runtime statement runs, so a later const is always
//! available. The entry module is exempt (role-relative): its
//! top-level surface is governed by the §4 entry rule in
//! `topaz_check`.

use std::collections::{BTreeMap, BTreeSet};

use topaz_diag::{Diagnostic, Label, Span};
use topaz_syntax::ast::*;

use crate::ResolveOutput;
use crate::codes;

pub(crate) fn check(out: &mut ResolveOutput) {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for module in &out.modules {
        if module.is_entry {
            continue;
        }
        let src = out.map.file(module.file).src();
        check_module(&module.program, src, &mut diagnostics);
    }
    out.diagnostics.append(&mut diagnostics);
}

/// What a top-level name denotes, and where it is declared.
#[derive(Clone, Copy)]
enum TopKind {
    Const,
    Function,
    /// A runtime binding (`let` / `let mut`).
    Let,
}

struct TopInfo {
    kind: TopKind,
    /// Textual index among the top-level items.
    index: usize,
}

fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

fn check_module(program: &Program, src: &str, diagnostics: &mut Vec<Diagnostic>) {
    // Index every top-level name (function, const, let) by its textual
    // position — a function binds at runtime only when its declaration
    // item is reached, so a later function is a forward reference too
    // (§4/§17). A destructuring `let` is one index; all its names share it.
    let mut names: BTreeMap<String, TopInfo> = BTreeMap::new();
    for (index, stmt) in program.items.iter().enumerate() {
        collect_item(stmt, index, src, &mut names);
    }

    // For each top-level item, scan its OWN immediately-evaluated
    // expression for a reference to a later runtime binding (`let` /
    // `function`). The item's own names label the diagnostic.
    for (index, stmt) in program.items.iter().enumerate() {
        let inner = unwrap_export(stmt);
        let mut violation: Option<(String, Span)> = None;
        let mut sink = |name: &str, span: Span| {
            if violation.is_some() {
                return;
            }
            if let Some(info) = names.get(name) {
                match info.kind {
                    // A const is always available (the const pass binds
                    // every const before any runtime statement runs).
                    TopKind::Const => {}
                    TopKind::Let | TopKind::Function if info.index >= index => {
                        violation = Some((name.to_string(), span));
                    }
                    _ => {}
                }
            }
        };
        scan_item(inner, src, &mut sink);

        if let Some((name, span)) = violation {
            // The diagnostic is anchored at the offending reference; the
            // owner is the item's binding name(s), or — for a bare
            // statement — the referenced name itself.
            let item_names = item_names_at(inner, src);
            let owner = if item_names.is_empty() {
                text(src, span).to_string()
            } else {
                item_names.join(", ")
            };
            diagnostics.push(Diagnostic::error(
                codes::INIT_FORWARD_REFERENCE,
                format!(
                    "the initializer of `{owner}` reaches `{name}`, whose initializer has not completed (v5.2 defines no partially initialized module binding)",
                ),
                Label::new(span, ""),
            ));
        }
    }
}

fn unwrap_export(stmt: &Stmt) -> &Stmt {
    match &stmt.kind {
        StmtKind::Export(inner) => inner,
        _ => stmt,
    }
}

/// The module-scope names a top-level item introduces (for the
/// diagnostic owner). A bare statement introduces none.
fn item_names_at(inner: &Stmt, src: &str) -> Vec<String> {
    match &inner.kind {
        StmtKind::Let { pattern, .. } => {
            let mut names = Vec::new();
            pattern_names(pattern, src, &mut names);
            names
        }
        StmtKind::Function(decl) => vec![text(src, decl.name.span).to_string()],
        _ => Vec::new(),
    }
}

fn collect_item(stmt: &Stmt, index: usize, src: &str, names: &mut BTreeMap<String, TopInfo>) {
    let inner = unwrap_export(stmt);
    match &inner.kind {
        StmtKind::Function(decl) => {
            names.insert(
                text(src, decl.name.span).to_string(),
                TopInfo {
                    kind: TopKind::Function,
                    index,
                },
            );
        }
        StmtKind::Const { name, .. } => {
            names.insert(
                text(src, name.span).to_string(),
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
                        kind: TopKind::Let,
                        index,
                    },
                );
            }
        }
        _ => {}
    }
}

/// Routes a top-level item to its IMMEDIATELY-EVALUATED expression(s).
/// A `function` only binds a closure (its body is delayed), a `const`
/// is gated by the const pass, a `defer`/`while`-body is delayed — so
/// each contributes only its immediate expression (or nothing).
fn scan_item(inner: &Stmt, src: &str, on_ref: &mut impl FnMut(&str, Span)) {
    let mut scope = Scope::default();
    match &inner.kind {
        StmtKind::Let { value, .. } => scan_expr(value, src, &mut scope, on_ref),
        StmtKind::Expr(e) => scan_expr(e, src, &mut scope, on_ref),
        StmtKind::Return(Some(e)) => scan_expr(e, src, &mut scope, on_ref),
        StmtKind::Assign { target, value, .. } => {
            scan_expr(target, src, &mut scope, on_ref);
            scan_expr(value, src, &mut scope, on_ref);
        }
        // The condition is immediate; the loop body is delayed.
        StmtKind::While { cond, .. } => scan_expr(cond, src, &mut scope, on_ref),
        // A `function` body, a `const` initializer (const pass), a
        // `defer` body, and the remaining statement forms contribute no
        // immediately-evaluated runtime reference here.
        _ => {}
    }
}

/// Lexical scope stack: locals (`let`/`const` in a top-level block
/// expression, match/for bindings on the IMMEDIATE scrutinee/iterable
/// side) shadow top-level names.
#[derive(Default)]
struct Scope {
    frames: Vec<BTreeSet<String>>,
}

impl Scope {
    fn contains(&self, name: &str) -> bool {
        self.frames.iter().any(|f| f.contains(name))
    }

    fn current_frame(&mut self) -> &mut BTreeSet<String> {
        let index = self.frames.len() - 1;
        &mut self.frames[index]
    }
}

fn pattern_names(pattern: &Pattern, src: &str, out: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Binding(name) | PatternKind::Typed { name, .. } => {
            out.push(text(src, name.span).to_string());
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
                    None => out.push(text(src, field.name.span).to_string()),
                }
            }
        }
        _ => {}
    }
}

/// Scans an expression's IMMEDIATELY-EVALUATED positions for resolved
/// top-level name references (read, call-by-name, or capture). Delayed
/// positions — short-circuit RHS and optional-call arguments,
/// lambda/defer bodies, branch/arm/loop bodies, default parameters,
/// `concurrent` arms — are NOT entered: a reference there
/// is §17-allowed (mirrors `topaz_check::forward`).
fn scan_expr(expr: &Expr, src: &str, scope: &mut Scope, on_ref: &mut impl FnMut(&str, Span)) {
    match &expr.kind {
        ExprKind::Ident => {
            let name = text(src, expr.span);
            if !scope.contains(name) {
                on_ref(name, expr.span);
            }
        }
        ExprKind::Call { callee, args, .. } => {
            let arguments_are_immediate = !matches!(&callee.kind, ExprKind::OptionalAccess { .. });
            match &callee.kind {
                ExprKind::Ident => {
                    let name = text(src, callee.span);
                    if !scope.contains(name) {
                        on_ref(name, callee.span);
                    }
                }
                // An immediately-invoked lambda's BODY is delayed (§17):
                // `(() => f())()` is allowed even when `f` is later.
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
        // its body, so a later binding it names is §17-allowed.
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
        // evaluated immediately; later clauses, the filters, and the body run inside
        // the (delayed) loop.
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
    scope.frames.push(BTreeSet::new());
    for stmt in &block.stmts {
        match &stmt.kind {
            // A nested function only binds a closure; its body is
            // delayed. We still record the name so a later reference in
            // this block resolves to the local, not a top-level name.
            StmtKind::Function(decl) => {
                scope
                    .current_frame()
                    .insert(text(src, decl.name.span).to_string());
            }
            StmtKind::Let { pattern, value, .. } => {
                scan_expr(value, src, scope, on_ref);
                let mut bound = Vec::new();
                pattern_names(pattern, src, &mut bound);
                scope.current_frame().extend(bound);
            }
            StmtKind::Const { name, value, .. } => {
                scan_expr(value, src, scope, on_ref);
                scope
                    .current_frame()
                    .insert(text(src, name.span).to_string());
            }
            StmtKind::Using { name, value, body } => {
                scan_expr(value, src, scope, on_ref);
                let mut using_scope = BTreeSet::new();
                using_scope.insert(text(src, name.span).to_string());
                scope.frames.push(using_scope);
                scan_block(body, src, scope, on_ref);
                scope.frames.pop();
            }
            StmtKind::Assign { target, value, .. } => {
                if let ExprKind::Ident = &target.kind {
                    let name = text(src, target.span);
                    if !scope.contains(name) {
                        on_ref(name, target.span);
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
